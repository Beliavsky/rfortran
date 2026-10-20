// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Convert Fortran fixed source form to free source form.
//!
//! Line `n` of the output is always line `n` of the input, and
//! [`Converted::map`] gives the input byte offset of every output byte, so a
//! compiler can convert a fixed-form file internally and still report
//! diagnostics at the original line and column.
//!
//! The conversion is lexical: comment lines become `!` comments, labels and
//! statement text keep their columns, a continuation line starts with `&` and
//! the line it continues ends with `&`, text after the last column (72 by
//! default) is dropped, and a character context continued onto the next line is
//! padded with blanks to the last column, as fixed form specifies. Hollerith
//! constants in FORMAT statements become character literals.

/// Which Hollerith constants become character literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hollerith {
    /// Only in FORMAT statements, where a character literal means the same.
    Format,
    /// Everywhere, which changes the meaning of a Hollerith constant that
    /// initializes or is passed as a numeric value.
    All,
    /// Nowhere.
    None,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// The last column of the statement field, or `None` to read whole lines.
    pub line_length: Option<usize>,
    /// Compile lines with `D` in column 1 instead of treating them as comments.
    pub debug_lines_as_code: bool,
    pub hollerith: Hollerith,
}

impl Default for Options {
    fn default() -> Self {
        Options { line_length: Some(72), debug_lines_as_code: false, hollerith: Hollerith::Format }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningKind {
    /// Text after the last column was dropped; reported once, for line 0.
    Truncation,
    /// An output line is longer than free form allows by default. Only a
    /// compiler with that limit needs it.
    FreeLineLength,
    /// Anything else, which may change or break the meaning of the code.
    Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// The 1-based input line.
    pub line: usize,
    pub kind: WarningKind,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Converted {
    /// Free-form source, with the line structure of the input.
    pub text: Vec<u8>,
    /// The input byte offset of each output byte, and of the end of the output.
    pub map: Vec<usize>,
    pub warnings: Vec<Warning>,
}

impl Converted {
    /// The input byte offset that output byte `offset` came from.
    pub fn source_offset(&self, offset: usize) -> usize {
        self.map[offset.min(self.map.len() - 1)]
    }
}

/// The longest line that free source form allows by default.
const FREE_LINE_LENGTH: usize = 132;

/// One output line and the input offset of each of its bytes.
#[derive(Default)]
struct Line {
    text: Vec<u8>,
    map: Vec<usize>,
}

impl Line {
    fn copy(&mut self, byte: u8, at: usize) {
        self.text.push(byte);
        self.map.push(at);
    }
    fn synth(&mut self, bytes: &[u8], at: usize) {
        self.text.extend_from_slice(bytes);
        self.map.extend(std::iter::repeat(at).take(bytes.len()));
    }
    fn truncate(&mut self, length: usize) {
        self.text.truncate(length);
        self.map.truncate(length);
    }
    fn insert(&mut self, position: usize, byte: u8) {
        let at = if position > 0 { self.map[position - 1] } else { self.map.first().copied().unwrap_or(0) };
        self.text.insert(position, byte);
        self.map.insert(position, at);
    }
    /// The length without trailing blanks, but not shorter than `floor`.
    fn trimmed(&self, floor: usize) -> usize {
        let mut end = self.text.len();
        while end > floor && matches!(self.text[end - 1], b' ' | b'\t') {
            end -= 1;
        }
        end
    }
}

/// A run of digits that may be the count of a Hollerith constant.
#[derive(Clone, Copy)]
struct Digits {
    line: usize,
    start: usize,
    end: usize,
    count: usize,
    allowed: bool,
}

/// The lexical state carried from one line of a statement to the next.
#[derive(Default)]
struct Lexical {
    quote: Option<u8>,
    /// Characters left in a Hollerith constant, and whether it is quoted.
    hollerith: usize,
    quoted: bool,
    digits: Option<Digits>,
    /// The last two nonblank bytes outside character contexts.
    last: u8,
    before_last: u8,
    format: bool,
    data: bool,
}

impl Lexical {
    fn in_character_context(&self) -> bool {
        self.quote.is_some() || self.hollerith > 0
    }
    fn significant(&mut self, byte: u8) {
        if !matches!(byte, b' ' | b'\t') {
            self.before_last = self.last;
            self.last = byte;
        }
    }
}

/// The code line that the next continuation line continues.
struct Continued {
    line: usize,
    code_end: usize,
    /// The code reaches the last column, so a name may be split there.
    full: bool,
    last_byte: u8,
}

enum Kind {
    Blank,
    Comment,
    Debug,
    Preprocessor,
    /// A statement line: the end of its label, whether it continues the
    /// previous statement, and where its statement field starts.
    Statement { label_end: usize, continuation: bool, field: usize, tab: bool },
    /// A statement that starts before column 7.
    Misplaced,
}

fn classify(line: &[u8], line_length: Option<usize>) -> Kind {
    let visible = &line[..line.len().min(line_length.unwrap_or(usize::MAX))];
    if visible.iter().all(|b| matches!(b, b' ' | b'\t' | b'\x0c')) {
        return Kind::Blank;
    }
    match line[0] {
        b'C' | b'c' | b'*' | b'!' => return Kind::Comment,
        b'D' | b'd' => return Kind::Debug,
        b'#' => return Kind::Preprocessor,
        _ => {}
    }
    // A tab in the label field starts the statement field; a nonzero digit
    // after the tab marks a continuation line.
    if let Some(tab) = line.iter().take(6).position(|b| *b == b'\t') {
        if line[..tab].iter().all(|b| b.is_ascii_digit() || *b == b' ') {
            let continuation = line.get(tab + 1).is_some_and(|b| (b'1'..=b'9').contains(b));
            let field = if continuation { tab + 2 } else { tab + 1 };
            return Kind::Statement { label_end: tab, continuation, field, tab: true };
        }
    }
    let first = line.iter().position(|b| *b != b' ').unwrap();
    if line[first] == b'!' && first != 5 {
        return Kind::Comment;
    }
    if line[..line.len().min(5)].iter().any(|b| !(b.is_ascii_digit() || *b == b' ')) {
        return Kind::Misplaced;
    }
    let column6 = line.get(5).copied().unwrap_or(b' ');
    Kind::Statement {
        label_end: line.len().min(5),
        continuation: !matches!(column6, b' ' | b'0'),
        field: line.len().min(6),
        tab: false,
    }
}

/// Whether a statement field starts with `keyword`, ignoring blanks and case.
fn starts_with(field: &[u8], keyword: &[u8]) -> bool {
    let mut letters = field.iter().filter(|b| !matches!(b, b' ' | b'\t')).map(|b| b.to_ascii_lowercase());
    keyword.iter().all(|expected| letters.next() == Some(*expected))
}

/// The length of an operator or logical constant such as `.AND .` at the
/// start of `text` that contains blanks, which free form does not allow.
fn dotted_operator(text: &[u8]) -> Option<usize> {
    let close = text[1..].iter().position(|b| *b == b'.')? + 1;
    let inner = &text[1..close];
    if !inner.iter().any(|b| matches!(b, b' ' | b'\t')) {
        return None;
    }
    let word: Vec<u8> = inner.iter().filter(|b| !matches!(b, b' ' | b'\t')).map(|b| b.to_ascii_lowercase()).collect();
    const OPERATORS: [&[u8]; 13] =
        [b"eq", b"ne", b"lt", b"le", b"gt", b"ge", b"and", b"or", b"not", b"eqv", b"neqv", b"true", b"false"];
    OPERATORS.contains(&word.as_slice()).then_some(close + 1)
}

/// Whether `code` ends with a name directly before a character literal, other
/// than the one-letter prefix of a BOZ constant such as Z'FF'.
fn keyword_before_literal(code: &[u8]) -> bool {
    let Some(&last) = code.last() else { return false };
    if !last.is_ascii_alphabetic() {
        return false;
    }
    let single = code.len() < 2 || !(code[code.len() - 2].is_ascii_alphanumeric() || code[code.len() - 2] == b'_');
    !(single && matches!(last.to_ascii_lowercase(), b'b' | b'o' | b'z' | b'x'))
}

/// Whether `code` ends with a real constant that has a decimal point, such as
/// 1.57 or .75, and not a name or label such as X1 or 100.
fn real_before(code: &[u8]) -> bool {
    let digits = code.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    let rest = &code[..code.len() - digits];
    if rest.last() != Some(&b'.') {
        return false;
    }
    let before = &rest[..rest.len() - 1];
    let whole = before.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 && whole == 0 {
        return false;
    }
    // The constant starts after an operator or delimiter, not inside a name.
    !before[..before.len() - whole].last().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
}

/// Whether `text`, after blanks, is an exponent such as D0, E+2 or d -3.
fn exponent_after(text: &[u8]) -> bool {
    let mut rest = text.iter().copied().filter(|b| !matches!(b, b' ' | b'\t'));
    if !matches!(rest.next().map(|b| b.to_ascii_lowercase()), Some(b'd' | b'e' | b'q')) {
        return false;
    }
    match rest.next() {
        Some(b'+' | b'-') => rest.next().is_some_and(|b| b.is_ascii_digit()),
        Some(b) => b.is_ascii_digit(),
        None => false,
    }
}

struct Converter<'a> {
    options: &'a Options,
    lines: Vec<Line>,
    warnings: Vec<Warning>,
    state: Lexical,
    continued: Option<Continued>,
    /// The number of lines with code after the last column.
    truncated: usize,
}

impl Converter<'_> {
    fn warn(&mut self, line: usize, message: impl Into<String>) {
        self.warnings.push(Warning { line, kind: WarningKind::Source, message: message.into() });
    }

    /// Scan the statement field `line[from..]`, whose first byte is in column
    /// `column`, into `out`. Returns where the code ends in `out`.
    fn field(&mut self, number: usize, line: &[u8], base: usize, from: usize, column: usize, out: &mut Line) -> usize {
        let limit = match self.options.line_length {
            Some(length) => from + length.saturating_sub(column - 1),
            None => usize::MAX,
        };
        let end = line.len().min(limit);
        if line.len() > limit && line[limit..].iter().any(|b| !matches!(b, b' ' | b'\t')) {
            self.truncated += 1;
        }
        let floor = out.text.len();
        let mut i = from.min(end);
        while i < end {
            let byte = line[i];
            let at = base + i;
            if let Some(quote) = self.state.quote {
                out.copy(byte, at);
                if byte == quote {
                    if i + 1 < end && line[i + 1] == quote {
                        out.copy(quote, at + 1);
                        i += 2;
                        continue;
                    }
                    self.state.quote = None;
                    self.state.significant(quote);
                }
                i += 1;
                continue;
            }
            if self.state.hollerith > 0 {
                self.hollerith_byte(byte, at, out);
                i += 1;
                continue;
            }
            match byte {
                b'\'' | b'"' => {
                    // A keyword runs into a literal, as in STOP'message';
                    // free form needs a blank there.
                    if out.text.len() > floor && keyword_before_literal(&out.text[floor..]) {
                        out.synth(b" ", at);
                    }
                    self.state.quote = Some(byte);
                    self.state.digits = None;
                    out.copy(byte, at);
                }
                b'!' => {
                    // An inline comment keeps its text, beyond the last column too.
                    let code_end = out.trimmed(floor);
                    for (j, b) in line[i..].iter().enumerate() {
                        out.copy(*b, at + j);
                    }
                    return code_end;
                }
                b'0'..=b'9' => {
                    let digit = usize::from(byte - b'0');
                    match &mut self.state.digits {
                        Some(run) if run.line == number => {
                            run.count = run.count.saturating_mul(10).saturating_add(digit);
                            run.end = out.text.len() + 1;
                        }
                        _ => {
                            // A count follows a delimiter, a repeat factor in
                            // DATA, or a relational operator such as .EQ.; never
                            // a name, a number or a kind such as REAL*8.
                            let allowed = matches!(self.state.last, b'(' | b',' | b'/' | b'=')
                                || self.state.last == b'*' && self.state.data
                                || self.state.last == b'.' && self.state.before_last.is_ascii_alphabetic();
                            let start = out.text.len();
                            self.state.digits = Some(Digits { line: number, start, end: start + 1, count: digit, allowed });
                        }
                    }
                    out.copy(byte, at);
                    self.state.significant(byte);
                }
                b'H' | b'h' if self.state.digits.is_some_and(|run| {
                    run.allowed && run.line == number && run.end == out.text.len() && (1..=1000).contains(&run.count)
                }) => {
                    let run = self.state.digits.take().unwrap();
                    let quoted = match self.options.hollerith {
                        Hollerith::Format => self.state.format,
                        Hollerith::All => true,
                        Hollerith::None => false,
                    };
                    self.state.hollerith = run.count;
                    self.state.quoted = quoted;
                    if quoted {
                        out.truncate(run.start);
                        out.synth(b"'", at);
                    } else {
                        out.copy(byte, at);
                        self.warn(number, "Hollerith constant kept; free source form does not allow it in standard Fortran");
                    }
                }
                // Blanks inside an operator such as .AND . or . EQ. are
                // ignored in fixed form, but not in free form.
                b'.' if dotted_operator(&line[i..end]).is_some() => {
                    let length = dotted_operator(&line[i..end]).unwrap();
                    for (j, b) in line[i..i + length].iter().enumerate() {
                        if !matches!(b, b' ' | b'\t') {
                            out.copy(*b, at + j);
                        }
                    }
                    self.state.digits = None;
                    self.state.significant(b'.');
                    i += length;
                    continue;
                }
                // Fixed form ignores blanks, so digits separated by blanks, as
                // in .70710 67811, are one number; free form needs them joined.
                b' ' | b'\t' if self.state.last.is_ascii_digit()
                    && out.text.last().is_some_and(|b| b.is_ascii_digit())
                    && line[i..end].iter().find(|b| !matches!(b, b' ' | b'\t')).is_some_and(|b| b.is_ascii_digit()) => {}
                // So are a real constant and its exponent, as in 1.57 D0.
                b' ' | b'\t' if out.text.last().is_some_and(|b| b.is_ascii_digit() || *b == b'.')
                    && real_before(&out.text[floor..])
                    && exponent_after(&line[i..end]) => {}
                b' ' | b'\t' => out.copy(byte, at),
                _ => {
                    self.state.digits = None;
                    out.copy(byte, at);
                    self.state.significant(byte);
                }
            }
            i += 1;
        }
        // A character context continued onto the next line includes the
        // blanks up to the last column.
        if self.state.in_character_context() && limit != usize::MAX {
            let at = base + line.len();
            for _ in end.max(from)..limit {
                if self.state.quote.is_some() {
                    out.synth(b" ", at);
                } else if self.state.hollerith > 0 {
                    self.hollerith_byte(b' ', at, out);
                } else {
                    break;
                }
            }
        }
        if self.state.in_character_context() {
            out.text.len()
        } else {
            let code_end = out.trimmed(floor);
            out.truncate(code_end);
            code_end
        }
    }

    fn hollerith_byte(&mut self, byte: u8, at: usize, out: &mut Line) {
        if self.state.quoted && byte == b'\'' {
            out.synth(b"''", at);
        } else {
            out.copy(byte, at);
        }
        self.state.hollerith -= 1;
        if self.state.hollerith == 0 {
            if self.state.quoted {
                out.synth(b"'", at);
            }
            self.state.significant(b'\'');
        }
    }

    /// Start a statement, closing the previous one.
    fn start_statement(&mut self, number: usize) {
        if self.state.quote.is_some() {
            self.warn(number - 1, "character literal not closed at the end of its statement");
        } else if self.state.hollerith > 0 {
            self.warn(number - 1, "Hollerith constant longer than its statement");
        }
        self.state = Lexical::default();
        self.continued = None;
    }

    fn statement(&mut self, number: usize, line: &[u8], base: usize, label_end: usize, continuation: bool, field: usize, tab: bool) {
        let mut out = Line::default();
        // A misplaced statement has no label field and starts in column 1.
        let column = if field == 0 { 1 } else { 7 };
        let continuation = continuation && {
            if self.continued.is_none() {
                self.warn(number, "continuation line without a statement to continue");
            }
            self.continued.is_some()
        };
        if continuation {
            if line[..label_end].iter().any(|b| *b != b' ') {
                self.warn(number, "label on a continuation line is ignored");
            }
            let previous = self.continued.take().unwrap();
            let target = &mut self.lines[previous.line];
            target.insert(previous.code_end, b'&');
            out.synth(b"     &", base + label_end.min(5));
            if !self.state.in_character_context() {
                // Fixed form ignores blanks, so names on either side of a
                // continuation join. Free form keeps them apart unless the
                // continued line fills its last column.
                let next = line.get(field).copied().unwrap_or(b' ');
                let first = line[field.min(line.len())..].iter().find(|b| !matches!(b, b' ' | b'\t')).copied().unwrap_or(b' ');
                let name = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
                let continued = &self.lines[previous.line].text[..previous.code_end];
                if name(previous.last_byte) && name(next) && !previous.full
                    || matches!(next, b'\'' | b'"') && keyword_before_literal(continued)
                {
                    out.synth(b" ", base + field);
                } else if name(previous.last_byte) && !name(next) && name(first) && previous.full {
                    self.warn(number, "a name or number may be split across this continuation; check its blanks");
                }
            }
        } else {
            self.start_statement(number);
            let text = &line[field.min(line.len())..];
            self.state.format = starts_with(text, b"format(");
            self.state.data = starts_with(text, b"data");
            if field > 0 {
                for (i, b) in line[..label_end].iter().enumerate() {
                    out.copy(*b, base + i);
                }
                let pad = if tab { 5usize.saturating_sub(label_end) } else { 5 - label_end };
                out.synth(&b"     "[..pad], base + label_end);
                out.synth(b" ", base + label_end);
            }
        }
        let code_start = out.text.len();
        let code_end = self.field(number, line, base, field, column, &mut out);
        let last_byte = if code_end > code_start { out.text[code_end - 1] } else { b' ' };
        // The code fills the statement field when its last byte is in the last column.
        let full = self.options.line_length.is_some_and(|length| {
            let consumed = line.len().min(field + length.saturating_sub(column - 1));
            let visible = &line[field.min(consumed)..consumed];
            visible.last().is_some_and(|b| !matches!(b, b' ' | b'\t')) && visible.len() == length - (column - 1)
        });
        self.continued = Some(Continued { line: self.lines.len(), code_end, full, last_byte });
        self.lines.push(out);
    }
}

/// Convert fixed-form source to free form.
pub fn convert(source: &[u8], options: &Options) -> Converted {
    let mut converter = Converter {
        options,
        lines: vec![],
        warnings: vec![],
        state: Lexical::default(),
        continued: None,
        truncated: 0,
    };
    let crlf = source.windows(2).any(|w| w == b"\r\n");
    let mut ends = vec![];
    let mut offset = 0;
    let mut number = 0;
    while offset < source.len() {
        number += 1;
        let newline = source[offset..].iter().position(|b| *b == b'\n').map(|p| offset + p);
        let mut end = newline.unwrap_or(source.len());
        let next = newline.map_or(source.len(), |p| p + 1);
        if end > offset && source[end - 1] == b'\r' {
            end -= 1;
        }
        let line = &source[offset..end];
        let mut out = Line::default();
        match classify(line, options.line_length) {
            Kind::Blank => {}
            Kind::Comment => {
                if line[0] == b'!' || line[0] == b' ' {
                    for (i, b) in line.iter().enumerate() {
                        out.copy(*b, offset + i);
                    }
                } else {
                    out.synth(b"!", offset);
                    for (i, b) in line.iter().enumerate().skip(1) {
                        out.copy(*b, offset + i);
                    }
                }
            }
            Kind::Debug if !options.debug_lines_as_code => {
                out.synth(b"!", offset);
                for (i, b) in line.iter().enumerate() {
                    out.copy(*b, offset + i);
                }
            }
            Kind::Debug => {
                let mut code = line.to_vec();
                code[0] = b' ';
                match classify(&code, options.line_length) {
                    Kind::Statement { label_end, continuation, field, tab } => {
                        converter.statement(number, &code, offset, label_end, continuation, field, tab);
                        ends.push((next, end));
                        offset = next;
                        continue;
                    }
                    _ => {
                        for (i, b) in line.iter().enumerate().skip(1) {
                            out.copy(*b, offset + i);
                        }
                    }
                }
            }
            Kind::Preprocessor => {
                for (i, b) in line.iter().enumerate() {
                    out.copy(*b, offset + i);
                }
            }
            Kind::Statement { label_end, continuation, field, tab } => {
                converter.statement(number, line, offset, label_end, continuation, field, tab);
                ends.push((next, end));
                offset = next;
                continue;
            }
            Kind::Misplaced => {
                converter.warn(number, "statement starts before column 7");
                converter.statement(number, line, offset, 0, false, 0, true);
                ends.push((next, end));
                offset = next;
                continue;
            }
        }
        converter.lines.push(out);
        ends.push((next, end));
        offset = next;
    }
    converter.start_statement(number + 1);
    if converter.truncated > 0 {
        let length = options.line_length.unwrap_or(0);
        let message = format!(
            "text after column {length} ignored on {} line(s); sequence numbers are expected there, otherwise set the line length",
            converter.truncated
        );
        converter.warnings.insert(0, Warning { line: 0, kind: WarningKind::Truncation, message });
    }
    let newline: &[u8] = if crlf { b"\r\n" } else { b"\n" };
    let mut text = vec![];
    let mut map = vec![];
    let count = converter.lines.len();
    for (i, (line, (next, end))) in converter.lines.into_iter().zip(ends).enumerate() {
        if line.text.len() > FREE_LINE_LENGTH {
            converter.warnings.push(Warning {
                line: i + 1,
                kind: WarningKind::FreeLineLength,
                message: format!("line has {} characters, more than free form allows by default ({FREE_LINE_LENGTH})", line.text.len()),
            });
        }
        text.extend_from_slice(&line.text);
        map.extend_from_slice(&line.map);
        // Keep a final line without a newline as it was.
        if i + 1 < count || next > end {
            text.extend_from_slice(newline);
            map.extend(std::iter::repeat(end).take(newline.len()));
        }
    }
    map.push(source.len());
    converter.warnings.sort_by_key(|w| w.line);
    Converted { text, map, warnings: converter.warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free(source: &str) -> String {
        free_with(source, &Options::default())
    }
    fn free_with(source: &str, options: &Options) -> String {
        let converted = convert(source.as_bytes(), options);
        assert_eq!(converted.map.len(), converted.text.len() + 1);
        String::from_utf8(converted.text).unwrap()
    }

    #[test]
    fn comments_labels_and_statements_keep_their_columns() {
        let source = "C     A comment\n* another\nc lower\n      PROGRAM P\n   10 CONTINUE\n      X = 1 ! inline\n\n      END\n";
        assert_eq!(free(source), "!     A comment\n! another\n! lower\n      PROGRAM P\n   10 CONTINUE\n      X = 1 ! inline\n\n      END\n");
    }

    #[test]
    fn continuation_lines_get_ampersands() {
        let source = "      CALL S(A,\n     1       B, ! note\nC comment between\n     $       C)\n";
        assert_eq!(free(source), "      CALL S(A,&\n     &       B,& ! note\n! comment between\n     &       C)\n");
    }

    #[test]
    fn continued_character_literal_is_padded_to_column_72() {
        let source = "      PRINT *, 'ABC\n     1DEF'\n";
        let expected = format!("      PRINT *, 'ABC{}&\n     &DEF'\n", " ".repeat(72 - 19));
        assert_eq!(free(source), expected);
    }

    #[test]
    fn exclamation_and_quotes_in_literals() {
        assert_eq!(free("      PRINT *, 'It''s ! not a comment', \"!\"\n"), "      PRINT *, 'It''s ! not a comment', \"!\"\n");
    }

    #[test]
    fn sequence_numbers_are_dropped() {
        let source = format!("      X = 1{}SEQ00010\n", " ".repeat(61));
        let converted = convert(source.as_bytes(), &Options::default());
        assert_eq!(String::from_utf8(converted.text).unwrap(), "      X = 1\n");
        assert_eq!(converted.warnings.len(), 1);
    }

    #[test]
    fn tab_format() {
        assert_eq!(free("\tX = 1\n10\tY = 2\n\t1+ 3\n"), "      X = 1\n10    Y = 2&\n     &+ 3\n");
    }

    #[test]
    fn hollerith_in_format_becomes_a_literal() {
        assert_eq!(free("  100 FORMAT(1X,5HHELLO,2HA',I3)\n"), "  100 FORMAT(1X,'HELLO','A''',I3)\n");
        assert_eq!(free("  100 FORMAT(1X,10HTWO  WORDS)\n"), "  100 FORMAT(1X,'TWO  WORDS')\n");
        assert_eq!(free("  100 FORMAT(1H1/1X,3HA B)\n"), "  100 FORMAT('1'/1X,'A B')\n");
        let all = Options { hollerith: Hollerith::All, ..Options::default() };
        assert_eq!(free_with("      DATA A /2*4HABCD/\n", &all), "      DATA A /2*'ABCD'/\n");
    }

    #[test]
    fn hollerith_elsewhere_is_kept_by_default() {
        let converted = convert(b"      DATA I /4HABCD/\n", &Options::default());
        assert_eq!(String::from_utf8(converted.text).unwrap(), "      DATA I /4HABCD/\n");
        assert_eq!(converted.warnings.len(), 1);
        let all = Options { hollerith: Hollerith::All, ..Options::default() };
        assert_eq!(free_with("      CALL S(5HA!B C)\n", &all), "      CALL S('A!B C')\n");
    }

    #[test]
    fn names_and_numbers_before_h_are_not_hollerith() {
        let source = "      X2H = 1\n      Y = A*2\n      Z = 1.5\n      DO 10 H = 1, 2\n      REAL*8 HALF\n      CHARACTER*4 HNAME\n      X = (2 H)\n";
        assert_eq!(free(source), source);
    }

    #[test]
    fn hollerith_continued_onto_the_next_line() {
        let source = format!("{:<64}8HABCDEF\n     1GH,I3)\n", "  100 FORMAT(1X,");
        let converted = free(&source);
        assert!(converted.ends_with("'ABCDEF&\n     &GH',I3)\n"), "{converted}");
    }

    #[test]
    fn names_join_across_a_full_line_and_separate_otherwise() {
        let full = format!("      {}\n     1BLE = 1\n", "X".repeat(61) + "VARIA");
        assert!(free(&full).ends_with("VARIA&\n     &BLE = 1\n"));
        assert_eq!(free("      REAL\n     1X\n"), "      REAL&\n     & X\n");
    }

    #[test]
    fn blanks_between_digits_are_removed() {
        assert_eq!(
            free("      PARAMETER (SPI4 = .70710 67811 8490D0, N = 1 000)\n      X = 'A 1 2'\n"),
            "      PARAMETER (SPI4 = .70710678118490D0, N = 1000)\n      X = 'A 1 2'\n"
        );
        assert_eq!(free("      GO TO 10\n      DO 10 I = 1, 2\n"), "      GO TO 10\n      DO 10 I = 1, 2\n");
    }

    #[test]
    fn blanks_inside_dotted_operators_are_removed() {
        assert_eq!(
            free("      IF( ( L.LT.M ) .AND .( . NOT.Q ) .OR. X. EQ .1.0 ) Q = . TRUE.\n"),
            "      IF( ( L.LT.M ) .AND.( .NOT.Q ) .OR. X.EQ.1.0 ) Q = .TRUE.\n"
        );
        assert_eq!(free("      X = 1. + 2.\n      P = 'A .AND .B'\n"), "      X = 1. + 2.\n      P = 'A .AND .B'\n");
    }

    #[test]
    fn blanks_before_exponents_are_removed() {
        assert_eq!(
            free("      PARAMETER (A = 1.57 d0, P = +0.75 D+2, Q = 1. E5, R = .5 e-1)\n"),
            "      PARAMETER (A = 1.57d0, P = +0.75D+2, Q = 1.E5, R = .5e-1)\n"
        );
        let kept = "      DO 100 E1 = 1, 2\n      IF (X .EQ. E1) Y = A1 D2\n";
        assert_eq!(free(kept), kept);
    }

    #[test]
    fn keyword_runs_into_literal() {
        assert_eq!(free("      STOP'Abort'\n  510 PAUSE\"X\"\n"), "      STOP 'Abort'\n  510 PAUSE \"X\"\n");
        assert_eq!(free("      IF (L .GT. 10) STOP\n     1'Done'\n"), "      IF (L .GT. 10) STOP&\n     & 'Done'\n");
        assert_eq!(free("      I = Z'FF' + B'101'\n      X = F(A,'B')\n"), "      I = Z'FF' + B'101'\n      X = F(A,'B')\n");
    }

    #[test]
    fn debug_lines() {
        assert_eq!(free("D     PRINT *, X\n"), "!D     PRINT *, X\n");
        let code = Options { debug_lines_as_code: true, ..Options::default() };
        assert_eq!(free_with("D     PRINT *, X\n", &code), "      PRINT *, X\n");
    }

    #[test]
    fn crlf_and_map() {
        let source = b"C x\r\n      X = 1\r\n     1 + 2\r\n";
        let converted = convert(source, &Options::default());
        assert_eq!(String::from_utf8(converted.text.clone()).unwrap(), "! x\r\n      X = 1&\r\n     & + 2\r\n");
        let x = converted.text.iter().position(|b| *b == b'X').unwrap();
        assert_eq!(source[converted.source_offset(x)], b'X');
        let two = converted.text.iter().position(|b| *b == b'2').unwrap();
        assert_eq!(source[converted.source_offset(two)], b'2');
    }

    #[test]
    fn line_numbers_are_preserved() {
        let source = "C a\n\n      X = 1\n     1 + 2\n# define Y\n      END";
        let converted = free(source);
        assert_eq!(converted.lines().count(), source.lines().count());
        assert!(converted.ends_with("END"));
    }

    #[test]
    fn misplaced_statement_and_orphan_continuation_warn() {
        let converted = convert(b"PROGRAM P\n     1X = 1\n", &Options::default());
        assert_eq!(converted.warnings.len(), 1, "{:?}", converted.warnings);
        let orphan = convert(b"     1X = 1\n", &Options::default());
        assert_eq!(orphan.warnings[0].message, "continuation line without a statement to continue");
    }
}

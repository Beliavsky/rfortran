// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Allocation-free format parser and renderer shared by the interpreter and
//! the standalone native runtime.
//!
//! Supported edit descriptors: A, AT, I, B, O, Z, F, E, EN, ES, D, G, L, X, T, TL,
//! TR, /, :, kP, SP, SS, S, BN and BZ, character string literals, nested and
//! unlimited groups, and format reversion. Numbers are converted with
//! `core::fmt`, which rounds the exact binary value to nearest, ties to even.

/// Internal descriptor codes for the multi-letter forms.
pub const ES: u8 = b'S';
pub const EN: u8 = b'N';
pub const TL: u8 = b'<';
pub const TR: u8 = b'>';
pub const T: u8 = b't';
/// CHARACTER output with trailing blanks removed (Fortran 2023).
pub const AT: u8 = b'v';
pub const SIGN_PLUS: u8 = b'+';
pub const SIGN_DEFAULT: u8 = b'-';
pub const SCALE: u8 = b'p';
/// Input-only modes (BN, BZ) that do not change output.
pub const NO_EFFECT: u8 = b'_';
/// Record break inserted by format reversion.
pub const REVERSION: u8 = b'r';

#[derive(Clone, Copy, Debug)]
pub struct Descriptor<'a> {
    pub code: u8,
    pub width: usize,
    pub precision: usize,
    /// Exponent digits of Ew.dEe, ESw.dEe, ENw.dEe and Gw.dEe; 0 if absent.
    pub exponent: usize,
    /// Whether an explicit .d or .m was given.
    pub has_precision: bool,
    pub repeat: usize,
    /// The k of kP.
    pub scale: i32,
    pub literal: &'a [u8],
}

/// Data edit descriptors, which consume an output list item.
pub fn is_data(code: u8) -> bool {
    matches!(code, b'a' | AT | b'i' | b'b' | b'o' | b'z' | b'f' | b'e' | ES | EN | b'd' | b'g' | b'l')
}

#[derive(Clone, Copy)]
pub struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Parser<'a> {
    pub fn new(mut bytes: &'a [u8]) -> Result<Self, &'static str> {
        if !bytes.is_ascii() || bytes.contains(&0) {
            return Err("format requires non-NUL ASCII characters");
        }
        while bytes.first().is_some_and(u8::is_ascii_whitespace) {
            bytes = &bytes[1..];
        }
        while bytes.last().is_some_and(u8::is_ascii_whitespace) {
            bytes = &bytes[..bytes.len() - 1];
        }
        if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
            return Err("output format must be parenthesized");
        }
        Ok(Self {
            bytes: &bytes[1..bytes.len() - 1],
            pos: 0,
        })
    }
    fn spaces(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn number(&mut self) -> Result<Option<usize>, &'static str> {
        self.spaces();
        let start = self.pos;
        let mut n = 0usize;
        while let Some(c @ b'0'..=b'9') = self.bytes.get(self.pos) {
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add((c - b'0') as usize))
                .ok_or("output descriptor exceeds supported limits")?;
            self.pos += 1;
        }
        Ok(if self.pos == start { None } else { Some(n) })
    }
    fn letter(&self, offset: usize) -> u8 {
        self.bytes.get(self.pos + offset).map_or(0, u8::to_ascii_lowercase)
    }
    pub fn next(&mut self) -> Result<Option<Descriptor<'a>>, &'static str> {
        self.spaces();
        if self.bytes.get(self.pos) == Some(&b',') {
            self.pos += 1;
            self.spaces();
        }
        if self.pos == self.bytes.len() {
            return Ok(None);
        }
        let mut d = Descriptor {
            code: 0,
            width: 0,
            precision: 0,
            exponent: 0,
            has_precision: false,
            repeat: 1,
            scale: 0,
            literal: &[],
        };
        let c = self.bytes[self.pos];
        let mut joined = false;
        if c == b'\'' || c == b'"' {
            self.pos += 1;
            let start = self.pos;
            loop {
                let Some(&ch) = self.bytes.get(self.pos) else {
                    return Err("unterminated literal in output format");
                };
                self.pos += 1;
                if ch == c {
                    if self.bytes.get(self.pos) == Some(&c) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }
            d.code = c;
            d.literal = &self.bytes[start..self.pos - 1];
        } else {
            let negative = matches!(c, b'-' | b'+');
            if negative {
                self.pos += 1;
            }
            let count = self.number()?;
            self.spaces();
            let Some(&code) = self.bytes.get(self.pos) else {
                return Err("invalid output descriptor");
            };
            let code = code.to_ascii_lowercase();
            self.pos += 1;
            if code == b'p' {
                // kP sets the scale factor; the next descriptor may follow
                // without a comma.
                let k = count.ok_or("P edit descriptor requires a scale factor")?;
                let k = i32::try_from(k).map_err(|_| "output descriptor exceeds supported limits")?;
                d.code = SCALE;
                d.scale = if c == b'-' { -k } else { k };
                joined = true;
            } else {
                if negative {
                    return Err("invalid output descriptor");
                }
                d.repeat = count.unwrap_or(1);
                d.code = match (code, self.letter(0)) {
                    (b'a', b't') => { self.pos += 1; AT }
                    (b'e', b's') => { self.pos += 1; ES }
                    (b'e', b'n') => { self.pos += 1; EN }
                    (b't', b'l') => { self.pos += 1; TL }
                    (b't', b'r') => { self.pos += 1; TR }
                    (b't', _) => T,
                    (b's', b'p') => { self.pos += 1; SIGN_PLUS }
                    (b's', b's') => { self.pos += 1; SIGN_DEFAULT }
                    (b's', _) => SIGN_DEFAULT,
                    (b'b', b'n' | b'z') => { self.pos += 1; NO_EFFECT }
                    (b'd', b'c') => return Err("DC (decimal comma) output is not yet supported"),
                    (b'd', b'p') => { self.pos += 1; NO_EFFECT }
                    (b'r', b'u' | b'd' | b'z' | b'n' | b'c' | b'p') => return Err("rounding-mode edit descriptors are not yet supported"),
                    (b'a' | b'i' | b'b' | b'o' | b'z' | b'f' | b'e' | b'd' | b'g' | b'l' | b'x' | b'/' | b':', _) => code,
                    _ => return Err("unsupported output descriptor"),
                };
                if matches!(d.code, T | TL | TR) {
                    if count.is_some() {
                        return Err("T, TL and TR edit descriptors cannot have a repeat count");
                    }
                    d.width = self.number()?.ok_or("T, TL and TR edit descriptors require a position")?;
                    if d.code == T && d.width == 0 {
                        return Err("T edit descriptor position must be positive");
                    }
                } else if matches!(d.code, SIGN_PLUS | SIGN_DEFAULT | NO_EFFECT) && count.is_some() {
                    return Err("invalid output descriptor");
                } else if is_data(d.code) {
                    let width = self.number()?;
                    if d.code == AT && width.is_some() {return Err("AT edit descriptor cannot have a width");}
                    if width.is_none() && !matches!(d.code, b'a' | AT) {
                        return Err("output descriptor requires a width");
                    }
                    d.width = width.unwrap_or(0);
                    if self.bytes.get(self.pos) == Some(&b'.') {
                        if matches!(d.code, b'a' | AT | b'l') {
                            return Err("invalid output descriptor precision");
                        }
                        self.pos += 1;
                        d.precision = self
                            .number()?
                            .ok_or("output descriptor requires a precision")?;
                        d.has_precision = true;
                        if matches!(d.code, b'e' | ES | EN | b'g' | b'd') && self.letter(0) == b'e' {
                            if d.code == b'd' {
                                return Err("D edit descriptor cannot have an exponent width");
                            }
                            self.pos += 1;
                            d.exponent = self.number()?.ok_or("exponent width required after E")?;
                            if d.exponent == 0 {
                                return Err("exponent width must be positive");
                            }
                        }
                    } else if matches!(d.code, b'i' | b'b' | b'o' | b'z') {
                        d.precision = 1;
                    } else if matches!(d.code, b'f' | b'e' | ES | EN | b'd') {
                        return Err("F, E, EN, ES and D descriptors require width and precision");
                    } else if d.code == b'g' && d.width != 0 {
                        return Err("Gw requires a precision (Gw.d)");
                    }
                    if matches!(d.code, b'e' | ES | EN | b'd') && d.width == 0 && d.precision == 0 {
                        return Err("E, EN, ES and D output require a precision");
                    }
                }
            }
        }
        if d.repeat == 0 || d.repeat > 10000 || d.width > 256 || d.precision > 256 || d.exponent > 9 || !(-128..=128).contains(&d.scale) {
            return Err("output descriptor exceeds supported limits");
        }
        self.spaces();
        if let Some(c) = self.bytes.get(self.pos) {
            if !joined && *c != b',' && *c != b'/' && *c != b':' && d.code != b'/' && d.code != b':' {
                return Err("expected comma between output descriptors");
            }
        }
        Ok(Some(d))
    }
}

/// Inspect descriptor syntax without mistaking quoted text for an AT descriptor.
pub fn contains_at(format: &[u8]) -> bool {
    fn scan(parser: &mut Parser<'_>, depth: usize) -> Result<bool, &'static str> {
        if depth>32 {return Err("format nesting exceeds supported limits");}
        loop {
            parser.separator();
            if let Some((body,_,_))=parser.group()? {
                if scan(&mut Parser {bytes:body,pos:0},depth+1)? {return Ok(true);}
            } else {
                match parser.next()? {Some(d) if d.code==AT=>return Ok(true),Some(_)=>{},None=>return Ok(false)}
            }
        }
    }
    Parser::new(format).and_then(|mut p|scan(&mut p,0)).unwrap_or(false)
}

/// Item tags: 1 INTEGER, 2 REAL, 3 LOGICAL, 4 CHARACTER.
pub fn matches_type(code: u8, tag: u64) -> bool {
    match code {
        b'i' | b'b' | b'o' | b'z' => tag == 1,
        b'f' | b'e' | ES | EN | b'd' => tag == 2,
        b'l' => tag == 3,
        b'a' | AT => tag == 4,
        b'g' => matches!(tag, 1..=4),
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    parser: Parser<'a>,
    remaining: usize,
    unlimited: bool,
}

/// Expand finite nested groups and a final unlimited-repeat group without
/// allocating. Unlimited repetition is bounded by the actual output list.
/// When the format ends with items left, it reverts to the last top-level
/// group (or the whole format) after a record break.
pub struct GroupParser<'a> {
    frames: [Frame<'a>; 32],
    depth: usize,
    count: usize,
    consumed: usize,
    operations: usize,
    data: bool,
    /// Start of the rightmost top-level group, including its repeat count.
    reversion: usize,
}
impl<'a> Parser<'a> {
    fn separator(&mut self) {
        self.spaces();
        if self.bytes.get(self.pos) == Some(&b',') {
            self.pos += 1;
            self.spaces();
        }
    }
    fn group(&mut self) -> Result<Option<(&'a [u8], usize, bool)>, &'static str> {
        let saved = self.pos;
        let unlimited = self.bytes.get(self.pos) == Some(&b'*');
        let repeat = if unlimited {
            self.pos += 1;
            1
        } else {
            self.number()?.unwrap_or(1)
        };
        self.spaces();
        if self.bytes.get(self.pos) != Some(&b'(') {
            self.pos = saved;
            return Ok(None);
        }
        if repeat == 0 || repeat > 10000 {
            return Err("output descriptor exceeds supported limits");
        }
        self.pos += 1;
        let start = self.pos;
        let mut nesting = 1;
        let mut quote = 0;
        while let Some(&ch) = self.bytes.get(self.pos) {
            if quote != 0 {
                if ch == quote {
                    if self.bytes.get(self.pos + 1) == Some(&quote) {
                        self.pos += 1;
                    } else {
                        quote = 0;
                    }
                }
            } else if ch == b'\'' || ch == b'"' {
                quote = ch;
            } else if ch == b'(' {
                nesting += 1;
            } else if ch == b')' {
                nesting -= 1;
                if nesting == 0 {
                    break;
                }
            }
            self.pos += 1;
        }
        if nesting != 0 {
            return Err("unterminated output format group");
        }
        let body = &self.bytes[start..self.pos];
        self.pos += 1;
        self.spaces();
        if self
            .bytes
            .get(self.pos)
            .is_some_and(|c| *c != b',' && *c != b'/' && *c != b':')
        {
            return Err("expected comma between output descriptors");
        }
        Ok(Some((body, repeat, unlimited)))
    }
}
// Returns whether the format has a data descriptor, and the start of the
// rightmost top-level group.
fn validate_groups(mut parser: Parser<'_>, depth: usize) -> Result<(bool, usize), &'static str> {
    if depth >= 32 {
        return Err("output format nesting exceeds supported limits");
    }
    let mut data = false;
    let mut reversion = 0;
    loop {
        parser.separator();
        if parser.pos == parser.bytes.len() {
            return Ok((data, reversion));
        }
        let start = parser.pos;
        if let Some((body, _, unlimited)) = parser.group()? {
            let (inner, _) = validate_groups(
                Parser {
                    bytes: body,
                    pos: 0,
                },
                depth + 1,
            )?;
            if unlimited {
                if depth != 0 || parser.pos != parser.bytes.len() {
                    return Err("unlimited output format group must be last and outermost");
                }
                if !inner {
                    return Err("unlimited output format group requires a data descriptor");
                }
            }
            reversion = start;
            data |= inner;
        } else if let Some(d) = parser.next()? {
            data |= is_data(d.code);
        }
    }
}
impl<'a> GroupParser<'a> {
    pub fn new(bytes: &'a [u8], count: usize) -> Result<Self, &'static str> {
        let parser = Parser::new(bytes)?;
        let (data, reversion) = validate_groups(parser, 0)?;
        if count > 0 && !data {
            return Err("format has no data edit descriptor for the output list");
        }
        let empty = Frame {
            parser: Parser { bytes: &[], pos: 0 },
            remaining: 0,
            unlimited: false,
        };
        let mut frames = [empty; 32];
        frames[0] = Frame {
            parser,
            remaining: 1,
            unlimited: false,
        };
        Ok(Self {
            frames,
            depth: 1,
            count,
            consumed: 0,
            operations: 0,
            data,
            reversion,
        })
    }
    pub fn next(&mut self) -> Result<Option<Descriptor<'a>>, &'static str> {
        while self.depth > 0 {
            self.operations += 1;
            if self.operations > self.count.saturating_mul(256).saturating_add(100000) {
                return Err("output format expansion exceeds supported limits");
            }
            let index = self.depth - 1;
            self.frames[index].parser.separator();
            if self.frames[index].parser.pos == self.frames[index].parser.bytes.len() {
                let frame = &mut self.frames[index];
                if (frame.unlimited && self.consumed < self.count)
                    || (!frame.unlimited && frame.remaining > 1)
                {
                    if !frame.unlimited {
                        frame.remaining -= 1;
                    }
                    frame.parser.pos = 0;
                } else if index == 0 && self.consumed < self.count && self.data {
                    // Format reversion: a new record, then the last top-level
                    // group with its repeat count, or the whole format.
                    frame.parser.pos = self.reversion;
                    return Ok(Some(Descriptor {
                        code: REVERSION,
                        width: 0,
                        precision: 0,
                        exponent: 0,
                        has_precision: false,
                        repeat: 1,
                        scale: 0,
                        literal: &[],
                    }));
                } else {
                    self.depth -= 1;
                }
                continue;
            }
            if let Some((body, repeat, unlimited)) = self.frames[index].parser.group()? {
                if unlimited && self.consumed >= self.count {
                    continue;
                }
                self.frames[self.depth] = Frame {
                    parser: Parser {
                        bytes: body,
                        pos: 0,
                    },
                    remaining: repeat,
                    unlimited,
                };
                self.depth += 1;
                continue;
            }
            if let Some(mut d) = self.frames[index].parser.next()? {
                // Count expanded descriptors, not only group traversal. Nested
                // literal/spacing repeats must not create unbounded field lists.
                self.operations = self.operations.saturating_add(d.repeat);
                if self.operations > self.count.saturating_mul(256).saturating_add(100000) {
                    return Err("output format expansion exceeds supported limits");
                }
                if d.code==b':' {
                    if d.repeat!=1 {return Err("colon edit descriptor cannot have a repeat count");}
                    if self.consumed>=self.count {self.depth=0;return Ok(None);}
                    continue;
                }
                if is_data(d.code) {
                    if self.consumed >= self.count {
                        self.depth = 0;
                        return Ok(None);
                    }
                    d.repeat = d.repeat.min(self.count - self.consumed);
                    self.consumed += d.repeat;
                }
                return Ok(Some(d));
            }
        }
        Ok(None)
    }
}

/// An output list item.
#[derive(Clone, Copy)]
pub enum Value<'a> {
    /// The value and its kind in bytes (1, 2, 4 or 8).
    Integer(i64, u8),
    /// The value and its kind (4 or 8).
    Real(f64, u8),
    Logical(bool),
    Character(&'a [u8]),
}
impl Value<'_> {
    pub fn tag(&self) -> u64 {
        match self {
            Value::Integer(..) => 1,
            Value::Real(..) => 2,
            Value::Logical(_) => 3,
            Value::Character(_) => 4,
        }
    }
}

/// A fixed-capacity byte buffer that `core::fmt` can write into.
struct Text {
    bytes: [u8; 720],
    len: usize,
}
impl Text {
    fn new() -> Self {
        Self { bytes: [0; 720], len: 0 }
    }
    fn push(&mut self, byte: u8) {
        if self.len < self.bytes.len() {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }
    fn extend(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push(b);
        }
    }
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
impl core::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if self.len + s.len() > self.bytes.len() {
            return Err(core::fmt::Error);
        }
        self.extend(s.as_bytes());
        Ok(())
    }
}

/// List-directed text of a REAL value with `digits` significant digits, as
/// %g chooses fixed or exponent form, without trailing zeros but with at least
/// one digit after the decimal point, and with an unpadded exponent: `0.5`,
/// `60.0`, `1.0e-6`, `2.5e10`. Writes into `out` and returns the length.
pub fn list_real(x: f64, digits: usize, out: &mut [u8]) -> usize {
    use core::fmt::Write;
    let mut text = Text::new();
    let digits = digits.max(1);
    if x.is_nan() {
        text.extend(b"NaN");
    } else if x.is_infinite() {
        text.extend(if x < 0.0 { b"-Infinity" } else { b"Infinity" });
    } else if x == 0.0 {
        text.extend(if x.is_sign_negative() { b"-0.0" } else { b"0.0" });
    } else {
        let mut raw = Text::new();
        let _ = write!(raw, "{:.*e}", digits - 1, x);
        let raw = raw.as_bytes();
        let e = raw.iter().position(|&c| c == b'e').unwrap_or(raw.len());
        let exponent = core::str::from_utf8(&raw[e + 1..]).ok().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
        // Trailing zeros of the digits are dropped, keeping one decimal.
        let trimmed = |t: &mut Text| {
            if let Some(point) = t.as_bytes().iter().position(|&c| c == b'.') {
                while t.len > point + 2 && t.bytes[t.len - 1] == b'0' { t.len -= 1; }
            } else {
                t.extend(b".0");
            }
        };
        if exponent < -4 || exponent >= digits as i32 {
            text.extend(&raw[..e]);
            trimmed(&mut text);
            let _ = write!(text, "e{exponent}");
        } else {
            let decimals = (digits as i32 - 1 - exponent).max(0) as usize;
            let _ = write!(text, "{:.*}", decimals, x);
            trimmed(&mut text);
        }
    }
    let n = text.len.min(out.len());
    out[..n].copy_from_slice(&text.as_bytes()[..n]);
    n
}

/// `|x|` rounded to `digits` significant digits: the digits and the decimal
/// exponent X with |x| = d.ddd * 10^X. Zero gives zeros and X = 0.
fn significant(x: f64, digits: usize) -> Result<(Text, i32), &'static str> {
    use core::fmt::Write;
    let digits = digits.max(1);
    let mut raw = Text::new();
    write!(raw, "{:.*e}", digits - 1, x.abs()).map_err(|_| "numeric formatting failed")?;
    let s = raw.as_bytes();
    let e = s.iter().position(|&c| c == b'e').ok_or("numeric formatting failed")?;
    let mut out = Text::new();
    for &c in &s[..e] {
        if c != b'.' {
            out.push(c);
        }
    }
    let mut exponent: i32 = 0;
    let mut negative = false;
    for &c in &s[e + 1..] {
        if c == b'-' {
            negative = true;
        } else {
            exponent = exponent * 10 + i32::from(c - b'0');
        }
    }
    if negative {
        exponent = -exponent;
    }
    if x == 0.0 {
        exponent = 0;
    }
    Ok((out, exponent))
}

struct Style {
    plus: bool,
    scale: i32,
}

fn sign(text: &mut Text, negative: bool, style: &Style) {
    if negative {
        text.push(b'-');
    } else if style.plus {
        text.push(b'+');
    }
}

/// Write `text` right-justified in `width` (0 = as is), or asterisks.
fn field(text: &[u8], width: usize, emit: &mut dyn FnMut(u8)) {
    if width != 0 && text.len() > width {
        for _ in 0..width {
            emit(b'*');
        }
        return;
    }
    for _ in text.len()..width {
        emit(b' ');
    }
    for &b in text {
        emit(b);
    }
}

// A field whose leading zero before the decimal point may be dropped to fit.
fn numeric_field(text: &Text, width: usize, drop_zero: bool, emit: &mut dyn FnMut(u8)) {
    let s = text.as_bytes();
    let skip = s.iter().position(|&c| c != b'-' && c != b'+').unwrap_or(0);
    let zero = s.get(skip) == Some(&b'0') && s.get(skip + 1) == Some(&b'.');
    if zero && (drop_zero || width != 0 && s.len() > width) {
        let mut shorter = Text::new();
        shorter.extend(&s[..skip]);
        shorter.extend(&s[skip + 1..]);
        field(shorter.as_bytes(), width, emit);
    } else {
        field(s, width, emit);
    }
}

fn special(x: f64, width: usize, style: &Style, emit: &mut dyn FnMut(u8)) {
    let mut text = Text::new();
    if x.is_nan() {
        text.extend(b"NaN");
    } else {
        let negative = x < 0.0;
        let room = if width == 0 { usize::MAX } else { width - usize::from(negative || style.plus) };
        sign(&mut text, negative, style);
        if room >= 8 {
            text.extend(b"Infinity");
        } else {
            text.extend(b"Inf");
        }
    }
    field(text.as_bytes(), width, emit);
}

fn render_f(x: f64, width: usize, decimals: usize, style: &Style, emit: &mut dyn FnMut(u8)) -> Result<(), &'static str> {
    use core::fmt::Write;
    if !x.is_finite() {
        special(x, width, style, emit);
        return Ok(());
    }
    let mut value = x;
    for _ in 0..style.scale.unsigned_abs() {
        value = if style.scale > 0 { value * 10.0 } else { value / 10.0 };
    }
    let mut digits = Text::new();
    write!(digits, "{:.*}", decimals, value.abs()).map_err(|_| "numeric formatting failed")?;
    let mut text = Text::new();
    sign(&mut text, value.is_sign_negative(), style);
    text.extend(digits.as_bytes());
    if decimals == 0 {
        text.push(b'.');
    }
    // F0.d writes no leading zero, except for a value with no fraction digits.
    numeric_field(&text, width, width == 0 && decimals > 0, emit);
    Ok(())
}

fn exponent_part(text: &mut Text, letter: u8, value: i32, digits: usize, minimal: bool) -> bool {
    let magnitude = value.unsigned_abs();
    let mut buffer = [0u8; 12];
    let mut n = 0;
    let mut m = magnitude;
    loop {
        buffer[n] = b'0' + (m % 10) as u8;
        n += 1;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    let width = if digits != 0 {
        if n > digits {
            return false;
        }
        text.push(letter);
        digits
    } else if magnitude <= 99 || minimal {
        text.push(letter);
        n.max(2)
    } else if magnitude <= 999 {
        3
    } else {
        return false;
    };
    text.push(if value < 0 { b'-' } else { b'+' });
    for _ in n..width {
        text.push(b'0');
    }
    for i in (0..n).rev() {
        text.push(buffer[i]);
    }
    true
}

// E, ES, EN and D editing.
fn render_e(x: f64, d: &Descriptor<'_>, style: &Style, emit: &mut dyn FnMut(u8)) -> Result<(), &'static str> {
    if !x.is_finite() {
        special(x, d.width, style, emit);
        return Ok(());
    }
    let decimals = d.precision;
    let mut text = Text::new();
    sign(&mut text, x.is_sign_negative(), style);
    let exponent = match d.code {
        ES => {
            let (digits, x10) = significant(x, decimals + 1)?;
            let s = digits.as_bytes();
            text.push(s[0]);
            text.push(b'.');
            text.extend(&s[1..]);
            x10
        }
        EN => {
            let (_, mut x10) = significant(x, decimals + 3)?;
            let mut result = None;
            for _ in 0..2 {
                let left = x10.rem_euclid(3) as usize + 1;
                let (digits, rounded) = significant(x, left + decimals)?;
                if rounded == x10 || x == 0.0 {
                    result = Some((digits, left));
                    break;
                }
                x10 = rounded;
            }
            let (digits, left) = result.ok_or("numeric formatting failed")?;
            let s = digits.as_bytes();
            text.extend(&s[..left]);
            text.push(b'.');
            text.extend(&s[left..]);
            x10 - (left as i32 - 1)
        }
        _ => {
            let k = style.scale;
            if k <= 0 {
                if k <= -(decimals as i32) {
                    return Err("scale factor out of range for E editing");
                }
                let (digits, x10) = significant(x, (decimals as i32 + k) as usize)?;
                text.extend(b"0.");
                for _ in 0..(-k) {
                    text.push(b'0');
                }
                text.extend(digits.as_bytes());
                if x == 0.0 { 0 } else { x10 + 1 - k }
            } else {
                if k >= decimals as i32 + 2 {
                    return Err("scale factor out of range for E editing");
                }
                let (digits, x10) = significant(x, decimals + 1)?;
                let s = digits.as_bytes();
                text.extend(&s[..k as usize]);
                text.push(b'.');
                text.extend(&s[k as usize..]);
                if x == 0.0 { 0 } else { x10 + 1 - k }
            }
        }
    };
    let letter = if d.code == b'd' { b'D' } else { b'E' };
    if !exponent_part(&mut text, letter, exponent, d.exponent, d.width == 0) {
        for _ in 0..d.width.max(1) {
            emit(b'*');
        }
        return Ok(());
    }
    numeric_field(&text, d.width, false, emit);
    Ok(())
}

// Gw.d and G0 editing of a REAL value.
fn render_g_real(x: f64, kind: u8, d: &Descriptor<'_>, style: &Style, emit: &mut dyn FnMut(u8)) -> Result<(), &'static str> {
    if !x.is_finite() {
        special(x, d.width, style, emit);
        return Ok(());
    }
    // G0 uses enough digits to distinguish values of the item's kind.
    let decimals = if d.width == 0 && !d.has_precision { if kind == 4 { 9 } else { 17 } } else { d.precision };
    let decimals = decimals.max(1);
    let (_, x10) = significant(x, decimals)?;
    let k = if x == 0.0 { 0 } else { x10 + 1 };
    if (0..=decimals as i32).contains(&k) {
        let blanks = if d.width == 0 { 0 } else if d.exponent != 0 { d.exponent + 2 } else { 4 };
        if d.width != 0 && d.width < blanks {
            for _ in 0..d.width {
                emit(b'*');
            }
            return Ok(());
        }
        let width = if d.width == 0 { 0 } else { d.width - blanks };
        let fixed = Style { plus: style.plus, scale: 0 };
        let mut inner = Text::new();
        render_f(x, 0, decimals - k.max(if x == 0.0 { 1 } else { 0 }) as usize, &fixed, &mut |b| inner.push(b))?;
        // G editing keeps the leading zero that F0.d drops.
        let mut text = Text::new();
        let s = inner.as_bytes();
        let skip = s.iter().position(|&c| c != b'-' && c != b'+').unwrap_or(0);
        text.extend(&s[..skip]);
        if s.get(skip) == Some(&b'.') {
            text.push(b'0');
        }
        text.extend(&s[skip..]);
        numeric_field(&text, width, false, emit);
        for _ in 0..blanks {
            emit(b' ');
        }
        Ok(())
    } else {
        let e = Descriptor { code: b'e', precision: decimals, ..*d };
        render_e(x, &e, style, emit)
    }
}

fn render_integer(value: i64, d: &Descriptor<'_>, style: &Style, emit: &mut dyn FnMut(u8)) {
    use core::fmt::Write;
    let mut text = Text::new();
    if value == 0 && d.precision == 0 {
        // Iw.0 of zero is blank; I0.0 still writes one blank.
        for _ in 0..d.width.max(1) {
            emit(b' ');
        }
        return;
    }
    sign(&mut text, value < 0, style);
    let mut digits = Text::new();
    let _ = write!(digits, "{}", value.unsigned_abs());
    for _ in digits.len..d.precision {
        text.push(b'0');
    }
    text.extend(digits.as_bytes());
    field(text.as_bytes(), d.width, emit);
}

// B, O and Z editing of the value's bits in its kind.
fn render_radix(value: i64, kind: u8, d: &Descriptor<'_>, emit: &mut dyn FnMut(u8)) {
    let bits = u32::from(kind.clamp(1, 8)) * 8;
    let pattern = if bits == 64 { value as u64 } else { (value as u64) & ((1u64 << bits) - 1) };
    let (shift, mask) = match d.code { b'b' => (1, 1), b'o' => (3, 7), _ => (4, 15) };
    let mut buffer = [0u8; 64];
    let mut n = 0;
    let mut m = pattern;
    while m != 0 {
        let digit = (m & mask) as u8;
        buffer[n] = if digit < 10 { b'0' + digit } else { b'A' + digit - 10 };
        n += 1;
        m >>= shift;
    }
    if pattern == 0 && d.precision == 0 {
        for _ in 0..d.width.max(1) {
            emit(b' ');
        }
        return;
    }
    let mut text = Text::new();
    for _ in n..d.precision.max(usize::from(n == 0)) {
        text.push(b'0');
    }
    for i in (0..n).rev() {
        text.push(buffer[i]);
    }
    field(text.as_bytes(), d.width, emit);
}

fn render_logical(value: bool, width: usize, emit: &mut dyn FnMut(u8)) {
    field(if value { b"T" } else { b"F" }, width.max(1), emit);
}

fn render_character(text: &[u8], width: usize, emit: &mut dyn FnMut(u8)) {
    if width == 0 {
        for &b in text {
            emit(b);
        }
    } else if text.len() >= width {
        for &b in &text[..width] {
            emit(b);
        }
    } else {
        field(text, width, emit);
    }
}

fn render_item(d: &Descriptor<'_>, value: Value<'_>, style: &Style, emit: &mut dyn FnMut(u8)) -> Result<(), &'static str> {
    // A REAL(4) value may arrive with the extra precision of a double
    // intermediate; write the single-precision value.
    let value = match value {
        Value::Real(x, 4) => Value::Real(x as f32 as f64, 4),
        other => other,
    };
    match (d.code, value) {
        (b'a', Value::Character(s)) => render_character(s, d.width, emit),
        (AT, Value::Character(s)) => {
            let end=s.iter().rposition(|b|*b!=b' ').map_or(0,|i|i+1);
            render_character(&s[..end],0,emit);
        },
        (b'i', Value::Integer(v, _)) => render_integer(v, d, style, emit),
        (b'b' | b'o' | b'z', Value::Integer(v, kind)) => render_radix(v, kind, d, emit),
        (b'f', Value::Real(x, _)) => render_f(x, d.width, d.precision, style, emit)?,
        (b'e' | ES | EN | b'd', Value::Real(x, _)) => render_e(x, d, style, emit)?,
        (b'l', Value::Logical(v)) => render_logical(v, d.width, emit),
        (b'g', Value::Real(x, kind)) => render_g_real(x, kind, d, style, emit)?,
        (b'g', Value::Integer(v, _)) => {
            let i = Descriptor { code: b'i', precision: 1, ..*d };
            render_integer(v, &i, style, emit)
        }
        (b'g', Value::Logical(v)) => render_logical(v, d.width, emit),
        (b'g', Value::Character(s)) => render_character(s, d.width, emit),
        _ => return Err("descriptor does not match output item type"),
    }
    Ok(())
}

/// The record being written. Without T or TL editing, bytes go straight to
/// `emit`; with them, a record is kept until it ends so positions can move left.
struct Record<'e> {
    emit: &'e mut dyn FnMut(u8),
    buffered: bool,
    bytes: [u8; 4096],
    len: usize,
    position: usize,
}
impl Record<'_> {
    fn put(&mut self, byte: u8) -> Result<(), &'static str> {
        if byte == b'\n' {
            self.end();
            (self.emit)(b'\n');
            return Ok(());
        }
        if !self.buffered {
            while self.len < self.position {
                (self.emit)(b' ');
                self.len += 1;
            }
            (self.emit)(byte);
            self.len += 1;
            self.position += 1;
            return Ok(());
        }
        if self.position >= self.bytes.len() {
            return Err("a record with T or TL editing exceeds 4096 characters");
        }
        while self.len < self.position {
            self.bytes[self.len] = b' ';
            self.len += 1;
        }
        self.bytes[self.position] = byte;
        self.position += 1;
        self.len = self.len.max(self.position);
        Ok(())
    }
    fn end(&mut self) {
        if self.buffered {
            for i in 0..self.len {
                (self.emit)(self.bytes[i]);
            }
        }
        self.len = 0;
        self.position = 0;
    }
}

/// Write `count` items with `format`, visiting item `i` with `item(i, f)`.
/// The whole statement is checked before anything is written; a record break
/// is a newline, and the final record is not terminated. Trailing X or TR
/// positioning writes blanks only when the output is nonadvancing, where the
/// next statement continues the record.
pub fn render_with(format: &[u8], count: usize, item: &dyn Fn(usize, &mut dyn FnMut(Value<'_>)), emit: &mut dyn FnMut(u8), advancing: bool) -> Result<(), &'static str> {
    let mut parser = GroupParser::new(format, count)?;
    let mut index = 0;
    let mut tabs = false;
    let mut error = None;
    while let Some(d) = parser.next()? {
        tabs |= matches!(d.code, T | TL);
        if is_data(d.code) {
            for _ in 0..d.repeat {
                if index == count {
                    break;
                }
                item(index, &mut |v| {
                    if !matches_type(d.code, v.tag()) {
                        error = Some("descriptor does not match output item type");
                    }
                });
                index += 1;
            }
        }
    }
    if let Some(e) = error {
        return Err(e);
    }
    let mut record = Record { emit, buffered: tabs, bytes: [0; 4096], len: 0, position: 0 };
    let mut style = Style { plus: false, scale: 0 };
    let mut parser = GroupParser::new(format, count)?;
    index = 0;
    'descriptors: while let Some(d) = parser.next()? {
        for _ in 0..d.repeat {
            match d.code {
                b'\'' | b'"' => {
                    let mut i = 0;
                    while i < d.literal.len() {
                        let c = d.literal[i];
                        record.put(c)?;
                        i += 1;
                        if c == d.code && d.literal.get(i) == Some(&c) {
                            i += 1;
                        }
                    }
                }
                b'x' => record.position += 1,
                b'/' | REVERSION => record.put(b'\n')?,
                T => record.position = d.width - 1,
                TL => record.position = record.position.saturating_sub(d.width),
                TR => record.position += d.width,
                SIGN_PLUS => style.plus = true,
                SIGN_DEFAULT => style.plus = false,
                SCALE => style.scale = d.scale,
                NO_EFFECT => {}
                _ => {
                    if index == count {
                        break 'descriptors;
                    }
                    // Fields go straight to the record: CHARACTER items have no
                    // length limit.
                    let mut result = Ok(());
                    let mut put = Ok(());
                    item(index, &mut |v| result = render_item(&d, v, &style, &mut |b| {
                        if put.is_ok() {
                            put = record.put(b);
                        }
                    }));
                    result?;
                    put?;
                    index += 1;
                }
            }
        }
    }
    if advancing {
        record.position = record.position.min(record.len);
    } else if record.position > record.len {
        // The final position carries over to the next nonadvancing statement.
        let position = record.position;
        record.position = record.len;
        while record.position < position {
            record.put(b' ')?;
        }
    }
    record.end();
    Ok(())
}

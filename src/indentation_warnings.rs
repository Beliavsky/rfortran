// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Windentation`, opt-in: free-form constructs whose layout hides their
//! structure. Three checks share one stack of open constructs:
//!
//! - a body not indented past the statement that opens it (for SELECT, past
//!   its CASE statement);
//! - statements of one body starting in different columns; the branches of
//!   an IF or the cases of a SELECT count as one body;
//! - END, ELSE, ELSE WHERE and CASE statements not aligned with the
//!   statement that opens the construct (CASE may be aligned with SELECT or
//!   indented past it, consistently).
//!
//! The constructs are IF, DO, SELECT, WHERE, FORALL, BLOCK, ASSOCIATE,
//! CRITICAL, CHANGE TEAM, interface blocks, derived type definitions and
//! enumerations. Bodies of program units are not checked, since many styles
//! do not indent them, but constructs inside them are. No indentation width
//! is required. Only the first line of a statement counts, from its first
//! keyword after any label; statements after `;` on a line, statements
//! ending labeled DO loops and fixed-form source are not checked. Tabs
//! advance to the next multiple of 8 columns.
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::ops::Range;

const CATEGORY: &str = "indentation";

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind { If, Do, Select, Where, Forall, Block, Associate, Critical, Team, Interface, Type, Enum, Unit }

impl Kind {
    fn construct(self) -> &'static str {
        match self {
            Kind::If => "IF block",
            Kind::Do => "DO loop",
            Kind::Select => "SELECT construct",
            Kind::Where => "WHERE construct",
            Kind::Forall => "FORALL construct",
            Kind::Block => "BLOCK construct",
            Kind::Associate => "ASSOCIATE construct",
            Kind::Critical => "CRITICAL construct",
            Kind::Team => "CHANGE TEAM construct",
            Kind::Interface => "interface block",
            Kind::Type => "derived type definition",
            Kind::Enum => "enumeration",
            Kind::Unit => "program unit",
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Role {
    /// A construct's first statement, with the label ending a labeled DO.
    Open(Kind, Option<u64>),
    /// ELSE IF and ELSE, or ELSE WHERE: aligned with the opening statement.
    Else(Kind),
    /// CASE, TYPE IS, CLASS IS, CLASS DEFAULT and RANK of a SELECT.
    Case,
    Contains,
    /// An END statement; `None` for a bare END, which ends a program unit.
    Close(Option<Kind>),
    Plain,
}

/// The words of a statement: lowercase names and numbers, `'` for each
/// character literal, and other characters, with `::`, `==` and `=>` kept
/// together. Continuations and comments are skipped.
pub(crate) fn words(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        let c = b[i];
        if c == b'!' {
            while i < b.len() && b[i] != b'\n' { i += 1; }
        } else if c.is_ascii_whitespace() || c == b'&' {
            i += 1;
        } else if c == b'\'' || c == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == c {
                    if b.get(i + 1) == Some(&c) { i += 2; continue; }
                    break;
                }
                i += 1;
            }
            i += 1;
            out.push("'".to_owned());
        } else if c.is_ascii_alphanumeric() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') { i += 1; }
            out.push(text[s..i].to_ascii_lowercase());
        } else if let Some(two @ ("::" | "==" | "=>")) = text.get(i..i + 2) {
            out.push(two.to_owned());
            i += 2;
        } else {
            let ch = text[i..].chars().next().unwrap_or(' ');
            out.push(ch.to_string());
            i += ch.len_utf8().max(1);
        }
    }
    out
}

/// The index of the parenthesis closing the one at `open`.
fn closing(w: &[String], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, x) in w.iter().enumerate().skip(open) {
        match x.as_str() {
            "(" => depth += 1,
            ")" => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}

fn is_name(w: &str) -> bool {
    w.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
}

/// Whether `w` starts a function, subroutine or separate module procedure:
/// FUNCTION or SUBROUTINE after only prefixes and a type specification.
fn procedure(w: &[String]) -> bool {
    const PREFIXES: &[&str] = &["pure", "impure", "elemental", "recursive", "non_recursive", "module", "simple",
        "integer", "real", "double", "precision", "doubleprecision", "complex", "doublecomplex", "logical",
        "character", "type", "class", "*"];
    let mut i = 0;
    while i < w.len() {
        match w[i].as_str() {
            "function" | "subroutine" => return i > 0 || w.len() > 1,
            "(" => i = match closing(w, i) { Some(c) => c + 1, None => return false },
            x if PREFIXES.contains(&x) || x.bytes().all(|b| b.is_ascii_digit()) => i += 1,
            _ => return false,
        }
    }
    false
}

pub(crate) fn classify(w: &[String], top: Option<Kind>) -> Role {
    // A construct name, as in `outer: do`.
    let w = if w.len() > 2 && w[1] == ":" && is_name(&w[0]) { &w[2..] } else { w };
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    // Assignments, pointer assignments and component assignments.
    if matches!(at(1), "=" | "=>" | "%") { return Role::Plain; }
    if top == Some(Kind::Select) {
        match (at(0), at(1)) {
            ("case" | "rank" | "typeis" | "classis" | "classdefault", _) | ("type", "is") | ("class", "is" | "default") =>
                return Role::Case,
            _ => {}
        }
    }
    if let Some(rest) = at(0).strip_prefix("end") {
        let (keyword, next) = if rest.is_empty() { (at(1), at(2)) } else { (rest, at(1)) };
        let kind = match keyword {
            "" => return Role::Close(None),
            "do" => Kind::Do,
            "if" => Kind::If,
            "select" => Kind::Select,
            "where" => Kind::Where,
            "forall" => Kind::Forall,
            "block" if next == "data" => Kind::Unit,
            "block" => Kind::Block,
            "associate" => Kind::Associate,
            "critical" => Kind::Critical,
            "team" => Kind::Team,
            "interface" => Kind::Interface,
            "type" => Kind::Type,
            "enum" | "enumeration" => Kind::Enum,
            "program" | "module" | "submodule" | "function" | "subroutine" | "procedure" | "blockdata" => Kind::Unit,
            // A bare END followed by a name is not valid; END FILE is I/O.
            _ => return Role::Plain,
        };
        return Role::Close(Some(kind));
    }
    if procedure(w) { return Role::Open(Kind::Unit, None); }
    match at(0) {
        "if" if at(1) == "(" => match closing(w, 1) {
            Some(c) if w.len() == c + 2 && at(c + 1) == "then" => Role::Open(Kind::If, None),
            _ => Role::Plain,
        },
        "else" if at(1) == "where" => Role::Else(Kind::Where),
        "else" | "elseif" => Role::Else(Kind::If),
        "elsewhere" => Role::Else(Kind::Where),
        "do" => Role::Open(Kind::Do, at(1).parse().ok()),
        "dowhile" | "doconcurrent" => Role::Open(Kind::Do, None),
        "select" | "selectcase" | "selecttype" | "selectrank" => Role::Open(Kind::Select, None),
        "where" | "forall" if at(1) == "(" => match closing(w, 1) {
            Some(c) if w.len() == c + 1 => Role::Open(if at(0) == "where" { Kind::Where } else { Kind::Forall }, None),
            _ => Role::Plain,
        },
        "block" if w.len() == 1 => Role::Open(Kind::Block, None),
        "block" if at(1) == "data" => Role::Open(Kind::Unit, None),
        "blockdata" | "program" | "submodule" => Role::Open(Kind::Unit, None),
        "associate" if at(1) == "(" => Role::Open(Kind::Associate, None),
        "critical" => Role::Open(Kind::Critical, None),
        "change" if at(1) == "team" => Role::Open(Kind::Team, None),
        "interface" => Role::Open(Kind::Interface, None),
        "abstract" if at(1) == "interface" => Role::Open(Kind::Interface, None),
        "enum" => Role::Open(Kind::Enum, None),
        "enumeration" if at(1) == "type" => Role::Open(Kind::Enum, None),
        // `module procedure` lists in an interface; elsewhere it begins a
        // separate module procedure.
        "module" if at(1) == "procedure" =>
            if top == Some(Kind::Interface) { Role::Plain } else { Role::Open(Kind::Unit, None) },
        "module" if is_name(at(1)) && w.len() == 2 => Role::Open(Kind::Unit, None),
        // A definition, not TYPE(t) declarations or the old TYPE output statement.
        "type" => match at(1) {
            "(" | "is" | "*" | "" => Role::Plain,
            _ => Role::Open(Kind::Type, None),
        },
        "contains" if w.len() == 1 => Role::Contains,
        _ => Role::Plain,
    }
}

/// The 1-based column of byte `at`, with tabs advancing to the next
/// multiple of 8.
fn column(src: &str, at: usize) -> usize {
    let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    let mut col = 0;
    for ch in src[start..at].chars() {
        col = if ch == '\t' { (col / 8 + 1) * 8 } else { col + 1 };
    }
    col + 1
}

/// The leading keywords of a statement, as in `DO` or `SELECT CASE`.
fn keyword(w: &[String]) -> String {
    let w = if w.len() > 2 && w[1] == ":" && is_name(&w[0]) { &w[2..] } else { w };
    let mut out = w.first().cloned().unwrap_or_default();
    if let Some(next) = w.get(1) {
        if matches!((out.as_str(), next.as_str()), ("select" | "else" | "end", _) | ("type" | "class", "is" | "default")
            | ("abstract", "interface") | ("do", "while" | "concurrent")) && is_name(next) {
            out = format!("{out} {next}");
        }
    }
    out.to_ascii_uppercase()
}

struct Frame {
    kind: Kind,
    label: Option<u64>,
    col: usize,
    span: Range<usize>,
    name: String,
    checked: bool,
    /// The statement the body must be indented past: the opening statement,
    /// or the latest CASE.
    anchor: (usize, Range<usize>, String),
    /// The column and first statement of the body.
    body: Option<(usize, Range<usize>)>,
    /// The column and first CASE statement of a SELECT.
    case: Option<(usize, Range<usize>)>,
    body_warned: bool,
    align_warned: bool,
}

#[derive(Debug)]
struct Warning {
    message: String,
    span: Range<usize>,
    label: String,
    related: Range<usize>,
    related_label: String,
}

/// Where a statement is, and how it reads.
struct Line<'a> {
    col: usize,
    span: Range<usize>,
    first: bool,
    text: &'a str,
}

fn child(frame: &mut Frame, s: &Line, out: &mut Vec<Warning>) {
    if !s.first || !frame.checked || frame.body_warned { return; }
    match &frame.body {
        None => {
            if s.col <= frame.anchor.0 {
                frame.body_warned = true;
                out.push(Warning {
                    message: format!("body of the {} is not indented [{CATEGORY}]", frame.kind.construct()),
                    span: s.span.clone(), label: format!("column {}", s.col),
                    related: frame.anchor.1.clone(), related_label: format!("{} statement at column {}", frame.anchor.2, frame.anchor.0),
                });
            }
            frame.body = Some((s.col, s.span.clone()));
        }
        Some((col, first)) if s.col != *col => {
            frame.body_warned = true;
            out.push(Warning {
                message: format!("statement is indented inconsistently with the rest of the {} [{CATEGORY}]", frame.kind.construct()),
                span: s.span.clone(), label: format!("column {}", s.col),
                related: first.clone(), related_label: format!("the body starts at column {col}"),
            });
        }
        _ => {}
    }
}

fn misaligned(frame: &mut Frame, s: &Line, message: String, related_label: String, out: &mut Vec<Warning>) {
    if !s.first || !frame.checked || frame.align_warned { return; }
    frame.align_warned = true;
    out.push(Warning { message, span: s.span.clone(), label: format!("column {}", s.col),
        related: frame.span.clone(), related_label });
}

fn analyze(src: &str, tokens: &[Token], own_len: usize) -> Vec<Warning> {
    let mut out = vec![];
    let mut stack: Vec<Frame> = vec![];
    let mut i = 0;
    while i < tokens.len() && tokens[i].span.start < own_len {
        let ts = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        i += ts.len().max(1);
        let Some(start) = ts.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))) else { continue };
        let label = ts[..start].iter().find_map(|t| src[t.span.clone()].trim().parse::<u64>().ok());
        let (head, last) = (&ts[start], &ts[ts.len() - 1]);
        let line_start = src[..ts[0].span.start].rfind('\n').map_or(0, |p| p + 1);
        let line_end = src[head.span.start..].find('\n').map_or(src.len(), |p| head.span.start + p);
        let text = &src[head.span.start..last.span.end.max(head.span.end)];
        let first_line = src[head.span.start..line_end.min(last.span.end.max(head.span.end))].trim_end();
        let line = Line {
            col: column(src, head.span.start),
            span: head.span.start..head.span.start + first_line.split('!').next().unwrap_or("").trim_end().len().max(1),
            first: src[line_start..ts[0].span.start].trim().is_empty(),
            text: first_line,
        };
        let w = words(text);
        // A statement ending labeled DO loops is neither checked nor a body.
        // Loops sharing a terminating statement all end there.
        if let Some(outermost) = stack.iter().position(|f| label.is_some() && f.kind == Kind::Do && f.label == label) {
            if !matches!(classify(&w, stack.last().map(|f| f.kind)), Role::Close(_)) {
                stack.truncate(outermost);
                continue;
            }
        }
        match classify(&w, stack.last().map(|f| f.kind)) {
            Role::Open(kind, do_label) => {
                if let Some(top) = stack.last_mut() { child(top, &line, &mut out); }
                let name = keyword(&w);
                stack.push(Frame {
                    kind, label: do_label, col: line.col, span: line.span.clone(), name: name.clone(),
                    checked: line.first && kind != Kind::Unit,
                    anchor: (line.col, line.span.clone(), name), body: None, case: None,
                    body_warned: false, align_warned: false,
                });
            }
            Role::Else(kind) if stack.last().is_some_and(|f| f.kind == kind) => {
                let top = stack.last_mut().expect("frame");
                if line.col != top.col {
                    let message = format!("`{}` is not aligned with its {} statement [{CATEGORY}]", line.text, top.name);
                    let related = format!("{} statement at column {}", top.name, top.col);
                    misaligned(top, &line, message, related, &mut out);
                }
            }
            Role::Case if stack.last().is_some_and(|f| f.kind == Kind::Select) => {
                let top = stack.last_mut().expect("frame");
                if line.first && top.checked {
                    match top.case.clone() {
                        _ if line.col < top.col => {
                            let message = format!("`{}` is indented less than its {} statement [{CATEGORY}]", line.text, top.name);
                            let related = format!("{} statement at column {}", top.name, top.col);
                            misaligned(top, &line, message, related, &mut out);
                        }
                        Some((col, first)) if line.col != col => {
                            if !top.align_warned {
                                top.align_warned = true;
                                out.push(Warning {
                                    message: format!("`{}` is indented inconsistently with the other CASE statements [{CATEGORY}]", line.text),
                                    span: line.span.clone(), label: format!("column {}", line.col),
                                    related: first, related_label: format!("the first CASE statement is at column {col}"),
                                });
                            }
                        }
                        None => top.case = Some((line.col, line.span.clone())),
                        _ => {}
                    }
                }
                top.anchor = (line.col, line.span.clone(), keyword(&w));
            }
            Role::Contains if stack.last().is_some_and(|f| f.kind == Kind::Type) => {
                let top = stack.last_mut().expect("frame");
                if line.col < top.col {
                    let message = format!("`contains` is indented less than its TYPE statement [{CATEGORY}]");
                    let related = format!("TYPE statement at column {}", top.col);
                    misaligned(top, &line, message, related, &mut out);
                }
            }
            Role::Close(kind) => {
                let wanted = kind.unwrap_or(Kind::Unit);
                if let Some(at) = stack.iter().rposition(|f| f.kind == wanted) {
                    stack.truncate(at + 1);
                    let mut frame = stack.pop().expect("frame");
                    if line.col != frame.col {
                        let message = format!("`{}` is not aligned with its {} statement [{CATEGORY}]", line.text, frame.name);
                        let related = format!("{} statement at column {}", frame.name, frame.col);
                        misaligned(&mut frame, &line, message, related, &mut out);
                    }
                }
            }
            Role::Contains | Role::Plain | Role::Else(_) | Role::Case => {
                if let Some(top) = stack.last_mut() { child(top, &line, &mut out); }
            }
        }
    }
    out
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
    fixed_form: bool,
) -> Result<()> {
    // Opt-in: -Wall does not enable this advice.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
    if !enabled || fixed_form { return Ok(()); }
    let tokens: Vec<Token> = crate::lexer::lex_source(src);
    let warnings = analyze(src, &tokens, own_len);
    if warnings.is_empty() { return Ok(()); }
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for w in &warnings {
        let (file, at) = locate(w.span.clone());
        let (related_file, related) = locate(w.related.clone());
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(w.message.clone())
            .with_labels(vec![
                Label::primary(file, at).with_message(w.label.clone()),
                Label::secondary(related_file, related).with_message(w.related_label.clone()),
            ])
            .with_notes(vec!["a formatter such as fprettify or findent can reindent the file".to_owned()]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each warning as (line, message without the category).
    fn warnings(src: &str) -> Vec<(usize, String)> {
        let tokens = crate::lexer::lex_source(src);
        super::analyze(src, &tokens, src.len()).into_iter().map(|w| {
            let line = src[..w.span.start].matches('\n').count() + 1;
            (line, w.message.trim_end_matches(" [indentation]").to_owned())
        }).collect()
    }

    #[test]
    fn well_indented_code_is_not_reported() {
        let src = "program p\nimplicit none\ninteger :: i, k\ntype :: t\n  integer :: x\ncontains\n  procedure :: f\nend type\n\
            outer: do i = 1, 3\n   if (i > 1) then\n      k = i\n   else if (i < 0) then\n      k = 0\n   else\n      k = 1\n   end if\n\
            \x20  select case (i)\n   case (1)\n      k = 2\n   case default\n      k = 3\n   end select\n\
            \x20  select case (k)\n      case (1)\n         k = 2\n      case default\n         k = 3\n   end select\n\
            \x20  if (k > 2) k = 0\n   do k = 1, 2; print *, k; end do\nend do outer\n\
            do 10 i = 1, 2\n   k = i\n10 continue\nwhere (k > 0) k = 1\ncontains\nsubroutine s()\nx = 1\nend subroutine\nend program\n";
        assert_eq!(warnings(src), []);
    }

    #[test]
    fn unindented_and_inconsistent_bodies() {
        assert_eq!(warnings("do i = 1, 4\nprint *, i\nend do\n"), [(2, "body of the DO loop is not indented".to_owned())]);
        assert_eq!(warnings("do i = 1, 4\n   print *, 'i', i\n    print *, i**2\n   print *, i\nend do\n"),
            [(3, "statement is indented inconsistently with the rest of the DO loop".to_owned())]);
        // The branches of an IF share one body column.
        assert_eq!(warnings("if (x > 0) then\n   y = 1\nelse\n    y = 2\nend if\n"),
            [(4, "statement is indented inconsistently with the rest of the IF block".to_owned())]);
        assert_eq!(warnings("type :: t\ninteger :: x\nend type\n"), [(2, "body of the derived type definition is not indented".to_owned())]);
        // Statements after CASE are indented past the CASE statement.
        assert_eq!(warnings("select case (i)\n   case (1)\n   k = 1\nend select\n"),
            [(3, "body of the SELECT construct is not indented".to_owned())]);
    }

    #[test]
    fn misaligned_end_else_and_case() {
        assert_eq!(warnings("do i = 1, 4\n   print *, i\n   end do\n"), [(3, "`end do` is not aligned with its DO statement".to_owned())]);
        assert_eq!(warnings("  if (x > 0) then\n     y = 1\n else\n     y = 2\n  end if\n"),
            [(3, "`else` is not aligned with its IF statement".to_owned())]);
        assert_eq!(warnings("select case (i)\n  case (1)\n     k = 1\n   case (2)\n     k = 2\nend select\n"),
            [(4, "`case (2)` is indented inconsistently with the other CASE statements".to_owned())]);
    }
}

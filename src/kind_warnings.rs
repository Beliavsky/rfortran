// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wliteral-kind`, opt-in: a kind given by a number, as in `real(8)`,
//! `integer(kind=4)`, `real*8`, `1.0_8` or `int(n, 8)`. Kind numbers are
//! processor-dependent; a named kind, such as `real64` from ISO_FORTRAN_ENV,
//! is portable. rfort's kinds are sizes in bytes, so the suggested names keep
//! the meaning the program has here.
//!
//! The check reads the tokens of each statement: a type specification at
//! the start of a statement (after any prefixes such as PURE, or IMPLICIT)
//! or before `::`, a literal with a numeric kind suffix, and the KIND
//! argument of an intrinsic conversion.
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};

const CATEGORY: &str = "literal-kind";

#[derive(Clone, Copy, PartialEq)]
enum Type { Real, Integer, Logical }

/// The ISO_FORTRAN_ENV name of a kind number, if it has one.
fn named(t: Type, kind: &str) -> Option<&'static str> {
    Some(match (t, kind) {
        (Type::Real, "4") => "real32",
        (Type::Real, "8") => "real64",
        (Type::Real, "16") => "real128",
        (Type::Integer, "1") => "int8",
        (Type::Integer, "2") => "int16",
        (Type::Integer, "4") => "int32",
        (Type::Integer, "8") => "int64",
        _ => return None,
    })
}

struct Finding {
    span: std::ops::Range<usize>,
    /// The text as written and the suggested replacement, if any.
    written: String,
    replacement: Option<String>,
    name: Option<&'static str>,
}

fn word(t: &Token) -> String {
    match &t.kind {
        TokenKind::Ident(n) => n.clone(),
        k => format!("{k:?}").trim_start_matches("Kw").to_ascii_lowercase(),
    }
}

fn type_of(t: &Token) -> Option<Type> {
    match t.kind {
        TokenKind::KwReal | TokenKind::KwComplex => Some(Type::Real),
        TokenKind::KwInteger => Some(Type::Integer),
        TokenKind::KwLogical => Some(Type::Logical),
        _ => None,
    }
}

/// The index of the parenthesis closing the one at `open`.
fn closing(ts: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(open) {
        match t.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}

/// A plain kind number: an integer literal without a kind suffix.
fn number(t: &Token) -> Option<&str> {
    match &t.kind { TokenKind::Integer(n) if n.bytes().all(|b| b.is_ascii_digit()) => Some(n), _ => None }
}

fn finding(src: &str, ts: &[Token], first: usize, last: usize, t: Type, kind: &str, replace: impl Fn(&str) -> String) -> Finding {
    let name = named(t, kind);
    Finding {
        span: ts[first].span.start..ts[last].span.end,
        written: src[ts[first].span.start..ts[last].span.end].to_owned(),
        replacement: name.map(replace),
        name,
    }
}

fn statement(src: &str, ts: &[Token], out: &mut Vec<Finding>) {
    let prefix = |t: &Token| matches!(word(t).as_str(),
        "pure" | "elemental" | "recursive" | "impure" | "simple" | "non_recursive" | "module" | "implicit")
        || matches!(t.kind, TokenKind::StatementLabel(_));
    let start = ts.iter().position(|t| !prefix(t)).unwrap_or(ts.len());
    let mut i = 0;
    while i < ts.len() {
        let t = &ts[i];
        // Literals with a numeric kind suffix, such as 1.0_8 or 5_8.
        if let TokenKind::Integer(text) | TokenKind::Float(text) = &t.kind {
            if let Some((value, kind)) = text.split_once('_').filter(|(_, k)| k.bytes().all(|b| b.is_ascii_digit())) {
                let ty = if matches!(t.kind, TokenKind::Float(_)) { Type::Real } else { Type::Integer };
                out.push(finding(src, ts, i, i, ty, kind, |n| format!("{value}_{n}")));
            }
        }
        if let Some(ty) = type_of(t) {
            let at_start = i == start;
            // REAL*8 and similar, at the start of a declaration.
            if at_start && matches!(ts.get(i + 1).map(|t| &t.kind), Some(TokenKind::Star)) {
                if let Some(size) = ts.get(i + 2).and_then(number) {
                    // COMPLEX*16 has REAL parts of half the size.
                    let kind = if matches!(t.kind, TokenKind::KwComplex) { size.parse::<u32>().map(|n| (n / 2).to_string()).unwrap_or_default() } else { size.to_owned() };
                    let head = word(t);
                    out.push(finding(src, ts, i, i + 2, ty, &kind, |n| format!("{head}({n})")));
                    i += 3;
                    continue;
                }
            }
            if matches!(ts.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
                if let Some(close) = closing(ts, i + 1) {
                    let inside = &ts[i + 2..close];
                    let declaration = at_start || matches!(ts.get(close + 1).map(|t| &t.kind), Some(TokenKind::DColon));
                    let head = word(t);
                    if declaration {
                        // real(8) or real(kind=8)
                        let kind = match inside {
                            [k] => number(k),
                            [k, eq, n] if word(k) == "kind" && matches!(eq.kind, TokenKind::Eq) => number(n),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            out.push(finding(src, ts, i, close, ty, kind, |n| format!("{head}({n})")));
                        }
                        i = close + 1;
                        continue;
                    }
                }
            }
        }
        // The KIND argument of an intrinsic conversion: int(n, 8), real(x, kind=8).
        let conversion = match word(t).as_str() {
            "real" | "aint" | "anint" => Some((Type::Real, 2)),
            "complex" | "cmplx" => Some((Type::Real, 3)),
            "int" | "nint" | "floor" | "ceiling" => Some((Type::Integer, 2)),
            "logical" => Some((Type::Logical, 2)),
            _ => None,
        };
        if let (Some((ty, position)), Some(TokenKind::LParen)) = (conversion, ts.get(i + 1).map(|t| &t.kind)) {
            if let Some(close) = closing(ts, i + 1) {
                // The arguments, split at commas of depth one.
                let mut args: Vec<(usize, usize)> = vec![];
                let (mut depth, mut begin) = (0, i + 2);
                for (j, a) in ts.iter().enumerate().take(close + 1).skip(i + 1) {
                    match a.kind {
                        TokenKind::LParen | TokenKind::LBracket => depth += 1,
                        TokenKind::RParen | TokenKind::RBracket => {
                            depth -= 1;
                            if depth == 0 { args.push((begin, j)); }
                        }
                        TokenKind::Comma if depth == 1 => { args.push((begin, j)); begin = j + 1; }
                        _ => {}
                    }
                }
                for (n, &(a, b)) in args.iter().enumerate() {
                    let arg = &ts[a..b];
                    let kind = match arg {
                        [k, eq, v] if word(k) == "kind" && matches!(eq.kind, TokenKind::Eq) => number(v).map(|v| (v, a + 2)),
                        [v] if n + 1 == position => number(v).map(|v| (v, a)),
                        _ => None,
                    };
                    if let Some((kind, at)) = kind {
                        // Labelled at the number, quoting the whole call.
                        let mut f = finding(src, ts, at, at, ty, kind, |name| name.to_owned());
                        f.written = src[t.span.start..ts[close].span.end].to_owned();
                        out.push(f);
                    }
                }
            }
        }
        i += 1;
    }
}

fn analyze(src: &str, tokens: &[Token], own_len: usize) -> Vec<Vec<Finding>> {
    let mut statements = vec![];
    let mut i = 0;
    while i < tokens.len() && tokens[i].span.start < own_len {
        let ts = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        let n = ts.len().max(1);
        let mut found = vec![];
        statement(src, ts, &mut found);
        if !found.is_empty() { statements.push(found); }
        i += n;
    }
    statements
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // Opt-in: -Wall does not enable this advice.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
    if !enabled { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    let statements = analyze(src, &tokens, own_len);
    if statements.is_empty() { return Ok(()); }
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for findings in &statements {
        let labels = findings.iter().map(|f| {
            let (file, at) = locate(f.span.clone());
            Label::primary(file, at).with_message(match &f.replacement {
                Some(r) => format!("use {r}"),
                None => "use a named kind".to_owned(),
            })
        }).collect();
        let written: Vec<String> = findings.iter().map(|f| format!("`{}`", f.written)).collect();
        let mut names: Vec<&str> = findings.iter().filter_map(|f| f.name).collect();
        names.sort();
        names.dedup();
        let mut notes = vec![];
        if !names.is_empty() {
            notes.push(format!("the names come from `use iso_fortran_env, only: {}`", names.join(", ")));
        }
        notes.push("a named constant such as `integer, parameter :: dp = kind(1.0d0)` or `selected_real_kind(15, 307)` also works".to_owned());
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("kind number{} {} {} processor-dependent [{CATEGORY}]",
                if findings.len() > 1 { "s in" } else { " in" }, written.join(", "), if findings.len() > 1 { "are" } else { "is" }))
            .with_labels(labels)
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each statement's findings, as (written, replacement).
    fn findings(src: &str) -> Vec<Vec<(String, Option<String>)>> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len()).into_iter()
            .map(|s| s.into_iter().map(|f| (f.written, f.replacement)).collect()).collect()
    }
    fn one(written: &str, replacement: &str) -> Vec<(String, Option<String>)> {
        vec![(written.to_owned(), Some(replacement.to_owned()))]
    }

    #[test]
    fn declarations() {
        assert_eq!(findings("real(8) :: x\nend\n"), [one("real(8)", "real(real64)")]);
        assert_eq!(findings("integer(kind=4), intent(in) :: n\nend\n"), [one("integer(kind=4)", "integer(int32)")]);
        assert_eq!(findings("real*8 x\ncomplex*16 z\nend\n"), [one("real*8", "real(real64)"), one("complex*16", "complex(real64)")]);
        assert_eq!(findings("pure real(8) function f(x)\nreal(8), intent(in) :: x\nf = x\nend function\n"),
                   [one("real(8)", "real(real64)"), one("real(8)", "real(real64)")]);
        // Typed ALLOCATE and the IMPLICIT statement.
        assert_eq!(findings("implicit real(8) (a-h)\nallocate(real(8) :: p)\nend\n"),
                   [one("real(8)", "real(real64)"), one("real(8)", "real(real64)")]);
    }

    #[test]
    fn literals_and_conversions() {
        assert_eq!(findings("x = 1.0_8 + 2_4\nend\n"), [vec![
            ("1.0_8".to_owned(), Some("1.0_real64".to_owned())), ("2_4".to_owned(), Some("2_int32".to_owned()))]]);
        assert_eq!(findings("y = real(n, 8) + int(x, kind=8)\nz = cmplx(a, b, 8)\nend\n"), [
            vec![("real(n, 8)".to_owned(), Some("real64".to_owned())), ("int(x, kind=8)".to_owned(), Some("int64".to_owned()))],
            one("cmplx(a, b, 8)", "real64")]);
    }

    #[test]
    fn named_kinds_and_values_are_not_reported() {
        for src in [
            "integer, parameter :: dp = kind(1.0d0)\nreal(dp) :: x = 1.0_dp\nend\n",
            "double precision :: d\ncharacter(len=8) :: s\ncharacter*8 t\nend\n",
            // A conversion of the value 8, and an array element.
            "x = real(8)\ny = int(8)\nz = a(8)\nend\n",
            "use iso_fortran_env, only: real64\nreal(real64) :: x\nx = real(n, real64)\nend\n",
        ] {
            assert!(findings(src).is_empty(), "{src}: {:?}", findings(src));
        }
    }
}

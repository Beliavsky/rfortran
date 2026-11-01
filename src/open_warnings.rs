// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wimplicit-open`, on by default: a READ or WRITE on a constant unit that
//! no OPEN statement of the program names. rfort then connects the unit to
//! the file fort.N, as gfortran and ifx do, which is a common extension
//! rather than portable Fortran.
//!
//! The check reads tokens. Units are integer literals or named constants with
//! an integer literal value; any OPEN of a unit it cannot resolve (other than
//! NEWUNIT=) leaves the program unchecked.
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::HashMap;

const CATEGORY: &str = "implicit-open";

enum Unit {
    Number(i64),
    /// A unit the scan cannot resolve.
    Unknown,
    /// No external unit: NEWUNIT=, `*`, or an internal file.
    Other,
}

fn statements<'a>(src: &str, tokens: &'a [Token]) -> Vec<&'a [Token]> {
    let mut out = vec![];
    let mut start = 0;
    for i in 1..=tokens.len() {
        if i == tokens.len() || !crate::lexer::same_statement(&src[tokens[i - 1].span.end..tokens[i].span.start]) {
            out.push(&tokens[start..i]);
            start = i;
        }
    }
    out
}

/// Named constants with an integer literal value; a name given two values is
/// left out.
fn constants(src: &str, tokens: &[Token]) -> HashMap<String, Option<i64>> {
    let mut out: HashMap<String, Option<i64>> = HashMap::new();
    for s in statements(src, tokens) {
        if !s.iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "parameter")) { continue; }
        for (i, w) in s.windows(3).enumerate() {
            if let [Token { kind: TokenKind::Ident(n), .. }, Token { kind: TokenKind::Eq, .. }, Token { kind: TokenKind::Integer(v), .. }] = w {
                if !matches!(s.get(i + 3).map(|t| &t.kind), None | Some(TokenKind::Comma | TokenKind::RParen)) { continue; }
                let value = v.parse().ok();
                let entry = out.entry(n.clone()).or_insert(value);
                if *entry != value { *entry = None; }
            }
        }
    }
    out
}

/// The control list items of the statement whose `(` is at `open`.
fn items(tokens: &[Token], open: usize) -> Vec<&[Token]> {
    let mut out = vec![];
    let mut depth = 0;
    let mut start = open + 1;
    for i in open..tokens.len() {
        match tokens[i].kind {
            TokenKind::LParen | TokenKind::ComplexOpen => depth += 1,
            TokenKind::RParen => {
                depth -= 1;
                if depth == 0 { out.push(&tokens[start..i]); return out; }
            }
            TokenKind::Comma if depth == 1 => { out.push(&tokens[start..i]); start = i + 1; }
            _ => {}
        }
    }
    vec![]
}

fn unit(items: &[&[Token]], constants: &HashMap<String, Option<i64>>) -> Unit {
    let keyword = |item: &[Token], name: &str| matches!(item, [Token { kind: TokenKind::Ident(k), .. }, Token { kind: TokenKind::Eq, .. }, ..] if k == name);
    if items.iter().any(|i| keyword(i, "newunit")) { return Unit::Other; }
    let value = match items.iter().find(|i| keyword(i, "unit")) {
        Some(item) => &item[2..],
        None => match items.first() {
            Some(item) if !matches!(item, [Token { kind: TokenKind::Ident(_), .. }, Token { kind: TokenKind::Eq, .. }, ..]) => item,
            _ => return Unit::Unknown,
        },
    };
    match value {
        [Token { kind: TokenKind::Integer(v), .. }] => v.parse().map(Unit::Number).unwrap_or(Unit::Unknown),
        [Token { kind: TokenKind::Ident(n), .. }] => match constants.get(n) {
            Some(Some(v)) => Unit::Number(*v),
            _ => Unit::Unknown,
        },
        [Token { kind: TokenKind::Star | TokenKind::Str(_), .. }] => Unit::Other,
        _ => Unit::Unknown,
    }
}

/// The first READ or WRITE of each unconnected constant unit, with the unit.
fn analyze(src: &str, tokens: &[Token]) -> Vec<(usize, &'static str, i64)> {
    let constants = constants(src, tokens);
    let mut opened = vec![];
    let mut transfers = vec![];
    for i in 0..tokens.len() {
        let statement = match &tokens[i].kind {
            TokenKind::KwWrite => "WRITE",
            TokenKind::KwRead => "READ",
            TokenKind::FileControl(c) if c == "open" => "OPEN",
            _ => continue,
        };
        if !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
        let unit = unit(&items(tokens, i + 1), &constants);
        match (statement, unit) {
            ("OPEN", Unit::Number(n)) => opened.push(n),
            // An OPEN of a unit the scan cannot resolve might connect any unit.
            ("OPEN", Unit::Unknown) => return vec![],
            (_, Unit::Number(n)) => transfers.push((tokens[i].span.start, statement, n)),
            _ => {}
        }
    }
    let mut seen = vec![];
    transfers.into_iter().filter(|(_, _, n)| {
        let eligible = (1..1024).contains(n) && !matches!(n, 5 | 6) && !opened.contains(n) && !seen.contains(n);
        if eligible { seen.push(*n); }
        eligible
    }).collect()
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // On by default: only -Wno-implicit-open or !#allow(implicit-open) turn it off.
    let (mut enabled, error) = crate::unused::selected(CATEGORY, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == CATEGORY).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(CATEGORY) { enabled = true; }
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut failed = false;
    for (pos, statement, n) in analyze(src, &tokens) {
        if pos >= own_len { continue; }
        let span = tokens.iter().find(|t| t.span.start == pos).map(|t| t.span.clone()).unwrap_or(pos..pos);
        let (name, source, span) = match origins.iter().find(|o| o.range.contains(&pos)) {
            Some(o) => (o.name.as_str(), o.source(src), o.span(span)),
            None => (filename, src, span),
        };
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("{statement} on unit {n}, which no OPEN statement connects [{CATEGORY}]"))
            .with_labels(vec![Label::primary((), span)])
            .with_notes(vec![format!("rfort connects it to the file fort.{n}, as gfortran and ifx do; this is a common extension, not portable Fortran")]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &SimpleFile::new(name, source), &d)?;
        failed |= error;
    }
    if failed { bail!("implicit-open warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::analyze;

    fn units(src: &str) -> Vec<i64> {
        analyze(src, &crate::lexer::lex(src)).into_iter().map(|(_, _, n)| n).collect()
    }

    #[test]
    fn constant_units_without_open() {
        assert_eq!(units("write(20,*) 1\nwrite(20,*) 2\nread(unit=21,fmt=*) x\nend\n"), vec![20, 21]);
        assert_eq!(units("integer, parameter :: out = 30\nwrite(out,*) 1\nend\n"), vec![30]);
    }

    #[test]
    fn opened_preconnected_and_unresolved_units() {
        assert!(units("open(20, file='a')\nwrite(20,*) 1\nend\n").is_empty());
        assert!(units("open(unit=20, file='a')\nwrite(unit=20,fmt=*) 1\nend\n").is_empty());
        assert!(units("write(6,*) 1\nwrite(0,*) 2\nread(5,*) x\nwrite(*,*) 3\nwrite(s,*) 4\nend\n").is_empty());
        assert!(units("open(iu, file='a')\nwrite(20,*) 1\nend\n").is_empty());
        assert_eq!(units("open(newunit=iu, file='a')\nwrite(20,*) 1\nend\n"), vec![20]);
    }
}

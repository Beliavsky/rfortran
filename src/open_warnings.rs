// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wimplicit-open`, on by default: a READ or WRITE on a constant unit that
//! no OPEN statement of the program names. rfort then connects the unit to
//! the file fort.N, as gfortran and ifx do, which is a common extension
//! rather than portable Fortran.
//!
//! `-Wfixed-unit`, in -Wextra: an OPEN of a constant unit in a procedure,
//! which may clash with a unit the rest of the program uses. NEWUNIT= picks a
//! free one. A main program owns its units and is exempt.
//!
//! The checks read tokens. Units are integer literals or named constants with
//! an integer literal value; for -Wimplicit-open, any OPEN of a unit it cannot
//! resolve (other than NEWUNIT=) leaves the program unchecked.
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::HashMap;

const CATEGORY: &str = "implicit-open";
const FIXED: &str = "fixed-unit";

enum Unit {
    Number(i64),
    /// A unit the scan cannot resolve.
    Unknown,
    /// No external unit: NEWUNIT=, `*`, or an internal file.
    Other,
}

pub(crate) fn statements<'a>(src: &str, tokens: &'a [Token]) -> Vec<&'a [Token]> {
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
pub(crate) fn constants(src: &str, tokens: &[Token]) -> HashMap<String, Option<i64>> {
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
pub(crate) fn items(tokens: &[Token], open: usize) -> Vec<&[Token]> {
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

/// A program unit: a main program, a module or submodule, BLOCK DATA, or a
/// procedure, with its name.
struct Scope {
    main: bool,
    name: String,
}

/// The innermost program unit of each token, or None outside every unit (the
/// statements of a main program without a PROGRAM statement).
fn scopes(src: &str, tokens: &[Token]) -> (Vec<Scope>, Vec<Option<usize>>) {
    let mut scopes = vec![];
    let mut stack: Vec<usize> = vec![];
    let mut of = vec![None; tokens.len()];
    let mut offset = 0;
    for s in statements(src, tokens) {
        let start = offset;
        offset += s.len();
        // A statement label comes first.
        let s = match s { [Token { kind: TokenKind::Integer(_), .. }, rest @ ..] if !rest.is_empty() => rest, _ => s };
        let start = start + (offset - start - s.len());
        let kind = |i: usize| s.get(i).map(|t| &t.kind);
        let name = |i: usize| match kind(i) { Some(TokenKind::Ident(n)) => n.clone(), _ => String::new() };
        let mut push = |main: bool, name: String, stack: &mut Vec<usize>| { scopes.push(Scope { main, name }); stack.push(scopes.len() - 1); };
        let mut pop = false;
        match kind(0) {
            Some(TokenKind::KwEnd) => pop = matches!(kind(1), None | Some(TokenKind::KwSubroutine | TokenKind::KwFunction | TokenKind::KwProgram | TokenKind::KwModule))
                || matches!(kind(1), Some(TokenKind::Ident(n)) if n == "submodule"),
            Some(TokenKind::BareEnd | TokenKind::KwEndBlockData) => pop = true,
            Some(TokenKind::KwProgram) => push(true, name(1), &mut stack),
            Some(TokenKind::KwBlockData) => push(false, name(1), &mut stack),
            // MODULE PROCEDURE bodies stay in their submodule.
            Some(TokenKind::KwModule) if matches!(kind(1), Some(TokenKind::Ident(_))) && !matches!(kind(1), Some(TokenKind::Ident(n)) if n == "procedure") => push(false, name(1), &mut stack),
            Some(TokenKind::Ident(n)) if n == "submodule" && matches!(kind(1), Some(TokenKind::LParen)) => {
                let close = s.iter().position(|t| matches!(t.kind, TokenKind::RParen)).unwrap_or(0);
                push(false, name(close + 1), &mut stack)
            }
            _ => if let Some(i) = s.iter().position(|t| matches!(t.kind, TokenKind::KwFunction | TokenKind::KwSubroutine)) {
                push(false, name(i + 1), &mut stack)
            },
        }
        for k in start..offset { of[k] = stack.last().copied(); }
        if pop { stack.pop(); }
    }
    (scopes, of)
}

/// OPENs of constant units in procedures and modules: the position, the
/// unit, the program unit's name, and the other program units opening it.
fn fixed_units(src: &str, tokens: &[Token]) -> Vec<(usize, i64, String, Vec<String>)> {
    let constants = constants(src, tokens);
    let (scopes, of) = scopes(src, tokens);
    let mut opens = vec![];
    for i in 0..tokens.len() {
        if !matches!(&tokens[i].kind, TokenKind::FileControl(c) if c == "open") { continue; }
        if !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
        if let Unit::Number(n) = unit(&items(tokens, i + 1), &constants) {
            if !matches!(n, 0 | 5 | 6) { opens.push((tokens[i].span.start, n, of[i])); }
        }
    }
    let label = |scope: Option<usize>| match scope {
        Some(s) if scopes[s].main && !scopes[s].name.is_empty() => format!("the main program `{}`", scopes[s].name),
        Some(s) if !scopes[s].name.is_empty() => format!("`{}`", scopes[s].name),
        Some(s) if scopes[s].main => "the main program".into(),
        None => "the main program".into(),
        Some(_) => "an unnamed program unit".into(),
    };
    opens.iter().filter(|(_, _, scope)| scope.is_some_and(|s| !scopes[s].main)).map(|(pos, n, scope)| {
        let mut others: Vec<String> = opens.iter().filter(|(_, m, other)| m == n && other != scope).map(|(_, _, other)| label(*other)).collect();
        others.dedup();
        (*pos, *n, label(*scope), others)
    }).collect()
}

/// Report warnings at token positions of the own source.
fn emit(src: &str, own_len: usize, filename: &str, origins: &[crate::source_diagnostics::SourceOrigin],
    tokens: &[Token], error: bool, warnings: Vec<(usize, String, Vec<String>)>) -> Result<bool> {
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut failed = false;
    for (pos, message, notes) in warnings {
        if pos >= own_len { continue; }
        let span = tokens.iter().find(|t| t.span.start == pos).map(|t| t.span.clone()).unwrap_or(pos..pos);
        let (name, source, span) = match origins.iter().find(|o| o.range.contains(&pos)) {
            Some(o) => (o.name.as_str(), o.source(src), o.span(span)),
            None => (filename, src, span),
        };
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(vec![Label::primary((), span)])
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &SimpleFile::new(name, source), &d)?;
        failed |= error;
    }
    Ok(failed)
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
    standard: Option<crate::standards::Standard>,
) -> Result<()> {
    // On by default: only -Wno-implicit-open or !#allow(implicit-open) turn it off.
    let (mut enabled, error) = crate::unused::selected(CATEGORY, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == CATEGORY).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(CATEGORY) { enabled = true; }
    let (fixed, fixed_error) = crate::unused::selected(FIXED, false, settings, options);
    if !enabled && !fixed { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let mut failed = vec![];
    if enabled {
        let warnings = analyze(src, &tokens).into_iter().map(|(pos, statement, n)| (pos,
            format!("{statement} on unit {n}, which no OPEN statement connects [{CATEGORY}]"),
            vec![format!("rfort connects it to the file fort.{n}, as gfortran and ifx do; this is a common extension, not portable Fortran")])).collect();
        if emit(src, own_len, filename, origins, &tokens, error, warnings)? { failed.push(CATEGORY); }
    }
    if fixed {
        // NEWUNIT= is Fortran 2008.
        let newunit = standard.is_none_or(|s| s >= "f2008".parse().unwrap());
        let warnings = fixed_units(src, &tokens).into_iter().map(|(pos, n, scope, others)| {
            let mut notes = vec![];
            if !others.is_empty() { notes.push(format!("unit {n} is also opened in {}", others.join(", "))); }
            if newunit { notes.push("open(newunit=u, ...) with an INTEGER variable u connects a unit that is free".into()); }
            (pos, format!("OPEN of fixed unit {n} in {scope}, which may already be in use elsewhere in the program [{FIXED}]"), notes)
        }).collect();
        if emit(src, own_len, filename, origins, &tokens, fixed_error, warnings)? { failed.push(FIXED); }
    }
    if !failed.is_empty() { bail!("{} warnings treated as errors", failed.join(" and ")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{analyze, fixed_units};

    fn fixed(src: &str) -> Vec<(i64, String, Vec<String>)> {
        fixed_units(src, &crate::lexer::lex(src)).into_iter().map(|(_, n, s, o)| (n, s, o)).collect()
    }

    #[test]
    fn fixed_units_in_procedures() {
        let src = "module m\ninteger, parameter :: lu = 21\ncontains\ninteger function f(x)\nreal :: x\nopen(20, file='a')\nf = 1\nend function f\nsubroutine s\nopen(unit=lu, file='b')\nend subroutine\nend module m\nprogram p\nopen(20, file='c')\nopen(6, file='d')\ncontains\nsubroutine t\n10 open(22, file='e')\nend\nend program\n";
        assert_eq!(fixed(src), vec![
            (20, "`f`".into(), vec!["the main program `p`".into()]),
            (21, "`s`".into(), vec![]),
            (22, "`t`".into(), vec![]),
        ]);
    }

    #[test]
    fn main_programs_and_newunit_are_exempt() {
        assert!(fixed("open(20, file='a')\nend\n").is_empty());
        assert!(fixed("program p\nif (.true.) then\nend if\nopen(20, file='a')\nend program p\n").is_empty());
        assert!(fixed("subroutine s\ninteger :: u\nopen(newunit=u, file='a')\nopen(u, file='b')\nend\n").is_empty());
        assert_eq!(fixed("subroutine s\nopen(20, file='a')\nend\nsubroutine r\nopen(20, file='b')\nend\n").len(), 2);
    }

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

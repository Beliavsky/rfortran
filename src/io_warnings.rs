// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Warnings about input/output units, read from tokens:
//!
//! `-Wrecursive-io`, on by default: an input/output statement in a procedure
//! that a function referenced by another input/output statement can reach,
//! directly or through the procedures it calls, on the same external unit.
//! Such recursive input/output is not allowed (F2018 12.12); gfortran stops
//! with "Recursive I/O not allowed". Internal files, other units and child
//! data transfer statements are allowed and not reported.
//!
//! `-Wunit-leak`, in -Wall: an OPEN with NEWUNIT= in a procedure, into a
//! local INTEGER variable that is not SAVE, a dummy argument or the function
//! result, with no CLOSE of the variable in the procedure. The file stays
//! connected to a unit whose number is lost when the procedure returns. A
//! variable used other than as a unit (assigned elsewhere or passed to a
//! procedure) and procedures with internal procedures are not checked.
//!
//! `-Wpreconnected-unit`, in -Wextra: READ from unit 5, or WRITE to unit 6 or
//! 0, as an integer literal or a named constant. Which units are preconnected
//! is processor dependent; `*` and INPUT_UNIT, OUTPUT_UNIT and ERROR_UNIT of
//! ISO_FORTRAN_ENV are portable. A unit the program OPENs is exempt.
//!
//! `-Wfunction-io`, in -Wstyle: a function that executes input/output on an
//! external unit, a side effect that keeps it from being PURE.
use crate::lexer::{Token, TokenKind};
use crate::open_warnings::{constants, items};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

const RECURSIVE: &str = "recursive-io";
const LEAK: &str = "unit-leak";
const PRECONNECTED: &str = "preconnected-unit";
const FUNCTION_IO: &str = "function-io";

/// The unit of a data transfer statement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dest {
    Input,
    Output,
    Error,
    Number(i64),
    Internal,
    Unknown,
}

impl Dest {
    fn external(self) -> bool {
        matches!(self, Dest::Input | Dest::Output | Dest::Error | Dest::Number(_))
    }
    fn describe(self) -> String {
        match self {
            Dest::Input => "standard input".into(),
            Dest::Output => "standard output".into(),
            Dest::Error => "unit 0".into(),
            Dest::Number(n) => format!("unit {n}"),
            Dest::Internal | Dest::Unknown => "a unit".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Main,
    Module,
    Function,
    Subroutine,
    Other,
}

struct Unit {
    kind: Kind,
    name: String,
    params: Vec<String>,
    result: Option<String>,
    contains: bool,
    /// Indices of the statements of the unit, outside internal procedures.
    statements: Vec<usize>,
}

/// A data transfer statement.
struct Io {
    statement: usize,
    /// The token of PRINT, READ or WRITE.
    head: usize,
    keyword: &'static str,
    writes: bool,
    dest: Dest,
    /// The first token of the unit, and the unit's value when it is an
    /// integer literal or a named constant.
    unit_token: Option<usize>,
    value: Option<i64>,
    /// The unit is a single name that is not a known CHARACTER variable.
    named: bool,
}

fn ranges(src: &str, tokens: &[Token]) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut start = 0;
    for i in 1..=tokens.len() {
        if i == tokens.len() || !crate::lexer::same_statement(&src[tokens[i - 1].span.end..tokens[i].span.start]) {
            if start < i { out.push(start..i); }
            start = i;
        }
    }
    out
}

/// The first token after a statement label.
fn first(tokens: &[Token], r: &Range<usize>) -> usize {
    match tokens[r.start].kind {
        TokenKind::Integer(_) | TokenKind::StatementLabel(_) if r.start + 1 < r.end => r.start + 1,
        _ => r.start,
    }
}

/// The first token of the action statement of a one-line IF, or of the
/// statement itself.
fn action(tokens: &[Token], r: &Range<usize>) -> usize {
    let a = first(tokens, r);
    if matches!(tokens[a].kind, TokenKind::KwIf) && matches!(tokens.get(a + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
        let mut depth = 0;
        for j in a + 1..r.end {
            match tokens[j].kind {
                TokenKind::LParen | TokenKind::ComplexOpen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return if j + 1 < r.end && !matches!(tokens[j + 1].kind, TokenKind::KwThen) { j + 1 } else { a };
                    }
                }
                _ => {}
            }
        }
    }
    a
}

fn index_of(tokens: &[Token], slice: &[Token]) -> usize {
    (slice.as_ptr() as usize - tokens.as_ptr() as usize) / std::mem::size_of::<Token>()
}

fn keyword(item: &[Token], name: &str) -> bool {
    matches!(item, [Token { kind: TokenKind::Ident(k), .. }, Token { kind: TokenKind::Eq, .. }, ..] if k == name)
}

/// The tokens of the unit in a control list: UNIT= or the first positional item.
fn unit_item<'a>(controls: &[&'a [Token]]) -> Option<&'a [Token]> {
    if let Some(item) = controls.iter().find(|i| keyword(i, "unit")) {
        return Some(&item[2..]);
    }
    match controls.first() {
        Some(item) if !matches!(item, [Token { kind: TokenKind::Ident(_), .. }, Token { kind: TokenKind::Eq, .. }, ..]) => Some(item),
        _ => None,
    }
}

fn units(src: &str, tokens: &[Token], ranges: &[Range<usize>]) -> Vec<Unit> {
    let unit = |kind, name: String| Unit { kind, name, params: vec![], result: None, contains: false, statements: vec![] };
    let mut units = vec![unit(Kind::Main, String::new())];
    let mut stack: Vec<usize> = vec![];
    let _ = src;
    for (si, r) in ranges.iter().enumerate() {
        let s = &tokens[first(tokens, r)..r.end];
        let kind = |i: usize| s.get(i).map(|t| &t.kind);
        let name = |i: usize| match kind(i) { Some(TokenKind::Ident(n)) => n.clone(), _ => String::new() };
        let mut pop = false;
        let mut pushed = None;
        match kind(0) {
            Some(TokenKind::KwEnd) => {
                pop = matches!(kind(1), None | Some(TokenKind::KwSubroutine | TokenKind::KwFunction | TokenKind::KwProgram | TokenKind::KwModule))
                    || matches!(kind(1), Some(TokenKind::Ident(n)) if n == "submodule")
            }
            Some(TokenKind::BareEnd | TokenKind::KwEndBlockData) => pop = true,
            Some(TokenKind::KwProgram) => pushed = Some(unit(Kind::Main, name(1))),
            Some(TokenKind::KwBlockData) => pushed = Some(unit(Kind::Other, name(1))),
            Some(TokenKind::KwModule) if matches!(kind(1), Some(TokenKind::Ident(n)) if n != "procedure") => pushed = Some(unit(Kind::Module, name(1))),
            Some(TokenKind::Ident(n)) if n == "submodule" && matches!(kind(1), Some(TokenKind::LParen)) => pushed = Some(unit(Kind::Module, String::new())),
            Some(TokenKind::KwContains) => {
                if let Some(&u) = stack.last() { units[u].contains = true; }
            }
            _ => {
                if let Some(i) = s.iter().position(|t| matches!(t.kind, TokenKind::KwFunction | TokenKind::KwSubroutine)) {
                    let function = matches!(s[i].kind, TokenKind::KwFunction);
                    let mut u = unit(if function { Kind::Function } else { Kind::Subroutine }, name(i + 1));
                    if matches!(kind(i + 2), Some(TokenKind::LParen)) {
                        for t in &s[i + 3..] {
                            match &t.kind {
                                TokenKind::RParen => break,
                                TokenKind::Ident(n) => u.params.push(n.clone()),
                                _ => {}
                            }
                        }
                    }
                    if let Some(k) = s.iter().position(|t| matches!(t.kind, TokenKind::KwResult)) {
                        if let Some(TokenKind::Ident(n)) = kind(k + 2) { u.result = Some(n.clone()); }
                    }
                    pushed = Some(u);
                }
            }
        }
        if let Some(u) = pushed {
            units.push(u);
            stack.push(units.len() - 1);
        }
        let owner = stack.last().copied().unwrap_or(0);
        units[owner].statements.push(si);
        if pop { stack.pop(); }
    }
    units
}

/// Names declared CHARACTER in a unit.
fn characters(tokens: &[Token], ranges: &[Range<usize>], unit: &Unit) -> HashSet<String> {
    let mut out = HashSet::new();
    for &si in &unit.statements {
        let r = &ranges[si];
        let a = first(tokens, r);
        if matches!(tokens[a].kind, TokenKind::KwCharacter) {
            for t in &tokens[a..r.end] {
                if let TokenKind::Ident(n) = &t.kind { out.insert(n.clone()); }
            }
        }
    }
    out
}

fn number(value: Option<i64>) -> Dest {
    match value {
        Some(5) => Dest::Input,
        Some(6) => Dest::Output,
        Some(0) => Dest::Error,
        Some(n) => Dest::Number(n),
        None => Dest::Unknown,
    }
}

fn transfer(tokens: &[Token], ranges: &[Range<usize>], si: usize, constants: &HashMap<String, Option<i64>>, characters: &HashSet<String>) -> Option<Io> {
    let r = &ranges[si];
    let head = action(tokens, r);
    let (keyword, writes) = match tokens[head].kind {
        TokenKind::KwPrint | TokenKind::KwPrintEmpty => ("PRINT", true),
        TokenKind::KwRead | TokenKind::KwReadEmpty => ("READ", false),
        TokenKind::KwWrite | TokenKind::KwWriteEmpty => ("WRITE", true),
        _ => return None,
    };
    let mut io = Io { statement: si, head, keyword, writes, dest: if writes { Dest::Output } else { Dest::Input }, unit_token: None, value: None, named: false };
    if keyword == "PRINT" || !matches!(tokens.get(head + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
        return Some(io);
    }
    let controls = items(tokens, head + 1);
    let Some(value) = unit_item(&controls) else {
        io.dest = Dest::Unknown;
        return Some(io);
    };
    if value.is_empty() { io.dest = Dest::Unknown; return Some(io); }
    io.unit_token = Some(index_of(tokens, value));
    io.dest = match value {
        [Token { kind: TokenKind::Star, .. }] => if writes { Dest::Output } else { Dest::Input },
        [Token { kind: TokenKind::Integer(v), .. }] => {
            io.value = v.parse().ok();
            number(io.value)
        }
        [Token { kind: TokenKind::Ident(n), .. }] => match constants.get(n) {
            Some(Some(v)) => {
                io.value = Some(*v);
                number(io.value)
            }
            Some(None) => Dest::Unknown,
            None => match n.as_str() {
                "input_unit" => Dest::Input,
                "output_unit" => Dest::Output,
                "error_unit" => Dest::Error,
                _ if characters.contains(n) => Dest::Internal,
                _ => {
                    io.named = true;
                    Dest::Unknown
                }
            },
        },
        [Token { kind: TokenKind::Str(_), .. }, ..] => Dest::Internal,
        _ => Dest::Unknown,
    };
    Some(io)
}

/// Functions referenced, and subroutines called, by the tokens of a range,
/// with the token of each reference.
fn references(tokens: &[Token], range: Range<usize>, functions: &HashSet<String>, subroutines: &HashSet<String>) -> Vec<(String, usize)> {
    let mut out = vec![];
    for k in range.clone() {
        match &tokens[k].kind {
            TokenKind::KwCall => {
                if let Some(TokenKind::Ident(n)) = tokens.get(k + 1).map(|t| &t.kind) {
                    if subroutines.contains(n) { out.push((n.clone(), k + 1)); }
                }
            }
            TokenKind::Ident(n) if functions.contains(n)
                && matches!(tokens.get(k + 1).map(|t| &t.kind), Some(TokenKind::LParen))
                && (k == 0 || !matches!(tokens[k - 1].kind, TokenKind::Percent | TokenKind::KwCall)) => out.push((n.clone(), k)),
            _ => {}
        }
    }
    out
}

struct Report {
    category: &'static str,
    error: bool,
    at: usize,
    message: String,
    secondary: Vec<(usize, String)>,
    notes: Vec<String>,
}

fn opt_in(settings: &SemaSettings) -> SemaSettings {
    let mut s = settings.clone();
    s.wall = false;
    s
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
    // -Wrecursive-io is on by default: only -Wno-recursive-io or
    // !#allow(recursive-io) turn it off.
    let (mut recursive, recursive_error) = crate::unused::selected(RECURSIVE, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == RECURSIVE).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(RECURSIVE) { recursive = true; }
    let (leak, leak_error) = crate::unused::selected(LEAK, false, settings, options);
    let quiet = opt_in(settings);
    let (preconnected, preconnected_error) = crate::unused::selected(PRECONNECTED, false, &quiet, options);
    let (function_io, function_io_error) = crate::unused::selected(FUNCTION_IO, false, &quiet, options);
    if !(recursive || leak || preconnected || function_io) { return Ok(()); }

    let tokens = crate::lexer::lex(src);
    if tokens.is_empty() { return Ok(()); }
    let ranges = ranges(src, &tokens);
    let units = units(src, &tokens, &ranges);
    let constants = constants(src, &tokens);
    let functions: HashSet<String> = units.iter().filter(|u| u.kind == Kind::Function).map(|u| u.name.clone()).collect();
    let subroutines: HashSet<String> = units.iter().filter(|u| u.kind == Kind::Subroutine).map(|u| u.name.clone()).collect();
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, u) in units.iter().enumerate() {
        if matches!(u.kind, Kind::Function | Kind::Subroutine) { by_name.entry(u.name.as_str()).or_default().push(i); }
    }
    let ios: Vec<Vec<Io>> = units.iter().map(|u| {
        let characters = characters(&tokens, &ranges, u);
        u.statements.iter().filter_map(|&si| transfer(&tokens, &ranges, si, &constants, &characters)).collect()
    }).collect();
    let edges: Vec<HashSet<String>> = units.iter().map(|u| {
        u.statements.iter().flat_map(|&si| references(&tokens, ranges[si].clone(), &functions, &subroutines)).map(|(n, _)| n).collect()
    }).collect();
    let reachable = |start: &str| -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut work: Vec<usize> = by_name.get(start).cloned().unwrap_or_default();
        while let Some(u) = work.pop() {
            if !seen.insert(u) { continue; }
            for n in &edges[u] {
                if let Some(next) = by_name.get(n.as_str()) { work.extend(next.iter().copied()); }
            }
        }
        seen
    };
    let label = |u: &Unit| if u.name.is_empty() { "the main program".to_string() } else { format!("`{}`", u.name) };
    let mut reports = vec![];

    if recursive {
        // One report for each inner statement, labelling every outer reference.
        let mut found: std::collections::BTreeMap<usize, Report> = std::collections::BTreeMap::new();
        let mut seen = HashSet::new();
        for unit_ios in &ios {
            for outer in unit_ios.iter().filter(|io| io.dest.external()) {
                let range = outer.head..ranges[outer.statement].end;
                for (f, at) in references(&tokens, range, &functions, &HashSet::new()) {
                    for u in reachable(&f) {
                        for inner in ios[u].iter().filter(|io| io.dest == outer.dest) {
                            if !seen.insert((inner.head, at)) { continue; }
                            let through = if units[u].name == f { String::new() } else { format!(", which `{f}` calls,") };
                            let report = found.entry(inner.head).or_insert_with(|| Report {
                                category: RECURSIVE,
                                error: recursive_error,
                                at: inner.head,
                                message: format!("{} on {} in {}{through} can run while another input/output statement on the same unit is in progress [{RECURSIVE}]",
                                    inner.keyword, inner.dest.describe(), label(&units[u])),
                                secondary: vec![],
                                notes: vec![
                                    "recursive input/output on an external unit is not allowed; gfortran stops with \"Recursive I/O not allowed\"".into(),
                                    "evaluate the function before the statement, or write to an internal file in the function".into(),
                                ],
                            });
                            report.secondary.push((at, format!("`{f}` is referenced during this {}", outer.keyword)));
                        }
                    }
                }
            }
        }
        reports.extend(found.into_values());
    }

    if leak {
        for u in units.iter().filter(|u| matches!(u.kind, Kind::Function | Kind::Subroutine) && !u.contains) {
            let statement_of = |si: usize| &tokens[first(&tokens, &ranges[si])..ranges[si].end];
            let mentions = |si: usize, v: &str| statement_of(si).iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == v));
            let all_saved = u.statements.iter().any(|&si| matches!(statement_of(si).first().map(|t| &t.kind), Some(TokenKind::KwSaveAll)));
            for &si in &u.statements {
                let head = action(&tokens, &ranges[si]);
                if !matches!(&tokens[head].kind, TokenKind::FileControl(c) if c == "open") { continue; }
                if !matches!(tokens.get(head + 1).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
                let controls = items(&tokens, head + 1);
                let Some(v) = controls.iter().find_map(|i| match i {
                    [Token { kind: TokenKind::Ident(k), .. }, Token { kind: TokenKind::Eq, .. }, Token { kind: TokenKind::Ident(v), .. }] if k == "newunit" => Some(v.clone()),
                    _ => None,
                }) else { continue };
                if all_saved || u.params.contains(&v) || u.name == v || u.result.as_deref() == Some(v.as_str()) { continue; }
                let mut declared = false;
                let mut skip = false;
                for &sj in &u.statements {
                    if !mentions(sj, &v) { continue; }
                    let s = statement_of(sj);
                    let start = action(&tokens, &ranges[sj]);
                    match &tokens[start].kind {
                        TokenKind::KwInteger => {
                            declared = true;
                            // SAVE, or an initialization, which implies SAVE.
                            let saved = s.iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "save"))
                                || s.windows(2).any(|w| matches!(&w[0].kind, TokenKind::Ident(n) if *n == v) && matches!(w[1].kind, TokenKind::Eq));
                            skip |= saved;
                        }
                        TokenKind::FileControl(c) if c == "close" => skip = true,
                        TokenKind::FileControl(_) | TokenKind::FilePosition(_) | TokenKind::KwRead | TokenKind::KwReadEmpty
                        | TokenKind::KwWrite | TokenKind::KwWriteEmpty | TokenKind::KwPrint | TokenKind::KwPrintEmpty => {}
                        // SAVE, COMMON, assignment, a call, or any other use
                        // may keep the unit number.
                        _ => skip = true,
                    }
                }
                if !declared || skip { continue; }
                reports.push(Report {
                    category: LEAK,
                    error: leak_error,
                    at: head,
                    message: format!("unit `{v}` opened with NEWUNIT= is never closed in {}, and its number is lost when it returns [{LEAK}]", label(u)),
                    secondary: vec![],
                    notes: vec![
                        "the file stays connected to a unit nothing can reach, and each call connects another".into(),
                        format!("add close({v}) before the procedure returns, or keep the unit in a SAVE or module variable"),
                    ],
                });
            }
        }
    }

    if preconnected {
        // Units the program OPENs are not relied on as preconnected.
        let mut opened = HashSet::new();
        for (si, r) in ranges.iter().enumerate() {
            let _ = si;
            let head = action(&tokens, r);
            if !matches!(&tokens[head].kind, TokenKind::FileControl(c) if c == "open") { continue; }
            if !matches!(tokens.get(head + 1).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
            let controls = items(&tokens, head + 1);
            match unit_item(&controls) {
                Some([Token { kind: TokenKind::Integer(v), .. }]) => { opened.extend(v.parse::<i64>().ok()); }
                Some([Token { kind: TokenKind::Ident(n), .. }]) => { if let Some(Some(v)) = constants.get(n) { opened.insert(*v); } }
                _ => {}
            }
        }
        let modern = standard.is_none_or(|s| s >= "f2003".parse().unwrap());
        for io in ios.iter().flatten() {
            let (Some(value), Some(at)) = (io.value, io.unit_token) else { continue };
            if opened.contains(&value) { continue; }
            let (stream, portable) = match (value, io.writes) {
                (5, false) => ("standard input", "INPUT_UNIT"),
                (6, true) => ("standard output", "OUTPUT_UNIT"),
                (0, true) if modern => ("standard error", "ERROR_UNIT"),
                _ => continue,
            };
            let direction = if io.writes { "to" } else { "from" };
            let mut notes = vec![];
            let constant = match &tokens[at].kind { TokenKind::Ident(n) => Some(n.clone()), _ => None };
            let star = value != 0;
            notes.push(match (star, modern) {
                (true, true) => format!("use * or {portable} from ISO_FORTRAN_ENV"),
                (true, false) => "use *".into(),
                (false, _) => format!("use {portable} from ISO_FORTRAN_ENV"),
            });
            if let (Some(n), true) = (constant, modern) {
                notes.push(format!("or define `{n}` as {portable} from ISO_FORTRAN_ENV"));
            }
            reports.push(Report {
                category: PRECONNECTED,
                error: preconnected_error,
                at,
                message: format!("{} {direction} unit {value} assumes it is preconnected to {stream}, which the standard does not guarantee [{PRECONNECTED}]", io.keyword),
                secondary: vec![],
                notes,
            });
        }
    }

    if function_io {
        for (u, unit) in units.iter().enumerate().filter(|(_, u)| u.kind == Kind::Function) {
            let Some(io) = ios[u].iter().find(|io| io.dest.external() || io.named) else { continue };
            reports.push(Report {
                category: FUNCTION_IO,
                error: function_io_error,
                at: io.head,
                message: format!("function `{}` performs input/output on an external unit, a side effect [{FUNCTION_IO}]", unit.name),
                secondary: vec![],
                notes: vec!["a function with side effects cannot be PURE, and referencing it in an input/output statement on the same unit is recursive input/output, which is not allowed; consider a subroutine".into()],
            });
        }
    }

    reports.retain(|r| tokens[r.at].span.start < own_len);
    if reports.is_empty() { return Ok(()); }
    reports.sort_by_key(|r| (tokens[r.at].span.start, r.category));
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    let mut failed = BTreeSet::new();
    for r in &reports {
        let (file, at) = locate(tokens[r.at].span.clone());
        let mut labels = vec![Label::primary(file, at)];
        for (k, message) in &r.secondary {
            let (file, at) = locate(tokens[*k].span.clone());
            labels.push(Label::secondary(file, at).with_message(message.clone()));
        }
        let mut notes = r.notes.clone();
        notes.push(format!("-Wno-{} disables this warning", r.category));
        let d = if r.error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(r.message.clone())
            .with_labels(labels)
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
        if r.error { failed.insert(r.category); }
    }
    if !failed.is_empty() {
        bail!("{} warnings treated as errors", failed.into_iter().collect::<Vec<_>>().join(" and "));
    }
    Ok(())
}

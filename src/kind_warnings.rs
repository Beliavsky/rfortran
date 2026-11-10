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
//! argument of an intrinsic conversion. A named constant set to an integer
//! literal, as in `integer, parameter :: wp = 8`, is reported at its
//! declaration when the file uses it as a kind in one of these places.
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};

const CATEGORY: &str = "literal-kind";
/// `-Wdefault-real`, opt-in: REAL and COMPLEX declarations without a kind.
const DEFAULT_REAL: &str = "default-real";
/// `-Wdimension-override`, in -Wextra: an entity's own shape or length that
/// overrides the DIMENSION attribute or type length of its declaration.
const DIMENSION_OVERRIDE: &str = "dimension-override";

/// Entities whose own array shape overrides DIMENSION(...), or whose own
/// length overrides the CHARACTER length, of their declaration.
struct Override {
    entities: Vec<(String, std::ops::Range<usize>)>,
    /// The overridden shape or length, as written.
    overridden: String,
    length: bool,
    suggestion: Option<String>,
}

/// The source text of `tokens`, with whitespace collapsed.
fn joined(src: &str, tokens: &[Token]) -> String {
    match (tokens.first(), tokens.last()) {
        (Some(a), Some(b)) => src[a.span.start..b.span.end].split_whitespace().collect::<Vec<_>>().join(" ").replace("& ", ""),
        _ => String::new(),
    }
}

/// Split tokens at commas outside parentheses and brackets.
fn top_level(tokens: &[Token]) -> Vec<&[Token]> {
    let (mut out, mut depth, mut begin) = (vec![], 0i32, 0);
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { out.push(&tokens[begin..i]); begin = i + 1; }
            _ => {}
        }
    }
    out.push(&tokens[begin..]);
    out
}

fn overrides(src: &str, ts: &[Token], out: &mut Vec<Override>) {
    let start = ts.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))).unwrap_or(ts.len());
    let Some(first) = ts.get(start) else { return };
    let declaration = matches!(first.kind, TokenKind::KwReal | TokenKind::KwInteger | TokenKind::KwComplex | TokenKind::KwLogical
        | TokenKind::KwCharacter | TokenKind::KwDouble | TokenKind::KwType) || word(first) == "class";
    if !declaration { return; }
    let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)) else { return };
    let head = &ts[start..colon];
    let entities = top_level(&ts[colon + 1..]);
    // DIMENSION(...) among the attributes.
    let parts = top_level(head);
    let dimension = parts.iter().enumerate().find(|(_, p)| p.first().is_some_and(|t| word(t) == "dimension")
        && matches!(p.get(1).map(|t| &t.kind), Some(TokenKind::LParen)));
    if let Some((index, part)) = dimension {
        let shape = joined(src, &part[1..]);
        let own: Vec<(String, std::ops::Range<usize>)> = entities.iter().filter_map(|e| {
            let name = e.first().and_then(named_kind)?;
            matches!(e.get(1).map(|t| &t.kind), Some(TokenKind::LParen)).then(|| {
                let close = closing(e, 1).unwrap_or(e.len() - 1);
                (name.to_owned(), e[1].span.start..e[close].span.end)
            })
        }).collect();
        if !own.is_empty() {
            // The statement without DIMENSION, each entity with its shape.
            let attributes: Vec<String> = parts.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, p)| joined(src, p)).collect();
            let items: Vec<String> = entities.iter().map(|e| {
                let text = joined(src, e);
                match e.first().and_then(named_kind) {
                    Some(name) if !matches!(e.get(1).map(|t| &t.kind), Some(TokenKind::LParen) | Some(TokenKind::Star)) =>
                        format!("{name}{shape}{}", &text[name.len()..]),
                    _ => text,
                }
            }).collect();
            out.push(Override { entities: own, overridden: format!("DIMENSION{shape}"), length: false,
                suggestion: Some(format!("{} :: {}", attributes.join(", "), items.join(", "))) });
        }
    }
    // CHARACTER(len=8) or CHARACTER*8, overridden by an entity's *length.
    if matches!(first.kind, TokenKind::KwCharacter) {
        let declared = match head.get(1).map(|t| &t.kind) {
            Some(TokenKind::LParen) => closing(head, 1).map(|c| joined(src, &head[1..=c])),
            Some(TokenKind::Star) => head.get(2).map(|t| format!("*{}", joined(src, std::slice::from_ref(t)))),
            _ => None,
        };
        if let Some(declared) = declared {
            let own: Vec<(String, std::ops::Range<usize>)> = entities.iter().filter_map(|e| {
                let name = e.first().and_then(named_kind)?;
                let star = e.iter().position(|t| matches!(t.kind, TokenKind::Star))?;
                // The star after the name and any shape, not one in an initializer.
                let eq = e.iter().position(|t| matches!(t.kind, TokenKind::Eq | TokenKind::Arrow)).unwrap_or(e.len());
                (star < eq).then(|| (name.to_owned(), e[star].span.start..e.get(star + 1).map_or(e[star].span.end, |t| t.span.end)))
            }).collect();
            if !own.is_empty() {
                out.push(Override { entities: own, overridden: declared, length: true, suggestion: None });
            }
        }
    }
}

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

/// A named constant used as a kind, in a type specification, a literal
/// suffix or a conversion KIND argument.
struct KindUse {
    name: String,
    ty: Type,
    span: std::ops::Range<usize>,
}

/// An INTEGER named constant set to an integer literal.
struct Constant {
    name: String,
    value: String,
    span: std::ops::Range<usize>,
}

/// A REAL or COMPLEX declaration without a kind: the type keyword, the
/// entities declared, and the function whose result it is, if any.
struct DefaultReal {
    span: std::ops::Range<usize>,
    head: String,
    entities: Vec<String>,
    function: Option<String>,
    implicit: bool,
}

/// A REAL or COMPLEX type specification without a kind at the start of a
/// declaration (after prefixes such as PURE, or IMPLICIT), or in a typed
/// ALLOCATE or constructor before `::`.
fn default_real(ts: &[Token], out: &mut Vec<DefaultReal>) {
    let prefix = |t: &Token| matches!(word(t).as_str(),
        "pure" | "elemental" | "recursive" | "impure" | "simple" | "non_recursive" | "module" | "implicit")
        || matches!(t.kind, TokenKind::StatementLabel(_));
    let start = ts.iter().position(|t| !prefix(t)).unwrap_or(ts.len());
    let implicit = ts[..start].iter().any(|t| word(t) == "implicit");
    for (i, t) in ts.iter().enumerate() {
        if !matches!(t.kind, TokenKind::KwReal | TokenKind::KwComplex) { continue; }
        let next = ts.get(i + 1).map(|t| &t.kind);
        // A type followed by `::` later in a statement: typed ALLOCATE or a constructor.
        let typed = i > start && matches!(next, Some(TokenKind::DColon));
        if i != start && !typed { continue; }
        // REAL*4 is a kind number, reported by -Wliteral-kind.
        if matches!(next, Some(TokenKind::Star)) { continue; }
        if matches!(next, Some(TokenKind::LParen)) {
            // In IMPLICIT, a single parenthesized list holds letters, not a kind:
            // IMPLICIT REAL (a-h) has no kind, IMPLICIT REAL(8) (a-h) does.
            let Some(close) = closing(ts, i + 1) else { continue };
            if !(implicit && !matches!(ts.get(close + 1).map(|t| &t.kind), Some(TokenKind::LParen))) { continue; }
        }
        // The entities: after `::`, or after the type and any attributes.
        let mut entities = vec![];
        let mut function = None;
        if let Some(f) = ts.iter().position(|t| matches!(t.kind, TokenKind::KwFunction)) {
            function = ts.get(f + 1).and_then(named_kind).map(str::to_owned);
        } else if !implicit && !typed {
            let begin = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)).map_or(i + 1, |c| c + 1);
            let mut depth = 0i32;
            let mut expect = true;
            for t in &ts[begin..] {
                match t.kind {
                    TokenKind::LParen | TokenKind::LBracket => depth += 1,
                    TokenKind::RParen | TokenKind::RBracket => depth -= 1,
                    TokenKind::Comma if depth == 0 => { expect = true; continue; }
                    _ => {}
                }
                if expect && depth == 0 {
                    if let Some(n) = named_kind(t) { entities.push(n.to_owned()); }
                    expect = false;
                }
            }
        }
        out.push(DefaultReal { span: t.span.clone(), head: word(t), entities, function, implicit });
        break;
    }
}

/// A constant set to a kind number and the kinds it is used as.
struct ConstantReport {
    constant: Constant,
    types: Vec<Type>,
    first_use: std::ops::Range<usize>,
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

/// A named kind: an identifier that may be a named constant.
fn named_kind(t: &Token) -> Option<&str> {
    match &t.kind { TokenKind::Ident(n) => Some(n), _ => None }
}

/// The named constants a statement sets to integer literals:
/// `integer[(kind)], parameter :: a = 8, b = 4` or `parameter (a = 8)`.
fn constants(ts: &[Token], out: &mut Vec<Constant>) {
    let start = ts.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))).unwrap_or(ts.len());
    let Some(first) = ts.get(start) else { return };
    let entities: &[Token] = if matches!(first.kind, TokenKind::KwInteger) {
        let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)) else { return };
        if !ts[..colon].iter().any(|t| word(t) == "parameter") { return; }
        &ts[colon + 1..]
    } else if word(first) == "parameter" && matches!(ts.get(start + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
        let Some(close) = closing(ts, start + 1) else { return };
        &ts[start + 2..close]
    } else {
        return;
    };
    let mut depth = 0i32;
    let mut begin = 0;
    for (i, t) in entities.iter().enumerate().chain(std::iter::once((entities.len(), &entities[0]))) {
        let end = i == entities.len();
        if !end {
            match t.kind {
                TokenKind::LParen | TokenKind::LBracket => depth += 1,
                TokenKind::RParen | TokenKind::RBracket => depth -= 1,
                _ => {}
            }
        }
        if end || depth == 0 && matches!(t.kind, TokenKind::Comma) {
            if let [name, eq, value] = &entities[begin..i] {
                if let (Some(n), TokenKind::Eq, Some(v)) = (named_kind(name), &eq.kind, number(value)) {
                    out.push(Constant { name: n.to_owned(), value: v.to_owned(), span: value.span.clone() });
                }
            }
            begin = i + 1;
        }
    }
}

fn statement(src: &str, ts: &[Token], out: &mut Vec<Finding>, uses: &mut Vec<KindUse>) {
    let prefix = |t: &Token| matches!(word(t).as_str(),
        "pure" | "elemental" | "recursive" | "impure" | "simple" | "non_recursive" | "module" | "implicit")
        || matches!(t.kind, TokenKind::StatementLabel(_));
    let start = ts.iter().position(|t| !prefix(t)).unwrap_or(ts.len());
    let mut i = 0;
    while i < ts.len() {
        let t = &ts[i];
        // Literals with a numeric kind suffix, such as 1.0_8 or 5_8.
        if let TokenKind::Integer(text) | TokenKind::Float(text) = &t.kind {
            let ty = if matches!(t.kind, TokenKind::Float(_)) { Type::Real } else { Type::Integer };
            if let Some((value, kind)) = text.split_once('_').filter(|(_, k)| k.bytes().all(|b| b.is_ascii_digit())) {
                out.push(finding(src, ts, i, i, ty, kind, |n| format!("{value}_{n}")));
            } else if let Some((_, kind)) = text.split_once('_') {
                uses.push(KindUse { name: kind.to_owned(), ty, span: t.span.clone() });
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
                        let named = match inside {
                            [k] => named_kind(k),
                            [k, eq, n] if word(k) == "kind" && matches!(eq.kind, TokenKind::Eq) => named_kind(n),
                            _ => None,
                        };
                        if let Some(name) = named {
                            uses.push(KindUse { name: name.to_owned(), ty, span: t.span.start..ts[close].span.end });
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
                    let named = match arg {
                        [k, eq, v] if word(k) == "kind" && matches!(eq.kind, TokenKind::Eq) => named_kind(v),
                        [v] if n + 1 == position => named_kind(v),
                        _ => None,
                    };
                    if let Some(name) = named {
                        uses.push(KindUse { name: name.to_owned(), ty, span: t.span.start..ts[close].span.end });
                    }
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

/// What the check finds: kind numbers by statement, constants set to kind
/// numbers, declarations of default REAL, and the REAL kind constant used
/// most often, for suggestions.
struct Analysis {
    overrides: Vec<Override>,
    statements: Vec<Vec<Finding>>,
    constants: Vec<ConstantReport>,
    defaults: Vec<DefaultReal>,
    real_kind: Option<String>,
}

fn analyze(src: &str, tokens: &[Token], own_len: usize) -> Analysis {
    let mut statements = vec![];
    let mut defaults = vec![];
    let mut overridden = vec![];
    let (mut uses, mut defined) = (vec![], vec![]);
    let mut i = 0;
    while i < tokens.len() {
        let ts = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        let n = ts.len().max(1);
        // Kind uses anywhere count, including in sources included after this file.
        let mut found = vec![];
        statement(src, ts, &mut found, &mut uses);
        if tokens[i].span.start < own_len {
            if !found.is_empty() { statements.push(found); }
            constants(ts, &mut defined);
            default_real(ts, &mut defaults);
            overrides(src, ts, &mut overridden);
        }
        i += n;
    }
    // A constant defined more than once may be different entities; skip it.
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in &defined { *counts.entry(c.name.as_str()).or_default() += 1; }
    let single: std::collections::HashSet<String> = counts.into_iter().filter(|(_, n)| *n == 1).map(|(c, _)| c.to_owned()).collect();
    let mut reports = vec![];
    for constant in defined.into_iter().filter(|c| single.contains(&c.name)) {
        let used: Vec<&KindUse> = uses.iter().filter(|u| u.name == constant.name).collect();
        let Some(first) = used.first() else { continue };
        let mut types: Vec<Type> = vec![];
        for u in &used { if !types.contains(&u.ty) { types.push(u.ty); } }
        let first_use = first.span.clone();
        reports.push(ConstantReport { constant, types, first_use });
    }
    // The REAL kind constant used most often, preferring the first on ties.
    let mut counts: Vec<(String, usize)> = vec![];
    for u in uses.iter().filter(|u| u.ty == Type::Real && u.name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
        match counts.iter_mut().find(|(n, _)| *n == u.name) {
            Some((_, c)) => *c += 1,
            None => counts.push((u.name.clone(), 1)),
        }
    }
    let most = counts.iter().map(|(_, c)| *c).max();
    let real_kind = counts.into_iter().find(|(_, c)| Some(*c) == most).map(|(n, _)| n);
    Analysis { overrides: overridden, statements, constants: reports, defaults, real_kind }
}

fn type_name(t: Type) -> &'static str {
    match t { Type::Real => "REAL", Type::Integer => "INTEGER", Type::Logical => "LOGICAL" }
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
    let (defaults_enabled, defaults_error) = crate::unused::selected(DEFAULT_REAL, false, &opt_in, options);
    // In -Wextra, not -Wall.
    let (overrides_enabled, overrides_error) = crate::unused::selected(DIMENSION_OVERRIDE, false, &opt_in, options);
    if !enabled && !defaults_enabled && !overrides_enabled { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    let Analysis { overrides, mut statements, constants: mut reports, defaults, real_kind } = analyze(src, &tokens, own_len);
    if !enabled { statements.clear(); reports.clear(); }
    if statements.is_empty() && reports.is_empty() && !(defaults_enabled && !defaults.is_empty())
        && !(overrides_enabled && !overrides.is_empty()) { return Ok(()); }
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
    for r in &reports {
        let c = &r.constant;
        let kinds: Vec<&str> = r.types.iter().map(|t| type_name(*t)).collect();
        let (file, at) = locate(c.span.clone());
        let mut labels = vec![Label::primary(file, at).with_message("kind number")];
        if r.first_use.start != c.span.start {
            let (file, at) = locate(r.first_use.clone());
            labels.push(Label::secondary(file, at).with_message("used as a kind here"));
        }
        let mut notes = vec![];
        match r.types.as_slice() {
            [t] => match named(*t, &c.value) {
                Some(n) => {
                    notes.push(format!("suggestion: integer, parameter :: {} = {n}", c.name));
                    notes.push(format!("the name comes from `use iso_fortran_env, only: {n}`"));
                }
                None => notes.push("ISO_FORTRAN_ENV has no name for this kind number; use a named kind".to_owned()),
            },
            _ => notes.push(format!("define a separate constant for each type, such as {}",
                r.types.iter().filter_map(|t| named(*t, &c.value).map(|n| format!("`{n}`"))).collect::<Vec<_>>().join(" and "))),
        }
        if r.types.contains(&Type::Real) {
            notes.push("`kind(1.0d0)` or `selected_real_kind(15, 307)` also work".to_owned());
        }
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let uses: Vec<String> = kinds.iter().map(|k| format!("{} {k} kind", if *k == "INTEGER" { "an" } else { "a" })).collect();
        let message = format!("`{}` is used as {} but is set to the kind number {}, which is processor-dependent [{CATEGORY}]",
            c.name, uses.join(" and "), c.value);
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(labels)
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if overrides_enabled {
        for o in &overrides {
            let names: Vec<String> = o.entities.iter().map(|(n, _)| format!("`{n}`")).collect();
            let (what, own) = if o.length { ("length", "lengths") } else { ("shape", "shapes") };
            let message = if names.len() == 1 {
                format!("{} is declared with its own {what}, overriding {} of this statement [{DIMENSION_OVERRIDE}]", names[0], o.overridden)
            } else {
                format!("{} are declared with their own {own}, overriding {} of this statement [{DIMENSION_OVERRIDE}]", names.join(", "), o.overridden)
            };
            let labels = o.entities.iter().map(|(_, span)| {
                let (file, at) = locate(span.clone());
                Label::primary(file, at).with_message(format!("its own {what}"))
            }).collect();
            let mut notes = vec![];
            match &o.suggestion {
                Some(text) => notes.push(format!("suggestion: {text}")),
                None => notes.push("declare it in a separate statement, or give every entity its own length".to_owned()),
            }
            notes.push(format!("-Wno-{DIMENSION_OVERRIDE} disables this warning"));
            let diagnostic = if overrides_error { Diagnostic::error() } else { Diagnostic::warning() }
                .with_message(message).with_labels(labels).with_notes(notes);
            emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
        }
        if overrides_error && !overrides.is_empty() { bail!("{DIMENSION_OVERRIDE} warnings treated as errors"); }
    }
    if defaults_enabled {
        for d in &defaults {
            let (file, at) = locate(d.span.clone());
            let kind = real_kind.clone().unwrap_or_else(|| "real32".to_owned());
            let what = match (&d.function, d.implicit) {
                (Some(f), _) => format!("function `{f}` has a default {} result", d.head.to_ascii_uppercase()),
                (None, true) => format!("IMPLICIT gives these letters the default {} kind", d.head.to_ascii_uppercase()),
                (None, false) if d.entities.is_empty() => format!("default {} type", d.head.to_ascii_uppercase()),
                (None, false) => format!("{} declared as default {}",
                    d.entities.iter().map(|e| format!("`{e}`")).collect::<Vec<_>>().join(", "), d.head.to_ascii_uppercase()),
            };
            let mut notes = vec![];
            match &real_kind {
                Some(k) => notes.push(format!("`{k}` is the REAL kind this file uses; make sure it is accessible here")),
                None => {
                    notes.push("`real32` from `use iso_fortran_env, only: real32` keeps the current precision; use `real64` if double precision was intended".to_owned());
                }
            }
            notes.push("changing the kind changes results; literals such as 0.1 also need the kind, which -Wreal-literal-kind finds".to_owned());
            notes.push(format!("-Wno-{DEFAULT_REAL} disables this warning"));
            let diagnostic = if defaults_error { Diagnostic::error() } else { Diagnostic::warning() }
                .with_message(format!("{what} [{DEFAULT_REAL}]"))
                .with_labels(vec![Label::primary(file, at).with_message(format!("use {}({kind})", d.head))])
                .with_notes(notes);
            emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
        }
        if defaults_error && !defaults.is_empty() { bail!("{DEFAULT_REAL} warnings treated as errors"); }
    }
    if error && (!statements.is_empty() || !reports.is_empty()) { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each statement's findings, as (written, replacement).
    fn findings(src: &str) -> Vec<Vec<(String, Option<String>)>> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len()).statements.into_iter()
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

    /// Each constant reported, its value and the kinds it is used as.
    fn constants(src: &str) -> Vec<(String, String, Vec<&'static str>)> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len()).constants.into_iter()
            .map(|r| (r.constant.name, r.constant.value, r.types.into_iter().map(super::type_name).collect())).collect()
    }

    #[test]
    fn constants_set_to_kind_numbers() {
        // The xdp.f90 example; a suffix, a conversion and the PARAMETER statement.
        assert_eq!(constants("implicit none\ninteger, parameter :: wp = 8\nreal(wp) :: x, y\nend\n"),
                   [("wp".into(), "8".into(), vec!["REAL"])]);
        assert_eq!(constants("integer, parameter :: ik = 8, n = 8\ninteger :: big\nparameter (dp = 8)\nbig = 5_ik + int(x, kind=ik)\ny = 1.0_dp\nend\n"),
                   [("ik".into(), "8".into(), vec!["INTEGER"]), ("dp".into(), "8".into(), vec!["REAL"])]);
        // One constant for both REAL and INTEGER kinds.
        assert_eq!(constants("integer, parameter :: k = 4\nreal(k) :: x\ninteger(kind=k) :: i\nend\n"),
                   [("k".into(), "4".into(), vec!["REAL", "INTEGER"])]);
        // Portable values, and a constant never used as a kind.
        assert!(constants("integer, parameter :: dp = kind(1.0d0), n = 8\nreal(dp) :: x(n)\nend\n").is_empty());
        assert!(constants("use iso_fortran_env, only: real64\ninteger, parameter :: wp = real64\nreal(wp) :: x\nend\n").is_empty());
    }

    /// Each default REAL declaration: its entities or function, and the kind suggested.
    fn defaults(src: &str) -> (Vec<(Vec<String>, Option<String>)>, Option<String>) {
        let tokens = crate::lexer::lex(src);
        let a = super::analyze(src, &tokens, src.len());
        (a.defaults.into_iter().map(|d| (d.entities, d.function)).collect(), a.real_kind)
    }

    #[test]
    fn default_real_declarations() {
        let (found, kind) = defaults("implicit none\nreal :: x, y(3) = 0, z\ncomplex c\nreal, intent(in) :: a\nreal function f(t)\nend function\nend\n");
        assert_eq!(found, [(vec!["x".to_owned(), "y".to_owned(), "z".to_owned()], None), (vec!["c".to_owned()], None),
            (vec!["a".to_owned()], None), (vec![], Some("f".to_owned()))]);
        assert_eq!(kind, None);
        // IMPLICIT REAL (a-h) has no kind; a typed ALLOCATE does not either.
        assert_eq!(defaults("implicit real (a-h)\nallocate(real :: p)\nend\n").0.len(), 2);
        // Kinds, star sizes, DOUBLE PRECISION and conversions are not reported, and
        // the REAL kind used most often is suggested.
        let (found, kind) = defaults("integer, parameter :: dp = kind(1d0)\nreal(dp) :: x\nreal(kind=dp) :: y\nreal*8 :: z\ndouble precision :: d\nimplicit real(8) (o-z)\nx = real(3)\nend\n");
        assert!(found.is_empty(), "{found:?}");
        assert_eq!(kind.as_deref(), Some("dp"));
    }

    /// Each override: the entities, what they override, and the suggestion.
    fn overridden(src: &str) -> Vec<(Vec<String>, String, Option<String>)> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len()).overrides.into_iter()
            .map(|o| (o.entities.into_iter().map(|(n, _)| n).collect(), o.overridden, o.suggestion)).collect()
    }

    #[test]
    fn dimension_and_length_overrides() {
        assert_eq!(overridden("real, dimension(2) :: x, y(3)\nend\n"),
            [(vec!["y".to_owned()], "DIMENSION(2)".to_owned(), Some("real :: x(2), y(3)".to_owned()))]);
        assert_eq!(overridden("integer, dimension(2), save :: a = 0, b(4) = 1, c(5)\nend\n"),
            [(vec!["b".to_owned(), "c".to_owned()], "DIMENSION(2)".to_owned(), Some("integer, save :: a(2) = 0, b(4) = 1, c(5)".to_owned()))]);
        assert_eq!(overridden("character(len=8) :: s, t*20\ncharacter*4 :: u*2, v\nend\n"),
            [(vec!["t".to_owned()], "(len=8)".to_owned(), None), (vec!["u".to_owned()], "*4".to_owned(), None)]);
        // No override: every entity uses the attribute, or there is no DIMENSION.
        assert!(overridden("real, dimension(2) :: x, y\nreal :: a(2), b(3)\ncharacter(len=8) :: s = 'a*b'\nend\n").is_empty());
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

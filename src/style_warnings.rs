// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Style warnings after Clerman and Spector, "Modern Fortran: Style and
//! Usage". All are opt-in, and `-Wstyle` enables them together with the
//! other style checks; `logical-comparison` is also in -Wextra.
//!
//! - `old-relational`: `.lt.`, `.le.`, `.eq.`, `.ne.`, `.gt.`, `.ge.`.
//! - `logical-comparison`: comparisons with `.true.` or `.false.`.
//! - `missing-double-colon`: type declarations without `::`.
//! - `labeled-do`: DO loops ended by a labeled statement.
//! - `double-precision`: DOUBLE PRECISION and DOUBLE COMPLEX.
//! - `bare-end`: END statements of program units and derived types
//!   without the name.
//! - `keyword-case`: keywords written in more than one case in a file.
//! - `tabs` and `line-length` (132 characters, or N with
//!   `-Wline-length=N`): free-form source layout.
//! - `unnamed-exit`: EXIT and CYCLE without a construct name.
//! - `goto`: GO TO statements.
//! - `obsolescent`: obsolescent and deleted features, such as computed
//!   GO TO, arithmetic IF, COMMON, EQUIVALENCE, ENTRY and alternate returns.
//! - `stop-in-procedure`: STOP in a function or subroutine.
//! - `assumed-size`: assumed-size dummy arrays.
//!
//! The checks read the tokens of each statement and a stack of open
//! constructs shared with -Windentation.
use crate::indentation_warnings::{classify, words, Kind, Role};
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::ops::Range;

pub(crate) const CATEGORIES: &[&str] = &[
    "old-relational", "logical-comparison", "missing-double-colon", "labeled-do", "double-precision", "bare-end",
    "keyword-case", "tabs", "line-length", "unnamed-exit", "goto", "obsolescent", "stop-in-procedure", "assumed-size",
];

struct Warning {
    category: &'static str,
    message: String,
    labels: Vec<(Range<usize>, String)>,
    related: Vec<(Range<usize>, String)>,
    notes: Vec<String>,
}

impl Warning {
    fn new(category: &'static str, message: String, span: Range<usize>, label: impl Into<String>) -> Self {
        Warning { category, message, labels: vec![(span, label.into())], related: vec![], notes: vec![] }
    }
    fn note(mut self, note: impl Into<String>) -> Self { self.notes.push(note.into()); self }
}

/// A program unit or derived type: what its END statement should name.
struct Unit {
    kind: String,
    name: String,
    args: Vec<String>,
    bind: bool,
}

struct Frame {
    kind: Kind,
    label: Option<u64>,
    span: Range<usize>,
    /// The construct name of a DO loop.
    name: Option<String>,
    unit: Option<Unit>,
    /// EXIT and CYCLE statements without a construct name.
    bare: Vec<(Range<usize>, String)>,
}

fn text<'a>(src: &'a str, t: &Token) -> &'a str { &src[t.span.clone()] }

/// The index of the token closing the parenthesis at `open`.
fn closing(ts: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(open) {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}

/// The index of the word closing the parenthesis at `open`.
fn closing_word(w: &[String], open: usize) -> Option<usize> {
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

/// Split tokens at commas outside parentheses and brackets.
fn top_level(ts: &[Token]) -> Vec<&[Token]> {
    let (mut out, mut depth, mut begin) = (vec![], 0i32, 0);
    for (i, t) in ts.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { out.push(&ts[begin..i]); begin = i + 1; }
            _ => {}
        }
    }
    out.push(&ts[begin..]);
    out
}

/// The words of a logical IF's action, or all the words.
fn action(w: &[String]) -> &[String] {
    if w.first().is_some_and(|x| x == "if") && w.get(1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, 1) {
            if w.get(c + 1).is_some_and(|x| x != "then") { return &w[c + 1..]; }
        }
    }
    w
}

/// Whether `w` is a type declaration statement.
fn declaration(w: &[String]) -> bool {
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let typed = match at(0) {
        "integer" | "real" | "complex" | "logical" | "character" | "doubleprecision" | "doublecomplex" => true,
        "double" => matches!(at(1), "precision" | "complex"),
        "type" | "class" => at(1) == "(",
        _ => false,
    };
    typed && !matches!(at(1), "=" | "=>" | "%") && !matches!(classify(w, None), Role::Open(..))
}

/// The index of the first token after the type specification at the start
/// of `ts`, as in `real(dp)`, `character*8` or `double precision`.
fn spec_end(ts: &[Token]) -> usize {
    let mut i = if matches!(ts[0].kind, TokenKind::KwDouble) { 2 } else { 1 };
    match ts.get(i).map(|t| &t.kind) {
        Some(TokenKind::LParen) => i = closing(ts, i).map_or(ts.len(), |c| c + 1),
        Some(TokenKind::Star) => {
            i += 1;
            if matches!(ts.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
                i = closing(ts, i).map_or(ts.len(), |c| c + 1);
            } else {
                i += 1;
            }
        }
        _ => {}
    }
    i.min(ts.len())
}

/// The `*` ending the bounds in parentheses at `open`, for an assumed size.
fn assumed_size(ts: &[Token], open: usize) -> Option<Range<usize>> {
    let close = closing(ts, open)?;
    let last = *top_level(&ts[open + 1..close]).last()?;
    let star = last.last().filter(|t| matches!(t.kind, TokenKind::Star))?;
    let n = last.len();
    (n == 1 || matches!(last[n - 2].kind, TokenKind::Colon)).then(|| star.span.clone())
}

/// The unit opened by `w`: its kind, name, dummy arguments and BIND(C).
fn unit(w: &[String], kind: Kind) -> Option<Unit> {
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let w = if w.len() > 2 && w[1] == ":" { &w[2..] } else { w };
    let bind = w.iter().any(|x| x == "bind");
    if kind == Kind::Type {
        let name = match w.iter().position(|x| x == "::") { Some(p) => w.get(p + 1)?, None => w.get(1)? };
        return Some(Unit { kind: "type".into(), name: name.clone(), args: vec![], bind });
    }
    let (kind, at_name) = if let Some(p) = w.iter().position(|x| matches!(x.as_str(), "function" | "subroutine")) {
        (w[p].clone(), p + 1)
    } else {
        match (at(0), at(1)) {
            ("module", "procedure") => ("procedure".to_owned(), 2),
            ("program" | "module", _) => (w[0].clone(), 1),
            ("submodule", _) => ("submodule".to_owned(), closing_word(w, 1)? + 1),
            ("block", "data") => ("block data".to_owned(), 2),
            ("blockdata", _) => ("block data".to_owned(), 1),
            _ => return None,
        }
    };
    let name = w.get(at_name).filter(|x| x.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()))?.clone();
    let mut args = vec![];
    if w.get(at_name + 1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, at_name + 1) {
            args = w[at_name + 2..c].iter().filter(|x| x.as_str() != ",").cloned().collect();
        }
    }
    Some(Unit { kind, name, args, bind })
}

fn construct_name(w: &[String]) -> Option<String> {
    (w.len() > 2 && w[1] == ":").then(|| w[0].clone())
}

/// Report the EXIT and CYCLE statements without a construct name of a
/// closed DO loop.
fn unnamed(frame: &Frame, out: &mut Vec<Warning>) {
    if frame.kind != Kind::Do || frame.bare.is_empty() { return; }
    let mut kinds: Vec<&str> = frame.bare.iter().map(|(_, k)| k.as_str()).collect();
    kinds.sort();
    kinds.dedup();
    let what = kinds.iter().map(|k| k.to_ascii_uppercase()).collect::<Vec<_>>().join(" and ");
    let mut w = Warning {
        category: "unnamed-exit",
        message: format!("{what} without a construct name in this DO loop [unnamed-exit]"),
        labels: frame.bare.iter().map(|(s, k)| (s.clone(), format!("`{k}` applies to the innermost DO loop"))).collect(),
        related: vec![(frame.span.clone(), "this loop".to_owned())],
        notes: vec![],
    };
    let k = kinds[0];
    w.notes.push(match &frame.name {
        Some(n) => format!("write `{k} {n}` so that the loop is named where control leaves it"),
        None => format!("name the loop, as in `name: do`, and write `{k} name`; a named EXIT or CYCLE keeps its meaning if loops are added around it"),
    });
    out.push(w);
}

/// The source of a statement's first line, from `start`.
fn first_line(src: &str, start: usize, end: usize) -> Range<usize> {
    let line_end = src[start..].find('\n').map_or(src.len(), |p| start + p).min(end);
    let code = src[start..line_end].trim_end_matches(['\r', ' ', '\t']);
    start..start + code.len().max(1)
}

/// The code of a line before any comment, outside character literals.
fn code_part(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '!') => return &line[..i],
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if q == c => quote = None,
            _ => {}
        }
    }
    line
}

fn relational(kind: &TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::EqEq => "==",
        TokenKind::Ne => "/=",
        TokenKind::Lt => "<",
        TokenKind::Le => "<=",
        TokenKind::Gt => ">",
        TokenKind::Ge => ">=",
        _ => return None,
    })
}

fn statement_checks(src: &str, ts: &[Token], w: &[String], span: Range<usize>, stack: &mut [Frame], out: &mut Vec<Warning>) {
    let a = action(w);
    let at = |i: usize| a.get(i).map(String::as_str).unwrap_or("");
    let find = |word: &str| ts.iter().rev().find(|t| text(src, t).replace([' ', '\t'], "").eq_ignore_ascii_case(word)).map(|t| t.span.clone());
    // Old relational operators, with the statement rewritten.
    let old: Vec<(&Token, &str)> = ts.iter().filter_map(|t| relational(&t.kind).filter(|_| text(src, t).starts_with('.')).map(|r| (t, r))).collect();
    if !old.is_empty() {
        let message = if old.len() == 1 {
            format!("`{}` is the old spelling of `{}` [old-relational]", text(src, old[0].0).to_ascii_lowercase(), old[0].1)
        } else {
            "old spellings of relational operators [old-relational]".to_owned()
        };
        let mut warning = Warning { category: "old-relational", message,
            labels: old.iter().map(|(t, r)| (t.span.clone(), format!("use {r}"))).collect(), related: vec![], notes: vec![] };
        let end = ts.last().map_or(span.end, |t| t.span.end);
        if !src[span.start..end].contains('\n') {
            let mut rewritten = String::new();
            let mut from = span.start;
            for (t, r) in &old {
                rewritten.push_str(&src[from..t.span.start]);
                rewritten.push_str(r);
                from = t.span.end;
            }
            rewritten.push_str(&src[from..end]);
            warning = warning.note(format!("suggestion: {rewritten}"));
        }
        out.push(warning);
    }
    // Comparisons with a LOGICAL literal.
    for (i, t) in ts.iter().enumerate() {
        let Some(positive) = (match t.kind {
            TokenKind::EqEq | TokenKind::Eqv => Some(true),
            TokenKind::Ne | TokenKind::Neqv => Some(false),
            _ => None,
        }) else { continue };
        let literal = |j: Option<usize>| j.and_then(|j| ts.get(j)).filter(|x| matches!(x.kind, TokenKind::True | TokenKind::False));
        let Some(lit) = literal(Some(i + 1)).or_else(|| literal(i.checked_sub(1))) else { continue };
        let truth = matches!(lit.kind, TokenKind::True);
        let lit_text = text(src, lit).to_ascii_lowercase();
        let keep = positive == truth;
        let mut warning = Warning::new("logical-comparison", format!("comparison with `{lit_text}` is redundant [logical-comparison]"),
            t.span.start.min(lit.span.start)..t.span.end.max(lit.span.end),
            if keep { "test the LOGICAL value itself" } else { "test `.not.` of the LOGICAL value" });
        // The other operand, when it is a single name.
        let lone = |j: usize| ts.get(j).filter(|x| matches!(x.kind, TokenKind::Ident(_)));
        let boundary = |j: Option<usize>| j.and_then(|j| ts.get(j)).is_none_or(|x| matches!(x.kind,
            TokenKind::LParen | TokenKind::RParen | TokenKind::Comma | TokenKind::And | TokenKind::Or | TokenKind::Not | TokenKind::KwThen));
        if lit.span.start > t.span.start {
            if let Some(name) = i.checked_sub(1).and_then(lone).filter(|_| boundary(i.checked_sub(2)) && boundary(Some(i + 2))) {
                let name = text(src, name);
                warning = warning.note(format!("suggestion: {}", if keep { name.to_owned() } else { format!(".not. {name}") }));
            }
        }
        if matches!(t.kind, TokenKind::EqEq | TokenKind::Ne) {
            warning = warning.note("`==` and `/=` do not apply to LOGICAL operands in standard Fortran; `.eqv.` and `.neqv.` do");
        }
        out.push(warning);
    }
    // DOUBLE PRECISION and DOUBLE COMPLEX.
    for (i, t) in ts.iter().enumerate() {
        let word = text(src, t).to_ascii_lowercase();
        let next = ts.get(i + 1).map(|n| text(src, n).to_ascii_lowercase());
        let (end, complex) = match (word.as_str(), next.as_deref()) {
            ("double", Some("precision")) => (ts[i + 1].span.end, false),
            ("double", Some("complex")) => (ts[i + 1].span.end, true),
            ("doubleprecision", _) => (t.span.end, false),
            ("doublecomplex", _) => (t.span.end, true),
            _ => continue,
        };
        let (written, use_) = if complex { ("DOUBLE COMPLEX", "complex(dp)") } else { ("DOUBLE PRECISION", "real(dp)") };
        out.push(Warning::new("double-precision", format!("{written} type [double-precision]"), t.span.start..end, format!("use {use_}"))
            .note("with `integer, parameter :: dp = kind(1.0d0)`, or `real64` from ISO_FORTRAN_ENV, one constant sets the precision of the program"));
        break;
    }
    // Declarations without `::`.
    if declaration(w) && !w.iter().any(|x| x == "::") {
        let end = spec_end(ts);
        if end < ts.len() {
            let last = ts[ts.len() - 1].span.end;
            let mut warning = Warning::new("missing-double-colon", "declaration without `::` [missing-double-colon]".to_owned(),
                ts[0].span.start..ts[end - 1].span.end, "add `::` after the type");
            if !src[ts[0].span.start..last].contains('\n') {
                warning = warning.note(format!("suggestion: {} :: {}", &src[ts[0].span.start..ts[end - 1].span.end], &src[ts[end].span.start..last]));
            }
            out.push(warning.note("`::` is required with attributes or initialization; using it always keeps declarations uniform"));
        }
    }
    // The innermost procedure.
    let procedure = stack.iter().rev().find_map(|f| f.unit.as_ref().filter(|_| f.kind == Kind::Unit));
    let in_procedure = procedure.filter(|u| matches!(u.kind.as_str(), "function" | "subroutine" | "procedure"));
    // GO TO and obsolescent or deleted features.
    let obsolete = |message: &str, note: &str| Warning::new("obsolescent", format!("{message} [obsolescent]"), span.clone(), "here").note(note);
    let goto = at(0) == "goto" || at(0) == "go" && at(1) == "to";
    if goto {
        let target = if at(0) == "goto" { at(1) } else { at(2) };
        if target == "(" {
            out.push(obsolete("computed GO TO is obsolescent", "use SELECT CASE"));
        } else if target.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()) {
            out.push(obsolete("assigned GO TO was deleted from the standard", "use SELECT CASE or a procedure"));
        } else {
            let at_goto = find("goto").unwrap_or(span.clone());
            out.push(Warning::new("goto", "GO TO statement [goto]".to_owned(), at_goto, "jumps to a label")
                .note("use a structured construct: EXIT or CYCLE in loops, a named BLOCK left with EXIT for forward jumps, or IF ... ELSE"));
        }
    }
    let first = w.first().map(String::as_str).unwrap_or("");
    if first == "if" && w.get(1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, 1) {
            if w.get(c + 1).is_some_and(|x| x.bytes().all(|b| b.is_ascii_digit())) && w.get(c + 2).is_some_and(|x| x == ",") {
                out.push(obsolete("arithmetic IF is obsolescent", "use IF ... ELSE IF ... ELSE with comparisons with zero"));
            }
        }
    }
    match (first, w.get(1).map(String::as_str)) {
        ("common", _) => out.push(obsolete("COMMON is obsolescent", "put shared data in a module")),
        ("equivalence", _) => out.push(obsolete("EQUIVALENCE is obsolescent", "use separate variables, TRANSFER for reinterpretation, or pointers")),
        ("entry", _) => out.push(obsolete("ENTRY is obsolescent", "write a separate module procedure for each entry point")),
        ("block", Some("data")) | ("blockdata", _) => out.push(obsolete("BLOCK DATA is obsolescent", "initialize module variables in their declarations")),
        ("forall", _) => out.push(obsolete("FORALL is obsolescent", "use DO CONCURRENT or array assignments")),
        ("pause", _) => out.push(obsolete("PAUSE was deleted from the standard", "use READ to wait for input")),
        ("assign", _) if w.get(2).is_some_and(|x| x == "to") => out.push(obsolete("ASSIGN was deleted from the standard", "use an integer variable and SELECT CASE")),
        ("character", Some("*")) => out.push(obsolete("the CHARACTER*length form is obsolescent", "write character(len=n)")),
        _ => {}
    }
    if at(0) == "return" && a.len() > 1 {
        out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
    }
    if first == "call" && w.windows(3).any(|x| matches!(x[0].as_str(), "(" | ",") && x[1] == "*" && x[2].bytes().all(|b| b.is_ascii_digit())) {
        out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
    }
    if matches!(classify(w, None), Role::Open(Kind::Unit, _)) && w.iter().any(|x| x == "subroutine") {
        if let Some(p) = w.iter().position(|x| x == "subroutine") {
            if w.get(p + 2).is_some_and(|x| x == "(") && closing_word(w, p + 2).is_some_and(|c| w[p + 3..c].iter().any(|x| x == "*")) {
                out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
            }
        }
    }
    // STOP in a procedure.
    if at(0) == "stop" {
        if let Some(u) = in_procedure {
            out.push(Warning::new("stop-in-procedure", format!("STOP in {} `{}` ends the whole program [stop-in-procedure]", u.kind, u.name),
                find("stop").unwrap_or(span.clone()), "ends the program")
                .note("return an error status argument so the caller can decide, or use ERROR STOP for an unrecoverable error so the exit status shows the failure"));
        }
    }
    // Assumed-size dummy arrays.
    if let Some(u) = in_procedure.filter(|u| !u.bind) {
        let dimension = first == "dimension";
        if declaration(w) || dimension {
            let colon = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon));
            let (attrs, entities) = match colon {
                Some(c) => (&ts[..c], &ts[c + 1..]),
                None if dimension => (&ts[..0], &ts[1.min(ts.len())..]),
                None => { let e = spec_end(ts); (&ts[..0], &ts[e..]) }
            };
            let shared = attrs.iter().position(|t| text(src, t).eq_ignore_ascii_case("dimension"))
                .filter(|&p| matches!(attrs.get(p + 1).map(|t| &t.kind), Some(TokenKind::LParen)))
                .and_then(|p| assumed_size(attrs, p + 1));
            for e in top_level(entities) {
                let Some(first) = e.first() else { continue };
                let name = text(src, first).to_ascii_lowercase();
                if !u.args.contains(&name) { continue; }
                let star = match e.get(1).map(|t| &t.kind) {
                    Some(TokenKind::LParen) => assumed_size(e, 1),
                    _ => shared.clone(),
                };
                if let Some(star) = star {
                    out.push(Warning::new("assumed-size", format!("dummy argument `{name}` is an assumed-size array [assumed-size]"), star, "assumed size")
                        .note(format!("declare it assumed-shape, as in `{name}(:)`, so that its size is known and bounds can be checked; the procedure then needs an explicit interface, as in a module")));
                }
            }
        }
    }
    // EXIT and CYCLE without a construct name, for the innermost DO loop.
    if matches!(at(0), "exit" | "cycle") && a.len() == 1 {
        let boundary = stack.iter().rposition(|f| f.kind == Kind::Unit).map_or(0, |p| p + 1);
        if let Some(p) = stack[boundary..].iter().rposition(|f| f.kind == Kind::Do) {
            let span = find(at(0)).unwrap_or(span.clone());
            stack[boundary + p].bare.push((span, at(0).to_owned()));
        }
    }
}

/// The END statement `w` of `unit` without its name.
fn bare_end(src: &str, ts: &[Token], w: &[String], unit: &Unit, out: &mut Vec<Warning>) {
    if w.last().is_some_and(|x| *x == unit.name) { return; }
    let span = first_line(src, ts[0].span.start, ts[ts.len() - 1].span.end);
    out.push(Warning::new("bare-end", format!("END statement without the name of {} `{}` [bare-end]", unit.kind, unit.name), span,
        format!("write `end {} {}`", unit.kind, unit.name))
        .note("naming the unit at its END lets the compiler check that the END closes the unit intended"));
}

/// The keyword tokens of a statement in keyword positions: the leading
/// keywords, after any construct name, those before `::`, and a final THEN.
/// Names spelled like keywords elsewhere in a statement are not counted.
fn keyword_positions(ts: &[Token], out: &mut Vec<Token>) {
    let keyword = |t: &Token| format!("{:?}", t.kind).starts_with("Kw");
    let start = if ts.len() > 2 && matches!(ts[1].kind, TokenKind::Colon) && matches!(ts[0].kind, TokenKind::Ident(_)) { 2 } else { 0 };
    out.extend(ts[start..].iter().take(3).take_while(|t| keyword(t)).cloned());
    if let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)) {
        out.extend(ts[start..colon].iter().skip(1).filter(|t| keyword(t)).cloned());
    }
    if let Some(last) = ts.last().filter(|t| matches!(t.kind, TokenKind::KwThen)) {
        if ts.len() > 1 { out.push(last.clone()); }
    }
}

fn keyword_case(src: &str, tokens: &[Token], out: &mut Vec<Warning>) {
    fn style(t: &str) -> &'static str {
        if !t.bytes().any(|b| b.is_ascii_uppercase()) { "lowercase" }
        else if !t.bytes().any(|b| b.is_ascii_lowercase()) { "uppercase" }
        else if t.split_whitespace().all(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && !w.chars().skip(1).any(|c| c.is_ascii_uppercase())) { "capitalized" }
        else { "mixed-case" }
    }
    let mut uses: Vec<(&'static str, Range<usize>)> = vec![];
    for t in tokens {
        let written = text(src, t);
        if written.len() < 2
            || !written.bytes().all(|b| b.is_ascii_alphabetic() || b == b' ' || b == b'_') { continue; }
        uses.push((style(written), t.span.clone()));
    }
    let mut counts: Vec<(&str, usize)> = vec![];
    for (s, _) in &uses {
        match counts.iter_mut().find(|(c, _)| c == s) { Some((_, n)) => *n += 1, None => counts.push((s, 1)) }
    }
    if counts.len() < 2 { return; }
    // The most common case, preferring the first written on ties.
    let most = counts.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let main = counts.iter().find(|(_, n)| *n == most).map(|(s, _)| *s).unwrap_or("lowercase");
    let summary = counts.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
    let others: Vec<&(&str, Range<usize>)> = uses.iter().filter(|(s, _)| *s != main).collect();
    let mut warning = Warning {
        category: "keyword-case",
        message: format!("keywords are written in more than one case: {summary} [keyword-case]"),
        labels: others.iter().take(5).map(|(s, span)| (span.clone(), format!("{s}; most keywords are {main}"))).collect(),
        related: vec![], notes: vec![],
    };
    if others.len() > 5 { warning = warning.note(format!("{} more keywords are not {main}", others.len() - 5)); }
    out.push(warning.note("write keywords in one case throughout the file"));
}

fn layout(src: &str, own_len: usize, tabs: bool, limit: Option<usize>, out: &mut Vec<Warning>) {
    let (mut tabbed, mut long) = (vec![], vec![]);
    let mut start = 0;
    for line in src[..own_len.min(src.len())].split('\n') {
        let line_text = line.trim_end_matches('\r');
        if tabs {
            if let Some(p) = code_part(line_text).find('\t') { tabbed.push(start + p..start + p + 1); }
        }
        if let Some(limit) = limit {
            let chars = line_text.chars().count();
            if chars > limit {
                let from = line_text.char_indices().nth(limit).map_or(line_text.len(), |(i, _)| i);
                long.push((start + from..start + line_text.len(), chars));
            }
        }
        start += line.len() + 1;
    }
    if !tabbed.is_empty() {
        let n = tabbed.len();
        let mut w = Warning { category: "tabs",
            message: format!("{n} line{} contain{} tab characters [tabs]", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }),
            labels: tabbed.iter().take(5).map(|s| (s.clone(), "tab".to_owned())).collect(), related: vec![], notes: vec![] };
        if n > 5 { w = w.note(format!("{} more lines contain tabs", n - 5)); }
        out.push(w.note("tab is not in the Fortran character set, and editors and compilers expand it differently; indent with spaces"));
    }
    if let (false, Some(limit)) = (long.is_empty(), limit) {
        let n = long.len();
        let mut w = Warning { category: "line-length",
            message: format!("{n} line{} longer than {limit} characters [line-length]", if n == 1 { " is" } else { "s are" }),
            labels: long.iter().take(5).map(|(s, c)| (s.clone(), format!("{c} characters"))).collect(), related: vec![], notes: vec![] };
        if n > 5 { w = w.note(format!("{} more lines are too long", n - 5)); }
        if limit == 132 {
            w = w.note("132 characters is the free-form limit before Fortran 2023; gfortran rejects longer lines without -ffree-line-length-none");
        }
        out.push(w.note("continue long statements with `&`"));
    }
}

fn analyze(src: &str, tokens: &[Token], own_len: usize, fixed_form: bool, line_limit: Option<usize>, tabs: bool) -> Vec<Warning> {
    let mut out = vec![];
    let mut stack: Vec<Frame> = vec![];
    let mut keywords = vec![];
    let mut i = 0;
    while i < tokens.len() && tokens[i].span.start < own_len {
        let all = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        i += all.len().max(1);
        let Some(start) = all.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))) else { continue };
        let label = all[..start].iter().find_map(|t| text(src, t).trim().parse::<u64>().ok());
        let ts = &all[start..];
        // Skip zero-width tokens the lexer inserts.
        let ts: Vec<Token> = ts.iter().filter(|t| !t.span.is_empty()).cloned().collect();
        if ts.is_empty() { continue; }
        keyword_positions(&ts, &mut keywords);
        let w = words(&src[ts[0].span.start..ts[ts.len() - 1].span.end]);
        let span = first_line(src, ts[0].span.start, ts[ts.len() - 1].span.end);
        let role = classify(&w, stack.last().map(|f| f.kind));
        statement_checks(src, &ts, &w, span.clone(), &mut stack, &mut out);
        // Loops ended by this labeled statement.
        if let Some(outermost) = label.and_then(|l| stack.iter().position(|f| f.kind == Kind::Do && f.label == Some(l))) {
            let ended = stack.split_off(outermost);
            let labeled = ended.iter().filter(|f| f.label == label).count();
            let action = !matches!(action(&w).first().map(String::as_str), Some("continue" | "enddo"))
                && !(w.first().is_some_and(|x| x == "end") && w.get(1).is_some_and(|x| x == "do"));
            for f in ended.iter().rev() {
                unnamed(f, &mut out);
                if f.label == label {
                    let l = label.unwrap_or(0);
                    let mut warning = Warning {
                        category: "labeled-do",
                        message: format!("DO loop ended by the statement labeled {l} [labeled-do]"),
                        labels: vec![(f.span.clone(), "labeled DO".to_owned())],
                        related: vec![(span.clone(), "ends here".to_owned())], notes: vec![],
                    };
                    if labeled > 1 { warning = warning.note("loops sharing a terminal statement are obsolescent"); }
                    else if action { warning = warning.note("a loop ended by a statement other than CONTINUE or END DO is obsolescent"); }
                    out.push(warning.note("write the loop without the label, ending it with END DO"));
                }
            }
            continue;
        }
        match role {
            Role::Open(kind, do_label) => stack.push(Frame {
                kind, label: do_label, span: span.clone(), name: construct_name(&w),
                unit: matches!(kind, Kind::Unit | Kind::Type).then(|| unit(&w, kind)).flatten(),
                bare: vec![],
            }),
            Role::Close(kind) => {
                let wanted = kind.unwrap_or(Kind::Unit);
                if let Some(p) = stack.iter().rposition(|f| f.kind == wanted) {
                    for f in stack.split_off(p).iter().rev() {
                        unnamed(f, &mut out);
                        if f.kind == wanted {
                            if let Some(u) = &f.unit { bare_end(src, &ts, &w, u, &mut out); }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    keywords.dedup_by(|a, b| a.span == b.span);
    keyword_case(src, &keywords, &mut out);
    if !fixed_form { layout(src, own_len, tabs, line_limit, &mut out); }
    out
}

/// Whether `line-length` is enabled, with its limit, and whether as an error.
fn line_length(settings: &SemaSettings, options: &[String]) -> (Option<usize>, bool) {
    let (mut enabled, mut error) = crate::unused::selected("line-length", false, settings, options);
    let mut limit = 132;
    for option in options {
        let (on, as_error, name) = match option.strip_prefix("no-") {
            Some(n) => (false, false, n),
            None => match option.strip_prefix("error=") { Some(n) => (true, true, n), None => (true, false, option.as_str()) },
        };
        if let Some(n) = name.strip_prefix("line-length=").and_then(|n| n.parse().ok()) {
            limit = n;
            enabled = on;
            error |= as_error;
        }
    }
    (enabled.then_some(limit), error)
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
    // Opt-in: -Wall does not enable these.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let controls: Vec<(&str, (bool, bool))> = CATEGORIES.iter().map(|c| (*c, crate::unused::selected(c, false, &opt_in, options))).collect();
    let (limit, limit_error) = line_length(&opt_in, options);
    let enabled = |c: &str| if c == "line-length" { limit.is_some() } else { controls.iter().any(|(n, (on, _))| *n == c && *on) };
    let as_error = |c: &str| if c == "line-length" { limit_error } else { controls.iter().any(|(n, (_, e))| *n == c && *e) };
    if !CATEGORIES.iter().any(|c| enabled(c)) { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    let mut warnings = analyze(src, &tokens, own_len, fixed_form, limit, enabled("tabs"));
    warnings.retain(|w| enabled(w.category));
    if warnings.is_empty() { return Ok(()); }
    warnings.sort_by_key(|w| w.labels.first().map_or(0, |l| l.0.start));
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    let mut failed = vec![];
    for w in &warnings {
        let mut labels = vec![];
        for (span, label) in &w.labels {
            let (file, at) = locate(span.clone());
            labels.push(Label::primary(file, at).with_message(label.clone()));
        }
        for (span, label) in &w.related {
            let (file, at) = locate(span.clone());
            labels.push(Label::secondary(file, at).with_message(label.clone()));
        }
        let error = as_error(w.category);
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(w.message.clone()).with_labels(labels).with_notes(w.notes.clone());
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
        if error && !failed.contains(&w.category) { failed.push(w.category); }
    }
    if !failed.is_empty() { bail!("{} warnings treated as errors", failed.join(", ")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each warning as (category, line, message without the category).
    fn warnings(src: &str) -> Vec<(&'static str, usize, String)> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len(), false, Some(132), true).into_iter().map(|w| {
            let at = w.labels.first().map_or(0, |l| l.0.start);
            let message = w.message.rsplit_once(" [").map_or(w.message.clone(), |(m, _)| m.to_owned());
            (w.category, src[..at].matches('\n').count() + 1, message)
        }).collect()
    }
    fn categories(src: &str) -> Vec<&'static str> {
        warnings(src).into_iter().map(|w| w.0).collect()
    }

    #[test]
    fn modern_code_is_not_reported() {
        let src = "module m\nimplicit none\ncontains\nsubroutine s(x, n, ok)\nreal, intent(inout) :: x(:)\n\
            integer, intent(in) :: n\nlogical, intent(in) :: ok\ninteger :: i\ncharacter(len=*), parameter :: c = 'a.lt.b'\n\
            outer: do i = 1, n\n   if (x(i) < 0 .and. ok) exit outer\n   if (x(i) == 0) cycle outer\nend do outer\nend subroutine s\n\
            end module m\n";
        assert_eq!(warnings(src), []);
    }

    #[test]
    fn style_findings() {
        let src = "subroutine s(x, y, n, flag)\ninteger n, i\nreal x(*), y(n, *)\nlogical flag\ndouble precision d\n\
            if (n .lt. 0 .or. flag .eqv. .true.) stop\ndo 10 i = 1, n\n   if (x(i) > 0) exit\n   x(i) = 0\n10 continue\n\
            go to 20\n20 continue\nend\n";
        assert_eq!(categories(src), ["missing-double-colon", "missing-double-colon", "assumed-size", "assumed-size",
            "missing-double-colon", "double-precision", "missing-double-colon", "old-relational", "logical-comparison",
            "stop-in-procedure", "unnamed-exit", "labeled-do", "goto", "bare-end"]);
        let w = warnings(src);
        assert!(w.iter().any(|x| x.2 == "`.lt.` is the old spelling of `<`"));
        assert!(w.iter().any(|x| x.2 == "END statement without the name of subroutine `s`"));
        assert!(w.iter().any(|x| x.2 == "comparison with `.true.` is redundant"));
    }

    #[test]
    fn obsolescent_features() {
        let src = "subroutine s(k, *)\ninteger :: k, a(2), b(2)\ncommon /blk/ a\nequivalence (a, b)\n\
            go to (1, 2), k\n1 if (k) 2, 2, 3\n2 return 1\n3 continue\nend subroutine s\n";
        let w: Vec<String> = warnings(src).into_iter().filter(|x| x.0 == "obsolescent").map(|x| x.2).collect();
        assert_eq!(w, ["alternate return is obsolescent", "COMMON is obsolescent", "EQUIVALENCE is obsolescent",
            "computed GO TO is obsolescent", "arithmetic IF is obsolescent", "alternate return is obsolescent"]);
    }

    #[test]
    fn keyword_case_and_layout() {
        let src = format!("program p\nINTEGER :: i\ndo i = 1, 2\n\tprint *, i\nend do\nprint *, '{}'\nend program p\n", "x".repeat(140));
        let w = warnings(&src);
        assert!(w.iter().any(|x| x.0 == "keyword-case" && x.2.starts_with("keywords are written in more than one case")), "{w:?}");
        assert!(w.iter().any(|x| x.0 == "tabs" && x.1 == 4));
        assert!(w.iter().any(|x| x.0 == "line-length" && x.1 == 6));
        // Names spelled like keywords are not keywords.
        let src = "program p\ninteger :: DEFAULT, LEN\nDEFAULT = 1\nLEN = DEFAULT\nprint *, LEN\nend program p\n";
        assert!(!warnings(src).iter().any(|x| x.0 == "keyword-case"), "{:?}", warnings(src));
    }
}

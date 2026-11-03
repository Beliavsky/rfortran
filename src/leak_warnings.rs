// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wmemory-leak`, on by default: storage allocated through a local POINTER
//! of a procedure that is lost without DEALLOCATE, when the procedure returns
//! or when the pointer is reassigned by ALLOCATE, NULLIFY or `=>`.
//!
//! The analysis follows the paths of each procedure. A leak on every path
//! "is not deallocated"; a leak on some paths "may not be deallocated", with
//! the IF whose outcome loses the storage when it can name one. The ELSE
//! branch of `IF (ASSOCIATED(p))` holds no storage.
//!
//! Only leaks that are certain on their path are reported, so a pointer that
//! might keep the storage elsewhere is not checked: one passed to a
//! procedure, used as the target of `=>`, returned, or named in an internal
//! procedure. Dummy arguments, function results, SAVE pointers (including
//! those initialized with `=> NULL()`), ALLOCATE with STAT= and procedures
//! with GO TO are skipped too.
use crate::ast::{Expr, Program, Stmt, UnOp};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{BTreeSet, HashSet};

const CATEGORY: &str = "memory-leak";

/// Intrinsic procedures that read or define a POINTER's target but cannot
/// keep the pointer.
const INTRINSICS: &[&str] = &[
    "abs", "acos", "aimag", "aint", "all", "anint", "any", "asin", "associated", "atan", "atan2", "ceiling",
    "cmplx", "conjg", "cos", "cosh", "count", "cpu_time", "cshift", "dble", "dot_product", "eoshift", "exp",
    "findloc", "floor", "huge", "int", "is_contiguous", "kind", "lbound", "len", "len_trim", "log", "log10",
    "matmul", "max", "maxloc", "maxval", "merge", "min", "minloc", "minval", "mod", "modulo", "nint", "norm2",
    "pack", "product", "random_number", "real", "repeat", "reshape", "shape", "sign", "sin", "sinh", "size",
    "spread", "sqrt", "sum", "tan", "tanh", "tiny", "transpose", "trim", "ubound", "unpack",
];

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    /// The pointer holds no storage this procedure allocated.
    Free,
    /// It holds the storage allocated at a statement offset; on a path
    /// through the IF at `via`, taken when its condition has that value.
    Owned { site: usize, via: Option<(usize, bool)> },
}
type States = BTreeSet<State>;

#[derive(Default)]
struct Flow {
    normal: States,
    exits: States,
    cycles: States,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Point {
    /// The procedure ends or RETURNs at the offset.
    Return(Option<usize>),
    /// ALLOCATE, NULLIFY or `=>` at the offset replaces the storage.
    Reassign(usize),
}

struct Leak {
    site: usize,
    point: Point,
    definite: bool,
    via: Option<(usize, bool)>,
}

fn root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Some(n.split('%').next().unwrap_or(n)),
        _ => None,
    }
}

fn names(e: &Expr, p: &str) -> bool {
    let mut found = false;
    crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |e| found |= root(e) == Some(p));
    found
}

/// Whether a statement, nested bodies included, names `p`.
fn mentions(s: &Stmt, p: &str) -> bool {
    let mut s = s.clone();
    if matches!(&s, Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } if name.split('%').next() == Some(p)) {
        return true;
    }
    if crate::procedure_arguments::expressions(&mut s).into_iter().any(|e| names(e, p)) {
        return true;
    }
    crate::procedure_arguments::children(&mut s).into_iter().any(|b| b.iter().any(|s| mentions(s, p)))
}

/// Whether `p` might keep its storage beyond what the procedure shows.
fn escapes(body: &[Stmt], p: &str) -> bool {
    // The whole pointer as an argument of anything but a known intrinsic.
    fn argument(e: &Expr, p: &str) -> bool {
        let mut found = false;
        crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |e| {
            if let Expr::Call(n, args) | Expr::Index(n, args) = e {
                if !INTRINSICS.contains(&n.as_str()) && !n.starts_with('@') {
                    found |= args.iter().any(|a| {
                        let a = crate::named_arguments::keyword(a).map_or(a, |(_, v)| v);
                        matches!(a, Expr::Ident(n) if n == p)
                    });
                }
            }
        });
        found
    }
    body.iter().any(|s| match s {
        Stmt::Function { .. } | Stmt::Subroutine { .. } => mentions(s, p),
        Stmt::CallSub { name, args } if name == "@pointer_associate" => args.get(1).is_some_and(|t| root(t) == Some(p)) || args[1..].iter().any(|a| argument(a, p)),
        Stmt::CallSub { name, args } if !name.starts_with('@') && !INTRINSICS.contains(&name.as_str()) => args.iter().any(|a| {
            let a = crate::named_arguments::keyword(a).map_or(a, |(_, v)| v);
            root(a) == Some(p) && matches!(a, Expr::Ident(_)) || argument(a, p)
        }),
        Stmt::Return(Some(e)) if names(e, p) => true,
        _ => {
            let mut copy = s.clone();
            crate::procedure_arguments::expressions(&mut copy).into_iter().any(|e| argument(e, p))
                || crate::procedure_arguments::children(&mut copy).into_iter().any(|b| escapes(b, p))
        }
    })
}

fn gotos(body: &[Stmt]) -> bool {
    body.iter().any(|s| matches!(s, Stmt::CallSub { name, .. } if name == "@goto") || {
        let mut copy = s.clone();
        crate::procedure_arguments::children(&mut copy).into_iter().any(|b| gotos(b))
    })
}

struct Walker<'a> {
    p: &'a str,
    leaks: Vec<Leak>,
    /// Allocations that some path deallocates.
    freed: HashSet<usize>,
}

impl Walker<'_> {
    fn leak(&mut self, states: &States, point: Point) {
        let definite = !states.contains(&State::Free);
        for s in states {
            if let State::Owned { site, via } = s {
                if !self.leaks.iter().any(|l| l.site == *site && l.point == point) {
                    self.leaks.push(Leak { site: *site, point, definite, via: *via });
                }
            }
        }
    }

    fn whole(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n) if n == self.p)
    }

    /// The states entering the THEN and ELSE branches of an IF.
    fn refine(&self, cond: &Expr, states: &States) -> (States, States) {
        let associated = |e: &Expr| matches!(e, Expr::Call(n, args) if n == "associated" && args.len() == 1 && self.whole(&args[0]));
        let free = || States::from([State::Free]);
        if associated(cond) {
            (states.clone(), free())
        } else if matches!(cond, Expr::Un(UnOp::Not, e) if associated(e)) {
            (free(), states.clone())
        } else {
            (states.clone(), states.clone())
        }
    }

    fn join_if(then: States, other: States, offset: usize) -> States {
        let mark = |states: &States, other: &States, value: bool| -> States {
            states.iter().map(|s| match s {
                State::Owned { site, via: None } if other.contains(&State::Free) => State::Owned { site: *site, via: Some((offset, value)) },
                s => s.clone(),
            }).collect()
        };
        let mut out = mark(&then, &other, true);
        out.extend(mark(&other, &then, false));
        out
    }

    fn looped(&mut self, body: &[Stmt], entry: States, offset: usize, endless: bool) -> Flow {
        let mut head = entry.clone();
        let mut exits = States::new();
        for _ in 0..8 {
            let f = self.walk(body, head.clone(), offset);
            exits.extend(f.exits);
            let mut next = entry.clone();
            next.extend(f.normal);
            next.extend(f.cycles);
            if next == head { break; }
            head = next;
        }
        let mut normal = if endless { States::new() } else { head };
        normal.extend(exits);
        Flow { normal, ..Flow::default() }
    }

    fn walk(&mut self, body: &[Stmt], input: States, mut offset: usize) -> Flow {
        let mut cur = input;
        let mut flow = Flow::default();
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if cur.is_empty() {
                continue;
            }
            match s {
                Stmt::CallSub { name, args } if name == "@allocate" && args.iter().any(|a| root(a) == Some(self.p)) => {
                    self.leak(&cur, Point::Reassign(offset));
                    let stat = args.iter().any(|a| matches!(crate::named_arguments::keyword(a), Some(("stat", _))));
                    cur = States::from([if stat { State::Free } else { State::Owned { site: offset, via: None } }]);
                }
                Stmt::CallSub { name, args } if name == "@deallocate" && args.iter().any(|a| self.whole(a)) => {
                    self.freed.extend(cur.iter().filter_map(|s| match s { State::Owned { site, .. } => Some(*site), State::Free => None }));
                    cur = States::from([State::Free]);
                }
                Stmt::CallSub { name, args } if (name == "@nullify" && args.iter().any(|a| self.whole(a)))
                    || (name == "@pointer_associate" && args.first().is_some_and(|a| self.whole(a))) => {
                    self.leak(&cur, Point::Reassign(offset));
                    cur = States::from([State::Free]);
                }
                Stmt::CallSub { name, .. } if name == "@stop" || name == "@error_stop" => cur.clear(),
                Stmt::Return(e) => {
                    self.leak(&cur, Point::Return(e.is_none().then_some(offset)));
                    cur.clear();
                }
                Stmt::Exit => flow.exits.extend(std::mem::take(&mut cur)),
                Stmt::Cycle => flow.cycles.extend(std::mem::take(&mut cur)),
                Stmt::If { cond, then_body, else_body } => {
                    let (t, e) = self.refine(cond, &cur);
                    let t = self.walk(then_body, t, offset);
                    let e = match else_body {
                        Some(b) => self.walk(b, e, offset),
                        None => Flow { normal: e, ..Flow::default() },
                    };
                    cur = Self::join_if(t.normal, e.normal, offset);
                    for f in [t.exits, e.exits] { flow.exits.extend(f); }
                    for f in [t.cycles, e.cycles] { flow.cycles.extend(f); }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    let mut out = if default.is_none() { cur.clone() } else { States::new() };
                    for b in cases.iter().map(|c| &c.body).chain(default.iter()) {
                        let f = self.walk(b, cur.clone(), offset);
                        out.extend(f.normal);
                        flow.exits.extend(f.exits);
                        flow.cycles.extend(f.cycles);
                    }
                    cur = out;
                }
                Stmt::Do { body, .. } => cur = self.looped(body, cur, offset, false).normal,
                Stmt::DoWhile { cond, body } => cur = self.looped(body, cur, offset, matches!(cond, Expr::Logical(true))).normal,
                // An explicit-step DO or FORALL is a BLOCK that starts with its control.
                Stmt::Block { body } if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name == "@do_step" || name == "@forall") => {
                    cur = self.looped(&body[1..], cur, offset, false).normal;
                }
                Stmt::Block { body } => {
                    let f = self.walk(body, cur, offset);
                    cur = f.normal;
                    flow.exits.extend(f.exits);
                    flow.cycles.extend(f.cycles);
                }
                _ => {}
            }
        }
        flow.normal = cur;
        flow
    }
}

/// Local POINTERs of a procedure body that the analysis can follow.
fn candidates(src: &str, tokens: &[Token], body: &[Stmt], params: &[String], name: &str) -> Vec<(String, usize)> {
    let mut pointers = vec![];
    let mut saved = HashSet::new();
    let mut declared = vec![];
    let mut offset = 0;
    for s in body {
        match s {
            Stmt::SourceLocation(n) => offset = *n,
            Stmt::DummyIntent { name, intent } if intent == "pointer" => pointers.push(name.clone()),
            Stmt::DummyIntent { name, intent } if intent == "save" => { saved.insert(name.clone()); }
            Stmt::InitializedDecl { attributes, entities, .. } => {
                for (n, _) in entities {
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("save")) { saved.insert(n.clone()); }
                    declared.push((n.clone(), offset));
                }
            }
            Stmt::VarDecl { names, .. } => declared.extend(names.iter().map(|n| (n.clone(), offset))),
            Stmt::ArrayDecl { name, .. } => declared.push((name.clone(), offset)),
            Stmt::DerivedDecl { names, .. } => declared.extend(names.iter().map(|n| (n.clone(), offset))),
            _ => {}
        }
    }
    if saved.contains(crate::data_statements::SAVE_ALL) { return vec![]; }
    pointers.into_iter().filter_map(|p| {
        let &(_, at) = declared.iter().find(|(n, _)| *n == p)?;
        // `=> NULL()` in the declaration implies SAVE.
        let initialized = crate::declaration_warnings::statement_tokens(tokens, at, usize::MAX, src)
            .iter().any(|t| matches!(t.kind, TokenKind::Arrow));
        (!initialized && !saved.contains(&p) && !params.contains(&p) && p != name).then_some((p, at))
    }).collect()
}

struct Report {
    procedure: String,
    pointer: String,
    leak: Leak,
}

fn procedures(src: &str, tokens: &[Token], body: &[Stmt], out: &mut Vec<Report>) {
    for s in body {
        match s {
            Stmt::Module { body, .. } => procedures(src, tokens, body, out),
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                procedures(src, tokens, body, out);
                if gotos(body) { continue; }
                for (p, _) in candidates(src, tokens, body, params, name) {
                    if escapes(body, &p) { continue; }
                    let mut w = Walker { p: &p, leaks: vec![], freed: HashSet::new() };
                    let end = w.walk(body, States::from([State::Free]), 0).normal;
                    w.leak(&end, Point::Return(None));
                    // A leak is certain only for storage no path deallocates.
                    for l in &mut w.leaks { l.definite &= !w.freed.contains(&l.site); }
                    out.extend(w.leaks.into_iter().map(|leak| Report { procedure: name.clone(), pointer: p.clone(), leak }));
                }
            }
            _ => {}
        }
    }
}

fn span(src: &str, tokens: &[Token], offset: usize, name: Option<&str>) -> std::ops::Range<usize> {
    let ts = crate::declaration_warnings::statement_tokens(tokens, offset, usize::MAX, src);
    name.and_then(|n| ts.iter().find(|t| matches!(&t.kind, TokenKind::Ident(i) if i == n)))
        .or(ts.first())
        .map_or(offset..offset, |t| t.span.clone())
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // On by default: only -Wno-memory-leak or !#allow(memory-leak) turn it off.
    let (mut enabled, error) = crate::unused::selected(CATEGORY, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == CATEGORY).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(CATEGORY) { enabled = true; }
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let mut reports = vec![];
    procedures(src, &tokens, &program.body, &mut reports);
    reports.retain(|r| r.leak.site < own_len);
    reports.sort_by_key(|r| r.leak.site);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for Report { procedure, pointer: p, leak } in &reports {
        let (file, site) = locate(span(src, &tokens, leak.site, Some(p)));
        let event = match leak.point {
            Point::Return(_) => format!("before `{procedure}` returns"),
            Point::Reassign(_) => "before the pointer is reassigned".to_string(),
        };
        let message = if leak.definite {
            format!("POINTER `{p}` is not deallocated {event}, so its storage leaks [{CATEGORY}]")
        } else {
            format!("POINTER `{p}` may not be deallocated {event}, so its storage may leak [{CATEGORY}]")
        };
        let again = leak.point == Point::Reassign(leak.site);
        let mut labels = vec![Label::primary(file, site).with_message(if again {
            "allocated here, then again on the next iteration without DEALLOCATE"
        } else {
            "allocated here"
        })];
        match leak.point {
            Point::Reassign(_) if again => {}
            Point::Return(Some(at)) => {
                let (file, at) = locate(span(src, &tokens, at, None));
                labels.push(Label::secondary(file, at).with_message(format!("returns with `{p}` still allocated")));
            }
            Point::Reassign(at) => {
                let (file, at) = locate(span(src, &tokens, at, Some(p)));
                labels.push(Label::secondary(file, at).with_message("reassigned here"));
            }
            Point::Return(None) => {}
        }
        if let Some((at, value)) = leak.via.filter(|_| !leak.definite) {
            let (file, at) = locate(span(src, &tokens, at, None));
            let value = if value { "true" } else { "false" };
            labels.push(Label::secondary(file, at).with_message(format!("when this condition is {value}, `{p}` is still allocated")));
        }
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(labels)
            .with_notes(vec![format!("DEALLOCATE `{p}` on every path; -Wno-{CATEGORY} disables this warning")]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error && !reports.is_empty() { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

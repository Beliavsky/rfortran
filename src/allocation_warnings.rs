// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wallocation-state`, on by default: DEALLOCATE of a local ALLOCATABLE
//! that is unallocated on every path reaching the statement, or ALLOCATE of
//! one that is allocated on every path, without STAT=. Either is an error
//! that stops the program when the statement runs.
//!
//! A local ALLOCATABLE starts unallocated. ALLOCATE, assignment to the whole
//! variable, DEALLOCATE and MOVE_ALLOC change its state, and the branches of
//! `IF (ALLOCATED(x))` know it. Only a state that is certain on every path is
//! reported, so a condition the analysis cannot follow leaves both states
//! possible and nothing is reported. Passing the whole variable to a
//! procedure that is not a known intrinsic makes its state unknown.
//! Variables named in an internal procedure, dummy arguments, function
//! results, SAVE variables, variables of a nested BLOCK and procedures with
//! GO TO are not checked.
use crate::ast::{Expr, Program, Stmt, UnOp};
use crate::leak_warnings::{gotos, mentions, span, INTRINSICS};
use crate::lexer::{lex, Token};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const CATEGORY: &str = "allocation-state";

/// The forms of ALLOCATE: plain, and with a derived type, intrinsic type or
/// CHARACTER length specification as the first argument.
const ALLOCATE: &[&str] = &["@allocate", "@allocate_derived_type", "@allocate_intrinsic_type", crate::substrings::ALLOCATE_CHARACTER];

/// Why a variable has its state, with the offset of the statement.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cause {
    Declared(usize),
    Allocated(usize),
    Assigned(usize),
    Deallocated(usize),
    Moved(usize),
    /// A branch of `IF (ALLOCATED(x))`.
    Tested(usize),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    /// Allocated or not, and why.
    Known(bool, Cause),
    Unknown,
}
type States = BTreeSet<State>;

#[derive(Default)]
struct Flow {
    normal: States,
    exits: States,
    cycles: States,
}

fn unwrap(e: &Expr) -> &Expr {
    crate::named_arguments::keyword(e).map_or(e, |(_, v)| v)
}

fn stat(args: &[Expr]) -> bool {
    args.iter().any(|a| matches!(crate::named_arguments::keyword(a), Some(("stat", _))))
}

struct Walker<'a> {
    x: &'a str,
    /// The states reaching each ALLOCATE (true) or DEALLOCATE (false)
    /// without STAT= that names the variable, over every pass of the loops.
    records: BTreeMap<(usize, bool), States>,
    diverged: bool,
}

impl Walker<'_> {
    fn whole(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n) if n == self.x)
    }

    /// Whether the statement passes the whole variable to a procedure that
    /// might allocate or deallocate it.
    fn passed(&self, s: &Stmt) -> bool {
        let callee = |n: &str| !n.starts_with('@') && !INTRINSICS.contains(&n)
            && !["allocated", "move_alloc"].contains(&n.to_ascii_lowercase().as_str());
        if let Stmt::CallSub { name, args } = s {
            if callee(name) && args.iter().any(|a| self.whole(unwrap(a))) {
                return true;
            }
        }
        let mut copy = s.clone();
        crate::procedure_arguments::expressions(&mut copy).into_iter().any(|e| {
            let mut found = false;
            crate::procedure_arguments::visit_expr(e, &mut |e| if let Expr::Call(n, args) | Expr::Index(n, args) = e {
                found |= callee(n) && args.iter().any(|a| self.whole(unwrap(a)));
            });
            found
        })
    }

    /// The states entering the THEN and ELSE branches of an IF.
    fn refine(&self, cond: &Expr, states: &States, offset: usize) -> (States, States) {
        let allocated = |e: &Expr| matches!(e, Expr::Call(n, args) if n == "allocated" && args.len() == 1 && self.whole(unwrap(&args[0])));
        let only = |value: bool| -> States {
            states.iter().filter_map(|s| match s {
                State::Known(a, _) if *a != value => None,
                State::Unknown => Some(State::Known(value, Cause::Tested(offset))),
                s => Some(s.clone()),
            }).collect()
        };
        if allocated(cond) {
            (only(true), only(false))
        } else if matches!(cond, Expr::Un(UnOp::Not, e) if allocated(e)) {
            (only(false), only(true))
        } else {
            (states.clone(), states.clone())
        }
    }

    fn record(&mut self, offset: usize, allocate: bool, states: &States) {
        self.records.entry((offset, allocate)).or_default().extend(states.iter().cloned());
    }

    fn looped(&mut self, body: &[Stmt], entry: States, offset: usize, endless: bool) -> Flow {
        let mut head = entry.clone();
        let mut exits = States::new();
        let mut settled = false;
        for _ in 0..32 {
            let f = self.walk(body, head.clone(), offset);
            exits.extend(f.exits);
            let mut next = entry.clone();
            next.extend(f.normal);
            next.extend(f.cycles);
            if next == head {
                settled = true;
                break;
            }
            head = next;
        }
        self.diverged |= !settled;
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
            if self.passed(s) {
                cur = States::from([State::Unknown]);
            }
            match s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
                // A type specification or CHARACTER length comes first.
                Stmt::CallSub { name, args } if ALLOCATE.contains(&name.as_str())
                    && args[usize::from(name != "@allocate")..].iter().any(|a| matches!(a, Expr::Call(n, _) | Expr::Section { name: n, .. } if n == self.x) || self.whole(a)) => {
                    let allocated = State::Known(true, Cause::Allocated(offset));
                    if stat(args) {
                        cur.insert(allocated);
                    } else {
                        self.record(offset, true, &cur);
                        cur = States::from([allocated]);
                    }
                }
                Stmt::CallSub { name, args } if name == "@deallocate" && args.iter().any(|a| self.whole(a)) => {
                    if !stat(args) {
                        self.record(offset, false, &cur);
                    }
                    cur = States::from([State::Known(false, Cause::Deallocated(offset))]);
                }
                Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => {
                    let arg = |key: &str, i: usize| args.iter()
                        .find(|a| matches!(crate::named_arguments::keyword(a), Some((k, _)) if k == key))
                        .or_else(|| args.get(i).filter(|a| crate::named_arguments::keyword(a).is_none()))
                        .map(unwrap);
                    if arg("from", 0).is_some_and(|a| self.whole(a)) {
                        cur = States::from([State::Known(false, Cause::Moved(offset))]);
                    } else if arg("to", 1).is_some_and(|a| self.whole(a)) {
                        cur = States::from([State::Unknown]);
                    }
                }
                Stmt::Assign { name, .. } if name == self.x => cur = States::from([State::Known(true, Cause::Assigned(offset))]),
                Stmt::CallSub { name, .. } if name == "@stop" || name == "@error_stop" => cur.clear(),
                Stmt::Return(_) => cur.clear(),
                Stmt::Exit => flow.exits.extend(std::mem::take(&mut cur)),
                Stmt::Cycle => flow.cycles.extend(std::mem::take(&mut cur)),
                Stmt::If { cond, then_body, else_body } => {
                    let (t, e) = self.refine(cond, &cur, offset);
                    let t = self.walk(then_body, t, offset);
                    let e = match else_body {
                        Some(b) => self.walk(b, e, offset),
                        None => Flow { normal: e, ..Flow::default() },
                    };
                    cur = t.normal;
                    cur.extend(e.normal);
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

/// Whether a nested body, outside internal procedures, declares `x`.
fn redeclared(body: &[Stmt], x: &str) -> bool {
    body.iter().any(|s| {
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
            return false;
        }
        let mut copy = s.clone();
        crate::procedure_arguments::children(&mut copy).into_iter().any(|b| b.iter().any(|s| match s {
            Stmt::DummyIntent { name, .. } => name == x,
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.iter().any(|n| n == x),
            Stmt::ArrayDecl { name, .. } => name == x,
            Stmt::InitializedDecl { entities, .. } => entities.iter().any(|(n, _)| n == x),
            _ => false,
        }) || redeclared(b, x))
    })
}

fn returned(body: &[Stmt], x: &str) -> bool {
    body.iter().any(|s| {
        let mut copy = s.clone();
        matches!(s, Stmt::Return(Some(e)) if mentions(&Stmt::Return(Some(e.clone())), x))
            || !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. })
                && crate::procedure_arguments::children(&mut copy).into_iter().any(|b| returned(b, x))
    })
}

/// Local ALLOCATABLEs of a procedure or main program body, with the offsets
/// of their declarations.
fn candidates(body: &[Stmt], params: &[String], name: &str, internal: &dyn Fn(&str) -> bool) -> Vec<(String, usize)> {
    let mut allocatables = vec![];
    let mut saved = HashSet::new();
    let mut offset = 0;
    for s in body {
        match s {
            Stmt::SourceLocation(n) => offset = *n,
            Stmt::DummyIntent { name, intent } if intent == "allocatable" => allocatables.push((name.clone(), offset)),
            Stmt::DerivedDecl { names, attributes, .. } if attributes.iter().any(|a| a == "allocatable") => {
                allocatables.extend(names.iter().map(|n| (n.clone(), offset)));
                if attributes.iter().any(|a| a == "save") { saved.extend(names.iter().cloned()); }
            }
            Stmt::DummyIntent { name, intent } if intent == "save" => { saved.insert(name.clone()); }
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a.eq_ignore_ascii_case("save")) => {
                saved.extend(entities.iter().map(|(n, _)| n.clone()));
            }
            _ => {}
        }
    }
    if saved.contains(crate::data_statements::SAVE_ALL) {
        return vec![];
    }
    let mut seen = HashSet::new();
    allocatables.retain(|(n, _)| seen.insert(n.clone()));
    allocatables.into_iter().filter(|(x, _)| {
        !saved.contains(x) && !params.contains(x) && x != name && !returned(body, x) && !redeclared(body, x)
            && !body.iter().any(|s| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if internal(name)) && mentions(s, x))
    }).collect()
}

struct Report {
    var: String,
    offset: usize,
    allocate: bool,
    causes: Vec<Cause>,
}

fn analyze(body: &[Stmt], params: &[String], name: &str, internal: &dyn Fn(&str) -> bool, out: &mut Vec<Report>) {
    if gotos(body) {
        return;
    }
    for (x, declared) in candidates(body, params, name, internal) {
        let mut w = Walker { x: &x, records: BTreeMap::new(), diverged: false };
        w.walk(body, States::from([State::Known(false, Cause::Declared(declared))]), 0);
        if w.diverged {
            continue;
        }
        for ((offset, allocate), states) in w.records {
            let causes: Option<Vec<Cause>> = states.iter().map(|s| match s {
                State::Known(a, c) if *a == allocate => Some(*c),
                _ => None,
            }).collect();
            if let Some(causes) = causes.filter(|c| !c.is_empty()) {
                out.push(Report { var: x.clone(), offset, allocate, causes });
            }
        }
    }
}

fn procedures(body: &[Stmt], out: &mut Vec<Report>) {
    for s in body {
        match s {
            Stmt::Module { body, .. } => procedures(body, out),
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                procedures(body, out);
                analyze(body, params, name, &|_| true, out);
            }
            _ => {}
        }
    }
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
    // On by default: only -Wno-allocation-state or !#allow(allocation-state) turn it off.
    let (mut enabled, error) = crate::unused::selected(CATEGORY, false, settings, options);
    let disabled = options.iter().rev().find(|o| o.trim_start_matches("no-") == CATEGORY).is_some_and(|o| o.starts_with("no-"));
    if !disabled && !settings.allow.contains(CATEGORY) { enabled = true; }
    if !enabled { return Ok(()); }
    let mut reports = vec![];
    procedures(&program.body, &mut reports);
    // The main program's own statements, whose internal procedures sit
    // beside the external ones.
    analyze(&program.body, &[], "", &|n| program.contains.iter().any(|c| c == n), &mut reports);
    reports.retain(|r| r.offset < own_len);
    if reports.is_empty() { return Ok(()); }
    reports.sort_by_key(|r| (r.offset, r.var.clone()));
    let tokens: Vec<Token> = lex(src);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for Report { var: x, offset, allocate, causes } in &reports {
        let (file, at) = locate(span(src, &tokens, *offset, Some(x)));
        let (message, here, note) = if *allocate {
            (format!("ALLOCATE of `{x}`, which is already allocated, is an error at run time [{CATEGORY}]"),
             format!("`{x}` is already allocated here"),
             format!("DEALLOCATE `{x}` first, or add STAT= to handle the error; -Wno-{CATEGORY} disables this warning"))
        } else {
            (format!("DEALLOCATE of `{x}`, which is not allocated, is an error at run time [{CATEGORY}]"),
             format!("`{x}` is not allocated here"),
             format!("test `allocated({x})` first, or add STAT= to handle the error; -Wno-{CATEGORY} disables this warning"))
        };
        let mut labels = vec![Label::primary(file, at).with_message(here)];
        for cause in causes {
            let (at, why) = match *cause {
                Cause::Declared(at) => (at, "declared here, unallocated until ALLOCATE"),
                Cause::Allocated(at) => (at, "allocated here"),
                Cause::Assigned(at) => (at, "allocated by this assignment"),
                Cause::Deallocated(at) => (at, "deallocated here"),
                Cause::Moved(at) => (at, "moved away by MOVE_ALLOC here"),
                Cause::Tested(at) => (at, if *allocate { "known to be allocated from this test" } else { "known to be unallocated from this test" }),
            };
            let (file, at) = locate(span(src, &tokens, at, Some(x)));
            labels.push(Label::secondary(file, at).with_message(why));
        }
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(labels)
            .with_notes(vec![note]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

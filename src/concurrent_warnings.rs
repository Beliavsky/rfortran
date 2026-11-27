// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wcould-be-concurrent`, in -Wextra: a counted DO loop whose iterations
//! are independent could be DO CONCURRENT, which rfort can run in parallel.
//!
//! DO CONCURRENT is a promise the compiler does not check, so a wrong
//! suggestion would be a bug. The proof is conservative, and a loop is
//! reported only when its body has
//!
//! - assignments, IF, SELECT CASE, inner counted DO loops, CYCLE, and EXIT
//!   of an inner loop: no I/O, procedure calls, GO TO, RETURN or STOP, and
//!   no named EXIT or CYCLE;
//! - function references only to pure intrinsics and to PURE functions of a
//!   module or of the program's top level, not to internal procedures, which
//!   could see the scope's variables;
//! - writes only to elements of arrays declared in the scope, local or dummy,
//!   that are neither POINTER nor TARGET. Every reference to a written array
//!   is an element whose subscript in one position is the loop index, or the
//!   index plus or minus a constant, the same in every reference: each
//!   iteration then has its own slice of the array;
//! - writes to scalars only for unsaved local scalars, inner loop indices
//!   among them, that each iteration defines before it reads them.
//!
//! The loop index and those scalars must not be read after the loop, nor in
//! a later iteration of an enclosing loop, before they are defined again:
//! DO CONCURRENT leaves them unchanged, where the DO loop leaves the values
//! of its last iteration. The scope must have no GO TO, COMMON or
//! EQUIVALENCE. Perfectly nested independent loops are suggested as one
//! construct. The scalars appear in a LOCAL clause, which needs Fortran
//! 2018; with `-std=f2008` they are left with unspecified locality, which
//! the same proof makes valid, and before Fortran 2008 the warning is off.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "could-be-concurrent";

struct Report {
    /// The offset of the outermost DO statement.
    at: usize,
    /// The offsets of the DO statements of the construct, outermost first.
    loops: Vec<usize>,
    locals: Vec<String>,
}

/// A counted DO loop: `do var = start, end [, step]`.
struct Loop<'a> {
    var: &'a str,
    bounds: Vec<&'a Expr>,
    body: &'a [Stmt],
}

fn counted(s: &Stmt) -> Option<Loop<'_>> {
    match s {
        Stmt::Do { var, start, end, body } => Some(Loop { var, bounds: vec![start, end], body }),
        Stmt::Block { body } => match body.first() {
            Some(Stmt::CallSub { name, args }) if name == "@do_step" && args.len() == 4 => {
                let Expr::Ident(var) = &args[0] else { return None };
                Some(Loop { var, bounds: args[1..].iter().collect(), body: &body[1..] })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Statements that repeat their body: a later iteration follows the end of it.
fn repeats(s: &Stmt) -> bool {
    counted(s).is_some() || matches!(s, Stmt::DoWhile { .. })
        || matches!(s, Stmt::Block { body } if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name == "@do_concurrent" || name == "@forall"))
}

fn bodies(s: &Stmt) -> Vec<&[Stmt]> {
    match s {
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => vec![body],
        Stmt::If { then_body, else_body, .. } => std::iter::once(then_body.as_slice()).chain(else_body.as_deref()).collect(),
        Stmt::SelectCase { cases, default, .. } => cases.iter().map(|c| c.body.as_slice()).chain(default.as_deref()).collect(),
        _ => vec![],
    }
}

fn mentions(s: &Stmt, n: &str) -> bool { crate::leak_warnings::mentions(s, n) }
fn names(e: &Expr, n: &str) -> bool { crate::allocatable_warnings::names(e, n) }

/// The names a scope declares, and which of them a loop may define.
struct Scope<'a> {
    /// Unsaved local scalars, which may be LOCAL to a construct.
    scalars: HashSet<String>,
    integers: HashSet<String>,
    /// Arrays of the scope, local or dummy, that are neither POINTER nor TARGET.
    arrays: HashSet<String>,
    declared: HashSet<String>,
    /// Array names of the whole program, of modules among them.
    all_arrays: &'a HashSet<String>,
    /// PURE functions of modules and of the top level.
    pure: &'a HashSet<String>,
}

impl<'a> Scope<'a> {
    fn new(body: &[Stmt], params: &[String], result: Option<&str>, all_arrays: &'a HashSet<String>, pure: &'a HashSet<String>) -> Option<Self> {
        let mut scope = Scope { scalars: HashSet::new(), integers: HashSet::new(), arrays: HashSet::new(), declared: HashSet::new(), all_arrays, pure };
        // Scalars that keep or share their values: dummies, the result,
        // saved, initialized or aliased variables.
        let mut kept: HashSet<String> = params.iter().cloned().collect();
        kept.extend(result.map(str::to_string));
        // Arrays a pointer may designate.
        let mut aliased = HashSet::new();
        let mut contained = vec![];
        for s in body {
            match s {
                Stmt::VarDecl { names, kind } => {
                    for n in names {
                        scope.declared.insert(n.clone());
                        scope.scalars.insert(n.clone());
                        if matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)) { scope.integers.insert(n.clone()); }
                    }
                }
                Stmt::ArrayDecl { name, .. } => { scope.declared.insert(name.clone()); scope.arrays.insert(name.clone()); }
                Stmt::InitializedDecl { entities, .. } => for (n, _) in entities { scope.declared.insert(n.clone()); kept.insert(n.clone()); },
                Stmt::DerivedDecl { names, .. } => for n in names { scope.declared.insert(n.clone()); kept.insert(n.clone()); },
                Stmt::DummyIntent { name, intent } => {
                    if name == crate::data_statements::SAVE_ALL { return None; }
                    match intent.as_str() {
                        "pointer" | "target" => { aliased.insert(name.clone()); kept.insert(name.clone()); }
                        "save" | "allocatable" | "external" | "intrinsic" => { kept.insert(name.clone()); }
                        i if i == crate::data_statements::SAVED_STORAGE => { kept.insert(name.clone()); }
                        _ => {}
                    }
                    scope.declared.insert(name.clone());
                }
                Stmt::CallSub { name, .. } if name == crate::common::STATEMENT || name == crate::equivalence::STATEMENT
                    || name.starts_with("@data") => return None,
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    scope.declared.insert(name.clone());
                    contained.push(body);
                }
                _ => {}
            }
        }
        // An internal procedure may read a scalar of its host at any call.
        scope.scalars.retain(|n| !kept.contains(n) && !scope.arrays.contains(n)
            && !contained.iter().any(|b| b.iter().any(|s| mentions(s, n))));
        scope.arrays.retain(|n| !aliased.contains(n));
        Some(scope)
    }
}

/// What one candidate loop's body does.
#[derive(Default)]
struct Audit {
    /// Subscripts of each array element written, and of each element read.
    writes: HashMap<String, Vec<Vec<Expr>>>,
    reads: HashMap<String, Vec<Vec<Expr>>>,
    /// Arrays referenced whole, by section or through a component.
    whole: HashSet<String>,
    /// Scalars the body defines, in order, and those it may read first.
    defined: Vec<String>,
    read_first: HashSet<String>,
}

impl Audit {
    fn expr(&mut self, e: &Expr, known: &HashSet<String>, scope: &Scope) -> bool {
        match e {
            Expr::Str(_) | Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) => true,
            Expr::Ident(n) => {
                if scope.arrays.contains(n) { self.whole.insert(n.clone()); }
                if !known.contains(n) { self.read_first.insert(n.clone()); }
                true
            }
            Expr::Bin(_, a, b) => self.expr(a, known, scope) && self.expr(b, known, scope),
            Expr::Un(_, a) => self.expr(a, known, scope),
            Expr::Section { name, lower, upper, stride } => {
                if !name.is_empty() {
                    if scope.arrays.contains(name) { self.whole.insert(name.clone()); }
                    if !known.contains(name) { self.read_first.insert(name.clone()); }
                }
                [lower, upper, stride].into_iter().flatten().all(|e| self.expr(e, known, scope))
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if !args.iter().all(|a| self.expr(a, known, scope)) { return false; }
                let root = n.split('%').next().unwrap_or(n);
                if n.contains('%') {
                    // A component: of a scope array, it is not one element.
                    if scope.arrays.contains(root) { self.whole.insert(root.to_string()); }
                    return scope.declared.contains(root) || scope.all_arrays.contains(root);
                }
                if scope.arrays.contains(n) {
                    if args.iter().any(|a| matches!(a, Expr::Section { .. })) { self.whole.insert(n.clone()); }
                    else { self.reads.entry(n.clone()).or_default().push(args.clone()); }
                    return true;
                }
                if scope.declared.contains(n) {
                    // A substring of a scalar, or an array declared without
                    // a writable role here.
                    if scope.scalars.contains(n) && !known.contains(n) { self.read_first.insert(n.clone()); }
                    return !body_procedure(n, scope);
                }
                if scope.all_arrays.contains(n) && !scope.pure.contains(n) { return true; }
                scope.pure.contains(n) || matches!(crate::intrinsics::purity(n), Some(crate::intrinsics::Purity::Pure))
            }
        }
    }

    /// Whether the statements are within the subset, tracking the scalars
    /// defined on every path so far. `depth` counts inner loops.
    fn statements(&mut self, body: &[Stmt], depth: usize, known: &mut HashSet<String>, scope: &Scope) -> bool {
        for s in body {
            let ok = match s {
                Stmt::SourceLocation(_) => true,
                Stmt::Cycle => true,
                Stmt::Exit => depth > 0,
                Stmt::Assign { name, value } => {
                    if !scope.scalars.contains(name) || !self.expr(value, known, scope) { return false; }
                    self.defined.push(name.clone());
                    known.insert(name.clone());
                    true
                }
                Stmt::AssignIndex { name, indices, value } => {
                    if !scope.arrays.contains(name) || indices.iter().any(|i| matches!(i, Expr::Section { .. })) { return false; }
                    if !indices.iter().all(|i| self.expr(i, known, scope)) || !self.expr(value, known, scope) { return false; }
                    self.writes.entry(name.clone()).or_default().push(indices.clone());
                    true
                }
                Stmt::If { cond, then_body, else_body } => {
                    if !self.expr(cond, known, scope) { return false; }
                    let mut then_known = known.clone();
                    let mut else_known = known.clone();
                    if !self.statements(then_body, depth, &mut then_known, scope) { return false; }
                    if let Some(b) = else_body { if !self.statements(b, depth, &mut else_known, scope) { return false; } }
                    *known = then_known.intersection(&else_known).cloned().collect();
                    true
                }
                Stmt::SelectCase { expr, cases, default } => {
                    if !self.expr(expr, known, scope) { return false; }
                    let mut all: Option<HashSet<String>> = default.is_none().then(|| known.clone());
                    for b in cases.iter().map(|c| &c.body).chain(default.as_ref()) {
                        let mut k = known.clone();
                        if !self.statements(b, depth, &mut k, scope) { return false; }
                        all = Some(match all { Some(a) => a.intersection(&k).cloned().collect(), None => k });
                    }
                    *known = all.unwrap_or_else(|| known.clone());
                    true
                }
                _ => match counted(s) {
                    Some(inner) => {
                        if !scope.scalars.contains(inner.var) || !inner.bounds.iter().all(|e| self.expr(e, known, scope)) { return false; }
                        self.defined.push(inner.var.to_string());
                        let mut k = known.clone();
                        k.insert(inner.var.to_string());
                        if !self.statements(inner.body, depth + 1, &mut k, scope) { return false; }
                        known.insert(inner.var.to_string());
                        true
                    }
                    None => false,
                },
            };
            if !ok { return false; }
        }
        true
    }
}

/// Whether `n` names a procedure of the scope itself, which could see its
/// variables.
fn body_procedure(n: &str, scope: &Scope) -> bool {
    scope.declared.contains(n) && !scope.scalars.contains(n) && !scope.arrays.contains(n) && !scope.all_arrays.contains(n)
}

/// `var`, or `var` plus or minus a constant.
fn owned(e: &Expr, var: &str) -> bool {
    match e {
        Expr::Ident(n) => n == var,
        Expr::Bin(BinOp::Add | BinOp::Sub, a, b) => matches!((a.as_ref(), b.as_ref()), (Expr::Ident(n), Expr::IntLit(_)) if n == var)
            || matches!(e, Expr::Bin(BinOp::Add, _, _)) && matches!((a.as_ref(), b.as_ref()), (Expr::IntLit(_), Expr::Ident(n)) if n == var),
        _ => false,
    }
}

/// Whether every element referenced of each written array lies in the
/// current iteration's slice.
fn independent(audit: &Audit, var: &str) -> bool {
    if audit.writes.is_empty() { return false; }
    audit.writes.iter().all(|(array, writes)| {
        if audit.whole.contains(array) { return false; }
        let all: Vec<&Vec<Expr>> = writes.iter().chain(audit.reads.get(array).into_iter().flatten()).collect();
        let rank = writes[0].len();
        if all.iter().any(|t| t.len() != rank) { return false; }
        (0..rank).any(|p| owned(&writes[0][p], var)
            && all.iter().all(|t| format!("{:?}", t[p]) == format!("{:?}", writes[0][p])))
    })
}

#[derive(PartialEq)]
enum Event { Defined, Read }

/// Whether `s` may transfer control out of itself other than by its end
/// or RETURN: a later definition may then be skipped.
fn jumps(s: &Stmt, depth: usize) -> bool {
    match s {
        Stmt::Cycle | Stmt::Exit => depth == 0,
        Stmt::CallSub { name, .. } => name == "@named_exit" || name == "@named_cycle",
        _ => {
            let inner = depth + usize::from(repeats(s));
            bodies(s).into_iter().flatten().any(|s| jumps(s, inner))
        }
    }
}

/// The first effect of `s` on `n`: a definition, a reference, or neither
/// for certain. A definition counts only when every path through `s` makes it.
fn first_event(s: &Stmt, n: &str) -> Option<Event> {
    if jumps(s, 0) { return Some(Event::Read); }
    let header = |e: &Expr| names(e, n).then_some(Event::Read);
    // The paths through the bodies of a statement: a reference on any of
    // them is one, and a definition needs all of them.
    let paths = |bs: Vec<&[Stmt]>, total: bool| -> Option<Event> {
        let events: Vec<Option<Event>> = bs.iter().map(|b| scan(b, n)).collect();
        if events.iter().any(|e| *e == Some(Event::Read)) { return Some(Event::Read); }
        (total && !events.is_empty() && events.iter().all(|e| *e == Some(Event::Defined))).then_some(Event::Defined)
    };
    match s {
        Stmt::SourceLocation(_) => None,
        Stmt::Assign { name, value } if name == n => Some(if names(value, n) { Event::Read } else { Event::Defined }),
        Stmt::If { cond, then_body, else_body } => header(cond).or_else(|| {
            paths(std::iter::once(then_body.as_slice()).chain(else_body.as_deref()).collect(), else_body.is_some())
        }),
        Stmt::SelectCase { expr, cases, default } => header(expr).or_else(|| {
            if cases.iter().any(|c| format!("{:?}", c.items).contains(&format!("{n:?}"))) { return Some(Event::Read); }
            paths(cases.iter().map(|c| c.body.as_slice()).chain(default.as_deref()).collect(), default.is_some())
        }),
        // A loop may run no iterations: only a reference counts.
        Stmt::DoWhile { cond, body } => header(cond).or_else(|| (scan(body, n) == Some(Event::Read)).then_some(Event::Read)),
        _ => match counted(s) {
            Some(l) if l.bounds.iter().any(|e| names(e, n)) => Some(Event::Read),
            Some(l) if l.var == n => Some(Event::Defined),
            Some(l) => (scan(l.body, n) == Some(Event::Read)).then_some(Event::Read),
            None => match s {
                // A named loop, or another block of statements run in order.
                Stmt::Block { body } if match body.first() {
                    Some(Stmt::CallSub { name, .. }) => name == "@named_loop" || !name.starts_with('@'),
                    _ => true,
                } => {
                    // A BLOCK's own `n` is another variable.
                    let shadows = body.iter().any(|s| matches!(s, Stmt::VarDecl { names, .. } if names.iter().any(|m| m == n))
                        || matches!(s, Stmt::ArrayDecl { name, .. } if name == n));
                    if shadows { mentions(s, n).then_some(Event::Read) } else { scan(body, n) }
                }
                _ => mentions(s, n).then_some(Event::Read),
            },
        },
    }
}

fn scan(body: &[Stmt], n: &str) -> Option<Event> {
    body.iter().find_map(|s| first_event(s, n))
}

/// Whether `s` mentions `n` outside the statement `except`.
fn mentions_except(s: &Stmt, n: &str, except: *const Stmt) -> bool {
    if std::ptr::eq(s, except) { return false; }
    let mut own = s.clone();
    for b in crate::procedure_arguments::children(&mut own) { b.clear(); }
    mentions(&own, n) || bodies(s).into_iter().flatten().any(|s| mentions_except(s, n, except))
}

/// Whether `n` may be read after the loop at the end of `frames`, before it
/// is defined again. Each frame is a body and the position in it of the
/// statement that leads to the loop.
fn live_after(frames: &[(&[Stmt], usize)], n: &str) -> bool {
    let candidate: *const Stmt = { let (b, i) = frames[frames.len() - 1]; &b[i] };
    for k in (0..frames.len()).rev() {
        let (body, i) = frames[k];
        match scan(&body[i + 1..], n) {
            Some(Event::Defined) => return false,
            Some(Event::Read) => return true,
            None => {}
        }
        if k == 0 { break; }
        let (parent, j) = frames[k - 1];
        let enclosing = &parent[j];
        if repeats(enclosing) {
            if let Stmt::DoWhile { cond, .. } = enclosing { if names(cond, n) { return true; } }
            match scan(&body[..i], n) {
                Some(Event::Defined) => return false,
                Some(Event::Read) => return true,
                None => {}
            }
            if mentions_except(&body[i], n, candidate) { return true; }
        }
    }
    false
}

struct Finder<'a, 'b> {
    scope: &'b Scope<'a>,
    reports: Vec<Report>,
}

fn location(frames: &[(&[Stmt], usize)]) -> Option<usize> {
    frames.iter().rev().find_map(|(b, i)| b[..*i].iter().rev().find_map(|s| match s { Stmt::SourceLocation(n) => Some(*n), _ => None }))
}

impl<'a, 'b> Finder<'a, 'b> {
    /// The proof for one loop, given its frames: the locals it needs.
    fn prove(&self, frames: &[(&[Stmt], usize)], l: &Loop) -> Option<Vec<String>> {
        if !self.scope.integers.contains(l.var) || !self.scope.scalars.contains(l.var) { return None; }
        let mut audit = Audit::default();
        let mut known: HashSet<String> = [l.var.to_string()].into();
        if !l.bounds.iter().all(|e| audit.expr(e, &HashSet::new(), self.scope)) { return None; }
        audit.read_first.clear();
        audit.whole.clear();
        audit.reads.clear();
        if !audit.statements(l.body, 0, &mut known, self.scope) { return None; }
        if !independent(&audit, l.var) { return None; }
        let mut locals = vec![];
        for n in &audit.defined {
            if n == l.var || audit.read_first.contains(n) { return None; }
            if !locals.contains(n) { locals.push(n.clone()); }
        }
        if std::iter::once(l.var).chain(locals.iter().map(String::as_str)).any(|n| live_after(frames, n)) { return None; }
        Some(locals)
    }

    fn visit(&mut self, body: &'a [Stmt], frames: &mut Vec<(&'a [Stmt], usize)>) {
        for (i, s) in body.iter().enumerate() {
            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) { continue; }
            frames.push((body, i));
            if let Some(l) = counted(s) {
                if let Some(mut locals) = self.prove(frames, &l) {
                    let mut loops = vec![location(frames)];
                    // Perfectly nested independent loops form one construct.
                    let mut outer = l;
                    let mut indices = vec![outer.var.to_string()];
                    let mut depth = 0;
                    loop {
                        let statements: Vec<(usize, &Stmt)> = outer.body.iter().enumerate().filter(|(_, s)| !matches!(s, Stmt::SourceLocation(_))).collect();
                        let [(j, inner)] = statements.as_slice() else { break };
                        let Some(next) = counted(inner) else { break };
                        if next.bounds.iter().any(|e| indices.iter().chain(&locals).any(|n| names(e, n) && n != next.var)) { break; }
                        frames.push((outer.body, *j));
                        depth += 1;
                        match self.prove(frames, &next) {
                            Some(more) => {
                                loops.push(location(frames));
                                locals.retain(|n| n != next.var);
                                for n in more { if !locals.contains(&n) { locals.push(n); } }
                                indices.push(next.var.to_string());
                                outer = next;
                            }
                            None => break,
                        }
                    }
                    for _ in 0..depth { frames.pop(); }
                    if let Some(loops) = loops.into_iter().collect::<Option<Vec<_>>>() {
                        locals.retain(|n| !indices.contains(n));
                        self.reports.push(Report { at: loops[0], loops, locals });
                    }
                    frames.pop();
                    continue;
                }
            }
            for b in bodies(s) { self.visit(b, frames); }
            frames.pop();
        }
    }
}

fn unit(body: &[Stmt], params: &[String], result: Option<&str>, all_arrays: &HashSet<String>, pure: &HashSet<String>, out: &mut Vec<Report>) {
    if !crate::leak_warnings::gotos(body) {
        if let Some(scope) = Scope::new(body, params, result, all_arrays, pure) {
            let mut finder = Finder { scope: &scope, reports: vec![] };
            finder.visit(body, &mut vec![]);
            out.extend(finder.reports);
        }
    }
    for s in body {
        if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
            let result = matches!(s, Stmt::Function { .. }).then(|| function_result(name, body));
            unit(body, params, result.as_deref(), all_arrays, pure, out);
        }
    }
}

fn function_result(name: &str, body: &[Stmt]) -> String {
    match body.last() { Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(), _ => name.to_string() }
}

/// PURE functions of modules and of the top level, which cannot see the
/// local variables of other scopes. Generic names are left out.
fn pure_functions(program: &Program) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut generic = HashSet::new();
    let add = |s: &Stmt, found: &mut HashSet<String>| if let Stmt::Function { name, pure: true, .. } = s { found.insert(name.clone()); };
    for s in &program.body {
        // Internal procedures of the main program are among its statements.
        if !matches!(s, Stmt::Function { name, .. } if program.contains.contains(name)) { add(s, &mut found); }
        if let Stmt::Module { body, .. } = s {
            for s in body {
                add(s, &mut found);
                if let Stmt::GenericInterface { name, .. } = s { generic.insert(name.clone()); }
            }
        }
        if let Stmt::GenericInterface { name, .. } = s { generic.insert(name.clone()); }
    }
    found.retain(|n| !generic.contains(n));
    found
}

fn analyze(program: &Program) -> Vec<Report> {
    let mut all_arrays = HashSet::new();
    crate::allocatable_warnings::arrays(&program.body, &mut all_arrays);
    let pure = pure_functions(program);
    let mut out = vec![];
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    unit(&main, &[], None, &all_arrays, &pure, &mut out);
    for s in &program.body {
        match s {
            Stmt::Module { body, .. } => {
                for s in body {
                    if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
                        let result = matches!(s, Stmt::Function { .. }).then(|| function_result(name, body));
                        unit(body, params, result.as_deref(), &all_arrays, &pure, &mut out);
                    }
                }
            }
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } if external(s) => {
                let result = matches!(s, Stmt::Function { .. }).then(|| function_result(name, body));
                unit(body, params, result.as_deref(), &all_arrays, &pure, &mut out);
            }
            _ => {}
        }
    }
    out.sort_by_key(|r| r.at);
    out
}

/// `i = 1:n` for the DO statement at `at`, from its source text.
fn control(tokens: &[Token], src: &str, at: usize) -> Option<String> {
    let ts = crate::declaration_warnings::statement_tokens(tokens, at, usize::MAX, src);
    let d = ts.iter().position(|t| matches!(t.kind, TokenKind::KwDo))?;
    let var = match &ts.get(d + 1)?.kind { TokenKind::Ident(n) => n.clone(), _ => return None };
    if !matches!(ts.get(d + 2)?.kind, TokenKind::Eq) { return None; }
    let mut parts = vec![];
    let (mut depth, mut start) = (0, d + 3);
    for (k, t) in ts.iter().enumerate().skip(d + 3) {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { parts.push((start, k)); start = k + 1; }
            _ => {}
        }
    }
    parts.push((start, ts.len()));
    let text: Vec<&str> = parts.iter().map(|&(a, b)| (a < b).then(|| &src[ts[a].span.start..ts[b - 1].span.end])).collect::<Option<_>>()?;
    if !(2..=3).contains(&text.len()) { return None; }
    Some(format!("{var} = {}", text.join(":")))
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
    standard: Option<crate::standards::Standard>,
) -> Result<()> {
    if standard.is_some_and(|s| s < "f2008".parse().unwrap()) { return Ok(()); }
    let locality = standard.is_none_or(|s| s >= "f2018".parse().unwrap());
    let mut extra_only = settings.clone();
    extra_only.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &extra_only, options);
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let warnings = analyze(program).into_iter().filter(|r| r.at < own_len).filter_map(|r| {
        let controls: Vec<String> = r.loops.iter().map(|&at| control(&tokens, src, at)).collect::<Option<_>>()?;
        let ts = crate::declaration_warnings::statement_tokens(&tokens, r.at, usize::MAX, src);
        let pos = ts.iter().find(|t| matches!(t.kind, TokenKind::KwDo)).or(ts.first()).map_or(r.at, |t| t.span.start);
        let mut header = format!("do concurrent ({})", controls.join(", "));
        if locality && !r.locals.is_empty() { header += &format!(" local({})", r.locals.join(", ")); }
        let what = if r.loops.len() > 1 { "these nested DO loops" } else { "this DO loop" };
        let closing = match r.loops.len() {
            1 => String::new(),
            2 => " in place of the 2 DO statements, and delete the inner END DO".to_string(),
            n => format!(" in place of the {n} DO statements, and delete the inner END DO statements"),
        };
        Some((pos, format!("the iterations of {what} are independent, so it could be DO CONCURRENT [{CATEGORY}]"),
            vec![format!("write `{header}`{closing}; rfort can then run the iterations in parallel")]))
    }).collect();
    if crate::open_warnings::emit(src, own_len, filename, origins, &tokens, error, warnings)? {
        bail!("{CATEGORY} warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn reports(src: &str) -> Vec<(Vec<usize>, Vec<String>)> {
        let tokens = crate::lexer::lex(src);
        let program = crate::parser::parse_with_src(src, &tokens, "t.f90").unwrap_or_else(|e| panic!("{src}: {e:?}"));
        super::analyze(&program).into_iter().map(|r| (r.loops, r.locals)).collect()
    }
    fn count(src: &str) -> usize { reports(src).len() }

    #[test]
    fn reported() {
        for src in [
            // An element per iteration, a temporary defined first, an
            // elemental intrinsic, reads of other arrays at any element.
            "integer :: i, n\nreal :: a(10), b(10), t\nn = 10\nb = 1\ndo i = 1, n\nt = 2*b(i)\na(i) = t + sqrt(b(n+1-i))\nend do\nprint *, a\nend\n",
            // A column per iteration, with an inner loop over its rows.
            "subroutine s(a, n)\ninteger, intent(in) :: n\nreal, intent(inout) :: a(n, n)\ninteger :: i, j\ndo j = 1, n\ndo i = 2, n\na(i, j) = a(i-1, j) + 1\nend do\nend do\nend subroutine\n",
            // The index reused by a later loop, an offset, IF and CYCLE.
            "integer :: i\nreal :: a(0:10)\ndo i = 1, 10\nif (i == 5) cycle\na(i-1) = i\nend do\ndo i = 1, 3\nprint *, a(i)\nend do\nend\n",
        ] {
            assert_eq!(count(src), 1, "{src}: {:?}", reports(src));
        }
        // The index and the temporary are defined again inside a later named
        // loop, on both branches of an IF; that loop is independent too.
        let src = "integer :: i, j\nreal :: a(10), c(3, 2), t\nlogical :: q\nq = .true.\ndo i = 1, 10\nt = i\na(i) = t\nend do\nrows: do j = 1, 2\nif (q) then\nt = 1\nelse\nt = 2\nend if\ndo i = 1, 3\nc(i, j) = t\nend do\nend do rows\nprint *, a, c\nend\n";
        assert_eq!(count(src), 2, "{:?}", reports(src));
        // Nested independent loops become one construct, with the local.
        let src = "integer :: i, j\nreal :: a(3, 4), t\ndo j = 1, 4\ndo i = 1, 3\nt = i + j\na(i, j) = t*t\nend do\nend do\nprint *, a\nend\n";
        let r = reports(src);
        assert_eq!(r.len(), 1, "{r:?}");
        assert_eq!(r[0].0.len(), 2);
        assert_eq!(r[0].1, vec!["t".to_string()]);
    }

    #[test]
    fn not_reported() {
        for src in [
            // A recurrence, a reduction, a scalar read before its definition.
            "integer :: i\nreal :: a(10)\na(1) = 1\ndo i = 2, 10\na(i) = a(i-1) + 1\nend do\nend\n",
            "integer :: i\nreal :: a(10), s\ns = 0\ndo i = 1, 10\na(i) = i\ns = s + a(i)\nend do\nprint *, s\nend\n",
            "integer :: i\nreal :: a(10), t\nt = 0\ndo i = 1, 10\na(i) = t\nt = i\nend do\nend\n",
            // A scalar or the index used after the loop.
            "integer :: i\nreal :: a(10), t\ndo i = 1, 10\nt = i\na(i) = t\nend do\nprint *, t\nend\n",
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\na(i) = i\nend do\nprint *, i\nend\n",
            // Live in the next iteration of an enclosing loop.
            "integer :: i, k\nreal :: a(10), t, b(3)\nt = 0\ndo k = 1, 3\nb(k) = t\ndo i = 1, 10\nt = i\na(i) = t\nend do\nend do\nend\n",
            // A write whose element does not follow the index, or a whole-array read.
            "integer :: i, m(10)\nreal :: a(10)\nm = 1\ndo i = 1, 10\na(m(i)) = i\nend do\nend\n",
            "integer :: i\nreal :: a(10)\na = 0\ndo i = 1, 10\na(i) = sum(a)\nend do\nend\n",
            // I/O, a call, EXIT, an impure or unknown function, a POINTER.
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\na(i) = i\nprint *, i\nend do\nend\n",
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\ncall s(a(i))\nend do\nend\n",
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\na(i) = i\nif (i > 3) exit\nend do\nend\n",
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\na(i) = f(i)\nend do\ncontains\nreal function f(i)\ninteger :: i\nf = i\nend function\nend\n",
            "subroutine s(a)\nreal, pointer :: a(:)\ninteger :: i\ndo i = 1, size(a)\na(i) = i\nend do\nend subroutine\n",
            // A module variable or a dummy scalar written.
            "subroutine s(a, t)\nreal :: a(10), t\ninteger :: i\ndo i = 1, 10\nt = i\na(i) = t\nend do\nend subroutine\n",
            // CYCLE skips the definition after the loop, so the next
            // iteration of the enclosing loop reads the loop's value; a
            // definition on one branch only does not count.
            "integer :: i, k\nreal :: a(10), b(3), t\nlogical :: q\nq = .false.\nt = 0\ndo k = 1, 3\nb(k) = t\ndo i = 1, 10\nt = i\na(i) = t\nend do\nif (q) cycle\nt = 0\nend do\nend\n",
            "integer :: i\nreal :: a(10), t\nlogical :: q\nq = .false.\ndo i = 1, 10\nt = i\na(i) = t\nend do\nif (q) then\nt = 0\nend if\nprint *, t\nend\n",
            // An internal procedure that reads the scalar after the loop.
            "integer :: i\nreal :: a(10), t\ndo i = 1, 10\nt = i\na(i) = t\nend do\ncall show\ncontains\nsubroutine show\nprint *, t\nend subroutine\nend\n",
            // GO TO in the scope.
            "integer :: i\nreal :: a(10)\ndo i = 1, 10\na(i) = i\nend do\ngo to 10\n10 continue\nend\n",
        ] {
            assert_eq!(count(src), 0, "{src}: {:?}", reports(src));
        }
    }
}

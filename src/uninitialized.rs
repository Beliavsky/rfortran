// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wmaybe-uninitialized`, in -Wextra: a local scalar read where some path
//! from the start of its procedure or main program reaches the read without
//! assigning it.
//!
//! The analysis is conservative, so that what it reports is a real path.
//! It considers local scalars of intrinsic type that are not dummy
//! arguments, function results, named constants, initialized, SAVE,
//! POINTER, TARGET, ALLOCATABLE, VOLATILE, ASYNCHRONOUS, in COMMON or
//! EQUIVALENCE, in DATA or NAMELIST, or referenced by a contained procedure.
//! A variable passed by name to a procedure, or named in a READ, ALLOCATE,
//! INQUIRE or other I/O statement, is taken as defined there. Procedures
//! with GO TO, statement labels or I/O branches are not analyzed. After a
//! loop that may run zero times, only what was assigned before it counts.
//! After a loop that runs at least once (a DO loop with constant bounds, or
//! one without loop control), what was assigned on every path that ends it
//! counts: its EXITs, and for a DO loop, the end of its body and its CYCLEs.
use super::*;
use crate::ast::UnOp;

/// Inquiry intrinsics, whose arguments' values are not read.
const INQUIRIES: &[&str] = &[
    "kind", "len", "size", "shape", "present", "allocated", "associated", "lbound", "ubound", "storage_size",
    "bit_size", "digits", "epsilon", "huge", "tiny", "precision", "range", "radix", "minexponent", "maxexponent",
    "new_line", "c_loc", "c_funloc", "c_sizeof", "loc", "sizeof", "rank", "is_contiguous", "same_type_as",
    "extends_type_of", "selected_real_kind", "selected_int_kind", "len_trim",
];

/// The definite-assignment state on a path: the variables assigned on every
/// path here, or None where no path arrives.
type State = Option<HashSet<usize>>;

fn meet(a: State, b: State) -> State {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(a.intersection(&b).copied().collect()),
    }
}

/// Names anywhere in statements, for what contained procedures reference.
fn all_names(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        let mut copy = s.clone();
        for e in crate::procedure_arguments::expressions(&mut copy) {
            crate::procedure_arguments::visit_expr(e, &mut |e| match e {
                Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) | Expr::Section { name: n, .. } => {
                    out.insert(root(n).to_owned());
                }
                _ => {}
            });
        }
        match s {
            Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => { out.insert(root(name).to_owned()); }
            Stmt::Do { var, .. } => { out.insert(var.clone()); }
            _ => {}
        }
        for b in children(s) { all_names(b, out); }
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } = s { all_names(body, out); }
    }
}

/// Whether statements contain GO TO, labels or I/O branches.
fn jumps(body: &[Stmt]) -> bool {
    body.iter().any(|s| {
        matches!(s, Stmt::CallSub { name, .. } if matches!(name.as_str(), "@goto" | "@statement_label"))
            || crate::action_warnings::branches(std::slice::from_ref(s))
            || children(s).into_iter().any(jumps)
    })
}

/// An enclosing loop, or a named construct that EXIT can leave: its name,
/// and the states at the EXITs and CYCLEs that leave or continue it.
struct Target {
    name: Option<String>,
    is_loop: bool,
    exits: State,
    cycles: State,
}

struct Walk {
    /// The candidates of the procedure being analyzed.
    candidates: HashSet<usize>,
    /// Variables assigned somewhere in the procedure.
    assigned: HashSet<usize>,
    reported: HashSet<usize>,
    targets: Vec<Target>,
    /// The name of the loop about to be walked, from its `@named_loop`.
    name: Option<String>,
}

impl Walk {
    /// The target of an EXIT or CYCLE: named, or the innermost loop.
    fn target(&mut self, name: Option<&str>) -> Option<&mut Target> {
        match name {
            Some(n) => self.targets.iter_mut().rev().find(|t| t.name.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(n))),
            None => self.targets.iter_mut().rev().find(|t| t.is_loop),
        }
    }
}

/// The constant value of an integer literal, possibly negated.
fn constant(e: &Expr) -> Option<i64> {
    match e {
        Expr::IntLit(t) => t.split('_').next()?.parse().ok(),
        Expr::Un(UnOp::Neg, x) => constant(x).map(|v| -v),
        _ => None,
    }
}

/// The name in a named EXIT or CYCLE.
fn transfer_name(args: &[Expr]) -> Option<&str> {
    match args.first() { Some(Expr::Str(n)) => Some(n.as_str()), _ => None }
}

impl<'a> Analysis<'a> {
    /// Analyze `body` and the procedures in it. For the main program,
    /// `internal` names its internal procedures; elsewhere every procedure
    /// in `body` is internal.
    pub(super) fn uninitialized(&mut self, body: &'a [Stmt], scope: usize, internal: Option<&[String]>) {
        for s in body {
            if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
                let inner = self.bodies.get(&(body.as_ptr() as usize)).copied().unwrap_or(scope);
                self.uninitialized(body, inner, None);
            }
        }
        if self.scopes[scope].kind == ScopeKind::Module || jumps(body) { return; }
        // Names referenced by contained procedures, which may define them.
        let mut contained = HashSet::new();
        for s in body {
            if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
                if internal.is_none_or(|names| names.contains(name)) { all_names(body, &mut contained); }
            }
        }
        let mut excluded: HashSet<String> = contained;
        for s in body {
            match s {
                Stmt::InitializedDecl { entities, .. } => {
                    excluded.extend(entities.iter().filter(|(_, e)| e.is_some()).map(|(n, _)| n.clone()));
                }
                Stmt::CallSub { name, args } if name.starts_with("@data") || name.starts_with("@namelist") || name.starts_with("@equivalence") => {
                    for a in args {
                        crate::procedure_arguments::visit_expr(&mut a.clone(), &mut |e| {
                            if let Expr::Ident(n) | Expr::Index(n, _) = e { excluded.insert(root(n).to_owned()); }
                        });
                    }
                }
                _ => {}
            }
        }
        let candidates: HashSet<usize> = self.scopes[scope].names.iter()
            .filter(|(n, &id)| !excluded.contains(n.as_str()) && self.ordinary_scalar(id, scope))
            .map(|(_, &id)| id).collect();
        if candidates.is_empty() { return; }
        let mut w = Walk { candidates, assigned: HashSet::new(), reported: HashSet::new(), targets: vec![], name: None };
        self.assignments(body, scope, &mut w);
        self.flow(body, scope, 0, Some(HashSet::new()), &mut w);
    }

    /// Record the candidates assigned anywhere in `body`.
    fn assignments(&self, body: &[Stmt], scope: usize, w: &mut Walk) {
        for s in body {
            if let Stmt::Assign { name, .. } = s {
                if let Some(id) = self.resolve(scope, name) { w.assigned.insert(id); }
            }
            let (_, defs) = self.uses(s, scope);
            w.assigned.extend(defs);
            if let Stmt::Do { var, .. } = s {
                if let Some(id) = self.resolve(scope, var) { w.assigned.insert(id); }
            }
            for b in children(s) { self.assignments(b, scope, w); }
        }
    }

    /// The candidates an expression reads, and those it may define, as
    /// names passed to a procedure or the variable of an implied DO.
    fn expression_uses(&self, e: &Expr, scope: usize, reads: &mut Vec<(usize, String)>, defs: &mut HashSet<usize>) {
        match e {
            Expr::Ident(n) => {
                if let Some(id) = self.resolve(scope, n) { reads.push((id, n.clone())); }
                return;
            }
            Expr::Call(n, args) if INQUIRIES.contains(&n.as_str()) => {
                let _ = args;
                return;
            }
            Expr::Call(n, args) if n.ends_with("implied_do") => {
                if let Some(Expr::Ident(v)) = args.first() {
                    if let Some(id) = self.resolve(scope, v) { defs.insert(id); }
                }
                for a in args.iter().skip(1) { self.expression_uses(a, scope, reads, defs); }
                return;
            }
            Expr::Call(n, args) if !n.starts_with('@') && self.user_call(&Expr::Call(n.clone(), vec![]), scope) => {
                // A user function may define the variables it is passed.
                for a in args { self.argument(a, scope, reads, defs); }
                return;
            }
            // An intrinsic's output passed by keyword, as in `next(a, stat=s)`.
            Expr::Call(n, args) if !n.starts_with('@') => {
                for a in args {
                    match a {
                        Expr::Call(k, inner) if k.starts_with("@keyword:") && matches!(inner.as_slice(), [Expr::Ident(_)]) =>
                            self.argument(a, scope, reads, defs),
                        _ => self.expression_uses(a, scope, reads, defs),
                    }
                }
                return;
            }
            _ => {}
        }
        for x in expressions(e) { self.expression_uses(x, scope, reads, defs); }
    }

    /// An actual argument: a variable passed by name, or by keyword, may be
    /// defined by the procedure.
    fn argument(&self, a: &Expr, scope: usize, reads: &mut Vec<(usize, String)>, defs: &mut HashSet<usize>) {
        match a {
            Expr::Ident(v) => { if let Some(id) = self.resolve(scope, v) { defs.insert(id); } }
            Expr::Call(k, inner) if k.starts_with("@keyword:") => for x in inner { self.argument(x, scope, reads, defs); },
            _ => self.expression_uses(a, scope, reads, defs),
        }
    }
    /// The candidates a statement reads, and those it may define, apart
    /// from an assignment's target.
    fn uses(&self, s: &Stmt, scope: usize) -> (Vec<(usize, String)>, HashSet<usize>) {
        let (mut reads, mut defs) = (vec![], HashSet::new());
        let mut copy = s.clone();
        let exprs = crate::procedure_arguments::expressions(&mut copy);
        match s {
            // Statements that may define what they name: READ, ALLOCATE,
            // INQUIRE, OPEN, internal WRITE and the like.
            Stmt::Read { .. } => {
                for e in exprs { let mut r = vec![]; self.expression_uses(e, scope, &mut r, &mut defs); defs.extend(r.into_iter().map(|(id, _)| id)); }
            }
            Stmt::CallSub { name, .. } if name.starts_with('@') && !matches!(name.as_str(), "@formatted_print" | "@stop" | "@error_stop") => {
                for e in exprs { let mut r = vec![]; self.expression_uses(e, scope, &mut r, &mut defs); defs.extend(r.into_iter().map(|(id, _)| id)); }
            }
            // A subroutine may define the variables it is passed.
            Stmt::CallSub { args, .. } => {
                for a in args { self.argument(a, scope, &mut reads, &mut defs); }
            }
            // Only the construct's own expressions; its bodies are walked.
            Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => self.expression_uses(cond, scope, &mut reads, &mut defs),
            Stmt::Do { start, end, .. } => {
                self.expression_uses(start, scope, &mut reads, &mut defs);
                self.expression_uses(end, scope, &mut reads, &mut defs);
            }
            Stmt::SelectCase { expr, .. } => self.expression_uses(expr, scope, &mut reads, &mut defs),
            Stmt::Block { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
            _ => for e in exprs { self.expression_uses(e, scope, &mut reads, &mut defs); },
        }
        (reads, defs)
    }

    fn check_reads(&mut self, reads: &[(usize, String)], state: &HashSet<usize>, defs: &HashSet<usize>, offset: usize, w: &mut Walk) {
        for (id, name) in reads {
            if !w.candidates.contains(id) || state.contains(id) || defs.contains(id) || !w.reported.insert(*id) { continue; }
            let message = if w.assigned.contains(id) {
                format!("`{name}` may be used before it is assigned")
            } else {
                format!("`{name}` is used but never assigned")
            };
            self.warnings.push(Advice{category:"maybe-uninitialized",span:self.span(offset,name),message,
                related:Some((self.decls[*id].span.clone(),"declared here".into())),suggestion:None,
                note:"some path from the start of the procedure reaches this read without assigning the variable; its value is then undefined"});
        }
    }

    /// Walk statements on the paths from `state`, returning the state after them.
    fn flow(&mut self, body: &'a [Stmt], scope: usize, mut offset: usize, mut state: State, w: &mut Walk) -> State {
        for s in body {
            if let Stmt::SourceLocation(p) = s { offset = *p; continue; }
            if self.dead.contains(&(s as *const Stmt as usize)) { continue; }
            let Some(current) = state.as_ref() else { continue };
            let (reads, defs) = self.uses(s, scope);
            let current = current.clone();
            self.check_reads(&reads, &current, &defs, offset, w);
            let mut next = current.clone();
            next.extend(defs.iter().copied());
            state = match s {
                Stmt::Assign { name, .. } => {
                    if let Some(id) = self.resolve(scope, name) { next.insert(id); }
                    Some(next)
                }
                Stmt::If { then_body, else_body, .. } => {
                    let yes = self.flow(then_body, scope, offset, Some(next.clone()), w);
                    let no = match else_body { Some(b) => self.flow(b, scope, offset, Some(next.clone()), w), None => Some(next) };
                    meet(yes, no)
                }
                Stmt::SelectCase { cases, default, .. } => {
                    let mut out: State = None;
                    for c in cases { let after = self.flow(&c.body, scope, offset, Some(next.clone()), w); out = meet(out, after); }
                    let after = match default { Some(b) => self.flow(b, scope, offset, Some(next.clone()), w), None => Some(next) };
                    meet(out, after)
                }
                Stmt::Block { body } => {
                    match body.first() {
                        // A named loop: the loop that follows takes the name.
                        Some(Stmt::CallSub { name, args }) if name == "@named_loop" => {
                            w.name = transfer_name(args).filter(|n| !n.is_empty()).map(str::to_owned);
                            self.flow(body, scope, offset, Some(next), w)
                        }
                        // A named construct that EXIT may leave.
                        Some(Stmt::CallSub { name, args }) if name == "@named_construct" => {
                            w.targets.push(Target { name: transfer_name(args).map(str::to_owned), is_loop: false, exits: None, cycles: None });
                            let after = self.flow(body, scope, offset, Some(next), w);
                            let t = w.targets.pop().expect("target");
                            meet(after, t.exits)
                        }
                        _ => self.flow(body, scope, offset, Some(next), w),
                    }
                }
                Stmt::Do { var, start, end, body } => {
                    if let Some(id) = self.resolve(scope, var) { next.insert(id); }
                    w.targets.push(Target { name: w.name.take(), is_loop: true, exits: None, cycles: None });
                    let end_state = self.flow(body, scope, offset, Some(next.clone()), w);
                    let t = w.targets.pop().expect("target");
                    // Constant bounds that give at least one iteration.
                    let runs = constant(start).zip(constant(end)).is_some_and(|(a, b)| a <= b);
                    if runs {
                        meet(meet(end_state, t.cycles), t.exits).map(|s| s.union(&next).copied().collect())
                    } else {
                        Some(next)
                    }
                }
                Stmt::DoWhile { cond, body } => {
                    let endless = matches!(cond, Expr::Logical(true));
                    w.targets.push(Target { name: w.name.take(), is_loop: true, exits: None, cycles: None });
                    self.flow(body, scope, offset, Some(next.clone()), w);
                    let t = w.targets.pop().expect("target");
                    // Without loop control, the loop ends only at its EXITs.
                    if endless { t.exits.map(|e| e.union(&next).copied().collect()) } else { Some(next) }
                }
                Stmt::Exit => {
                    if let Some(t) = w.target(None) { t.exits = meet(t.exits.take(), Some(next)); }
                    None
                }
                Stmt::Cycle => {
                    if let Some(t) = w.target(None) { t.cycles = meet(t.cycles.take(), Some(next)); }
                    None
                }
                Stmt::CallSub { name, args } if name == "@named_exit" => {
                    if let Some(t) = w.target(transfer_name(args)) { t.exits = meet(t.exits.take(), Some(next)); }
                    None
                }
                Stmt::CallSub { name, args } if name == "@named_cycle" => {
                    if let Some(t) = w.target(transfer_name(args)) { t.cycles = meet(t.cycles.take(), Some(next)); }
                    None
                }
                Stmt::Return(_) => None,
                Stmt::CallSub { name, .. } if matches!(name.as_str(), "@stop" | "@error_stop") => None,
                _ => Some(next),
            };
        }
        state
    }
}


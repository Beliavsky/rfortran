// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Size relationships between array dummies of unknown size.
//!
//! `-Wunchecked-conformance`, in -Wextra: a procedure that combines two
//! assumed-shape or deferred-shape dummies in a way that requires a size
//! relationship, as `x*y` requires `size(x) == size(y)`, and that never
//! compares their sizes. Elemental operations and intrinsics,
//! `dot_product`, `matmul` and assignment to a non-ALLOCATABLE dummy are
//! considered.
//!
//! `-Wconformance`, in -Wall: a call whose actual arguments, of sizes known
//! at compile time, break such a relationship, whether or not the procedure
//! checks it.
use super::*;

/// `size(a[, da]) == size(b[, db])`, or `shape(a) == shape(b)` when both are
/// rank-2 or more and no dimension is given.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct Requirement {
    a: String,
    da: Option<usize>,
    b: String,
    db: Option<usize>,
    shape: bool,
    /// The operation, for messages, as `x*y` or `matmul(a, b)`.
    operation: String,
}

impl Requirement {
    fn new(a: &str, da: Option<usize>, b: &str, db: Option<usize>, shape: bool, operation: String) -> Self {
        // One order for each pair.
        if (b, db) < (a, da) {
            Requirement { a: b.into(), da: db, b: a.into(), db: da, shape, operation }
        } else {
            Requirement { a: a.into(), da, b: b.into(), db, shape, operation }
        }
    }
    fn side(name: &str, dim: Option<usize>, shape: bool) -> String {
        match (dim, shape) {
            (Some(d), _) => format!("size({name}, {d})"),
            (None, true) => format!("shape({name})"),
            (None, false) => format!("size({name})"),
        }
    }
    fn text(&self) -> String {
        let relation = format!("{} == {}", Self::side(&self.a, self.da, self.shape), Self::side(&self.b, self.db, self.shape));
        if self.shape { format!("all({relation})") } else { relation }
    }
    fn check(&self, procedure: &str) -> String {
        let (a, b) = (Self::side(&self.a, self.da, self.shape), Self::side(&self.b, self.db, self.shape));
        let test = if self.shape { format!("any({a} /= {b})") } else { format!("{a} /= {b}") };
        format!("if ({test}) error stop \"{procedure}: {} and {} do not conform\"", self.a, self.b)
    }
}

/// Elemental intrinsics, whose array arguments must conform.
fn elemental(name: &str) -> bool {
    crate::intrinsics::math_arity(name).is_some()
        || matches!(name, "abs" | "max" | "min" | "merge" | "mod" | "modulo" | "sign" | "real" | "dble" | "int" | "nint"
            | "aimag" | "conjg" | "cmplx" | "dim" | "atan2" | "hypot" | "ieor" | "ior" | "iand")
}

fn operator(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "**",
        BinOp::Eq => " == ",
        BinOp::Ne => " /= ",
        BinOp::Lt => " < ",
        BinOp::Le => " <= ",
        BinOp::Gt => " > ",
        BinOp::Ge => " >= ",
        BinOp::And => " .and. ",
        BinOp::Or => " .or. ",
        BinOp::Eqv => " .eqv. ",
        BinOp::Neqv => " .neqv. ",
        BinOp::Concat => "//",
    }
}

impl<'a> Analysis<'a> {
    /// The rank of dummy `n` of the procedure, if it is an array of unknown
    /// size that this check considers.
    fn unknown_size_rank(&self, scope: usize, n: &str) -> Option<usize> {
        let id = *self.scopes[scope].names.get(n)?;
        let d = &self.decls[id];
        (d.dummy && d.scope == scope && !d.dims.is_empty() && !d.attrs.contains("optional")
            && d.dims.iter().all(|e| matches!(e, Expr::Ident(x) if x == "@deferred_extent")))
            .then_some(d.dims.len())
    }

    /// The dummy whose shape `e` has, recording the requirements its
    /// operations make.
    fn conformance_shape(&self, e: &Expr, scope: usize, out: &mut Vec<Requirement>) -> Option<String> {
        let rank = |n: &str| self.unknown_size_rank(scope, n);
        let same = |x: &Option<String>, y: &Option<String>, operation: String, out: &mut Vec<Requirement>| {
            if let (Some(x), Some(y)) = (x, y) {
                if x != y && rank(x) == rank(y) {
                    out.push(Requirement::new(x, None, y, None, rank(x).unwrap_or(1) > 1, operation));
                }
            }
        };
        match e {
            Expr::Ident(n) if rank(n).is_some() => Some(n.clone()),
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.conformance_shape(a, scope, out), self.conformance_shape(b, scope, out));
                if let (Some(p), Some(q)) = (&x, &y) {
                    same(&x, &y, format!("{p}{}{q}", operator(op)), out);
                }
                x.or(y)
            }
            Expr::Un(_, a) => self.conformance_shape(a, scope, out),
            Expr::Call(k, inner) if k.starts_with("@keyword:") => inner.first().and_then(|x| self.conformance_shape(x, scope, out)),
            Expr::Call(n, args) if !self.user_call(e, scope) && n == "dot_product" && args.len() == 2 => {
                let (x, y) = (self.conformance_shape(&args[0], scope, out), self.conformance_shape(&args[1], scope, out));
                if let (Some(p), Some(q)) = (&x, &y) { same(&x, &y, format!("dot_product({p}, {q})"), out); }
                None
            }
            Expr::Call(n, args) if !self.user_call(e, scope) && n == "matmul" && args.len() == 2 => {
                let (x, y) = (self.conformance_shape(&args[0], scope, out), self.conformance_shape(&args[1], scope, out));
                if let (Some(p), Some(q)) = (&x, &y) {
                    let (dp, dq) = match (rank(p), rank(q)) {
                        (Some(2), Some(2)) => (Some(2), Some(1)),
                        (Some(1), Some(2)) => (None, Some(1)),
                        (Some(2), Some(1)) => (Some(2), None),
                        _ => return None,
                    };
                    out.push(Requirement::new(p, dp, q, dq, false, format!("matmul({p}, {q})")));
                }
                None
            }
            Expr::Call(n, args) if !self.user_call(e, scope) && elemental(n) => {
                let shapes: Vec<Option<String>> = args.iter().map(|x| self.conformance_shape(x, scope, out)).collect();
                let first = shapes.iter().flatten().next().cloned();
                for s in &shapes {
                    if let (Some(p), Some(q)) = (&first, s) {
                        same(&first, s, format!("{n}({p}, {q})"), out);
                    }
                }
                first
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for x in args { self.conformance_shape(x, scope, out); }
                None
            }
            _ => None,
        }
    }

    /// The requirements of statements; the names that each comparison, or
    /// each call, combines with sizes, as `size(x) /= size(y)` or
    /// `call assert_equal(size(x), size(y))`; and variables assigned a size,
    /// as `n = size(x)`.
    fn conformance_body(&self, body: &[Stmt], scope: usize, out: &mut Vec<Requirement>, groups: &mut Vec<Vec<String>>, aliases: &mut HashMap<String, String>) {
        // The arrays whose sizes `e` refers to, and the variables it names.
        fn sized(e: &Expr, names: &mut Vec<String>) {
            crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |y| match y {
                Expr::Call(f, a) if matches!(f.as_str(), "size" | "shape" | "ubound" | "lbound") => {
                    if let Some(Expr::Ident(n)) = a.first() { names.push(n.clone()); }
                }
                // Kept only if it holds a size.
                Expr::Ident(n) => names.push(format!("@ident:{n}")),
                _ => {}
            });
        }
        for s in body {
            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                continue;
            }
            if let Stmt::CallSub { name, args } = s {
                if !name.starts_with('@') {
                    let mut names = vec![];
                    for a in args { sized(a, &mut names); }
                    groups.push(names);
                }
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                self.conformance_shape(e, scope, out);
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| match x {
                    Expr::Bin(BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, _, _) => {
                        let mut names = vec![];
                        sized(x, &mut names);
                        groups.push(names);
                    }
                    Expr::Call(f, a) if !f.starts_with('@') && crate::intrinsics::purity(f).is_none() => {
                        let mut names = vec![];
                        for y in a.iter() { sized(y, &mut names); }
                        groups.push(names);
                    }
                    _ => {}
                });
            }
            if let Stmt::Assign { name, value } = s {
                // A variable that holds a size.
                if let Expr::Call(f, a) = value {
                    if f == "size" {
                        if let [Expr::Ident(x)] = a.as_slice() { aliases.insert(name.clone(), x.clone()); }
                    }
                }
                // Assignment to a dummy that cannot be reallocated.
                if let Some(r) = self.unknown_size_rank(scope, name) {
                    let id = self.scopes[scope].names[name];
                    if !self.decls[id].attrs.contains("allocatable") {
                        if let Some(v) = self.conformance_shape(value, scope, &mut vec![]) {
                            if v != *name && self.unknown_size_rank(scope, &v) == Some(r) {
                                out.push(Requirement::new(name, None, &v, None, r > 1, format!("assigning to `{name}`")));
                            }
                        }
                    }
                }
            }
            for b in children(s) { self.conformance_body(b, scope, out, groups, aliases); }
        }
    }

    /// Each procedure's requirements, and whether it compares the sizes.
    pub(super) fn conformance_requirements(&self) -> Vec<Vec<(Requirement, bool)>> {
        self.procedures.iter().map(|p| {
            let (mut out, mut groups, mut aliases) = (vec![], vec![], HashMap::new());
            self.conformance_body(p.body, p.scope, &mut out, &mut groups, &mut aliases);
            out.sort();
            out.dedup_by(|x, y| (&x.a, x.da, &x.b, x.db) == (&y.a, y.da, &y.b, y.db));
            // Pairs of arrays whose sizes some comparison or call combines.
            let mut compared = HashSet::new();
            for g in &groups {
                let names: Vec<&String> = g.iter().filter_map(|n| match n.strip_prefix("@ident:") {
                    Some(v) => aliases.get(v),
                    None => Some(n),
                }).collect();
                for x in &names {
                    for y in &names { compared.insert(((*x).clone(), (*y).clone())); }
                }
            }
            out.into_iter().map(|r| {
                let checked = compared.contains(&(r.a.clone(), r.b.clone()));
                (r, checked)
            }).collect()
        }).collect()
    }

    /// `-Wunchecked-conformance`.
    pub(super) fn unchecked_conformance(&mut self, requirements: &[Vec<(Requirement, bool)>]) {
        for (p, reqs) in self.procedures.iter().zip(requirements) {
            for (r, checked) in reqs {
                if *checked { continue; }
                let Some(&id) = self.scopes[p.scope].names.get(&r.b) else { continue };
                let span = self.decls[id].span.clone();
                self.warnings.push(Advice {
                    category: "unchecked-conformance", span, related: None,
                    message: format!("{} requires `{}`, which `{}` does not check", if r.operation.starts_with("assigning") { r.operation.clone() } else { format!("`{}`", r.operation) }, r.text(), p.name),
                    suggestion: Some(r.check(&p.name)),
                    note: "arrays that do not conform give undefined results; check the sizes, or declare the dummy with the size it must have, as in `y(size(x))`",
                });
            }
        }
    }

    /// The extents of an actual argument known at compile time.
    fn constant_shape(&self, e: &Expr, scope: usize) -> Option<Vec<i64>> {
        match e {
            Expr::Call(k, inner) if k.starts_with("@keyword:") => self.constant_shape(inner.first()?, scope),
            Expr::Call(n, items) if n == "@array_constructor" => {
                let scalar = |x: &Expr| matches!(x, Expr::IntLit(_) | Expr::RealLit(_) | Expr::Str(_) | Expr::Logical(_))
                    || matches!(x, Expr::Un(_, y) if matches!(**y, Expr::IntLit(_) | Expr::RealLit(_)))
                    || matches!(x, Expr::Ident(v) if self.resolve(scope, v).is_some_and(|id| self.decls[id].dims.is_empty() && self.decls[id].procedure.is_none()));
                items.iter().all(scalar).then(|| vec![items.len() as i64])
            }
            Expr::Ident(n) => {
                let d = &self.decls[self.resolve(scope, n)?];
                if d.dims.is_empty() || d.procedure.is_some() { return None; }
                d.dims.iter().map(|x| self.integer(x, scope, &mut HashSet::new()).filter(|v| *v >= 0)).collect()
            }
            _ => None,
        }
    }

    /// `-Wconformance`: calls whose actuals break a procedure's requirements.
    pub(super) fn conformance_calls(&mut self, body: &'a [Stmt], scope: usize, mut offset: usize, requirements: &[Vec<(Requirement, bool)>]) {
        for s in body {
            if let Stmt::SourceLocation(p) = s { offset = *p; continue; }
            let mut calls: Vec<(String, Vec<Expr>)> = vec![];
            if let Stmt::CallSub { name, args } = s {
                if !name.starts_with('@') { calls.push((name.clone(), args.clone())); }
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| {
                    if let Expr::Call(n, a) = x { if !n.starts_with('@') { calls.push((n.clone(), a.clone())); } }
                });
            }
            for (name, args) in calls {
                let Some(id) = self.resolve(scope, &name) else { continue };
                let Some(p) = self.decls[id].procedure else { continue };
                if requirements[p].is_empty() { continue; }
                let params = self.procedures[p].params.clone();
                // The actual of each dummy, by position or keyword.
                let actual = |dummy: &str| -> Option<&Expr> {
                    if let Some(k) = args.iter().find(|a| matches!(a, Expr::Call(k, _) if k.strip_prefix("@keyword:") == Some(dummy))) {
                        return Some(k);
                    }
                    let i = params.iter().position(|q| q == dummy)?;
                    args.get(i).filter(|a| !matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:")))
                };
                for (r, _) in &requirements[p] {
                    let (Some(x), Some(y)) = (actual(&r.a).and_then(|e| self.constant_shape(e, scope)), actual(&r.b).and_then(|e| self.constant_shape(e, scope))) else { continue };
                    let extent = |s: &[i64], d: Option<usize>| match d { Some(d) => s.get(d - 1).copied(), None => Some(s.iter().product()) };
                    let (Some(m), Some(n)) = (extent(&x, r.da), extent(&y, r.db)) else { continue };
                    let differ = if r.shape { x != y } else { m != n };
                    if !differ { continue; }
                    let passes = if r.shape {
                        let shape = |s: &[i64]| s.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                        format!("arrays of shape [{}] and [{}]", shape(&x), shape(&y))
                    } else if r.da.is_none() && r.db.is_none() {
                        format!("{m} and {n} elements")
                    } else {
                        format!("{} = {m} and {} = {n}", Requirement::side(&r.a, r.da, false), Requirement::side(&r.b, r.db, false))
                    };
                    let procedure = self.procedures[p].name.clone();
                    self.warnings.push(Advice {
                        category: "conformance", span: self.span(offset, &name), related: None,
                        message: format!("`{procedure}` requires `{}`, but this call passes {passes}", r.text()),
                        suggestion: None,
                        note: "the procedure combines these arguments, which must conform; with these sizes the result is undefined",
                    });
                }
            }
            match s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    let inner = self.bodies.get(&(body.as_ptr() as usize)).copied().unwrap_or(scope);
                    self.conformance_calls(body, inner, offset, requirements);
                }
                _ => for b in children(s) { self.conformance_calls(b, scope, offset, requirements); },
            }
        }
    }
}

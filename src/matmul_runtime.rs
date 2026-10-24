// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! MATMUL of ALLOCATABLE, assumed-shape and other runtime-sized arrays.
//!
//! Runtime-sized products and products with section operands are computed
//! before their statement into an ALLOCATABLE
//! temporary of the result's rank, which then replaces the call. The extents
//! come from SIZE and the subscripts start at each argument's LBOUND. The
//! inner extents are checked at run time, and the temporary is deallocated
//! after the statement, so a product inside a loop is reallocated each time.
//! MATMUL of constant-size arrays keeps its fixed-size lowering.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Array {
    kind: TypeSpec,
    rank: usize,
    runtime: bool,
}
type Arrays = HashMap<String, Array>;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn set(name: &str, value: Expr) -> Stmt {
    Stmt::Assign { name: name.into(), value }
}

// Whether a declared extent is known only at run time: a deferred or assumed
// extent, or one that depends on a variable rather than named constants.
fn runtime_extent(e: &Expr, constants: &HashSet<String>) -> bool {
    match e {
        Expr::Ident(n) => n.starts_with('@') || !constants.contains(n),
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(|a| runtime_extent(a, constants)),
        Expr::Bin(_, a, b) => runtime_extent(a, constants) || runtime_extent(b, constants),
        Expr::Un(_, a) => runtime_extent(a, constants),
        _ => false,
    }
}

fn declarations(body: &[Stmt], inherited: &Arrays) -> Arrays {
    let mut arrays = inherited.clone();
    let mut constants = HashSet::new();
    for s in body {
        match s {
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "parameter") => {
                constants.extend(entities.iter().map(|(n, _)| n.clone()));
            }
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    arrays.remove(n);
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::ArrayDecl { kind, name, dims } = s {
            let runtime = dims.iter().any(|d| runtime_extent(d, &constants));
            arrays.insert(name.clone(), Array { kind: kind.clone(), rank: dims.len(), runtime });
        }
    }
    arrays
}

// The type of a mixed product: a REAL operand decides over an INTEGER one,
// and a nondefault kind over the default kind of the same type.
fn result_kind(a: &TypeSpec, b: &TypeSpec) -> Option<TypeSpec> {
    let rank = |t: &TypeSpec| match t {
        TypeSpec::Integer(None | Some(4)) => Some(0),
        TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) => Some(1),
        TypeSpec::Real => Some(2),
        TypeSpec::DoublePrecision | TypeSpec::RealKind(_) => Some(3),
        _ => None,
    };
    Some(if rank(b)? > rank(a)? { b.clone() } else { a.clone() })
}

struct Lowering<'a> {
    arrays: &'a Arrays,
    serial: &'a mut usize,
    declarations: Vec<Stmt>,
    before: Vec<Stmt>,
    after: Vec<Stmt>,
}

impl Lowering<'_> {
    fn product(&mut self, a: &str, b: &str) -> Option<Expr> {
        let (x, y) = (self.arrays.get(a)?, self.arrays.get(b)?);
        if !(x.runtime || y.runtime) || x.rank > 2 || y.rank > 2 || x.rank == 0 || y.rank == 0 || x.rank == 1 && y.rank == 1 {
            return None;
        }
        let kind = result_kind(&x.kind, &y.kind)?;
        let (ra, rb) = (x.rank, y.rank);
        *self.serial += 1;
        let n = *self.serial;
        let name = |part: &str| format!("@matmul_{part}_{n}");
        let (result, sum) = (name("result"), name("sum"));
        let (row, col, k) = (name("row"), name("col"), name("inner"));
        let lowers = [name("lower_a1"), name("lower_a2"), name("lower_b1"), name("lower_b2")];
        let size = |array: &str, dim: usize| call("size", vec![id(array), int(dim as i64)]);
        let rows = if ra == 2 { size(a, 1) } else { int(1) };
        let cols = if rb == 2 { size(b, 2) } else { int(1) };
        let mut extents = vec![];
        if ra == 2 { extents.push(rows.clone()); }
        if rb == 2 { extents.push(cols.clone()); }

        self.declarations.push(Stmt::DummyIntent { name: result.clone(), intent: "allocatable".into() });
        self.declarations.push(Stmt::ArrayDecl {
            kind: kind.clone(),
            name: result.clone(),
            dims: vec![id("@deferred_extent"); extents.len()],
        });
        self.declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![sum.clone()] });
        let mut integers = vec![row.clone(), col.clone(), k.clone()];
        integers.extend(lowers.iter().cloned());
        self.declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: integers });

        self.before.push(Stmt::If {
            cond: bin(BinOp::Ne, size(a, ra), size(b, 1)),
            then_body: vec![Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str("MATMUL operand shapes do not conform".into())] }],
            else_body: None,
        });
        self.before.push(Stmt::CallSub { name: "@allocate".into(), args: vec![call(&result, extents)] });
        for (lower, (array, dim)) in lowers.iter().zip([(a, 1), (a, 2), (b, 1), (b, 2)]) {
            let rank = if array == a { ra } else { rb };
            let value = if dim <= rank { call("lbound", vec![id(array), int(dim as i64)]) } else { int(1) };
            self.before.push(set(lower, value));
        }
        // Subscript `offset` of an axis whose first element is `lower`.
        let at = |lower: &str, offset: &str| bin(BinOp::Sub, bin(BinOp::Add, id(lower), id(offset)), int(1));
        let a_element = if ra == 2 { vec![at(&lowers[0], &row), at(&lowers[1], &k)] } else { vec![at(&lowers[0], &k)] };
        let b_element = if rb == 2 { vec![at(&lowers[2], &k), at(&lowers[3], &col)] } else { vec![at(&lowers[2], &k)] };
        let target = match (ra, rb) { (2, 2) => vec![id(&row), id(&col)], (2, _) => vec![id(&row)], _ => vec![id(&col)] };
        let accumulate = set(&sum, bin(BinOp::Add, id(&sum), bin(BinOp::Mul, call(a, a_element), call(b, b_element))));
        let cell = vec![
            set(&sum, int(0)),
            Stmt::Do { var: k.clone(), start: int(1), end: size(a, ra), body: vec![accumulate] },
            Stmt::AssignIndex { name: result.clone(), indices: target, value: id(&sum) },
        ];
        let column = vec![Stmt::Do { var: row, start: int(1), end: rows, body: cell }];
        self.before.push(Stmt::Do { var: col, start: int(1), end: cols, body: column });
        self.after.push(Stmt::CallSub { name: "@deallocate".into(), args: vec![id(&result)] });
        Some(id(&result))
    }

    // The rank and type of an array expression built from whole arrays or triplet sections with
    // elemental operations, TRANSPOSE and RESHAPE, and whether it has a
    // runtime shape.
    fn shape(&self, e: &Expr) -> Option<(usize, TypeSpec, bool)> {
        match e {
            Expr::Ident(n) => self.arrays.get(n).map(|a| (a.rank, a.kind.clone(), a.runtime)),
            Expr::Section { name, .. } => self.arrays.get(name)
                .filter(|a| a.rank == 1).map(|a| (1, a.kind.clone(), true)),
            Expr::Call(n, args) | Expr::Index(n, args) if self.arrays.contains_key(n) => {
                let array = &self.arrays[n];
                if args.len() != array.rank { return None; }
                let rank = args.iter().filter(|e| matches!(e, Expr::Section { .. })
                    || matches!(e, Expr::Call(tag, _) if tag == "@section_subscript")).count();
                (rank > 0).then(|| (rank, array.kind.clone(), true))
            }
            Expr::Bin(op, a, b) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) => {
                match (self.shape(a), self.shape(b)) {
                    (Some(x), Some(y)) => (x.0 == y.0).then(|| Some((x.0, result_kind(&x.1, &y.1)?, x.2 || y.2))).flatten(),
                    (Some(x), None) | (None, Some(x)) => Some(x),
                    (None, None) => None,
                }
            }
            Expr::Un(_, a) => self.shape(a),
            Expr::Call(n, args) if n == "transpose" && args.len() == 1 => self.shape(&args[0]).filter(|s| s.0 == 2),
            Expr::Call(n, args) if matches!(n.as_str(), "abs" | "sqrt" | "exp" | "log" | "sin" | "cos") && args.len() == 1 => self.shape(&args[0]),
            Expr::Call(n, args) if n == "reshape" && !self.arrays.contains_key(n) => {
                let Some(Expr::Call(tag, extents)) = args.get(1) else { return None };
                let (_, kind, runtime) = self.shape(&args[0])?;
                (tag == "@array_constructor").then(|| (extents.len(), kind, runtime))
            }
            _ => None,
        }
    }

    // The extents of an expression accepted by `shape`.
    fn extents(&self, e: &Expr) -> Option<Vec<Expr>> {
        match e {
            Expr::Ident(n) => Some((1..=self.arrays.get(n)?.rank).map(|d| call("size", vec![id(n), int(d as i64)])).collect()),
            Expr::Section { .. } => Some(vec![call("size", vec![e.clone(), int(1)])]),
            Expr::Call(n, _) | Expr::Index(n, _) if self.arrays.contains_key(n) =>
                Some((1..=self.shape(e)?.0).map(|d| call("size", vec![e.clone(), int(d as i64)])).collect()),
            Expr::Bin(_, a, b) => match (self.shape(a), self.shape(b)) {
                (Some(x), _) if x.2 || self.shape(b).is_none() => self.extents(a),
                (_, Some(_)) => self.extents(b),
                _ => self.extents(a),
            },
            Expr::Un(_, a) => self.extents(a),
            Expr::Call(n, args) if n == "transpose" => {
                let mut extents = self.extents(&args[0])?;
                extents.reverse();
                Some(extents)
            }
            Expr::Call(n, args) if n == "reshape" => match args.get(1) {
                Some(Expr::Call(_, extents)) => Some(extents.clone()),
                _ => None,
            },
            Expr::Call(_, args) => self.extents(args.first()?),
            _ => None,
        }
    }

    fn capture_integer(&mut self, e: &mut Expr) {
        if crate::parameters::integer_constant(e).is_ok() { return; }
        *self.serial += 1;
        let name = format!("@matmul_selector_{}", self.serial);
        self.declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![name.clone()] });
        self.before.push(set(&name, e.clone()));
        *e = id(&name);
    }

    fn capture_selectors(&mut self, e: &mut Expr) {
        match e {
            Expr::Section { lower, upper, stride, .. } => {
                for bound in [lower, upper, stride].into_iter().flatten() { self.capture_integer(bound); }
            }
            Expr::Call(n, args) | Expr::Index(n, args) if self.arrays.contains_key(n) => {
                for arg in args {
                    if matches!(arg, Expr::Section { .. }) {
                        self.capture_selectors(arg);
                    } else if let Expr::Call(tag, items) = arg {
                        if tag == "@section_subscript" {
                            for item in items { self.capture_selectors(item); }
                        } else { self.capture_integer(arg); }
                    } else { self.capture_integer(arg); }
                }
            }
            Expr::Call(_, args) | Expr::Index(_, args) => for arg in args { self.capture_selectors(arg); },
            Expr::Bin(_, a, b) => { self.capture_selectors(a); self.capture_selectors(b); }
            Expr::Un(_, a) => self.capture_selectors(a),
            _ => {}
        }
    }

    // A whole-array argument, or an expression assigned to an ALLOCATABLE
    // temporary that the product then reads.
    fn operand(&mut self, e: &Expr) -> Option<(String, Array)> {
        if let Expr::Ident(n) = e {
            return self.arrays.get(n).map(|a| (n.clone(), a.clone()));
        }
        let (rank, kind, runtime) = self.shape(e)?;
        // Extent inquiries and the copy must use the same evaluated selectors.
        let mut captured = e.clone();
        self.capture_selectors(&mut captured);
        let e = &captured;
        let transpose = matches!(e, Expr::Call(n, args) if n == "transpose" && matches!(args.as_slice(), [Expr::Ident(_)]));
        let extents = if transpose { vec![] } else { self.extents(e)? };
        *self.serial += 1;
        let name = format!("@matmul_argument_{}", self.serial);
        self.declarations.push(Stmt::DummyIntent { name: name.clone(), intent: "allocatable".into() });
        self.declarations.push(Stmt::ArrayDecl { kind: kind.clone(), name: name.clone(), dims: vec![id("@deferred_extent"); rank] });
        match e {
            // TRANSPOSE of a runtime-sized matrix, copied element by element.
            Expr::Call(_, args) if transpose => {
                let Expr::Ident(source) = &args[0] else { unreachable!() };
                let (i, j) = (format!("{name}_row"), format!("{name}_col"));
                self.declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![i.clone(), j.clone()] });
                let size = |dim: i64| call("size", vec![id(source), int(dim)]);
                let at = |dim: i64, offset: &str| bin(BinOp::Sub, bin(BinOp::Add, call("lbound", vec![id(source), int(dim)]), id(offset)), int(1));
                self.before.push(Stmt::CallSub { name: "@allocate".into(), args: vec![call(&name, vec![size(2), size(1)])] });
                let copy = Stmt::AssignIndex { name: name.clone(), indices: vec![id(&j), id(&i)], value: call(source, vec![at(1, &i), at(2, &j)]) };
                let rows = Stmt::Do { var: i, start: int(1), end: size(1), body: vec![copy] };
                self.before.push(Stmt::Do { var: j, start: int(1), end: size(2), body: vec![rows] });
            }
            _ => {
                // Allocate first: assignment reallocates only for whole-array copies.
                self.before.push(Stmt::CallSub { name: "@allocate".into(), args: vec![call(&name, extents)] });
                self.before.push(set(&name, e.clone()));
            }
        }
        self.after.push(Stmt::CallSub { name: "@deallocate".into(), args: vec![id(&name)] });
        Some((name, Array { kind, rank, runtime }))
    }

    fn expression(&mut self, e: &mut Expr) {
        visit_expr(e, &mut |e| {
            let Expr::Call(name, args) = e else { return };
            if name != "matmul" || self.arrays.contains_key(name) || args.len() != 2 {
                return;
            }
            // Constant-size products keep their fixed-size lowering.
            if !args.iter().any(|arg| self.shape(arg).is_some_and(|s| s.2)) {
                return;
            }
            // Discard operand temporaries if the product cannot be formed.
            let marks = (self.declarations.len(), self.before.len(), self.after.len());
            let operands = self.operand(&args[0]).zip(self.operand(&args[1]));
            let mut product = None;
            if let Some(((a, x), (b, y))) = operands {
                let mut arrays = self.arrays.clone();
                arrays.insert(a.clone(), Array { runtime: true, ..x });
                arrays.insert(b.clone(), Array { runtime: true, ..y });
                let mut inner = Lowering { arrays: &arrays, serial: self.serial, declarations: vec![], before: vec![], after: vec![] };
                product = inner.product(&a, &b).map(|result| (result, inner.declarations, inner.before, inner.after));
            }
            let Some((result, declarations, before, after)) = product else {
                self.declarations.truncate(marks.0);
                self.before.truncate(marks.1);
                self.after.truncate(marks.2);
                return;
            };
            self.declarations.extend(declarations);
            self.before.extend(before);
            self.after.splice(0..0, after);
            *e = result;
        });
    }
}

fn scope(body: &mut Vec<Stmt>, inherited: &Arrays, serial: &mut usize) {
    let arrays = declarations(body, inherited);
    let mut declarations = vec![];
    statements(body, &arrays, serial, &mut declarations);
    // Declarations follow the USE and IMPLICIT statements that start the scope.
    let start = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
    body.splice(start..start, declarations);
}

fn statements(body: &mut Vec<Stmt>, arrays: &Arrays, serial: &mut usize, declarations: &mut Vec<Stmt>) {
    let mut output = Vec::with_capacity(body.len());
    for mut s in std::mem::take(body) {
        match &mut s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => scope(body, arrays, serial),
            Stmt::Module { body, .. } => scope(body, &Arrays::new(), serial),
            _ => {
                let mut lowering = Lowering { arrays, serial, declarations: vec![], before: vec![], after: vec![] };
                // A DO WHILE condition is evaluated on every iteration.
                if !matches!(s, Stmt::DoWhile { .. }) {
                    for e in expressions(&mut s) {
                        lowering.expression(e);
                    }
                }
                let (before, after) = (lowering.before, lowering.after);
                declarations.extend(lowering.declarations);
                for child in children(&mut s) {
                    statements(child, arrays, serial, declarations);
                }
                output.extend(before);
                output.push(s);
                output.extend(after);
                continue;
            }
        }
        output.push(s);
    }
    *body = output;
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut serial = 0;
    scope(&mut program.body, &Arrays::new(), &mut serial);
    Ok(program)
}

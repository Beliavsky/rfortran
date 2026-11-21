// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Local rank-one POINTER arrays of a derived type whose components record
//! pointer descriptors cannot hold (CHARACTER components, array or
//! ALLOCATABLE components), when every association is with one TARGET array
//! of the same scope or a rank-one section of it, as in
//!
//! ```fortran
//! type(quote), intent(in out), target :: xx(:)
//! type(quote), pointer :: yy(:)
//! yy => xx(i1:i2)
//! call reorder(yy)
//! ```
//!
//! Such a pointer is that section: its first element, stride and extent are
//! recorded at each association, `yy(i)` is `xx(lo + (i-1)*st)`, and `yy`,
//! `yy%c` and sections of `yy` are the corresponding sections of `xx`. A
//! pointer used any other way (ASSOCIATED, NULLIFY, ALLOCATE, another target,
//! use in an internal procedure) is left to the general lowering.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use std::collections::HashMap;

type Types = HashMap<String, Vec<Stmt>>;

pub(crate) fn resolve(mut p: Program) -> Program {
    let mut types = Types::new();
    collect_types(&p.body, &mut types);
    let mut serial = 0;
    units(&mut p.body, &types, &mut serial);
    p
}

fn collect_types(body: &[Stmt], out: &mut Types) {
    for s in body {
        match s {
            Stmt::DerivedType { name, components, .. } => {
                out.insert(name.clone(), components.clone());
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => collect_types(body, out),
            _ => {}
        }
    }
}

/// Whether a type has only numeric and LOGICAL scalar components, directly
/// or in nested scalar records: the types record pointer descriptors hold.
fn primitive(ty: &str, types: &Types, depth: usize) -> bool {
    let Some(components) = types.get(ty) else { return true };
    depth < 16 && components.iter().all(|c| match c {
        Stmt::VarDecl { kind, .. } | Stmt::InitializedDecl { kind, .. } => matches!(kind,
            TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Real | TypeSpec::RealKind(_)
            | TypeSpec::DoublePrecision | TypeSpec::Logical | TypeSpec::LogicalKind(_)),
        Stmt::DerivedDecl { type_name, dims, attributes, .. } => dims.is_empty() && attributes.is_empty() && primitive(type_name, types, depth + 1),
        Stmt::SourceLocation(_) => true,
        _ => false,
    })
}

fn units(body: &mut [Stmt], types: &Types, serial: &mut usize) {
    for s in body.iter_mut() {
        match s {
            Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                procedure(body, params, types, serial);
                units(body, types, serial);
            }
            Stmt::Module { body, .. } => units(body, types, serial),
            _ => {}
        }
    }
}

/// Whether a statement or expression names `p`, by its Debug form: `"p"`
/// as a name or `"p%` as the start of a component reference. A conservative
/// test: a false match only leaves the pointer to the general lowering.
fn mentions(text: &str, p: &str) -> bool {
    text.contains(&format!("\"{p}\"")) || text.contains(&format!("\"{p}%"))
}

fn procedure(body: &mut Vec<Stmt>, params: &[String], types: &Types, serial: &mut usize) {
    let candidates: Vec<(String, String)> = body.iter().flat_map(|s| match s {
        Stmt::DerivedDecl { type_name, names, attributes, dims, polymorphic: false }
            if dims.len() == 1 && attributes.len() == 1 && attributes[0] == "pointer" && !primitive(type_name, types, 0) =>
            names.iter().filter(|n| !params.contains(n)).map(|n| (n.clone(), type_name.clone())).collect(),
        _ => vec![],
    }).collect();
    for (pointer, ty) in candidates {
        // Rank-one TARGET arrays of the type in this scope.
        let targets: Vec<String> = body.iter().flat_map(|s| match s {
            Stmt::DerivedDecl { type_name, names, attributes, dims, .. }
                if type_name == &ty && dims.len() == 1 && attributes.iter().any(|a| a == "target") => names.clone(),
            _ => vec![],
        }).collect();
        // A use in an internal procedure would need the alias there too.
        if body.iter().any(|s| matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) && mentions(&format!("{s:?}"), &pointer)) {
            continue;
        }
        let Some((base, unit)) = associations(body, &pointer, &targets) else { continue };
        *serial += 1;
        let alias = Alias {
            pointer: pointer.clone(),
            base,
            unit,
            lo: format!("@alias_lo_{serial}"),
            st: format!("@alias_st_{serial}"),
            n: format!("@alias_n_{serial}"),
        };
        let mut rewritten = body.clone();
        if !alias.body(&mut rewritten) {
            continue;
        }
        // Replace the declaration by the recorded bounds.
        let mut out = vec![];
        for s in rewritten {
            match s {
                Stmt::DerivedDecl { type_name, mut names, attributes, polymorphic, dims } if names.contains(&pointer) => {
                    names.retain(|n| n != &pointer);
                    if !names.is_empty() {
                        out.push(Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims });
                    }
                    out.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(None),
                        names: vec![alias.lo.clone(), alias.st.clone(), alias.n.clone()],
                    });
                }
                s => out.push(s),
            }
        }
        if out.iter().any(|s| !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) && mentions(&format!("{s:?}"), &pointer)) {
            continue;
        }
        *body = out;
    }
}

/// The one target of every association of `pointer`, and whether all of
/// them have unit stride; None when some use is not supported.
fn associations(body: &[Stmt], pointer: &str, targets: &[String]) -> Option<(String, bool)> {
    fn walk(body: &[Stmt], pointer: &str, targets: &[String], base: &mut Option<String>, unit: &mut bool) -> bool {
        for s in body {
            match s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } => {}
                Stmt::CallSub { name, args } if name == "@pointer_associate" && matches!(args.first(), Some(Expr::Ident(n)) if n == pointer) => {
                    let (target, stride) = match args.get(1) {
                        Some(Expr::Ident(t)) => (t, false),
                        Some(Expr::Section { name, stride, .. }) => (name, stride.is_some()),
                        _ => return false,
                    };
                    if !targets.contains(target) || base.as_ref().is_some_and(|b| b != target) || mentions(&format!("{:?}", args[1]), pointer) {
                        return false;
                    }
                    *base = Some(target.clone());
                    *unit &= !stride;
                }
                // Other pointer operations keep the general lowering.
                Stmt::CallSub { name, args } if matches!(name.as_str(), "@pointer_associate" | "@nullify" | "@allocate" | "@deallocate")
                    && mentions(&format!("{args:?}"), pointer) => return false,
                _ => {
                    let mut s = s.clone();
                    let mut ok = true;
                    for e in crate::procedure_arguments::expressions(&mut s) {
                        crate::procedure_arguments::visit_expr(e, &mut |e| {
                            if let Expr::Call(n, args) = e {
                                if matches!(n.as_str(), "associated" | "null" | "loc" | "c_loc") && mentions(&format!("{args:?}"), pointer) {
                                    ok = false;
                                }
                            }
                        });
                    }
                    if !ok {
                        return false;
                    }
                    for child in crate::procedure_arguments::children(&mut s) {
                        if !walk(child, pointer, targets, base, unit) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
    let (mut base, mut unit) = (None, true);
    walk(body, pointer, targets, &mut base, &mut unit).then_some(())?;
    base.map(|b| (b, unit))
}

struct Alias {
    pointer: String,
    base: String,
    unit: bool,
    lo: String,
    st: String,
    n: String,
}

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

impl Alias {
    /// The base index of pointer element `i`.
    fn index(&self, i: Expr) -> Expr {
        let offset = bin(BinOp::Sub, i, int(1));
        let offset = if self.unit { offset } else { bin(BinOp::Mul, offset, id(&self.st)) };
        bin(BinOp::Add, id(&self.lo), offset)
    }
    /// The base section for the pointer section `lower:upper:stride`.
    fn section(&self, name: &str, lower: Option<Expr>, upper: Option<Expr>, stride: Option<Expr>) -> Expr {
        let stride = match (self.unit, stride) {
            (true, s) => s,
            (false, None) => Some(id(&self.st)),
            (false, Some(s)) => Some(bin(BinOp::Mul, id(&self.st), s)),
        };
        Expr::Section {
            name: name.into(),
            lower: Some(Box::new(self.index(lower.unwrap_or_else(|| int(1))))),
            upper: Some(Box::new(self.index(upper.unwrap_or_else(|| id(&self.n))))),
            stride: stride.map(Box::new),
        }
    }
    fn whole(&self, name: &str) -> Expr {
        self.section(name, None, None, None)
    }
    /// The component path after the pointer's name, if the name is the
    /// pointer or one of its components.
    fn rest<'a>(&self, name: &'a str) -> Option<&'a str> {
        if name == self.pointer {
            Some("")
        } else {
            name.strip_prefix(&self.pointer).and_then(|r| r.strip_prefix('%'))
        }
    }
    fn subscript(&self, e: Expr) -> Expr {
        match e {
            Expr::Section { name, lower, upper, stride } => self.section(&name.replace(&self.pointer, &self.base), lower.map(|b| *b), upper.map(|b| *b), stride.map(|b| *b)),
            e => self.index(e),
        }
    }
    /// Rewrite an expression after its operands; false for an unsupported use.
    fn expr(&self, e: &mut Expr) -> bool {
        let mut ok = true;
        crate::procedure_arguments::visit_expr(e, &mut |e| {
            match e {
                Expr::Ident(n) => match self.rest(n) {
                    Some("") => *e = self.whole(&self.base),
                    Some(rest) => *e = Expr::Call(format!("@record_component:{rest}"), vec![self.whole(&self.base)]),
                    None => {}
                },
                Expr::Section { name, lower, upper, stride } if name == &self.pointer => {
                    *e = self.section(&self.base, lower.take().map(|b| *b), upper.take().map(|b| *b), stride.take().map(|b| *b));
                }
                Expr::Call(n, args) | Expr::Index(n, args) => match self.rest(n) {
                    Some("") if !args.is_empty() => {
                        let first = args.remove(0);
                        args.insert(0, self.subscript(first));
                        *n = self.base.clone();
                    }
                    Some(_) => ok = false,
                    None => {}
                },
                _ => {}
            }
        });
        ok
    }
    fn body(&self, body: &mut Vec<Stmt>) -> bool {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } => {}
                Stmt::CallSub { name, args } if name == "@pointer_associate" && matches!(args.first(), Some(Expr::Ident(n)) if n == &self.pointer) => {
                    let (lower, upper, stride) = match &args[1] {
                        Expr::Section { lower, upper, stride, .. } => (lower.clone().map(|b| *b), upper.clone().map(|b| *b), stride.clone().map(|b| *b)),
                        _ => (None, None, None),
                    };
                    let bound = |f: &str| Expr::Call(f.into(), vec![id(&self.base), int(1)]);
                    let lower = lower.unwrap_or_else(|| bound("lbound"));
                    let upper = upper.unwrap_or_else(|| bound("ubound"));
                    let stride = stride.unwrap_or_else(|| int(1));
                    out.push(Stmt::Assign { name: self.lo.clone(), value: lower.clone() });
                    out.push(Stmt::Assign { name: self.st.clone(), value: stride });
                    // The extent max(0, (upper - lower + stride) / stride).
                    let extent = bin(BinOp::Div, bin(BinOp::Add, bin(BinOp::Sub, upper, lower), id(&self.st)), id(&self.st));
                    out.push(Stmt::Assign { name: self.n.clone(), value: Expr::Call("max".into(), vec![int(0), extent]) });
                    continue;
                }
                Stmt::Assign { name, value } if self.rest(name).is_some() => {
                    let rest = self.rest(name).unwrap().to_owned();
                    let mut value = value.clone();
                    if !self.expr(&mut value) {
                        return false;
                    }
                    let (target, section) = if rest.is_empty() { (self.base.clone(), self.whole(&self.base)) } else { (format!("{}%{rest}", self.base), self.whole("")) };
                    out.push(Stmt::AssignIndex { name: target, indices: vec![section], value });
                    continue;
                }
                Stmt::AssignIndex { name, indices, value } if self.rest(name).is_some() => {
                    let rest = self.rest(name).unwrap().to_owned();
                    *name = if rest.is_empty() { self.base.clone() } else { format!("{}%{rest}", self.base) };
                    if indices.is_empty() {
                        return false;
                    }
                    for e in indices.iter_mut().skip(1).chain(std::iter::once(value)) {
                        if !self.expr(e) {
                            return false;
                        }
                    }
                    let mut first = indices.remove(0);
                    if !self.expr_inside_subscript(&mut first) {
                        return false;
                    }
                    indices.insert(0, self.subscript(first));
                }
                _ => {
                    for e in crate::procedure_arguments::expressions(&mut s) {
                        if !self.expr(e) {
                            return false;
                        }
                    }
                    for child in crate::procedure_arguments::children(&mut s) {
                        if !self.body(child) {
                            return false;
                        }
                    }
                }
            }
            out.push(s);
        }
        *body = out;
        true
    }
    /// Rewrite the operands of a subscript, leaving the subscript itself to
    /// `subscript`.
    fn expr_inside_subscript(&self, e: &mut Expr) -> bool {
        match e {
            Expr::Section { lower, upper, stride, .. } => [lower, upper, stride].into_iter().flatten().all(|b| self.expr(b)),
            e => self.expr(e),
        }
    }
}

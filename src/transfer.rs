// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! TRANSFER of a REAL array to a COMPLEX array of the same kind, as an actual
//! argument of a CALL, as LAPACK's xxxRFSX routines pass a REAL workspace to a
//! COMPLEX dummy:
//!
//! `CALL F(TRANSFER(RWORK(1:2*N), (/ (ZERO, ZERO) /), N))`
//!
//! The actual is an expression, so the callee receives a temporary: an
//! allocatable COMPLEX array whose element I holds the REAL elements 2I-1 and
//! 2I of the source, which is the same bits. The temporary is allocated
//! before the call and deallocated after it.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec};
use crate::procedure_arguments::children;
use anyhow::{bail, Result};
use std::collections::HashMap;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}

fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

/// The kind of a REAL type.
fn real_kind(kind: &TypeSpec) -> Option<String> {
    match kind {
        TypeSpec::Real => Some("4".into()),
        TypeSpec::DoublePrecision => Some("8".into()),
        TypeSpec::RealKind(k) => Some(k.clone()),
        _ => None,
    }
}

/// Whether `mold` has a COMPLEX type.
fn complex_mold(mold: &Expr, types: &HashMap<String, TypeSpec>) -> bool {
    match mold {
        Expr::Call(n, items) if n == "@array_constructor" => items.iter().any(|i| complex_mold(i, types)),
        Expr::Call(n, _) if n == "@complex_literal" || n == "cmplx" || n == "dcmplx" => true,
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => {
            matches!(types.get(n), Some(TypeSpec::Complex(_)))
        }
        _ => false,
    }
}

struct Lower<'a> {
    /// The declared types of the unit's names.
    types: &'a HashMap<String, TypeSpec>,
    /// The temporaries to declare at the start of the unit.
    declarations: Vec<Stmt>,
    serial: usize,
}

impl Lower<'_> {
    /// The statements that set a temporary to `transfer(args)`, and its name,
    /// or None if TRANSFER is not of a REAL array to a COMPLEX array.
    fn transfer(&mut self, args: &[Expr]) -> Result<Option<(Vec<Stmt>, String)>> {
        let [source, mold, size] = args else {
            return Ok(None);
        };
        if !complex_mold(mold, self.types) {
            return Ok(None);
        }
        let (array, lower) = match source {
            Expr::Section { name, lower, stride: None, .. } => (name, lower.as_deref().cloned()),
            Expr::Ident(name) => (name, None),
            _ => return Ok(None),
        };
        let Some(kind) = self.types.get(array).and_then(real_kind) else {
            return Ok(None);
        };
        if matches!(size, Expr::Call(n, _) if n.starts_with("@keyword:")) {
            bail!("TRANSFER with a SIZE= keyword is not yet supported");
        }
        let lower = lower.unwrap_or_else(|| Expr::Call("lbound".into(), vec![Expr::Ident(array.clone()), int(1)]));
        self.serial += 1;
        let (temporary, index) = (format!("rfort_transfer_{}", self.serial), format!("rfort_transfer_i_{}", self.serial));
        self.declarations.push(Stmt::ArrayDecl {
            kind: TypeSpec::Complex(kind.clone()),
            name: temporary.clone(),
            dims: vec![Expr::Ident("@deferred_extent".into())],
        });
        self.declarations.push(Stmt::DummyIntent { name: temporary.clone(), intent: "allocatable".into() });
        self.declarations.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
        // Element I takes source elements LOWER+2(I-1) and LOWER+2(I-1)+1.
        let first = bin(BinOp::Add, lower, bin(BinOp::Mul, int(2), bin(BinOp::Sub, Expr::Ident(index.clone()), int(1))));
        let second = bin(BinOp::Add, first.clone(), int(1));
        let element = |e: Expr| Expr::Call(array.clone(), vec![e]);
        let set = vec![
            Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(temporary.clone(), vec![size.clone()])],
            },
            Stmt::Do {
                var: index.clone(),
                start: int(1),
                end: size.clone(),
                body: vec![Stmt::AssignIndex {
                    name: temporary.clone(),
                    indices: vec![Expr::Ident(index.clone())],
                    value: Expr::Call("cmplx".into(), vec![
                        element(first),
                        element(second),
                        Expr::Call("@keyword:kind".into(), vec![Expr::IntLit(kind)]),
                    ]),
                }],
            },
        ];
        Ok(Some((set, temporary)))
    }

    fn statements(&mut self, body: &mut Vec<Stmt>) -> Result<()> {
        for s in body.iter_mut() {
            match s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => continue,
                Stmt::CallSub { name, args } if !name.starts_with('@') => {
                    let mut before = vec![];
                    let mut after = vec![];
                    for a in args.iter_mut() {
                        let Expr::Call(n, transfer_args) = a else { continue };
                        if n != "transfer" || self.types.contains_key(n) {
                            continue;
                        }
                        if let Some((set, temporary)) = self.transfer(transfer_args)? {
                            before.extend(set);
                            after.push(Stmt::CallSub { name: "@deallocate".into(), args: vec![Expr::Ident(temporary.clone())] });
                            *a = Expr::Ident(temporary);
                        }
                    }
                    if !before.is_empty() {
                        let call = std::mem::replace(s, Stmt::Exit);
                        before.push(call);
                        before.extend(after);
                        *s = Stmt::Block { body: before };
                    }
                    continue;
                }
                _ => {}
            }
            for b in children(s) {
                self.statements(b)?;
            }
        }
        Ok(())
    }
}

/// The declared types of the names of a unit's body.
fn types(body: &[Stmt]) -> HashMap<String, TypeSpec> {
    let mut types = HashMap::new();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    types.insert(n.clone(), kind.clone());
                }
            }
            Stmt::ArrayDecl { kind, name, .. } => {
                types.insert(name.clone(), kind.clone());
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    types.insert(n.clone(), kind.clone());
                }
            }
            _ => {}
        }
    }
    types
}

fn unit(body: &mut Vec<Stmt>) -> Result<()> {
    for s in body.iter_mut() {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
            unit(body)?;
        }
    }
    let types = types(body);
    let mut lower = Lower { types: &types, declarations: vec![], serial: 0 };
    lower.statements(body)?;
    let declarations = lower.declarations;
    body.splice(0..0, declarations);
    Ok(())
}

pub(crate) fn resolve(mut p: crate::ast::Program) -> Result<crate::ast::Program> {
    unit(&mut p.body)?;
    Ok(p)
}

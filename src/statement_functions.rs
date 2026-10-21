// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Statement functions, such as `cabs1(z) = abs(dble(z)) + abs(dimag(z))`.
//! The parser reads a definition as an assignment to an element of a scalar.
//! Each reference is replaced by the defining expression with the actual
//! arguments in place of the dummies, converted to the function's type, so
//! later passes see an ordinary expression of any type, COMPLEX included.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

struct Definition {
    dummies: Vec<String>,
    value: Expr,
}

/// Whether `s` belongs to the specification part, where statement functions
/// are defined.
fn specification(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::SourceLocation(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. }
            | Stmt::DummyIntent { .. } | Stmt::Implicit(_) | Stmt::Use { .. } | Stmt::DerivedDecl { .. }
            | Stmt::DerivedType { .. } | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. }
            | Stmt::OperatorInterface { .. }
    ) || matches!(s, Stmt::CallSub { name, .. } if name == "@data" || name == "@format_statement" || name == crate::equivalence::STATEMENT)
}

fn substitute(e: &mut Expr, actuals: &HashMap<String, Expr>) {
    match e {
        Expr::Ident(n) => {
            if let Some(a) = actuals.get(n.as_str()) {
                *e = a.clone();
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args {
                substitute(a, actuals);
            }
        }
        Expr::Bin(_, a, b) => {
            substitute(a, actuals);
            substitute(b, actuals);
        }
        Expr::Un(_, a) => substitute(a, actuals),
        _ => {}
    }
}

/// Replace references to statement functions in `e`, innermost first.
fn expand(e: &mut Expr, definitions: &HashMap<String, Definition>) -> Result<()> {
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args.iter_mut() {
                expand(a, definitions)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expand(a, definitions)?;
            expand(b, definitions)?;
        }
        Expr::Un(_, a) => expand(a, definitions)?,
        Expr::Section { lower, upper, stride, .. } => {
            for x in [lower, upper, stride].into_iter().flatten() {
                expand(x, definitions)?;
            }
        }
        _ => {}
    }
    if let Expr::Call(name, args) | Expr::Index(name, args) = e {
        if let Some(d) = definitions.get(name.as_str()) {
            if args.len() != d.dummies.len() {
                bail!("statement function `{name}` has {} arguments, not {}", d.dummies.len(), args.len());
            }
            let actuals = d.dummies.iter().cloned().zip(args.iter().cloned()).collect();
            let mut value = d.value.clone();
            substitute(&mut value, &actuals);
            *e = value;
        }
    }
    Ok(())
}

fn statements(body: &mut [Stmt], definitions: &HashMap<String, Definition>) -> Result<()> {
    for s in body {
        if matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. }) {
            continue;
        }
        for e in crate::procedure_arguments::expressions(s) {
            expand(e, definitions)?;
        }
        for b in crate::procedure_arguments::children(s) {
            statements(b, definitions)?;
        }
    }
    Ok(())
}

/// The value of a statement function, converted to its declared type.
fn converted(value: Expr, kind: Option<&TypeSpec>) -> Expr {
    let conversion = match kind {
        Some(TypeSpec::DoublePrecision) => "dble",
        Some(TypeSpec::Real) => "real",
        Some(TypeSpec::RealKind(k)) if k == "8" => "dble",
        Some(TypeSpec::RealKind(k)) if k == "4" => "real",
        Some(TypeSpec::Integer(None)) => "int",
        _ => return value,
    };
    Expr::Call(conversion.into(), vec![value])
}

fn unit(body: &mut Vec<Stmt>) -> Result<()> {
    for s in body.iter_mut() {
        if let Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } | Stmt::Module { body, .. } = s {
            unit(body)?;
        }
    }
    let arrays: HashSet<String> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let kinds: HashMap<String, TypeSpec> = body
        .iter()
        .flat_map(|s| match s {
            Stmt::VarDecl { kind, names } => names.iter().map(|n| (n.clone(), kind.clone())).collect(),
            _ => vec![],
        })
        .collect();
    let mut definitions = HashMap::new();
    let mut kept = vec![];
    let mut specifying = true;
    for s in std::mem::take(body) {
        let definition = specifying
            && matches!(&s, Stmt::AssignIndex { name, indices, .. } if !arrays.contains(name)
                && !indices.is_empty() && indices.iter().all(|i| matches!(i, Expr::Ident(_))));
        if definition {
            let Stmt::AssignIndex { name, indices, mut value } = s else { unreachable!() };
            // A definition may use the statement functions before it.
            expand(&mut value, &definitions)?;
            let dummies = indices.into_iter().map(|i| match i {
                Expr::Ident(n) => n,
                _ => unreachable!(),
            }).collect();
            let value = converted(value, kinds.get(&name));
            definitions.insert(name, Definition { dummies, value });
            continue;
        }
        specifying &= specification(&s) || matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. });
        kept.push(s);
    }
    *body = kept;
    if !definitions.is_empty() {
        // Their names and dummies are no longer variables.
        for s in body.iter_mut() {
            if let Stmt::VarDecl { names, .. } = s {
                names.retain(|n| !definitions.contains_key(n));
            }
        }
        body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
        statements(body, &definitions)?;
    }
    Ok(())
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    unit(&mut p.body)?;
    Ok(p)
}

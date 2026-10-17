// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! The declarations of a BLOCK construct are local to it. Each declared name
//! is renamed throughout the BLOCK, so it has storage separate from a variable
//! with the same name outside the BLOCK. Diagnostics print the original name.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::Result;

/// Marks a renamed BLOCK local: `name$blockN`.
pub(crate) const MARK: &str = "$block";

/// The name as the programmer wrote it.
pub(crate) fn original(name: &str) -> &str {
    name.find(MARK).map_or(name, |at| &name[..at])
}

fn declared(s: &Stmt) -> Vec<String> {
    match s {
        Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
        _ => vec![],
    }
}

// The name itself, or a component designator `name%...` based on it.
fn rename_designator(n: &mut String, from: &str, to: &str) {
    if n == from {
        *n = to.into();
    } else if n.strip_prefix(from).is_some_and(|rest| rest.starts_with('%')) {
        *n = format!("{to}{}", &n[from.len()..]);
    }
}

fn rename_expr(e: &mut Expr, from: &str, to: &str) {
    visit_expr(e, &mut |e| match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => {
            rename_designator(n, from, to)
        }
        _ => {}
    });
}

// A named constant used as a kind or length type parameter.
fn rename_kind(kind: &mut TypeSpec, from: &str, to: &str) {
    match kind {
        TypeSpec::IntegerKind(k) | TypeSpec::RealKind(k) | TypeSpec::Complex(k) if k == from => *k = to.into(),
        TypeSpec::CharacterLength(length) => rename_expr(length, from, to),
        _ => {}
    }
}

fn rename(body: &mut [Stmt], from: &str, to: &str) {
    for s in body.iter_mut() {
        if let Stmt::VarDecl { kind, .. } | Stmt::ArrayDecl { kind, .. } | Stmt::InitializedDecl { kind, .. } = s {
            rename_kind(kind, from, to);
        }
        match s {
            Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } | Stmt::DummyIntent { name, .. }
            | Stmt::ArrayDecl { name, .. } | Stmt::Do { var: name, .. } => rename_designator(name, from, to),
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                for n in names.iter_mut() {
                    rename_designator(n, from, to);
                }
            }
            Stmt::InitializedDecl { entities, .. } => {
                for (n, _) in entities.iter_mut() {
                    rename_designator(n, from, to);
                }
            }
            _ => {}
        }
        for e in expressions(s) {
            rename_expr(e, from, to);
        }
        for b in children(s) {
            rename(b, from, to);
        }
    }
}

fn visit(body: &mut [Stmt], serial: &mut usize) {
    for s in body.iter_mut() {
        if let Stmt::Block { body } = s {
            let names: Vec<String> = body.iter().flat_map(declared).collect();
            if !names.is_empty() {
                *serial += 1;
                for name in names {
                    let local = format!("{name}{MARK}{serial}");
                    rename(body, &name, &local);
                }
            }
        }
        for b in children(s) {
            visit(b, serial);
        }
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    visit(&mut program.body, &mut 0);
    Ok(program)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Preserve explicit constructor types until kind/length parameters resolve,
//! then provide ordinary zero-extent arrays to the existing array passes.
use crate::ast::{Expr, Stmt, TypeSpec};
use anyhow::{Context, Result};

pub(crate) fn real_kind(args: Vec<Expr>) -> std::result::Result<TypeSpec, &'static str> {
    let [arg] = args.as_slice() else {
        return Err("REAL constructor requires one KIND selector");
    };
    let arg = if let Expr::Call(name, args) = arg {
        if name != "@keyword:kind" {
            return Err("REAL constructor accepts only KIND");
        }
        &args[0]
    } else {
        arg
    };
    match arg {
        Expr::Ident(name) | Expr::IntLit(name) => Ok(TypeSpec::RealKind(name.clone())),
        _ => Err("REAL constructor KIND must be a literal or named integer constant"),
    }
}
pub(crate) fn parsed(kind: TypeSpec, items: Vec<Expr>) -> Expr {
    let mut args = vec![Expr::Str(
        serde_json::to_string(&kind).expect("serialize constructor type"),
    )];
    args.extend(items);
    Expr::Call("@typed_constructor".into(), args)
}
pub(crate) fn kind(expr: &Expr) -> Result<Option<TypeSpec>> {
    if let Expr::Call(name, args) = expr {
        if name == "@typed_constructor" {
            let Some(Expr::Str(encoded)) = args.first() else {
                anyhow::bail!("invalid typed constructor");
            };
            return Ok(Some(
                serde_json::from_str(encoded).context("invalid constructor type")?,
            ));
        }
    }
    Ok(None)
}
fn expr(value: &mut Expr, prefix: &mut Vec<Stmt>) -> Result<()> {
    if let Some(kind) = kind(value)? {
        let name = format!("@empty_constructor_{}", prefix.len());
        prefix.push(Stmt::ArrayDecl {
            kind,
            name: name.clone(),
            dims: vec![Expr::IntLit("0".into())],
        });
        *value = Expr::Ident(name);
        return Ok(());
    }
    match value {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for value in args {
                expr(value, prefix)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expr(a, prefix)?;
            expr(b, prefix)?;
        }
        Expr::Un(_, value) => expr(value, prefix)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for value in [lower, upper, stride].into_iter().flatten() {
                expr(value, prefix)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn statements(body: &mut [Stmt], prefix: &mut Vec<Stmt>) -> Result<()> {
    for stmt in body {
        match stmt {
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => expr(value, prefix)?,
            Stmt::AssignIndex { indices, value, .. } => {
                for value in indices {
                    expr(value, prefix)?;
                }
                expr(value, prefix)?;
            }
            Stmt::CallSub { args, .. } | Stmt::Read { args } => {
                for value in args {
                    expr(value, prefix)?;
                }
            }
            Stmt::Print { items } => {
                for value in items {
                    expr(value, prefix)?;
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, prefix)?;
                statements(then_body, prefix)?;
                if let Some(body) = else_body {
                    statements(body, prefix)?;
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                expr(start, prefix)?;
                expr(end, prefix)?;
                statements(body, prefix)?;
            }
            Stmt::DoWhile { cond, body } => {
                expr(cond, prefix)?;
                statements(body, prefix)?;
            }
            Stmt::Block { body } => statements(body, prefix)?,
            Stmt::SelectCase {
                expr: value,
                cases,
                default,
            } => {
                expr(value, prefix)?;
                for case in cases {
                    statements(&mut case.body, prefix)?;
                }
                if let Some(body) = default {
                    statements(body, prefix)?;
                }
            }
            // Contained procedures resolve and materialize in their own scopes.
            _ => {}
        }
    }
    Ok(())
}
pub(crate) fn materialize(body: &mut Vec<Stmt>) -> Result<()> {
    let mut prefix = vec![];
    statements(body, &mut prefix)?;
    prefix.append(body);
    *body = prefix;
    Ok(())
}

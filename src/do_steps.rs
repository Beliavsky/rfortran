// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower explicit constant-step DO loops after parameter and shape resolution.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

fn int(n: i64) -> Expr { Expr::IntLit(n.to_string()) }
fn id(n: &str) -> Expr { Expr::Ident(n.into()) }
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr { Expr::Bin(op, Box::new(a), Box::new(b)) }
fn assign(n: &str, value: Expr) -> Stmt { Stmt::Assign { name: n.into(), value } }

fn cycles(body: &mut [Stmt], increment: &[Stmt]) {
    for stmt in body {
        match stmt {
            Stmt::Cycle => {
                let mut body = increment.to_vec();
                body.push(Stmt::Cycle);
                *stmt = Stmt::Block { body };
            }
            Stmt::If { then_body, else_body, .. } => {
                cycles(then_body, increment);
                if let Some(body) = else_body { cycles(body, increment); }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { cycles(&mut case.body, increment); }
                if let Some(body) = default { cycles(body, increment); }
            }
            Stmt::Block { body } if !matches!(body.first(), Some(Stmt::CallSub {name,..}) if name == "@do_step" || name == "@forall") => cycles(body, increment),
            // CYCLE inside a nested loop belongs to that loop.
            _ => {}
        }
    }
}

pub fn lower(body: &mut Vec<Stmt>, symbols: &HashMap<String,(TypeSpec,Vec<Expr>)>, serial: &mut usize) -> Result<()> {
    let Some(Stmt::CallSub { name, args }) = body.first() else { return Ok(()); };
    if name != "@do_step" { return Ok(()); }
    let [Expr::Ident(var), start, end, step] = args.as_slice() else { bail!("DO variable must be a writable scalar INTEGER"); };
    if !crate::output::integer(&id(var), symbols) { bail!("explicit-step DO requires a declared scalar INTEGER variable"); }
    if !crate::output::integer(start, symbols) || !crate::output::integer(end, symbols) {
        bail!("explicit-step DO bounds must be scalar INTEGER expressions");
    }
    let step = crate::parameters::integer_constant(step).context("DO step must currently be a constant INTEGER")?;
    if step == 0 { bail!("DO step must not be zero"); }
    let var = var.clone();
    let start = start.clone();
    let end = end.clone();
    *serial += 1;
    let first = format!("@do_start_{serial}");
    let last = format!("@do_end_{serial}");
    let limit = if step > 0 { i64::MAX - step } else { i64::MIN - step };
    let increment = vec![
        Stmt::If {
            cond: bin(if step > 0 { BinOp::Gt } else { BinOp::Lt }, id(&var), int(limit)),
            then_body: vec![Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str("DO variable overflow".into())] }],
            else_body: None,
        },
        assign(&var, bin(BinOp::Add, id(&var), int(step))),
    ];
    let mut inner = body.split_off(1);
    cycles(&mut inner, &increment);
    inner.extend(increment);
    *body = vec![
        Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![first.clone(),last.clone()] },
        assign(&first, start), assign(&last, end), assign(&var, id(&first)),
        Stmt::DoWhile { cond: bin(if step > 0 { BinOp::Le } else { BinOp::Ge }, id(&var), id(&last)), body: inner },
    ];
    Ok(())
}

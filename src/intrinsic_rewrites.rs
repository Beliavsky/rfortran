// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Intrinsics defined in terms of other supported operations.
//!
//! LGE, LGT, LLE and LLT compare in the ASCII collating sequence, which is the
//! order of the intrinsic CHARACTER relational operators for rfort's ASCII
//! storage. ISHFTC rotates the rightmost SIZE bits of I with IBITS, ISHFT and
//! masks, using DIGITS(I)+1 as the default SIZE (the bit size of I's kind).
//! BGE, BGT, BLE and BLT compare as unsigned integers: the signed comparison,
//! reversed when exactly one argument is negative.
//! A user procedure with one of these names keeps its own definition.
use crate::ast::{BinOp, Expr, Program, Stmt};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Result};
use std::collections::HashSet;

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(name.into(), args)
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

fn user_procedures(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                out.insert(name.to_ascii_lowercase());
                user_procedures(body, out);
            }
            Stmt::Module { body, .. } => user_procedures(body, out),
            Stmt::GenericInterface { name, .. } => {
                out.insert(name.to_ascii_lowercase());
            }
            _ => {}
        }
    }
}

fn ishftc(args: &[Expr]) -> Result<Expr> {
    if !(2..=3).contains(&args.len()) {
        bail!("ISHFTC expects I, SHIFT and optional SIZE");
    }
    let i = args[0].clone();
    let size = args.get(2).cloned().unwrap_or_else(|| bin(BinOp::Add, call("digits", vec![i.clone()]), int(1)));
    let shift = call("modulo", vec![args[1].clone(), size.clone()]);
    let field = call("ibits", vec![i.clone(), int(0), size.clone()]);
    let zero = call("ieor", vec![i.clone(), i.clone()]);
    let mask = call("ibits", vec![call("not", vec![zero]), int(0), size.clone()]);
    let left = call("iand", vec![call("ishft", vec![field.clone(), shift.clone()]), mask.clone()]);
    let right = call("ishft", vec![field, bin(BinOp::Sub, shift, size)]);
    Ok(call("ior", vec![call("iand", vec![i, call("not", vec![mask])]), call("ior", vec![left, right])]))
}

fn rewrite(e: &mut Expr, user: &HashSet<String>, error: &mut Option<anyhow::Error>) {
    let Expr::Call(name, args) = e else { return };
    let lname = name.to_ascii_lowercase();
    if user.contains(&lname) {
        return;
    }
    if let Some(op) = match lname.as_str() { "bge" => Some(BinOp::Ge), "bgt" => Some(BinOp::Gt), "ble" => Some(BinOp::Le), "blt" => Some(BinOp::Lt), _ => None } {
        if args.len() != 2 {
            *error = Some(anyhow::anyhow!("{} expects I and J", lname.to_ascii_uppercase()));
            return;
        }
        // Unsigned order is the signed order, reversed when the signs differ.
        let (i, j) = (args[0].clone(), args[1].clone());
        let signs = bin(BinOp::Neqv, bin(BinOp::Lt, i.clone(), int(0)), bin(BinOp::Lt, j.clone(), int(0)));
        *e = bin(BinOp::Neqv, bin(op, i, j), signs);
        return;
    }
    let op = match lname.as_str() {
        "lge" => BinOp::Ge,
        "lgt" => BinOp::Gt,
        "lle" => BinOp::Le,
        "llt" => BinOp::Lt,
        "ishftc" => {
            match ishftc(args) {
                Ok(value) => *e = value,
                Err(err) => *error = Some(err),
            }
            return;
        }
        _ => return,
    };
    if args.len() != 2 {
        *error = Some(anyhow::anyhow!("{} expects STRING_A and STRING_B", lname.to_ascii_uppercase()));
        return;
    }
    *e = bin(op, args[0].clone(), args[1].clone());
}

fn visit(body: &mut [Stmt], user: &HashSet<String>) -> Result<()> {
    for s in body.iter_mut() {
        let mut error = None;
        for e in expressions(s) {
            visit_expr(e, &mut |e| rewrite(e, user, &mut error));
        }
        if let Some(error) = error {
            return Err(error);
        }
        for b in children(s) {
            visit(b, user)?;
        }
    }
    Ok(())
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut user = HashSet::new();
    user_procedures(&program.body, &mut user);
    visit(&mut program.body, &user)?;
    Ok(program)
}

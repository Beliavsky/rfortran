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
//! reversed when exactly one argument is negative. IS_IOSTAT_END and
//! IS_IOSTAT_EOR compare with the IOSTAT_END and IOSTAT_EOR values, -1 and -2.
//! SIND, ASIND, SINPI, ATAN2D and the rest of the degree and half-revolution
//! functions scale the argument or result of SIN, ASIN or ATAN2 by PI in the
//! argument's kind, and ATAN(Y, X) is ATAN2(Y, X).
//! A user procedure or variable with one of these names keeps its own definition.
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
            Stmt::GenericInterface { name, .. } | Stmt::ArrayDecl { name, .. } => {
                out.insert(name.to_ascii_lowercase());
            }
            Stmt::VarDecl { names, .. } => out.extend(names.iter().map(|n| n.to_ascii_lowercase())),
            Stmt::InitializedDecl { entities, .. } => out.extend(entities.iter().map(|(n, _)| n.to_ascii_lowercase())),
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

// Pi in the kind of the REAL expression x, which may be an array.
fn pi_like(x: &Expr) -> Expr {
    call("acos", vec![bin(BinOp::Sub, bin(BinOp::Mul, x.clone(), int(0)), int(1))])
}

// Degree and half-revolution trigonometric functions: the argument is scaled
// to radians before the ordinary function, or the result scaled after it.
fn scaled_trig(lname: &str, args: &[Expr]) -> Option<Result<Expr>> {
    let (base, unit) = if let Some(base) = lname.strip_suffix("pi") {
        (base, None)
    } else {
        (lname.strip_suffix('d')?, Some(180))
    };
    let two = matches!(base, "atan2") || base == "atan" && args.len() == 2;
    if !matches!(base, "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2") {
        return None;
    }
    if args.len() != if two { 2 } else { 1 } {
        return Some(Err(anyhow::anyhow!("{} expects {} argument(s)", lname.to_ascii_uppercase(), if two { 2 } else { 1 })));
    }
    let x = &args[0];
    let radians_per_unit = match unit { Some(u) => bin(BinOp::Div, pi_like(x), int(u)), None => pi_like(x) };
    Some(Ok(if matches!(base, "sin" | "cos" | "tan") {
        call(base, vec![bin(BinOp::Mul, x.clone(), radians_per_unit)])
    } else if two {
        bin(BinOp::Div, call("atan2", args.to_vec()), radians_per_unit)
    } else {
        bin(BinOp::Div, call(base, vec![x.clone()]), radians_per_unit)
    }))
}

fn rewrite(e: &mut Expr, user: &HashSet<String>, error: &mut Option<anyhow::Error>) {
    let Expr::Call(name, args) = e else { return };
    let lname = name.to_ascii_lowercase();
    if user.contains(&lname) {
        return;
    }
    if lname == "atan" && args.len() == 2 {
        *e = call("atan2", args.clone());
        return;
    }
    if let Some(result) = scaled_trig(&lname, args) {
        match result {
            Ok(value) => *e = value,
            Err(err) => *error = Some(err),
        }
        return;
    }
    if let Some(code) = match lname.as_str() { "is_iostat_end" => Some(-1), "is_iostat_eor" => Some(-2), _ => None } {
        if args.len() != 1 {
            *error = Some(anyhow::anyhow!("{} expects I", lname.to_ascii_uppercase()));
            return;
        }
        *e = bin(BinOp::Eq, args[0].clone(), int(code));
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

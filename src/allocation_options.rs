// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower MOVE_ALLOC and the SOURCE=, MOLD=, STAT= and ERRMSG= options of
//! ALLOCATE and DEALLOCATE to plain allocation statements.
//!
//! MOVE_ALLOC copies the data and deallocates the source, which is equivalent
//! except for pointers associated with the moved array. STAT= reports the
//! errors the language defines for an allocation status (allocating an
//! allocated array, deallocating an unallocated one) instead of stopping.
use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

type Ranks = HashMap<String, usize>;

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn call(n: &str, args: Vec<Expr>) -> Stmt {
    Stmt::CallSub { name: n.into(), args }
}
fn test(f: &str, n: &str) -> Expr {
    Expr::Call(f.into(), vec![id(n)])
}
fn when(cond: Expr, then_body: Vec<Stmt>, else_body: Option<Vec<Stmt>>) -> Stmt {
    Stmt::If { cond, then_body, else_body }
}
fn any(conditions: Vec<Expr>) -> Expr {
    conditions.into_iter().reduce(|a, b| Expr::Bin(BinOp::Or, Box::new(a), Box::new(b))).unwrap_or(Expr::Logical(false))
}

/// ALLOCATE `to` with the bounds of the array `from`.
fn allocate_like(to: &str, from: &str, rank: usize) -> Stmt {
    let bound = |f: &str, d: usize| Some(Box::new(Expr::Call(f.into(), vec![id(from), Expr::IntLit(d.to_string())])));
    let subscripts = (1..=rank)
        .map(|d| {
            Expr::Call("@section_subscript".into(), vec![Expr::Section {
                name: to.into(),
                lower: bound("lbound", d),
                upper: bound("ubound", d),
                stride: None,
            }])
        })
        .collect();
    call("@allocate", vec![Expr::Call(to.into(), subscripts)])
}

fn keyword(e: &Expr) -> Option<(&str, &Expr)> {
    match e {
        Expr::Call(k, v) if v.len() == 1 => k.strip_prefix("@keyword:").map(|k| (k, &v[0])),
        _ => None,
    }
}

fn status(stat: Option<&Expr>, errmsg: Option<&Expr>, code: i64, message: &str) -> Result<Vec<Stmt>> {
    let mut out = vec![];
    if let Some(stat) = stat {
        let Expr::Ident(s) = stat else { bail!("STAT= requires a scalar INTEGER variable") };
        out.push(Stmt::Assign { name: s.clone(), value: Expr::IntLit(code.to_string()) });
    }
    if code != 0 {
        if let Some(errmsg) = errmsg {
            let Expr::Ident(m) = errmsg else { bail!("ERRMSG= requires a scalar CHARACTER variable") };
            out.push(Stmt::Assign { name: m.clone(), value: Expr::Str(message.into()) });
        }
    }
    Ok(out)
}

fn allocate(args: &[Expr], ranks: &Ranks) -> Result<Option<Vec<Stmt>>> {
    let mut objects = vec![];
    let (mut stat, mut errmsg, mut source, mut mold) = (None, None, None, None);
    for a in args {
        match keyword(a) {
            Some(("stat", v)) => stat = Some(v),
            Some(("errmsg", v)) => errmsg = Some(v),
            Some(("source", v)) => source = Some(v),
            Some(("mold", v)) => mold = Some(v),
            Some((k, _)) => bail!("unsupported ALLOCATE option `{k}=`"),
            None => objects.push(a.clone()),
        }
    }
    if stat.is_none() && errmsg.is_none() && source.is_none() && mold.is_none() {
        return Ok(None);
    }
    let shape = source.or(mold);
    let mut allocations = vec![];
    let mut names = vec![];
    for object in &objects {
        let name = match object {
            Expr::Ident(n) | Expr::Call(n, _) | Expr::Section { name: n, .. } => n.clone(),
            _ => bail!("ALLOCATE requires ALLOCATABLE variables"),
        };
        match (object, shape) {
            // Bounds come from an array SOURCE= or MOLD=.
            (Expr::Ident(n), Some(Expr::Ident(from))) if ranks.get(from).is_some_and(|r| *r > 0) => {
                allocations.push(allocate_like(n, from, ranks[from]));
            }
            (Expr::Ident(n), _) if ranks.get(n).is_some_and(|r| *r > 0) => {
                bail!("ALLOCATE of array `{n}` requires bounds or an array variable as SOURCE= or MOLD=")
            }
            _ => allocations.push(call("@allocate", vec![object.clone()])),
        }
        if let Some(value) = source {
            allocations.push(Stmt::Assign { name: name.clone(), value: value.clone() });
        }
        names.push(name);
    }
    if stat.is_none() {
        return Ok(Some(allocations));
    }
    allocations.extend(status(stat, errmsg, 0, "")?);
    let arrays: Vec<Expr> = names.iter().filter(|n| ranks.get(*n).is_some_and(|r| *r > 0)).map(|n| test("allocated", n)).collect();
    if arrays.is_empty() {
        return Ok(Some(allocations));
    }
    Ok(Some(vec![when(any(arrays), status(stat, errmsg, 1, "array is already allocated")?, Some(allocations))]))
}

fn deallocate(args: &[Expr], ranks: &Ranks) -> Result<Option<Vec<Stmt>>> {
    let mut objects = vec![];
    let (mut stat, mut errmsg) = (None, None);
    for a in args {
        match keyword(a) {
            Some(("stat", v)) => stat = Some(v),
            Some(("errmsg", v)) => errmsg = Some(v),
            Some((k, _)) => bail!("unsupported DEALLOCATE option `{k}=`"),
            None => objects.push(a.clone()),
        }
    }
    if stat.is_none() && errmsg.is_none() {
        return Ok(None);
    }
    let mut body = vec![call("@deallocate", objects.clone())];
    if stat.is_none() {
        return Ok(Some(body));
    }
    body.extend(status(stat, errmsg, 0, "")?);
    let unallocated: Vec<Expr> = objects
        .iter()
        .filter_map(|o| match o {
            Expr::Ident(n) if ranks.get(n).is_some_and(|r| *r > 0) => {
                Some(Expr::Un(UnOp::Not, Box::new(test("allocated", n))))
            }
            _ => None,
        })
        .collect();
    if unallocated.is_empty() {
        return Ok(Some(body));
    }
    Ok(Some(vec![when(any(unallocated), status(stat, errmsg, 1, "array is not allocated")?, Some(body))]))
}

fn move_alloc(args: &[Expr], ranks: &Ranks) -> Result<Vec<Stmt>> {
    let mut from = None;
    let mut to = None;
    for (i, a) in args.iter().enumerate() {
        match (keyword(a), i) {
            (Some(("from", v)), _) => from = Some(v.clone()),
            (Some(("to", v)), _) => to = Some(v.clone()),
            (Some((k, _)), _) => bail!("unsupported MOVE_ALLOC argument `{k}=`"),
            (None, 0) => from = Some(a.clone()),
            (None, 1) => to = Some(a.clone()),
            _ => bail!("MOVE_ALLOC takes FROM and TO"),
        }
    }
    let (Some(Expr::Ident(from)), Some(Expr::Ident(to))) = (from, to) else {
        bail!("MOVE_ALLOC requires ALLOCATABLE variables FROM and TO")
    };
    let Some(&rank) = ranks.get(&from).filter(|r| **r > 0) else {
        bail!("MOVE_ALLOC currently requires ALLOCATABLE arrays")
    };
    if ranks.get(&to) != Some(&rank) {
        bail!("MOVE_ALLOC arguments `{from}` and `{to}` must have the same rank");
    }
    Ok(vec![
        when(test("allocated", &to), vec![call("@deallocate", vec![id(&to)])], None),
        when(
            test("allocated", &from),
            vec![
                allocate_like(&to, &from, rank),
                // The move also moves elements that were never set, so the
                // copy is not checked for reads of unset elements.
                call(crate::initialized::UNCHECKED_BEGIN, vec![]),
                Stmt::Assign { name: to.clone(), value: id(&from) },
                call(crate::initialized::UNCHECKED_END, vec![]),
                call(crate::initialized::COPY_INITIALIZATION, vec![id(&to),id(&from)]),
                call("@deallocate", vec![id(&from)]),
            ],
            None,
        ),
    ])
}

fn lower(body: &mut Vec<Stmt>, inherited: &Ranks) -> Result<()> {
    let mut ranks = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, dims, .. } => {
                ranks.insert(name.clone(), dims.len());
            }
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    ranks.insert(n.clone(), 0);
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        let replacement = match &s {
            Stmt::CallSub { name, args } if name == "@allocate" => allocate(args, &ranks)?,
            Stmt::CallSub { name, args } if name == "@deallocate" => deallocate(args, &ranks)?,
            Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => Some(move_alloc(args, &ranks)?),
            _ => None,
        };
        if let Some(statements) = replacement {
            out.extend(statements);
            continue;
        }
        match &mut s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => lower(body, &ranks)?,
            Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => lower(body, &ranks)?,
            Stmt::If { then_body, else_body, .. } => {
                lower(then_body, &ranks)?;
                if let Some(b) = else_body {
                    lower(b, &ranks)?;
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    lower(&mut c.body, &ranks)?;
                }
                if let Some(b) = default {
                    lower(b, &ranks)?;
                }
            }
            _ => {}
        }
        out.push(s);
    }
    *body = out;
    Ok(())
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    lower(&mut program.body, &Ranks::new())?;
    Ok(program)
}

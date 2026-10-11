// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Conservative bounds for local allocation sizes; unknown writes discard facts.
use crate::allocatables::Shapes;
use crate::ast::{BinOp, Expr, Stmt, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Range = (i64, i64);
type Values = HashMap<String, Range>;

fn value(e: &Expr, values: &Values, shapes: &Shapes) -> Option<Range> {
    if let Ok(n) = crate::parameters::integer_constant(e) {
        return Some((n, n));
    }
    match e {
        Expr::Ident(n) => values.get(n).copied(),
        Expr::Un(UnOp::Neg, a) => {
            let (lo, hi) = value(a, values, shapes)?;
            Some((hi.checked_neg()?, lo.checked_neg()?))
        }
        Expr::Bin(op, a, b) => {
            let (al, ah) = value(a, values, shapes)?;
            let (bl, bh) = value(b, values, shapes)?;
            match op {
                BinOp::Add => Some((al.checked_add(bl)?, ah.checked_add(bh)?)),
                BinOp::Sub => Some((al.checked_sub(bh)?, ah.checked_sub(bl)?)),
                BinOp::Mul | BinOp::Div => {
                    if matches!(op, BinOp::Div) && bl <= 0 && bh >= 0 {
                        return None;
                    }
                    let mut endpoints = vec![];
                    for a in [al, ah] {
                        for b in [bl, bh] {
                            endpoints.push(if matches!(op, BinOp::Mul) {
                                a.checked_mul(b)?
                            } else {
                                a.checked_div(b)?
                            });
                        }
                    }
                    Some((*endpoints.iter().min()?, *endpoints.iter().max()?))
                }
                _ => None,
            }
        }
        Expr::Call(n, args) if n == "size" => {
            let Expr::Ident(a) = args.first()? else {
                return None;
            };
            let dims = &shapes.get(a)?.1;
            if args.len() == 1 {
                dims.iter().try_fold((1i64, 1i64), |(lo, hi), d| {
                    let (dl, dh) = value(d, values, shapes)?;
                    Some((lo.checked_mul(dl.max(0))?, hi.checked_mul(dh.max(0))?))
                })
            } else if args.len() == 2 {
                let axis = crate::parameters::integer_constant(&args[1]).ok()?;
                value(
                    dims.get(usize::try_from(axis.checked_sub(1)?).ok()?)?,
                    values,
                    shapes,
                )
            } else {
                None
            }
        }
        Expr::Call(n, args) if matches!(n.as_str(), "min" | "max") && !args.is_empty() => {
            let mut ranges = args.iter().map(|a| value(a, values, shapes));
            let mut result = ranges.next()??;
            for range in ranges {
                let (lo, hi) = range?;
                result = if n == "min" {
                    (result.0.min(lo), result.1.min(hi))
                } else {
                    (result.0.max(lo), result.1.max(hi))
                };
            }
            Some(result)
        }
        _ => None,
    }
}

fn join(a: &Values, b: &Values) -> Values {
    a.iter()
        .filter_map(|(n, (lo, hi))| {
            b.get(n)
                .map(|(bl, bh)| (n.clone(), ((*lo).min(*bl), (*hi).max(*bh))))
        })
        .collect()
}

fn invalidate(body: &[Stmt], values: &mut Values) {
    for s in body {
        match s {
            Stmt::Assign { name, .. }
            | Stmt::AssignIndex { name, .. }
            | Stmt::Do { var: name, .. } => {
                values.remove(name);
            }
            Stmt::CallSub { name, args }
                if !matches!(name.as_str(), "@allocate" | "@deallocate") =>
            {
                for arg in args {
                    if let Expr::Ident(n) = arg {
                        values.remove(n);
                    }
                }
            }
            Stmt::Read { args } => {
                for arg in args {
                    if let Expr::Ident(n) = arg {
                        values.remove(n);
                    }
                }
            }
            _ => {}
        }
        match s {
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                invalidate(then_body, values);
                if let Some(b) = else_body {
                    invalidate(b, values);
                }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                invalidate(body, values)
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    invalidate(&c.body, values);
                }
                if let Some(b) = default {
                    invalidate(b, values);
                }
            }
            _ => {}
        }
    }
}

fn scan(
    body: &[Stmt],
    targets: &HashMap<String, usize>,
    shapes: &Shapes,
    values: &mut Values,
    bounds: &mut HashMap<String, Vec<i64>>,
) -> Result<()> {
    for s in body {
        match s {
            Stmt::Assign { name, value: e } => {
                if let Some(range) = value(e, values, shapes) {
                    values.insert(name.clone(), range);
                } else {
                    values.remove(name);
                }
            }
            Stmt::CallSub { name, args } if name == "@allocate" => {
                for arg in args {
                    if let Expr::Call(n, dims) = arg {
                        if let Some(rank) = targets.get(n) {
                            if dims.len() != *rank {
                                bail!("ALLOCATE rank mismatch for `{n}`");
                            }
                            let sizes=dims.iter().map(|d|value(d,values,shapes).map(|(_,hi)|hi.max(0))
                        .with_context(||format!("cannot infer a finite storage bound for ALLOCATE of `{n}`; allocation dimensions must have proven bounds")))
                        .collect::<Result<Vec<_>>>()?;
                            let current = bounds.entry(n.clone()).or_insert_with(|| vec![0; *rank]);
                            for (old, new) in current.iter_mut().zip(sizes) {
                                *old = (*old).max(new);
                            }
                        }
                    }
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                if let Expr::Logical(chosen) = cond {
                    if *chosen {
                        scan(then_body, targets, shapes, values, bounds)?;
                    } else if let Some(b) = else_body {
                        scan(b, targets, shapes, values, bounds)?;
                    }
                } else {
                    let mut a = values.clone();
                    let mut b = values.clone();
                    scan(then_body, targets, shapes, &mut a, bounds)?;
                    if let Some(body) = else_body {
                        scan(body, targets, shapes, &mut b, bounds)?;
                    }
                    *values = join(&a, &b);
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                let mut inner = values.clone();
                let range = value(start, values, shapes)
                    .zip(value(end, values, shapes))
                    .map(|((lo, _), (_, hi))| (lo.min(hi), lo.max(hi)));
                // Loop-carried assignments cannot retain their entry-time bounds.
                // Assignments which reset values in every iteration reestablish facts.
                invalidate(body, &mut inner);
                inner.remove(var);
                if let Some(range) = range {
                    inner.insert(var.clone(), range);
                }
                scan(body, targets, shapes, &mut inner, bounds)?;
                invalidate(body, values);
                values.remove(var);
            }
            Stmt::DoWhile { body, .. } => {
                let mut inner = values.clone();
                invalidate(body, &mut inner);
                scan(body, targets, shapes, &mut inner, bounds)?;
                invalidate(body, values);
            }
            Stmt::Block { body } => {
                let mut inner = values.clone();
                let mut visible = targets.clone();
                for s in body {
                    match s {
                        Stmt::ArrayDecl { name, .. } => {
                            visible.remove(name);
                            inner.remove(name);
                        }
                        Stmt::VarDecl { names, .. } => {
                            for n in names {
                                visible.remove(n);
                                inner.remove(n);
                            }
                        }
                        _ => {}
                    }
                }
                scan(body, &visible, shapes, &mut inner, bounds)?;
                invalidate(body, values);
            }
            Stmt::SelectCase { cases, default, .. } => {
                let initial = values.clone();
                let mut paths = vec![];
                for c in cases {
                    let mut path = initial.clone();
                    scan(&c.body, targets, shapes, &mut path, bounds)?;
                    paths.push(path);
                }
                let mut path = initial;
                if let Some(b) = default {
                    scan(b, targets, shapes, &mut path, bounds)?;
                }
                paths.push(path);
                *values = paths.into_iter().reduce(|a, b| join(&a, &b)).unwrap();
            }
            Stmt::CallSub { name, .. } if name == "@deallocate" => {}
            Stmt::CallSub { .. } | Stmt::Read { .. } => invalidate(std::slice::from_ref(s), values),
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn infer(
    body: &[Stmt],
    targets: &HashMap<String, usize>,
    shapes: &Shapes,
    constants: &HashMap<String, Expr>,
) -> Result<HashMap<String, Vec<i64>>> {
    let mut values = constants
        .iter()
        .filter_map(|(n, e)| {
            crate::parameters::integer_constant(e)
                .ok()
                .map(|v| (n.clone(), (v, v)))
        })
        .collect();
    let mut bounds = HashMap::new();
    scan(body, targets, shapes, &mut values, &mut bounds)?;
    Ok(bounds)
}

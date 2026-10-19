// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Expand bounded formatted input lists without reading destination storage.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

pub(crate) type Shapes = HashMap<String, Vec<Expr>>;
const LIMIT: usize = 10000;
pub(crate) const INDEX: &str = "@formatted_read_index";
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}

/// Capture bounded input storage before CHARACTER array scalarization can
/// hoist a designator depending on an implied-DO index out of its iteration.
pub(crate) fn prepare(mut program: Program) -> Result<Program> {
    fn body(statements: &mut [Stmt], parent: &Shapes) -> Result<()> {
        let mut shapes = parent.clone();
        for s in statements.iter() {
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    shapes.insert(name.clone(), dims.clone());
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        shapes.insert(n.clone(), vec![]);
                    }
                }
                _ => {}
            }
        }
        let mut location = None;
        for s in statements {
            if let Stmt::SourceLocation(pos) = s {
                location = Some(*pos);
            }
            if let Stmt::CallSub { name, args } = s {
                if matches!(name.as_str(), "@formatted_read" | "@formatted_read_status") {
                    let first = if name == "@formatted_read" { 2 } else { 4 };
                    let values = expand(&args[first..], &shapes, &|e| {
                        crate::parameters::integer_constant(e).ok()
                    })
                    .map_err(|e| crate::source_diagnostics::at(e, location))?;
                    args.truncate(first);
                    args.extend(values);
                }
            }
            for child in crate::procedure_arguments::children(s) {
                body(child, &shapes)?;
            }
        }
        Ok(())
    }
    body(&mut program.body, &Shapes::new())?;
    Ok(program)
}

/// Index updates are kept in list order, including zero-trip and final values.
/// They consume no edit descriptor and are executed only until input fails.
pub(crate) fn index(e: &Expr) -> Option<(&str, &Expr)> {
    match e {
        Expr::Call(n, a) if n == INDEX => match a.as_slice() {
            [Expr::Ident(n), value] => Some((n, value)),
            _ => None,
        },
        _ => None,
    }
}
pub(crate) fn expand(
    items: &[Expr],
    shapes: &Shapes,
    base_constant: &dyn Fn(&Expr) -> Option<i64>,
) -> Result<Vec<Expr>> {
    let constant = |e: &Expr| {
        let mut e = e.clone();
        crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
            if let Expr::Call(n, a) = e {
                if !matches!(n.as_str(), "size" | "lbound" | "ubound") {
                    return;
                }
                let Some(Expr::Ident(array)) = a.first() else {
                    return;
                };
                let Some(dims) = shapes.get(array).filter(|d| !d.is_empty()) else {
                    return;
                };
                let Ok(axes) = dims
                    .iter()
                    .map(|d| bounds(d, base_constant))
                    .collect::<Result<Vec<_>>>()
                else {
                    return;
                };
                let value = if let Some(dim) = a.get(1).and_then(base_constant) {
                    let Some(&(lo, hi)) = dim
                        .checked_sub(1)
                        .and_then(|d| usize::try_from(d).ok())
                        .and_then(|d| axes.get(d))
                    else {
                        return;
                    };
                    match n.as_str() {
                        "lbound" => Some(if hi < lo { 1 } else { lo }),
                        "ubound" => Some(if hi < lo { 0 } else { hi }),
                        _ => hi
                            .checked_sub(lo)
                            .and_then(|d| d.checked_add(1))
                            .map(|d| d.max(0)),
                    }
                } else if n == "size" && a.len() == 1 {
                    axes.into_iter().try_fold(1i64, |size, (lo, hi)| {
                        hi.checked_sub(lo)?.checked_add(1)?.max(0).checked_mul(size)
                    })
                } else {
                    None
                };
                if let Some(value) = value {
                    *e = int(value);
                }
            }
        });
        base_constant(&e)
    };
    let mut out = vec![];
    expand_into(items, shapes, &constant, &mut HashMap::new(), &mut out)?;
    Ok(out)
}
fn substitute(e: &Expr, bindings: &HashMap<String, i64>) -> Expr {
    let mut e = e.clone();
    crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
        if let Expr::Ident(n) = e {
            if let Some(value) = bindings.get(n) {
                *e = int(*value);
            }
        }
    });
    e
}
fn push(out: &mut Vec<Expr>, e: Expr) -> Result<()> {
    if out.len() >= LIMIT {
        bail!("formatted READ input-list expansion exceeds {LIMIT} entries");
    }
    out.push(e);
    Ok(())
}
fn values(first: i64, last: i64, step: i64) -> Result<Vec<i64>> {
    if step == 0 {
        bail!("formatted READ section or implied-DO step must not be zero");
    }
    let (mut i, last, step) = (i128::from(first), i128::from(last), i128::from(step));
    let mut out = vec![];
    while if step > 0 { i <= last } else { i >= last } {
        if out.len() >= LIMIT {
            bail!("formatted READ input-list expansion exceeds {LIMIT} entries");
        }
        out.push(i as i64);
        i += step;
    }
    Ok(out)
}
fn bounds(dim: &Expr, constant: &dyn Fn(&Expr) -> Option<i64>) -> Result<(i64, i64)> {
    let (lo, hi) = match dim {
        Expr::Call(n, a) if n == "@declared_bounds" => (constant(&a[0]), constant(&a[1])),
        _ => (Some(1), constant(dim)),
    };
    Ok((
        lo.context("formatted READ currently requires fixed array bounds")?,
        hi.context("formatted READ currently requires fixed array bounds")?,
    ))
}
fn expand_into(
    items: &[Expr],
    shapes: &Shapes,
    constant: &dyn Fn(&Expr) -> Option<i64>,
    bindings: &mut HashMap<String, i64>,
    out: &mut Vec<Expr>,
) -> Result<()> {
    for original in items {
        if index(original).is_some() {
            push(out, original.clone())?;
            continue;
        }
        if let Expr::Call(n, a) = original {
            if n == "@input_implied_do" || n == "@output_implied_do" {
                let Some(Expr::Ident(var)) = a.first() else {
                    bail!("READ implied-DO requires an INTEGER variable");
                };
                if bindings.contains_key(var) {
                    bail!("nested READ implied-DO cannot reuse active variable `{var}`");
                }
                let control=a[1..4].iter().map(|e|constant(&substitute(e,bindings)).context("formatted READ implied-DO bounds and step must currently be constant integers")).collect::<Result<Vec<_>>>()?;
                let (first, last, step) = (control[0], control[1], control[2]);
                let iterations = values(first, last, step)?;
                let mut final_value = first;
                for i in iterations {
                    push(
                        out,
                        Expr::Call(INDEX.into(), vec![Expr::Ident(var.clone()), int(i)]),
                    )?;
                    bindings.insert(var.clone(), i);
                    expand_into(&a[4..], shapes, constant, bindings, out)?;
                    bindings.remove(var);
                    final_value = i
                        .checked_add(step)
                        .context("READ implied-DO variable overflow")?;
                }
                push(
                    out,
                    Expr::Call(
                        INDEX.into(),
                        vec![Expr::Ident(var.clone()), int(final_value)],
                    ),
                )?;
                continue;
            }
        }
        let root = match original {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => Some(n),
            _ => None,
        };
        if root.is_some_and(|n| bindings.contains_key(n)) {
            bail!(
                "READ cannot redefine active implied-DO variable `{}`",
                root.unwrap()
            );
        }
        let e = substitute(original, bindings);
        if let Some((_, count)) = crate::multidim::view_element(&e, &int(1)) {
            for i in 1..=count {
                push(out, crate::multidim::view_element(&e, &int(i)).unwrap().0)?;
            }
            continue;
        }
        let (name, args) = match &e {
            Expr::Ident(n) if shapes.get(n).is_some_and(|d| !d.is_empty()) => (n, None),
            Expr::Section { name, .. } => (name, Some(vec![e.clone()])),
            Expr::Index(n, a) | Expr::Call(n, a)
                if a.iter()
                    .any(|e| matches!(e,Expr::Call(n,_) if n=="@section_subscript")) =>
            {
                (n, Some(a.clone()))
            }
            _ => {
                push(out, e)?;
                continue;
            }
        };
        let dims = shapes
            .get(name)
            .with_context(|| format!("READ section `{name}` requires declared array storage"))?;
        if args.as_ref().is_some_and(|a| a.len() != dims.len()) {
            bail!("READ section `{name}` has the wrong number of subscripts");
        }
        let mut indices: Vec<Vec<Expr>> = vec![vec![]];
        for (axis, dim) in dims.iter().enumerate() {
            let (lo, hi) = bounds(dim, constant)?;
            let arg = args.as_ref().map(|a| &a[axis]);
            let arg = match arg {
                Some(Expr::Call(n, a)) if n == "@section_subscript" => a.first(),
                _ => arg,
            };
            let at =
                match arg {
                    Some(Expr::Section {
                        lower,
                        upper,
                        stride,
                        ..
                    }) => {
                        let step =
                            stride.as_deref().map(constant).unwrap_or(Some(1)).context(
                                "formatted READ section stride must currently be constant",
                            )?;
                        let first =
                            lower.as_deref().map(constant).unwrap_or(Some(lo)).context(
                                "formatted READ section bounds must currently be constant",
                            )?;
                        let last =
                            upper.as_deref().map(constant).unwrap_or(Some(hi)).context(
                                "formatted READ section bounds must currently be constant",
                            )?;
                        let at = values(first, last, step)?;
                        if at.iter().any(|i| *i < lo || *i > hi) {
                            bail!("READ section `{name}` selects an index outside {lo}:{hi}");
                        }
                        at.into_iter().map(int).collect::<Vec<_>>()
                    }
                    None => values(lo, hi, 1)?.into_iter().map(int).collect(),
                    Some(e) => vec![e.clone()],
                };
            let mut expanded = vec![];
            for i in at {
                for previous in &indices {
                    if expanded.len() >= LIMIT {
                        bail!("formatted READ input-list expansion exceeds {LIMIT} entries");
                    }
                    let mut next = previous.clone();
                    next.push(i.clone());
                    expanded.push(next);
                }
            }
            indices = expanded;
        }
        for at in indices {
            push(out, Expr::Index(name.clone(), at))?;
        }
    }
    Ok(())
}

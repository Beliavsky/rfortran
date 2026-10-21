// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! EQUIVALENCE of local variables of the same type. Each object of a set
//! after the first shares the first one's storage, so its references become
//! references to elements of the first object at the same storage offset, as
//! in `equivalence (cr(1,1), crv(1))`, where `crv(k)` is the k-th element of
//! the 2 by 2 array `cr` in column-major order.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// The statement that lists the EQUIVALENCE sets.
pub(crate) const STATEMENT: &str = "@equivalence";

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

/// The lower bound and extent of each dimension of a declared variable.
#[derive(Clone)]
struct Shape {
    kind: TypeSpec,
    dims: Vec<(i64, i64)>,
}
impl Shape {
    fn size(&self) -> i64 {
        self.dims.iter().map(|(_, e)| *e).product()
    }
    /// The storage offset of the element with constant `subscripts`.
    fn offset(&self, subscripts: &[Expr], name: &str) -> Result<i64> {
        if subscripts.is_empty() {
            return Ok(0);
        }
        if subscripts.len() != self.dims.len() {
            bail!("EQUIVALENCE object `{name}` has the wrong number of subscripts");
        }
        let mut offset = 0;
        let mut stride = 1;
        for (s, (lower, extent)) in subscripts.iter().zip(&self.dims) {
            let s = crate::parameters::integer_constant(s).context("EQUIVALENCE subscripts must be constant")?;
            if s < *lower || s >= lower + extent {
                bail!("EQUIVALENCE subscript of `{name}` is outside its bounds");
            }
            offset += (s - lower) * stride;
            stride *= extent;
        }
        Ok(offset)
    }
    /// The offset expression of an element with `subscripts`.
    fn linear(&self, subscripts: &[Expr]) -> Expr {
        let mut offset = int(0);
        let mut stride = 1;
        for (s, (lower, extent)) in subscripts.iter().zip(&self.dims) {
            offset = bin(BinOp::Add, offset, bin(BinOp::Mul, bin(BinOp::Sub, s.clone(), int(*lower)), int(stride)));
            stride *= extent;
        }
        offset
    }
    /// The subscripts of the element at offset `offset`.
    fn subscripts(&self, offset: Expr) -> Vec<Expr> {
        let mut stride = 1;
        let rank = self.dims.len();
        self.dims
            .iter()
            .enumerate()
            .map(|(axis, (lower, extent))| {
                let quotient = bin(BinOp::Div, offset.clone(), int(stride));
                stride *= extent;
                let digit = if axis + 1 == rank { quotient } else { Expr::Call("mod".into(), vec![quotient, int(*extent)]) };
                bin(BinOp::Add, int(*lower), digit)
            })
            .collect()
    }
}

/// A variable that shares the storage of `primary` from offset `delta`.
struct Alias {
    primary: String,
    shape: Shape,
    primary_shape: Shape,
    delta: i64,
}

fn shapes(body: &[Stmt]) -> HashMap<String, Result<Shape>> {
    let mut out = HashMap::new();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    out.insert(n.clone(), Ok(Shape { kind: kind.clone(), dims: vec![] }));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                let dims = dims
                    .iter()
                    .map(|d| -> Result<(i64, i64)> {
                        // Named constants are not folded yet.
                        let constant = |e: &Expr| {
                            crate::data_statements::integer(body, e).map_or_else(|| crate::parameters::integer_constant(e), Ok)
                        };
                        let (lower, upper) = match d {
                            Expr::Call(tag, a) if tag == "@declared_bounds" => (constant(&a[0])?, constant(&a[1])?),
                            _ => (1, constant(d)?),
                        };
                        Ok((lower, (upper - lower + 1).max(0)))
                    })
                    .collect::<Result<Vec<_>>>()
                    .with_context(|| format!("EQUIVALENCE array `{name}` must have constant bounds"));
                out.insert(name.clone(), dims.map(|dims| Shape { kind: kind.clone(), dims }));
            }
            _ => {}
        }
    }
    out
}

fn object(e: &Expr) -> Result<(String, Vec<Expr>)> {
    match e {
        Expr::Ident(n) => Ok((n.clone(), vec![])),
        Expr::Call(n, a) | Expr::Index(n, a) => Ok((n.clone(), a.clone())),
        _ => bail!("EQUIVALENCE objects must be variables or array elements"),
    }
}

fn rewrite_expr(e: &mut Expr, aliases: &HashMap<String, Alias>) -> Result<()> {
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args.iter_mut() {
                rewrite_expr(a, aliases)?;
            }
        }
        Expr::Bin(_, a, b) => {
            rewrite_expr(a, aliases)?;
            rewrite_expr(b, aliases)?;
        }
        Expr::Un(_, a) => rewrite_expr(a, aliases)?,
        Expr::Section { name, .. } if aliases.contains_key(name.as_str()) => {
            bail!("a section of EQUIVALENCE object `{name}` is not yet supported")
        }
        _ => {}
    }
    let replacement = match e {
        Expr::Call(n, args) | Expr::Index(n, args) => aliases.get(n.as_str()).map(|a| element(a, args)),
        Expr::Ident(n) => match aliases.get(n.as_str()) {
            Some(a) if a.shape.dims.is_empty() => Some(element(a, &[])),
            Some(_) => bail!("whole-array use of EQUIVALENCE object `{n}` is not yet supported"),
            None => None,
        },
        _ => None,
    };
    if let Some(r) = replacement {
        *e = r;
    }
    Ok(())
}

/// The element of the primary object that `alias(subscripts)` shares.
fn element(alias: &Alias, subscripts: &[Expr]) -> Expr {
    let offset = bin(BinOp::Add, int(alias.delta), alias.shape.linear(subscripts));
    if alias.primary_shape.dims.is_empty() {
        Expr::Ident(alias.primary.clone())
    } else {
        Expr::Call(alias.primary.clone(), alias.primary_shape.subscripts(offset))
    }
}

fn rewrite(body: &mut [Stmt], aliases: &HashMap<String, Alias>) -> Result<()> {
    for s in body.iter_mut() {
        for e in crate::procedure_arguments::expressions(s) {
            rewrite_expr(e, aliases)?;
        }
        let target = match s {
            Stmt::Assign { name, value } => aliases.get(name.as_str()).map(|a| (element(a, &[]), value.clone())),
            Stmt::AssignIndex { name, indices, value } => aliases.get(name.as_str()).map(|a| (element(a, indices), value.clone())),
            _ => None,
        };
        if let Some((target, value)) = target {
            *s = match target {
                Expr::Call(name, indices) => Stmt::AssignIndex { name, indices, value },
                Expr::Ident(name) => Stmt::Assign { name, value },
                _ => unreachable!(),
            };
        }
        for b in crate::procedure_arguments::children(s) {
            rewrite(b, aliases)?;
        }
    }
    Ok(())
}

fn unit(body: &mut Vec<Stmt>) -> Result<()> {
    for s in body.iter_mut() {
        if let Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } | Stmt::Module { body, .. } = s {
            unit(body)?;
        }
    }
    let mut sets = vec![];
    let mut location = None;
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == STATEMENT => {
            sets.extend(args.iter().cloned());
            false
        }
        Stmt::SourceLocation(offset) => {
            location.get_or_insert(*offset);
            true
        }
        _ => true,
    });
    if sets.is_empty() {
        return Ok(());
    }
    let declared = shapes(body);
    let shape = |n: &str| -> Result<Shape> {
        match declared.get(n) {
            Some(Ok(s)) => Ok(s.clone()),
            Some(Err(e)) => bail!("{e:#}"),
            None => bail!("EQUIVALENCE object `{n}` must be declared before the EQUIVALENCE statement"),
        }
    };
    let mut aliases = HashMap::new();
    for set in sets {
        let Expr::Call(_, objects) = set else { unreachable!("EQUIVALENCE set") };
        let (primary, primary_subscripts) = object(&objects[0])?;
        let primary_shape = shape(&primary)?;
        let origin = primary_shape.offset(&primary_subscripts, &primary)?;
        for other in &objects[1..] {
            let (name, subscripts) = object(other)?;
            if aliases.contains_key(&name) || name == primary {
                bail!("`{name}` appears in more than one EQUIVALENCE set; this is not yet supported");
            }
            let shape = shape(&name)?;
            if shape.kind != primary_shape.kind {
                bail!("EQUIVALENCE of `{name}` and `{primary}`, which have different types, is not yet supported");
            }
            let delta = origin - shape.offset(&subscripts, &name)?;
            if delta < 0 || delta + shape.size() > primary_shape.size() {
                bail!("EQUIVALENCE of `{name}` must lie within `{primary}`; this is not yet supported");
            }
            aliases.insert(name, Alias { primary: primary.clone(), shape, primary_shape: primary_shape.clone(), delta });
        }
    }
    if body.iter().any(|s| matches!(s, Stmt::InitializedDecl { entities, .. } if entities.iter().any(|(n, _)| aliases.contains_key(n)))) {
        bail!("initialized EQUIVALENCE objects are not yet supported");
    }
    // The aliases have no storage of their own.
    body.retain(|s| !matches!(s, Stmt::ArrayDecl { name, .. } if aliases.contains_key(name)));
    for s in body.iter_mut() {
        if let Stmt::VarDecl { names, .. } = s {
            names.retain(|n| !aliases.contains_key(n));
        }
    }
    body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
    rewrite(body, &aliases).map_err(|e| crate::source_diagnostics::at(e, location))
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    unit(&mut p.body)?;
    Ok(p)
}

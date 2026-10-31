// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! EQUIVALENCE of local and COMMON variables. The objects of a set share a
//! storage point; sets with an object in common form one group. One object of
//! each group, the primary, keeps its storage, and the others become views of
//! it: their references become references to elements of the primary at the
//! same storage offset, as in `equivalence (cr(1,1), crv(1))`, where `crv(k)`
//! is the k-th element of the 2 by 2 array `cr` in column-major order.
//!
//! The primary is the group's COMMON object, whose storage other units
//! share, or else an object whose storage contains every other one. Objects
//! of different types whose elements have the same size, such as REAL and
//! INTEGER(4) or DOUBLE PRECISION and INTEGER(8), share bits: a read
//! reinterprets the primary's element and a write stores the value's bits.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// The statement that lists the EQUIVALENCE sets.
pub(crate) const STATEMENT: &str = "@equivalence";

/// The result types of the bit reinterpretation primitives.
pub(crate) fn bits_kind(name: &str) -> Option<TypeSpec> {
    match name {
        "@real32_bits" => Some(TypeSpec::Integer(Some(4))),
        "@real64_bits" => Some(TypeSpec::Integer(Some(8))),
        "@bits_real32" => Some(TypeSpec::Real),
        "@bits_real64" => Some(TypeSpec::DoublePrecision),
        _ => None,
    }
}

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

/// The bytes of one element of a type, for storage offsets.
fn element_bytes(kind: &TypeSpec) -> Option<i64> {
    let parse = |k: &str| k.parse::<i64>().ok();
    match kind {
        TypeSpec::Integer(k) => Some(k.map_or(4, i64::from)),
        TypeSpec::Real => Some(4),
        TypeSpec::DoublePrecision => Some(8),
        TypeSpec::RealKind(k) => parse(k),
        TypeSpec::Logical => Some(4),
        TypeSpec::LogicalKind(k) => parse(k),
        TypeSpec::Complex(k) => parse(k).map(|k| 2 * k),
        TypeSpec::Character(Some(n)) => i64::try_from(*n).ok(),
        _ => None,
    }
}

/// How a view of another type reads and writes the primary's elements.
#[derive(Clone)]
struct Conversion {
    /// The primitive that reinterprets a primary element as the view's type.
    read: &'static str,
    /// The primitive that gives a value of the view's type the primary's type.
    write: &'static str,
    /// The conversion of an assigned value to the view's type.
    cast: Expr,
}

fn conversion(view: &TypeSpec, primary: &TypeSpec) -> Option<Conversion> {
    let real32 = |k: &TypeSpec| matches!(k, TypeSpec::Real) || matches!(k, TypeSpec::RealKind(k) if k == "4");
    let real64 = |k: &TypeSpec| matches!(k, TypeSpec::DoublePrecision) || matches!(k, TypeSpec::RealKind(k) if k == "8");
    let int32 = |k: &TypeSpec| matches!(k, TypeSpec::Integer(None | Some(4)));
    let int64 = |k: &TypeSpec| matches!(k, TypeSpec::Integer(Some(8)));
    let cast = |name: &str, kind: Option<i64>| {
        let mut args = vec![Expr::Ident("@value".into())];
        if let Some(k) = kind { args.push(int(k)); }
        Expr::Call(name.into(), args)
    };
    if int32(view) && real32(primary) {
        Some(Conversion { read: "@real32_bits", write: "@bits_real32", cast: cast("int", None) })
    } else if real32(view) && int32(primary) {
        Some(Conversion { read: "@bits_real32", write: "@real32_bits", cast: cast("real", None) })
    } else if int64(view) && real64(primary) {
        Some(Conversion { read: "@real64_bits", write: "@bits_real64", cast: cast("int", Some(8)) })
    } else if real64(view) && int64(primary) {
        Some(Conversion { read: "@bits_real64", write: "@real64_bits", cast: cast("dble", None) })
    } else {
        None
    }
}

/// The lower bound and extent of each dimension of a declared variable.
#[derive(Clone)]
struct Shape {
    kind: TypeSpec,
    dims: Vec<(i64, i64)>,
    bytes: i64,
}
impl Shape {
    fn size(&self) -> i64 {
        self.dims.iter().map(|(_, e)| *e).product()
    }
    /// The element offset of the element with constant `subscripts`.
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

/// A variable that shares the storage of `primary` from element `delta`.
struct Alias {
    primary: String,
    shape: Shape,
    primary_shape: Shape,
    delta: i64,
    conversion: Option<Conversion>,
}

fn shape_of(kind: &TypeSpec, dims: &[Expr], body: &[Stmt], name: &str) -> Result<Shape> {
    let bytes = element_bytes(kind).with_context(|| format!("EQUIVALENCE object `{name}` has a type whose storage is not yet supported"))?;
    let dims = dims
        .iter()
        .map(|d| -> Result<(i64, i64)> {
            // Named constants are not folded yet.
            let constant = |e: &Expr| crate::data_statements::integer(body, e).map_or_else(|| crate::parameters::integer_constant(e), Ok);
            let (lower, upper) = match d {
                Expr::Call(tag, a) if tag == "@declared_bounds" => (constant(&a[0])?, constant(&a[1])?),
                _ => (1, constant(d)?),
            };
            Ok((lower, (upper - lower + 1).max(0)))
        })
        .collect::<Result<Vec<_>>>()
        .with_context(|| format!("EQUIVALENCE array `{name}` must have constant bounds"))?;
    Ok(Shape { kind: kind.clone(), dims, bytes })
}

/// The declared objects of a unit, including COMMON objects, which are USEd
/// from their block's module under the unit's names (see crate::common).
fn shapes(body: &[Stmt], commons: &HashMap<(String, String), (TypeSpec, Vec<Expr>)>) -> (HashMap<String, Result<Shape>>, HashMap<String, String>) {
    let mut out = HashMap::new();
    let mut common = HashMap::new();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    out.insert(n.clone(), shape_of(kind, &[], body, n));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                out.insert(name.clone(), shape_of(kind, dims, body, name));
            }
            Stmt::Use { module, renames, .. } => {
                for (local, remote) in renames {
                    if let Some((kind, dims)) = commons.get(&(module.clone(), remote.clone())) {
                        out.insert(local.clone(), shape_of(kind, dims, body, local));
                        common.insert(local.clone(), module.clone());
                    }
                }
            }
            _ => {}
        }
    }
    // DATA and SAVE turn declarations into initialized ones; an array keeps
    // its ArrayDecl too.
    for s in body {
        if let Stmt::InitializedDecl { kind, entities, .. } = s {
            for (n, _) in entities {
                out.entry(n.clone()).or_insert_with(|| shape_of(kind, &[], body, n));
            }
        }
    }
    (out, common)
}

/// Names a unit reads into or passes to a procedure, which define their
/// storage in place.
fn by_reference(body: &[Stmt], out: &mut std::collections::HashSet<String>) {
    for s in body {
        if let Stmt::CallSub { name, args } = s {
            let defined: &[Expr] = if let Some(start) = crate::input::transfer_start(name) {
                args.get(start..).unwrap_or(&[])
            } else if !name.starts_with('@') {
                args
            } else {
                &[]
            };
            for a in defined {
                if let Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) = a { out.insert(n.clone()); }
            }
        }
        let mut s = s.clone();
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) { continue; }
        for b in crate::procedure_arguments::children(&mut s) { by_reference(b, out); }
    }
}

fn object(e: &Expr) -> Result<(String, Vec<Expr>)> {
    match e {
        Expr::Ident(n) => Ok((n.clone(), vec![])),
        Expr::Call(n, a) | Expr::Index(n, a) => Ok((n.clone(), a.clone())),
        _ => bail!("EQUIVALENCE objects must be variables or array elements; substrings are not yet supported"),
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
        Expr::Call(n, args) | Expr::Index(n, args) => aliases.get(n.as_str()).map(|a| read(a, args)),
        Expr::Ident(n) => match aliases.get(n.as_str()) {
            Some(a) if a.shape.dims.is_empty() => Some(read(a, &[])),
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

/// The value of `alias(subscripts)`, reinterpreted when its type differs.
fn read(alias: &Alias, subscripts: &[Expr]) -> Expr {
    let element = element(alias, subscripts);
    match &alias.conversion {
        Some(c) => Expr::Call(c.read.into(), vec![element]),
        None => element,
    }
}

/// The value to store in the primary for `alias = value`.
fn written(alias: &Alias, value: Expr) -> Expr {
    let Some(c) = &alias.conversion else { return value };
    let mut cast = c.cast.clone();
    if let Expr::Call(_, args) = &mut cast {
        args[0] = value;
    }
    Expr::Call(c.write.into(), vec![cast])
}

/// Whether `e` names a view whose type differs from its primary's.
fn converted(e: &Expr, aliases: &HashMap<String, Alias>) -> Option<String> {
    match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) => aliases.get(n.as_str()).filter(|a| a.conversion.is_some()).map(|_| n.clone()),
        _ => None,
    }
}

fn rewrite(body: &mut [Stmt], aliases: &HashMap<String, Alias>) -> Result<()> {
    for s in body.iter_mut() {
        // Input and procedure arguments define their actuals in place, which a
        // reinterpreted value cannot do.
        if let Stmt::CallSub { name, args } = s {
            let defined: &[Expr] = if let Some(start) = crate::input::transfer_start(name) {
                args.get(start..).unwrap_or(&[])
            } else if !name.starts_with('@') {
                args
            } else {
                &[]
            };
            if let Some(n) = defined.iter().find_map(|a| converted(a, aliases)) {
                bail!("EQUIVALENCE object `{n}` has a different type from the object whose storage it shares; reading into it or passing it to a procedure is not yet supported");
            }
        }
        for e in crate::procedure_arguments::expressions(s) {
            rewrite_expr(e, aliases)?;
        }
        let target = match s {
            Stmt::Assign { name, value } => aliases.get(name.as_str()).map(|a| (element(a, &[]), written(a, value.clone()))),
            Stmt::AssignIndex { name, indices, value } => aliases.get(name.as_str()).map(|a| (element(a, indices), written(a, value.clone()))),
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

fn unit(body: &mut Vec<Stmt>, commons: &HashMap<(String, String), (TypeSpec, Vec<Expr>)>) -> Result<()> {
    for s in body.iter_mut() {
        if let Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } | Stmt::Module { body, .. } = s {
            unit(body, commons)?;
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
    let (declared, common) = shapes(body, commons);
    let initialized: std::collections::HashSet<String> = body.iter().flat_map(|s| match s {
        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
        _ => vec![],
    }).collect();
    let mut referenced = std::collections::HashSet::new();
    by_reference(body, &mut referenced);
    let shape = |n: &str| -> Result<Shape> {
        match declared.get(n) {
            Some(Ok(s)) => Ok(s.clone()),
            Some(Err(e)) => bail!("{e:#}"),
            None => bail!("EQUIVALENCE object `{n}` must be declared before the EQUIVALENCE statement"),
        }
    };
    // Each set's objects, with their element offsets.
    let mut members: Vec<Vec<(String, i64)>> = vec![];
    for set in sets {
        let Expr::Call(_, objects) = set else { unreachable!("EQUIVALENCE set") };
        let mut set = vec![];
        for o in &objects {
            let (name, subscripts) = object(o)?;
            let s = shape(&name)?;
            set.push((name.clone(), s.offset(&subscripts, &name)? * s.bytes));
        }
        members.push(set);
    }
    // Group sets that share an object, and place every object of a group at
    // a byte offset from the group's first object.
    let mut placed: Vec<HashMap<String, i64>> = vec![];
    for set in members {
        let groups: Vec<usize> = placed.iter().enumerate().filter(|(_, g)| set.iter().any(|(n, _)| g.contains_key(n))).map(|(i, _)| i).collect();
        let mut group = HashMap::new();
        // The storage point of the set, in the coordinates of the merged group.
        let mut point = None;
        for &i in groups.iter().rev() {
            let other = placed.remove(i);
            let (name, at) = set.iter().find(|(n, _)| other.contains_key(n)).unwrap();
            let other_point = other[name] + at;
            let shift = point.map_or(0, |p| p - other_point);
            point.get_or_insert(other_point);
            for (n, start) in other {
                let start = start + shift;
                if group.insert(n.clone(), start).is_some_and(|old| old != start) {
                    bail!("EQUIVALENCE sets give `{n}` two different storage positions");
                }
            }
        }
        let point = point.unwrap_or(0);
        for (n, at) in &set {
            let start = point - at;
            if group.insert(n.clone(), start).is_some_and(|old| old != start) {
                bail!("EQUIVALENCE sets give `{n}` two different storage positions");
            }
        }
        placed.push(group);
    }
    let mut aliases = HashMap::new();
    for group in placed {
        let span = |n: &str| -> Result<(i64, i64)> {
            let s = shape(n)?;
            Ok((group[n], group[n] + s.size() * s.bytes))
        };
        let in_common: Vec<&String> = group.keys().filter(|n| common.contains_key(n.as_str())).collect();
        let primary = match in_common.as_slice() {
            [one] => (*one).clone(),
            [] => {
                let mut names: Vec<&String> = group.keys().collect();
                // Prefer an initialized or SAVEd object, which keeps its
                // value, then one read into or passed to a procedure, which
                // must be real storage of its own type.
                // Then a REAL object, so that REAL uses, the more common ones
                // in bit manipulation, need no reinterpretation.
                let real = |n: &str| shape(n).is_ok_and(|s| matches!(s.kind, TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::RealKind(_)));
                names.sort_by_key(|n| (!initialized.contains(*n), !referenced.contains(*n), !real(n), (*n).clone()));
                names.into_iter()
                    .find(|p| {
                        let (start, end) = span(p).unwrap_or((0, -1));
                        group.keys().all(|o| span(o).is_ok_and(|(s, e)| s >= start && e <= end))
                    })
                    .cloned()
                    .with_context(|| format!("EQUIVALENCE objects `{}` overlap without one containing the others; this is not yet supported",
                        { let mut n: Vec<&String> = group.keys().collect(); n.sort(); n.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("`, `") }))?
            }
            _ => bail!("EQUIVALENCE associates COMMON objects `{}`; this is not yet supported",
                in_common.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("`, `")),
        };
        let primary_shape = shape(&primary)?;
        let (primary_start, primary_end) = span(&primary)?;
        for name in group.keys().filter(|n| **n != primary) {
            let shape = shape(name)?;
            let (start, end) = span(name)?;
            if start < primary_start || end > primary_end {
                bail!("EQUIVALENCE of `{name}` extends `{primary}`{}; this is not yet supported",
                    if common.contains_key(primary.as_str()) { ", a COMMON object," } else { "" });
            }
            let conversion = if format!("{:?}", shape.kind) == format!("{:?}", primary_shape.kind) {
                None
            } else if shape.bytes == primary_shape.bytes {
                Some(conversion(&shape.kind, &primary_shape.kind).with_context(|| format!(
                    "EQUIVALENCE of `{name}` and `{primary}`, of types {} and {}, is not yet supported",
                    crate::sema::show_type(&shape.kind), crate::sema::show_type(&primary_shape.kind)))?)
            } else {
                bail!("EQUIVALENCE of `{name}` and `{primary}`, whose elements have different sizes, is not yet supported");
            };
            let delta = start - primary_start;
            if delta % primary_shape.bytes != 0 {
                bail!("EQUIVALENCE of `{name}` does not start at an element of `{primary}`; this is not yet supported");
            }
            aliases.insert(name.clone(), Alias {
                primary: primary.clone(),
                shape,
                primary_shape: primary_shape.clone(),
                delta: delta / primary_shape.bytes,
                conversion,
            });
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
    // The declarations of COMMON block modules, by (module, variable).
    let mut commons = HashMap::new();
    for s in &p.body {
        if let Stmt::Module { name, body } = s {
            if !crate::common::is_block_module(name) { continue; }
            for d in body {
                match d {
                    Stmt::VarDecl { kind, names } => for n in names { commons.insert((name.clone(), n.clone()), (kind.clone(), vec![])); },
                    Stmt::ArrayDecl { kind, name: n, dims } => { commons.insert((name.clone(), n.clone()), (kind.clone(), dims.clone())); }
                    _ => {}
                }
            }
        }
    }
    unit(&mut p.body, &commons)?;
    Ok(p)
}

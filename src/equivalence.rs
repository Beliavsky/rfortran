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

/// `@storage_view(object, storage, offset)`: a COMMON object, or an object
/// equivalenced to one, that is the elements of a COMMON block's storage
/// array from element offset + 1 (see crate::common).
pub(crate) const VIEW: &str = "@storage_view";

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
pub(crate) fn element_bytes(kind: &TypeSpec) -> Option<i64> {
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
pub(crate) struct Shape {
    pub(crate) kind: TypeSpec,
    pub(crate) dims: Vec<(i64, i64)>,
    pub(crate) bytes: i64,
}
impl Shape {
    pub(crate) fn size(&self) -> i64 {
        self.dims.iter().map(|(_, e)| *e).product()
    }
    /// The element offset of the element with constant `subscripts`.
    pub(crate) fn offset(&self, subscripts: &[Expr], name: &str) -> Result<i64> {
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

/// The `count` initial values of an initialization: the items of an array
/// constructor, with DATA repeat counts expanded, or one value for each.
fn expand(value: &Expr, count: i64) -> Result<Vec<Expr>> {
    let Expr::Call(n, items) = value else { return Ok(vec![value.clone(); count as usize]) };
    if n != "@array_constructor" { return Ok(vec![value.clone(); count as usize]); }
    let mut out = vec![];
    for item in items {
        match item {
            Expr::Call(r, a) if r == "@data_repeat" && a.len() == 2 => {
                let times = crate::parameters::integer_constant(&a[0]).context("a DATA repeat count of an EQUIVALENCE object must be constant")?;
                out.extend(std::iter::repeat_n(a[1].clone(), times.max(0) as usize));
            }
            other => out.push(other.clone()),
        }
    }
    if out.len() as i64 != count { bail!("an initialization of an EQUIVALENCE object has {} values for {count} elements", out.len()); }
    Ok(out)
}

/// A storage object as messages name it.
fn shown(primary: &str) -> String {
    if primary.starts_with("rfort_equivalence_") {
        "the storage it shares".into()
    } else if let Some(block) = primary.strip_suffix("_storage") {
        if block == "blank" { "blank COMMON".into() } else { format!("COMMON block /{block}/") }
    } else {
        format!("`{primary}`")
    }
}

/// The view of `primary` that `name` is, from byte `offset` of the primary.
fn view(name: &str, shape: Shape, primary: &str, primary_shape: &Shape, offset: i64) -> Result<Alias> {
    let conversion = if same_type(&shape.kind, &primary_shape.kind) {
        None
    } else if shape.bytes == primary_shape.bytes {
        Some(conversion(&shape.kind, &primary_shape.kind).with_context(|| format!(
            "storage association of `{name}` with {}, of types {} and {}, is not yet supported",
            shown(primary), crate::sema::show_type(&shape.kind), crate::sema::show_type(&primary_shape.kind)))?)
    } else if wide_kind(&shape.kind) && matches!(primary_shape.kind, TypeSpec::Integer(Some(_))) && shape.bytes % primary_shape.bytes == 0 {
        None
    } else if matches!(shape.kind, TypeSpec::Character(_)) && matches!(primary_shape.kind, TypeSpec::Character(_)) && shape.bytes % primary_shape.bytes == 0 {
        None
    } else {
        bail!("storage association of `{name}` with {}, whose elements have different sizes, is not yet supported", shown(primary));
    };
    if offset % primary_shape.bytes != 0 {
        bail!("`{name}` does not start at an element of {}; this is not yet supported", shown(primary));
    }
    let width = shape.bytes / primary_shape.bytes;
    Ok(Alias { primary: primary.into(), shape, primary_shape: primary_shape.clone(), delta: offset / primary_shape.bytes, conversion, width })
}

/// A variable that shares the storage of `primary` from element `delta`.
struct Alias {
    primary: String,
    shape: Shape,
    primary_shape: Shape,
    delta: i64,
    conversion: Option<Conversion>,
    /// The INTEGER storage elements of one element of a wider view, such as
    /// two INTEGER(4) words of a DOUBLE PRECISION element; 1 otherwise.
    width: i64,
}

pub(crate) fn shape_of(kind: &TypeSpec, dims: &[Expr], body: &[Stmt], name: &str) -> Result<Shape> {
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

/// The name of an EQUIVALENCE object.
pub(crate) fn object_name(e: &Expr) -> Result<String> {
    match e {
        Expr::Call(n, a) if n == "@array_substring" && a.len() == 2 => object_name(&a[0]),
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Ok(n.clone()),
        _ => bail!("EQUIVALENCE objects must be variables, array elements or substrings"),
    }
}

/// The byte offset, from the start of its variable, of an EQUIVALENCE
/// object: an element, or a substring `c(i:j)` of a CHARACTER scalar.
pub(crate) fn object_offset(e: &Expr, shape: &Shape) -> Result<i64> {
    // The start of a substring range, from 1.
    let start = |lower: Option<&Expr>, name: &str| -> Result<i64> {
        let start = lower.map(crate::parameters::integer_constant).transpose()
            .context("EQUIVALENCE substring bounds must be constant")?.unwrap_or(1);
        if !matches!(shape.kind, TypeSpec::Character(_)) { bail!("EQUIVALENCE object `{name}`: substrings require a CHARACTER variable"); }
        if start < 1 || start > shape.bytes { bail!("EQUIVALENCE substring of `{name}` is outside its length"); }
        Ok(start - 1)
    };
    match e {
        Expr::Call(n, a) if n == "@array_substring" && a.len() == 2 => {
            let lower = match &a[1] { Expr::Section { lower, .. } => lower.as_deref(), _ => bail!("invalid EQUIVALENCE substring") };
            let name = object_name(&a[0])?;
            Ok(object_offset(&a[0], shape)? + start(lower, &name)?)
        }
        Expr::Section { name, lower, .. } => {
            if !matches!(shape.kind, TypeSpec::Character(_)) || !shape.dims.is_empty() {
                bail!("EQUIVALENCE object `{name}`: only substrings of CHARACTER scalars are supported");
            }
            let start = lower.as_deref().map(crate::parameters::integer_constant).transpose()
                .context("EQUIVALENCE substring bounds must be constant")?.unwrap_or(1);
            if start < 1 || start > shape.bytes { bail!("EQUIVALENCE substring of `{name}` is outside its length"); }
            Ok(start - 1)
        }
        _ => {
            let (name, subscripts) = object(e)?;
            Ok(shape.offset(&subscripts, &name)? * shape.bytes)
        }
    }
}

pub(crate) fn object(e: &Expr) -> Result<(String, Vec<Expr>)> {
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
        Expr::Section { name, lower, upper, stride } => {
            for b in [&mut *lower, &mut *upper, &mut *stride].into_iter().flatten() { rewrite_expr(b, aliases)?; }
            if let Some(a) = aliases.get(name.as_str()) {
                let name = name.clone();
                *e = section(a, lower.as_deref(), upper.as_deref(), stride.as_deref())
                    .with_context(|| format!("this section of EQUIVALENCE object `{name}` is not yet supported"))?;
            }
            return Ok(());
        }
        _ => {}
    }
    let replacement = match e {
        Expr::Call(n, args) | Expr::Index(n, args) => match aliases.get(n.as_str()) {
            Some(_) if args.iter().any(|x| matches!(x, Expr::Section { .. }) || matches!(x, Expr::Call(t, _) if t == "@section_subscript" || t == "@vector_subscript")) =>
                bail!("sections of the multidimensional EQUIVALENCE object `{n}` are not yet supported"),
            Some(a) => Some(read(a, args)),
            None => None,
        },
        Expr::Ident(n) => match aliases.get(n.as_str()) {
            Some(a) if a.shape.dims.is_empty() => Some(read(a, &[])),
            Some(a) => Some(whole(a).with_context(|| format!("whole-array use of EQUIVALENCE object `{n}` is not yet supported"))?),
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

/// The elements of a rank-one primary that a rank-one alias shares, as the
/// section `primary(lo:hi:stride)` for `alias(lower:upper:stride)`.
fn section(alias: &Alias, lower: Option<&Expr>, upper: Option<&Expr>, stride: Option<&Expr>) -> Option<Expr> {
    if alias.conversion.is_some() || alias.width > 1 || alias.shape.dims.len() != 1 || alias.primary_shape.dims.len() != 1 { return None; }
    let (alias_lower, extent) = alias.shape.dims[0];
    let primary_lower = alias.primary_shape.dims[0].0;
    // Alias element k is primary element k - alias_lower + primary_lower + delta.
    let shift = primary_lower + alias.delta - alias_lower;
    let moved = |e: Expr| bin(BinOp::Add, e, int(shift));
    Some(Expr::Section {
        name: alias.primary.clone(),
        lower: Some(Box::new(moved(lower.cloned().unwrap_or(int(alias_lower))))),
        upper: Some(Box::new(moved(upper.cloned().unwrap_or(int(alias_lower + extent - 1))))),
        stride: stride.cloned().map(Box::new),
    })
}

/// The whole array `alias`: a section of a rank-one primary, reshaped to
/// the alias's shape when it has more than one dimension.
fn whole(alias: &Alias) -> Option<Expr> {
    if alias.conversion.is_some() || alias.width > 1 || alias.primary_shape.dims.len() != 1 { return None; }
    let lo = alias.primary_shape.dims[0].0 + alias.delta;
    let section = Expr::Section {
        name: alias.primary.clone(),
        lower: Some(Box::new(int(lo))),
        upper: Some(Box::new(int(lo + alias.shape.size() - 1))),
        stride: None,
    };
    if alias.shape.dims.len() == 1 { return Some(section); }
    let extents = alias.shape.dims.iter().map(|(_, e)| int(*e)).collect();
    Some(Expr::Call("reshape".into(), vec![section, Expr::Call("@array_constructor".into(), extents)]))
}

/// Whether two types are the same, taking default kinds into account.
fn same_type(a: &TypeSpec, b: &TypeSpec) -> bool {
    let normal = |k: &TypeSpec| match k {
        TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)),
        TypeSpec::Real => TypeSpec::RealKind("4".into()),
        TypeSpec::DoublePrecision => TypeSpec::RealKind("8".into()),
        TypeSpec::Logical => TypeSpec::LogicalKind("4".into()),
        k => k.clone(),
    };
    format!("{:?}", normal(a)) == format!("{:?}", normal(b))
}

/// Types whose elements can span several INTEGER storage elements.
fn wide_kind(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::Integer(Some(4 | 8)) | TypeSpec::Integer(None) | TypeSpec::Real | TypeSpec::DoublePrecision)
        || matches!(kind, TypeSpec::RealKind(k) if k == "4" || k == "8")
}

/// The storage elements of element `subscripts` of a wider view.
fn pieces(alias: &Alias, subscripts: &[Expr]) -> Vec<Expr> {
    let base = bin(BinOp::Add, int(alias.delta), bin(BinOp::Mul, alias.shape.linear(subscripts), int(alias.width)));
    (0..alias.width).map(|j| Expr::Call(alias.primary.clone(), alias.primary_shape.subscripts(bin(BinOp::Add, base.clone(), int(j))))).collect()
}

/// The unsigned value of a storage element of `bytes` bytes, as INTEGER(8).
fn unsigned(e: Expr, bytes: i64) -> Expr {
    let mask = (1i128 << (8 * bytes)) - 1;
    Expr::Call("iand".into(), vec![Expr::Call("int".into(), vec![e, int(8)]), Expr::IntLit(format!("{mask}_8"))])
}

/// The value of element `subscripts` of a wider view, from its pieces in
/// little-endian order.
fn wide_read(alias: &Alias, subscripts: &[Expr]) -> Expr {
    let unit = alias.primary_shape.bytes;
    if matches!(alias.shape.kind, TypeSpec::Character(_)) {
        return pieces(alias, subscripts).into_iter().reduce(|a, b| bin(BinOp::Concat, a, b)).unwrap();
    }
    let bits = pieces(alias, subscripts).into_iter().enumerate().map(|(j, w)| {
        let piece = unsigned(w, unit);
        if j == 0 { piece } else { Expr::Call("ishft".into(), vec![piece, int(8 * unit * j as i64)]) }
    }).reduce(|a, b| Expr::Call("ior".into(), vec![a, b])).unwrap();
    match &alias.shape.kind {
        TypeSpec::Integer(Some(8)) => bits,
        TypeSpec::Integer(_) => bin(BinOp::Sub, bits.clone(), Expr::Call("merge".into(), vec![
            Expr::IntLit("4294967296_8".into()), Expr::IntLit("0_8".into()), bin(BinOp::Ge, bits, Expr::IntLit("2147483648_8".into()))])),
        TypeSpec::DoublePrecision => Expr::Call("@bits_real64".into(), vec![bits]),
        TypeSpec::RealKind(k) if k == "8" => Expr::Call("@bits_real64".into(), vec![bits]),
        _ => Expr::Call("@bits_real32".into(), vec![bits]),
    }
}

/// The statements for `alias(subscripts) = value` of a wider view: the
/// value's bits, then each storage element from its piece of them.
fn wide_write(alias: &Alias, subscripts: &[Expr], value: Expr, serial: &mut usize) -> Stmt {
    let unit = alias.primary_shape.bytes;
    if let TypeSpec::Character(_) = &alias.shape.kind {
        // The value, padded or truncated to the view's length, then each
        // storage element from its substring.
        *serial += 1;
        let temp = format!("@equivalence_text_{serial}");
        let mut body = vec![
            Stmt::VarDecl { kind: alias.shape.kind.clone(), names: vec![temp.clone()] },
            Stmt::Assign { name: temp.clone(), value },
        ];
        for (j, target) in pieces(alias, subscripts).into_iter().enumerate() {
            let j = j as i64;
            let text = Expr::Section { name: temp.clone(), lower: Some(Box::new(int(j * unit + 1))), upper: Some(Box::new(int((j + 1) * unit))), stride: None };
            let Expr::Call(name, indices) = target else { unreachable!() };
            body.push(Stmt::AssignIndex { name, indices, value: text });
        }
        return Stmt::Block { body };
    }
    let bits = match &alias.shape.kind {
        TypeSpec::Integer(_) => Expr::Call("int".into(), vec![value, int(8)]),
        TypeSpec::DoublePrecision => Expr::Call("@real64_bits".into(), vec![Expr::Call("dble".into(), vec![value])]),
        TypeSpec::RealKind(k) if k == "8" => Expr::Call("@real64_bits".into(), vec![Expr::Call("dble".into(), vec![value])]),
        _ => Expr::Call("@real32_bits".into(), vec![Expr::Call("real".into(), vec![value])]),
    };
    *serial += 1;
    let temp = format!("@equivalence_bits_{serial}");
    let mut body = vec![
        Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![temp.clone()] },
        Stmt::Assign { name: temp.clone(), value: bits },
    ];
    let half = 1i128 << (8 * unit - 1);
    for (j, target) in pieces(alias, subscripts).into_iter().enumerate() {
        let shifted = Expr::Call("ishft".into(), vec![Expr::Ident(temp.clone()), int(-8 * unit * j as i64)]);
        let piece = unsigned(shifted, unit);
        // The piece as a signed value of the storage kind.
        let signed = bin(BinOp::Sub, piece.clone(), Expr::Call("merge".into(), vec![
            Expr::IntLit(format!("{}_8", 2 * half)), Expr::IntLit("0_8".into()), bin(BinOp::Ge, piece, Expr::IntLit(format!("{half}_8")))]));
        let Expr::Call(name, indices) = target else { unreachable!() };
        body.push(Stmt::AssignIndex { name, indices, value: Expr::Call("int".into(), vec![signed, int(unit)]) });
    }
    Stmt::Block { body }
}

/// The value of `alias(subscripts)`, reinterpreted when its type differs.
fn read(alias: &Alias, subscripts: &[Expr]) -> Expr {
    if alias.width > 1 { return wide_read(alias, subscripts); }
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
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) => aliases.get(n.as_str()).filter(|a| a.conversion.is_some() || a.width > 1).map(|_| n.clone()),
        _ => None,
    }
}

fn rewrite(body: &mut [Stmt], aliases: &HashMap<String, Alias>) -> Result<()> {
    let mut serial = 0;
    rewrite_with(body, aliases, &mut serial)
}

fn rewrite_with(body: &mut [Stmt], aliases: &HashMap<String, Alias>, serial: &mut usize) -> Result<()> {
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
            // A whole array passed to a procedure is its first element, which
            // is associated with the dummy's elements in storage order.
            if !name.starts_with('@') {
                for a in args.iter_mut() {
                    if let Expr::Ident(n) = a {
                        if let Some(alias) = aliases.get(n.as_str()).filter(|x| !x.shape.dims.is_empty()) {
                            let first: Vec<Expr> = alias.shape.dims.iter().map(|(l, _)| int(*l)).collect();
                            *a = element(alias, &first);
                        }
                    }
                }
            }
        }
        for e in crate::procedure_arguments::expressions(s) {
            rewrite_expr(e, aliases)?;
        }
        let wide = match s {
            Stmt::Assign { name, value } => aliases.get(name.as_str()).filter(|a| a.width > 1 && a.shape.dims.is_empty()).map(|a| wide_write(a, &[], value.clone(), serial)),
            Stmt::AssignIndex { name, indices, value } => aliases.get(name.as_str()).filter(|a| a.width > 1).map(|a| wide_write(a, indices, value.clone(), serial)),
            _ => None,
        };
        if let Some(w) = wide {
            *s = w;
            continue;
        }
        if let Stmt::Assign { name, .. } = s {
            if aliases.get(name.as_str()).is_some_and(|a| a.width > 1) {
                bail!("whole-array assignment to EQUIVALENCE object `{name}` is not yet supported");
            }
        }
        let target = match s {
            Stmt::Assign { name, value } => match aliases.get(name.as_str()) {
                Some(a) if !a.shape.dims.is_empty() => {
                    let Some(section) = whole(a).filter(|_| a.shape.dims.len() == 1) else {
                        bail!("whole-array assignment to EQUIVALENCE object `{name}` is not yet supported");
                    };
                    Some((section, value.clone()))
                }
                Some(a) => Some((element(a, &[]), written(a, value.clone()))),
                None => None,
            },
            Stmt::AssignIndex { name, indices, value } => match aliases.get(name.as_str()) {
                Some(a) => match indices.as_slice() {
                    [Expr::Section { lower, upper, stride, .. }] => {
                        let Some(section) = section(a, lower.as_deref(), upper.as_deref(), stride.as_deref()) else {
                            bail!("assignment to this section of EQUIVALENCE object `{name}` is not yet supported");
                        };
                        Some((section, value.clone()))
                    }
                    _ if indices.iter().any(|x| matches!(x, Expr::Section { .. }) || matches!(x, Expr::Call(t, _) if t == "@section_subscript" || t == "@vector_subscript")) =>
                        bail!("assignment to a section of the multidimensional EQUIVALENCE object `{name}` is not yet supported"),
                    _ => Some((element(a, indices), written(a, value.clone()))),
                },
                None => None,
            },
            _ => None,
        };
        if let Some((target, value)) = target {
            *s = match target {
                Expr::Call(name, indices) => Stmt::AssignIndex { name, indices, value },
                Expr::Ident(name) => Stmt::Assign { name, value },
                section @ Expr::Section { .. } => {
                    let Expr::Section { name, .. } = &section else { unreachable!() };
                    Stmt::AssignIndex { name: name.clone(), indices: vec![section.clone()], value }
                }
                _ => unreachable!(),
            };
        }
        for b in crate::procedure_arguments::children(s) {
            rewrite_with(b, aliases, serial)?;
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
    let mut views = vec![];
    let mut location = None;
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == STATEMENT => {
            sets.extend(args.iter().cloned());
            false
        }
        Stmt::CallSub { name, args } if name == VIEW => {
            views.push(args.clone());
            false
        }
        Stmt::SourceLocation(offset) => {
            location.get_or_insert(*offset);
            true
        }
        _ => true,
    });
    if sets.is_empty() && views.is_empty() {
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
            let name = object_name(o)?;
            let s = shape(&name)?;
            set.push((name.clone(), object_offset(o, &s)?));
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
    for args in &views {
        let [Expr::Ident(name), Expr::Ident(storage), offset] = args.as_slice() else { unreachable!("storage view") };
        let offset = crate::parameters::integer_constant(offset)?;
        let storage_shape = shape(storage)?;
        aliases.insert(name.clone(), view(name, shape(name)?, storage, &storage_shape, offset * storage_shape.bytes)?);
    }
    // Local storage for groups in which no object contains the others.
    let mut storage = vec![];
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
                let preferred = names[0].clone();
                let sizes: Vec<i64> = group.keys().map(|n| shape(n).map(|s| s.bytes)).collect::<Result<_>>()?;
                let origin = group.values().copied().min().unwrap();
                // One element size, and every object starting at an element.
                let uniform = sizes.iter().all(|b| *b == sizes[0]) && group.values().all(|start| (start - origin) % sizes[0] == 0);
                match names.into_iter().filter(|_| uniform).find(|p| {
                    let (start, end) = span(p).unwrap_or((0, -1));
                    group.keys().all(|o| span(o).is_ok_and(|(s, e)| s >= start && e <= end))
                }) {
                    Some(p) => p.clone(),
                    None => {
                        // Every object becomes a view of a new array that spans
                        // the group: of the preferred object's type, or for
                        // elements of different sizes, INTEGER elements of
                        // their greatest common size.
                        fn gcd(a: i64, b: i64) -> i64 { if b == 0 { a } else { gcd(b, a % b) } }
                        let unit = sizes.iter().copied().chain(group.values().map(|s| s - origin)).reduce(gcd).unwrap();
                        let characters = group.keys().map(|n| shape(n).map(|s| matches!(s.kind, TypeSpec::Character(_)))).collect::<Result<Vec<_>>>()?;
                        let kind = if uniform { shape(&preferred)?.kind } else if characters.iter().all(|c| *c) {
                            TypeSpec::Character(Some(unit as usize))
                        } else if characters.iter().any(|c| *c) {
                            bail!("EQUIVALENCE of CHARACTER and other objects, such as `{preferred}`, is not yet supported");
                        } else {
                            if !matches!(unit, 1 | 2 | 4 | 8) { bail!("EQUIVALENCE objects `{preferred}` have no common element size; this is not yet supported"); }
                            TypeSpec::Integer(Some(unit as u8))
                        };
                        let bytes = element_bytes(&kind).unwrap();
                        let first = group.values().copied().min().unwrap();
                        let last = group.keys().map(|n| span(n).map(|(_, e)| e)).collect::<Result<Vec<_>>>()?.into_iter().max().unwrap();
                        if (last - first) % bytes != 0 {
                            bail!("EQUIVALENCE objects `{}` do not span whole elements of their storage; this is not yet supported", preferred);
                        }
                        let name = format!("rfort_equivalence_{}", storage.len() + 1);
                        let storage_shape = Shape { kind: kind.clone(), dims: vec![(1, (last - first) / bytes)], bytes };
                        for (n, start) in &group {
                            aliases.insert(n.clone(), view(n, shape(n)?, &name, &storage_shape, start - first)?);
                        }
                        storage.push((name, kind, (last - first) / bytes));
                        continue;
                    }
                }
            }
            _ => bail!("EQUIVALENCE associates COMMON objects `{}`; this is not yet supported",
                in_common.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("`, `")),
        };
        let primary_shape = shape(&primary)?;
        let (primary_start, primary_end) = span(&primary)?;
        for name in group.keys().filter(|n| **n != primary) {
            let (start, end) = span(name)?;
            if start < primary_start || end > primary_end {
                bail!("EQUIVALENCE of `{name}` extends `{primary}`{}; this is not yet supported",
                    if common.contains_key(primary.as_str()) { ", a COMMON object," } else { "" });
            }
            aliases.insert(name.clone(), view(name, shape(name)?, &primary, &primary_shape, start - primary_start)?);
        }
    }
    // A SAVE of a view saves its storage. The initial values of views become
    // initial values of the storage at the views' offsets.
    let mut saved_storage = std::collections::HashSet::new();
    let mut initial: HashMap<String, Vec<(i64, Expr, i64)>> = HashMap::new();
    for s in body.iter_mut() {
        if let Stmt::InitializedDecl { entities, .. } = s {
            for (n, value) in entities.iter() {
                if let Some(a) = aliases.get(n) {
                    saved_storage.insert(a.primary.clone());
                    if let Some(value) = value {
                        if a.width != 1 || a.conversion.is_some() || common.contains_key(a.primary.as_str()) || commons.keys().any(|(_, v)| *v == a.primary) {
                            bail!("initialized EQUIVALENCE object `{n}` is not yet supported here; initialize an object of the type of the storage it shares");
                        }
                        initial.entry(a.primary.clone()).or_default().push((a.delta, value.clone(), a.shape.size()));
                    }
                }
            }
            entities.retain(|(n, _)| !aliases.contains_key(n));
        }
    }
    body.retain(|s| !matches!(s, Stmt::InitializedDecl { entities, .. } if entities.is_empty()));
    let local_storage: HashMap<String, (TypeSpec, i64)> = storage.iter().map(|(n, k, e)| (n.clone(), (k.clone(), *e))).collect();
    for (primary, values) in initial {
        let (kind, size, dims) = match local_storage.get(&primary) {
            Some((kind, extent)) => (kind.clone(), *extent, true),
            None => { let s = shape(&primary)?; (s.kind.clone(), s.size(), !s.dims.is_empty()) }
        };
        // The primary's own initial values, then the views' over them.
        let mut cells: Vec<Option<Expr>> = vec![None; size as usize];
        let mut existing_attributes = vec![];
        for s in body.iter_mut() {
            if let Stmt::InitializedDecl { attributes, entities, .. } = s {
                if let Some(i) = entities.iter().position(|(n, _)| *n == primary) {
                    if let Some(v) = entities[i].1.clone() {
                        for (k, v) in expand(&v, size)?.into_iter().enumerate() { cells[k] = Some(v); }
                    }
                    existing_attributes = attributes.iter().filter(|a| *a != "parameter").cloned().collect();
                    entities.remove(i);
                }
            }
        }
        body.retain(|s| !matches!(s, Stmt::InitializedDecl { entities, .. } if entities.is_empty()));
        for (delta, value, count) in values {
            for (k, v) in expand(&value, count)?.into_iter().enumerate() {
                let cell = &mut cells[(delta + k as i64) as usize];
                if cell.is_some() { bail!("EQUIVALENCE objects give storage element {} of `{primary}` two initial values", delta + k as i64 + 1); }
                *cell = Some(v);
            }
        }
        let zero = match &kind {
            TypeSpec::Character(Some(n)) => Expr::Str(" ".repeat(*n)),
            TypeSpec::Logical | TypeSpec::LogicalKind(_) => Expr::Logical(false),
            TypeSpec::Integer(_) => int(0),
            _ => Expr::RealLit("0.0".into()),
        };
        let cells: Vec<Expr> = cells.into_iter().map(|c| c.unwrap_or_else(|| zero.clone())).collect();
        let value = if dims { Expr::Call("@array_constructor".into(), cells) } else { cells.into_iter().next().unwrap() };
        saved_storage.remove(&primary);
        body.push(Stmt::InitializedDecl { kind, attributes: existing_attributes, entities: vec![(primary, Some(value))] });
    }
    let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
    let mut declarations = vec![];
    for (name, kind, extent) in storage {
        declarations.push(Stmt::ArrayDecl { kind: kind.clone(), name: name.clone(), dims: vec![int(extent)] });
        if saved_storage.contains(&name) {
            declarations.push(Stmt::InitializedDecl { kind, attributes: vec!["save".into()], entities: vec![(name, None)] });
        }
    }
    body.splice(at..at, declarations);
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

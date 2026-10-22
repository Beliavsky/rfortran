// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! COMPLEX has two REAL components. Lower both together before record/array
//! lowering, so assignment snapshots, descriptors and definition checks are shared.
//! DOT_PRODUCT uses this original type/rank information for all intrinsic types,
//! before sections and multidimensional arrays are normalized.
use crate::ast::{BinOp, Expr, Implicit, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn real(n: &str, k: u8) -> Expr {
    Expr::RealLit(if k == 8 { format!("{n}d0") } else { n.into() })
}
fn call(n: &str, a: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), a)
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn add(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Add, a, b)
}
fn sub(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Sub, a, b)
}
fn mul(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Mul, a, b)
}
fn neg(a: Expr) -> Expr {
    Expr::Un(UnOp::Neg, Box::new(a))
}
fn typename(k: u8) -> String {
    format!("@complex{k}")
}
pub(crate) fn type_kind(n: &str) -> Option<u8> {
    n.strip_prefix("@complex")?
        .parse()
        .ok()
        .filter(|k| matches!(k, 4 | 8))
}
fn complex_kind(t: &TypeSpec) -> Option<u8> {
    if let TypeSpec::Complex(n) = t {
        n.parse().ok()
    } else {
        None
    }
}
fn component_type(k: u8) -> TypeSpec {
    if k == 8 {
        TypeSpec::DoublePrecision
    } else {
        TypeSpec::Real
    }
}
fn component_kind(t: &TypeSpec) -> u8 {
    if t == &TypeSpec::DoublePrecision {
        8
    } else {
        complex_kind(t).unwrap_or(4)
    }
}
fn cast(e: Expr, k: u8) -> Expr {
    if k == 8 {
        call("dble", vec![e])
    } else {
        call("real", vec![e, int(4)])
    }
}
#[derive(Clone)]
struct Info {
    kind: TypeSpec,
    dims: Vec<Expr>,
}
impl Info {
    fn scalar(kind: TypeSpec) -> Self {
        Self { kind, dims: vec![] }
    }
}
type Scope = HashMap<String, Info>;
#[derive(Clone)]
struct Function {
    result: Info,
    params: Vec<String>,
    arguments: Scope,
    elemental: bool,
}
#[derive(Clone)]
struct Pair {
    re: Expr,
    im: Expr,
    kind: u8,
    dims: Vec<Expr>,
}
impl Pair {
    fn pack(self) -> Expr {
        call(
            &if self.dims.is_empty() {
                typename(self.kind)
            } else {
                format!("@complex_array${}", self.kind)
            },
            vec![self.re, self.im],
        )
    }
    fn converted(self, kind: u8) -> Self {
        Self {
            re: cast(self.re, kind),
            im: cast(self.im, kind),
            kind,
            ..self
        }
    }
}
#[derive(Default)]
struct Lower {
    serial: usize,
    functions: HashMap<String, Function>,
    helpers: HashSet<(String, u8)>,
    /// Scalar temporaries of the current unit free for reuse, by type. A
    /// statement's temporaries are live until it, nested bodies included, is
    /// lowered, so a unit declares only as many as one statement needs.
    free: HashMap<String, Vec<String>>,
    live: Vec<(String, String)>,
}
fn declarations(body: &[Stmt], scope: &mut Scope) {
    for stmt in body {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    scope.insert(n.clone(), Info::scalar(kind.clone()));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                scope.insert(
                    name.clone(),
                    Info {
                        kind: kind.clone(),
                        dims: dims.clone(),
                    },
                );
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    scope
                        .entry(n.clone())
                        .or_insert_with(|| Info::scalar(kind.clone()));
                }
            }
            _ => {}
        }
    }
}
fn dims(n: &str, v: &Info) -> Vec<Expr> {
    v.dims
        .iter()
        .enumerate()
        .map(|(axis, d)| {
            if let Some(n) = crate::vector_subscripts::fixed_extent(d) {
                return int(n);
            }
            if matches!(d,Expr::Ident(tag) if tag=="@deferred_extent") {
                call("size", vec![id(n), int(axis as i64 + 1)])
            } else {
                d.clone()
            }
        })
        .collect()
}
fn selection_dims(n: &str, args: &[Expr], v: &Info, s: &Scope, l: &Lower) -> Result<Vec<Expr>> {
    if args.len() != v.dims.len() {
        bail!("array `{n}` requires {} subscripts", v.dims.len());
    }
    let mut result = vec![];
    for e in args {
        if matches!(e,Expr::Call(tag,_) if tag=="@section_subscript") {
            result.push(call(
                "size",
                vec![call(n, args.to_vec()), int(result.len() as i64 + 1)],
            ));
        } else if let Some(i) = l.info(e, s)? {
            result.extend(i.dims);
        }
    }
    Ok(result)
}
impl Lower {
    fn collect(&mut self, body: &[Stmt], inherited: &Scope) -> Result<()> {
        let mut scope = inherited.clone();
        declarations(body, &mut scope);
        for stmt in body {
            match stmt {
                Stmt::Function {
                    name,
                    params,
                    body,
                    return_type,
                    elemental,
                    ..
                } => {
                    let mut local = scope.clone();
                    declarations(body, &mut local);
                    let result_name = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                        _ => name,
                    };
                    let result = local
                        .get(result_name)
                        .cloned()
                        .or_else(|| return_type.clone().map(Info::scalar));
                    if let Some(result) = result {
                        self.functions.insert(
                            name.clone(),
                            Function {
                                result,
                                params: params.clone(),
                                arguments: local.clone(),
                                elemental: *elemental,
                            },
                        );
                    }
                    self.collect(body, &local)?;
                }
                Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. }
                | Stmt::Block { body } => self.collect(body, &scope)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn dot_product_info(&self, a: &[Expr], s: &Scope) -> Result<Info> {
        if a.len() != 2 {
            bail!("DOT_PRODUCT requires exactly two arguments");
        }
        let left = self
            .info(&a[0], s)?
            .context("DOT_PRODUCT: cannot determine VECTOR_A type and rank")?;
        let right = self
            .info(&a[1], s)?
            .context("DOT_PRODUCT: cannot determine VECTOR_B type and rank")?;
        if left.dims.len() != 1 || right.dims.len() != 1 {
            bail!("DOT_PRODUCT arguments must be rank-one arrays");
        }
        let numeric = |k: &TypeSpec| {
            matches!(
                k,
                TypeSpec::Integer(_)
                    | TypeSpec::Real
                    | TypeSpec::DoublePrecision
                    | TypeSpec::Complex(_)
            )
        };
        for kind in [&left.kind, &right.kind] {
            if matches!(kind,TypeSpec::Integer(Some(k)) if !matches!(k,1|2|4|8)) {
                bail!("DOT_PRODUCT currently supports INTEGER kinds 1, 2, 4 and 8");
            }
        }
        let logical = left.kind == TypeSpec::Logical && right.kind == TypeSpec::Logical;
        if !logical && !(numeric(&left.kind) && numeric(&right.kind)) {
            bail!("DOT_PRODUCT requires two numeric arrays or two LOGICAL arrays");
        }
        if let (Ok(a), Ok(b)) = (
            crate::parameters::integer_constant(&left.dims[0]),
            crate::parameters::integer_constant(&right.dims[0]),
        ) {
            if a != b {
                bail!("DOT_PRODUCT array lengths do not conform: {a} and {b}");
            }
        }
        let kind = if logical {
            TypeSpec::Logical
        } else if complex_kind(&left.kind).is_some() || complex_kind(&right.kind).is_some() {
            TypeSpec::Complex(
                component_kind(&left.kind)
                    .max(component_kind(&right.kind))
                    .to_string(),
            )
        } else {
            crate::vector_subscripts::merge(left.kind, right.kind)
        };
        Ok(Info::scalar(kind))
    }
    fn info(&self, e: &Expr, s: &Scope) -> Result<Option<Info>> {
        let scalar = |kind| Ok(Some(Info::scalar(kind)));
        match e {
            Expr::Ident(n) => {
                if let Some((base, field)) = n.rsplit_once('%') {
                    if matches!(field, "re" | "im") {
                        if let Some(v) = s.get(base) {
                            if let Some(k) = complex_kind(&v.kind) {
                                return Ok(Some(Info {
                                    kind: component_type(k),
                                    dims: dims(base, v),
                                }));
                            }
                        }
                    }
                }
                Ok(s.get(n).map(|v| Info {
                    dims: dims(n, v),
                    ..v.clone()
                }))
            }
            Expr::IntLit(_) => scalar(TypeSpec::Integer(None)),
            Expr::RealLit(n) => scalar(if n.to_ascii_lowercase().contains('d') {
                TypeSpec::DoublePrecision
            } else {
                TypeSpec::Real
            }),
            Expr::Logical(_) => scalar(TypeSpec::Logical),
            Expr::Str(n) => scalar(TypeSpec::Character(Some(n.len()))),
            Expr::Section { name, .. } => Ok(s.get(name).map(|v| Info {
                kind: v.kind.clone(),
                dims: vec![call("size", vec![e.clone()])],
            })),
            Expr::Un(UnOp::Not, a) => Ok(self.info(a, s)?.map(|mut i| {
                i.kind = TypeSpec::Logical;
                i
            })),
            Expr::Un(_, a) => self.info(a, s),
            Expr::Bin(op, a, b) => {
                let (Some(a), Some(b)) = (self.info(a, s)?, self.info(b, s)?) else {
                    return Ok(None);
                };
                let dimensions = if a.dims.is_empty() {
                    b.dims.clone()
                } else {
                    a.dims.clone()
                };
                let kind = if !matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) {
                    TypeSpec::Logical
                } else if complex_kind(&a.kind).is_some() || complex_kind(&b.kind).is_some() {
                    TypeSpec::Complex(
                        component_kind(&a.kind)
                            .max(component_kind(&b.kind))
                            .to_string(),
                    )
                } else {
                    crate::vector_subscripts::merge(a.kind, b.kind)
                };
                Ok(Some(Info {
                    kind,
                    dims: dimensions,
                }))
            }
            Expr::Call(n, _) if s.get(n).is_some_and(|v| v.dims.is_empty()) && !self.functions.contains_key(n) => {
                Ok(Some(Info::scalar(s[n].kind.clone())))
            }
            Expr::Call(n, a) | Expr::Index(n, a)
                if s.get(n).is_some_and(|v| !v.dims.is_empty()) =>
            {
                let v = &s[n];
                Ok(Some(Info {
                    kind: v.kind.clone(),
                    dims: selection_dims(n, a, v, s, self)?,
                }))
            }
            Expr::Call(n, a) if n == "@complex_literal" => {
                if a.len() != 2 {
                    bail!("a COMPLEX literal requires exactly two numeric constant components");
                }
                let mut k = 4;
                for e in a {
                    let i = self
                        .info(e, s)?
                        .context("unknown COMPLEX literal component type")?;
                    if !i.dims.is_empty()
                        || !matches!(
                            i.kind,
                            TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision
                        )
                    {
                        bail!("COMPLEX literal components must be INTEGER or REAL constants");
                    }
                    k = k.max(component_kind(&i.kind));
                }
                scalar(TypeSpec::Complex(k.to_string()))
            }
            Expr::Call(n, a) if n == "cmplx" => {
                let first = a.first().context("CMPLX requires X")?;
                let info = self
                    .info(first, s)?
                    .context("unknown CMPLX argument type")?;
                let k = a
                    .get(2)
                    .filter(|e| !crate::named_arguments::omitted(e))
                    .map(crate::parameters::integer_constant)
                    .transpose()?
                    .unwrap_or(4);
                if !matches!(k, 4 | 8) {
                    bail!("unsupported COMPLEX kind {k}; supported component kinds are 4 and 8");
                }
                let mut dimensions = info.dims;
                if let Some(y) = a.get(1).filter(|e| !crate::named_arguments::omitted(e)) {
                    let y = self.info(y, s)?.context("unknown CMPLX Y type")?;
                    if dimensions.is_empty() {
                        dimensions = y.dims;
                    }
                }
                Ok(Some(Info {
                    kind: TypeSpec::Complex(k.to_string()),
                    dims: dimensions,
                }))
            }
            Expr::Call(n, a) if n == "@integer_literal" => scalar(TypeSpec::Integer(Some(
                crate::parameters::integer_constant(&a[1])? as u8,
            ))),
            Expr::Call(n, a) if n == "@output_implied_do" && a.len() >= 5 => {
                let mut local = s.clone();
                if let Expr::Ident(n) = &a[0] {
                    local.insert(n.clone(), Info::scalar(TypeSpec::Integer(None)));
                }
                let constant = |e: &Expr| crate::parameters::integer_constant(e).ok();
                let length = match (constant(&a[1]), constant(&a[2]), constant(&a[3])) {
                    (_, _, Some(0)) => bail!("constructor implied-DO stride must not be zero"),
                    (Some(start), Some(end), Some(step)) => int(
                        if (step > 0 && end >= start) || (step < 0 && end <= start) { (end - start) / step + 1 } else { 0 },
                    ),
                    // Bounds known only at run time, as in `[(i, i=1,n)]`.
                    _ => Expr::Call("max".into(), vec![
                        Expr::Bin(BinOp::Div, Box::new(add(Expr::Bin(BinOp::Sub, Box::new(a[2].clone()), Box::new(a[1].clone())), a[3].clone())), Box::new(a[3].clone())),
                        int(0),
                    ]),
                };
                let first = self.info(&a[4], &local)?;
                Ok(first.map(|i| Info {
                    kind: i.kind,
                    dims: vec![length],
                }))
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                let mut kind = None;
                let mut lengths = vec![];
                for e in a {
                    let Some(i) = self.info(e, s)? else {
                        return Ok(None);
                    };
                    if i.dims.len() > 1 {
                        bail!("array constructor items must be scalar or rank one");
                    }
                    // Default INTEGER is INTEGER(4), as in `[i, i+1]`.
                    let normal = |k: &TypeSpec| match k { TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)), k => k.clone() };
                    if kind.as_ref().is_some_and(|k| normal(k) != normal(&i.kind)) {
                        bail!("array constructor items must have the same type and kind");
                    }
                    kind = Some(i.kind);
                    lengths.push(i.dims.first().cloned().unwrap_or(int(1)));
                }
                Ok(kind.map(|kind| Info {
                    kind,
                    dims: vec![lengths.into_iter().fold(int(0), add)],
                }))
            }
            Expr::Call(n, a)
                if matches!(
                    n.as_str(),
                    "real" | "dble" | "aimag" | "abs" | "conjg" | "int" | "@unary_plus"
                ) || crate::intrinsics::math_arity(n).is_some() =>
            {
                let Some(mut i) = a.first().map(|e| self.info(e, s)).transpose()?.flatten() else {
                    return Ok(None);
                };
                i.kind = match n.as_str() {
                    "real" => component_type(
                        a.get(1)
                            .filter(|e| !crate::named_arguments::omitted(e))
                            .map(crate::parameters::integer_constant)
                            .transpose()?
                            .map(|k| k as u8)
                            .unwrap_or(if complex_kind(&i.kind).is_some() {
                                component_kind(&i.kind)
                            } else {
                                4
                            }),
                    ),
                    "dble" => TypeSpec::DoublePrecision,
                    "int" => TypeSpec::Integer(None),
                    "aimag" | "abs" if complex_kind(&i.kind).is_some() => {
                        component_type(component_kind(&i.kind))
                    }
                    _ => i.kind,
                };
                Ok(Some(i))
            }
            // Intrinsics whose result has the kind of their arguments, and
            // their FORTRAN 77 specific forms.
            Expr::Call(n, a)
                if matches!(
                    n.as_str(),
                    "max" | "min" | "mod" | "modulo" | "sign" | "dim" | "max0" | "min0" | "amax1" | "amin1" | "dmax1"
                        | "dmin1" | "dsign" | "isign" | "dmod" | "amod" | "idim" | "ddim" | "dabs" | "iabs"
                ) && !s.contains_key(n) && !self.functions.contains_key(n) =>
            {
                let mut result: Option<Info> = None;
                for e in a {
                    let Some(i) = self.info(e, s)? else {
                        return Ok(None);
                    };
                    result = Some(match result {
                        None => i,
                        Some(r) => Info {
                            kind: crate::vector_subscripts::merge(r.kind, i.kind),
                            dims: if r.dims.is_empty() { i.dims } else { r.dims },
                        },
                    });
                }
                Ok(result)
            }
            Expr::Call(n, a)
                if matches!(n.as_str(), "nint" | "idnint" | "ifix" | "idint" | "ichar" | "iachar" | "len" | "len_trim" | "index")
                    && !s.contains_key(n) && !self.functions.contains_key(n) =>
            {
                let dims = match a.first().map(|e| self.info(e, s)).transpose()?.flatten() {
                    Some(i) if matches!(n.as_str(), "nint" | "idnint" | "ifix" | "idint") => i.dims,
                    _ => vec![],
                };
                Ok(Some(Info { kind: TypeSpec::Integer(None), dims }))
            }
            Expr::Call(n, a)
                if n == "dot_product" && !s.contains_key(n) && !self.functions.contains_key(n) =>
            {
                Ok(Some(self.dot_product_info(a, s)?))
            }
            Expr::Call(n, a)
                if matches!(
                    n.as_str(),
                    "reshape" | "transpose" | "pack" | "sum" | "product" | "matmul" | "merge"
                ) =>
            {
                let Some(mut i) = a.first().map(|e| self.info(e, s)).transpose()?.flatten() else {
                    return Ok(None);
                };
                match n.as_str() {
                    "reshape" => {
                        let Some(Expr::Call(tag, shape)) = a.get(1) else {
                            return Ok(None);
                        };
                        if tag != "@array_constructor" {
                            return Ok(None);
                        }
                        i.dims = shape.clone();
                    }
                    "transpose" => i.dims.reverse(),
                    "sum" | "product" => {
                        // The DIM argument, by keyword or position. A LOGICAL
                        // second argument is MASK, as in `sum(x, x > 0)`.
                        let mut dim = None;
                        for (position, e) in a.iter().enumerate().skip(1) {
                            match crate::named_arguments::keyword(e) {
                                Some(("dim", value)) => dim = Some(value),
                                Some(_) => {}
                                None if position == 1 && !crate::named_arguments::omitted(e)
                                    && !self.info(e, s)?.is_some_and(|i| matches!(i.kind, TypeSpec::Logical)) => dim = Some(e),
                                None => {}
                            }
                        }
                        if let Some(dim) = dim {
                            // The rank of a reduction along a runtime DIM is not
                            // known here.
                            let Ok(dim) = crate::parameters::integer_constant(dim) else { return Ok(None) };
                            if dim < 1 || dim as usize > i.dims.len() {
                                bail!("invalid reduction DIM");
                            }
                            i.dims.remove(dim as usize - 1);
                        } else {
                            i.dims.clear();
                        }
                    }
                    "matmul" => {
                        let b = self.info(&a[1], s)?.context("unknown MATMUL operand")?;
                        i.kind =
                            if complex_kind(&i.kind).is_some() || complex_kind(&b.kind).is_some() {
                                TypeSpec::Complex(
                                    component_kind(&i.kind)
                                        .max(component_kind(&b.kind))
                                        .to_string(),
                                )
                            } else {
                                crate::vector_subscripts::merge(i.kind, b.kind)
                            };
                        i.dims = match (i.dims.as_slice(), b.dims.as_slice()) {
                            ([rows, _], [_, cols]) => vec![rows.clone(), cols.clone()],
                            ([rows, _], [_]) => vec![rows.clone()],
                            ([_], [_, cols]) => vec![cols.clone()],
                            _ => bail!("MATMUL requires conforming rank-one or rank-two operands"),
                        };
                    }
                    "pack" => i.dims = vec![call("count", vec![a[1].clone()])],
                    _ => {}
                }
                Ok(Some(i))
            }
            Expr::Call(n, a) if self.functions.contains_key(n) => {
                let function = &self.functions[n];
                let mut result = function.result.clone();
                if function.elemental {
                    for argument in a {
                        if let Some(info) = self.info(argument, s)? {
                            if !info.dims.is_empty() {
                                result.dims = info.dims;
                                break;
                            }
                        }
                    }
                }
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }
    fn primitive(
        &mut self,
        kind: TypeSpec,
        dimensions: Vec<Expr>,
        value: Expr,
        s: &mut Scope,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        let key = format!("{kind:?}");
        let reused = if dimensions.is_empty() { self.free.get_mut(&key).and_then(Vec::pop) } else { None };
        let n = reused.clone().unwrap_or_else(|| {
            self.serial += 1;
            format!("@complex_value_{}", self.serial)
        });
        if dimensions.is_empty() {
            self.live.push((key, n.clone()));
        }
        if reused.is_some() {
        } else if dimensions.is_empty() {
            out.push(Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![n.clone()],
            });
        } else if dimensions
            .iter()
            .all(|e| crate::parameters::integer_constant(e).is_ok())
        {
            out.push(Stmt::ArrayDecl {
                kind: kind.clone(),
                name: n.clone(),
                dims: dimensions.clone(),
            });
        } else {
            out.push(Stmt::ArrayDecl {
                kind: kind.clone(),
                name: n.clone(),
                dims: vec![id("@deferred_extent"); dimensions.len()],
            });
            out.push(Stmt::DummyIntent {
                name: n.clone(),
                intent: "allocatable".into(),
            });
            out.push(Stmt::If {
                cond: call("allocated", vec![id(&n)]),
                then_body: vec![Stmt::CallSub {
                    name: "@deallocate".into(),
                    args: vec![id(&n)],
                }],
                else_body: None,
            });
            out.push(Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![call(&n, dimensions.clone())],
            });
        }
        out.push(Stmt::Assign {
            name: n.clone(),
            value,
        });
        s.insert(
            n.clone(),
            Info {
                kind,
                dims: dimensions,
            },
        );
        id(&n)
    }
    fn frozen(&mut self, p: Pair, s: &mut Scope, out: &mut Vec<Stmt>) -> Pair {
        let re = self.primitive(component_type(p.kind), p.dims.clone(), p.re, s, out);
        let im = self.primitive(component_type(p.kind), p.dims.clone(), p.im, s, out);
        Pair { re, im, ..p }
    }
    fn helper(&mut self, name: &str, k: u8, args: Vec<Expr>) -> Expr {
        self.helpers.insert((name.into(), k));
        call(&format!("@complex_{name}${k}"), args)
    }
    fn indices(&mut self, a: &mut [Expr], s: &mut Scope, out: &mut Vec<Stmt>) -> Result<()> {
        for e in a {
            if let Expr::Call(tag, args) = e {
                if tag == "@section_subscript" || tag == "@vector_subscript" {
                    self.indices(args, s, out)?;
                    continue;
                }
            }
            if let Expr::Section {
                lower,
                upper,
                stride,
                ..
            } = e
            {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.indices(std::slice::from_mut(e.as_mut()), s, out)?;
                }
                continue;
            }
            self.expression(e, s, out)?;
            if crate::parameters::integer_constant(e).is_ok() {
                continue;
            }
            let info = self
                .info(e, s)?
                .context("unknown complex array subscript type")?;
            if !matches!(info.kind, TypeSpec::Integer(_)) {
                bail!("complex array subscript must be INTEGER");
            }
            *e = self.primitive(info.kind, info.dims, e.clone(), s, out);
        }
        Ok(())
    }
    fn pair(&mut self, e: Expr, s: &mut Scope, out: &mut Vec<Stmt>) -> Result<Pair> {
        let info = self
            .info(&e, s)?
            .context("unknown COMPLEX expression type")?;
        let k = component_kind(&info.kind);
        let dimensions = info.dims.clone();
        if complex_kind(&info.kind).is_none() {
            let mut re = e;
            self.expression(&mut re, s, out)?;
            if !matches!(
                info.kind,
                TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision
            ) {
                bail!("COMPLEX conversion requires numeric arguments");
            }
            return Ok(Pair {
                re,
                im: real("0.0", k),
                kind: k,
                dims: dimensions,
            });
        }
        let (re, im) = match e {
            Expr::Ident(n) => (id(&format!("{n}%re")), id(&format!("{n}%im"))),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                let mut a = [Expr::Section {
                    name: String::new(),
                    lower,
                    upper,
                    stride,
                }];
                self.indices(&mut a, s, out)?;
                let Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } = a[0].clone()
                else {
                    unreachable!()
                };
                (
                    Expr::Section {
                        name: format!("{name}%re"),
                        lower: lower.clone(),
                        upper: upper.clone(),
                        stride: stride.clone(),
                    },
                    Expr::Section {
                        name: format!("{name}%im"),
                        lower,
                        upper,
                        stride,
                    },
                )
            }
            Expr::Call(n, mut a) | Expr::Index(n, mut a)
                if s.get(&n).is_some_and(|v| !v.dims.is_empty()) =>
            {
                self.indices(&mut a, s, out)?;
                (
                    call(&format!("{n}%re"), a.clone()),
                    call(&format!("{n}%im"), a),
                )
            }
            Expr::Call(n, a) if n == "@complex_literal" => {
                if a.len() != 2 {
                    bail!("a COMPLEX literal requires two components");
                }
                for component in &a {
                    if !constant_component(component) {
                        bail!("COMPLEX literal components must be numeric constants; use CMPLX for expressions");
                    }
                }
                (cast(a[0].clone(), k), cast(a[1].clone(), k))
            }
            Expr::Call(n, a) if n == "cmplx" => {
                let xinfo = self.info(&a[0], s)?.context("unknown CMPLX X type")?;
                if !matches!(
                    &xinfo.kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::Real
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Complex(_)
                ) {
                    bail!("CMPLX X must be INTEGER, REAL or COMPLEX");
                }
                if complex_kind(&xinfo.kind).is_some() {
                    if a.get(1)
                        .is_some_and(|e| !crate::named_arguments::omitted(e))
                    {
                        bail!("CMPLX Y must be absent when X is COMPLEX");
                    }
                    let p = self.pair(a[0].clone(), s, out)?.converted(k);
                    (p.re, p.im)
                } else {
                    let mut x = a[0].clone();
                    self.expression(&mut x, s, out)?;
                    let mut y = a
                        .get(1)
                        .filter(|e| !crate::named_arguments::omitted(e))
                        .cloned()
                        .unwrap_or(real("0.0", k));
                    self.expression(&mut y, s, out)?;
                    if self.info(&y, s)?.is_some_and(|i| {
                        !matches!(
                            i.kind,
                            TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision
                        )
                    }) {
                        bail!("CMPLX Y must be INTEGER or REAL");
                    }
                    (cast(x, k), cast(y, k))
                }
            }
            Expr::Un(UnOp::Neg, a) => {
                let p = self.pair(*a, s, out)?;
                (neg(p.re), neg(p.im))
            }
            Expr::Call(n, a) if n == "@unary_plus" => {
                let p = self.pair(a[0].clone(), s, out)?;
                (p.re, p.im)
            }
            Expr::Bin(op, a, b) => {
                let integer_power = matches!(op, BinOp::Pow)
                    && self
                        .info(&b, s)?
                        .is_some_and(|i| matches!(i.kind, TypeSpec::Integer(_)));
                let p = self.pair(*a, s, out)?.converted(k);
                let p = self.frozen(p, s, out);
                if integer_power {
                    let mut exponent = *b;
                    self.expression(&mut exponent, s, out)?;
                    let exponent =
                        self.primitive(TypeSpec::Integer(Some(8)), vec![], exponent, s, out);
                    let args = vec![p.re, p.im, exponent];
                    (
                        self.helper("powi_re", k, args.clone()),
                        self.helper("powi_im", k, args),
                    )
                } else {
                    let q = self.pair(*b, s, out)?.converted(k);
                    let q = self.frozen(q, s, out);
                    match op {
                        BinOp::Add => (add(p.re, q.re), add(p.im, q.im)),
                        BinOp::Sub => (sub(p.re, q.re), sub(p.im, q.im)),
                        BinOp::Mul => (
                            sub(
                                mul(p.re.clone(), q.re.clone()),
                                mul(p.im.clone(), q.im.clone()),
                            ),
                            add(mul(p.re, q.im), mul(p.im, q.re)),
                        ),
                        BinOp::Div => {
                            let args = vec![p.re, p.im, q.re, q.im];
                            (
                                self.helper("div_re", k, args.clone()),
                                self.helper("div_im", k, args),
                            )
                        }
                        BinOp::Pow => {
                            let args = vec![p.re, p.im, q.re, q.im];
                            (
                                self.helper("pow_re", k, args.clone()),
                                self.helper("pow_im", k, args),
                            )
                        }
                        _ => bail!("unsupported COMPLEX arithmetic operator"),
                    }
                }
            }
            Expr::Call(n, a) if n == "@output_implied_do" && a.len() >= 5 => {
                let mut local = s.clone();
                if let Expr::Ident(n) = &a[0] {
                    local.insert(n.clone(), Info::scalar(TypeSpec::Integer(None)));
                }
                let mut ra = a[..4].to_vec();
                let mut ia = ra.clone();
                for e in &a[4..] {
                    let mut inside = vec![];
                    let p = self.pair(e.clone(), &mut local, &mut inside)?.converted(k);
                    if !inside.is_empty() {
                        bail!(
                            "this COMPLEX constructor implied-DO expression is not yet supported"
                        );
                    }
                    ra.push(p.re);
                    ia.push(p.im);
                }
                (call(&n, ra), call(&n, ia))
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                let mut r = vec![];
                let mut i = vec![];
                for e in a {
                    let p = self.pair(e, s, out)?.converted(k);
                    r.push(p.re);
                    i.push(p.im);
                }
                (call("@array_constructor", r), call("@array_constructor", i))
            }
            Expr::Call(n, a) if matches!(n.as_str(), "reshape" | "transpose" | "pack" | "sum") => {
                let p = self.pair(a[0].clone(), s, out)?;
                let mut ra = vec![p.re];
                let mut ia = vec![p.im];
                for mut e in a.into_iter().skip(1) {
                    self.expression(&mut e, s, out)?;
                    ra.push(e.clone());
                    ia.push(e);
                }
                (call(&n, ra), call(&n, ia))
            }
            Expr::Call(n, a) if n == "dot_product" || n == "matmul" => {
                let ac = self
                    .info(&a[0], s)?
                    .is_some_and(|i| complex_kind(&i.kind).is_some());
                let bc = self
                    .info(&a[1], s)?
                    .is_some_and(|i| complex_kind(&i.kind).is_some());
                let p = self.pair(a[0].clone(), s, out)?.converted(k);
                let p = self.frozen(p, s, out);
                let q = self.pair(a[1].clone(), s, out)?.converted(k);
                let q = self.frozen(q, s, out);
                if n == "matmul" {
                    let (re, im) = self.matrix(p, q, dimensions.clone(), s, out)?;
                    return Ok(Pair {
                        re,
                        im,
                        kind: k,
                        dims: dimensions,
                    });
                }
                let rr = if n == "dot_product" {
                    call("sum", vec![mul(p.re.clone(), q.re.clone())])
                } else {
                    call(&n, vec![p.re.clone(), q.re.clone()])
                };
                let ii = if ac && bc {
                    if n == "dot_product" {
                        call("sum", vec![mul(p.im.clone(), q.im.clone())])
                    } else {
                        call(&n, vec![p.im.clone(), q.im.clone()])
                    }
                } else {
                    real("0.0", k)
                };
                let ri = if bc {
                    if n == "dot_product" {
                        call("sum", vec![mul(p.re, q.im)])
                    } else {
                        call(&n, vec![p.re, q.im])
                    }
                } else {
                    real("0.0", k)
                };
                let ir = if ac {
                    if n == "dot_product" {
                        call("sum", vec![mul(p.im, q.re)])
                    } else {
                        call(&n, vec![p.im, q.re])
                    }
                } else {
                    real("0.0", k)
                };
                if n == "dot_product" {
                    (add(rr, ii), sub(ri, ir))
                } else {
                    (sub(rr, ii), add(ri, ir))
                }
            }
            Expr::Call(n, a) if n == "conjg" => {
                let p = self.pair(a[0].clone(), s, out)?;
                (p.re, neg(p.im))
            }
            Expr::Call(n, a) if crate::intrinsics::math_arity(&n).is_some() => {
                if a.len() != 1 {
                    bail!("{n} with COMPLEX requires one argument");
                }
                let p = self.pair(a[0].clone(), s, out)?.converted(k);
                let p = self.frozen(p, s, out);
                let args = vec![p.re, p.im];
                if !matches!(
                    n.as_str(),
                    "sqrt"
                        | "exp"
                        | "log"
                        | "log10"
                        | "sin"
                        | "cos"
                        | "tan"
                        | "sinh"
                        | "cosh"
                        | "tanh"
                ) {
                    bail!("COMPLEX intrinsic `{n}` is not yet supported");
                }
                (
                    self.helper(&format!("{n}_re"), k, args.clone()),
                    self.helper(&format!("{n}_im"), k, args),
                )
            }
            Expr::Call(n, mut a) if self.functions.contains_key(&n) => {
                let f = self.functions[&n].clone();
                if f.elemental
                    && a.iter().any(|e| {
                        self.info(e, s)
                            .ok()
                            .flatten()
                            .is_some_and(|i| !i.dims.is_empty())
                    })
                {
                    bail!("array invocation of user-defined COMPLEX ELEMENTAL functions is not yet supported");
                }
                self.arguments(&n, &mut a, s, out)?;
                self.serial += 1;
                let temp = format!("@complex_result_{}", self.serial);
                let mut attributes = vec![];
                let storage = if f.result.dims.is_empty() {
                    vec![]
                } else {
                    attributes.push("allocatable".into());
                    vec![id("@deferred_extent"); dimensions.len()]
                };
                out.push(Stmt::DerivedDecl {
                    type_name: typename(k),
                    names: vec![temp.clone()],
                    attributes,
                    polymorphic: false,
                    dims: storage,
                });
                out.push(Stmt::Assign {
                    name: temp.clone(),
                    value: call(&n, a),
                });
                s.insert(temp.clone(), info.clone());
                (id(&format!("{temp}%re")), id(&format!("{temp}%im")))
            }
            Expr::Call(n, _) => bail!("COMPLEX expression `{n}` is not yet supported"),
            _ => bail!("unsupported COMPLEX expression"),
        };
        Ok(Pair {
            re,
            im,
            kind: k,
            dims: dimensions,
        })
    }
    fn matrix(
        &mut self,
        p: Pair,
        q: Pair,
        dimensions: Vec<Expr>,
        s: &mut Scope,
        out: &mut Vec<Stmt>,
    ) -> Result<(Expr, Expr)> {
        let inner = p.dims.last().context("MATMUL requires arrays")?.clone();
        let other = q.dims.first().context("MATMUL requires arrays")?.clone();
        out.push(Stmt::If {
            cond: bin(BinOp::Ne, inner.clone(), other),
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str("MATMUL operand shapes do not conform".into())],
            }],
            else_body: None,
        });
        let re = self.primitive(
            component_type(p.kind),
            dimensions.clone(),
            real("0.0", p.kind),
            s,
            out,
        );
        let im = self.primitive(
            component_type(p.kind),
            dimensions.clone(),
            real("0.0", p.kind),
            s,
            out,
        );
        self.serial += 1;
        let names = [
            format!("@complex_row_{}", self.serial),
            format!("@complex_col_{}", self.serial),
            format!("@complex_inner_{}", self.serial),
        ];
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: names.to_vec(),
        });
        let row = id(&names[0]);
        let col = id(&names[1]);
        let k = id(&names[2]);
        let pi = if p.dims.len() == 2 {
            vec![row.clone(), k.clone()]
        } else {
            vec![k.clone()]
        };
        let qi = if q.dims.len() == 2 {
            vec![k.clone(), col.clone()]
        } else {
            vec![k.clone()]
        };
        let ri = if p.dims.len() == 2 && q.dims.len() == 2 {
            vec![row.clone(), col.clone()]
        } else if p.dims.len() == 2 {
            vec![row.clone()]
        } else {
            vec![col.clone()]
        };
        let element = |e: &Expr, i: Vec<Expr>| {
            if let Expr::Ident(n) = e {
                call(n, i)
            } else {
                unreachable!()
            }
        };
        let rr = mul(element(&p.re, pi.clone()), element(&q.re, qi.clone()));
        let ii = mul(element(&p.im, pi.clone()), element(&q.im, qi.clone()));
        let ir = mul(element(&p.im, pi.clone()), element(&q.re, qi.clone()));
        let r_i = mul(element(&p.re, pi), element(&q.im, qi));
        let Expr::Ident(rn) = &re else { unreachable!() };
        let Expr::Ident(iname) = &im else {
            unreachable!()
        };
        let body = vec![
            Stmt::AssignIndex {
                name: rn.clone(),
                indices: ri.clone(),
                value: add(element(&re, ri.clone()), sub(rr, ii)),
            },
            Stmt::AssignIndex {
                name: iname.clone(),
                indices: ri.clone(),
                value: add(element(&im, ri), add(r_i, ir)),
            },
        ];
        let mut loop_body = vec![Stmt::Do {
            var: names[2].clone(),
            start: int(1),
            end: inner,
            body,
        }];
        if q.dims.len() == 2 {
            loop_body = vec![Stmt::Do {
                var: names[1].clone(),
                start: int(1),
                end: q.dims[1].clone(),
                body: loop_body,
            }];
        }
        if p.dims.len() == 2 {
            loop_body = vec![Stmt::Do {
                var: names[0].clone(),
                start: int(1),
                end: p.dims[0].clone(),
                body: loop_body,
            }];
        }
        out.extend(loop_body);
        Ok((re, im))
    }
    fn arguments(
        &mut self,
        n: &str,
        a: &mut [Expr],
        s: &mut Scope,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let f = self.functions.get(n).cloned();
        for (index, e) in a.iter_mut().enumerate() {
            if let Some(k) = f
                .as_ref()
                .and_then(|f| f.params.get(index).and_then(|p| f.arguments.get(p)))
                .and_then(|i| complex_kind(&i.kind))
            {
                let info = self
                    .info(e, s)?
                    .context("unknown COMPLEX procedure argument")?;
                if complex_kind(&info.kind) != Some(k) {
                    bail!(
                        "COMPLEX type or kind mismatch for argument {} of `{n}`",
                        index + 1
                    );
                }
                if !(matches!(e, Expr::Ident(_))
                    || matches!(e,Expr::Call(name,_) if s.contains_key(name)))
                {
                    *e = self.pair(e.clone(), s, out)?.pack();
                }
            } else {
                self.expression(e, s, out)?;
            }
        }
        Ok(())
    }
    fn expression(&mut self, e: &mut Expr, s: &mut Scope, out: &mut Vec<Stmt>) -> Result<()> {
        if let Expr::Call(n, a) = e {
            if n == "dot_product" && !s.contains_key(n) && !self.functions.contains_key(n) {
                let info = self.dot_product_info(a, s)?;
                if complex_kind(&info.kind).is_none() {
                    let logical = info.kind == TypeSpec::Logical;
                    *e = call(
                        if logical { "any" } else { "sum" },
                        vec![bin(
                            if logical { BinOp::And } else { BinOp::Mul },
                            a[0].clone(),
                            a[1].clone(),
                        )],
                    );
                    return self.expression(e, s, out);
                }
            }
        }
        if let Expr::Call(n, a) = e {
            // Type inquiries must not materialize a dot product or read its
            // operand values. Validate the original operands, then use the
            // promoted scalar result type directly.
            if a.len() == 1
                && !s.contains_key(n)
                && !self.functions.contains_key(n)
                && matches!(&a[0],Expr::Call(dot,_) if dot=="dot_product" && !s.contains_key(dot) && !self.functions.contains_key(dot))
            {
                let info = self
                    .info(&a[0], s)?
                    .context("unknown DOT_PRODUCT result type")?;
                if n == "kind" {
                    let k = match &info.kind {
                        TypeSpec::Integer(k) => k.unwrap_or(4),
                        TypeSpec::DoublePrecision => 8,
                        TypeSpec::Complex(k) => k.parse()?,
                        _ => 4,
                    };
                    *e = int(k as i64);
                    return Ok(());
                } else if crate::inquiries::numeric_model(n) {
                    *e = crate::inquiries::model_value(n, &info.kind)?;
                    return Ok(());
                }
            }
        }
        if let Expr::Call(n, a) = e {
            if n == "kind" && a.len() == 1 {
                if let Some(i) = self.info(&a[0], s)? {
                    if let Some(k) = complex_kind(&i.kind) {
                        *e = int(k as i64);
                        return Ok(());
                    }
                }
            }
            if matches!(n.as_str(), "precision" | "range") && a.len() == 1 {
                if let Some(i) = self.info(&a[0], s)? {
                    if let Some(k) = complex_kind(&i.kind) {
                        *e = crate::inquiries::model_value(n, &component_type(k))?;
                        return Ok(());
                    }
                }
            }
            if matches!(
                n.as_str(),
                "size" | "shape" | "lbound" | "ubound" | "allocated" | "present"
            ) && !a.is_empty()
            {
                if self
                    .info(&a[0], s)?
                    .is_some_and(|i| complex_kind(&i.kind).is_some())
                {
                    let p = self.pair(a[0].clone(), s, out)?;
                    a[0] = p.re;
                    for e in a.iter_mut().skip(1) {
                        self.expression(e, s, out)?;
                    }
                    return Ok(());
                }
            }
            if matches!(n.as_str(), "abs" | "aimag" | "real" | "dble" | "int")
                && !a.is_empty()
                && self
                    .info(&a[0], s)?
                    .is_some_and(|i| complex_kind(&i.kind).is_some())
            {
                let p = self.pair(a[0].clone(), s, out)?;
                *e = match n.as_str() {
                    "abs" => {
                        let p = self.frozen(p, s, out);
                        self.helper("abs", p.kind, vec![p.re, p.im])
                    }
                    "aimag" => p.im,
                    "real" if a.len() == 1 => p.re,
                    _ => {
                        let mut args = vec![p.re];
                        args.extend(a.iter().skip(1).cloned());
                        call(n, args)
                    }
                };
                return Ok(());
            }
        }
        if let Expr::Bin(op, a, b) = e {
            if self
                .info(a, s)?
                .is_some_and(|i| complex_kind(&i.kind).is_some())
                || self
                    .info(b, s)?
                    .is_some_and(|i| complex_kind(&i.kind).is_some())
            {
                if matches!(op, BinOp::Eq | BinOp::Ne) {
                    let p = self.pair((**a).clone(), s, out)?;
                    let p = self.frozen(p, s, out);
                    let q = self.pair((**b).clone(), s, out)?;
                    let q = self.frozen(q, s, out);
                    *e = bin(
                        if matches!(op, BinOp::Eq) {
                            BinOp::And
                        } else {
                            BinOp::Or
                        },
                        bin(op.clone(), p.re, q.re),
                        bin(op.clone(), p.im, q.im),
                    );
                    return Ok(());
                }
                if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                    bail!("ordered comparisons are not defined for COMPLEX operands");
                }
            }
        }
        if self
            .info(e, s)?
            .is_some_and(|i| complex_kind(&i.kind).is_some())
        {
            if matches!(e, Expr::Ident(_)) {
                return Ok(());
            }
            *e = self.pair(e.clone(), s, out)?.pack();
            return Ok(());
        }
        match e {
            Expr::Call(n, a) => self.arguments(n, a, s, out)?,
            Expr::Index(_, a) => {
                for e in a {
                    self.expression(e, s, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, s, out)?;
                self.expression(b, s, out)?;
            }
            Expr::Un(_, a) => self.expression(a, s, out)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, s, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn input_items(&mut self,items:&mut [Expr],s:&mut Scope,out:&mut Vec<Stmt>)->Result<()> {
        for e in items {
            if let Expr::Call(n,a)=e {if n=="@output_implied_do" || n=="@input_implied_do" {self.input_items(&mut a[4..],s,out)?;continue;}}
            if self.info(e,s)?.is_some_and(|i|complex_kind(&i.kind).is_some()) {
                let p=self.pair(e.clone(),s,out)?;
                if p.dims.is_empty() {*e=call("@input_complex",vec![p.re,p.im,int(p.kind as i64)]);}
                else {
                    let (Expr::Ident(re),Expr::Ident(im))=(p.re,p.im) else {bail!("COMPLEX READ sections are not yet supported");};
                    let mut vars=vec![];let mut subs=vec![];let mut bounds=vec![];
                    for dim in p.dims {
                        self.serial+=1;let index=format!("@complex_read_index_{}",self.serial);
                        out.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                        let (lower,upper)=match dim {Expr::Call(n,a) if n=="@declared_bounds"=>(a[0].clone(),a[1].clone()),d=>(int(1),d)};
                        subs.push(id(&index));vars.push(index);bounds.push((lower,upper));
                    }
                    let mut item=call("@input_complex",vec![Expr::Index(re,subs.clone()),Expr::Index(im,subs),int(p.kind as i64)]);
                    for (var,(lower,upper)) in vars.into_iter().zip(bounds) {item=call("@input_implied_do",vec![id(&var),lower,upper,int(1),item]);}
                    *e=item;
                }
            }
        }Ok(())
    }
    // Formatted transfer consumes a real descriptor for each component. Keep
    // destination indices in the input list, where earlier input can define them.
    fn formatted_input(&mut self, items: Vec<Expr>, scope: &Scope, prefix: &mut Vec<Stmt>) -> Result<Vec<Expr>> {
        let mut values=vec![];
        for mut item in items {
            if let Expr::Call(n,a)=&mut item {
                if matches!(n.as_str(),"@input_implied_do"|"@output_implied_do") {
                    let nested=a.split_off(4);a.extend(self.formatted_input(nested,scope,prefix)?);
                    values.push(item);continue;
                }
            }
            if let Some(info)=self.info(&item,scope)?.filter(|i|complex_kind(&i.kind).is_some()) {
                if !info.dims.is_empty() {
                    let Expr::Ident(name)=item else {bail!("formatted COMPLEX READ sections are not yet supported");};
                    let mut indices=vec![];let mut controls=vec![];
                    for dim in info.dims {
                        self.serial+=1;let index=format!("@complex_formatted_read_{}",self.serial);
                        prefix.push(Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                        let (lo,hi)=match dim {Expr::Call(n,a) if n=="@declared_bounds"=>(a[0].clone(),a[1].clone()),dim=>(int(1),dim)};
                        indices.push(id(&index));controls.push((index,lo,hi));
                    }
                    let mut element=Expr::Index(name,indices);
                    for (index,lo,hi) in controls {element=call("@input_implied_do",vec![id(&index),lo,hi,int(1),element]);}
                    values.extend(self.formatted_input(vec![element],scope,prefix)?);
                    continue;
                }
                let mut re=item.clone();let mut im=item;
                for (part,suffix) in [(&mut re,"%re"),(&mut im,"%im")] {
                    match part {
                        Expr::Ident(n)=>n.push_str(suffix),
                        Expr::Index(n,a)|Expr::Call(n,a)=>{
                            let mut call_subscript=false;
                            for subscript in a {crate::procedure_arguments::visit_expr(subscript,&mut |e| {
                                if matches!(e,Expr::Call(_,_)|Expr::Index(_,_)) {call_subscript=true;}
                            });}
                            if call_subscript {bail!("formatted COMPLEX READ currently requires subscripts without function calls or array references");}
                            n.push_str(suffix);
                        },
                        _=>bail!("formatted COMPLEX READ item must designate writable storage"),
                    }
                }
                values.extend([re,im]);
            } else {values.push(item);}
        }
        Ok(values)
    }
    fn output(
        &mut self,
        items: &mut Vec<Expr>,
        list: bool,
        s: &mut Scope,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let mut values = vec![];
        let mut previous_character = false;
        let mut previous_complex = false;
        for mut e in std::mem::take(items) {
            // An I/O implied-DO is a transfer list, not an array constructor.
            // Its items may have different types and its bounds need not be
            // constant. In particular, do not ask info() for a constructor
            // extent before inspecting the individual transfer items.
            if let Expr::Call(n, args) = &mut e {
                if n == "@output_implied_do" && args.len() >= 5 {
                    let mut inner = args[4..].to_vec();
                    let mut prefix = vec![];
                    let mut local = s.clone();
                    self.output(&mut inner, list, &mut local, &mut prefix)?;
                    if prefix.is_empty() {
                        *s = local;
                        args.truncate(4);
                        args.extend(inner);
                        values.push(e);
                        previous_character = false;
                        previous_complex = false;
                        continue;
                    }
                    // Preserve the existing bounded COMPLEX constructor path
                    // when producing an item requires materialized storage.
                    if args[1..4].iter().any(|e| crate::parameters::integer_constant(e).is_err()) {
                        bail!("this output implied-DO expression requires temporaries inside the loop that are not yet supported");
                    }
                }
            }
            if self
                .info(&e, s)?
                .is_some_and(|i| complex_kind(&i.kind).is_some())
            {
                let p = self.pair(e, s, out)?;
                if p.dims.is_empty() {
                    if list {
                        let p = self.frozen(p, s, out);
                        if previous_character {
                            values.push(Expr::Str(" ".into()));
                        }
                        values.push(call(
                            "trim",
                            vec![self.helper("string", p.kind, vec![p.re, p.im])],
                        ));
                        previous_character = true;
                        previous_complex = true;
                    } else {
                        values.extend([p.re, p.im]);
                    }
                } else if list {
                    let p = self.frozen(p, s, out);
                    self.serial += 1;
                    let name = format!("@complex_output_{}", self.serial);
                    let index = format!("@complex_output_index_{}", self.serial);
                    let length = p.dims.iter().cloned().fold(int(1), mul);
                    if crate::parameters::integer_constant(&length).is_err() {
                        bail!("list-directed output of runtime-sized COMPLEX arrays is not yet supported");
                    }
                    out.push(Stmt::ArrayDecl {
                        kind: TypeSpec::Character(Some(96)),
                        name: name.clone(),
                        dims: vec![length.clone()],
                    });
                    out.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![index.clone()],
                    });
                    let mut ordinal = sub(id(&index), int(1));
                    let mut indices = vec![];
                    for d in &p.dims {
                        indices.push(add(call("mod", vec![ordinal.clone(), d.clone()]), int(1)));
                        ordinal = bin(BinOp::Div, ordinal, d.clone());
                    }
                    let element = |e: Expr| {
                        if let Expr::Ident(n) = e {
                            call(&n, indices.clone())
                        } else {
                            unreachable!()
                        }
                    };
                    let value = self.helper("string", p.kind, vec![element(p.re), element(p.im)]);
                    out.push(Stmt::Do {
                        var: index,
                        start: int(1),
                        end: length.clone(),
                        body: vec![Stmt::AssignIndex {
                            name: name.clone(),
                            indices: vec![id(&format!("@complex_output_index_{}", self.serial))],
                            value,
                        }],
                    });
                    // Each buffer is fixed-width storage, not an output field.
                    // Expand its bounded elements just as ordinary CHARACTER
                    // array output does, trimming only these generated buffers.
                    let count = crate::parameters::integer_constant(&length)?;
                    for i in 1..=count {
                        if previous_character {
                            values.push(Expr::Str(" ".into()));
                        }
                        values.push(call("trim", vec![call(&name, vec![int(i)])]));
                        previous_character = true;
                        previous_complex = true;
                    }
                } else {
                    // Formatted complex transfer consumes two real descriptors per element.
                    let p = self.frozen(p, s, out);
                    self.serial += 1;
                    let name = format!("@complex_output_{}", self.serial);
                    let dimensions = p.dims.clone();
                    let k = p.kind;
                    out.push(Stmt::DerivedDecl {
                        type_name: typename(k),
                        names: vec![name.clone()],
                        attributes: vec![],
                        polymorphic: false,
                        dims: dimensions.clone(),
                    });
                    out.push(Stmt::Assign {
                        name: name.clone(),
                        value: p.pack(),
                    });
                    s.insert(
                        name.clone(),
                        Info {
                            kind: TypeSpec::Complex(k.to_string()),
                            dims: dimensions,
                        },
                    );
                    values.push(id(&name));
                }
            } else {
                let character = self.info(&e, s)?.is_some_and(|i| {
                    matches!(
                        i.kind,
                        TypeSpec::Character(_)
                            | TypeSpec::CharacterLength(_)
                            | TypeSpec::CharacterAssumed
                    )
                }) || matches!(&e, Expr::Bin(BinOp::Concat, _, _))
                    || matches!(&e, Expr::Call(n, _) if matches!(n.as_str(), "trim" | "adjustl" | "adjustr" | "repeat" | "char" | "achar" | "new_line"));
                if list && previous_complex && character {
                    values.push(Expr::Str(" ".into()));
                }
                self.expression(&mut e, s, out)?;
                values.push(e);
                previous_character = character;
                previous_complex = false;
            }
        }
        *items = values;
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Scope, params: &[String]) -> Result<()> {
        let mut scope = inherited.clone();
        for p in params {
            scope.remove(p);
        }
        declarations(body, &mut scope);
        let attrs: HashMap<String, Vec<String>> = scope
            .keys()
            .map(|n| {
                (
                    n.clone(),
                    body.iter()
                        .filter_map(|s| {
                            if let Stmt::DummyIntent { name, intent } = s {
                                (name == n).then(|| intent.clone())
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        let arrays: HashSet<_> = body
            .iter()
            .filter_map(|s| {
                if let Stmt::ArrayDecl { name, .. } = s {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut result = vec![];
        let mut location = None;
        let base = self.live.len();
        for mut stmt in std::mem::take(body) {
            for (kind, name) in self.live.drain(base..) {
                self.free.entry(kind).or_default().push(name);
            }
            // A procedure's temporaries are declared in it, so they are
            // not reused in another.
            let unit = matches!(stmt, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. });
            let outer = if unit { Some((std::mem::take(&mut self.free), std::mem::take(&mut self.live))) } else { None };
            if let Stmt::SourceLocation(offset) = stmt {
                location = Some(offset);
                result.push(Stmt::SourceLocation(offset));
                continue;
            }
            let mut prefix = vec![];
            (||->Result<()>{match &mut stmt {
                Stmt::Implicit(i) if complex_implicit(i)=>bail!("implicit COMPLEX declarations are not yet supported; declare complex variables explicitly"),
                Stmt::DerivedType{components,..} if uses_complex(components)=>bail!("COMPLEX components inside user-defined types are not yet supported"),
                Stmt::VarDecl{kind,names} if complex_kind(kind).is_some()=>{
                    let k=complex_kind(kind).unwrap();for n in names {prefix.push(Stmt::DerivedDecl{type_name:typename(k),names:vec![n.clone()],attributes:attrs.get(n).cloned().unwrap_or_default(),polymorphic:false,dims:vec![]});}return Ok(());
                },
                Stmt::ArrayDecl{kind,name,dims} if complex_kind(kind).is_some()=>{prefix.push(Stmt::DerivedDecl{type_name:typename(complex_kind(kind).unwrap()),names:vec![name.clone()],attributes:attrs.get(name).cloned().unwrap_or_default(),polymorphic:false,dims:dims.clone()});return Ok(());},
                Stmt::InitializedDecl{kind,attributes,entities} if complex_kind(kind).is_some()=>{
                    if attributes.iter().any(|a|a=="parameter"){bail!("COMPLEX PARAMETER initialization is not yet supported");}
                    let k=complex_kind(kind).unwrap();for(n,e)in entities {if !arrays.contains(n){prefix.push(Stmt::DerivedDecl{type_name:typename(k),names:vec![n.clone()],attributes:attrs.get(n).cloned().unwrap_or_default(),polymorphic:false,dims:vec![]});}if let Some(e)=e {let p=self.pair(e.clone(),&mut scope,&mut prefix)?.converted(k);prefix.push(Stmt::Assign{name:n.clone(),value:p.pack()});}}return Ok(());
                },
                Stmt::DummyIntent{name,..} if scope.get(name).is_some_and(|i|complex_kind(&i.kind).is_some())=>return Ok(()),
                Stmt::Assign{name,..}|Stmt::AssignIndex{name,..} if scope.get(name).is_some_and(|i|complex_kind(&i.kind).is_some())=>{
                    let k=complex_kind(&scope[name].kind).unwrap();
                    let mut indices=if let Stmt::AssignIndex{indices,..}=&stmt{Some(indices.clone())}else{None};
                    if let Some(a)=&mut indices{self.indices(a,&mut scope,&mut prefix)?;}
                    let (name,value)=match &mut stmt{Stmt::Assign{name,value}|Stmt::AssignIndex{name,value,..}=>(name,value),_=>unreachable!()};
                    let p=self.pair(value.clone(),&mut scope,&mut prefix)?.converted(k);
                    prefix.push(if let Some(indices)=indices{Stmt::AssignIndex{name:name.clone(),indices,value:p.pack()}}else{Stmt::Assign{name:name.clone(),value:p.pack()}});return Ok(());
                },
                Stmt::Assign{value,..}|Stmt::AssignIndex{value,..}=>self.expression(value,&mut scope,&mut prefix)?,
                Stmt::Print{items}=>self.output(items,true,&mut scope,&mut prefix)?,
                Stmt::Read{args}=>if args.iter().any(|e|self.info(e,&scope).ok().flatten().is_some_and(|i|complex_kind(&i.kind).is_some())){bail!("list-directed COMPLEX input is not yet supported");},
                Stmt::CallSub{name,args} if name=="@internal_read"=>{self.input_items(&mut args[3..],&mut scope,&mut prefix)?;},
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@formatted_read"|"@formatted_read_status")=>{
                    let first=if name=="@formatted_read" {2}else{4};
                    let items=args.split_off(first);args.extend(self.formatted_input(items,&scope,&mut prefix)?);
                },
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@write"|"@formatted_print")=>{
                    let offset=if name=="@write"{2}else{1};let list=name=="@write"&&matches!(args.get(1),Some(Expr::Call(n,_)) if n=="@list_directed");let mut items=args.split_off(offset);self.output(&mut items,list,&mut scope,&mut prefix)?;args.extend(items);
                },
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@allocate"|"@deallocate"|"@nullify")=>{},
                Stmt::CallSub{name,args}=>self.arguments(name,args,&mut scope,&mut prefix)?,
                Stmt::Return(Some(e))=>{if self.info(e,&scope)?.is_none_or(|i|complex_kind(&i.kind).is_none()){self.expression(e,&mut scope,&mut prefix)?;}},
                Stmt::If{cond,then_body,else_body}=>{self.expression(cond,&mut scope,&mut prefix)?;self.body(then_body,&scope,&[])?;if let Some(b)=else_body{self.body(b,&scope,&[])?;}},
                Stmt::Do{start,end,body,..}=>{self.expression(start,&mut scope,&mut prefix)?;self.expression(end,&mut scope,&mut prefix)?;self.body(body,&scope,&[])?;},
                Stmt::DoWhile{cond,body}=>{self.expression(cond,&mut scope,&mut prefix)?;self.body(body,&scope,&[])?;if !prefix.is_empty(){let mut evaluate=std::mem::take(&mut prefix);evaluate.push(Stmt::If{cond:Expr::Un(UnOp::Not,Box::new(cond.clone())),then_body:vec![Stmt::Exit],else_body:None});evaluate.append(body);*body=evaluate;*cond=Expr::Logical(true);}},
                Stmt::Function{body,params,return_type,..}=>{if return_type.as_ref().is_some_and(|k|complex_kind(k).is_some()){*return_type=None;}self.body(body,&scope,params)?;},
                Stmt::Subroutine{body,params,..}=>self.body(body,&scope,params)?,
                Stmt::Module{body,..}|Stmt::Block{body}=>self.body(body,&scope,&[])?,
                Stmt::SelectCase{expr,cases,default}=>{self.expression(expr,&mut scope,&mut prefix)?;for c in cases{self.body(&mut c.body,&scope,&[])?;}if let Some(b)=default{self.body(b,&scope,&[])?;}},
                _=>{}
            }prefix.push(stmt.clone());Ok(())})().map_err(|e|crate::source_diagnostics::at(e,location))?;
            if let Some((free, live)) = outer {
                self.free = free;
                self.live = live;
            }
            result.extend(prefix);
        }
        for (kind, name) in self.live.drain(base..) {
            self.free.entry(kind).or_default().push(name);
        }
        *body = result;
        Ok(())
    }
}
fn constant_component(e: &Expr) -> bool {
    match e {
        Expr::IntLit(_) | Expr::RealLit(_) => true,
        Expr::Un(UnOp::Neg, a) => constant_component(a),
        Expr::Call(n, a) if matches!(n.as_str(), "real" | "dble" | "@integer_literal") => {
            a.first().is_some_and(constant_component)
        }
        _ => false,
    }
}

/// Fold declaration constants before the ordinary PARAMETER substitution pass.
pub(crate) fn constant(value: &Expr, kind: &str) -> Result<Expr> {
    fn pair(e: &Expr) -> Result<(f64, f64)> {
        if let Ok(x) = crate::parameters::real_constant(e) {
            return Ok((x, 0.));
        }
        match e {
            Expr::Call(n, a) if n == "@complex_literal" && a.len() == 2 => Ok((
                crate::parameters::real_constant(&a[0])?,
                crate::parameters::real_constant(&a[1])?,
            )),
            Expr::Call(n, a) if n == "cmplx" && !a.is_empty() => {
                let (x, mut y) = pair(&a[0])?;
                if let Some(e) = a.get(1).filter(|e| !crate::named_arguments::omitted(e)) {
                    if crate::parameters::real_constant(&a[0]).is_err() {
                        bail!("CMPLX Y must be absent when X is COMPLEX");
                    }
                    y = crate::parameters::real_constant(e)?;
                }
                let k = a
                    .get(2)
                    .filter(|e| !crate::named_arguments::omitted(e))
                    .map(crate::parameters::integer_constant)
                    .transpose()?
                    .unwrap_or(4);
                if !matches!(k, 4 | 8) {
                    bail!("unsupported COMPLEX kind {k}");
                }
                Ok(if k == 4 {
                    (x as f32 as f64, y as f32 as f64)
                } else {
                    (x, y)
                })
            }
            Expr::Call(n, a) if n == "conjg" && a.len() == 1 => {
                let (x, y) = pair(&a[0])?;
                Ok((x, -y))
            }
            Expr::Un(UnOp::Neg, a) => {
                let (x, y) = pair(a)?;
                Ok((-x, -y))
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = pair(a)?;
                let (u, v) = pair(b)?;
                Ok(match op {
                    BinOp::Add => (x + u, y + v),
                    BinOp::Sub => (x - u, y - v),
                    BinOp::Mul => (x * u - y * v, x * v + y * u),
                    BinOp::Div => {
                        let d = u * u + v * v;
                        ((x * u + y * v) / d, (y * u - x * v) / d)
                    }
                    _ => bail!("unsupported COMPLEX constant operator"),
                })
            }
            _ => bail!("COMPLEX initializer must be a supported constant expression"),
        }
    }
    let (mut x, mut y) = pair(value)?;
    if kind == "4" {
        x = x as f32 as f64;
        y = y as f32 as f64;
    }
    if !x.is_finite() || !y.is_finite() {
        bail!("COMPLEX initializer exceeds the component kind's range");
    }
    let component = |x: f64| {
        if kind == "8" {
            call("dble", vec![Expr::RealLit(x.to_string())])
        } else {
            Expr::RealLit(x.to_string())
        }
    };
    Ok(call("@complex_literal", vec![component(x), component(y)]))
}

fn complex_implicit(i: &Implicit) -> bool {
    match i {
        Implicit::Rule { type_spec, .. } => matches!(type_spec, TypeSpec::Complex(_)),
        Implicit::Rules { rules } => rules
            .iter()
            .any(|r| matches!(&r.type_spec, TypeSpec::Complex(_))),
        _ => false,
    }
}
fn uses_feature(body: &[Stmt], complex_declarations: bool, intrinsic: fn(&str) -> bool) -> bool {
    fn expression(e: &Expr, intrinsic: fn(&str) -> bool) -> bool {
        match e {
            Expr::Call(n, a) => intrinsic(n) || a.iter().any(|e| expression(e, intrinsic)),
            Expr::Index(_, a) => a.iter().any(|e| expression(e, intrinsic)),
            Expr::Bin(_, a, b) => expression(a, intrinsic) || expression(b, intrinsic),
            Expr::Un(_, a) => expression(a, intrinsic),
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => [lower, upper, stride]
                .into_iter()
                .flatten()
                .any(|e| expression(e, intrinsic)),
            _ => false,
        }
    }
    let expr = |e: &Expr| expression(e, intrinsic);
    let children = |b: &[Stmt]| uses_feature(b, complex_declarations, intrinsic);
    body.iter().any(|s| match s {
        Stmt::Implicit(i) => complex_declarations && complex_implicit(i),
        Stmt::VarDecl { kind, .. } | Stmt::ArrayDecl { kind, .. } => {
            complex_declarations && matches!(kind, TypeSpec::Complex(_))
        }
        Stmt::InitializedDecl { kind, entities, .. } => {
            (complex_declarations && matches!(kind, TypeSpec::Complex(_)))
                || entities.iter().filter_map(|(_, e)| e.as_ref()).any(expr)
        }
        Stmt::Assign { value, .. } | Stmt::AssignIndex { value, .. } => expr(value),
        Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => {
            items.iter().any(expr)
        }
        Stmt::Return(Some(e)) => expr(e),
        Stmt::Function {
            body, return_type, ..
        } => {
            (complex_declarations
                && return_type
                    .as_ref()
                    .is_some_and(|k| matches!(k, TypeSpec::Complex(_))))
                || children(body)
        }
        Stmt::Subroutine { body, .. }
        | Stmt::Module { body, .. }
        | Stmt::Block { body }
        | Stmt::DerivedType {
            components: body, ..
        } => children(body),
        Stmt::Do {
            start, end, body, ..
        } => expr(start) || expr(end) || children(body),
        Stmt::DoWhile { cond, body } => expr(cond) || children(body),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => expr(cond) || children(then_body) || else_body.as_ref().is_some_and(|b| children(b)),
        Stmt::SelectCase {
            expr: e,
            cases,
            default,
        } => {
            expr(e)
                || cases.iter().any(|c| children(&c.body))
                || default.as_ref().is_some_and(|b| children(b))
        }
        _ => false,
    })
}
pub(crate) fn uses_complex(body: &[Stmt]) -> bool {
    uses_feature(body, true, |n| matches!(n, "cmplx" | "@complex_literal"))
}
pub(crate) fn uses_dot_product(body: &[Stmt]) -> bool {
    uses_feature(body, false, |n| n == "dot_product")
}

/// FORTRAN 77 specific names of the COMPLEX intrinsics, as their generic
/// forms: DIMAG, DCONJG, DREAL and DCMPLX.
fn specific_names(body: &mut [Stmt]) {
    fn expr(e: &mut Expr) {
        crate::procedure_arguments::visit_expr(e, &mut |e| {
            if let Expr::Call(n, args) = e {
                match n.as_str() {
                    "dimag" => *n = "aimag".into(),
                    "dconjg" => *n = "conjg".into(),
                    "dreal" => *n = "dble".into(),
                    "dcmplx" if !args.iter().any(|a| matches!(a, Expr::Call(t, _) if t.starts_with("@keyword:"))) => {
                        *n = "cmplx".into();
                        if args.len() == 1 {
                            args.push(Expr::RealLit("0d0".into()));
                        }
                        args.push(int(8));
                    }
                    _ => {}
                }
            }
        });
    }
    for s in body {
        for e in crate::procedure_arguments::expressions(s) {
            expr(e);
        }
        for b in crate::procedure_arguments::children(s) {
            specific_names(b);
        }
    }
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    specific_names(&mut p.body);
    let complex = uses_complex(&p.body);
    if !complex && !uses_dot_product(&p.body) {
        return Ok(p);
    }
    let mut lower = Lower::default();
    lower.collect(&p.body, &Scope::new())?;
    lower.body(&mut p.body, &Scope::new(), &[])?;
    for k in [4, 8].into_iter().filter(|_| complex) {
        p.body.insert(
            0,
            Stmt::DerivedType {
                name: typename(k),
                components: vec![Stmt::VarDecl {
                    kind: component_type(k),
                    names: vec!["re".into(), "im".into()],
                }],
                bindings: vec![],
            },
        );
    }
    // Helpers are generated as ordinary typed ELEMENTAL procedures, so the
    // interpreter and native compiler execute the same arithmetic and branches.
    let mut helpers = lower.helpers.into_iter().collect::<Vec<_>>();
    helpers.sort();
    for (name, k) in helpers {
        p.body.push(helper_definition(&name, k)?);
    }
    hoist(&mut p.body);
    Ok(p)
}

pub(crate) fn hoist(body: &mut Vec<Stmt>) {
    fn walk(body: &mut Vec<Stmt>, decls: &mut Vec<Stmt>) {
        let mut out = vec![];
        for mut stmt in std::mem::take(body) {
            let generated = matches!(&stmt,Stmt::VarDecl{names,..} if names.iter().all(|n|n.starts_with("@complex_")||n.starts_with("@derived_")))
                || matches!(&stmt,Stmt::ArrayDecl{name,..}|Stmt::DummyIntent{name,..} if name.starts_with("@complex_")||name.starts_with("@derived_"))
                || matches!(&stmt,Stmt::DerivedDecl{names,..} if names.iter().all(|n|n.starts_with("@complex_")||n.starts_with("@derived_")));
            if generated {
                decls.push(stmt);
                continue;
            }
            match &mut stmt {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => hoist(body),
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    walk(body, decls)
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, decls);
                    if let Some(b) = else_body {
                        walk(b, decls);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        walk(&mut case.body, decls);
                    }
                    if let Some(body) = default {
                        walk(body, decls);
                    }
                }
                _ => {}
            }
            out.push(stmt);
        }
        *body = out;
    }
    let mut declarations = vec![];
    walk(body, &mut declarations);
    declarations.append(body);
    *body = declarations;
}

fn helper_definition(name: &str, k: u8) -> Result<Stmt> {
    let t = if k == 8 { "double precision" } else { "real" };
    let zero = if k == 8 { "0.0d0" } else { "0.0" };
    let one = if k == 8 { "1.0d0" } else { "1.0" };
    let half = if k == 8 { "0.5d0" } else { "0.5" };
    let (params, decls, body, character) = if name == "string" {
        (
            "ar,ai".to_string(),
            format!("{t},intent(in)::ar,ai\ncharacter(96)::r"),
            "write(r,'(\"(\",g0,\",\",g0,\")\")') ar,ai".to_string(),
            true,
        )
    } else if name == "abs" {
        ("ar,ai".into(),format!("{t},intent(in)::ar,ai\n{t}::r,s"),format!("s=max(abs(ar),abs(ai))\nif(s=={zero})then\nr={zero}\nelse\nr=s*sqrt((ar/s)**2+(ai/s)**2)\nend if"),false)
    } else if name.starts_with("div_") {
        let expression = if name.ends_with("re") {
            "(xr*cr+xi*ci)/den"
        } else {
            "(xi*cr-xr*ci)/den"
        };
        ("ar,ai,br,bi".into(),format!("{t},intent(in)::ar,ai,br,bi\n{t}::r,s,cr,ci,xr,xi,den"),format!("s=max(abs(br),abs(bi))\ncr=br/s\nci=bi/s\nxr=ar/s\nxi=ai/s\nden=cr*cr+ci*ci\nr={expression}"),false)
    } else if name.starts_with("powi_") {
        let component = if name.ends_with("re") { "rr" } else { "ri" };
        ("ar,ai,n".into(),format!("{t},intent(in)::ar,ai\ninteger(8),intent(in)::n\ninteger(8)::m\n{t}::r,rr,ri,br,bi,tr,ti,s,cr,ci,den"),format!("br=ar\nbi=ai\nrr={one}\nri={zero}\nm=n\nif(n<0)then\ns=max(abs(ar),abs(ai))\ncr=ar/s\nci=ai/s\nden=cr*cr+ci*ci\nbr=(cr/s)/den\nbi=-(ci/s)/den\nrr=br\nri=bi\nm=-(n+1)\nend if\ndo while(m>0)\nif(mod(m,2_8)==1_8)then\ntr=rr*br-ri*bi\nti=rr*bi+ri*br\nrr=tr\nri=ti\nend if\nm=m/2_8\nif(m>0)then\ntr=br*br-bi*bi\nti=2*br*bi\nbr=tr\nbi=ti\nend if\nend do\nr={component}"),false)
    } else if name.starts_with("pow_") {
        let expression = if name.ends_with("re") {
            "mag*cos(angle)"
        } else {
            "mag*sin(angle)"
        };
        ("ar,ai,br,bi".into(),format!("{t},intent(in)::ar,ai,br,bi\n{t}::r,s,mag,angle,lr,theta"),format!("if(br=={zero}.and.bi=={zero})then\nr={}\nelse\ns=max(abs(ar),abs(ai))\nif(s=={zero}.and.br>{zero}.and.bi=={zero})then\nr={zero}\nelse\nlr=log(s)+{half}*log((ar/s)**2+(ai/s)**2)\ntheta=atan2(ai,ar)\nmag=exp(br*lr-bi*theta)\nangle=bi*lr+br*theta\nr={expression}\nend if\nend if",if name.ends_with("re"){one}else{zero}),false)
    } else {
        let (operation, component) = name.rsplit_once('_').context("invalid COMPLEX helper")?;
        let re = component == "re";
        let expr = match operation {
            "exp" => if re {
                "exp(ar)*cos(ai)"
            } else {
                "exp(ar)*sin(ai)"
            }
            .into(),
            "sin" => if re {
                "sin(ar)*cosh(ai)"
            } else {
                "cos(ar)*sinh(ai)"
            }
            .into(),
            "cos" => if re {
                "cos(ar)*cosh(ai)"
            } else {
                "-sin(ar)*sinh(ai)"
            }
            .into(),
            "sinh" => if re {
                "sinh(ar)*cos(ai)"
            } else {
                "cosh(ar)*sin(ai)"
            }
            .into(),
            "cosh" => if re {
                "cosh(ar)*cos(ai)"
            } else {
                "sinh(ar)*sin(ai)"
            }
            .into(),
            "tan" => if re {
                "sin(2*ar)/(cos(2*ar)+cosh(2*ai))"
            } else {
                "sinh(2*ai)/(cos(2*ar)+cosh(2*ai))"
            }
            .into(),
            "tanh" => if re {
                "sinh(2*ar)/(cosh(2*ar)+cos(2*ai))"
            } else {
                "sin(2*ai)/(cosh(2*ar)+cos(2*ai))"
            }
            .into(),
            "log" | "log10" => {
                if re {
                    format!("log(s)+{half}*log((ar/s)**2+(ai/s)**2)")
                } else {
                    "atan2(ai,ar)".into()
                }
            }
            "sqrt" => String::new(),
            _ => bail!("unknown COMPLEX helper `{name}`"),
        };
        let body = if operation == "sqrt" {
            format!("s=max(abs(ar),abs(ai))\nif(s=={zero})then\nr={}\nelse\nmag=sqrt((ar/s)**2+(ai/s)**2)\nt=sqrt(s)*sqrt({half}*mag+{half}*abs(ar/s))\nif(ar>={zero})then\nr={}\nelse\nr={}\nend if\nend if",if re{zero}else{"ai"},if re{"t"}else{"ai/(2*t)"},if re{"abs(ai)/(2*t)"}else{if k==8 {"t\nif(atan2(ai,ar)<0.d0) r=-t"}else{"t\nif(atan2(ai,ar)<0.) r=-t"}})
        } else {
            format!(
                "s=max(abs(ar),abs(ai))\nr={expr}{}",
                if operation == "log10" {
                    format!("/log({})", if k == 8 { "10.0d0" } else { "10.0" })
                } else {
                    String::new()
                }
            )
        };
        (
            "ar,ai".into(),
            format!("{t},intent(in)::ar,ai\n{t}::r,s,mag,t"),
            body,
            false,
        )
    };
    let source=format!("program helpers\ncontains\n{}function helper({params}) result(r)\n{decls}\n{body}\nend function helper\nend program helpers\n",if character{"pure "}else{"pure elemental "});
    let mut p =
        crate::parser::parse_with_src(&source, &crate::lexer::lex(&source), "<complex intrinsic>")
            .map_err(|_| anyhow::anyhow!("could not parse COMPLEX helper {name}"))?;
    let mut f = p
        .body
        .drain(..)
        .find(|s| matches!(s, Stmt::Function { .. }))
        .context("missing COMPLEX helper")?;
    if let Stmt::Function { name: internal, .. } = &mut f {
        *internal = format!("@complex_{name}${k}");
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use crate::{
        ast::*,
        lexer::{lex, TokenKind},
        parser::parse_with_src,
    };
    #[test]
    fn literal_parentheses_are_contextual() {
        let source =
            "complex::z\nreal::a(2,2)\nz=(1.,2.)\ncall f(1,2)\na(1,2)=3\nprint *,((1.,2.))\nend\n";
        let tokens = lex(source);
        assert_eq!(
            tokens
                .iter()
                .filter(|t| matches!(t.kind, TokenKind::ComplexOpen))
                .count(),
            2
        );
        parse_with_src(source, &tokens, "complex.f90").unwrap();
    }
    #[test]
    fn implicit_complex_reports_a_limit() {
        let source = "implicit complex(z)\nz=(1.,2.)\nprint *,z\nend\n";
        let p = parse_with_src(source, &lex(source), "complex.f90").unwrap();
        assert!(super::resolve(p)
            .unwrap_err()
            .to_string()
            .contains("implicit COMPLEX"));
    }
    #[test]
    fn programs_without_complex_are_unchanged() {
        let source = "integer::i\ni=2\nprint *,i\nend\n";
        let p = parse_with_src(source, &lex(source), "plain.f90").unwrap();
        let before = serde_json::to_value(&p).unwrap();
        assert_eq!(
            before,
            serde_json::to_value(super::resolve(p).unwrap()).unwrap()
        );
    }
    #[test]
    fn constructor_kind_records_component_kind() {
        let value = Expr::Call(
            "@complex_literal".into(),
            vec![Expr::RealLit("1.".into()), Expr::RealLit("2.".into())],
        );
        let result = super::constant(&value, "8").unwrap();
        let Expr::Call(_, parts) = result else {
            panic!()
        };
        assert!(matches!(&parts[0],Expr::Call(n,_) if n=="dble"));
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower statically known multidimensional arrays to column-major vector storage.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Shapes = HashMap<String, Vec<i64>>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
/// The runtime extent of a rank-one expression of runtime array views.
pub(crate) fn runtime_extent(e: &Expr) -> Option<Expr> {
    match e {
        Expr::Call(n, args) if n == "@packed_view" && args.len() == 2 => Some(args[1].clone()),
        Expr::Bin(_, a, b) => runtime_extent(a).or_else(|| runtime_extent(b)),
        Expr::Un(_, a) => runtime_extent(a),
        Expr::Call(n, args) if crate::intrinsics::math_arity(n).is_some()
            || matches!(n.as_str(), "abs" | "real" | "dble" | "int" | "nint" | "mod" | "modulo" | "sign" | "min" | "max"
                | "len_trim" | "index" | "scan" | "verify" | "ichar" | "iachar") =>
            args.iter().find_map(runtime_extent),
        _ => None,
    }
}
fn total(dims: &[i64]) -> Result<i64> {
    dims.iter().try_fold(1i64, |n, d| {
        if *d < 0 { bail!("this array operation requires a statically known shape; runtime PACK results are not supported here"); }
        n.checked_mul(*d)
            .context("array element count overflows INTEGER(8)")
    })
}

// -1 denotes a runtime PACK extent; reductions inserts the runtime checks.
fn conform(a: &[i64], b: &[i64]) -> bool {
    a.len()==b.len() && a.iter().zip(b).all(|(a,b)| a==b || *a == -1 || *b == -1)
}

pub(crate) fn reshape_shape(args: &[Expr]) -> Result<Vec<i64>> {
    if args.len() != 4 { bail!("RESHAPE requires normalized SOURCE, SHAPE, PAD and ORDER arguments"); }
    let Expr::Call(tag, items) = &args[1] else { bail!("RESHAPE SHAPE must currently be a constant array constructor"); };
    if tag != "@array_constructor" || items.is_empty() { bail!("RESHAPE SHAPE must be a nonempty constant array constructor"); }
    let dims = items.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?;
    if dims.iter().any(|n| *n < 0) { bail!("RESHAPE SHAPE extents must be nonnegative"); }
    total(&dims)?;
    Ok(dims)
}

pub(crate) fn reshape_order(args: &[Expr], rank: usize) -> Result<Vec<usize>> {
    if matches!(&args[3],Expr::Call(tag,_) if tag == "@no_order") { return Ok((0..rank).collect()); }
    let Expr::Call(tag,items) = &args[3] else { bail!("RESHAPE ORDER must currently be a constant array constructor"); };
    if tag != "@array_constructor" { bail!("RESHAPE ORDER must currently be a constant array constructor"); }
    let values = items.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>().context("RESHAPE ORDER must be constant INTEGER")?;
    let mut sorted = values.clone(); sorted.sort_unstable();
    if sorted != (1..=rank as i64).collect::<Vec<_>>() { bail!("RESHAPE ORDER must be a permutation of 1 through result rank"); }
    Ok(values.into_iter().map(|n| n as usize - 1).collect())
}

// A view stores its source, element count, and a linear index formula.
// The private placeholder is replaced only when a consumer selects an element.
pub(crate) fn view_element(e: &Expr, index: &Expr) -> Option<(Expr, i64)> {
    let Expr::Call(tag, args) = e else {
        return None;
    };
    if tag != "@matrix_view" {
        return None;
    }
    let [Expr::Ident(name), count, offset] = args.as_slice() else {
        unreachable!()
    };
    fn substitute(e: &Expr, index: &Expr) -> Expr {
        let value = match e {
            Expr::Ident(n) if n == "@view_index" => index.clone(),
            Expr::Bin(op, a, b) => bin(op.clone(), substitute(a, index), substitute(b, index)),
            Expr::Call(n, args) => Expr::Call(
                n.clone(),
                args.iter().map(|e| substitute(e, index)).collect(),
            ),
            _ => e.clone(),
        };
        if let Expr::Call(n, args) = &value {
            if n == "mod" && args.len() == 2 {
                if let (Ok(a), Ok(b)) = (
                    crate::parameters::integer_constant(&args[0]),
                    crate::parameters::integer_constant(&args[1]),
                ) {
                    if let Some(v) = a.checked_rem(b) {
                        return int(v);
                    }
                }
            }
        }
        crate::parameters::integer_constant(&value)
            .map(int)
            .unwrap_or(value)
    }
    Some((
        Expr::Index(name.clone(), vec![substitute(offset, index)]),
        crate::parameters::integer_constant(count).unwrap(),
    ))
}

fn triplet(e: &Expr) -> Option<&Expr> {
    if let Expr::Call(tag, args) = e {
        if tag == "@section_subscript" {
            return args.first();
        }
    }
    None
}

fn section_view(name: &str, args: &[Expr], dims: &[i64]) -> Result<(Vec<i64>, Expr)> {
    if args.len() != dims.len() {
        bail!(
            "array `{name}` requires {} subscripts, got {}",
            dims.len(),
            args.len()
        );
    }
    let mut shape = vec![];
    let mut offset = int(1);
    let mut storage_stride = 1;
    let mut view_stride = 1i64;
    for (axis, (arg, extent)) in args.iter().zip(dims).enumerate() {
        let constant = |e: &Expr| {
            crate::parameters::integer_constant(e)
            .context("multidimensional section bounds and scalar subscripts must currently be constant integers")
        };
        let position = if let Some(Expr::Section {
            lower,
            upper,
            stride,
            ..
        }) = triplet(arg)
        {
            let step = stride.as_deref().map(&constant).transpose()?.unwrap_or(1);
            if step == 0 {
                bail!("array section stride must not be zero");
            }
            let first = lower.as_deref().map(&constant).transpose()?.unwrap_or(if step>0 {1} else {*extent});
            let last = upper
                .as_deref()
                .map(&constant)
                .transpose()?
                .unwrap_or(if step>0 {*extent} else {1});
            let distance = (i128::from(last) - i128::from(first)) * i128::from(step.signum());
            let count = if distance < 0 {
                0
            } else {
                i64::try_from(distance / i128::from(step).abs() + 1)
                    .context("section size overflow")?
            };
            let final_index =
                i128::from(first) + i128::from(count.saturating_sub(1)) * i128::from(step);
            if count > 0
                && (first < 1
                    || first > *extent
                    || final_index < 1
                    || final_index > i128::from(*extent))
            {
                bail!(
                    "array `{name}` dimension {}: section outside 1:{extent}",
                    axis + 1
                );
            }
            let ordinal = Expr::Call(
                "mod".into(),
                vec![
                    bin(
                        BinOp::Div,
                        bin(BinOp::Sub, Expr::Ident("@view_index".into()), int(1)),
                        int(view_stride.max(1)),
                    ),
                    int(count.max(1)),
                ],
            );
            shape.push(count);
            view_stride = view_stride
                .checked_mul(count)
                .context("section size overflow")?;
            bin(BinOp::Add, int(first), bin(BinOp::Mul, ordinal, int(step)))
        } else {
            let index = constant(arg)?;
            if index < 1 || index > *extent {
                bail!(
                    "array `{name}` dimension {}: subscript {index} outside 1:{extent}",
                    axis + 1
                );
            }
            int(index)
        };
        offset = bin(
            BinOp::Add,
            offset,
            bin(
                BinOp::Mul,
                bin(BinOp::Sub, position, int(1)),
                int(storage_stride),
            ),
        );
        storage_stride = storage_stride
            .checked_mul(*extent)
            .context("array stride overflow")?;
    }
    Ok((
        shape.clone(),
        Expr::Call(
            "@matrix_view".into(),
            vec![Expr::Ident(name.into()), int(total(&shape)?), offset],
        ),
    ))
}

#[derive(Default)]
struct Lowerer {
    guards: Vec<Stmt>,
    forall_serial: usize,
    defer_forall_shapes: bool,
    pure_functions: HashMap<String, bool>,
    elementals: std::collections::HashSet<String>,
    types: HashMap<String, TypeSpec>,
    functions: HashMap<String, TypeSpec>,
    module_arrays: std::collections::HashSet<String>,
}
impl Lowerer {
    fn check_association(&self, name: &str) -> Result<()> {
        if self.module_arrays.contains(name)
            && !self.types.contains_key(name)
            && !self.functions.contains_key(name)
        {
            bail!("USE-associated multidimensional array `{name}` is not yet supported");
        }
        Ok(())
    }
    fn integer_expr(&self, e: &Expr, shapes: &Shapes) -> bool {
        match e {
            Expr::IntLit(n) => n.parse::<i64>().is_ok(),
            Expr::Ident(n) => {
                matches!(
                    self.types.get(n),
                    Some(TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
                ) && !shapes.contains_key(n)
            }
            Expr::Bin(op, a, b) => {
                matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) && self.integer_expr(a, shapes)
                    && self.integer_expr(b, shapes)
            }
            Expr::Un(crate::ast::UnOp::Neg, a) => self.integer_expr(a, shapes),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if let Some(dims) = shapes.get(n) {
                    return matches!(
                        self.types.get(n),
                        Some(TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
                    ) && args.len() == dims.len()
                        && args.iter().all(|a| self.integer_expr(a, shapes));
                }
                if let Some(kind) = self.functions.get(n) {
                    return matches!(kind, TypeSpec::Integer(_));
                }
                match n.as_str() {
                    "size" | "len" | "count" | "@count_default" | "@count_i8" => true,
                    "lbound" | "ubound" => args.len() == 2,
                    "int" | "nint" | "floor" | "ceiling" => args
                        .first()
                        .is_some_and(|e| self.shape(e, shapes).is_ok_and(|s| s.is_none())),
                    "abs" | "mod" | "modulo" | "sign" | "min" | "max" => {
                        args.iter().all(|a| self.integer_expr(a, shapes))
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
    fn check_index_types(&self, name: &str, indices: &[Expr], shapes: &Shapes) -> Result<()> {
        fn has_procedure(e: &Expr, functions: &HashMap<String, TypeSpec>, shapes: &Shapes) -> bool {
            match e {
                Expr::Call(n, args) | Expr::Index(n, args) => {
                    (!n.starts_with("@lower_bound_index_") && !shapes.contains_key(n) && functions.contains_key(n))
                        || args.iter().any(|e| has_procedure(e, functions, shapes))
                }
                Expr::Bin(_, a, b) => {
                    has_procedure(a, functions, shapes) || has_procedure(b, functions, shapes)
                }
                Expr::Un(_, a) => has_procedure(a, functions, shapes),
                _ => false,
            }
        }
        if indices
            .iter()
            .any(|e| has_procedure(e, &self.functions, shapes))
        {
            bail!("procedure calls in multidimensional subscripts are not yet supported");
        }
        if !indices.iter().all(|e| self.integer_expr(e, shapes)) {
            bail!("subscripts of multidimensional array `{name}` must be explicitly typed scalar INTEGER expressions");
        }
        Ok(())
    }
    fn indices(&mut self, name: &str, indices: &mut Vec<Expr>, dims: &[i64]) -> Result<()> {
        if indices.len() != dims.len() {
            bail!(
                "array `{name}` requires {} subscripts, got {}",
                dims.len(),
                indices.len()
            );
        }
        let mut offset = int(1);
        let mut stride = 1i64;
        for (axis, (e, extent)) in indices.iter().zip(dims).enumerate() {
            let checked = if let Ok(index) = crate::parameters::integer_constant(e) {
                if index < 1 || index > *extent {
                    bail!(
                        "array `{name}` dimension {}: subscript {index} outside 1:{extent}",
                        axis + 1
                    );
                }
                int(index)
            } else {
                // A function call checks each dynamic subscript exactly once, including
                // in loop conditions and nested expressions. Checking only the flattened
                // offset would miss invalid subscripts that alias another column.
                let function = format!("@matrix_bound_{}", self.guards.len());
                let parameter = format!("@matrix_index_{}", self.guards.len());
                let value = Expr::Ident(parameter.clone());
                self.guards.push(Stmt::Function {
                    name: function.clone(),
                    params: vec![parameter.clone()],
                    pure: false,
                    elemental: false,
                    return_type: Some(TypeSpec::Integer(Some(8))),
                    body: vec![
                        Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![parameter],
                        },
                        Stmt::If {
                            cond: bin(
                                BinOp::Or,
                                bin(BinOp::Lt, value.clone(), int(1)),
                                bin(BinOp::Gt, value.clone(), int(*extent)),
                            ),
                            then_body: vec![Stmt::CallSub {
                                name: "@error_stop".into(),
                                args: vec![Expr::Str(format!(
                                    "array `{name}` dimension {}: subscript outside 1:{extent}",
                                    axis + 1
                                ))],
                            }],
                            else_body: None,
                        },
                        Stmt::Return(Some(value)),
                    ],
                });
                Expr::Call(function, vec![e.clone()])
            };
            offset = bin(
                BinOp::Add,
                offset,
                bin(BinOp::Mul, bin(BinOp::Sub, checked, int(1)), int(stride)),
            );
            stride = stride
                .checked_mul(*extent)
                .context("array stride overflow")?;
        }
        if let Ok(n) = crate::parameters::integer_constant(&offset) {
            offset = int(n);
        }
        *indices = vec![offset];
        Ok(())
    }

    fn expression(&mut self, e: &mut Expr, shapes: &Shapes) -> Result<()> {
        if matches!(e,Expr::Call(n,_) if crate::real_intrinsics::keys(n).is_some() && !self.functions.contains_key(n) && !shapes.contains_key(n)) {
            self.shape(e,shapes)?;
        }
        if matches!(e,Expr::Call(n,_) if self.elementals.contains(n) || matches!(n.as_str(),"mod" | "modulo" | "sign") || matches!(n.as_str(),"min"|"max") && !self.functions.contains_key(n) && !shapes.contains_key(n)) || matches!(e,Expr::Bin(..)) {
            self.shape(e, shapes)?;
        }
        match e {
            Expr::Ident(n) => self.check_association(n)?,
            Expr::Call(n,_) if matches!(n.as_str(),"@minloc" | "@maxloc" | "@findloc") => {
                let result_dims = self.shape(e,shapes)?.unwrap_or_default();
                let Expr::Call(_,args) = e else { unreachable!() };
                let Some(dims) = self.shape(&args[0],shapes)? else {
                    // A runtime rank-one ARRAY, as an ALLOCATABLE array, is
                    // searched up to its runtime extent.
                    let length = runtime_extent(&args[0]).context("location ARRAY must be an array")?;
                    if !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_dim") && crate::parameters::integer_constant(&args[2]).ok() != Some(1) {
                        bail!("location DIM must be 1 for a rank-one ARRAY");
                    }
                    for i in [0,1,3,5] { self.expression(&mut args[i],shapes)?; }
                    args.extend([int(1),length,Expr::Call("@location_source_shape".into(),vec![int(-1)]),Expr::Call("@location_result_shape".into(),result_dims.into_iter().map(int).collect())]);
                    return Ok(());
                };
                let count = total(&dims)?;
                if let Some(mask) = self.shape(&args[3],shapes)? {
                    if mask != dims { bail!("location MASK shape must conform to ARRAY"); }
                }
                let (stride,axis_len) = if matches!(&args[2],Expr::Call(tag,_) if tag == "@no_dim") { (1,count) } else {
                    let axis = crate::parameters::integer_constant(&args[2])? as usize - 1;
                    (total(&dims[..axis])?,dims[axis])
                };
                for i in [0,1,3,5] { self.expression(&mut args[i],shapes)?; }
                args.extend([int(stride),int(axis_len),Expr::Call("@location_source_shape".into(),dims.into_iter().map(int).collect()),Expr::Call("@location_result_shape".into(),result_dims.into_iter().map(int).collect())]);
            }
            Expr::Call(n, _) if matches!(n.as_str(),"pack" | "unpack") => {
                let dims = self.shape(e,shapes)?.unwrap();
                let Expr::Call(n,args) = e else { unreachable!() };
                if n == "pack" {
                    let source = self.shape(&args[0],shapes)?.context("PACK ARRAY must be an array")?;
                    total(&source)?;
                    if let Some(mask) = self.shape(&args[1],shapes)? {
                        if mask != source { bail!("PACK MASK shape must conform to ARRAY"); }
                    }
                } else {
                    total(&dims)?;
                    if let Some(field) = self.shape(&args[2],shapes)? {
                        if field != dims { bail!("UNPACK FIELD shape must conform to MASK"); }
                    }
                }
                for arg in args.iter_mut() { self.expression(arg,shapes)?; }
                args.push(Expr::Call("@packing_shape".into(),dims.into_iter().map(int).collect()));
                *n = format!("@{n}");
            }
            Expr::Call(n, _) if n == "spread" => {
                let dims = self.shape(e,shapes)?.unwrap();
                let Expr::Call(n,args) = e else { unreachable!() };
                let axis = crate::parameters::integer_constant(&args[1])? as usize - 1;
                self.expression(&mut args[0],shapes)?;
                args[1] = int(total(&dims[..axis])?);
                args[2] = int(dims[axis]);
                args.push(Expr::Call("@spread_shape".into(),dims.into_iter().map(int).collect()));
                *n = "@spread".into();
            }
            Expr::Call(n, _) if n == "matmul" => {
                let dims = self.shape(e,shapes)?.unwrap();
                let Expr::Call(n,args) = e else { unreachable!() };
                let a = self.shape(&args[0],shapes)?.unwrap();
                let b = self.shape(&args[1],shapes)?.unwrap();
                let rows = if a.len() == 2 { a[0] } else { 1 };
                let cols = if b.len() == 2 { b[1] } else { 1 };
                for arg in args.iter_mut() { self.expression(arg,shapes)?; }
                args.extend([int(rows), int(*a.last().unwrap()), int(cols)]);
                args.push(Expr::Call("@matmul_shape".into(),dims.into_iter().map(int).collect()));
                *n = "@matmul".into();
            }
            Expr::Call(n, args) if n == "transpose" => {
                let dims = self.shape(e,shapes)?.unwrap();
                let Expr::Call(_,args) = e else { unreachable!() };
                *e = Expr::Call("reshape".into(),vec![args[0].clone(),Expr::Call("@array_constructor".into(),dims.into_iter().map(int).collect()),Expr::Call("@no_pad".into(),vec![]),Expr::Call("@array_constructor".into(),vec![int(2),int(1)])]);
                self.expression(e,shapes)?;
            }
            Expr::Call(n, args) if n == "eoshift" => {
                let dims = self.shape(&args[0],shapes)?.context("EOSHIFT ARRAY must be an array")?;
                total(&dims)?;
                let dim = crate::parameters::integer_constant(&args[3]).context("EOSHIFT DIM must currently be a constant INTEGER")?;
                if dim < 1 || dim as usize > dims.len() { bail!("EOSHIFT DIM must be between 1 and {}",dims.len()); }
                let axis = dim as usize - 1;
                let mut expected = dims.clone(); expected.remove(axis);
                for (arg,label) in [(&args[1],"SHIFT"),(&args[2],"BOUNDARY")] {
                    if let Some(shape) = self.shape(arg,shapes)? {
                        if shape != expected { bail!("EOSHIFT {label} shape must match ARRAY with DIM removed"); }
                    }
                }
                for arg in &mut args[..3] { self.expression(arg,shapes)?; }
                let boundary = args[2].clone();
                args[2] = int(total(&dims[..axis])?);
                args[3] = int(dims[axis]);
                args.push(Expr::Call("@shift_shape".into(),dims.into_iter().map(int).collect()));
                args.push(boundary);
                *n = "@eoshift".into();
            }
            Expr::Call(n, args) if n == "cshift" => {
                let dims = self.shape(&args[0], shapes)?.context("CSHIFT ARRAY must be an array")?;
                total(&dims)?;
                let dim = crate::parameters::integer_constant(&args[2]).context("CSHIFT DIM must currently be a constant INTEGER")?;
                if dim < 1 || dim as usize > dims.len() { bail!("CSHIFT DIM must be between 1 and {}", dims.len()); }
                if let Some(shift_shape) = self.shape(&args[1], shapes)? {
                    let mut expected = dims.clone(); expected.remove(dim as usize - 1);
                    if shift_shape != expected { bail!("CSHIFT SHIFT shape must match ARRAY with DIM removed"); }
                }
                self.expression(&mut args[0], shapes)?;
                self.expression(&mut args[1], shapes)?;
                let axis = dim as usize - 1;
                args[2] = int(total(&dims[..axis])?);
                args.push(int(dims[axis]));
                args.push(Expr::Call("@shift_shape".into(),dims.into_iter().map(int).collect()));
                *n = "@cshift".into();
            }
            Expr::Call(n, args) if n == "reshape" => {
                let dims = reshape_shape(args)?;
                reshape_order(args,dims.len())?;
                self.shape(&args[0], shapes)?.context("RESHAPE SOURCE must be an array")?;
                self.expression(&mut args[0], shapes)?;
                if !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_pad") {
                    self.shape(&args[2], shapes)?.context("RESHAPE PAD must be an array")?;
                    self.expression(&mut args[2], shapes)?;
                }
            }
            Expr::Call(n, args) if matches!(n.as_str(), "sum" | "product" | "minval" | "maxval" | "iany" | "iall" | "iparity" | "parity" | "@count_default" | "@count_i8" | "@any_reduction" | "@all_reduction") && args.len() == 3 => {
                let intrinsic = match n.as_str() { "@count_default" | "@count_i8" => "count", "@any_reduction" => "any", "@all_reduction" => "all", _ => n.as_str() };
                let dims = self.shape(&args[0], shapes)?.context("reduction argument must be an array")?;
                total(&dims)?;
                // In the (ARRAY, MASK) form an array second argument is the MASK.
                if matches!(&args[2], Expr::Logical(true)) && self.shape(&args[1], shapes)?.is_some() {
                    args[2] = std::mem::replace(&mut args[1], Expr::Call("@no_dim".into(), vec![]));
                }
                let mut reduction_axis = None;
                if !matches!(&args[1], Expr::Call(tag, _) if tag == "@no_dim") {
                    let dim = crate::parameters::integer_constant(&args[1]).context("reduction DIM must currently be a constant INTEGER; use MASK= for masks")?;
                    if dim < 1 || dim as usize > dims.len() { bail!("{intrinsic}: DIM must be between 1 and {}", dims.len()); }
                    if dims.len() > 1 { reduction_axis = Some(dim as usize - 1); }
                }
                if let Some(mask_shape) = self.shape(&args[2], shapes)? {
                    if mask_shape != dims { bail!("{n}: MASK shape does not conform to ARRAY"); }
                }
                args.remove(1);
                for arg in args.iter_mut() { self.expression(arg, shapes)?; }
                if let Some(axis) = reduction_axis {
                    let stride = total(&dims[..axis])?;
                    let mut result_dims = dims.clone();
                    result_dims.remove(axis);
                    args.extend([int(stride), int(dims[axis]), Expr::Call("@reduction_shape".into(), result_dims.into_iter().map(int).collect())]);
                }
            }
            Expr::Call(n, args) if n == "shape" => {
                let [source] = args.as_slice() else { bail!("SHAPE expects SOURCE and optional KIND"); };
                let owner = match source {
                    Expr::Ident(n) | Expr::Section { name: n, .. } | Expr::Call(n, _) | Expr::Index(n, _) => n,
                    _ => bail!("SHAPE currently requires an array variable or section"),
                };
                self.check_association(owner)?;
                let dims = self.shape(source, shapes)?.context("SHAPE requires an array with a statically known shape")?;
                total(&dims)?;
                *e = Expr::Call("@bounds_vector".into(), dims.into_iter().map(int).collect());
            }
            Expr::Call(n, args) if matches!(n.as_str(), "lbound" | "ubound") => {
                let array = args.first().context("array bound inquiry requires ARRAY")?;
                let owner = match array {
                    Expr::Ident(n) | Expr::Section { name: n, .. } | Expr::Call(n, _) | Expr::Index(n, _) => n,
                    _ => bail!("{n} currently requires an array variable or section"),
                };
                self.check_association(owner)?;
                let dims = self.shape(array, shapes)?.with_context(|| format!("{n} requires an array with a statically known shape"))?;
                total(&dims)?;
                *e = crate::inquiries::bounds(n, &dims, args.get(1))?;
            }
            Expr::Call(n, args) if n == "size" => {
                let Some(Expr::Ident(array)) = args.first() else {
                    // An array expression, as SIZE(a+1) or SIZE(LEN_TRIM(s)),
                    // has the extent of its arrays, which is not evaluated.
                    let source = args.first().context("SIZE expects an array and optional dimension")?;
                    if args.len() > 2 { bail!("SIZE expects an array and optional dimension"); }
                    let axis = args.get(1).map(|d| crate::parameters::integer_constant(d).context("SIZE dimension must currently be constant")).transpose()?;
                    if let Some(mut extent) = runtime_extent(source) {
                        if axis.is_some_and(|a| a != 1) { bail!("SIZE dimension must be 1 for a rank-one array"); }
                        self.expression(&mut extent, shapes)?;
                        *e = extent;
                        return Ok(());
                    }
                    let dims = self.shape(source, shapes)?.context("SIZE currently requires an array")?;
                    *e = int(match axis {
                        None => total(&dims)?,
                        Some(axis) if axis >= 1 && axis as usize <= dims.len() => dims[axis as usize - 1],
                        Some(_) => bail!("SIZE dimension must be between 1 and {}", dims.len()),
                    });
                    return Ok(());
                };
                self.check_association(array)?;
                let Some(dims) = shapes.get(array) else {
                    return Ok(());
                };
                let value = match args.as_slice() {
                    [_] => total(dims)?,
                    [_, dim] => {
                        let axis = crate::parameters::integer_constant(dim)
                            .context("SIZE dimension must currently be constant")?;
                        if axis < 1 || axis as usize > dims.len() {
                            bail!("SIZE dimension must be between 1 and {}", dims.len());
                        }
                        dims[axis as usize - 1]
                    }
                    _ => bail!("SIZE expects an array and optional dimension"),
                };
                *e = int(value);
            }
            Expr::Call(n, args) | Expr::Index(n, args)
                if args.iter().any(|e| triplet(e).is_some()) =>
            {
                self.check_association(n)?;
                let dims = shapes
                    .get(n)
                    .with_context(|| format!("section `{n}` requires a declared array"))?;
                *e = section_view(n, args, dims)?.1;
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                self.check_association(n)?;
                if shapes.get(n).is_some_and(|d| d.len() > 1) {
                    self.check_index_types(n, args, shapes)?;
                }
                for a in args.iter_mut() {
                    self.expression(a, shapes)?;
                }
                if let Some(dims) = shapes.get(n).filter(|d| d.len() > 1) {
                    self.indices(n, args, dims)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, shapes)?;
                self.expression(b, shapes)?;
            }
            Expr::Un(_, a) => self.expression(a, shapes)?,
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if shapes.get(name).is_some_and(|d| d.len() > 1) {
                    bail!("multidimensional sections of `{name}` are not yet supported");
                }
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, shapes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shape(&self, e: &Expr, shapes: &Shapes) -> Result<Option<Vec<i64>>> {
        match e {
            Expr::Call(n,args) if matches!(n.as_str(),"@minloc" | "@maxloc" | "@findloc") => {
                if args.len()==10 {
                    let Expr::Call(_,dims) = &args[9] else { unreachable!() };
                    return Ok(if dims.is_empty() { None } else { Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?) });
                }
                let Some(mut dims) = self.shape(&args[0],shapes)? else {
                    runtime_extent(&args[0]).context("location ARRAY must be an array")?;
                    return Ok(if matches!(&args[2],Expr::Call(tag,_) if tag == "@no_dim") { Some(vec![1]) } else { None });
                };
                total(&dims)?;
                if matches!(&args[2],Expr::Call(tag,_) if tag == "@no_dim") { return Ok(Some(vec![dims.len() as i64])); }
                let dim = crate::parameters::integer_constant(&args[2]).context("location DIM must currently be a constant INTEGER; use MASK= when omitting DIM")?;
                if dim<1 || dim as usize>dims.len() { bail!("location DIM must be between 1 and {}",dims.len()); }
                dims.remove(dim as usize-1);
                Ok(if dims.is_empty() {None} else {Some(dims)})
            }
            Expr::Call(n,args) if n == "unpack" => {
                let vector = self.shape(&args[0],shapes)?.context("UNPACK VECTOR must be a rank-one array")?;
                if vector.len()!=1 { bail!("UNPACK VECTOR must be a rank-one array"); }
                total(&vector)?;
                Ok(Some(self.shape(&args[1],shapes)?.context("UNPACK MASK must be an array")?))
            }
            Expr::Call(n,args) if n == "pack" => {
                let source = self.shape(&args[0],shapes)?.context("PACK ARRAY must be an array")?;
                let count = if !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_vector") {
                    let vector = self.shape(&args[2],shapes)?.context("PACK VECTOR must be a rank-one array")?;
                    if vector.len()!=1 { bail!("PACK VECTOR must be a rank-one array"); }
                    total(&vector)?;
                    vector[0]
                } else {
                    match &args[1] {
                        Expr::Logical(value) => if *value { total(&source)? } else { 0 },
                        Expr::Call(tag,items) if tag == "@array_constructor" && items.iter().all(|e| matches!(e,Expr::Logical(_))) => items.iter().filter(|e| matches!(e,Expr::Logical(true))).count() as i64,
                        _ => -1,
                    }
                };
                Ok(Some(vec![count]))
            }
            Expr::Call(n,args) if matches!(n.as_str(),"@pack" | "@unpack") => {
                let Expr::Call(_,dims) = &args[3] else { unreachable!() };
                Ok(Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?))
            }
            Expr::Call(n,args) if n == "spread" => {
                let mut dims = self.shape(&args[0],shapes)?.unwrap_or_default();
                let dim = crate::parameters::integer_constant(&args[1]).context("SPREAD DIM must currently be a constant INTEGER")?;
                if dim < 1 || dim as usize > dims.len()+1 { bail!("SPREAD DIM must be between 1 and {}",dims.len()+1); }
                let copies = crate::parameters::integer_constant(&args[2]).context("SPREAD NCOPIES must currently be a constant INTEGER")?.max(0);
                dims.insert(dim as usize - 1,copies);
                total(&dims)?;
                Ok(Some(dims))
            }
            Expr::Call(n,args) if n == "@spread" => {
                let Expr::Call(_,dims) = &args[3] else { unreachable!() };
                Ok(Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?))
            }
            Expr::Call(n, args) if n == "matmul" => {
                let [a, b] = args.as_slice() else { bail!("MATMUL expects MATRIX_A and MATRIX_B"); };
                let a = self.shape(a,shapes)?.context("MATMUL MATRIX_A must be an array")?;
                let b = self.shape(b,shapes)?.context("MATMUL MATRIX_B must be an array")?;
                if a.len() > 2 || b.len() > 2 || a.len() == 1 && b.len() == 1 {
                    bail!("MATMUL requires rank-one or rank-two arguments, at least one of rank two");
                }
                if a.last() != b.first() { bail!("MATMUL operand shapes do not conform"); }
                let mut dims = vec![];
                if a.len() == 2 { dims.push(a[0]); }
                if b.len() == 2 { dims.push(b[1]); }
                total(&dims)?;
                Ok(Some(dims))
            }
            Expr::Call(n, args) if n == "@matmul" => {
                let Expr::Call(_, dims) = &args[5] else { unreachable!() };
                Ok(Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?))
            }
            Expr::Call(n, args) if n == "transpose" => {
                let mut dims = self.shape(&args[0],shapes)?.context("TRANSPOSE MATRIX must be a rank-two array")?;
                if dims.len() != 2 { bail!("TRANSPOSE MATRIX must be a rank-two array"); }
                dims.reverse();
                Ok(Some(dims))
            }
            Expr::Call(n, args) if matches!(n.as_str(),"cshift" | "eoshift") => self.shape(&args[0], shapes),
            Expr::Call(n, args) if matches!(n.as_str(),"@cshift" | "@eoshift") => {
                let Expr::Call(_, dims) = &args[4] else { unreachable!() };
                Ok(Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?))
            }
            Expr::Call(n, args) if n == "reshape" => Ok(Some(reshape_shape(args)?)),
            Expr::Call(n, args) if n == "@array_constructor" => {
                let mut count = 0i64;
                for arg in args { count = count.checked_add(match self.shape(arg, shapes)? { Some(dims) => total(&dims)?, None => 1 }).context("array constructor extent overflow")?; }
                Ok(Some(vec![count]))
            }
            Expr::Call(n, args) if matches!(n.as_str(), "sum" | "product" | "minval" | "maxval" | "iany" | "iall" | "iparity" | "parity" | "@count_default" | "@count_i8" | "@any_reduction" | "@all_reduction") => {
                if args.len() == 5 {
                    let Expr::Call(_, dims) = &args[4] else { unreachable!() };
                    return Ok(Some(dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?));
                }
                if args.len() != 3 || matches!(&args[1],Expr::Call(tag,_) if tag == "@no_dim") || self.shape(&args[1], shapes)?.is_some() { return Ok(None); }
                let mut dims = self.shape(&args[0], shapes)?.context("reduction argument must be an array")?;
                let axis = crate::parameters::integer_constant(&args[1]).context("reduction DIM must currently be a constant INTEGER")?;
                if axis < 1 || axis as usize > dims.len() { bail!("reduction DIM must be between 1 and {}", dims.len()); }
                dims.remove(axis as usize - 1);
                Ok(if dims.is_empty() { None } else { Some(dims) })
            }
            Expr::Call(n, args) if n == "@bounds_vector" => Ok(Some(vec![args.len() as i64])),
            Expr::Call(n, args) if n == "shape" => {
                let [source] = args.as_slice() else { bail!("SHAPE expects SOURCE and optional KIND"); };
                let dims = self.shape(source, shapes)?.context("SHAPE requires an array")?;
                Ok(Some(vec![dims.len() as i64]))
            }
            Expr::Call(n, args) if matches!(n.as_str(), "lbound" | "ubound") => {
                let dims = self.shape(&args[0], shapes)?.context("bound inquiry requires an array")?;
                Ok(if args.len() == 1 { Some(vec![dims.len() as i64]) } else { None })
            }
            Expr::Ident(n) => Ok(shapes.get(n).cloned()),

            Expr::Section { name, .. } if shapes.get(name).is_some_and(|d| d.len() == 1) => {
                Ok(Some(
                    section_view(
                        name,
                        &[Expr::Call("@section_subscript".into(), vec![e.clone()])],
                        &shapes[name],
                    )?
                    .0,
                ))
            }

            Expr::Call(n, args) | Expr::Index(n, args)
                if args.iter().any(|e| triplet(e).is_some()) =>
            {
                let dims = shapes
                    .get(n)
                    .with_context(|| format!("section `{n}` requires a declared array"))?;
                Ok(Some(section_view(n, args, dims)?.0))
            }

            Expr::Bin(_, a, b) => {
                let a = self.shape(a, shapes)?;
                let b = self.shape(b, shapes)?;
                if let (Some(a), Some(b)) = (&a, &b) {
                    if !conform(a,b) {
                        bail!("nonconforming array shapes {a:?} and {b:?}");
                    }
                }
                Ok(a.or(b))
            }
            Expr::Un(_, a) => self.shape(a, shapes),
            Expr::Call(n, args)
                if self.elementals.contains(n)
                    || crate::intrinsics::math_arity(n).is_some()
                    || (crate::intrinsics::integer_rounding(n) || crate::intrinsics::real_predicate(n)) && !self.functions.contains_key(n) && !shapes.contains_key(n)
                    || crate::real_intrinsics::keys(n).is_some() && !self.functions.contains_key(n) && !shapes.contains_key(n)
                    || matches!(n.as_str(),"min"|"max") && !self.functions.contains_key(n) && !shapes.contains_key(n)
                    || matches!(n.as_str(), "abs" | "dble" | "real" | "mod" | "modulo" | "sign") =>
            {
                let mut shape:Option<Vec<i64>> = None;
                for arg in args {
                    let next = self.shape(arg, shapes)?;
                    if let (Some(a), Some(b)) = (&shape, &next) {
                        if !conform(a,b) {
                            bail!("nonconforming array argument shapes {a:?} and {b:?}");
                        }
                    }
                    shape = shape.or(next);
                }
                Ok(shape)
            }
            _ => Ok(None),
        }
    }

    fn expand_forall(&mut self, body: &mut Vec<Stmt>, shapes: &Shapes, symbols: Option<&crate::allocatables::Shapes>) -> Result<()> {
        let Some(Stmt::CallSub { name: tag, args }) = body.first() else {
            return Ok(());
        };
        if tag != "@forall" {
            return Ok(());
        }
        if args.is_empty() || args.len() % 4 != 0 {
            bail!("FORALL requires index triplets");
        }
        let constant = crate::parameters::integer_constant;
        let mut controls = Vec::new();
        let mut runtime = false;
        let mut resolved_controls = Vec::new();
        let mut names = std::collections::HashSet::new();
        for control in args.chunks_exact(4) {
            let Expr::Ident(index) = &control[0] else { bail!("FORALL index must be an integer variable"); };
            if !names.insert(index.clone()) { bail!("duplicate FORALL index `{index}`"); }
            if shapes.contains_key(index) || !matches!(self.types.get(index), Some(TypeSpec::Integer(None | Some(4 | 8)))) {
                bail!("FORALL index `{index}` currently requires an explicitly declared scalar INTEGER of kind 4 or 8");
            }
            let mut bounds = control[1..].to_vec();
            for bound in &mut bounds {
                let integer=self.integer_expr(bound,shapes);
                self.expression(bound, shapes)?;
                if !integer && !self.integer_expr(bound,shapes) {
                    bail!("FORALL bounds and stride must be scalar INTEGER expressions");
                }
            }
            resolved_controls.push(Expr::Ident(index.clone()));
            // Keep original-rank references for the later storage pass. The
            // normalized copies above are only for constant/type inspection.
            resolved_controls.extend_from_slice(&control[1..]);
            if bounds.iter().any(|e|constant(e).is_err()) {
                runtime = true;
                continue;
            }
            let (start, end, step) = (i128::from(constant(&bounds[0])?), i128::from(constant(&bounds[1])?), i128::from(constant(&bounds[2])?));
            if step == 0 { bail!("FORALL stride must not be zero"); }
            let distance = if step > 0 { end - start } else { start - end };
            let count = if distance < 0 { 0 } else { distance / step.abs() + 1 };
            controls.push((index.clone(), start, step, count));
        }
        let iterations = if runtime || controls.iter().any(|c| c.3 == 0) { 0 } else {
            controls.iter().try_fold(1usize, |n, c| {
                usize::try_from(c.3).ok().and_then(|count| n.checked_mul(count))
                    .context("FORALL iteration count is too large")
            })?
        };
        let [_, Stmt::AssignIndex {
            name,
            indices,
            value,
        }] = body.as_slice()
        else {
            bail!("FORALL currently requires one array-element or array-section assignment");
        };
        self.check_association(name)?;
        let runtime_axes=symbols.and_then(|s|crate::allocatables::axes(name,s));
        let dims=shapes.get(name).map(Vec::as_slice).unwrap_or(&[]);
        if dims.is_empty() && runtime_axes.is_none() {bail!("FORALL destination `{name}` requires a known array shape");}
        let kind = self
            .types
            .get(name)
            .context("FORALL destination type is unknown")?
            .clone();
        if !matches!(
            kind,
            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                | TypeSpec::Real
                | TypeSpec::DoublePrecision | TypeSpec::Logical | TypeSpec::Character(_)
        ) {
            bail!("FORALL currently requires a supported numeric or CHARACTER destination array");
        }
        fn pure(e: &Expr, shapes: &Shapes, known: &HashMap<String, bool>) -> Result<()> {
            match e {
                Expr::Call(n, args) | Expr::Index(n, args) => {
                    let intrinsic = n.starts_with("@lower_bound_index_")
                        || crate::intrinsics::purity(n) == Some(crate::intrinsics::Purity::Pure)
                        || matches!(
                            n.as_str(),
                            "@section_subscript" | "@array_constructor" | "@typed_constructor" | "@packed_view" | "@allocated_extent"
                                | "@integer_literal" | "@no_dim" | "@no_mask" | "@no_kind" | "@any_reduction" | "@all_reduction" | "@count_default" | "@count_i8"
                                | "@unary_plus"
                                | "abs"
                                | "min"
                                | "max"
                                | "mod"
                                | "modulo"
                                | "int"
                                | "real"
                                | "dble"
                                | "size"
                                | "lbound"
                                | "ubound"
                                | "sum"
                                | "product" | "iany" | "iall" | "iparity" | "parity"
                                | "minval"
                                | "maxval" | "trim" | "len" | "len_trim" | "adjustl" | "adjustr" | "repeat" | "char" | "achar"
                        );
                    if !shapes.contains_key(n) && !known.get(n).copied().unwrap_or(intrinsic) {
                        bail!("FORALL cannot call impure or unsupported procedure `{n}`");
                    }
                    for e in args {
                        pure(e, shapes, known)?;
                    }
                }
                Expr::Bin(_, a, b) => {
                    pure(a, shapes, known)?;
                    pure(b, shapes, known)?;
                }
                Expr::Un(_, a) => pure(a, shapes, known)?,
                Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } => {
                    for e in [lower, upper, stride].into_iter().flatten() {
                        pure(e, shapes, known)?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        let mut arrays=shapes.clone();
        if let Some(symbols)=symbols {for (n,(_,d)) in symbols {if !d.is_empty() {arrays.entry(n.clone()).or_default();}}}
        pure(value, &arrays, &self.pure_functions)?;
        for e in indices {pure(e,&arrays,&self.pure_functions)?;}
        if runtime || runtime_axes.is_some() {
            for control in resolved_controls.chunks_exact(4) {
                for e in &control[1..] {pure(e,&arrays,&self.pure_functions)?;}
            }
            let extents=runtime_axes.unwrap_or_else(||dims.iter().copied().map(int).collect());
            *body=crate::forall_runtime::expand(&resolved_controls,name,indices,value,&kind,&extents,symbols,&self.types,&mut self.forall_serial)?;
            return Ok(());
        }
        fn replace(e: &mut Expr, index: &str, value: i64) {
            match e {
                Expr::Ident(n) if n == index => *e = int(value),
                Expr::Bin(_, a, b) => {
                    replace(a, index, value);
                    replace(b, index, value);
                }
                Expr::Un(_, a) => replace(a, index, value),
                Expr::Call(_, args) | Expr::Index(_, args) => {
                    for e in args {
                        replace(e, index, value);
                    }
                }
                Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } => {
                    for e in [lower, upper, stride].into_iter().flatten() {
                        replace(e, index, value);
                    }
                }
                _ => {}
            }
        }
        let mut captures: Vec<Stmt> = controls.iter().map(|c| Stmt::DummyIntent {
            name: c.0.clone(), intent: "@forall_index".into(),
        }).collect();
        let mut writes = vec![];
        let mut destinations = std::collections::HashSet::new();
        // Enumerate the Cartesian product, capturing every RHS before any write.
        for iteration in 0..iterations {
            let mut ordinal = iteration;
            let mut indices = indices.clone();
            let mut value = value.clone();
            for (index, start, step, count) in &controls {
                let count = *count as usize;
                let current = start + (ordinal % count) as i128 * step;
                ordinal /= count;
                for e in &mut indices { replace(e, index, current as i64); }
                replace(&mut value, index, current as i64);
            }
            let normalized = match indices.as_slice() {
                [section @ Expr::Section { .. }] => vec![Expr::Call(
                    "@section_subscript".into(),
                    vec![section.clone()],
                )],
                _ => indices.clone(),
            };
            let shape = if normalized.iter().any(|e| triplet(e).is_some()) {
                let (shape, view) = section_view(name, &normalized, dims)?;
                for i in 1..=total(&shape)? {
                    let (Expr::Index(_, offset), _) = view_element(&view, &int(i)).unwrap() else {
                        unreachable!()
                    };
                    if !destinations.insert(constant(&offset[0])?) {
                        bail!("FORALL iterations define the same destination element of `{name}` more than once");
                    }
                }
                shape
            } else {
                for e in &indices {
                    constant(e).context(
                        "FORALL destination subscripts must be known for every iteration",
                    )?;
                }
                let mut flattened = indices.clone();
                self.indices(name, &mut flattened, dims)?;
                if !destinations.insert(constant(&flattened[0])?) {
                    bail!("FORALL iterations define the same destination element of `{name}` more than once");
                }
                vec![]
            };
            fn pending_call(e:&Expr, known:&HashMap<String,bool>, shapes:&Shapes)->bool {
                match e {
                    Expr::Call(n,a)|Expr::Index(n,a)=>known.contains_key(n)&&!shapes.contains_key(n)
                        || a.iter().any(|e|pending_call(e,known,shapes)),
                    Expr::Bin(_,a,b)=>pending_call(a,known,shapes)||pending_call(b,known,shapes),
                    Expr::Un(_,a)=>pending_call(a,known,shapes),_=>false,
                }
            }
            // A function inside a constructor may still produce an array.
            // Check known shapes now, and defer those calls until specialized.
            if !(self.defer_forall_shapes && pending_call(&value,&self.pure_functions,shapes)) {
                match self.shape(&value, shapes) {
                    Ok(Some(rhs_shape)) if rhs_shape!=shape => bail!("nonconforming FORALL assignment shapes {shape:?} and {rhs_shape:?}"),
                    Err(error) if !self.defer_forall_shapes => return Err(error),
                    _ => {},
                }
            }
            if !shape.is_empty() && total(&shape)? == 0 {
                continue;
            }
            self.forall_serial += 1;
            let temp = if matches!(kind,TypeSpec::Character(_)) {format!("@character_forall_value_{}",self.forall_serial)} else {format!("@forall_value_{}",self.forall_serial)};
            captures.push(if shape.is_empty() {
                Stmt::VarDecl {
                    kind: kind.clone(),
                    names: vec![temp.clone()],
                }
            } else {
                Stmt::ArrayDecl {
                    kind: kind.clone(),
                    name: temp.clone(),
                    dims: shape.into_iter().map(int).collect(),
                }
            });
            captures.push(Stmt::Assign {
                name: temp.clone(),
                value,
            });
            writes.push(Stmt::AssignIndex {
                name: name.clone(),
                indices,
                value: Expr::Ident(temp),
            });
        }
        captures.extend(writes);
        *body = captures;
        Ok(())
    }

    fn body(&mut self, body: &mut [Stmt], inherited: &Shapes) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut shapes = inherited.clone();
        let inherited_types = self.types.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, kind } => {
                    for n in names {
                        shapes.remove(n);
                        self.types.insert(n.clone(), kind.clone());
                    }
                }
                Stmt::ArrayDecl { name, kind, dims } => {
                    self.types.insert(name.clone(), kind.clone());
                    shapes.remove(name);
                    let extents = dims
                        .iter()
                        .map(crate::parameters::integer_constant)
                        .collect::<Result<Vec<_>>>();
                    if dims.len() > 1 {
                        if !matches!(
                            kind,
                            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                                | TypeSpec::Real
                                | TypeSpec::DoublePrecision
                                | TypeSpec::Logical
                                | TypeSpec::Character(_)
                        ) {
                            bail!("multidimensional array `{name}` requires a supported numeric or LOGICAL element type");
                        }
                        let extents = extents.with_context(|| {
                            format!(
                                "multidimensional array `{name}` requires a statically known shape"
                            )
                        })?;
                        if extents.iter().any(|d| *d < 0) {
                            bail!("multidimensional array `{name}` currently requires nonnegative extents");
                        }
                        total(&extents)?;
                        shapes.insert(name.clone(), extents);
                    } else if let Ok(extents) = extents {
                        shapes.insert(name.clone(), extents);
                    }
                }
                _ => {}
            }
        }
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) { statement_location.set(Some(p)); }
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    if let Some(shape) = shapes.get(name).filter(|d| d.len() > 1) {
                        *dims = vec![int(total(shape)?)];
                    }
                }
                Stmt::Assign { name, value } => {
                    self.check_association(name)?;
                    if let (Some(a), Some(b)) = (shapes.get(name), self.shape(value, &shapes)?) {
                        if !conform(a,&b) {
                            bail!("nonconforming array assignment to `{name}`: {a:?} and {b:?}");
                        }
                    }
                    self.expression(value, &shapes)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.check_association(name)?;
                    let section_indices = match indices.as_slice() {
                        [section @ Expr::Section { .. }] => Some(vec![Expr::Call(
                            "@section_subscript".into(),
                            vec![section.clone()],
                        )]),
                        _ if indices.iter().any(|e| triplet(e).is_some()) => Some(indices.clone()),
                        _ => None,
                    };
                    if let Some(section_indices) = section_indices {
                        let dims = shapes.get(name).with_context(|| {
                            format!("section `{name}` requires a declared array")
                        })?;
                        let (target_shape, view) = section_view(name, &section_indices, dims)?;
                        let rhs_shape = self.shape(value, &shapes)?;
                        if let Some(rhs_shape) = rhs_shape {
                            if target_shape != rhs_shape {
                                bail!("nonconforming section assignment to `{name}`: {target_shape:?} and {rhs_shape:?}");
                            }
                        }
                        *indices = vec![view];
                        self.expression(value, &shapes)?;
                        continue;
                    }
                    if shapes.get(name).is_some_and(|d| d.len() > 1) {
                        self.check_index_types(name, indices, &shapes)?;
                    }
                    for e in indices.iter_mut() {
                        self.expression(e, &shapes)?;
                    }
                    if let Some(dims) = shapes.get(name).filter(|d| d.len() > 1) {
                        self.indices(name, indices, dims)?;
                    }
                    self.expression(value, &shapes)?;
                }
                Stmt::Print { items }
                | Stmt::Read { args: items }
                | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expression(e, &shapes)?;
                    }
                }
                Stmt::Return(Some(e)) => self.expression(e, &shapes)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &shapes)?;
                    self.body(then_body, &shapes)?;
                    if let Some(b) = else_body {
                        self.body(b, &shapes)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &shapes)?;
                    self.expression(end, &shapes)?;
                    self.body(body, &shapes)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &shapes)?;
                    self.body(body, &shapes)?;
                }
                Stmt::Block { body } => {
                    // Check WHERE shapes before numeric arrays are flattened.
                    if let Some(Stmt::CallSub{name,args}) = body.first() {
                        if name == "@where" {
                            let shape=self.shape(&args[0],&shapes)?.context("WHERE mask must be an array")?;
                            for stmt in body.iter() {
                                match stmt {
                                    Stmt::Assign{name,..} if shapes.get(name)!=Some(&shape) => bail!("WHERE destination shape does not conform to mask"),
                                    Stmt::Block { body } => {
                                        if let Some(Stmt::CallSub{name,args}) = body.first() {
                                            if name == "@where" && self.shape(&args[0],&shapes)?.as_ref()!=Some(&shape) {
                                                bail!("nested WHERE mask shapes do not conform");
                                            }
                                        }
                                    }
                                    Stmt::CallSub{name,args} if name == "@elsewhere" && !args.is_empty() => {
                                        if self.shape(&args[0],&shapes)?.as_ref()!=Some(&shape) {bail!("WHERE mask shapes do not conform");}
                                    }
                                    _=>{}
                                }
                            }
                        }
                    }
                    self.expand_forall(body, &shapes, None)?;
                    self.body(body, &shapes)?;
                }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                    self.body(body, &shapes)?
                }
                Stmt::Module { body, .. } => {
                    let saved = std::mem::take(&mut self.types);
                    self.body(body, &Shapes::new())?;
                    self.types = saved;
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &shapes)?;
                    for c in cases {
                        for item in &mut c.items {
                            match item {
                                CaseItem::Single(e) => self.expression(e, &shapes)?,
                                CaseItem::Range(a, b) => {
                                    self.expression(a, &shapes)?;
                                    self.expression(b, &shapes)?;
                                }
                            }
                        }
                        self.body(&mut c.body, &shapes)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &shapes)?;
                    }
                }
                _ => {}
            }
        }
        self.types = inherited_types;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
// Function specialization precedes the main multidimensional pass. Reuse its
// inquiry lowering while the original ranks and extents are still available.
pub(crate) fn fixed_inquiry(e: &Expr, symbols: &HashMap<String,(TypeSpec,Vec<Expr>)>) -> Result<Expr> {
    let mut lower = Lowerer::default();
    lower.types = symbols.iter().map(|(n,(kind,_))| (n.clone(),kind.clone())).collect();
    let shapes = symbols.iter().filter_map(|(n,(_,dims))| {
        if dims.is_empty() { return None; }
        dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()
            .ok().map(|dims| (n.clone(),dims))
    }).collect();
    let mut value = e.clone();
    lower.expression(&mut value, &shapes)?;
    Ok(value)
}

// Expand a FORALL before function specialization only when its destination
// shape is known. RHS conformance is checked after calls have concrete results.
pub(crate) fn specialize_forall(body: &mut Vec<Stmt>, symbols: &HashMap<String,(TypeSpec,Vec<Expr>)>, pure: &HashMap<String,bool>, returns: &HashMap<String,(bool,TypeSpec)>, serial: &mut usize) -> Result<bool> {
    let Some(marker)=body.iter().position(|s|matches!(s,Stmt::CallSub{name,..} if name=="@forall")) else {return Ok(false)};
    if !body[..marker].iter().all(|s|matches!(s,Stmt::ArrayDecl{..}|Stmt::VarDecl{..}|Stmt::SourceLocation(_))) {return Ok(false);}
    let Some(Stmt::AssignIndex{name,..})=body.get(marker+1) else {return Ok(false)};
    let mut symbols=symbols.clone();
    for s in &body[..marker] {match s {
        Stmt::ArrayDecl{kind,name,dims}=>{symbols.insert(name.clone(),(kind.clone(),dims.clone()));},
        Stmt::VarDecl{kind,names}=>for n in names {symbols.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
    }}
    if crate::allocatables::axes(name,&symbols).is_none() && crate::allocatables::length(name,&symbols).is_some_and(|len|crate::parameters::integer_constant(&len).is_err()) {
        bail!("FORALL currently requires a statically shaped destination; runtime-sized array `{name}` is not yet supported");
    }
    let mut lower=Lowerer {forall_serial:*serial,defer_forall_shapes:true,..Default::default()};
    lower.types=symbols.iter().map(|(n,(kind,_))|(n.clone(),kind.clone())).collect();
    lower.pure_functions=pure.clone();
    lower.functions=returns.iter().map(|(n,(_,kind))|(n.clone(),kind.clone())).collect();
    let shapes:Shapes=symbols.iter().filter_map(|(n,(_,dims))| {
        if dims.is_empty() || crate::allocatables::length(n,&symbols).is_some_and(|len|crate::parameters::integer_constant(&len).is_err()) {None}
        else {Some((n.clone(),dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()))}
    }).filter_map(|(n,dims)|dims.ok().map(|d|(n,d))).collect();
    if !shapes.contains_key(name) && crate::allocatables::axes(name,&symbols).is_none() {return Ok(false);}
    if matches!(lower.types.get(name),Some(TypeSpec::Character(_))) {
        let dims=shapes.get(name).context("runtime-sized CHARACTER FORALL destinations are not yet supported")?;
        if dims.len()!=1 || !(0..=1024).contains(&dims[0]) {bail!("CHARACTER FORALL requires a fixed rank-one destination with at most 1024 elements");}
    }
    let mut statement=body[marker..].to_vec();
    lower.expand_forall(&mut statement,&shapes,Some(&symbols))?;
    body.truncate(marker);body.extend(statement);
    *serial=lower.forall_serial;
    Ok(true)
}

// Build original-rank element references for a fixed matrix section. Flattening
// remains the responsibility of the later multidimensional storage pass.
pub(crate) fn fixed_section_elements(e: &Expr, symbols: &HashMap<String,(TypeSpec,Vec<Expr>)>) -> Result<Option<Vec<Expr>>> {
    let (Expr::Call(name,args)|Expr::Index(name,args))=e else {return Ok(None)};
    let Some((_,dims))=symbols.get(name) else {return Ok(None)};
    if !args.iter().any(|e|triplet(e).is_some()) {return Ok(None);}
    let dims=dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()?;
    let (shape,view)=section_view(name,args,&dims)?;
    let count=total(&shape)?;
    if !(0..=1024).contains(&count) {bail!("function section specialization supports at most 1024 elements");}
    let mut items=vec![];
    for i in 1..=count {
        let (Expr::Index(_,offset),_)=view_element(&view,&int(i)).unwrap() else {unreachable!()};
        let mut ordinal=crate::parameters::integer_constant(&offset[0])?-1;
        let indices=dims.iter().map(|extent| {let index=ordinal%extent+1;ordinal/=extent;int(index)}).collect();
        items.push(Expr::Index(name.clone(),indices));
    }
    Ok(Some(items))
}

pub(crate) fn fixed_actual_shape(e:&Expr,symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>)->Result<Option<Vec<i64>>> {
    let shapes=symbols.iter().filter_map(|(name,(_,dims))| {
        if dims.is_empty() {return None;}
        let dims=dims.iter().map(|d|crate::parameters::integer_constant(d).map(|n|n.max(0))).collect::<Result<Vec<_>>>().ok()?;
        Some((name.clone(),dims))
    }).collect();
    Lowerer::default().shape(e,&shapes)
}

pub fn resolve(mut p: Program) -> Result<Program> {
    crate::elemental::validate_program(&p)?;
    let mut lower = Lowerer::default();
    lower.elementals = crate::elemental::functions(&p).into_keys().collect();
    fn collect_pure(body: &[Stmt], out: &mut HashMap<String, bool>) {
        for s in body {
            match s {
                Stmt::Function {
                    name, pure, body, ..
                } => {
                    out.insert(name.clone(), *pure);
                    collect_pure(body, out);
                }
                Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => collect_pure(body, out),
                _ => {}
            }
        }
    }
    collect_pure(&p.body, &mut lower.pure_functions);
    for s in &p.body {
        if let Stmt::Module { body, .. } = s {
            for s in body {
                if let Stmt::ArrayDecl { name, dims, .. } = s {
                    if dims.len() > 1 {
                        lower.module_arrays.insert(name.clone());
                    }
                }
            }
        }
    }
    fn functions(body: &[Stmt], out: &mut HashMap<String, TypeSpec>) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    if let Some(kind) = return_type {
                        out.insert(name.clone(), kind.clone());
                    } else {
                        let result = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                            _ => name,
                        };
                        for s in body {
                            if let Stmt::VarDecl { names, kind } = s {
                                if names.contains(result) {
                                    out.insert(name.clone(), kind.clone());
                                }
                            }
                        }
                    }
                    functions(body, out);
                }
                Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => functions(body, out),
                _ => {}
            }
        }
    }
    functions(&p.body, &mut lower.functions);
    lower.body(&mut p.body, &Shapes::new())?;
    p.body.extend(lower.guards);
    Ok(p)
}

// CHARACTER arrays become individual scalar slots. Preserve and validate their
// original shapes, and flatten multidimensional indices before that storage pass.
pub(crate) fn prepare_character_storage(p:Program)->Result<Program> {
    fn needed(body:&[Stmt])->bool {
        body.iter().any(|s|match s {
            Stmt::ArrayDecl{kind:TypeSpec::Character(_),dims,..}=>dims.len()>1,
            Stmt::Block{body}|Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Function{body,..}|Stmt::Subroutine{body,..}|Stmt::Module{body,..}=>needed(body),
            Stmt::If{then_body,else_body,..}=>needed(then_body)||else_body.as_ref().is_some_and(|b|needed(b)),
            Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|needed(&c.body))||default.as_ref().is_some_and(|b|needed(b)),_=>false,
        })
    }
    if needed(&p.body) {resolve(p)} else {Ok(p)}
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> Result<Program> {
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "matrix.f90")
            .unwrap();
        crate::array_calls::resolve(crate::parameters::resolve(crate::derived::resolve(p)?)?)
    }
    use super::*;
    #[test]
    fn rejects_invalid_matrix_shapes_and_subscripts() {
        for (source, expected) in [
            ("integer,allocatable::a(:,:)\na=1\nallocate(a(2,2))\nend", "used before ALLOCATE"),
            ("integer::a(2,3)\ninteger(16)::i\ni=1\na=0\nprint *,a(i,1)\nend", "scalar INTEGER"),
            ("integer::a(2,3)\na=0\nprint *,a(f(),1)\ncontains\ninteger function f()\nf=1\nend function f\nend", "procedure calls"),
            (
                "integer,allocatable::a(:,:)\nallocate(a(2))\nend",
                "rank mismatch",
            ),
            (
                "integer,allocatable::a(2,:)\nallocate(a(2,2))\nend",
                "all dimensions",
            ),
            (
                "integer::a(2,3),b(3,2)\na=0\nb=a\nend",
                "nonconforming array assignment",
            ),
            ("integer::a(2,3)\na=0\nprint *,a(3,1)\nend", "dimension 1"),
            ("integer::a(2,3)\na=0\nprint *,a(0,2)\nend", "dimension 1"),
            ("integer::a(2,3)\na=0\nprint *,a(1,4)\nend", "dimension 2"),
            (
                "integer::a(2,3)\na=0\nprint *,a(1)\nend",
                "requires 2 subscripts",
            ),
            (
                "integer::a(2,3)\nreal::r\na=0\nr=1.5\nprint *,a(r,1)\nend",
                "scalar INTEGER",
            ),
            ("integer::a(2,3)\nprint *,size(a,3)\nend", "SIZE dimension"),
            (
                "integer::a(2,3)\na=0\nprint *,a(:)\nend",
                "multidimensional sections",
            ),
        ] {
            let err = lower(source).unwrap_err().to_string();
            assert!(err.contains(expected), "{source}: {err}");
        }
    }
    #[test]
    fn rejects_invalid_multidimensional_sections() {
        for (statement, expected) in [
            ("print *,a(0:2,:)", "dimension 1"),
            ("print *,a(:,1:4)", "dimension 2"),
            ("print *,a(::0,:)", "stride must not be zero"),
            ("print *,a(:,4)", "dimension 2"),
            ("print *,a(:,1.5)", "INTEGER"),
            ("print *,a(:,:,1)", "requires 2 subscripts"),
            ("b=a(:,1:2)", "nonconforming array assignment"),
            ("b=a(1:1,:)+a(:,1:1)", "nonconforming array shapes"),
            ("a(:,1)=b", "nonconforming section assignment"),
        ] {
            let source = format!("integer::a(2,3),b(1,3),i\ni=1\na=0\n{statement}\nend");
            let error = format!("{:#}", lower(&source).unwrap_err());
            assert!(error.contains(expected), "{statement}: {error}");
        }
    }

    #[test]
    fn lowers_multidimensional_section_expressions() {
        let p = lower(include_str!("../tests/69_matrix_sections.f90")).unwrap();
        let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
        assert!(!format!("{p:?}").contains("@matrix_view"));
        let source = "real::a(2,3)\ncall random_number(a(:,1:2))\nend";
        let p = lower(source).unwrap();
        assert!(crate::sections::resolve(crate::reductions::resolve(p).unwrap()).is_ok());
    }

    #[test]
    fn section_destinations_preserve_initialization_and_shape_checks() {
        for (assignments, output, valid) in [
            ("a(:,1)=1", "a(:,1)", true),
            ("a(:,1)=1", "a(:,2)", false),
            ("a(:,1)=1\na(:,2)=a(:,1)", "a", true),
            ("a(:,1)=a(:,2)", "a(:,1)", false),
            ("a(2:1,:)=1", "a", false),
        ] {
            let source = format!("integer::a(2,2)\n{assignments}\nprint *,{output}\nend");
            let p = lower(&source).unwrap();
            let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
            assert_eq!(crate::initialized::instrument(p).is_ok(), valid, "{source}");
        }
        let p = lower("logical::a(2)\na(:)=.true.\nend").unwrap();
        let p = crate::reductions::resolve(p).unwrap();
        assert!(crate::initialized::instrument(p).is_ok());
        for (statement, expected) in [
            ("a(:,1:2)=b", "nonconforming section assignment"),
            ("v(:)=b", "nonconforming section assignment"),
            ("a(0:2,:)=1", "dimension 1"),
            ("a(:,::0)=1", "stride must not be zero"),
        ] {
            let source = format!("integer::a(2,3),b(1,4),v(4),i\n{statement}\nend");
            let error = format!("{:#}", lower(&source).unwrap_err());
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn forall_rejects_invalid_iterations_and_destinations() {
        for (statement, expected) in [
            ("forall(i=1:3:0) a(i)=i", "stride must not be zero"),
            ("forall(i=1:3) a(1)=i", "same destination element"),
            ("forall(i=1:3) a(i:i+1)=i", "same destination element"),
            ("forall(i=1:6) a(i)=i", "outside 1:5"),
            ("forall(i=1:r) a(i)=i", "scalar INTEGER"),
            ("forall(i=1:3) a(n)=i", "destination subscripts"),
            ("forall(i=1:3) a(i)=a", "nonconforming FORALL"),
            ("forall(i=1:3) a(i)=unknown(i)", "impure or unsupported"),
            ("forall(i=1:3) i=1", "requires one array-element"),
            ("forall(r=1:3) a(1)=1", "scalar INTEGER"),
        ] {
            let source = format!("integer::i,n,a(5)\nreal::r\n{statement}\nend");
            let error = format!("{:#}", lower(&source).unwrap_err());
            assert!(error.contains(expected), "{source}: {error}");
        }
        let source="integer::i,a(3)\nforall(i=1:3) a(i)=abs(i)\ncontains\ninteger function abs(x)\ninteger::x\nabs=x\nend function abs\nend";
        assert!(lower(source)
            .unwrap_err()
            .to_string()
            .contains("impure or unsupported"));
    }

    #[test]
    fn multi_index_forall_checks_destinations_and_controls() {
        for (statement, expected) in [
            ("forall(i=1:2,i=1:2) a(i)=i", "duplicate FORALL index"),
            ("forall(i=1:2,j=1:2) a(i)=j", "same destination element"),
            ("forall(i=1:2,j=1:i) a(i+j)=j", "must not reference a FORALL index"),
            ("forall(i=2:1,j=1:2:0) a(i+j)=j", "stride must not be zero"),
            ("forall(i=1:2,j=1:2) a(2*i+j)=j", "outside 1:5"),
            ("forall(i=1:4294967296,j=1:4294967296) a(1)=1", "iteration count is too large"),
        ] {
            let source = format!("integer::i,j,n,a(5)\n{statement}\nend");
            let error = format!("{:#}", lower(&source).unwrap_err());
            assert!(error.contains(expected), "{source}: {error}");
        }
        for output in ["i", "j"] {
            let source = format!("integer::i,j,a(2,2)\nforall(i=1:2,j=1:2) a(i,j)=i+j\nprint *,{output}\nend");
            let p = lower(&source).unwrap();
            let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
            assert!(crate::initialized::instrument(p).is_err());
        }
    }

    #[test]
    fn forall_preserves_definite_initialization() {
        for source in [
            "integer::i\nlogical::a(3)\nforall(i=1:3) a(i)=.true.\nprint *,a\nend",
            "integer::i,a(3)\nforall(i=1:1) a(:)=[1,2,3]\nprint *,a\nend",
        ] {
            let p=lower(source).unwrap();
            let p=crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
            assert!(crate::initialized::instrument(p).is_ok(),"{source}");
        }
        for (statement, output, valid) in [
            ("forall(i=1:3) a(i)=i", "a(1:3)", true),
            ("forall(i=1:3) a(i)=i", "a", false),
            ("forall(i=1:3) a(i)=a(4)", "a(1)", false),
            ("forall(i=2:1) a(i)=i", "a(1)", false),
            ("forall(i=1:5) a(i)=i", "i", false),
        ] {
            let p = lower(&format!(
                "integer::i,a(5)\n{statement}\nprint *,{output}\nend"
            ))
            .unwrap();
            let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
            assert_eq!(
                crate::initialized::instrument(p).is_ok(),
                valid,
                "{statement}; {output}"
            );
        }
        let p = lower(include_str!("../tests/72_forall.f90")).unwrap();
        crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
    }

    #[test]
    fn matrices_preserve_column_major_storage_and_initialization_checks() {
        let p = lower(include_str!("../tests/66_allocatable_matrices.f90")).unwrap();
        assert!(p.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name=="a"&&intent=="@heap")));
        assert!(p.body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@heap_allocate")));
        let empty=lower("integer,allocatable::a(:,:)\nallocate(a(2,0))\nend").unwrap();
        assert!(empty.body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@heap_allocate")));
        let p = lower("integer::a(2,3)\na(1,1)=1\nprint *,a(2,1)\nend").unwrap();
        let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
        let err = crate::initialized::instrument(p).unwrap_err().to_string();
        assert!(err.contains("uninitialized variable `a`"), "{err}");
    }
}

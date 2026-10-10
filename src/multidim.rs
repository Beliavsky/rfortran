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
fn total(dims: &[i64]) -> Result<i64> {
    dims.iter().try_fold(1i64, |n, d| {
        n.checked_mul(*d)
            .context("array element count overflows INTEGER(8)")
    })
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
            let first = lower.as_deref().map(&constant).transpose()?.unwrap_or(1);
            let last = upper
                .as_deref()
                .map(&constant)
                .transpose()?
                .unwrap_or(*extent);
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
                    "size" | "len" | "count" => true,
                    "int" | "nint" | "floor" | "ceiling" => args
                        .first()
                        .is_some_and(|e| self.shape(e, shapes).is_ok_and(|s| s.is_none())),
                    "abs" | "mod" | "modulo" | "min" | "max" => {
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
                    (!shapes.contains_key(n) && functions.contains_key(n))
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
        if matches!(e,Expr::Call(n,_) if self.elementals.contains(n)) {
            self.shape(e, shapes)?;
        }
        match e {
            Expr::Ident(n) => self.check_association(n)?,
            Expr::Call(n, args) if n == "size" => {
                let Some(Expr::Ident(array)) = args.first() else {
                    bail!("SIZE currently requires a whole array");
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
                    if a != b {
                        bail!("nonconforming array shapes {a:?} and {b:?}");
                    }
                }
                Ok(a.or(b))
            }
            Expr::Un(_, a) => self.shape(a, shapes),
            Expr::Call(n, args)
                if self.elementals.contains(n)
                    || crate::intrinsics::math_arity(n).is_some()
                    || matches!(n.as_str(), "abs" | "dble" | "real") =>
            {
                let mut shape = None;
                for arg in args {
                    let next = self.shape(arg, shapes)?;
                    if let (Some(a), Some(b)) = (&shape, &next) {
                        if a != b {
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

    fn expand_forall(&mut self, body: &mut Vec<Stmt>, shapes: &Shapes) -> Result<()> {
        let Some(Stmt::CallSub { name: tag, args }) = body.first() else {
            return Ok(());
        };
        if tag != "@forall" {
            return Ok(());
        }
        let [Expr::Ident(index), start, end, step] = args.as_slice() else {
            bail!("FORALL index must be an integer variable");
        };
        if shapes.contains_key(index)
            || !matches!(
                self.types.get(index),
                Some(TypeSpec::Integer(None | Some(4 | 8)))
            )
        {
            bail!("FORALL index `{index}` currently requires an explicitly declared scalar INTEGER of kind 4 or 8");
        }
        let (index, mut start, mut end, mut step) =
            (index.clone(), start.clone(), end.clone(), step.clone());
        for bound in [&mut start, &mut end, &mut step] {
            self.expression(bound, shapes)?;
        }
        let constant = |e: &Expr| {
            crate::parameters::integer_constant(e)
                .context("FORALL bounds and stride must currently be constant integers")
        };
        let (start, end, step) = (constant(&start)?, constant(&end)?, constant(&step)?);
        if step == 0 {
            bail!("FORALL stride must not be zero");
        }
        let [_, Stmt::AssignIndex {
            name,
            indices,
            value,
        }] = body.as_slice()
        else {
            bail!("FORALL currently requires one array-element or array-section assignment");
        };
        self.check_association(name)?;
        let dims = shapes.get(name).with_context(|| {
            format!("FORALL destination `{name}` requires a statically shaped array")
        })?;
        let kind = self
            .types
            .get(name)
            .context("FORALL destination type is unknown")?
            .clone();
        if !matches!(
            kind,
            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                | TypeSpec::Real
                | TypeSpec::DoublePrecision
        ) {
            bail!("FORALL currently requires a supported numeric destination array");
        }
        fn pure(e: &Expr, shapes: &Shapes, known: &HashMap<String, bool>) -> Result<()> {
            match e {
                Expr::Call(n, args) | Expr::Index(n, args) => {
                    if n == "@array_constructor" {
                        bail!("array constructors in FORALL assignments are not yet supported");
                    }
                    let intrinsic = crate::intrinsics::math_arity(n).is_some()
                        || matches!(
                            n.as_str(),
                            "@section_subscript"
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
                                | "sum"
                                | "product"
                                | "minval"
                                | "maxval"
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
        pure(value, shapes, &self.pure_functions)?;
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
        let mut captures = vec![Stmt::DummyIntent {
            name: index.clone(),
            intent: "@forall_index".into(),
        }];
        let mut writes = vec![];
        let mut destinations = std::collections::HashSet::new();
        let (mut current, end, step) = (i128::from(start), i128::from(end), i128::from(step));
        while if step > 0 {
            current <= end
        } else {
            current >= end
        } {
            let mut indices = indices.clone();
            let mut value = value.clone();
            for e in &mut indices {
                replace(e, &index, current as i64);
            }
            replace(&mut value, &index, current as i64);
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
            if let Some(rhs_shape) = self.shape(&value, shapes)? {
                if rhs_shape != shape {
                    bail!("nonconforming FORALL assignment shapes {shape:?} and {rhs_shape:?}");
                }
            }
            if !shape.is_empty() && total(&shape)? == 0 {
                current += step;
                continue;
            }
            self.forall_serial += 1;
            let temp = format!("@forall_value_{}", self.forall_serial);
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
            current += step;
        }
        captures.extend(writes);
        *body = captures;
        Ok(())
    }

    fn body(&mut self, body: &mut [Stmt], inherited: &Shapes) -> Result<()> {
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
                        ) {
                            bail!("multidimensional array `{name}` requires a supported numeric element type");
                        }
                        let extents = extents.with_context(|| {
                            format!(
                                "multidimensional array `{name}` requires a statically known shape"
                            )
                        })?;
                        if extents.iter().any(|d| *d < 1) {
                            bail!("multidimensional array `{name}` currently requires positive extents");
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
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    if let Some(shape) = shapes.get(name).filter(|d| d.len() > 1) {
                        *dims = vec![int(total(shape)?)];
                    }
                }
                Stmt::Assign { name, value } => {
                    self.check_association(name)?;
                    if let (Some(a), Some(b)) = (shapes.get(name), self.shape(value, &shapes)?) {
                        if *a != b {
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
                        let rhs_shape = if let Expr::Call(tag, items) = value {
                            if tag == "@array_constructor" {
                                Some(vec![items.len() as i64])
                            } else {
                                self.shape(value, &shapes)?
                            }
                        } else {
                            self.shape(value, &shapes)?
                        };
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
                    self.expand_forall(body, &shapes)?;
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
    }
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
            ("module m\ninteger::a(2,3)\nend module m\nuse m, only:a\nprint *,a\nend", "USE-associated multidimensional array"),
            ("integer,allocatable::a(:,:)\na=1\nallocate(a(2,2))\nend", "used before ALLOCATE"),
            ("integer::a(2,3)\ninteger(16)::i\ni=1\na=0\nprint *,a(i,1)\nend", "scalar INTEGER"),
            ("integer::a(2,3)\na=0\nprint *,a(f(),1)\ncontains\ninteger function f()\nf=1\nend function f\nend", "procedure calls"),
            (
                "integer,allocatable::a(:,:)\nallocate(a(2))\nend",
                "rank mismatch",
            ),
            (
                "integer,allocatable::a(:,:)\nallocate(a(2,0))\nend",
                "positive extent",
            ),
            (
                "integer,allocatable::a(:,:)\ninteger::n\nread(*,*) n\nallocate(a(n,2))\nend",
                "shape known",
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
            ("print *,a(:,1.5)", "constant integers"),
            ("print *,a(:,a(:,1))", "constant integers"),
            ("print *,a(:,i)", "constant integers"),
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
        let source = "integer::a(2,3)\na=1\ncall random_number(a(:,1:2))\nend";
        let p = lower(source).unwrap();
        assert!(crate::sections::resolve(crate::reductions::resolve(p).unwrap()).is_err());
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
        assert!(crate::reductions::resolve(p)
            .unwrap_err()
            .to_string()
            .contains("numeric arrays"));
        for (statement, expected) in [
            ("a(:,1:2)=b", "nonconforming section assignment"),
            ("v(:)=b", "nonconforming section assignment"),
            ("a(0:2,:)=1", "dimension 1"),
            ("a(:,::0)=1", "stride must not be zero"),
            ("a(:,i)=1", "constant integers"),
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
            ("forall(i=1:n) a(i)=i", "constant integers"),
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
    fn forall_preserves_definite_initialization() {
        assert!(
            lower("integer::i\nlogical::a(3)\nforall(i=1:3) a(i)=.true.\nend")
                .unwrap_err()
                .to_string()
                .contains("numeric destination")
        );
        assert!(lower("integer::i,a(3)\nforall(i=1:1) a(:)=[1,2,3]\nend")
            .unwrap_err()
            .to_string()
            .contains("array constructors"));
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
        assert!(p.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,dims,..} if name=="a" && matches!(dims.as_slice(),[Expr::IntLit(n)] if n=="6"))));
        let p = lower("integer::a(2,3)\na(1,1)=1\nprint *,a(2,1)\nend").unwrap();
        let p = crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
        let err = crate::initialized::instrument(p).unwrap_err().to_string();
        assert!(err.contains("uninitialized variable `a`"), "{err}");
    }
}

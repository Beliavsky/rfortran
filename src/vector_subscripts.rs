// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Snapshot vector selections in array element order before bounds normalization.
#[path = "vector_runtime.rs"]
mod runtime;

use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Variable {
    kind: TypeSpec,
    dims: Vec<Expr>,
}
#[derive(Clone)]
struct Info {
    kind: TypeSpec,
    shape: Vec<i64>,
}
#[derive(Clone, Default)]
struct Scope {
    variables: HashMap<String, Variable>,
    // Signatures are immutable while lowering a program. Sharing them avoids
    // cloning all module interfaces for every generated block and branch.
    functions: std::sync::Arc<HashMap<String, TypeSpec>>,
    procedures: std::sync::Arc<HashMap<String, Vec<String>>>,
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn axis(e: &Expr) -> Result<(i64, i64)> {
    let (lower, upper) = match e {
        Expr::Call(n, args) if n == "@declared_bounds" => (
            crate::parameters::integer_constant(&args[0])?,
            crate::parameters::integer_constant(&args[1])?,
        ),
        _ => (
            1,
            crate::parameters::integer_constant(e)
                .context("vector selections require statically known array extents")?,
        ),
    };
    let count = i64::try_from((i128::from(upper) - i128::from(lower) + 1).max(0))?;
    Ok((lower, count))
}
pub(crate) fn fixed_extent(dimension: &Expr) -> Option<i64> {
    axis(dimension).ok().map(|(_, n)| n)
}
fn section(e: &Expr, scope: &Scope) -> Result<(i64, i64, i64)> {
    let Expr::Section {
        name,
        lower,
        upper,
        stride,
    } = e
    else {
        unreachable!()
    };
    let variable = scope
        .variables
        .get(name)
        .context("undefined vector section source")?;
    if variable.dims.len() != 1 {
        bail!("vector subscript sections must currently be rank one");
    }
    let (base, extent) = axis(&variable.dims[0])?;
    let first = lower
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(base);
    let last = upper
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(i64::try_from(i128::from(base) + i128::from(extent) - 1)?);
    let step = stride
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(1);
    if step == 0 {
        bail!("vector subscript section stride must not be zero");
    }
    let distance = (i128::from(last) - i128::from(first)) * i128::from(step.signum());
    let count = if distance < 0 {
        0
    } else {
        i64::try_from(distance / i128::from(step).abs() + 1)?
    };
    if count > 0 {
        let end = i128::from(first) + (i128::from(count) - 1) * i128::from(step);
        let upper = i128::from(base) + i128::from(extent) - 1;
        if first < base || i128::from(first) > upper || end < i128::from(base) || end > upper {
            bail!("vector subscript section of `{name}` is outside bounds {base}:{upper}");
        }
    }
    Ok((first, count, step))
}
// A triplet belongs to a particular source dimension, not necessarily dimension one.
fn section_axis(e: &Expr, dimension: &Expr) -> Result<(i64, i64, i64)> {
    let Expr::Section {
        lower,
        upper,
        stride,
        ..
    } = e
    else {
        bail!("invalid section subscript");
    };
    let (base, extent) = axis(dimension)?;
    let step = stride
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(1);
    if step == 0 {
        bail!("array section stride must not be zero");
    }
    let last = i64::try_from(i128::from(base) + i128::from(extent) - 1)?;
    let first = lower
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(if step > 0 { base } else { last });
    let end = upper
        .as_deref()
        .map(crate::parameters::integer_constant)
        .transpose()?
        .unwrap_or(if step > 0 { last } else { base });
    let distance = (i128::from(end) - i128::from(first)) * i128::from(step.signum());
    let count = if distance < 0 {
        0
    } else {
        i64::try_from(distance / i128::from(step).abs() + 1)?
    };
    if count > 0 {
        let final_index = i128::from(first) + (i128::from(count) - 1) * i128::from(step);
        if first < base
            || first > last
            || final_index < i128::from(base)
            || final_index > i128::from(last)
        {
            bail!("array section is outside bounds {base}:{last}");
        }
    }
    Ok((first, count, step))
}
#[derive(Clone)]
enum SelectedAxis {
    Scalar(Expr),
    Vector(String),
    Triplet(i64, i64),
}
pub(crate) fn merge(a: TypeSpec, b: TypeSpec) -> TypeSpec {
    if a == TypeSpec::DoublePrecision || b == TypeSpec::DoublePrecision {
        TypeSpec::DoublePrecision
    } else if a == TypeSpec::Real || b == TypeSpec::Real {
        TypeSpec::Real
    } else if let (TypeSpec::Integer(a), TypeSpec::Integer(b)) = (&a, &b) {
        TypeSpec::Integer(Some(a.unwrap_or(4).max(b.unwrap_or(4))))
    } else {
        a
    }
}
fn concatenation_kind(a: &TypeSpec, b: &TypeSpec) -> Result<TypeSpec> {
    fn length(kind: &TypeSpec) -> Result<Expr> {
        Ok(match kind {
            TypeSpec::Character(n) => int(n.unwrap_or(1) as i64),
            TypeSpec::CharacterLength(e) => *e.clone(),
            _ => bail!("CHARACTER concatenation requires CHARACTER operands"),
        })
    }
    let expression = bin(BinOp::Add, length(a)?, length(b)?);
    Ok(
        if let Ok(n) = crate::parameters::integer_constant(&expression) {
            TypeSpec::Character(Some(usize::try_from(n)?))
        } else {
            TypeSpec::CharacterLength(Box::new(expression))
        },
    )
}
fn info(e: &Expr, scope: &Scope) -> Result<Option<Info>> {
    let scalar = |kind| {
        Ok(Some(Info {
            kind,
            shape: vec![],
        }))
    };
    match e {
        Expr::Ident(n) => scope
            .variables
            .get(n)
            .map(|v| {
                Ok(Info {
                    kind: v.kind.clone(),
                    shape: v
                        .dims
                        .iter()
                        .map(|d| Ok(axis(d)?.1))
                        .collect::<Result<_>>()?,
                })
            })
            .transpose(),
        Expr::IntLit(_) => scalar(TypeSpec::Integer(None)),
        Expr::RealLit(s) => scalar(if s.to_ascii_lowercase().contains('d') {
            TypeSpec::DoublePrecision
        } else {
            TypeSpec::Real
        }),
        Expr::Logical(_) => scalar(TypeSpec::Logical),
        Expr::Str(s) => scalar(TypeSpec::Character(Some(s.len()))),
        Expr::Section { name, .. } => Ok(Some(Info {
            kind: scope
                .variables
                .get(name)
                .context("undefined vector section source")?
                .kind
                .clone(),
            shape: vec![section(e, scope)?.1],
        })),
        Expr::Un(UnOp::Not, a) => Ok(info(a, scope)?.map(|v| Info {
            kind: TypeSpec::Logical,
            ..v
        })),
        Expr::Un(_, a) => info(a, scope),
        Expr::Bin(op, a, b) => {
            let (Some(a), Some(b)) = (info(a, scope)?, info(b, scope)?) else {
                return Ok(None);
            };
            if !a.shape.is_empty() && !b.shape.is_empty() && a.shape != b.shape {
                bail!("vector subscript expression shapes do not conform");
            }
            let shape = if a.shape.is_empty() { b.shape } else { a.shape };
            let kind = if matches!(op, BinOp::Concat) {
                concatenation_kind(&a.kind, &b.kind)?
            } else if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
            ) {
                merge(a.kind, b.kind)
            } else {
                TypeSpec::Logical
            };
            Ok(Some(Info { kind, shape }))
        }
        Expr::Call(n, args) if n == "@typed_constructor" && args.len() == 1 => Ok(Some(Info {
            kind: crate::empty_constructors::kind(e)?.context("invalid typed constructor")?,
            shape: vec![0],
        })),
        Expr::Call(n, args) if n == "@vector_subscript" => info(&args[0], scope),
        Expr::Call(n, args) if n == "@integer_literal" => scalar(TypeSpec::Integer(Some(
            crate::parameters::integer_constant(&args[1])? as u8,
        ))),
        Expr::Call(n, args) if n == "@array_constructor" => {
            let mut count = 0i64;
            let mut kind = TypeSpec::Integer(None);
            for (i, arg) in args.iter().enumerate() {
                let Some(value) = info(arg, scope)? else {
                    return Ok(None);
                };
                if value.shape.len() > 1 {
                    bail!("vector constructors require scalar or rank-one items");
                }
                if i == 0 {
                    kind = value.kind;
                } else {
                    let same = match (&kind, &value.kind) {
                        (TypeSpec::Integer(a), TypeSpec::Integer(b)) => {
                            a.unwrap_or(4) == b.unwrap_or(4)
                        }
                        _ => kind == value.kind,
                    };
                    if !same {
                        bail!("vector constructor items must have the same type and kind");
                    }
                }
                count = count
                    .checked_add(value.shape.first().copied().unwrap_or(1))
                    .context("vector constructor extent overflow")?;
            }
            Ok(Some(Info {
                kind,
                shape: vec![count],
            }))
        }
        Expr::Call(n, args) | Expr::Index(n, args)
            if scope.variables.get(n).is_some_and(|v| !v.dims.is_empty()) =>
        {
            let v = &scope.variables[n];
            let mut shape = vec![];
            for (dimension, arg) in args.iter().enumerate() {
                if let Expr::Call(tag, items) = arg {
                    if tag == "@section_subscript" {
                        shape.push(section_axis(&items[0], &v.dims[dimension])?.1);
                        continue;
                    }
                }
                if let Some(index) = info(arg, scope)? {
                    if !index.shape.is_empty()
                        && (index.shape.len() != 1
                            || !matches!(index.kind, TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))))
                    {
                        bail!("vector subscript must be a rank-one INTEGER array");
                    }
                    shape.extend(index.shape);
                }
            }
            Ok(Some(Info {
                kind: v.kind.clone(),
                shape,
            }))
        }
        Expr::Call(n, _) if scope.functions.contains_key(n) => scalar(scope.functions[n].clone()),
        Expr::Call(n, args)
            if matches!(
                n.as_str(),
                "abs" | "mod" | "modulo" | "sign" | "dble" | "real" | "int"
            ) =>
        {
            let Some(mut result) = info(&args[0], scope)? else {
                return Ok(None);
            };
            for arg in &args[1..] {
                let Some(next) = info(arg, scope)? else {
                    return Ok(None);
                };
                if !result.shape.is_empty() && !next.shape.is_empty() && result.shape != next.shape
                {
                    bail!("vector subscript expression shapes do not conform");
                }
                if result.shape.is_empty() {
                    result.shape = next.shape;
                }
                result.kind = merge(result.kind, next.kind);
            }
            result.kind = match n.as_str() {
                "dble" => TypeSpec::DoublePrecision,
                "real" => TypeSpec::Real,
                "int" => TypeSpec::Integer(None),
                _ => result.kind,
            };
            Ok(Some(result))
        }
        _ => Ok(None),
    }
}
fn vector(e: &Expr, scope: &Scope) -> Result<bool> {
    Ok(info(e, scope)?.is_some_and(|i| !i.shape.is_empty()))
}
fn selection(name: &str, args: &[Expr], scope: &Scope) -> Result<bool> {
    if !scope
        .variables
        .get(name)
        .is_some_and(|v| !v.dims.is_empty())
    {
        return Ok(false);
    }
    for arg in args {
        if matches!(arg,Expr::Call(n,_) if n == "@section_subscript") {
            continue;
        }
        if vector(arg, scope)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn contains(e: &Expr, scope: &Scope) -> Result<bool> {
    match e {
        Expr::Call(n, args) | Expr::Index(n, args) => {
            if selection(n, args, scope)? {
                return Ok(true);
            }
            for arg in args {
                if contains(arg, scope)? {
                    return Ok(true);
                }
            }
        }
        Expr::Bin(_, a, b) => return Ok(contains(a, scope)? || contains(b, scope)?),
        Expr::Un(_, a) => return contains(a, scope),
        _ => {}
    }
    Ok(false)
}
fn item(e: &Expr, ordinal: i64, scope: &Scope) -> Result<Expr> {
    if !vector(e, scope)? {
        return Ok(e.clone());
    }
    match e {
        Expr::Ident(n) => {
            let v = &scope.variables[n];
            if v.dims.len() != 1 {
                bail!("vector subscript must be a rank-one INTEGER array");
            }
            Ok(Expr::Index(
                n.clone(),
                vec![int(i64::try_from(
                    i128::from(axis(&v.dims[0])?.0) + i128::from(ordinal) - 1,
                )?)],
            ))
        }
        Expr::Section { name, .. } => {
            let (first, _, step) = section(e, scope)?;
            Ok(Expr::Index(
                name.clone(),
                vec![int(i64::try_from(
                    i128::from(first) + (i128::from(ordinal) - 1) * i128::from(step),
                )?)],
            ))
        }
        Expr::Call(n, args) if n == "@array_constructor" => {
            let mut offset = ordinal;
            for arg in args {
                let count = info(arg, scope)?
                    .context("unknown vector constructor item type")?
                    .shape
                    .first()
                    .copied()
                    .unwrap_or(1);
                if offset <= count {
                    return item(arg, offset, scope);
                }
                offset -= count;
            }
            bail!("vector constructor element is outside its shape")
        }
        Expr::Call(n, args) | Expr::Index(n, args)
            if scope.variables.get(n).is_some_and(|v| !v.dims.is_empty()) =>
        {
            let source = &scope.variables[n];
            let mut selected = vec![];
            for (dimension, arg) in args.iter().enumerate() {
                if let Expr::Call(tag, items) = arg {
                    if tag == "@section_subscript" {
                        let (first, _, step) = section_axis(&items[0], &source.dims[dimension])?;
                        selected.push(int(i64::try_from(
                            i128::from(first) + (i128::from(ordinal) - 1) * i128::from(step),
                        )?));
                        continue;
                    }
                }
                selected.push(arg.clone());
            }
            Ok(Expr::Index(n.clone(), selected))
        }
        Expr::Bin(op, a, b) => Ok(bin(
            op.clone(),
            item(a, ordinal, scope)?,
            item(b, ordinal, scope)?,
        )),
        Expr::Un(op, a) => Ok(Expr::Un(op.clone(), Box::new(item(a, ordinal, scope)?))),
        Expr::Call(n, args) if matches!(n.as_str(), "abs" | "mod" | "modulo" | "sign" | "int") => {
            Ok(Expr::Call(
                n.clone(),
                args.iter()
                    .map(|a| item(a, ordinal, scope))
                    .collect::<Result<_>>()?,
            ))
        }
        _ => bail!("unsupported vector subscript expression"),
    }
}
#[derive(Default)]
struct Lowerer {
    serial: usize,
}
impl Lowerer {
    fn temporary(
        &mut self,
        role: &str,
        kind: TypeSpec,
        shape: &[i64],
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
    ) -> String {
        self.serial += 1;
        let name = format!("@vector_{role}_{}", self.serial);
        let dims = shape.iter().copied().map(int).collect::<Vec<_>>();
        output.push(if shape.is_empty() {
            Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![name.clone()],
            }
        } else {
            Stmt::ArrayDecl {
                kind: kind.clone(),
                name: name.clone(),
                dims: dims.clone(),
            }
        });
        scope
            .variables
            .insert(name.clone(), Variable { kind, dims });
        name
    }
    fn freeze_scalars(
        &mut self,
        e: &mut Expr,
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
    ) -> Result<()> {
        if !vector(e, scope)? {
            if crate::parameters::integer_constant(e).is_err() {
                let kind = info(e, scope)?
                    .context("unknown scalar operand in vector subscript")?
                    .kind;
                let temp = self.temporary("scalar", kind, &[], scope, output);
                output.push(Stmt::Assign {
                    name: temp.clone(),
                    value: e.clone(),
                });
                *e = Expr::Ident(temp);
            }
            return Ok(());
        }
        match e {
            Expr::Bin(_, a, b) => {
                self.freeze_scalars(a, scope, output)?;
                self.freeze_scalars(b, scope, output)?;
            }
            Expr::Un(_, a) => self.freeze_scalars(a, scope, output)?,
            Expr::Call(n, args) if matches!(n.as_str(), "abs" | "mod" | "modulo" | "sign" | "int") => {
                for a in args {
                    self.freeze_scalars(a, scope, output)?;
                }
            }
            Expr::Call(n, args) | Expr::Index(n, args)
                if scope.variables.get(n).is_some_and(|v| !v.dims.is_empty()) =>
            {
                for arg in args {
                    if !matches!(arg,Expr::Call(tag,_) if tag=="@section_subscript") {
                        self.freeze_scalars(arg, scope, output)?;
                    }
                }
            }
            // Constructor scalar items are already evaluated once by index capture.
            _ => {}
        }
        Ok(())
    }
    fn indices(
        &mut self,
        name: &str,
        e: &mut Expr,
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
        writable: bool,
        dimension: usize,
    ) -> Result<(Vec<String>, i64, TypeSpec)> {
        let v = scope
            .variables
            .get(name)
            .context("vector selection requires a declared array")?
            .clone();
        let value = info(e, scope)?.context("unknown vector subscript type")?;
        if value.shape.len() != 1
            || !matches!(value.kind, TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
        {
            bail!("vector subscript must be a rank-one INTEGER array");
        }
        let count = value.shape[0];
        if !(0..=1024).contains(&count) {
            bail!("vector subscripts currently support at most 1024 selected elements");
        }
        self.expression(e, scope, output)?;
        let constants = (1..=count)
            .map(|i| crate::parameters::integer_constant(&item(e, i, scope)?))
            .collect::<Result<Vec<_>>>()
            .ok();
        if writable {
            if let Some(values) = &constants {
                let mut seen = HashSet::new();
                for value in values {
                    if !seen.insert(*value) {
                        bail!("vector-subscript assignment to `{name}` selects element {value} more than once");
                    }
                }
            }
        }
        let (lower, extent) = axis(&v.dims[dimension])?;
        if count > 0 && extent == 0 {
            bail!("nonempty vector selects from empty array `{name}`");
        }
        if let Some(values) = &constants {
            for value in values {
                if i128::from(*value) < i128::from(lower)
                    || i128::from(*value) >= i128::from(lower) + i128::from(extent)
                {
                    bail!(
                        "vector subscript of `{name}` selects index {value} outside {lower}:{}",
                        i128::from(lower) + i128::from(extent) - 1
                    );
                }
            }
        }
        self.freeze_scalars(e, scope, output)?;
        let mut captured = vec![];
        for i in 1..=count {
            let index = self.temporary("index", TypeSpec::Integer(Some(8)), &[], scope, output);
            output.push(Stmt::Assign {
                name: index.clone(),
                value: item(e, i, scope)?,
            });
            captured.push(index);
        }
        if constants.is_none() && count > 0 {
            let upper = i64::try_from(i128::from(lower) + i128::from(extent) - 1)?;
            // Check every original index before any gather or destination write.
            for index in &captured {
                let value = Expr::Ident(index.clone());
                output.push(Stmt::If {
                    cond: bin(
                        BinOp::Or,
                        bin(BinOp::Lt, value.clone(), int(lower)),
                        bin(BinOp::Gt, value, int(upper)),
                    ),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str(format!(
                            "vector subscript of `{name}` is outside bounds {lower}:{upper}"
                        ))],
                    }],
                    else_body: None,
                });
            }
        }
        if writable && constants.is_none() && count > 1 {
            let indices = self.temporary(
                "indices",
                TypeSpec::Integer(Some(8)),
                &[count],
                scope,
                output,
            );
            for (i, value) in captured.iter().enumerate() {
                output.push(Stmt::AssignIndex {
                    name: indices.clone(),
                    indices: vec![int(i as i64 + 1)],
                    value: Expr::Ident(value.clone()),
                });
            }
            let j = self.temporary(
                "duplicate_i",
                TypeSpec::Integer(Some(8)),
                &[],
                scope,
                output,
            );
            let k = self.temporary(
                "duplicate_j",
                TypeSpec::Integer(Some(8)),
                &[],
                scope,
                output,
            );
            let read = |n: &str| Expr::Index(indices.clone(), vec![Expr::Ident(n.into())]);
            output.push(Stmt::Do {var:j.clone(),start:int(1),end:int(count-1),body:vec![Stmt::Do {
                var:k.clone(),start:bin(BinOp::Add,Expr::Ident(j.clone()),int(1)),end:int(count),body:vec![Stmt::If {
                    cond:bin(BinOp::Eq,read(&j),read(&k)),then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("vector-subscript assignment to `{name}` selects an element more than once"))]}],else_body:None,
                }],
            }]});
        }
        Ok((captured, count, v.kind))
    }
    fn selection_indices(
        &mut self,
        name: &str,
        args: &mut [Expr],
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
        writable: bool,
    ) -> Result<(Vec<SelectedAxis>, Vec<i64>)> {
        let variable = scope.variables[name].clone();
        if args.len() != variable.dims.len() {
            bail!("array `{name}` requires {} subscripts", variable.dims.len());
        }
        let mut selected = vec![];
        let mut shape = vec![];
        for (dimension, arg) in args.iter_mut().enumerate() {
            if let Expr::Call(tag, items) = arg {
                if tag == "@vector_subscript" {
                    *arg = items[0].clone();
                }
            }
            if let Expr::Call(tag, items) = arg {
                if tag == "@section_subscript" {
                    let (first, count, step) = section_axis(&items[0], &variable.dims[dimension])?;
                    selected.push(SelectedAxis::Triplet(first, step));
                    shape.push(count);
                    continue;
                }
            }
            if vector(arg, scope)? {
                let (indices, count, _) =
                    self.indices(name, arg, scope, output, writable, dimension)?;
                let captured =
                    self.temporary("axis", TypeSpec::Integer(Some(8)), &[count], scope, output);
                for (i, index) in indices.iter().enumerate() {
                    output.push(Stmt::AssignIndex {
                        name: captured.clone(),
                        indices: vec![int(i as i64 + 1)],
                        value: Expr::Ident(index.clone()),
                    });
                }
                selected.push(SelectedAxis::Vector(captured));
                shape.push(count);
            } else {
                self.expression(arg, scope, output)?;
                let value = info(arg, scope)?.context("unknown array subscript type")?;
                if !value.shape.is_empty() || !matches!(value.kind, TypeSpec::Integer(_)) {
                    bail!("array subscript must be a scalar INTEGER or rank-one INTEGER array");
                }
                let captured = self.temporary(
                    "scalar_index",
                    TypeSpec::Integer(Some(8)),
                    &[],
                    scope,
                    output,
                );
                output.push(Stmt::Assign {
                    name: captured.clone(),
                    value: arg.clone(),
                });
                selected.push(SelectedAxis::Scalar(Expr::Ident(captured)));
            }
        }
        Ok((selected, shape))
    }
    fn coordinates(
        &mut self,
        shape: &[i64],
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
    ) -> (Vec<Expr>, Vec<(String, i64)>) {
        let vars = shape
            .iter()
            .map(|n| {
                (
                    self.temporary("coordinate", TypeSpec::Integer(Some(8)), &[], scope, output),
                    *n,
                )
            })
            .collect::<Vec<_>>();
        (
            vars.iter().map(|(n, _)| Expr::Ident(n.clone())).collect(),
            vars,
        )
    }
    fn nest(&self, loops: Vec<(String, i64)>, mut body: Vec<Stmt>) -> Vec<Stmt> {
        // Fortran array element order: the first retained dimension varies fastest.
        for (var, count) in loops {
            body = vec![Stmt::Do {
                var,
                start: int(1),
                end: int(count),
                body,
            }];
        }
        body
    }
    fn capture_selected(
        &mut self,
        selected: &[SelectedAxis],
        coordinates: &[Expr],
        scope: &mut Scope,
        body: &mut Vec<Stmt>,
    ) -> Vec<Expr> {
        self.selected_element(selected, coordinates)
            .into_iter()
            .map(|value| {
                let n = self.temporary(
                    "selected_index",
                    TypeSpec::Integer(Some(8)),
                    &[],
                    scope,
                    body,
                );
                body.push(Stmt::Assign {
                    name: n.clone(),
                    value,
                });
                Expr::Ident(n)
            })
            .collect()
    }
    fn selected_element(&self, selected: &[SelectedAxis], coordinates: &[Expr]) -> Vec<Expr> {
        let mut coordinate = coordinates.iter();
        selected
            .iter()
            .map(|axis| match axis {
                SelectedAxis::Scalar(e) => e.clone(),
                SelectedAxis::Vector(n) => {
                    Expr::Index(n.clone(), vec![coordinate.next().unwrap().clone()])
                }
                SelectedAxis::Triplet(first, step) => bin(
                    BinOp::Add,
                    int(*first),
                    bin(
                        BinOp::Mul,
                        bin(BinOp::Sub, coordinate.next().unwrap().clone(), int(1)),
                        int(*step),
                    ),
                ),
            })
            .collect()
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
    ) -> Result<()> {
        // Keep target identity and its original section coordinates.
        if matches!(e,Expr::Call(n,_) if n=="associated") {return Ok(());}
        if let Expr::Call(n, args) = e {
            if matches!(
                n.as_str(),
                "size" | "shape" | "lbound" | "ubound" | "kind" | "len"
            ) && !args.is_empty()
                && contains(&args[0], scope)?
            {
                let v = info(&args[0], scope)?.context("unknown vector inquiry shape")?;
                let dimension = args
                    .get(1)
                    .map(crate::parameters::integer_constant)
                    .transpose()?;
                if dimension.is_some_and(|d| d < 1 || d as usize > v.shape.len()) {
                    bail!("vector inquiry DIM must be between 1 and {}", v.shape.len());
                }
                *e = match n.as_str() {
                    "size" => int(dimension
                        .map(|d| v.shape[d as usize - 1])
                        .unwrap_or(v.shape.iter().product())),
                    "shape" => Expr::Call(
                        "@array_constructor".into(),
                        v.shape.iter().map(|d| int(*d)).collect(),
                    ),
                    "len" => match &v.kind {
                        TypeSpec::Character(n) => int(n.unwrap_or(1) as i64),
                        TypeSpec::CharacterLength(e) => *e.clone(),
                        _ => bail!("LEN requires a CHARACTER argument"),
                    },
                    "kind" => int(match v.kind {
                        TypeSpec::Integer(k) => k.unwrap_or(4) as i64,
                        TypeSpec::DoublePrecision => 8,
                        TypeSpec::Character(_)
                        | TypeSpec::CharacterLength(_)
                        | TypeSpec::CharacterAssumed => 1,
                        _ => 4,
                    }),
                    _ => {
                        let values = v
                            .shape
                            .iter()
                            .map(|nsize| int(if n == "lbound" { 1 } else { *nsize }))
                            .collect::<Vec<_>>();
                        if let Some(d) = dimension {
                            values[d as usize - 1].clone()
                        } else {
                            Expr::Call("@array_constructor".into(), values)
                        }
                    }
                };
                return Ok(());
            }
        }
        match e {
            Expr::Call(n, args) | Expr::Index(n, args) if selection(n, args, scope)? => {
                let name = n.clone();
                let (selected, shape) =
                    self.selection_indices(&name, args, scope, output, false)?;
                let kind = scope.variables[&name].kind.clone();
                let result = self.temporary("gather", kind, &shape, scope, output);
                let (coordinates, loops) = self.coordinates(&shape, scope, output);
                let mut body = vec![];
                let source = self.capture_selected(&selected, &coordinates, scope, &mut body);
                body.push(Stmt::AssignIndex {
                    name: result.clone(),
                    indices: coordinates,
                    value: Expr::Index(name, source),
                });
                output.extend(self.nest(loops, body));
                *e = Expr::Ident(result);
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    self.expression(arg, scope, output)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, scope, output)?;
                self.expression(b, scope, output)?;
            }
            Expr::Un(_, a) => self.expression(a, scope, output)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for bound in [lower, upper, stride].into_iter().flatten() {
                    self.expression(bound, scope, output)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn output_expression(
        &mut self,
        e: &mut Expr,
        scope: &mut Scope,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let selected = contains(e, scope)?;
        self.expression(e, scope, prefix)?;
        if selected && !matches!(e, Expr::Ident(_)) {
            if let Some(value) = info(e, scope)?.filter(|v| !v.shape.is_empty()) {
                let result = self.temporary("output", value.kind, &value.shape, scope, prefix);
                prefix.push(Stmt::Assign {
                    name: result.clone(),
                    value: e.clone(),
                });
                *e = Expr::Ident(result);
            }
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Scope, params: &[String]) -> Result<()> {
        let mut scope = inherited.clone();
        for p in params {
            scope.variables.remove(p);
        }
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        scope.variables.insert(
                            n.clone(),
                            Variable {
                                kind: kind.clone(),
                                dims: vec![],
                            },
                        );
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    scope.variables.insert(
                        name.clone(),
                        Variable {
                            kind: kind.clone(),
                            dims: dims.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        if !params.is_empty()
            && scope.variables.iter().any(|(n, v)| {
                params.contains(n)
                    && v.dims
                        .iter()
                        .any(|d| matches!(d,Expr::Ident(tag) if tag=="@deferred_extent"))
            })
        {
            return Ok(());
        }
        if body.iter().any(
            |s| matches!(s, Stmt::CallSub {name,..} if name == "@where" || name == "@elsewhere"),
        ) {
            // Eager gathers would read inactive RHS elements. Keep these forms
            // explicit until vector capture can follow the enclosing mask.
            for stmt in body.iter() {
                match stmt {
                    Stmt::Assign { value, .. } if contains(value, &scope)? => {
                        bail!("vector selections in WHERE assignments are not yet supported")
                    }
                    Stmt::AssignIndex {
                        name,
                        indices,
                        value,
                    } if contains(value, &scope)?
                        || (!matches!(indices.as_slice(), [Expr::Section { .. }])
                            && selection(name, indices, &scope)?) =>
                    {
                        bail!("vector selections in WHERE assignments are not yet supported")
                    }
                    _ => {}
                }
            }
        }
        let mut result = vec![];
        for mut stmt in std::mem::take(body) {
            let mut prefix = vec![];
            match &mut stmt {
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if !matches!(indices.as_slice(), [Expr::Section { .. }])
                    && selection(name, indices, &scope)? =>
                {
                    let (selected, selected_shape) =
                        self.selection_indices(name, indices, &mut scope, &mut prefix, true)?;
                    self.expression(value, &mut scope, &mut prefix)?;
                    let rhs = info(value, &scope)?;
                    if rhs
                        .as_ref()
                        .is_some_and(|v| !v.shape.is_empty() && v.shape != selected_shape)
                    {
                        bail!("vector-subscript assignment shapes do not conform");
                    }
                    let shape = if rhs.is_some_and(|v| !v.shape.is_empty()) {
                        selected_shape.clone()
                    } else {
                        vec![]
                    };
                    let kind = scope.variables[name].kind.clone();
                    let values = self.temporary("values", kind, &shape, &mut scope, &mut prefix);
                    prefix.push(Stmt::Assign {
                        name: values.clone(),
                        value: value.clone(),
                    });
                    let (coordinates, loops) =
                        self.coordinates(&selected_shape, &mut scope, &mut prefix);
                    let mut body = vec![];
                    let destination =
                        self.capture_selected(&selected, &coordinates, &mut scope, &mut body);
                    body.push(Stmt::AssignIndex {
                        name: name.clone(),
                        indices: destination,
                        value: if shape.is_empty() {
                            Expr::Ident(values)
                        } else {
                            Expr::Index(values, coordinates)
                        },
                    });
                    prefix.extend(self.nest(loops, body));
                    result.extend(prefix);
                    continue;
                }
                Stmt::Assign { name, value } => {
                    if contains(value, &scope)?
                        && info(value, &scope)?.is_some_and(|v| !v.shape.is_empty())
                        && scope.variables.get(name).is_some_and(|v| v.dims.is_empty())
                    {
                        bail!(
                            "vector selection is array-valued and cannot be assigned to a scalar"
                        );
                    }
                    self.expression(value, &mut scope, &mut prefix)?;
                }
                Stmt::AssignIndex { indices, value, .. } => {
                    if contains(value, &scope)?
                        && info(value, &scope)?.is_some_and(|v| !v.shape.is_empty())
                        && !indices.iter().any(|e| {
                            matches!(e, Expr::Section { .. })
                                || matches!(e,Expr::Call(n,_) if n=="@section_subscript")
                        })
                    {
                        bail!("vector selection is array-valued and cannot be assigned to a scalar element");
                    }
                    self.expression(value, &mut scope, &mut prefix)?;
                    for e in indices {
                        self.expression(e, &mut scope, &mut prefix)?;
                    }
                }
                Stmt::Print { items } => {
                    for e in items {
                        self.output_expression(e, &mut scope, &mut prefix)?;
                    }
                }
                Stmt::Read { args } => {
                    for e in args {
                        if contains(e, &scope)? {
                            bail!("READ into vector-subscript selections is not yet supported");
                        }
                    }
                }
                Stmt::CallSub { name, .. } if matches!(name.as_str(),"@c_desc_bind"|"@c_array_bind"|"@c_f_pointer") => {},
                Stmt::CallSub { name, args } => {
                    for (i, e) in args.iter().enumerate() {
                        if contains(e, &scope)? {
                            validate_actual(name, i, &scope.procedures)?;
                        }
                    }
                    let read_start=crate::input::transfer_start(name);
                    for (i,e) in args.iter_mut().enumerate() {
                        if read_start.is_some_and(|start|i>=start) && contains(e, &scope)? {
                            bail!("READ into vector-subscript selections is not yet supported");
                        }
                        if name == "@write" {
                            self.output_expression(e, &mut scope, &mut prefix)?;
                        } else {
                            self.expression(e, &mut scope, &mut prefix)?;
                        }
                    }
                }
                Stmt::Return(Some(e)) => self.expression(e, &mut scope, &mut prefix)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &mut scope, &mut prefix)?;
                    self.body(then_body, &scope, &[])?;
                    if let Some(b) = else_body {
                        self.body(b, &scope, &[])?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &mut scope, &mut prefix)?;
                    self.expression(end, &mut scope, &mut prefix)?;
                    self.body(body, &scope, &[])?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &mut scope, &mut prefix)?;
                    self.body(body, &scope, &[])?;
                    if !prefix.is_empty() {
                        let (declarations, mut evaluate): (Vec<_>, Vec<_>) =
                            prefix.into_iter().partition(|s| {
                                matches!(s, Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. })
                            });
                        prefix = declarations;
                        evaluate.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        evaluate.append(body);
                        *body = evaluate;
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    self.body(body, &scope, params)?
                }
                Stmt::Block { body } | Stmt::Module { body, .. } => self.body(body, &scope, &[])?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &mut scope, &mut prefix)?;
                    for c in cases {
                        self.body(&mut c.body, &scope, &[])?;
                    }
                    if let Some(b) = default {
                        self.body(b, &scope, &[])?;
                    }
                }
                _ => {}
            }
            result.extend(prefix);
            result.push(stmt);
        }
        *body = result;
        Ok(())
    }
}
fn serial_start(body: &[Stmt]) -> usize {
    let mut serial = 0;
    for stmt in body {
        let names = match stmt {
            Stmt::ArrayDecl { name, .. } => vec![name],
            Stmt::VarDecl { names, .. } => names.iter().collect(),
            _ => vec![],
        };
        for name in names {
            if name.contains("@vector_") {
                serial = serial.max(
                    name.rsplit('_')
                        .next()
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(0),
                );
            }
        }
        match stmt {
            Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Module { body, .. }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => serial = serial.max(serial_start(body)),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                serial = serial.max(serial_start(then_body));
                if let Some(b) = else_body {
                    serial = serial.max(serial_start(b));
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    serial = serial.max(serial_start(&c.body));
                }
                if let Some(b) = default {
                    serial = serial.max(serial_start(b));
                }
            }
            _ => {}
        }
    }
    serial
}
fn procedure_intents(body: &[Stmt]) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();
    for stmt in body {
        match stmt {
            Stmt::Subroutine {
                name, params, body, ..
            } => {
                result.insert(
                    name.clone(),
                    params
                        .iter()
                        .map(|p|crate::c_interop::association_intent(body,p))
                        .collect(),
                );
                result.extend(procedure_intents(body));
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } => {
                result.extend(procedure_intents(body))
            }
            _ => {}
        }
    }
    result
}
fn validate_actual(
    name: &str,
    index: usize,
    procedures: &HashMap<String, Vec<String>>,
) -> Result<()> {
    if let Some(intents) = procedures.get(name) {
        if intents.get(index).is_none_or(|i| i.strip_prefix("@c_descriptor:").unwrap_or(i) != "intent(in)") {
            bail!("vector-subscript actual argument cannot be associated with writable dummy argument {} of `{name}`",index+1);
        }
    } else if !name.starts_with('@')
        || matches!(
            name,
            "@random_number" | "@random_seed" | "@cpu_time" | "@system_clock" | "@date_and_time"
        )
    {
        bail!("vector-subscript actual argument to `{name}` requires a known INTENT(IN) dummy");
    }
    Ok(())
}
pub fn resolve(program: Program) -> Result<Program> {
    resolve_inner(program, false)
}
pub(crate) fn finish(program: Program) -> Result<Program> {
    resolve_inner(program, true)
}
pub(crate) fn specialize(body:&mut Vec<Stmt>, shapes:&crate::allocatables::Shapes, procedures:std::sync::Arc<HashMap<String,Vec<String>>>, serial:&mut usize)->Result<()> {
    // Fixed selections were handled by the first pass. Revisit a scope when
    // call specialization or runtime descriptors can expose additional sections.
    // Call specialization can reveal runtime selectors on fixed storage too;
    // neither a descriptor nor a remaining subroutine interface is required.
    runtime::specialize(body,shapes,procedures,serial)
}
pub(crate) fn specialize_expression(e:&mut Expr, shapes:&mut crate::allocatables::Shapes, prefix:&mut Vec<Stmt>, serial:&mut usize)->Result<()> {
    runtime::specialize_expression(e,shapes,prefix,serial)
}
pub(crate) fn argument_expression_shape(e:&Expr, shapes:&crate::allocatables::Shapes, functions:&HashMap<String,TypeSpec>)->Result<Option<(TypeSpec,Vec<Expr>)>> {
    runtime::argument_expression_shape(e,shapes,functions)
}

/// Apply a private scalar implementation over descriptor-backed actuals.
pub(crate) fn elemental_function(e:&mut Expr, shapes:&mut crate::allocatables::Shapes,
    prefix:&mut Vec<Stmt>, serial:&mut usize,
    routine:(&[String],&[Stmt],&str,&TypeSpec))->Result<bool> {
    runtime::elemental_function(e,shapes,prefix,serial,routine)
}

fn resolve_inner(mut program: Program, after_calls: bool) -> Result<Program> {
    let procedures = procedure_intents(&program.body);
    program = runtime::resolve(program, &procedures)?;
    fn functions(body: &[Stmt], scope: &mut Scope) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    let kind = return_type.clone().or_else(|| {
                        body.iter().find_map(|s| match s {
                            Stmt::VarDecl { kind, names } if names.contains(name) => {
                                Some(kind.clone())
                            }
                            _ => None,
                        })
                    });
                    if let Some(kind) = kind {
                        std::sync::Arc::make_mut(&mut scope.functions).insert(name.clone(), kind);
                    }
                }
                Stmt::Module { body, .. } => functions(body, scope),
                _ => {}
            }
        }
    }
    let mut scope = Scope::default();
    scope.procedures = std::sync::Arc::new(procedures);
    functions(&program.body, &mut scope);
    Lowerer {
        serial: serial_start(&program.body),
    }
    .body(&mut program.body, &scope, &[])?;
    if after_calls {
        crate::substrings::hoist_vectors(&mut program.body);
    } else {
        crate::substrings::hoist(&mut program.body);
    }
    Ok(program)
}

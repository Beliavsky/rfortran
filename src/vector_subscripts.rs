// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Snapshot fixed rank-one vector selections before bounds normalization.
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
    functions: HashMap<String, TypeSpec>,
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
fn merge(a: TypeSpec, b: TypeSpec) -> TypeSpec {
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
            let kind = if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
            ) {
                merge(a.kind, b.kind)
            } else {
                TypeSpec::Logical
            };
            Ok(Some(Info { kind, shape }))
        }
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
            for arg in args {
                if let Expr::Call(tag, items) = arg {
                    if tag == "@section_subscript" {
                        shape.push(section(&items[0], scope)?.1);
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
                "abs" | "mod" | "modulo" | "dble" | "real" | "int"
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
    // Higher-rank selections retain their existing explicit diagnostics.
    if !scope.variables.get(name).is_some_and(|v| v.dims.len() == 1) {
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
        Expr::Bin(op, a, b) => Ok(bin(
            op.clone(),
            item(a, ordinal, scope)?,
            item(b, ordinal, scope)?,
        )),
        Expr::Un(op, a) => Ok(Expr::Un(op.clone(), Box::new(item(a, ordinal, scope)?))),
        Expr::Call(n, args) if matches!(n.as_str(), "abs" | "mod" | "modulo" | "int") => {
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
            Expr::Call(n, args) if matches!(n.as_str(), "abs" | "mod" | "modulo" | "int") => {
                for a in args {
                    self.freeze_scalars(a, scope, output)?;
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
    ) -> Result<(Vec<String>, i64, TypeSpec)> {
        let v = scope
            .variables
            .get(name)
            .context("vector selection requires a declared array")?
            .clone();
        if v.dims.len() != 1
            || !matches!(
                v.kind,
                TypeSpec::Integer(_)
                    | TypeSpec::Real
                    | TypeSpec::DoublePrecision
                    | TypeSpec::Logical
            )
        {
            bail!("vector subscripts currently require a rank-one numeric or LOGICAL source array");
        }
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
        let (lower, extent) = axis(&v.dims[0])?;
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
    fn expression(
        &mut self,
        e: &mut Expr,
        scope: &mut Scope,
        output: &mut Vec<Stmt>,
    ) -> Result<()> {
        if let Expr::Call(n, args) = e {
            if matches!(n.as_str(), "size" | "lbound" | "ubound" | "kind")
                && !args.is_empty()
                && contains(&args[0], scope)?
            {
                let v = info(&args[0], scope)?.context("unknown vector inquiry shape")?;
                if v.shape.len() != 1 {
                    bail!("vector inquiries currently require a rank-one selection");
                }
                if let Some(dim) = args.get(1) {
                    if crate::parameters::integer_constant(dim)? != 1 {
                        bail!("vector inquiry DIM must be 1");
                    }
                }
                *e = match n.as_str() {
                    "size" => int(v.shape[0]),
                    "kind" => int(match v.kind {
                        TypeSpec::Integer(k) => k.unwrap_or(4) as i64,
                        TypeSpec::DoublePrecision => 8,
                        _ => 4,
                    }),
                    _ => {
                        let value = int(if n == "lbound" { 1 } else { v.shape[0] });
                        if args.len() == 1 {
                            Expr::Call("@array_constructor".into(), vec![value])
                        } else {
                            value
                        }
                    }
                };
                return Ok(());
            }
        }
        match e {
            Expr::Call(n, args) | Expr::Index(n, args) if selection(n, args, scope)? => {
                if args.len() != 1 {
                    bail!("vector subscripts currently require a rank-one source array");
                }
                let name = n.clone();
                let (indices, count, kind) =
                    self.indices(&name, &mut args[0], scope, output, false)?;
                let result = self.temporary("gather", kind, &[count], scope, output);
                for i in 1..=count {
                    output.push(Stmt::AssignIndex {
                        name: result.clone(),
                        indices: vec![int(i)],
                        value: Expr::Index(
                            name.clone(),
                            vec![Expr::Ident(indices[(i - 1) as usize].clone())],
                        ),
                    });
                }
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
            if let Some(value) = info(e, scope)?.filter(|v| v.shape.len() == 1) {
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
                    if indices.len() != 1 {
                        bail!("vector subscripts currently require a rank-one destination array");
                    }
                    let (captured, count, kind) =
                        self.indices(name, &mut indices[0], &mut scope, &mut prefix, true)?;
                    self.expression(value, &mut scope, &mut prefix)?;
                    let rhs = info(value, &scope)?;
                    if rhs
                        .as_ref()
                        .is_some_and(|v| !v.shape.is_empty() && v.shape != vec![count])
                    {
                        bail!("vector-subscript assignment shapes do not conform");
                    }
                    let shape = if rhs.is_some_and(|v| !v.shape.is_empty()) {
                        vec![count]
                    } else {
                        vec![]
                    };
                    let values = self.temporary("values", kind, &shape, &mut scope, &mut prefix);
                    // Existing array assignment lowering captures the complete RHS.
                    prefix.push(Stmt::Assign {
                        name: values.clone(),
                        value: value.clone(),
                    });
                    for i in 1..=count {
                        prefix.push(Stmt::AssignIndex {
                            name: name.clone(),
                            indices: vec![Expr::Ident(captured[(i - 1) as usize].clone())],
                            value: if shape.is_empty() {
                                Expr::Ident(values.clone())
                            } else {
                                Expr::Index(values.clone(), vec![int(i)])
                            },
                        });
                    }
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
                Stmt::CallSub { name, args } => {
                    for e in args {
                        if name.contains("read") && name.starts_with('@') && contains(e, &scope)? {
                            bail!("READ into vector-subscript selections is not yet supported");
                        }
                        if !name.starts_with('@') && contains(e, &scope)? {
                            bail!("vector-subscript subroutine actual arguments are not yet supported");
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
pub fn resolve(mut program: Program) -> Result<Program> {
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
                        scope.functions.insert(name.clone(), kind);
                    }
                }
                Stmt::Module { body, .. } => functions(body, scope),
                _ => {}
            }
        }
    }
    let mut scope = Scope::default();
    functions(&program.body, &mut scope);
    Lowerer::default().body(&mut program.body, &scope, &[])?;
    crate::substrings::hoist(&mut program.body);
    Ok(program)
}

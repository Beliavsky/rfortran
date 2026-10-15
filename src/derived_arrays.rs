// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Arrays of records share their shape across separate primitive component arrays.
//! Array results use a single hidden subroutine call, preserving side effects.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone)]
struct Field {
    name: String,
    kind: TypeSpec,
    default: Option<Expr>,
}
#[derive(Clone)]
struct Object {
    ty: String,
    dims: Vec<Expr>,
}
#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    arrays: HashMap<String, Object>,
    result: Option<(String, Object)>,
}
type Objects = HashMap<String, Object>;
struct Lower {
    types: HashMap<String, Vec<Field>>,
    procedures: HashMap<String, Procedure>,
    serial: usize,
    array_shapes: HashMap<String, Vec<Expr>>,
}
fn storage(object: &str, field: &str) -> String {
    format!("@record${object}${field}")
}
fn deferred(rank: usize) -> Vec<Expr> {
    vec![Expr::Ident("@deferred_extent".into()); rank]
}
impl Lower {
    fn collect(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::DerivedType {
                    name, components, ..
                } => {
                    let mut fields = vec![];
                    for c in components {
                        match c {
                            Stmt::VarDecl { kind, names } => {
                                for name in names {
                                    fields.push(Field {
                                        name: name.clone(),
                                        kind: kind.clone(),
                                        default: None,
                                    });
                                }
                            }
                            Stmt::InitializedDecl { kind, entities, .. } => {
                                for (name, value) in entities {
                                    fields.push(Field {
                                        name: name.clone(),
                                        kind: kind.clone(),
                                        default: value.clone(),
                                    });
                                }
                            }
                            Stmt::SourceLocation(_) => {}
                            _ => {
                                fields.clear();
                                break;
                            }
                        }
                    }
                    self.types.insert(name.clone(), fields);
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let arrays: Objects = body
                        .iter()
                        .flat_map(|s| match s {
                            Stmt::DerivedDecl {
                                type_name,
                                names,
                                dims,
                                ..
                            } if !dims.is_empty() => names
                                .iter()
                                .map(|n| {
                                    (
                                        n.clone(),
                                        Object {
                                            ty: type_name.clone(),
                                            dims: dims.clone(),
                                        },
                                    )
                                })
                                .collect::<Vec<_>>(),
                            _ => vec![],
                        })
                        .collect();
                    let result = if matches!(s, Stmt::Function { .. }) {
                        let n = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                            _ => name,
                        };
                        arrays.get(n).cloned().map(|o| (n.clone(), o))
                    } else {
                        None
                    };
                    if result.is_some() || params.iter().any(|n| arrays.contains_key(n)) {
                        self.procedures.insert(
                            name.clone(),
                            Procedure {
                                signature: crate::named_arguments::Signature::new(params, body),
                                arrays,
                                result,
                            },
                        );
                    }
                    self.collect(body)?;
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => self.collect(body)?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.collect(then_body)?;
                    if let Some(b) = else_body {
                        self.collect(b)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn fields(&self, ty: &str) -> Result<Vec<Field>> {
        let fields = self
            .types
            .get(ty)
            .with_context(|| format!("unknown derived array type `{ty}`"))?;
        if fields.is_empty()
            || fields.iter().any(|f| {
                !matches!(
                    f.kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::IntegerKind(_)
                        | TypeSpec::Real
                        | TypeSpec::RealKind(_)
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                        | TypeSpec::Character(_)
                        | TypeSpec::CharacterLength(_)
                        | TypeSpec::CharacterAssumed
                )
            })
        {
            bail!("derived arrays currently require scalar intrinsic components: `{ty}`");
        }
        Ok(fields.clone())
    }
    fn component(&self, n: &mut String, objects: &Objects) -> Result<()> {
        if let Some((object, field)) = n.split_once('%') {
            if let Some(o) = objects.get(object).filter(|o| !o.dims.is_empty()) {
                if !self.fields(&o.ty)?.iter().any(|f| f.name == field) {
                    bail!("unknown component `{field}` of `{}`", o.ty);
                }
                *n = storage(object, field);
            }
        }
        Ok(())
    }
    fn array_shape(&self, e: &Expr) -> Option<Vec<Expr>> {
        match e {
            Expr::Ident(n) => self.array_shapes.get(n).cloned(),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                let source = self.array_shapes.get(name)?;
                if source.len() != 1 {
                    return None;
                }
                let bounds = match &source[0] {
                    Expr::Call(tag, a) if tag == "@declared_bounds" => (
                        crate::parameters::integer_constant(&a[0]).ok()?,
                        crate::parameters::integer_constant(&a[1]).ok()?,
                    ),
                    d => (1, crate::parameters::integer_constant(d).ok()?),
                };
                let step = stride
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(1);
                if step == 0 {
                    return None;
                }
                let first = lower
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(if step > 0 { bounds.0 } else { bounds.1 });
                let end = upper
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(if step > 0 { bounds.1 } else { bounds.0 });
                Some(vec![Expr::IntLit(
                    (((i128::from(end) - i128::from(first) + i128::from(step)) / i128::from(step))
                        .max(0))
                    .to_string(),
                )])
            }
            Expr::Call(n, a) if n == "@typed_constructor" && a.len() == 1 => {
                Some(vec![Expr::IntLit("0".into())])
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                Some(vec![Expr::IntLit(a.len().to_string())])
            }
            Expr::Call(n, a) if n == "@vector_subscript" => self.array_shape(&a[0]),
            Expr::Bin(_, a, b) => self.array_shape(a).or_else(|| self.array_shape(b)),
            Expr::Un(_, a) => self.array_shape(a),
            _ => None,
        }
    }
    fn capture_indices(&mut self, args: &mut [Expr], prefix: &mut Vec<Stmt>) {
        for arg in args {
            if let Some(dims) = self.array_shape(arg).filter(|d| !d.is_empty()) {
                // Captured vectors have lower bound one; retain known capacity
                // even when the original index vector has explicit bounds.
                let dims = dims
                    .into_iter()
                    .map(|d| {
                        crate::vector_subscripts::fixed_extent(&d)
                            .map_or(d, |n| Expr::IntLit(n.to_string()))
                    })
                    .collect::<Vec<_>>();
                self.serial += 1;
                let n = format!("@record_vector_index_{}", self.serial);
                prefix.push(Stmt::ArrayDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    name: n.clone(),
                    dims: if dims
                        .iter()
                        .all(|e| crate::parameters::integer_constant(e).is_ok())
                    {
                        dims.clone()
                    } else {
                        deferred(1)
                    },
                });
                if dims
                    .iter()
                    .any(|e| crate::parameters::integer_constant(e).is_err())
                {
                    prefix.push(Stmt::DummyIntent {
                        name: n.clone(),
                        intent: "allocatable".into(),
                    });
                    prefix.push(Stmt::CallSub {
                        name: "@allocate".into(),
                        args: vec![Expr::Call(
                            n.clone(),
                            vec![Expr::Call("size".into(), vec![arg.clone()])],
                        )],
                    });
                }
                prefix.push(Stmt::Assign {
                    name: n.clone(),
                    value: match arg {
                        Expr::Call(tag, a) if tag == "@vector_subscript" => a[0].clone(),
                        _ => arg.clone(),
                    },
                });
                self.array_shapes.insert(n.clone(), dims);
                *arg = Expr::Ident(n);
                continue;
            }
            match arg {
                Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } => {
                    for e in [lower, upper, stride].into_iter().flatten() {
                        self.capture_indices(std::slice::from_mut(e.as_mut()), prefix);
                    }
                }
                Expr::Call(tag, a) if tag == "@section_subscript" => {
                    self.capture_indices(a, prefix)
                }
                _ => {
                    if crate::parameters::integer_constant(arg).is_ok()
                        || matches!(arg, Expr::Ident(_))
                    {
                        continue;
                    }
                    self.serial += 1;
                    let n = format!("@record_index_{}", self.serial);
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![n.clone()],
                    });
                    prefix.push(Stmt::Assign {
                        name: n.clone(),
                        value: arg.clone(),
                    });
                    *arg = Expr::Ident(n);
                }
            }
        }
    }
    fn parts(
        &mut self,
        mut e: Expr,
        ty: &str,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<(Vec<Expr>, bool)> {
        self.expression(&mut e, objects, prefix)?;
        let fields = self.fields(ty)?;
        match e {
            Expr::Call(n, args) if n.starts_with("@complex_array$") && crate::complex::type_kind(ty).is_some() => {
                if args.len() != 2 { bail!("invalid internal COMPLEX array components"); }
                Ok((args, true))
            }
            Expr::Ident(n) if objects.contains_key(&n) => {
                let o = &objects[&n];
                if o.ty != ty {
                    bail!(
                        "derived array type mismatch: expected `{ty}`, got `{}`",
                        o.ty
                    );
                }
                Ok((
                    fields
                        .iter()
                        .map(|f| {
                            Expr::Ident(if o.dims.is_empty() {
                                format!("{n}%{}", f.name)
                            } else {
                                storage(&n, &f.name)
                            })
                        })
                        .collect(),
                    !o.dims.is_empty(),
                ))
            }
            Expr::Call(n, mut args) if objects.contains_key(&n) => {
                let o = &objects[&n];
                if o.ty != ty {
                    bail!("derived array type mismatch");
                }
                let array = args.iter().any(|e| {
                    self.array_shape(e).is_some()
                        || matches!(e, Expr::Section { .. })
                        || matches!(e,Expr::Call(tag,_) if tag=="@section_subscript")
                });
                self.capture_indices(&mut args, prefix);
                Ok((
                    fields
                        .iter()
                        .map(|f| Expr::Call(storage(&n, &f.name), args.clone()))
                        .collect(),
                    array,
                ))
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } if objects.contains_key(&name) => {
                if objects[&name].ty != ty {
                    bail!("derived array type mismatch");
                }
                Ok((
                    fields
                        .iter()
                        .map(|f| Expr::Section {
                            name: storage(&name, &f.name),
                            lower: lower.clone(),
                            upper: upper.clone(),
                            stride: stride.clone(),
                        })
                        .collect(),
                    true,
                ))
            }
            Expr::Call(n, mut args) if n == ty => {
                let signature = crate::named_arguments::Signature {
                    params: fields.iter().map(|f| f.name.clone()).collect(),
                    optional: fields
                        .iter()
                        .filter(|f| f.default.is_some())
                        .map(|f| f.name.clone())
                        .collect(),
                };
                signature.reorder(ty, &mut args)?;
                args.resize_with(fields.len(), || {
                    Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
                });
                for (arg, f) in args.iter_mut().zip(&fields) {
                    if crate::named_arguments::omitted(arg) {
                        *arg = f
                            .default
                            .clone()
                            .context("missing derived constructor component")?;
                    }
                }
                Ok((args, false))
            }
            Expr::Call(n, args) if n == "@record_constructor" => {
                let Some(Expr::Ident(name)) = args.first() else {
                    bail!("invalid derived constructor type");
                };
                if name != ty {
                    bail!("derived array constructor type mismatch");
                }
                if args.len() == 1 {
                    return Ok((
                        fields
                            .iter()
                            .map(|f| crate::empty_constructors::parsed(f.kind.clone(), vec![]))
                            .collect(),
                        true,
                    ));
                }
                self.parts(
                    Expr::Call("@array_constructor".into(), args[1..].to_vec()),
                    ty,
                    objects,
                    prefix,
                )
            }
            Expr::Call(n, args) if n == "@array_constructor" => {
                let mut values = vec![vec![]; fields.len()];
                for e in args {
                    let (parts, array) = self.parts(e, ty, objects, prefix)?;
                    if array {
                        bail!("nested derived array constructors are not yet supported");
                    }
                    for (out, e) in values.iter_mut().zip(parts) {
                        out.push(e);
                    }
                }
                Ok((
                    values
                        .into_iter()
                        .map(|v| Expr::Call("@array_constructor".into(), v))
                        .collect(),
                    true,
                ))
            }
            _ => bail!("unsupported derived array value of type `{ty}`"),
        }
    }
    fn arguments(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Option<(String, Object)>> {
        let Some(p) = self.procedures.get(name).cloned() else {
            for e in args {
                self.expression(e, objects, prefix)?;
            }
            return Ok(None);
        };
        p.signature.reorder(name, args)?;
        args.resize_with(p.signature.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        let mut flat = vec![];
        for (mut e, param) in std::mem::take(args).into_iter().zip(&p.signature.params) {
            if let Some(o) = p.arrays.get(param) {
                if crate::named_arguments::omitted(&e) {
                    flat.extend(std::iter::repeat_n(e, self.fields(&o.ty)?.len()));
                    continue;
                }
                let (values, array) = self.parts(e, &o.ty, objects, prefix)?;
                if !array {
                    bail!("derived array dummy `{param}` requires an array actual");
                }
                flat.extend(values);
            } else {
                self.expression(&mut e, objects, prefix)?;
                flat.push(e);
            }
        }
        *args = flat;
        Ok(p.result)
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        match e {
            Expr::Ident(n) => self.component(n, objects)?,
            Expr::Call(n, args) if n.starts_with("@record_component:") => {
                let field = n[18..].to_owned();
                let [value] = args.as_mut_slice() else {
                    bail!("invalid record component selection");
                };
                self.expression(value, objects, prefix)?;
                let (object, indices) = match value {
                    Expr::Call(n, a) | Expr::Index(n, a) => (n, a),
                    _ => bail!("record element component requires an array element"),
                };
                let o = objects
                    .get(object)
                    .context("component selection requires a derived array")?;
                if !self.fields(&o.ty)?.iter().any(|f| f.name == field) {
                    bail!("unknown component `{field}`");
                }
                *e = Expr::Call(storage(object, &field), indices.clone());
            }
            Expr::Call(n, args)
                if matches!(
                    n.as_str(),
                    "size" | "shape" | "lbound" | "ubound" | "allocated" | "present"
                ) =>
            {
                for e in args.iter_mut() {
                    self.expression(e, objects, prefix)?;
                }
                if let Some(Expr::Ident(array)) = args.first_mut() {
                    if let Some(o) = objects.get(array).filter(|o| !o.dims.is_empty()) {
                        *array = storage(array, &self.fields(&o.ty)?[0].name);
                    }
                }
            }
            Expr::Call(n, args) => {
                self.component(n, objects)?;
                if objects.contains_key(n) {
                    for e in args {
                        self.expression(e, objects, prefix)?;
                    }
                    return Ok(());
                }
                if let Some((_, o)) = self.arguments(n, args, objects, prefix)? {
                    self.serial += 1;
                    let temp = format!("@record_result_{}", self.serial);
                    for f in self.fields(&o.ty)? {
                        let field = storage(&temp, &f.name);
                        prefix.push(Stmt::ArrayDecl {
                            kind: f.kind,
                            name: field.clone(),
                            dims: deferred(o.dims.len()),
                        });
                        prefix.push(Stmt::DummyIntent {
                            name: field.clone(),
                            intent: "allocatable".into(),
                        });
                        args.push(Expr::Ident(field));
                    }
                    prefix.push(Stmt::CallSub {
                        name: format!("@record_result${n}"),
                        args: args.clone(),
                    });
                    objects.insert(
                        temp.clone(),
                        Object {
                            ty: o.ty,
                            dims: deferred(o.dims.len()),
                        },
                    );
                    *e = Expr::Ident(temp);
                }
            }
            Expr::Index(n, args) => {
                self.component(n, objects)?;
                for e in args {
                    self.expression(e, objects, prefix)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                self.component(name, objects)?;
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, objects, prefix)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, objects, prefix)?;
                self.expression(b, objects, prefix)?;
            }
            Expr::Un(_, e) => self.expression(e, objects, prefix)?,
            _ => {}
        }
        Ok(())
    }
    fn output(
        &mut self,
        items: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let mut expanded = vec![];
        for mut item in std::mem::take(items) {
            self.expression(&mut item, objects, prefix)?;
            if let Expr::Ident(array) = &item {
                if let Some(o) = objects.get(array).filter(|o| !o.dims.is_empty()).cloned() {
                    let fields = self.fields(&o.ty)?;
                    let first = storage(array, &fields[0].name);
                    self.serial += 1;
                    let index = format!("@record_output_{}", self.serial);
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![index.clone()],
                    });
                    let mut ordinal = Expr::Bin(
                        crate::ast::BinOp::Sub,
                        Box::new(Expr::Ident(index.clone())),
                        Box::new(Expr::IntLit("1".into())),
                    );
                    let mut indices = vec![];
                    for axis in 1..=o.dims.len() {
                        let extent = Expr::Call(
                            "size".into(),
                            vec![Expr::Ident(first.clone()), Expr::IntLit(axis.to_string())],
                        );
                        indices.push(Expr::Bin(
                            crate::ast::BinOp::Add,
                            Box::new(Expr::Call(
                                "mod".into(),
                                vec![ordinal.clone(), extent.clone()],
                            )),
                            Box::new(Expr::IntLit("1".into())),
                        ));
                        ordinal =
                            Expr::Bin(crate::ast::BinOp::Div, Box::new(ordinal), Box::new(extent));
                    }
                    let mut args = vec![
                        Expr::Ident(index),
                        Expr::IntLit("1".into()),
                        Expr::Call("size".into(), vec![Expr::Ident(first)]),
                        Expr::IntLit("1".into()),
                    ];
                    args.extend(fields.iter().map(|f| Expr::Index(storage(array, &f.name), indices.clone())));
                    expanded.push(Expr::Call("@output_implied_do".into(), args));
                    continue;
                }
            }
            if let Expr::Call(array, _) = &item {
                if let Some(o) = objects.get(array).filter(|o| !o.dims.is_empty()).cloned() {
                    let mut output_dims = vec![];
                    let Expr::Call(_, args) = &item else {
                        unreachable!()
                    };
                    for arg in args {
                        if let Some(d) = self.array_shape(arg) {
                            for dimension in d {
                                if let Expr::Call(tag, a) = &dimension {
                                    if tag == "@declared_bounds" {
                                        let lower = crate::parameters::integer_constant(&a[0])?;
                                        let upper = crate::parameters::integer_constant(&a[1])?;
                                        output_dims.push(Expr::IntLit(
                                            (upper - lower + 1).max(0).to_string(),
                                        ));
                                        continue;
                                    }
                                }
                                output_dims.push(dimension);
                            }
                        } else if matches!(arg,Expr::Call(tag,_) if tag=="@section_subscript") {
                            bail!("output of derived triplet sections is not yet supported");
                        }
                    }
                    let (parts, array) = self.parts(item, &o.ty, objects, prefix)?;
                    if array {
                        if output_dims.is_empty()
                            || output_dims
                                .iter()
                                .any(|e| crate::parameters::integer_constant(e).is_err())
                        {
                            bail!("output of runtime-sized derived vector selections is not yet supported");
                        }
                        self.serial += 1;
                        let temp = format!("@record_output_selection_{}", self.serial);
                        for (field, value) in self.fields(&o.ty)?.into_iter().zip(parts) {
                            let name = storage(&temp, &field.name);
                            prefix.push(Stmt::ArrayDecl {
                                kind: field.kind,
                                name: name.clone(),
                                dims: output_dims.clone(),
                            });
                            prefix.push(Stmt::Assign { name, value });
                        }
                        objects.insert(
                            temp.clone(),
                            Object {
                                ty: o.ty,
                                dims: output_dims,
                            },
                        );
                        let mut items = vec![Expr::Ident(temp)];
                        self.output(&mut items, objects, prefix)?;
                        expanded.extend(items);
                    } else {
                        expanded.extend(parts);
                    }
                    continue;
                }
            }
            expanded.push(item);
        }
        *items = expanded;
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Objects) -> Result<()> {
        let mut objects = inherited.clone();
        let outer_shapes = self.array_shapes.clone();
        for s in body.iter() {
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    self.array_shapes.insert(name.clone(), dims.clone());
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        self.array_shapes.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } => {
                    for n in names {
                        objects.insert(
                            n.clone(),
                            Object {
                                ty: type_name.clone(),
                                dims: dims.clone(),
                            },
                        );
                    }
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        objects.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    objects.remove(name);
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut offset = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(p);
                out.push(Stmt::SourceLocation(p));
                continue;
            }
            let mut prefix = vec![];
            (|| -> Result<()> {
                match &mut s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        attributes,
                        polymorphic,
                    } if !dims.is_empty() => {
                        if *polymorphic {
                            bail!("polymorphic derived arrays are not yet supported");
                        }
                        if attributes
                            .iter()
                            .any(|a| matches!(a.as_str(), "save" | "parameter" | "pointer"))
                        {
                            bail!(
                                "SAVED, PARAMETER and POINTER derived arrays are not yet supported"
                            );
                        }
                        for e in dims.iter_mut() {
                            self.expression(e, &mut objects, &mut prefix)?;
                        }
                        for n in names {
                            for f in self.fields(type_name)? {
                                let field = storage(n, &f.name);
                                prefix.push(Stmt::ArrayDecl {
                                    kind: f.kind,
                                    name: field.clone(),
                                    dims: dims.clone(),
                                });
                                for intent in attributes.iter().filter(|a| {
                                    matches!(
                                        a.as_str(),
                                        "allocatable" | "optional" | "pointer" | "target"
                                    ) || a.starts_with("intent(")
                                }) {
                                    prefix.push(Stmt::DummyIntent {
                                        name: field.clone(),
                                        intent: intent.clone(),
                                    });
                                }
                            }
                        }
                        return Ok(());
                    }
                    Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. }
                        if objects.get(name).is_some_and(|o| !o.dims.is_empty()) =>
                    {
                        let name = name.clone();
                        let o = objects[&name].clone();
                        let (values, array) =
                            self.parts(value.clone(), &o.ty, &mut objects, &mut prefix)?;
                        let mut indices = match &s {
                            Stmt::AssignIndex { indices, .. } => Some(indices.clone()),
                            _ => None,
                        };
                        if let Some(indices) = &mut indices {
                            self.capture_indices(indices, &mut prefix);
                        }
                        let fields = self.fields(&o.ty)?;
                        let mut captured = vec![];
                        for (f, e) in fields.iter().zip(values) {
                            if array {
                                if let Expr::Call(tag, args) = &e {
                                    if tag == "@array_constructor" {
                                        self.serial += 1;
                                        let temp = format!("@record_array_value_{}", self.serial);
                                        prefix.push(Stmt::ArrayDecl {
                                            kind: f.kind.clone(),
                                            name: temp.clone(),
                                            dims: vec![Expr::IntLit(args.len().to_string())],
                                        });
                                        if !args.is_empty() {
                                            prefix.push(Stmt::Assign {
                                                name: temp.clone(),
                                                value: e,
                                            });
                                        }
                                        captured.push(Expr::Ident(temp));
                                        continue;
                                    }
                                }
                                captured.push(e);
                            } else {
                                self.serial += 1;
                                let temp = format!("@record_value_{}", self.serial);
                                prefix.push(Stmt::VarDecl {
                                    kind: f.kind.clone(),
                                    names: vec![temp.clone()],
                                });
                                prefix.push(Stmt::Assign {
                                    name: temp.clone(),
                                    value: e,
                                });
                                captured.push(Expr::Ident(temp));
                            }
                        }
                        for (f, value) in fields.into_iter().zip(captured) {
                            let target = storage(&name, &f.name);
                            prefix.push(if let Some(indices) = &indices {
                                fn retarget(e: &mut Expr, name: &str, target: &str) {
                                    match e {
                                        Expr::Section { name: n, .. } if n == name => {
                                            *n = target.to_owned()
                                        }
                                        Expr::Call(_, args) => {
                                            for e in args {
                                                retarget(e, name, target);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                let mut indices = indices.clone();
                                for e in &mut indices {
                                    retarget(e, &name, &target);
                                }
                                Stmt::AssignIndex {
                                    name: target,
                                    indices,
                                    value,
                                }
                            } else {
                                Stmt::Assign {
                                    name: target,
                                    value,
                                }
                            });
                        }
                        return Ok(());
                    }
                    Stmt::CallSub { name, args }
                        if name == "@allocate" || name == "@deallocate" =>
                    {
                        let mut flat = vec![];
                        let mut defaults = vec![];
                        for mut e in std::mem::take(args) {
                            let (n, dims) = match &e {
                                Expr::Call(n, d) => (n, Some(d)),
                                Expr::Ident(n) => (n, None),
                                _ => {
                                    self.expression(&mut e, &mut objects, &mut prefix)?;
                                    flat.push(e);
                                    continue;
                                }
                            };
                            if let Some(o) = objects.get(n).filter(|o| !o.dims.is_empty()).cloned()
                            {
                                let mut bounds = dims.cloned();
                                if let Some(d) = &mut bounds {
                                    for e in d.iter_mut() {
                                        self.expression(e, &mut objects, &mut prefix)?;
                                    }
                                    self.capture_indices(d, &mut prefix);
                                }
                                for f in self.fields(&o.ty)? {
                                    let field = storage(n, &f.name);
                                    flat.push(if let Some(d) = &bounds {
                                        Expr::Call(field.clone(), d.clone())
                                    } else {
                                        Expr::Ident(field.clone())
                                    });
                                    if name == "@allocate" {
                                        if let Some(value) = f.default {
                                            defaults.push(Stmt::Assign { name: field, value });
                                        }
                                    }
                                }
                            } else {
                                flat.push(e);
                            }
                        }
                        *args = flat;
                        prefix.push(s.clone());
                        prefix.extend(defaults);
                        return Ok(());
                    }
                    Stmt::CallSub { name, args }
                        if name == "@write" || name == "@formatted_print" =>
                    {
                        let mut values = args.split_off(if name == "@write" { 2 } else { 1 });
                        self.output(&mut values, &mut objects, &mut prefix)?;
                        args.extend(values);
                    }
                    Stmt::CallSub { name, args } => {
                        if self
                            .arguments(name, args, &mut objects, &mut prefix)?
                            .is_some()
                        {
                            bail!("derived array function cannot be invoked with CALL");
                        }
                    }
                    Stmt::Print { items } => self.output(items, &mut objects, &mut prefix)?,
                    Stmt::Read { args: items } => {
                        for e in items {
                            self.expression(e, &mut objects, &mut prefix)?;
                        }
                    }
                    Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. } => {
                        self.component(name, &objects)?;
                        self.expression(value, &mut objects, &mut prefix)?;
                        if let Stmt::AssignIndex { indices, .. } = &mut s {
                            for e in indices {
                                self.expression(e, &mut objects, &mut prefix)?;
                            }
                        }
                    }
                    Stmt::Return(Some(e)) => self.expression(e, &mut objects, &mut prefix)?,
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    } => {
                        self.expression(cond, &mut objects, &mut prefix)?;
                        self.body(then_body, &objects)?;
                        if let Some(b) = else_body {
                            self.body(b, &objects)?;
                        }
                    }
                    Stmt::Do {
                        start, end, body, ..
                    } => {
                        self.expression(start, &mut objects, &mut prefix)?;
                        self.expression(end, &mut objects, &mut prefix)?;
                        self.body(body, &objects)?;
                    }
                    Stmt::DoWhile { cond, body } => {
                        self.expression(cond, &mut objects, &mut prefix)?;
                        self.body(body, &objects)?;
                    }
                    Stmt::Block { body } | Stmt::Module { body, .. } => {
                        self.body(body, &objects)?
                    }
                    Stmt::Function {
                        name,
                        params,
                        body,
                        pure,
                        ..
                    }
                    | Stmt::Subroutine {
                        name,
                        params,
                        body,
                        pure,
                        ..
                    } => {
                        let old_name = name.clone();
                        if let Some(p) = self.procedures.get(name).cloned() {
                            let mut flat = vec![];
                            for n in params.iter() {
                                if let Some(o) = p.arrays.get(n) {
                                    flat.extend(
                                        self.fields(&o.ty)?.iter().map(|f| storage(n, &f.name)),
                                    );
                                } else {
                                    flat.push(n.clone());
                                }
                            }
                            if let Some((result, o)) = &p.result {
                                for statement in body.iter_mut() {
                                    if let Stmt::DerivedDecl {
                                        names,
                                        dims,
                                        attributes,
                                        ..
                                    } = statement
                                    {
                                        if names.contains(result) {
                                            *dims = deferred(o.dims.len());
                                            if !attributes.iter().any(|a| a == "allocatable") {
                                                attributes.push("allocatable".into());
                                            }
                                            attributes.push("intent(out)".into());
                                        }
                                    }
                                }
                                let allocatable = p.arrays[result]
                                    .dims
                                    .iter()
                                    .all(|d| matches!(d,Expr::Ident(n) if n=="@deferred_extent"));
                                if !allocatable {
                                    let pos = body
                                        .iter()
                                        .position(|s| {
                                            !matches!(
                                                s,
                                                Stmt::SourceLocation(_)
                                                    | Stmt::DerivedDecl { .. }
                                                    | Stmt::VarDecl { .. }
                                                    | Stmt::ArrayDecl { .. }
                                                    | Stmt::InitializedDecl { .. }
                                                    | Stmt::DummyIntent { .. }
                                                    | Stmt::Implicit(_)
                                                    | Stmt::Use { .. }
                                            )
                                        })
                                        .unwrap_or(body.len());
                                    body.insert(
                                        pos,
                                        Stmt::CallSub {
                                            name: "@allocate".into(),
                                            args: vec![Expr::Call(result.clone(), o.dims.clone())],
                                        },
                                    );
                                }
                                fn returns(body: &mut [Stmt], result: &str) {
                                    for s in body {
                                        match s {
                                            Stmt::Return(Some(Expr::Ident(n))) if n == result => {
                                                *s = Stmt::Return(None)
                                            }
                                            Stmt::If {
                                                then_body,
                                                else_body,
                                                ..
                                            } => {
                                                returns(then_body, result);
                                                if let Some(b) = else_body {
                                                    returns(b, result);
                                                }
                                            }
                                            Stmt::Do { body, .. }
                                            | Stmt::DoWhile { body, .. }
                                            | Stmt::Block { body } => returns(body, result),
                                            _ => {}
                                        }
                                    }
                                }
                                returns(body, result);
                                flat.extend(
                                    self.fields(&o.ty)?.iter().map(|f| storage(result, &f.name)),
                                );
                                self.body(body, &objects)?;
                                s = Stmt::Subroutine {
                                    name: format!("@record_result${old_name}"),
                                    params: flat,
                                    body: std::mem::take(body),
                                    pure: *pure,
                                    elemental: false,
                                };
                            } else {
                                *params = flat;
                                self.body(body, &objects)?;
                            }
                        } else {
                            self.body(body, &objects)?;
                        }
                    }
                    _ => {}
                }
                prefix.push(s.clone());
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, offset))?;
            out.extend(prefix);
        }
        *body = out;
        self.array_shapes = outer_shapes;
        Ok(())
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut lower = Lower {
        types: HashMap::new(),
        procedures: HashMap::new(),
        serial: 0,
        array_shapes: HashMap::new(),
    };
    lower.collect(&p.body)?;
    lower.body(&mut p.body, &Objects::new())?;
    Ok(p)
}

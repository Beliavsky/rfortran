// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Evaluate each derived-valued ELEMENTAL call once into component storage.
//! Hidden elemental subroutines reuse the ordinary dummy/array call lowering.
use super::*;

#[derive(Clone)]
pub(super) struct ResultProcedure {
    signature: crate::named_arguments::Signature,
    ty: String,
    helper: String,
    kinds: Vec<Option<TypeSpec>>,
}

impl Lower {
    pub(super) fn prepare_elemental_results(&mut self, body: &mut [Stmt]) {
        for s in body {
            if let Stmt::Function {
                name,
                params,
                body,
                elemental: true,
                pure,
                ..
            } = s
            {
                let result = match body.last() {
                    Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                    _ => name.clone(),
                };
                let ty = body.iter().find_map(|s| match s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        ..
                    } if dims.is_empty() && names.contains(&result) => Some(type_name.clone()),
                    _ => None,
                });
                if let Some(ty) = ty {
                    let helper = format!("@elemental_result_{name}");
                    self.elemental_results.insert(
                        name.clone(),
                        ResultProcedure {
                            signature: crate::named_arguments::Signature::new(params, body),
                            ty,
                            helper: helper.clone(),
                            kinds: params
                                .iter()
                                .map(|p| {
                                    body.iter().find_map(|s| match s {
                                        Stmt::VarDecl { kind, names } if names.contains(p) => {
                                            Some(kind.clone())
                                        }
                                        Stmt::ArrayDecl { kind, name, .. } if name == p => {
                                            Some(kind.clone())
                                        }
                                        _ => None,
                                    })
                                })
                                .collect(),
                        },
                    );
                    if matches!(body.last(), Some(Stmt::Return(Some(Expr::Ident(n)))) if n == &result)
                    {
                        body.pop();
                    }
                    fn returns(body: &mut [Stmt]) {
                        for s in body {
                            if let Stmt::Return(value) = s {
                                *value = None;
                            }
                            // An internal procedure has its own result and returns.
                            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                                for child in crate::procedure_arguments::children(s) {
                                    returns(child);
                                }
                            }
                        }
                    }
                    returns(body);
                    // Keep the result local until argument flattening; marking
                    // its declaration OUT applies component defaults as on entry.
                    let mut prepared = vec![];
                    for mut declaration in std::mem::take(body) {
                        if let Stmt::DerivedDecl {
                            names, attributes, ..
                        } = &mut declaration
                        {
                            if names.contains(&result) {
                                if names.len() == 1 {
                                    attributes.push("intent(out)".into());
                                } else {
                                    names.retain(|n| n != &result);
                                    let mut output = declaration.clone();
                                    if let Stmt::DerivedDecl {
                                        names, attributes, ..
                                    } = &mut output
                                    {
                                        *names = vec![result.clone()];
                                        attributes.push("intent(out)".into());
                                    }
                                    prepared.push(output);
                                }
                            }
                        }
                        prepared.push(declaration);
                    }
                    *body = prepared;
                    params.push(result);
                    *s = Stmt::Subroutine {
                        name: helper,
                        params: params.clone(),
                        body: std::mem::take(body),
                        pure: *pure,
                        elemental: true,
                    };
                }
            }
            for child in crate::procedure_arguments::children(s) {
                self.prepare_elemental_results(child);
            }
        }
    }

    fn result_actual_shape(
        &mut self,
        actual: &Expr,
        objects: &Objects,
    ) -> Result<Option<Vec<Expr>>> {
        if crate::named_arguments::omitted(actual) {
            return Ok(None);
        }
        if let Expr::Ident(n) = actual {
            if let Some(o) = objects.get(n).filter(|o| !o.dims.is_empty()) {
                return Ok(Some(o.dims.clone()));
            }
        }
        if let Expr::Section { name, .. } = actual {
            if let Some(o) = objects.get(name).filter(|o| !o.dims.is_empty()) {
                let previous = self.array_shapes.insert(name.clone(), o.dims.clone());
                let shape = self.array_shape(actual);
                match previous {
                    Some(previous) => {
                        self.array_shapes.insert(name.clone(), previous);
                    }
                    None => {
                        self.array_shapes.remove(name);
                    }
                }
                // A section of a rank-one array whose bounds are known only
                // at run time has the section's size.
                return match shape {
                    Some(shape) => Ok(Some(shape)),
                    None if o.dims.len() == 1 => Ok(Some(vec![Expr::Call("size".into(), vec![actual.clone()])])),
                    None => bail!("ELEMENTAL derived results currently require rank-one array sections"),
                };
            }
        }
        if let Some(shape) = self.array_shape(actual) {
            return Ok(Some(shape));
        }
        if let Expr::Call(n, args) | Expr::Index(n, args) = actual {
            if self.array_shapes.contains_key(n)
                || objects.get(n).is_some_and(|o| !o.dims.is_empty())
            {
                for index in args {
                    if let Some(shape) = self.array_shape(index) {
                        return Ok(Some(shape));
                    }
                }
            }
        }
        match actual {
            Expr::Call(n, args)
                if !self.array_shapes.contains_key(n)
                    && !objects.contains_key(n)
                    && (self.elemental_functions.contains(n)
                        || crate::real_intrinsics::keys(n).is_some()
                        || matches!(
                            n.as_str(),
                            "abs"
                                | "int"
                                | "real"
                                | "dble"
                                | "nint"
                                | "ceiling"
                                | "floor"
                                | "max"
                                | "min"
                                | "mod"
                                | "modulo"
                                | "merge"
                        )) =>
            {
                // Only known elemental calls inherit an operand shape;
                // a scalar function may legitimately accept an array input.
                for arg in args {
                    if let Some(shape) = self.result_actual_shape(arg, objects)? {
                        return Ok(Some(shape));
                    }
                }
            }
            Expr::Section { .. } => {
                bail!("ELEMENTAL derived results currently require fixed rank-one array sections")
            }
            _ => {}
        }
        Ok(None)
    }

    pub(super) fn materialize_elemental_result(
        &mut self,
        e: &mut Expr,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(name, args) = e else {
            return Ok(false);
        };
        let Some(procedure) = self.elemental_results.get(name).cloned() else {
            return Ok(false);
        };
        procedure.signature.reorder(name, args)?;
        args.resize_with(procedure.signature.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        for arg in args.iter_mut() {
            self.expression(arg, objects, prefix)?;
            // An array-valued function's result has no shape here: evaluate it
            // once into an ALLOCATABLE temporary, of the result's run-time extent.
            if let Expr::Call(n, _) = &*arg {
                if let Some((kind, rank)) = self.array_functions.get(n).cloned() {
                    if !objects.contains_key(n) && !self.array_shapes.contains_key(n) {
                        self.serial += 1;
                        let actual = format!("@elemental_function_actual_{}", self.serial);
                        let deferred = vec![Expr::Ident("@deferred_extent".into()); rank];
                        prefix.push(Stmt::ArrayDecl { kind, name: actual.clone(), dims: deferred.clone() });
                        prefix.push(Stmt::DummyIntent { name: actual.clone(), intent: "allocatable".into() });
                        prefix.push(Stmt::Assign { name: actual.clone(), value: std::mem::replace(arg, Expr::Ident(actual.clone())) });
                        self.array_shapes.insert(actual, deferred);
                    }
                }
            }
        }
        let mut dims = vec![];
        let mut shaped = None;
        for arg in args.iter() {
            if let Some(shape) = self.result_actual_shape(arg, objects)? {
                shaped = Some(arg.clone());
                dims = shape
                    .into_iter()
                    .map(|dim| match dim {
                        Expr::Call(n, a) if n == "@declared_bounds" => Expr::Bin(
                            crate::ast::BinOp::Add,
                            Box::new(Expr::Bin(
                                crate::ast::BinOp::Sub,
                                Box::new(a[1].clone()),
                                Box::new(a[0].clone()),
                            )),
                            Box::new(Expr::IntLit("1".into())),
                        ),
                        dim => dim,
                    })
                    .collect();
                break;
            }
        }
        if dims.len() > 1 {
            bail!("ELEMENTAL derived results currently require scalar or rank-one actuals");
        }
        // An extent known only at run time, as of an assumed-shape or
        // ALLOCATABLE actual, is the actual's size.
        for dim in dims.iter_mut() {
            if matches!(dim, Expr::Ident(n) if n == "@deferred_extent") {
                let actual = shaped.clone().context("ELEMENTAL derived result extent")?;
                *dim = Expr::Call("size".into(), vec![actual]);
            }
        }
        self.serial += 1;
        let temp = format!("@elemental_record_result_{}", self.serial);
        // A result whose extent is known only at run time is an ALLOCATABLE
        // temporary, allocated here, as automatic derived arrays are.
        let runtime = dims.iter().any(|d| crate::vector_subscripts::fixed_extent(d).is_none());
        let declared = if runtime { vec![Expr::Ident("@deferred_extent".into()); dims.len()] } else { dims.clone() };
        let mut generated = vec![Stmt::DerivedDecl {
            type_name: procedure.ty.clone(),
            names: vec![temp.clone()],
            attributes: if runtime { vec!["allocatable".into()] } else { vec![] },
            polymorphic: false,
            dims: declared.clone(),
        }];
        if runtime {
            generated.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(temp.clone(), dims.clone())] });
        }
        let owned = self.fields(&procedure.ty)?.iter().any(|f| f.heap.is_some());
        if (owned || runtime) && !dims.is_empty() {
            // Scalarize before record argument flattening: each invocation owns
            // an independent result and receives ordinary scalar record actuals.
            // So does a result of run-time extent, whose shape the elemental
            // call lowering cannot check.
            let index = format!("@owned_elemental_index_{}", self.serial);
            generated.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![index.clone()],
            });
            let mut scalar_args = vec![];
            for (position, arg) in std::mem::take(args).into_iter().enumerate() {
                if crate::named_arguments::omitted(&arg) {
                    scalar_args.push(arg);
                    continue;
                }
                if let Some(info) = self.pack_object(&arg, objects) {
                    if info.dims.is_empty() {
                        scalar_args.push(self.capture_elemental_record(
                            arg,
                            info.ty,
                            &mut generated,
                        ));
                    } else {
                        let input = self.pack_input(&arg, info, objects, prefix)?;
                        if input.dims.len() != 1 {
                            bail!("ELEMENTAL owned record results require rank-one actuals");
                        }
                        generated.push(Stmt::If {
                            cond: Expr::Bin(
                                crate::ast::BinOp::Ne,
                                Box::new(input.length()),
                                Box::new(dims[0].clone()),
                            ),
                            then_body: vec![Stmt::CallSub {
                                name: "@error_stop".into(),
                                args: vec![Expr::Str("ELEMENTAL argument shape mismatch".into())],
                            }],
                            else_body: None,
                        });
                        scalar_args.push(input.element(Expr::Ident(index.clone())));
                    }
                } else if let Some(shape) = self.result_actual_shape(&arg, objects)? {
                    if shape.len() != 1 {
                        bail!("ELEMENTAL owned record results require rank-one actuals");
                    }
                    let kind = procedure.kinds[position]
                        .clone()
                        .context("missing ELEMENTAL intrinsic dummy type")?;
                    self.serial += 1;
                    let actual = format!("@owned_elemental_actual_{}", self.serial);
                    let fixed: Option<Vec<i64>> = shape.iter().map(crate::vector_subscripts::fixed_extent).collect();
                    match fixed {
                        Some(extents) => generated.push(Stmt::ArrayDecl {
                            kind,
                            name: actual.clone(),
                            dims: extents.iter().map(|n| Expr::IntLit(n.to_string())).collect(),
                        }),
                        // Of run-time extent: ALLOCATABLE, allocated by the
                        // assignment below.
                        None => {
                            generated.push(Stmt::ArrayDecl {
                                kind,
                                name: actual.clone(),
                                dims: vec![Expr::Ident("@deferred_extent".into()); shape.len()],
                            });
                            generated.push(Stmt::DummyIntent { name: actual.clone(), intent: "allocatable".into() });
                        }
                    }
                    generated.push(Stmt::Assign {
                        name: actual.clone(),
                        value: arg,
                    });
                    generated.push(Stmt::If {
                        cond: Expr::Bin(
                            crate::ast::BinOp::Ne,
                            Box::new(Expr::Call("size".into(), vec![Expr::Ident(actual.clone())])),
                            Box::new(dims[0].clone()),
                        ),
                        then_body: vec![Stmt::CallSub {
                            name: "@error_stop".into(),
                            args: vec![Expr::Str("ELEMENTAL argument shape mismatch".into())],
                        }],
                        else_body: None,
                    });
                    scalar_args.push(Expr::Call(actual, vec![Expr::Ident(index.clone())]));
                } else {
                    scalar_args.push(self.capture_elemental_scalar(
                        arg,
                        procedure.kinds[position].clone(),
                        &mut generated,
                    )?);
                }
            }
            scalar_args.push(Expr::Call(temp.clone(), vec![Expr::Ident(index.clone())]));
            generated.push(Stmt::Do {
                var: index,
                start: Expr::IntLit("1".into()),
                end: dims[0].clone(),
                body: vec![Stmt::CallSub {
                    name: format!("{PREPARED_CALL}{}", procedure.helper),
                    args: scalar_args,
                }],
            });
        } else {
            args.push(Expr::Ident(temp.clone()));
            generated.push(Stmt::CallSub {
                name: format!("{PREPARED_CALL}{}", procedure.helper),
                args: std::mem::take(args),
            });
        }
        // Process generated storage and the hidden call through the same record
        // expansion as source declarations. No function body is duplicated.
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        objects.insert(
            temp.clone(),
            Object {
                ty: procedure.ty.clone(),
                dims: declared.clone(),
            },
        );
        if dims.is_empty() {
            self.register_components(&temp, &procedure.ty, objects);
        } else if !runtime {
            for field in self.fields(&procedure.ty)? {
                self.array_shapes
                    .insert(storage(&temp, &field.name), storage_dims(&field, &dims));
            }
        }
        *e = Expr::Ident(temp);
        Ok(true)
    }
}

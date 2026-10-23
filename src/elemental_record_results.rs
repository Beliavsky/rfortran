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
                return shape.map(Some).context(
                    "ELEMENTAL derived results currently require fixed rank-one array sections",
                );
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
        }
        let mut dims = vec![];
        for arg in args.iter() {
            if let Some(shape) = self.result_actual_shape(arg, objects)? {
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
        if dims
            .iter()
            .any(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent"))
        {
            bail!("ELEMENTAL derived results currently require constant rank-one extents");
        }
        self.serial += 1;
        let temp = format!("@elemental_record_result_{}", self.serial);
        let mut generated = vec![Stmt::DerivedDecl {
            type_name: procedure.ty.clone(),
            names: vec![temp.clone()],
            attributes: vec![],
            polymorphic: false,
            dims: dims.clone(),
        }];
        args.push(Expr::Ident(temp.clone()));
        generated.push(Stmt::CallSub {
            name: procedure.helper,
            args: std::mem::take(args),
        });
        // Process generated storage and the hidden call through the same record
        // expansion as source declarations. No function body is duplicated.
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        objects.insert(
            temp.clone(),
            Object {
                ty: procedure.ty.clone(),
                dims: dims.clone(),
            },
        );
        if dims.is_empty() {
            self.register_components(&temp, &procedure.ty, objects);
        } else {
            for field in self.fields(&procedure.ty)? {
                self.array_shapes
                    .insert(storage(&temp, &field.name), storage_dims(&field, &dims));
            }
        }
        *e = Expr::Ident(temp);
        Ok(true)
    }
}

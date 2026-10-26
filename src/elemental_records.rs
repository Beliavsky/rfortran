// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Carry record component arrays through scalar record argument flattening.
//! Ordinary elemental lowering then supplies shape checks and scalar calls.
use super::*;

#[derive(Clone)]
pub(super) struct Procedure {
    signature: crate::named_arguments::Signature,
    types: Vec<Option<String>>,
    kinds: Vec<Option<TypeSpec>>,
    intents: Vec<String>,
}

impl Lower {
    pub(super) fn collect_elemental(&mut self, name: &str, params: &[String], body: &[Stmt]) {
        let types: Vec<_> = params
            .iter()
            .map(|param| {
                body.iter().find_map(|s| match s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        ..
                    } if dims.is_empty() && names.contains(param) => Some(type_name.clone()),
                    _ => None,
                })
            })
            .collect();
        if types.iter().any(Option::is_some) {
            self.elemental_records.insert(
                name.into(),
                Procedure {
                    signature: crate::named_arguments::Signature::new(params, body),
                    types,
                    kinds: params
                        .iter()
                        .map(|p| {
                            body.iter().find_map(|s| match s {
                                Stmt::VarDecl { kind, names } if names.contains(p) => {
                                    Some(kind.clone())
                                }
                                _ => None,
                            })
                        })
                        .collect(),
                    intents: params
                        .iter()
                        .map(|p| {
                            body.iter()
                                .find_map(|s| match s {
                                    Stmt::DummyIntent { name, intent }
                                        if name == p && intent.starts_with("intent(") =>
                                    {
                                        Some(intent.clone())
                                    }
                                    Stmt::DerivedDecl {
                                        names, attributes, ..
                                    } if names.contains(p) => attributes
                                        .iter()
                                        .find(|a| a.starts_with("intent("))
                                        .cloned(),
                                    _ => None,
                                })
                                .unwrap_or_default()
                        })
                        .collect(),
                },
            );
        }
    }

    pub(super) fn capture_elemental_record(
        &mut self,
        arg: Expr,
        ty: String,
        out: &mut Vec<Stmt>,
    ) -> Expr {
        if matches!(arg, Expr::Ident(_)) {
            return arg;
        }
        self.serial += 1;
        let name = format!("@owned_elemental_record_{}", self.serial);
        out.push(Stmt::DerivedDecl {
            type_name: ty,
            names: vec![name.clone()],
            attributes: vec![],
            polymorphic: false,
            dims: vec![],
        });
        out.push(Stmt::Assign {
            name: name.clone(),
            value: arg,
        });
        Expr::Ident(name)
    }

    pub(super) fn capture_elemental_scalar(
        &mut self,
        arg: Expr,
        kind: Option<TypeSpec>,
        out: &mut Vec<Stmt>,
    ) -> Result<Expr> {
        if matches!(
            arg,
            Expr::Ident(_) | Expr::IntLit(_) | Expr::RealLit(_) | Expr::Str(_) | Expr::Logical(_)
        ) {
            return Ok(arg);
        }
        let Some(kind) = kind else { return Ok(arg) };
        // Character expressions use existing length specialization; numeric
        // scalar expressions must be evaluated once, outside the element loop.
        if matches!(kind, TypeSpec::CharacterAssumed) {
            return Ok(arg);
        }
        self.serial += 1;
        let name = format!("@owned_elemental_scalar_{}", self.serial);
        out.push(Stmt::VarDecl {
            kind,
            names: vec![name.clone()],
        });
        out.push(Stmt::Assign {
            name: name.clone(),
            value: arg,
        });
        Ok(Expr::Ident(name))
    }

    pub(super) fn owned_elemental_call(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Some(p) = self.elemental_records.get(name).cloned() else {
            return Ok(false);
        };
        if !p.types.iter().flatten().any(|ty| {
            self.types
                .get(ty)
                .is_some_and(|f| f.iter().any(|f| f.heap.is_some() || !f.dims.is_empty()))
        }) {
            return Ok(false);
        }
        p.signature.reorder(name, args)?;
        let mut count = None;
        let mut conform = vec![];
        for arg in args.iter() {
            if let Some(info) = self.pack_object(arg, objects) {
                if !info.dims.is_empty() {
                    let input = self.pack_input(arg, info, objects, prefix)?;
                    conform = input.dims.clone();
                    count = Some(input.length());
                    break;
                }
            } else if let Some(shape) = self.array_shape(arg) {
                if shape.len() != 1 {
                    bail!("ELEMENTAL owned record calls currently require rank-one intrinsic array actuals");
                }
                count = Some(match &shape[0] {
                    Expr::Ident(n) if n == "@deferred_extent" => {
                        Expr::Call("size".into(), vec![arg.clone()])
                    }
                    d => crate::vector_subscripts::fixed_extent(d)
                        .map_or_else(|| d.clone(), |n| Expr::IntLit(n.to_string())),
                });
                conform = vec![count.clone().unwrap()];
                break;
            }
        }
        let Some(count) = count else { return Ok(false) };
        self.serial += 1;
        let index = format!("@owned_elemental_call_{}", self.serial);
        let mut generated = vec![Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![index.clone()],
        }];
        let mut actuals = vec![];
        for (i, arg) in args.iter().cloned().enumerate() {
            if crate::named_arguments::omitted(&arg) {
                actuals.push(arg);
                continue;
            }
            if let Some(info) = self.pack_object(&arg, objects) {
                if info.dims.is_empty() {
                    actuals.push(self.capture_elemental_record(arg, info.ty, &mut generated));
                    continue;
                }
                let input = self.pack_input(&arg, info, objects, prefix)?;
                if input.dims.len() != conform.len() {
                    bail!("ELEMENTAL argument rank mismatch");
                }
                for (a, b) in input.dims.iter().zip(&conform) {
                    generated.push(shape_check(a.clone(), b.clone()));
                }
                actuals.push(input.element(Expr::Ident(index.clone())));
            } else if let Some(shape) = self.array_shape(&arg) {
                if shape.len() != 1 {
                    bail!("ELEMENTAL owned record calls currently require rank-one intrinsic array actuals");
                }
                if conform.len() != 1 {
                    bail!("ELEMENTAL argument rank mismatch");
                }
                if let Expr::Ident(n) = &arg {
                    generated.push(shape_check(
                        Expr::Call("size".into(), vec![arg.clone()]),
                        count.clone(),
                    ));
                    let at = Expr::Bin(
                        crate::ast::BinOp::Add,
                        Box::new(Expr::Call(
                            "lbound".into(),
                            vec![arg.clone(), Expr::IntLit("1".into())],
                        )),
                        Box::new(Expr::Bin(
                            crate::ast::BinOp::Sub,
                            Box::new(Expr::Ident(index.clone())),
                            Box::new(Expr::IntLit("1".into())),
                        )),
                    );
                    actuals.push(Expr::Call(n.clone(), vec![at]));
                } else {
                    if p.intents[i] != "intent(in)" {
                        bail!("ELEMENTAL output actual must designate variable storage");
                    }
                    let mut kind = p.kinds[i].clone().context("missing ELEMENTAL dummy type")?;
                    if matches!(kind, TypeSpec::CharacterAssumed) {
                        let Expr::Call(tag, items) = &arg else {
                            bail!("CHARACTER elemental expression requires a known length")
                        };
                        if tag != "@array_constructor" {
                            bail!("CHARACTER elemental expression requires a known length");
                        }
                        let width = items
                            .iter()
                            .map(|e| match e {
                                Expr::Str(s) => Some(s.len()),
                                _ => None,
                            })
                            .collect::<Option<Vec<_>>>()
                            .context(
                                "CHARACTER elemental constructor requires known element lengths",
                            )?
                            .into_iter()
                            .max()
                            .unwrap_or(0);
                        kind = TypeSpec::Character(Some(width));
                    }
                    self.serial += 1;
                    let temp = format!("@owned_elemental_input_{}", self.serial);
                    generated.push(Stmt::ArrayDecl {
                        kind,
                        name: temp.clone(),
                        dims: shape,
                    });
                    generated.push(Stmt::Assign {
                        name: temp.clone(),
                        value: arg,
                    });
                    generated.push(shape_check(
                        Expr::Call("size".into(), vec![Expr::Ident(temp.clone())]),
                        count.clone(),
                    ));
                    actuals.push(Expr::Call(temp, vec![Expr::Ident(index.clone())]));
                }
            } else {
                actuals.push(self.capture_elemental_scalar(
                    arg,
                    p.kinds[i].clone(),
                    &mut generated,
                )?);
            }
        }
        generated.push(Stmt::Do {
            var: index,
            start: Expr::IntLit("1".into()),
            end: count,
            body: vec![Stmt::CallSub {
                name: format!("{PREPARED_CALL}{name}"),
                args: actuals,
            }],
        });
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        Ok(true)
    }

    pub(super) fn elemental_actuals(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let Some(p) = self.elemental_records.get(name).cloned() else {
            return Ok(());
        };
        p.signature.reorder(name, args)?;
        // Source scalar records may contain arrays. Their flattened ABI is an
        // ordinary procedure with array dummies, not an elemental array call.
        let array_actual = args.iter().any(|arg| {
            self.array_shape(arg).is_some()
                || match arg {
                    Expr::Ident(n) | Expr::Section { name: n, .. } => {
                        objects.get(n).is_some_and(|o| !o.dims.is_empty())
                    }
                    Expr::Call(n, _) | Expr::Index(n, _) => {
                        objects.get(n).is_some_and(|o| !o.dims.is_empty())
                            && self.element(arg, objects).is_none()
                    }
                    _ => false,
                }
        });
        for (arg, ty) in args.iter_mut().zip(&p.types) {
            let Some(ty) = ty else { continue };
            if crate::named_arguments::omitted(arg) {
                continue;
            }
            let array = match arg {
                Expr::Ident(n) | Expr::Section { name: n, .. } => {
                    objects.get(n).is_some_and(|o| !o.dims.is_empty())
                }
                Expr::Call(n, _) | Expr::Index(n, _) => {
                    objects.get(n).is_some_and(|o| !o.dims.is_empty())
                        && self.element(arg, objects).is_none()
                }
                _ => false,
            };
            // Pass scalar elements as component storage too. Copying a whole
            // record to a temporary would read unset INTENT(OUT) components.
            let element = self.element(arg, objects).is_some();
            if self.types.get(ty).is_some_and(|fields| {
                fields
                    .iter()
                    .any(|f| !f.dims.is_empty() || f.heap.is_some())
            }) {
                if array_actual {
                    bail!("ELEMENTAL derived array actuals currently require scalar primitive components");
                }
                // Scalar array elements are materialized by scalar_procedures;
                // whole scalar records already have component storage.
                continue;
            }
            if !array && !element {
                continue;
            }
            if let Expr::Index(n, indices) = arg {
                *arg = Expr::Call(n.clone(), indices.clone());
            }
            let fields = self.fields(ty)?;
            let (parts, _) = self.prepared_parts(arg.clone(), ty, objects, prefix)?;
            let mut values = vec![Expr::Str(ty.clone())];
            values.extend(
                fields
                    .iter()
                    .zip(parts)
                    .map(|(f, value)| Expr::Call(format!("@record_field:{}", f.name), vec![value])),
            );
            *arg = Expr::Call("@elemental_record".into(), values);
        }
        Ok(())
    }
}

fn shape_check(a: Expr, b: Expr) -> Stmt {
    Stmt::If {
        cond: Expr::Bin(crate::ast::BinOp::Ne, Box::new(a), Box::new(b)),
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str("ELEMENTAL argument shape mismatch".into())],
        }],
        else_body: None,
    }
}

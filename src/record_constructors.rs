// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Materialize derived constructors as records, sharing PACK's record gather.
use super::record_pack::Input;
use super::*;
use crate::ast::BinOp;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn add(a: Expr, b: Expr) -> Expr {
    Expr::Bin(BinOp::Add, Box::new(a), Box::new(b))
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}

impl Lower {
    /// Keep scalar structure constructors in step with the field-wise storage
    /// used for their derived-type array components.
    pub(super) fn expand_component_constructor(
        &mut self, value: &mut Expr, objects: &mut Objects, prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(ty, args) = value else { return Ok(false); };
        let Some(components) = self.constructor_components.get(ty).cloned() else { return Ok(false); };
        let arrays = self.array_components[ty].clone();
        crate::named_arguments::Signature {
            params: components.clone(),
            // The scalar constructor pass diagnoses missing required fields;
            // omitted allocatables and default-initialized fields stay omitted.
            optional: components.iter().cloned().collect(),
        }.reorder(ty, args)?;
        args.resize_with(components.len(), || Expr::Call(crate::named_arguments::OMITTED.into(), vec![]));
        let mut expanded = vec![];
        for (component, mut arg) in components.into_iter().zip(std::mem::take(args)) {
            if let Some((_, array)) = arrays.iter().find(|(n,_)| n == &component) {
                if crate::named_arguments::omitted(&arg)
                    || matches!(&arg, Expr::Call(n, a) if n == "null" && a.is_empty())
                {
                    expanded.extend(self.fields(&array.ty)?.iter().map(|_|arg.clone()));
                } else {
                    if self.fields(&array.ty)?.iter().any(|f| f.heap.is_some()) {
                        bail!("structure constructors with derived array components whose elements own allocatable components are not yet supported");
                    }
                    let (parts, _) = self.parts(arg, &array.ty, objects, prefix)?;
                    expanded.extend(parts);
                }
            } else {
                self.expression(&mut arg, objects, prefix)?;
                expanded.push(arg);
            }
        }
        *args = expanded;
        Ok(true)
    }

    /// Capture each item once, then append its records in column-major order.
    /// A scalar item contributes one record; an array contributes SIZE(item).
    pub(super) fn constructor_records(
        &mut self,
        value: &mut Expr,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(tag, _) = value else {
            return Ok(false);
        };
        if !matches!(tag.as_str(), "@array_constructor" | "@record_constructor") {
            return Ok(false);
        }
        let Some(info) = self.pack_object(value, objects) else {
            return Ok(false);
        };
        let fields = self.fields(&info.ty)?;
        if fields.iter().any(|f| f.heap.is_some()) {
            bail!(
                "array constructors of records with allocatable components are not yet supported"
            );
        }
        let Expr::Call(tag, args) = value else {
            unreachable!()
        };
        let skip = usize::from(tag == "@record_constructor");
        let mut sources = vec![];
        let mut known_length = Some(0i64);
        for mut item in args[skip..].to_vec() {
            self.expression(&mut item, objects, prefix)?;
            let source = self.pack_object(&item, objects).with_context(|| {
                format!(
                    "derived array constructor requires items of type `{}`",
                    info.ty
                )
            })?;
            if source.ty != info.ty {
                bail!(
                    "derived array constructor type mismatch: expected `{}`, got `{}`",
                    info.ty,
                    source.ty
                );
            }
            let known_item = if source.dims.is_empty() {
                Some(1)
            } else if matches!(&item, Expr::Ident(_)) {
                source.dims.iter().try_fold(1i64, |n, d| {
                    n.checked_mul(crate::vector_subscripts::fixed_extent(d)?)
                })
            } else {
                None
            };
            known_length = known_length.and_then(|n| n.checked_add(known_item?));
            // A scalar object is stored with '%' component names, unlike an
            // element/section of a record array. Materialize scalar expressions
            // immediately, before evaluating another item with side effects.
            let scalar_object =
                matches!(&item,Expr::Ident(n) if objects.get(n).is_some_and(|o|o.dims.is_empty()));
            let scalar_expression = source.dims.is_empty()
                && !matches!(&item,
                Expr::Call(n,_)|Expr::Index(n,_) if objects.get(n).is_some_and(|o|!o.dims.is_empty()));
            if scalar_object || scalar_expression {
                self.serial += 1;
                let name = format!("@constructor_record_{}", self.serial);
                prefix.push(Stmt::DerivedDecl {
                    type_name: info.ty.clone(),
                    names: vec![name.clone()],
                    attributes: vec![],
                    polymorphic: false,
                    dims: vec![],
                });
                prefix.push(set(&name, item));
                objects.insert(
                    name.clone(),
                    Object {
                        ty: info.ty.clone(),
                        dims: vec![],
                    },
                );
                self.register_components(&name, &info.ty, objects);
                sources.push(Input::scalar(name, info.ty.clone()));
            } else {
                sources.push(self.pack_input(&item, source, objects, prefix)?);
            }
        }
        self.serial += 1;
        let name = format!("@record_constructor_{}", self.serial);
        let length = format!("{name}_length");
        let cursor = format!("{name}_cursor");
        let index = format!("{name}_index");
        prefix.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![length.clone(), cursor.clone(), index.clone()],
        });
        prefix.push(set(
            &length,
            sources
                .iter()
                .map(Input::length)
                .reduce(add)
                .unwrap_or(int(0)),
        ));
        let result_dims = known_length.map_or_else(|| deferred(1), |n| vec![int(n)]);
        for field in &fields {
            let storage = storage(&name, &field.name);
            let mut dims = field.dims.clone();
            dims.extend(result_dims.clone());
            prefix.push(Stmt::ArrayDecl {
                kind: field.kind.clone(),
                name: storage.clone(),
                dims: dims.clone(),
            });
            if known_length.is_none() {
                prefix.push(Stmt::DummyIntent {
                    name: storage.clone(),
                    intent: "allocatable".into(),
                });
                let mut allocated = component_allocation_dims(field, &storage);
                allocated.push(id(&length));
                prefix.push(Stmt::CallSub {
                    name: "@allocate".into(),
                    args: vec![Expr::Call(storage.clone(), allocated)],
                });
            }
            self.array_shapes.insert(storage, dims);
        }
        prefix.push(set(&cursor, int(0)));
        for source in sources {
            let mut body = vec![set(&cursor, add(id(&cursor), int(1)))];
            body.extend(self.pack_copy(
                &source,
                &fields,
                id(&index),
                &name,
                id(&cursor),
                prefix,
            )?);
            prefix.push(Stmt::Do {
                var: index.clone(),
                start: int(1),
                end: source.length(),
                body,
            });
        }
        objects.insert(
            name.clone(),
            Object {
                ty: info.ty,
                dims: result_dims,
            },
        );
        *value = id(&name);
        Ok(true)
    }
}

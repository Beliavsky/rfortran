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
                },
            );
        }
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
            self.array_shape(arg).is_some() || match arg {
                Expr::Ident(n) | Expr::Section { name: n, .. } =>
                    objects.get(n).is_some_and(|o| !o.dims.is_empty()),
                Expr::Call(n, _) | Expr::Index(n, _) =>
                    objects.get(n).is_some_and(|o| !o.dims.is_empty())
                        && self.element(arg, objects).is_none(),
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
            if self.types.get(ty).is_some_and(|fields| fields.iter().any(|f| !f.dims.is_empty() || f.heap.is_some())) {
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
            let (parts, _) = self.parts(arg.clone(), ty, objects, prefix)?;
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

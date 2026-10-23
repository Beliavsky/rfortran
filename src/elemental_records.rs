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
            if !array {
                continue;
            }
            let fields = self.fields(ty)?;
            if fields.iter().any(|f| !f.dims.is_empty()) {
                bail!(
                    "ELEMENTAL derived array actuals currently require scalar primitive components"
                );
            }
            let (parts, array) = self.parts(arg.clone(), ty, objects, prefix)?;
            if !array {
                bail!("ELEMENTAL record actual must be an array");
            }
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

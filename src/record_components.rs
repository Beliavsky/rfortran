// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Materialize record elements without flattening owned allocations into values.
use super::*;
impl Lower {
    pub(super) fn heap_component(&self, owner: &str, field: &str, objects: &Objects) -> bool {
        objects
            .get(owner)
            .filter(|o| !o.dims.is_empty())
            .is_some_and(|o| {
                self.types.get(&o.ty).is_some_and(|fields| {
                    fields.iter().any(|f| {
                        f.heap.is_some()
                            && (f.name == field
                                || f.name.starts_with(&format!("{field}$"))
                                || f.name.starts_with(&format!("{field}%")))
                    })
                })
            })
    }
    pub(super) fn heap_component_actual(
        &mut self,
        e: &mut Expr,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
        suffix: Option<&mut Vec<Stmt>>,
    ) -> Result<bool> {
        let Expr::Call(tag, args) = e else {
            return Ok(false);
        };
        let Some(field) = tag.strip_prefix("@record_component:").map(str::to_owned) else {
            return Ok(false);
        };
        let Some(element) = args.first() else {
            return Ok(false);
        };
        let Some((owner, _)) = self.element(element, objects) else {
            return Ok(false);
        };
        if !self.heap_component(owner, &field, objects) {
            return Ok(false);
        }
        let element = element.clone();
        let indices = component_subscripts(args);
        let temp = self.element_storage(&element, objects, prefix, suffix, true, Some(&field))?;
        *e = if indices.is_empty() {
            Expr::Ident(format!("{temp}%{field}"))
        } else {
            Expr::Call(format!("{temp}%{field}"), indices)
        };
        Ok(true)
    }
    pub(super) fn heap_component_assignment(
        &mut self,
        s: &mut Stmt,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
        suffix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let Stmt::AssignIndex {
            name,
            indices,
            value,
        } = s
        else {
            return Ok(());
        };
        if objects.get(name).is_some_and(|o| {
            !o.dims.is_empty()
                && self
                    .types
                    .get(&o.ty)
                    .is_some_and(|fields| fields.iter().any(|f| f.heap.is_some()))
        }) {
            let element = Expr::Call(name.clone(), indices.clone());
            if self.element(&element, objects).is_some() {
                let mut source = value.clone();
                if self.element(&source, objects).is_some() {
                    source = Expr::Ident(self.element_temp(&source, objects, prefix, None)?);
                }
                let target =
                    self.element_storage(&element, objects, prefix, Some(suffix), false, None)?;
                *s = Stmt::Assign {
                    name: target,
                    value: source,
                };
                return Ok(());
            }
        }
        let Some((owner, field)) = name.rsplit_once('%') else {
            return Ok(());
        };
        if !self.heap_component(owner, field, objects) {
            return Ok(());
        }
        let (owner, field) = (owner.to_owned(), field.to_owned());
        let mut parent = indices.clone();
        let component = component_subscripts(&mut parent);
        let element = Expr::Call(owner, parent);
        if self.element(&element, objects).is_none() {
            bail!("allocatable record components require scalar parent subscripts");
        }
        let temp = self.element_storage(&element, objects, prefix, Some(suffix), true, Some(&field))?;
        *s = if component.is_empty() {
            Stmt::Assign {
                name: format!("{temp}%{field}"),
                value: value.clone(),
            }
        } else {
            Stmt::AssignIndex {
                name: format!("{temp}%{field}"),
                indices: component,
                value: value.clone(),
            }
        };
        Ok(())
    }
}

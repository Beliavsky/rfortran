// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Record pointer arrays share shape through one primitive pointer per field.
//! Primitive descriptor lowering retains storage identity, strides and masks.
use super::*;

impl Lower {
    pub(super) fn pointer_fields(&self, ty: &str) -> Result<Vec<Field>> {
        let fields = self.fields(ty)?;
        if fields.iter().any(|f| !f.dims.is_empty() || f.heap.is_some()
            || !matches!(f.kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)
                | TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision
                | TypeSpec::Logical | TypeSpec::LogicalKind(_))) {
            bail!("derived pointer arrays currently require scalar numeric or LOGICAL components (including nested scalar records)");
        }
        Ok(fields)
    }

    fn owned_pointer_target(&mut self, target: &Expr, ty: &str, objects: &mut Objects,
        prefix: &mut Vec<Stmt>) -> Result<Option<Vec<Expr>>> {
        let Expr::Call(tag,args) = target else { return Ok(None) };
        let Some(component) = tag.strip_prefix("@record_component:") else { return Ok(None) };
        let Some(parent) = args.first() else { return Ok(None) };
        let Some((owner,indices)) = self.element(parent,objects) else { return Ok(None) };
        if !self.heap_component(owner,component,objects) { return Ok(None) }
        let root=owner.split('%').next().unwrap();
        if !self.target_roots.contains(root) {bail!("owned pointer target `{owner}` requires TARGET attribute on its parent object");}
        if args.len()!=1 { bail!("subscripts on owned pointer targets are not yet supported"); }
        let parent_type = &objects[owner].ty;
        let source = self.array_components.get(parent_type).into_iter().flatten()
            .find(|(name,_)| name==component).map(|(_,o)|o.clone())
            .context("owned pointer target requires a derived array component")?;
        if source.ty != ty { bail!("derived array type mismatch in owned pointer target"); }
        let (owner,mut indices) = (owner.to_owned(),indices.to_vec());
        self.capture_indices(&mut indices,prefix);
        let parent_fields = self.fields(parent_type)?;
        let mut values = vec![];
        for field in self.pointer_fields(ty)? {
            let path = format!("{component}${}",field.name.replace('%', "$"));
            let slot = parent_fields.iter().find(|f|f.name==path)
                .with_context(||format!("missing owned component `{path}`"))?;
            let (_,dims) = slot.heap.as_ref().context("owned pointer target requires a descriptor")?;
            self.serial+=1;
            let alias=format!("@owned_pointer_{}",self.serial);
            prefix.push(Stmt::ArrayDecl {kind:field.kind,name:alias.clone(),dims:deferred(dims.len())});
            prefix.push(Stmt::DummyIntent {name:alias.clone(),intent:"pointer".into()});
            prefix.push(Stmt::CallSub {name:"@record_pointer_associate".into(),args:vec![
                Expr::Ident(alias.clone()),select(&owner,slot,vec![],&indices)?,
                Expr::Str(format!("{owner}%{component}%{}",field.name)),
            ]});
            values.push(Expr::Ident(alias));
        }
        Ok(Some(values))
    }

    pub(super) fn pointer_statement(&mut self, s: &Stmt, objects: &mut Objects,
        prefix: &mut Vec<Stmt>) -> Result<bool> {
        let Stmt::CallSub { name, args } = s else { return Ok(false) };
        if name == "@pointer_associate" {
            let [Expr::Ident(pointer), target] = args.as_slice() else { return Ok(false) };
            // Borrow owned primitive components directly from their heap handle.
            // Materializing a scalar record view loses the parent's TARGET
            // attribute and needlessly loads unrelated component values.
            if let Expr::Call(tag,args)=target {
                if let Some(field)=tag.strip_prefix("@record_component:") {
                    if let Some((owner,indices))=args.first().and_then(|e|self.element(e,objects)) {
                        if let Some(f)=self.fields(&objects[owner].ty)?.into_iter().find(|f|f.name==field && f.heap.is_some()) {
                            if args.len()!=1 {bail!("subscripts on owned pointer targets are not yet supported");}
                            if !self.target_roots.contains(owner.split('%').next().unwrap()) {bail!("owned pointer target `{owner}` requires TARGET attribute on its parent object");}
                            let (kind,dims)=f.heap.as_ref().unwrap();
                            let Some(pdims)=self.array_shapes.get(pointer) else {bail!("owned component association requires an array POINTER");};
                            if self.intrinsic_kinds.get(pointer)!=Some(kind) || pdims.len()!=dims.len() {bail!("pointer and owned component must have matching type, kind and rank");}
                            let (owner,mut indices)=(owner.to_owned(),indices.to_vec());
                            self.capture_indices(&mut indices,prefix);
                            prefix.push(Stmt::CallSub{name:"@record_pointer_associate".into(),args:vec![Expr::Ident(pointer.clone()),select(&owner,&f,vec![],&indices)?,Expr::Str(format!("{owner}%{field}"))]});
                            return Ok(true);
                        }
                    }
                }
            }
            let Some(object) = objects.get(pointer).filter(|o| !o.dims.is_empty()).cloned() else { return Ok(false) };
            let fields = self.pointer_fields(&object.ty)?;
            let values = if matches!(target, Expr::Call(n,a) if n == "null" && a.is_empty()) {
                vec![target.clone(); fields.len()]
            } else {
                if let Some(parts) = self.owned_pointer_target(target, &object.ty, objects, prefix)? {
                    for (field,value) in fields.iter().zip(parts) {
                        prefix.push(Stmt::CallSub {name:name.clone(),args:vec![Expr::Ident(storage(pointer,&field.name)),value]});
                    }
                    return Ok(true);
                }
                let (parts, array) = self.parts(target.clone(), &object.ty, objects, prefix)?;
                if !array { bail!("derived pointer array association requires an array target"); }
                parts
            };
            for (field, value) in fields.iter().zip(values) {
                prefix.push(Stmt::CallSub { name:name.clone(), args:vec![
                    Expr::Ident(storage(pointer, &field.name)), value,
                ] });
            }
            return Ok(true);
        }
        if name == "@nullify" {
            let mut flat = vec![];
            let mut changed = false;
            for arg in args {
                if let Expr::Ident(pointer) = arg {
                    if let Some(object) = objects.get(pointer).filter(|o| !o.dims.is_empty()) {
                        flat.extend(self.pointer_fields(&object.ty)?.iter()
                            .map(|f| Expr::Ident(storage(pointer, &f.name))));
                        changed = true;
                        continue;
                    }
                }
                flat.push(arg.clone());
            }
            if changed { prefix.push(Stmt::CallSub {name:name.clone(),args:flat}); }
            return Ok(changed);
        }
        Ok(false)
    }

    pub(super) fn pointer_inquiry(&mut self, e: &mut Expr, objects: &mut Objects,
        prefix: &mut Vec<Stmt>) -> Result<bool> {
        let Expr::Call(name, args) = e else { return Ok(false) };
        if name != "associated" { return Ok(false) }
        crate::named_arguments::Signature {
            params:vec!["pointer".into(),"target".into()],
            optional:std::collections::HashSet::from(["target".into()]),
        }.reorder(name,args)?;
        let Some(Expr::Ident(pointer)) = args.first() else { return Ok(false) };
        let Some(object) = objects.get(pointer).filter(|o| !o.dims.is_empty()).cloned() else { return Ok(false) };
        let fields = self.pointer_fields(&object.ty)?;
        let targets = if let Some(target) = args.get(1).filter(|a| !crate::named_arguments::omitted(a)) {
            let (parts,array) = if let Some(parts) = self.owned_pointer_target(target, &object.ty, objects, prefix)? {
                (parts,true)
            } else { self.parts(target.clone(), &object.ty, objects, prefix)? };
            if !array { bail!("ASSOCIATED derived array target requires an array"); }
            Some(parts)
        } else { None };
        let mut checks = fields.iter().enumerate().map(|(i,field)| {
            let mut args = vec![Expr::Ident(storage(pointer,&field.name))];
            if let Some(targets) = &targets { args.push(targets[i].clone()); }
            Expr::Call("associated".into(),args)
        });
        let mut result = checks.next().context("derived pointer inquiry requires a component")?;
        for check in checks {
            result = Expr::Bin(crate::ast::BinOp::And,Box::new(result),Box::new(check));
        }
        *e = result;
        Ok(true)
    }
}

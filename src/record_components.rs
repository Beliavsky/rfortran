// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Materialize record elements without flattening owned allocations into values.
use super::*;
impl Lower {
    /// A function can change an owned descriptor just as a subroutine can.
    /// Evaluate its result before writing staged components back to the owner.
    pub(super) fn component_function_actuals(&mut self,e:&mut Expr,objects:&mut Objects,prefix:&mut Vec<Stmt>)->Result<bool> {
        let Expr::Call(name,args)=e else{return Ok(false)};
        let Some(mut declaration)=self.scalar_result_declarations.get(name).cloned() else{return Ok(false)};
        let mut suffix=vec![];
        for (i,actual) in args.iter_mut().enumerate() {
            if !self.readonly_actual(name,i,objects) {
                self.heap_component_actual(actual,objects,prefix,Some(&mut suffix))?;
            }
        }
        if suffix.is_empty() {return Ok(false);}
        // Recursion sees the materialized actuals rather than component views.
        self.expression(e,objects,prefix)?;
        self.serial+=1;
        let temp=format!("@component_function_result_{}",self.serial);
        match &mut declaration {
            Stmt::VarDecl{names,..}=>*names=vec![temp.clone()],
            Stmt::DerivedDecl{names,type_name,..}=>{
                *names=vec![temp.clone()];
                objects.insert(temp.clone(),Object{ty:type_name.clone(),dims:vec![]});
            }
            _=>unreachable!(),
        }
        prefix.push(declaration);
        prefix.push(Stmt::Assign{name:temp.clone(),value:e.clone()});
        prefix.extend(suffix);
        *e=Expr::Ident(temp);
        Ok(true)
    }
    fn fixed_record_component_name(&self,owner:&str,field:&str,objects:&Objects)->Option<String> {
        let fields=self.types.get(&objects.get(owner).filter(|o|!o.dims.is_empty())?.ty)?;
        let folded=field.replace('%',"$");
        if fields.iter().any(|f|!f.dims.is_empty() && f.name.contains('$') && f.name==folded) {return Some(folded);}
        fields.iter().any(|f|!f.dims.is_empty() && f.name.starts_with(&format!("{field}$"))).then(||field.into())
    }
    fn heap_component_name(&self,owner:&str,field:&str,objects:&Objects)->Option<String> {
        let fields=&self.types.get(&objects.get(owner).filter(|o|!o.dims.is_empty())?.ty)?;
        let folded=field.replace('%',"$");
        if fields.iter().any(|f|f.heap.is_some() && f.name==folded) {return Some(folded);}
        fields.iter().any(|f|f.heap.is_some() && (f.name==field || f.name.starts_with(&format!("{field}$")) || f.name.starts_with(&format!("{field}%")))).then(||field.into())
    }
    pub(super) fn transport_element_field(&mut self, owner:&str, temp:&str, field:&Field, parent_indices:&[Expr], copy_back:bool, out:&mut Vec<Stmt>)->Result<()> {
        let local=format!("{temp}%{}",field.name);
        let mut indices=vec![];
        for _ in &field.dims {
            self.serial+=1;
            let index=format!("@record_transport_index_{}",self.serial);
            out.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
            indices.push(Expr::Ident(index));
        }
        let source=select(owner,field,indices.clone(),parent_indices)?;
        let target=if indices.is_empty(){Expr::Ident(local.clone())}else{Expr::Call(local.clone(),indices.clone())};
        let mut body=vec![Stmt::CallSub{name:crate::initialized::TRANSPORT.into(),args:if copy_back{vec![source,target]}else{vec![target,source]}}];
        for (axis,index) in indices.into_iter().enumerate() {
            let Expr::Ident(var)=index else{unreachable!()};
            let bound=|name:&str|Expr::Call(name.into(),vec![Expr::Ident(local.clone()),Expr::IntLit((axis+1).to_string())]);
            body=vec![Stmt::Do{var,start:bound("lbound"),end:bound("ubound"),body}];
        }
        out.extend(body);Ok(())
    }
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
        let Some(field)=self.heap_component_name(owner,&field,objects)
            .or_else(||self.fixed_record_component_name(owner,&field,objects)) else{return Ok(false);};
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
        let Some((owner, field)) = name.match_indices('%').find_map(|(i, _)| {
            let (owner, field) = (&name[..i], &name[i + 1..]);
            self.heap_component_name(owner, field, objects)
                .or_else(||self.fixed_record_component_name(owner,field,objects)).map(|field|(owner, field))
        }) else {
            return Ok(());
        };
        let (owner, field) = (owner.to_owned(), field);
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

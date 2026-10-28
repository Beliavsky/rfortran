// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Insert source-level lifetime calls before record/pointer storage lowering.
//! Synthetic copies created by later passes are not Fortran entities to finalize.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

pub(crate) const OUTPUT: &str = "@final_intent_out";

#[derive(Clone)]
struct Object {
    ty: String,
    attrs: Vec<String>,
}
impl Object {
    fn has(&self, attr: &str) -> bool {
        self.attrs.iter().any(|a| a == attr)
    }
}
#[derive(Clone, Default)]
struct Definition {
    finals: Vec<(String, usize)>,
    components: Vec<(String, String, Vec<String>, usize)>,
    module: Option<String>,
}
struct Routine {
    params: Vec<String>,
    body: Vec<Stmt>,
    module: Option<String>,
    function: bool,
    elemental: bool,
}
type Objects = HashMap<String, Object>;
struct Lower {
    types: HashMap<String, Definition>,
    routines: HashMap<String, Routine>,
    finalizable: HashSet<String>,
    assignments: HashSet<String>,
    serial: usize,
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn call(n: &str, args: Vec<Expr>) -> Stmt {
    Stmt::CallSub {
        name: n.into(),
        args,
    }
}
fn guarded(condition: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::If {
        cond: condition,
        then_body: body,
        else_body: None,
    }
}
impl Lower {
    fn collect(&mut self, body: &[Stmt], module: Option<&str>) {
        for s in body {
            match s {
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    self.assignments
                        .extend(bindings.iter().filter_map(|(k, n)| {
                            (k == "@operator:=").then(|| {
                                bindings
                                    .iter()
                                    .find(|(binding, _)| binding == n)
                                    .map_or_else(|| n.clone(), |(_, target)| target.clone())
                            })
                        }));
                    self.types.insert(
                        name.clone(),
                        Definition {
                            finals: bindings
                                .iter()
                                .filter_map(|(k, n)| {
                                    k.strip_prefix("@final:")
                                        .and_then(|p| p.parse().ok())
                                        .map(|p| (n.clone(), p))
                                })
                                .collect(),
                            components: components
                                .iter()
                                .flat_map(|s| match s {
                                    Stmt::DerivedDecl {
                                        type_name,
                                        names,
                                        attributes,
                                        dims,
                                        ..
                                    } => names
                                        .iter()
                                        .map(|n| {
                                            (
                                                n.clone(),
                                                type_name.clone(),
                                                attributes.clone(),
                                                dims.len(),
                                            )
                                        })
                                        .collect(),
                                    _ => vec![],
                                })
                                .collect(),
                            module: module.map(str::to_owned),
                        },
                    );
                }
                Stmt::OperatorInterface {
                    operator,
                    procedures,
                    ..
                } if operator == "=" => self.assignments.extend(procedures.iter().cloned()),
                Stmt::Module { name, body } => self.collect(body, Some(name)),
                Stmt::Subroutine {
                    name,
                    params,
                    body,
                    elemental,
                    ..
                }
                | Stmt::Function {
                    name,
                    params,
                    body,
                    elemental,
                    ..
                } => {
                    self.routines.insert(
                        name.clone(),
                        Routine {
                            params: params.clone(),
                            body: body.clone(),
                            module: module.map(str::to_owned),
                            function: matches!(s, Stmt::Function { .. }),
                            elemental: *elemental,
                        },
                    );
                    self.collect(body, None);
                }
                _ => {
                    let mut s = s.clone();
                    for child in crate::procedure_arguments::children(&mut s) {
                        self.collect(child, module);
                    }
                }
            }
        }
    }
    fn validate(&mut self) -> Result<()> {
        for (ty, d) in &self.types {
            let mut seen = HashSet::new();
            for (name, pos) in &d.finals {
                let result = (|| -> Result<()> {
                    if !seen.insert(name) {
                        bail!("duplicate FINAL procedure `{name}` for `{ty}`");
                    }
                    let r = self.routines.get(name).with_context(|| {
                        format!("FINAL procedure `{name}` must be a module subroutine")
                    })?;
                    if d.module.is_none()
                        || r.module != d.module
                        || r.function
                        || r.params.len() != 1
                    {
                        bail!("FINAL procedure `{name}` must be a module subroutine with exactly one dummy argument");
                    }
                    let p = &r.params[0];
                    let decl = r
                        .body
                        .iter()
                        .find_map(|s| match s {
                            Stmt::DerivedDecl {
                                type_name,
                                names,
                                attributes,
                                polymorphic,
                                dims,
                            } if names.contains(p) => {
                                Some((type_name, attributes, polymorphic, dims))
                            }
                            _ => None,
                        })
                        .context("FINAL dummy must have the derived type being defined")?;
                    if decl.0 != ty
                        || *decl.2
                        || decl.1.iter().any(|a| {
                            matches!(
                                a.as_str(),
                                "optional" | "pointer" | "allocatable" | "value" | "intent(out)"
                            )
                        })
                    {
                        bail!("FINAL dummy must be nonoptional, nonpointer, nonallocatable and nonpolymorphic, of type `{ty}`, without VALUE or INTENT(OUT)");
                    }
                    if !decl.3.is_empty() || r.elemental {
                        bail!("array and ELEMENTAL FINAL procedures are not yet supported");
                    }
                    Ok(())
                })();
                result.map_err(|e| crate::source_diagnostics::at(e, Some(*pos)))?;
            }
            if d.finals.len() > 1 {
                return Err(crate::source_diagnostics::at(
                    anyhow::anyhow!(
                        "multiple scalar FINAL procedures for type `{ty}` have the same rank"
                    ),
                    Some(d.finals[1].1),
                ));
            }
            if !d.finals.is_empty() {
                self.finalizable.insert(ty.clone());
            }
        }
        loop {
            let added = self
                .types
                .iter()
                .filter(|(ty, d)| {
                    !self.finalizable.contains(*ty)
                        && d.components.iter().any(|(_, t, a, _)| {
                            !a.iter().any(|a| a == "pointer") && self.finalizable.contains(t)
                        })
                })
                .map(|(t, _)| t.clone())
                .collect::<Vec<_>>();
            if added.is_empty() {
                break;
            }
            self.finalizable.extend(added);
        }
        for d in self.types.values() {
            for (_, ty, attrs, rank) in &d.components {
                if self.finalizable.contains(ty)
                    && !attrs.iter().any(|a| a == "pointer")
                    && (*rank != 0 || attrs.iter().any(|a| a == "allocatable"))
                {
                    bail!("array and ALLOCATABLE components of finalizable type are not yet supported");
                }
            }
        }
        for name in &self.assignments {
            if self.routines.get(name).is_some_and(|r|r.body.iter().any(|s|matches!(s,Stmt::DerivedDecl{type_name,names,..} if self.finalizable.contains(type_name)&&names.iter().any(|n|r.params.contains(n))))) {
                bail!("defined assignment involving finalizable types is not yet supported");
            }
        }
        Ok(())
    }
    fn components(&self, name: &str, ty: &str, objects: &mut Objects) {
        for (field, t, attrs, rank) in &self.types[ty].components {
            if *rank == 0 && !attrs.iter().any(|a| a == "pointer" || a == "allocatable") {
                let name = format!("{name}%{field}");
                objects.insert(
                    name.clone(),
                    Object {
                        ty: t.clone(),
                        attrs: attrs.clone(),
                    },
                );
                if self.types.contains_key(t) {
                    self.components(&name, t, objects);
                }
            }
        }
    }
    fn finals(&self, name: &str, ty: &str) -> Vec<Stmt> {
        let d = &self.types[ty];
        let mut out = d
            .finals
            .iter()
            .flat_map(|(f, _)| {
                [
                    crate::imports::reference(f.clone()),
                    call(f, vec![id(name)]),
                ]
            })
            .collect::<Vec<_>>();
        for (c, t, attrs, _) in &d.components {
            if !attrs.iter().any(|a| a == "pointer") && self.finalizable.contains(t) {
                out.extend(self.finals(&format!("{name}%{c}"), t));
            }
        }
        out
    }
    fn finalize(&self, name: &str, o: &Object) -> Vec<Stmt> {
        let out = self.finals(name, &o.ty);
        if o.has("allocatable") {
            vec![guarded(Expr::Call("allocated".into(), vec![id(name)]), out)]
        } else {
            out
        }
    }
    fn cleanup(&self, live: &[(String, Object)]) -> Vec<Stmt> {
        let mut out = vec![];
        for (name, o) in live.iter().rev() {
            if o.has("allocatable") {
                let mut body = self.finals(name, &o.ty);
                body.push(call("@deallocate", vec![id(name)]));
                out.push(guarded(
                    Expr::Call("allocated".into(), vec![id(name)]),
                    body,
                ));
            } else {
                out.extend(self.finals(name, &o.ty));
            }
        }
        out
    }
    fn body(
        &mut self,
        body: &mut Vec<Stmt>,
        inherited: &Objects,
        params: &[String],
        scope: bool,
        inherited_live: &[(String, Object)],
        loop_boundary: usize,
    ) -> Result<()> {
        let mut objects = inherited.clone();
        for p in params {
            objects.remove(p);
        }
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        objects.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    objects.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        objects.remove(n);
                    }
                }
                _ => {}
            }
        }
        let mut locals = vec![];
        let mut entry = vec![];
        let mut pos = None;
        let save_all = body.iter().any(
            |s| matches!(s,Stmt::DummyIntent{name,..} if name==crate::data_statements::SAVE_ALL),
        );
        for s in body.iter() {
            if let Stmt::SourceLocation(p) = s {
                pos = Some(*p);
            }
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic,
                dims,
            } = s
            {
                for name in names {
                    let o = Object {
                        ty: type_name.clone(),
                        attrs: attributes.clone(),
                    };
                    objects.insert(name.clone(), o.clone());
                    if self.types.contains_key(type_name) {
                        self.components(name, type_name, &mut objects);
                    }
                    if !self.finalizable.contains(type_name) {
                        continue;
                    }
                    let result = (|| -> Result<()> {
                        if !dims.is_empty() || *polymorphic || o.has("pointer") {
                            bail!("finalization of arrays, polymorphic objects and POINTER objects is not yet supported");
                        }
                        if params.contains(name) && o.has("allocatable") {
                            bail!("scalar ALLOCATABLE dummies of finalizable type are not yet supported");
                        }
                        if params.contains(name) && o.has("intent(out)") {
                            let mut calls = self.finalize(name, &o);
                            if o.has("optional") {
                                calls = vec![guarded(
                                    Expr::Call("present".into(), vec![id(name)]),
                                    calls,
                                )];
                            }
                            entry.extend(calls);
                        } else if scope
                            && !params.contains(name)
                            && !o.has("save")
                            && !o.has("parameter")
                            && !save_all
                        {
                            locals.push((name.clone(), o));
                        }
                        Ok(())
                    })();
                    result.map_err(|e| crate::source_diagnostics::at(e, pos))?;
                }
            }
        }
        for s in body.iter_mut() {
            if let Stmt::DerivedDecl {
                type_name,
                attributes,
                ..
            } = s
            {
                if self.finalizable.contains(type_name)
                    && attributes.iter().any(|a| a == "intent(out)")
                {
                    attributes.push(OUTPUT.into());
                }
            }
        }
        let mut live = inherited_live.to_vec();
        live.extend(locals.clone());
        let original = std::mem::take(body);
        let mut out = entry;
        let mut offset = None;
        for (i, mut s) in original.iter().cloned().enumerate() {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(p);
            }
            (||->Result<()> {
                match &mut s {
                    Stmt::DerivedType{..}=>{},
                    Stmt::Module{body,..}=>self.body(body,&objects,&[],false,&[],0)?,
                    Stmt::Function{name,params,body,..}|Stmt::Subroutine{name,params,body,..}=>{
                        if matches!(&original[i],Stmt::Function{..}) {
                            let result=body.last().and_then(|s|if let Stmt::Return(Some(Expr::Ident(n)))=s{Some(n)}else{None}).unwrap_or(name);
                            if body.iter().any(|s|matches!(s,Stmt::DerivedDecl{type_name,names,..} if names.contains(result)&&self.finalizable.contains(type_name))) {bail!("function results of finalizable type are not yet supported");}
                        }
                        self.body(body,&objects,params,true,&[],0)?;
                    }
                    Stmt::Block{body}=>self.body(body,&objects,params,true,&live,loop_boundary)?,
                    Stmt::If{then_body,else_body,..}=>{
                        self.body(then_body,&objects,params,false,&live,loop_boundary)?;
                        if let Some(b)=else_body{self.body(b,&objects,params,false,&live,loop_boundary)?;}
                    }
                    Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>self.body(body,&objects,params,false,&live,live.len())?,
                    Stmt::SelectCase{cases,default,..}=>{
                        for c in cases{self.body(&mut c.body,&objects,params,false,&live,loop_boundary)?;}
                        if let Some(b)=default{self.body(b,&objects,params,false,&live,loop_boundary)?;}
                    }
                    Stmt::Return(_)=>out.extend(self.cleanup(&live)),
                    Stmt::Exit|Stmt::Cycle=>out.extend(self.cleanup(&live[loop_boundary..])),
                    Stmt::CallSub{name,..} if matches!(name.as_str(),"@named_exit"|"@named_cycle"|"@goto") && !live.is_empty()=>bail!("named EXIT/CYCLE or GOTO with finalizable locals is not yet supported"),
                    Stmt::CallSub{name,args} if name=="@deallocate"=>{
                        let targets=args.iter().filter_map(|e|if let Expr::Ident(n)=e{objects.get(n).filter(|o|self.finalizable.contains(&o.ty)).map(|o|(n,o))}else{None}).collect::<Vec<_>>();
                        if !targets.is_empty() && args.iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@keyword:"))) {bail!("DEALLOCATE STAT/ERRMSG with finalizable objects is not yet supported");}
                        for (n,o) in targets {
                            out.extend(self.finalize(n,o));
                        }
                    }
                    Stmt::CallSub{name,args} if name=="move_alloc" && !self.routines.contains_key(name)=>{
                        crate::named_arguments::Signature{params:vec!["from".into(),"to".into(),"stat".into(),"errmsg".into()],optional:["stat".into(),"errmsg".into()].into_iter().collect()}.reorder(name,args)?;
                        if let Some(Expr::Ident(n))=args.get(1) {if let Some(o)=objects.get(n).filter(|o|self.finalizable.contains(&o.ty)){out.extend(self.finalize(n,o));}}
                    }
                    Stmt::VarDecl{kind:crate::ast::TypeSpec::Referenced{source,..},..}
                    | Stmt::ArrayDecl{kind:crate::ast::TypeSpec::Referenced{source,..},..}
                        if matches!(source.as_ref(),Expr::Ident(n) if objects.get(n).is_some_and(|o|self.finalizable.contains(&o.ty)))=>{
                        bail!("TYPEOF/CLASSOF of finalizable objects is not yet supported");
                    }
                    Stmt::Assign{name,value} if objects.get(name).is_some_and(|o|self.finalizable.contains(&o.ty))=>{
                        // Declaration initialization is not intrinsic assignment.
                        let initialization=i>0 && matches!(&original[i-1],Stmt::DerivedDecl{names,attributes,..} if names.contains(name)&&attributes.iter().any(|a|a=="save"));
                        if !initialization {
                            let o=&objects[name];self.serial+=1;
                            let temp=format!("@final_rhs_{}",self.serial);
                            out.push(Stmt::DerivedDecl{type_name:o.ty.clone(),names:vec![temp.clone()],attributes:vec![],polymorphic:false,dims:vec![]});
                            out.push(Stmt::Assign{name:temp.clone(),value:value.clone()});
                            out.extend(self.finalize(name,o));
                            *value=id(&temp);
                        }
                    }
                    _=>{}
                }
                Ok(())
            })().map_err(|e|crate::source_diagnostics::at(e,offset))?;
            out.push(s);
        }
        if !out.last().is_some_and(terminated) {
            out.extend(self.cleanup(&locals));
        }
        *body = out;
        Ok(())
    }
}
fn terminated(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) | Stmt::Exit | Stmt::Cycle => true,
        Stmt::Block { body } => body.last().is_some_and(terminated),
        Stmt::CallSub { name, .. } => matches!(name.as_str(), "@stop" | "@error_stop"),
        Stmt::If {
            then_body,
            else_body: Some(b),
            ..
        } => then_body.last().is_some_and(terminated) && b.last().is_some_and(terminated),
        _ => false,
    }
}
pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    fn contains(body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::DerivedType { bindings, .. } => {
                bindings.iter().any(|(n, _)| n.starts_with("@final:"))
            }
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => contains(body),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => contains(then_body) || else_body.as_ref().is_some_and(|b| contains(b)),
            Stmt::SelectCase { cases, default, .. } => {
                cases.iter().any(|c| contains(&c.body))
                    || default.as_ref().is_some_and(|b| contains(b))
            }
            _ => false,
        })
    }
    if !contains(&program.body) {
        return Ok(program);
    }
    let mut lower = Lower {
        types: HashMap::new(),
        routines: HashMap::new(),
        finalizable: HashSet::new(),
        assignments: HashSet::new(),
        serial: 0,
    };
    lower.collect(&program.body, None);
    lower.validate()?;
    if lower.finalizable.is_empty() {
        return Ok(program);
    }
    // Parent finalizers must not be inherited as ordinary bindings.
    fn extensions(body: &mut [Stmt], finalizable: &HashSet<String>) -> Result<()> {
        for s in body {
            if matches!(s,Stmt::CallSub{name,args} if name==crate::extensions::EXTENDS && args.iter().any(|e|matches!(e,Expr::Ident(n) if finalizable.contains(n))))
            {
                bail!("type extension involving finalizable types is not yet supported");
            }
            for child in crate::procedure_arguments::children(s) {
                extensions(child, finalizable)?;
            }
        }
        Ok(())
    }
    extensions(&mut program.body, &lower.finalizable)?;
    lower.body(&mut program.body, &Objects::new(), &[], false, &[], 0)?;
    Ok(program)
}

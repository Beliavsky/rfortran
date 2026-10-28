// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Insert source-level lifetime calls before record/pointer storage lowering.
//! Synthetic copies created by later passes are not Fortran entities to finalize.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
#[path = "finalization_components.rs"]
mod owned_components;

pub(crate) const OUTPUT: &str = "@final_intent_out";
pub(crate) const ARRAY_ASSIGNMENT: &str = "@final_array_assignment";
pub(crate) fn array_helper(name: &str) -> bool {
    name.strip_prefix("@finalize_array_")
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Clone)]
struct Object {
    ty: String,
    attrs: Vec<String>,
    dims: Vec<Expr>,
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
    component_dims: HashMap<String, Vec<Expr>>,
    module: Option<String>,
}
struct Routine {
    params: Vec<String>,
    body: Vec<Stmt>,
    module: Option<String>,
    function: bool,
    elemental: bool,
    pure: bool,
}
type Objects = HashMap<String, Object>;
struct Lower {
    types: HashMap<String, Definition>,
    routines: HashMap<String, Routine>,
    finalizable: HashSet<String>,
    assignments: HashSet<String>,
    serial: usize,
    ranks: HashMap<String, usize>,
    helpers: HashMap<(String, usize), String>,
    generated: Vec<Stmt>,
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
                                        polymorphic,
                                        ..
                                    } => names
                                        .iter()
                                        .map(|n| {
                                            (
                                                n.clone(),
                                                type_name.clone(),
                                                attributes.iter().cloned().chain(polymorphic.then(||"@polymorphic".into())).collect(),
                                                dims.len(),
                                            )
                                        })
                                        .collect(),
                                    _ => vec![],
                                })
                                .collect(),
                            component_dims: components.iter().flat_map(|s| match s {
                                Stmt::DerivedDecl { names, dims, .. } => names.iter().map(|n| (n.clone(), dims.clone())).collect(),
                                _ => vec![],
                            }).collect(),
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
                    pure,
                    ..
                }
                | Stmt::Function {
                    name,
                    params,
                    body,
                    elemental,
                    pure,
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
                            pure: *pure,
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
            let mut ranks = HashSet::new();
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
                    if decl.3.len() > 15 {
                        bail!("FINAL dummy rank must not exceed 15");
                    }
                    if r.elemental && !decl.3.is_empty() {
                        bail!("ELEMENTAL FINAL dummy must be scalar");
                    }
                    if !ranks.insert(decl.3.len()) {
                        bail!(
                            "multiple FINAL procedures for type `{ty}` have the same rank {}",
                            decl.3.len()
                        );
                    }
                    self.ranks.insert(name.clone(), decl.3.len());
                    Ok(())
                })();
                result.map_err(|e| crate::source_diagnostics::at(e, Some(*pos)))?;
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
            for (_, ty, attrs, _) in &d.components {
                if self.finalizable.contains(ty) && attrs.iter().any(|a|a=="@polymorphic") && !attrs.iter().any(|a|a=="pointer") {bail!("finalization of polymorphic components is not yet supported");}
            }
        }
        fn acyclic(ty:&str,types:&HashMap<String,Definition>,active:&mut HashSet<String>)->Result<()> {
            if !active.insert(ty.into()) {bail!("recursive owned layouts involving finalizable types are not yet supported");}
            if let Some(d)=types.get(ty) {for (_,t,a,_) in &d.components {if !a.iter().any(|a|a=="pointer") {acyclic(t,types,active)?;}}}
            active.remove(ty);Ok(())
        }
        for ty in &self.finalizable {acyclic(ty,&self.types,&mut HashSet::new())?;}
        for name in &self.assignments {
            if self.routines.get(name).is_some_and(|r|r.body.iter().any(|s|matches!(s,Stmt::DerivedDecl{type_name,names,..} if self.finalizable.contains(type_name)&&names.iter().any(|n|r.params.contains(n))))) {
                bail!("defined assignment involving finalizable types is not yet supported");
            }
        }
        Ok(())
    }
    fn components(&self, name: &str, ty: &str, dims: &[Expr], objects: &mut Objects) {
        for (field, t, attrs, rank) in &self.types[ty].components {
            if !attrs.iter().any(|a| a == "pointer") {
                let name = format!("{name}%{field}");
                let component_dims = if attrs.iter().any(|a|a=="allocatable") {
                    vec![id("@deferred_extent");*rank]
                } else if *rank != 0 {
                    self.types[ty].component_dims[field].clone()
                } else {dims.to_vec()};
                objects.insert(
                    name.clone(),
                    Object {
                        ty: t.clone(),
                        attrs: attrs.clone(),
                        dims: component_dims.clone(),
                    },
                );
                if self.types.contains_key(t) && !attrs.iter().any(|a|a=="allocatable") {
                    self.components(&name, t, &component_dims, objects);
                }
            }
        }
    }
    fn selected(&self, ty: &str, rank: usize) -> Option<String> {
        self.types[ty]
            .finals
            .iter()
            .find(|(f, _)| self.ranks.get(f) == Some(&rank))
            .or_else(|| {
                self.types[ty]
                    .finals
                    .iter()
                    .find(|(f, _)| self.routines[f].elemental)
            })
            .map(|(f, _)| f.clone())
    }
    fn pure(&self, ty: &str, rank: usize) -> bool {
        self.selected(ty, rank)
            .is_none_or(|f| self.routines[&f].pure)
            && self.components_pure(ty)
    }
    fn components_pure(&self, ty:&str)->bool {
        self.types[ty]
                .components
                .iter()
                .filter(|(_, t, a, _)| {
                    !a.iter().any(|a| a == "pointer") && self.finalizable.contains(t)
                })
                .all(|(_, t, _, rank)| self.pure(t, *rank))
    }
    fn member(target: Expr, field: &str) -> Expr {
        match target {
            Expr::Ident(n) => id(&format!("{n}%{field}")),
            e => Expr::Call(format!("@record_component:{field}"), vec![e]),
        }
    }
    fn component_calls(&mut self, target:Expr, ty:&str)->Vec<Stmt> {
        let mut out=vec![];
        // Nonallocatable components precede deallocation of owned components.
        let components=self.types[ty].components.clone();
        for owned in [false,true] {
            for (c,t,attrs,rank) in &components {
                if attrs.iter().any(|a|a=="pointer") || !self.finalizable.contains(t) || attrs.iter().any(|a|a=="allocatable")!=owned {continue;}
                let component=Self::member(target.clone(),c);
                let mut calls=if *rank==0 && !owned {self.scalar_calls(component.clone(),t)} else {
                    let helper=self.helper(t,*rank);vec![call(&helper,vec![component.clone()])]
                };
                if owned {
                    calls.push(call("@deallocate",vec![component.clone()]));
                    out.push(guarded(Expr::Call("allocated".into(),vec![component]),calls));
                } else {out.extend(calls);}
            }
        }
        out
    }
    fn scalar_calls(&mut self, target: Expr, ty: &str) -> Vec<Stmt> {
        let mut out = vec![];
        if let Some(f) = self.selected(ty, 0) {
            out.push(crate::imports::reference(f.clone()));
            out.push(call(&f, vec![target.clone()]));
        }
        out.extend(self.component_calls(target,ty));
        out
    }
    fn helper(&mut self, ty: &str, rank: usize) -> String {
        let key = (ty.to_owned(), rank);
        if let Some(name) = self.helpers.get(&key) {
            return name.clone();
        }
        self.serial += 1;
        let name = format!("@finalize_array_{}", self.serial);
        self.helpers.insert(key, name.clone());
        let param = "@final_object";
        let mut body = vec![
            crate::imports::reference(ty.into()),
            Stmt::DerivedDecl {
                type_name: ty.into(),
                names: vec![param.into()],
                attributes: vec!["intent(inout)".into()],
                polymorphic: false,
                dims: vec![id("@deferred_extent"); rank],
            },
        ];
        if rank == 0 {
            body.extend(self.scalar_calls(id(param), ty));
        } else {
            let vars = (0..rank)
                .map(|axis| format!("@final_index_{axis}"))
                .collect::<Vec<_>>();
            for var in &vars {
                body.push(Stmt::VarDecl {
                    kind: crate::ast::TypeSpec::Integer(Some(8)),
                    names: vec![var.clone()],
                });
            }
            let element = Expr::Call(param.into(), vars.iter().map(|n| id(n)).collect());
            let loops = |mut inner: Vec<Stmt>| {
                for (axis, var) in vars.iter().enumerate() {
                    inner = vec![Stmt::Do {
                        var: var.clone(),
                        start: Expr::IntLit("1".into()),
                        end: Expr::Call(
                            "size".into(),
                            vec![id(param), Expr::IntLit((axis + 1).to_string())],
                        ),
                        body: inner,
                    }];
                }
                inner
            };
            if let Some(f) = self.selected(ty, rank) {
                body.push(crate::imports::reference(f.clone()));
                if self.ranks[&f] == rank {
                    body.push(call(&f, vec![id(param)]));
                } else {
                    body.extend(loops(vec![call(&f, vec![element.clone()])]));
                }
            }
            // Materialize component arrays once per scalar parent, preserving
            // their own rank and any owned descriptors across child FINAL.
            let components = if self.types[ty].components.iter().any(|(_,t,a,r)|(*r != 0 || a.iter().any(|a|a=="allocatable"))&&self.finalizable.contains(t)) {
                let f=self.component_helper(ty);vec![call(&f,vec![element.clone()])]
            } else {self.component_calls(element.clone(),ty)};
            if !components.is_empty() {
                body.extend(loops(components));
            }
        }
        self.generated.push(Stmt::Subroutine {
            name: name.clone(),
            params: vec![param.into()],
            body,
            pure: self.pure(ty, rank),
            elemental: false,
        });
        name
    }
    fn component_helper(&mut self,ty:&str)->String {
        let key=(ty.to_owned(),16);
        if let Some(n)=self.helpers.get(&key) {return n.clone();}
        self.serial+=1;
        let name=format!("@finalize_array_{}",self.serial);
        self.helpers.insert(key,name.clone());
        let param="@final_component_owner";
        let mut body=vec![crate::imports::reference(ty.into()),Stmt::DerivedDecl{type_name:ty.into(),names:vec![param.into()],attributes:vec!["intent(inout)".into()],polymorphic:false,dims:vec![]}];
        body.extend(self.component_calls(id(param),ty));
        self.generated.push(Stmt::Subroutine{name:name.clone(),params:vec![param.into()],body,pure:self.components_pure(ty),elemental:false});
        name
    }
    fn finals(&mut self, name: &str, ty: &str, rank: usize) -> Vec<Stmt> {
        if rank == 0 {
            self.scalar_calls(id(name), ty)
        } else {
            let f = self.helper(ty, rank);
            vec![call(&f, vec![id(name)])]
        }
    }
    fn finalize(&mut self, name: &str, o: &Object) -> Vec<Stmt> {
        let out = self.finals(name, &o.ty, o.dims.len());
        if o.has("allocatable") {
            vec![guarded(Expr::Call("allocated".into(), vec![id(name)]), out)]
        } else {
            out
        }
    }
    fn cleanup(&mut self, live: &[(String, Object)]) -> Vec<Stmt> {
        let mut out = vec![];
        for (name, o) in live.iter().rev() {
            if o.has("allocatable") {
                let mut body = self.finals(name, &o.ty, o.dims.len());
                body.push(call("@deallocate", vec![id(name)]));
                out.push(guarded(
                    Expr::Call("allocated".into(), vec![id(name)]),
                    body,
                ));
            } else {
                out.extend(self.finals(name, &o.ty, o.dims.len()));
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
                        dims: dims.clone(),
                    };
                    objects.insert(name.clone(), o.clone());
                    if self.types.contains_key(type_name) {
                        self.components(name, type_name, dims, &mut objects);
                    }
                    if !self.finalizable.contains(type_name) {
                        continue;
                    }
                    let result = (|| -> Result<()> {
                        if *polymorphic || o.has("pointer") {
                            bail!("finalization of polymorphic objects and POINTER objects is not yet supported");
                        }
                        if params.contains(name) && dims.is_empty() && o.has("allocatable") {
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
                        if let Some(Expr::Ident(n))=args.get(1) {if let Some(o)=objects.get(n).filter(|o|self.finalizable.contains(&o.ty)){
                            out.extend(self.finalize(n,o));
                            if !o.dims.is_empty() || n.contains('%') {
                                let Some(Expr::Ident(from))=args.first() else{bail!("MOVE_ALLOC requires ALLOCATABLE variables")};
                                if from==n {bail!("MOVE_ALLOC source and destination must differ");}
                                if !o.has("allocatable") || objects.get(from).is_none_or(|a|!a.has("allocatable")||a.ty!=o.ty||a.dims.len()!=o.dims.len()) {bail!("MOVE_ALLOC requires matching ALLOCATABLE type and rank");}
                                if args.iter().skip(2).any(|e|!crate::named_arguments::omitted(e)) {bail!("MOVE_ALLOC STAT/ERRMSG for finalizable arrays is not yet supported");}
                                s=Stmt::Block{body:vec![guarded(Expr::Call("allocated".into(),vec![id(n)]),vec![call("@deallocate",vec![id(n)])]),call("@move_allocation_storage",vec![id(n),id(from)])]};
                            }
                        }}
                    }
                    Stmt::VarDecl{kind:crate::ast::TypeSpec::Referenced{source,..},..}
                    | Stmt::ArrayDecl{kind:crate::ast::TypeSpec::Referenced{source,..},..}
                        if matches!(source.as_ref(),Expr::Ident(n) if objects.get(n).is_some_and(|o|self.finalizable.contains(&o.ty)))=>{
                        bail!("TYPEOF/CLASSOF of finalizable objects is not yet supported");
                    }
                    Stmt::Assign{name,value} if objects.get(name).is_some_and(|o|self.finalizable.contains(&o.ty))=>{
                        if !objects[name].dims.is_empty() {
                            let f=self.helper(&objects[name].ty,objects[name].dims.len());
                            s=call(ARRAY_ASSIGNMENT,vec![Expr::Str(f),id(name),value.clone(),Expr::Logical(objects[name].has("allocatable"))]);
                            return Ok(());
                        }
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
                    Stmt::AssignIndex{name,indices,value} if objects.get(name).is_some_and(|o|self.finalizable.contains(&o.ty))=>{
                        // Vector subscripts acquire their rank in record-array
                        // lowering. Supply each possible rank without guessing
                        // whether an INTEGER actual is scalar or an array.
                        let fs=(0..=objects[name].dims.len()).map(|rank|Expr::Str(self.helper(&objects[name].ty,rank))).collect();
                        s=call(ARRAY_ASSIGNMENT,vec![Expr::Call("@final_helpers".into(),fs),Expr::Call(name.clone(),indices.clone()),value.clone(),Expr::Logical(false)]);
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
        ranks: HashMap::new(),
        helpers: HashMap::new(),
        generated: vec![],
    };
    lower.collect(&program.body, None);
    lower.validate()?;
    if lower.finalizable.is_empty() {
        return Ok(program);
    }
    // A nonfinal nested type still contributes owned allocation state to a
    // finalizable parent's assignment snapshots and finalizer arguments.
    let mut descriptor_types = lower.finalizable.clone();
    loop {
        let nested = descriptor_types.iter().filter_map(|t|lower.types.get(t))
            .flat_map(|d|d.components.iter())
            .filter(|(_,_,attrs,_)|!attrs.iter().any(|a|a=="pointer"))
            .map(|(_,t,_,_)|t.clone()).filter(|t|!descriptor_types.contains(t))
            .collect::<Vec<_>>();
        if nested.is_empty() {break;}
        descriptor_types.extend(nested);
    }
    // Parent finalizers must not be inherited as ordinary bindings.
    fn extensions(body: &mut [Stmt], finalizable: &HashSet<String>, descriptor_types: &HashSet<String>) -> Result<()> {
        for s in body {
            if let Stmt::DerivedType { name, components, .. } = s {
                if descriptor_types.contains(name) {
                    // FINAL and staged intrinsic assignment must preserve the
                    // allocation state even for a component never allocated in
                    // this scope. Use descriptors for scalar record fields too.
                    let owned = components.iter().filter_map(|s| match s {
                        Stmt::DummyIntent { name, intent } if intent == "allocatable" => Some(name.clone()),
                        _ => None,
                    }).collect::<Vec<_>>();
                    for name in owned {
                        if !components.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==&name && intent=="@scalar_heap")) {
                            components.push(Stmt::DummyIntent {name,intent:"@scalar_heap".into()});
                        }
                    }
                }
            }
            if matches!(s,Stmt::CallSub{name,args} if name==crate::extensions::EXTENDS && args.iter().any(|e|matches!(e,Expr::Ident(n) if finalizable.contains(n))))
            {
                bail!("type extension involving finalizable types is not yet supported");
            }
            for child in crate::procedure_arguments::children(s) {
                extensions(child, finalizable, descriptor_types)?;
            }
        }
        Ok(())
    }
    extensions(&mut program.body, &lower.finalizable, &descriptor_types)?;
    lower.body(&mut program.body, &Objects::new(), &[], false, &[], 0)?;
    program.body.extend(lower.generated);
    owned_components::normalize(&mut program, &lower.types)?;
    Ok(program)
}

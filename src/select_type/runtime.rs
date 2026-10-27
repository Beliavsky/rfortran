// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Runtime type tags and owned, one-element concrete allocation descriptors.
//! Runtime branches call ordinary CLASS specializations; concrete storage is
//! allocated only for the selected type, and is released through record-array
//! allocation lowering (including owned components).
use super::*;
use crate::ast::{BinOp, CaseBlock, TypeSpec};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Object {
    kind: String,
    polymorphic: bool,
    tag: String,
    variants: Vec<(String, String)>,
}
type Runtime = BTreeMap<String, Object>;
type Declarations = BTreeMap<String, Vec<Stmt>>;
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn set(n: &str, v: usize) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: int(v),
    }
}
fn failure(message: String) -> Vec<Stmt> {
    vec![Stmt::CallSub {
        name: "@error_stop".into(),
        args: vec![Expr::Str(message)],
    }]
}
fn cases(object: &Object, bodies: Vec<Vec<Stmt>>, default: Vec<Stmt>) -> Stmt {
    Stmt::SelectCase {
        expr: id(&object.tag),
        cases: bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| CaseBlock {
                items: vec![CaseItem::Single(int(i + 1))],
                body,
            })
            .collect(),
        default: Some(default),
    }
}
fn declarations(body: &[Stmt], parent: &Declarations) -> Declarations {
    let mut out = parent.clone();
    for s in body {
        let entries: Vec<(String, Stmt)> = match s {
            Stmt::VarDecl { kind, names } => names
                .iter()
                .map(|n| {
                    (
                        n.clone(),
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![n.clone()],
                        },
                    )
                })
                .collect(),
            Stmt::ArrayDecl { name, .. } => vec![(name.clone(), s.clone())],
            Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic,
                dims,
            } => names
                .iter()
                .map(|n| {
                    (
                        n.clone(),
                        Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![n.clone()],
                            attributes: attributes.clone(),
                            polymorphic: *polymorphic,
                            dims: dims.clone(),
                        },
                    )
                })
                .collect(),
            Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } => entities
                .iter()
                .map(|(n, v)| {
                    let d = if attributes.iter().any(|a| a == "parameter") {
                        Stmt::InitializedDecl {
                            kind: kind.clone(),
                            attributes: attributes.clone(),
                            entities: vec![(n.clone(), v.clone())],
                        }
                    } else {
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![n.clone()],
                        }
                    };
                    (n.clone(), d)
                })
                .collect(),
            _ => vec![],
        };
        for (n, d) in entries {
            out.insert(n, vec![d]);
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(d) = out.get_mut(name) {
                if !intent.starts_with('@') && !matches!(intent.as_str(), "save" | "parameter") {
                    d.push(s.clone());
                }
            }
        }
    }
    out
}
fn uses(s: &Stmt, n: &str) -> bool {
    let matches = |v: &str| v == n || v.strip_prefix(n).is_some_and(|r| r.starts_with('%'));
    let mut s = s.clone();
    if matches!(&s,Stmt::Assign{name,..}|Stmt::AssignIndex{name,..}|Stmt::CallSub{name,..}|Stmt::Do{var:name,..} if matches(name))
    {
        return true;
    }
    let mut found = false;
    for e in expressions(&mut s) {
        visit_expr(e, &mut |e| {
            if matches!(e,Expr::Ident(v)|Expr::Call(v,_)|Expr::Index(v,_)|Expr::Section{name:v,..} if matches(v))
            {
                found = true;
            }
        });
    }
    found
        || children(&mut s)
            .into_iter()
            .any(|b| b.iter().any(|s| uses(s, n)))
}
struct Lower<'a> {
    defs: &'a Definitions,
    generated: Vec<Stmt>,
    serial: usize,
}
impl Lower<'_> {
    fn scope(
        &mut self,
        body: &mut Vec<Stmt>,
        parent: &Declarations,
        params: &[String],
        pure: bool,
    ) -> Result<()> {
        let declared = declarations(body, parent);
        let vars = super::objects(body, &Objects::new());
        let mut runtime = Runtime::new();
        for (n, v) in &vars {
            if !v.polymorphic || !v.attrs.iter().any(|a| a == "allocatable") {
                continue;
            }
            if v.rank != 0 {
                bail!("runtime polymorphic ALLOCATABLE arrays are not yet supported");
            }
            if v.attrs.iter().any(|a| a == "save" || a == "pointer") {
                bail!("SAVED or POINTER runtime polymorphic objects are not yet supported");
            }
            if params.contains(n) {
                bail!("runtime polymorphic ALLOCATABLE dummy arguments are not yet supported");
            }
            let mut kinds: Vec<_> = self
                .defs
                .fields
                .keys()
                .filter(|k| {
                    !self.defs.abstracts.contains(*k) && self.defs.distance(k, &v.kind).is_some()
                })
                .cloned()
                .collect();
            kinds.sort();
            self.serial += 1;
            let stem = format!("@polymorphic{}", self.serial);
            runtime.insert(
                n.clone(),
                Object {
                    kind: v.kind.clone(),
                    polymorphic: true,
                    tag: format!("{stem}$tag"),
                    variants: kinds
                        .into_iter()
                        .enumerate()
                        .map(|(i, k)| (k, format!("{stem}$storage{i}")))
                        .collect(),
                },
            );
        }
        // Procedures have independent allocation lifetimes. Module objects need
        // persistent descriptors and are intentionally diagnosed for now.
        for s in body.iter_mut() {
            match s {
                Stmt::Function {
                    body, params, pure, ..
                }
                | Stmt::Subroutine {
                    body, params, pure, ..
                } => self.scope(body, &declared, params, *pure)?,
                Stmt::Module { body, .. } => {
                    if super::objects(body, &Objects::new())
                        .values()
                        .any(|v| v.polymorphic && v.attrs.iter().any(|a| a == "allocatable"))
                    {
                        bail!("module polymorphic ALLOCATABLE objects are not yet supported");
                    }
                    self.scope(body, &declared, &[], false)?;
                }
                _ => {}
            }
        }
        if runtime.is_empty() {
            return Ok(());
        }
        let mut cleaned = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::DerivedDecl { names, .. } => {
                    names.retain(|n| !runtime.contains_key(n));
                    if names.is_empty() {
                        continue;
                    }
                }
                Stmt::DummyIntent { name, .. } if runtime.contains_key(name) => continue,
                _ => {}
            }
            cleaned.push(s);
        }
        let mut storage = vec![];
        for object in runtime.values() {
            storage.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: vec![object.tag.clone()],
            });
            for (kind, name) in &object.variants {
                storage.push(Stmt::DerivedDecl {
                    type_name: kind.clone(),
                    names: vec![name.clone()],
                    attributes: vec!["allocatable".into(), "@scalar_heap".into()],
                    polymorphic: false,
                    dims: vec![id("@deferred_extent")],
                });
            }
        }
        let declared = declarations(&storage, &declared);
        // Keep every specification statement ahead of generated executable
        // initialization. Automatic-array lowering inserts entry allocations
        // at this boundary and needs the storage declarations first.
        let at = cleaned
            .iter()
            .position(|s| {
                !matches!(
                    s,
                    Stmt::GenericInterface { .. }
                        | Stmt::OperatorInterface { .. }
                        | Stmt::DerivedType { .. }
                        | Stmt::DerivedDecl { .. }
                        | Stmt::Accessibility { .. }
                        | Stmt::DummyIntent { .. }
                        | Stmt::VarDecl { .. }
                        | Stmt::InitializedDecl { .. }
                        | Stmt::ArrayDecl { .. }
                        | Stmt::Use { .. }
                        | Stmt::Implicit(_)
                        | Stmt::SourceLocation(_)
                        | Stmt::Module { .. }
                        | Stmt::Function { .. }
                        | Stmt::Subroutine { .. }
                ) && !matches!(s,Stmt::CallSub{name,..} if name=="@extends")
            })
            .unwrap_or(cleaned.len());
        cleaned.splice(at..at, runtime.values().map(|o| set(&o.tag, 0)));
        self.body(&mut cleaned, &runtime, &declared, pure)?;
        storage.extend(cleaned);
        // Record-array lowering releases automatic allocation descriptors on
        // procedure exit, just as it does for ordinary local allocatables.
        *body = storage;
        Ok(())
    }
    fn transfer(
        &self,
        to: &str,
        object: &Object,
        kind: &str,
        value: Option<Expr>,
        assignment: bool,
    ) -> Result<Vec<Stmt>> {
        if self.defs.abstracts.contains(kind) {
            bail!("cannot allocate ABSTRACT type `{kind}`");
        }
        let Some((index, (_, storage))) = object
            .variants
            .iter()
            .enumerate()
            .find(|(_, (k, _))| k == kind)
        else {
            bail!(
                "allocation or assignment type `{kind}` must extend declared type `{}`",
                object.kind
            );
        };
        let allocate = vec![
            Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(storage.clone(), vec![int(1)])],
            },
            set(&object.tag, index + 1),
        ];
        let mut out = vec![];
        if assignment {
            let release = cases(
                object,
                object
                    .variants
                    .iter()
                    .map(|(_, s)| {
                        vec![Stmt::CallSub {
                            name: "@deallocate".into(),
                            args: vec![id(s)],
                        }]
                    })
                    .collect(),
                vec![],
            );
            let mut replace = vec![release];
            replace.extend(allocate);
            out.push(Stmt::If {
                cond: Expr::Bin(
                    BinOp::Ne,
                    Box::new(id(&object.tag)),
                    Box::new(int(index + 1)),
                ),
                then_body: replace,
                else_body: None,
            });
        } else {
            out.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&object.tag)), Box::new(int(0))),
                then_body: failure(format!(
                    "allocation of already allocated polymorphic object `{to}`"
                )),
                else_body: None,
            });
            out.extend(allocate);
        }
        if let Some(value) = value {
            out.push(Stmt::AssignIndex {
                name: storage.clone(),
                indices: vec![int(1)],
                value,
            });
        }
        Ok(out)
    }
    fn source_kind(&self, value: &Expr, decls: &Declarations) -> Result<String> {
        match value {
            Expr::Ident(n) => {
                if let Some(d) = decls.get(n) {
                    for s in d {
                        if let Stmt::DerivedDecl {
                            type_name,
                            polymorphic: false,
                            dims,
                            ..
                        } = s
                        {
                            if dims.is_empty() {
                                return Ok(type_name.clone());
                            }
                        }
                    }
                }
            }
            Expr::Call(n, _) if self.defs.fields.contains_key(n) => return Ok(n.clone()),
            _ => {}
        }
        bail!("polymorphic allocation and assignment currently require a scalar derived variable or constructor")
    }
    fn from_source(
        &self,
        to: &str,
        object: &Object,
        source: &Expr,
        copy: bool,
        assignment: bool,
        vars: &Runtime,
        decls: &Declarations,
    ) -> Result<Vec<Stmt>> {
        if let Expr::Ident(n) = source {
            if let Some(from) = vars.get(n) {
                if assignment && from.tag == object.tag {
                    return Ok(vec![Stmt::If {
                        cond: Expr::Bin(BinOp::Eq, Box::new(id(&from.tag)), Box::new(int(0))),
                        then_body: failure(format!(
                            "assignment from unallocated polymorphic object `{n}`"
                        )),
                        else_body: None,
                    }]);
                }
                let mut branches = vec![];
                for (kind, storage) in &from.variants {
                    let value = copy.then(|| Expr::Call(storage.clone(), vec![int(1)]));
                    branches.push(if self.defs.distance(kind,&object.kind).is_some() {
                        self.transfer(to,object,kind,value,assignment)?
                    } else {failure(format!("dynamic type `{kind}` is incompatible with polymorphic destination `{to}`"))});
                }
                let default = if !copy && !self.defs.abstracts.contains(&from.kind) {
                    self.transfer(to, object, &from.kind, None, assignment)?
                } else {
                    failure(format!("unallocated polymorphic SOURCE or MOLD `{n}` has no usable dynamic value or concrete type"))
                };
                return Ok(vec![cases(from, branches, default)]);
            }
        }
        let kind = self.source_kind(source, decls)?;
        self.transfer(to, object, &kind, copy.then(|| source.clone()), assignment)
    }
    fn evaluate_source(
        &mut self,
        source: &Expr,
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<(Vec<Stmt>, Expr, Declarations)> {
        if matches!(source, Expr::Ident(_)) {
            return Ok((vec![], source.clone(), decls.clone()));
        }
        let kind = self.source_kind(source, decls)?;
        self.serial += 1;
        let temp = format!("@polymorphic_source{}", self.serial);
        let declaration = Stmt::DerivedDecl {
            type_name: kind,
            names: vec![temp.clone()],
            attributes: vec![],
            polymorphic: false,
            dims: vec![],
        };
        let mut local = decls.clone();
        local.insert(temp.clone(), vec![declaration.clone()]);
        let mut evaluation = vec![Stmt::Assign {
            name: temp.clone(),
            value: source.clone(),
        }];
        self.body(&mut evaluation, vars, &local, pure)?;
        let mut prefix = vec![declaration];
        prefix.extend(evaluation);
        Ok((prefix, id(&temp), local))
    }
    fn allocation(
        &mut self,
        name: &str,
        args: &[Expr],
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<Option<Vec<Stmt>>> {
        let (kind, args) = if name == "@allocate_derived_type" {
            let Some(Expr::Ident(kind)) = args.first() else {
                bail!("invalid derived ALLOCATE type specification");
            };
            (Some(kind.as_str()), &args[1..])
        } else {
            (None, args)
        };
        if !args
            .iter()
            .any(|a| matches!(a,Expr::Ident(n) if vars.contains_key(n)))
        {
            return Ok(None);
        }
        let mut objects = vec![];
        let mut source = None;
        let mut mold = None;
        for a in args {
            if let Expr::Call(k, v) = a {
                if let Some(k) = k.strip_prefix("@keyword:") {
                    if name == "@deallocate" {
                        bail!("polymorphic DEALLOCATE options are not yet supported");
                    }
                    let [value] = v.as_slice() else {
                        bail!("invalid ALLOCATE option");
                    };
                    match k {
                        "source" => {
                            if source.replace(value).is_some() {
                                bail!("duplicate SOURCE= option");
                            }
                        }
                        "mold" => {
                            if mold.replace(value).is_some() {
                                bail!("duplicate MOLD= option");
                            }
                        }
                        _ => bail!("polymorphic ALLOCATE option `{k}=` is not yet supported"),
                    }
                    continue;
                }
            }
            objects.push(a);
        }
        if source.is_some() && mold.is_some() {
            bail!("ALLOCATE cannot specify both SOURCE= and MOLD=");
        }
        if kind.is_some() && (source.is_some() || mold.is_some()) {
            bail!("ALLOCATE cannot combine a type specification with SOURCE= or MOLD=");
        }
        let (mut out, evaluated, local) = if let Some(value) = source {
            let (prefix, value, local) = self.evaluate_source(value, vars, decls, pure)?;
            (prefix, Some(value), local)
        } else {
            (vec![], None, decls.clone())
        };
        for a in objects {
            let Expr::Ident(n) = a else {
                bail!("polymorphic allocation currently requires scalar variables");
            };
            let Some(object) = vars.get(n) else {
                bail!(
                    "mixed polymorphic and nonpolymorphic allocation lists are not yet supported"
                );
            };
            if name == "@deallocate" {
                out.push(cases(
                    object,
                    object
                        .variants
                        .iter()
                        .map(|(_, s)| {
                            vec![Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![id(s)],
                            }]
                        })
                        .collect(),
                    failure(format!(
                        "deallocation of unallocated polymorphic object `{n}`"
                    )),
                ));
                out.push(set(&object.tag, 0));
            } else if let Some(value) = evaluated.as_ref().or(mold) {
                out.extend(self.from_source(
                    n,
                    object,
                    value,
                    source.is_some(),
                    false,
                    vars,
                    &local,
                )?);
            } else {
                out.extend(self.transfer(n, object, kind.unwrap_or(&object.kind), None, false)?);
            }
        }
        Ok(Some(out))
    }
    fn body(
        &mut self,
        body: &mut Vec<Stmt>,
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<()> {
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = s {
                location = Some(p);
                out.push(s);
                continue;
            }
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                out.push(s);
                continue;
            }
            let lowered=(||->Result<Vec<Stmt>> {
                if let Stmt::CallSub{name,args}=&s {
                    if matches!(name.as_str(),"@allocate"|"@allocate_derived_type"|"@deallocate") {
                        if let Some(out)=self.allocation(name,args,vars,decls,pure)? {return Ok(out);}
                    }
                }
                if let Stmt::Assign{name,value}=&s {
                    if let Some(object)=vars.get(name) {
                        if !object.polymorphic {bail!("whole-object assignment of a SELECT TYPE associate requires ordinary derived assignment");}
                        let (mut prefix,value,local)=self.evaluate_source(value,vars,decls,pure)?;
                        prefix.extend(self.from_source(name,object,&value,true,true,vars,&local)?);
                        return Ok(prefix);
                    }
                }
                // Inquiries do not evaluate the stored value.
                for e in expressions(&mut s) {visit_expr(e,&mut |e| {
                    if let Expr::Call(n,a)=e {
                        if n=="allocated"&&a.len()==1 {
                            let actual=match &a[0] {Expr::Call(k,v) if k=="@keyword:array"&&v.len()==1=>&v[0],e=>e};
                            if let Expr::Ident(v)=actual {if let Some(o)=vars.get(v) {*e=Expr::Bin(BinOp::Ne,Box::new(id(&o.tag)),Box::new(int(0)));}}
                        }
                    }
                });}
                let selected=matches!(&s,Stmt::SelectCase{expr,..} if selection(expr).is_some_and(|(n,_)|vars.contains_key(n)));
                if selected {
                    let Stmt::SelectCase{expr,cases:guards,default}=&s else {unreachable!()};
                    let (selector,alias)=selection(expr).unwrap();
                    let object=&vars[selector];
                    let mut branches=vec![];
                    for (kind,_) in &object.variants {
                        let mut best=None;
                        let mut distance=usize::MAX;
                        for c in guards {
                            let (class,guard_kind)=guard(&c.items[0])?;
                            let d=if class {self.defs.distance(kind,guard_kind).map(|d|d+1)} else {(kind==guard_kind).then_some(0)};
                            if let Some(d)=d {if d<distance {distance=d;best=Some((class,guard_kind,c.body.clone()));}}
                        }
                        let mut local=vars.clone();
                        let mut view=object.clone();
                        let mut branch=if let Some((class,guard_kind,body))=best {
                            view.kind=guard_kind.into(); view.polymorphic=class; body
                        } else {default.clone().unwrap_or_default()};
                        local.insert(alias.into(),view);
                        self.body(&mut branch,&local,decls,pure)?;
                        branches.push(branch);
                    }
                    return Ok(vec![cases(object,branches,failure(format!("SELECT TYPE on unallocated polymorphic object `{selector}`")))]);
                }
                // Evaluate runtime-dependent conditions at their original
                // control point, never move EXIT/CYCLE into a helper procedure.
                if let Stmt::If{cond,then_body,else_body}=&mut s {
                    let probe=Stmt::Assign{name:String::new(),value:cond.clone()};
                    if vars.keys().any(|n|uses(&probe,n)) {
                        self.serial+=1;
                        let temp=format!("@polymorphic_condition{}",self.serial);
                        let declaration=Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![temp.clone()]};
                        let mut local=decls.clone(); local.insert(temp.clone(),vec![declaration.clone()]);
                        let mut evaluation=vec![Stmt::Assign{name:temp.clone(),value:cond.clone()}];
                        self.body(&mut evaluation,vars,&local,pure)?;
                        *cond=id(&temp);
                        self.body(then_body,vars,decls,pure)?;
                        if let Some(b)=else_body {self.body(b,vars,decls,pure)?;}
                        let mut lowered=vec![declaration]; lowered.extend(evaluation); lowered.push(s);
                        return Ok(lowered);
                    }
                }
                if let Stmt::DoWhile{cond,body}=&mut s {
                    let probe=Stmt::Assign{name:String::new(),value:cond.clone()};
                    if vars.keys().any(|n|uses(&probe,n)) {
                        self.serial+=1;
                        let temp=format!("@polymorphic_condition{}",self.serial);
                        let declaration=Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![temp.clone()]};
                        let mut local=decls.clone(); local.insert(temp.clone(),vec![declaration.clone()]);
                        let mut loop_body=vec![Stmt::Assign{name:temp.clone(),value:cond.clone()},
                            Stmt::If{cond:Expr::Un(crate::ast::UnOp::Not,Box::new(id(&temp))),then_body:vec![Stmt::Exit],else_body:None}];
                        loop_body.append(body);
                        self.body(&mut loop_body,vars,&local,pure)?;
                        return Ok(vec![declaration,Stmt::DoWhile{cond:Expr::Logical(true),body:loop_body}]);
                    }
                }
                for b in children(&mut s) {
                    let local=declarations(b,decls);
                    self.body(b,vars,&local,pure)?;
                }
                let used:Vec<_>=vars.iter().filter(|(n,_)|uses(&s,n)).collect();
                if used.is_empty() {return Ok(vec![s]);}
                if matches!(s,Stmt::Do{..}) {bail!("polymorphic values in counted DO bounds are not yet supported");}
                self.dispatch(s,&used,0,&mut vec![],decls,pure,location)
            })().map_err(|e|crate::source_diagnostics::at(e,location))?;
            out.extend(lowered);
        }
        *body = out;
        Ok(())
    }
    fn dispatch(
        &mut self,
        s: Stmt,
        used: &[(&String, &Object)],
        index: usize,
        chosen: &mut Vec<(String, String, String)>,
        decls: &Declarations,
        pure: bool,
        location: Option<usize>,
    ) -> Result<Vec<Stmt>> {
        if index < used.len() {
            let (n, o) = used[index];
            let mut bodies = vec![];
            for (kind, storage) in &o.variants {
                if self.defs.distance(kind, &o.kind).is_none() {
                    bodies.push(failure("invalid polymorphic associate type".into()));
                    continue;
                }
                chosen.push((n.clone(), kind.clone(), storage.clone()));
                bodies.push(self.dispatch(
                    s.clone(),
                    used,
                    index + 1,
                    chosen,
                    decls,
                    pure,
                    location,
                )?);
                chosen.pop();
            }
            return Ok(vec![cases(
                o,
                bodies,
                failure(format!("use of unallocated polymorphic object `{n}`")),
            )]);
        }
        self.serial += 1;
        let name = format!("@runtime_polymorphic_call{}", self.serial);
        let mut params = vec![];
        let mut args = vec![];
        let mut body = vec![];
        for (n, _, storage) in chosen.iter() {
            let object = used.iter().find(|(v, _)| *v == n).unwrap().1;
            params.push(n.clone());
            args.push(Expr::Call(storage.clone(), vec![int(1)]));
            body.push(Stmt::DerivedDecl {
                type_name: object.kind.clone(),
                names: vec![n.clone()],
                attributes: vec!["intent(inout)".into()],
                polymorphic: object.polymorphic,
                dims: vec![],
            });
        }
        let mut captures: HashSet<String> = decls.keys().filter(|n| uses(&s, n)).cloned().collect();
        loop {
            let mut added = false;
            for n in decls.keys() {
                if captures.contains(n) {
                    continue;
                }
                if captures.iter().any(|c| decls[c].iter().any(|s| uses(s, n))) {
                    captures.insert(n.clone());
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        for (n, d) in decls {
            let parameter=d.iter().any(|s|matches!(s,Stmt::InitializedDecl{attributes,..} if attributes.iter().any(|a|a=="parameter")));
            if used.iter().any(|(v, _)| *v == n) || (!parameter && !captures.contains(n)) {
                continue;
            }
            if !parameter {
                params.push(n.clone());
                args.push(id(n));
            }
            body.extend(d.clone());
        }
        if let Some(p) = location {
            body.push(Stmt::SourceLocation(p));
        }
        body.push(s);
        self.generated.push(Stmt::Subroutine {
            elemental: false,
            pure,
            name: name.clone(),
            params,
            body,
        });
        Ok(vec![Stmt::CallSub { name, args }])
    }
}

pub(super) fn resolve(mut program: Program, defs: &Definitions) -> Result<Program> {
    fn needed(body: &[Stmt]) -> bool {
        for s in body {
            if matches!(s,Stmt::DerivedDecl{polymorphic:true,attributes,..} if attributes.iter().any(|a|a=="allocatable"))
            {
                return true;
            }
            let nested = match s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => needed(body),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => needed(then_body) || else_body.as_ref().is_some_and(|b| needed(b)),
                Stmt::SelectCase { cases, default, .. } => {
                    cases.iter().any(|c| needed(&c.body))
                        || default.as_ref().is_some_and(|b| needed(b))
                }
                _ => false,
            };
            if nested {
                return true;
            }
        }
        false
    }
    if !needed(&program.body) {
        return Ok(program);
    }
    defs.validate(&program.body, &Objects::new())?;
    let mut lower = Lower {
        defs,
        generated: vec![],
        serial: 0,
    };
    lower.scope(&mut program.body, &Declarations::new(), &[], false)?;
    program.body.extend(lower.generated);
    Ok(program)
}

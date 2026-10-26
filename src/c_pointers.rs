// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Validate opaque C pointers before lowering their native address representation.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

const ENTITIES: &[&str] = &[
    "c_ptr",
    "c_null_ptr",
    "c_associated",
    "c_loc",
    "c_f_pointer",
];

/// Interfaces for source analysis, before intrinsic lowering removes the USEs.
pub fn interface_body() -> Vec<Stmt> {
    let mut out = vec![
        Stmt::DerivedType {
            name: "c_ptr".into(),
            components: vec![],
            bindings: vec![],
        },
        Stmt::InitializedDecl {
            kind: TypeSpec::Integer(Some(8)),
            attributes: vec!["parameter".into()],
            entities: vec![("c_null_ptr".into(), Some(int(0)))],
        },
    ];
    for (name, params, optional, function) in [
        (
            "c_associated",
            vec!["c_ptr_1", "c_ptr_2"],
            vec!["c_ptr_2"],
            true,
        ),
        ("c_loc", vec!["x"], vec![], true),
        (
            "c_f_pointer",
            vec!["cptr", "fptr", "shape", "lower"],
            vec!["shape", "lower"],
            false,
        ),
    ] {
        let mut body = vec![];
        for p in &params {
            body.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![p.to_string()],
            });
            body.push(Stmt::DummyIntent {
                name: p.to_string(),
                intent: if *p == "fptr" {
                    "intent(out)"
                } else {
                    "intent(in)"
                }
                .into(),
            });
        }
        for p in optional {
            body.push(Stmt::DummyIntent {
                name: p.into(),
                intent: "optional".into(),
            });
        }
        let params = params.into_iter().map(str::to_string).collect();
        out.push(if function {
            Stmt::Function {
                elemental: false,
                pure: true,
                name: name.into(),
                params,
                return_type: Some(TypeSpec::Integer(Some(8))),
                body,
            }
        } else {
            Stmt::Subroutine {
                elemental: false,
                pure: false,
                name: name.into(),
                params,
                body,
            }
        });
    }
    out
}
#[derive(Clone, Default)]
struct Scope {
    entities: HashMap<String, String>,
    pointers: HashSet<String>,
    targets: HashSet<String>,
    temporary_dummies: HashSet<String>,
    results: HashSet<String>,
    signatures: HashMap<String, Vec<(String, bool)>>,
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn intrinsic<'a>(scope: &'a Scope, n: &str) -> Option<&'a str> {
    scope.entities.get(n).map(String::as_str)
}
fn pointer_type(scope: &Scope, n: &str) -> bool {
    intrinsic(scope, n) == Some("c_ptr")
}
fn pointer(e: &Expr, scope: &Scope) -> bool {
    match e {
        Expr::Ident(n) => scope.pointers.contains(n) || intrinsic(scope, n) == Some("c_null_ptr"),
        Expr::Call(n, _) => scope.results.contains(n) || intrinsic(scope, n) == Some("c_loc"),
        _ => false,
    }
}
fn import(
    scope: &mut Scope,
    module: &Scope,
    only: &Option<Vec<String>>,
    renames: &[(String, String)],
) {
    let mut names = module
        .entities
        .keys()
        .chain(module.pointers.iter())
        .chain(module.targets.iter())
        .chain(module.results.iter())
        .chain(module.signatures.keys())
        .cloned()
        .collect::<HashSet<_>>();
    if let Some(only) = only {
        names.retain(|n| only.contains(n) || renames.iter().any(|(_, remote)| remote == n));
    }
    for remote in names {
        let local = renames
            .iter()
            .find(|(_, r)| r == &remote)
            .map_or(remote.as_str(), |(l, _)| l.as_str());
        if let Some(e) = module.entities.get(&remote) {
            scope.entities.insert(local.into(), e.clone());
        }
        if module.targets.contains(&remote) {
            scope.targets.insert(local.into());
        }
        if module.pointers.contains(&remote) {
            scope.pointers.insert(local.into());
        }
        if module.results.contains(&remote) {
            scope.results.insert(local.into());
        }
        if let Some(s) = module.signatures.get(&remote) {
            scope.signatures.insert(local.into(), s.clone());
        }
    }
}
fn make_scope(body: &[Stmt], inherited: &Scope, modules: &HashMap<String, Scope>) -> Scope {
    let mut scope = inherited.clone();
    for s in body {
        if let Stmt::Use {
            module,
            only,
            renames,
            ..
        } = s
        {
            if let Some(exports) = modules.get(module) {
                import(&mut scope, exports, only, renames);
            }
        }
    }
    for s in body {
        let names = match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
            Stmt::InitializedDecl { entities, .. } => {
                entities.iter().map(|(n, _)| n.clone()).collect()
            }
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            _ => vec![],
        };
        for n in names {
            scope.pointers.remove(&n);
            scope.targets.remove(&n);
        }
    }
    for s in body {
        match s {
            Stmt::DummyIntent { name, intent } if intent == "target" || intent == "pointer" => {
                scope.targets.insert(name.clone());
            }
            Stmt::DerivedDecl {
                names, attributes, ..
            } if attributes.iter().any(|a| a == "target" || a == "pointer") => {
                scope.targets.extend(names.iter().cloned())
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DerivedDecl {
            type_name, names, ..
        } = s
        {
            if pointer_type(&scope, type_name) {
                scope.pointers.extend(names.iter().cloned());
            }
        }
    }
    for s in body {
        if let Stmt::Function {
            name, params, body, ..
        }
        | Stmt::Subroutine {
            name, params, body, ..
        } = s
        {
            let local = make_scope(body, &scope, modules);
            scope.signatures.insert(
                name.clone(),
                params
                    .iter()
                    .map(|p| (p.clone(), local.pointers.contains(p)))
                    .collect(),
            );
            if matches!(s, Stmt::Function { .. }) {
                let result = body
                    .iter()
                    .rev()
                    .find_map(|s| match s {
                        Stmt::Return(Some(Expr::Ident(n))) => Some(n.as_str()),
                        _ => None,
                    })
                    .unwrap_or(name);
                if local.pointers.contains(result) {
                    scope.results.insert(name.clone());
                }
            }
        }
    }
    scope
}

fn actuals(args: &[Expr], names: &[&str], required: usize) -> Result<Vec<Option<Expr>>> {
    let mut out = vec![None; names.len()];
    let mut position = 0;
    let mut keywords = false;
    for a in args {
        let (i, value) = if let Some((name, value)) = crate::named_arguments::keyword(a) {
            keywords = true;
            (
                names
                    .iter()
                    .position(|n| *n == name)
                    .with_context(|| format!("unknown C interoperability keyword `{name}`"))?,
                value,
            )
        } else {
            if keywords {
                bail!("positional argument after a keyword argument");
            }
            let i = position;
            position += 1;
            (i, a)
        };
        if i >= out.len() {
            bail!("too many arguments to C interoperability intrinsic");
        }
        if out[i].is_some() {
            bail!("duplicate argument `{}`", names[i]);
        }
        out[i] = Some(value.clone());
    }
    if out[..required].iter().any(Option::is_none) {
        bail!("missing required C interoperability argument");
    }
    Ok(out)
}
fn expression(e: &mut Expr, scope: &Scope) -> Result<()> {
    // Check source operations before C pointers become address-sized integers.
    match e {
        Expr::Bin(_, a, b) if pointer(a, scope) || pointer(b, scope) => {
            bail!("C_PTR values are opaque; use C_ASSOCIATED rather than arithmetic or comparison")
        }
        Expr::Un(_, a) if pointer(a, scope) => bail!("C_PTR values are opaque"),
        Expr::Call(n, args) if intrinsic(scope, n) == Some("c_associated") => {
            let args = actuals(args, &["c_ptr_1", "c_ptr_2"], 1)?;
            if args.iter().flatten().any(|e| !pointer(e, scope)) {
                bail!("C_ASSOCIATED arguments must have type C_PTR");
            }
            let mut a = args[0].clone().unwrap();
            expression(&mut a, scope)?;
            let mut values = vec![a];
            if let Some(mut b) = args[1].clone() {
                expression(&mut b, scope)?;
                values.push(b);
            }
            *e = bin(
                BinOp::Ne,
                Expr::Call("@c_associated".into(), values),
                int(0),
            );
            return Ok(());
        }
        Expr::Call(n, args) if intrinsic(scope, n) == Some("c_loc") => {
            let args = actuals(args, &["x"], 1)?;
            let actual = args[0].as_ref().unwrap();
            let root = match actual {
                Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => Some(n),
                Expr::Section { name, .. } => Some(name),
                _ => None,
            };
            if !root.is_some_and(|n| scope.targets.contains(n)) {
                bail!("C_LOC argument requires TARGET or POINTER attribute");
            }
            if root.is_some_and(|n| scope.temporary_dummies.contains(n)) {
                bail!("C_LOC of a nonpointer dummy argument is not yet supported because its adapter may use temporary storage");
            }

            *e = Expr::Call("@c_loc".into(), vec![args[0].clone().unwrap()]);
            return Ok(());
        }
        Expr::Ident(n) if intrinsic(scope, n) == Some("c_null_ptr") => {
            *e = int(0);
            return Ok(());
        }
        Expr::Call(n, args)
            if n.contains("constructor") && args.iter().any(|a| pointer(a, scope)) =>
        {
            bail!("C_PTR array constructors are not yet supported")
        }
        Expr::Call(n, args) => check_call(n, args, scope)?,
        _ => {}
    }
    // Visit children once, rather than rechecking the lowered expression.
    match e {
        Expr::Call(_, a) | Expr::Index(_, a) => {
            for e in a {
                expression(e, scope)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, scope)?;
            expression(b, scope)?;
        }
        Expr::Un(_, a) => expression(a, scope)?,
        _ => crate::procedure_arguments::visit_expr(e, &mut |e| {
            if let Expr::Ident(n) = e {
                if intrinsic(scope, n) == Some("c_null_ptr") {
                    *e = int(0);
                }
            }
        }),
    }
    Ok(())
}
fn check_call(name: &str, args: &[Expr], scope: &Scope) -> Result<()> {
    if let Some(types) = scope.signatures.get(name) {
        for (i, arg) in args.iter().enumerate() {
            let (actual, expected) =
                if let Some((name, value)) = crate::named_arguments::keyword(arg) {
                    (
                        value,
                        types.iter().find(|(n, _)| n == name).map(|(_, t)| *t),
                    )
                } else {
                    (arg, types.get(i).map(|(_, t)| *t))
                };
            if let Some(expected) = expected {
                if pointer(actual, scope) != expected {
                    bail!("argument {} of `{name}` has incompatible C_PTR type", i + 1);
                }
            }
        }
    } else if !name.starts_with('@')
        && args.iter().any(|a| {
            pointer(
                crate::named_arguments::keyword(a).map_or(a, |(_, v)| v),
                scope,
            )
        })
    {
        bail!("C_PTR argument to `{name}` requires an explicit compatible procedure interface");
    }
    Ok(())
}
fn unit(body: &mut Vec<Stmt>, inherited: &Scope, modules: &HashMap<String, Scope>) -> Result<()> {
    let local = make_scope(body, inherited, modules);
    fn raw_calls(body: &mut Vec<Stmt>, scope: &Scope, raw: &mut HashSet<String>) {
        for s in body {
            if let Stmt::CallSub { name, args } = s {
                if intrinsic(scope, name) == Some("c_f_pointer") {
                    if let Ok(a) = actuals(args, &["cptr", "fptr", "shape", "lower"], 2) {
                        if let Some(Expr::Ident(n)) = &a[1] {
                            raw.insert(n.clone());
                        }
                    }
                }
            }
            if !matches!(
                s,
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }
            ) {
                for child in crate::procedure_arguments::children(s) {
                    raw_calls(child, scope, raw);
                }
            }
        }
    }
    let mut raw = HashSet::new();
    raw_calls(&mut body.clone(), &local, &mut raw);
    let old = std::mem::take(body);
    let mut statements = old.into_iter().peekable();
    let mut location = None;
    while let Some(mut s) = statements.next() {
        if let Stmt::SourceLocation(n) = s {
            location = Some(n);
        }
        (||->Result<()> {
            match &mut s {
                Stmt::DerivedDecl{type_name,names,attributes,polymorphic,dims} if pointer_type(&local,type_name)=>{
                    if *polymorphic || !dims.is_empty() || attributes.iter().any(|a|matches!(a.as_str(),"pointer"|"allocatable")) {bail!("C_PTR currently supports nonpolymorphic scalar objects without POINTER or ALLOCATABLE");}
                    if names.len()==1 && attributes.iter().any(|a|a=="save"||a=="parameter") {
                        if let Some(Stmt::Assign{name,value})=statements.peek() {
                            if name==&names[0] {
                                if !pointer(value,&local) {bail!("C_PTR initializer must have type C_PTR");}
                                let mut value=value.clone();expression(&mut value,&local)?;
                                body.push(Stmt::InitializedDecl{kind:TypeSpec::Integer(Some(8)),attributes:attributes.clone(),entities:vec![(name.clone(),Some(value))]});statements.next();return Ok(());
                            }
                        }
                    }
                    let attrs=attributes.clone();let ns=names.clone();
                    body.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:ns.clone()});
                    for n in ns {for a in &attrs {body.push(Stmt::DummyIntent{name:n.clone(),intent:a.clone()});}}
                    return Ok(());
                }
                Stmt::Use{module,only,renames,..}=>{
                    if let Some(exports)=modules.get(module) {
                        if let Some(only)=only {only.retain(|n|!exports.entities.contains_key(n));}
                        renames.retain(|(_,remote)|!exports.entities.contains_key(remote));
                    }
                }
                Stmt::Function{params,body:child,..}|Stmt::Subroutine{params,body:child,..}=>{
                    let mut inherited=local.clone();
                    for p in params {
                        let pointer=child.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p && intent=="pointer"));
                        let direct=child.iter().any(|s|matches!(s,Stmt::DummyIntent{name,..} if name==crate::c_interop::BIND)) && child.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,dims,..} if name==p && (crate::c_interop::assumed_shape(dims)||crate::c_interop::assumed_size(dims))));
                        if !pointer && !direct {inherited.temporary_dummies.insert(p.clone());}
                    }
                    unit(child,&inherited,modules)?;body.push(s);return Ok(());
                }
                Stmt::Module{body:child,..}=>{unit(child,&local,modules)?;body.push(s);return Ok(());}
                Stmt::VarDecl{names,..}=>for n in names.iter() {if raw.contains(n) {body.push(Stmt::DummyIntent{name:n.clone(),intent:"@c_pointer_storage".into()});}},
                Stmt::ArrayDecl{name,..} if raw.contains(name)=>body.push(Stmt::DummyIntent{name:name.clone(),intent:"@c_pointer_storage".into()}),
                Stmt::CallSub{name,args} if intrinsic(&local,name)==Some("c_f_pointer")=>{
                    let a=actuals(args,&["cptr","fptr","shape","lower"],2)?;
                    if !pointer(a[0].as_ref().unwrap(),&local) {bail!("C_F_POINTER CPTR must have type C_PTR");}
                    let Expr::Ident(n)=a[1].as_ref().unwrap() else {bail!("C_F_POINTER FPTR must be a Fortran pointer variable");};
                    *name="@c_f_pointer".into();
                    *args=vec![a[0].clone().unwrap(),id(n),a[2].clone().unwrap_or(Expr::Call("@no_c_shape".into(),vec![])),a[3].clone().unwrap_or(Expr::Call("@no_c_lower".into(),vec![]))];
                }
                Stmt::Assign{name,value}=>{
                    if local.pointers.contains(name)!=pointer(value,&local) {bail!("assignment to or from C_PTR requires another C_PTR value");}
                }
                Stmt::Do{var,start,end,..} if local.pointers.contains(var) || pointer(start,&local) || pointer(end,&local)=>bail!("C_PTR values cannot be DO controls"),
                Stmt::If{cond,..}|Stmt::DoWhile{cond,..} if pointer(cond,&local)=>bail!("C_PTR values cannot be logical conditions"),
                Stmt::ArrayDecl{dims,..} if dims.iter().any(|e|pointer(e,&local))=>bail!("C_PTR values cannot be array bounds"),
                Stmt::SelectCase{expr,..} if pointer(expr,&local)=>bail!("C_PTR values cannot be SELECT CASE selectors"),
                Stmt::AssignIndex{value,..} if pointer(value,&local)=>bail!("assignment to or from C_PTR requires another C_PTR value"),
                Stmt::InitializedDecl{entities,..} if entities.iter().filter_map(|(_,v)|v.as_ref()).any(|v|pointer(v,&local))=>bail!("C_PTR initializer requires a C_PTR declaration"),
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@formatted_print"|"@formatted_write"|"@write") && args.iter().any(|e|pointer(e,&local))=>bail!("C_PTR values do not support Fortran input/output"),
                Stmt::Print{items}|Stmt::Read{args:items} if items.iter().any(|e|pointer(e,&local))=>bail!("C_PTR values do not support Fortran input/output"),
                Stmt::CallSub{name,args}=>check_call(name,args,&local)?,
                _=>{},
            }
            for e in crate::procedure_arguments::expressions(&mut s) {expression(e,&local)?;}
            for child in crate::procedure_arguments::children(&mut s) {unit(child,&local,modules)?;}
            body.push(s);Ok(())
        })().map_err(|e|crate::source_diagnostics::at(e,location))?;
    }
    Ok(())
}
// Association shape checks are made after numeric kinds and array expressions
// have been resolved, in pointers::resolve.
pub fn prepare(mut p: Program) -> Result<Program> {
    fn needed(body: &mut Vec<Stmt>) -> bool {
        body.iter_mut().any(|s| {
            matches!(s,Stmt::Use{module,only,renames,..} if module=="iso_c_binding" && (only.is_none() || only.as_ref().is_some_and(|a|a.iter().any(|n|ENTITIES.contains(&n.as_str()))) || renames.iter().any(|(_,r)|ENTITIES.contains(&r.as_str()))))
            || crate::procedure_arguments::children(s).into_iter().any(needed)
        })
    }
    if !needed(&mut p.body) {
        return Ok(p);
    }

    let builtin = Scope {
        entities: ENTITIES
            .iter()
            .map(|n| (n.to_string(), n.to_string()))
            .collect(),
        ..Scope::default()
    };
    let mut modules = HashMap::from([("iso_c_binding".into(), builtin)]);
    for _ in 0..=p.body.len() {
        let mut changed = false;
        for s in &p.body {
            if let Stmt::Module { name, body } = s {
                let mut exports = make_scope(body, &Scope::default(), &modules);
                let mut default_public = true;
                let mut access = HashMap::new();
                for s in body {
                    if let Stmt::Accessibility { public, names } = s {
                        if names.is_empty() {
                            default_public = *public;
                        } else {
                            for n in names {
                                access.insert(n.clone(), *public);
                            }
                        }
                    }
                }
                let visible = |n: &String| access.get(n).copied().unwrap_or(default_public);
                exports.entities.retain(|n, _| visible(n));
                exports.pointers.retain(visible);
                exports.targets.retain(visible);
                exports.results.retain(visible);
                exports.signatures.retain(|n, _| visible(n));
                if modules.get(name).map(|m| {
                    (
                        &m.entities,
                        &m.pointers,
                        &m.targets,
                        &m.results,
                        &m.signatures,
                    )
                }) != Some((
                    &exports.entities,
                    &exports.pointers,
                    &exports.targets,
                    &exports.results,
                    &exports.signatures,
                )) {
                    modules.insert(name.clone(), exports);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    unit(&mut p.body, &Scope::default(), &modules)?;
    Ok(p)
}

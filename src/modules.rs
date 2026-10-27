// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Bind module entities in their defining scope and capture shared data explicitly.
use crate::ast::{CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
type Names = HashMap<String, String>;
#[derive(Clone, Default)]
struct Module {
    local: Names,
    exports: Names,
    generics: HashSet<String>,
}
fn canonical(module: &str, name: &str) -> String {
    format!("@module${}${name}", module.replace('@', "_"))
}
fn declarations(body: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in body {
        match statement {
            Stmt::VarDecl { names: ns, .. } | Stmt::DerivedDecl { names: ns, .. } => {
                names.extend(ns.iter().cloned())
            }
            Stmt::ArrayDecl { name, .. }
            | Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. }
            | Stmt::DerivedType { name, .. }
            | Stmt::GenericInterface { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::InitializedDecl { entities, .. } => {
                names.extend(entities.iter().map(|(n, _)| n.clone()))
            }
            _ => {}
        }
    }
    names
}
fn imports(body: &[Stmt], modules: &HashMap<String, Module>) -> Result<Names> {
    let mut names = Names::new();
    for statement in body {
        if let Stmt::Use {
            module,
            only,
            renames,
            ..
        } = statement
        {
            let Some(definition) = modules.get(module) else {
                continue;
            };
            if let Some(selected) = only {
                for name in selected {
                    if name.starts_with("operator(") {
                        continue;
                    }
                    if !definition.exports.contains_key(name) {
                        bail!("`{name}` is not a public entity of module `{module}` in USE ONLY");
                    }
                }
            }
            for (_, remote) in renames {
                if remote.starts_with("operator(") { continue; }
                if !definition.exports.contains_key(remote) {
                    bail!("`{remote}` is not a public entity of module `{module}` in USE");
                }
            }
            for (remote, target) in &definition.exports {
                if only
                    .as_ref()
                    .is_some_and(|selected| !selected.contains(remote))
                {
                    continue;
                }
                let mut aliases = renames
                    .iter()
                    .filter_map(|(local, n)| (n == remote).then_some(local))
                    .collect::<Vec<_>>();
                if aliases.is_empty()
                    || only.as_ref().is_some_and(|selected| {
                        selected.iter().filter(|n| *n == remote).count() > aliases.len()
                    })
                {
                    aliases.push(remote);
                }
                for name in aliases {
                    let target = if definition.generics.contains(remote) { name } else { target };
                    if let Some(previous) = names.insert(name.clone(), target.clone()) {
                        if previous != *target {
                            bail!("ambiguous imported name `{name}`");
                        }
                    }
                }
            }
        }
    }
    Ok(names)
}
fn rename(name: &mut String, names: &Names) {
    if let Some((root, component)) = name.split_once('%') {
        if let Some(target) = names.get(root) {
            *name = format!("{target}%{component}");
        }
    } else if let Some(target) = names.get(name) {
        *name = target.clone();
    }
}
fn expression(expr: &mut Expr, names: &Names) {
    match expr {
        Expr::Ident(name) => rename(name, names),
        Expr::Call(name, args) | Expr::Index(name, args) => {
            if name == "@typed_constructor" {
                if let Some(Expr::Str(encoded)) = args.first_mut() {
                    if let Ok(mut ty) = serde_json::from_str::<TypeSpec>(encoded) {
                        kind(&mut ty, names);
                        *encoded = serde_json::to_string(&ty).unwrap();
                    }
                }
            }
            rename(name, names);
            for arg in args {
                expression(arg, names);
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, names);
            expression(b, names);
        }
        Expr::Un(_, e) => expression(e, names),
        Expr::RealLit(value) | Expr::IntLit(value) => {
            if let Some((number, kind)) = value.split_once('_') {
                if let Some(target) = names.get(&kind.to_ascii_lowercase()) {
                    *value = format!("{number}_{target}");
                }
            }
        }
        Expr::Section {
            name,
            lower,
            upper,
            stride,
        } => {
            rename(name, names);
            for e in [lower, upper, stride].into_iter().flatten() {
                expression(e, names);
            }
        }
        _ => {}
    }
}
fn kind(kind: &mut TypeSpec, names: &Names) {
    match kind {
        TypeSpec::IntegerKind(n) | TypeSpec::RealKind(n) | TypeSpec::LogicalKind(n) | TypeSpec::Complex(n) => rename(n, names),
        TypeSpec::CharacterLength(e) | TypeSpec::Referenced {source:e,..} => expression(e, names),
        _ => {}
    }
}
fn scope(
    body: &mut Vec<Stmt>,
    inherited: &Names,
    own: Option<&Names>,
    modules: &HashMap<String, Module>,
    objects: &HashSet<String>,
    result_binding: Option<(String, String)>,
) -> Result<()> {
    let selected = imports(body, modules)?;
    let declared = declarations(body);
    let mut names = inherited.clone();
    for name in &declared {
        names.remove(name);
    }
    if let Some(own) = own {
        names.extend(own.clone());
    }
    for name in selected.keys() {
        if declared.contains(name)
            && !body
                .iter()
                .any(|s| matches!(s,Stmt::GenericInterface {name:n,..} if n==name))
        {
            bail!("local declaration `{name}` conflicts with a USE-associated entity");
        }
    }
    names.extend(selected);
    if let Some((original, target)) = result_binding {
        names.insert(original, target);
    }
    for statement in body.iter_mut() {
        match statement {
            Stmt::VarDecl {
                kind: ty,
                names: ns,
            } => {
                kind(ty, &names);
                for n in ns {
                    rename(n, &names);
                }
            }
            Stmt::InitializedDecl {
                kind: ty,
                entities,
                attributes,
            } => {
                if own.is_some() && !attributes.iter().any(|a| a == "parameter") {
                    attributes.push("@module_initializer".into());
                }
                kind(ty, &names);
                for (n, e) in entities {
                    rename(n, &names);
                    if let Some(e) = e {
                        expression(e, &names);
                    }
                }
            }
            Stmt::ArrayDecl {
                kind: ty,
                name,
                dims,
            } => {
                kind(ty, &names);
                rename(name, &names);
                for e in dims {
                    expression(e, &names);
                }
            }
            Stmt::DerivedDecl {
                type_name,
                names: ns, dims,
                ..
            } => {
                for e in dims {expression(e,&names);}
                rename(type_name, &names);
                for n in ns {
                    rename(n, &names);
                }
            }
            Stmt::DerivedType {
                name,
                components,
                bindings,
            } => {
                rename(name, &names);
                // Component names are local to the type; their kinds and defaults see its host.
                for component in components {
                    match component {
                        Stmt::VarDecl { kind: ty, .. } => kind(ty, &names),
                        Stmt::ArrayDecl { kind: ty, dims, .. } => {
                            kind(ty, &names);
                            for e in dims {
                                expression(e, &names);
                            }
                        }
                        Stmt::InitializedDecl {
                            kind: ty, entities, ..
                        } => {
                            kind(ty, &names);
                            for (_, value) in entities {
                                if let Some(e) = value {
                                    expression(e, &names);
                                }
                            }
                        }
                        Stmt::DerivedDecl { type_name, dims, .. } => {
                            rename(type_name, &names);
                            for e in dims { expression(e, &names); }
                        }
                        // Derived component defaults follow their declarations
                        // as assignments. Only the value sees the host scope.
                        Stmt::Assign { value, .. } => expression(value, &names),
                        _ => {}
                    }
                }
                for (_, target) in bindings {
                    rename(target, &names);
                }
            }
            Stmt::GenericInterface {
                name,
                procedures,
                end_name,
            } => {
                rename(name, &names);
                for target in procedures {
                    rename(target, &names);
                }
                if let Some(n) = end_name {
                    rename(n, &names);
                }
            }
            Stmt::OperatorInterface { procedures, .. } => {
                for target in procedures {
                    rename(target, &names);
                }
            }
            Stmt::Accessibility { names: ns, .. } => {
                for n in ns {
                    rename(n, &names);
                }
            }
            Stmt::DummyIntent { name, .. } => rename(name, &names),
            Stmt::Use {
                module,
                only,
                renames,
                ..
            } => {
                if let Some(definition) = modules.get(module) {
                    if let Some(only) = only {
                        for n in only {
                            if let Some(target) = definition.exports.get(n) {
                                *n = target.clone();
                            }
                        }
                    }
                    // Generic association merges specific procedures under the
                    // local spelling; preserve those aliases for the resolver.
                    renames.retain(|(_, remote)| definition.generics.contains(remote) || remote.starts_with("operator("));
                }
            }
            Stmt::Assign { name, value } => {
                rename(name, &names);
                expression(value, &names);
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                rename(name, &names);
                for e in indices {
                    expression(e, &names);
                }
                expression(value, &names);
            }
            Stmt::Print { items } | Stmt::Read { args: items } => {
                for e in items {
                    expression(e, &names);
                }
            }
            Stmt::CallSub { name, args } => {
                rename(name, &names);
                for e in args {
                    expression(e, &names);
                }
            }
            Stmt::Return(Some(e)) => expression(e, &names),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, &names);
                scope(then_body, &names, None, modules, objects, None)?;
                if let Some(body) = else_body {
                    scope(body, &names, None, modules, objects, None)?;
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                rename(var, &names);
                expression(start, &names);
                expression(end, &names);
                scope(body, &names, None, modules, objects, None)?;
            }
            Stmt::DoWhile { cond, body } => {
                expression(cond, &names);
                scope(body, &names, None, modules, objects, None)?;
            }
            Stmt::Block { body } => scope(body, &names, None, modules, objects, None)?,
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                let alias = crate::select_type::selection(expr).map(|(selector,alias)| (selector.to_owned(),alias.to_owned()));
                let mut branch_names = names.clone();
                if let Some((selector,alias)) = alias { if selector != alias {branch_names.remove(&alias);} }
                expression(expr, &names);
                for case in cases {
                    for item in &mut case.items {
                        match item {
                            CaseItem::Single(e) => expression(e, &names),
                            CaseItem::Range(a, b) => {
                                expression(a, &names);
                                expression(b, &names);
                            }
                        }
                    }
                    scope(&mut case.body, &branch_names, None, modules, objects, None)?;
                }
                if let Some(body) = default {
                    scope(body, &branch_names, None, modules, objects, None)?;
                }
            }
            Stmt::Module { name, body } => {
                let typed = body
                    .iter()
                    .filter_map(|s| {
                        if let Stmt::VarDecl { kind, names } = s {
                            Some(
                                names
                                    .iter()
                                    .map(|n| (n.clone(), kind.clone()))
                                    .collect::<Vec<_>>(),
                            )
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .collect::<HashMap<_, _>>();
                let procedures = body
                    .iter()
                    .filter_map(|s| {
                        if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<HashSet<_>>();
                for statement in body.iter_mut() {
                    if let Stmt::Function {
                        name, return_type, ..
                    } = statement
                    {
                        if return_type.is_none() {
                            *return_type = typed.get(name).cloned();
                        }
                    }
                }
                body.retain_mut(|s| {
                    if let Stmt::VarDecl { names, .. } = s {
                        names.retain(|n| !procedures.contains(n));
                        !names.is_empty()
                    } else {
                        true
                    }
                });
                scope(
                    body,
                    &Names::new(),
                    Some(&modules[name].local),
                    modules,
                    objects,
                    None,
                )?;
            }
            Stmt::Function {
                name,
                params,
                body,
                return_type,
                ..
            } => {
                let original = name.clone();
                rename(name, &names);
                let mut host = names.clone();
                for param in params.iter() {
                    host.remove(param);
                }
                let implicit_result = body
                    .iter()
                    .any(|s| matches!(s,Stmt::VarDecl {names,..} if names.contains(&original)));
                if let Some(ty) = return_type {
                    // Result specifications see the procedure's dummy/local
                    // declarations, which shadow same-named module entities.
                    let mut result_scope=host.clone();
                    for local in declarations(body) {result_scope.remove(&local);}
                    result_scope.extend(imports(body, modules)?);
                    kind(ty, &result_scope);
                }
                scope(
                    body,
                    &host,
                    None,
                    modules,
                    objects,
                    implicit_result.then(|| (original, name.clone())),
                )?;
                let locals = declarations(body);
                let mut visible = host
                    .values()
                    .filter(|n| objects.contains(*n) && !locals.contains(*n))
                    .cloned()
                    .collect::<Vec<_>>();
                visible.sort();
                visible.dedup();
                body.splice(
                    0..0,
                    visible.into_iter().map(|name| Stmt::DummyIntent {
                        name,
                        intent: "@module_host".into(),
                    }),
                );
            }
            Stmt::Subroutine {
                name, params, body, ..
            } => {
                rename(name, &names);
                let mut host = names.clone();
                for param in params.iter() {
                    host.remove(param);
                }
                scope(body, &host, None, modules, objects, None)?;
                let locals = declarations(body);
                let mut visible = host
                    .values()
                    .filter(|n| objects.contains(*n) && !locals.contains(*n))
                    .cloned()
                    .collect::<Vec<_>>();
                visible.sort();
                visible.dedup();
                body.splice(
                    0..0,
                    visible.into_iter().map(|name| Stmt::DummyIntent {
                        name,
                        intent: "@module_host".into(),
                    }),
                );
            }
            _ => {}
        }
    }
    Ok(())
}
pub(crate) fn associate(mut program: Program) -> Result<Program> {
    let mut modules = HashMap::new();
    let mut objects = HashSet::new();
    for statement in &program.body {
        if let Stmt::Module { name, body } = statement {
            let declared = declarations(body);
            let inherited = imports(body, &modules)?;
            let mut local: Names = declared
                .iter()
                .map(|n| {
                    let generic = body
                        .iter()
                        .any(|s| matches!(s,Stmt::GenericInterface {name,..} if name==n));
                    (
                        n.clone(),
                        if generic {
                            n.clone()
                        } else {
                            canonical(name, n)
                        },
                    )
                })
                .collect();
            for statement in body {
                if let Stmt::GenericInterface { name, .. } = statement {
                    if let Some(target) = inherited.get(name) {
                        local.insert(name.clone(), target.clone());
                    }
                }
            }
            let procedures = body
                .iter()
                .filter_map(|s| {
                    if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect::<HashSet<_>>();
            for statement in body {
                match statement {
                    Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => objects
                        .extend(
                            names
                                .iter()
                                .filter(|n| !procedures.contains(n))
                                .map(|n| local[n].clone()),
                        ),
                    Stmt::ArrayDecl { name, .. } => {
                        objects.insert(local[name].clone());
                    }
                    Stmt::InitializedDecl { entities, .. } => {
                        objects.extend(entities.iter().map(|(n, _)| local[n].clone()))
                    }
                    _ => {}
                }
            }
            let mut exports = inherited;
            exports.extend(local.clone());
            let mut default = true;
            let mut access = HashMap::new();
            for statement in body {
                match statement {
                    Stmt::Accessibility { public, names } => {
                        if names.is_empty() {
                            default = *public;
                        }
                        for n in names {
                            access.insert(n.clone(), *public);
                        }
                    }
                    Stmt::DerivedDecl { attributes, names, .. } => {
                        for n in names {
                            if attributes.iter().any(|a| a == "private") {
                                access.insert(n.clone(), false);
                            }
                            if attributes.iter().any(|a| a == "public") {
                                access.insert(n.clone(), true);
                            }
                        }
                    }
                    Stmt::InitializedDecl {
                        attributes,
                        entities,
                        ..
                    } => {
                        for (n, _) in entities {
                            if attributes.iter().any(|a| a == "private") {
                                access.insert(n.clone(), false);
                            }
                            if attributes.iter().any(|a| a == "public") {
                                access.insert(n.clone(), true);
                            }
                        }
                    }
                    _ => {}
                }
            }
            exports.retain(|n, _| access.get(n).copied().unwrap_or(default));
            let generics = exports.iter().filter_map(|(n,target)| (n == target).then_some(n.clone())).collect();
            modules.insert(name.clone(), Module { local, exports, generics });
        }
    }
    // Preserve operator interfaces: their spelling is an operation, not a data name.
    scope(
        &mut program.body,
        &Names::new(),
        None,
        &modules,
        &objects,
        None,
    )?;
    Ok(program)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Storage {
    pub kind: TypeSpec,
    pub values: Vec<Expr>,
    pub initialized: bool,
    pub rank: usize,
    #[serde(default)]
    pub deferred: bool,
}
#[derive(Clone)]
struct Data {
    declarations: Vec<Stmt>,
    initializers: Vec<Stmt>,
    constant: bool,
    storage: Storage,
}
fn refs(body: &[Stmt], used: &mut HashSet<String>) {
    fn expr(e: &Expr, used: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => {
                used.insert(n.clone());
            }
            Expr::Call(n, a) | Expr::Index(n, a) => {
                used.insert(n.clone());
                for e in a {
                    expr(e, used);
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, used);
                expr(b, used);
            }
            Expr::Un(_, e) => expr(e, used),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                used.insert(name.clone());
                for e in [lower, upper, stride].into_iter().flatten() {
                    expr(e, used);
                }
            }
            _ => {}
        }
    }
    for s in body {
        match s {
            Stmt::Assign { name, value } => {
                used.insert(name.clone());
                expr(value, used);
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                used.insert(name.clone());
                for e in indices {
                    expr(e, used);
                }
                expr(value, used);
            }
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expr(e, used);
                }
            }
            Stmt::Return(Some(e)) => expr(e, used),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, used);
                refs(then_body, used);
                if let Some(body) = else_body {
                    refs(body, used);
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                expr(start, used);
                expr(end, used);
                refs(body, used);
            }
            Stmt::DoWhile { cond, body } => {
                expr(cond, used);
                refs(body, used);
            }
            Stmt::Block { body } => refs(body, used),
            Stmt::SelectCase {
                expr: e,
                cases,
                default,
            } => {
                expr(e, used);
                for c in cases {
                    refs(&c.body, used);
                }
                if let Some(body) = default {
                    refs(body, used);
                }
            }
            _ => {}
        }
    }
}
fn extent(dims: &[Expr]) -> Result<usize> {
    dims.iter().try_fold(1usize, |n, d| {
        let count = if let Expr::Call(tag, a) = d {
            if tag == "@declared_bounds" {
                (crate::parameters::integer_constant(&a[1])?
                    - crate::parameters::integer_constant(&a[0])?
                    + 1)
                .max(0)
            } else {
                crate::parameters::integer_constant(d)?
            }
        } else {
            crate::parameters::integer_constant(d)?.max(0)
        };
        n.checked_mul(count as usize)
            .context("module array extent overflow")
    })
}
pub(crate) fn capture(mut program: Program) -> Result<Program> {
    let mut data: HashMap<String, Data> = HashMap::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            for declaration in body {
                let (kind, names, dims) = match declaration {
                    Stmt::VarDecl { kind, names } => (kind, names.clone(), vec![]),
                    Stmt::ArrayDecl { kind, name, dims } => {
                        (kind, vec![name.clone()], dims.clone())
                    }
                    _ => continue,
                };
                for name in names {
                    let initializers=body.iter().filter(|s|matches!(s,Stmt::Assign {name:n,..}|Stmt::AssignIndex {name:n,..} if n==&name)).cloned().collect::<Vec<_>>();
                    let constant=body.iter().any(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==&name && intent=="parameter"));
                    let deferred = dims
                        .iter()
                        .any(|e| matches!(e,Expr::Ident(n) if n=="@deferred_extent"));
                    let count = if deferred {
                        1
                    } else {
                        extent(&dims).with_context(|| format!("module declaration `{name}`"))?
                    };
                    let default = match kind {
                        TypeSpec::Character(_) => Expr::Str(String::new()),
                        TypeSpec::Real | TypeSpec::DoublePrecision => Expr::RealLit("0.0".into()),
                        TypeSpec::Logical => Expr::Logical(false),
                        _ => Expr::IntLit("0".into()),
                    };
                    let mut values = vec![default; count];
                    let mut cursor = 0;
                    for initializer in &initializers {
                        match initializer {
                            Stmt::Assign { value, .. } => {
                                if let Expr::Call(tag, items) = value {
                                    if tag == "@array_constructor" {
                                        if items.len() != count {
                                            bail!(
                                                "module array `{name}` initializer shape mismatch"
                                            );
                                        }
                                        values = items.clone();
                                        continue;
                                    }
                                }
                                values = vec![value.clone(); count];
                            }
                            Stmt::AssignIndex { value, .. } => {
                                if cursor >= count {
                                    bail!("module array initializer exceeds extent");
                                }
                                values[cursor] = value.clone();
                                cursor += 1;
                            }
                            _ => {}
                        }
                    }
                    for value in &mut values {
                        *value = crate::parameters::initial_value(kind, value)?;
                    }
                    let declaration = if dims.is_empty() {
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![name.clone()],
                        }
                    } else {
                        declaration.clone()
                    };
                    let mut declarations = vec![declaration];
                    declarations.extend(body.iter().filter(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==&name && matches!(intent.as_str(),"allocatable"|"pointer"|"target"))).cloned());
                    data.insert(
                        name,
                        Data {
                            declarations,
                            constant,
                            storage: Storage {
                                kind: kind.clone(),
                                values,
                                initialized: !initializers.is_empty(),
                                rank: dims.len(),
                                deferred,
                            },
                            initializers,
                        },
                    );
                }
            }
        }
    }
    fn inject(body: &mut Vec<Stmt>, data: &HashMap<String, Data>, module: bool) -> Result<()> {
        for statement in body.iter_mut() {
            match statement {
                Stmt::Module { body, .. } => inject(body, data, true)?,
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                    inject(body, data, false)?
                }
                _ => {}
            }
        }
        if module {
            // Every executable scope now owns declarations bound to the same
            // module storage; backend module containers hold procedures only.
            body.retain_mut(|statement| match statement {
                Stmt::VarDecl { names, .. } => {
                    names.retain(|n| !data.contains_key(n));
                    !names.is_empty()
                }
                Stmt::ArrayDecl { name, .. }
                | Stmt::Assign { name, .. }
                | Stmt::AssignIndex { name, .. } => !data.contains_key(name),
                Stmt::DummyIntent { name, .. } => !data.contains_key(name),
                _ => true,
            });
            return Ok(());
        }
        let declared = declarations(body);
        let mut used = HashSet::new();
        refs(body, &mut used);
        let mut names = used
            .into_iter()
            .filter(|n| data.contains_key(n) && !declared.contains(n))
            .collect::<Vec<_>>();
        names.sort();
        let mut prefix = vec![];
        for name in names {
            let definition = &data[&name];
            prefix.extend(definition.declarations.clone());
            if definition.constant {
                prefix.push(Stmt::DummyIntent {
                    name: name.clone(),
                    intent: "parameter".into(),
                });
                prefix.extend(definition.initializers.clone());
            } else {
                prefix.push(Stmt::CallSub {
                    name: "@module_bind".into(),
                    args: vec![
                        Expr::Ident(name.clone()),
                        Expr::Str(name),
                        Expr::Str(serde_json::to_string(&definition.storage)?),
                    ],
                });
            }
        }
        prefix.append(body);
        *body = prefix;
        Ok(())
    }
    // Keep a storage definition even when this compilation has no callers of
    // the module entity. A later separately compiled user may reference it.
    let mut bindings = Vec::new();
    let mut names = data.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let definition = &data[&name];
        if definition.constant { continue; }
        bindings.extend(definition.declarations.clone());
        bindings.push(Stmt::CallSub {
            name: "@module_bind".into(),
            args: vec![Expr::Ident(name.clone()), Expr::Str(name),
                Expr::Str(serde_json::to_string(&definition.storage)?)],
        });
    }
    fn saved(body: &[Stmt], bindings: &mut Vec<Stmt>) {
        for statement in body {
            match statement {
                Stmt::CallSub { name, args } if name == "@saved_bind" => {
                    if let [Expr::Ident(local), Expr::Str(key), value, rest @ ..] = args.as_slice() {
                        if key.starts_with("@module$") {
                            if let Some(kind) = body.iter().find_map(|s| match s {
                                Stmt::VarDecl { kind, names } if names.contains(local) => Some(kind.clone()),
                                _ => None,
                            }) {
                                let local = format!("@module_saved_owner_{}", bindings.len());
                                bindings.push(Stmt::VarDecl { kind, names: vec![local.clone()] });
                                let mut args=vec![Expr::Ident(local),Expr::Str(key.clone()),value.clone()];
                                args.extend_from_slice(rest);
                                bindings.push(Stmt::CallSub { name: name.clone(), args });
                            }
                        }
                    }
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } | Stmt::Block { body } => saved(body, bindings),
                _ => {}
            }
        }
    }
    saved(&program.body, &mut bindings);
    if !bindings.is_empty() {
        program.body.push(Stmt::Subroutine { name: "@internal_module_storage".into(),
            params: vec![], body: bindings, pure: false, elemental: false });
    }
    inject(&mut program.body, &data, false)?;
    Ok(program)
}

fn children(body: &mut [Stmt], visit: &mut impl FnMut(&mut Vec<Stmt>) -> Result<()>) -> Result<()> {
    for statement in body {
        match statement {
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => visit(body)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                visit(then_body)?;
                if let Some(body) = else_body {
                    visit(body)?;
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    visit(&mut c.body)?;
                }
                if let Some(body) = default {
                    visit(body)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
/// Bind allocation descriptors before specialization can fold their zero initializers.
pub(crate) fn bind_scope(body: &mut Vec<Stmt>) -> Result<()> {
    let mut shapes = crate::allocatables::Shapes::new();
    for statement in body.iter() {
        match statement {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            _ => {}
        }
    }
    crate::allocatables::metadata(body, &mut shapes);
    let mut auxiliaries = HashMap::new();
    let mut existing = HashSet::new();
    for statement in body.iter() {
        if let Stmt::CallSub { name, args } = statement {
            if name != "@module_bind" {
                continue;
            }
            let [Expr::Ident(local), Expr::Str(key), Expr::Str(encoded)] = args.as_slice() else {
                continue;
            };
            existing.insert(local.clone());
            let data: Storage = serde_json::from_str(encoded)?;
            if !data.deferred {
                continue;
            }
            let mut fields = vec![];
            if let Some(length) = crate::allocatables::length(local, &shapes) {
                fields.push(("length".to_owned(), length));
            }
            if let Some(flag) = crate::allocatables::allocated(local, &shapes) {
                fields.push(("allocated".to_owned(), flag));
            }
            if let Some(axes) = crate::allocatables::axes(local, &shapes) {
                fields.extend(
                    axes.into_iter()
                        .enumerate()
                        .map(|(i, e)| (format!("axis{i}"), e)),
                );
            }
            if let Some(lower)=crate::allocatables::lowers(local,&shapes){fields.extend(lower.into_iter().enumerate().map(|(i,e)|(format!("lower{i}"),e)));}
            for (field, value) in fields {
                if let Expr::Ident(n) = value {
                    let kind = shapes
                        .get(&n)
                        .context("missing module allocation descriptor")?
                        .0
                        .clone();
                    let value = if matches!(kind, TypeSpec::Logical) {
                        Expr::Logical(false)
                    } else {
                        Expr::IntLit("0".into())
                    };
                    auxiliaries.insert(
                        n,
                        (
                            format!("{key}.{field}"),
                            Storage {
                                kind,
                                values: vec![value],
                                initialized: true,
                                rank: 0,
                                deferred: false,
                            },
                        ),
                    );
                }
            }
        }
    }
    if auxiliaries.is_empty() {
        return Ok(());
    }
    let mut out = vec![];
    for statement in std::mem::take(body) {
        if matches!(&statement,Stmt::Assign {name,value} if auxiliaries.contains_key(name)&&(matches!(value,Expr::Logical(false))||matches!(crate::parameters::integer_constant(value).ok(),Some(0|1))))
        {
            // Only descriptor setup occurs at this scope's declaration prefix;
            // deallocation and allocation changes live in the executable blocks.
            if !existing.contains(if let Stmt::Assign { name, .. } = &statement {
                name
            } else {
                unreachable!()
            }) {
                continue;
            }
        }
        let declared = if let Stmt::VarDecl { names, .. } = &statement {
            names.clone()
        } else {
            vec![]
        };
        out.push(statement);
        for n in declared {
            if let Some((key, data)) = auxiliaries.get(&n) {
                if !existing.contains(&n) {
                    out.push(Stmt::CallSub {
                        name: "@module_bind".into(),
                        args: vec![
                            Expr::Ident(n),
                            Expr::Str(key.clone()),
                            Expr::Str(serde_json::to_string(data)?),
                        ],
                    });
                }
            }
        }
    }
    *body = out;
    Ok(())
}
pub(crate) fn bind_descriptors(mut program: Program) -> Result<Program> {
    fn visit(body: &mut Vec<Stmt>) -> Result<()> {
        bind_scope(body)?;
        children(body, &mut visit)
    }
    visit(&mut program.body)?;
    Ok(program)
}
/// Reserve a common capacity for every alias of bounded module array storage.
pub(crate) fn finish_bounds(mut program: Program) -> Result<Program> {
    let mut capacities: HashMap<String, usize> = HashMap::new();
    let mut limits: HashMap<String, Vec<i64>> = HashMap::new();
    fn collect(
        body: &mut Vec<Stmt>,
        capacities: &mut HashMap<String, usize>,
        limits: &mut HashMap<String, Vec<i64>>,
    ) -> Result<()> {
        let mut shapes = crate::allocatables::Shapes::new();
        for s in body.iter() {
            if let Stmt::ArrayDecl { kind, name, dims } = s {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
        }
        crate::allocatables::metadata(body, &mut shapes);
        for s in body.iter() {
            if let Stmt::CallSub { name, args } = s {
                if name == "@module_bind" {
                    let [Expr::Ident(local), Expr::Str(key), Expr::Str(encoded)] = args.as_slice()
                    else {
                        continue;
                    };
                    let data: Storage = serde_json::from_str(encoded)?;
                    if !data.deferred {
                        continue;
                    }
                    let dims = &shapes
                        .get(local)
                        .context("unmaterialized module array storage")?
                        .1;
                    let count = extent(dims)
                        .with_context(|| format!("module backing storage `{local}`"))?;
                    let capacity = capacities.entry(key.clone()).or_default();
                    *capacity = (*capacity).max(count);
                    if let Some((_, axes)) = shapes.get(&format!("@axis_limits:{local}")) {
                        let target = limits.entry(key.clone()).or_insert(vec![0; axes.len()]);
                        for (limit, axis) in target.iter_mut().zip(axes) {
                            *limit = (*limit).max(crate::parameters::integer_constant(axis)?);
                        }
                    }
                }
            }
        }
        children(body, &mut |b| collect(b, capacities, limits))
    }
    collect(&mut program.body, &mut capacities, &mut limits)?;
    fn update(
        body: &mut Vec<Stmt>,
        capacities: &HashMap<String, usize>,
        limits: &HashMap<String, Vec<i64>>,
    ) -> Result<()> {
        let mut local = HashMap::new();
        for s in body.iter_mut() {
            if let Stmt::CallSub { name, args } = s {
                if name == "@module_bind" {
                    let [Expr::Ident(n), Expr::Str(key), Expr::Str(encoded)] = args.as_mut_slice()
                    else {
                        continue;
                    };
                    let mut data: Storage = serde_json::from_str(encoded)?;
                    if data.deferred && !matches!(data.kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical) {
                        let count = capacities[key];
                        let default = match data.kind {
                            TypeSpec::Real | TypeSpec::DoublePrecision => {
                                Expr::RealLit("0.0".into())
                            }
                            TypeSpec::Logical => Expr::Logical(false),
                            TypeSpec::Character(_) => Expr::Str(String::new()),
                            _ => Expr::IntLit("0".into()),
                        };
                        data.values = vec![default; count];
                        *encoded = serde_json::to_string(&data)?;
                        local.insert(n.clone(), (count, limits.get(key).cloned()));
                    }
                }
            }
        }
        for s in body.iter_mut() {
            match s {
                Stmt::ArrayDecl { name, dims, .. } if local.contains_key(name) => {
                    *dims = vec![Expr::IntLit(local[name].0.to_string())]
                }
                Stmt::DummyIntent { name, intent }
                    if intent.starts_with("@bounded_limits:") && local.contains_key(name) =>
                {
                    if let Some(axes) = &local[name].1 {
                        *intent = format!(
                            "@bounded_limits:{}",
                            axes.iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(":")
                        );
                    }
                }
                _ => {}
            }
        }
        children(body, &mut |b| update(b, capacities, limits))
    }
    update(&mut program.body, &capacities, &limits)?;
    Ok(program)
}
/// Initialization masks also share the largest capacity of their module object.
pub(crate) fn unify_storage(mut program: Program) -> Result<Program> {
    let mut sizes = HashMap::new();
    fn visit(body: &mut Vec<Stmt>, sizes: &mut HashMap<String, usize>, apply: bool) -> Result<()> {
        for s in body.iter_mut() {
            if let Stmt::CallSub { name, args } = s {
                if name == "@module_bind" {
                    let [_, Expr::Str(key), Expr::Str(encoded)] = args.as_mut_slice() else {
                        continue;
                    };
                    let mut data: Storage = serde_json::from_str(encoded)?;
                    if !apply {
                        let size = sizes.entry(key.clone()).or_default();
                        *size = (*size).max(data.values.len());
                    } else if data.rank != 0 {
                        let value = data
                            .values
                            .first()
                            .cloned()
                            .unwrap_or(Expr::IntLit("0".into()));
                        data.values.resize(sizes[key], value);
                        *encoded = serde_json::to_string(&data)?;
                    }
                }
            }
        }
        children(body, &mut |b| visit(b, sizes, apply))
    }
    visit(&mut program.body, &mut sizes, false)?;
    visit(&mut program.body, &mut sizes, true)?;
    Ok(program)
}

pub(crate) fn prepare_globals(mut program: Program) -> Result<Program> {
    fn visit(body: &mut Vec<Stmt>) -> Result<()> {
        let globals = body
            .iter()
            .filter_map(|s| {
                if let Stmt::CallSub { name, args } = s {
                    if name == "@module_bind" {
                        if let Some(Expr::Ident(n)) = args.first() {
                            return Some(n.clone());
                        }
                    }
                    None
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>();
        let mut attributes = vec![];
        body.retain(|s|if matches!(s,Stmt::DummyIntent {name,intent} if intent=="allocatable"&&!globals.contains(name)) {attributes.push(s.clone());false}else{true});
        crate::allocatables::prepare_matrices(
            body,
            &mut crate::allocatables::Shapes::new(),
            &HashMap::new(),
        )?;
        bind_scope(body)?;
        body.splice(0..0, attributes);
        children(body, &mut visit)
    }
    visit(&mut program.body)?;
    Ok(program)
}

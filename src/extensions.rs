// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower type extension (EXTENDS) by copying the parent's components and
//! bindings into the extended type. The parent component `c%parent` names
//! the inherited components themselves, so `c%parent%x` is `c%x`. Each
//! extended type keeps an `@extends` marker naming its parent, which derived
//! lowering uses to pass an extended object as its parent type. Scalar CLASS
//! calls are specialized by select_type before this pass. Unsupported remaining
//! polymorphic calls are diagnosed by derived lowering.
use crate::ast::{Program, Stmt};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

/// The marker statement the parser emits before an extended type, and the
/// component marker that records the parent in the lowered type.
pub(crate) const EXTENDS: &str = "@extends";

type Definition = (Vec<Stmt>, Vec<(String, String)>);

mod constructors;
pub(crate) use constructors::resolve as resolve_constructors;

fn collect(body: &mut Vec<Stmt>, types: &mut HashMap<String, Definition>, parents: &mut HashMap<String, String>) -> Result<()> {
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == EXTENDS => {
            if let [crate::ast::Expr::Ident(child), crate::ast::Expr::Ident(parent)] = args.as_slice() {
                parents.insert(child.clone(), parent.clone());
            }
            false
        }
        _ => true,
    });
    for s in body.iter_mut() {
        match s {
            Stmt::DerivedType { name, components, bindings } => {
                // Retain the declaring type of each specific binding through
                // flattening, so inherited defined I/O validates its original
                // passed-object type rather than the extended object's type.
                let owners: Vec<_> = bindings.iter().filter(|(n,_)| !n.starts_with('@'))
                    .map(|(n,_)| (format!("@defined_io_binding_owner:{n}"), name.clone())).collect();
                bindings.extend(owners);
                types.insert(name.clone(), (components.clone(), bindings.clone()));
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => {
                collect(body, types, parents)?
            }
            _ => {}
        }
    }
    Ok(())
}

/// The components and bindings of `name` including those it inherits.
fn flatten(name: &str, types: &HashMap<String, Definition>, parents: &HashMap<String, String>,
    done: &mut HashMap<String, Definition>, active: &mut Vec<String>) -> Result<Definition> {
    if let Some(d) = done.get(name) {
        return Ok(d.clone());
    }
    let Some((components, bindings)) = types.get(name).cloned() else {
        bail!("unknown parent type `{name}`");
    };
    let Some(parent) = parents.get(name) else {
        return Ok((components, bindings));
    };
    if active.iter().any(|n| n == name) {
        bail!("type `{name}` extends itself");
    }
    active.push(name.to_owned());
    let (inherited, mut all) = flatten(parent, types, parents, done, active)?;
    active.pop();
    let mut fields = vec![Stmt::DummyIntent { name: EXTENDS.into(), intent: parent.clone() }];
    fields.extend(inherited.into_iter().filter(|s| !matches!(s, Stmt::DummyIntent { name, .. } if name == EXTENDS)));
    fields.extend(components);
    // An extension's binding overrides the parent's binding of the same name.
    for (n, target) in bindings {
        if n.starts_with('@') {
            if !all.contains(&(n.clone(), target.clone())) {
                all.push((n, target));
            }
        } else if let Some(slot) = all.iter_mut().find(|(b, _)| *b == n) {
            slot.1 = target;
        } else {
            all.push((n, target));
        }
    }
    let definition = (fields, all);
    done.insert(name.to_owned(), definition.clone());
    Ok(definition)
}

fn write_back(body: &mut [Stmt], done: &HashMap<String, Definition>) {
    for s in body.iter_mut() {
        match s {
            Stmt::DerivedType { name, components, bindings } => {
                if let Some((c, b)) = done.get(name) {
                    *components = c.clone();
                    *bindings = b.clone();
                }
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => {
                write_back(body, done)
            }
            _ => {}
        }
    }
}

/// Every component name of every type, to keep a parent-component rewrite
/// from changing a real component of the same name.
fn component_names(types: &HashMap<String, Definition>) -> HashSet<String> {
    let mut names = HashSet::new();
    for (components, _) in types.values() {
        for c in components {
            match c {
                Stmt::VarDecl { names: n, .. } | Stmt::DerivedDecl { names: n, .. } => names.extend(n.iter().cloned()),
                Stmt::InitializedDecl { entities, .. } => names.extend(entities.iter().map(|(n, _)| n.clone())),
                Stmt::ArrayDecl { name, .. } => {
                    names.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    names
}

/// Rewrite `x%parent%y` to `x%y` and `x%parent` to `x` in designators.
fn parent_components(value: &mut serde_json::Value, parents: &HashSet<String>) {
    use serde_json::Value;
    fn designator(s: &str) -> bool {
        s.contains('%') && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '%' | '$' | '@'))
    }
    match value {
        Value::String(s) if designator(s) => {
            let parts: Vec<&str> = s.split('%').collect();
            let kept: Vec<&str> = parts.iter().enumerate()
                .filter(|(i, p)| *i == 0 || !parents.contains(**p))
                .map(|(_, p)| *p).collect();
            if kept.len() != parts.len() {
                *s = kept.join("%");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| parent_components(v, parents)),
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                // Character literals are data, not designators.
                if key != "Str" {
                    parent_components(v, parents);
                }
            }
        }
        _ => {}
    }
}

// Output of an explicit parent component retains its declared type when
// storage flattening removes the parent selector from its designator.
fn preserve_output_views(body: &mut [Stmt], parents: &HashMap<String, String>,
    types: &HashMap<String, Definition>, inherited: &HashMap<String, String>) {
    use crate::ast::Expr;
    let mut objects = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::DerivedDecl { type_name, names, .. } => {
                for n in names { objects.insert(n.clone(), type_name.clone()); }
            }
            Stmt::VarDecl { names, .. } => { for n in names { objects.remove(n); } }
            Stmt::ArrayDecl { name, .. } => { objects.remove(name); }
            Stmt::InitializedDecl { entities, .. } => { for (n, _) in entities { objects.remove(n); } }
            _ => {}
        }
    }
    for s in body {
        let items: &mut [Expr] = match s {
            Stmt::Print { items } => items,
            Stmt::CallSub { name, args } if name == "@write" => &mut args[2..],
            Stmt::CallSub { name, args } if name == "@formatted_print" => &mut args[1..],
            _ => &mut [],
        };
        for item in items {
            let Expr::Ident(n) = item else { continue };
            let mut parts = n.split('%');
            let Some(mut ty) = parts.next().and_then(|root| objects.get(root)) else { continue };
            let mut parent_view = false;
            for selector in parts {
                let mut ancestor = parents.get(ty);
                while let Some(parent) = ancestor {
                    if parent.rsplit('$').next() == Some(selector) { break; }
                    ancestor = parents.get(parent);
                }
                let Some(parent) = ancestor else { parent_view = false; break };
                ty = parent;
                parent_view = true;
            }
            if parent_view && types.get(ty).is_some_and(|(_, b)| b.iter().any(|(n, _)| n == "@defined_io:write:formatted")) {
                *item = Expr::Call(format!("@defined_output_parent:{ty}"), vec![item.clone()]);
            }
        }
        for child in crate::procedure_arguments::children(s) {
            preserve_output_views(child, parents, types, &objects);
        }
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut types = HashMap::new();
    let mut parents = HashMap::new();
    collect(&mut program.body, &mut types, &mut parents)?;
    if parents.is_empty() {
        return Ok(program);
    }
    let mut done = HashMap::new();
    let names: Vec<String> = types.keys().cloned().collect();
    for name in &names {
        flatten(name, &types, &parents, &mut done, &mut vec![])?;
    }
    write_back(&mut program.body, &done);
    // The parent component is named by the parent type's own name.
    let components = component_names(&types);
    let parent_names: HashSet<String> = parents.values()
        .map(|p| p.rsplit('$').next().unwrap_or(p).to_owned())
        .collect();
    if let Some(n) = parent_names.iter().find(|n| components.contains(*n)) {
        bail!("a component named `{n}`, like a parent type, is not yet supported");
    }
    let mut output_types = types.clone();
    output_types.extend(done.clone());
    preserve_output_views(&mut program.body, &parents, &output_types, &HashMap::new());
    constructors::preserve_parent_views(&mut program.body, &parents);
    let mut value = serde_json::to_value(&program.body)?;
    parent_components(&mut value, &parent_names);
    program.body = serde_json::from_value(value)?;
    Ok(program)
}

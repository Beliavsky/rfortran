// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! SIMPLE is stronger than PURE. Check association before closure conversion
//! and procedure arguments are lowered; check resolved calls after generics.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

pub fn prefix(
    modifiers: &[(Option<bool>, bool, bool)],
) -> std::result::Result<(bool, bool, bool), &'static str> {
    let pure = modifiers.iter().any(|p| p.0 == Some(true));
    let impure = modifiers.iter().any(|p| p.0 == Some(false));
    let elemental = modifiers.iter().any(|p| p.1);
    let simple = modifiers.iter().any(|p| p.2);
    if simple && (pure || impure) {
        return Err("SIMPLE cannot be combined with PURE or IMPURE");
    }
    if pure && impure {
        return Err("a procedure cannot specify both PURE and IMPURE");
    }
    if modifiers.iter().filter(|p| p.2).count() > 1 {
        return Err("duplicate SIMPLE prefix");
    }
    Ok((simple || pure || elemental && !impure, elemental, simple))
}

pub fn marker() -> Stmt {
    Stmt::DummyIntent {
        name: "@procedure".into(),
        intent: "@simple".into(),
    }
}
pub fn is_simple(body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| matches!(s, Stmt::DummyIntent { intent, .. } if intent == "@simple"))
}

#[derive(Clone, Default)]
struct Scope {
    constants: HashSet<String>,
    procedures: HashMap<String, bool>,
    types: HashSet<String>,
    callbacks: HashMap<String, Vec<(usize, String)>>,
}
fn declarations(body: &[Stmt], scope: &mut Scope) -> HashSet<String> {
    let mut locals = HashSet::new();
    for s in body {
        match s {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => {
                scope.procedures.insert(name.clone(), is_simple(body));
                let required = params.iter().enumerate().filter_map(|(i, p)| {
                    let simple = body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == p && intent == "@simple_interface"))
                        || body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == p && intent.strip_prefix("procedure(").and_then(|s|s.strip_suffix(')')).is_some_and(|n|scope.procedures.get(n)==Some(&true) || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n && intent=="@simple_interface")))));
                    simple.then(|| (i,p.clone()))
                }).collect();
                scope.callbacks.insert(name.clone(), required);
            }
            Stmt::GenericInterface { name, .. } => {
                scope.procedures.entry(name.clone()).or_insert(true);
            }
            Stmt::DerivedType { name, .. } => {
                scope.types.insert(name.clone());
            }
            Stmt::VarDecl { names, .. } => locals.extend(names.iter().cloned()),
            Stmt::DerivedDecl {
                names, attributes, ..
            } => {
                locals.extend(names.iter().cloned());
                if attributes.iter().any(|a| a == "parameter") {
                    scope.constants.extend(names.iter().cloned());
                }
            }
            Stmt::ArrayDecl { name, .. } => {
                locals.insert(name.clone());
            }
            Stmt::InitializedDecl {
                entities,
                attributes,
                ..
            } => {
                locals.extend(entities.iter().map(|(n, _)| n.clone()));
                if attributes.iter().any(|a| a == "parameter") {
                    scope
                        .constants
                        .extend(entities.iter().map(|(n, _)| n.clone()));
                }
            }
            Stmt::DummyIntent { name, intent } if intent == "parameter" => {
                scope.constants.insert(name.clone());
            }
            Stmt::DummyIntent { name, intent } if intent == "@simple_interface" => {
                scope.procedures.insert(name.clone(), true);
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if intent
                .strip_prefix("procedure(")
                .and_then(|s| s.strip_suffix(')'))
                .is_some_and(|n| scope.procedures.get(n) == Some(&true))
            {
                scope.procedures.insert(name.clone(), true);
            }
        }
    }
    locals
}
fn root(name: &str) -> &str {
    name.split('%').next().unwrap_or(name)
}
fn variable(name: &str, locals: &HashSet<String>, scope: &Scope) -> Result<()> {
    let n = root(name);
    if matches!(
        n,
        "@deferred_extent" | "@assumed_extent" | "@deferred_character"
    ) {
        return Ok(());
    }
    if !n.is_empty()
        && !locals.contains(n)
        && !scope.constants.contains(n)
        && !scope.types.contains(n)
        && !scope.procedures.contains_key(n)
    {
        bail!("SIMPLE procedure cannot reference nonlocal variable `{n}`; pass it as a dummy argument");
    }
    Ok(())
}
fn callable(name: &str, scope: &Scope) -> Result<()> {
    // Internal AST operators aren't procedure calls. Their operands are still
    // visited, and PURE's validation checks side effects of I/O/STOP markers.
    if name.starts_with('@') && !name.starts_with("@module$") || name.contains('%') {
        return Ok(());
    }
    let simple = scope.procedures.get(name).copied().unwrap_or_else(|| {
        crate::intrinsics::purity(name) == Some(crate::intrinsics::Purity::Pure)
            || matches!(name, "move_alloc" | "mvbits")
            || scope.types.contains(name)
    });
    if !simple {
        bail!("SIMPLE procedure cannot reference non-SIMPLE or unknown procedure `{name}`; an explicit SIMPLE interface is required");
    }
    Ok(())
}
fn actuals(name: &str, args: &[Expr], scope: &Scope) -> Result<()> {
    if let Some(required) = scope.callbacks.get(name) {
        for (index, parameter) in required {
            let argument = args
                .iter()
                .find_map(|e| match e {
                    Expr::Call(n, a) if n == &format!("@keyword:{parameter}") => a.first(),
                    _ => None,
                })
                .or_else(|| {
                    args.get(*index)
                        .filter(|e| !matches!(e, Expr::Call(n,_) if n.starts_with("@keyword:")))
                });
            if let Some(Expr::Ident(n)) = argument {
                callable(n, scope)?;
            }
        }
    }
    Ok(())
}
fn expression(e: &Expr, locals: &HashSet<String>, scope: &Scope, calls_only: bool) -> Result<()> {
    match e {
        Expr::Ident(n) => {
            if !calls_only {
                variable(n, locals, scope)?;
            }
        }
        Expr::Call(n, args) | Expr::Index(n, args) => {
            if n == "@output_implied_do" && args.len() >= 4 {
                for e in &args[1..4] {
                    expression(e, locals, scope, calls_only)?;
                }
                let mut inner = locals.clone();
                if let Expr::Ident(n) = &args[0] {
                    inner.insert(n.clone());
                }
                for e in &args[4..] {
                    expression(e, &inner, scope, calls_only)?;
                }
                return Ok(());
            }
            if locals.contains(root(n)) || scope.constants.contains(root(n)) {
                // Array indexing and substring references share Call syntax.
            } else if n.contains('%') {
                if !calls_only {
                    variable(n, locals, scope)?;
                }
                // Resolved type-bound calls are checked in the second pass.
            } else {
                callable(n, scope)?;
            }
            for e in args {
                expression(e, locals, scope, calls_only)?;
            }
        }
        Expr::Section {
            name,
            lower,
            upper,
            stride,
        } => {
            if !calls_only {
                variable(name, locals, scope)?;
            }
            for e in [lower, upper, stride].into_iter().flatten() {
                expression(e, locals, scope, calls_only)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, locals, scope, calls_only)?;
            expression(b, locals, scope, calls_only)?;
        }
        Expr::Un(_, a) => expression(a, locals, scope, calls_only)?,
        _ => {}
    }
    Ok(())
}
fn check(
    body: &[Stmt],
    inherited: &HashSet<String>,
    scope: &Scope,
    calls_only: bool,
) -> Result<()> {
    let mut scope = scope.clone();
    let mut locals = inherited.clone();
    locals.extend(declarations(body, &mut scope));
    // A declaration in this scope shadows a host named constant.
    for n in &locals {
        scope.constants.remove(n);
    }
    let mut offset = None;
    for s in body {
        if let Some(p) = crate::source_diagnostics::statement_offset(s) {
            offset = Some(p);
        }
        let result = (|| -> Result<()> {
            match s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                    if !is_simple(body) {
                        bail!("all internal procedures of a SIMPLE procedure must be SIMPLE");
                    }
                    return Ok(());
                }
                Stmt::DummyIntent { name, intent }
                    if intent == "external" || intent.starts_with("procedure(") =>
                {
                    let interface = intent
                        .strip_prefix("procedure(")
                        .and_then(|i| i.strip_suffix(')'))
                        .unwrap_or(name);
                    let explicit = if intent == "external" {
                        body.iter().any(|s| matches!(s,Stmt::DummyIntent {name:n,intent} if n==name && intent=="@simple_interface"))
                    } else {
                        scope.procedures.get(interface) == Some(&true)
                    };
                    if !explicit {
                        bail!(
                            "SIMPLE dummy procedure `{name}` requires an explicit SIMPLE interface"
                        );
                    }
                    scope.procedures.insert(name.clone(), true);
                }
                Stmt::Assign { name, .. }
                | Stmt::AssignIndex { name, .. }
                | Stmt::Do { var: name, .. }
                    if !calls_only =>
                {
                    variable(name, &locals, &scope)?
                }
                Stmt::CallSub { name, .. } => callable(name, &scope)?,
                _ => {}
            }
            // Specification expressions also reference variables (e.g. LEN(host)).
            let kind = match s {
                Stmt::VarDecl { kind, .. }
                | Stmt::ArrayDecl { kind, .. }
                | Stmt::InitializedDecl { kind, .. } => Some(kind),
                _ => None,
            };
            if let Some(TypeSpec::CharacterLength(e)) = kind {
                expression(e, &locals, &scope, calls_only)?;
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                expression(e, &locals, &scope, calls_only)?;
            }
            for child in crate::procedure_arguments::children(&mut copy) {
                check(child, &locals, &scope, calls_only)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}
fn walk(body: &[Stmt], inherited: &Scope, calls_only: bool) -> Result<()> {
    let mut scope = inherited.clone();
    declarations(body, &mut scope);
    // Module-associated names have unique identities; make module constants,
    // types and procedure characteristics available to USE-associated callers.
    for s in body {
        if let Stmt::Module { body, .. } = s {
            declarations(body, &mut scope);
        }
    }
    for s in body {
        if !calls_only {
            let mut copy = s.clone();
            let mut result = Ok(());
            if let Stmt::CallSub { name, args } = s {
                actuals(name, args, &scope)?;
            }
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(e, &mut |e| {
                    if result.is_ok() {
                        if let Expr::Call(n, a) = e {
                            result = actuals(n, a, &scope);
                        }
                    }
                });
            }
            result?;
        }
        match s {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => {
                if is_simple(body) {
                    if !calls_only {
                        for dummy in crate::procedure_arguments::dummy_procedures(params, body) {
                            let explicit=body.iter().any(|s|match s {
                                Stmt::DummyIntent {name,intent} if name==&dummy => intent=="@simple_interface"
                                    || intent.strip_prefix("procedure(").and_then(|i|i.strip_suffix(')')).is_some_and(|n|scope.procedures.get(n)==Some(&true)
                                        || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n && intent=="@simple_interface"))),
                                _=>false,
                            });
                            if !explicit {
                                return Err(crate::source_diagnostics::declaration(body,&dummy,None,format!("SIMPLE dummy procedure `{dummy}` requires an explicit SIMPLE interface")));
                            }
                        }
                    }
                    let mut locals: HashSet<_> = params.iter().cloned().collect();
                    locals.insert(name.clone());
                    check(body, &locals, &scope, calls_only)?;
                }
                walk(body, &scope, calls_only)?;
            }
            Stmt::Module { body, .. } => walk(body, &scope, calls_only)?,
            _ => {
                let mut copy = s.clone();
                for child in crate::procedure_arguments::children(&mut copy) {
                    walk(child, &scope, calls_only)?;
                }
            }
        }
    }
    Ok(())
}
fn needed(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::DummyIntent { intent, .. } => {
            matches!(intent.as_str(), "@simple" | "@simple_interface")
        }
        Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
            needed(body)
        }
        _ => false,
    })
}
fn bindings(program: &Program) -> Result<()> {
    fn collect(
        body: &[Stmt],
        types: &mut HashMap<String, (Vec<(String, String)>, Option<usize>)>,
        parents: &mut HashMap<String, String>,
        procedures: &mut HashMap<String, bool>,
    ) {
        let mut offset = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                offset = Some(p);
            }
            match s {
                Stmt::DerivedType { name, bindings, .. } => {
                    types.insert(name.clone(), (bindings.clone(), offset));
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() {
                        parents.insert(child.clone(), parent.clone());
                    }
                }
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    procedures.insert(name.clone(), is_simple(body));
                    collect(body, types, parents, procedures);
                }
                Stmt::Module { body, .. } | Stmt::Block { body } => {
                    collect(body, types, parents, procedures)
                }
                _ => {}
            }
        }
    }
    let (mut types, mut parents, mut procedures) = (HashMap::new(), HashMap::new(), HashMap::new());
    collect(&program.body, &mut types, &mut parents, &mut procedures);
    for (child, (bindings, offset)) in &types {
        for (binding, target) in bindings.iter().filter(|(n, _)| !n.starts_with('@')) {
            let mut ancestor = parents.get(child);
            let mut seen = HashSet::new();
            while let Some(parent) = ancestor {
                if !seen.insert(parent) {
                    break;
                } // EXTENDS diagnoses cycles.
                if let Some((base, _)) = types.get(parent) {
                    if let Some((_, original)) = base.iter().find(|(n, _)| n == binding) {
                        if let (Some(a), Some(b)) =
                            (procedures.get(target), procedures.get(original))
                        {
                            if a != b {
                                return Err(crate::source_diagnostics::at(anyhow::anyhow!("overriding binding `{binding}` must have the same SIMPLE characteristic as its inherited binding"),*offset));
                            }
                        }
                        break;
                    }
                }
                ancestor = parents.get(parent);
            }
        }
    }
    Ok(())
}
pub fn validate(program: &Program) -> Result<()> {
    if !needed(&program.body) {
        return Ok(());
    }
    bindings(program)?;
    walk(&program.body, &Scope::default(), false)
}
pub fn validate_calls(program: &Program) -> Result<()> {
    if !needed(&program.body) {
        return Ok(());
    }
    walk(&program.body, &Scope::default(), true)
}

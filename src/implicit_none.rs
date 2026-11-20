// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Under IMPLICIT NONE, a variable that is assigned (`y = 1`) or used as a
//! DO variable must be declared. Reads of undeclared names are diagnosed
//! later; a name that is only defined was accepted and given a type.
//!
//! A scope sees its own declarations, those of its hosts, and the entities
//! of the modules it uses that this source defines. The check errs toward
//! accepting: any name that a declaration or a construct marker mentions
//! counts as declared, and a scope that uses a module defined elsewhere,
//! whose entities are not known here, is not checked.
use crate::ast::{Expr, Implicit, Stmt};
use std::collections::{HashMap, HashSet};

/// Intrinsic modules: their entities are constants, types and procedures,
/// none of which can be assigned, so they hide no variable.
const INTRINSIC_MODULES: &[&str] = &["iso_fortran_env", "iso_c_binding", "ieee_arithmetic", "ieee_exceptions",
    "ieee_features", "omp_lib", "omp_lib_kinds"];

#[derive(Clone, Default)]
struct Scope {
    names: HashSet<String>,
    /// IMPLICIT NONE applies.
    none: bool,
    /// The scope uses a module whose entities are unknown.
    opaque: bool,
}

type Error = (String, Option<usize>);

pub(crate) fn check(body: &[Stmt], contains: &[String]) -> Result<(), Error> {
    let mut modules: HashMap<String, Scope> = HashMap::new();
    for s in body {
        if let Stmt::Module { name, body } = s {
            let scope = unit(body, None, &modules);
            check_unit(body, &scope, &modules)?;
            modules.insert(name.to_ascii_lowercase(), scope);
        }
    }
    let contained: HashSet<String> = contains.iter().map(|n| n.to_ascii_lowercase()).collect();
    let program = unit(body, None, &modules);
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let host = contained.contains(&name.to_ascii_lowercase()).then_some(&program);
                procedure(name, params, body, host, &modules)?;
            }
            _ => {}
        }
    }
    statements(body, &program)
}

/// The scope of a program unit or procedure with this body.
fn unit(body: &[Stmt], host: Option<&Scope>, modules: &HashMap<String, Scope>) -> Scope {
    let mut scope = host.cloned().unwrap_or_default();
    let mut rules = false;
    for s in body {
        match s {
            Stmt::Implicit(Implicit::None) => scope.none = true,
            Stmt::Implicit(_) => rules = true,
            Stmt::Use { module, only, renames, .. } => {
                let module = module.to_ascii_lowercase();
                match modules.get(&module) {
                    Some(used) => {
                        scope.opaque |= used.opaque;
                        scope.names.extend(used.names.iter().cloned());
                        scope.names.extend(renames.iter().map(|(local, _)| local.to_ascii_lowercase()));
                        scope.names.extend(only.iter().flatten().map(|n| n.to_ascii_lowercase()));
                    }
                    None if INTRINSIC_MODULES.contains(&module.as_str()) => {}
                    None => scope.opaque = true,
                }
            }
            _ => {}
        }
    }
    if rules {
        scope.none = false;
    }
    declared(body, &mut scope.names);
    scope
}

fn procedure(name: &str, params: &[String], body: &[Stmt], host: Option<&Scope>, modules: &HashMap<String, Scope>) -> Result<(), Error> {
    let mut scope = unit(body, host, modules);
    scope.names.insert(name.to_ascii_lowercase());
    // A RESULT variable: an undeclared one gets its own diagnostic.
    if let Some(Stmt::Return(Some(Expr::Ident(result)))) = body.last() {
        scope.names.insert(result.to_ascii_lowercase());
    }
    scope.names.extend(params.iter().map(|p| p.to_ascii_lowercase()));
    check_unit(body, &scope, modules)?;
    statements(body, &scope)
}

/// Check the procedures that `body`, a module or procedure, contains.
fn check_unit(body: &[Stmt], scope: &Scope, modules: &HashMap<String, Scope>) -> Result<(), Error> {
    for s in body {
        if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
            procedure(name, params, body, Some(scope), modules)?;
        }
    }
    Ok(())
}

/// Add the names that `body` declares or mentions in a construct marker.
pub(crate) fn declared(body: &[Stmt], names: &mut HashSet<String>) {
    for s in body {
        let mut add = |n: &str| {
            names.insert(n.to_ascii_lowercase());
        };
        match s {
            Stmt::VarDecl { names: n, .. } | Stmt::DerivedDecl { names: n, .. } | Stmt::Accessibility { names: n, .. } => {
                n.iter().for_each(|n| add(n))
            }
            Stmt::InitializedDecl { entities, .. } => entities.iter().for_each(|(n, _)| add(n)),
            Stmt::ArrayDecl { name, .. } | Stmt::DummyIntent { name, .. } | Stmt::GenericInterface { name, .. } => add(name),
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => add(name),
            Stmt::CallSub { name, args } if name.starts_with('@') => {
                let mut found = Vec::new();
                for a in args {
                    mentioned(a, &mut found);
                }
                found.iter().for_each(|n| add(n));
            }
            // SELECT TYPE and SELECT RANK, with their associate names.
            Stmt::SelectCase { expr: e @ Expr::Call(name, _), .. } if name.starts_with('@') => {
                let mut found = Vec::new();
                mentioned(e, &mut found);
                found.iter().for_each(|n| add(n));
            }
            _ => {}
        }
        for child in nested(s) {
            declared(child, names);
        }
    }
}

fn mentioned(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n) | Expr::Str(n) => out.push(n.clone()),
        Expr::Call(_, args) => args.iter().for_each(|a| mentioned(a, out)),
        _ => {}
    }
}

/// The statement lists inside a construct, not inside a nested scope.
fn nested(s: &Stmt) -> Vec<&[Stmt]> {
    match s {
        Stmt::If { then_body, else_body, .. } => {
            let mut v = vec![then_body.as_slice()];
            if let Some(e) = else_body {
                v.push(e);
            }
            v
        }
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => vec![body],
        Stmt::SelectCase { cases, default, .. } => {
            let mut v: Vec<&[Stmt]> = cases.iter().map(|c| c.body.as_slice()).collect();
            if let Some(d) = default {
                v.push(d);
            }
            v
        }
        _ => vec![],
    }
}

/// Report the first assignment target or DO variable without a declaration.
fn statements(body: &[Stmt], scope: &Scope) -> Result<(), Error> {
    if !scope.none || scope.opaque {
        return Ok(());
    }
    fn walk(body: &[Stmt], scope: &Scope, at: &mut Option<usize>) -> Result<(), Error> {
        for s in body {
            let target = match s {
                Stmt::SourceLocation(p) => {
                    *at = Some(*p);
                    None
                }
                Stmt::Assign { name, .. } => Some(name),
                Stmt::Do { var, .. } => Some(var),
                _ => None,
            };
            if let Some(name) = target {
                let lower = name.to_ascii_lowercase();
                if !lower.starts_with('@') && !lower.contains(['%', '(']) && !scope.names.contains(&lower) {
                    return Err((format!("`{name}` has no IMPLICIT type: IMPLICIT NONE is in effect, so declare it"), *at));
                }
            }
            for child in nested(s) {
                walk(child, scope, at)?;
            }
        }
        Ok(())
    }
    walk(body, scope, &mut None)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Saved ALLOCATABLE and POINTER locals of intrinsic type, such as
//! `real, allocatable, save :: work(:)` or `real, pointer :: p(:) => null()`
//! (an initialization implies SAVE), keep their storage in a generated
//! module, which the procedure uses with an ONLY list. A saved local has one
//! instance, kept between calls and shared by recursive activations, as a
//! module variable has, so the module variable is exact. `data_statements`
//! marks the variables (SAVED_STORAGE). The module sees the USE statements
//! and named constants of the procedure and of its hosts, for kinds and
//! lengths.
use crate::ast::{Program, Stmt};
use anyhow::Result;
use std::collections::HashSet;

/// The prefix of the generated modules: `rfort_save_<n>_<m>_<unit>` for a
/// procedure of module m, whose name has n characters and whose object owns
/// the storage (a program that uses m compiles m's procedures too, and
/// imports it), and `rfort_save_0__<unit>` for another procedure, whose
/// object alone has it.
pub(crate) const MODULE: &str = "rfort_save_";

fn module_name(owner: &str, unit: &str) -> String {
    format!("{MODULE}{}_{owner}_{unit}", owner.len())
}

/// The module whose object defines the storage of a generated module: Some
/// of a module name, Some("") for the object that has the procedure, or None
/// when `module` is not generated here.
pub(crate) fn owner(module: &str) -> Option<&str> {
    let rest = module.strip_prefix(MODULE)?;
    let (length, rest) = rest.split_once('_')?;
    rest.get(..length.parse::<usize>().ok()?)
}

fn parameter(s: &Stmt) -> bool {
    matches!(s, Stmt::InitializedDecl { attributes, .. } if attributes.iter().any(|a| a == "parameter"))
}

fn constant_names(s: &Stmt) -> Vec<String> {
    match s {
        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
        _ => vec![],
    }
}

/// The USE statements and named constants a scope sees: its host's, then
/// its own, which hide the host's constants of the same names.
#[derive(Clone, Default)]
struct Context {
    uses: Vec<Stmt>,
    constants: Vec<Stmt>,
}

impl Context {
    fn inner(&self, body: &[Stmt]) -> Context {
        let mut out = self.clone();
        out.uses.extend(body.iter().filter(|s| matches!(s, Stmt::Use { .. })).cloned());
        let own: Vec<Stmt> = body.iter().filter(|s| parameter(s)).cloned().collect();
        let hidden: HashSet<String> = own.iter().flat_map(constant_names).collect();
        out.constants.retain(|s| constant_names(s).iter().all(|n| !hidden.contains(n)));
        out.constants.extend(own);
        out
    }
}

fn procedures(body: &mut [Stmt], owner: &str, unit: &str, context: &Context, at: usize, modules: &mut Vec<(usize, Stmt)>) {
    for s in body.iter_mut() {
        if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
            let unit = if unit.is_empty() { name.clone() } else { format!("{unit}_{name}") };
            procedure(body, owner, &unit, context, at, modules);
        }
    }
}

fn procedure(body: &mut Vec<Stmt>, owner: &str, unit: &str, host: &Context, at: usize, modules: &mut Vec<(usize, Stmt)>) {
    let context = host.inner(body);
    procedures(body, owner, unit, &context, at, modules);
    let mut moved: Vec<String> = vec![];
    for s in body.iter() {
        if let Stmt::DummyIntent { name, intent } = s {
            if intent == crate::data_statements::SAVED_STORAGE && !moved.contains(name) { moved.push(name.clone()); }
        }
    }
    if moved.is_empty() { return; }
    let set: HashSet<&String> = moved.iter().collect();
    let module = module_name(owner, unit);
    let mut contents = context.uses.clone();
    contents.push(Stmt::Implicit(crate::ast::Implicit::None));
    contents.extend(context.constants.iter().cloned());
    // The declarations move to the module, the variable's attributes first.
    for s in body.iter() {
        if let Stmt::DummyIntent { name, intent } = s {
            if set.contains(name) && !matches!(intent.as_str(), "save") && intent != crate::data_statements::SAVED_STORAGE {
                contents.push(s.clone());
            }
        }
    }
    let mut kept = vec![Stmt::Use { module: module.clone(), nature: None, only: Some(moved.clone()), renames: vec![] }];
    for s in std::mem::take(body) {
        match s {
            Stmt::DummyIntent { ref name, .. } if set.contains(name) => {}
            Stmt::ArrayDecl { ref name, .. } if set.contains(name) => contents.push(s),
            Stmt::VarDecl { kind, names } => {
                let (here, there): (Vec<String>, Vec<String>) = names.into_iter().partition(|n| !set.contains(n));
                if !there.is_empty() { contents.push(Stmt::VarDecl { kind: kind.clone(), names: there }); }
                if !here.is_empty() { kept.push(Stmt::VarDecl { kind, names: here }); }
            }
            Stmt::InitializedDecl { kind, attributes, entities } => {
                let (here, there): (Vec<_>, Vec<_>) = entities.into_iter().partition(|(n, _)| !set.contains(n));
                if !there.is_empty() {
                    contents.push(Stmt::VarDecl { kind: kind.clone(), names: there.into_iter().map(|(n, _)| n).collect() });
                }
                if !here.is_empty() { kept.push(Stmt::InitializedDecl { kind, attributes, entities: here }); }
            }
            other => kept.push(other),
        }
    }
    *body = kept;
    modules.push((at, Stmt::Module { name: module, body: contents }));
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut modules = vec![];
    // The main program's USE statements and constants, for the procedures it
    // contains.
    let main = Context::default().inner(&p.body.iter().filter(|s| !matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. })).cloned().collect::<Vec<_>>());
    let contains = p.contains.clone();
    let none = Context::default();
    for (i, s) in p.body.iter_mut().enumerate() {
        match s {
            Stmt::Module { name, body } => {
                let context = Context::default().inner(body);
                let owner = name.clone();
                procedures(body, &owner, "", &context, i, &mut modules);
            }
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                let internal = contains.contains(&name.to_ascii_lowercase()) || contains.contains(name);
                let unit = if internal { format!("main_{}_{name}", p.name) } else { name.clone() };
                procedure(body, "", &unit, if internal { &main } else { &none }, i, &mut modules);
            }
            _ => {}
        }
    }
    // Each module goes before the unit of its procedure, in order.
    modules.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, m) in modules {
        p.body.insert(at, m);
    }
    Ok(p)
}

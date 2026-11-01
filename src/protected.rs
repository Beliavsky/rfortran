// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! The PROTECTED attribute. A PROTECTED module variable may be defined only
//! in its module and the module's procedures; elsewhere, where USE makes it
//! available, it may not be assigned, read into, used as a DO variable,
//! allocated, deallocated or passed to an INTENT(OUT) or INTENT(INOUT)
//! dummy, and a PROTECTED pointer's association may not change (its target
//! may). The attribute changes nothing else, so after these checks it is
//! removed from the program.
use crate::ast::{Expr, Program, Stmt};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// A PROTECTED variable visible in a scope: its module, and whether it is a
/// POINTER.
#[derive(Clone)]
struct Protected {
    module: String,
    pointer: bool,
}
type Visible = HashMap<String, Protected>;

/// The PROTECTED variables of a module body, which loses the attribute.
fn own(body: &mut Vec<Stmt>) -> HashSet<String> {
    let mut names = HashSet::new();
    body.retain(|s| match s {
        Stmt::DummyIntent { name, intent } if intent == "protected" => { names.insert(name.clone()); false }
        _ => true,
    });
    for s in body.iter_mut() {
        match s {
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "protected") => {
                names.extend(entities.iter().map(|(n, _)| n.clone()));
                attributes.retain(|a| a != "protected");
            }
            Stmt::DerivedDecl { attributes, names: n, .. } if attributes.iter().any(|a| a == "protected") => {
                names.extend(n.iter().cloned());
                attributes.retain(|a| a != "protected");
            }
            _ => {}
        }
    }
    names
}

fn pointers(body: &[Stmt]) -> HashSet<String> {
    body.iter().flat_map(|s| match s {
        Stmt::DummyIntent { name, intent } if intent == "pointer" => vec![name.clone()],
        Stmt::DerivedDecl { names, attributes, .. } if attributes.iter().any(|a| a == "pointer") => names.clone(),
        _ => vec![],
    }).collect()
}

/// Remove the attribute where a module was not its owner (an internal
/// procedure or a main program cannot have it, but keep the program valid).
fn strip(body: &mut Vec<Stmt>) {
    own(body);
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => strip(body),
            _ => {}
        }
    }
}

/// The variable a designator defines: the name before any component or
/// subscript.
fn base(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } if !n.starts_with('@') => n.split('%').next(),
        Expr::Call(n, args) if n.starts_with("@record_component:") || n == "@array_substring" => args.first().and_then(base),
        _ => None,
    }
}

/// The dummy arguments of each procedure that its body may define.
type Outputs = HashMap<String, Vec<bool>>;

fn outputs(body: &[Stmt], out: &mut Outputs) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let defined: Vec<bool> = params.iter().map(|p| body.iter().any(|d| match d {
                    Stmt::DummyIntent { name, intent } => name == p && (intent == "intent(out)" || intent == "intent(inout)"),
                    Stmt::DerivedDecl { names, attributes, .. } => names.contains(p) && attributes.iter().any(|a| a == "intent(out)" || a == "intent(inout)"),
                    _ => false,
                })).collect();
                out.insert(name.clone(), defined);
                outputs(body, out);
            }
            Stmt::Module { body, .. } => outputs(body, out),
            _ => {}
        }
    }
}

struct Check<'a> {
    outputs: &'a Outputs,
}

impl Check<'_> {
    fn error(name: &str, p: &Protected, what: &str) -> anyhow::Error {
        anyhow::anyhow!("`{name}` is PROTECTED in module `{}` and cannot be {what} outside it", p.module)
    }

    /// A definition of the variable that `e` designates.
    fn defined(e: &Expr, visible: &Visible, what: &str) -> Result<()> {
        if let Some(n) = base(e) {
            if let Some(p) = visible.get(n) {
                // A PROTECTED pointer's target may be defined.
                if !(p.pointer && matches!(what, "assigned" | "read into")) {
                    return Err(Self::error(n, p, what));
                }
            }
        }
        Ok(())
    }

    /// The items of an input statement after its unit, controls and format.
    fn inputs(args: &[Expr], visible: &Visible) -> Result<()> {
        let mut i = 1;
        while i < args.len() {
            match &args[i] {
                // A control is a name and a value; IOSTAT, IOMSG and SIZE define theirs.
                Expr::Str(s) if s.is_empty() || s.chars().all(|c| c.is_ascii_alphabetic()) => {
                    if matches!(s.to_ascii_lowercase().as_str(), "iostat" | "iomsg" | "size") {
                        if let Some(v) = args.get(i + 1) { Self::defined(v, visible, "read into")?; }
                    }
                    i += 2;
                    continue;
                }
                Expr::Str(_) => {}
                Expr::Call(n, a) if n.ends_with("implied_do") => {
                    if let Some(v) = a.first() { Self::defined(v, visible, "used as a DO variable")?; }
                    let mut items = vec![Expr::IntLit("0".into())];
                    items.extend(a.iter().skip(4).cloned());
                    Self::inputs(&items, visible)?;
                }
                item => Self::defined(item, visible, "read into")?,
            }
            i += 1;
        }
        Ok(())
    }

    fn actuals(&self, name: &str, args: &[Expr], visible: &Visible) -> Result<()> {
        let Some(defined) = self.outputs.get(name) else { return Ok(()) };
        for (i, a) in args.iter().enumerate() {
            let a = match a { Expr::Call(k, v) if k.starts_with("@keyword:") => &v[0], other => other };
            if defined.get(i).copied().unwrap_or(false) {
                if let Some(n) = base(a) {
                    if let Some(p) = visible.get(n) { return Err(Self::error(n, p, &format!("passed to an INTENT(OUT) or INTENT(INOUT) dummy of `{name}`"))); }
                }
            }
        }
        Ok(())
    }

    fn expr(&self, e: &Expr, visible: &Visible) -> Result<()> {
        let mut e = e.clone();
        let mut result = Ok(());
        crate::procedure_arguments::visit_expr(&mut e, &mut |x| {
            if result.is_err() { return; }
            if let Expr::Call(n, args) = x { result = self.actuals(n, args, visible); }
        });
        result
    }

    fn statements(&self, body: &mut [Stmt], visible: &Visible) -> Result<()> {
        for s in body.iter_mut() {
            match s {
                Stmt::Assign { name, .. } => Self::defined(&Expr::Ident(name.clone()), visible, "assigned")?,
                Stmt::AssignIndex { name, indices, .. } => Self::defined(&Expr::Call(name.clone(), indices.clone()), visible, "assigned")?,
                Stmt::Do { var, .. } => Self::defined(&Expr::Ident(var.clone()), visible, "used as a DO variable")?,
                Stmt::CallSub { name, args } => match name.as_str() {
                    "@allocate" | "@deallocate" => for a in args {
                        if !matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:")) {
                            if let Some(n) = base(a) { if let Some(p) = visible.get(n) { return Err(Self::error(n, p, "allocated or deallocated")); } }
                        }
                    },
                    "@allocate_derived_type" => for a in args.iter().skip(1) {
                        if let Some(n) = base(a) { if let Some(p) = visible.get(n) { return Err(Self::error(n, p, "allocated")); } }
                    },
                    n if n == crate::substrings::ALLOCATE_CHARACTER => for a in args.iter().skip(1) {
                        if let Some(n) = base(a) { if let Some(p) = visible.get(n) { return Err(Self::error(n, p, "allocated")); } }
                    },
                    "@pointer_associate" | "@nullify" => {
                        let targets: &[Expr] = if name == "@nullify" { args } else { &args[..1] };
                        for a in targets {
                            if let (Some(n), Expr::Ident(full)) = (base(a), a) {
                                if let Some(p) = visible.get(n) {
                                    if n == full { return Err(Self::error(n, p, "pointer associated or nullified")); }
                                }
                            }
                        }
                    }
                    n if n.eq_ignore_ascii_case("move_alloc") => for a in args {
                        if let Some(n) = base(a) { if let Some(p) = visible.get(n) { return Err(Self::error(n, p, "moved by MOVE_ALLOC")); } }
                    },
                    n if n.starts_with('@') && n.contains("read") => Self::inputs(args, visible)?,
                    n => self.actuals(n, args, visible)?,
                },
                _ => {}
            }
            for e in crate::procedure_arguments::expressions(s) { self.expr(e, visible)?; }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
                for b in crate::procedure_arguments::children(s) { self.statements(b, visible)?; }
            }
        }
        Ok(())
    }

    /// Check a scope that is not the variables' module, and the procedures
    /// it contains.
    fn scope(&self, body: &mut Vec<Stmt>, host: &Visible, exports: &HashMap<String, Visible>) -> Result<()> {
        let mut visible = host.clone();
        for s in body.iter() {
            let Stmt::Use { module, only, renames, .. } = s else { continue };
            let Some(exported) = exports.get(module) else { continue };
            for (remote, p) in exported {
                let local = renames.iter().find(|(_, r)| r == remote).map_or(remote.clone(), |(l, _)| l.clone());
                let listed = only.as_ref().is_none_or(|o| o.contains(&local) || o.contains(remote));
                if listed { visible.insert(local, p.clone()); }
            }
        }
        // A local declaration hides a variable of a module.
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => for n in names { visible.remove(n); },
                Stmt::ArrayDecl { name, .. } => { visible.remove(name); }
                Stmt::InitializedDecl { entities, .. } => for (n, _) in entities { visible.remove(n); },
                _ => {}
            }
        }
        if !visible.is_empty() { self.statements(body, &visible)?; }
        for s in body.iter_mut() {
            match s {
                Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                    let mut inner = visible.clone();
                    for p in params.iter() { inner.remove(p); }
                    self.scope(body, &inner, exports)?;
                }
                Stmt::Block { body } => self.scope(body, &visible, exports)?,
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    // Each module's PROTECTED variables, and those it makes available by USE.
    let mut exports: HashMap<String, Visible> = HashMap::new();
    let mut uses: HashMap<String, Vec<Stmt>> = HashMap::new();
    for s in program.body.iter_mut() {
        if let Stmt::Module { name, body } = s {
            let names = own(body);
            let pointer = pointers(body);
            exports.insert(name.clone(), names.into_iter().map(|n| { let p = Protected { module: name.clone(), pointer: pointer.contains(&n) }; (n, p) }).collect());
            uses.insert(name.clone(), body.iter().filter(|s| matches!(s, Stmt::Use { .. })).cloned().collect());
        }
    }
    if exports.values().all(|v| v.is_empty()) {
        strip(&mut program.body);
        return Ok(program);
    }
    for _ in 0..exports.len() + 1 {
        let mut changed = false;
        for (m, list) in &uses {
            for u in list {
                let Stmt::Use { module, only, renames, .. } = u else { continue };
                let Some(from) = exports.get(module).cloned() else { continue };
                let entry = exports.get_mut(m).unwrap();
                for (remote, p) in from {
                    let local = renames.iter().find(|(_, r)| *r == remote).map_or(remote.clone(), |(l, _)| l.clone());
                    if only.as_ref().is_some_and(|o| !o.contains(&local) && !o.contains(&remote)) { continue; }
                    if !entry.contains_key(&local) { entry.insert(local, p); changed = true; }
                }
            }
        }
        if !changed { break; }
    }
    let mut outs = Outputs::new();
    outputs(&program.body, &mut outs);
    let check = Check { outputs: &outs };
    // A module's own procedures may define its variables; other modules'
    // procedures may not.
    for s in program.body.iter_mut() {
        match s {
            Stmt::Module { name, body } => {
                let host: Visible = exports.get(name.as_str()).map(|v| v.iter().filter(|(_, p)| p.module != *name).map(|(n, p)| (n.clone(), p.clone())).collect()).unwrap_or_default();
                // The module's own variables are not visible as another's.
                let mut others = exports.clone();
                others.remove(name.as_str());
                check.scope(body, &host, &others)?;
            }
            _ => {}
        }
    }
    // The main program, with its internal procedures; external procedures
    // have no host.
    let contains = program.contains.clone();
    let mut main: Vec<Stmt> = program.body.iter().filter(|s| match s {
        Stmt::Module { .. } => false,
        Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => contains.contains(name),
        _ => true,
    }).cloned().collect();
    check.scope(&mut main, &Visible::new(), &exports)?;
    for s in program.body.iter() {
        if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
            if !contains.contains(name) {
                let mut unit = vec![s.clone()];
                check.scope(&mut unit, &Visible::new(), &exports)?;
            }
        }
    }
    strip(&mut program.body);
    Ok(program)
}

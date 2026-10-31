// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! ALLOCATABLE scalar components: an intrinsic scalar, a deferred-length
//! CHARACTER scalar or a scalar of another derived type. Each becomes a
//! one-element ALLOCATABLE array component, which has runtime storage in
//! scalar objects, in arrays of the type and in procedure arguments:
//!
//! * a reference `x%r` is `x%r(1)`, and `v(i)%r` is `v(i)%r(1)`;
//! * `x%c%k`, through a derived component `c`, is `x%c%k(1)`;
//! * assignment `x%r = value` is `x%r = [value]`, which allocates it;
//! * `allocate(x%r)` is `allocate(x%r(1))`;
//! * ALLOCATED, DEALLOCATE and MOVE_ALLOC use the component as a whole.
//!
//! A component of the type that contains it, as in a linked list, is not
//! one of these; it needs records with storage of their own.
use crate::ast::{Expr, Program, Stmt};
use std::collections::{HashMap, HashSet};

const DEFERRED: &str = "@deferred_extent";

#[derive(Default)]
struct Types {
    /// The rewritten components of each type.
    rewritten: HashMap<String, HashSet<String>>,
    /// The derived-type components of each type, with their types.
    derived: HashMap<String, HashMap<String, String>>,
    /// The components of each type in order, for structure constructors.
    order: HashMap<String, Vec<String>>,
    parents: HashMap<String, String>,
    /// The derived-type variables of each module.
    modules: HashMap<String, HashMap<String, String>>,
    /// Types with a FINAL procedure, whose ALLOCATABLE scalars have owned
    /// storage of their own (see crate::select_type::runtime).
    finalized: HashSet<String>,
    owned_scalars: bool,
}

fn one() -> Expr { Expr::IntLit("1".into()) }
fn subscripts() -> Expr { Expr::Call("@component_subscripts".into(), vec![one()]) }

impl Types {
    /// Types with a FINAL procedure, and whether any ALLOCATABLE scalar
    /// variable or dummy has a derived type: both use the owned scalar
    /// storage of crate::select_type::runtime for derived components too.
    fn finalized(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::DerivedType { name, bindings, .. } if bindings.iter().any(|(b, _)| b.starts_with("@final")) => { self.finalized.insert(name.clone()); }
                Stmt::DerivedDecl { attributes, dims, polymorphic: false, .. } if dims.is_empty() && attributes.iter().any(|a| a == "allocatable") => self.owned_scalars = true,
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => self.finalized(body),
                _ => {}
            }
        }
    }

    fn collect(&mut self, body: &mut Vec<Stmt>) {
        for s in body.iter_mut() {
            match s {
                Stmt::DerivedType { name, components, .. } => self.definition(name, components),
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() {
                        self.parents.insert(child.clone(), parent.clone());
                    }
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => self.collect(body),
                _ => {}
            }
        }
    }

    /// Make the ALLOCATABLE scalar components of a definition one-element
    /// arrays.
    fn definition(&mut self, name: &str, components: &mut Vec<Stmt>) {
        let allocatable: HashSet<String> = components.iter().filter_map(|c| match c {
            Stmt::DummyIntent { name, intent } if intent == "allocatable" => Some(name.clone()),
            _ => None,
        }).collect();
        let mut rewritten = HashSet::new();
        let mut order = vec![];
        let mut derived = HashMap::new();
        let mut out = vec![];
        for c in components.drain(..) {
            match c {
                Stmt::VarDecl { kind, names } => {
                    order.extend(names.iter().cloned());
                    let (scalar, array): (Vec<String>, Vec<String>) = names.into_iter().partition(|n| allocatable.contains(n));
                    if !array.is_empty() { out.push(Stmt::VarDecl { kind: kind.clone(), names: array }); }
                    for n in scalar {
                        rewritten.insert(n.clone());
                        out.push(Stmt::ArrayDecl { kind: kind.clone(), name: n, dims: vec![Expr::Ident(DEFERRED.into())] });
                    }
                }
                Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims } => {
                    order.extend(names.iter().cloned());
                    for n in &names { derived.insert(n.clone(), type_name.clone()); }
                    let scalar = dims.is_empty() && attributes.iter().any(|a| a == "allocatable") && !polymorphic
                        && type_name != name && !self.finalized.contains(&type_name) && !self.owned_scalars;
                    if scalar {
                        rewritten.extend(names.iter().cloned());
                        out.push(Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims: vec![Expr::Ident(DEFERRED.into())] });
                    } else {
                        out.push(Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims });
                    }
                }
                Stmt::ArrayDecl { ref name, .. } => { order.push(name.clone()); out.push(c); }
                Stmt::InitializedDecl { ref entities, .. } => { order.extend(entities.iter().map(|(n, _)| n.clone())); out.push(c); }
                other => out.push(other),
            }
        }
        *components = out;
        self.rewritten.insert(name.to_owned(), rewritten);
        self.derived.insert(name.to_owned(), derived);
        self.order.insert(name.to_owned(), order);
    }

    /// Whether component `c` of type `ty`, or of a type it extends, was
    /// rewritten, and its type if it is of a derived type.
    fn component(&self, ty: &str, c: &str) -> Option<(bool, Option<String>)> {
        let mut ty = ty.to_owned();
        for _ in 0..64 {
            if let Some(order) = self.order.get(&ty) {
                if order.iter().any(|n| n == c) {
                    let rewritten = self.rewritten.get(&ty).is_some_and(|r| r.contains(c));
                    return Some((rewritten, self.derived.get(&ty).and_then(|d| d.get(c)).cloned()));
                }
            }
            ty = self.parents.get(&ty)?.clone();
        }
        None
    }

    /// All components of a type in constructor order, the parent's first.
    fn constructor_order(&self, ty: &str) -> Vec<String> {
        let mut out = self.parents.get(ty).map(|p| self.constructor_order(p)).unwrap_or_default();
        out.extend(self.order.get(ty).cloned().unwrap_or_default());
        out
    }

    fn rewritten_in(&self, ty: &str, c: &str) -> bool {
        self.component(ty, c).is_some_and(|(r, _)| r)
    }

    fn any(&self) -> bool { self.rewritten.values().any(|r| !r.is_empty()) }
}

/// The designator `x%c%w(subscripts)` through the rewritten component at
/// part `at` of the path: the components after it of the element `x%c(1)`.
fn through(path: &str, at: usize, subscripts: Vec<Expr>) -> Expr {
    let parts: Vec<&str> = path.split('%').collect();
    let mut e = Expr::Call(parts[..=at].join("%"), vec![one()]);
    for (i, field) in parts.iter().enumerate().skip(at + 1) {
        let mut args = vec![e];
        if i + 1 == parts.len() && !subscripts.is_empty() {
            args.push(Expr::Call("@component_subscripts".into(), subscripts.clone()));
        }
        e = Expr::Call(format!("@record_component:{field}"), args);
    }
    e
}

/// The scope's derived-type variables, by name.
type Vars = HashMap<String, String>;

struct Rewriter<'a> {
    types: &'a Types,
}

/// How a path `x%a%b` resolves: the index of the first rewritten
/// component, and whether it is the last.
fn resolve_path(types: &Types, vars: &Vars, path: &str) -> Option<(usize, bool)> {
    let parts: Vec<&str> = path.split('%').collect();
    let mut ty = vars.get(parts[0])?.clone();
    for (i, c) in parts.iter().enumerate().skip(1) {
        let (rewritten, derived) = types.component(&ty, c)?;
        if rewritten { return Some((i, i + 1 == parts.len())); }
        ty = derived?;
    }
    None
}

impl Rewriter<'_> {
    /// The type of a record designator: a variable, an element of an array
    /// of records, or a component of either.
    fn type_of(&self, e: &Expr, vars: &Vars) -> Option<String> {
        match e {
            Expr::Ident(n) | Expr::Call(n, _) if !n.starts_with('@') => {
                let parts: Vec<&str> = n.split('%').collect();
                let mut ty = vars.get(parts[0])?.clone();
                for c in &parts[1..] { ty = self.types.component(&ty, c)?.1?; }
                Some(ty)
            }
            Expr::Call(n, args) if n.starts_with("@record_component:") => {
                let base = self.type_of(args.first()?, vars)?;
                self.types.component(&base, &n["@record_component:".len()..])?.1
            }
            _ => None,
        }
    }

    /// Rewrite an expression; `whole` when it names a component as a whole,
    /// as the argument of ALLOCATED.
    fn expr(&self, e: &mut Expr, vars: &Vars, whole: bool) {
        match e {
            Expr::Ident(n) if n.contains('%') => {
                match resolve_path(self.types, vars, n) {
                    Some((_, true)) if !whole => *e = Expr::Call(n.clone(), vec![one()]),
                    // x%c%w, a component of the rewritten component c: of its element.
                    Some((at, false)) => *e = through(n, at, vec![]),
                    _ => {}
                }
            }
            // x%c%w(i): an element of a component of the rewritten component c.
            Expr::Call(n, args) if n.contains('%') && resolve_path(self.types, vars, n).is_some_and(|(_, last)| !last) => {
                for a in args.iter_mut() { self.expr(a, vars, false); }
                let at = resolve_path(self.types, vars, n).unwrap().0;
                *e = through(n, at, std::mem::take(args));
            }
            Expr::Call(n, args) if matches!(n.to_ascii_lowercase().as_str(), "allocated" | "move_alloc" | "@deallocate") => {
                for a in args.iter_mut() { self.expr(a, vars, true); }
            }
            // A substring of a deferred-length CHARACTER component.
            Expr::Call(n, args) if n.contains('%') && args.len() == 1 && matches!(args[0], Expr::Section { .. })
                && resolve_path(self.types, vars, n).is_some_and(|(_, last)| last) => {
                let mut range = args[0].clone();
                if let Expr::Section { name, .. } = &mut range { name.clear(); }
                *e = Expr::Call("@array_substring".into(), vec![Expr::Call(n.clone(), vec![one()]), range]);
            }
            Expr::Call(n, args) if n.starts_with("@record_component:") => {
                for a in args.iter_mut() { self.expr(a, vars, false); }
                let field = &n["@record_component:".len()..];
                let rewritten = args.first().and_then(|b| self.type_of(b, vars)).is_some_and(|ty| self.types.rewritten_in(&ty, field));
                let indexed = args.iter().any(|a| matches!(a, Expr::Call(t, _) if t == "@component_subscripts"));
                if rewritten && !indexed && !whole { args.push(subscripts()); }
            }
            Expr::Call(n, args) => {
                // A structure constructor gives a rewritten component a
                // one-element array value.
                let order = (!n.starts_with('@') && self.types.order.contains_key(n.as_str()) && !vars.contains_key(n.as_str()))
                    .then(|| self.types.constructor_order(n));
                for (i, a) in args.iter_mut().enumerate() {
                    self.expr(a, vars, false);
                    let Some(order) = &order else { continue };
                    let component = match &*a {
                        Expr::Call(k, _) if k.starts_with("@keyword:") => k["@keyword:".len()..].to_owned(),
                        _ => match order.get(i) { Some(c) => c.clone(), None => continue },
                    };
                    if !self.types.rewritten_in(n, &component) { continue; }
                    match a {
                        Expr::Call(k, v) if k.starts_with("@keyword:") && !v.is_empty() =>
                            v[0] = Expr::Call("@array_constructor".into(), vec![v[0].clone()]),
                        _ => *a = Expr::Call("@array_constructor".into(), vec![a.clone()]),
                    }
                }
            }
            Expr::Index(_, args) => for a in args { self.expr(a, vars, false) },
            Expr::Bin(_, a, b) => { self.expr(a, vars, false); self.expr(b, vars, false); }
            Expr::Un(_, a) => self.expr(a, vars, false),
            Expr::Section { name, lower, upper, stride } => {
                for b in [lower.as_deref_mut(), upper.as_deref_mut(), stride.as_deref_mut()].into_iter().flatten() { self.expr(b, vars, false); }
                // x%s(i:j), a substring of a deferred-length CHARACTER component.
                if name.contains('%') && resolve_path(self.types, vars, name).is_some_and(|(_, last)| last) {
                    let element = Expr::Call(name.clone(), vec![one()]);
                    let mut range = e.clone();
                    if let Expr::Section { name, .. } = &mut range { name.clear(); }
                    *e = Expr::Call("@array_substring".into(), vec![element, range]);
                }
            }
            _ => {}
        }
    }

    fn scope(&self, body: &mut Vec<Stmt>, host: &Vars) {
        let mut vars = host.clone();
        for s in body.iter() {
            match s {
                Stmt::Use { module, renames, .. } => {
                    if let Some(exported) = self.types.modules.get(module) {
                        for (n, t) in exported {
                            let local = renames.iter().find(|(_, r)| r == n).map_or(n.clone(), |(l, _)| l.clone());
                            vars.insert(local, t.clone());
                        }
                    }
                }
                Stmt::DerivedDecl { type_name, names, .. } => for n in names { vars.insert(n.clone(), type_name.clone()); },
                Stmt::VarDecl { names, .. } => for n in names { vars.remove(n); },
                Stmt::ArrayDecl { name, .. } => { vars.remove(name); }
                _ => {}
            }
        }
        self.statements(body, &vars);
    }

    fn statements(&self, body: &mut Vec<Stmt>, vars: &Vars) {
        for s in body.iter_mut() {
            match s {
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => { self.scope(body, vars); continue; }
                Stmt::Block { body } => { self.scope(body, vars); continue; }
                Stmt::Assign { name, value } => {
                    self.expr(value, vars, false);
                    match resolve_path(self.types, vars, name) {
                        // Intrinsic assignment allocates the component.
                        Some((_, true)) => *value = Expr::Call("@array_constructor".into(), vec![value.clone()]),
                        // A component of a rewritten derived component.
                        Some((_, false)) => *s = Stmt::AssignIndex { name: name.clone(), indices: vec![one()], value: value.clone() },
                        None => {}
                    }
                    continue;
                }
                Stmt::AssignIndex { name, indices, value } => {
                    self.expr(value, vars, false);
                    for i in indices.iter_mut() { self.expr(i, vars, false); }
                    let parts: Vec<&str> = name.split('%').collect();
                    // v(i)%r = value of an array of records.
                    if parts.len() == 2 && vars.contains_key(parts[0]) && self.types.rewritten_in(&vars[parts[0]], parts[1])
                        && !indices.iter().any(|i| matches!(i, Expr::Call(t, _) if t == "@component_subscripts")) {
                        *value = Expr::Call("@array_constructor".into(), vec![value.clone()]);
                    } else if let Some((_, false)) = resolve_path(self.types, vars, name) {
                        // x%c%w(i) = value: element 1 of c, then the subscripts.
                        let given = std::mem::take(indices);
                        *indices = vec![one(), Expr::Call("@component_subscripts".into(), given)];
                    }
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@allocate" || name == crate::substrings::ALLOCATE_CHARACTER || name == "@allocate_derived_type" => {
                    let start = if name == "@allocate" { 0 } else { 1 };
                    for a in args.iter_mut().skip(start) {
                        match a {
                            Expr::Ident(n) if resolve_path(self.types, vars, n).is_some_and(|(_, last)| last) => *a = Expr::Call(n.clone(), vec![one()]),
                            Expr::Call(n, b) if n.starts_with("@record_component:") => {
                                for x in b.iter_mut() { self.expr(x, vars, false); }
                                let field = &n["@record_component:".len()..];
                                let rewritten = b.first().and_then(|x| self.type_of(x, vars)).is_some_and(|ty| self.types.rewritten_in(&ty, field));
                                if rewritten && !b.iter().any(|x| matches!(x, Expr::Call(t, _) if t == "@component_subscripts")) { b.push(subscripts()); }
                            }
                            other => self.expr(other, vars, false),
                        }
                    }
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@deallocate" || name.eq_ignore_ascii_case("move_alloc") => {
                    for a in args.iter_mut() { self.expr(a, vars, true); }
                    continue;
                }
                _ => {}
            }
            for e in crate::procedure_arguments::expressions(s) { self.expr(e, vars, false); }
            for b in crate::procedure_arguments::children(s) { self.statements(b, vars); }
        }
    }
}

pub fn resolve(mut program: Program) -> anyhow::Result<Program> {
    let mut types = Types::default();
    types.finalized(&program.body);
    types.collect(&mut program.body);
    if !types.any() { return Ok(program); }
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let vars: Vars = body.iter().flat_map(|s| match s {
                Stmt::DerivedDecl { type_name, names, .. } => names.iter().map(|n| (n.clone(), type_name.clone())).collect::<Vec<_>>(),
                _ => vec![],
            }).collect();
            types.modules.insert(name.clone(), vars);
        }
    }
    Rewriter { types: &types }.scope(&mut program.body, &Vars::new());
    Ok(program)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! CHARACTER pointers of constant length. A POINTER or TARGET entity of type
//! `character(len=n)` becomes an object of the generated derived type
//! `rfort_string_n`, whose one component `v` holds the value, so that
//! `recursive_types` keeps it in a storage pool and pointer association,
//! ALLOCATE and DEALLOCATE work on handles. References to the value become
//! references to the component:
//!
//! * `c` is `c%v` and the substring `c(i:j)` is `c%v(i:j)`;
//! * an element `a(k)` is `a(k)%v` and `a(k)(i:j)` is `a(k)%v(i:j)`;
//! * `p => t`, NULLIFY, ASSOCIATED, ALLOCATE and DEALLOCATE keep the objects.
//!
//! TARGET entities are converted only for lengths that some POINTER has.
//! Deferred-length and assumed-length CHARACTER pointers, initialized
//! TARGET variables outside the main program, TARGET dummy arguments used
//! as targets, and CHARACTER POINTER function results are not yet supported.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

/// The prefix of the generated wrapper types.
pub(crate) const WRAPPER: &str = "rfort_string_";
const MODULE: &str = "rfort_strings";

fn wrapper(length: i64) -> String {
    format!("{WRAPPER}{length}")
}

/// A converted entity: its length, and whether it is an array.
#[derive(Clone, Copy, Debug)]
struct Entity {
    length: i64,
    array: bool,
}
type Scope = HashMap<String, Entity>;

/// The attributes, from separate DummyIntent statements, of each name of
/// a specification part.
fn attributes(body: &[Stmt]) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            out.entry(name.clone()).or_default().push(intent.clone());
        }
    }
    out
}

fn has(attrs: &HashMap<String, Vec<String>>, n: &str, a: &str) -> bool {
    attrs.get(n).is_some_and(|v| v.iter().any(|x| x == a))
}

/// The lengths of the program's CHARACTER pointers.
fn pointer_lengths(body: &[Stmt], out: &mut HashSet<i64>) -> Result<()> {
    let attrs = attributes(body);
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names.iter().filter(|n| has(&attrs, n, "pointer")) {
                    match kind {
                        TypeSpec::Character(Some(l)) => {
                            out.insert(*l as i64);
                        }
                        TypeSpec::CharacterLength(e) if matches!(&**e, Expr::Ident(d) if d == "@deferred_character") => {
                            bail!("deferred-length CHARACTER pointer `{n}` is not yet supported")
                        }
                        TypeSpec::Character(None) => {
                            out.insert(1);
                        }
                        TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => {
                            bail!("CHARACTER pointer `{n}` currently requires a constant length")
                        }
                        _ => {}
                    }
                }
            }
            Stmt::ArrayDecl { kind, name, .. } if has(&attrs, name, "pointer") => match kind {
                TypeSpec::Character(Some(l)) => {
                    out.insert(*l as i64);
                }
                TypeSpec::Character(None) => {
                    out.insert(1);
                }
                TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => {
                    bail!("CHARACTER pointer `{name}` currently requires a constant length")
                }
                _ => {}
            },
            _ => {}
        }
        if let Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } = s {
            pointer_lengths(body, out)?;
        }
    }
    Ok(())
}

fn length(kind: &TypeSpec) -> Option<i64> {
    match kind {
        TypeSpec::Character(Some(l)) => Some(*l as i64),
        TypeSpec::Character(None) => Some(1),
        _ => None,
    }
}

struct Lower {
    lengths: HashSet<i64>,
    /// The POINTER dummies of the program's procedures that are converted,
    /// by position and name.
    dummies: HashMap<String, Vec<(usize, String)>>,
    /// The converted variables of the program's modules.
    modules: HashMap<String, Scope>,
    /// Initial values of module TARGET variables.
    module_initial: Vec<Stmt>,
}

impl Lower {
    /// The entities of a specification part that are converted.
    fn converted(&self, body: &[Stmt], params: &[String]) -> Scope {
        let attrs = attributes(body);
        let mut out = Scope::new();
        let mut add = |n: &str, kind: &TypeSpec, array: bool| {
            let Some(l) = length(kind) else { return };
            if !self.lengths.contains(&l) {
                return;
            }
            let pointer = has(&attrs, n, "pointer");
            // A TARGET dummy keeps the representation of its actual.
            let target = has(&attrs, n, "target") && !params.iter().any(|p| p == n);
            if pointer || target {
                out.insert(n.to_owned(), Entity { length: l, array });
            }
        };
        for s in body {
            match s {
                Stmt::VarDecl { kind, names } => names.iter().for_each(|n| add(n, kind, false)),
                Stmt::ArrayDecl { kind, name, .. } => add(name, kind, true),
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (n, _) in entities {
                        let array = body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == n));
                        add(n, kind, array);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Lower a scope; `host` holds the converted entities of its host.
    fn scope(&mut self, body: &mut Vec<Stmt>, host: &Scope, params: &[String], result: Option<&str>, main: bool) -> Result<()> {
        self.unit(body, host, params, result, main, false)
    }

    fn unit(&mut self, body: &mut Vec<Stmt>, host: &Scope, params: &[String], result: Option<&str>, main: bool, module: bool) -> Result<()> {
        let local = self.converted(body, params);
        if let Some(r) = result {
            if local.contains_key(r) {
                bail!("CHARACTER POINTER function result `{r}` is not yet supported");
            }
        }
        let mut scope = host.clone();
        // Local declarations and dummies hide host entities.
        for s in body.iter() {
            let names: Vec<&String> = match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.iter().collect(),
                Stmt::ArrayDecl { name, .. } => vec![name],
                Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n).collect(),
                _ => vec![],
            };
            for n in names {
                scope.remove(n);
            }
        }
        for p in params {
            scope.remove(p);
        }
        for s in body.iter() {
            if let Stmt::Use { module, only, renames, .. } = s {
                let Some(public) = self.modules.get(module) else { continue };
                for (n, e) in public {
                    if only.as_ref().is_none_or(|o| o.contains(n)) && !renames.iter().any(|(_, r)| r == n) {
                        scope.insert(n.clone(), *e);
                    }
                }
                for (l, r) in renames {
                    if let Some(e) = public.get(r) {
                        scope.insert(l.clone(), *e);
                    }
                }
            }
        }
        scope.extend(local.iter().map(|(n, e)| (n.clone(), *e)));
        self.declarations(body, &local, main, module)?;
        self.statements(body, &scope)
    }

    /// Declare the converted entities as objects of the wrapper types.
    fn declarations(&mut self, body: &mut Vec<Stmt>, local: &Scope, main: bool, module: bool) -> Result<()> {
        if local.is_empty() {
            return Ok(());
        }
        let attrs = attributes(body);
        let object = |n: &str, dims: Vec<Expr>| {
            let mut attributes: Vec<String> = attrs.get(n).cloned().unwrap_or_default();
            attributes.retain(|a| matches!(a.as_str(), "pointer" | "target" | "save" | "optional" | "value") || a.starts_with("intent("));
            Stmt::DerivedDecl { type_name: wrapper(local[n].length), names: vec![n.to_owned()], attributes, polymorphic: false, dims }
        };
        let mut out = vec![];
        let mut initial = vec![];
        for s in std::mem::take(body) {
            match s {
                Stmt::DummyIntent { name, intent } if local.contains_key(&name) => {
                    if !(matches!(intent.as_str(), "pointer" | "target" | "save" | "optional" | "value") || intent.starts_with("intent(")) {
                        out.push(Stmt::DummyIntent { name, intent });
                    }
                }
                Stmt::VarDecl { kind, names } if names.iter().any(|n| local.contains_key(n)) => {
                    let (converted, kept): (Vec<String>, Vec<String>) = names.into_iter().partition(|n| local.contains_key(n));
                    if !kept.is_empty() {
                        out.push(Stmt::VarDecl { kind, names: kept });
                    }
                    out.extend(converted.iter().map(|n| object(n, vec![])));
                }
                Stmt::ArrayDecl { name, dims, .. } if local.contains_key(&name) => out.push(object(&name, dims)),
                Stmt::InitializedDecl { kind, attributes: a, entities } if entities.iter().any(|(n, _)| local.contains_key(n)) => {
                    let (converted, kept): (Vec<_>, Vec<_>) = entities.into_iter().partition(|(n, _)| local.contains_key(n));
                    if !kept.is_empty() {
                        out.push(Stmt::InitializedDecl { kind, attributes: a, entities: kept });
                    }
                    for (n, value) in converted {
                        // An array has its own ArrayDecl, converted there.
                        if !local[&n].array {
                            out.push(object(&n, vec![]));
                        }
                        if let Some(value) = value {
                            let value = Stmt::Assign { name: format!("{n}%v"), value };
                            if module {
                                // Given when the main program starts.
                                self.module_initial.push(value);
                            } else if main {
                                initial.push(value);
                            } else {
                                bail!("initialized CHARACTER TARGET `{n}` in a procedure is not yet supported");
                            }
                        }
                    }
                }
                other => out.push(other),
            }
        }
        if !initial.is_empty() {
            let at = out.iter().position(|s| !specification(s)).unwrap_or(out.len());
            out.splice(at..at, initial);
        }
        *body = out;
        Ok(())
    }

    fn statements(&mut self, body: &mut [Stmt], scope: &Scope) -> Result<()> {
        for s in body.iter_mut() {
            match s {
                Stmt::Function { name, params, body, .. } => {
                    let r = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(),
                        _ => name.clone(),
                    };
                    let params = params.clone();
                    self.scope(body, scope, &params, Some(&r), false)?;
                    continue;
                }
                Stmt::Subroutine { params, body, .. } => {
                    let params = params.clone();
                    self.scope(body, scope, &params, None, false)?;
                    continue;
                }
                Stmt::Module { .. } => continue,
                Stmt::Block { body } => {
                    self.scope(body, scope, &[], None, false)?;
                    continue;
                }
                _ => {}
            }
            if !scope.is_empty() {
                self.statement(s, scope)?;
            }
            for b in crate::procedure_arguments::children(s) {
                self.statements(b, scope)?;
            }
        }
        Ok(())
    }

    fn statement(&mut self, s: &mut Stmt, scope: &Scope) -> Result<()> {
        match s {
            Stmt::Assign { name, value } => {
                self.expr(value, scope)?;
                if scope.contains_key(name.as_str()) {
                    *name = format!("{name}%v");
                }
            }
            Stmt::AssignIndex { name, indices, value } => {
                self.expr(value, scope)?;
                for i in indices.iter_mut() {
                    self.expr(i, scope)?;
                }
                if let Some(e) = scope.get(name.as_str()) {
                    let component = format!("{name}%v");
                    if !e.array {
                        // A substring of a scalar.
                        for i in indices.iter_mut() {
                            if let Expr::Section { name: owner, .. } = i {
                                *owner = component.clone();
                            }
                        }
                    }
                    *name = component;
                }
            }
            Stmt::CallSub { name, args } => match name.as_str() {
                "@array_substring_assign" => {
                    for a in args.iter_mut() {
                        self.expr(a, scope)?;
                    }
                    // a(k)(i:j) = v, which the expressions made a(k)%v(i:j).
                    if let [Expr::Call(tag, parts), range, value] = args.as_slice() {
                        if let (Some(field), [Expr::Call(array, subs)]) = (tag.strip_prefix("@record_component:"), parts.as_slice()) {
                            let mut indices = subs.clone();
                            indices.push(Expr::Call("@component_subscripts".into(), vec![range.clone()]));
                            *s = Stmt::AssignIndex { name: format!("{array}%{field}"), indices, value: value.clone() };
                        }
                    }
                }
                "@pointer_associate" => {
                    let target = &args[1];
                    if let Expr::Section { name: n, .. } = target {
                        if scope.get(n.as_str()).is_some_and(|e| !e.array) {
                            bail!("pointer association with a substring of `{n}` is not yet supported");
                        }
                    }
                    for a in args.iter_mut() {
                        self.object(a, scope)?;
                    }
                }
                "@nullify" | "@allocate" | "@deallocate" => {
                    for a in args.iter_mut() {
                        self.object(a, scope)?;
                    }
                }
                "@allocate_character" => {
                    if let [len, object] = args.as_slice() {
                        if let Some(e) = root(object).and_then(|n| scope.get(n)) {
                            if crate::parameters::integer_constant(len).ok() != Some(e.length) {
                                bail!("ALLOCATE of a CHARACTER pointer must give its declared length {}", e.length);
                            }
                            let mut object = object.clone();
                            self.object(&mut object, scope)?;
                            *s = Stmt::CallSub { name: "@allocate".into(), args: vec![object] };
                        }
                    }
                }
                _ => {
                    let keep = self.dummies.get(name.as_str()).cloned().unwrap_or_default();
                    for (i, a) in args.iter_mut().enumerate() {
                        if passes_object(a, i, &keep, scope) {
                            continue;
                        }
                        self.expr(a, scope)?;
                    }
                }
            },
            _ => {
                for e in crate::procedure_arguments::expressions(s) {
                    self.expr(e, scope)?;
                }
            }
        }
        Ok(())
    }

    /// The designator of an object in a pointer statement keeps the
    /// object; only its subscripts and bounds are values.
    fn object(&mut self, e: &mut Expr, scope: &Scope) -> Result<()> {
        match e {
            Expr::Ident(_) => {}
            Expr::Call(n, args) | Expr::Index(n, args) if scope.contains_key(n.as_str()) => {
                for a in args {
                    self.expr(a, scope)?;
                }
            }
            Expr::Section { name, lower, upper, stride } if scope.contains_key(name.as_str()) => {
                for b in [lower, upper, stride].into_iter().flatten() {
                    self.expr(b, scope)?;
                }
            }
            Expr::Call(n, args) if n == "@pointer_remap" => {
                for a in args {
                    self.expr(a, scope)?;
                }
            }
            _ => self.expr(e, scope)?,
        }
        Ok(())
    }

    fn expr(&mut self, e: &mut Expr, scope: &Scope) -> Result<()> {
        match e {
            Expr::Ident(n) => {
                if scope.contains_key(n.as_str()) {
                    *n = format!("{n}%v");
                }
            }
            Expr::Section { name, lower, upper, stride } => {
                for b in [&mut *lower, &mut *upper, &mut *stride].into_iter().flatten() {
                    self.expr(b, scope)?;
                }
                if let Some(entity) = scope.get(name.as_str()) {
                    if entity.array {
                        *e = Expr::Call("@record_component:v".into(), vec![e.clone()]);
                    } else {
                        *name = format!("{name}%v");
                    }
                }
            }
            Expr::Call(n, args) if n == "associated" => {
                for a in args.iter_mut() {
                    match a {
                        Expr::Call(k, v) if k.starts_with("@keyword:") => {
                            for x in v {
                                self.object(x, scope)?;
                            }
                        }
                        _ => self.object(a, scope)?,
                    }
                }
            }
            Expr::Call(n, args) if n == "@array_substring" && args.len() == 2 => {
                self.expr(&mut args[1], scope)?;
                if let Expr::Call(array, subs) | Expr::Index(array, subs) = &mut args[0] {
                    if scope.get(array.as_str()).is_some_and(|e| e.array) {
                        for a in subs.iter_mut() {
                            self.expr(a, scope)?;
                        }
                        let element = args[0].clone();
                        let range = args[1].clone();
                        *e = Expr::Call("@record_component:v".into(), vec![element, Expr::Call("@component_subscripts".into(), vec![range])]);
                        return Ok(());
                    }
                }
                self.expr(&mut args[0], scope)?;
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                let keep = self.dummies.get(n.as_str()).cloned().unwrap_or_default();
                for (i, a) in args.iter_mut().enumerate() {
                    if passes_object(a, i, &keep, scope) {
                        continue;
                    }
                    self.expr(a, scope)?;
                }
                if scope.get(n.as_str()).is_some_and(|e| e.array) {
                    *e = Expr::Call("@record_component:v".into(), vec![e.clone()]);
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, scope)?;
                self.expr(b, scope)?;
            }
            Expr::Un(_, a) => self.expr(a, scope)?,
            _ => {}
        }
        Ok(())
    }
}

/// Whether actual `i` of a call is a converted POINTER passed to a POINTER
/// dummy, which receives the object.
fn passes_object(a: &Expr, i: usize, dummies: &[(usize, String)], scope: &Scope) -> bool {
    let (position, actual) = match a {
        Expr::Call(k, v) if k.starts_with("@keyword:") && v.len() == 1 => (None, &v[0]),
        _ => (Some(i), a),
    };
    let keyword = match a {
        Expr::Call(k, _) => k.strip_prefix("@keyword:"),
        _ => None,
    };
    matches!(actual, Expr::Ident(n) if scope.contains_key(n.as_str()))
        && dummies.iter().any(|(p, n)| Some(*p) == position || keyword == Some(n.as_str()))
}

fn root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Some(n),
        _ => None,
    }
}

fn specification(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::SourceLocation(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. }
            | Stmt::DummyIntent { .. } | Stmt::Implicit(_) | Stmt::Use { .. } | Stmt::DerivedDecl { .. }
            | Stmt::DerivedType { .. } | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. }
            | Stmt::OperatorInterface { .. } | Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }
    )
}

/// The converted POINTER dummies of every procedure.
fn pointer_dummies(body: &[Stmt], lower: &mut Lower) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let attrs = attributes(body);
                let converted = lower.converted(body, params);
                let dummies: Vec<_> = params.iter().enumerate()
                    .filter(|(_, p)| converted.contains_key(*p) && has(&attrs, p, "pointer"))
                    .map(|(i, p)| (i, p.clone())).collect();
                if !dummies.is_empty() {
                    lower.dummies.insert(name.clone(), dummies);
                }
                pointer_dummies(body, lower);
            }
            Stmt::Module { body, .. } => pointer_dummies(body, lower),
            _ => {}
        }
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut lengths = HashSet::new();
    pointer_lengths(&program.body, &mut lengths)?;
    if lengths.is_empty() {
        return Ok(program);
    }
    let mut lower = Lower { lengths: lengths.clone(), dummies: HashMap::new(), modules: HashMap::new(), module_initial: vec![] };
    pointer_dummies(&program.body, &mut lower);
    // Modules first, in order, so that the units using them see their
    // converted variables.
    for s in program.body.iter_mut() {
        if let Stmt::Module { name, body } = s {
            let mut public = Scope::new();
            for u in body.iter() {
                if let Stmt::Use { module, only: None, renames, .. } = u {
                    if renames.is_empty() {
                        public.extend(lower.modules.get(module).cloned().unwrap_or_default());
                    }
                }
            }
            public.extend(lower.converted(body, &[]));
            lower.unit(body, &Scope::new(), &[], None, false, true)?;
            lower.modules.insert(name.clone(), public);
        }
    }
    let mut main = std::mem::take(&mut program.body);
    let mut units = vec![];
    let mut rest = vec![];
    // Internal procedures of the main program stay with it.
    for s in main.drain(..) {
        let external = match &s {
            Stmt::Module { .. } => true,
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => !program.contains.contains(name),
            _ => false,
        };
        if external {
            units.push(s);
        } else {
            rest.push(s);
        }
    }
    // External procedures, then the main program.
    for s in units.iter_mut() {
        match s {
            Stmt::Function { .. } | Stmt::Subroutine { .. } => lower.statements(std::slice::from_mut(s), &Scope::new())?,
            _ => {}
        }
    }
    lower.scope(&mut rest, &Scope::new(), &[], None, true)?;
    if !lower.module_initial.is_empty() {
        let Some(at) = rest.iter().position(|s| !specification(s)) else {
            bail!("initialized CHARACTER TARGET module variables need the main program in the same compilation");
        };
        let initial = std::mem::take(&mut lower.module_initial);
        rest.splice(at..at, initial);
    }
    // The wrapper types, in a module every unit uses.
    let mut lengths: Vec<_> = lengths.into_iter().collect();
    lengths.sort();
    let types = lengths.into_iter().map(|l| Stmt::DerivedType {
        name: wrapper(l),
        components: vec![Stmt::VarDecl { kind: TypeSpec::Character(Some(l as usize)), names: vec!["v".into()] }],
        bindings: vec![],
    }).collect();
    let use_strings = Stmt::Use { module: MODULE.into(), nature: None, only: None, renames: vec![] };
    for s in units.iter_mut() {
        if let Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } = s {
            body.insert(0, use_strings.clone());
        }
    }
    let at = rest.iter().position(|s| !matches!(s, Stmt::SourceLocation(_))).unwrap_or(rest.len());
    rest.insert(at, use_strings);
    program.body.push(Stmt::Module { name: MODULE.into(), body: types });
    program.body.extend(units);
    program.body.extend(rest);
    Ok(program)
}

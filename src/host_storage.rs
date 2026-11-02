// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Host variables that internal procedures use, kept in module storage. The
//! variables of a host that its internal procedures refer to move into a
//! generated module, `rfort_host_n`, which the host and those internal
//! procedures use with ONLY lists, so that every unit sees one object with
//! all the support module variables have: whole arrays and sections,
//! ALLOCATABLE and POINTER variables, CHARACTER and derived types.
//!
//! A main program has one instance of its variables, so this is exact there.
//! A procedure host that cannot call itself, directly or through other
//! procedures of the program, also has one active instance at a time; its
//! ALLOCATABLE variables are deallocated when it returns, as locals are.
//! Recursive hosts, dummy arguments, and variables in COMMON, EQUIVALENCE,
//! DATA or NAMELIST statements keep the borrowed host environment of
//! `internal_procedures`, as do automatic objects, deferred-length CHARACTER
//! variables, derived-type variables of types the host or its module
//! defines, and, in a procedure host, derived-type variables without SAVE
//! (whose default initialization applies on each entry).
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub(crate) const MODULE: &str = "rfort_host_";

/// The names of variables, procedures and constants a statement list refers
/// to, outside nested procedures.
fn mentions(body: &[Stmt], out: &mut HashSet<String>) {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => {
                out.insert(base(n));
            }
            Expr::Section { name, lower, upper, stride } => {
                out.insert(base(name));
                for b in [lower, upper, stride].into_iter().flatten() {
                    expr(b, out);
                }
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                out.insert(base(n));
                for a in args {
                    expr(a, out);
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, out);
                expr(b, out);
            }
            Expr::Un(_, a) => expr(a, out),
            _ => {}
        }
    }
    for s in body {
        match s {
            Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => continue,
            Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } | Stmt::CallSub { name, .. } => {
                out.insert(base(name));
            }
            Stmt::Do { var, .. } => {
                out.insert(var.clone());
            }
            _ => {}
        }
        let mut s = s.clone();
        for e in crate::procedure_arguments::expressions(&mut s) {
            expr(e, out);
        }
        match &s {
            Stmt::VarDecl { kind, .. } | Stmt::InitializedDecl { kind, .. } | Stmt::ArrayDecl { kind, .. } => kind_mentions(kind, out),
            Stmt::DerivedDecl { type_name, .. } => {
                out.insert(type_name.clone());
            }
            _ => {}
        }
        for b in crate::procedure_arguments::children(&mut s) {
            mentions(b, out);
        }
    }
}

fn kind_mentions(kind: &TypeSpec, out: &mut HashSet<String>) {
    match kind {
        TypeSpec::CharacterLength(e) | TypeSpec::Referenced { source: e, .. } => {
            let mut found = HashSet::new();
            mentions(&[Stmt::Print { items: vec![(**e).clone()] }], &mut found);
            out.extend(found);
        }
        TypeSpec::IntegerKind(k) | TypeSpec::RealKind(k) | TypeSpec::LogicalKind(k) | TypeSpec::Complex(k) => {
            out.insert(k.clone());
        }
        _ => {}
    }
}

/// The base variable of a designator such as `a%b%c`.
fn base(n: &str) -> String {
    n.split('%').next().unwrap_or(n).to_owned()
}

/// The names a specification part declares, with each one's declaration
/// statements and attributes.
#[derive(Default)]
struct Declared {
    statements: HashMap<String, Vec<Stmt>>,
    /// Attributes from declarations and attribute statements.
    attributes: HashMap<String, Vec<String>>,
    /// Attributes from attribute statements only.
    intents: HashMap<String, Vec<String>>,
    parameters: HashSet<String>,
    types: HashSet<String>,
}

fn declared(body: &[Stmt]) -> Declared {
    let mut d = Declared::default();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    d.statements.entry(n.clone()).or_default().push(Stmt::VarDecl { kind: kind.clone(), names: vec![n.clone()] });
                }
            }
            Stmt::ArrayDecl { name, .. } => d.statements.entry(name.clone()).or_default().push(s.clone()),
            Stmt::InitializedDecl { kind, attributes, entities } => {
                for (n, v) in entities {
                    if attributes.iter().any(|a| a == "parameter") {
                        d.parameters.insert(n.clone());
                    }
                    d.statements.entry(n.clone()).or_default().push(Stmt::InitializedDecl {
                        kind: kind.clone(), attributes: attributes.clone(), entities: vec![(n.clone(), v.clone())],
                    });
                }
            }
            Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims } => {
                for n in names {
                    d.statements.entry(n.clone()).or_default().push(Stmt::DerivedDecl {
                        type_name: type_name.clone(), names: vec![n.clone()], attributes: attributes.clone(),
                        polymorphic: *polymorphic, dims: dims.clone(),
                    });
                    d.attributes.entry(n.clone()).or_default().extend(attributes.iter().cloned());
                }
            }
            Stmt::DummyIntent { name, intent } => {
                d.attributes.entry(name.clone()).or_default().push(intent.clone());
                d.intents.entry(name.clone()).or_default().push(intent.clone());
            }
            Stmt::DerivedType { name, .. } => {
                d.types.insert(name.clone());
            }
            _ => {}
        }
    }
    d
}

/// Names that statements such as COMMON, EQUIVALENCE, DATA and NAMELIST give
/// a storage association or initial value.
fn associated(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        if let Stmt::CallSub { name, args } = s {
            if name.starts_with("@namelist") || name == "@data" || name == crate::common::STATEMENT || name == crate::equivalence::STATEMENT {
                mentions(&[Stmt::Print { items: args.clone() }], out);
            }
        }
        if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
            let mut s = s.clone();
            for b in crate::procedure_arguments::children(&mut s) {
                associated(b, out);
            }
        }
    }
}

/// Whether an array declaration has a shape fixed at compile time or
/// deferred (ALLOCATABLE or POINTER).
fn static_shape(dims: &[Expr], constants: &HashSet<String>) -> bool {
    let mut names = HashSet::new();
    mentions(&[Stmt::Print { items: dims.to_vec() }], &mut names);
    names.retain(|n| !n.starts_with('@'));
    names.is_subset(constants)
}

struct Lower {
    /// Procedures each procedure calls, its internal procedures' calls included.
    calls: HashMap<String, HashSet<String>>,
    /// The generated modules, each with the name of the unit it goes before.
    modules: Vec<(usize, Stmt)>,
}

impl Lower {
    fn recursive(&self, name: &str) -> bool {
        let mut seen = HashSet::new();
        let mut stack: Vec<&String> = self.calls.get(name).into_iter().flatten().collect();
        while let Some(n) = stack.pop() {
            if n == name {
                return true;
            }
            if seen.insert(n.clone()) {
                stack.extend(self.calls.get(n).into_iter().flatten());
            }
        }
        false
    }

    /// Move the variables of a host that its internal procedures use into
    /// a generated module. `uses` are the USE statements in scope for the
    /// module, `outer` the names the host's own module declares, and `at`
    /// the position of the host's unit in the program.
    #[allow(clippy::too_many_arguments)]
    fn host(&mut self, unit: &str, body: &mut Vec<Stmt>, internal: &[String], params: &[String], result: Option<&str>,
        main: bool, uses: &[Stmt], outer: &HashSet<String>, at: usize) -> Result<()> {
        let mut d = declared(body);
        // The named constants of the host's module are copied too.
        let outer_constants: Vec<Stmt> = uses.iter().filter(|s| !matches!(s, Stmt::Use { .. })).cloned().collect();
        let uses: Vec<Stmt> = uses.iter().filter(|s| matches!(s, Stmt::Use { .. })).cloned().collect();
        for s in &outer_constants {
            if let Stmt::InitializedDecl { entities, .. } = s {
                for (n, _) in entities {
                    if !d.statements.contains_key(n) {
                        d.parameters.insert(n.clone());
                    }
                }
            }
        }
        let outer: HashSet<String> = outer.iter().filter(|n| !d.parameters.contains(*n)).cloned().collect();
        let outer = &outer;
        // The host's internal procedures, and the names each uses from the host.
        let mut used = HashSet::new();
        for s in body.iter() {
            let (Stmt::Function { name, params: p, body: b, .. } | Stmt::Subroutine { name, params: p, body: b, .. }) = s else { continue };
            if !internal.contains(name) {
                continue;
            }
            let mut names = HashSet::new();
            mentions(b, &mut names);
            let own = declared(b);
            names.retain(|n| !own.statements.contains_key(n) && !p.contains(n) && n != name);
            used.extend(names);
        }
        let mut excluded = HashSet::new();
        associated(body, &mut excluded);
        // A variable an internal procedure's declarations inquire about, as
        // LEN(a) in a length, keeps the host declaration they evaluate.
        for s in body.iter() {
            let (Stmt::Function { name, body: b, .. } | Stmt::Subroutine { name, body: b, .. }) = s else { continue };
            if !internal.contains(name) {
                continue;
            }
            let declarations: Vec<Stmt> = b.iter().filter(|s| matches!(s, Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. }
                | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. })).cloned().collect();
            mentions(&declarations, &mut excluded);
        }
        let allowed = |a: &str| matches!(a, "allocatable" | "pointer" | "target" | "save" | "contiguous" | "volatile" | "asynchronous");
        let mut moved: Vec<String> = vec![];
        let mut names: Vec<&String> = d.statements.keys().collect();
        names.sort();
        for n in names {
            if !used.contains(n) || params.contains(n) || Some(n.as_str()) == result || d.parameters.contains(n) || excluded.contains(n) || outer.contains(n) {
                continue;
            }
            let attributes = d.attributes.get(n).cloned().unwrap_or_default();
            if !attributes.iter().all(|a| allowed(a)) {
                continue;
            }
            let statements = &d.statements[n];
            let mut fits = true;
            for s in statements {
                match s {
                    Stmt::VarDecl { kind, .. } | Stmt::InitializedDecl { kind, .. } | Stmt::ArrayDecl { kind, .. } => {
                        let mut names = HashSet::new();
                        kind_mentions(kind, &mut names);
                        // Constant kinds and lengths, from the host's PARAMETERs or modules.
                        let local = names.iter().any(|k| d.statements.contains_key(k) && !d.parameters.contains(k));
                        if local || matches!(kind, TypeSpec::CharacterAssumed)
                            || matches!(kind, TypeSpec::CharacterLength(e) if matches!(&**e, Expr::Ident(x) if x == "@deferred_character"))
                        {
                            fits = false;
                        }
                        if let TypeSpec::CharacterLength(e) = kind {
                            let mut names = HashSet::new();
                            mentions(&[Stmt::Print { items: vec![(**e).clone()] }], &mut names);
                            if names.iter().any(|k| !k.starts_with('@') && !d.parameters.contains(k)) {
                                fits = false;
                            }
                        }
                        if let Stmt::ArrayDecl { dims, .. } = s {
                            if !static_shape(dims, &d.parameters) {
                                fits = false;
                            }
                        }
                    }
                    Stmt::DerivedDecl { type_name, dims, attributes: a, .. } => {
                        let saved = attributes.iter().chain(a).any(|x| x == "save");
                        if d.types.contains(type_name) || outer.contains(type_name) || (!main && !saved) || !static_shape(dims, &d.parameters) {
                            fits = false;
                        }
                    }
                    _ => {}
                }
            }
            if fits {
                moved.push(n.clone());
            }
        }
        if moved.is_empty() {
            return Ok(());
        }
        // Named after the host, so that separately compiled hosts differ.
        let module = format!("{MODULE}{unit}");
        // The module sees the host's modules and named constants.
        let mut contents: Vec<Stmt> = uses.to_vec();
        contents.extend(body.iter().filter(|s| matches!(s, Stmt::Use { .. })).cloned());
        contents.push(Stmt::Implicit(crate::ast::Implicit::None));
        contents.extend(outer_constants);
        for s in body.iter() {
            match s {
                Stmt::InitializedDecl { attributes, .. } if attributes.iter().any(|a| a == "parameter") => contents.push(s.clone()),
                Stmt::DummyIntent { name, intent } if intent == "parameter" && d.parameters.contains(name) => contents.push(s.clone()),
                _ => {}
            }
        }
        let set: HashSet<&String> = moved.iter().collect();
        for n in &moved {
            for a in d.intents.get(n).into_iter().flatten() {
                contents.push(Stmt::DummyIntent { name: n.clone(), intent: a.clone() });
            }
            // An initialized local is saved; in a module it is a module variable.
            contents.extend(d.statements.remove(n).unwrap_or_default().into_iter().map(|s| match s {
                Stmt::InitializedDecl { kind, attributes, entities } => Stmt::InitializedDecl {
                    kind, attributes: attributes.into_iter().filter(|a| !a.starts_with("@save_key:")).collect(), entities,
                },
                s => s,
            }));
        }
        // The host loses the declarations, and uses the module.
        let mut kept = vec![Stmt::Use { module: module.clone(), nature: None, only: Some(moved.clone()), renames: vec![] }];
        for s in std::mem::take(body) {
            match s {
                Stmt::VarDecl { kind, names } => {
                    let names: Vec<String> = names.into_iter().filter(|n| !set.contains(n)).collect();
                    if !names.is_empty() {
                        kept.push(Stmt::VarDecl { kind, names });
                    }
                }
                Stmt::ArrayDecl { ref name, .. } if set.contains(name) => {}
                Stmt::DummyIntent { ref name, .. } if set.contains(name) => {}
                Stmt::InitializedDecl { kind, attributes, entities } => {
                    let entities: Vec<_> = entities.into_iter().filter(|(n, _)| !set.contains(n)).collect();
                    if !entities.is_empty() {
                        kept.push(Stmt::InitializedDecl { kind, attributes, entities });
                    }
                }
                Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims } => {
                    let names: Vec<String> = names.into_iter().filter(|n| !set.contains(n)).collect();
                    if !names.is_empty() {
                        kept.push(Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims });
                    }
                }
                other => kept.push(other),
            }
        }
        *body = kept;
        // Each internal procedure uses the variables it refers to.
        for s in body.iter_mut() {
            let (Stmt::Function { name, params: p, body: b, .. } | Stmt::Subroutine { name, params: p, body: b, .. }) = s else { continue };
            if !internal.contains(name) {
                continue;
            }
            let mut names = HashSet::new();
            mentions(b, &mut names);
            let own = declared(b);
            let only: Vec<String> = moved.iter().filter(|n| names.contains(*n) && !own.statements.contains_key(*n) && !p.contains(n)).cloned().collect();
            if !only.is_empty() {
                b.insert(0, Stmt::Use { module: module.clone(), nature: None, only: Some(only), renames: vec![] });
            }
        }
        // A procedure host deallocates its ALLOCATABLE variables when it returns.
        if !main {
            let allocatables: Vec<&String> = moved.iter().filter(|n| d.attributes.get(*n).is_some_and(|a| a.iter().any(|x| x == "allocatable"))).collect();
            if !allocatables.is_empty() {
                let release: Vec<Stmt> = allocatables.iter().map(|n| Stmt::If {
                    cond: Expr::Call("allocated".into(), vec![Expr::Ident((*n).clone())]),
                    then_body: vec![Stmt::CallSub { name: "@deallocate".into(), args: vec![Expr::Ident((*n).clone())] }],
                    else_body: None,
                }).collect();
                before_returns(body, &release);
                let end = body.iter().rposition(|s| !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. })).map_or(0, |i| i + 1);
                let end = if result.is_some() && matches!(body.get(end.wrapping_sub(1)), Some(Stmt::Return(Some(_)))) { end - 1 } else { end };
                body.splice(end..end, release);
            }
        }
        self.modules.push((at, Stmt::Module { name: module, body: contents }));
        Ok(())
    }
}

/// Insert `release` before each RETURN of a host, outside its internal procedures.
fn before_returns(body: &mut Vec<Stmt>, release: &[Stmt]) {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        if matches!(s, Stmt::Return(None)) {
            out.extend(release.iter().cloned());
        }
        if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
            for b in crate::procedure_arguments::children(&mut s) {
                before_returns(b, release);
            }
        }
        out.push(s);
    }
    *body = out;
}

/// The procedures each procedure calls; an internal procedure's calls count
/// as its host's.
fn calls(body: &[Stmt], host: Option<&str>, out: &mut HashMap<String, HashSet<String>>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                let owner = host.unwrap_or(name).to_owned();
                let mut called = HashSet::new();
                crate::procedure_arguments::invoked(body, &mut called);
                out.entry(owner.clone()).or_default().extend(called);
                calls(body, Some(&owner), out);
            }
            Stmt::Module { body, .. } => calls(body, None, out),
            _ => {}
        }
    }
}

fn internal_names(body: &[Stmt]) -> Vec<String> {
    body.iter().filter_map(|s| match s {
        Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => Some(name.clone()),
        _ => None,
    }).collect()
}

fn result_name(name: &str, body: &[Stmt]) -> String {
    match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(),
        _ => name.to_owned(),
    }
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut graph = HashMap::new();
    calls(&p.body, None, &mut graph);
    let mut lower = Lower { calls: graph, modules: vec![] };
    let main_internal: Vec<String> = p.body.iter().filter_map(|s| match s {
        Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if p.contains.contains(&name.to_ascii_lowercase()) => Some(name.clone()),
        _ => None,
    }).collect();
    for i in 0..p.body.len() {
        match &mut p.body[i] {
            Stmt::Module { name: module, body } => {
                let module = module.clone();
                let mut own = declared(body);
                let outer: HashSet<String> = own.statements.drain().map(|(n, _)| n).chain(own.types.drain()).chain(own.parameters.drain()).collect();
                // The module's USE statements and named constants.
                let uses: Vec<Stmt> = body.iter().filter(|s| matches!(s, Stmt::Use { .. })
                    || matches!(s, Stmt::InitializedDecl { attributes, .. } if attributes.iter().any(|a| a == "parameter"))).cloned().collect();
                for s in body.iter_mut() {
                    let is_function = matches!(s, Stmt::Function { .. });
                    if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
                        let internal = internal_names(body);
                        if internal.is_empty() || lower.recursive(name) {
                            continue;
                        }
                        let result = is_function.then(|| result_name(name, body));
                        let params = params.clone();
                        let unit = format!("{module}_{name}");
                        lower.host(&unit, body, &internal, &params, result.as_deref(), false, &uses, &outer, i)?;
                    }
                }
            }
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if main_internal.contains(name) => {}
            s @ (Stmt::Function { .. } | Stmt::Subroutine { .. }) => {
                let is_function = matches!(s, Stmt::Function { .. });
                let (Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. }) = s else { unreachable!() };
                let internal = internal_names(body);
                if internal.is_empty() || lower.recursive(name) {
                    continue;
                }
                let result = is_function.then(|| result_name(name, body));
                let params = params.clone();
                let unit = name.clone();
                lower.host(&unit, body, &internal, &params, result.as_deref(), false, &[], &HashSet::new(), i)?;
            }
            _ => {}
        }
    }
    if !main_internal.is_empty() {
        // The main program: its statements and the procedures it CONTAINS.
        let mut main = vec![];
        let mut positions = vec![];
        for (i, s) in p.body.iter().enumerate() {
            let unit = match s {
                Stmt::Module { .. } => true,
                Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => !main_internal.contains(name),
                _ => false,
            };
            if !unit {
                positions.push(i);
            }
        }
        for &i in positions.iter().rev() {
            main.push(p.body.remove(i));
        }
        main.reverse();
        let at = positions.first().copied().unwrap_or(p.body.len());
        let unit = format!("main_{}", p.name);
        lower.host(&unit, &mut main, &main_internal, &[], None, true, &[], &HashSet::new(), at)?;
        p.body.splice(at..at, main);
    }
    // Each module goes before the unit of its host, in order.
    let mut modules = std::mem::take(&mut lower.modules);
    modules.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, m) in modules {
        p.body.insert(at, m);
    }
    Ok(p)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Designators with subscripts on several parts, such as `y(1)%at(j)%i`,
//! are parsed as the component `at%i` of `y(1)` with combined subscripts
//! (see `crate::ast::Chain`). A last part without subscripts takes all of its
//! axes; this pass replaces its `WHOLE_AXES` marker by one `:` per axis of
//! that component, from the declared types.
use crate::ast::{Expr, Program, Stmt, WHOLE_AXES};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// Components of a derived type: name -> (derived type name, rank).
type Components = HashMap<String, (Option<String>, usize)>;

/// The derived types visible in a scope: their components and parents.
#[derive(Default, Clone)]
struct Types {
    components: HashMap<String, Components>,
    parents: HashMap<String, String>,
}
/// Each module's types and derived-type variables (name -> (type, rank)).
type Modules = HashMap<String, (Types, HashMap<String, (String, usize)>)>;

fn contains_marker(body: &mut [Stmt]) -> bool {
    let mut found = false;
    for s in body.iter_mut() {
        visit(s, &mut |e| {
            if matches!(e, Expr::Call(n, _) if n == WHOLE_AXES) {
                found = true;
            }
        });
    }
    found
}

fn visit(s: &mut Stmt, f: &mut dyn FnMut(&mut Expr)) {
    for e in expressions(s) {
        visit_expr(e, f);
    }
    for child in children(s) {
        for s in child.iter_mut() {
            visit(s, f);
        }
    }
}

impl Types {
    /// Add the types that `body` itself defines.
    fn collect(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::DerivedType { name: type_name_of, components, .. } => {
                    let mut out = Components::new();
                    for c in components {
                        match c {
                            Stmt::VarDecl { names, .. } => {
                                for n in names {
                                    out.entry(n.clone()).or_insert((None, 0));
                                }
                            }
                            Stmt::InitializedDecl { entities, .. } => {
                                for (n, _) in entities {
                                    out.entry(n.clone()).or_insert((None, 0));
                                }
                            }
                            Stmt::ArrayDecl { name, dims, .. } => {
                                out.entry(name.clone()).or_insert((None, 0)).1 = dims.len();
                            }
                            Stmt::DerivedDecl { type_name, names, dims, .. } => {
                                for n in names {
                                    out.insert(n.clone(), (Some(type_name.clone()), dims.len()));
                                }
                            }
                            Stmt::DummyIntent { name, intent } if name == crate::extensions::EXTENDS => {
                                self.parents.insert(type_name_of.clone(), intent.clone());
                            }
                            _ => {}
                        }
                    }
                    self.components.insert(type_name_of.clone(), out);
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() {
                        self.parents.insert(child.clone(), parent.clone());
                    }
                }
                _ => {}
            }
        }
    }
    fn component(&self, ty: &str, name: &str) -> Option<&(Option<String>, usize)> {
        let mut ty = ty;
        for _ in 0..64 {
            if let Some(c) = self.components.get(ty).and_then(|c| c.get(name)) {
                return Some(c);
            }
            ty = self.parents.get(ty)?;
        }
        None
    }
    /// The rank of the last component of `path` from type `ty`.
    fn rank(&self, ty: &str, path: &str) -> Option<usize> {
        let mut ty = ty.to_string();
        let mut rank = 0;
        for name in path.split('%') {
            let (next, r) = self.component(&ty, name)?.clone();
            rank = r;
            match next {
                Some(t) => ty = t,
                None => ty = String::new(),
            }
        }
        Some(rank)
    }
}

fn variables(body: &[Stmt]) -> HashMap<String, (String, usize)> {
    let mut vars = HashMap::new();
    for s in body {
        if let Stmt::DerivedDecl { type_name, names, dims, .. } = s {
            for n in names {
                vars.insert(n.clone(), (type_name.clone(), dims.len()));
            }
        }
    }
    vars
}

fn body(statements: &mut [Stmt], inherited: &HashMap<String, (String, usize)>, host: &Types, modules: &Modules) -> Result<()> {
    if !contains_marker(statements) {
        return Ok(());
    }
    let mut vars = inherited.clone();
    let mut scope = host.clone();
    for s in statements.iter() {
        if let Stmt::Use { module, .. } = s {
            if let Some((t, m)) = modules.get(module) {
                vars.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
                scope.components.extend(t.components.iter().map(|(k, v)| (k.clone(), v.clone())));
                scope.parents.extend(t.parents.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
    }
    scope.collect(statements);
    let types = &scope;
    vars.extend(variables(statements));
    let mut failure = None;
    for s in statements.iter_mut() {
        let mut resolve = |e: &mut Expr| {
            let Expr::Call(tag, args) = e else { return };
            let Some(path) = tag.strip_prefix("@record_component:") else { return };
            let Some(Expr::Call(marker, subs)) = args.get(1) else { return };
            if marker != "@component_subscripts" || !matches!(subs.first(), Some(Expr::Call(n, _)) if n == WHOLE_AXES) {
                return;
            }
            let base = match &args[0] {
                Expr::Call(n, _) | Expr::Ident(n) => n.clone(),
                Expr::Section { name, .. } => name.clone(),
                _ => String::new(),
            };
            let rank = owner_type(&base, &vars, types).and_then(|ty| types.rank(&ty, path));
            let Some(rank) = rank else {
                failure = Some(format!("cannot determine the rank of component `{path}` of `{base}`; give its subscripts"));
                return;
            };
            let Expr::Call(_, subs) = &mut args[1] else { unreachable!() };
            let whole = (0..rank).map(|_| Expr::Section { name: String::new(), lower: None, upper: None, stride: None });
            subs.splice(0..1, whole);
        };
        for e in expressions(s) {
            visit_expr(e, &mut resolve);
        }
        // An assignment target spells the chain as `base%path` with its
        // subscripts last.
        if let Stmt::AssignIndex { name, indices, .. } = s {
            if let Some(Expr::Call(marker, subs)) = indices.last_mut() {
                if marker == "@component_subscripts" && matches!(subs.first(), Some(Expr::Call(n, _)) if n == WHOLE_AXES) {
                    let Some(Expr::Call(_, path)) = subs.first() else { unreachable!() };
                    let Some(Expr::Str(path)) = path.first() else { unreachable!() };
                    let base = name.strip_suffix(&format!("%{path}")).unwrap_or(name).to_string();
                    match owner_type(&base, &vars, types).and_then(|ty| types.rank(&ty, path)) {
                        Some(rank) => {
                            let whole = (0..rank).map(|_| Expr::Section { name: String::new(), lower: None, upper: None, stride: None });
                            subs.splice(0..1, whole);
                        }
                        None => failure = Some(format!("cannot determine the rank of component `{path}` of `{base}`; give its subscripts")),
                    }
                }
            }
        }
        if let Some(f) = failure.take() {
            bail!(f);
        }
        for child in children(s) {
            body(child, &vars, types, modules)?;
        }
    }
    Ok(())
}

/// The derived type of the array `base` (a variable or component path).
fn owner_type(base: &str, vars: &HashMap<String, (String, usize)>, types: &Types) -> Option<String> {
    let mut parts = base.split('%');
    let (mut ty, _) = vars.get(parts.next()?)?.clone();
    for name in parts {
        ty = types.component(&ty, name)?.0.clone()?;
    }
    Some(ty)
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    if !contains_marker(&mut program.body) {
        return Ok(program);
    }
    let mut modules = Modules::new();
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let mut types = Types::default();
            for u in body {
                if let Stmt::Use { module, .. } = u {
                    if let Some((t, _)) = modules.get(module) {
                        let t = t.clone();
                        types.components.extend(t.components);
                        types.parents.extend(t.parents);
                    }
                }
            }
            types.collect(body);
            modules.insert(name.clone(), (types, variables(body)));
        }
    }
    body(&mut program.body, &HashMap::new(), &Types::default(), &modules)?;
    Ok(program)
}

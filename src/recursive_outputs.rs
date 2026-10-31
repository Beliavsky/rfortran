// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Recursive subroutines with scalar dummy arguments they may define. Calls
//! of subroutines with such dummies are expanded in place (see
//! crate::subroutines), which recursion does not allow, but array dummies
//! are passed by reference. So in a subroutine that calls itself, directly
//! or through others, each scalar dummy that is not INTENT(IN) or VALUE
//! becomes a one-element array, `d` being `d(1)`, and each call passes
//! the actual through a one-element temporary, copied back after the call.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Routine {
    params: Vec<String>,
    /// The dummies that become one-element arrays, with their types and
    /// whether they are INTENT(OUT).
    outputs: HashMap<String, (TypeSpec, bool)>,
    calls: HashSet<String>,
}

fn calls(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        if let Stmt::CallSub { name, .. } = s { out.insert(name.clone()); }
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) { continue; }
        let mut s = s.clone();
        for b in crate::procedure_arguments::children(&mut s) { calls(b, out); }
    }
}

fn collect(body: &[Stmt], out: &mut HashMap<String, Routine>) {
    for s in body {
        match s {
            Stmt::Subroutine { name, params, body, .. } => {
                let intents: HashMap<&String, Vec<&String>> = body.iter().fold(HashMap::new(), |mut m, s| {
                    if let Stmt::DummyIntent { name, intent } = s { m.entry(name).or_default().push(intent); }
                    m
                });
                let mut outputs = HashMap::new();
                for p in params {
                    let kind = body.iter().find_map(|s| match s {
                        Stmt::VarDecl { kind, names } if names.contains(p) => Some(kind.clone()),
                        _ => None,
                    });
                    let Some(kind) = kind else { continue };
                    if matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed) { continue; }
                    let attrs = intents.get(p).cloned().unwrap_or_default();
                    if attrs.iter().any(|a| *a == "intent(in)" || *a == "value" || *a == "allocatable" || *a == "pointer" || *a == "optional") { continue; }
                    let out_only = attrs.iter().any(|a| *a == "intent(out)");
                    outputs.insert(p.clone(), (kind, out_only));
                }
                let mut c = HashSet::new();
                calls(body, &mut c);
                out.insert(name.clone(), Routine { params: params.clone(), outputs, calls: c });
                collect(body, out);
            }
            Stmt::Function { body, .. } | Stmt::Module { body, .. } => collect(body, out),
            _ => {}
        }
    }
}

/// The subroutines with output dummies that reach themselves by calls.
fn recursive(routines: &HashMap<String, Routine>) -> HashSet<String> {
    let mut out = HashSet::new();
    for (start, r) in routines {
        if r.outputs.is_empty() { continue; }
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = r.calls.iter().cloned().collect();
        while let Some(n) = stack.pop() {
            if n == *start { out.insert(start.clone()); break; }
            if !seen.insert(n.clone()) { continue; }
            if let Some(next) = routines.get(&n) { stack.extend(next.calls.iter().cloned()); }
        }
    }
    out
}

struct Lower {
    routines: HashMap<String, Routine>,
    recursive: HashSet<String>,
    serial: usize,
}

fn one() -> Expr { Expr::IntLit("1".into()) }

impl Lower {
    /// The scope's one-element array dummies.
    fn rewrite_refs(e: &mut Expr, arrays: &HashSet<String>) {
        crate::procedure_arguments::visit_expr(e, &mut |e| {
            if let Expr::Ident(n) = e {
                if arrays.contains(n) { *e = Expr::Call(n.clone(), vec![one()]); }
            }
        });
    }

    fn body(&mut self, body: &mut Vec<Stmt>, arrays: &HashSet<String>) {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::Subroutine { name, body, .. } => {
                    let own: HashSet<String> = if self.recursive.contains(name.as_str()) {
                        self.routines[name.as_str()].outputs.keys().cloned().collect()
                    } else { HashSet::new() };
                    if !own.is_empty() {
                        let outputs = self.routines[name.as_str()].outputs.clone();
                        for d in body.iter_mut() {
                            if let Stmt::VarDecl { names, .. } = d { names.retain(|n| !own.contains(n)); }
                        }
                        body.retain(|d| !matches!(d, Stmt::VarDecl { names, .. } if names.is_empty()));
                        let mut decls: Vec<Stmt> = outputs.iter().map(|(n, (k, _))| Stmt::ArrayDecl { kind: k.clone(), name: n.clone(), dims: vec![one()] }).collect();
                        decls.sort_by_key(|d| match d { Stmt::ArrayDecl { name, .. } => name.clone(), _ => String::new() });
                        let at = body.iter().position(|d| !matches!(d, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
                        body.splice(at..at, decls);
                    }
                    self.body(body, &own);
                    out.push(s);
                    continue;
                }
                Stmt::Function { body, .. } | Stmt::Module { body, .. } => {
                    self.body(body, &HashSet::new());
                    out.push(s);
                    continue;
                }
                Stmt::CallSub { name, args } if self.recursive.contains(name.as_str()) => {
                    let r = self.routines[name.as_str()].clone();
                    let mut prefix = vec![];
                    let mut back = vec![];
                    for (i, a) in args.iter_mut().enumerate() {
                        let (param, value) = match a {
                            Expr::Call(k, v) if k.starts_with("@keyword:") => (k["@keyword:".len()..].to_owned(), &mut v[0]),
                            other => match r.params.get(i) { Some(p) => (p.clone(), other), None => continue },
                        };
                        let Some((kind, out_only)) = r.outputs.get(&param).cloned() else {
                            Self::rewrite_refs(value, arrays);
                            continue;
                        };
                        // A one-element array dummy of this scope passes itself.
                        if matches!(value, Expr::Ident(n) if arrays.contains(n)) { continue; }
                        Self::rewrite_refs(value, arrays);
                        self.serial += 1;
                        let temp = format!("@byref_{}", self.serial);
                        prefix.push(Stmt::ArrayDecl { kind, name: temp.clone(), dims: vec![one()] });
                        if !out_only {
                            prefix.push(Stmt::AssignIndex { name: temp.clone(), indices: vec![one()], value: value.clone() });
                        }
                        let element = Expr::Call(temp.clone(), vec![one()]);
                        match value.clone() {
                            Expr::Ident(n) => back.push(Stmt::Assign { name: n, value: element }),
                            Expr::Call(n, idx) if !n.starts_with('@') => back.push(Stmt::AssignIndex { name: n, indices: idx, value: element }),
                            _ => {}
                        }
                        *value = Expr::Ident(temp);
                    }
                    out.extend(prefix);
                    out.push(s);
                    out.extend(back);
                    continue;
                }
                _ => {}
            }
            if !arrays.is_empty() {
                match &mut s {
                    Stmt::Assign { name, value } if arrays.contains(name.as_str()) => {
                        Self::rewrite_refs(value, arrays);
                        s = Stmt::AssignIndex { name: name.clone(), indices: vec![one()], value: value.clone() };
                        out.push(s);
                        continue;
                    }
                    _ => {}
                }
                for e in crate::procedure_arguments::expressions(&mut s) { Self::rewrite_refs(e, arrays); }
            }
            for b in crate::procedure_arguments::children(&mut s) { self.body(b, arrays); }
            out.push(s);
        }
        *body = out;
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut routines = HashMap::new();
    collect(&program.body, &mut routines);
    let recursive = recursive(&routines);
    if recursive.is_empty() { return Ok(program); }
    let mut lower = Lower { routines, recursive, serial: 0 };
    lower.body(&mut program.body, &HashSet::new());
    hoist(&mut program.body);
    Ok(program)
}

/// Declare the temporaries at the start of their unit.
fn hoist(body: &mut Vec<Stmt>) {
    fn take(body: &mut Vec<Stmt>, out: &mut Vec<Stmt>) {
        body.retain(|s| {
            let t = matches!(s, Stmt::ArrayDecl { name, .. } if name.starts_with("@byref_"));
            if t { out.push(s.clone()); }
            !t
        });
        for s in body.iter_mut() {
            if matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }) { continue; }
            for b in crate::procedure_arguments::children(s) { take(b, out); }
        }
    }
    let mut decls = vec![];
    take(body, &mut decls);
    if !decls.is_empty() {
        let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::SourceLocation(_) | Stmt::Implicit(_))).unwrap_or(body.len());
        body.splice(at..at, decls);
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => hoist(body),
            _ => {}
        }
    }
}

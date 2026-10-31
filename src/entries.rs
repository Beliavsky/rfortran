// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! ENTRY statements. A procedure with entries becomes one procedure for each
//! entry point, each a copy of the whole procedure: the copy for an entry
//! jumps from the start of its executable statements to the ENTRY's place,
//! and has that entry's dummy arguments and result. The other entries'
//! dummies become locals of the copy, unused in a valid program, and the
//! results of the same type as the copy's are its result, since all such
//! results share storage. Locals that keep their values between calls (SAVE,
//! DATA or an initializer) are a COMMON block of the copies, so that all
//! entry points share them; the first copy alone gives their initial values.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use std::collections::{HashMap, HashSet};

const ENTRY: &str = "@entry";

/// The statement from an ENTRY token's text `name;dummy,...;result`.
pub(crate) fn statement(text: &str) -> Stmt {
    let mut parts = text.split(';');
    let name = parts.next().unwrap_or_default().to_owned();
    let dummies = parts.next().unwrap_or_default().split(',').filter(|d| !d.is_empty()).map(|d| Expr::Ident(d.to_owned())).collect();
    let result = parts.next().unwrap_or_default().to_owned();
    Stmt::CallSub { name: ENTRY.into(), args: vec![Expr::Str(name), Expr::Call("@dummies".into(), dummies), Expr::Str(result)] }
}

struct Entry {
    name: String,
    params: Vec<String>,
    /// The result variable of a function entry.
    result: String,
}

pub(crate) fn resolve(program: &mut Program) -> Result<(), String> {
    let mut renamed: Vec<(String, Vec<String>)> = vec![];
    unit(&mut program.body, &mut renamed, true)?;
    for (name, names) in renamed {
        if program.contains.contains(&name) { program.contains.extend(names); }
    }
    Ok(())
}

fn has_entry(body: &[Stmt]) -> bool {
    body.iter().any(|s| matches!(s, Stmt::CallSub { name, .. } if name == ENTRY))
}

/// Replace the procedures of `body` that have entries by their copies.
fn unit(body: &mut Vec<Stmt>, renamed: &mut Vec<(String, Vec<String>)>, top: bool) -> Result<(), String> {
    if top || !body.is_empty() {
        for s in body.iter() {
            nested(s)?;
        }
    }
    let mut i = 0;
    while i < body.len() {
        match &mut body[i] {
            Stmt::Module { body, .. } => unit(body, renamed, false)?,
            Stmt::Function { body: b, .. } | Stmt::Subroutine { body: b, .. } => {
                unit(b, renamed, false)?;
                if has_entry(b) {
                    let copies = expand(body[i].clone())?;
                    let name = match &body[i] { Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => name.clone(), _ => unreachable!() };
                    let names = copies.iter().skip(1).map(|c| match c { Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => name.clone(), _ => unreachable!() }).collect();
                    renamed.push((name, names));
                    let count = copies.len();
                    body.splice(i..=i, copies);
                    i += count;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if top && has_entry(body) { return Err("ENTRY is allowed only in a subroutine or function".into()); }
    Ok(())
}

/// Reject ENTRY inside a construct.
fn nested(s: &Stmt) -> Result<(), String> {
    let bodies: Vec<&Vec<Stmt>> = match s {
        Stmt::If { then_body, else_body, .. } => std::iter::once(then_body).chain(else_body.as_ref()).collect(),
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => vec![body],
        Stmt::SelectCase { cases, default, .. } => cases.iter().map(|c| &c.body).chain(default.as_ref()).collect(),
        _ => return Ok(()),
    };
    for b in bodies {
        if has_entry(b) { return Err("ENTRY cannot be inside a construct".into()); }
        for s in b { nested(s)?; }
    }
    Ok(())
}

/// Whether a statement belongs to the specification part, before the
/// executable statements where an entry's jump goes.
fn specification(s: &Stmt) -> bool {
    match s {
        Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. }
        | Stmt::DerivedType { .. } | Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::DummyIntent { .. } | Stmt::SourceLocation(_)
        | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } => true,
        Stmt::CallSub { name, .. } => name == "@data" || name == crate::common::STATEMENT || name == crate::equivalence::STATEMENT
            || name == "@format" || name.starts_with("@namelist") || name.starts_with("@interface"),
        _ => false,
    }
}

/// The type of `name` from its declaration in `body`, or by IMPLICIT rules.
fn type_of(body: &[Stmt], name: &str) -> Option<String> {
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } if names.iter().any(|n| n == name) => return Some(format!("{kind:?}")),
            Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| n == name) => return Some(format!("{kind:?}")),
            Stmt::ArrayDecl { kind, name: n, dims } if n == name => return Some(format!("{kind:?}{dims:?}")),
            Stmt::DerivedDecl { type_name, names, .. } if names.iter().any(|n| n == name) => return Some(format!("type({type_name})")),
            _ => {}
        }
    }
    let letter = name.chars().next()?.to_ascii_lowercase();
    crate::common::implicit_rules(body)?.get(&letter).map(|k: &TypeSpec| format!("{k:?}"))
}

/// Remove the declarations of `name` from `body`.
fn undeclare(body: &mut Vec<Stmt>, name: &str) {
    for s in body.iter_mut() {
        match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.retain(|n| n != name),
            Stmt::InitializedDecl { entities, .. } => entities.retain(|(n, _)| n != name),
            _ => {}
        }
    }
    body.retain(|s| match s {
        Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => !names.is_empty(),
        Stmt::InitializedDecl { entities, .. } => !entities.is_empty(),
        Stmt::ArrayDecl { name: n, .. } | Stmt::DummyIntent { name: n, .. } => n != name,
        _ => true,
    });
}

/// Turn the RETURN statements that name the result back into plain RETURN.
fn plain_returns(body: &mut [Stmt], result: &str) {
    for s in body.iter_mut() {
        if matches!(s, Stmt::Return(Some(Expr::Ident(r))) if r == result) { *s = Stmt::Return(None); }
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) { continue; }
        for b in crate::procedure_arguments::children(s) { plain_returns(b, result); }
    }
}

fn labels(body: &[Stmt], out: &mut Vec<i64>) {
    for s in body {
        if let Stmt::CallSub { name, args } = s {
            if name == "@statement_label" {
                if let Some(Expr::IntLit(n)) = args.first() { out.extend(n.parse::<i64>().ok()); }
            }
        }
        let mut s = s.clone();
        for b in crate::procedure_arguments::children(&mut s) { labels(b, out); }
    }
}

/// The variables a DATA statement names.
fn data_names(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(n) | Expr::Section { name: n, .. } => { out.insert(n.clone()); }
        Expr::Call(n, args) if n.starts_with('@') => for a in args { data_names(a, out) },
        Expr::Call(n, _) | Expr::Index(n, _) => { out.insert(n.clone()); }
        _ => {}
    }
}

fn expand(procedure: Stmt) -> Result<Vec<Stmt>, String> {
    let (function, name, params, mut body, return_type, pure, elemental) = match procedure {
        Stmt::Function { name, params, body, return_type, pure, elemental } => (true, name, params, body, return_type, pure, elemental),
        Stmt::Subroutine { name, params, body, pure, elemental } => (false, name, params, body, None, pure, elemental),
        _ => unreachable!(),
    };
    // The function's result, without the RETURN that names it.
    let mut result = name.clone();
    if function {
        if let Some(Stmt::Return(Some(Expr::Ident(r)))) = body.last() {
            if *r != name {
                result = r.clone();
                body.pop();
                plain_returns(&mut body, &result);
            }
        }
    }
    let mut entries = vec![Entry { name: name.clone(), params: params.clone(), result }];
    for s in &body {
        let Stmt::CallSub { name: n, args } = s else { continue };
        if n != ENTRY { continue; }
        let [Expr::Str(e), Expr::Call(_, dummies), Expr::Str(r)] = args.as_slice() else { unreachable!() };
        let params: Vec<String> = dummies.iter().map(|d| match d { Expr::Ident(d) => d.clone(), _ => unreachable!() }).collect();
        if params.iter().any(|p| p == "*") { return Err(format!("alternate returns of ENTRY `{e}` are not yet supported")); }
        if !function && !r.is_empty() { return Err(format!("ENTRY `{e}` of a subroutine cannot have a RESULT")); }
        entries.push(Entry { name: e.clone(), params, result: if r.is_empty() { e.clone() } else { r.clone() } });
    }
    let dummies: HashSet<String> = entries.iter().flat_map(|e| e.params.iter().cloned()).collect();
    let results: HashSet<String> = entries.iter().map(|e| e.result.clone()).chain(entries.iter().map(|e| e.name.clone())).collect();
    let mut used = vec![];
    labels(&body, &mut used);
    let base = used.into_iter().max().unwrap_or(0).max(90000) + 1;

    // The locals that keep their values between calls.
    let mut saved: HashSet<String> = HashSet::new();
    let mut save_all = false;
    let mut excluded: HashSet<String> = HashSet::new();
    for s in &body {
        match s {
            Stmt::DummyIntent { name, intent } if intent == "save" => {
                if name == crate::data_statements::SAVE_ALL { save_all = true; } else { saved.insert(name.clone()); }
            }
            Stmt::DummyIntent { name, intent } if matches!(intent.as_str(), "allocatable" | "pointer" | "target" | "external" | "intrinsic") => { excluded.insert(name.clone()); }
            Stmt::InitializedDecl { attributes, entities, kind } => {
                if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) || matches!(kind, TypeSpec::CharacterAssumed) {
                    excluded.extend(entities.iter().map(|(n, _)| n.clone()));
                } else {
                    saved.extend(entities.iter().filter(|(_, v)| v.is_some()).map(|(n, _)| n.clone()));
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("save")) { saved.extend(entities.iter().map(|(n, _)| n.clone())); }
                }
            }
            Stmt::DerivedDecl { names, .. } => excluded.extend(names.iter().cloned()),
            Stmt::CallSub { name, args } if name == "@data" => {
                for set in args {
                    if let Expr::Call(_, parts) = set {
                        if let Some(Expr::Call(_, objects)) = parts.first() { for o in objects { data_names(o, &mut saved) } }
                    }
                }
            }
            Stmt::CallSub { name, args } if name == crate::common::STATEMENT => {
                for a in args { if let Expr::Ident(n) | Expr::Call(n, _) = a { excluded.insert(n.clone()); } }
            }
            _ => {}
        }
    }
    if save_all {
        for s in &body {
            match s {
                Stmt::VarDecl { names, .. } => saved.extend(names.iter().cloned()),
                Stmt::ArrayDecl { name, .. } => { saved.insert(name.clone()); }
                Stmt::InitializedDecl { entities, .. } => saved.extend(entities.iter().map(|(n, _)| n.clone())),
                _ => {}
            }
        }
    }
    let declared: HashSet<String> = body.iter().flat_map(|s| match s {
        Stmt::VarDecl { names, .. } => names.clone(),
        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
        _ => vec![],
    }).collect();
    let mut shared: Vec<String> = saved.into_iter()
        .filter(|n| declared.contains(n) && !dummies.contains(n) && !results.contains(n) && !excluded.contains(n))
        .collect();
    shared.sort();

    let types: HashMap<String, Option<String>> = entries.iter().map(|e| (e.result.clone(), type_of(&body, &e.result))).collect();
    let mut copies = vec![];
    for (k, entry) in entries.iter().enumerate() {
        let mut b = body.clone();
        // Each ENTRY becomes a labeled CONTINUE.
        let mut j = 0;
        let mut out = vec![];
        for s in b.drain(..) {
            if matches!(&s, Stmt::CallSub { name, .. } if name == ENTRY) {
                j += 1;
                out.push(Stmt::CallSub { name: "@statement_label".into(), args: vec![Expr::IntLit((base + j).to_string())] });
                out.push(Stmt::CallSub { name: "@continue".into(), args: vec![] });
            } else {
                out.push(s);
            }
        }
        b = out;
        // The other entries' dummies are locals here.
        for d in dummies.iter().filter(|d| !entry.params.contains(d)) {
            b.retain(|s| !matches!(s, Stmt::DummyIntent { name, intent } if name == d
                && (intent.starts_with("intent") || intent == "optional" || intent == "value")));
            for s in b.iter_mut() {
                match s {
                    Stmt::ArrayDecl { name, dims, .. } if name == d => *dims = vec![Expr::IntLit("1".into()); dims.len()],
                    Stmt::DerivedDecl { names, dims, .. } if names.contains(d) && !dims.is_empty() => *dims = vec![Expr::IntLit("1".into()); dims.len()],
                    _ => {}
                }
            }
        }
        // Results of the same type as this entry's are its result.
        if function {
            for other in entries.iter().map(|e| &e.result).collect::<HashSet<_>>() {
                if *other == entry.result || types[other].is_none() || types[other] != types[&entry.result] { continue; }
                undeclare(&mut b, other);
                crate::procedure_arguments::rename(&mut b, other, &entry.result);
            }
        }
        // Shared locals, with their initial values only in the first copy.
        if k > 0 {
            for s in b.iter_mut() {
                if let Stmt::InitializedDecl { attributes, entities, .. } = s {
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) { continue; }
                    for (n, v) in entities.iter_mut() { if shared.contains(n) { *v = None; } }
                }
            }
            b.retain(|s| match s {
                Stmt::CallSub { name, args } if name == "@data" => {
                    let mut names = HashSet::new();
                    for set in args {
                        if let Expr::Call(_, parts) = set {
                            if let Some(Expr::Call(_, objects)) = parts.first() { for o in objects { data_names(o, &mut names) } }
                        }
                    }
                    !names.iter().all(|n| shared.contains(n))
                }
                _ => true,
            });
        }
        if !shared.is_empty() {
            b.retain(|s| !matches!(s, Stmt::DummyIntent { name, intent } if intent == "save" && shared.contains(name)));
        }
        let at = b.iter().position(|s| !specification(s)).unwrap_or(b.len());
        let mut head = vec![];
        if !shared.is_empty() {
            let mut args = vec![Expr::Str(format!("rfort_entry_{name}"))];
            args.extend(shared.iter().map(|n| Expr::Ident(n.clone())));
            head.push(Stmt::CallSub { name: crate::common::STATEMENT.into(), args });
        }
        if k > 0 {
            head.push(Stmt::CallSub { name: "@goto".into(), args: vec![Expr::IntLit((base + k as i64).to_string())] });
        }
        b.splice(at..at, head);
        copies.push(if !function {
            Stmt::Subroutine { name: entry.name.clone(), params: entry.params.clone(), body: b, pure, elemental }
        } else if entry.result != entry.name {
            match crate::ast::function_with_result(entry.name.clone(), entry.params.clone(), entry.result.clone(), b) {
                Stmt::Function { name, params, return_type, body, .. } => Stmt::Function { name, params, return_type, body, pure, elemental },
                _ => unreachable!(),
            }
        } else {
            let return_type = if k == 0 { return_type.clone() } else { None };
            Stmt::Function { name: entry.name.clone(), params: entry.params.clone(), return_type, body: b, pure, elemental }
        });
    }
    Ok(copies)
}

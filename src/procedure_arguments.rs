// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize procedures with dummy procedure arguments.
//!
//! A call that passes a procedure name to a dummy procedure (declared EXTERNAL,
//! in an interface block, with PROCEDURE(interface), or only invoked) is
//! redirected to a copy of the callee in which the dummy is replaced by that
//! procedure. Copies are made on demand until no call passes a procedure, so a
//! dummy procedure forwarded to another procedure is specialized in turn. The
//! templates are then removed, together with the declarations that only
//! described procedures.
use crate::ast::{CaseItem, Expr, Program, Stmt};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

/// Attributes that declare a name to be a procedure rather than a variable.
fn procedure_attribute(intent: &str) -> bool {
    intent == "external" || intent.starts_with("@interface:") || intent.starts_with("procedure(")
}

#[derive(Clone)]
struct Template {
    params: Vec<String>,
    /// Positions of dummy procedures among `params`.
    dummies: Vec<usize>,
}

pub(crate) fn children(s: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match s {
        Stmt::Function { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Module { body, .. }
        | Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. } => vec![body],
        Stmt::If { then_body, else_body, .. } => {
            let mut v = vec![then_body];
            if let Some(b) = else_body {
                v.push(b);
            }
            v
        }
        Stmt::SelectCase { cases, default, .. } => {
            let mut v: Vec<&mut Vec<Stmt>> = cases.iter_mut().map(|c| &mut c.body).collect();
            if let Some(b) = default {
                v.push(b);
            }
            v
        }
        _ => vec![],
    }
}

/// The expressions held directly by a statement (not by nested bodies).
pub(crate) fn expressions(s: &mut Stmt) -> Vec<&mut Expr> {
    match s {
        Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => items.iter_mut().collect(),
        Stmt::Assign { value, .. } => vec![value],
        Stmt::AssignIndex { indices, value, .. } => indices.iter_mut().chain(std::iter::once(value)).collect(),
        Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => vec![cond],
        Stmt::Do { start, end, .. } => vec![start, end],
        Stmt::Return(Some(e)) => vec![e],
        Stmt::ArrayDecl { dims, .. } => dims.iter_mut().collect(),
        Stmt::InitializedDecl { entities, .. } => entities.iter_mut().filter_map(|(_, v)| v.as_mut()).collect(),
        Stmt::DerivedDecl { dims, .. } => dims.iter_mut().collect(),
        Stmt::SelectCase { expr, cases, .. } => {
            let mut v = vec![expr];
            for c in cases {
                for item in &mut c.items {
                    match item {
                        CaseItem::Single(e) => v.push(e),
                        CaseItem::Range(a, b) => {
                            v.push(a);
                            v.push(b);
                        }
                    }
                }
            }
            v
        }
        _ => vec![],
    }
}

pub(crate) fn visit_expr(e: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args {
                visit_expr(a, f);
            }
        }
        Expr::Bin(_, a, b) => {
            visit_expr(a, f);
            visit_expr(b, f);
        }
        Expr::Un(_, a) => visit_expr(a, f),
        Expr::Section { lower, upper, stride, .. } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                visit_expr(e, f);
            }
        }
        _ => {}
    }
    f(e);
}

/// Rename every use of `from` (as a procedure, variable or declared entity)
/// to `to`, including inside nested bodies.
fn rename(body: &mut [Stmt], from: &str, to: &str) {
    for s in body.iter_mut() {
        match s {
            Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } | Stmt::CallSub { name, .. }
            | Stmt::DummyIntent { name, .. } | Stmt::ArrayDecl { name, .. }
                if name == from => *name = to.into(),
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                for n in names.iter_mut().filter(|n| *n == from) {
                    *n = to.into();
                }
            }
            Stmt::InitializedDecl { entities, .. } => {
                for (n, _) in entities.iter_mut().filter(|(n, _)| n == from) {
                    *n = to.into();
                }
            }
            _ => {}
        }
        for e in expressions(s) {
            visit_expr(e, &mut |e| match e {
                Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } if n == from => {
                    *n = to.into()
                }
                _ => {}
            });
        }
        for b in children(s) {
            rename(b, from, to);
        }
    }
}

/// Names invoked as procedures in a body (CALL targets and function references).
fn invoked(body: &[Stmt], names: &mut HashSet<String>) {
    for s in body {
        let mut s = s.clone();
        if let Stmt::CallSub { name, .. } = &s {
            names.insert(name.clone());
        }
        for e in expressions(&mut s) {
            visit_expr(e, &mut |e| {
                if let Expr::Call(n, _) = e {
                    names.insert(n.clone());
                }
            });
        }
        for b in children(&mut s) {
            invoked(b, names);
        }
    }
}

fn template(params: &[String], body: &[Stmt]) -> Option<Template> {
    let declared_procedure: HashSet<&str> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if procedure_attribute(intent) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    // Any data declaration (type, shape, INTENT, ...) makes a dummy a variable;
    // only an undeclared dummy that is invoked is inferred to be a procedure.
    let mut data: HashSet<&str> = HashSet::new();
    for s in body {
        match s {
            Stmt::ArrayDecl { name, .. } => { data.insert(name); }
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => data.extend(names.iter().map(String::as_str)),
            Stmt::InitializedDecl { entities, .. } => data.extend(entities.iter().map(|(n, _)| n.as_str())),
            Stmt::DummyIntent { name, intent } if !procedure_attribute(intent) => { data.insert(name); }
            _ => {}
        }
    }
    let mut calls = HashSet::new();
    invoked(body, &mut calls);
    let dummies: Vec<usize> = params
        .iter()
        .enumerate()
        .filter(|(_, p)| declared_procedure.contains(p.as_str()) || calls.contains(*p) && !data.contains(p.as_str()))
        .map(|(i, _)| i)
        .collect();
    (!dummies.is_empty()).then(|| Template { params: params.to_vec(), dummies })
}

fn collect(body: &[Stmt], templates: &mut HashMap<String, Template>, procedures: &mut HashSet<String>) -> Result<()> {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                procedures.insert(name.clone());
                if let Some(t) = template(params, body) {
                    if templates.insert(name.clone(), t).is_some() {
                        bail!("procedure `{name}` with dummy procedure arguments is defined more than once");
                    }
                }
                collect(body, templates, procedures)?;
            }
            Stmt::Module { body, .. } => collect(body, templates, procedures)?,
            _ => {}
        }
    }
    Ok(())
}

struct Specializer {
    templates: HashMap<String, Template>,
    procedures: HashSet<String>,
    /// (template, actual procedures) -> specialized name
    made: HashMap<(String, Vec<String>), String>,
    /// Specializations to insert next to their template.
    pending: Vec<(String, Stmt)>,
}

impl Specializer {
    /// The actual procedure passed for each dummy procedure, if all are names.
    fn actuals(&self, callee: &str, args: &[Expr], scope_dummies: &HashSet<String>) -> Result<Option<Vec<String>>> {
        let t = &self.templates[callee];
        let mut out = vec![];
        for &i in &t.dummies {
            let dummy = &t.params[i];
            let keyword = format!("@keyword:{dummy}");
            let actual = args.iter().find_map(|a| match a {
                Expr::Call(k, v) if *k == keyword && v.len() == 1 => Some(&v[0]),
                _ => None,
            });
            let actual = match actual {
                Some(a) => Some(a),
                None => args.get(i).filter(|a| !matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:"))),
            };
            match actual {
                Some(Expr::Ident(n)) if scope_dummies.contains(n) => return Ok(None),
                Some(Expr::Ident(n)) if self.procedures.contains(n) => out.push(n.clone()),
                Some(Expr::Ident(n)) => bail!("actual argument `{n}` for dummy procedure `{dummy}` of `{callee}` must be a procedure defined in the program"),
                Some(Expr::Call(n, a)) if n == crate::named_arguments::OMITTED && a.is_empty() => {
                    bail!("an omitted OPTIONAL dummy procedure `{dummy}` of `{callee}` is not yet supported")
                }
                None => bail!("missing actual argument for dummy procedure `{dummy}` of `{callee}`"),
                Some(_) => bail!("actual argument for dummy procedure `{dummy}` of `{callee}` must be a procedure name"),
            }
        }
        Ok(Some(out))
    }

    fn specialize(&mut self, callee: &str, actuals: Vec<String>, definition: &Stmt) -> String {
        let key = (callee.to_string(), actuals.clone());
        if let Some(name) = self.made.get(&key) {
            return name.clone();
        }
        let name = format!("{callee}@with@{}", actuals.join("@"));
        self.made.insert(key, name.clone());
        let t = self.templates[callee].clone();
        let mut copy = definition.clone();
        if let Stmt::Function { name: n, params, body, .. } | Stmt::Subroutine { name: n, params, body, .. } = &mut copy {
            let removed: Vec<String> = t.dummies.iter().map(|&i| t.params[i].clone()).collect();
            *params = params.iter().filter(|p| !removed.contains(p)).cloned().collect();
            body.retain(|s| match s {
                Stmt::DummyIntent { name, .. } => !removed.contains(name),
                _ => true,
            });
            for s in body.iter_mut() {
                if let Stmt::VarDecl { names, .. } = s {
                    names.retain(|n| !removed.contains(n));
                }
            }
            body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
            // A function result named after the function follows the rename.
            rename(body, n, &name);
            for (dummy, actual) in removed.iter().zip(&actuals) {
                rename(body, dummy, actual);
            }
            *n = name.clone();
        }
        self.pending.push((callee.to_string(), copy));
        name
    }

    /// Redirect calls passing procedures in `body`. `definitions` holds every
    /// template definition; `dummies` are dummy procedures of the enclosing
    /// procedure, which are only resolved in its specializations.
    fn calls(&mut self, body: &mut [Stmt], definitions: &HashMap<String, Stmt>, dummies: &HashSet<String>) -> Result<()> {
        for s in body.iter_mut() {
            if let Stmt::CallSub { name, args } = s {
                if self.templates.contains_key(name.as_str()) {
                    if let Some(actuals) = self.actuals(name, args, dummies)? {
                        self.redirect(name, args, actuals, definitions);
                    }
                }
            }
            let mut found = Ok(());
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if found.is_err() {
                        return;
                    }
                    if let Expr::Call(name, args) = e {
                        if self.templates.contains_key(name.as_str()) {
                            match self.actuals(name, args, dummies) {
                                Ok(Some(actuals)) => self.redirect(name, args, actuals, definitions),
                                Ok(None) => {}
                                Err(error) => found = Err(error),
                            }
                        }
                    }
                });
            }
            found?;
            let inner = match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    // Template bodies are processed only as specializations.
                    if self.templates.contains_key(name.as_str()) {
                        continue;
                    }
                    body
                }
                _ => {
                    for b in children(s) {
                        self.calls(b, definitions, dummies)?;
                    }
                    continue;
                }
            };
            self.calls(inner, definitions, &HashSet::new())?;
        }
        Ok(())
    }

    fn redirect(&mut self, name: &mut String, args: &mut Vec<Expr>, actuals: Vec<String>, definitions: &HashMap<String, Stmt>) {
        let t = self.templates[name.as_str()].clone();
        let removed: Vec<&String> = t.dummies.iter().map(|&i| &t.params[i]).collect();
        let mut kept = vec![];
        for (i, a) in std::mem::take(args).into_iter().enumerate() {
            let keyword = matches!(&a, Expr::Call(k, _) if k.strip_prefix("@keyword:").is_some_and(|k| removed.iter().any(|r| *r == k)));
            let positional = !matches!(&a, Expr::Call(k, _) if k.starts_with("@keyword:")) && t.dummies.contains(&i);
            if !keyword && !positional {
                kept.push(a);
            }
        }
        *args = kept;
        *name = self.specialize(name, actuals, &definitions[name.as_str()]);
    }
}

fn definitions(body: &[Stmt], templates: &HashMap<String, Template>, out: &mut HashMap<String, Stmt>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                if templates.contains_key(name) {
                    out.insert(name.clone(), s.clone());
                }
                definitions(body, templates, out);
            }
            Stmt::Module { body, .. } => definitions(body, templates, out),
            _ => {}
        }
    }
}

/// Insert specializations after their templates, then drop the templates.
fn place(body: &mut Vec<Stmt>, made: &mut Vec<(String, Stmt)>, templates: &HashMap<String, Template>) {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = &mut s {
            place(body, made, templates);
        }
        match &s {
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if templates.contains_key(name) => {
                let name = name.clone();
                let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(made).into_iter().partition(|(t, _)| *t == name);
                *made = rest;
                for (_, mut copy) in mine {
                    if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } = &mut copy {
                        place(body, made, templates);
                    }
                    out.push(copy);
                }
            }
            _ => out.push(s),
        }
    }
    *body = out;
}

/// Remove declarations that only describe procedures: EXTERNAL, interface
/// bodies, ABSTRACT interfaces and PROCEDURE(interface), and the type
/// declarations of names declared EXTERNAL.
fn strip(body: &mut Vec<Stmt>) {
    let external: HashSet<String> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if procedure_attribute(intent) => Some(name.clone()),
            _ => None,
        })
        .collect();
    body.retain(|s| match s {
        Stmt::DummyIntent { name, intent } => {
            !(procedure_attribute(intent) || intent.starts_with("@abstract:") || external.contains(name) && intent == "pointer")
        }
        _ => true,
    });
    for s in body.iter_mut() {
        if let Stmt::VarDecl { names, .. } = s {
            names.retain(|n| !external.contains(n));
        }
    }
    body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
    for s in body.iter_mut() {
        for b in children(s) {
            strip(b);
        }
    }
}

/// Procedure pointers declared in a scope become INTEGER tags: 0 when
/// disassociated, otherwise the 1-based index of the target among the
/// procedures ever associated with that pointer. Calls dispatch on the tag
/// through a generated procedure with the first target's interface.
fn pointers(body: &mut Vec<Stmt>, all: &HashMap<String, Stmt>, serial: &mut usize) -> Result<()> {
    for s in body.iter_mut() {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
            pointers(body, all, serial)?;
        }
    }
    let procedure: HashSet<String> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent.starts_with("procedure(") => Some(name.clone()),
            _ => None,
        })
        .collect();
    let declared: Vec<String> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "pointer" && procedure.contains(name) => Some(name.clone()),
            _ => None,
        })
        .collect();
    for pointer in declared {
        // Targets are collected from this scope and procedures it contains.
        fn targets(body: &[Stmt], pointer: &str, out: &mut Vec<String>) {
            for s in body {
                if let Stmt::CallSub { name, args } = s {
                    if name == "@pointer_associate" {
                        if let [Expr::Ident(p), Expr::Ident(t)] = args.as_slice() {
                            if p == pointer && !out.contains(t) {
                                out.push(t.clone());
                            }
                        }
                    }
                }
                let mut s = s.clone();
                for b in children(&mut s) {
                    targets(b, pointer, out);
                }
            }
        }
        let mut found = vec![];
        targets(body, &pointer, &mut found);
        for t in &found {
            if !all.contains_key(t) {
                bail!("procedure pointer `{pointer}` target `{t}` must be a procedure defined in the program");
            }
        }
        *serial += 1;
        let dispatch = format!("{pointer}@dispatch{serial}");
        let function = found.first().is_some_and(|t| matches!(all[t], Stmt::Function { .. }));
        let index = |t: &str| found.iter().position(|f| f == t).map(|i| i + 1);
        fn rewrite(body: &mut [Stmt], pointer: &str, dispatch: &str, found: &[String], index: &dyn Fn(&str) -> Option<usize>) -> Result<()> {
            for s in body.iter_mut() {
                // A contained procedure that declares the name shadows the pointer.
                if let Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } = s {
                    let shadows = params.iter().any(|p| p == pointer)
                        || body.iter().any(|d| matches!(d, Stmt::VarDecl { names, .. } if names.iter().any(|n| n == pointer))
                            || matches!(d, Stmt::DummyIntent { name, .. } | Stmt::ArrayDecl { name, .. } if name == pointer));
                    if !shadows {
                        rewrite(body, pointer, dispatch, found, index)?;
                    }
                    continue;
                }
                match s {
                    Stmt::CallSub { name, args } if name == "@pointer_associate" && matches!(args.first(), Some(Expr::Ident(p)) if p == pointer) => {
                        let value = match args.get(1) {
                            Some(Expr::Ident(t)) => Expr::IntLit(index(t).unwrap().to_string()),
                            Some(Expr::Call(n, a)) if n.eq_ignore_ascii_case("null") && a.is_empty() => Expr::IntLit("0".into()),
                            _ => bail!("procedure pointer `{pointer}` can only be associated with a procedure name or NULL()"),
                        };
                        *s = Stmt::Assign { name: pointer.into(), value };
                        continue;
                    }
                    Stmt::CallSub { name, args } if name == "@nullify" && args.iter().any(|a| matches!(a, Expr::Ident(p) if p == pointer)) => {
                        if args.len() != 1 {
                            bail!("NULLIFY of procedure pointer `{pointer}` must name it alone");
                        }
                        *s = Stmt::Assign { name: pointer.into(), value: Expr::IntLit("0".into()) };
                        continue;
                    }
                    Stmt::CallSub { name, args } if name == pointer => {
                        args.insert(0, Expr::Ident(pointer.into()));
                        *name = dispatch.into();
                    }
                    _ => {}
                }
                let mut error = Ok(());
                for e in expressions(s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(n, args) = e {
                            if n.eq_ignore_ascii_case("associated") && matches!(args.first(), Some(Expr::Ident(p)) if p == pointer) {
                                let value = match args.get(1) {
                                    None => Expr::Bin(crate::ast::BinOp::Ne, Box::new(Expr::Ident(pointer.into())), Box::new(Expr::IntLit("0".into()))),
                                    Some(Expr::Ident(t)) => match index(t) {
                                        Some(i) => Expr::Bin(crate::ast::BinOp::Eq, Box::new(Expr::Ident(pointer.into())), Box::new(Expr::IntLit(i.to_string()))),
                                        None => Expr::Logical(false),
                                    },
                                    _ => {
                                        error = Err(anyhow::anyhow!("ASSOCIATED target for procedure pointer `{pointer}` must be a procedure name"));
                                        return;
                                    }
                                };
                                *e = value;
                            } else if n == pointer {
                                args.insert(0, Expr::Ident(pointer.into()));
                                *n = dispatch.into();
                            }
                        }
                    });
                }
                error?;
                for b in children(s) {
                    rewrite(b, pointer, dispatch, found, index)?;
                }
            }
            Ok(())
        }
        rewrite(body, &pointer, &dispatch, &found, &index)?;
        // The pointer itself becomes a tag, initially disassociated.
        body.retain(|s| !matches!(s, Stmt::DummyIntent { name, .. } if *name == pointer));
        let position = body.iter().position(|s| !matches!(s, Stmt::SourceLocation(_) | Stmt::Use { .. } | Stmt::Implicit(_))).unwrap_or(0);
        body.insert(position, Stmt::InitializedDecl {
            kind: crate::ast::TypeSpec::Integer(None),
            attributes: vec![],
            entities: vec![(pointer.clone(), Some(Expr::IntLit("0".into())))],
        });
        if let Some(first) = found.first() {
            body.push(dispatcher(&dispatch, &pointer, function, &found, &all[first])?);
        }
    }
    Ok(())
}

/// A procedure taking the pointer tag followed by the first target's dummy
/// arguments, which calls the target selected by the tag.
fn dispatcher(name: &str, pointer: &str, function: bool, targets: &[String], first: &Stmt) -> Result<Stmt> {
    let (Stmt::Function { params, body, name: target, .. } | Stmt::Subroutine { params, body, name: target, .. }) = first else { unreachable!() };
    let result = match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(),
        _ => target.clone(),
    };
    let tag = format!("{pointer}@tag");
    let mut out = vec![
        Stmt::VarDecl { kind: crate::ast::TypeSpec::Integer(None), names: vec![tag.clone()] },
        Stmt::DummyIntent { name: tag.clone(), intent: "intent(in)".into() },
    ];
    // The dummy declarations (and result declaration) of the first target.
    for s in body {
        let keep = match s {
            Stmt::VarDecl { names, .. } => names.iter().any(|n| params.contains(n) || function && *n == result),
            Stmt::ArrayDecl { name, .. } | Stmt::DummyIntent { name, .. } => params.contains(name),
            _ => false,
        };
        if keep {
            out.push(s.clone());
        }
    }
    if function {
        rename(&mut out, &result, name);
    }
    let args: Vec<Expr> = params.iter().map(|p| Expr::Ident(p.clone())).collect();
    let cases = targets
        .iter()
        .enumerate()
        .map(|(i, t)| crate::ast::CaseBlock {
            items: vec![CaseItem::Single(Expr::IntLit((i + 1).to_string()))],
            body: vec![if function {
                Stmt::Assign { name: name.into(), value: Expr::Call(t.clone(), args.clone()) }
            } else {
                Stmt::CallSub { name: t.clone(), args: args.clone() }
            }],
        })
        .collect();
    out.push(Stmt::SelectCase {
        expr: Expr::Ident(tag.clone()),
        cases,
        default: Some(vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(format!("call through disassociated procedure pointer `{pointer}`"))],
        }]),
    });
    if function {
        out.push(Stmt::Return(Some(Expr::Ident(name.into()))));
    }
    let mut params_out = vec![tag];
    params_out.extend(params.iter().cloned());
    Ok(if function {
        let return_type = match first { Stmt::Function { return_type, .. } => return_type.clone(), _ => None };
        Stmt::Function { elemental: false, pure: false, name: name.into(), params: params_out, return_type, body: out }
    } else {
        Stmt::Subroutine { elemental: false, pure: false, name: name.into(), params: params_out, body: out }
    })
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut templates = HashMap::new();
    let mut procedures = HashSet::new();
    collect(&program.body, &mut templates, &mut procedures)?;
    let mut all = HashMap::new();
    fn every(body: &[Stmt], out: &mut HashMap<String, Stmt>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    out.insert(name.clone(), s.clone());
                    every(body, out);
                }
                Stmt::Module { body, .. } => every(body, out),
                _ => {}
            }
        }
    }
    every(&program.body, &mut all);
    let mut serial = 0;
    pointers(&mut program.body, &all, &mut serial)?;
    if !templates.is_empty() {
        let mut defs = HashMap::new();
        definitions(&program.body, &templates, &mut defs);
        let mut s = Specializer { templates: templates.clone(), procedures, made: HashMap::new(), pending: vec![] };
        s.calls(&mut program.body, &defs, &HashSet::new())?;
        // Specializations can pass their substituted procedures onward.
        let mut placed = vec![];
        let mut rounds = 0;
        while !s.pending.is_empty() {
            rounds += 1;
            if rounds > 64 {
                bail!("dummy procedure specialization does not terminate; recursive procedure arguments are not yet supported");
            }
            let mut batch = std::mem::take(&mut s.pending);
            for (_, copy) in batch.iter_mut() {
                if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } = copy {
                    s.calls(body, &defs, &HashSet::new())?;
                }
            }
            placed.extend(batch);
        }
        place(&mut program.body, &mut placed, &templates);
        if let Some((template, _)) = placed.first() {
            bail!("could not place the specialization of `{template}`");
        }
    }
    strip(&mut program.body);
    Ok(program)
}

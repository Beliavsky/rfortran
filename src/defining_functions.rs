// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Module and internal functions that define a scalar dummy argument, as in
//! `k = k + 1` in the body of `f(k)`. The native ABI passes scalars by value,
//! so a reference to such a function would leave its actual argument
//! unchanged. Each one gets a subroutine twin whose last argument is the
//! result, and each reference becomes a call of the twin before its
//! statement, which the expansion of output-argument subroutines gives the
//! actual's storage. External functions are called through their
//! by-reference entry points instead (see external_calls).
use crate::ast::{Expr, Program, Stmt, TypeSpec, UnOp};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::Result;
use std::collections::HashMap;

/// The prefix of the name of a function's subroutine twin.
const TWIN: &str = "rfort_defining_";
/// The name of the twin's result argument.
const RESULT: &str = "rfort_result";

/// The twin of function `name` with result type `kind`, or None if its body
/// does not define a scalar dummy.
fn twin(name: &str, params: &[String], body: &[Stmt], kind: &TypeSpec) -> Option<Stmt> {
    // The expansion of the twin's calls requires declared scalar dummies.
    // CHARACTER dummies and results have their own copies.
    let character = |k: &TypeSpec| matches!(k, TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed);
    let scalar = |p: &String| body.iter().any(|s| matches!(s, Stmt::VarDecl { names, kind } if names.contains(p) && !character(kind)));
    if character(kind) || !params.iter().all(scalar) || !params.iter().any(|p| crate::external_calls::defines(body, p)) {
        return None;
    }
    // The expansion does not take a recursive twin.
    let mut recursive = false;
    let mut body_copy = body.to_vec();
    fn references(body: &mut [Stmt], name: &str, found: &mut bool) {
        for s in body.iter_mut() {
            for e in expressions(s) {
                visit_expr(e, &mut |e| *found |= matches!(e, Expr::Call(n, _) if n == name));
            }
            *found |= matches!(s, Stmt::CallSub { name: n, .. } if n == name);
            for b in children(s) {
                references(b, name, found);
            }
        }
    }
    references(&mut body_copy, name, &mut recursive);
    if recursive {
        return None;
    }
    fn result(body: &mut [Stmt], name: &str) {
        for s in body.iter_mut() {
            match s {
                Stmt::Assign { name: n, .. } if n == name => *n = RESULT.into(),
                Stmt::VarDecl { names, .. } => {
                    for n in names.iter_mut().filter(|n| *n == name) {
                        *n = RESULT.into();
                    }
                }
                // A RESULT variable is returned by value.
                Stmt::Return(Some(value)) => {
                    *s = Stmt::Block { body: vec![
                        Stmt::Assign { name: RESULT.into(), value: value.clone() },
                        Stmt::Return(None),
                    ] };
                    continue;
                }
                _ => {}
            }
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if matches!(e, Expr::Ident(n) if n == name) {
                        *e = Expr::Ident(RESULT.into());
                    }
                });
            }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                for b in children(s) {
                    result(b, name);
                }
            }
        }
    }
    let mut body = body.to_vec();
    result(&mut body, name);
    if !body.iter().any(|s| matches!(s, Stmt::VarDecl { names, .. } if names.iter().any(|n| n == RESULT))) {
        body.insert(0, Stmt::VarDecl { kind: kind.clone(), names: vec![RESULT.into()] });
    }
    body.insert(0, Stmt::DummyIntent { name: RESULT.into(), intent: "intent(out)".into() });
    let mut params = params.to_vec();
    params.push(RESULT.into());
    Some(Stmt::Subroutine { elemental: false, pure: false, name: format!("{TWIN}{name}"), params, body })
}

/// Give the defining functions among the procedures of `body` their twins,
/// recording their result types in `found`. `contained` says which functions
/// of `body` are module or internal procedures rather than external ones.
fn twins(body: &mut Vec<Stmt>, contained: &dyn Fn(&str) -> bool, found: &mut HashMap<String, TypeSpec>) {
    let mut added = vec![];
    for (i, s) in body.iter_mut().enumerate() {
        match s {
            Stmt::Function { name, params, body: inner, return_type, pure, elemental } => {
                let kind = return_type.clone().or_else(|| inner.iter().find_map(|s| match s {
                    Stmt::VarDecl { kind, names } if names.contains(name) => Some(kind.clone()),
                    _ => None,
                }));
                if let (true, false, Some(kind)) = (contained(name), *pure || *elemental, kind) {
                    if let Some(t) = twin(name, params, inner, &kind) {
                        found.insert(name.clone(), kind);
                        added.push((i + 1, t, name.clone()));
                    }
                }
                twins(inner, &|_| true, found);
            }
            Stmt::Subroutine { body: inner, .. } => twins(inner, &|_| true, found),
            Stmt::Module { body: inner, .. } => twins(inner, &|_| true, found),
            _ => {}
        }
    }
    for (at, t, name) in added.into_iter().rev() {
        body.insert(at, t);
        // The twin has the function's accessibility.
        for s in body.iter_mut() {
            if let Stmt::Accessibility { names, .. } = s {
                if names.contains(&name) {
                    names.push(format!("{TWIN}{name}"));
                }
            }
        }
    }
}

struct Calls<'a> {
    functions: &'a HashMap<String, TypeSpec>,
    serial: usize,
}

impl Calls<'_> {
    /// Replace references to defining functions in `e` by temporaries that
    /// calls of their twins in `prefix` set, declared in `declarations`.
    fn expr(&mut self, e: &mut Expr, prefix: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        visit_expr(e, &mut |e| {
            if let Expr::Call(n, args) = e {
                if let Some(kind) = self.functions.get(n.as_str()) {
                    self.serial += 1;
                    let temp = format!("rfort_defined_{}", self.serial);
                    declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![temp.clone()] });
                    let mut args = std::mem::take(args);
                    args.push(Expr::Ident(temp.clone()));
                    prefix.push(Stmt::CallSub { name: format!("{TWIN}{n}"), args });
                    *e = Expr::Ident(temp);
                }
            }
        });
    }

    fn statements(&mut self, body: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            let mut prefix = vec![];
            match &mut s {
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {
                    out.push(s);
                    continue;
                }
                // The condition is evaluated in the loop, before each iteration.
                Stmt::DoWhile { cond, body } => {
                    let mut evaluate = vec![];
                    self.expr(cond, &mut evaluate, declarations);
                    if !evaluate.is_empty() {
                        evaluate.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        evaluate.append(body);
                        *body = evaluate;
                        *cond = Expr::Logical(true);
                    }
                }
                // Calls include formatted output and WRITE.
                Stmt::Print { .. } | Stmt::Assign { .. } | Stmt::AssignIndex { .. } | Stmt::If { .. } | Stmt::Do { .. }
                | Stmt::Return(Some(_)) | Stmt::SelectCase { .. } | Stmt::CallSub { .. } => {
                    for e in expressions(&mut s) {
                        self.expr(e, &mut prefix, declarations);
                    }
                }
                _ => {}
            }
            for b in children(&mut s) {
                self.statements(b, declarations);
            }
            out.extend(prefix);
            out.push(s);
        }
        *body = out;
    }

    /// Rewrite the references in a program unit and in the units it contains.
    fn unit(&mut self, body: &mut Vec<Stmt>) {
        for s in body.iter_mut() {
            if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
                self.unit(body);
            }
        }
        // A local variable or array of the same name hides a function.
        let hidden = body.iter().any(|s| match s {
            Stmt::VarDecl { names, .. } => names.iter().any(|n| self.functions.contains_key(n)),
            Stmt::ArrayDecl { name, .. } => self.functions.contains_key(name),
            _ => false,
        });
        if hidden {
            return;
        }
        let mut declarations = vec![];
        self.statements(body, &mut declarations);
        body.splice(0..0, declarations);
    }
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut functions = HashMap::new();
    let contains = p.contains.clone();
    twins(&mut p.body, &|n: &str| contains.iter().any(|c| c == n), &mut functions);
    if functions.is_empty() {
        return Ok(p);
    }
    // A function that USE ... ONLY imports brings its twin.
    fn imports(body: &mut [Stmt], functions: &HashMap<String, TypeSpec>) {
        for s in body.iter_mut() {
            match s {
                Stmt::Use { only: Some(names), .. } => {
                    let twins: Vec<String> = names.iter().filter(|n| functions.contains_key(*n)).map(|n| format!("{TWIN}{n}")).collect();
                    names.extend(twins);
                }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => imports(body, functions),
                _ => {}
            }
        }
    }
    imports(&mut p.body, &functions);
    let mut calls = Calls { functions: &functions, serial: 0 };
    calls.unit(&mut p.body);
    if !p.contains.is_empty() {
        let twins: Vec<String> = p.contains.iter().filter(|n| functions.contains_key(*n)).map(|n| format!("{TWIN}{n}")).collect();
        p.contains.extend(twins);
    }
    Ok(p)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Give recursive numeric array functions a caller-owned result. Unlike
//! call-site inlining, the existing borrowed-array ABI supports recursive frames.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Function {
    params: Vec<String>,
    result: String,
    kind: TypeSpec,
    dims: Vec<Expr>,
    inputs: HashMap<String, TypeSpec>,
    arrays: HashMap<String, (TypeSpec, usize)>,
    calls: HashSet<String>,
}

fn collect(body: &[Stmt], functions: &mut HashMap<String, Function>) {
    for s in body {
        if let Stmt::Function {
            name, params, body, ..
        } = s
        {
            let result = match body.last() {
                Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                _ => name.clone(),
            };
            let array = body.iter().find_map(|s| match s {
                Stmt::ArrayDecl { name, kind, dims } if name == &result => Some((kind, dims)),
                _ => None,
            });
            if let Some((kind, dims)) = array.filter(|(k, _)| {
                matches!(
                    k,
                    TypeSpec::Integer(_)
                        | TypeSpec::Real
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                )
            }) {
                if !body.iter().any(|s| {
                    matches!(s, Stmt::DummyIntent { name, intent }
                    if name == &result && matches!(intent.as_str(), "allocatable" | "pointer"))
                }) {
                    let mut calls = HashSet::new();
                    fn scan(body: &[Stmt], result: &str, calls: &mut HashSet<String>) {
                        for mut s in body.to_vec() {
                            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                                continue;
                            }
                            for e in crate::procedure_arguments::expressions(&mut s) {
                                crate::procedure_arguments::visit_expr(e, &mut |e| {
                                    if let Expr::Call(n, _) = e {
                                        if n != result {
                                            calls.insert(n.clone());
                                        }
                                    }
                                });
                            }
                            for b in crate::procedure_arguments::children(&mut s) {
                                scan(b, result, calls);
                            }
                        }
                    }
                    scan(body, &result, &mut calls);
                    let inputs = body.iter().filter_map(|s| match s {
                        Stmt::VarDecl { kind, names } => Some(names.iter().filter(|n|
                            params.contains(n) && body.iter().any(|s| matches!(s,
                                Stmt::DummyIntent { name, intent } if name == *n && intent == "intent(in)")))
                            .map(|n| (n.clone(), kind.clone())).collect::<Vec<_>>()),
                        _ => None,
                    }).flatten().collect();
                    let arrays = body
                        .iter()
                        .filter_map(|s| match s {
                            Stmt::ArrayDecl { kind, name, dims } if params.contains(name) => {
                                Some((name.clone(), (kind.clone(), dims.len())))
                            }
                            _ => None,
                        })
                        .collect();
                    functions.insert(
                        name.clone(),
                        Function {
                            params: params.clone(),
                            result,
                            kind: kind.clone(),
                            dims: dims.clone(),
                            inputs,
                            arrays,
                            calls,
                        },
                    );
                }
            }
        }
        let mut s = s.clone();
        for b in crate::procedure_arguments::children(&mut s) {
            collect(b, functions);
        }
    }
}

fn reaches(
    start: &str,
    name: &str,
    functions: &HashMap<String, Function>,
    seen: &mut HashSet<String>,
) -> bool {
    if !seen.insert(name.into()) {
        return false;
    }
    functions.get(name).is_some_and(|f| {
        f.calls
            .iter()
            .any(|n| n == start || reaches(start, n, functions, seen))
    })
}

struct Lower {
    functions: HashMap<String, Function>,
    serial: usize,
}
impl Lower {
    fn expression(
        &mut self,
        e: &mut Expr,
        before: &mut Vec<Stmt>,
        after: &mut Vec<Stmt>,
    ) -> Result<()> {
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for a in args {
                    self.expression(a, before, after)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, before, after)?;
                self.expression(b, before, after)?;
            }
            Expr::Un(_, a) => self.expression(a, before, after)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expression(a, before, after)?;
                }
            }
            _ => {}
        }
        let Expr::Call(name, args) = e else {
            return Ok(());
        };
        let Some(function) = self.functions.get(name).cloned() else {
            return Ok(());
        };
        if args.len() != function.params.len() {
            bail!("wrong argument count in recursive function `{name}`");
        }
        self.serial += 1;
        let result = format!("@recursive_result_{}", self.serial);
        // Bound expressions and the call must see the same scalar values,
        // including when an actual argument is a function with side effects.
        for (param, arg) in function.params.iter().zip(args.iter_mut()) {
            if let Some((kind, rank)) = function.arrays.get(param) {
                if !matches!(arg, Expr::Ident(_)) {
                    let temp = format!("{result}${param}");
                    before.push(Stmt::ArrayDecl {
                        kind: kind.clone(),
                        name: temp.clone(),
                        dims: vec![Expr::Ident("@deferred_extent".into()); *rank],
                    });
                    before.push(Stmt::DummyIntent {
                        name: temp.clone(),
                        intent: "allocatable".into(),
                    });
                    before.push(Stmt::Assign {
                        name: temp.clone(),
                        value: arg.clone(),
                    });
                    after.insert(
                        0,
                        Stmt::CallSub {
                            name: "@heap_drop".into(),
                            args: vec![Expr::Ident(temp.clone())],
                        },
                    );
                    *arg = Expr::Ident(temp);
                }
            }
            if let Some(kind) = function.inputs.get(param) {
                let temp = format!("{result}${param}");
                before.push(Stmt::VarDecl {
                    kind: kind.clone(),
                    names: vec![temp.clone()],
                });
                before.push(Stmt::Assign {
                    name: temp.clone(),
                    value: arg.clone(),
                });
                *arg = Expr::Ident(temp);
            }
        }
        let bindings: HashMap<_, _> = function
            .params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let mut bounds = function.dims.clone();
        for bound in &mut bounds {
            crate::procedure_arguments::visit_expr(bound, &mut |e| {
                if let Expr::Ident(n) = e {
                    if let Some(actual) = bindings.get(n) {
                        *e = actual.clone();
                    }
                }
            });
            if let Expr::Call(tag, _) = bound {
                if tag == "@declared_bounds" {
                    *tag = "@heap_bounds".into();
                }
            }
        }
        before.push(Stmt::ArrayDecl {
            kind: function.kind,
            name: result.clone(),
            dims: vec![Expr::Ident("@deferred_extent".into()); bounds.len()],
        });
        before.push(Stmt::DummyIntent {
            name: result.clone(),
            intent: "allocatable".into(),
        });
        before.push(Stmt::CallSub {
            name: "@allocate".into(),
            args: vec![Expr::Call(result.clone(), bounds)],
        });
        let mut actuals = args.clone();
        actuals.push(Expr::Ident(result.clone()));
        before.push(Stmt::CallSub {
            name: name.clone(),
            args: actuals,
        });
        // This is an internal expression buffer, not a source DEALLOCATE:
        // release its data and mask without undefining the freed storage.
        after.insert(
            0,
            Stmt::CallSub {
                name: "@heap_drop".into(),
                args: vec![Expr::Ident(result.clone())],
            },
        );
        *e = Expr::Ident(result);
        Ok(())
    }

    fn body(&mut self, body: &mut Vec<Stmt>) -> Result<()> {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::Function {
                name,
                params,
                body,
                pure,
                ..
            } = &mut s
            {
                if let Some(f) = self.functions.get(name) {
                    fn returns(body: &mut [Stmt]) {
                        for s in body {
                            if let Stmt::Return(value) = s {
                                *value = None;
                            }
                            for b in crate::procedure_arguments::children(s) {
                                returns(b);
                            }
                        }
                    }
                    returns(body);
                    body.push(Stmt::DummyIntent {
                        name: f.result.clone(),
                        intent: "intent(out)".into(),
                    });
                    let mut params = params.clone();
                    params.push(f.result.clone());
                    s = Stmt::Subroutine {
                        name: name.clone(),
                        params,
                        body: body.clone(),
                        pure: *pure,
                        elemental: false,
                    };
                }
            }
            for b in crate::procedure_arguments::children(&mut s) {
                self.body(b)?;
            }
            let mut before = vec![];
            let mut after = vec![];
            for e in crate::procedure_arguments::expressions(&mut s) {
                self.expression(e, &mut before, &mut after)?;
            }
            if let Stmt::DoWhile { cond, body } = &mut s {
                if !before.is_empty() {
                    // Reevaluate calls in the condition on every iteration.
                    let mut iteration = before;
                    iteration.push(Stmt::If {
                        cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())),
                        then_body: after.iter().cloned().chain([Stmt::Exit]).collect(),
                        else_body: None,
                    });
                    iteration.extend(after);
                    iteration.append(body);
                    out.push(Stmt::DoWhile {
                        cond: Expr::Logical(true),
                        body: iteration,
                    });
                    continue;
                }
            }
            out.extend(before);
            out.push(s);
            out.extend(after);
        }
        *body = out;
        Ok(())
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut functions = HashMap::new();
    collect(&program.body, &mut functions);
    let selected: HashSet<_> = functions
        .keys()
        .filter(|n| reaches(n, n, &functions, &mut HashSet::new()))
        .cloned()
        .collect();
    functions.retain(|n, _| selected.contains(n));
    if !functions.is_empty() {
        Lower {
            functions,
            serial: 0,
        }
        .body(&mut program.body)?;
    }
    Ok(program)
}

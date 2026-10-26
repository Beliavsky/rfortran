// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Scalar function results use caller-owned ALLOCATABLE storage, passed through
//! the same hidden output argument machinery as scalar ALLOCATABLE dummies.
//! This runs after generic resolution, while source ranks are still available.
use crate::ast::{Expr, Program, Stmt, TypeSpec, UnOp};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{ensure, Result};
use std::collections::HashMap;

fn result(name: &str, body: &[Stmt]) -> String {
    match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
        _ => name.into(),
    }
}

fn at_result(error: anyhow::Error, body: &[Stmt], result: &str) -> anyhow::Error {
    let mut offset = None;
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
        }
        if matches!(s, Stmt::VarDecl { names, .. } if names.iter().any(|n| n == result)) {
            break;
        }
    }
    crate::source_diagnostics::at(error, offset)
}

fn returns(body: &mut [Stmt]) {
    for s in body {
        match s {
            Stmt::Function { .. } | Stmt::Subroutine { .. } => continue,
            Stmt::Return(_) => *s = Stmt::Return(None),
            _ => {}
        }
        for b in children(s) {
            returns(b);
        }
    }
}

fn collect(body: &mut [Stmt], found: &mut HashMap<String, (TypeSpec, usize)>) -> Result<()> {
    for s in body {
        if let Stmt::Function {
            name,
            params,
            body,
            pure,
            elemental,
            ..
        } = s
        {
            let output = result(name, body);
            let allocated = body.iter().any(|s| {
                matches!(s, Stmt::DummyIntent { name, intent }
                if name == &output && intent == "allocatable")
            });
            let array = body
                .iter()
                .any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == &output));
            let kind = body.iter().find_map(|s| match s {
                Stmt::VarDecl { kind, names } if names.contains(&output) => Some(kind.clone()),
                _ => None,
            });
            if let (true, false, Some(kind)) = (allocated, array, kind) {
                ensure!(
                    !*elemental,
                    "ELEMENTAL functions cannot have an ALLOCATABLE result"
                );
                // Deferred strings already have a length-aware result lowering.
                if crate::substrings::deferred(&kind) {
                    for b in children(s) {
                        collect(b, found)?;
                    }
                    continue;
                }
                let mut recursive = false;
                fn calls(body: &mut [Stmt], name: &str, recursive: &mut bool) {
                    for s in body {
                        for e in expressions(s) {
                            visit_expr(e, &mut |e| {
                                *recursive |= matches!(e, Expr::Call(n, _) if n == name)
                            });
                        }
                        for b in children(s) {
                            calls(b, name, recursive);
                        }
                    }
                }
                calls(body, name, &mut recursive);
                if recursive {
                    return Err(at_result(
                        anyhow::anyhow!(
                            "recursive ALLOCATABLE scalar function results are not yet supported"
                        ),
                        body,
                        &output,
                    ));
                }
                let constants = body
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::InitializedDecl {
                            attributes,
                            entities,
                            ..
                        } if attributes.iter().any(|a| a == "parameter") => Some(entities),
                        _ => None,
                    })
                    .flatten()
                    .filter_map(|(n, e)| e.as_ref().map(|e| (n.clone(), e.clone())))
                    .collect();
                let kind = crate::parameters::concrete_kind_with(kind, &constants)
                    .map_err(|e| at_result(e, body, &output))?;
                found.insert(name.clone(), (kind, params.len()));
                params.push(output.clone());
                body.insert(
                    0,
                    Stmt::DummyIntent {
                        name: output,
                        intent: "intent(out)".into(),
                    },
                );
                returns(body);
                *s = Stmt::Subroutine {
                    name: name.clone(),
                    params: params.clone(),
                    body: std::mem::take(body),
                    pure: *pure,
                    elemental: false,
                };
            }
        }
        for b in children(s) {
            collect(b, found)?;
        }
    }
    Ok(())
}

struct Calls {
    functions: HashMap<String, (TypeSpec, usize)>,
    serial: usize,
}

fn fold_inquiries(
    body: &mut [Stmt],
    inherited: &HashMap<String, (TypeSpec, usize)>,
    types: &crate::operators::ExpressionTypes,
    functions: &HashMap<String, (TypeSpec, usize)>,
    procedures: &HashMap<String, Vec<usize>>,
) -> Result<()> {
    let mut symbols = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    symbols.insert(n.clone(), (kind.clone(), 0));
                }
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    symbols.insert(n.clone(), (kind.clone(), 0));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                symbols.insert(name.clone(), (kind.clone(), dims.len()));
            }
            _ => {}
        }
    }
    fn expr(
        e: &mut Expr,
        symbols: &HashMap<String, (TypeSpec, usize)>,
        types: &crate::operators::ExpressionTypes,
        functions: &HashMap<String, (TypeSpec, usize)>,
        procedures: &HashMap<String, Vec<usize>>,
    ) -> Result<()> {
        if let Expr::Call(name, args) = e {
            if args.len() == 1
                && !procedures.contains_key(name)
                && !symbols.contains_key(name)
                && (matches!(name.as_str(), "kind" | "len" | "huge" | "tiny")
                    || crate::inquiries::numeric_model(name))
            {
                let mut argument = args[0].clone();
                let mut declarations = symbols.clone();
                let mut found = false;
                // Substitute types, not values: an inquiry must not invoke the
                // function or demand allocation/initialization of its result.
                visit_expr(&mut argument, &mut |e| {
                    if let Expr::Call(n, _) = e {
                        if let Some((kind, _)) = functions.get(n) {
                            let symbol = format!("@inquiry_result_type:{n}");
                            declarations.insert(symbol.clone(), (kind.clone(), 0));
                            *e = Expr::Ident(symbol);
                            found = true;
                        }
                    }
                });
                if found {
                    if name == "len" {
                        fn length(
                            e: &Expr,
                            symbols: &HashMap<String, (TypeSpec, usize)>,
                            functions: &HashMap<String, (TypeSpec, usize)>,
                        ) -> Option<usize> {
                            let kind = match e {
                                Expr::Call(n, _) => &functions.get(n)?.0,
                                Expr::Ident(n) => &symbols.get(n)?.0,
                                Expr::Str(s) => return Some(s.len()),
                                Expr::Bin(crate::ast::BinOp::Concat, a, b) => {
                                    return length(a, symbols, functions)?
                                        .checked_add(length(b, symbols, functions)?)
                                }
                                _ => return None,
                            };
                            if let TypeSpec::Character(n) = kind {
                                Some(n.unwrap_or(1))
                            } else {
                                None
                            }
                        }
                        // Generic type inference intentionally erases string
                        // lengths; LEN needs the original result declaration.
                        if let Some(n) = length(&args[0], symbols, functions) {
                            *e = Expr::IntLit(n.to_string());
                            return Ok(());
                        }
                    }
                    if let Some((kind, _)) = types.kind_rank(&argument, &declarations)? {
                        let folded = match name.as_str() {
                            "kind" => {
                                let n = match &kind {
                                    TypeSpec::Integer(n) => Some(n.unwrap_or(4) as usize),
                                    TypeSpec::Real | TypeSpec::Logical => Some(4),
                                    TypeSpec::DoublePrecision => Some(8),
                                    TypeSpec::Complex(n) => n.parse().ok(),
                                    TypeSpec::Character(_) => Some(1),
                                    _ => None,
                                };
                                n.map(|n| Expr::IntLit(n.to_string()))
                            }
                            "len" => None,
                            _ => Some(crate::inquiries::model_value(name, &kind)?),
                        };
                        if let Some(value) = folded {
                            *e = value;
                            return Ok(());
                        }
                    }
                }
            }
        }
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for e in args {
                    expr(e, symbols, types, functions, procedures)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, symbols, types, functions, procedures)?;
                expr(b, symbols, types, functions, procedures)?;
            }
            Expr::Un(_, e) => expr(e, symbols, types, functions, procedures)?,
            _ => {}
        }
        Ok(())
    }
    let mut offset = None;
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
        }
        for e in expressions(s) {
            expr(e, &symbols, types, functions, procedures)
                .map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        for b in children(s) {
            fold_inquiries(b, &symbols, types, functions, procedures)?;
        }
    }
    Ok(())
}
impl Calls {
    fn expr(&mut self, e: &mut Expr, prefix: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        visit_expr(e, &mut |e| {
            if let Expr::Call(name, args) = e {
                if let Some((kind, count)) = self.functions.get(name) {
                    self.serial += 1;
                    let temp = format!("@scalar_result_{}", self.serial);
                    declarations.extend([
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![temp.clone()],
                        },
                        Stmt::DummyIntent {
                            name: temp.clone(),
                            intent: "allocatable".into(),
                        },
                    ]);
                    let mut actuals = std::mem::take(args);
                    actuals.resize_with(*count, || {
                        Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
                    });
                    actuals.push(Expr::Ident(temp.clone()));
                    prefix.push(Stmt::CallSub {
                        name: name.clone(),
                        args: actuals,
                    });
                    prefix.push(Stmt::If {
                        cond: Expr::Un(
                            UnOp::Not,
                            Box::new(Expr::Call(
                                "allocated".into(),
                                vec![Expr::Ident(temp.clone())],
                            )),
                        ),
                        then_body: vec![Stmt::CallSub {
                            name: "@error_stop".into(),
                            args: vec![Expr::Str(format!(
                                "unallocated result of function `{name}`"
                            ))],
                        }],
                        else_body: None,
                    });
                    *e = Expr::Ident(temp);
                }
            }
        });
    }
    fn body(&mut self, body: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if matches!(
                s,
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }
            ) {
                for b in children(&mut s) {
                    self.unit(b);
                }
                out.push(s);
                continue;
            }
            let mut prefix = vec![];
            if let Stmt::DoWhile { cond, body } = &mut s {
                // Reevaluate the result on every loop header, including CYCLE.
                self.expr(cond, &mut prefix, declarations);
                if !prefix.is_empty() {
                    prefix.push(Stmt::If {
                        cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                        then_body: vec![Stmt::Exit],
                        else_body: None,
                    });
                    prefix.append(body);
                    *body = std::mem::take(&mut prefix);
                    *cond = Expr::Logical(true);
                }
            } else {
                for e in expressions(&mut s) {
                    self.expr(e, &mut prefix, declarations);
                }
            }
            for b in children(&mut s) {
                self.body(b, declarations);
            }
            out.extend(prefix);
            out.push(s);
        }
        *body = out;
    }
    fn unit(&mut self, body: &mut Vec<Stmt>) {
        let mut declarations = vec![];
        self.body(body, &mut declarations);
        body.splice(0..0, declarations);
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    fn needed(body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::Function { name, body, .. } => {
                let output = result(name, body);
                (body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == &output && intent == "allocatable"))
                    && !body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == &output))
                    && body.iter().any(|s| matches!(s, Stmt::VarDecl { kind, names } if names.contains(&output) && !crate::substrings::deferred(kind)))) || needed(body)
            }
            Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => needed(body),
            _ => false,
        })
    }
    if !needed(&program.body) {
        return Ok(program);
    }
    // Validate the original PURE function, whose original dummies must be
    // read-only, before adding a writable hidden result to a subroutine.
    crate::purity::validate(&program)?;
    let mut variables: HashMap<String, Vec<usize>> = HashMap::from([
        ("allocated".into(), vec![0]),
        ("move_alloc".into(), vec![0, 1]),
        ("random_number".into(), vec![0]),
    ]);
    fn variable_arguments(body: &mut [Stmt], variables: &mut HashMap<String, Vec<usize>>) {
        for s in body {
            if let Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } = s
            {
                variables.insert(name.clone(), params.iter().enumerate().filter_map(|(i,p)| {
                    body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == p &&
                        matches!(intent.as_str(), "allocatable" | "pointer" | "intent(out)" | "intent(inout)"))).then_some(i)
                }).collect());
            }
            for b in children(s) {
                variable_arguments(b, variables);
            }
        }
    }
    variable_arguments(&mut program.body, &mut variables);
    let types = crate::operators::ExpressionTypes::new(&program)?;
    let mut functions = HashMap::new();
    collect(&mut program.body, &mut functions)?;
    if !functions.is_empty() {
        // Materializing an expression must not make it a legal variable actual
        // for ALLOCATED, an ALLOCATABLE dummy, or a writable dummy.
        fn validate(
            body: &mut [Stmt],
            functions: &HashMap<String, (TypeSpec, usize)>,
            variables: &HashMap<String, Vec<usize>>,
        ) -> Result<()> {
            let check = |name: &str, args: &[Expr]| -> Result<()> {
                if let Some(positions) = variables.get(name) {
                    for &i in positions {
                        if let Some(arg) = args.get(i) {
                            let arg = crate::named_arguments::keyword(arg).map_or(arg, |(_, e)| e);
                            ensure!(!matches!(arg, Expr::Call(n, _) if functions.contains_key(n)),
                                "argument {} of `{name}` requires a variable, not a function result", i+1);
                        }
                    }
                }
                Ok(())
            };
            let mut offset = None;
            for s in body {
                if let Stmt::SourceLocation(p) = s {
                    offset = Some(*p);
                }
                let mut checked = if let Stmt::CallSub { name, args } = s {
                    check(name, args)
                } else {
                    Ok(())
                };
                for e in expressions(s) {
                    visit_expr(e, &mut |e| {
                        if checked.is_ok() {
                            if let Expr::Call(name, args) = e {
                                checked = check(name, args);
                            }
                        }
                    });
                }
                checked.map_err(|e| crate::source_diagnostics::at(e, offset))?;
                for b in children(s) {
                    validate(b, functions, variables)?;
                }
            }
            Ok(())
        }
        validate(&mut program.body, &functions, &variables)?;
        fold_inquiries(
            &mut program.body,
            &HashMap::new(),
            &types,
            &functions,
            &variables,
        )?;
        Calls {
            functions,
            serial: 0,
        }
        .unit(&mut program.body);
    }
    Ok(program)
}

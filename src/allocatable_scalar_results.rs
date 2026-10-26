// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Scalar function results use caller-owned ALLOCATABLE storage, passed through
//! the same hidden output argument machinery as scalar ALLOCATABLE dummies.
//! This runs after generic resolution, while source ranks are still available.
use crate::ast::{Expr, Program, Stmt, TypeSpec, UnOp};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{ensure, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct RuntimeCharacter {
    query: String,
}

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

// Recursive calls transport the scalar value through a nonallocatable
// one-element result buffer. Allocation state stays local to each invocation;
// the existing recursive array ABI borrows the caller's checked value buffer.
fn recursive_returns(
    body: &mut [Stmt],
    result: &str,
    output: &str,
    function: &str,
    character: Option<&Expr>,
    length_output: Option<&str>,
) {
    for s in body {
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
            continue;
        }
        if matches!(s, Stmt::Return(_)) {
            let mut copy = vec![Stmt::If {
                cond: Expr::Un(
                    UnOp::Not,
                    Box::new(Expr::Call(
                        "allocated".into(),
                        vec![Expr::Ident(result.into())],
                    )),
                ),
                then_body: vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(format!(
                        "unallocated result of function `{function}`"
                    ))],
                }],
                else_body: None,
            }];
            if let Some(field) = length_output {
                copy.push(Stmt::Assign {
                    name: field.into(),
                    value: character.unwrap().clone(),
                });
            }
            if let Some(length) = character {
                let index = format!("@recursive_character_index${output}");
                copy.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![index.clone()],
                });
                copy.push(Stmt::Do {
                    var: index.clone(),
                    start: Expr::IntLit("1".into()),
                    end: length.clone(),
                    body: vec![Stmt::AssignIndex {
                        name: output.into(),
                        indices: vec![Expr::Ident(index.clone())],
                        value: Expr::Call(
                            "iachar".into(),
                            vec![Expr::Section {
                                name: result.into(),
                                lower: Some(Box::new(Expr::Ident(index.clone()))),
                                upper: Some(Box::new(Expr::Ident(index))),
                                stride: None,
                            }],
                        ),
                    }],
                });
            } else {
                copy.push(Stmt::AssignIndex {
                    name: output.into(),
                    indices: vec![Expr::IntLit("1".into())],
                    value: Expr::Ident(result.into()),
                });
            }
            copy.push(Stmt::Return(None));
            *s = Stmt::Block { body: copy };
            continue;
        }
        for b in children(s) {
            recursive_returns(b, result, output, function, character, length_output);
        }
    }
}
fn recursive_procedures(body: &mut [Stmt]) -> HashSet<String> {
    fn graph(body: &mut [Stmt], edges: &mut HashMap<String, HashSet<String>>) {
        fn calls(body: &mut [Stmt], names: &mut HashSet<String>) {
            for s in body {
                if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                    continue;
                }
                if let Stmt::CallSub { name, .. } = s {
                    names.insert(name.clone());
                }
                for e in expressions(s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(n, _) = e {
                            names.insert(n.clone());
                        }
                    });
                }
                for b in children(s) {
                    calls(b, names);
                }
            }
        }
        for s in body {
            if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
                let mut names = HashSet::new();
                calls(body, &mut names);
                edges.insert(name.clone(), names);
            }
            for b in children(s) {
                graph(b, edges);
            }
        }
    }
    fn reaches(
        from: &str,
        target: &str,
        edges: &HashMap<String, HashSet<String>>,
        seen: &mut HashSet<String>,
    ) -> bool {
        seen.insert(from.into())
            && edges.get(from).is_some_and(|next| {
                next.iter()
                    .any(|n| n == target || reaches(n, target, edges, seen))
            })
    }
    let mut edges = HashMap::new();
    graph(body, &mut edges);
    edges
        .keys()
        .filter(|n| reaches(n, n, &edges, &mut HashSet::new()))
        .cloned()
        .collect()
}
fn collect(
    body: &mut [Stmt],
    found: &mut HashMap<String, (TypeSpec, usize)>,
    characters: &mut HashMap<String, RuntimeCharacter>,
    queries: &mut Vec<Stmt>,
    cycles: &HashSet<String>,
    recursive_functions: &mut HashSet<String>,
    recursive_characters: &mut HashMap<String, usize>,
) -> Result<()> {
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
                        collect(
                            b,
                            found,
                            characters,
                            queries,
                            cycles,
                            recursive_functions,
                            recursive_characters,
                        )?;
                    }
                    continue;
                }
                let recursive = cycles.contains(name);
                if recursive && output == *name {
                    fn direct(body: &mut [Stmt], name: &str) -> bool {
                        let mut found = false;
                        for s in body {
                            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                                continue;
                            }
                            for e in expressions(s) {
                                visit_expr(e, &mut |e| {
                                    found |= matches!(e,Expr::Call(n,_) if n==name)
                                });
                            }
                            for b in children(s) {
                                found |= direct(b, name);
                            }
                        }
                        found
                    }
                    if direct(body, name) {
                        return Err(at_result(
                            anyhow::anyhow!(
                                "direct recursion requires an explicit RESULT variable"
                            ),
                            body,
                            &output,
                        ));
                    }
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
                let kind = match crate::parameters::concrete_kind_with(kind.clone(), &constants) {
                    Ok(kind) => kind,
                    Err(error) => {
                        let TypeSpec::CharacterLength(mut length) = kind else {
                            return Err(at_result(error, body, &output));
                        };
                        visit_expr(&mut length, &mut |e| {
                            if let Expr::Ident(n) = e {
                                if let Some(value) = constants.get(n) {
                                    *e = value.clone();
                                }
                            }
                        });
                        let field = format!("@scalar_result_length${name}");
                        let query = format!("@scalar_result_length_query${name}");
                        let mut declarations = vec![];
                        for s in body.iter() {
                            match s {
                                Stmt::VarDecl { kind, names } => {
                                    let names = names
                                        .iter()
                                        .filter(|n| params.contains(n))
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    if !names.is_empty() {
                                        declarations.push(Stmt::VarDecl {
                                            kind: kind.clone(),
                                            names,
                                        });
                                    }
                                }
                                Stmt::ArrayDecl { name, .. } if params.contains(name) => {
                                    declarations.push(s.clone())
                                }
                                Stmt::DummyIntent { name, intent }
                                    if params.contains(name) && !intent.starts_with("intent(") =>
                                {
                                    declarations.push(s.clone())
                                }
                                Stmt::InitializedDecl { attributes, .. }
                                    if attributes.iter().any(|a| a == "parameter") =>
                                {
                                    declarations.push(s.clone())
                                }
                                _ => {}
                            }
                        }
                        for param in params.iter() {
                            declarations.push(Stmt::DummyIntent {
                                name: param.clone(),
                                intent: "intent(in)".into(),
                            });
                        }
                        declarations.push(Stmt::Return(Some(Expr::Call(
                            "max".into(),
                            vec![Expr::IntLit("0".into()), (*length).clone()],
                        ))));
                        queries.push(Stmt::Function {
                            name: query.clone(),
                            params: params.clone(),
                            body: declarations,
                            return_type: Some(TypeSpec::Integer(None)),
                            pure: true,
                            elemental: false,
                        });
                        characters.insert(name.clone(), RuntimeCharacter { query });
                        for s in body.iter_mut() {
                            if let Stmt::VarDecl { kind, names } = s {
                                if names.contains(&output) {
                                    *kind = TypeSpec::Character(Some(
                                        crate::substrings::FALLBACK_CAPACITY,
                                    ));
                                }
                            }
                        }
                        body.splice(
                            0..0,
                            [
                                Stmt::VarDecl {
                                    kind: TypeSpec::Integer(Some(8)),
                                    names: vec![field.clone()],
                                },
                                Stmt::DummyIntent {
                                    name: field.clone(),
                                    intent: "intent(out)".into(),
                                },
                                Stmt::DummyIntent {
                                    name: output.clone(),
                                    intent: format!("@character_spec_length:{field}"),
                                },
                            ],
                        );
                        let entry = body
                            .iter()
                            .position(|s| {
                                matches!(
                                    s,
                                    Stmt::Assign { .. }
                                        | Stmt::AssignIndex { .. }
                                        | Stmt::CallSub { .. }
                                        | Stmt::Print { .. }
                                        | Stmt::Read { .. }
                                        | Stmt::If { .. }
                                        | Stmt::Do { .. }
                                        | Stmt::DoWhile { .. }
                                        | Stmt::Block { .. }
                                        | Stmt::Return(_)
                                )
                            })
                            .unwrap_or(body.len());
                        body.splice(entry..entry, [
                            Stmt::Assign {name:field.clone(),value:Expr::Call("max".into(),vec![Expr::IntLit("0".into()),*length])},
                            Stmt::If {cond:Expr::Bin(crate::ast::BinOp::Gt,Box::new(Expr::Ident(field.clone())),Box::new(Expr::IntLit(crate::substrings::FALLBACK_CAPACITY.to_string()))),
                                then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("CHARACTER result of `{name}` exceeds its {}-character expression buffer",crate::substrings::FALLBACK_CAPACITY))]}],else_body:None},
                        ]);
                        TypeSpec::Character(Some(crate::substrings::FALLBACK_CAPACITY))
                    }
                };
                found.insert(name.clone(), (kind.clone(), params.len()));
                if recursive {
                    let character = if let TypeSpec::Character(capacity) = &kind {
                        Some(capacity.unwrap_or(1))
                    } else {
                        None
                    };
                    if let Some(capacity) = character {
                        recursive_characters.insert(name.clone(), capacity);
                    }
                    recursive_functions.insert(name.clone());
                    let buffer = format!("@recursive_scalar_result${name}");
                    // Also normalize an implicit final return in functions
                    // which use their name rather than a RESULT clause.
                    if !matches!(body.last(), Some(Stmt::Return(_))) {
                        body.push(Stmt::Return(None));
                    }
                    let length = character.map(|capacity| {
                        if characters.contains_key(name) {
                            Expr::Ident(format!("@scalar_result_length${name}"))
                        } else {
                            Expr::IntLit(capacity.to_string())
                        }
                    });
                    let length_output = characters
                        .contains_key(name)
                        .then(|| format!("@scalar_result_length${name}_out"));
                    if let Some(out) = &length_output {
                        let field = format!("@scalar_result_length${name}");
                        body.retain(|s|!matches!(s,Stmt::DummyIntent{name,intent} if name==&field && intent=="intent(out)"));
                        body.splice(
                            0..0,
                            [
                                Stmt::VarDecl {
                                    kind: TypeSpec::Integer(Some(8)),
                                    names: vec![out.clone()],
                                },
                                Stmt::DummyIntent {
                                    name: out.clone(),
                                    intent: "intent(out)".into(),
                                },
                            ],
                        );
                    }
                    recursive_returns(
                        body,
                        &output,
                        &buffer,
                        name,
                        length.as_ref(),
                        length_output.as_deref(),
                    );
                    body.splice(
                        0..0,
                        [
                            Stmt::ArrayDecl {
                                kind: if character.is_some() {
                                    TypeSpec::Integer(Some(8))
                                } else {
                                    kind.clone()
                                },
                                name: buffer.clone(),
                                dims: vec![Expr::IntLit(character.unwrap_or(1).max(1).to_string())],
                            },
                            Stmt::DummyIntent {
                                name: buffer.clone(),
                                intent: "intent(out)".into(),
                            },
                            Stmt::DummyIntent {
                                name: buffer.clone(),
                                intent: "@scalar_result_value_dummy".into(),
                            },
                        ],
                    );
                    if character.is_some() {
                        body.retain(|s|!matches!(s,Stmt::DummyIntent{name,intent} if name==&buffer && intent=="@scalar_result_value_dummy"));
                    }
                    params.push(buffer);
                    if let Some(field) = length_output {
                        params.push(field);
                    }
                    *s = Stmt::Subroutine {
                        name: name.clone(),
                        params: params.clone(),
                        body: std::mem::take(body),
                        pure: *pure,
                        elemental: false,
                    };
                    for b in children(s) {
                        collect(
                            b,
                            found,
                            characters,
                            queries,
                            cycles,
                            recursive_functions,
                            recursive_characters,
                        )?;
                    }
                    continue;
                }
                params.push(output.clone());
                if characters.contains_key(name) {
                    params.push(format!("@scalar_result_length${name}"));
                }
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
            collect(
                b,
                found,
                characters,
                queries,
                cycles,
                recursive_functions,
                recursive_characters,
            )?;
        }
    }
    Ok(())
}

struct Calls {
    functions: HashMap<String, (TypeSpec, usize)>,
    serial: usize,
    characters: HashMap<String, RuntimeCharacter>,
    recursive_functions: HashSet<String>,
    recursive_characters: HashMap<String, usize>,
}

fn fold_inquiries(
    body: &mut [Stmt],
    inherited: &HashMap<String, (TypeSpec, usize)>,
    types: &crate::operators::ExpressionTypes,
    functions: &HashMap<String, (TypeSpec, usize)>,
    procedures: &HashMap<String, Vec<usize>>,
    characters: &HashMap<String, RuntimeCharacter>,
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
        characters: &HashMap<String, RuntimeCharacter>,
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
                        if let Expr::Call(function, actuals) = &args[0] {
                            if let Some(spec) = characters.get(function) {
                                *e = Expr::Call(spec.query.clone(), actuals.clone());
                                return Ok(());
                            }
                        }
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
                    expr(e, symbols, types, functions, procedures, characters)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, symbols, types, functions, procedures, characters)?;
                expr(b, symbols, types, functions, procedures, characters)?;
            }
            Expr::Un(_, e) => expr(e, symbols, types, functions, procedures, characters)?,
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
            expr(e, &symbols, types, functions, procedures, characters)
                .map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        for b in children(s) {
            fold_inquiries(b, &symbols, types, functions, procedures, characters)?;
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
                    if self.characters.contains_key(name) {
                        let length = format!("{temp}_length");
                        declarations.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![length.clone()],
                        });
                        declarations.push(Stmt::DummyIntent {
                            name: temp.clone(),
                            intent: format!("@character_spec_length:{length}"),
                        });
                        actuals.push(Expr::Ident(length));
                    }
                    if let Some(capacity) = self.recursive_characters.get(name) {
                        let codes = format!("{temp}_codes");
                        declarations.push(Stmt::ArrayDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            name: codes.clone(),
                            dims: vec![Expr::IntLit(capacity.max(&1).to_string())],
                        });
                        actuals[*count] = Expr::Ident(codes);
                    }
                    if self.recursive_functions.contains(name)
                        && !self.recursive_characters.contains_key(name)
                    {
                        prefix.push(Stmt::If {
                            cond: Expr::Call("allocated".into(), vec![Expr::Ident(temp.clone())]),
                            then_body: vec![Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![Expr::Ident(temp.clone())],
                            }],
                            else_body: None,
                        });
                        prefix.push(Stmt::CallSub {
                            name: "@allocate".into(),
                            args: vec![Expr::Ident(temp.clone())],
                        });
                    }
                    prefix.push(Stmt::CallSub {
                        name: name.clone(),
                        args: actuals,
                    });
                    if let Some(capacity) = self.recursive_characters.get(name) {
                        prefix.push(Stmt::If {
                            cond: Expr::Call("allocated".into(), vec![Expr::Ident(temp.clone())]),
                            then_body: vec![Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![Expr::Ident(temp.clone())],
                            }],
                            else_body: None,
                        });
                        prefix.push(Stmt::CallSub {
                            name: "@allocate".into(),
                            args: vec![Expr::Ident(temp.clone())],
                        });
                        let length = if self.characters.contains_key(name) {
                            Expr::Ident(format!("{temp}_length"))
                        } else {
                            Expr::IntLit(capacity.to_string())
                        };
                        prefix.push(Stmt::CallSub {
                            name: "@scalar_character_from_codes".into(),
                            args: vec![
                                Expr::Ident(temp.clone()),
                                Expr::Ident(format!("{temp}_codes")),
                                length,
                            ],
                        });
                    }
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
    let mut characters = HashMap::new();
    let mut queries = vec![];
    let cycles = recursive_procedures(&mut program.body);
    let mut recursive_functions = HashSet::new();
    let mut recursive_characters = HashMap::new();
    collect(
        &mut program.body,
        &mut functions,
        &mut characters,
        &mut queries,
        &cycles,
        &mut recursive_functions,
        &mut recursive_characters,
    )?;
    for query in &queries {
        if let Stmt::Function { name, .. } = query {
            program.contains.push(name.clone());
        }
    }
    program.body.extend(queries);
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
            &characters,
        )?;
        Calls {
            functions,
            serial: 0,
            characters,
            recursive_functions,
            recursive_characters,
        }
        .unit(&mut program.body);
    }
    Ok(program)
}

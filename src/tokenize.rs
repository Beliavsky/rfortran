// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Fortran 2023 TOKENIZE, lowered to shared delimiter search and array storage.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::children;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
const CAPACITY: usize = 4096;
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn call(n: &str, a: Vec<Expr>) -> Stmt {
    Stmt::CallSub {
        name: n.into(),
        args: a,
    }
}
fn expr(n: &str, a: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), a)
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn width(n: &str) -> String {
    crate::deferred_arrays::width(n)
}
#[derive(Clone)]
struct Variable {
    kind: TypeSpec,
    rank: usize,
    allocatable: bool,
    readonly: bool,
    deferred: bool,
}
fn lower(
    args: &mut Vec<Expr>,
    vars: &HashMap<String, Variable>,
    types: &crate::operators::ExpressionTypes,
    serial: &mut usize,
) -> Result<Vec<Stmt>> {
    let candidate = args.get(2).or_else(|| {
        args.iter().find_map(|e| {
            crate::named_arguments::keyword(e)
                .filter(|(k, _)| matches!(*k, "tokens" | "first"))
                .map(|(_, v)| v)
        })
    });
    let character = matches!(candidate,Some(Expr::Ident(n)) if vars.get(n).is_some_and(|v|matches!(v.kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_))));
    let provisional = crate::named_arguments::tokenize_signature(args, Some(character))?;
    provisional.reorder("tokenize", args)?;
    let declarations = vars
        .iter()
        .map(|(n, v)| (n.clone(), (v.kind.clone(), v.rank)))
        .collect();
    for (slot, label) in [(0, "STRING"), (1, "SET")] {
        if !matches!(
            types.kind_rank(&args[slot], &declarations)?,
            Some((
                TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_),
                0
            ))
        ) {
            bail!("TOKENIZE {label} requires a scalar CHARACTER expression");
        }
    }
    let mut targets = vec![];
    for (i, arg) in args.iter().enumerate().skip(2) {
        let label = &provisional.params[i];
        let Expr::Ident(n) = arg else {
            bail!("TOKENIZE {label} requires a writable allocatable rank-one array");
        };
        let Some(v) = vars.get(n) else {
            bail!("TOKENIZE {label} requires a declared allocatable array");
        };
        if v.rank != 1 || !v.allocatable || v.readonly {
            bail!("TOKENIZE {label} requires a writable allocatable rank-one array");
        }
        if character && !v.deferred {
            bail!("TOKENIZE {label} requires deferred-length CHARACTER");
        }
        if !character && !matches!(v.kind, TypeSpec::Integer(Some(1 | 2 | 4 | 8) | None)) {
            bail!("TOKENIZE {label} requires INTEGER kind 1, 2, 4 or 8");
        }
        if targets.contains(n) {
            bail!("TOKENIZE output arguments must be distinct");
        }
        targets.push(n.clone());
    }
    *serial += 1;
    let stem = format!("@tokenize_{}", serial);
    let names: Vec<_> = [
        "text", "set", "pos", "previous", "count", "longest", "index",
    ]
    .iter()
    .map(|s| format!("{stem}_{s}"))
    .collect();
    let (text, set, pos, previous, count, longest, index) = (
        &names[0], &names[1], &names[2], &names[3], &names[4], &names[5], &names[6],
    );
    let mut out = vec![Stmt::VarDecl {
        kind: TypeSpec::Integer(Some(8)),
        names: names[2..].to_vec(),
    }];
    for (n, value) in [(text, args[0].clone()), (set, args[1].clone())] {
        out.push(Stmt::VarDecl {
            kind: crate::ast::character_length(id("@deferred_character")),
            names: vec![n.clone()],
        });
        out.push(Stmt::DummyIntent {
            name: n.clone(),
            intent: "allocatable".into(),
        });
        out.push(assign(n, value));
    }
    let search = || {
        assign(
            pos,
            expr(
                "@split_scan",
                vec![id(text), id(set), Expr::Logical(false), id(pos)],
            ),
        )
    };
    let done = || Stmt::If {
        cond: bin(BinOp::Gt, id(pos), expr("len", vec![id(text)])),
        then_body: vec![Stmt::Exit],
        else_body: None,
    };
    out.extend([
        assign(pos, int(0)),
        assign(previous, int(0)),
        assign(count, int(0)),
        assign(longest, int(0)),
        Stmt::DoWhile {
            cond: Expr::Logical(true),
            body: vec![
                search(),
                assign(count, bin(BinOp::Add, id(count), int(1))),
                assign(
                    longest,
                    expr(
                        "max",
                        vec![
                            id(longest),
                            bin(BinOp::Sub, bin(BinOp::Sub, id(pos), id(previous)), int(1)),
                        ],
                    ),
                ),
                done(),
                assign(previous, id(pos)),
            ],
        },
    ]);
    if character {
        if let TypeSpec::Character(Some(capacity)) = vars[&targets[0]].kind {
            out.push(Stmt::If {
                cond: bin(BinOp::Gt, id(longest), int(capacity as i64)),
                then_body: vec![call(
                    "@error_stop",
                    vec![Expr::Str(format!(
                        "TOKENIZE token exceeds its {capacity}-character storage capacity"
                    ))],
                )],
                else_body: None,
            });
        }
    }
    for (i, n) in targets.iter().enumerate() {
        out.push(Stmt::If {
            cond: expr("allocated", vec![id(n)]),
            then_body: vec![call("@deallocate", vec![id(n)])],
            else_body: None,
        });
        if character {
            out.push(assign(&width(n), if i == 0 { id(longest) } else { int(1) }));
        }
        out.push(call(
            "@allocate",
            vec![expr(
                n,
                vec![if character && i == 1 {
                    bin(BinOp::Sub, id(count), int(1))
                } else {
                    id(count)
                }],
            )],
        ));
    }
    let substring = |start: Expr, end: Expr| Expr::Section {
        name: text.clone(),
        lower: Some(Box::new(start)),
        upper: Some(Box::new(end)),
        stride: None,
    };
    let mut body = vec![search()];
    let first = bin(BinOp::Add, id(previous), int(1));
    let last = bin(BinOp::Sub, id(pos), int(1));
    if character {
        body.push(Stmt::AssignIndex {
            name: targets[0].clone(),
            indices: vec![id(index)],
            value: substring(first, last),
        });
        if targets.len() == 2 {
            body.push(Stmt::If {
                cond: bin(BinOp::Le, id(pos), expr("len", vec![id(text)])),
                then_body: vec![Stmt::AssignIndex {
                    name: targets[1].clone(),
                    indices: vec![id(index)],
                    value: substring(id(pos), id(pos)),
                }],
                else_body: None,
            });
        }
    } else {
        for (slot, (n, value)) in targets.iter().zip([first, last]).enumerate() {
            if let TypeSpec::Integer(kind) = vars[n].kind {
                let max = crate::inquiries::integer_huge(kind)?;
                body.push(Stmt::If {
                    cond: bin(BinOp::Gt, value.clone(), int(max)),
                    then_body: vec![call(
                        "@error_stop",
                        vec![Expr::Str(format!(
                            "TOKENIZE {} result cannot be represented in its INTEGER kind",
                            ["FIRST", "LAST"][slot]
                        ))],
                    )],
                    else_body: None,
                });
            }
            body.push(Stmt::AssignIndex {
                name: n.clone(),
                indices: vec![id(index)],
                value,
            });
        }
    }
    body.push(assign(previous, id(pos)));
    out.extend([
        assign(pos, int(0)),
        assign(previous, int(0)),
        Stmt::Do {
            var: index.clone(),
            start: int(1),
            end: id(count),
            body,
        },
    ]);
    Ok(out)
}
pub fn prepare(mut p: Program) -> Result<Program> {
    fn used(b: &[Stmt]) -> bool {
        b.iter().any(|s| {
            if matches!(s,Stmt::CallSub{name,..} if name=="tokenize") {
                return true;
            }
            let mut copy = s.clone();
            children(&mut copy).into_iter().any(|b| used(b))
        })
    }
    fn user(b: &[Stmt]) -> bool {
        b.iter().any(|s| {
            if matches!(s,Stmt::Function{name,..}|Stmt::Subroutine{name,..} if name=="tokenize") {
                return true;
            }
            let mut copy = s.clone();
            children(&mut copy).into_iter().any(|b| user(b))
        })
    }
    fn deferred(b: &[Stmt]) -> bool {
        b.iter().any(|s| {
            if matches!(s,Stmt::ArrayDecl{kind,..} if crate::substrings::deferred(kind)) {
                return true;
            }
            let mut copy = s.clone();
            children(&mut copy).into_iter().any(|b| deferred(b))
        })
    }
    // Deferred-length CHARACTER arrays whose length is set at run time are
    // stored like TOKENIZE results.
    let intrinsic = used(&p.body) && !user(&p.body);
    if !intrinsic && !deferred(&p.body) {
        return Ok(p);
    }
    fn expression_capacity(e: &Expr) -> usize {
        match e {
            Expr::Str(s) => s.len(),
            Expr::Bin(BinOp::Concat, a, b) => {
                expression_capacity(a).saturating_add(expression_capacity(b))
            }
            Expr::Call(n, a) if n == "repeat" && a.len() == 2 => {
                crate::parameters::integer_constant(&a[1])
                    .ok()
                    .and_then(|n| usize::try_from(n.max(0)).ok())
                    .unwrap_or(1)
                    .saturating_mul(expression_capacity(&a[0]))
            }
            Expr::Call(_, a) | Expr::Index(_, a) => {
                a.iter().map(expression_capacity).max().unwrap_or(0)
            }
            Expr::Bin(_, a, b) => expression_capacity(a).max(expression_capacity(b)),
            Expr::Un(_, a) => expression_capacity(a),
            _ => 0,
        }
    }
    fn storage_capacity(b: &[Stmt]) -> usize {
        b.iter()
            .map(|s| {
                let mut max = match s {
                    Stmt::VarDecl {
                        kind: TypeSpec::Character(n),
                        ..
                    }
                    | Stmt::ArrayDecl {
                        kind: TypeSpec::Character(n),
                        ..
                    }
                    | Stmt::InitializedDecl {
                        kind: TypeSpec::Character(n),
                        ..
                    } => n.unwrap_or(1),
                    _ => 0,
                };
                let mut copy = s.clone();
                for e in crate::procedure_arguments::expressions(&mut copy) {
                    max = max.max(expression_capacity(e));
                }
                for child in children(&mut copy) {
                    max = max.max(storage_capacity(child));
                }
                max
            })
            .max()
            .unwrap_or(0)
    }
    let capacity = CAPACITY.max(storage_capacity(&p.body));
    if capacity > i32::MAX as usize - 1 {
        bail!("TOKENIZE CHARACTER storage capacity exceeds the supported size");
    }
    fn body(
        b: &mut Vec<Stmt>,
        capacity: usize,
        inherited: &HashMap<String, TypeSpec>,
        intrinsic: bool,
    ) -> Result<()> {
        let mut kinds = inherited.clone();
        for s in b.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        kinds.insert(n.clone(), kind.clone());
                    }
                }
                Stmt::ArrayDecl { kind, name, .. } => {
                    kinds.insert(name.clone(), kind.clone());
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (n, _) in entities {
                        kinds.insert(n.clone(), kind.clone());
                    }
                }
                _ => {}
            }
        }
        fn character(e: &Expr, kinds: &HashMap<String, TypeSpec>) -> Option<bool> {
            match e {
                Expr::Ident(n) => kinds.get(n).map(|k| {
                    matches!(
                        k,
                        TypeSpec::Character(_)
                            | TypeSpec::CharacterAssumed
                            | TypeSpec::CharacterLength(_)
                    )
                }),
                Expr::Call(n, a) if n == "@conditional" => a
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % 2 == 1 || *i + 1 == a.len())
                    .find_map(|(_, e)| character(e, kinds)),
                _ => None,
            }
        }
        let mut location = None;
        let mut added = vec![];
        for s in b.iter_mut() {
            if let Stmt::SourceLocation(n) = s {
                location = Some(*n);
            }
            if let Stmt::CallSub { name, args } = s {
                if name == "tokenize" && intrinsic {
                    (|| -> Result<()> {
                        let provisional = crate::named_arguments::tokenize_signature(args, None)?;
                        let slots = provisional.associate(name, args)?;
                        let candidate = slots[2].map(|i| {
                            crate::named_arguments::keyword(&args[i]).map_or(&args[i], |(_, v)| v)
                        });
                        let signature = crate::named_arguments::tokenize_signature(
                            args,
                            candidate.and_then(|e| character(e, &kinds)),
                        )?;
                        signature.reorder(name, args)
                    })()
                    .map_err(|e| crate::source_diagnostics::at(e, location))?;
                }
            }
            if let Stmt::ArrayDecl { kind, name, .. } = s {
                if crate::substrings::deferred(kind) {
                    *kind = TypeSpec::Character(Some(capacity));
                    added.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![width(name)],
                    });
                    added.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@character_spec_length:{}", width(name)),
                    });
                    added.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: "@deferred_character_array".into(),
                    });
                }
            }
            for child in children(s) {
                body(child, capacity, &kinds, intrinsic)?;
            }
        }
        b.splice(0..0, added);
        Ok(())
    }
    // A length set at run time may not exceed the storage capacity.
    fn capacity_checks(b: &mut Vec<Stmt>, capacity: usize) {
        let mut i = 0;
        while i < b.len() {
            if let Stmt::Assign { name, .. } = &b[i] {
                if name.starts_with("@deferred_width$") {
                    let array = name["@deferred_width$".len()..].to_string();
                    b.insert(i + 1, Stmt::If {
                        cond: bin(BinOp::Gt, id(name), int(capacity as i64)),
                        then_body: vec![call("@error_stop", vec![Expr::Str(format!(
                            "length of deferred-length CHARACTER array `{array}` exceeds its {capacity}-character storage capacity"))])],
                        else_body: None,
                    });
                    i += 1;
                }
            }
            for child in children(&mut b[i]) {
                capacity_checks(child, capacity);
            }
            i += 1;
        }
    }
    body(&mut p.body, capacity, &HashMap::new(), intrinsic)?;
    capacity_checks(&mut p.body, capacity);
    Ok(p)
}

pub fn resolve(mut p: Program) -> Result<Program> {
    fn procedures(body: &[Stmt], names: &mut HashSet<String>) -> bool {
        let mut used = false;
        for s in body {
            if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
                names.insert(name.clone());
            }
            if matches!(s,Stmt::CallSub{name,..} if name=="tokenize") {
                used = true;
            }
            let mut copy = s.clone();
            for child in children(&mut copy) {
                used |= procedures(child, names);
            }
        }
        used
    }
    let mut users = HashSet::new();
    if !procedures(&p.body, &mut users) || users.contains("tokenize") {
        return Ok(p);
    }
    let types = crate::operators::ExpressionTypes::new(&p)?;
    fn body(
        b: &mut Vec<Stmt>,
        inherited: &HashMap<String, Variable>,
        users: &HashSet<String>,
        types: &crate::operators::ExpressionTypes,
        serial: &mut usize,
    ) -> Result<()> {
        let mut vars = inherited.clone();
        for s in b.iter_mut() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        vars.insert(
                            n.clone(),
                            Variable {
                                kind: kind.clone(),
                                rank: 0,
                                allocatable: false,
                                readonly: false,
                                deferred: false,
                            },
                        );
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    let deferred = crate::substrings::deferred(kind);
                    vars.insert(
                        name.clone(),
                        Variable {
                            kind: kind.clone(),
                            rank: dims.len(),
                            allocatable: false,
                            readonly: false,
                            deferred,
                        },
                    );
                }
                _ => {}
            }
        }
        for s in b.iter() {
            if let Stmt::DummyIntent { name, intent } = s {
                if let Some(v) = vars.get_mut(name) {
                    v.deferred |= intent == "@deferred_character_array";
                    v.allocatable |= intent == "allocatable";
                    v.readonly |= matches!(intent.as_str(), "intent(in)" | "parameter");
                }
            }
        }
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(b) {
            if let Stmt::SourceLocation(n) = s {
                location = Some(n);
            }
            (|| -> Result<()> {
                if let Stmt::CallSub { name, args } = &mut s {
                    if name == "tokenize" && !users.contains(name) {
                        s = Stmt::Block {
                            body: lower(args, &vars, types, serial)?,
                        };
                    }
                }
                for child in children(&mut s) {
                    body(child, &vars, users, types, serial)?;
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, location))?;
            out.push(s);
        }
        *b = out;
        Ok(())
    }
    body(&mut p.body, &HashMap::new(), &users, &types, &mut 0)?;
    Ok(p)
}

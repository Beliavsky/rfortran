// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Sequential F2023 REDUCE lowering. Table 11.1 note 1 permits reusing an
//! accumulator across iterations. Keep that accumulator separate from the
//! outside object, and combine only after the construct completes.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Object {
    kind: TypeSpec,
    dims: Vec<Expr>,
    attrs: HashSet<String>,
}
type Scope = HashMap<String, Object>;
fn declarations(body: &[Stmt], parent: &Scope) -> Scope {
    let mut scope = parent.clone();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    scope.insert(
                        n.clone(),
                        Object {
                            kind: kind.clone(),
                            dims: vec![],
                            attrs: HashSet::new(),
                        },
                    );
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                scope.insert(
                    name.clone(),
                    Object {
                        kind: kind.clone(),
                        dims: dims.clone(),
                        attrs: HashSet::new(),
                    },
                );
            }
            Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } => {
                for (n, _) in entities {
                    scope.insert(
                        n.clone(),
                        Object {
                            kind: kind.clone(),
                            dims: vec![],
                            attrs: attributes.iter().cloned().collect(),
                        },
                    );
                }
            }
            Stmt::DerivedDecl { names, .. } => {
                for n in names {
                    scope.remove(n);
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(o) = scope.get_mut(name) {
                o.attrs.insert(intent.clone());
            }
        }
    }
    scope
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn binary(op: &str) -> Option<BinOp> {
    Some(match op {
        "+" => BinOp::Add,
        "*" => BinOp::Mul,
        ".and." => BinOp::And,
        ".or." => BinOp::Or,
        ".eqv." => BinOp::Eqv,
        ".neqv." => BinOp::Neqv,
        _ => return None,
    })
}
fn combine(op: &str, a: Expr, b: Expr) -> Expr {
    match binary(op) {
        Some(op) => Expr::Bin(op, Box::new(a), Box::new(b)),
        None => Expr::Call(op.into(), vec![a, b]),
    }
}
fn identity(op: &str, kind: &TypeSpec, temporary: &str) -> Result<Expr> {
    let integer = matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_));
    let real = matches!(
        kind,
        TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision
    );
    let complex = matches!(kind, TypeSpec::Complex(_));
    let logical = matches!(kind, TypeSpec::Logical | TypeSpec::LogicalKind(_));
    let value = match op {
        "+" | "*" if integer || real || complex => int(if op == "+" { 0 } else { 1 }),
        ".and." | ".or." | ".eqv." | ".neqv." if logical => {
            Expr::Logical(matches!(op, ".and." | ".eqv."))
        }
        "iand" | "ior" | "ieor" if integer => int(if op == "iand" { -1 } else { 0 }),
        "min" | "max" if integer || real => {
            let huge = Expr::Call("huge".into(), vec![id(temporary)]);
            if op == "min" {
                huge
            } else {
                let negative = Expr::Un(crate::ast::UnOp::Neg, Box::new(huge));
                if integer {
                    Expr::sub(negative, int(1))
                } else {
                    negative
                }
            }
        }
        "+" | "*" | ".and." | ".or." | ".eqv." | ".neqv." | "iand" | "ior" | "ieor" | "min"
        | "max" => bail!("REDUCE({op}:...) is not suitable for {kind:?}"),
        _ => bail!("unsupported REDUCE operation `{op}`"),
    };
    Ok(value)
}
fn mentions(e: &Expr, names: &HashSet<String>) -> bool {
    let mut copy = e.clone();
    let mut found = false;
    visit_expr(&mut copy, &mut |e| {
        if let Expr::Ident(n)
        | Expr::Call(n, _)
        | Expr::Index(n, _)
        | Expr::Section { name: n, .. } = e
        {
            found |= names.contains(n);
        }
    });
    found
}
fn same(a: &Expr, b: &Expr) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
// Only the reduction occurrence matching the LHS may mention any reduction
// object. In particular s=s+s and t=t+s are not valid reduction updates.
fn update(value: &Expr, target: &Expr, op: &str, names: &HashSet<String>) -> bool {
    match value {
        Expr::Bin(b, a, c) if binary(op).as_ref() == Some(b) => {
            same(a, target) && !mentions(c, names) || same(c, target) && !mentions(a, names)
        }
        Expr::Call(n, args) if n == op && binary(op).is_none() && args.len() >= 2 => {
            args.iter().filter(|a| same(a, target)).count() == 1
                && args
                    .iter()
                    .filter(|a| !same(a, target))
                    .all(|a| !mentions(a, names))
        }
        _ => false,
    }
}
fn rewrite(
    body: &mut [Stmt],
    ops: &HashMap<String, (String, String)>,
    mut offset: Option<usize>,
    depth: usize,
    loop_names: &[String],
) -> Result<()> {
    let names = ops.keys().cloned().collect();
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
            continue;
        }
        let result = (|| -> Result<()> {
            let target = match &*s {
                Stmt::Assign { name, .. } if ops.contains_key(name) => {
                    Some((name.clone(), id(name)))
                }
                Stmt::AssignIndex { name, indices, .. } if ops.contains_key(name) => Some((
                    name.clone(),
                    if indices.len() == 1 && matches!(&indices[0], Expr::Section { .. }) {
                        indices[0].clone()
                    } else {
                        Expr::Call(name.clone(), indices.clone())
                    },
                )),
                _ => None,
            };
            if let Some((name, target)) = target {
                let (op, temporary) = &ops[&name];
                let value = match &*s {
                    Stmt::Assign { value, .. } | Stmt::AssignIndex { value, .. } => value,
                    _ => unreachable!(),
                };
                if !update(value, &target, op, &names) {
                    bail!("invalid REDUCE update of `{name}`: expected matching `{op}` reduction assignment");
                }
                if let Stmt::AssignIndex { indices, .. } = &*s {
                    if indices.iter().any(|e| {
                        let mut e = e.clone();
                        // Parser sections retain the array's name alongside
                        // their bounds; that name is the designator, not an
                        // extra reference in a subscript expression.
                        visit_expr(&mut e, &mut |e| {
                            if let Expr::Section { name: n, .. } = e {
                                if n == &name {
                                    *n = String::new();
                                }
                            }
                        });
                        mentions(&e, &names)
                    }) {
                        bail!("REDUCE variable cannot appear in reduction subscripts");
                    }
                }
                match s {
                    Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                        *name = temporary.clone()
                    }
                    _ => {}
                }
                for e in expressions(s) {
                    visit_expr(e, &mut |e| match e {
                        Expr::Ident(n)
                        | Expr::Call(n, _)
                        | Expr::Index(n, _)
                        | Expr::Section { name: n, .. }
                            if n == &name =>
                        {
                            *n = temporary.clone()
                        }
                        _ => {}
                    });
                }
                return Ok(());
            }
            if let Stmt::Do { var, .. } = s {
                if names.contains(var) {
                    bail!("REDUCE variable `{var}` cannot be a DO index");
                }
            }
            for e in expressions(s) {
                if mentions(e, &names) {
                    bail!("REDUCE variable may only appear in its reduction assignment");
                }
            }
            if matches!(s, Stmt::Exit) && depth == 0 {
                bail!("EXIT cannot leave a DO CONCURRENT construct");
            }
            if matches!(s, Stmt::Return(_)) {
                bail!("RETURN cannot leave a DO CONCURRENT construct");
            }
            // Named exits need the enclosing construct stack; do not silently
            // accept a transfer that could bypass the final combination.
            if matches!(s,Stmt::CallSub{name,..} if name=="@named_exit") {
                bail!("named EXIT inside a REDUCE construct is not yet supported");
            }
            if let Stmt::CallSub { name, args } = s {
                if name == "@named_cycle"
                    && !matches!(args.first(),Some(Expr::Str(n)) if loop_names.contains(n))
                {
                    bail!("CYCLE cannot leave a DO CONCURRENT construct");
                }
            }
            let inner = depth
                + usize::from(
                    matches!(s, Stmt::Do { .. } | Stmt::DoWhile { .. })
                        || matches!(s,Stmt::Block{body} if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@do_step")),
                );
            let mut loop_names = loop_names.to_vec();
            if let Stmt::Block { body } = s {
                if let Some(Stmt::CallSub { name, args }) = body.first() {
                    if name == "@named_loop" {
                        if let Some(Expr::Str(n)) = args.first() {
                            loop_names.push(n.clone());
                        }
                    }
                }
            }
            for b in children(s) {
                rewrite(b, ops, offset, inner, &loop_names)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}

fn lower(
    args: Vec<Expr>,
    mut body: Vec<Stmt>,
    scope: &Scope,
    serial: &mut usize,
    offset: Option<usize>,
    loop_name: Option<&str>,
) -> Result<Vec<Stmt>> {
    let mut args = args.into_iter();
    let Expr::Call(_, mut controls) = args.next().context("missing concurrent controls")? else {
        bail!("invalid concurrent controls")
    };
    let mask = args.next().context("missing concurrent mask")?;
    let mut prefix = vec![];
    let mut ops = HashMap::new();
    let mut suffix = vec![];
    for clause in args {
        let Expr::Call(tag, variables) = clause else {
            bail!("invalid reduction clause")
        };
        let op = tag
            .strip_prefix("@reduce:")
            .context("invalid reduction operation")?;
        for variable in variables {
            let Expr::Ident(n) = variable else {
                bail!("REDUCE requires a variable name")
            };
            if ops.contains_key(&n) {
                bail!("duplicate REDUCE variable `{n}`");
            }
            if controls
                .chunks(4)
                .any(|c| matches!(&c[0],Expr::Ident(index) if index==&n))
            {
                bail!("REDUCE variable `{n}` cannot be a concurrent index");
            }
            let object = scope.get(&n).with_context(|| {
                format!("REDUCE variable `{n}` requires a declared intrinsic type")
            })?;
            for attr in [
                "parameter",
                "intent(in)",
                "in",
                "optional",
                "asynchronous",
                "volatile",
                "protected",
            ] {
                if object.attrs.contains(attr) {
                    bail!("REDUCE variable `{n}` cannot have the {attr} attribute");
                }
            }
            if object.attrs.contains("pointer") || object.attrs.contains("allocatable") {
                bail!("REDUCE of POINTER or ALLOCATABLE variable `{n}` is not yet supported");
            }
            if object.dims.iter().any(|e|matches!(e,Expr::Ident(n)|Expr::Call(n,_) if matches!(n.as_str(),"@deferred_extent"|"@assumed_extent"))) {bail!("REDUCE variable `{n}` currently requires explicit array bounds");}
            for dimension in &object.dims {
                let mut dimension = dimension.clone();
                let mut runtime = false;
                visit_expr(&mut dimension, &mut |e| {
                    if let Expr::Ident(n) = e {
                        runtime |= scope.get(n).is_some_and(|o| !o.attrs.contains("parameter"));
                    }
                });
                if runtime {
                    bail!("REDUCE variable `{n}` currently requires constant array bounds; runtime-sized reduction storage is not yet supported");
                }
            }
            *serial += 1;
            let temporary = format!("@reduce_value_{serial}");
            let initial = identity(op, &object.kind, &temporary)?;
            prefix.push(if object.dims.is_empty() {
                Stmt::VarDecl {
                    kind: object.kind.clone(),
                    names: vec![temporary.clone()],
                }
            } else {
                Stmt::ArrayDecl {
                    kind: object.kind.clone(),
                    name: temporary.clone(),
                    dims: object.dims.clone(),
                }
            });
            prefix.push(set(&temporary, initial));
            suffix.push(set(&n, combine(op, id(&n), id(&temporary))));
            ops.insert(n, (op.to_string(), temporary));
        }
    }
    rewrite(
        &mut body,
        &ops,
        offset,
        0,
        &loop_name
            .map(str::to_string)
            .into_iter()
            .collect::<Vec<_>>(),
    )?;
    *serial += 1;
    let active = format!("@reduce_active_{serial}");
    prefix.push(Stmt::VarDecl {
        kind: TypeSpec::Logical,
        names: vec![active.clone()],
    });
    prefix.push(set(&active, Expr::Logical(false)));
    body.insert(0, set(&active, Expr::Logical(true)));
    // Evaluate every control once, before any iteration changes outside data.
    for c in controls.chunks_mut(4) {
        let kind = match &c[0] {
            Expr::Ident(n) => scope
                .get(n)
                .map(|o| o.kind.clone())
                .unwrap_or(TypeSpec::Integer(None)),
            _ => TypeSpec::Integer(None),
        };
        for e in &mut c[1..3] {
            *serial += 1;
            let temporary = format!("@reduce_bound_{serial}");
            prefix.push(Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![temporary.clone()],
            });
            prefix.push(set(&temporary, e.clone()));
            *e = id(&temporary);
        }
    }
    prefix.push(crate::ast::do_concurrent(controls, Some(mask), body));
    prefix.push(Stmt::If {
        cond: id(&active),
        then_body: suffix,
        else_body: None,
    });
    Ok(prefix)
}
fn walk(
    body: &mut Vec<Stmt>,
    parent: &Scope,
    serial: &mut usize,
    mut offset: Option<usize>,
    loop_name: Option<&str>,
) -> Result<()> {
    let scope = declarations(body, parent);
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
            continue;
        }
        // Lower before walking children, while the marker still owns its body.
        if let Stmt::Block { body } = s {
            if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@concurrent_reduce") {
                let Stmt::CallSub { args, .. } = body.remove(0) else {
                    unreachable!()
                };
                *body = lower(
                    args,
                    std::mem::take(body),
                    &scope,
                    serial,
                    offset,
                    loop_name,
                )
                .map_err(|e| crate::source_diagnostics::at(e, offset))?;
            }
        }
        let child_name = match &*s {
            Stmt::Block { body } => match body.first() {
                Some(Stmt::CallSub { name, args }) if name == "@named_loop" => match args.first() {
                    Some(Expr::Str(n)) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        for b in children(s) {
            walk(b, &scope, serial, offset, child_name.as_deref())?;
        }
        // Keep a named-loop wrapper around the generated DO, not around the
        // declarations and final outside-value combination.
        if let Stmt::Block { body } = s {
            if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@named_loop")
                && body.len() == 2
            {
                if let Stmt::Block { body: generated } = &body[1] {
                    if generated.iter().any(|s|matches!(s,Stmt::VarDecl{names,..} if names.iter().any(|n|n.starts_with("@reduce_active_")))) {
                        let marker=body.remove(0);
                        let Stmt::Block{body:mut generated}=body.remove(0) else {unreachable!()};
                        let index=generated.len()-2;
                        let loop_stmt=generated.remove(index);
                        generated.insert(index,name_iteration(loop_stmt,marker));
                        *body=generated;
                    }
                }
            }
        }
    }
    Ok(())
}
// CYCLE of a multi-index concurrent construct advances to the next tuple,
// so the source construct name belongs to the innermost generated DO.
fn name_iteration(mut stmt: Stmt, marker: Stmt) -> Stmt {
    let is_loop = |s: &Stmt| {
        matches!(s, Stmt::Do { .. })
            || matches!(s,Stmt::Block{body} if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@do_step"))
    };
    let nested = match &mut stmt {
        Stmt::Do { body, .. } if body.len() == 1 && is_loop(&body[0]) => Some(&mut body[0]),
        Stmt::Block { body } if body.len() == 2 && is_loop(&body[1]) => Some(&mut body[1]),
        _ => None,
    };
    if let Some(child) = nested {
        let child_stmt = std::mem::replace(child, Stmt::Cycle);
        *child = name_iteration(child_stmt, marker);
        stmt
    } else {
        Stmt::Block {
            body: vec![marker, stmt],
        }
    }
}
pub fn resolve(mut program: Program) -> Result<Program> {
    walk(&mut program.body, &Scope::new(), &mut 0, None, None)?;
    Ok(program)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize local assumed-shape subroutine calls with statically known arrays.
//! No descriptor ABI is claimed: unsupported dynamic shapes are diagnosed here.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
#[derive(Clone)]
struct Routine {
    elemental: bool,
    params: Vec<String>,
    body: Vec<Stmt>,
}

fn scalar_actual_kind(e: &Expr, shapes: &Shapes) -> Result<TypeSpec> {
    use crate::ast::BinOp;
    match e {
        Expr::Ident(n) => {
            let (kind, dims) = shapes.get(n).context("scalar actual argument must be declared")?;
            if !dims.is_empty() { bail!("subroutine array actual arguments are not yet supported; pass scalar arguments"); }
            Ok(kind.clone())
        }
        Expr::IntLit(_) => Ok(TypeSpec::Integer(None)),
        Expr::RealLit(_) => Ok(TypeSpec::Real),
        Expr::Logical(_) => Ok(TypeSpec::Logical),
        Expr::Str(s) => Ok(TypeSpec::Character(Some(s.len()))),
        Expr::Un(_, a) => scalar_actual_kind(a, shapes),
        Expr::Bin(op, a, b) => {
            let a = scalar_actual_kind(a, shapes)?;
            let b = scalar_actual_kind(b, shapes)?;
            if matches!(op, BinOp::Concat) {
                if let (TypeSpec::Character(a), TypeSpec::Character(b)) = (a, b) {
                    return Ok(TypeSpec::Character(Some(a.unwrap_or(1) + b.unwrap_or(1))));
                }
                bail!("CHARACTER concatenation requires CHARACTER operands");
            }
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) {
                Ok(if matches!(a, TypeSpec::DoublePrecision) || matches!(b, TypeSpec::DoublePrecision) {TypeSpec::DoublePrecision}
                   else if matches!(a, TypeSpec::Real) || matches!(b, TypeSpec::Real) {TypeSpec::Real} else {a})
            } else { Ok(TypeSpec::Logical) }
        }
        Expr::Call(n, args) if matches!(n.as_str(), "dble" | "real" | "int") && args.len() == 1 => {
            scalar_actual_kind(&args[0], shapes)?;
            Ok(match n.as_str() { "dble" => TypeSpec::DoublePrecision, "real" => TypeSpec::Real, _ => TypeSpec::Integer(None) })
        }
        Expr::Section { .. } => bail!("subroutine array actual arguments are not yet supported; pass scalar arguments"),
        _ => bail!("unsupported scalar actual expression in specialized subroutine call"),
    }
}

fn rewrite(
    e: &mut Expr,
    shapes: &Shapes,
    constants: &HashMap<String, Expr>,
    absent: &HashSet<String>,
) -> Result<()> {
    match e {
        Expr::Ident(n) => {
            if absent.contains(n) { bail!("reference to absent OPTIONAL dummy `{n}`"); }
            if let Some(v) = constants.get(n) {
                *e = v.clone();
            }
        }
        Expr::Call(n, args) if n == "present" => {
            let [Expr::Ident(param)] = args.as_slice() else {
                bail!("PRESENT requires an optional dummy argument");
            };
            if !shapes.contains_key(&format!("@optional:{param}")) { bail!("PRESENT requires an OPTIONAL dummy argument"); }
            *e = Expr::Logical(!absent.contains(param));
        }
        Expr::Call(n, args) if n == "size" => {
            let Some(Expr::Ident(array)) = args.first() else {
                bail!("SIZE currently requires a whole array");
            };
            let (_, dims) = shapes
                .get(array)
                .context("SIZE argument must be an array")?;
            if dims.is_empty() {
                bail!("SIZE argument must be an array");
            }
            *e = match args.as_slice() {
                [_] => dims
                    .iter()
                    .cloned()
                    .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Mul, Box::new(a), Box::new(b)))
                    .unwrap(),
                [_, dim] => {
                    let mut dim = dim.clone();
                    rewrite(&mut dim, shapes, constants, absent)?;
                    let i = crate::parameters::integer_constant(&dim)?;
                    if i < 1 || i as usize > dims.len() {
                        bail!("SIZE dimension must be between 1 and {}", dims.len());
                    }
                    dims[i as usize - 1].clone()
                }
                _ => bail!("SIZE expects an array and optional dimension"),
            };
        }
        Expr::Call(n, args) if matches!(n.as_str(), "lbound" | "ubound") => {
            for dim in args.iter_mut().skip(1) { rewrite(dim, shapes, constants, absent)?; }
            if let Some(Expr::Ident(array)) = args.first() {
                if let Some((_, dims)) = shapes.get(array) {
                    if let Ok(dims) = dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>() {
                        *e = crate::inquiries::bounds(n, &dims, args.get(1))?;
                    }
                }
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args {
                rewrite(a, shapes, constants, absent)?;
            }
        }
        Expr::Bin(_, a, b) => {
            rewrite(a, shapes, constants, absent)?;
            rewrite(b, shapes, constants, absent)?;
        }
        Expr::Un(_, a) => rewrite(a, shapes, constants, absent)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                rewrite(e, shapes, constants, absent)?;
            }
        }

        _ => {}
    }
    if let Ok(n) = crate::parameters::integer_constant(e) {
        *e = Expr::IntLit(n.to_string());
    }
    Ok(())
}

// Substitute proven constants only in section bounds; keep ordinary variable
// references so later initialization and unused-variable checks retain them.
fn rewrite_value(
    e: &mut Expr,
    shapes: &Shapes,
    constants: &HashMap<String, Expr>,
    absent: &HashSet<String>,
) -> Result<()> {
    match e {
        Expr::Section { .. } => return rewrite(e, shapes, constants, absent),
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                rewrite_value(e, shapes, constants, absent)?;
            }
        }
        Expr::Bin(_, a, b) => {
            rewrite_value(a, shapes, constants, absent)?;
            rewrite_value(b, shapes, constants, absent)?;
        }
        Expr::Un(_, a) => rewrite_value(a, shapes, constants, absent)?,
        _ => {}
    }
    rewrite(e, shapes, &HashMap::new(), absent)
}

fn specialize(
    body: &mut Vec<Stmt>,
    shapes: &mut Shapes,
    absent: &HashSet<String>,
    constants: &mut HashMap<String, Expr>,
    stable: &HashSet<String>,
) -> Result<()> {
    if let Some(Stmt::CallSub { name, args }) = body.first() {
        if name == "@forall" || name == "@do_step" {
            for control in args.chunks_exact(4) {
                if let Expr::Ident(index) = &control[0] { constants.remove(index); }
            }
        }
    }
    let allocatable: HashSet<_> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "allocatable" => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut allocated = HashSet::new();
    let deferred: HashMap<_, _> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::ArrayDecl { name, kind, dims }
                if dims
                    .iter()
                    .any(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent")) =>
            {
                Some((name.clone(), (kind.clone(), dims.clone())))
            }
            _ => None,
        })
        .collect();
    body.retain(|s| !matches!(s, Stmt::ArrayDecl { name, .. } if deferred.contains_key(name)));
    let mut output = Vec::new();
    for mut s in std::mem::take(body) {
        let pending: HashSet<_> = deferred
            .keys()
            .filter(|n| !allocated.contains(*n))
            .cloned()
            .collect();
        check_allocated(&s, &pending)?;
        match &mut s {
            Stmt::ArrayDecl { name, kind, dims } => {
                for d in dims.iter_mut() {
                    rewrite(d, shapes, constants, absent)?;
                }
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { names, kind } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::CallSub { name, args } if name == "@allocate" => {
                for a in args {
                    let Expr::Call(n, dims) = a else {
                        bail!("ALLOCATE expects array bounds");
                    };
                    if !allocatable.contains(n) {
                        bail!("ALLOCATE requires ALLOCATABLE array `{n}`");
                    }
                    if !allocated.insert(n.clone()) {
                        bail!("repeated ALLOCATE of `{n}` without DEALLOCATE is not supported");
                    }
                    let (kind, declared_dims) = deferred
                        .get(n)
                        .context(
                            "ALLOCATE requires a local deferred-shape array declared in this scope",
                        )?
                        .clone();
                    for d in dims.iter_mut() {
                        rewrite(d, shapes, constants, absent)?;
                    }
                    if dims.len() != declared_dims.len() {
                        bail!("ALLOCATE rank mismatch for `{n}`");
                    }
                    if !declared_dims
                        .iter()
                        .all(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent"))
                    {
                        bail!("ALLOCATABLE array `{n}` requires all dimensions to be deferred");
                    }
                    if dims
                        .iter()
                        .any(|d| crate::parameters::integer_constant(d).is_err())
                    {
                        bail!("ALLOCATE currently requires a shape known at the call site");
                    }
                    if dims
                        .iter()
                        .any(|d| crate::parameters::integer_constant(d).is_ok_and(|n| n < 1))
                    {
                        bail!("ALLOCATE currently requires a positive extent");
                    }
                    shapes.insert(n.clone(), (kind.clone(), dims.clone()));
                    output.push(Stmt::ArrayDecl {
                        name: n.clone(),
                        kind,
                        dims: dims.clone(),
                    });
                }
                continue;
            }
            Stmt::Assign { name, value } => {
                if absent.contains(name) { bail!("assignment to absent OPTIONAL dummy `{name}`"); }
                let mut folded = value.clone();
                rewrite(&mut folded, shapes, constants, absent)?;
                if stable.contains(name)
                    && shapes.get(name).is_some_and(|(_, dims)| dims.is_empty())
                    && crate::parameters::integer_constant(&folded).is_ok()
                {
                    constants.insert(name.clone(), folded);
                }
                rewrite_value(value, shapes, constants, absent)?;
            }
            Stmt::AssignIndex { value, indices, .. } => {
                rewrite_value(value, shapes, constants, absent)?;
                for e in indices {
                    rewrite_value(e, shapes, constants, absent)?;
                }
            }
            Stmt::CallSub { name, args } if name == "@forall" || name == "@do_step" => {
                for control in args.chunks_exact_mut(4) {
                    for arg in &mut control[1..] { rewrite(arg, shapes, constants, absent)?; }
                }
            }
            Stmt::CallSub { args, .. } | Stmt::Print { items: args } => {
                for e in args {
                    rewrite_value(e, shapes, constants, absent)?;
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                rewrite(cond, shapes, constants, absent)?;
                if let Expr::Logical(value) = cond {
                    let mut chosen = if *value {
                        std::mem::take(then_body)
                    } else {
                        else_body.take().unwrap_or_default()
                    };
                    specialize(&mut chosen, shapes, absent, constants, stable)?;
                    output.extend(chosen);
                    continue;
                }
                specialize(
                    then_body,
                    &mut shapes.clone(),
                    absent,
                    &mut constants.clone(),
                    stable,
                )?;
                if let Some(b) = else_body {
                    specialize(
                        b,
                        &mut shapes.clone(),
                        absent,
                        &mut constants.clone(),
                        stable,
                    )?;
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                rewrite(start, shapes, constants, absent)?;
                rewrite(end, shapes, constants, absent)?;
                specialize(
                    body,
                    &mut shapes.clone(),
                    absent,
                    &mut constants.clone(),
                    stable,
                )?;
            }
            Stmt::Block { body } => specialize(
                body,
                &mut shapes.clone(),
                absent,
                &mut constants.clone(),
                stable,
            )?,
            _ => {}
        }
        output.push(s);
    }
    *body = output;
    Ok(())
}
fn check_allocated(s: &Stmt, pending: &HashSet<String>) -> Result<()> {
    fn name(n: &str, pending: &HashSet<String>) -> Result<()> {
        if pending.contains(n) {
            bail!("array `{n}` is used before ALLOCATE");
        }
        Ok(())
    }
    fn expr(e: &Expr, pending: &HashSet<String>) -> Result<()> {
        match e {
            Expr::Ident(n) => name(n, pending)?,
            Expr::Call(n, args) | Expr::Index(n, args) => {
                name(n, pending)?;
                for a in args {
                    expr(a, pending)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, pending)?;
                expr(b, pending)?;
            }
            Expr::Un(_, a) => expr(a, pending)?,
            Expr::Section {
                name: n,
                lower,
                upper,
                stride,
            } => {
                name(n, pending)?;
                for a in [lower, upper, stride].into_iter().flatten() {
                    expr(a, pending)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    match s {
        Stmt::Assign { name: n, value } => {
            name(n, pending)?;
            expr(value, pending)?;
        }
        Stmt::AssignIndex {
            name: n,
            indices,
            value,
        } => {
            name(n, pending)?;
            for e in indices {
                expr(e, pending)?;
            }
            expr(value, pending)?;
        }
        Stmt::CallSub { name: n, .. } if n == "@allocate" => {}
        Stmt::CallSub { args, .. } | Stmt::Read { args } | Stmt::Print { items: args } => {
            for e in args {
                expr(e, pending)?;
            }
        }
        Stmt::Return(Some(e)) => expr(e, pending)?,
        Stmt::ArrayDecl { dims, .. } => {
            for e in dims {
                expr(e, pending)?;
            }
        }
        Stmt::SelectCase {
            expr: e,
            cases,
            default,
        } => {
            expr(e, pending)?;
            for c in cases {
                for s in &c.body {
                    check_allocated(s, pending)?;
                }
            }
            if let Some(b) = default {
                for s in b {
                    check_allocated(s, pending)?;
                }
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr(cond, pending)?;
            for s in then_body {
                check_allocated(s, pending)?;
            }
            if let Some(b) = else_body {
                for s in b {
                    check_allocated(s, pending)?;
                }
            }
        }
        Stmt::Do {
            start, end, body, ..
        } => {
            expr(start, pending)?;
            expr(end, pending)?;
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        Stmt::DoWhile { cond, body } => {
            expr(cond, pending)?;
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        Stmt::Block { body } => {
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn stable(body: &[Stmt]) -> HashSet<String> {
    fn scan(body: &[Stmt], counts: &mut HashMap<String, usize>, nested: bool) {
        for s in body {
            match s {
                Stmt::Assign { name, .. } => {
                    *counts.entry(name.clone()).or_default() += if nested { 2 } else { 1 }
                }
                Stmt::Do { var, body, .. } => {
                    counts.insert(var.clone(), 2);
                    scan(body, counts, true);
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scan(then_body, counts, true);
                    if let Some(b) = else_body {
                        scan(b, counts, true);
                    }
                }
                Stmt::DoWhile { body, .. } | Stmt::Block { body } => scan(body, counts, true),
                Stmt::CallSub { name, .. } if name == "@forall" => {}
                Stmt::CallSub { args, .. } | Stmt::Read { args } => {
                    for a in args {
                        if let Expr::Ident(n) = a {
                            counts.insert(n.clone(), 2);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut counts = HashMap::new();
    scan(body, &mut counts, false);
    counts
        .into_iter()
        .filter_map(|(n, c)| (c == 1).then_some(n))
        .collect()
}
fn validate_input_writes(body: &[Stmt], inputs: &HashSet<String>) -> Result<()> {
    for s in body {
        match s {
            Stmt::Assign { name, .. }
            | Stmt::AssignIndex { name, .. }
            | Stmt::Do { var: name, .. }
                if inputs.contains(name) =>
            {
                bail!("cannot modify INTENT(IN) dummy `{name}`")
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                validate_input_writes(then_body, inputs)?;
                if let Some(b) = else_body {
                    validate_input_writes(b, inputs)?;
                }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                validate_input_writes(body, inputs)?
            }
            _ => {}
        }
    }
    Ok(())
}
fn expand(
    body: &mut Vec<Stmt>,
    inherited: &Shapes,
    routines: &HashMap<String, Routine>,
    active: &mut Vec<String>,
    serial: &mut usize,
) -> Result<()> {
    if body
        .iter()
        .any(|s| matches!(s, Stmt::DummyIntent { intent, .. } if intent == "allocatable"))
    {
        let stable = stable(body);
        specialize(
            body,
            &mut inherited.clone(),
            &HashSet::new(),
            &mut HashMap::new(),
            &stable,
        )?;
    }
    let readonly: HashSet<_> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "parameter" => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut shapes = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, kind, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { names, kind } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            _ => {}
        }
    }
    for s in body.iter_mut() {
        if let Stmt::CallSub { name, args } = s {
            if let Some(r) = routines.get(name) {
                if r.elemental { crate::elemental::validate_subroutine(name, &r.params, &r.body)?; }
                if active.contains(name) {
                    bail!("recursive specialized subroutine calls are not yet supported");
                }
                if args.len() > r.params.len() {
                    bail!("too many arguments to `{name}`");
                }
                let optional: HashSet<_> = r
                    .body
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::DummyIntent { name, intent } if intent == "optional" => {
                            Some(name.clone())
                        }
                        _ => None,
                    })
                    .collect();
                let intents: HashMap<_, _> = r
                    .body
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::DummyIntent { name, intent } if intent.starts_with("intent(") => {
                            Some((name.clone(), intent.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                validate_input_writes(
                    &r.body,
                    &intents
                        .iter()
                        .filter_map(|(n, i)| (i == "intent(in)").then_some(n.clone()))
                        .collect(),
                )?;
                let mut local = Shapes::new();
                for d in &r.body {
                    match d {
                        Stmt::VarDecl { kind, names } => {
                            for n in names {
                                local.insert(n.clone(), (kind.clone(), vec![]));
                            }
                        }
                        Stmt::ArrayDecl { kind, name, dims } => {
                            local.insert(name.clone(), (kind.clone(), dims.clone()));
                        }
                        _ => {}
                    }
                }
                for p in &optional {
                    if !r.params.contains(p) { bail!("OPTIONAL attribute requires a dummy argument `{p}`"); }
                    local.insert(format!("@optional:{p}"),(TypeSpec::Logical,vec![]));
                }
                let mut names = HashMap::new();
                let mut absent = HashSet::new();
                let mut prefix = vec![];
                for (i, p) in r.params.iter().enumerate() {
                    let Some(actual) = args.get(i) else {
                        if !optional.contains(p) {
                            bail!("missing required argument `{p}` of `{name}`");
                        }
                        absent.insert(p.clone());
                        continue;
                    };
                    let (mut kind, dims) = local
                        .get(p)
                        .context("dummy argument must be declared")?
                        .clone();
                    if r.elemental || matches!(kind,TypeSpec::Character(_) | TypeSpec::CharacterAssumed) {
                        let actual_kind = scalar_actual_kind(actual, &shapes)?;
                        if let TypeSpec::Character(length) = actual_kind {
                            match kind {
                                TypeSpec::CharacterAssumed => kind = TypeSpec::Character(length),
                                TypeSpec::Character(expected) if expected.unwrap_or(1) == length.unwrap_or(1) => {},
                                _ => bail!("type or CHARACTER length mismatch for dummy `{p}`"),
                            }
                        } else if kind != actual_kind {
                            bail!("type mismatch for dummy `{p}`");
                        }
                    }
                    local.insert(p.clone(),(kind.clone(),dims.clone()));
                    let intent = intents.get(p).map(String::as_str).unwrap_or("");
                    if intent != "intent(in)"
                        && matches!(actual,Expr::Ident(n) if readonly.contains(n))
                    {
                        bail!("cannot pass PARAMETER as writable argument `{p}`");
                    }
                    if !dims.is_empty() {
                        let Expr::Ident(a) = actual else {
                            bail!("array dummy `{p}` currently requires a whole-array actual argument");
                        };
                        let (actual_kind, actual_dims) =
                            shapes.get(a).context("undeclared array argument")?;
                        if actual_kind != &kind || actual_dims.len() != dims.len() {
                            bail!("type or rank mismatch for `{p}`");
                        }
                        if !dims
                            .iter()
                            .all(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent"))
                        {
                            if dims.iter().zip(actual_dims).any(|(a, b)| {
                                crate::parameters::integer_constant(a)
                                    .ok()
                                    .zip(crate::parameters::integer_constant(b).ok())
                                    .is_none_or(|(a, b)| a != b)
                            }) {
                                bail!("explicit-shape dummy `{p}` currently requires an exactly matching extent");
                            }
                        }
                        local.insert(p.clone(), (kind, actual_dims.clone()));
                        names.insert(p.clone(), a.clone());
                        if intent == "intent(out)" {
                            prefix.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![actual.clone()],
                            });
                        }
                    } else if intent == "intent(in)" {
                        *serial += 1;
                        let temp = format!("@array_call_input_{serial}");
                        prefix.push(Stmt::VarDecl {
                            kind,
                            names: vec![temp.clone()],
                        });
                        prefix.push(Stmt::Assign {
                            name: temp.clone(),
                            value: actual.clone(),
                        });
                        names.insert(p.clone(), temp);
                    } else {
                        let Expr::Ident(a) = actual else {
                            bail!("writable dummy `{p}` requires a variable");
                        };
                        names.insert(p.clone(), a.clone());
                        if intent == "intent(out)" {
                            prefix.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![actual.clone()],
                            });
                        }
                    }
                }
                let mut cloned = r.body.clone();
                for declaration in &mut cloned {
                    if let Stmt::VarDecl {names,..} = declaration { names.retain(|n| !r.params.contains(n)); }
                }
                cloned.retain(|s| !matches!(s,Stmt::ArrayDecl{name,..} if r.params.contains(name))
                    && !matches!(s,Stmt::VarDecl {names,..} if names.is_empty()));
                let stable = stable(&cloned);
                specialize(
                    &mut cloned,
                    &mut local,
                    &absent,
                    &mut HashMap::new(),
                    &stable,
                )?;
                crate::subroutines::rename(&mut cloned, &names, &r.params, serial)?;
                let mut call_shapes = shapes.clone();
                for statement in &prefix {
                    if let Stmt::VarDecl { kind, names } = statement {
                        for name in names { call_shapes.insert(name.clone(), (kind.clone(), vec![])); }
                    }
                }
                active.push(name.clone());
                expand(&mut cloned, &call_shapes, routines, active, serial)?;
                active.pop();
                prefix.extend(cloned);
                *s = Stmt::Block { body: prefix };
                continue;
            }
        }
        match s {
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => expand(body, &shapes, routines, active, serial)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                expand(then_body, &shapes, routines, active, serial)?;
                if let Some(b) = else_body {
                    expand(b, &shapes, routines, active, serial)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn resolve(mut p: Program) -> Result<Program> {
    p = crate::optional::resolve(p)?;
    p = crate::vector_subscripts::resolve(p)?;
    p = crate::array_bounds::resolve(p)?;
    crate::elemental::validate_program(&p)?;
    fn collect(body: &mut Vec<Stmt>, routines: &mut HashMap<String, Routine>) {
        body.retain_mut(|s| match s {
            Stmt::Subroutine {
                name, params, body, elemental, ..
            } if *elemental || body
                .iter()
                .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name))
                    || matches!(s,Stmt::VarDecl {kind:TypeSpec::Character(_) | TypeSpec::CharacterAssumed,names} if names.iter().any(|n| params.contains(n)))) =>
            {
                routines.insert(
                    name.clone(),
                    Routine {
                        elemental: *elemental,
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                false
            }
            Stmt::Module { body, .. } => {
                collect(body, routines);
                true
            }
            _ => true,
        });
    }
    let mut routines = HashMap::new();
    collect(&mut p.body, &mut routines);
    expand(&mut p.body, &Shapes::new(), &routines, &mut vec![], &mut 0)?;
    let p = crate::character_arrays::resolve(p)?;
    crate::multidim::resolve(crate::function_arrays::resolve(crate::substrings::resolve(p)?)?)
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> anyhow::Result<crate::ast::Program> {
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "array_calls.f90").unwrap();
        crate::reductions::resolve(super::resolve(crate::parameters::resolve(p)?)?)
    }
    #[test]
    fn rejects_unsupported_or_invalid_array_calls() {
        for (actual,decl,dummy,body,error) in [
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:),c(:)","b=0.0d0","missing required"),
            ("a","integer :: a(2)","real(8),intent(out)::b(:)","b=0.0d0","type or rank"),
            ("a","real(8) :: a(2)","real(8),intent(in)::b(:)","b=0.0d0","INTENT(IN)"),
            ("a","real(8),parameter::a(2)=[1.0d0,2.0d0]","real(8),intent(out)::b(:)","b=0.0d0","PARAMETER"),
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:)","real(8)::scratch(:)\nallocate(scratch(2))\nb=0.0d0","ALLOCATABLE"),
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:)","real(8),allocatable::scratch(:)\nallocate(scratch(2))\nallocate(scratch(2))\nb=0.0d0","repeated ALLOCATE"),
        ] {
            let params=if dummy.contains(",c(:)") {"b,c"} else {"b"};
            let source=format!("program main\n{decl}\ncall f({actual})\ncontains\nsubroutine f({params})\n{dummy}\n{body}\nend subroutine f\nend program main");
            let err=lower(&source).unwrap_err();assert!(format!("{err:#}").contains(error),"{err:#}");
        }
    }
    #[test]
    fn mixture_and_array_features_lower_and_check_initialization() {
        for source in [
            include_str!("../tests/47_normal_mixture.f90"),
            include_str!("../tests/48_array_calls_expressions.f90"),
        ] {
            let p = lower(source).unwrap();
            crate::initialized::instrument(p).unwrap();
        }
    }
}

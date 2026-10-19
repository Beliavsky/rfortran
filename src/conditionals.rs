// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower Fortran 2023 lazy choices before call specialization can evaluate actuals.
//! Branching around the consuming statement preserves array/derived values and
//! the identity of writable actual arguments without an extra value copy.
use crate::ast::{Expr, Program, Stmt};
use crate::procedure_arguments::{children, expressions};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

fn procedure_name(n: &str) -> bool {
    !n.starts_with('@') || n.starts_with("@module$") || n.starts_with("@procedure_")
}
pub fn nil(e: &Expr) -> bool {
    matches!(e,Expr::Call(n,a) if n=="@conditional_nil" && a.is_empty())
}
pub fn choice(e: &Expr) -> bool {
    matches!(e,Expr::Call(n,_) if n=="@conditional")
}
fn present(e: &Expr) -> bool {
    if choice(e) || nil(e) {
        return true;
    }
    match e {
        Expr::Call(_, a) | Expr::Index(_, a) => a.iter().any(present),
        Expr::Bin(_, a, b) => present(a) || present(b),
        Expr::Un(_, a) => present(a),
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => [lower, upper, stride]
            .into_iter()
            .flatten()
            .any(|e| present(e)),
        _ => false,
    }
}
pub fn contains(program: &Program) -> bool {
    fn body(b: &[Stmt]) -> bool {
        b.iter().any(|s| {
            let mut copy = s.clone();
            expressions(&mut copy)
                .into_iter()
                .any(|e| present(e) || nil(e))
                || children(&mut copy).into_iter().any(|b| body(b))
        })
    }
    body(&program.body)
}
fn first(e: &Expr) -> Option<Vec<Expr>> {
    if let Expr::Call(n, a) = e {
        if n == "@conditional" {
            return Some(a.clone());
        }
    }
    match e {
        Expr::Call(_, a) | Expr::Index(_, a) => a.iter().find_map(first),
        Expr::Bin(_, a, b) => first(a).or_else(|| first(b)),
        Expr::Un(_, a) => first(a),
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => [lower, upper, stride]
            .into_iter()
            .flatten()
            .find_map(|e| first(e)),
        _ => None,
    }
}
fn replace(e: &mut Expr, value: &Expr) -> bool {
    if choice(e) {
        *e = value.clone();
        return true;
    }
    match e {
        Expr::Call(_, a) | Expr::Index(_, a) => a.iter_mut().any(|e| replace(e, value)),
        Expr::Bin(_, a, b) => replace(a, value) || replace(b, value),
        Expr::Un(_, a) => replace(a, value),
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => [lower, upper, stride]
            .into_iter()
            .flatten()
            .any(|e| replace(e, value)),
        _ => false,
    }
}
// NIL is deliberately separate from the existing omitted-argument marker until
// its source context has been checked.
fn context(
    e: &mut Expr,
    actual: bool,
    attributes: &HashMap<String, HashSet<String>>,
) -> Result<()> {
    if nil(e) {
        bail!(".NIL. is only permitted as a consequent of a conditional argument");
    }
    if let Expr::Call(n, a) = e {
        if n == "@conditional" {
            let count = a.len();
            let mut attrs = None;
            for (i, item) in a.iter_mut().enumerate() {
                if i % 2 == 0 && i + 1 < count {
                    context(item, false, attributes)?;
                    continue;
                }
                if nil(item) {
                    if !actual {
                        bail!(".NIL. requires a conditional argument in a procedure reference");
                    }
                    *item = Expr::Call(crate::named_arguments::OMITTED.into(), vec![]);
                    continue;
                }
                if actual {
                    let flags = if let Expr::Ident(n) = item {
                        attributes.get(n).cloned().unwrap_or_default()
                    } else {
                        HashSet::new()
                    };
                    let flags = (flags.contains("allocatable"), flags.contains("pointer"));
                    if attrs.is_some_and(|previous| previous != flags) {
                        bail!("conditional argument consequents must agree in ALLOCATABLE and POINTER attributes");
                    }
                    attrs = Some(flags);
                }
                // A nested consequent may be an expression, but cannot itself
                // be a conditional argument with an absent consequent.
                context(item, false, attributes)?;
            }
            return Ok(());
        }
        if n.starts_with("@keyword:") {
            for e in a {
                context(e, actual, attributes)?;
            }
            return Ok(());
        }
        let procedure = procedure_name(n) && !attributes.contains_key(n);
        for item in a {
            context(item, procedure, attributes)?;
        }
        return Ok(());
    }
    match e {
        Expr::Index(_, a) => {
            for e in a {
                context(e, false, attributes)?;
            }
        }
        Expr::Bin(_, a, b) => {
            context(a, false, attributes)?;
            context(b, false, attributes)?;
        }
        Expr::Un(_, a) => context(a, false, attributes)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                context(e, false, attributes)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn expand(mut s: Stmt) -> Result<Vec<Stmt>> {
    // A WHILE condition must be recomputed after CYCLE as well as fallthrough.
    if let Stmt::DoWhile { cond, body } = &mut s {
        if present(cond) {
            let mut test = expand(Stmt::If {
                cond: Expr::not(cond.clone()),
                then_body: vec![Stmt::Exit],
                else_body: None,
            })?;
            test.append(body);
            return Ok(vec![Stmt::DoWhile {
                cond: Expr::Logical(true),
                body: test,
            }]);
        }
    }
    let found = expressions(&mut s).into_iter().find_map(|e| first(e));
    let Some(args) = found else {
        return Ok(vec![s]);
    };
    if matches!(
        s,
        Stmt::InitializedDecl { .. }
            | Stmt::ArrayDecl { .. }
            | Stmt::DerivedDecl { .. }
            | Stmt::VarDecl { .. }
    ) {
        bail!("conditional specification or initialization expression must currently be constant");
    }
    let mut yes = s.clone();
    for e in expressions(&mut yes) {
        if replace(e, &args[1]) {
            break;
        }
    }
    let tail = if args.len() == 3 {
        args[2].clone()
    } else {
        Expr::Call("@conditional".into(), args[2..].to_vec())
    };
    for e in expressions(&mut s) {
        if replace(e, &tail) {
            break;
        }
    }
    expand(Stmt::If {
        cond: args[0].clone(),
        then_body: expand(yes)?,
        else_body: Some(expand(s)?),
    })
}
// Check source-only contexts before resolving kinds or constant initializers.
// In particular, a discarded NIL in a type selector is still invalid Fortran.
pub(crate) fn validate_source(program: &Program) -> Result<()> {
    fn kind(kind: &crate::ast::TypeSpec) -> Result<()> {
        use crate::ast::TypeSpec;
        match kind {
            TypeSpec::CharacterLength(e) => {
                context(&mut e.as_ref().clone(), false, &HashMap::new())?
            }
            TypeSpec::IntegerKind(s)
            | TypeSpec::RealKind(s)
            | TypeSpec::LogicalKind(s)
            | TypeSpec::Complex(s) => {
                if let Some(json) = s.strip_prefix(crate::ast::KIND_EXPRESSION) {
                    let mut e = serde_json::from_str::<Expr>(json)?;
                    context(&mut e, false, &HashMap::new())?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(body: &[Stmt]) -> Result<()> {
        let mut location = None;
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                location = Some(*n);
            }
            (|| -> Result<()> {
                match s {
                    Stmt::VarDecl { kind: k, .. }
                    | Stmt::ArrayDecl { kind: k, .. }
                    | Stmt::InitializedDecl { kind: k, .. } => kind(k)?,
                    Stmt::Function {
                        return_type: Some(k),
                        ..
                    } => kind(k)?,
                    Stmt::DerivedType { components, .. } => body_fn(components)?,
                    _ => {}
                }
                let input_start = match s {
                    Stmt::Read { .. } => Some(0),
                    Stmt::CallSub { name, .. } => match name.as_str() {
                        "@internal_read" => Some(3),
                        "@formatted_read" => Some(2),
                        "@formatted_read_status" => Some(4),
                        _ => None,
                    },
                    _ => None,
                };
                let actual = matches!(s,Stmt::CallSub{name,..} if procedure_name(name));
                let mut copy = s.clone();
                for (index, e) in expressions(&mut copy).into_iter().enumerate() {
                    if input_start.is_some_and(|start| index >= start) && choice(e) {
                        bail!("a conditional expression is not a writable READ item");
                    }
                    context(e, actual, &HashMap::new())?;
                }
                for b in children(&mut copy) {
                    body_fn(b)?;
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
    fn body_fn(b: &[Stmt]) -> Result<()> {
        body(b)
    }
    body(&program.body)
}

pub fn resolve(mut p: Program) -> Result<Program> {
    if !contains(&p) {
        return Ok(p);
    }
    type Outputs = HashMap<String, Vec<(String, bool)>>;
    fn collect(b: &[Stmt], outputs: &mut Outputs) {
        for s in b {
            match s {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let out = params
                        .iter()
                        .map(|p| {
                            (
                                p.clone(),
                                body.iter().any(|s| match s {
                                    Stmt::DummyIntent { name, intent } => {
                                        name == p
                                            && matches!(
                                                intent.as_str(),
                                                "intent(out)" | "intent(inout)"
                                            )
                                    }
                                    Stmt::DerivedDecl {
                                        names, attributes, ..
                                    } => {
                                        names.contains(p)
                                            && attributes.iter().any(|a| {
                                                matches!(
                                                    a.as_str(),
                                                    "intent(out)" | "intent(inout)"
                                                )
                                            })
                                    }
                                    _ => false,
                                }),
                            )
                        })
                        .collect();
                    outputs.insert(name.clone(), out);
                    collect(body, outputs);
                }
                Stmt::Module { body, .. } => collect(body, outputs),
                _ => {}
            }
        }
    }
    fn args(
        name: &str,
        values: &[Expr],
        outputs: &Outputs,
        vars: &HashMap<String, HashSet<String>>,
    ) -> Result<()> {
        let Some(params) = outputs.get(name) else {
            return Ok(());
        };
        for (i, arg) in values.iter().enumerate() {
            let (slot, value) = if let Some((key, v)) = crate::named_arguments::keyword(arg) {
                (params.iter().position(|(n, _)| n == key).unwrap_or(i), v)
            } else {
                (i, arg)
            };
            if !params.get(slot).is_some_and(|(_, out)| *out) {
                continue;
            }
            if let Expr::Call(n, a) = value {
                if n == "@conditional" {
                    for (i, value) in a.iter().enumerate() {
                        if i % 2 == 0 && i + 1 < a.len()
                            || nil(value)
                            || crate::named_arguments::omitted(value)
                        {
                            continue;
                        }
                        let writable = match value {
                            Expr::Ident(_) | Expr::Index(_, _) => true,
                            Expr::Section { name, .. } => !name.is_empty(),
                            Expr::Call(n, _) => {
                                vars.contains_key(n) || n.starts_with("@record_component:")
                            }
                            _ => false,
                        };
                        if !writable {
                            bail!("every consequent of conditional argument `{}` to `{name}` must be writable",params[slot].0);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    let mut outputs = Outputs::new();
    collect(&p.body, &mut outputs);
    for (name, positions) in [
        ("random_number", vec![0]),
        ("split", vec![2]),
        ("cpu_time", vec![0]),
        ("system_clock", vec![0, 1, 2]),
        ("date_and_time", vec![0, 1, 2, 3]),
    ] {
        if let Some(sig) = crate::named_arguments::intrinsic(name) {
            outputs.entry(name.into()).or_insert_with(|| {
                sig.params
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| (n, positions.contains(&i)))
                    .collect()
            });
        }
    }
    p = crate::output::prepare_formatted(p)?;
    fn lower_body(
        body: &mut Vec<Stmt>,
        inherited: &HashMap<String, HashSet<String>>,
        inherited_symbols: &HashMap<String, (crate::ast::TypeSpec, Vec<Expr>)>,
        serial: &mut usize,
        outputs: &Outputs,
    ) -> Result<()> {
        let mut symbols = inherited_symbols.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (n, _) in entities {
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                _ => {}
            }
        }
        let mut attributes = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        attributes.insert(n.clone(), HashSet::new());
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    attributes.insert(name.clone(), HashSet::new());
                }
                Stmt::InitializedDecl {
                    entities,
                    attributes: a,
                    ..
                } => {
                    for (n, _) in entities {
                        attributes.insert(n.clone(), a.iter().cloned().collect());
                    }
                }
                Stmt::DerivedDecl {
                    names,
                    attributes: a,
                    ..
                } => {
                    for n in names {
                        attributes.insert(n.clone(), a.iter().cloned().collect());
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            if let Stmt::DummyIntent { name, intent } = s {
                attributes
                    .entry(name.clone())
                    .or_default()
                    .insert(intent.clone());
            }
        }
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(n) = s {
                location = Some(n);
                out.push(s);
                continue;
            }
            let result = (|| -> Result<Vec<Stmt>> {
                if let Stmt::Print { items } = &s {
                    if items.iter().any(crate::output::contains_implied_do) {
                        s = crate::output::lower(items, &symbols, serial)?;
                    }
                }
                let has_choice = expressions(&mut s).into_iter().any(|e| present(e));
                if has_choice {
                    let (mut before, after) = crate::constructor_expressions::conditional_items(
                        &mut s, &symbols, serial,
                    )?;
                    if !before.is_empty() || !after.is_empty() {
                        before.push(s);
                        before.extend(after);
                        lower_body(&mut before, &attributes, &symbols, serial, outputs)?;
                        return Ok(vec![Stmt::Block { body: before }]);
                    }
                }
                for b in children(&mut s) {
                    lower_body(b, &attributes, &symbols, serial, outputs)?;
                }
                if let Stmt::CallSub { name, args: values } = &s {
                    args(name, values, outputs, &attributes)?;
                }
                for e in expressions(&mut s) {
                    let mut result = Ok(());
                    crate::procedure_arguments::visit_expr(e, &mut |e| {
                        if let Expr::Call(n, a) = e {
                            if result.is_ok() {
                                result = args(n, a, outputs, &attributes);
                            }
                        }
                    });
                    result?;
                }
                let actual = matches!(&s,Stmt::CallSub{name,..} if procedure_name(name));
                for e in expressions(&mut s) {
                    context(e, actual, &attributes)?;
                }
                let mut prefix = vec![];
                if let Stmt::ArrayDecl { dims, .. } = &mut s {
                    for dim in dims {
                        if present(dim) {
                            *serial += 1;
                            let name = format!("@conditional_bound_{serial}");
                            prefix.push(Stmt::VarDecl {
                                kind: crate::ast::TypeSpec::Integer(Some(8)),
                                names: vec![name.clone()],
                            });
                            prefix.extend(expand(Stmt::Assign {
                                name: name.clone(),
                                value: dim.clone(),
                            })?);
                            *dim = Expr::Ident(name);
                        }
                    }
                }
                prefix.extend(expand(s)?);
                Ok(prefix)
            })();
            out.extend(result.map_err(|e| crate::source_diagnostics::at(e, location))?);
        }
        *body = out;
        Ok(())
    }
    fn function_types(
        body: &[Stmt],
        symbols: &mut HashMap<String, (crate::ast::TypeSpec, Vec<Expr>)>,
    ) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    if let Some(kind) = crate::elemental::result_kind(name, return_type, body) {
                        symbols.insert(format!("@constructor_function:{name}"), (kind, vec![]));
                    }
                    function_types(body, symbols);
                }
                Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    function_types(body, symbols)
                }
                _ => {}
            }
        }
    }
    let mut symbols = HashMap::new();
    function_types(&p.body, &mut symbols);
    lower_body(
        &mut p.body,
        &HashMap::new(),
        &symbols,
        &mut 1000000,
        &outputs,
    )?;
    Ok(p)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Use the existing owned allocation descriptor for intrinsic scalar storage.
//! Only allocation operations see the backing array; value expressions remain
//! scalar element expressions, preserving their type, kind and rank.
use crate::ast::{Expr, Stmt, TypeSpec};
use anyhow::{ensure, Result};
use std::collections::{HashMap, HashSet};
#[derive(Default)]
struct Procedures {
    names: HashSet<String>,
    descriptors: HashMap<String, HashSet<usize>>,
    serial: std::cell::Cell<usize>,
}
impl Procedures {
    fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}
type Symbols = HashMap<String, (TypeSpec, usize)>;
fn value_kind(name: &str, symbols: &Symbols) -> TypeSpec {
    match &symbols[name].0 {
        TypeSpec::CharacterAssumed => TypeSpec::CharacterLength(Box::new(Expr::Call(
            "len".into(),
            vec![Expr::Ident(name.into())],
        ))),
        kind => kind.clone(),
    }
}
fn element(name: &str) -> Expr {
    Expr::Call(name.into(), vec![Expr::IntLit("1".into())])
}
fn declared_length(kind: &TypeSpec, constants: &HashMap<String, Expr>) -> Option<usize> {
    match kind {
        TypeSpec::Character(n) => Some(n.unwrap_or(1)),
        TypeSpec::CharacterLength(e) => crate::parameters::constant_integer_with(e, constants)
            .ok()
            .map(|n| n.max(0) as usize),
        _ => None,
    }
}
fn character_length(
    e: &Expr,
    symbols: &Symbols,
    constants: &HashMap<String, Expr>,
) -> Option<usize> {
    match e {
        Expr::Str(s) => Some(s.len()),
        Expr::Ident(n) => declared_length(&symbols.get(n)?.0, constants),
        Expr::Bin(crate::ast::BinOp::Concat, a, b) => character_length(a, symbols, constants)?
            .checked_add(character_length(b, symbols, constants)?),
        _ => None,
    }
}
fn fold_models(
    e: &mut Expr,
    scalars: &HashSet<String>,
    symbols: &Symbols,
    types: &crate::operators::ExpressionTypes,
    users: &Procedures,
) -> Result<()> {
    if let Expr::Call(name, args) = e {
        if !users.contains(name)
            && args.len() == 1
            && (crate::inquiries::numeric_model(name) || matches!(name.as_str(), "huge" | "tiny"))
        {
            let mut uses_scalar = false;
            crate::procedure_arguments::visit_expr(&mut args[0].clone(), &mut |e| {
                if matches!(e,Expr::Ident(n) if scalars.contains(n)) {
                    uses_scalar = true;
                }
            });
            if uses_scalar {
                if let Some((kind, _)) = types.kind_rank(&args[0], symbols)? {
                    *e = crate::inquiries::model_value(name, &kind)?;
                    return Ok(());
                }
            }
        }
    }
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                fold_models(e, scalars, symbols, types, users)?;
            }
        }
        Expr::Bin(_, a, b) => {
            fold_models(a, scalars, symbols, types, users)?;
            fold_models(b, scalars, symbols, types, users)?;
        }
        Expr::Un(_, a) => fold_models(a, scalars, symbols, types, users)?,
        _ => {}
    }
    Ok(())
}
fn value(e: &mut Expr, scalars: &HashSet<String>, users: &Procedures) {
    match e {
        Expr::Ident(n) if scalars.contains(n) => *e = element(n),
        Expr::Call(n, args)
            if !users.contains(n)
                && (n == "allocated"
                    || n == "present"
                    || n == "kind"
                    || n == "len"
                    || crate::inquiries::numeric_model(n)
                    || matches!(n.as_str(), "huge" | "tiny")) => {}
        Expr::Call(name, args) => call_values(name, args, scalars, users),
        Expr::Index(_, args) => {
            for a in args {
                value(a, scalars, users);
            }
        }
        Expr::Bin(_, a, b) => {
            value(a, scalars, users);
            value(b, scalars, users);
        }
        Expr::Un(_, a) => value(a, scalars, users),
        Expr::Section { name, .. } if scalars.contains(name) => {
            let n = name.clone();
            let mut range = e.clone();
            if let Expr::Section {
                name,
                lower,
                upper,
                stride,
            } = &mut range
            {
                name.clear();
                for bound in [lower, upper, stride].into_iter().flatten() {
                    value(bound, scalars, users);
                }
            }
            *e = Expr::Call("@literal_substring".into(), vec![element(&n), range]);
        }
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for a in [lower, upper, stride].into_iter().flatten() {
                value(a, scalars, users);
            }
        }
        _ => {}
    }
}
// ALLOCATABLE dummies receive the descriptor, ordinary scalar dummies
// receive its element. Keep the source signature until after generic dispatch.
fn call_values(name: &str, args: &mut [Expr], scalars: &HashSet<String>, users: &Procedures) {
    for (i, arg) in args.iter_mut().enumerate() {
        if !users.descriptors.get(name).is_some_and(|p| p.contains(&i)) {
            value(arg, scalars, users);
        }
    }
}
fn check_call(
    name: &str,
    args: &[Expr],
    scalars: &HashSet<String>,
    users: &Procedures,
) -> Result<()> {
    if let Some(positions) = users.descriptors.get(name) {
        for &i in positions {
            if let Some(arg) = args.get(i) {
                ensure!(crate::named_arguments::omitted(arg) || matches!(arg, Expr::Ident(n) if scalars.contains(n)),
                    "ALLOCATABLE scalar dummy argument {} of `{name}` requires an ALLOCATABLE scalar actual", i + 1);
            }
        }
    }
    Ok(())
}
pub(crate) fn resolve(mut program: crate::ast::Program) -> Result<crate::ast::Program> {
    fn needed(statements: &mut [Stmt]) -> bool {
        let mut candidates = HashSet::new();
        for s in statements.iter() {
            match s {
                Stmt::VarDecl { kind, names } if !crate::substrings::deferred(kind) => {
                    candidates.extend(names.iter().cloned())
                }
                Stmt::InitializedDecl { kind, entities, .. }
                    if !crate::substrings::deferred(kind) =>
                {
                    candidates.extend(entities.iter().map(|(n, _)| n.clone()))
                }
                _ => {}
            }
        }
        for s in statements.iter() {
            if let Stmt::ArrayDecl { name, .. } = s {
                candidates.remove(name);
            }
        }
        if statements.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if intent=="allocatable" && candidates.contains(name))) {return true;}
        for s in statements {
            for b in crate::procedure_arguments::children(s) {
                if needed(b) {
                    return true;
                }
            }
        }
        false
    }
    program = crate::allocatable_scalar_results::resolve(program)?;
    if !needed(&mut program.body) {
        return Ok(program);
    }
    fn body(
        statements: &mut Vec<Stmt>,
        inherited: &HashSet<String>,
        inherited_types: &Symbols,
        types: &crate::operators::ExpressionTypes,
        users: &Procedures,
    ) -> Result<()> {
        let mut symbols = inherited_types.clone();
        let constants = statements
            .iter()
            .filter_map(|s| {
                if let Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } = s
                {
                    attributes
                        .iter()
                        .any(|a| a == "parameter")
                        .then_some(entities)
                } else {
                    None
                }
            })
            .flatten()
            .filter_map(|(n, e)| e.as_ref().map(|e| (n.clone(), e.clone())))
            .collect::<HashMap<_, _>>();
        let concrete = |kind: TypeSpec| -> Result<TypeSpec> {
            let kind = crate::parameters::concrete_kind_with(kind, &constants)?;
            if let TypeSpec::CharacterLength(e) = &kind {
                if let Ok(n) = crate::parameters::constant_integer_with(e, &constants) {
                    return Ok(TypeSpec::Character(Some(n.max(0) as usize)));
                }
            }
            Ok(kind)
        };
        for s in statements.iter() {
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
        let mut scalars = inherited.clone();
        let mut local = HashSet::new();
        let arrays = statements
            .iter()
            .filter_map(|s| {
                if let Stmt::ArrayDecl { name, .. } = s {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>();
        for stmt in statements.iter() {
            let names = match stmt {
                Stmt::VarDecl { names, .. } => names.clone(),
                Stmt::InitializedDecl { entities, .. } => {
                    entities.iter().map(|(n, _)| n.clone()).collect()
                }
                Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                _ => vec![],
            };
            for n in names {
                scalars.remove(&n);
            }
        }
        for stmt in statements.iter() {
            if let Stmt::DummyIntent { name, intent } = stmt {
                if intent == "allocatable" && !arrays.contains(name) {
                    let kind = statements.iter().find_map(|s| match s {
                        Stmt::VarDecl { kind, names } if names.contains(name) => Some(kind),
                        Stmt::InitializedDecl { kind, entities, .. }
                            if entities.iter().any(|(n, _)| n == name) =>
                        {
                            Some(kind)
                        }
                        _ => None,
                    });
                    // Deferred CHARACTER scalars already have their own length-aware lowering.
                    if kind.is_some_and(|k| !crate::substrings::deferred(k)) {
                        local.insert(name.clone());
                        scalars.insert(name.clone());
                    }
                }
            }
        }
        let mut output = vec![];
        let mut offset = None;
        for mut stmt in std::mem::take(statements) {
            if let Stmt::SourceLocation(n) = stmt {
                offset = Some(n);
                output.push(stmt);
                continue;
            }
            let result = (|| -> Result<()> {
                if let Stmt::CallSub { name, args } = &stmt {
                    check_call(name, args, &scalars, users)?;
                }
                for e in crate::procedure_arguments::expressions(&mut stmt) {
                    let mut checked = Ok(());
                    crate::procedure_arguments::visit_expr(e, &mut |expr| {
                        if checked.is_ok() {
                            if let Expr::Call(name, args) = expr {
                                checked = check_call(name, args, &scalars, users);
                            }
                        }
                    });
                    checked?;
                    fold_models(e, &scalars, &symbols, types, users)?;
                }
                match &mut stmt {
                    Stmt::VarDecl { kind, names } => {
                        for n in names.iter().filter(|n| local.contains(*n)) {
                            output.push(Stmt::DummyIntent {
                                name: n.clone(),
                                intent: "@scalar_heap".into(),
                            });
                            output.push(Stmt::ArrayDecl {
                                kind: kind.clone(),
                                name: n.clone(),
                                dims: vec![Expr::Ident("@deferred_extent".into())],
                            });
                        }
                        names.retain(|n| !local.contains(n));
                    }
                    Stmt::InitializedDecl { entities, .. }
                        if entities.iter().any(|(n, _)| local.contains(n)) =>
                    {
                        ensure!(false,"SAVE or declaration initialization of ALLOCATABLE scalars is not yet supported");
                    }
                    Stmt::Assign { name, value: rhs } if scalars.contains(name) => {
                        if let Some((_, rank)) = types.kind_rank(rhs, &symbols)? {
                            ensure!(
                                rank == 0,
                                "assignment to ALLOCATABLE scalar `{name}` requires a scalar value"
                            );
                        }
                        value(rhs, &scalars, users);
                        if !matches!(&symbols[name].0, TypeSpec::CharacterAssumed) {
                            // Evaluate and convert the scalar before allocation;
                            // no one-element constructor or vector lowering is needed.
                            let serial = users.serial.get() + 1;
                            users.serial.set(serial);
                            let temp = format!("@allocatable_scalar_assignment_{serial}");
                            output.push(Stmt::VarDecl {
                                kind: symbols[name].0.clone(),
                                names: vec![temp.clone()],
                            });
                            output.push(Stmt::Assign {
                                name: temp.clone(),
                                value: rhs.clone(),
                            });
                            let target = name.clone();
                            output.push(Stmt::If {
                                cond: Expr::Un(
                                    crate::ast::UnOp::Not,
                                    Box::new(Expr::Call(
                                        "allocated".into(),
                                        vec![Expr::Ident(target.clone())],
                                    )),
                                ),
                                then_body: vec![Stmt::CallSub {
                                    name: "@allocate".into(),
                                    args: vec![element(&target)],
                                }],
                                else_body: None,
                            });
                            stmt = Stmt::AssignIndex {
                                name: target,
                                indices: vec![Expr::IntLit("1".into())],
                                value: Expr::Ident(temp),
                            };
                            return Ok(());
                        }
                        *rhs = Expr::Call("@array_constructor".into(), vec![rhs.clone()]);
                    }
                    Stmt::AssignIndex {
                        name,
                        indices,
                        value: rhs,
                    } if scalars.contains(name)
                        && indices.len() == 1
                        && matches!(indices.first(), Some(Expr::Section { .. })) =>
                    {
                        value(rhs, &scalars, users);
                        let mut range = indices[0].clone();
                        if let Expr::Section {
                            name,
                            lower,
                            upper,
                            stride,
                        } = &mut range
                        {
                            name.clear();
                            for e in [lower, upper, stride].into_iter().flatten() {
                                value(e, &scalars, users);
                            }
                        }
                        let temp = format!("@allocatable_scalar_substring_{}", output.len());
                        stmt = Stmt::Block {
                            body: vec![
                                Stmt::VarDecl {
                                    kind: value_kind(name, &symbols),
                                    names: vec![temp.clone()],
                                },
                                Stmt::Assign {
                                    name: temp.clone(),
                                    value: element(name),
                                },
                                Stmt::AssignIndex {
                                    name: temp.clone(),
                                    indices: vec![range],
                                    value: rhs.clone(),
                                },
                                Stmt::AssignIndex {
                                    name: name.clone(),
                                    indices: vec![Expr::IntLit("1".into())],
                                    value: Expr::Ident(temp),
                                },
                            ],
                        };
                        return Ok(());
                    }
                    Stmt::CallSub { name, args } if name == "@allocate" => {
                        let targets = args
                            .iter()
                            .filter_map(|e| match e {
                                Expr::Ident(n) if scalars.contains(n) => Some(n.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        for e in args.iter() {
                            if let Expr::Call(k, a) = e {
                                if matches!(k.as_str(), "@keyword:source" | "@keyword:mold")
                                    && !targets.is_empty()
                                {
                                    if let Some((source_kind, rank)) =
                                        types.kind_rank(&a[0], &symbols)?
                                    {
                                        ensure!(rank==0,"SOURCE= and MOLD= for an ALLOCATABLE scalar require a scalar expression");
                                        for n in &targets {
                                            if let (Ok(target), Ok(source)) = (
                                                concrete(symbols[n].0.clone()),
                                                concrete(source_kind.clone()),
                                            ) {
                                                // Generic resolution ignores CHARACTER length;
                                                // consult the original expression instead.
                                                if matches!(
                                                    (&target, &source),
                                                    (
                                                        TypeSpec::Character(_)
                                                            | TypeSpec::CharacterLength(_),
                                                        TypeSpec::Character(_)
                                                            | TypeSpec::CharacterLength(_)
                                                    )
                                                ) {
                                                    if let (Some(a_len), Some(b_len)) = (
                                                        declared_length(&target, &constants),
                                                        character_length(
                                                            &a[0], &symbols, &constants,
                                                        ),
                                                    ) {
                                                        ensure!(a_len==b_len,"SOURCE= and MOLD= must have matching CHARACTER lengths");
                                                    }
                                                    continue;
                                                }
                                                ensure!(target==source,"SOURCE= and MOLD= must have the same type and kind as ALLOCATABLE scalar `{n}`");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let targeted = args
                            .iter()
                            .any(|e| matches!(e,Expr::Ident(n) if scalars.contains(n)));
                        // SOURCE is evaluated once, before changing allocation
                        // status, including when several objects share it.
                        if targeted {
                            if let Some(Expr::Call(_, values)) = args
                                .iter_mut()
                                .find(|e| matches!(e,Expr::Call(n,_) if n=="@keyword:source"))
                            {
                                let temp = format!("@allocatable_scalar_source_{}", output.len());
                                let mut source = values[0].clone();
                                value(&mut source, &scalars, users);
                                output.push(Stmt::VarDecl {
                                    kind: value_kind(&targets[0], &symbols),
                                    names: vec![temp.clone()],
                                });
                                output.push(Stmt::Assign {
                                    name: temp.clone(),
                                    value: source,
                                });
                                values[0] = Expr::Ident(temp);
                            }
                        }
                        for a in args.iter_mut() {
                            match a {
                                Expr::Ident(n) if scalars.contains(n) => *a = element(n),
                                Expr::Call(n, _) if scalars.contains(n) => anyhow::bail!(
                                    "ALLOCATE of scalar `{n}` must not specify array bounds"
                                ),
                                Expr::Call(n, values) if n.starts_with("@keyword:") => {
                                    for e in values {
                                        value(e, &scalars, users);
                                    }
                                }
                                _ => value(a, &scalars, users),
                            }
                        }
                        if targeted {
                            let mut ranks = std::collections::HashMap::new();
                            for n in &scalars {
                                ranks.insert(n.clone(), 1);
                            }
                            for s in &output {
                                if let Stmt::ArrayDecl { name, dims, .. } = s {
                                    ranks.insert(name.clone(), dims.len());
                                }
                            }
                            if let Some(expanded) =
                                crate::allocation_options::allocate(args, &ranks)?
                            {
                                stmt = Stmt::Block { body: expanded };
                                return Ok(());
                            }
                        }
                    }
                    Stmt::Do { var, .. } if scalars.contains(var) => {
                        anyhow::bail!("ALLOCATABLE scalar DO variables are not yet supported")
                    }
                    Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => {
                        let actuals = args
                            .iter()
                            .filter_map(|e| match e {
                                Expr::Ident(n) => Some(n),
                                _ => None,
                            })
                            .take(2)
                            .collect::<Vec<_>>();
                        if actuals.len() == 2 && actuals.iter().any(|n| scalars.contains(*n)) {
                            ensure!(
                                actuals
                                    .iter()
                                    .all(|n| symbols.get(*n).is_some_and(|(_, rank)| *rank == 0)),
                                "MOVE_ALLOC arguments must have the same rank"
                            );
                            let ranks = actuals.iter().map(|n| ((*n).clone(), 1)).collect();
                            stmt = Stmt::Block {
                                body: crate::allocation_options::move_alloc(args, &ranks)?,
                            };
                            return Ok(());
                        }
                    }
                    Stmt::CallSub { name, .. }
                        if name == "@deallocate"
                            || name.eq_ignore_ascii_case("move_alloc")
                            || name == "@module_bind"
                            || name == "@scalar_character_from_codes" => {}
                    Stmt::CallSub { name, args } if users.contains(name) => {
                        call_values(name, args, &scalars, users);
                    }
                    Stmt::Function { body: b, .. } | Stmt::Subroutine { body: b, .. } => {
                        body(b, &scalars, &symbols, types, users)?;
                        return Ok(());
                    }
                    Stmt::Module { body: b, .. } => {
                        body(b, inherited, inherited_types, types, users)?;
                        return Ok(());
                    }
                    _ => {
                        for e in crate::procedure_arguments::expressions(&mut stmt) {
                            value(e, &scalars, users);
                        }
                    }
                }
                for b in crate::procedure_arguments::children(&mut stmt) {
                    body(b, &scalars, &symbols, types, users)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
            output.push(stmt);
        }
        *statements = output;
        Ok(())
    }
    fn collect_users(statements: &mut [Stmt], users: &mut Procedures) {
        for s in statements {
            if let Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } = s
            {
                users.names.insert(name.clone());
                let positions = params.iter().enumerate().filter_map(|(i, p)| {
                    let scalar = body.iter().any(|s| matches!(s, Stmt::VarDecl { kind, names } if names.contains(p) && !crate::substrings::deferred(kind)))
                        && !body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == p));
                    ((scalar && body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == p && intent == "allocatable")))
                        || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p && intent=="@scalar_result_value_dummy"))).then_some(i)
                }).collect();
                users.descriptors.insert(name.clone(), positions);
            }
            for child in crate::procedure_arguments::children(s) {
                collect_users(child, users);
            }
        }
    }
    let mut users = Procedures::default();
    collect_users(&mut program.body, &mut users);
    let types = crate::operators::ExpressionTypes::new(&program)?;
    // USE association has assigned module objects unique names. Their original
    // scalar declarations still live in MODULE bodies; make that metadata
    // available in every scope containing a USE-associated reference.
    let mut globals = Symbols::new();
    let mut global_scalars = HashSet::new();
    for stmt in &program.body {
        if let Stmt::Module { body, .. } = stmt {
            for s in body {
                match s {
                    Stmt::VarDecl { kind, names } => {
                        for n in names {
                            globals.insert(n.clone(), (kind.clone(), 0));
                        }
                    }
                    Stmt::InitializedDecl { kind, entities, .. } => {
                        for (n, _) in entities {
                            globals.insert(n.clone(), (kind.clone(), 0));
                        }
                    }
                    Stmt::ArrayDecl { kind, name, dims } => {
                        globals.insert(name.clone(), (kind.clone(), dims.len()));
                    }
                    _ => {}
                }
            }
            for s in body {
                if let Stmt::DummyIntent { name, intent } = s {
                    if intent == "allocatable"
                        && globals
                            .get(name)
                            .is_some_and(|(k, r)| *r == 0 && !crate::substrings::deferred(k))
                    {
                        global_scalars.insert(name.clone());
                    }
                }
            }
        }
    }
    body(&mut program.body, &global_scalars, &globals, &types, &users)?;
    Ok(program)
}

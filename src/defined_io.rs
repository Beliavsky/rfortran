// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Initial type-bound defined formatted output: scalar objects on output unit 6.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn call(n: &str, args: Vec<Expr>) -> Stmt {
    Stmt::CallSub {
        name: n.into(),
        args,
    }
}
fn decl(n: &str, kind: TypeSpec) -> Stmt {
    Stmt::VarDecl {
        kind,
        names: vec![n.into()],
    }
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}

fn collect(
    body: &mut [Stmt],
    bindings: &mut HashMap<String, String>,
    procedures: &mut HashMap<String, Stmt>,
) -> Result<()> {
    for stmt in body {
        match stmt {
            Stmt::DerivedType {
                name,
                bindings: entries,
                ..
            } => {
                let private_default = entries
                    .iter()
                    .any(|(key, value)| key == "@binding_default_access" && value == "@private");
                for (key, target) in entries.iter() {
                    if key.starts_with("@defined_io:") {
                        if private_default
                            && !entries.iter().any(|(key, value)| {
                                key == "@defined_io_access:write:formatted" && value == "@public"
                            })
                        {
                            bail!("private defined I/O bindings are not yet supported");
                        }
                        if key != "@defined_io:write:formatted" {
                            bail!("defined I/O currently supports WRITE(FORMATTED) only");
                        }
                        if bindings.insert(name.clone(), target.clone()).is_some() {
                            bail!("multiple defined formatted output bindings for type `{name}` are not yet supported");
                        }
                    }
                }
            }
            Stmt::Subroutine { name, .. } | Stmt::Function { name, .. } => {
                procedures.insert(name.clone(), stmt.clone());
            }
            _ => {}
        }
        for child in crate::procedure_arguments::children(stmt) {
            collect(child, bindings, procedures)?;
        }
    }
    Ok(())
}
fn validate(ty: &str, target: &str, procedure: &Stmt) -> Result<()> {
    let Stmt::Subroutine { params, body, .. } = procedure else {
        bail!("defined formatted output `{target}` must be a subroutine");
    };
    if params.len() != 6 {
        bail!("defined formatted output `{target}` requires six dummy arguments: DTV, UNIT, IOTYPE, V_LIST, IOSTAT, IOMSG");
    }
    let mut kinds = HashMap::new();
    let mut intents = HashMap::new();
    let mut derived = None;
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    kinds.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                kinds.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                dims,
                polymorphic,
            } if names.contains(&params[0]) => {
                derived = Some((type_name, dims, *polymorphic));
                for a in attributes {
                    if matches!(a.as_str(), "optional" | "allocatable" | "pointer" | "value") {
                        bail!("defined I/O DTV cannot have {a}");
                    }
                    if a.starts_with("intent(") {
                        intents.insert(params[0].clone(), a.clone());
                    }
                }
            }
            Stmt::DummyIntent { name, intent } if params.contains(name) => {
                if matches!(
                    intent.as_str(),
                    "optional" | "allocatable" | "pointer" | "value"
                ) {
                    bail!("defined I/O dummy `{name}` cannot have {intent}");
                }
                if intent.starts_with("intent(") {
                    intents.insert(name.clone(), intent.clone());
                }
            }
            _ => {}
        }
    }
    if !derived.is_some_and(|(t, d, poly)| t == ty && d.is_empty() && poly) {
        bail!("defined formatted output DTV must be a scalar CLASS({ty}) dummy");
    }
    for (i, required) in [
        "intent(in)",
        "intent(in)",
        "intent(in)",
        "intent(in)",
        "intent(out)",
        "intent(inout)",
    ]
    .iter()
    .enumerate()
    {
        if intents.get(&params[i]).map(String::as_str) != Some(*required) {
            bail!(
                "defined formatted output dummy `{}` requires {required}",
                params[i]
            );
        }
    }
    for i in [1, 4] {
        if !matches!(kinds.get(&params[i]),Some((TypeSpec::Integer(None|Some(4)),d)) if d.is_empty())
        {
            bail!(
                "defined formatted output dummy `{}` must be default INTEGER scalar",
                params[i]
            );
        }
    }
    for i in [2, 5] {
        if !matches!(kinds.get(&params[i]),Some((TypeSpec::CharacterAssumed,d)) if d.is_empty()) {
            bail!(
                "defined formatted output dummy `{}` must be CHARACTER(len=*) scalar",
                params[i]
            );
        }
    }
    if !matches!(kinds.get(&params[3]),Some((TypeSpec::Integer(None|Some(4)),d)) if d.len()==1 && matches!(&d[0],Expr::Ident(n) if n=="@deferred_extent"))
    {
        bail!("defined formatted output V_LIST must be an assumed-shape default INTEGER vector");
    }
    Ok(())
}

/// Accept a single DT descriptor, optionally with its character string and v-list.
/// More general parent formats need resumable format traversal, not splitting strings.
fn dt(format: &Expr) -> Result<(String, Vec<Expr>)> {
    let Expr::Str(format) = format else {
        bail!(
            "defined formatted output requires a literal single-DT format or list-directed output"
        );
    };
    let mut text = format
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .context("defined output requires a single DT descriptor")?
        .trim();
    if !text
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("dt"))
    {
        bail!("defined output currently requires a single DT descriptor");
    }
    text = text[2..].trim();
    let mut iotype = String::from("DT");
    if let Some(quote) = text.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        let mut chars = text.char_indices().skip(1).peekable();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            if c == quote {
                if chars.peek().is_some_and(|(_, next)| *next == quote) {
                    chars.next();
                    iotype.push(quote);
                } else {
                    end = Some(i + 1);
                    break;
                }
            } else {
                iotype.push(c);
            }
        }
        text = text[end.context("unterminated DT character string")?..].trim();
    }
    let mut values = vec![];
    if !text.is_empty() {
        let list = text
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .context("defined output currently requires a single DT descriptor")?;
        if !list.trim().is_empty() {
            for value in list.split(',') {
                values.push(int(value
                    .trim()
                    .parse::<i32>()
                    .context("DT v-list requires default INTEGER literals")?
                    as i64));
            }
        }
    }
    Ok((iotype, values))
}

type Objects = HashMap<String, (String, bool)>;
type Components = HashMap<(String, String), (String, bool)>;
fn lower(
    body: &mut Vec<Stmt>,
    inherited: &Objects,
    bindings: &HashMap<String, String>,
    components: &Components,
    child_routines: &HashSet<String>,
    child: bool,
    serial: &mut usize,
) -> Result<()> {
    let mut objects = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    objects.remove(n);
                }
            }
            Stmt::ArrayDecl { name, .. } => {
                objects.remove(name);
            }
            Stmt::DerivedDecl {
                type_name,
                names,
                dims,
                ..
            } => {
                for n in names {
                    objects.insert(n.clone(), (type_name.clone(), !dims.is_empty()));
                }
            }
            _ => {}
        }
    }
    let mut offset = None;
    for s in body.iter_mut() {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
        }
        let result = (|| -> Result<()> {
            let selected = match s {
                Stmt::Print { items } => Some((int(6), None, items.clone())),
                Stmt::CallSub { name, args } if name == "@write" => {
                    Some((args[0].clone(), Some(args[1].clone()), args[2..].to_vec()))
                }
                Stmt::CallSub { name, args } if name == "@formatted_print" => {
                    Some((int(6), Some(args[0].clone()), args[1..].to_vec()))
                }
                _ => None,
            };
            if let Some((unit, format, mut items)) = selected {
                let target = |e: &Expr| match e {
                    Expr::Ident(n) => {
                        let mut names = n.split('%');
                        let (mut ty, mut array) = objects.get(names.next().unwrap())?.clone();
                        for field in names {
                            let value = components.get(&(ty, field.into()))?;
                            ty = value.0.clone();
                            array = true;
                        }
                        bindings.get(&ty).map(|p| (p.clone(), array))
                    }
                    Expr::Section { name: n, .. } => objects
                        .get(n)
                        .and_then(|(ty, _)| bindings.get(ty))
                        .map(|p| (p.clone(), true)),
                    Expr::Call(n, args)
                        if n == "@array_constructor" || n == "@typed_constructor" =>
                    {
                        fn constructor_target(
                            e: &Expr,
                            objects: &Objects,
                            bindings: &HashMap<String, String>,
                        ) -> Option<String> {
                            match e {
                                Expr::Ident(n) | Expr::Section { name: n, .. } => {
                                    objects.get(n).and_then(|(ty, _)| bindings.get(ty)).cloned()
                                }
                                Expr::Call(n, args) | Expr::Index(n, args) => bindings
                                    .get(n)
                                    .or_else(|| objects.get(n).and_then(|(ty, _)| bindings.get(ty)))
                                    .cloned()
                                    .or_else(|| {
                                        args.iter()
                                            .find_map(|e| constructor_target(e, objects, bindings))
                                    }),
                                _ => None,
                            }
                        }
                        args.iter()
                            .find_map(|e| constructor_target(e, &objects, bindings))
                            .map(|p| (p, true))
                    }
                    Expr::Call(n, _) | Expr::Index(n, _) => bindings
                        .get(n)
                        .or_else(|| objects.get(n).and_then(|(ty, _)| bindings.get(ty)))
                        .map(|p| (p.clone(), true)),
                    _ => None,
                };
                if items.iter().any(|e| target(e).is_some()) {
                    if crate::parameters::integer_constant(&unit).ok() != Some(6) {
                        bail!("defined formatted output currently requires standard output (UNIT=* or 6)");
                    }
                    let mut status_target = None;
                    let mut message_target = None;
                    while matches!(items.first(),Some(Expr::Call(n,_)) if n.starts_with("@write_option:"))
                    {
                        let Expr::Call(n, a) = items.remove(0) else {
                            unreachable!()
                        };
                        let Some(Expr::Ident(dest)) = a.first() else {
                            bail!("defined I/O parent status controls require scalar variables");
                        };
                        match n.as_str() {
                            "@write_option:iostat" => status_target = Some(dest.clone()),
                            "@write_option:iomsg" => message_target = Some(dest.clone()),
                            _ => bail!(
                                "defined I/O parent WRITE currently supports IOSTAT and IOMSG only"
                            ),
                        }
                    }
                    let list = format
                        .as_ref()
                        .is_none_or(|e| matches!(e,Expr::Call(n,_) if n=="@list_directed"));
                    let (iotype, values) = if list {
                        ("LISTDIRECTED".into(), vec![])
                    } else {
                        if items.len() != 1 {
                            bail!("explicit defined output currently requires one scalar object and one DT descriptor");
                        }
                        dt(format.as_ref().unwrap())?
                    };
                    *serial += 1;
                    let prefix = format!("@defined_output_{serial}");
                    let ios = format!("{prefix}_status");
                    let msg = format!("{prefix}_message");
                    let vlist = format!("{prefix}_vlist");
                    let mut out = vec![
                        decl(&ios, TypeSpec::Integer(None)),
                        decl(&msg, TypeSpec::Character(Some(256))),
                        Stmt::ArrayDecl {
                            name: vlist.clone(),
                            kind: TypeSpec::Integer(None),
                            dims: vec![int(values.len() as i64)],
                        },
                    ];
                    out.push(assign(&ios, int(0)));
                    for (i, v) in values.iter().enumerate() {
                        out.push(Stmt::AssignIndex {
                            name: vlist.clone(),
                            indices: vec![int(i as i64 + 1)],
                            value: v.clone(),
                        });
                    }
                    for item in items {
                        let start = out.len();
                        if let Some((procedure, array)) = target(&item) {
                            if array {
                                bail!("defined formatted output currently requires scalar derived variables");
                            }
                            if list {
                                out.push(call(
                                    "@formatted_write_noadvance",
                                    vec![Expr::Str("(1x)".into())],
                                ));
                            }
                            out.extend([
                                assign(&msg, Expr::Str(String::new())),
                                call("@defined_output_enter", vec![]),
                                call(
                                    &procedure,
                                    vec![
                                        item,
                                        int(6),
                                        Expr::Str(iotype.clone()),
                                        id(&vlist),
                                        id(&ios),
                                        id(&msg),
                                    ],
                                ),
                                call("@defined_output_leave", vec![]),
                            ]);
                            if status_target.is_none() {
                                out.push(Stmt::If {
                                    cond: Expr::Bin(
                                        BinOp::Ne,
                                        Box::new(id(&ios)),
                                        Box::new(int(0)),
                                    ),
                                    then_body: vec![call("@error_stop", vec![id(&msg)])],
                                    else_body: None,
                                });
                            }
                        } else {
                            out.push(call(
                                "@formatted_write_noadvance",
                                vec![Expr::Str("(1x,g0)".into()), item],
                            ));
                        }
                        let group = out.split_off(start);
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Eq, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: group,
                            else_body: None,
                        });
                    }
                    out.push(Stmt::If {
                        cond: Expr::Bin(BinOp::Eq, Box::new(id(&ios)), Box::new(int(0))),
                        then_body: vec![call("@formatted_print", vec![Expr::Str("()".into())])],
                        else_body: None,
                    });
                    if let Some(dest) = status_target {
                        out.push(assign(&dest, id(&ios)));
                    }
                    if let Some(dest) = message_target {
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: vec![assign(&dest, id(&msg))],
                            else_body: None,
                        });
                    }
                    *s = Stmt::Block { body: out };
                    return Ok(());
                }
                // Route child list output through the formatted runtime so its
                // record terminator is suppressed by the active parent context.
                if child {
                    match s {
                        Stmt::CallSub { name, args }
                            if name == "@write"
                                && matches!(&args[1],Expr::Call(n,_) if n=="@list_directed") =>
                        {
                            args[1] = Expr::Str("(*(1x,g0))".into());
                        }
                        Stmt::Print { items } => {
                            let mut args = vec![Expr::Str("(*(1x,g0))".into())];
                            args.append(items);
                            *s = call("@formatted_print", args);
                        }
                        _ => {}
                    }
                }
            }
            let nested_child = match s {
                Stmt::Subroutine { name, .. } | Stmt::Function { name, .. } => {
                    child_routines.contains(name)
                }
                _ => child,
            };
            for nested in crate::procedure_arguments::children(s) {
                lower(
                    nested,
                    &objects,
                    bindings,
                    components,
                    child_routines,
                    nested_child,
                    serial,
                )?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}

pub fn resolve(mut program: Program) -> Result<Program> {
    fn contains(body: &mut [Stmt]) -> bool {
        body.iter_mut().any(|s| {
            if matches!(s,Stmt::DerivedType{bindings,..} if bindings.iter().any(|(n,_)|n.starts_with("@defined_io:"))) {return true;}
            crate::procedure_arguments::children(s).into_iter().any(|b|contains(b))
        })
    }
    if !contains(&mut program.body) {
        return Ok(program);
    }
    let mut bindings = HashMap::new();
    let mut procedures = HashMap::new();
    collect(&mut program.body, &mut bindings, &mut procedures)?;
    if bindings.is_empty() {
        return Ok(program);
    }
    for (ty, target) in &bindings {
        let procedure = procedures
            .get(target)
            .with_context(|| format!("undefined defined I/O procedure `{target}`"))?;
        let offset = match procedure {
            Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } => body
                .iter()
                .find_map(crate::source_diagnostics::statement_offset),
            _ => None,
        };
        validate(ty, target, procedure).map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    // Include helper subroutines so child I/O is not tied to lexical placement.
    let mut children: HashSet<_> = bindings.values().cloned().collect();
    loop {
        let mut found = children.clone();
        fn calls(s: &mut Stmt, found: &mut HashSet<String>) {
            if let Stmt::CallSub { name, .. } = s {
                found.insert(name.clone());
            }
            for b in crate::procedure_arguments::children(s) {
                for s in b {
                    calls(s, found);
                }
            }
        }
        for n in &children {
            if let Some(s) = procedures.get(n) {
                calls(&mut s.clone(), &mut found);
            }
        }
        if found == children {
            break;
        }
        children = found;
    }
    fn fields(body: &mut [Stmt], components: &mut Components) {
        for s in body {
            if let Stmt::DerivedType {
                name,
                components: fields,
                ..
            } = s
            {
                for f in fields {
                    if let Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        ..
                    } = f
                    {
                        for field in names {
                            components.insert(
                                (name.clone(), field.clone()),
                                (type_name.clone(), !dims.is_empty()),
                            );
                        }
                    }
                }
            }
            for b in crate::procedure_arguments::children(s) {
                fields(b, components);
            }
        }
    }
    let mut components = Components::new();
    fields(&mut program.body, &mut components);
    let mut function_results = Objects::new();
    for (name, procedure) in &procedures {
        if let Stmt::Function { body, .. } = procedure {
            let result = match body.last() {
                Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                _ => name,
            };
            for declaration in body {
                if let Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } = declaration
                {
                    if names.contains(result) {
                        function_results
                            .insert(name.clone(), (type_name.clone(), !dims.is_empty()));
                    }
                }
            }
        }
    }
    lower(
        &mut program.body,
        &function_results,
        &bindings,
        &components,
        &children,
        false,
        &mut 0,
    )?;
    Ok(program)
}

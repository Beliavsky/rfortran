// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Initial type-bound defined formatted output: scalar objects on external output units.
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

fn checked_write(
    unit: Expr,
    format: &str,
    items: Vec<Expr>,
    advance: bool,
    ios: &str,
    msg: &str,
) -> Stmt {
    let mut args = vec![
        unit,
        Expr::Str(format.into()),
        int(i64::from(advance)),
        id(ios),
        id(msg),
    ];
    args.extend(items);
    call("@write_checked", args)
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

type Objects = HashMap<String, (String, usize)>;
type Declarations = HashMap<String, (TypeSpec, usize)>;
type OutputLoop = (String, Expr, Expr, Expr);

/// Capture every section control before iterating, including scalar subscripts
/// on other axes. Impure bounds must not be evaluated for each output element.
fn array_item(
    item: Expr,
    prefix: &str,
    types: &crate::operators::ExpressionTypes,
    declarations: &Declarations,
    objects: &Objects,
) -> Result<(Expr, Vec<Stmt>, Vec<OutputLoop>)> {
    let (array, supplied) = match item {
        Expr::Ident(n) => (n, None),
        Expr::Section {
            name,
            lower,
            upper,
            stride,
        } => (
            name,
            Some(vec![Expr::Section {
                name: String::new(),
                lower,
                upper,
                stride,
            }]),
        ),
        Expr::Call(n, a) | Expr::Index(n, a) => (n, Some(a)),
        _ => bail!("defined formatted array output requires a whole array or triplet section"),
    };
    if !objects.contains_key(array.split('%').next().unwrap()) {
        bail!("defined formatted array output requires a whole array or triplet section");
    }
    let rank = match types.type_rank(&id(&array), declarations, objects)? {
        Some((crate::operators::ExpressionElement::Derived(_), rank)) if rank > 0 => rank,
        _ => bail!("defined formatted array output requires a whole array or triplet section"),
    };
    let subscripts = supplied.unwrap_or_else(|| {
        (0..rank)
            .map(|_| Expr::Section {
                name: String::new(),
                lower: None,
                upper: None,
                stride: None,
            })
            .collect()
    });
    if subscripts.len() != rank {
        bail!("defined output array requires {rank} subscripts");
    }
    let mut before = vec![];
    let mut indices = vec![];
    let mut loops = vec![];
    for (axis, subscript) in subscripts.into_iter().enumerate() {
        let subscript = match subscript {
            Expr::Call(n, mut a) if n == "@section_subscript" && a.len() == 1 => a.remove(0),
            other => other,
        };
        let name = format!("{prefix}_axis_{axis}");
        if let Expr::Section {
            lower,
            upper,
            stride,
            ..
        } = subscript
        {
            let bound = |n: &str| Expr::Call(n.into(), vec![id(&array), int(axis as i64 + 1)]);
            let controls = [
                lower.map(|e| *e).unwrap_or_else(|| bound("lbound")),
                upper.map(|e| *e).unwrap_or_else(|| bound("ubound")),
                stride.map(|e| *e).unwrap_or(int(1)),
            ];
            let mut captured = vec![];
            for (control, value) in controls.into_iter().enumerate() {
                if !matches!(
                    types.type_rank(&value, declarations, objects)?,
                    Some((
                        crate::operators::ExpressionElement::Intrinsic(
                            TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)
                        ),
                        0
                    ))
                ) {
                    bail!("defined output section bounds and stride must be scalar INTEGER");
                }
                let temp = format!("{name}_control_{control}");
                before.push(decl(&temp, TypeSpec::Integer(None)));
                before.push(assign(&temp, value));
                captured.push(id(&temp));
            }
            before.push(Stmt::If {
                cond: Expr::Bin(BinOp::Eq, Box::new(captured[2].clone()), Box::new(int(0))),
                then_body: vec![call(
                    "@error_stop",
                    vec![Expr::Str("array section stride must not be zero".into())],
                )],
                else_body: None,
            });
            before.push(decl(&name, TypeSpec::Integer(None)));
            indices.push(id(&name));
            loops.push((
                name,
                captured[0].clone(),
                captured[1].clone(),
                captured[2].clone(),
            ));
        } else {
            match types.type_rank(&subscript, declarations, objects)? {
                Some((
                    crate::operators::ExpressionElement::Intrinsic(
                        kind @ (TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)),
                    ),
                    1,
                )) => {
                    // Assignment captures the complete vector exactly once, including
                    // function results. Iterate its actual bounds after allocation.
                    let vector = format!("{name}_vector");
                    before.push(Stmt::ArrayDecl {
                        kind,
                        name: vector.clone(),
                        dims: vec![id("@deferred_extent")],
                    });
                    before.push(Stmt::DummyIntent {
                        name: vector.clone(),
                        intent: "allocatable".into(),
                    });
                    before.push(Stmt::DummyIntent {
                        name: vector.clone(),
                        intent: "@scalar_heap".into(),
                    });
                    // Force descriptor storage even for a constant vector. Start
                    // allocated so allocation analysis need not infer the shape
                    // of an enclosing scope's index variable.
                    before.push(call(
                        "@allocate",
                        vec![Expr::Call(vector.clone(), vec![int(0)])],
                    ));
                    before.push(assign(&vector, subscript));
                    before.push(decl(&name, TypeSpec::Integer(None)));
                    indices.push(Expr::Index(vector.clone(), vec![id(&name)]));
                    loops.push((
                        name,
                        Expr::Call("lbound".into(), vec![id(&vector), int(1)]),
                        Expr::Call("ubound".into(), vec![id(&vector), int(1)]),
                        int(1),
                    ));
                }
                Some((
                    crate::operators::ExpressionElement::Intrinsic(
                        TypeSpec::Integer(_) | TypeSpec::IntegerKind(_),
                    ),
                    0,
                )) => {
                    before.push(decl(&name, TypeSpec::Integer(None)));
                    before.push(assign(&name, subscript));
                    indices.push(id(&name));
                }
                _ => bail!(
                    "defined output subscripts must be scalar INTEGER or rank-one INTEGER vectors"
                ),
            }
        }
    }
    Ok((Expr::Call(array, indices), before, loops))
}

fn lower(
    body: &mut Vec<Stmt>,
    inherited: &Objects,
    inherited_declarations: &Declarations,
    bindings: &HashMap<String, String>,
    expression_types: &crate::operators::ExpressionTypes,
    child_routines: &HashSet<String>,
    child: bool,
    serial: &mut usize,
) -> Result<()> {
    let mut objects = inherited.clone();
    let mut declarations = inherited_declarations.clone();
    for s in body.iter() {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    objects.remove(n);
                    declarations.insert(n.clone(), (kind.clone(), 0));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                objects.remove(name);
                declarations.insert(name.clone(), (kind.clone(), dims.len()));
            }
            Stmt::DerivedDecl {
                type_name,
                names,
                dims,
                ..
            } => {
                for n in names {
                    objects.insert(n.clone(), (type_name.clone(), dims.len()));
                    declarations.remove(n);
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
                // Share type/rank inference with generic procedure resolution, so
                // constructors, components and function results retain their type.
                let target = |e: &Expr| -> Result<Option<(String, usize)>> {
                    Ok(
                        match expression_types.type_rank(e, &declarations, &objects)? {
                            Some((crate::operators::ExpressionElement::Derived(ty), rank)) => {
                                bindings.get(&ty).map(|p| (p.clone(), rank))
                            }
                            _ => None,
                        },
                    )
                };
                if items
                    .iter()
                    .map(&target)
                    .collect::<Result<Vec<_>>>()?
                    .iter()
                    .any(Option::is_some)
                {
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
                    let parent_unit = format!("{prefix}_unit");
                    let mut out = vec![
                        decl(&ios, TypeSpec::Integer(None)),
                        decl(&parent_unit, TypeSpec::Integer(None)),
                        decl(&msg, TypeSpec::Character(Some(256))),
                        Stmt::ArrayDecl {
                            name: vlist.clone(),
                            kind: TypeSpec::Integer(None),
                            dims: vec![int(values.len() as i64)],
                        },
                    ];
                    out.push(assign(&parent_unit, unit));
                    out.push(assign(&msg, Expr::Str(String::new())));
                    out.push(checked_write(
                        id(&parent_unit),
                        "()",
                        vec![],
                        false,
                        &ios,
                        &msg,
                    ));
                    for (i, v) in values.iter().enumerate() {
                        out.push(Stmt::AssignIndex {
                            name: vlist.clone(),
                            indices: vec![int(i as i64 + 1)],
                            value: v.clone(),
                        });
                    }
                    for (item_number, item) in items.into_iter().enumerate() {
                        let start = out.len();
                        let mut array_loop = None;
                        if let Some((procedure, rank)) = target(&item)? {
                            let item = if rank != 0 {
                                if !list {
                                    bail!("defined formatted array output currently requires a list-directed format");
                                }
                                let (element, before, loops) = array_item(
                                    item,
                                    &format!("{prefix}_item_{item_number}"),
                                    expression_types,
                                    &declarations,
                                    &objects,
                                )?;
                                array_loop = Some((before, loops));
                                element
                            } else {
                                item
                            };
                            if list {
                                out.push(checked_write(
                                    id(&parent_unit),
                                    "(1x)",
                                    vec![],
                                    false,
                                    &ios,
                                    &msg,
                                ));
                            }
                            let invoke = vec![
                                assign(&msg, Expr::Str(String::new())),
                                call("@defined_output_enter", vec![id(&parent_unit)]),
                                call(
                                    &procedure,
                                    vec![
                                        item,
                                        id(&parent_unit),
                                        Expr::Str(iotype.clone()),
                                        id(&vlist),
                                        id(&ios),
                                        id(&msg),
                                    ],
                                ),
                                call("@defined_output_leave", vec![]),
                            ];
                            out.push(Stmt::If {
                                cond: Expr::Bin(BinOp::Eq, Box::new(id(&ios)), Box::new(int(0))),
                                then_body: invoke,
                                else_body: None,
                            });
                        } else {
                            out.push(checked_write(
                                id(&parent_unit),
                                "(1x,g0)",
                                vec![item],
                                false,
                                &ios,
                                &msg,
                            ));
                        }
                        let mut group = out.split_off(start);
                        if let Some((mut before, loops)) = array_loop {
                            // First subscript varies fastest in Fortran array element order.
                            for (index, first, last, step) in loops {
                                let mut body = vec![Stmt::If {
                                    cond: Expr::Bin(
                                        BinOp::Ne,
                                        Box::new(id(&ios)),
                                        Box::new(int(0)),
                                    ),
                                    then_body: vec![Stmt::Exit],
                                    else_body: None,
                                }];
                                body.extend(group);
                                let mut loop_body =
                                    vec![call("@do_step", vec![id(&index), first, last, step])];
                                loop_body.extend(body);
                                group = vec![Stmt::Block { body: loop_body }];
                            }
                            before.extend(group);
                            // Give captured arrays a local scope for descriptor
                            // initialization and cleanup, including on writer error.
                            group = vec![Stmt::Block { body: before }];
                        }
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Eq, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: group,
                            else_body: None,
                        });
                    }
                    out.push(Stmt::If {
                        cond: Expr::Bin(BinOp::Eq, Box::new(id(&ios)), Box::new(int(0))),
                        then_body: vec![checked_write(
                            id(&parent_unit),
                            "()",
                            vec![],
                            true,
                            &ios,
                            &msg,
                        )],
                        else_body: None,
                    });
                    if status_target.is_none() {
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: vec![call("@error_stop", vec![id(&msg)])],
                            else_body: None,
                        });
                    }
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
                // Child statements need fallible output and unit-specific record continuation.
                if child {
                    *serial += 1;
                    let ios = format!("@child_output_{serial}_status");
                    let msg = format!("@child_output_{serial}_message");
                    let mut out = vec![
                        decl(&ios, TypeSpec::Integer(None)),
                        decl(&msg, TypeSpec::Character(Some(256))),
                    ];
                    let mut status_target = None;
                    let mut message_target = None;
                    let mut advance = int(1);
                    while matches!(items.first(),Some(Expr::Call(n,_)) if n.starts_with("@write_option:"))
                    {
                        let Expr::Call(n, a) = items.remove(0) else {
                            unreachable!()
                        };
                        match n.as_str() {
                            "@write_option:iostat" => status_target = Some(a[0].clone()),
                            "@write_option:iomsg" => message_target = Some(a[0].clone()),
                            "@write_option:advance" => {
                                advance = match crate::parameters::character_constant(&a[0])?
                                    .trim()
                                    .to_ascii_lowercase()
                                    .as_str()
                                {
                                    "yes" => int(1),
                                    "no" => int(0),
                                    _ => bail!("ADVANCE must be YES or NO"),
                                };
                            }
                            _ => bail!("unsupported defined child WRITE control `{n}`"),
                        }
                    }
                    let format = format
                        .filter(|e| !matches!(e,Expr::Call(n,_) if n=="@list_directed"))
                        .unwrap_or(Expr::Str("(*(1x,g0))".into()));
                    out.push(assign(&msg, Expr::Str(String::new())));
                    let mut args = vec![unit, format, advance, id(&ios), id(&msg)];
                    args.extend(items);
                    out.push(call("@write_checked", args));
                    if let Some(Expr::Ident(dest)) = status_target {
                        out.push(assign(&dest, id(&ios)));
                    } else if status_target.is_some() {
                        bail!("child IOSTAT requires a scalar variable");
                    } else {
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: vec![call("@error_stop", vec![id(&msg)])],
                            else_body: None,
                        });
                    }
                    if let Some(Expr::Ident(dest)) = message_target {
                        out.push(Stmt::If {
                            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
                            then_body: vec![assign(&dest, id(&msg))],
                            else_body: None,
                        });
                    } else if message_target.is_some() {
                        bail!("child IOMSG requires a scalar variable");
                    }
                    *s = Stmt::Block { body: out };
                    return Ok(());
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
                    &declarations,
                    bindings,
                    expression_types,
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
    let expression_types = crate::operators::ExpressionTypes::new(&program)?;
    lower(
        &mut program.body,
        &Objects::new(),
        &Declarations::new(),
        &bindings,
        &expression_types,
        &children,
        false,
        &mut 0,
    )?;
    Ok(program)
}

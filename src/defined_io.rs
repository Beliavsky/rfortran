// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Type-bound defined formatted output on external units.
mod dynamic;
mod internal;
mod unformatted;
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
    owners: &mut HashMap<String, String>,
    input: bool,
) -> Result<()> {
    collect_form(body, bindings, procedures, owners, input, true)
}
fn collect_form(
    body: &mut [Stmt], bindings: &mut HashMap<String, String>,
    procedures: &mut HashMap<String, Stmt>, owners: &mut HashMap<String, String>,
    input: bool, formatted: bool,
) -> Result<()> {
    let direction = if input { "read" } else { "write" };
    let form = if formatted { "formatted" } else { "unformatted" };
    let wanted = format!("@defined_io:{direction}:{form}");
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
                        if key != &wanted {
                            continue;
                        }
                        if private_default
                            && !entries.iter().any(|(key, value)| {
                                key == &format!(
                                    "@defined_io_access:{}:{form}",
                                    if input { "read" } else { "write" }
                                ) && value == "@public"
                            })
                        {
                            bail!("private defined I/O bindings are not yet supported");
                        }
                        // The generic names a binding, not just an implementation.
                        // An extension may override that binding while another alias
                        // still names the parent's implementation.
                        let marker =
                            format!("@defined_io_binding:{}:", &key["@defined_io:".len()..]);
                        let selected = entries.iter().find_map(|(n, original)| {
                            let binding = n.strip_prefix(&marker)?;
                            if original != target {
                                return None;
                            }
                            entries
                                .iter()
                                .find(|(n, _)| n == binding)
                                .map(|(_, procedure)| (binding, procedure))
                        });
                        let resolved = selected.map_or(target, |(_, procedure)| procedure);
                        let owner = selected
                            .and_then(|(binding, _)| {
                                let key = format!("@defined_io_binding_owner:{binding}");
                                entries
                                    .iter()
                                    .rev()
                                    .find(|(n, _)| n == &key)
                                    .map(|(_, owner)| owner)
                            })
                            .unwrap_or(name);
                        owners.insert(name.clone(), owner.clone());
                        if bindings.insert(name.clone(), resolved.clone()).is_some() {
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
            collect_form(child, bindings, procedures, owners, input, formatted)?;
        }
    }
    Ok(())
}
fn validate(ty: &str, target: &str, procedure: &Stmt, input: bool) -> Result<()> {
    validate_form(ty, target, procedure, input, true)
}
fn validate_form(ty: &str, target: &str, procedure: &Stmt, input: bool, formatted: bool) -> Result<()> {
    let Stmt::Subroutine { params, body, .. } = procedure else {
        bail!("defined formatted output `{target}` must be a subroutine");
    };
    if params.len() != if formatted { 6 } else { 4 } {
        if formatted {
            bail!("defined formatted output `{target}` requires six dummy arguments: DTV, UNIT, IOTYPE, V_LIST, IOSTAT, IOMSG");
        }
        bail!("defined unformatted I/O `{target}` requires four dummy arguments: DTV, UNIT, IOSTAT, IOMSG");
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
    let mut required_intents = vec![if input { "intent(inout)" } else { "intent(in)" }, "intent(in)"];
    if formatted { required_intents.extend(["intent(in)", "intent(in)"]); }
    required_intents.extend(["intent(out)", "intent(inout)"]);
    for (i, required) in required_intents.iter().enumerate() {
        if intents.get(&params[i]).map(String::as_str) != Some(*required) {
            bail!(
                "defined formatted output dummy `{}` requires {required}",
                params[i]
            );
        }
    }
    for i in [1, params.len()-2] {
        if !matches!(kinds.get(&params[i]),Some((TypeSpec::Integer(None|Some(4)),d)) if d.is_empty())
        {
            bail!(
                "defined formatted output dummy `{}` must be default INTEGER scalar",
                params[i]
            );
        }
    }
    for i in if formatted { vec![2, 5] } else { vec![3] } {
        if !matches!(kinds.get(&params[i]),Some((TypeSpec::CharacterAssumed,d)) if d.is_empty()) {
            bail!(
                "defined formatted output dummy `{}` must be CHARACTER(len=*) scalar",
                params[i]
            );
        }
    }
    if formatted && !matches!(kinds.get(&params[3]),Some((TypeSpec::Integer(None|Some(4)),d)) if d.len()==1 && matches!(&d[0],Expr::Ident(n) if n=="@deferred_extent"))
    {
        bail!("defined formatted output V_LIST must be an assumed-shape default INTEGER vector");
    }
    Ok(())
}

/// Decode one DT descriptor supplied by the shared format traversal.
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

/// Plan a literal scalar format with the shared group/reversion traversal.
struct FormatItem {
    before: String,
    data: String,
    defined: Option<(String, Vec<Expr>)>,
}
fn has_tabs(format: &Expr) -> Result<bool> {
    let Expr::Str(text)=format else {return Ok(false)};
    crate::format_spec::uses_tabs(text.as_bytes()).map_err(anyhow::Error::msg)
}
fn format_items(format: &Expr, count: usize) -> Result<(Vec<FormatItem>, String)> {
    use crate::format_spec::{self as f, GroupParser};
    let Expr::Str(text) = format else {
        bail!("defined formatted output currently requires a literal parent format");
    };
    let mut parser = GroupParser::new(text.as_bytes(), count).map_err(anyhow::Error::msg)?;
    let mut items = vec![];
    let mut pending = String::new();
    let mut scale = String::new();
    let mut sign = String::new();
    while let Some(d) = parser.next().map_err(anyhow::Error::msg)? {
        let raw = std::str::from_utf8(d.raw)?.trim();
        if f::is_data(d.code) {
            let defined = if d.code == f::DT {
                let descriptor =
                    raw.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_ascii_whitespace());
                Some(dt(&Expr::Str(format!("({descriptor})")))?)
            } else {
                None
            };
            for _ in 0..d.repeat {
                items.push(FormatItem {
                    before: format!("({pending})"),
                    data: format!("({scale}{sign}{pending}{raw})"),
                    defined: defined.clone(),
                });
                pending.clear();
            }
        } else {
            if d.code == f::SCALE {
                scale = format!("{raw},");
            }
            if matches!(d.code, f::SIGN_PLUS | f::SIGN_DEFAULT) {
                sign = format!("{raw},");
            }
            pending.push_str(raw);
            pending.push(',');
        }
    }
    Ok((items, format!("({pending})")))
}

/// One cursor state consumes one scalar item; `finish` is the control editing
/// performed instead if the output list ends at this position.
struct FormatState {
    item: FormatItem,
    finish: String,
    next: usize,
}
fn format_states(format: &Expr) -> Result<Vec<FormatState>> {
    use crate::format_spec::{self as f, GroupParser};
    let Expr::Str(text) = format else {
        bail!("defined formatted output currently requires a literal parent format");
    };
    let mut parser = GroupParser::new(text.as_bytes(), usize::MAX).map_err(anyhow::Error::msg)?;
    parser.preserve_boundaries();
    let mut states: Vec<FormatState> = vec![];
    let mut seen = HashMap::new();
    let mut repeat = None;
    let mut remaining = 0usize;
    let mut scale = String::new();
    let mut sign = String::new();
    loop {
        let key = (parser.position(), remaining, scale.clone(), sign.clone());
        if let Some(&next) = seen.get(&key) {
            states.last_mut().unwrap().next = next;
            break;
        }
        if states.len() == 1024 {
            bail!("defined output format cursor exceeds 1024 states");
        }
        seen.insert(key, states.len());
        let mut pending = String::new();
        let mut finish = String::new();
        let mut finished = false;
        let mut found = None;
        for _ in 0..10000 {
            let d = if remaining > 0 {
                remaining -= 1;
                repeat.unwrap()
            } else {
                let d = parser
                    .next()
                    .map_err(anyhow::Error::msg)?
                    .context("defined output format has no next data descriptor")?;
                if f::is_data(d.code) {
                    remaining = d.repeat - 1;
                    repeat = Some(d);
                }
                d
            };
            let raw = std::str::from_utf8(d.raw)?.trim();
            if f::is_data(d.code) {
                let defined = if d.code == f::DT {
                    let descriptor = raw.trim_start_matches(|c: char| {
                        c.is_ascii_digit() || c.is_ascii_whitespace()
                    });
                    Some(dt(&Expr::Str(format!("({descriptor})")))?)
                } else {
                    None
                };
                found = Some(FormatItem {
                    before: format!("({pending})"),
                    data: format!("({scale}{sign}{pending}{raw})"),
                    defined,
                });
                break;
            }
            if matches!(d.code, b':' | f::REVERSION) {
                finished = true;
            }
            if matches!(d.code, b':') {
                continue;
            }
            if d.code == f::SCALE {
                scale = format!("{raw},");
            }
            if matches!(d.code, f::SIGN_PLUS | f::SIGN_DEFAULT) {
                sign = format!("{raw},");
            }
            pending.push_str(raw);
            pending.push(',');
            if !finished {
                finish.push_str(raw);
                finish.push(',');
            }
        }
        let item =
            found.context("defined output format control expansion exceeds supported limits")?;
        let next = states.len() + 1;
        states.push(FormatState {
            item,
            finish: format!("({finish})"),
            next,
        });
    }
    Ok(states)
}

fn when_equal(name: &str, value: i64, then_body: Vec<Stmt>, else_body: Option<Vec<Stmt>>) -> Stmt {
    Stmt::If {
        cond: Expr::Bin(BinOp::Eq, Box::new(id(name)), Box::new(int(value))),
        then_body,
        else_body,
    }
}

/// Runtime array loops advance a small finite format cursor. Storage and code
/// size depend on the format, never on the number of array elements.
fn formatted_arrays(
    unit: Expr,
    format: &Expr,
    items: Vec<Expr>,
    prefix: &str,
    types: &crate::operators::ExpressionTypes,
    declarations: &Declarations,
    objects: &Objects,
    bindings: &HashMap<String, String>,
    status_target: Option<String>,
    message_target: Option<String>,
) -> Result<Stmt> {
    let states = format_states(format)?;
    let ios = format!("{prefix}_status");
    let msg = format!("{prefix}_message");
    let parent_unit = format!("{prefix}_unit");
    let cursor = format!("{prefix}_cursor");
    let mut out = vec![
        decl(&ios, TypeSpec::Integer(None)),
        decl(&parent_unit, TypeSpec::Integer(None)),
        decl(&cursor, TypeSpec::Integer(None)),
        decl(&msg, TypeSpec::Character(Some(256))),
        assign(&parent_unit, unit),
        assign(&cursor, int(0)),
        assign(&msg, Expr::Str(String::new())),
        call("@defined_output_record_begin",vec![id(&parent_unit)]),
        checked_write(id(&parent_unit), "()", vec![], false, &ios, &msg),
    ];
    for (index, state) in states.iter().enumerate() {
        if let Some((_, values)) = &state.item.defined {
            let vlist = format!("{prefix}_vlist_{index}");
            out.push(Stmt::ArrayDecl {
                name: vlist.clone(),
                kind: TypeSpec::Integer(None),
                dims: vec![int(values.len() as i64)],
            });
            for (i, value) in values.iter().enumerate() {
                out.push(Stmt::AssignIndex {
                    name: vlist.clone(),
                    indices: vec![int(i as i64 + 1)],
                    value: value.clone(),
                });
            }
        }
    }
    for (item_number, item) in items.into_iter().enumerate() {
        let (element, rank) = output_type(types, &item, declarations, objects)?
            .context("cannot determine explicitly formatted output item type")?;
        let procedure = match element {
            crate::operators::ExpressionElement::Derived(ty) => Some(
                bindings
                    .get(&ty)
                    .with_context(|| {
                        format!("derived output item `{ty}` requires a defined writer")
                    })?
                    .clone(),
            ),
            _ => None,
        };
        let item = output_value(item);
        let (item, mut before, loops) = if rank > 0 {
            array_item(
                item,
                &format!("{prefix}_item_{item_number}"),
                types,
                declarations,
                objects,
            )?
        } else {
            (item, vec![], vec![])
        };
        let mut dispatch = vec![];
        for (index, state) in states.iter().enumerate().rev() {
            let mut body = match (&procedure, &state.item.defined) {
                (Some(procedure), Some((iotype, _))) => vec![
                    checked_write(
                        id(&parent_unit),
                        &state.item.before,
                        vec![],
                        false,
                        &ios,
                        &msg,
                    ),
                    when_equal(
                        &ios,
                        0,
                        vec![
                            crate::imports::reference(procedure.clone()),
                            assign(&msg, Expr::Str(String::new())),
                            call("@defined_output_enter", vec![id(&parent_unit)]),
                            call(
                                procedure,
                                vec![
                                    item.clone(),
                                    id(&parent_unit),
                                    Expr::Str(iotype.clone()),
                                    id(&format!("{prefix}_vlist_{index}")),
                                    id(&ios),
                                    id(&msg),
                                ],
                            ),
                            call("@defined_output_leave", vec![]),
                        ],
                        None,
                    ),
                ],
                (None, None) => vec![checked_write(
                    id(&parent_unit),
                    &state.item.data,
                    vec![item.clone()],
                    false,
                    &ios,
                    &msg,
                )],
                _ => vec![
                    assign(&ios, int(1)),
                    assign(
                        &msg,
                        Expr::Str("DT edit descriptor and output item type do not match".into()),
                    ),
                ],
            };
            body.push(assign(&cursor, int(state.next as i64)));
            dispatch = vec![when_equal(&cursor, index as i64, body, Some(dispatch))];
        }
        for (index, first, last, step) in loops {
            let mut body = vec![
                call("@do_step", vec![id(&index), first, last, step]),
                Stmt::If {
                    cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
                    then_body: vec![Stmt::Exit],
                    else_body: None,
                },
            ];
            body.extend(dispatch);
            dispatch = vec![Stmt::Block { body }];
        }
        before.extend(dispatch);
        out.push(when_equal(
            &ios,
            0,
            vec![Stmt::Block { body: before }],
            None,
        ));
    }
    let mut finish = vec![];
    for (index, state) in states.iter().enumerate().rev() {
        finish = vec![when_equal(
            &cursor,
            index as i64,
            vec![checked_write(
                id(&parent_unit),
                &state.finish,
                vec![],
                true,
                &ios,
                &msg,
            )],
            Some(finish),
        )];
    }
    out.push(when_equal(&ios, 0, finish, None));
    out.push(call("@defined_output_record_end",vec![id(&parent_unit),id(&ios),id(&msg)]));
    if let Some(dest) = status_target {
        out.push(assign(&dest, id(&ios)));
    } else {
        out.push(Stmt::If {
            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
            then_body: vec![call("@error_stop", vec![id(&msg)])],
            else_body: None,
        });
    }
    if let Some(dest) = message_target {
        out.push(Stmt::If {
            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
            then_body: vec![assign(&dest, id(&msg))],
            else_body: None,
        });
    }
    Ok(Stmt::Block { body: out })
}

fn output_type(
    types: &crate::operators::ExpressionTypes,
    item: &Expr,
    declarations: &Declarations,
    objects: &Objects,
) -> Result<Option<(crate::operators::ExpressionElement, usize)>> {
    if let Expr::Call(name, args) = item {
        if let Some(parent) = name.strip_prefix("@defined_output_parent:") {
            let (_, rank) = types
                .type_rank(&args[0], declarations, objects)?
                .context("cannot determine parent output object rank")?;
            return Ok(Some((
                crate::operators::ExpressionElement::Derived(parent.into()),
                rank,
            )));
        }
    }
    types.type_rank(item, declarations, objects)
}
fn output_value(item: Expr) -> Expr {
    match item {
        Expr::Call(n, mut a) if n.starts_with("@defined_output_parent:") => a.remove(0),
        item => item,
    }
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
    if !objects.contains_key(array.split('%').next().unwrap()) && !declarations.contains_key(&array)
    {
        bail!("defined formatted array output requires a whole array or triplet section");
    }
    let rank = match types.type_rank(&id(&array), declarations, objects)? {
        Some((_, rank)) if rank > 0 => rank,
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
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n,_) in entities { objects.remove(n); declarations.insert(n.clone(),(kind.clone(),0)); }
            }
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
                        match output_type(expression_types, e, &declarations, &objects)? {
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
                    if let Some((crate::operators::ExpressionElement::Intrinsic(kind @ (TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed)),rank))=expression_types.type_rank(&unit,&declarations,&objects)? {
                        *serial+=1;
                        let mut body=internal::wrap(unit,kind,format,items,true,status_target,message_target,&format!("@defined_internal_{serial}"),rank,expression_types,&declarations,&objects)?;
                        lower(&mut body,&objects,&declarations,bindings,expression_types,child_routines,child,serial)?;
                        *s=Stmt::Block{body};return Ok(());
                    }
                    let list = format
                        .as_ref()
                        .is_none_or(|e| matches!(e,Expr::Call(n,_) if n=="@list_directed"));
                    if !list && (!matches!(format.as_ref(),Some(Expr::Str(_))) || items.iter().map(|e| output_type(expression_types,e,&declarations,&objects)).collect::<Result<Vec<_>>>()?.iter().any(|t| matches!(t,Some((crate::operators::ExpressionElement::Intrinsic(TypeSpec::Complex(_)),_))))) {
                        *serial+=1;
                        *s=dynamic::output(unit,format.clone().unwrap(),items,&format!("@defined_dynamic_{serial}"),expression_types,&declarations,&objects,bindings,status_target,message_target)?;
                        return Ok(());
                    }
                    if !list
                        && items
                            .iter()
                            .map(|e| output_type(expression_types, e, &declarations, &objects))
                            .collect::<Result<Vec<_>>>()?
                            .iter()
                            .any(|ty| ty.as_ref().is_some_and(|(_, rank)| *rank > 0))
                    {
                        *serial += 1;
                        *s = formatted_arrays(
                            unit,
                            format.as_ref().unwrap(),
                            items,
                            &format!("@defined_output_{serial}"),
                            expression_types,
                            &declarations,
                            &objects,
                            bindings,
                            status_target,
                            message_target,
                        )?;
                        return Ok(());
                    }
                    let (plan, trailing) = if list {
                        (vec![], "()".into())
                    } else {
                        format_items(format.as_ref().unwrap(), items.len())?
                    };
                    *serial += 1;
                    let prefix = format!("@defined_output_{serial}");
                    let ios = format!("{prefix}_status");
                    let msg = format!("{prefix}_message");
                    let parent_unit = format!("{prefix}_unit");
                    let mut out = vec![
                        decl(&ios, TypeSpec::Integer(None)),
                        decl(&parent_unit, TypeSpec::Integer(None)),
                        decl(&msg, TypeSpec::Character(Some(256))),
                    ];
                    out.push(assign(&parent_unit, unit));
                    out.push(call("@defined_output_record_begin",vec![id(&parent_unit)]));
                    out.push(assign(&msg, Expr::Str(String::new())));
                    out.push(checked_write(
                        id(&parent_unit),
                        "()",
                        vec![],
                        false,
                        &ios,
                        &msg,
                    ));
                    for (item_number, item) in items.into_iter().enumerate() {
                        let planned = plan.get(item_number);
                        let defined = target(&item)?;
                        if !list {
                            let (_, rank) =
                                output_type(expression_types, &item, &declarations, &objects)?
                                    .context(
                                        "cannot determine explicitly formatted output item type",
                                    )?;
                            if rank != 0 {
                                bail!("defined formatted array output currently requires a list-directed format");
                            }
                            if defined.is_some() != planned.unwrap().defined.is_some() {
                                bail!("DT edit descriptor requires a derived item with a defined writer; intrinsic items require intrinsic data descriptors");
                            }
                        }
                        let item = output_value(item);
                        let (iotype, values) = planned
                            .and_then(|p| p.defined.clone())
                            .unwrap_or_else(|| ("LISTDIRECTED".into(), vec![]));
                        let vlist = format!("{prefix}_vlist_{item_number}");
                        out.push(Stmt::ArrayDecl {
                            name: vlist.clone(),
                            kind: TypeSpec::Integer(None),
                            dims: vec![int(values.len() as i64)],
                        });
                        for (i, value) in values.into_iter().enumerate() {
                            out.push(Stmt::AssignIndex {
                                name: vlist.clone(),
                                indices: vec![int(i as i64 + 1)],
                                value,
                            });
                        }
                        let start = out.len();
                        let mut array_loop = None;
                        if let Some((procedure, rank)) = defined {
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
                            if let Some(p) = planned {
                                out.push(checked_write(
                                    id(&parent_unit),
                                    &p.before,
                                    vec![],
                                    false,
                                    &ios,
                                    &msg,
                                ));
                            }
                            let invoke = vec![
                                crate::imports::reference(procedure.clone()),
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
                                planned.map_or("(1x,g0)", |p| p.data.as_str()),
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
                            &trailing,
                            vec![],
                            true,
                            &ios,
                            &msg,
                        )],
                        else_body: None,
                    });
                    out.push(call("@defined_output_record_end",vec![id(&parent_unit),id(&ios),id(&msg)]));
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

mod input;

pub fn resolve(mut program: Program) -> Result<Program> {
    unformatted::resolve(&mut program)?;
    input::resolve(&mut program)?;
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
    let mut owners = HashMap::new();
    collect(
        &mut program.body,
        &mut bindings,
        &mut procedures,
        &mut owners,
        false,
    )?;
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
        validate(&owners[ty], target, procedure, false)
            .map_err(|e| crate::source_diagnostics::at(e, offset))?;
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

#[cfg(test)]
mod cursor_tests {
    use super::*;
    #[test]
    fn cursor_matches_counted_format_traversal() {
        for text in [
            "(dt)",
            "(2(dt,1x))",
            "(dt,3(dt,'!'))",
            "(*(dt,:,','))",
            "('outer',*('inner',dt))",
            "(sp,i0,dt,(ss,i0,1x,dt))",
            "(dt,/,dt,:,'end')",
            "(2dt'quoted'(1,2),1x,dt'other'(3))",
        ] {
            let format = Expr::Str(text.into());
            let states = format_states(&format).unwrap();
            for count in 0..12 {
                let (expected, tail) = format_items(&format, count).unwrap();
                let mut cursor = 0;
                for item in expected {
                    let actual = &states[cursor];
                    assert_eq!(actual.item.before, item.before, "{text}, count={count}");
                    assert_eq!(actual.item.data, item.data, "{text}, count={count}");
                    assert_eq!(
                        format!("{:?}", actual.item.defined),
                        format!("{:?}", item.defined)
                    );
                    cursor = actual.next;
                }
                assert_eq!(states[cursor].finish, tail, "{text}, count={count}");
            }
        }
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Initial scalar defined input, with list-directed child sessions.
use super::*;

fn read(s: &Stmt) -> Option<(Expr, Option<Expr>, String, Expr, Vec<Expr>)> {
    let Stmt::CallSub { name, args } = s else {
        return None;
    };
    match name.as_str() {
        "@internal_read" => {
            let Expr::Str(control) = &args[1] else {
                return None;
            };
            Some((
                args[0].clone(),
                None,
                control.clone(),
                args[2].clone(),
                args[3..].to_vec(),
            ))
        }
        "@formatted_read" => Some((
            args[0].clone(),
            Some(args[1].clone()),
            String::new(),
            int(0),
            args[2..].to_vec(),
        )),
        "@formatted_read_status" => {
            let Expr::Str(control) = &args[2] else {
                return None;
            };
            Some((
                args[0].clone(),
                Some(args[1].clone()),
                control.clone(),
                args[3].clone(),
                args[4..].to_vec(),
            ))
        }
        _ => None,
    }
}
fn nonzero(n: &str, body: Vec<Stmt>) -> Stmt {
    Stmt::If {
        cond: Expr::Bin(BinOp::Ne, Box::new(id(n)), Box::new(int(0))),
        then_body: body,
        else_body: None,
    }
}
fn lower_one(
    s: &Stmt,
    message: Option<&Expr>,
    objects: &Objects,
    declarations: &Declarations,
    bindings: &HashMap<String, String>,
    types: &crate::operators::ExpressionTypes,
    serial: &mut usize,
    polymorphic: &HashSet<String>,
) -> Result<Option<Stmt>> {
    let Some((unit, format, control, status, items)) = read(s) else {
        return Ok(None);
    };
    let mut writers = vec![];
    for item in &items {
        writers.push(match types.type_rank(item, declarations, objects)? {
            Some((crate::operators::ExpressionElement::Derived(ty), rank)) => {
                bindings.get(&ty).map(|p| (p.clone(), rank))
            }
            _ => None,
        });
    }
    if !writers.iter().any(Option::is_some) {
        return Ok(None);
    }
    if items
        .iter()
        .any(|e| matches!(e,Expr::Ident(n) if polymorphic.contains(n)))
    {
        bail!("polymorphic defined input dispatch is not yet supported");
    }
    if !matches!(
        types.type_rank(&unit, declarations, objects)?,
        Some((
            crate::operators::ExpressionElement::Intrinsic(TypeSpec::Integer(_)),
            0
        ))
    ) {
        bail!("defined formatted input currently requires an external INTEGER unit");
    }
    if !control.is_empty() && control != "iostat" {
        bail!("defined input currently supports IOSTAT and IOMSG controls only");
    }
    if !control.is_empty()
        && (!matches!(&status, Expr::Ident(_))
            || !matches!(
                types.type_rank(&status, declarations, objects)?,
                Some((
                    crate::operators::ExpressionElement::Intrinsic(TypeSpec::Integer(_)),
                    0
                ))
            ))
    {
        bail!("defined input IOSTAT requires a scalar INTEGER variable");
    }
    if let Some(msg) = message {
        if !matches!(msg, Expr::Ident(_))
            || !matches!(
                types.type_rank(msg, declarations, objects)?,
                Some((
                    crate::operators::ExpressionElement::Intrinsic(
                        TypeSpec::Character(_) | TypeSpec::CharacterLength(_)
                    ),
                    0
                ))
            )
        {
            bail!("defined input IOMSG requires a scalar CHARACTER variable");
        }
    }
    let plan = if let Some(format) = &format {
        let (plan, trailing) = format_items(format, items.len())?;
        if trailing != "()" || plan.iter().any(|p| p.before != "()" || p.defined.is_none()) {
            bail!("defined input parent format currently requires DT descriptors without positioning or intrinsic items");
        }
        plan
    } else {
        vec![]
    };
    *serial += 1;
    let prefix = format!("@defined_input_{serial}");
    let unit_name = format!("{prefix}_unit");
    let ios = format!("{prefix}_status");
    let msg = format!("{prefix}_message");
    let handle = format!("{prefix}_handle");
    let mut out = vec![
        decl(&unit_name, TypeSpec::Integer(None)),
        decl(&ios, TypeSpec::Integer(None)),
        decl(&handle, TypeSpec::Integer(Some(8))),
        decl(&msg, TypeSpec::Character(Some(256))),
        assign(&unit_name, unit),
        assign(&ios, int(0)),
        assign(&msg, Expr::Str(String::new())),
        call("@defined_input_enter", vec![id(&unit_name)]),
        call(
            "@list_read_begin",
            vec![id(&handle), id(&ios), id(&unit_name)],
        ),
    ];
    for (i, (item, writer)) in items.into_iter().zip(writers).enumerate() {
        let Some((procedure, rank)) = writer else {
            bail!("defined input currently requires only derived scalar items");
        };
        if rank != 0 || !matches!(item, Expr::Ident(_)) {
            bail!("defined input currently requires named scalar derived variables or scalar components");
        }
        let (iotype, values) = plan
            .get(i)
            .and_then(|p| p.defined.clone())
            .unwrap_or(("LISTDIRECTED".into(), vec![]));
        let vlist = format!("{prefix}_vlist_{i}");
        out.push(Stmt::ArrayDecl {
            name: vlist.clone(),
            kind: TypeSpec::Integer(None),
            dims: vec![int(values.len() as i64)],
        });
        for (j, value) in values.into_iter().enumerate() {
            out.push(Stmt::AssignIndex {
                name: vlist.clone(),
                indices: vec![int(j as i64 + 1)],
                value,
            });
        }
        out.push(when_equal(
            &ios,
            0,
            vec![
                crate::imports::reference(procedure.clone()),
                assign(&msg, Expr::Str(String::new())),
                call(
                    &procedure,
                    vec![
                        item,
                        id(&unit_name),
                        Expr::Str(iotype),
                        id(&vlist),
                        id(&ios),
                        id(&msg),
                    ],
                ),
            ],
            None,
        ));
    }
    out.push(call("@defined_input_leave", vec![id(&ios)]));
    out.push(nonzero(
        &ios,
        vec![Stmt::If {
            cond: Expr::Bin(
                BinOp::Eq,
                Box::new(id(&msg)),
                Box::new(Expr::Str(String::new())),
            ),
            then_body: vec![assign(
                &msg,
                Expr::Str("defined formatted input failed: invalid input or end of file".into()),
            )],
            else_body: None,
        }],
    ));
    if control.is_empty() {
        out.push(nonzero(&ios, vec![call("@error_stop", vec![id(&msg)])]));
    } else {
        let Expr::Ident(status) = status else {
            unreachable!()
        };
        out.push(assign(&status, id(&ios)));
    }
    if let Some(Expr::Ident(message)) = message {
        out.push(nonzero(&ios, vec![assign(message, id(&msg))]));
    }
    Ok(Some(Stmt::Block { body: out }))
}
fn lower(
    body: &mut [Stmt],
    inherited: &Objects,
    inherited_declarations: &Declarations,
    bindings: &HashMap<String, String>,
    types: &crate::operators::ExpressionTypes,
    serial: &mut usize,
    inherited_polymorphic: &HashSet<String>,
) -> Result<()> {
    let mut polymorphic = inherited_polymorphic.clone();
    let mut objects = inherited.clone();
    let mut declarations = inherited_declarations.clone();
    for s in body.iter() {
        match s {
            Stmt::DerivedDecl {
                type_name,
                names,
                dims,
                polymorphic: poly,
                ..
            } => {
                for n in names {
                    if *poly {
                        polymorphic.insert(n.clone());
                    } else {
                        polymorphic.remove(n);
                    }
                    objects.insert(n.clone(), (type_name.clone(), dims.len()));
                    declarations.remove(n);
                }
            }
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    declarations.insert(n.clone(), (kind.clone(), 0));
                    objects.remove(n);
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                declarations.insert(name.clone(), (kind.clone(), dims.len()));
                objects.remove(name);
            }
            _ => {}
        }
    }
    let mut offset = None;
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
        }
        let result = (|| -> Result<()> {
            let mut replacement = None;
            if let Stmt::Block { body } = s {
                if let [read, Stmt::CallSub { name, args }] = body.as_slice() {
                    if name == crate::initialized::MAY_DEFINE && args.len() == 1 {
                        replacement = lower_one(
                            read,
                            Some(&args[0]),
                            &objects,
                            &declarations,
                            bindings,
                            types,
                            serial,
                            &polymorphic,
                        )?;
                    }
                }
            }
            if replacement.is_none() {
                replacement = lower_one(
                    s,
                    None,
                    &objects,
                    &declarations,
                    bindings,
                    types,
                    serial,
                    &polymorphic,
                )?;
            }
            if let Some(new) = replacement {
                *s = new;
                return Ok(());
            }
            for child in crate::procedure_arguments::children(s) {
                lower(
                    child,
                    &objects,
                    &declarations,
                    bindings,
                    types,
                    serial,
                    &polymorphic,
                )?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}
pub(super) fn resolve(program: &mut Program) -> Result<()> {
    fn contains(body: &mut [Stmt]) -> bool {
        body.iter_mut().any(|s| {
            if matches!(s,Stmt::DerivedType{bindings,..} if bindings.iter().any(|(n,_)|n=="@defined_io:read:formatted")) {return true;}
            crate::procedure_arguments::children(s).into_iter().any(|b|contains(b))
        })
    }
    if !contains(&mut program.body) {
        return Ok(());
    }

    let mut bindings = HashMap::new();
    let mut procedures = HashMap::new();
    let mut owners = HashMap::new();
    collect(
        &mut program.body,
        &mut bindings,
        &mut procedures,
        &mut owners,
        true,
    )?;
    if bindings.is_empty() {
        return Ok(());
    }
    for (ty, target) in &bindings {
        let procedure = procedures
            .get(target)
            .with_context(|| format!("undefined defined input procedure {target}"))?;
        let offset = match procedure {
            Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } => body
                .iter()
                .find_map(crate::source_diagnostics::statement_offset),
            _ => None,
        };
        validate(&owners[ty], target, procedure, true).map_err(|e| {
            crate::source_diagnostics::at(
                anyhow::anyhow!(e.to_string().replace("formatted output", "formatted input")),
                offset,
            )
        })?;
    }
    // Reject child transfers that do not yet preserve the shared input position.
    fn check(s: &mut Stmt, called: &mut HashSet<String>) -> Result<()> {
        if let Stmt::CallSub { name, .. } = s {
            if matches!(
                name.as_str(),
                "@formatted_read" | "@formatted_read_status" | "@empty_read" | "@unformatted_read"
            ) {
                bail!("defined input currently supports list-directed child READ statements only");
            }
            called.insert(name.clone());
        }
        for b in crate::procedure_arguments::children(s) {
            for s in b {
                check(s, called)?;
            }
        }
        Ok(())
    }
    let mut pending: Vec<_> = bindings.values().cloned().collect();
    let mut seen = HashSet::new();
    while let Some(n) = pending.pop() {
        if seen.insert(n.clone()) {
            if let Some(s) = procedures.get(&n) {
                let mut called = HashSet::new();
                check(&mut s.clone(), &mut called)?;
                pending.extend(called);
            }
        }
    }
    fn messages(body: &mut [Stmt], readers: &HashSet<String>, active: bool) {
        for s in body {
            if let Stmt::Block { body } = s {
                if active {
                    if let [statement, Stmt::CallSub { name, args }] = body.as_slice() {
                        if name == crate::initialized::MAY_DEFINE && args.len() == 1 {
                            if let Some((_, _, control, Expr::Ident(status), _)) = read(statement) {
                                if control == "iostat" {
                                    if let Expr::Ident(message) = &args[0] {
                                        body.push(nonzero(&status,vec![assign(message,Expr::Str("list-directed input failed: invalid input or end of file".into()))]));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let active = match s {
                Stmt::Subroutine { name, .. } | Stmt::Function { name, .. } => {
                    readers.contains(name)
                }
                _ => active,
            };
            for child in crate::procedure_arguments::children(s) {
                messages(child, readers, active);
            }
        }
    }
    // Keep canonical parent READ+IOMSG pairs intact until parent lowering.
    let types = crate::operators::ExpressionTypes::new(program)?;
    lower(
        &mut program.body,
        &Objects::new(),
        &Declarations::new(),
        &bindings,
        &types,
        &mut 0,
        &HashSet::new(),
    )?;
    messages(&mut program.body, &seen, false);
    Ok(())
}

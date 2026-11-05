// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Defined formatted input with shared child sessions and counted array transfers.
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
/// Parent control editing shares the child session's record position.
/// The existing field helper discards a span or advances explicitly; it does
/// not terminate the parent record on return.
fn controls(text: &str, unit: &str, ios: &str, scratch: &str) -> Result<Vec<Stmt>> {
    use crate::format_spec as f;
    let mut parser=f::GroupParser::new(text.as_bytes(),0).map_err(anyhow::Error::msg)?;
    let mut out=vec![];
    while let Some(d)=parser.next().map_err(anyhow::Error::msg)? {
        let (width,advance,count)=match d.code {
            b'/' => (0,1,d.repeat.max(1)),
            b'x' | f::TR => (if d.width>0 {d.width}else{d.repeat.max(1)},0,1),
            _ => bail!("defined input parent format currently supports DT descriptors, X/TR spacing, slash and colon editing only"),
        };
        // One byte is enough: discard spacing one character at a time so a
        // large repeat count cannot create a large CHARACTER temporary.
        for _ in 0..count {
            if width==0 {
                out.push(when_equal(ios,0,vec![call("@read_record_status",vec![id(ios),id(scratch),int(1),id(unit),int(0),int(advance)])],None));
            } else {
                let index=format!("{scratch}_skip");
                out.push(Stmt::Block{body:vec![
                    call("@do_step",vec![id(&index),int(1),int(width as i64),int(1)]),
                    nonzero(ios,vec![Stmt::Exit]),
                    call("@read_record_status",vec![id(ios),id(scratch),int(1),id(unit),int(1),int(0)])]});
            }
        }
    }
    Ok(out)
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
        .any(|e| match e {
            Expr::Ident(n) | Expr::Call(n,_) | Expr::Index(n,_) | Expr::Section{name:n,..} =>
                polymorphic.contains(n),
            _ => false,
        })
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
    let states = if let Some(format) = &format {
        let states=format_states(format).map_err(|e|anyhow::anyhow!(e.to_string().replace("defined output","defined input").replace("formatted output","formatted input")))?;
        if states.iter().any(|s|s.item.defined.is_none()) {
            bail!("defined input parent format currently requires DT descriptors without intrinsic items");
        }
        states
    } else {
        vec![FormatState{item:FormatItem{before:"()".into(),data:String::new(),defined:Some(("LISTDIRECTED".into(),vec![]))},finish:"()".into(),next:0}]
    };
    *serial += 1;
    let prefix = format!("@defined_input_{serial}");
    let unit_name = format!("{prefix}_unit");
    let ios = format!("{prefix}_status");
    let msg = format!("{prefix}_message");
    let handle = format!("{prefix}_handle");
    let cursor = format!("{prefix}_cursor");
    let scratch = format!("{prefix}_scratch");
    let mut out = vec![
        decl(&cursor, TypeSpec::Integer(None)),
        decl(&scratch, TypeSpec::Character(Some(1))),
        decl(&format!("{scratch}_skip"),TypeSpec::Integer(None)),
        assign(&cursor,int(0)),
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
    for (i,state) in states.iter().enumerate() {
        let (_,values)=state.item.defined.as_ref().unwrap();
        let vlist=format!("{prefix}_vlist_{i}");
        out.push(Stmt::ArrayDecl{name:vlist.clone(),kind:TypeSpec::Integer(None),dims:vec![int(values.len() as i64)]});
        for (j,value) in values.iter().enumerate() {
            out.push(Stmt::AssignIndex{name:vlist.clone(),indices:vec![int(j as i64+1)],value:value.clone()});
        }
    }
    for (i, (item, writer)) in items.into_iter().zip(writers).enumerate() {
        let Some((procedure, rank)) = writer else {
            bail!("defined input currently requires only derived items");
        };
        let indexed = matches!(&item, Expr::Call(n,_) | Expr::Index(n,_)
            if matches!(types.type_rank(&id(n),declarations,objects)?,Some((_,r)) if r>0));
        let (item, mut before, loops) = if rank != 0 || indexed {
            array_item(item,&format!("{prefix}_item_{i}"),types,declarations,objects)
                .map_err(|e|anyhow::anyhow!(e.to_string().replace("output","input")))?
        } else if matches!(item,Expr::Ident(_)) {
            (item,vec![],vec![])
        } else {
            bail!("defined input requires writable derived variables, elements or sections");
        };
        let mut transfer=vec![];
        for (index,state) in states.iter().enumerate().rev() {
            let (iotype,_)=state.item.defined.as_ref().unwrap();
            let mut body=controls(&state.item.before,&unit_name,&ios,&scratch)?;
            body.push(when_equal(&ios,0,vec![
                crate::imports::reference(procedure.clone()),
                assign(&msg,Expr::Str(String::new())),
                call(&procedure,vec![item.clone(),id(&unit_name),Expr::Str(iotype.clone()),
                    id(&format!("{prefix}_vlist_{index}")),id(&ios),id(&msg)]),
                assign(&cursor,int(state.next as i64)),
            ],None));
            transfer=vec![when_equal(&cursor,index as i64,body,Some(transfer))];
        }
        // Wrap axis one first so it varies fastest in Fortran element order.
        for (index, first, last, step) in loops {
            let mut body=vec![call("@do_step",vec![id(&index),first,last,step]),
                nonzero(&ios,vec![Stmt::Exit])];
            body.extend(transfer);
            transfer=vec![Stmt::Block{body}];
        }
        before.extend(transfer);
        out.push(when_equal(&ios,0,vec![Stmt::Block{body:before}],None));
    }
    let mut finish=vec![];
    for (index,state) in states.iter().enumerate().rev() {
        finish=vec![when_equal(&cursor,index as i64,
            controls(&state.finish,&unit_name,&ios,&scratch)?,Some(finish))];
    }
    out.push(when_equal(&ios,0,finish,None));
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
        if let Stmt::CallSub { name, args } = s {
            if matches!(
                name.as_str(),
                "@empty_read" | "@unformatted_read"
            ) {
                bail!("defined input does not yet support empty or unformatted child READ statements");
            }
            if matches!(name.as_str(), "@formatted_read" | "@formatted_read_status") {
                let text=crate::parameters::character_constant(&args[1])
                    .map_err(|_|anyhow::anyhow!("defined input child READ currently requires a constant format"))?;
                // Until a bidirectional record buffer is available, reject left
                // tabbing rather than consuming past the requested final position.
                let mut parser=crate::format_spec::GroupParser::new(text.as_bytes(),args.len()-if name=="@formatted_read" {2}else{4}).map_err(anyhow::Error::msg)?;
                while let Some(d)=parser.next().map_err(anyhow::Error::msg)? {
                    if matches!(d.code,crate::format_spec::T | crate::format_spec::TL) {
                        bail!("defined input child READ does not yet support T or TL positioning");
                    }
                }
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
                            if let Some((_, format, control, Expr::Ident(status), _)) = read(statement) {
                                if control == "iostat" {
                                    if let Expr::Ident(message) = &args[0] {
                                        let text=if format.is_some() {"formatted input failed: invalid input or end of file"}else{"list-directed input failed: invalid input or end of file"};
                                        body.push(nonzero(&status,vec![assign(message,Expr::Str(text.into()))]));
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

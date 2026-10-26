// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! NAMELIST groups expand to checked scalar transfers, preserving omitted objects.
use crate::ast::{BinOp as B, Expr as E, Stmt as S, TypeSpec as T};
use anyhow::{bail, ensure, Result};
use std::collections::{HashMap, HashSet};
fn i(n: i64) -> E {
    E::IntLit(n.to_string())
}
fn v(n: &str) -> E {
    E::Ident(n.into())
}
fn text(n: &str) -> E {
    E::Str(n.into())
}
fn bin(op: B, a: E, b: E) -> E {
    E::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, e: E) -> S {
    S::Assign {
        name: n.into(),
        value: e,
    }
}
fn call(n: &str, args: Vec<E>) -> S {
    S::CallSub {
        name: n.into(),
        args,
    }
}
fn branch(cond: E, body: Vec<S>) -> S {
    S::If {
        cond,
        then_body: body,
        else_body: None,
    }
}
fn decl(kind: T, n: &str) -> S {
    S::VarDecl {
        kind,
        names: vec![n.into()],
    }
}
pub fn tokens(tokens: &mut Vec<crate::lexer::Token>, input: &str) {
    use crate::lexer::TokenKind as K;
    let mut n = 0;
    while n + 1 < tokens.len() {
        if matches!(&tokens[n].kind,K::Ident(s) if s=="namelist")
            && matches!(tokens[n + 1].kind, K::Slash)
            && (n == 0
                || !crate::lexer::same_statement(
                    &input[tokens[n - 1].span.end..tokens[n].span.start],
                ))
        {
            let mut end = n + 1;
            while end < tokens.len()
                && crate::lexer::same_statement(
                    &input[tokens[end - 1].span.end..tokens[end].span.start],
                )
            {
                end += 1;
            }
            let src = tokens[n + 1..end]
                .iter()
                .map(|t| input[t.span.clone()].to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            tokens[n].kind = K::ProcedureDeclaration(format!("namelist|{src}"));
            tokens[n].span.end = tokens[end - 1].span.end;
            tokens.drain(n + 1..end);
        }
        n += 1;
    }
}
pub fn declaration(src: &str) -> std::result::Result<Vec<S>, &'static str> {
    let mut parts = src.split('/');
    if !parts.next().unwrap_or("").trim().is_empty() {
        return Err("invalid NAMELIST declaration");
    }
    let mut out = vec![];
    while let Some(group) = parts.next() {
        let group = group.trim();
        let members = parts.next().ok_or("NAMELIST requires /group/ members")?;
        let names = members
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>();
        let identifier = |n: &str| {
            n.starts_with(|c: char| c.is_ascii_alphabetic())
                && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !identifier(group) || names.is_empty() || names.iter().any(|n| !identifier(n)) {
            return Err("NAMELIST requires a group name and variable names");
        }
        let mut args = vec![text(group)];
        args.extend(names.into_iter().map(v));
        out.push(call("@namelist_decl", args));
    }
    Ok(out)
}
pub fn controls(
    controls: Vec<E>,
    items: Vec<E>,
    read: bool,
) -> std::result::Result<S, &'static str> {
    if !items.is_empty() {
        return Err("NAMELIST transfer cannot have an input/output item list");
    }
    let mut values: HashMap<String, E> = HashMap::new();
    let mut named = false;
    for (pos, e) in controls.into_iter().enumerate() {
        let (key, e) = match e {
            E::Call(n, a)
                if n.starts_with("@read_control:") || n.starts_with("@write_control:") =>
            {
                named = true;
                (n.split(':').nth(1).unwrap().to_string(), a[0].clone())
            }
            e => {
                if named {
                    return Err("positional controls must precede named controls");
                }
                (
                    (match pos {
                        0 => "unit",
                        1 => "nml",
                        _ => return Err("too many NAMELIST controls"),
                    })
                    .into(),
                    e,
                )
            }
        };
        if !matches!(key.as_str(), "unit" | "nml" | "iostat" | "iomsg") {
            return Err("NAMELIST currently supports UNIT, NML, IOSTAT and IOMSG");
        }
        if matches!(key.as_str(), "iostat" | "iomsg") && !matches!(e, E::Ident(_)) {
            return Err("NAMELIST IOSTAT and IOMSG require scalar variables");
        }
        if values.insert(key, e).is_some() {
            return Err("duplicate NAMELIST control");
        }
    }
    let mut unit = values.remove("unit").ok_or("NAMELIST requires UNIT")?;
    if matches!(&unit,E::Call(n,_) if n=="@write_star") {
        unit = i(if read { 5 } else { 6 });
    }
    let E::Ident(group) = values.remove("nml").ok_or("NAMELIST requires NML")? else {
        return Err("NML requires a NAMELIST group name");
    };
    let status = values.remove("iostat").unwrap_or(i(0));
    let message = values.remove("iomsg").unwrap_or(i(0));
    Ok(call(
        if read {
            "@namelist_read"
        } else {
            "@namelist_write"
        },
        vec![unit, text(&group), status, message],
    ))
}
type Symbols = HashMap<String, (T, Vec<E>, Vec<String>)>;
type Groups = HashMap<String, Vec<String>>;
fn bounds(d: &E) -> (E, E) {
    match d {
        E::Call(n, a) if n == "@declared_bounds" => (a[0].clone(), a[1].clone()),
        E::Section { lower, upper, .. } => (
            lower.as_deref().cloned().unwrap_or(i(1)),
            upper.as_deref().cloned().unwrap_or(i(0)),
        ),
        _ => (i(1), d.clone()),
    }
}
fn size(d: &E) -> E {
    let (lo, hi) = bounds(d);
    E::Call(
        "max".into(),
        vec![i(0), bin(B::Add, bin(B::Sub, hi, lo), i(1))],
    )
}
fn count(dims: &[E]) -> E {
    dims.iter().fold(i(1), |n, d| bin(B::Mul, n, size(d)))
}
fn element(name: &str, dims: &[E], index: E) -> E {
    if dims.is_empty() {
        return v(name);
    }
    let mut offset = bin(B::Sub, index, i(1));
    let mut indices = vec![];
    for d in dims {
        let (lo, _) = bounds(d);
        indices.push(bin(
            B::Add,
            lo,
            E::Call("mod".into(), vec![offset.clone(), size(d)]),
        ));
        offset = bin(B::Div, offset, size(d));
    }
    E::Call(name.into(), indices)
}
pub fn resolve(mut program: crate::ast::Program) -> Result<crate::ast::Program> {
    fn body(
        statements: &mut Vec<S>,
        inherited: &Symbols,
        host: &Groups,
        serial: &mut usize,
    ) -> Result<()> {
        let mut symbols = inherited.clone();
        let mut groups = host.clone();
        let mut local = HashSet::new();
        for s in statements.iter() {
            match s {
                S::VarDecl { kind, names } => {
                    for n in names {
                        symbols.insert(n.clone(), (kind.clone(), vec![], vec![]));
                    }
                }
                S::ArrayDecl { kind, name, dims } => {
                    symbols.insert(name.clone(), (kind.clone(), dims.clone(), vec![]));
                }
                S::InitializedDecl {
                    kind,
                    attributes,
                    entities,
                } => {
                    for (n, _) in entities {
                        symbols.insert(n.clone(), (kind.clone(), vec![], attributes.clone()));
                    }
                }
                S::CallSub { name, args } if name == "@namelist_decl" => {
                    let E::Str(group) = &args[0] else {
                        unreachable!()
                    };
                    if local.insert(group.clone()) {
                        groups.insert(group.clone(), vec![]);
                    }
                    let members = groups.get_mut(group).unwrap();
                    for e in &args[1..] {
                        let E::Ident(n) = e else { unreachable!() };
                        if !members.contains(n) {
                            members.push(n.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        for stmt in statements.iter() {
            if let S::DummyIntent { name, intent } = stmt {
                if let Some((_, _, attrs)) = symbols.get_mut(name) {
                    attrs.push(intent.clone());
                }
            }
        }
        let mut offset = None;
        for s in statements.iter_mut() {
            if let S::SourceLocation(n) = s {
                offset = Some(*n);
                continue;
            }
            let result = (|| -> Result<()> {
                if let S::CallSub { name, args } = s {
                    if name == "@namelist_decl" {
                        *s = S::Block { body: vec![] };
                        return Ok(());
                    }
                    // A positional second control denotes a group only when a
                    // visible NAMELIST declaration establishes that meaning.
                    if name == "@write" || name == "@formatted_read" {
                        if let Some(E::Ident(group)) = args.get(1) {
                            if groups.contains_key(group) {
                                ensure!(
                                    args.len() == 2,
                                    "NAMELIST does not permit a transfer item list"
                                );
                                *s = call(
                                    if name == "@write" {
                                        "@namelist_write"
                                    } else {
                                        "@namelist_read"
                                    },
                                    vec![args[0].clone(), text(group), i(0), i(0)],
                                );
                            }
                        }
                    }
                }
                if let S::CallSub { name, args } = s {
                    if name == "@namelist_read" || name == "@namelist_write" {
                        let read = name == "@namelist_read";
                        let E::Str(group) = &args[1] else {
                            unreachable!()
                        };
                        let members = groups
                            .get(group)
                            .ok_or_else(|| anyhow::anyhow!("unknown NAMELIST group `{group}`"))?;
                        for n in members {
                            let (_, dims, attrs) = symbols.get(n).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "NAMELIST member `{n}` requires an intrinsic declaration"
                                )
                            })?;
                            ensure!(!attrs.iter().any(|a|matches!(a.as_str(),"parameter"|"allocatable"|"pointer")),"NAMELIST member `{n}` currently requires nonallocatable, nonpointer variable storage");
                            ensure!(
                                dims.iter()
                                    .all(|d| !matches!(d,E::Ident(n) if n.starts_with('@'))),
                                "NAMELIST currently requires fixed-size arrays"
                            );
                        }
                        if read {
                            for n in members {
                                ensure!(
                                    !symbols[n].2.iter().any(|a| a == "intent(in)"),
                                    "NAMELIST READ cannot define INTENT(IN) member `{n}`"
                                );
                            }
                        }
                        *serial += 1;
                        *s = transfer(args, read, members, &symbols, *serial)?;
                        return Ok(());
                    }
                }
                for child in crate::procedure_arguments::children(s) {
                    body(child, &symbols, &groups, serial)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
    body(&mut program.body, &Symbols::new(), &Groups::new(), &mut 0)?;
    Ok(program)
}
fn transfer(
    args: &[E],
    read: bool,
    members: &[String],
    symbols: &Symbols,
    serial: usize,
) -> Result<S> {
    let [unit, E::Str(group), status, message] = args else {
        unreachable!()
    };
    ensure!(
        matches!(status, E::Ident(_) | E::IntLit(_)),
        "NAMELIST IOSTAT requires an INTEGER variable"
    );
    ensure!(
        matches!(message, E::Ident(_) | E::IntLit(_)),
        "NAMELIST IOMSG requires a CHARACTER variable"
    );
    if let E::Ident(n) = status {
        ensure!(
            matches!(symbols.get(n),Some((T::Integer(_)|T::IntegerKind(_),d,_)) if d.is_empty()),
            "NAMELIST IOSTAT requires an INTEGER scalar variable"
        );
    }
    if let E::Ident(n) = message {
        ensure!(
            matches!(symbols.get(n),Some((T::Character(_)|T::CharacterLength(_),d,_)) if d.is_empty()),
            "NAMELIST IOMSG requires a CHARACTER scalar variable"
        );
    }
    let temp = |s: &str| format!("@nml_{serial}_{s}");
    let (code, h, field, token, index) = (
        temp("code"),
        temp("handle"),
        temp("field"),
        temp("token"),
        temp("index"),
    );
    let mut out = vec![decl(T::Integer(Some(8)), &code), set(&code, i(0))];
    if read {
        out.extend([
            decl(T::Integer(Some(8)), &h),
            decl(T::Integer(Some(8)), &index),
            decl(T::Character(Some(128)), &field),
            decl(T::Character(Some(4096)), &token),
        ]);
        let internal = matches!(unit,E::Ident(n) if matches!(symbols.get(n),Some((T::Character(_)|T::CharacterLength(_),d,_)) if d.is_empty()));
        out.push(call(
            if internal {
                "@nml_internal_begin"
            } else {
                "@nml_begin"
            },
            vec![v(&h), v(&code), unit.clone(), text(group)],
        ));
        let mut work = vec![
            call(
                "@nml_next",
                vec![v(&code), v(&index), v(&field), v(&token), v(&h)],
            ),
            branch(bin(B::Lt, v(&code), i(0)), vec![S::Exit]),
        ];
        let mut dispatch = vec![set(&code, i(3))];
        for n in members.iter().rev() {
            let (kind, dims, _) = &symbols[n];
            let item = element(n, dims, v(&index));
            let buffer = format!("@nml_{serial}_value_{n}");
            out.push(decl(kind.clone(), &buffer));
            let value = call(
                "@internal_read",
                vec![v(&token), text("iostat"), v(&code), v(&buffer)],
            );
            let assign = match item {
                E::Ident(n) => set(&n, v(&buffer)),
                E::Call(n, indices) => S::AssignIndex {
                    name: n,
                    indices,
                    value: v(&buffer),
                },
                _ => unreachable!(),
            };
            let mut matched = vec![call(
                "@nml_layout",
                vec![
                    v(&code),
                    v(&index),
                    v(&h),
                    i(0),
                    i(dims.len() as i64),
                    i(0),
                    i(0),
                ],
            )];
            for (axis, d) in dims.iter().enumerate() {
                let (lo, hi) = bounds(d);
                matched.push(call(
                    "@nml_layout",
                    vec![
                        v(&code),
                        v(&index),
                        v(&h),
                        i(axis as i64 + 1),
                        i(dims.len() as i64),
                        lo,
                        hi,
                    ],
                ));
            }
            matched.push(branch(
                bin(
                    B::Or,
                    bin(B::Lt, v(&index), i(1)),
                    bin(B::Gt, v(&index), count(dims)),
                ),
                vec![set(&code, i(4))],
            ));
            // A null advances the array position without defining its object.
            // Handle it before conversion: conversion errors may also be 2.
            matched.push(branch(
                bin(B::Eq, v(&code), i(2)),
                vec![set(&code, i(0)), S::Cycle],
            ));
            matched.push(branch(
                bin(B::Eq, v(&code), i(0)),
                vec![
                    value,
                    branch(bin(B::Eq, v(&code), i(0)), vec![assign]),
                    branch(bin(B::Lt, v(&code), i(0)), vec![set(&code, i(5))]),
                ],
            ));

            dispatch = vec![S::If {
                cond: bin(B::Eq, v(&field), text(n)),
                then_body: matched,
                else_body: Some(dispatch),
            }];
        }
        work.push(branch(
            bin(
                B::Or,
                bin(B::Eq, v(&code), i(0)),
                bin(B::Eq, v(&code), i(2)),
            ),
            dispatch,
        ));
        out.push(S::DoWhile {
            cond: bin(B::Eq, v(&code), i(0)),
            body: work,
        });
        out.push(call("@nml_end", vec![v(&h)]));
        out.push(branch(bin(B::Eq, v(&code), i(-2)), vec![set(&code, i(0))]));
    } else {
        ensure!(
            !matches!(unit,E::Ident(n) if matches!(symbols.get(n),Some((T::Character(_)|T::CharacterLength(_),_,_)))),
            "internal NAMELIST WRITE is not yet supported"
        );
        let unit_temp = temp("unit");
        out.extend([
            decl(T::Integer(Some(8)), &unit_temp),
            set(&unit_temp, unit.clone()),
        ]);
        let unit = v(&unit_temp);
        let record = temp("record");
        out.push(decl(T::Character(Some(8192)), &record));
        let emit = |e: E| {
            branch(
                bin(B::Eq, v(&code), i(0)),
                vec![call("@nml_record", vec![v(&code), unit.clone(), e])],
            )
        };
        out.push(emit(text(&format!("&{group}"))));
        for (m, n) in members.iter().enumerate() {
            let (kind, dims, _) = &symbols[n];
            let ix = format!("@nml_{serial}_item_{m}");
            out.push(decl(T::Integer(Some(8)), &ix));
            let item = element(n, dims, v(&ix));
            let mut body = vec![];
            let format;
            let mut values = vec![];
            match kind {
                T::Integer(_)|T::IntegerKind(_)=>{format="(i0,a)";values.push(item);},
                T::Real|T::RealKind(_)|T::DoublePrecision=>{format="(es26.17,a)";values.push(item);},
                T::Logical|T::LogicalKind(_)=>{format="(l1,a)";values.push(item);},
                T::Complex(_)=>{format="('(',es26.17,',',es26.17,')',a)";values.extend([E::Call("real".into(),vec![item.clone()]),E::Call("aimag".into(),vec![item])]);},
                T::Character(Some(width))=>{
                    ensure!(*width<=4000,"NAMELIST CHARACTER output currently supports lengths up to 4000");
                    let escaped=format!("@nml_{serial}_escaped_{m}");let pos=format!("@nml_{serial}_pos_{m}");let ch=format!("@nml_{serial}_ch_{m}");let copy=format!("@nml_{serial}_copy_{m}");
                    out.extend([decl(T::Character(Some(2*width+2)),&escaped),decl(T::Character(Some(*width)),&copy),decl(T::Integer(Some(8)),&pos),decl(T::Integer(Some(8)),&ch)]);
                    let sub=|n:&str,start:E,end:E|E::Section{name:n.into(),lower:Some(Box::new(start)),upper:Some(Box::new(end)),stride:None};
                    let put=|value:E|S::AssignIndex{name:escaped.clone(),indices:vec![E::Section{name:String::new(),lower:Some(Box::new(v(&pos))),upper:Some(Box::new(v(&pos))),stride:None}],value};
                    let advance=||set(&pos,bin(B::Add,v(&pos),i(1)));
                    body.extend([set(&copy,item),set(&escaped,text("'")),set(&pos,i(2)),S::Do{var:ch.clone(),start:i(1),end:i(*width as i64),body:vec![put(sub(&copy,v(&ch),v(&ch))),advance(),branch(bin(B::Eq,sub(&copy,v(&ch),v(&ch)),text("'")),vec![put(text("'")),advance()])]},put(text("'"))]);
                    format="(a,a)";values.push(sub(&escaped,i(1),v(&pos)));
                },_=>bail!("NAMELIST output currently requires an intrinsic type with a fixed CHARACTER length"),
            }
            values.push(text(","));
            let mut write = vec![v(&record), text(format)];
            write.extend(values);
            body.push(call("@write", write));
            body.push(emit(E::Call("trim".into(), vec![v(&record)])));
            out.push(emit(text(&format!(" {n}="))));
            out.push(S::Do {
                var: ix,
                start: i(1),
                end: count(dims),
                body: vec![branch(bin(B::Eq, v(&code), i(0)), body)],
            });
        }
        out.push(emit(text("/")));
    }
    if let E::Ident(n) = message {
        let mut error = vec![set(
            n,
            text("NAMELIST input/output error (invalid name, value, bounds or unit)"),
        )];
        error.push(branch(
            bin(B::Eq, v(&code), i(-1)),
            vec![set(n, text("End of file while reading NAMELIST"))],
        ));
        out.push(branch(bin(B::Ne, v(&code), i(0)), error));
    }
    if let E::Ident(n) = status {
        out.push(set(n, v(&code)));
    } else {
        out.push(branch(
            bin(B::Ne, v(&code), i(0)),
            vec![call(
                "@error_stop",
                vec![text("NAMELIST input/output error")],
            )],
        ));
    }
    Ok(S::Block { body: out })
}

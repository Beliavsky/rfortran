// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate timing and environment intrinsic outputs and lower them to ordinary assignments.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
fn assign(target: &Expr, value: Expr) -> Stmt {
    match target {
        Expr::Ident(name) => Stmt::Assign {
            name: name.clone(),
            value,
        },
        Expr::Index(name, indices) | Expr::Call(name, indices) => Stmt::AssignIndex {
            name: name.clone(),
            indices: indices.clone(),
            value,
        },
        _ => unreachable!("validated output"),
    }
}
pub fn resolve(mut p: Program) -> Result<Program> {
    fn collect(body: &[Stmt], procedures: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    procedures.insert(name.clone());
                    collect(body, procedures);
                }
                Stmt::Module { body, .. } | Stmt::Block { body } => collect(body, procedures),
                _ => {}
            }
        }
    }
    fn scope(
        body: &mut Vec<Stmt>,
        inherited: &HashMap<String, (TypeSpec, usize)>,
        inherited_readonly: &HashSet<String>,
        procedures: &HashSet<String>,
        pure: bool,
        serial: &mut usize,
        types: &crate::operators::ExpressionTypes,
    ) -> Result<()> {
        let mut vars = inherited.clone();
        let mut readonly = inherited_readonly.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        readonly.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    readonly.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        readonly.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        vars.insert(n.clone(), (kind.clone(), 0));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    vars.insert(name.clone(), (kind.clone(), dims.len()));
                }
                Stmt::InitializedDecl {
                    kind,
                    entities,
                    attributes,
                } => {
                    for (name, _) in entities {
                        vars.entry(name.clone())
                            .or_insert_with(|| (kind.clone(), 0));
                        if attributes.iter().any(|a| a == "parameter") {
                            readonly.insert(name.clone());
                        }
                    }
                }
                Stmt::DummyIntent { name, intent }
                    if matches!(intent.as_str(), "parameter" | "intent(in)") =>
                {
                    readonly.insert(name.clone());
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(offset) = &s {
                location = Some(*offset);
            }
            let result = match &mut s {
                Stmt::CallSub { name, args }
                    if matches!(name.as_str(), "cpu_time" | "system_clock" | "date_and_time" | "get_environment_variable" | "getcwd" | "get_command_argument")
                        && !procedures.contains(name) =>
                {
                    lower_call(name, args, &vars, &readonly, pure, serial,types).map(|statements| {
                        s = Stmt::Block { body: statements };
                    })
                }
                Stmt::Function { body, pure, .. } | Stmt::Subroutine { body, pure, .. } => {
                    scope(body, &vars, &readonly, procedures, *pure, serial,types)
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => {
                    scope(body, &vars, &readonly, procedures, pure, serial,types)
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scope(then_body, &vars, &readonly, procedures, pure, serial,types)?;
                    if let Some(body) = else_body {
                        scope(body, &vars, &readonly, procedures, pure, serial,types)?;
                    }
                    Ok(())
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        scope(&mut case.body, &vars, &readonly, procedures, pure, serial,types)?;
                    }
                    if let Some(body) = default {
                        scope(body, &vars, &readonly, procedures, pure, serial,types)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
            result.map_err(|error| crate::source_diagnostics::at(error, location))?;
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    fn environment_call(body:&[Stmt])->bool {
        body.iter().any(|s|match s {
            Stmt::VarDecl{kind:TypeSpec::CharacterLength(_),names}=>names.iter().any(|n|n.starts_with("@clock_env_value_")),
            Stmt::Function{body,..}|Stmt::Subroutine{body,..}|Stmt::Module{body,..}|Stmt::Block{body}|Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>environment_call(body),
            Stmt::If{then_body,else_body,..}=>environment_call(then_body)||else_body.as_ref().is_some_and(|b|environment_call(b)),
            Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|environment_call(&c.body))||default.as_ref().is_some_and(|b|environment_call(b)),
            _=>false,
        })
    }
    let types=crate::operators::ExpressionTypes::new(&p)?;
    let mut procedures = HashSet::new();
    collect(&p.body, &mut procedures);
    scope(
        &mut p.body,
        &HashMap::new(),
        &HashSet::new(),
        &procedures,
        false,
        &mut 0,
        &types,
    )?;
    // Environment output temporaries can depend on LEN of an assumed-length
    // dummy. Resolve their declaration lengths before array specialization.
    if environment_call(&p.body) {crate::parameters::resolve_constants(p)} else {Ok(p)}
}

fn lower_call(
    name: &str,
    args: &mut Vec<Expr>,
    vars: &HashMap<String, (TypeSpec, usize)>,
    readonly: &HashSet<String>,
    pure: bool,
    serial: &mut usize,
    types: &crate::operators::ExpressionTypes,
) -> Result<Vec<Stmt>> {
    let intrinsic = name.to_uppercase();
    if pure {
        bail!("{intrinsic} is not permitted in a PURE procedure");
    }
    crate::named_arguments::intrinsic(name)
        .unwrap()
        .reorder(name, args)?;
    if name == "getcwd" {return lower_cwd(args,vars,readonly,serial);}
    if matches!(name,"get_environment_variable"|"get_command_argument") {return lower_environment(name,args,vars,readonly,serial,types);}
    if name == "date_and_time" {
        return lower_date(args, vars, readonly, serial);
    }
    let mut kinds = vec![];
    for (slot, arg) in args.iter().enumerate() {
        if crate::named_arguments::omitted(arg) {
            kinds.push(None);
            continue;
        }
        let variable = match arg {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => n,
            _ => bail!("{intrinsic} requires a writable scalar variable"),
        };
        let (kind, rank) = vars
            .get(variable)
            .context("timing intrinsic requires a declared writable scalar variable")?;
        let scalar = match arg {
            Expr::Ident(_) => *rank == 0,
            Expr::Index(_, indices) | Expr::Call(_, indices) => {
                *rank > 0
                    && indices.len() == *rank
                    && indices.iter().all(|index| scalar_index(index, vars))
            }
            _ => false,
        };
        if !scalar {
            bail!("{intrinsic} requires a scalar output");
        }
        if readonly.contains(variable) {
            bail!("{intrinsic} requires a writable variable");
        }
        let real = matches!(kind, TypeSpec::Real | TypeSpec::DoublePrecision);
        if name == "cpu_time" && !real {
            bail!("CPU_TIME TIME requires a REAL variable");
        }
        if name == "system_clock"
            && !(matches!(kind, TypeSpec::Integer(None | Some(4 | 8))) || slot == 1 && real)
        {
            bail!(
                "SYSTEM_CLOCK {} requires {}",
                ["COUNT", "COUNT_RATE", "COUNT_MAX"][slot],
                if slot == 1 {
                    "INTEGER kind 4 or 8, or REAL"
                } else {
                    "INTEGER kind 4 or 8"
                }
            );
        }
        kinds.push(Some(kind.clone()));
    }
    let mut out = vec![];
    // Bind subscript expressions before writing any output, as procedure actuals do.
    for arg in args.iter_mut() {
        if crate::named_arguments::omitted(arg) {
            continue;
        }
        if let Expr::Index(_, indices) | Expr::Call(_, indices) = arg {
            for index in indices {
                let temp = temporary(serial, "subscript");
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![temp.clone()],
                });
                out.push(Stmt::Assign {
                    name: temp.clone(),
                    value: index.clone(),
                });
                *index = Expr::Ident(temp);
            }
        }
    }
    if name == "cpu_time" {
        let value = Expr::Call("@cpu_seconds".into(), vec![]);
        let value = if matches!(kinds[0], Some(TypeSpec::Real)) {
            Expr::Call("real".into(), vec![value])
        } else {
            value
        };
        out.push(assign(&args[0], value));
        return Ok(out);
    }
    if args.is_empty() {
        return Ok(out);
    }
    // Select COUNT's representation first, then MAX, then an integer RATE.
    let kind = [0, 2, 1]
        .into_iter()
        .find_map(|slot| match kinds.get(slot) {
            Some(Some(TypeSpec::Integer(bytes))) => Some(bytes.unwrap_or(4)),
            _ => None,
        })
        .unwrap_or(4);
    if kinds
        .iter()
        .flatten()
        .any(|k| matches!(k, TypeSpec::Integer(bytes) if bytes.unwrap_or(4) != kind))
    {
        bail!("SYSTEM_CLOCK integer arguments must have the same kind");
    }
    let temp = temporary(serial, "snapshot");
    out.push(Stmt::VarDecl {
        kind: TypeSpec::Integer(Some(8)),
        names: vec![temp.clone()],
    });
    out.push(Stmt::Assign {
        name: temp.clone(),
        value: Expr::Call("@clock_count".into(), vec![Expr::IntLit(kind.to_string())]),
    });
    if !crate::named_arguments::omitted(&args[0]) {
        out.push(assign(&args[0], Expr::Ident(temp.clone())));
    }
    let mut success = vec![];
    let mut failure = vec![];
    for slot in 1..args.len() {
        if crate::named_arguments::omitted(&args[slot]) {
            continue;
        }
        let value = if slot == 1 {
            crate::timing::rate(i64::from(kind))
        } else {
            crate::timing::maximum(i64::from(kind))
        };
        let real = matches!(
            kinds[slot],
            Some(TypeSpec::Real | TypeSpec::DoublePrecision)
        );
        let literal = |n: i64| {
            if real {
                Expr::RealLit(format!("{n}.0"))
            } else {
                Expr::IntLit(n.to_string())
            }
        };
        success.push(assign(&args[slot], literal(value)));
        failure.push(assign(&args[slot], literal(0)));
    }
    out.push(Stmt::If {
        cond: Expr::Bin(
            crate::ast::BinOp::Ge,
            Box::new(Expr::Ident(temp)),
            Box::new(Expr::IntLit("0".into())),
        ),
        then_body: success,
        else_body: Some(failure),
    });
    Ok(out)
}
fn temporary(serial: &mut usize, label: &str) -> String {
    let name = format!("@clock_{label}_{}", *serial);
    *serial += 1;
    name
}
fn scalar_index(expr: &Expr, vars: &HashMap<String, (TypeSpec, usize)>) -> bool {
    match expr {
        Expr::Ident(n) => !vars.get(n).is_some_and(|(_, rank)| *rank != 0),
        Expr::Bin(_, a, b) => scalar_index(a, vars) && scalar_index(b, vars),
        Expr::Un(_, a) => scalar_index(a, vars),
        Expr::Call(n, _)
            if n.starts_with("@section")
                || n == "@range"
                || n == "@array_constructor"
                || n == "@typed_constructor" =>
        {
            false
        }
        Expr::Call(n, args) | Expr::Index(n, args)
            if vars.get(n).is_some_and(|(_, rank)| *rank > 0) =>
        {
            args.iter().all(|a| scalar_index(a, vars))
        }
        Expr::Call(_, args) => args.iter().all(|a| scalar_index(a, vars)),
        _ => true,
    }
}

fn lower_date(
    args: &[Expr],
    vars: &HashMap<String, (TypeSpec, usize)>,
    readonly: &HashSet<String>,
    serial: &mut usize,
) -> Result<Vec<Stmt>> {
    use crate::ast::BinOp;
    let mut out = vec![];
    for (slot, arg) in args.iter().enumerate() {
        if crate::named_arguments::omitted(arg) {
            continue;
        }
        let variable = match arg {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => n,
            _ => bail!("DATE_AND_TIME requires a writable variable"),
        };
        let (kind, rank) = vars
            .get(variable)
            .context("DATE_AND_TIME requires a declared writable variable")?;
        if readonly.contains(variable) {
            bail!("DATE_AND_TIME requires a writable variable");
        }
        if slot == 3 {
            if !matches!(arg, Expr::Ident(_))
                || *rank != 1
                || !matches!(kind, TypeSpec::Integer(None | Some(2 | 4 | 8)))
            {
                bail!("DATE_AND_TIME VALUES requires a rank-one INTEGER array of kind 2, 4 or 8");
            }
            out.push(Stmt::If {
                cond: Expr::Bin(
                    BinOp::Lt,
                    Box::new(Expr::Call("size".into(), vec![arg.clone()])),
                    Box::new(Expr::IntLit("8".into())),
                ),
                then_body: vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(
                        "DATE_AND_TIME VALUES requires at least 8 elements".into(),
                    )],
                }],
                else_body: None,
            });
        } else {
            let scalar = match arg {
                Expr::Ident(_) => *rank == 0,
                Expr::Index(_, indices) | Expr::Call(_, indices) => {
                    *rank > 0
                        && indices.len() == *rank
                        && indices.iter().all(|i| scalar_index(i, vars))
                }
                _ => false,
            };
            if !scalar
                || !matches!(
                    kind,
                    TypeSpec::Character(_)
                        | TypeSpec::CharacterAssumed
                        | TypeSpec::CharacterLength(_)
                )
            {
                bail!(
                    "DATE_AND_TIME {} requires a scalar CHARACTER variable",
                    ["DATE", "TIME", "ZONE"][slot]
                );
            }
        }
    }
    if args.is_empty() {
        return Ok(out);
    }
    let mut targets = args.to_vec();
    for arg in targets.iter_mut().take(3) {
        if crate::named_arguments::omitted(arg) {
            continue;
        }
        if let Expr::Index(_, indices) | Expr::Call(_, indices) = arg {
            for index in indices {
                let temp = temporary(serial, "subscript");
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![temp.clone()],
                });
                out.push(Stmt::Assign {
                    name: temp.clone(),
                    value: index.clone(),
                });
                *index = Expr::Ident(temp);
            }
        }
    }
    let buffer = temporary(serial, "calendar");
    out.push(Stmt::ArrayDecl {
        kind: TypeSpec::Integer(Some(8)),
        name: buffer.clone(),
        dims: vec![Expr::IntLit("8".into())],
    });
    let field = |index: usize| Expr::Index(buffer.clone(), vec![Expr::IntLit(index.to_string())]);
    out.push(Stmt::CallSub {
        name: "@date_snapshot".into(),
        args: (1..=8).map(field).collect(),
    });
    let mut success = vec![];
    let mut failure = vec![];
    for (slot, arg) in targets.iter().enumerate() {
        if crate::named_arguments::omitted(arg) {
            continue;
        }
        if slot == 3 {
            let Expr::Ident(array) = arg else {
                unreachable!()
            };
            let first = Expr::Call("lbound".into(), vec![arg.clone(), Expr::IntLit("1".into())]);
            let (kind, _) = &vars[array];
            let max = match kind {
                TypeSpec::Integer(Some(2)) => i64::from(i16::MAX),
                TypeSpec::Integer(Some(8)) => i64::MAX,
                _ => i64::from(i32::MAX),
            };
            for index in 0..8 {
                let target = Expr::Index(
                    array.clone(),
                    vec![Expr::Bin(
                        BinOp::Add,
                        Box::new(first.clone()),
                        Box::new(Expr::IntLit(index.to_string())),
                    )],
                );
                success.push(assign(&target, field(index + 1)));
                failure.push(assign(&target, Expr::IntLit((-max).to_string())));
            }
        } else {
            let value = match slot {
                0 => concat(vec![
                    digits(field(1), 4),
                    digits(field(2), 2),
                    digits(field(3), 2),
                ]),
                1 => concat(vec![
                    digits(field(5), 2),
                    digits(field(6), 2),
                    digits(field(7), 2),
                    Expr::Str(".".into()),
                    digits(field(8), 3),
                ]),
                _ => {
                    let offset = Expr::Call("abs".into(), vec![field(4)]);
                    let suffix = concat(vec![
                        digits(
                            Expr::Bin(
                                BinOp::Div,
                                Box::new(offset.clone()),
                                Box::new(Expr::IntLit("60".into())),
                            ),
                            2,
                        ),
                        digits(
                            Expr::Call("mod".into(), vec![offset, Expr::IntLit("60".into())]),
                            2,
                        ),
                    ]);
                    success.push(Stmt::If {
                        cond: Expr::Bin(
                            BinOp::Lt,
                            Box::new(field(4)),
                            Box::new(Expr::IntLit("0".into())),
                        ),
                        then_body: vec![assign(
                            arg,
                            concat(vec![Expr::Str("-".into()), suffix.clone()]),
                        )],
                        else_body: Some(vec![assign(
                            arg,
                            concat(vec![Expr::Str("+".into()), suffix]),
                        )]),
                    });
                    failure.push(assign(arg, Expr::Str(" ".into())));
                    continue;
                }
            };
            success.push(assign(arg, value));
            failure.push(assign(arg, Expr::Str(" ".into())));
        }
    }
    out.push(Stmt::If {
        cond: Expr::Bin(
            BinOp::Ge,
            Box::new(field(1)),
            Box::new(Expr::IntLit("0".into())),
        ),
        then_body: success,
        else_body: Some(failure),
    });
    Ok(out)
}
fn concat(parts: Vec<Expr>) -> Expr {
    parts
        .into_iter()
        .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Concat, Box::new(a), Box::new(b)))
        .unwrap()
}
fn digits(value: Expr, width: u32) -> Expr {
    use crate::ast::BinOp;
    concat(
        (0..width)
            .rev()
            .map(|position| {
                let digit = Expr::Call(
                    "mod".into(),
                    vec![
                        Expr::Bin(
                            BinOp::Div,
                            Box::new(value.clone()),
                            Box::new(Expr::IntLit(10i64.pow(position).to_string())),
                        ),
                        Expr::IntLit("10".into()),
                    ],
                );
                Expr::Call(
                    "achar".into(),
                    vec![Expr::Call(
                        "int".into(),
                        vec![Expr::Bin(
                            BinOp::Add,
                            Box::new(Expr::IntLit("48".into())),
                            Box::new(digit),
                        )],
                    )],
                )
            })
            .collect(),
    )
}

fn lower_environment(name:&str,args:&[Expr],vars:&HashMap<String,(TypeSpec,usize)>,readonly:&HashSet<String>,serial:&mut usize,types:&crate::operators::ExpressionTypes)->Result<Vec<Stmt>> {
    let intrinsic=name.to_ascii_uppercase();
    let command=name=="get_command_argument";
    let kind=|e:&Expr|crate::inquiries::model_type(e,&|n|vars.get(n).cloned());
    let character=|k:&TypeSpec|matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_));
    fn text(e:&Expr,vars:&HashMap<String,(TypeSpec,usize)>)->bool {
        match e {
            Expr::Str(_)=>true,
            Expr::Bin(crate::ast::BinOp::Concat,a,b)=>text(a,vars)&&text(b,vars),
            Expr::Call(n,a) if matches!(n.as_str(),"trim"|"adjustl"|"adjustr") && a.len()==1=>text(&a[0],vars),
            _=>crate::inquiries::model_type(e,&|n|vars.get(n).cloned()).ok().flatten().is_some_and(|k|matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_))),
        }
    }
    if command {
        let inferred=types.kind_rank(&args[0],vars)?;
        let input=kind(&args[0]).ok().flatten().or_else(||inferred.as_ref().map(|(k,_)|k.clone()));
        let scalar=inferred.as_ref().map_or_else(||scalar_index(&args[0],vars),|(_,rank)|*rank==0);
        if !matches!(input,Some(TypeSpec::Integer(None|Some(4)))) || !scalar {bail!("GET_COMMAND_ARGUMENT NUMBER requires scalar default INTEGER");}
    } else if !text(&args[0],vars) || !scalar_index(&args[0],vars) {bail!("GET_ENVIRONMENT_VARIABLE NAME requires scalar CHARACTER");}
    let supplied=|i:usize|args.get(i).filter(|e|!crate::named_arguments::omitted(e));
    if let Some(e)=supplied(4) {if kind(e)?!=Some(TypeSpec::Logical) || !scalar_index(e,vars) {bail!("GET_ENVIRONMENT_VARIABLE TRIM_NAME requires scalar LOGICAL");}}
    for i in 1..=3 {if let Some(e)=supplied(i) {
        let n=match e {Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)=>n,_=>bail!("{intrinsic} requires writable output variables")};
        let (k,rank)=vars.get(n).with_context(||format!("{intrinsic} requires declared output variables"))?;
        let scalar=match e {Expr::Ident(_)=>*rank==0,Expr::Index(_,a)|Expr::Call(_,a)=>*rank>0 && a.len()==*rank && a.iter().all(|e|scalar_index(e,vars)),_=>false};
        if !scalar || readonly.contains(n) {bail!("{intrinsic} requires writable scalar output variables");}
        if i==1 {if !character(k) {bail!("{intrinsic} VALUE requires CHARACTER");}}
        else if !matches!(k,TypeSpec::Integer(None|Some(4))) {bail!("{intrinsic} LENGTH and STATUS require default INTEGER");}
    }}
    let status=temporary(serial,"env_status");let length=temporary(serial,"env_length");let value=temporary(serial,"env_value");
    let width=if let Some(e)=supplied(1) {
        let n=match e {Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)=>n,_=>unreachable!()};
        match &vars[n].0 {
            TypeSpec::Character(size)=>Expr::IntLit(size.unwrap_or(1).to_string()),
            _=>Expr::Call("len".into(),vec![Expr::Ident(n.clone())]),
        }
    } else {Expr::IntLit("0".into())};
    let value_kind=if let Expr::IntLit(n)=&width {TypeSpec::Character(Some(n.parse()?))} else {TypeSpec::CharacterLength(Box::new(width.clone()))};
    let mut out=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![status.clone(),length.clone()]},
        Stmt::VarDecl{kind:value_kind,names:vec![value.clone()]},
        Stmt::CallSub{name:if command {"@argument_query"} else {"@environment_query"}.into(),args:vec![Expr::Ident(status.clone()),Expr::Ident(length.clone()),Expr::Ident(value.clone()),args[0].clone(),supplied(4).cloned().unwrap_or(Expr::Logical(true)),width,Expr::Logical(supplied(1).is_some())]}];
    for (i,temp) in [(1,value),(2,length),(3,status)] {if let Some(target)=supplied(i) {out.push(assign(target,Expr::Ident(temp)));}}
    Ok(out)
}

fn lower_cwd(args:&[Expr],vars:&HashMap<String,(TypeSpec,usize)>,readonly:&HashSet<String>,serial:&mut usize)->Result<Vec<Stmt>> {
    for (i,e) in args.iter().enumerate() {
        if crate::named_arguments::omitted(e) {continue;}
        let n=match e {Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)=>n,_=>bail!("GETCWD requires writable scalar output variables")};
        let (kind,rank)=vars.get(n).context("GETCWD requires declared output variables")?;
        let scalar=match e {Expr::Ident(_)=>*rank==0,Expr::Index(_,a)|Expr::Call(_,a)=>*rank>0 && a.len()==*rank && a.iter().all(|e|scalar_index(e,vars)),_=>false};
        if !scalar || readonly.contains(n) {bail!("GETCWD requires writable scalar output variables");}
        if i==0 {if !matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_)) {bail!("GETCWD CWD requires CHARACTER");}}
        else if !matches!(kind,TypeSpec::Integer(None|Some(4|8))) {bail!("GETCWD STATUS requires INTEGER kind 4 or 8");}
    }
    // Capture output through its declared length, without reading unset text.
    let n=match &args[0] {Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)=>n,_=>unreachable!()};
    let width=match &vars[n].0 {TypeSpec::Character(size)=>Expr::IntLit(size.unwrap_or(1).to_string()),_=>Expr::Call("len".into(),vec![Expr::Ident(n.clone())])};
    let status=temporary(serial,"cwd_status");let value=temporary(serial,"env_value");
    let kind=if let Expr::IntLit(n)=&width {TypeSpec::Character(Some(n.parse()?))} else {TypeSpec::CharacterLength(Box::new(width.clone()))};
    let mut out=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![status.clone()]},Stmt::VarDecl{kind,names:vec![value.clone()]},
        Stmt::CallSub{name:"@cwd_query".into(),args:vec![Expr::Ident(status.clone()),Expr::Ident(value.clone()),width]},assign(&args[0],Expr::Ident(value))];
    if let Some(target)=args.get(1).filter(|e|!crate::named_arguments::omitted(e)) {out.push(assign(target,Expr::Ident(status)));}
    Ok(out)
}

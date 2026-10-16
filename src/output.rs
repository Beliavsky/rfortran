// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower list-directed output implied-DOs to ordinary checked control flow.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// Normalize positional/named controls once, so all later passes see the
/// existing UNIT, FMT, options, items layout (including reordered keywords).
pub(crate) fn write_controls(
    controls: Vec<Expr>,
    items: Vec<Expr>,
) -> std::result::Result<Stmt, &'static str> {
    let mut unit = None;
    let mut format = None;
    let mut options = vec![];
    let mut keywords = std::collections::HashSet::new();
    let mut named = false;
    for (position, control) in controls.into_iter().enumerate() {
        let (key, value) = if let Expr::Call(name, args) = &control {
            if let Some(key) = name.strip_prefix("@write_control:") {
                named = true;
                (key.to_ascii_lowercase(), args[0].clone())
            } else {
                if named {
                    return Err("positional WRITE controls must precede named controls");
                }
                (
                    match position {
                        0 => "unit",
                        1 => "fmt",
                        _ => return Err("only UNIT and FMT may be positional in WRITE"),
                    }
                    .into(),
                    control,
                )
            }
        } else {
            if named {
                return Err("positional WRITE controls must precede named controls");
            }
            (
                match position {
                    0 => "unit",
                    1 => "fmt",
                    _ => return Err("only UNIT and FMT may be positional in WRITE"),
                }
                .into(),
                control,
            )
        };
        if !keywords.insert(key.clone()) {
            return Err("duplicate WRITE control specifier");
        }
        let star = matches!(&value,Expr::Call(n,_) if n=="@write_star");
        match key.as_str() {
            "unit" => {
                unit = Some(if star {
                    Expr::IntLit("6".into())
                } else {
                    value
                })
            }
            "fmt" => {
                format = Some(if star {
                    Expr::Call("@list_directed".into(), vec![])
                } else {
                    value
                })
            }
            _ => {
                if star {
                    return Err("only WRITE UNIT and FMT may use '*'");
                }
                options.push(Expr::Call(format!("@write_option:{key}"), vec![value]));
            }
        }
    }
    let mut args = vec![
        unit.ok_or("WRITE requires UNIT")?,
        format.ok_or("WRITE currently requires FMT")?,
    ];
    args.extend(options);
    args.extend(items);
    Ok(Stmt::CallSub {
        name: "@write".into(),
        args,
    })
}

type Symbols = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
pub(crate) fn require_stdout(unit: Expr) -> Stmt {
    Stmt::If {cond:bin(BinOp::Ne,unit,int(6)),then_body:vec![Stmt::CallSub {
        name:"@error_stop".into(),args:vec![Expr::Str("this WRITE requires standard output unit 6; external file WRITE currently supports only (A) without control options".into())],
    }],else_body:None}
}
fn fragment(e: Expr) -> Stmt {
    Stmt::CallSub {
        name: "@print_fragment".into(),
        args: vec![e],
    }
}

pub fn contains_implied_do(e: &Expr) -> bool {
    match e {
        Expr::Call(n, _) if n == "@array_constructor" => false,
        Expr::Call(n, _) if n == "@output_implied_do" => true,
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(contains_implied_do),
        Expr::Bin(_, a, b) => contains_implied_do(a) || contains_implied_do(b),
        Expr::Un(_, a) => contains_implied_do(a),
        _ => false,
    }
}

pub(crate) fn integer(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::IntLit(_) => true,
        Expr::Ident(n) => {
            matches!(symbols.get(n), Some((TypeSpec::Integer(None | Some(4) | Some(8)), dims)) if dims.is_empty())
        }
        Expr::Un(UnOp::Neg, a) => integer(a, symbols),
        Expr::Bin(BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow, a, b) => {
            integer(a, symbols) && integer(b, symbols)
        }
        Expr::Call(n, args) if matches!(n.as_str(), "min" | "max" | "mod" | "abs") => {
            !args.is_empty() && args.iter().all(|e| integer(e, symbols))
        }
        Expr::Call(n, args) if matches!(n.as_str(), "size" | "len" | "len_trim" | "int") => {
            args.len() == 1
        }
        _ => false,
    }
}

fn character(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Str(_) | Expr::Bin(BinOp::Concat, _, _) => true,
        Expr::Ident(n) => matches!(symbols.get(n), Some((TypeSpec::Character(_), _))),
        Expr::Call(n, _) => matches!(
            n.as_str(),
            "trim" | "adjustl" | "adjustr" | "achar" | "char" | "repeat"
        ),
        _ => false,
    }
}

pub fn lower(items: &[Expr], symbols: &Symbols, serial: &mut usize) -> Result<Stmt> {
    *serial += 1;
    let previous = format!("@output_character_{serial}");
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Logical,
            names: vec![previous.clone()],
        },
        assign(&previous, Expr::Logical(false)),
    ];
    emit(
        items,
        symbols,
        serial,
        &previous,
        &mut Vec::new(),
        &mut body,
        None,
    )?;
    body.push(Stmt::Print { items: vec![] });
    Ok(Stmt::Block { body })
}

fn emit(
    items: &[Expr],
    symbols: &Symbols,
    serial: &mut usize,
    previous: &str,
    active: &mut Vec<String>,
    body: &mut Vec<Stmt>,
    session: Option<&str>,
) -> Result<()> {
    for item in items {
        if let Expr::Call(tag, args) = item {
            if tag == "@output_implied_do" {
                let Expr::Ident(var) = &args[0] else {
                    bail!("output implied-DO variable must be a writable scalar INTEGER of kind 4 or 8");
                };
                if !integer(&args[0], symbols) {
                    bail!("output implied-DO variable `{var}` must be a declared scalar INTEGER of kind 4 or 8");
                }
                if active.contains(var) {
                    bail!("nested output implied-DO cannot reuse active variable `{var}`");
                }
                if !args[1..4].iter().all(|e| integer(e, symbols)) {
                    bail!("output implied-DO bounds and step require supported scalar INTEGER expressions");
                }
                if crate::parameters::integer_constant(&args[3]).ok() == Some(0) {
                    bail!("output implied-DO step must not be zero");
                }
                *serial += 1;
                let first = format!("@output_first_{serial}");
                let last = format!("@output_last_{serial}");
                let step = format!("@output_step_{serial}");
                body.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![first.clone(), last.clone(), step.clone()],
                });
                // Evaluate controls exactly once, before assigning the control variable.
                body.extend([
                    assign(&first, args[1].clone()),
                    assign(&last, args[2].clone()),
                    assign(&step, args[3].clone()),
                ]);
                body.push(Stmt::If {
                    cond: bin(BinOp::Eq, id(&step), int(0)),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("output implied-DO step must not be zero".into())],
                    }],
                    else_body: None,
                });
                body.push(assign(var, id(&first)));
                if let (Ok(first),Ok(last),Ok(step))=(crate::parameters::integer_constant(&args[1]),crate::parameters::integer_constant(&args[2]),crate::parameters::integer_constant(&args[3])) {
                    if step>0 && first>last || step<0 && first<last {continue;}
                }
                active.push(var.clone());
                let mut inner = Vec::new();
                emit(
                    &args[4..],
                    symbols,
                    serial,
                    previous,
                    active,
                    &mut inner,
                    session,
                )?;
                active.pop();
                // Avoid wrapping the final increment into an infinite loop at i64 limits.
                let overflow = vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(
                        "output implied-DO control variable overflow".into(),
                    )],
                }];
                inner.push(Stmt::If {
                    cond: bin(BinOp::Gt, id(&step), int(0)),
                    then_body: vec![Stmt::If {
                        cond: bin(
                            BinOp::Gt,
                            id(var),
                            bin(BinOp::Sub, int(i64::MAX), id(&step)),
                        ),
                        then_body: overflow.clone(),
                        else_body: None,
                    }],
                    else_body: Some(vec![Stmt::If {
                        cond: bin(
                            BinOp::Lt,
                            id(var),
                            bin(BinOp::Sub, int(i64::MIN), id(&step)),
                        ),
                        then_body: overflow,
                        else_body: None,
                    }]),
                });
                inner.push(assign(var, bin(BinOp::Add, id(var), id(&step))));
                body.push(Stmt::DoWhile {
                    cond: bin(
                        BinOp::Or,
                        bin(
                            BinOp::And,
                            bin(BinOp::Gt, id(&step), int(0)),
                            bin(BinOp::Le, id(var), id(&last)),
                        ),
                        bin(
                            BinOp::And,
                            bin(BinOp::Lt, id(&step), int(0)),
                            bin(BinOp::Ge, id(var), id(&last)),
                        ),
                    ),
                    body: inner,
                });
                continue;
            }
        }
        if contains_implied_do(item) {
            bail!("output implied-DO must be a direct list-directed output item");
        }
        if let Some(session)=session {
            // Vector selections, function results and multidimensional views
            // acquire their storage later. Preserve the complete item until
            // reduction lowering can emit each of its active elements.
            body.push(Stmt::CallSub {name:"@format_item".into(),args:vec![id(session),item.clone()]});
            continue;
        }
        let arrays = symbols
            .iter()
            .filter(|(_, (_, dims))| !dims.is_empty())
            .map(|(n, (_, dims))| (n.clone(), dims.clone()))
            .collect();
        let mut expanded = Vec::new();
        if matches!(item,Expr::Section {name,..} if matches!(symbols.get(name),Some((TypeSpec::Character(_),dims)) if dims.is_empty()))
        {
            expanded.push(item.clone());
        } else {
            crate::sections::expand(item.clone(), &arrays, &mut expanded)?;
        }
        // Whole arrays and sections keep their existing element order.
        let mut scalars = Vec::new();
        for e in expanded {
            if let Expr::Ident(n) = &e {
                if let Some((_, dims)) = symbols.get(n).filter(|(_, dims)| !dims.is_empty()) {
                    if dims.len() != 1 {
                        bail!("output implied-DO array item requires a known flattened shape");
                    }
                    let extent = crate::parameters::integer_constant(&dims[0])?;
                    for i in 1..=extent {
                        scalars.push(Expr::Index(n.clone(), vec![int(i)]));
                    }
                    continue;
                }
            }
            scalars.push(e);
        }
        for e in scalars {
            if let Some(session) = session {
                body.push(Stmt::CallSub {
                    name: "@format_item".into(),
                    args: vec![id(session), e],
                });
                continue;
            }
            let is_character = character(&e, symbols);
            let space = fragment(Expr::Str(" ".into()));
            body.push(if is_character {
                Stmt::If {
                    cond: Expr::Un(UnOp::Not, Box::new(id(previous))),
                    then_body: vec![space],
                    else_body: None,
                }
            } else {
                space
            });
            body.push(fragment(e));
            body.push(assign(previous, Expr::Logical(is_character)));
        }
    }
    Ok(())
}

/// Expand formatted implied-DOs before CHARACTER arrays become scalar slots.
/// Each value is copied into a statement-local list, then formatted together.
pub(crate) fn prepare_formatted(mut program: crate::ast::Program) -> Result<crate::ast::Program> {
    fn ready(e:&Expr,symbols:&Symbols)->bool {
        let storage=|n:&String|symbols.get(n).is_none_or(|(_,d)|d.is_empty()||d.iter().all(|d|crate::parameters::integer_constant(d).is_ok())||symbols.contains_key(&format!("@axes:{n}")));
        match e {
            Expr::Ident(n)=>storage(n),
            Expr::Call(n,a)|Expr::Index(n,a)=>storage(n)&&a.iter().all(|e|ready(e,symbols)),
            Expr::Section{name,lower,upper,stride}=>storage(name)&&[lower,upper,stride].into_iter().flatten().all(|e|ready(e,symbols)),
            Expr::Bin(_,a,b)=>ready(a,symbols)&&ready(b,symbols),Expr::Un(_,a)=>ready(a,symbols),_=>true,
        }
    }
    fn scope(body: &mut Vec<Stmt>, inherited: &Symbols, serial: &mut usize) -> Result<()> {
        let mut symbols = inherited.clone();
        for stmt in body.iter() {
            match stmt {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                _ => {}
            }
        }
        crate::allocatables::metadata(body,&mut symbols);
        for stmt in body.iter_mut() {
            let mut internal = None;
            let mut unit_check = None;
            let mut control_prefix = vec![];
            let selected = if let Stmt::CallSub { name, args } = stmt {
                if name=="@write" && matches!(&args[0],Expr::Ident(n) if matches!(symbols.get(n),Some((TypeSpec::Character(_),dims)) if dims.is_empty())) {
                    let Expr::Ident(target)=&args[0] else {unreachable!()};
                    let (TypeSpec::Character(length),_)=&symbols[target] else {unreachable!()};
                    if args[2..].iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@write_option:"))) {
                        bail!("internal WRITE control options are not yet supported");
                    }
                    if matches!(&args[1],Expr::Call(n,_) if n=="@list_directed") {
                        bail!("list-directed internal WRITE is not yet supported");
                    }
                    internal=Some((target.clone(),length.unwrap_or(1)));
                    Some((args[1].clone(),args[2..].to_vec(),int(0)))
                } else if matches!(
                    name.as_str(),
                    "@formatted_print" | "@formatted_write_noadvance"
                ) && args[1..].iter().any(contains_implied_do) && args[1..].iter().all(|e|ready(e,&symbols))
                {
                    Some((
                        args[0].clone(),
                        args[1..].to_vec(),
                        int(i64::from(name == "@formatted_print")),
                    ))
                } else if name == "@write"
                    && (crate::parameters::integer_constant(&args[0]).ok() == Some(6)
                        || crate::parameters::integer_constant(&args[0]).is_err() && crate::file_io::unit(&args[0],&symbols).is_ok())
                    && args[2..].iter().any(contains_implied_do)
                    && args[2..].iter().all(|e|ready(e,&symbols))
                    && !matches!(&args[1],Expr::Call(n,_) if n=="@list_directed")
                {
                    if crate::parameters::integer_constant(&args[0]).ok()!=Some(6) {unit_check=Some(args[0].clone());}
                    let mut first = 2;
                    let mut advance = int(1);
                    let mut deferred = false;
                    while let Some(Expr::Call(n, a)) = args.get(first) {
                        let Some(option) = n.strip_prefix("@write_option:") else {
                            break;
                        };
                        if option != "advance" {
                            bail!("unsupported WRITE control `{option}`");
                        }
                        match crate::reductions::advance_control(&a[0],&symbols,&mut control_prefix,serial) {
                            Ok(value)=>advance=value,
                            Err(error) if crate::parameters::character_constant(&a[0]).is_ok()=>return Err(error),
                            Err(_)=>{deferred=true;break;},
                        }
                        first += 1;
                    }
                    if deferred {None} else {Some((args[1].clone(), args[first..].to_vec(), advance))}
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((format, items, advance)) = selected {
                *serial += 1;
                let handle = format!("@formatted_list_{serial}");
                let mut inner = vec![
                    Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![handle.clone()],
                    },
                    Stmt::CallSub {
                        name: "@format_begin".into(),
                        args: vec![id(&handle), format, advance],
                    },
                ];
                inner.splice(1..1,control_prefix);
                if let Some(unit)=unit_check {inner.insert(0,require_stdout(unit));}
                emit(
                    &items,
                    &symbols,
                    serial,
                    "",
                    &mut Vec::new(),
                    &mut inner,
                    Some(&handle),
                )?;
                inner.push(if let Some((target,length))=internal {
                    Stmt::CallSub {name:"@internal_format_end".into(),args:vec![id(&target),id(&handle),int(length as i64)]}
                } else {
                    Stmt::CallSub {name:"@format_end".into(),args:vec![id(&handle)]}
                });
                *stmt = Stmt::Block { body: inner };
                continue;
            }
            match stmt {
                Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. }
                | Stmt::Block { body }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => scope(body, &symbols, serial)?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scope(then_body, &symbols, serial)?;
                    if let Some(b) = else_body {
                        scope(b, &symbols, serial)?;
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        scope(&mut c.body, &symbols, serial)?;
                    }
                    if let Some(b) = default {
                        scope(b, &symbols, serial)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    scope(&mut program.body, &Symbols::new(), &mut 0)?;
    Ok(program)
}

/// Turn active CHARACTER array views into I/O implied-DOs before scalarizing
/// their bounded backing storage. Unselected capacity slots are never read.
pub(crate) fn prepare_character_views(mut program:crate::ast::Program)->Result<crate::ast::Program> {
    fn scope(body:&mut Vec<Stmt>,inherited:&Symbols,serial:&mut usize)->Result<()> {
        let mut symbols=inherited.clone();
        for s in body.iter() {match s {
            Stmt::ArrayDecl{kind,name,dims}=>{symbols.insert(name.clone(),(kind.clone(),dims.clone()));},
            Stmt::VarDecl{kind,names}=>for n in names {symbols.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
        }}
        let mut out=vec![];
        for mut s in std::mem::take(body) {
            let items=match &mut s {
                Stmt::Print{items}=>Some(items.as_mut_slice()),
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@formatted_print"|"@formatted_write_noadvance")=>Some(&mut args[1..]),
                Stmt::CallSub{name,args} if name=="@write"=>Some(&mut args[2..]),_=>None,
            };
            if let Some(items)=items {
                for item in items {
                    if let Expr::Call(tag,args)=item {
                        if tag=="@packed_view" && args.len()==2 {
                            if let Expr::Ident(array)=&args[0] {
                                if matches!(symbols.get(array),Some((TypeSpec::Character(_)|TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical,dims)) if dims.len()==1) {
                                    *serial+=1;
                                    let index=format!("@character_view_output_{serial}");
                                    let kind=TypeSpec::Integer(Some(8));
                                    out.push(Stmt::VarDecl{kind:kind.clone(),names:vec![index.clone()]});
                                    symbols.insert(index.clone(),(kind,vec![]));
                                    *item=Expr::Call("@output_implied_do".into(),vec![Expr::Ident(index.clone()),Expr::IntLit("1".into()),args[1].clone(),Expr::IntLit("1".into()),Expr::Index(array.clone(),vec![Expr::Ident(index)])]);
                                }
                            }
                        }
                    }
                }
            }
            match &mut s {
                Stmt::Print{items} if items.iter().any(contains_implied_do)=>s=lower(items,&symbols,serial)?,
                Stmt::Block{body}|Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>scope(body,&symbols,serial)?,
                Stmt::If{then_body,else_body,..}=>{scope(then_body,&symbols,serial)?;if let Some(body)=else_body {scope(body,&symbols,serial)?;}},
                Stmt::Subroutine{body,..}|Stmt::Function{body,..}|Stmt::Module{body,..}=>scope(body,&symbols,serial)?,
                Stmt::SelectCase{cases,default,..}=>{for c in cases {scope(&mut c.body,&symbols,serial)?;}if let Some(body)=default {scope(body,&symbols,serial)?;}},_=>{}
            }
            out.push(s);
        }
        *body=out;
        Ok(())
    }
    scope(&mut program.body,&Symbols::new(),&mut 0)?;
    prepare_formatted(program)
}

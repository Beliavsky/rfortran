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
    let unit = unit.ok_or("WRITE requires UNIT")?;
    // Without FMT the WRITE is unformatted: [unit, control, status, pos, items].
    let Some(format) = format else {
        let (mut control, mut status, mut position) = (String::new(), Expr::IntLit("0".into()), Expr::IntLit("0".into()));
        for option in options {
            let Expr::Call(name, mut values) = option else { unreachable!() };
            match name.trim_start_matches("@write_option:") {
                "iostat" => { control = "iostat".into(); status = values.remove(0); }
                "pos" => position = values.remove(0),
                _ => return Err("unformatted WRITE currently supports UNIT, IOSTAT and POS"),
            }
        }
        let mut args = vec![unit, Expr::Str(control), status, position];
        args.extend(items);
        return Ok(Stmt::CallSub { name: "@unformatted_write".into(), args });
    };
    let mut args = vec![unit, format];
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
        Expr::Call(n,a) if n=="@conditional"=>integer(&a[1],symbols),
        // Inquiries lowered after argument association retain their allocation
        // guard. Its logical status and name do not change the value's type.
        Expr::Call(n,_) if ["@elemental", "@constructor_function"].iter().any(|tag|matches!(symbols.get(&format!("{tag}:{n}")),Some((TypeSpec::Integer(_),_))))=>true,
        Expr::Call(n,a) if n=="@allocated_extent"=>a.get(1).is_some_and(|e|integer(e,symbols)),
        Expr::Call(n,a) if n=="@heap_bound"=>a.len()==3 && a[..2].iter().all(|e|integer(e,symbols)),
        Expr::Call(n,a) if matches!(n.as_str(),"@heap_lower_bound"|"@heap_upper_bound")=>a.len()==2 && a.iter().all(|e|integer(e,symbols)),
        Expr::IntLit(_) => true,
        Expr::Ident(n) => {
            matches!(symbols.get(n), Some((TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)), dims)) if dims.is_empty())
        }
        Expr::Call(n, args) | Expr::Index(n, args)
            if matches!(symbols.get(n), Some((TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)), dims)) if !dims.is_empty() && dims.len()==args.len()) =>
        {
            args.iter().all(|e| integer(e, symbols))
        }
        Expr::Un(UnOp::Neg, a) => integer(a, symbols),
        Expr::Bin(BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow, a, b) => {
            integer(a, symbols) && integer(b, symbols)
        }
        Expr::Call(n, args) if matches!(n.as_str(), "min" | "max" | "mod" | "abs") => {
            !args.is_empty() && args.iter().all(|e| integer(e, symbols))
        }
        Expr::Call(n, args) if matches!(n.as_str(), "size" | "len" | "len_trim" | "int" | "lbound" | "ubound") => {
            args.len() == 1 && !matches!(n.as_str(),"lbound"|"ubound")
                || matches!(n.as_str(),"size"|"lbound"|"ubound") && args.len() == 2 && integer(&args[1], symbols)
        }
        _ => false,
    }
}

fn character(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Call(n,a) if n=="@conditional"=>character(&a[1],symbols),
        Expr::Call(n,_) if matches!(symbols.get(&format!("@constructor_function:{n}")),Some((TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_),_)))=>true,
        Expr::Str(_) | Expr::Bin(BinOp::Concat, _, _) => true,
        Expr::Ident(n) => matches!(symbols.get(n), Some((TypeSpec::Character(_), _))),
        Expr::Index(n,_) | Expr::Call(n,_) if matches!(symbols.get(n),Some((TypeSpec::Character(_),_)))=>true,
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
        // An earlier item may already have forced this statement into fragment
        // output. Keep elemental array expressions as arrays here too, rather
        // than handing an array-valued cast or arithmetic expression to a
        // scalar fragment printer.
        if matches!(item, Expr::Call(..) | Expr::Bin(..) | Expr::Un(..)) {
            *serial += 1;
            let index = format!("@expression_output_{serial}");
            if let Ok((value, Some(count), _)) =
                crate::reductions::elemental_element(item, &id(&index), symbols)
            {
                if let Some(length) = crate::reductions::runtime_extent(item)
                    .or_else(|| (count >= 0).then(|| int(count)))
                {
                    let mut scoped = symbols.clone();
                    let kind = TypeSpec::Integer(Some(8));
                    scoped.insert(index.clone(), (kind.clone(), vec![]));
                    body.push(Stmt::VarDecl { kind, names: vec![index.clone()] });
                    let loop_item = Expr::Call("@output_implied_do".into(),
                        vec![id(&index), int(1), length, int(1), value]);
                    emit(&[loop_item], &scoped, serial, previous, active, body, session)?;
                    continue;
                }
            }
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

/// Expand output implied-DOs before expressions acquire CHARACTER temporaries.
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
        let mut location = None;
        for stmt in body.iter_mut() {
            if let Some(offset)=crate::source_diagnostics::statement_offset(&*stmt) {location=Some(offset);}
            // Materializing an indexed item before its implied-DO assigns the
            // index reads an unset value (or repeats a stale value). Expose the
            // control flow before allocation and CHARACTER expression lowering.
            if let Stmt::CallSub{name,args}=stmt {
                if name=="@write" && matches!(&args[1],Expr::Call(n,_) if n=="@list_directed")
                    && args[2..].iter().any(contains_implied_do)
                    && args[2..].iter().all(|e|ready(e,&symbols))
                    && !args[2..].iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@write_option:")))
                {
                    if crate::parameters::integer_constant(&args[0]).ok()==Some(6) {
                        *stmt=Stmt::Print{items:args[2..].to_vec()};
                    } else if crate::file_io::unit(&args[0],&symbols).is_ok() {
                        // File lists already use this G0 format. A variable
                        // unit can select stdout, whose character spacing is
                        // handled by the fragment printer instead.
                        let mut file_args=args.clone();
                        file_args[1]=Expr::Str("(*(1x,g0))".into());
                        if crate::parameters::integer_constant(&args[0]).is_ok() {
                            *args=file_args;
                        } else {
                            *serial+=1;
                            let unit=format!("@list_output_unit_{serial}");
                            let source=args[0].clone();
                            file_args[0]=id(&unit);
                            let mut local=symbols.clone();
                            local.insert(unit.clone(),(TypeSpec::Integer(Some(8)),vec![]));
                            let mut file_body=vec![Stmt::CallSub{name:"@write".into(),args:file_args}];
                            scope(&mut file_body,&local,serial)?;
                            let stdout=lower(&args[2..],&symbols,serial)
                                .map_err(|e|crate::source_diagnostics::at(e,location))?;
                            *stmt=Stmt::Block{body:vec![
                                Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![unit.clone()]},
                                assign(&unit,source),
                                Stmt::If{cond:bin(BinOp::Eq,id(&unit),int(6)),then_body:vec![stdout],else_body:Some(file_body)},
                            ]};
                            continue;
                        }
                    }
                }
            }
            if let Stmt::Print{items}=stmt {
                if items.iter().any(contains_implied_do) && items.iter().all(|e|ready(e,&symbols)) {
                    *stmt=lower(items,&symbols,serial)
                        .map_err(|e|crate::source_diagnostics::at(e,location))?;
                    continue;
                }
            }
            let mut internal = None;
            let mut unit_check = None;
            let mut control_prefix = vec![];
            let mut internal_store = None;
            // Format into a scalar record, then copy it back through the usual
            // checked array assignment. Capture subscripts before output items
            // are evaluated, and never read the destination's old contents.
            if let Stmt::CallSub {name,args}=stmt {
                if name=="@write" {
                    if let Expr::Index(array,indices)|Expr::Call(array,indices)=&args[0] {
                        if let Some((TypeSpec::Character(length),dims))=symbols.get(array) {
                            if !dims.is_empty() && indices.len()==dims.len() && indices.iter().all(|e|integer(e,&symbols)) {
                                *serial+=1;
                                let target=format!("@internal_record_{serial}");
                                let kind=TypeSpec::Character(*length);
                                let array=array.clone();
                                let mut captured=vec![];
                                for (axis,index) in indices.iter().enumerate() {
                                    let n=format!("@internal_index_{serial}_{axis}");
                                    control_prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![n.clone()]});
                                    control_prefix.push(assign(&n,index.clone()));
                                    captured.push(id(&n));
                                }
                                control_prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![target.clone()]});
                                internal_store=Some(Stmt::AssignIndex{name:array,indices:captured,value:id(&target)});
                                symbols.insert(target.clone(),(kind,vec![]));
                                args[0]=id(&target);
                            }
                        }
                    }
                }
            }
            let selected = if let Stmt::CallSub { name, args } = stmt {
                if name=="@write" && matches!(&args[0],Expr::Ident(n) if matches!(symbols.get(n),Some((TypeSpec::Character(_),dims)) if dims.is_empty())) {
                    let Expr::Ident(target)=&args[0] else {unreachable!()};
                    let (TypeSpec::Character(length),_)=&symbols[target] else {unreachable!()};
                    if args[2..].iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@write_option:"))) {
                        bail!("internal WRITE control options are not yet supported");
                    }
                    // List-directed internal output: a blank, then each item as
                    // G0 separated by blanks.
                    let format=if matches!(&args[1],Expr::Call(n,_) if n=="@list_directed") {Expr::Str("(*(1x,g0))".into())} else {args[1].clone()};
                    internal=Some((target.clone(),length.unwrap_or(1)));
                    Some((format,args[2..].to_vec(),int(0)))
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
                        || crate::file_io::unit(&args[0],&symbols).is_ok())
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
                emit(
                    &items,
                    &symbols,
                    serial,
                    "",
                    &mut Vec::new(),
                    &mut inner,
                    Some(&handle),
                ).map_err(|e|crate::source_diagnostics::at(e,location))?;
                inner.push(if let Some((target,length))=internal {
                    Stmt::CallSub {name:"@internal_format_end".into(),args:vec![id(&target),id(&handle),int(length as i64)]}
                } else if let Some(unit)=unit_check {
                    // A file unit, or a variable that may name standard output.
                    let file=Stmt::CallSub {name:"@format_end_file".into(),args:vec![id(&handle),unit.clone()]};
                    if crate::parameters::integer_constant(&unit).is_ok() {file} else {
                        Stmt::If {cond:bin(BinOp::Eq,unit,int(6)),
                            then_body:vec![Stmt::CallSub {name:"@format_end".into(),args:vec![id(&handle)]}],else_body:Some(vec![file])}
                    }
                } else {
                    Stmt::CallSub {name:"@format_end".into(),args:vec![id(&handle)]}
                });
                if let Some(store)=internal_store {inner.push(store);}
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
    let mut functions:Symbols=crate::elemental::functions(&program).into_iter()
        .map(|(name,kind)|(format!("@elemental:{name}"),(kind,vec![]))).collect();
    functions.extend(crate::constructor_expressions::function_shapes(&program));
    scope(&mut program.body, &functions, &mut 0)?;
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
        let mut location=None;
        for mut s in std::mem::take(body) {
            if let Some(offset)=crate::source_diagnostics::statement_offset(&s) {location=Some(offset);}
            let items=match &mut s {
                Stmt::Print{items}=>Some(items.as_mut_slice()),
                Stmt::CallSub{name,args} if matches!(name.as_str(),"@formatted_print"|"@formatted_write_noadvance")=>Some(&mut args[1..]),
                Stmt::CallSub{name,args} if name=="@write"=>Some(&mut args[2..]),_=>None,
            };
            if let Some(items)=items {
                for item in items {
                    // Capture CHARACTER constructors before CHARACTER array
                    // storage is lowered. Leaving [scalar_text] for the later
                    // numeric constructor pass rejects its element type.
                    if matches!(item,Expr::Call(tag,_) if tag=="@array_constructor") {
                        if let Ok((kind,values))=crate::character_elementals::Lower::fixed_items(item,&symbols) {
                            if matches!(kind,TypeSpec::Character(_)) {
                                *item=if values.is_empty() {crate::empty_constructors::parsed(kind,values)}
                                    else {Expr::Call("@array_constructor".into(),values)};
                                let start=out.len();
                                crate::array_calls::character_constructor(item,&symbols,&mut out,serial)?;
                                for generated in &out[start..] {
                                    if let Stmt::ArrayDecl{kind,name,dims}=generated {symbols.insert(name.clone(),(kind.clone(),dims.clone()));}
                                }
                            }
                        }
                    }
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
                Stmt::Print{items} if items.iter().any(contains_implied_do)=>s=lower(items,&symbols,serial).map_err(|e|crate::source_diagnostics::at(e,location))?,
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
    let mut functions:Symbols=crate::elemental::functions(&program).into_iter()
        .map(|(name,kind)|(format!("@elemental:{name}"),(kind,vec![]))).collect();
    functions.extend(crate::constructor_expressions::function_shapes(&program));
    scope(&mut program.body,&functions,&mut 0)?;
    prepare_formatted(program)
}

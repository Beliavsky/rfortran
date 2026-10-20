// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve scalar substrings and bounded deferred-length CHARACTER storage.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Characters = HashMap<String, usize>;
fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn slice(name: &str, start: usize, length: usize) -> Expr {
    Expr::Call(
        "@substring".into(),
        vec![Expr::Ident(name.into()), int(start), int(length)],
    )
}
pub(crate) fn bounds(section: &Expr, length: usize) -> Result<(usize, usize)> {
    let Expr::Section {
        lower,
        upper,
        stride,
        ..
    } = section
    else {
        bail!("scalar CHARACTER subscripts require a substring range");
    };
    if stride.is_some() {
        bail!("CHARACTER substrings do not allow a stride");
    }
    let constant = |e: &Expr| {
        crate::parameters::integer_constant(e)
            .context("substring bounds must currently be constant integers")
    };
    let lower = lower.as_deref().map(constant).transpose()?.unwrap_or(1);
    let upper = upper
        .as_deref()
        .map(constant)
        .transpose()?
        .unwrap_or(length as i64);
    if lower < 1 || lower > length as i64 + 1 || upper < 0 || upper > length as i64 {
        bail!("substring bounds {lower}:{upper} are outside CHARACTER length {length}");
    }
    Ok(((lower - 1) as usize, (upper - lower + 1).max(0) as usize))
}
fn expression(e: &mut Expr, chars: &Characters) -> Result<()> {
    match e {
        Expr::Section { name, .. } if chars.contains_key(name) => {
            let name = name.clone();
            let (start, length) = bounds(e, chars[&name])?;
            *e = slice(&name, start, length);
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                expression(e, chars)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, chars)?;
            expression(b, chars)?;
        }
        Expr::Un(_, a) => expression(a, chars)?,
        _ => {}
    }
    Ok(())
}
fn body(statements: &mut Vec<Stmt>, inherited: &Characters, serial: &mut usize) -> Result<()> {
    let mut chars = inherited.clone();
    for s in statements.iter() {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    chars.remove(n);
                    if let TypeSpec::Character(length) = kind {
                        chars.insert(n.clone(), length.unwrap_or(1));
                    }
                }
            }
            Stmt::ArrayDecl { name, .. } => {
                chars.remove(name);
            }
            _ => {}
        }
    }
    let mut output = Vec::new();
    for mut s in std::mem::take(statements) {
        if let Stmt::AssignIndex {
            name,
            indices,
            value,
        } = &mut s
        {
            if let Some(&total) = chars.get(name) {
                let [section] = indices.as_slice() else {
                    bail!("scalar CHARACTER assignment requires one substring range");
                };
                let (start, length) = bounds(section, total)?;
                expression(value, &chars)?;
                *serial += 1;
                let temp = format!("@substring_value_{serial}");
                output.push(Stmt::VarDecl {
                    kind: TypeSpec::Character(Some(length)),
                    names: vec![temp.clone()],
                });
                output.push(Stmt::Assign {
                    name: temp.clone(),
                    value: value.clone(),
                });
                // Empty destinations perform no write. Full destinations need
                // no read of the previous value; partial ones preserve it.
                if length > 0 || total == 0 {
                    let mut value = Expr::Ident(temp);
                    if start > 0 {
                        value = Expr::Bin(
                            BinOp::Concat,
                            Box::new(slice(name, 0, start)),
                            Box::new(value),
                        );
                    }
                    if start + length < total {
                        value = Expr::Bin(
                            BinOp::Concat,
                            Box::new(value),
                            Box::new(slice(name, start + length, total - start - length)),
                        );
                    }
                    // A variable may be defined one substring at a time, so
                    // its unchanged characters are read without checks.
                    let partial = start > 0 || start + length < total;
                    if partial { output.push(unchecked(true)); }
                    output.push(Stmt::Assign {
                        name: name.clone(),
                        value,
                    });
                    if partial { output.push(unchecked(false)); }
                }
                continue;
            }
        }
        match &mut s {
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => expression(value, &chars)?,
            Stmt::AssignIndex { indices, value, .. } => {
                for e in indices {
                    expression(e, &chars)?;
                }
                expression(value, &chars)?;
            }
            Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expression(e, &chars)?;
                }
            }
            Stmt::Read { args } => {
                if args
                    .iter()
                    .any(|e| matches!(e,Expr::Section{name,..} if chars.contains_key(name)))
                {
                    bail!("READ into a substring is not yet supported");
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, &chars)?;
                body(then_body, &chars, serial)?;
                if let Some(b) = else_body {
                    body(b, &chars, serial)?;
                }
            }
            Stmt::Do {
                start,
                end,
                body: b,
                ..
            } => {
                expression(start, &chars)?;
                expression(end, &chars)?;
                body(b, &chars, serial)?;
            }
            Stmt::DoWhile { cond, body: b } => {
                expression(cond, &chars)?;
                body(b, &chars, serial)?;
            }
            Stmt::Block { body: b }
            | Stmt::Module { body: b, .. }
            | Stmt::Function { body: b, .. }
            | Stmt::Subroutine { body: b, .. } => body(b, &chars, serial)?,
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                expression(expr, &chars)?;
                for c in cases {
                    body(&mut c.body, &chars, serial)?;
                }
                if let Some(b) = default {
                    body(b, &chars, serial)?;
                }
            }
            _ => {}
        }
        output.push(s);
    }
    *statements = output;
    Ok(())
}
/// The start or end of statements whose reads are not checked for
/// initialization.
fn unchecked(begin: bool) -> Stmt {
    let name = if begin { crate::initialized::UNCHECKED_BEGIN } else { crate::initialized::UNCHECKED_END };
    Stmt::CallSub { name: name.into(), args: vec![] }
}
pub fn resolve(mut program: Program) -> Result<Program> {
    fn procedures(body:&[Stmt],names:&mut std::collections::HashSet<String>,returns:&mut HashMap<String,TypeSpec>) {
        for statement in body {match statement {
            Stmt::Function {name,return_type,body,..} => {
                names.insert(name.clone());if let Some(kind)=return_type {returns.insert(name.clone(),kind.clone());}
                procedures(body,names,returns);
            },
            Stmt::Subroutine {name,body,..}=>{names.insert(name.clone());procedures(body,names,returns);},
            Stmt::Module {body,..}=>procedures(body,names,returns),_=>{},
        }}
    }
    let mut scope=DynamicScope::default();procedures(&program.body,&mut scope.procedures,&mut scope.function_results);
    dynamic_body(&mut program.body, &scope, true, &mut 0)?;
    body(&mut program.body, &Characters::new(), &mut 0)?;
    Ok(program)
}

/// ALLOCATE(CHARACTER(LEN=n) :: s): the length, then the objects.
pub(crate) const ALLOCATE_CHARACTER: &str = "@allocate_character";
/// Storage of a deferred-length CHARACTER whose largest value is not known at
/// compile time; a longer value stops the program.
const FALLBACK_CAPACITY: usize = 4096;

pub(crate) fn deferred(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::CharacterLength(e) if matches!(e.as_ref(), Expr::Ident(n) if n == "@deferred_character"))
}

#[derive(Clone, Default)]
struct DynamicScope {
    characters: Characters,
    lengths: HashMap<String, String>,
    fixed: std::collections::HashSet<String>,
    symbols: HashMap<String, (TypeSpec, Vec<Expr>)>,
    procedures: std::collections::HashSet<String>,
    function_results: HashMap<String,TypeSpec>,
    procedure: bool,
}
fn allocation_flag(length: &str) -> String { format!("{length}_allocated") }
fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn ident(n: &str) -> Expr { Expr::Ident(n.into()) }
fn runtime_character(e: &Expr) -> bool {
    match e {
        Expr::Call(n, _) if n == "@dynamic_substring" => true,
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(runtime_character),
        Expr::Bin(_, a, b) => runtime_character(a) || runtime_character(b),
        Expr::Un(_, a) => runtime_character(a),
        _ => false,
    }
}
fn capacity(e: &Expr, scope: &DynamicScope) -> Option<usize> {
    match e {
        Expr::Str(s) => Some(s.len()),
        Expr::Ident(n) | Expr::Section { name: n, .. } => scope.characters.get(n).copied(),
        Expr::Bin(BinOp::Concat, a, b) => capacity(a, scope)?.checked_add(capacity(b, scope)?),
        Expr::Call(n,args) if n=="repeat" && args.len()==2 && !scope.procedures.contains(n) => capacity(&args[0],scope)?.checked_mul(usize::try_from(crate::parameters::integer_constant(&args[1]).ok()?.max(0)).ok()?),
        Expr::Call(n, args) if matches!(n.as_str(),"trim"|"adjustl"|"adjustr") && args.len() == 1 => capacity(&args[0], scope),
        Expr::Call(n, args) if matches!(n.as_str(), "char" | "achar") && args.len()==1 => Some(1),
        Expr::Call(n, args) if n == "@literal_substring" => capacity(&args[0], scope),
        Expr::Call(n, args) if n == "@dynamic_substring" => crate::parameters::integer_constant(&args[3]).ok()?.try_into().ok(),
        Expr::Call(n, args) if n == "@substring" => crate::parameters::integer_constant(&args[2]).ok()?.try_into().ok(),
        _ => None,
    }
}

// Infer an upper bound, not a runtime length. Cyclic growth (s=s//'x') cannot
// be represented by this bounded storage model and is rejected explicitly.
fn assignment_capacities(statements: &[Stmt], targets: &[String], scope: &DynamicScope, sizes: &mut Characters) -> Result<()> {
    for statement in statements {
        match statement {
            Stmt::Assign { name, value } if targets.contains(name) => {
                let size = capacity(value, scope).unwrap_or(FALLBACK_CAPACITY);
                *sizes.get_mut(name).unwrap() = sizes[name].max(size);
            }
            Stmt::CallSub { name, args } if name == ALLOCATE_CHARACTER => {
                let size = crate::parameters::integer_constant(&args[0]).ok().and_then(|n| usize::try_from(n.max(0)).ok()).unwrap_or(FALLBACK_CAPACITY);
                for object in &args[1..] {
                    if let Expr::Ident(n) = object {
                        if let Some(current) = sizes.get_mut(n) { *current = (*current).max(size); }
                    }
                }
            }
            Stmt::If { then_body, else_body, .. } => {
                assignment_capacities(then_body, targets, scope, sizes)?;
                if let Some(b) = else_body { assignment_capacities(b, targets, scope, sizes)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => assignment_capacities(body, targets, scope, sizes)?,
            Stmt::Block { body } => {
                let mut local = scope.clone();
                let mut targets = targets.to_vec();
                for s in body {
                    if let Stmt::VarDecl { kind, names } = s {
                        for name in names {
                            targets.retain(|n| n != name);
                            local.characters.remove(name);
                            if let TypeSpec::Character(n) = kind { local.characters.insert(name.clone(), n.unwrap_or(1)); }
                        }
                    }
                }
                assignment_capacities(body, &targets, &local, sizes)?;
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { assignment_capacities(&c.body, targets, scope, sizes)?; }
                if let Some(b) = default { assignment_capacities(b, targets, scope, sizes)?; }
            }
            _ => {}
        }
    }
    Ok(())
}
fn capture(value: Expr, scope: &mut DynamicScope, output: &mut Vec<Stmt>, serial: &mut usize) -> Expr {
    *serial += 1;
    let name = format!("@substring_bound_{serial}");
    let kind = TypeSpec::Integer(Some(8));
    scope.symbols.insert(name.clone(), (kind.clone(), vec![]));
    output.push(Stmt::VarDecl { kind, names: vec![name.clone()] });
    output.push(Stmt::Assign { name: name.clone(), value });
    ident(&name)
}
fn logical_length(e: &Expr, scope: &mut DynamicScope, output: &mut Vec<Stmt>, serial: &mut usize) -> Result<Expr> {
    Ok(match e {
        Expr::Str(s) => int(s.len()),
        Expr::Ident(n) => if let Some(n) = scope.lengths.get(n) { ident(n) }
            else { int(*scope.characters.get(n).context("expected scalar CHARACTER expression")?) },
        Expr::Call(n, a) if n == "@dynamic_substring" || n == "@substring" => a[2].clone(),
        Expr::Call(n, a) if matches!(n.as_str(), "char" | "achar") && a.len()==1 => int(1),
        Expr::Call(n,a) if matches!(n.as_str(),"adjustl"|"adjustr") && a.len()==1 => logical_length(&a[0],scope,output,serial)?,
        Expr::Bin(BinOp::Concat, a, b) => binary(BinOp::Add, logical_length(a, scope, output, serial)?, logical_length(b, scope, output, serial)?),
        Expr::Call(n, a) if n == "repeat" && a.len() == 2 => {
            let copies = capture(Expr::Call("max".into(), vec![int(0), a[1].clone()]), scope, output, serial);
            binary(BinOp::Mul, logical_length(&a[0], scope, output, serial)?, copies)
        }
        Expr::Call(n, a) if n == "trim" && a.len() == 1 => {
            *serial += 1;
            let name = format!("@substring_length_{serial}");
            let kind = TypeSpec::Integer(Some(8));
            scope.symbols.insert(name.clone(), (kind.clone(), vec![]));
            output.push(Stmt::VarDecl { kind, names: vec![name.clone()] });
            output.push(Stmt::CallSub { name: "@character_length".into(), args: vec![ident(&name), e.clone()] });
            ident(&name)
        }
        _ => bail!("unsupported deferred CHARACTER assignment expression"),
    })
}
fn dynamic_slice(source: Expr, range: &Expr, scope: &mut DynamicScope, output: &mut Vec<Stmt>, serial: &mut usize) -> Result<Expr> {
    let Expr::Section { lower, upper, stride, .. } = range else { bail!("scalar CHARACTER subscripts require a substring range"); };
    if stride.is_some() { bail!("CHARACTER substrings do not allow a stride"); }
    if matches!(&source, Expr::Str(s) if !s.is_ascii() || s.contains('\0')) { bail!("CHARACTER substrings require non-NUL ASCII text"); }
    let cap = capacity(&source, scope).context("substring requires scalar CHARACTER storage")?;
    let length = logical_length(&source, scope, output, serial)?;
    let mut low = lower.as_deref().cloned().unwrap_or(int(1));
    let mut high = upper.as_deref().cloned().unwrap_or(length.clone());
    dynamic_expression(&mut low, scope, output, serial)?;
    dynamic_expression(&mut high, scope, output, serial)?;
    let mut integer_symbols = scope.symbols.clone();
    for (kind, _) in integer_symbols.values_mut() {
        if matches!(kind, TypeSpec::Integer(Some(1 | 2))) { *kind = TypeSpec::Integer(Some(8)); }
    }
    if !crate::output::integer(&low, &integer_symbols) || !crate::output::integer(&high, &integer_symbols) {
        bail!("substring bounds require scalar INTEGER expressions");
    }
    if crate::parameters::integer_constant(&length).is_ok()
        && crate::parameters::integer_constant(&low).is_ok()
        && crate::parameters::integer_constant(&high).is_ok()
    {
        let range = Expr::Section { name: String::new(), lower: Some(Box::new(low)), upper: Some(Box::new(high)), stride: None };
        let (start, size) = bounds(&range, cap)?;
        return Ok(Expr::Call("@substring".into(), vec![source, int(start), int(size)]));
    }
    let low = capture(low, scope, output, serial);
    let high = capture(high, scope, output, serial);
    let invalid = binary(BinOp::Or,
        binary(BinOp::Or, binary(BinOp::Lt, low.clone(), int(1)), binary(BinOp::Gt, low.clone(), binary(BinOp::Add, length.clone(), int(1)))),
        binary(BinOp::Or, binary(BinOp::Lt, high.clone(), int(0)), binary(BinOp::Gt, high.clone(), length)));
    output.push(Stmt::If { cond: invalid, then_body: vec![Stmt::CallSub {
        name: "@error_stop".into(), args: vec![Expr::Str("substring bounds are outside CHARACTER length".into())],
    }], else_body: None });
    let start = capture(binary(BinOp::Sub, low.clone(), int(1)), scope, output, serial);
    let size = capture(Expr::Call("max".into(), vec![int(0), binary(BinOp::Add, binary(BinOp::Sub, high, low), int(1))]), scope, output, serial);
    Ok(Expr::Call("@dynamic_substring".into(), vec![source, start, size, int(cap)]))
}
fn dynamic_expression(e: &mut Expr, scope: &mut DynamicScope, output: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    if scope.procedure && matches!(e, Expr::Ident(n) | Expr::Section {name:n,..} if scope.lengths.contains_key(n) && !scope.fixed.contains(n)) {
        bail!("deferred CHARACTER host association in procedures is not yet supported");
    }
    match e {
        Expr::Call(n, args) if n == "allocated" && args.len() == 1 && matches!(&args[0], Expr::Ident(n) if scope.lengths.contains_key(n)) => {
            let Expr::Ident(n) = &args[0] else { unreachable!() };
            if scope.procedure { bail!("deferred CHARACTER host association in procedures is not yet supported"); }
            *e = ident(&allocation_flag(&scope.lengths[n]));
        }
        Expr::Call(n, args) if matches!(n.as_str(), "len" | "len_trim") && args.len() == 1 && capacity(&args[0], scope).is_some() => {
            let trim = n == "len_trim";
            let mut source = args[0].clone();
            dynamic_expression(&mut source, scope, output, serial)?;
            if trim { source = Expr::Call("trim".into(), vec![source]); }
            *e = logical_length(&source, scope, output, serial)?;
        }
        Expr::Ident(n) if scope.lengths.contains_key(n) => {
            *e = Expr::Call("@dynamic_substring".into(), vec![ident(n), int(0), ident(&scope.lengths[n]), int(scope.characters[n])]);
        }
        Expr::Section { name, .. } if scope.characters.contains_key(name) => {
            let source = if let Some(length) = scope.lengths.get(name) {
                Expr::Call("@dynamic_substring".into(), vec![ident(name), int(0), ident(length), int(scope.characters[name])])
            } else { ident(name) };
            *e = dynamic_slice(source, &e.clone(), scope, output, serial)?;
        }
        Expr::Call(n, args) if n == "@literal_substring" => {
            let [source, range] = args.as_slice() else { unreachable!() };
            *e = dynamic_slice(source.clone(), range, scope, output, serial)?;
        }
        Expr::Call(n, _) if n == "@dynamic_substring" => {},
        Expr::Call(n,args) if n=="new_line" && !scope.procedures.contains(n) && !scope.symbols.contains_key(n) => {
            if args.len()!=1 {bail!("NEW_LINE requires one CHARACTER argument");}
            if capacity(&args[0],scope).is_none() {bail!("NEW_LINE requires a scalar CHARACTER argument");}
            *e=Expr::Str("\n".into());
        }
        Expr::Call(n,args) if matches!(n.as_str(),"index"|"scan"|"verify"|"@split_scan") && !scope.procedures.contains(n) && !scope.symbols.contains_key(n) => {
            let operation=n.clone();
            let split=operation=="@split_scan";
            if !(2..=4).contains(&args.len()) {bail!("{operation} requires two CHARACTER arguments and optional BACK and KIND");}
            let mut back=args.get(2).filter(|e|!crate::named_arguments::omitted(e)).cloned().unwrap_or(Expr::Logical(false));
            dynamic_expression(&mut back,scope,output,serial)?;
            fn logical(e:&Expr,scope:&DynamicScope)->bool {match e {
                Expr::Logical(_)=>true,
                Expr::Ident(n)=>matches!(scope.symbols.get(n),Some((TypeSpec::Logical,dims)) if dims.is_empty()),
                Expr::Call(n,_)=>scope.function_results.get(n)==Some(&TypeSpec::Logical)
                    || (!scope.procedures.contains(n) && matches!(n.as_str(),"all"|"any"|"@all_reduction"|"@any_reduction")),
                Expr::Un(crate::ast::UnOp::Not,e)=>logical(e,scope),
                Expr::Bin(BinOp::And|BinOp::Or|BinOp::Eqv|BinOp::Neqv,a,b)=>logical(a,scope)&&logical(b,scope),
                Expr::Bin(BinOp::Eq|BinOp::Ne|BinOp::Lt|BinOp::Le|BinOp::Gt|BinOp::Ge,_,_)=>true,
                _=>false,
            }}
            if !split && !logical(&back,scope) {bail!("{operation} BACK must be a scalar LOGICAL expression");}
            let kind=args.get(3).filter(|_|!split).filter(|e|!crate::named_arguments::omitted(e)).map(crate::parameters::integer_constant).transpose()
                .context("CHARACTER search KIND must be a constant INTEGER")?.unwrap_or(4);
            if !matches!(kind,1|2|4|8) {bail!("{operation} KIND must be a supported INTEGER kind (1, 2, 4 or 8)");}
            *serial+=1;let back_name=format!("@substring_search_back_{serial}");
            scope.symbols.insert(back_name.clone(),(TypeSpec::Logical,vec![]));
            output.push(Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![back_name.clone()]});
            output.push(Stmt::Assign{name:back_name.clone(),value:back});
            let back=ident(&back_name);
            let mut sources=args[..2].to_vec();
            let mut buffers=vec![];let mut lengths=vec![];let mut caps=vec![];
            for source in &mut sources {
                dynamic_expression(source,scope,output,serial)?;
                let cap=capacity(source,scope).with_context(||format!("{operation} requires scalar CHARACTER arguments"))?;
                let size=logical_length(source,scope,output,serial)?;
                lengths.push(capture(size,scope,output,serial));caps.push(cap);
                *serial+=1;let buffer=format!("@substring_search_text_{serial}");
                scope.characters.insert(buffer.clone(),cap);
                scope.symbols.insert(buffer.clone(),(TypeSpec::Character(Some(cap)),vec![]));
                output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(cap)),names:vec![buffer.clone()]});
                output.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![ident(&buffer),source.clone(),int(cap)]});
                buffers.push(buffer);
            }
            *serial+=1;let stem=format!("@substring_search_{serial}");
            let result=format!("{stem}_result");let i=format!("{stem}_i");let j=format!("{stem}_j");
            let offset=format!("{stem}_offset");let a=format!("{stem}_a");let b=format!("{stem}_b");let matched=format!("{stem}_matched");
            for (kind,names) in [(TypeSpec::Integer(Some(kind as u8)),vec![result.clone()]),
                (TypeSpec::Integer(Some(8)),vec![i.clone(),j.clone(),offset.clone(),a.clone(),b.clone()]),
                (TypeSpec::Logical,vec![matched.clone()])] {
                for name in &names {scope.symbols.insert(name.clone(),(kind.clone(),vec![]));}
                output.push(Stmt::VarDecl {kind,names});
            }
            let assign=|name:&str,value:Expr|Stmt::Assign {name:name.into(),value};
            let position=if split {
                let mut pos=args[3].clone();
                dynamic_expression(&mut pos,scope,output,serial)?;
                Some(capture(pos,scope,output,serial))
            } else {None};
            output.push(assign(&result,int(0)));
            if let Some(pos)=&position {
                let invalid=Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("SPLIT POS is outside the permitted range".into())]};
                output.push(Stmt::If{cond:back.clone(),then_body:vec![
                    Stmt::If{cond:binary(BinOp::Or,binary(BinOp::Lt,pos.clone(),int(1)),binary(BinOp::Gt,pos.clone(),binary(BinOp::Add,lengths[0].clone(),int(1)))),then_body:vec![invalid.clone()],else_body:None}
                ],else_body:Some(vec![
                    Stmt::If{cond:binary(BinOp::Or,binary(BinOp::Lt,pos.clone(),int(0)),binary(BinOp::Gt,pos.clone(),lengths[0].clone())),then_body:vec![invalid],else_body:None},
                    assign(&result,binary(BinOp::Add,lengths[0].clone(),int(1)))
                ])});
            }
            let found=vec![assign(&result,ident(&i)),Stmt::If {cond:Expr::Un(crate::ast::UnOp::Not,Box::new(back.clone())),then_body:vec![Stmt::Exit],else_body:None}];
            if operation=="index" {
                let comparisons=vec![
                    assign(&offset,binary(BinOp::Add,binary(BinOp::Sub,ident(&i),int(1)),ident(&j))),
                    Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![ident(&a),ident(&buffers[0]),ident(&offset),int(caps[0])]},
                    Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![ident(&b),ident(&buffers[1]),ident(&j),int(caps[1])]},
                    Stmt::If {cond:binary(BinOp::Ne,ident(&a),ident(&b)),then_body:vec![assign(&matched,Expr::Logical(false)),Stmt::Exit],else_body:None},
                ];
                let search=Stmt::Do {var:i.clone(),start:int(1),end:binary(BinOp::Add,binary(BinOp::Sub,lengths[0].clone(),lengths[1].clone()),int(1)),body:vec![
                    assign(&matched,Expr::Logical(true)),
                    Stmt::Do {var:j,start:int(0),end:binary(BinOp::Sub,lengths[1].clone(),int(1)),body:comparisons},
                    Stmt::If {cond:ident(&matched),then_body:found,else_body:None},
                ]};
                output.push(Stmt::If {cond:binary(BinOp::Eq,lengths[1].clone(),int(0)),then_body:vec![Stmt::If {
                    cond:back,then_body:vec![assign(&result,binary(BinOp::Add,lengths[0].clone(),int(1)))],else_body:Some(vec![assign(&result,int(1))])
                }],else_body:Some(vec![search])});
            } else {
                let condition=if operation=="scan" || split {ident(&matched)} else {Expr::Un(crate::ast::UnOp::Not,Box::new(ident(&matched)))};
                let start=capture(int(1),scope,output,serial);
                let end=capture(lengths[0].clone(),scope,output,serial);
                if let Some(pos)=&position {
                    let Expr::Ident(start_name)=&start else {unreachable!()};
                    let Expr::Ident(end_name)=&end else {unreachable!()};
                    output.push(Stmt::If{cond:back.clone(),then_body:vec![assign(end_name,binary(BinOp::Sub,pos.clone(),int(1)))],
                        else_body:Some(vec![assign(start_name,binary(BinOp::Add,pos.clone(),int(1)))])});
                }
                output.push(Stmt::Do {var:i.clone(),start,end,body:vec![
                    assign(&offset,binary(BinOp::Sub,ident(&i),int(1))),
                    Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![ident(&a),ident(&buffers[0]),ident(&offset),int(caps[0])]},
                    assign(&matched,Expr::Logical(false)),
                    Stmt::Do {var:j.clone(),start:int(0),end:binary(BinOp::Sub,lengths[1].clone(),int(1)),body:vec![
                        Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![ident(&b),ident(&buffers[1]),ident(&j),int(caps[1])]},
                        Stmt::If {cond:binary(BinOp::Eq,ident(&a),ident(&b)),then_body:vec![assign(&matched,Expr::Logical(true)),Stmt::Exit],else_body:None},
                    ]},
                    Stmt::If {cond:condition,then_body:found,else_body:None},
                ]});
            }
            *e=ident(&result);
        }
        Expr::Call(n, args) if matches!(n.as_str(), "ichar" | "iachar" | "char" | "achar") && !scope.procedures.contains(n) && !scope.symbols.contains_key(n) => {
            if args.len()!=1 {bail!("{n} currently requires one argument; KIND is not yet supported");}
            let mut argument=args[0].clone();
            dynamic_expression(&mut argument, scope, output, serial)?;
            if matches!(n.as_str(), "ichar" | "iachar") {
                let size=logical_length(&argument,scope,output,serial)
                    .with_context(||format!("{n} requires a scalar CHARACTER argument"))?;
                if let Ok(size)=crate::parameters::integer_constant(&size) {
                    if size!=1 {bail!("{n} argument must have length one");}
                } else {
                    output.push(Stmt::If {cond:binary(BinOp::Ne,size,int(1)),then_body:vec![Stmt::CallSub {
                        name:"@error_stop".into(),args:vec![Expr::Str(format!("{n} argument must have length one"))],
                    }],else_body:None});
                }
                *serial+=1;
                let buffer=format!("@character_code_buffer_{serial}");
                scope.characters.insert(buffer.clone(),1);
                scope.symbols.insert(buffer.clone(),(TypeSpec::Character(Some(1)),vec![]));
                output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(1)),names:vec![buffer.clone()]});
                output.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![ident(&buffer),argument,int(1)]});
                let offset=capture(int(0),scope,output,serial);
                let result=capture(int(0),scope,output,serial);
                output.push(Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![result.clone(),ident(&buffer),offset,int(1)]});
                *e=Expr::Call("int".into(),vec![result]);
            } else {
                let mut symbols=scope.symbols.clone();
                for (kind,_) in symbols.values_mut() {if matches!(kind,TypeSpec::Integer(Some(1 | 2))) {*kind=TypeSpec::Integer(Some(8));}}
                if !crate::output::integer(&argument,&symbols) {bail!("{n} requires a scalar INTEGER argument");}
                let value=capture(argument,scope,output,serial);
                output.push(Stmt::If {cond:binary(BinOp::Or,binary(BinOp::Lt,value.clone(),int(1)),binary(BinOp::Gt,value.clone(),int(127))),
                    then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("{n} currently supports character codes 1 through 127"))]}],else_body:None});
                *serial+=1;
                let buffer=format!("@character_code_buffer_{serial}");
                scope.characters.insert(buffer.clone(),1);
                scope.symbols.insert(buffer.clone(),(TypeSpec::Character(Some(1)),vec![]));
                output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(1)),names:vec![buffer.clone()]});
                output.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![ident(&buffer),Expr::Call(n.clone(),vec![value]),int(1)]});
                *e=ident(&buffer);
            }
        }
        Expr::Call(n, args) => {
            for e in args.iter_mut() { dynamic_expression(e, scope, output, serial)?; }
            if !matches!(n.as_str(), "trim" | "@substring") && args.iter().any(runtime_character) {
                bail!("`{n}` does not yet support runtime-length CHARACTER arguments");
            }
        }
        Expr::Index(_, args) => for e in args { dynamic_expression(e, scope, output, serial)?; },
        Expr::Bin(_, a, b) => { dynamic_expression(a, scope, output, serial)?; dynamic_expression(b, scope, output, serial)?; }
        Expr::Un(_, a) => dynamic_expression(a, scope, output, serial)?,
        _ => {}
    }
    Ok(())
}
fn dynamic_body(statements: &mut Vec<Stmt>, inherited: &DynamicScope, local: bool, serial: &mut usize) -> Result<()> {
    let statement_location = std::cell::Cell::new(None);
    let result = (|| -> Result<()> {
    let mut scope = inherited.clone();
    scope.procedure = !local;
    let mut targets = vec![];
    for s in statements.iter() {
        match s {
            Stmt::VarDecl { kind, names } => for name in names {
                scope.characters.remove(name);
                scope.lengths.remove(name);
                scope.fixed.remove(name);
                scope.symbols.insert(name.clone(), (kind.clone(), vec![]));
                if deferred(kind) {
                    if !local { bail!("deferred CHARACTER storage currently requires a main-program or BLOCK local variable"); }
                    if !statements.iter().any(|s| matches!(s, Stmt::DummyIntent {name:n,intent} if n == name && intent == "allocatable")) {
                        bail!("deferred CHARACTER `{name}` requires ALLOCATABLE");
                    }
                    *serial += 1;
                    let length = format!("@substring_allocated_length_{serial}");
                    scope.symbols.insert(length.clone(), (TypeSpec::Integer(Some(8)), vec![]));
                    scope.lengths.insert(name.clone(), length);
                    scope.characters.insert(name.clone(), 0);
                    targets.push(name.clone());
                } else if let TypeSpec::Character(n) = kind { scope.characters.insert(name.clone(), n.unwrap_or(1)); }
            },
            Stmt::ArrayDecl { kind, name, dims } => {
                if deferred(kind) { bail!("deferred CHARACTER arrays are not yet supported"); }
                scope.characters.remove(name);
                scope.lengths.remove(name);
                scope.symbols.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            _ => {}
        }
    }
    for s in statements.iter() {if let Stmt::DummyIntent{name,intent}=s {
        if let Some(length)=intent.strip_prefix("@character_spec_length:").filter(|_|scope.characters.contains_key(name)) {
            scope.lengths.insert(name.clone(),length.to_owned());scope.fixed.insert(name.clone());
        }
    }}
    for iteration in 0..=targets.len() + 1 {
        let mut sizes = targets.iter().map(|n| (n.clone(), scope.characters[n])).collect::<Characters>();
        assignment_capacities(statements, &targets, &scope, &mut sizes)?;
        if targets.iter().all(|n| sizes[n] == scope.characters[n]) { break; }
        // Growth such as s = s // 'x' uses the fallback storage.
        if iteration == targets.len() + 1 {
            for name in &targets {
                if sizes[name] != scope.characters[name] { scope.characters.insert(name.clone(), scope.characters[name].max(FALLBACK_CAPACITY)); }
            }
            break;
        }
        for name in &targets {
            if sizes[name] >= i32::MAX as usize { bail!("deferred CHARACTER storage is too large"); }
            scope.characters.insert(name.clone(), sizes[name]);
        }
    }
    let mut result = vec![];
    for mut statement in std::mem::take(statements) {
        if let Some(p) = crate::source_diagnostics::statement_offset(&statement) { statement_location.set(Some(p)); }
        let mut prefix = vec![];
        match &mut statement {
            Stmt::CallSub {name,args} if name==ALLOCATE_CHARACTER => {
                let mut length=args[0].clone();
                dynamic_expression(&mut length,&mut scope,&mut result,serial)?;
                let saved=capture(Expr::Call("max".into(),vec![int(0),length]),&mut scope,&mut result,serial);
                for object in &args[1..] {
                    let Expr::Ident(n)=object else {bail!("ALLOCATE(CHARACTER(LEN=...) :: ...) requires deferred-length CHARACTER scalars");};
                    let length=scope.lengths.get(n).filter(|_|!scope.fixed.contains(n))
                        .with_context(||format!("ALLOCATE(CHARACTER(LEN=...) :: {n}) requires a deferred-length ALLOCATABLE CHARACTER scalar"))?.clone();
                    if scope.procedure {bail!("deferred CHARACTER host association in procedures is not yet supported");}
                    let flag=allocation_flag(&length);
                    let cap=scope.characters[n.as_str()];
                    let stop=|text:String|vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(text)]}];
                    result.push(Stmt::If {cond:ident(&flag),then_body:stop(format!("ALLOCATE of already allocated CHARACTER `{n}`")),else_body:None});
                    result.push(Stmt::If {cond:binary(BinOp::Gt,saved.clone(),int(cap)),then_body:stop(format!("deferred-length CHARACTER `{n}` would exceed its {cap}-character storage")),else_body:None});
                    // The value is undefined until assigned; blanks fill the storage.
                    result.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![ident(n),Expr::Str(String::new()),int(cap)]});
                    result.push(Stmt::Assign {name:flag,value:Expr::Logical(true)});
                    result.push(Stmt::Assign {name:length,value:saved.clone()});
                }
                continue;
            }
            Stmt::CallSub {name,args} if name=="@deallocate" => {
                for n in crate::allocatables::deallocation_targets(args)? {
                    let length=scope.lengths.get(&n).with_context(||format!("DEALLOCATE requires a supported ALLOCATABLE variable `{n}`"))?.clone();
                    if scope.procedure {bail!("deferred CHARACTER host association in procedures is not yet supported");}
                    let flag=allocation_flag(&length);
                    result.push(Stmt::If {cond:Expr::Un(crate::ast::UnOp::Not,Box::new(ident(&flag))),
                        then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("DEALLOCATE of unallocated CHARACTER `{n}`"))]}],else_body:None});
                    result.push(Stmt::Assign {name:flag,value:Expr::Logical(false)});
                    result.push(Stmt::Assign {name:length,value:int(0)});
                    result.push(Stmt::CallSub {name:"@undefine".into(),args:vec![ident(&n)]});
                }
                continue;
            }
            Stmt::VarDecl { kind, names } if deferred(kind) => {
                for name in names {
                    result.push(Stmt::VarDecl { kind: TypeSpec::Character(Some(scope.characters[name])), names: vec![name.clone()] });
                    result.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![scope.lengths[name].clone()] });
                    let flag = allocation_flag(&scope.lengths[name]);
                    result.push(Stmt::VarDecl { kind: TypeSpec::Logical, names: vec![flag.clone()] });
                    result.push(Stmt::Assign { name: flag, value: Expr::Logical(false) });
                    scope.symbols.insert(name.clone(), (TypeSpec::Character(Some(scope.characters[name])), vec![]));
                }
                continue;
            }
            Stmt::Assign { name, value } => {
                dynamic_expression(value, &mut scope, &mut prefix, serial)?;
                if let Some(length) = scope.lengths.get(name).filter(|_|!scope.fixed.contains(name)).cloned() {
                    let length_value = logical_length(value, &mut scope, &mut prefix, serial)?;
                    let saved = capture(length_value, &mut scope, &mut prefix, serial);
                    let cap = scope.characters[name.as_str()];
                    prefix.push(Stmt::If { cond: binary(BinOp::Gt, saved.clone(), int(cap)), then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(), args: vec![Expr::Str(format!("deferred-length CHARACTER `{name}` would exceed its {cap}-character storage"))] }], else_body: None });
                    result.extend(prefix);
                    result.push(statement);
                    result.push(Stmt::Assign { name: allocation_flag(&length), value: Expr::Logical(true) });
                    result.push(Stmt::Assign { name: length, value: saved });
                    continue;
                }
            }
            Stmt::AssignIndex { name, indices, value } => {
                dynamic_expression(value, &mut scope, &mut prefix, serial)?;
                if let Some(&cap)=scope.characters.get(name) {
                    let [range]=indices.as_slice() else {bail!("scalar CHARACTER assignment requires one substring range");};
                    let variable_bounds=scope.lengths.contains_key(name) || matches!(range,Expr::Section {lower,upper,..}
                        if lower.as_deref().is_some_and(|e|crate::parameters::integer_constant(e).is_err())
                        || upper.as_deref().is_some_and(|e|crate::parameters::integer_constant(e).is_err()));
                    if variable_bounds {
                        let source=if let Some(length)=scope.lengths.get(name) {
                            Expr::Call("@dynamic_substring".into(),vec![ident(name),int(0),ident(length),int(cap)])
                        } else {ident(name)};
                        let dest=dynamic_slice(source.clone(),range,&mut scope,&mut prefix,serial)?;
                        let Expr::Call(_,parts)=dest else {unreachable!()};
                        let start=parts[1].clone();let size=parts[2].clone();
                        // Snapshot and pad the RHS before modifying any destination bytes.
                        // A bounded buffer then supplies exactly the selected runtime width.
                        *serial+=1;let temp=format!("@dynamic_substring_value_{serial}");
                        prefix.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(cap)),names:vec![temp.clone()]});
                        prefix.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![ident(&temp),value.clone(),int(cap)]});
                        let end=capture(binary(BinOp::Add,start.clone(),size.clone()),&mut scope,&mut prefix,serial);
                        let length=logical_length(&source,&mut scope,&mut prefix,serial)?;
                        let tail=capture(binary(BinOp::Sub,length,end.clone()),&mut scope,&mut prefix,serial);
                        let slice=|source:Expr,start:Expr,size:Expr|Expr::Call("@dynamic_substring".into(),vec![source,start,size,int(cap)]);
                        let replacement=binary(BinOp::Concat,
                            binary(BinOp::Concat,slice(source.clone(),int(0),start),slice(ident(&temp),int(0),size.clone())),
                            slice(source,end,tail));
                        prefix.push(Stmt::If {cond:binary(BinOp::Gt,size,int(0)),then_body:vec![
                            unchecked(true),Stmt::Assign {name:name.clone(),value:replacement},unchecked(false)],else_body:None});
                        result.extend(prefix);
                        continue;
                    }
                }
                for e in indices {
                    if !matches!(e, Expr::Section { .. }) { dynamic_expression(e, &mut scope, &mut prefix, serial)?; }
                }
            }
            Stmt::CallSub{name,args} if name=="@internal_format_end" => {
                if let Expr::Ident(target)=&args[0] {
                    if let Some(length)=scope.lengths.get(target).filter(|_|scope.fixed.contains(target)) {args[2]=ident(length);}
                }
                for e in &mut args[1..] {dynamic_expression(e,&mut scope,&mut prefix,serial)?;}
            }
            Stmt::Print { items } | Stmt::CallSub { args: items, .. } => for e in items { dynamic_expression(e, &mut scope, &mut prefix, serial)?; },
            Stmt::Return(Some(e)) => dynamic_expression(e, &mut scope, &mut prefix, serial)?,
            Stmt::Read { args } => {
                if args.iter().any(|e| matches!(e, Expr::Ident(n) if scope.lengths.contains_key(n))) {
                    bail!("READ into deferred CHARACTER is not yet supported");
                }
            }
            Stmt::If { cond, then_body, else_body } => {
                dynamic_expression(cond, &mut scope, &mut prefix, serial)?;
                dynamic_body(then_body, &scope, local, serial)?;
                if let Some(b) = else_body { dynamic_body(b, &scope, local, serial)?; }
            }
            Stmt::Do { start, end, body, .. } => {
                dynamic_expression(start, &mut scope, &mut prefix, serial)?;
                dynamic_expression(end, &mut scope, &mut prefix, serial)?;
                dynamic_body(body, &scope, local, serial)?;
            }
            Stmt::DoWhile { cond, body } => {
                dynamic_expression(cond, &mut scope, &mut prefix, serial)?;
                dynamic_body(body, &scope, local, serial)?;
                if !prefix.is_empty() {
                    prefix.push(Stmt::If { cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())), then_body: vec![Stmt::Exit], else_body: None });
                    prefix.append(body);
                    *body = std::mem::take(&mut prefix);
                    *cond = Expr::Logical(true);
                }
            }
            Stmt::Block { body } => {
                // A BLOCK's own deferred-length variables are local to it,
                // even in a procedure, where no host shares them.
                let declares = body.iter().any(|s| matches!(s, Stmt::VarDecl { kind, .. } if deferred(kind)));
                dynamic_body(body, &scope, local || declares, serial)?
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                dynamic_body(body, &scope, false, serial)?;
            }
            Stmt::SelectCase { expr, cases, default } => {
                dynamic_expression(expr, &mut scope, &mut prefix, serial)?;
                for c in cases { dynamic_body(&mut c.body, &scope, local, serial)?; }
                if let Some(b) = default { dynamic_body(b, &scope, local, serial)?; }
            }
            _ => {}
        }
        result.extend(prefix);
        result.push(statement);
    }
    *statements = result;
    Ok(())
    })();
    result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
}

/// Generated declarations belong to the enclosing procedure, even when their
/// captures execute conditionally or inside a loop.
pub fn hoist(body: &mut Vec<Stmt>) { hoist_kind(body,false); }
pub(crate) fn hoist_vectors(body:&mut Vec<Stmt>) { hoist_kind(body,true); }
fn hoist_kind(body:&mut Vec<Stmt>,vectors:bool) {
    fn walk(body: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>, vectors:bool) {
        let local_arrays=body.iter().filter_map(|s|match s {Stmt::ArrayDecl{name,..}=>Some(name.clone()),_=>None}).collect::<std::collections::HashSet<_>>();
        let mut output = Vec::new();
        for mut s in std::mem::take(body) {
            let vector_declaration=matches!(&s,Stmt::VarDecl{names,..} if names.iter().all(|n|n.starts_with("@vector_") || n.starts_with("@matrix_alloc_@vector_") || n.starts_with("@record_value_") || n.starts_with("@record_index_") || n.starts_with("@record_vector_index_") || n.starts_with("@record_output_")))
                || matches!(&s,Stmt::ArrayDecl{name,..} if name.starts_with("@vector_") || name.starts_with("@record_array_value_") || name.starts_with("@record_vector_index_") || name.starts_with("@record$@record_output_selection_"))
                || matches!(&s,Stmt::DummyIntent{name,..} if local_arrays.contains(name) && (name.starts_with("@vector_") || name.starts_with("@record_array_value_") || name.starts_with("@record_vector_index_") || name.starts_with("@record$@record_output_selection_")));
            if vector_declaration || !vectors && (matches!(&s, Stmt::VarDecl {names,..} if names.iter().all(|n|n.starts_with("@pointer_") || n.starts_with("@substring_") || n.starts_with("@character_") || n.starts_with("@vector_") || n.starts_with("@reduction_")))
                || matches!(&s, Stmt::ArrayDecl {name,..} if name.starts_with("@pointer_") || name.starts_with("@vector_") || name.starts_with("@reduction_")))
            {
                declarations.push(s);
                continue;
            }
            match &mut s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => hoist_kind(body,vectors),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, declarations,vectors);
                    if let Some(b) = else_body {
                        walk(b, declarations,vectors);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    walk(body, declarations,vectors)
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        walk(&mut c.body, declarations,vectors);
                    }
                    if let Some(b) = default {
                        walk(b, declarations,vectors);
                    }
                }
                _ => {}
            }
            output.push(s);
        }
        *body = output;
    }
    let mut declarations = Vec::new();
    walk(body, &mut declarations,vectors);
    declarations.append(body);
    *body = declarations;
}

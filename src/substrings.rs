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
                    output.push(Stmt::Assign {
                        name: name.clone(),
                        value,
                    });
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
pub fn resolve(mut program: Program) -> Result<Program> {
    fn procedures(body:&[Stmt],names:&mut std::collections::HashSet<String>) {
        for statement in body {match statement {
            Stmt::Function {name,body,..} | Stmt::Subroutine {name,body,..} => {names.insert(name.clone());procedures(body,names);},
            Stmt::Module {body,..}=>procedures(body,names),_=>{},
        }}
    }
    let mut scope=DynamicScope::default();procedures(&program.body,&mut scope.procedures);
    dynamic_body(&mut program.body, &scope, true, &mut 0)?;
    body(&mut program.body, &Characters::new(), &mut 0)?;
    Ok(program)
}

pub(crate) fn deferred(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::CharacterLength(e) if matches!(e.as_ref(), Expr::Ident(n) if n == "@deferred_character"))
}

#[derive(Clone, Default)]
struct DynamicScope {
    characters: Characters,
    lengths: HashMap<String, String>,
    symbols: HashMap<String, (TypeSpec, Vec<Expr>)>,
    procedures: std::collections::HashSet<String>,
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
        Expr::Call(n, args) if n == "trim" && args.len() == 1 => capacity(&args[0], scope),
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
                let size = capacity(value, scope).with_context(|| format!("cannot infer bounded storage for deferred CHARACTER `{name}`"))?;
                *sizes.get_mut(name).unwrap() = sizes[name].max(size);
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
        Expr::Bin(BinOp::Concat, a, b) => binary(BinOp::Add, logical_length(a, scope, output, serial)?, logical_length(b, scope, output, serial)?),
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
    if scope.procedure && matches!(e, Expr::Ident(n) | Expr::Section {name:n,..} if scope.lengths.contains_key(n)) {
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
    let mut scope = inherited.clone();
    scope.procedure = !local;
    let mut targets = vec![];
    for s in statements.iter() {
        match s {
            Stmt::VarDecl { kind, names } => for name in names {
                scope.characters.remove(name);
                scope.lengths.remove(name);
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
    for iteration in 0..=targets.len() + 1 {
        let mut sizes = targets.iter().map(|n| (n.clone(), scope.characters[n])).collect::<Characters>();
        assignment_capacities(statements, &targets, &scope, &mut sizes)?;
        if targets.iter().all(|n| sizes[n] == scope.characters[n]) { break; }
        if iteration == targets.len() + 1 { bail!("deferred CHARACTER assignment has unbounded storage growth"); }
        for name in &targets {
            if sizes[name] >= i32::MAX as usize { bail!("deferred CHARACTER storage is too large"); }
            scope.characters.insert(name.clone(), sizes[name]);
        }
    }
    let mut result = vec![];
    for mut statement in std::mem::take(statements) {
        let mut prefix = vec![];
        match &mut statement {
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
                if let Some(length) = scope.lengths.get(name).cloned() {
                    let length_value = logical_length(value, &mut scope, &mut prefix, serial)?;
                    let saved = capture(length_value, &mut scope, &mut prefix, serial);
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
                        prefix.push(Stmt::If {cond:binary(BinOp::Gt,size,int(0)),then_body:vec![Stmt::Assign {name:name.clone(),value:replacement}],else_body:None});
                        result.extend(prefix);
                        continue;
                    }
                }
                for e in indices {
                    if !matches!(e, Expr::Section { .. }) { dynamic_expression(e, &mut scope, &mut prefix, serial)?; }
                }
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
            Stmt::Block { body } => dynamic_body(body, &scope, local, serial)?,
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
}

/// Generated declarations belong to the enclosing procedure, even when their
/// captures execute conditionally or inside a loop.
pub fn hoist(body: &mut Vec<Stmt>) {
    fn walk(body: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        let mut output = Vec::new();
        for mut s in std::mem::take(body) {
            if matches!(&s, Stmt::VarDecl {names,..} if names.iter().all(|n|n.starts_with("@pointer_") || n.starts_with("@substring_") || n.starts_with("@character_") || n.starts_with("@vector_") || n.starts_with("@reduction_")))
                || matches!(&s, Stmt::ArrayDecl {name,..} if name.starts_with("@pointer_") || name.starts_with("@vector_") || name.starts_with("@reduction_"))
            {
                declarations.push(s);
                continue;
            }
            match &mut s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => hoist(body),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, declarations);
                    if let Some(b) = else_body {
                        walk(b, declarations);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    walk(body, declarations)
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        walk(&mut c.body, declarations);
                    }
                    if let Some(b) = default {
                        walk(b, declarations);
                    }
                }
                _ => {}
            }
            output.push(s);
        }
        *body = output;
    }
    let mut declarations = Vec::new();
    walk(body, &mut declarations);
    declarations.append(body);
    *body = declarations;
}

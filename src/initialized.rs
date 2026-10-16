// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Definite-unassignment errors plus runtime flags for path-dependent reads.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq)]
enum State { No, Maybe, Yes }
fn join(a: State, b: State) -> State { if a == b { a } else { State::Maybe } }
#[derive(Clone)]
struct Variable {
    flag: String,
    extent: Option<i64>,
    state: State,
    elements: BTreeMap<i64, State>,
    runtime_sized: bool,
    heap_length: Option<Expr>,
    element_width: i64,
}
type Env = BTreeMap<String, Variable>;
fn int(n: i64) -> Expr { Expr::IntLit(n.to_string()) }
fn key(e: &Expr) -> Option<(&str, Option<&Expr>)> {
    match e {
        Expr::Ident(n) => Some((n, None)),
        Expr::Index(n, a) | Expr::Call(n, a) if a.len() == 1 => Some((n, Some(&a[0]))),
        _ => None,
    }
}
fn flag(v: &Variable, index: Option<Expr>) -> Expr {
    match index { Some(i) => Expr::Index(v.flag.clone(), vec![i]), None => Expr::Ident(v.flag.clone()) }
}
fn guard(value: Expr, name: &str, index: Option<Expr>, width: i64) -> Stmt {
    let index=index.map(|i|if width==1 {i} else {Expr::Bin(crate::ast::BinOp::Add,Box::new(Expr::Bin(crate::ast::BinOp::Div,Box::new(Expr::Bin(crate::ast::BinOp::Sub,Box::new(i),Box::new(int(1)))),Box::new(int(width)))),Box::new(int(1)))});
    let (name,index)=if let Some((original,element))=crate::character_arrays::original_element(name) {
        (original,Some(int(element)))
    } else {(name,index)};
    let mut args = vec![value, Expr::Str(name.into())];
    args.extend(index);
    Stmt::CallSub { name: "@check_initialized".into(), args }
}
fn constant(e: &Expr) -> Option<i64> { crate::parameters::integer_constant(e).ok() }

fn check(name: &str, index: Option<&Expr>, env: &Env, out: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    let Some(v) = env.get(name) else { return Ok(()) };
    if v.heap_length.is_none() {if let (Some(n), None) = (v.extent, index) {
        if let Some(i) = (1..=n).find(|i| *v.elements.get(i).unwrap_or(&v.state) == State::No) {
            bail!("read of uninitialized variable `{name}`: element {i} before assignment");
        }
    }
    }
    let state = if v.extent.is_some() {
        if v.extent == Some(0) { return Ok(()); }
        if let Some(i) = index.and_then(constant) { *v.elements.get(&i).unwrap_or(&v.state) }
        else if v.state == State::No && !v.elements.is_empty() { State::Maybe } else { v.state }
    } else { v.state };
    match state {
        State::No => {
            if let Some((original,element))=crate::character_arrays::original_element(name) {
                bail!("read of uninitialized variable `{original}`: element {element} before assignment");
            }
            bail!("read of uninitialized variable `{name}` before assignment");
        }
        State::Yes => {}
        State::Maybe => {
            if let (Some(n), None) = (v.extent, index) {
                *serial += 1;
                let idx = format!("@init_scan_{serial}");
                out.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![idx.clone()] });
                let index = Expr::Ident(idx.clone());
                out.push(Stmt::Do { var: idx, start: int(1), end: v.heap_length.clone().unwrap_or_else(||int(n)), body: vec![guard(flag(v, Some(index.clone())), name, Some(index),v.element_width)] });
            } else { out.push(guard(flag(v, index.cloned()), name, index.cloned(),v.element_width)); }
        }
    }
    Ok(())
}
fn read(e: &Expr, env: &Env, out: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    match e {
        Expr::Ident(n) => check(n, None, env, out, serial)?,
        Expr::Index(n, args) | Expr::Call(n, args) => {
            for arg in args { read(arg, env, out, serial)?; }
            if env.get(n).is_some_and(|v| v.extent.is_some()) {
                check(n, args.first(), env, out, serial)?;
            }
        }
        Expr::Bin(_, a, b) => { read(a, env, out, serial)?; read(b, env, out, serial)?; }
        Expr::Un(_, a) => read(a, env, out, serial)?,
        _ => {}
    }
    Ok(())
}
fn write(name: &str, index: Option<&Expr>, env: &mut Env, out: &mut Vec<Stmt>, serial: &mut usize) {
    let Some(v) = env.get_mut(name) else { return; };
    if let Some(n) = v.extent {
        if let Some(index) = index {
            if let Some(i) = constant(index) {
                v.elements.insert(i, State::Yes);
                if v.heap_length.is_none() && (1..=n).all(|i| v.elements.get(&i) == Some(&State::Yes)) { v.state = State::Yes; v.elements.clear(); }
            } else if v.state != State::Yes { v.state = State::Maybe; }
            out.push(Stmt::AssignIndex { name: v.flag.clone(), indices: vec![index.clone()], value: int(1) });
        } else {
            v.state = State::Yes; v.elements.clear();
            *serial += 1;
            let idx = format!("@init_fill_{serial}");
            out.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![idx.clone()] });
            out.push(Stmt::Do { var: idx.clone(), start: int(1), end: v.heap_length.clone().unwrap_or_else(||int(n)), body: vec![Stmt::AssignIndex { name: v.flag.clone(), indices: vec![Expr::Ident(idx)], value: int(1) }] });
        }
    } else {
        v.state = State::Yes;
        out.push(Stmt::Assign { name: v.flag.clone(), value: int(1) });
    }
}
fn merge(a: &Env, b: &Env) -> Env {
    let mut result = a.clone();
    for (name, v) in &mut result {
        if let Some(other) = b.get(name) {
            for i in v.elements.keys().chain(other.elements.keys()).copied().collect::<Vec<_>>() {
                v.elements.insert(i, join(*a[name].elements.get(&i).unwrap_or(&a[name].state), *other.elements.get(&i).unwrap_or(&other.state)));
            }
            v.state = join(v.state, other.state);
        }
    }
    result
}
fn possible_writes(body: &[Stmt], env: &mut Env) {
    for s in body {
        let names: Vec<&str> = match s {
            Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => vec![name],
            Stmt::Read { args } | Stmt::CallSub { args, .. } => args.iter().filter_map(key).map(|(n, _)| n).collect(),
            _ => vec![],
        };
        for name in names {
            if let Some(v) = env.get_mut(name) { if v.state == State::No { v.state = State::Maybe; } }
        }
        match s {
            Stmt::If { then_body, else_body, .. } => { possible_writes(then_body, env); if let Some(b) = else_body { possible_writes(b, env); } }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => possible_writes(body, env),
            Stmt::SelectCase { cases, default, .. } => { for c in cases { possible_writes(&c.body, env); } if let Some(b) = default { possible_writes(b, env); } }
            _ => {}
        }
    }
}

fn flow(body: &mut Vec<Stmt>, env: &mut Env, result: Option<&str>, serial: &mut usize) -> Result<bool> {
    let mut output = Vec::new();
    let mut terminated = false;
    for mut stmt in std::mem::take(body) {
        if terminated { output.push(stmt); continue; }
        let mut before = Vec::new();
        let mut after = Vec::new();
        match &mut stmt {
            Stmt::CallSub {name,args} if name=="@module_bind" => {
                let [Expr::Ident(local),Expr::Str(key),Expr::Str(initializer)]=args.as_slice() else {unreachable!()};
                let initial:crate::modules::Storage=serde_json::from_str(initializer)?;
                let variable=env.get_mut(local).context("missing module variable initialization state")?;
                let flags=crate::modules::Storage {kind:TypeSpec::Integer(None),values:vec![int(i64::from(initial.initialized));variable.extent.unwrap_or(1) as usize],initialized:true,rank:usize::from(variable.extent.is_some()),deferred:variable.heap_length.is_some()};
                before.push(Stmt::CallSub {name:"@module_bind".into(),args:vec![Expr::Ident(variable.flag.clone()),Expr::Str(format!("{key}.initialized")),Expr::Str(serde_json::to_string(&flags)?)]});
                variable.state=State::Maybe;variable.elements.clear();
            }
            Stmt::CallSub {name,args} if name=="@heap_bind" => {
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                let v=&env[local];before.push(Stmt::CallSub{name:"@heap_bind".into(),args:vec![Expr::Ident(v.flag.clone())]});
            }
            Stmt::CallSub {name,args} if name=="@heap_bounds" => {
                for value in &args[2..]{read(value,env,&mut before,serial)?;}
                for target in &args[..2]{let Expr::Ident(n)=target else{unreachable!()};write(n,None,env,&mut after,serial);}
            }
            Stmt::CallSub {name,args} if name=="@heap_allocate" || name=="@heap_resize" => {
                for arg in &args[2..] {read(arg,env,&mut before,serial)?;}
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                let flag=env[local].flag.clone();args.insert(2,Expr::Ident(flag));
                let Expr::Ident(length)=&args[1] else{unreachable!()};write(length,None,env,&mut after,serial);
            }
            Stmt::CallSub {name,args} if name=="@heap_free" || name=="@heap_drop" => {
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                args.push(Expr::Ident(env[local].flag.clone()));
            }
            Stmt::CallSub {name,args} if name=="@format_begin" => {
                for arg in &args[1..] {read(arg,env,&mut before,serial)?;}
                let Expr::Ident(dest)=&args[0] else {unreachable!()};
                write(dest,None,env,&mut after,serial);
            }
            Stmt::CallSub { name, args } if name == "@ptr_bind" => {
                let Expr::Ident(target) = &args[3] else { bail!("invalid pointer storage binding"); };
                let target=target.clone();
                let variable=env.get_mut(&target).ok_or_else(||anyhow::anyhow!("unknown pointer target `{target}`"))?;
                let target_flag=variable.flag.clone();
                // Subsequent pointer stores can define this storage indirectly.
                if variable.state==State::No {variable.state=State::Maybe;}
                for arg in &args[..3] {let Expr::Ident(dest)=arg else{unreachable!()};write(dest,None,env,&mut after,serial);}
                args.push(Expr::Ident(target_flag));
            }
            Stmt::Print { items } => for e in items { read(e, env, &mut before, serial)?; },
            Stmt::Assign { name, value } => { read(value, env, &mut before, serial)?; write(name, None, env, &mut after, serial); }
            Stmt::AssignIndex { name, indices, value } => {
                for i in indices.iter() { read(i, env, &mut before, serial)?; }
                read(value, env, &mut before, serial)?;
                write(name, indices.first(), env, &mut after, serial);
            }
            Stmt::Read { args } => for arg in args {
                if let Some((name, index)) = key(arg) { if let Some(i) = index { read(i, env, &mut before, serial)?; } write(name, index, env, &mut after, serial); }
            },
            Stmt::CallSub { name, args } if name=="@character_element_read" => {
                // The selected slot is known only at runtime. An unset different
                // slot must not make the whole dynamic access a compile error.
                let mut selected=env.clone();
                if let Some((source,_))=key(&args[1]) {
                    if let Some(v)=selected.get_mut(source) {if v.state==State::No {v.state=State::Maybe;}}
                }
                read(&args[1],&selected,&mut before,serial)?;
                let Expr::Ident(dest)=&args[0] else {unreachable!()};
                write(dest,None,env,&mut after,serial);
            }
            Stmt::CallSub { name, args } if name == "@saved_bind" || name == "@internal_read_byte" || name == "@internal_format_end" || name == "@character_assign" || name == "@character_length" || name=="@file_name" || name=="@file_newunit" => {
                for arg in &args[1..] { read(arg, env, &mut before, serial)?; }
                let Expr::Ident(dest)=&args[0] else {unreachable!()};
                write(dest,None,env,&mut after,serial);
            }
            Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("random_number") || name == "@read_record" => {
                if name == "@read_record" { for arg in &args[1..] { read(arg, env, &mut before, serial)?; } }
                if let Some((name, index)) = args.first().and_then(key) { if let Some(i) = index { read(i, env, &mut before, serial)?; } write(name, index, env, &mut after, serial); }
            }
            Stmt::CallSub {name,args} if name=="@random_seed_get" || name=="@date_snapshot" => {
                for arg in args {
                    if let Some((name,index))=key(arg) {if let Some(i)=index {read(i,env,&mut before,serial)?;}write(name,index,env,&mut after,serial);}
                }
            }
            Stmt::CallSub {name,args} if matches!(name.as_str(),"@file_inquire_unit"|"@file_inquire_file"|"@environment_query") => {
                let count=if name=="@environment_query" {3} else {4};
                for arg in &args[count..] {read(arg,env,&mut before,serial)?;}
                for arg in &args[..count] {
                    let Expr::Ident(dest)=arg else {unreachable!()};write(dest,None,env,&mut after,serial);
                }
            }
            Stmt::CallSub { name, args } if name == "@read_record_status" || name=="@read_character_field" => {
                for arg in &args[2..] { read(arg, env, &mut before, serial)?; }
                for arg in &args[..2] {
                    let Expr::Ident(dest) = arg else { unreachable!() };
                    write(dest, None, env, &mut after, serial);
                }
            }
            Stmt::CallSub { name, args } if name == "@undefine" => {
                let Expr::Ident(name) = &args[0] else { unreachable!() };
                let v = env.get_mut(name).ok_or_else(|| anyhow::anyhow!("undefined output argument `{name}`"))?;

                v.state = State::No; v.elements.clear();
                stmt = if v.heap_length.is_some() {Stmt::Block{body:vec![]}} else if let Some(extent)=v.extent {
                    *serial+=1;let index=format!("@undefine_index_{serial}");
                    Stmt::Block{body:vec![Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]},Stmt::Do{var:index.clone(),start:int(1),end:int(extent),body:vec![Stmt::AssignIndex{name:v.flag.clone(),indices:vec![Expr::Ident(index)],value:int(0)}]}]}
                } else { Stmt::Assign { name: v.flag.clone(), value: int(0) } };
            }
            Stmt::CallSub { name,args } if name=="@stop" || name=="@error_stop" => {
                for argument in args {read(argument,env,&mut before,serial)?;}
                terminated=true;
            }
            Stmt::CallSub { args, .. } => for arg in args { read(arg, env, &mut before, serial)?; },
            Stmt::If { cond, then_body, else_body } => {
                read(cond, env, &mut before, serial)?;
                let mut a = env.clone(); let mut b = env.clone();
                if !matches!(cond, Expr::Logical(false)) { flow(then_body, &mut a, result, serial)?; }
                if !matches!(cond, Expr::Logical(true)) { if let Some(body) = else_body { flow(body, &mut b, result, serial)?; } }
                *env = match cond { Expr::Logical(true) => a, Expr::Logical(false) => b, _ => merge(&a, &b) };
            }
            Stmt::Do { var, start, end, body } => {
                read(start, env, &mut before, serial)?; read(end, env, &mut before, serial)?;
                write(var, None, env, &mut before, serial);
                if !matches!((constant(start), constant(end)), (Some(a), Some(b)) if a > b) {
                    let mut inner = env.clone(); possible_writes(body, &mut inner);
                    // A reduction over runtime-sized storage can execute zero
                    // iterations. Check its uninitialized elements only if read.
                    if constant(end).is_none() {
                        for v in inner.values_mut() {
                            if v.runtime_sized && v.state==State::No {v.state=State::Maybe;}
                        }
                    }
                    flow(body, &mut inner, result, serial)?;
                    *env = merge(env, &inner);
                }
            }
            Stmt::DoWhile { cond, body } => {
                let mut inner = env.clone(); possible_writes(body, &mut inner);
                read(cond, &inner, &mut before, serial)?;
                flow(body, &mut inner, result, serial)?;
                // Check at the loop header so CYCLE cannot bypass a guard.
                if !before.is_empty() {
                    let condition = std::mem::replace(cond, Expr::Logical(true));
                    before.push(Stmt::If { cond: Expr::Un(crate::ast::UnOp::Not, Box::new(condition)), then_body: vec![Stmt::Exit], else_body: None });
                    before.append(body);
                    *body = std::mem::take(&mut before);
                }
                *env = merge(env, &inner);
            }
            Stmt::SelectCase { expr, cases, default } => {
                read(expr, env, &mut before, serial)?;
                let original = env.clone();
                let mut merged = if default.is_none() { Some(original.clone()) } else { None };
                for case in cases {
                    let mut branch = original.clone(); flow(&mut case.body, &mut branch, result, serial)?;
                    merged = Some(merged.map_or(branch.clone(), |a| merge(&a, &branch)));
                }
                if let Some(body) = default {
                    let mut branch = original; flow(body, &mut branch, result, serial)?;
                    merged = Some(merged.map_or(branch.clone(), |a| merge(&a, &branch)));
                }
                if let Some(merged) = merged { *env = merged; }
            }
            Stmt::Block { body } => { scope(body, env, &[], None, serial)?; }
            Stmt::Function { name, params, body, .. } => { scope(body, &mut Env::new(), params, Some(name), serial)?; }
            Stmt::Subroutine { params, body, .. } => { scope(body, &mut Env::new(), params, None, serial)?; }
            Stmt::Module { body, .. } => {
                for member in body {
                    match member {
                        Stmt::Function { name, params, body, .. } => { scope(body, &mut Env::new(), params, Some(name), serial)?; }
                        Stmt::Subroutine { params, body, .. } => { scope(body, &mut Env::new(), params, None, serial)?; }
                        _ => {}
                    }
                }
            }
            Stmt::Return(value) => {
                if let Some(e) = value { read(e, env, &mut before, serial)?; }
                else if let Some(name) = result { check(name, None, env, &mut before, serial)?; }
                terminated = true;
            }
            Stmt::Exit | Stmt::Cycle => { terminated = true; }
            _ => {}
        }
        output.extend(before); output.push(stmt); output.extend(after);
    }
    *body = output;
    Ok(terminated)
}

fn collect(body: &[Stmt], variables: &mut BTreeMap<String, Option<i64>>) {
    fn expr(e: &Expr, vars: &mut BTreeMap<String, Option<i64>>) {
        match e {
            Expr::Ident(n) => { vars.entry(n.clone()).or_insert(None); }
            Expr::Call(_, args) | Expr::Index(_, args) => for a in args { expr(a, vars); },
            Expr::Bin(_, a, b) => { expr(a, vars); expr(b, vars); }
            Expr::Un(_, a) => expr(a, vars),
            _ => {}
        }
    }
    for stmt in body {
        match stmt {
            Stmt::VarDecl { names, .. } => for n in names { variables.entry(n.clone()).or_insert(None); },
            Stmt::ArrayDecl { name, dims, .. } => { variables.insert(name.clone(), dims.first().and_then(constant)); }
            Stmt::Assign { name, value } => { variables.entry(name.clone()).or_insert(None); expr(value, variables); }
            Stmt::AssignIndex { indices, value, .. } => { for i in indices { expr(i, variables); } expr(value, variables); }
            Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => for e in items { expr(e, variables); },
            Stmt::If { cond, then_body, else_body } => { expr(cond, variables); collect(then_body, variables); if let Some(b) = else_body { collect(b, variables); } }
            Stmt::Do { var, start, end, body } => { variables.entry(var.clone()).or_insert(None); expr(start, variables); expr(end, variables); collect(body, variables); }
            Stmt::DoWhile { cond, body } => { expr(cond, variables); collect(body, variables); }
            Stmt::SelectCase { expr: e, cases, default } => { expr(e, variables); for c in cases { collect(&c.body, variables); } if let Some(b) = default { collect(b, variables); } }
            Stmt::Return(Some(e)) => expr(e, variables),
            _ => {}
        }
    }
}
fn scope(body: &mut Vec<Stmt>, inherited: &mut Env, params: &[String], result: Option<&str>, serial: &mut usize) -> Result<()> {
    let mut names = BTreeMap::new(); collect(body, &mut names);
    let declared: Vec<String> = body.iter().flat_map(|s| match s {
        Stmt::VarDecl { names, .. } => names.clone(),
        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
        _ => vec![],
    }).collect();
    if let Some(name) = result { names.entry(name.into()).or_insert(None); }
    let mut heap_shapes=crate::allocatables::Shapes::new();crate::allocatables::metadata(body,&mut heap_shapes);
    let mut env = inherited.clone();
    let mut prefix = Vec::new();
    for (name, extent) in names {
        if env.contains_key(&name) && !declared.contains(&name) { continue; }
        *serial += 1;
        let flag = format!("@initialized_{serial}");
        let assigned = params.contains(&name) && !body.iter().any(|s| matches!(s, Stmt::DummyIntent { name: n, intent } if n == &name && intent == "intent(out)"));
        let runtime_sized=name.starts_with("@pointer_") || body.iter().any(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==&name && intent.starts_with("@bounded:")));
        let element_width=body.iter().find_map(|s|match s {Stmt::DummyIntent{name:n,intent} if n==&name=>intent.strip_prefix("@character_element_width:").and_then(|n|n.parse().ok()),_=>None}).unwrap_or(1);
        let v = Variable { element_width, flag: flag.clone(), extent, state: if assigned { State::Yes } else { State::No }, elements: BTreeMap::new(), runtime_sized, heap_length: heap_shapes.contains_key(&format!("@heap:{name}")).then(||crate::allocatables::length(&name,&heap_shapes).unwrap()) };
        if let Some(n) = extent {
            prefix.push(Stmt::ArrayDecl { kind: TypeSpec::Integer(None), name: flag.clone(), dims: vec![int(n)] });
            let index = format!("@init_clear_{serial}");
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
            prefix.push(Stmt::Do { var: index.clone(), start: int(1), end: int(n), body: vec![Stmt::AssignIndex { name: flag, indices: vec![Expr::Ident(index)], value: int(i64::from(assigned)) }] });
        } else {
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![flag.clone()] });
            prefix.push(Stmt::Assign { name: flag, value: int(i64::from(assigned)) });
        }
        env.insert(name, v);
    }
    let terminated = flow(body, &mut env, result, serial)?;
    if !terminated { if let Some(name) = result { check(name, None, &env, body, serial)?; } }
    for (name, value) in inherited { if !declared.contains(name) { if let Some(v) = env.get(name) { *value = v.clone(); } } }
    prefix.append(body); *body = prefix;
    Ok(())
}
pub fn instrument(mut program: Program) -> Result<Program> {
    program = crate::subroutines::resolve(program)?;
    program = crate::heap::cleanup(program);
    scope(&mut program.body, &mut Env::new(), &[], None, &mut 0)?;
    crate::modules::unify_storage(program)
}

#[cfg(test)]
pub fn instrument_with_source(program: Program, source: &str, filename: &str) -> Result<Program> {
    instrument_with_origins(program, source, filename, &[])
}

pub(crate) fn instrument_with_origins(program: Program, source: &str, filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin]) -> Result<Program> {
    fn locate(body: &mut Vec<Stmt>, inherited: Option<usize>, source: &str, filename: &str,
        origins: &[crate::source_diagnostics::SourceOrigin]) {
        let mut position = inherited;
        for stmt in body.iter_mut() {
            match stmt {
                Stmt::SourceLocation(offset) => position = Some(*offset),
                Stmt::CallSub { name, args } if name == "@check_initialized" => {
                    if let Some(offset) = position {
                        let (source, offset, filename) = if let Some(origin) = origins.iter().find(|o| o.range.contains(&offset)) {
                            let filename = std::path::Path::new(&origin.name).file_name()
                                .and_then(|n| n.to_str()).unwrap_or(&origin.name);
                            (&source[origin.range.clone()], offset - origin.range.start, filename)
                        } else { (source, offset, filename) };
                        let line = source.as_bytes()[..offset.min(source.len())].iter().filter(|&&b| b == b'\n').count() + 1;
                        if args.len() == 2 { args.push(Expr::Str(String::new())); }
                        args.push(Expr::Str(format!("{filename}:{line}")));
                    }
                }
                Stmt::If { then_body, else_body, .. } => {
                    locate(then_body, position, source, filename, origins);
                    if let Some(body) = else_body { locate(body, position, source, filename, origins); }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body }
                | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => locate(body, position, source, filename, origins),
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases { locate(&mut case.body, position, source, filename, origins); }
                    if let Some(body) = default { locate(body, position, source, filename, origins); }
                }
                _ => {}
            }
        }
        body.retain(|stmt| !matches!(stmt, Stmt::SourceLocation(_)));
    }
    let mut program = instrument(program)?;
    locate(&mut program.body, None, source, filename, origins);
    Ok(program)
}

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_guards_keep_original_statement_lines() {
        let source = "! comment\nimplicit none\ninteger :: n,i,a(3),x\nread(*,*) n\ndo i=1,n\na(i)=7\nend do\nif(n>0) then\nx=7\nend if\nprint *, sum(a)\nprint *, a\nprint *, x\nblock\nprint *, sum(a)\nend block\nend";
        let p = crate::sections::resolve(crate::reductions::resolve(crate::parameters::resolve(parse(source)).unwrap()).unwrap()).unwrap();
        let p = super::instrument_with_source(p, source, "locations.f90").unwrap();
        let dump = format!("{p:?}");
        for line in [11, 12, 13, 15] {
            assert!(dump.contains(&format!("locations.f90:{line}")), "{dump}");
        }
        assert!(!dump.contains("SourceLocation"), "{dump}");
    }

    fn parse(source: &str) -> crate::ast::Program {
        let tokens = crate::lexer::lex(source);
        crate::parser::parse_with_src(source, &tokens, "initialization.f90").unwrap()
    }
    #[test]
    fn rejects_definite_unassigned_reads() {
        for source in [
            "integer :: i\nprint *, i\nend",
            "integer :: i\ni=i+1\nend",
            "real :: x\nprint *, sin(x)\nend",
            "integer :: a(2)\na(1)=1\nprint *, a(2)\nend",
            "integer :: i\ni=3\nblock\ninteger :: i\nprint *, i\nend block\nend",
        ] {
            assert!(super::instrument(parse(source)).unwrap_err().to_string().contains("uninitialized"), "{source}");
        }
    }
    #[test]
    fn preserves_valid_initialization_and_guards_uncertain_reads() {
        for source in [
            "integer :: i\ni=3\nprint *, i\nend",
            "integer :: i\nread(*,*) i\nprint *, i\nend",
            "real :: x\ncall random_number(x)\nprint *, x\nend",
            "integer :: i, x\nread(*,*) i\nif(i>0) then\nx=7\nend if\nprint *, x\nend",
            "integer :: i, a(2)\nread(*,*) i\na(i)=7\nprint *, a(1)\nend",
        ] { super::instrument(parse(source)).unwrap(); }
        let p = super::instrument(parse("integer :: i,x\nread(*,*) i\nif(i>0) then\nx=7\nend if\nprint *, x\nend")).unwrap();
        assert!(format!("{p:?}").contains("@check_initialized"));
    }
}

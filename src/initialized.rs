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
    zero_sized: bool,
    flag: String,
    extent: Option<i64>,
    state: State,
    elements: BTreeMap<i64, State>,
    runtime_sized: bool,
    heap_length: Option<Expr>,
    heap_allocated: Option<Expr>,
    element_width: i64,
    character_length: Option<usize>,
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

/// Statements between these markers read without initialization checks; the
/// values they write count as set.
pub(crate) const UNCHECKED_BEGIN: &str = "@unchecked_begin";
/// The variable may have been given a value here, as IOMSG may be by I/O.
pub(crate) const MAY_DEFINE: &str = "@may_define";
pub(crate) const UNCHECKED_END: &str = "@unchecked_end";
pub(crate) const COPY_INITIALIZATION: &str = "@copy_initialization";
/// Copy storage without reading undefined values, preserving its definedness.
pub(crate) const TRANSPORT: &str = "@transport_value";
pub(crate) const TRANSPORT_FROM_CODES: &str = "@transport_from_codes";
pub(crate) const TRANSPORT_TO_CODES: &str = "@transport_to_codes";
thread_local! { static UNCHECKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn check(name: &str, index: Option<&Expr>, env: &Env, out: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    if UNCHECKED.with(|c| c.get()) > 0 { return Ok(()); }
    let Some(v) = env.get(name) else { return Ok(()) };
    if v.zero_sized {return Ok(());}
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
    let mut location=None;
    for mut stmt in std::mem::take(body) {
        if let Stmt::CallSub { name, args } = &stmt {
            if name == TRANSPORT_FROM_CODES || name == TRANSPORT_TO_CODES {
                let [Expr::Ident(to), offset, width, Expr::Ident(from), capacity] = args.as_slice() else {bail!("invalid CHARACTER storage transport");};
                let dst=env.get(to).cloned().context("missing CHARACTER transport destination")?;
                let src=env.get(from).cloned().context("missing CHARACTER transport source")?;
                for e in [offset,width,capacity] {read(e,env,&mut output,serial)?;}
                *serial+=1;
                let index=format!("@transport_code_index_{serial}");
                let status=format!("@transport_code_status_{serial}");
                let code=format!("@transport_code_value_{serial}");
                for n in [&index,&status,&code] {output.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![n.clone()]});}
                let id=|n:&str|Expr::Ident(n.into());
                let bin=|op,a,b|Expr::Bin(op,Box::new(a),Box::new(b));
                let position=bin(crate::ast::BinOp::Add,offset.clone(),bin(crate::ast::BinOp::Add,id(&index),int(1)));
                let assign=|name:String,value|Stmt::Assign{name,value};
                let each=|body|Stmt::Do{var:index.clone(),start:int(0),end:bin(crate::ast::BinOp::Sub,width.clone(),int(1)),body};
                if name==TRANSPORT_FROM_CODES {
                    output.push(assign(status.clone(),int(1)));
                    output.push(each(vec![assign(status.clone(),bin(crate::ast::BinOp::Mul,id(&status),flag(&src,Some(position.clone()))))]));
                    output.push(Stmt::If{cond:bin(crate::ast::BinOp::Ne,id(&status),int(0)),then_body:vec![
                        assign(to.clone(),Expr::Str(" ".repeat(usize::try_from(constant(capacity).context("CHARACTER transport requires a fixed capacity")?)?))),
                        each(vec![Stmt::CallSub{name:"@character_code_store".into(),args:vec![id(to),id(&index),Expr::Index(from.clone(),vec![position])]}])],else_body:None});
                    output.push(assign(dst.flag,id(&status)));
                } else {
                    output.push(each(vec![
                        Stmt::If{cond:bin(crate::ast::BinOp::Ne,flag(&src,None),int(0)),then_body:vec![
                            Stmt::CallSub{name:"@internal_read_byte".into(),args:vec![id(&code),id(from),id(&index),capacity.clone()]},
                            Stmt::AssignIndex{name:to.clone(),indices:vec![position.clone()],value:id(&code)}],else_body:None},
                        Stmt::AssignIndex{name:dst.flag,indices:vec![position],value:flag(&src,None)}]));
                }
                let dst=env.get_mut(to).unwrap();dst.state=State::Maybe;dst.elements.clear();
                continue;
            }
            if name == TRANSPORT {
                let [target, source] = args.as_slice() else {bail!("invalid storage transport");};
                let (to, ti) = key(target).context("storage transport requires a variable destination")?;
                let (from, si) = key(source).context("storage transport requires a variable source")?;
                let dst = env.get(to).cloned().context("missing storage transport destination")?;
                let src = env.get(from).cloned().context("missing storage transport source")?;
                for index in [ti, si].into_iter().flatten() {read(index,env,&mut output,serial)?;}
                let df = flag(&dst,ti.cloned());
                let sf = flag(&src,si.cloned());
                let assign = |target:Expr,value:Expr| match target {
                    Expr::Ident(name)=>Stmt::Assign{name,value},
                    Expr::Index(name,indices)|Expr::Call(name,indices)=>Stmt::AssignIndex{name,indices,value},
                    _=>unreachable!(),
                };
                let copy=if let Some(length)=dst.character_length {
                    Stmt::CallSub{name:"@character_assign".into(),args:vec![target.clone(),source.clone(),int(length as i64)]}
                } else {assign(target.clone(),source.clone())};
                output.push(Stmt::If {cond:Expr::Bin(crate::ast::BinOp::Ne,Box::new(sf.clone()),Box::new(int(0))),
                    then_body:vec![copy],else_body:None});
                output.push(assign(df,sf));
                let dst=env.get_mut(to).unwrap();dst.state=State::Maybe;dst.elements.clear();
                continue;
            }
            if name == COPY_INITIALIZATION {
                fn storage(e:&Expr)->Option<&str> {
                    match e {
                        Expr::Ident(n)=>Some(n),
                        Expr::Call(n,a) if n=="@packed_view"=>storage(&a[0]),
                        _=>None,
                    }
                }
                let arrays=args.iter().filter_map(storage).collect::<Vec<_>>();
                if let [target,source]=arrays.as_slice() {
                    if let (Some(dst),Some(src))=(env.get(*target).cloned(),env.get(*source).cloned()) {
                        if dst.extent.is_some() && src.extent.is_some() {
                            *serial+=1;
                            let index=format!("@move_initialization_{serial}");
                            output.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                            output.push(Stmt::Do {var:index.clone(),start:int(1),
                                end:src.heap_length.clone().unwrap_or_else(||int(src.extent.unwrap())),
                                body:vec![Stmt::AssignIndex{name:dst.flag,indices:vec![Expr::Ident(index.clone())],
                                    value:Expr::Index(src.flag,vec![Expr::Ident(index)])}]});
                            let dst=env.get_mut(*target).unwrap();
                            dst.state=State::Maybe;
                            dst.elements.clear();
                        }
                    }
                }
                continue;
            }
            if name == MAY_DEFINE {
                if let Some(Expr::Ident(n)) = args.first() {
                    if let Some(v) = env.get_mut(n) {
                        if v.state == State::No { v.state = State::Maybe; }
                        for e in v.elements.values_mut() { if *e == State::No { *e = State::Maybe; } }
                    }
                }
                continue;
            }
            if name == UNCHECKED_BEGIN || name == UNCHECKED_END {
                let begin = name == UNCHECKED_BEGIN;
                UNCHECKED.with(|c| c.set(if begin { c.get() + 1 } else { c.get().saturating_sub(1) }));
                continue;
            }
        }
        if terminated { output.push(stmt); continue; }
        let mut before = Vec::new();
        let mut after = Vec::new();
        if let Stmt::SourceLocation(offset)=&stmt {location=Some(*offset);}
        (|| -> Result<()> {match &mut stmt {
            Stmt::CallSub {name,args} if name=="@c_invoke" => {
                let Expr::Str(encoded)=&args[0] else {bail!("invalid C adapter metadata")};
                let sig:crate::c_interop::Signature=serde_json::from_str(encoded)?;
                let mut values=args[1..].iter();
                for arg in &sig.args {
                    let value=values.next().context("missing C adapter argument")?;
                    if !arg.dims.is_empty() {
                        for field in std::iter::once(value).chain(values.by_ref().take(if crate::c_interop::assumed_shape(&arg.dims) {3+2*arg.dims.len()}else{3})) {read(field,env,&mut before,serial)?;}
                        continue;
                    }
                    if arg.input {read(value,env,&mut before,serial)?;}
                    if arg.output {if let Expr::Ident(n)=value {write(n,None,env,&mut after,serial);}}
                }
                if sig.result.is_some() {if let Some(Expr::Ident(n))=args.last(){write(n,None,env,&mut after,serial);}}
            }
            Stmt::CallSub {name,args} if name=="@module_bind" => {
                let [Expr::Ident(local),Expr::Str(key),Expr::Str(initializer)]=args.as_slice() else {unreachable!()};
                let initial:crate::modules::Storage=serde_json::from_str(initializer)?;
                let variable=env.get_mut(local).context("missing module variable initialization state")?;
                let flags=crate::modules::Storage {kind:TypeSpec::Integer(None),values:vec![int(i64::from(initial.initialized));variable.extent.unwrap_or(1) as usize],initialized:true,rank:usize::from(variable.extent.is_some()),deferred:variable.heap_length.is_some()};
                before.push(Stmt::CallSub {name:"@module_bind".into(),args:vec![Expr::Ident(variable.flag.clone()),Expr::Str(format!("{key}.initialized")),Expr::Str(serde_json::to_string(&flags)?)]});
                variable.state=State::Maybe;variable.elements.clear();
            }
            Stmt::CallSub {name,args} if name=="@heap_identity" => {
                let Expr::Ident(n)=&args[0] else {bail!("invalid allocation identity destination")};
                write(n,None,env,&mut after,serial);
            }
            Stmt::CallSub {name,args} if name=="@heap_transfer_buffers" => {
                let [Expr::Ident(to),Expr::Ident(from)]=args.as_slice() else {bail!("invalid allocation transfer")};
                let (to,from)=(to.clone(),from.clone());
                args.extend([Expr::Ident(env[&to].flag.clone()),Expr::Ident(env[&from].flag.clone())]);
                for n in [&to,&from] {let v=env.get_mut(n).unwrap();v.state=State::Maybe;v.elements.clear();}
            }
            Stmt::CallSub {name,args} if name=="@heap_bind" => {
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                let v=&env[local];before.push(Stmt::CallSub{name:"@heap_bind".into(),args:vec![Expr::Ident(v.flag.clone())]});
            }
            Stmt::CallSub {name,args} if name=="@heap_record_pointer" => {
                read(&args[0],env,&mut before,serial)?;
                for target in &args[2..] {let Expr::Ident(n)=target else{bail!("invalid owned pointer descriptor")};write(n,None,env,&mut after,serial);}
            }
            Stmt::CallSub {name,args} if matches!(name.as_str(),"@record_field_borrow"|"@record_field_unborrow") => {
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                let local=local.clone();
                let mask=env[&local].flag.clone();
                if name=="@record_field_borrow" {
                    let (source,index)=key(&args[1]).context("field borrow requires a designator")?;
                    let source_flag=flag(&env[source],index.cloned());
                    // Writes through the view can define any borrowed cell.
                    // Retain the actual masks for runtime checks on both aliases.
                    let variable=env.get_mut(source).unwrap();
                    variable.state=State::Maybe;variable.elements.clear();
                    let count=args.pop().unwrap();
                    args.extend([Expr::Ident(mask),source_flag,count]);
                    let v=env.get_mut(&local).unwrap();v.state=State::Maybe;v.elements.clear();
                } else {args.push(Expr::Ident(mask));}
            }
            Stmt::CallSub {name,args} if matches!(name.as_str(),"@heap_record_load"|"@heap_record_store"|"@heap_record_borrow"|"@heap_record_unborrow") => {
                let Expr::Ident(local)=&args[0] else{unreachable!()};
                let local=local.clone();
                let mask=env[&local].flag.clone();
                if name!="@heap_record_store" {
                    if name!="@heap_record_unborrow" {read(&args[1],env,&mut before,serial)?;}
                    for meta in &args[2..] {let Expr::Ident(n)=meta else{unreachable!()};write(n,None,env,&mut after,serial);}
                    let variable=env.get_mut(&local).unwrap();variable.state=State::Maybe;variable.elements.clear();
                } else {
                    for meta in &args[2..] {read(meta,env,&mut before,serial)?;}
                    if let Some((n,index))=key(&args[1]) {write(n,index,env,&mut after,serial);}
                }
                args.push(Expr::Ident(mask));
            }
            Stmt::CallSub {name,args} if name=="@heap_record_dispose" || name=="@heap_record_drop_array" => {
                if name=="@heap_record_dispose" {read(&args[0],env,&mut before,serial)?;}
                else {read(&args[1],env,&mut before,serial)?;}
            }
            Stmt::CallSub {name,..} if name=="@heap_character_bind" || name=="@heap_character_clear" => {},
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
            Stmt::CallSub {name,args} if name=="@saved_bind" => {
                let undefined=args.get(3)==Some(&Expr::Logical(false));
                args.truncate(3);
                for arg in &args[1..] {read(arg,env,&mut before,serial)?;}
                let Expr::Ident(dest)=&args[0] else {unreachable!()};
                if undefined {
                    let Expr::Str(key)=&args[1] else {unreachable!()};
                    let variable=env.get_mut(dest).context("missing saved variable initialization state")?;
                    before.push(Stmt::CallSub {name:"@saved_bind".into(),args:vec![
                        Expr::Ident(variable.flag.clone()),Expr::Str(format!("{key}.initialized")),int(0)]});
                    variable.state=State::Maybe;
                } else {write(dest,None,env,&mut after,serial);}
            }
            Stmt::CallSub { name, args } if name == "@internal_read_byte" || name == "@internal_format_end" || name == "@character_assign" || name == "@character_length" || name == "@character_address" || name=="@file_name" || name=="@file_newunit" || name=="@file_open_dynamic" || name=="@file_open_try" || name=="@list_read_end" || name=="@stream_put" || name=="@stream_seek" => {
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
            Stmt::CallSub {name,args} if matches!(name.as_str(),"@file_inquire_unit"|"@file_inquire_file"|"@environment_query"|"@argument_query"|"@cwd_query") => {
                let count=if matches!(name.as_str(),"@environment_query"|"@argument_query") {3} else if name=="@cwd_query" {2} else {7};
                for arg in &args[count..] {read(arg,env,&mut before,serial)?;}
                for arg in &args[..count] {
                    let Expr::Ident(dest)=arg else {unreachable!()};write(dest,None,env,&mut after,serial);
                }
            }
            Stmt::CallSub{name,args} if name=="@nml_layout" => {
                for arg in args.iter() {read(arg,env,&mut before,serial)?;}
                for arg in &args[..2] {let Expr::Ident(dest)=arg else {unreachable!()};write(dest,None,env,&mut after,serial);}
            }
            Stmt::CallSub{name,args} if matches!(name.as_str(),"@nml_begin"|"@nml_internal_begin"|"@nml_next"|"@nml_record") => {
                let count=if name=="@nml_next" {4}else if name=="@nml_record" {1}else{2};
                for arg in &args[count..] {read(arg,env,&mut before,serial)?;}
                for arg in &args[..count] {let Expr::Ident(dest)=arg else {unreachable!()};write(dest,None,env,&mut after,serial);}
            }
            Stmt::CallSub{name,args} if name=="@list_read_complex_next" => {
                for arg in &args[3..] {read(arg,env,&mut before,serial)?;}
                for arg in &args[..3] {let Expr::Ident(dest)=arg else {unreachable!()};write(dest,None,env,&mut after,serial);}
            }
            Stmt::CallSub { name, args } if name == "@list_read_internal_begin" || name == "@list_read_begin" || name=="@list_read_next" || name=="@stream_get" || name == "@read_record_status" || name=="@read_record_checked" || name=="@read_character_field" => {
                for arg in &args[2..] { read(arg, env, &mut before, serial)?; }
                for arg in &args[..2] {
                    let Expr::Ident(dest) = arg else { unreachable!() };
                    write(dest, None, env, &mut after, serial);
                }
            }
            Stmt::CallSub{name,args} if name=="@undefine" && matches!(args.first(),Some(Expr::Index(_,_)|Expr::Call(_,_))) => {
                let Some((name,Some(index)))=args.first().and_then(key) else {bail!("invalid uninitialized READ destination");};
                read(index,env,&mut before,serial)?;
                let v=env.get_mut(name).context("undefined READ destination")?;
                if let Some(i)=constant(index) {v.elements.insert(i,State::No);} else {v.state=State::Maybe;v.elements.clear();}
                stmt=Stmt::AssignIndex{name:v.flag.clone(),indices:vec![index.clone()],value:int(0)};
            }
            Stmt::CallSub { name, args } if name == "@undefine" => {
                let Expr::Ident(name) = &args[0] else { unreachable!() };
                let v = env.get_mut(name).ok_or_else(|| anyhow::anyhow!("undefined output argument `{name}`"))?;

                v.state = State::No; v.elements.clear();
                // OUT association undefines existing heap elements as well as
                // fixed storage. A CHARACTER byte count can outlive its freed
                // buffer, so only clear the mask while the allocation exists.
                stmt = if let Some(extent)=v.heap_length.clone().or_else(||v.extent.map(int)) {
                    *serial+=1;let index=format!("@undefine_index_{serial}");
                    let clear=Stmt::Block{body:vec![Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]},Stmt::Do{var:index.clone(),start:int(1),end:extent,body:vec![Stmt::AssignIndex{name:v.flag.clone(),indices:vec![Expr::Ident(index)],value:int(0)}]}]};
                    if v.heap_length.is_some() {
                        if let Some(cond)=&v.heap_allocated {Stmt::If{cond:cond.clone(),then_body:vec![clear],else_body:None}} else {clear}
                    } else {clear}
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
                // Reads of heap storage under a runtime condition may never
                // execute (for example IF (ALLOCATED(x)) PRINT *, x). Preserve
                // the initialization mask and check only on the path taken.
                if !matches!(cond, Expr::Logical(_)) {
                    for branch in [&mut a, &mut b] {
                        for v in branch.values_mut().filter(|v| v.heap_length.is_some()) {
                            if v.state == State::No { v.state = State::Maybe; }
                            for state in v.elements.values_mut() {
                                if *state == State::No { *state = State::Maybe; }
                            }
                        }
                    }
                }
                // A branch that cannot run is removed rather than left
                // without the instrumentation that code generation expects.
                if !matches!(cond, Expr::Logical(false)) { flow(then_body, &mut a, result, serial)?; } else { then_body.clear(); }
                if !matches!(cond, Expr::Logical(true)) { if let Some(body) = else_body { flow(body, &mut b, result, serial)?; } } else { *else_body = None; }
                *env = match cond { Expr::Logical(true) => a, Expr::Logical(false) => b, _ => merge(&a, &b) };
            }
            Stmt::Do { var, start, end, body } => {
                read(start, env, &mut before, serial)?; read(end, env, &mut before, serial)?;
                write(var, None, env, &mut before, serial);
                if matches!((constant(start), constant(end)), (Some(a), Some(b)) if a > b) {
                    // Unreachable owned-allocation operations would otherwise
                    // reach codegen without their initialization-mask operands.
                    body.clear();
                } else {
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
            Stmt::Block { body } => { scope(body, env, &[], None, serial, None)?; }
            Stmt::Function { name, params, body, .. } => { scope(body, &mut Env::new(), params, Some(name), serial, None)?; }
            Stmt::Subroutine { params, body, .. } => { scope(body, &mut Env::new(), params, None, serial, None)?; }
            Stmt::Module { body, .. } => {
                for member in body {
                    match member {
                        Stmt::Function { name, params, body, .. } => { scope(body, &mut Env::new(), params, Some(name), serial, None)?; }
                        Stmt::Subroutine { params, body, .. } => { scope(body, &mut Env::new(), params, None, serial, None)?; }
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
        Ok(())})().map_err(|error|crate::source_diagnostics::at(error,location))?;
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
fn scope(body: &mut Vec<Stmt>, inherited: &mut Env, params: &[String], result: Option<&str>, serial: &mut usize, capture: Option<&mut BTreeMap<String,String>>) -> Result<()> {
    let mut names = BTreeMap::new(); collect(body, &mut names);
    let declared: Vec<String> = body.iter().flat_map(|s| match s {
        Stmt::VarDecl { names, .. } => names.clone(),
        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
        _ => vec![],
    }).collect();
    if let Some(name) = result { names.entry(name.into()).or_insert(None); }
    let mut heap_shapes=crate::allocatables::Shapes::new();crate::allocatables::metadata(body,&mut heap_shapes);
    let character_lengths=body.iter().filter_map(|s|match s {
        Stmt::VarDecl{kind:TypeSpec::Character(length),names}=>Some(names.iter().map(move |name|(name.clone(),length.unwrap_or(1)))),
        _=>None,
    }).flatten().collect::<BTreeMap<_,_>>();
    let mut env = inherited.clone();
    let mut prefix = Vec::new();
    for (name, extent) in names {
        if env.contains_key(&name) && !declared.contains(&name) { continue; }
        *serial += 1;
        let flag = format!("@initialized_{serial}");
        let assigned = params.contains(&name) && !body.iter().any(|s| matches!(s, Stmt::DummyIntent { name: n, intent } if n == &name && intent == "intent(out)"));
        let runtime_sized=name.starts_with("@pointer_") || body.iter().any(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==&name && intent.starts_with("@bounded:")));
        let element_width=body.iter().find_map(|s|match s {Stmt::DummyIntent{name:n,intent} if n==&name=>intent.strip_prefix("@character_element_width:").and_then(|n|n.parse().ok()),_=>None}).unwrap_or(1);
        let zero_sized=body.iter().any(|s|matches!(s,Stmt::VarDecl{kind:TypeSpec::Character(Some(0)),names} if names.contains(&name)))
            && !body.iter().any(|s|matches!(s,Stmt::CallSub{name:n,args} if n=="@heap_character_bind" && args.first()==Some(&Expr::Ident(name.clone()))));
        let character_length=character_lengths.get(&name).copied();
        let v = Variable { zero_sized, element_width, character_length, flag: flag.clone(), extent, state: if assigned { State::Yes } else { State::No }, elements: BTreeMap::new(), runtime_sized, heap_length: heap_shapes.contains_key(&format!("@heap:{name}")).then(||crate::allocatables::length(&name,&heap_shapes).unwrap()), heap_allocated:crate::allocatables::allocated(&name,&heap_shapes) };
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
    if let Some(capture) = capture { capture.extend(env.iter().map(|(name,v)|(name.clone(),v.flag.clone()))); }
    prefix.append(body); *body = prefix;
    Ok(())
}
pub fn instrument(mut program: Program) -> Result<Program> {
    program = crate::subroutines::resolve(program)?;
    program = crate::heap::cleanup(program);
    // An earlier failed instrumentation may have left a region open.
    UNCHECKED.with(|c| c.set(0));
    scope(&mut program.body, &mut Env::new(), &[], None, &mut 0, None)?;
    crate::modules::unify_storage(program)
}

/// Instrument a fresh scalar submission and expose its initialization flags.
pub(crate) fn instrument_scalars(mut program: Program) -> Result<(Program, BTreeMap<String,String>)> {
    program = crate::subroutines::resolve(program)?;
    program = crate::heap::cleanup(program);
    UNCHECKED.with(|c| c.set(0));
    let mut flags = BTreeMap::new();
    scope(&mut program.body, &mut Env::new(), &[], None, &mut 0, Some(&mut flags))?;
    Ok((crate::modules::unify_storage(program)?, flags))
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
                Stmt::CallSub {name,args} if name=="@error_stop"
                    && matches!(args.first(),Some(Expr::Str(s)) if s.starts_with("@enumeration_boundary:") || s.starts_with("@enumeration_runtime:")) => {
                    if let Some(Expr::Str(message))=args.first_mut() {
                        *message=message.split_once(':').unwrap().1.to_owned();
                        if let Some(offset)=position {
                            let (text,offset,file)=if let Some(origin)=origins.iter().find(|o|o.range.contains(&offset)) {
                                (origin.source(source),origin.position(offset),origin.name.as_str())
                            }else {(source,offset,filename)};
                            let line=text.as_bytes()[..offset.min(text.len())].iter().filter(|&&b|b==b'\n').count()+1;
                            let file=std::path::Path::new(file).file_name().and_then(|n|n.to_str()).unwrap_or(file);
                            message.push_str(&format!(" at {file}:{line}"));
                        }
                    }
                }
                Stmt::CallSub { name, args } if name == "@check_initialized" => {
                    if let Some(offset) = position {
                        let (source, offset, filename) = if let Some(origin) = origins.iter().find(|o| o.range.contains(&offset)) {
                            let filename = std::path::Path::new(&origin.name).file_name()
                                .and_then(|n| n.to_str()).unwrap_or(&origin.name);
                            (origin.source(source), origin.position(offset), filename)
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
    let mut program = instrument(program).map_err(|error|crate::source_diagnostics::report(error,source,&crate::lexer::lex(source),filename,Some(origins)))?;
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
        let program=crate::parser::parse_with_src(source, &tokens, "initialization.f90").unwrap();
        crate::reductions::resolve(crate::parameters::resolve(program).unwrap()).unwrap()
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

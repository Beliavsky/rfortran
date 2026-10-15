// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Select allocatable and automatic storage backed by runtime allocation.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

// Caller fields can have the same generated spelling as callee local fields.
// Protect their identity during callee alpha-renaming; rename removes this tag.
pub(crate) const BORROWED_FIELD: &str = "@borrowed_field$";
pub(crate) fn borrow_fields(name: &str, shapes: &mut crate::allocatables::Shapes) {
    let mut fields = vec![];
    for key in ["@allocated", "@extent", "@axes", "@lowers"] {
        if let Some((_, values)) = shapes.get_mut(&format!("{key}:{name}")) {
            for value in values {
                if let Expr::Ident(n) = value {
                    let kind = if key == "@allocated" {
                        TypeSpec::Logical
                    } else {
                        TypeSpec::Integer(Some(8))
                    };
                    *n = format!("{BORROWED_FIELD}{n}");
                    fields.push((n.clone(), kind));
                }
            }
        }
    }
    for (n, kind) in fields {
        shapes.insert(n, (kind, vec![]));
    }
}

pub(crate) fn prepare(mut program: Program) -> Result<Program> {
    fn numeric(kind: &TypeSpec) -> bool {
        matches!(
            kind,
            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                | TypeSpec::Real
                | TypeSpec::DoublePrecision
                | TypeSpec::Logical
                | TypeSpec::Character(_)
        )
    }
    fn signatures(body: &mut [Stmt], result: &mut HashMap<String, Vec<usize>>) {
        for statement in body {
            match statement {
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let positions=params.iter().enumerate().filter_map(|(i,p)|
                    (body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,kind,dims} if name==p&&numeric(kind)&&(!matches!(kind,TypeSpec::Character(_))||dims.len()>1)))
                     &&body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p&&intent=="allocatable"))).then_some(i)).collect::<Vec<_>>();
                    for &i in &positions {
                        body.insert(
                            0,
                            Stmt::DummyIntent {
                                name: params[i].clone(),
                                intent: "@heap_dummy".into(),
                            },
                        );
                    }
                    result.insert(name.clone(), positions);
                    signatures(body, result);
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. } => signatures(body, result),
                _ => {}
            }
        }
    }
    fn actuals(
        body: &[Stmt],
        signatures: &HashMap<String, Vec<usize>>,
        wanted: &mut HashSet<String>,
    ) {
        for statement in body {
            match statement {
                Stmt::CallSub { name, args } => {
                    if let Some(positions) = signatures.get(name) {
                        for &i in positions {
                            if let Some(Expr::Ident(n)) = args.get(i) {
                                wanted.insert(n.clone());
                            }
                        }
                    }
                }
                Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                    actuals(body, signatures, wanted)
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    actuals(then_body, signatures, wanted);
                    if let Some(b) = else_body {
                        actuals(b, signatures, wanted);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        actuals(&c.body, signatures, wanted);
                    }
                    if let Some(b) = default {
                        actuals(b, signatures, wanted);
                    }
                }
                _ => {}
            }
        }
    }
    let mut interfaces = HashMap::new();
    signatures(&mut program.body, &mut interfaces);
    fn visit(
        body: &mut Vec<Stmt>,
        params: &[String],
        interfaces: &HashMap<String, Vec<usize>>,
    ) -> Result<()> {
        let mut passed = HashSet::new();
        actuals(body, interfaces, &mut passed);
        fn allocation(body: &[Stmt], array: &str) -> bool {
            body.iter().any(|s|match s {
                Stmt::CallSub{name,args} if name=="@allocate"=>args.iter().any(|e|matches!(e,Expr::Call(n,dims) if n==array) || matches!(e,Expr::Section{name:n,..} if n==array)),
                Stmt::If{then_body,else_body,..}=>allocation(then_body,array)||else_body.as_ref().is_some_and(|b|allocation(b,array)),
                Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>allocation(body,array),_=>false,
            })
        }
        let array_dummies = body
            .iter()
            .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name)));
        let arrays=body.iter().filter_map(|s|if let Stmt::ArrayDecl{kind,name,dims}=s {
            (dims.iter().all(|e|matches!(e,Expr::Ident(n) if n=="@deferred_extent"))
              && !params.contains(name)
              && (passed.contains(name) || name.starts_with("@module$") || ((dims.len()>1 || !array_dummies) && allocation(body,name)) || dims.len()>1 && body.iter().any(|s|matches!(s,Stmt::CallSub{name:call,args} if call=="@deallocate"&&args.iter().any(|e|matches!(e,Expr::Ident(n) if n==name)))))
              && numeric(kind) && (!matches!(kind,TypeSpec::Character(_)) || dims.len()>1)
              && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name&&intent=="allocatable")))
                .then_some((name.clone(),dims.len()))
        } else {None}).collect::<Vec<_>>();
        // Retain definite errors in straight-line local code before converting
        // allocation state to runtime descriptors. Calls/control flow require
        // runtime guards because they can change or condition that state.
        let mut pending:HashSet<_>=arrays.iter().filter(|(n,_)|!n.contains("@module$")).map(|(n,_)|n.clone()).collect();
        let tracked=pending.clone();
        let mut offset=None;
        for s in body.iter() {match s {
            Stmt::SourceLocation(p)=>offset=Some(*p),
            Stmt::CallSub{name,args} if name=="@allocate"=>for e in args {match e{Expr::Call(n,_)|Expr::Section{name:n,..}=>{pending.remove(n);},_=>{}}},
            Stmt::CallSub{name,args} if name=="@deallocate"=>for e in args {if let Expr::Ident(n)=e {if tracked.contains(n){pending.insert(n.clone());}}},
            Stmt::Assign{..}|Stmt::AssignIndex{..}|Stmt::Print{..}|Stmt::Read{..}=>crate::array_calls::check_allocated(s,&pending).map_err(|e|crate::source_diagnostics::at(e,offset))?,
            Stmt::CallSub{..}|Stmt::If{..}|Stmt::Do{..}|Stmt::DoWhile{..}|Stmt::SelectCase{..}|Stmt::Block{..}=>break,
            _=>{},
        }}
        let module = body
            .iter()
            .filter_map(|s| {
                if let Stmt::CallSub { name, args } = s {
                    if name == "@module_bind" {
                        if let Some(Expr::Ident(n)) = args.first() {
                            return Some(n.clone());
                        }
                    }
                    None
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>();
        for (name, rank) in arrays {
            if rank > 15 {
                bail!("allocatable array rank exceeds 15");
            }
            body.insert(
                0,
                Stmt::DummyIntent {
                    name: name.clone(),
                    intent: "@heap".into(),
                },
            );
            if !module.contains(&name) {
                body.push(Stmt::CallSub {
                    name: "@heap_bind".into(),
                    args: vec![Expr::Ident(name)],
                });
            }
        }
        for statement in body.iter_mut() {
            if let Stmt::CallSub { name, args } = statement {
                if name == "@allocate" {
                    fn bound(e: &mut Expr) -> Result<()> {
                        if let Expr::Call(tag, args) = e {
                            if tag == "@section_subscript" {
                                let mut inner = args.remove(0);
                                bound(&mut inner)?;
                                *e = inner;
                                return Ok(());
                            }
                        }
                        if let Expr::Section {
                            lower,
                            upper,
                            stride,
                            ..
                        } = e
                        {
                            if stride.is_some() || lower.is_none() || upper.is_none() {
                                bail!("ALLOCATE bounds require lower:upper without a stride");
                            }
                            *e = Expr::Call(
                                "@heap_bounds".into(),
                                vec![*lower.take().unwrap(), *upper.take().unwrap()],
                            );
                        }
                        Ok(())
                    }
                    for arg in args {
                        if let Expr::Section { name, .. } = arg {
                            let n = name.clone();
                            let mut range = arg.clone();
                            bound(&mut range)?;
                            *arg = Expr::Call(n, vec![range]);
                        }
                        if let Expr::Call(_, dims) = arg {
                            for dim in dims {
                                bound(dim)?;
                            }
                        }
                    }
                }
            }
        }
        crate::allocatables::prepare_matrices(body, &mut HashMap::new(), &HashMap::new())?;
        // Bind local pointers after declarations, before executable statements.
        let mut binds = vec![];
        body.retain(|s| {
            if matches!(s,Stmt::CallSub{name,..} if name=="@heap_bind") {
                binds.push(s.clone());
                false
            } else {
                true
            }
        });
        // Generated expression results can be declared after executable
        // statements. Bind each descriptor at its own declaration, before any
        // allocation or hidden-result call can use it.
        for bind in binds {
            let Stmt::CallSub{args,..}=&bind else{unreachable!()};
            let Expr::Ident(name)=&args[0] else{unreachable!()};
            let index=body.iter().position(|s|matches!(s,Stmt::ArrayDecl{name:n,..} if n==name)).map_or(0,|i|i+1);
            body.insert(index,bind);
        }
        for s in body.iter_mut() {
            match s {
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    visit(body, params, interfaces)?
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => visit(body, &[], interfaces)?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    visit(then_body, &[], interfaces)?;
                    if let Some(b) = else_body {
                        visit(b, &[], interfaces)?;
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        visit(&mut c.body, &[], interfaces)?;
                    }
                    if let Some(b) = default {
                        visit(b, &[], interfaces)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    visit(&mut program.body, &[], &interfaces)?;
    Ok(program)
}

/// Allocate source locals after call-site bounds have been specialized.
pub(crate) fn automatic(body:&mut Vec<Stmt>,shapes:&mut crate::allocatables::Shapes)->Result<()> {
    let dummies=body.iter().filter_map(|s|match s {Stmt::DummyIntent{name,intent} if intent.starts_with("intent(")=>Some(name.clone()),_=>None}).collect::<HashSet<_>>();
    let mut out=vec![];
    for stmt in std::mem::take(body) {
        if let Stmt::ArrayDecl{kind,name,dims}=&stmt {
            // Generated result storage has its own allocation policy. Source
            // locals reach here only after their dummy bounds are specialized.
            if !name.starts_with('@') && !dummies.contains(name) && !dims.is_empty()
                && !dims.iter().any(|d|matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
                && dims.iter().any(|d|crate::vector_subscripts::fixed_extent(d).is_none()) {
                let bounds=dims.iter().map(|d|match d {Expr::Call(tag,a) if tag=="@declared_bounds"=>Expr::Call("@heap_bounds".into(),a.clone()),_=>d.clone()}).collect();
                unallocated_result(kind.clone(),name.clone(),dims.len(),&mut out,shapes)?;
                out.push(Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(name.clone(),bounds)]});
                continue;
            }
        }
        out.push(stmt);
    }
    *body=out;
    Ok(())
}

/// Caller-owned result storage, allocated by a function's hidden output dummy.
pub(crate) fn unallocated_result(kind: TypeSpec, name: String, rank: usize,
    body: &mut Vec<Stmt>, shapes: &mut crate::allocatables::Shapes) -> Result<()> {
    if rank==0 || rank>15 {bail!("function result array rank must be between 1 and 15");}
    let mut declarations=vec![
        Stmt::ArrayDecl{kind,name:name.clone(),dims:vec![Expr::Ident("@deferred_extent".into());rank]},
        Stmt::DummyIntent{name:name.clone(),intent:"allocatable".into()},
        Stmt::DummyIntent{name:name.clone(),intent:"@heap".into()},
        Stmt::CallSub{name:"@heap_bind".into(),args:vec![Expr::Ident(name)]},
    ];
    crate::allocatables::prepare_matrices(&mut declarations,shapes,&HashMap::new())?;
    body.extend(declarations);
    Ok(())
}

/// Materialize a numeric expression using its active runtime extent.
pub(crate) fn temporary(kind: TypeSpec, name: String, length: Expr, body: &mut Vec<Stmt>) {
    let len = format!("{name}$length");
    let flag = format!("{name}$allocated");
    body.extend([
        Stmt::ArrayDecl {
            kind,
            name: name.clone(),
            dims: vec![Expr::IntLit("1".into())],
        },
        Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![len.clone()],
        },
        Stmt::VarDecl {
            kind: TypeSpec::Logical,
            names: vec![flag.clone()],
        },
        Stmt::Assign {
            name: flag.clone(),
            value: Expr::Logical(true),
        },
        Stmt::DummyIntent {
            name: name.clone(),
            intent: "@heap".into(),
        },
        Stmt::DummyIntent {
            name: name.clone(),
            intent: format!("@bounded:{len}:{flag}"),
        },
        Stmt::CallSub {
            name: "@heap_bind".into(),
            args: vec![Expr::Ident(name.clone())],
        },
        Stmt::CallSub {
            name: "@heap_allocate".into(),
            args: vec![Expr::Ident(name), Expr::Ident(len), length],
        },
    ]);
}

/// Release procedure/BLOCK locals and expression buffers at scope exit.
pub(crate) fn cleanup(mut program: Program) -> Program {
    fn drop(name: &String) -> Stmt {
        Stmt::CallSub {
            name: "@heap_drop".into(),
            args: vec![Expr::Ident(name.clone())],
        }
    }
    fn body(statements: &mut Vec<Stmt>, inherited: &[String], loop_boundary: usize) {
        let mut live = inherited.to_vec();
        let mut out = vec![];
        for mut statement in std::mem::take(statements) {
            match &mut statement {
                Stmt::CallSub { name, args } if name == "@heap_bind" => {
                    if let Some(Expr::Ident(n)) = args.first() {
                        live.push(n.clone());
                    }
                }
                Stmt::Return(_) => out.extend(live.iter().rev().map(drop)),
                Stmt::Exit | Stmt::Cycle => {
                    out.extend(live[loop_boundary..].iter().rev().map(drop))
                }
                Stmt::Function { body: b, .. }
                | Stmt::Subroutine { body: b, .. }
                | Stmt::Module { body: b, .. } => body(b, &[], 0),
                Stmt::Block { body: b } => body(b, &live, loop_boundary),
                Stmt::Do { body: b, .. } | Stmt::DoWhile { body: b, .. } => {
                    body(b, &live, live.len())
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    body(then_body, &live, loop_boundary);
                    if let Some(b) = else_body {
                        body(b, &live, loop_boundary);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        body(&mut c.body, &live, loop_boundary);
                    }
                    if let Some(b) = default {
                        body(b, &live, loop_boundary);
                    }
                }
                _ => {}
            }
            out.push(statement);
        }
        out.extend(live[inherited.len()..].iter().rev().map(drop));
        *statements = out;
    }
    body(&mut program.body, &[], 0);
    program
}

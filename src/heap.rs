// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Select numeric allocatable storage backed by runtime allocation.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

pub(crate) fn prepare(mut program: Program) -> Result<Program> {
    fn visit(body: &mut Vec<Stmt>, params: &[String]) -> Result<()> {
        fn allocation(body: &[Stmt], array: &str) -> bool {
            body.iter().any(|s|match s {
                Stmt::CallSub{name,args} if name=="@allocate"=>args.iter().any(|e|matches!(e,Expr::Call(n,dims) if n==array&&dims.iter().any(|e|crate::parameters::integer_constant(e).is_err())) || matches!(e,Expr::Section{name:n,..} if n==array)),
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
              && (name.starts_with("@module$") || ((dims.len()>1 || !array_dummies) && allocation(body,name)) || dims.len()>1 && body.iter().any(|s|matches!(s,Stmt::CallSub{name:call,args} if call=="@deallocate"&&args.iter().any(|e|matches!(e,Expr::Ident(n) if n==name)))))
              && matches!(kind,TypeSpec::Integer(None|Some(1|2|4|8))|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical)
              && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name&&intent=="allocatable")))
                .then_some((name.clone(),dims.len()))
        } else {None}).collect::<Vec<_>>();
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
            if let Stmt::CallSub{name,args}=statement {if name=="@allocate" {
                fn bound(e:&mut Expr)->Result<()> {
                    if let Expr::Call(tag,args)=e {if tag=="@section_subscript" {let mut inner=args.remove(0);bound(&mut inner)?;*e=inner;return Ok(());}}
                    if let Expr::Section{lower,upper,stride,..}=e {
                        if stride.is_some() || lower.is_none() || upper.is_none(){bail!("ALLOCATE bounds require lower:upper without a stride");}
                        *e=Expr::Call("@heap_bounds".into(),vec![*lower.take().unwrap(),*upper.take().unwrap()]);
                    }
                    Ok(())
                }
                for arg in args {
                    if let Expr::Section{name,..}=arg {let n=name.clone();let mut range=arg.clone();bound(&mut range)?;*arg=Expr::Call(n,vec![range]);}
                    if let Expr::Call(_,dims)=arg {for dim in dims{bound(dim)?;}}
                }
            }}
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
        let index = body
            .iter()
            .rposition(|s| matches!(s, Stmt::ArrayDecl { .. }))
            .map_or(0, |i| i + 1);
        body.splice(index..index, binds);
        for s in body.iter_mut() {
            match s {
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    visit(body, params)?
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => visit(body, &[])?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    visit(then_body, &[])?;
                    if let Some(b) = else_body {
                        visit(b, &[])?;
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        visit(&mut c.body, &[])?;
                    }
                    if let Some(b) = default {
                        visit(b, &[])?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    visit(&mut program.body, &[])?;
    Ok(program)
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

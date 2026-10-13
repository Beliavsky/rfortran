// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Expand locally defined scalar output-argument subroutines into scoped calls.
//! The existing object ABI passes values; expansion preserves writable argument storage.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone)]
struct Routine { params: Vec<String>, body: Vec<Stmt> }
type Names = HashMap<String, String>;
fn name(n: &mut String, names: &Names) { if let Some(new) = names.get(n) { *n = new.clone(); } }
fn expr(e: &mut Expr, names: &Names) {
    match e {
        Expr::Ident(n) => name(n, names),
        Expr::Index(n, args) | Expr::Call(n, args) => { name(n, names); for a in args { expr(a, names); } }
        Expr::Bin(_, a, b) => { expr(a, names); expr(b, names); }
        Expr::Un(_, a) => expr(a, names),
        Expr::Section { name: n, lower, upper, stride } => {
            name(n, names); for e in [lower, upper, stride].into_iter().flatten() { expr(e, names); }
        }
        _ => {}
    }
}
pub(crate) fn rename(
    body: &mut Vec<Stmt>,
    inherited: &Names,
    params: &[String],
    serial: &mut usize,
) -> Result<()> {
    rename_with_prefix(body, inherited, params, serial, "@sub_local")
}
pub(crate) fn rename_allocation_block(body: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    rename_with_prefix(body, &Names::new(), &[], serial, "@alloc_block")
}
pub(crate) fn rename_with_prefix(
    body: &mut Vec<Stmt>,
    inherited: &Names,
    params: &[String],
    serial: &mut usize,
    prefix: &str,
) -> Result<()> {
    let mut names = inherited.clone();
    for s in body.iter() {
        let declared = match s {
            Stmt::VarDecl { names, .. } => names.clone(),
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            _ => vec![],
        };
        for n in declared {
            if !params.contains(&n) {
                *serial += 1;
                names.insert(n, format!("{prefix}_{serial}"));
            }
        }
    }
    for s in body.iter_mut() {
        match s {
            Stmt::DummyIntent { name: n, intent } if intent.starts_with("@bounded_shape:") => {
                let rest = intent.trim_start_matches("@bounded_shape:");
                let axes = rest
                    .split(':')
                    .map(|n| {
                        let mut n = n.to_owned();
                        name(&mut n, &names);
                        n
                    })
                    .collect::<Vec<_>>();
                name(n, &names);
                *intent = format!("@bounded_shape:{}", axes.join(":"));
            }
            Stmt::DummyIntent { name: n, intent } if intent.starts_with("@bounded_limits:") => {
                name(n, &names)
            }
            Stmt::DummyIntent { name: n, intent } if intent.starts_with("@bounded:") => {
                let rest = intent.trim_start_matches("@bounded:");
                if let Some((length, flag)) = rest.split_once(':') {
                    let mut length = length.to_owned();
                    let mut flag = flag.to_owned();
                    name(&mut length, &names);
                    name(&mut flag, &names);
                    name(n, &names);
                    *intent = format!("@bounded:{length}:{flag}");
                }
            }
            Stmt::DummyIntent { name: n, intent } if prefix == "@alloc_block" || intent=="@module_host" || (matches!(intent.as_str(),"pointer"|"target"|"@pointer_readonly_target") || intent.starts_with("@pointer_bounds:")) => name(n, &names),
            Stmt::VarDecl {
                names: declared, ..
            } => {
                declared.retain(|n| !params.contains(n));
                for n in declared {
                    name(n, &names);
                }
            }
            Stmt::ArrayDecl { name: n, dims, .. } => {
                if !params.contains(n) {
                    name(n, &names);
                    for e in dims {
                        expr(e, &names);
                    }
                }
            }
            Stmt::Assign { name: n, value } => {
                name(n, &names);
                expr(value, &names);
            }
            Stmt::AssignIndex {
                name: n,
                indices,
                value,
            } => {
                name(n, &names);
                for e in indices {
                    expr(e, &names);
                }
                expr(value, &names);
            }
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expr(e, &names);
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, &names);
                rename_with_prefix(then_body, &names, params, serial, prefix)?;
                if let Some(body) = else_body {
                    rename_with_prefix(body, &names, params, serial, prefix)?;
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                name(var, &names);
                expr(start, &names);
                expr(end, &names);
                rename_with_prefix(body, &names, params, serial, prefix)?;
            }
            Stmt::DoWhile { cond, body } => {
                expr(cond, &names);
                rename_with_prefix(body, &names, params, serial, prefix)?;
            }
            Stmt::Block { body } => rename_with_prefix(body, &names, &[], serial, prefix)?,
            Stmt::SelectCase {
                expr: e,
                cases,
                default,
            } => {
                expr(e, &names);
                for case in cases {
                    rename_with_prefix(&mut case.body, &names, params, serial, prefix)?;
                }
                if let Some(body) = default {
                    rename_with_prefix(body, &names, params, serial, prefix)?;
                }
            }
            Stmt::Return(Some(value)) => expr(value, &names),
            _ => {}
        }
    }
    body.retain(|s| !matches!(s, Stmt::ArrayDecl { name, .. } if params.contains(name)) && !matches!(s, Stmt::DummyIntent { intent,.. } if prefix!="@alloc_block" && !(matches!(intent.as_str(),"pointer"|"target"|"@pointer_readonly_target") || intent.starts_with("@pointer_bounds:")) && !intent.starts_with("@bounded:") && !intent.starts_with("@bounded_shape:") && !intent.starts_with("@bounded_limits:")) && !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
    Ok(())
}
/// Preserve procedure RETURN when its body is expanded into the caller.
/// A flag skips the remainder of the call and exits enclosing loops, without
/// changing ordinary EXIT/CYCLE or returning from the caller itself.
pub(crate) fn inline_returns(body: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    inline_returns_with_prefix(body, serial, "@sub_returned")
}
pub(crate) fn inline_returns_with_prefix(body: &mut Vec<Stmt>, serial: &mut usize, prefix: &str) -> Result<()> {
    fn contains(body: &[Stmt]) -> bool { body.iter().any(|s| match s {
        Stmt::Return(_) => true,
        Stmt::If {then_body,else_body,..} => contains(then_body) || else_body.as_ref().is_some_and(|b| contains(b)),
        Stmt::Do {body,..} | Stmt::DoWhile {body,..} | Stmt::Block {body} => contains(body),
        Stmt::SelectCase {cases,default,..} => cases.iter().any(|c| contains(&c.body)) || default.as_ref().is_some_and(|b|contains(b)),
        _ => false,
    }) }
    if !contains(body) { return Ok(()); }
    *serial += 1;
    let flag = format!("{prefix}_{serial}");
    fn lower(body: &mut Vec<Stmt>, flag: &str) -> Result<()> {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::Return(Some(_)) => bail!("a subroutine RETURN cannot have a value"),
                Stmt::Return(None) => s = Stmt::Assign {name:flag.into(),value:Expr::Logical(true)},
                Stmt::If {then_body,else_body,..} => { lower(then_body,flag)?; if let Some(b)=else_body {lower(b,flag)?;} }
                Stmt::Do {body,..} | Stmt::DoWhile {body,..} => {
                    lower(body,flag)?;
                    body.push(Stmt::If {cond:Expr::Ident(flag.into()),then_body:vec![Stmt::Exit],else_body:None});
                }
                Stmt::Block {body} => lower(body,flag)?,
                Stmt::SelectCase {cases,default,..} => { for c in cases {lower(&mut c.body,flag)?;} if let Some(b)=default {lower(b,flag)?;} }
                _ => {}
            }
            if matches!(s,Stmt::VarDecl {..} | Stmt::ArrayDecl {..} | Stmt::SourceLocation(_) | Stmt::Implicit(_) | Stmt::DummyIntent {..}) { out.push(s); }
            else { out.push(Stmt::If {cond:Expr::Un(crate::ast::UnOp::Not,Box::new(Expr::Ident(flag.into()))),then_body:vec![s],else_body:None}); }
        }
        *body=out; Ok(())
    }
    lower(body,&flag)?;
    body.insert(0,Stmt::Assign {name:flag.clone(),value:Expr::Logical(false)});
    body.insert(0,Stmt::VarDecl {kind:TypeSpec::Logical,names:vec![flag]});
    Ok(())
}
fn expand(body: &mut Vec<Stmt>, routines: &HashMap<String, Routine>, active: &mut Vec<String>, serial: &mut usize) -> Result<()> {
    for stmt in body {
        if let Stmt::CallSub { name: callee, args } = stmt {
            if let Some(routine) = routines.get(callee) {
                let intents: HashMap<_, _> = routine.body.iter().filter_map(|s| match s {
                    Stmt::DummyIntent { name, intent } => Some((name.clone(), intent.clone())), _ => None,
                }).collect();
                if intents.values().any(|i| i == "intent(out)" || i == "intent(inout)") {
                    if active.contains(callee) { bail!("recursive output-argument subroutine `{callee}` is not yet supported"); }
                    if args.len() != routine.params.len() { bail!("wrong argument count for subroutine `{callee}`"); }
                    let types: HashMap<String, TypeSpec> = routine.body.iter().flat_map(|s| match s {
                        Stmt::VarDecl { names, kind } => names.iter().map(|n| (n.clone(), kind.clone())).collect(),
                        _ => Vec::new(),
                    }).collect();
                    let mut prefix = Vec::new();
                    let mut undefine = Vec::new();
                    let mut names = Names::new();
                    for (param, actual) in routine.params.iter().zip(args.iter()) {
                        let kind = types.get(param).with_context(|| format!("output-argument calls currently require explicitly declared scalar dummy `{param}`"))?;
                        let intent = intents.get(param).map(String::as_str).unwrap_or("");
                        if intent == "intent(in)" {
                            *serial += 1;
                            let temp = format!("@sub_input_{serial}");
                            prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![temp.clone()] });
                            prefix.push(Stmt::Assign { name: temp.clone(), value: actual.clone() });
                            names.insert(param.clone(), temp);
                        } else {
                            let Expr::Ident(actual) = actual else { bail!("writable argument `{param}` of `{callee}` must currently be a scalar variable"); };
                            names.insert(param.clone(), actual.clone());
                            if intent == "intent(out)" {
                                undefine.push(Stmt::CallSub { name: "@undefine".into(), args: vec![Expr::Ident(actual.clone())] });
                            }
                        }
                    }
                    let mut expanded = routine.body.clone();
                    rename(&mut expanded, &names, &routine.params, serial)?;
                    active.push(callee.clone());
                    expand(&mut expanded, routines, active, serial)?;
                    active.pop();
                    inline_returns(&mut expanded, serial)?;
                    prefix.extend(undefine); prefix.extend(expanded);
                    *stmt = Stmt::Block { body: prefix };
                    continue;
                }
            }
        }
        match stmt {
            Stmt::If { then_body, else_body, .. } => {
                expand(then_body, routines, active, serial)?;
                if let Some(body) = else_body { expand(body, routines, active, serial)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body }
            | Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => expand(body, routines, active, serial)?,
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { expand(&mut case.body, routines, active, serial)?; }
                if let Some(body) = default { expand(body, routines, active, serial)?; }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn resolve(mut program: Program) -> Result<Program> {
    fn collect(body: &[Stmt], routines: &mut HashMap<String, Routine>) {
        for s in body {
            match s {
                Stmt::Subroutine { name, params, body, .. } => { routines.insert(name.clone(), Routine { params: params.clone(), body: body.clone() }); }
                Stmt::Module { body, .. } => collect(body, routines),
                _ => {}
            }
        }
    }
    let mut routines = HashMap::new(); collect(&program.body, &mut routines);
    expand(&mut program.body, &routines, &mut Vec::new(), &mut 0)?;
    Ok(program)
}

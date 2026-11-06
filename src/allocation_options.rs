// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower MOVE_ALLOC and the SOURCE=, MOLD=, STAT= and ERRMSG= options of
//! ALLOCATE and DEALLOCATE to plain allocation statements.
//!
//! MOVE_ALLOC copies the data and deallocates the source, which is equivalent
//! except for pointers associated with the moved array. STAT= reports the
//! errors the language defines for an allocation status (allocating an
//! allocated array, deallocating an unallocated one) instead of stopping.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// Variable ranks. A POINTER `p` also has the key `@pointer:p`.
type Ranks = HashMap<String, usize>;
type Kinds = HashMap<String, TypeSpec>;

fn pointer(ranks: &Ranks, n: &str) -> bool {
    ranks.contains_key(&format!("@pointer:{n}"))
}

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn call(n: &str, args: Vec<Expr>) -> Stmt {
    Stmt::CallSub { name: n.into(), args }
}
fn test(f: &str, n: &str) -> Expr {
    Expr::Call(f.into(), vec![id(n)])
}
fn when(cond: Expr, then_body: Vec<Stmt>, else_body: Option<Vec<Stmt>>) -> Stmt {
    Stmt::If { cond, then_body, else_body }
}
fn any(conditions: Vec<Expr>) -> Expr {
    conditions.into_iter().reduce(|a, b| Expr::Bin(BinOp::Or, Box::new(a), Box::new(b))).unwrap_or(Expr::Logical(false))
}

/// ALLOCATE `to` with the bounds of the array `from`.
fn allocate_like(to: &str, from: &str, rank: usize) -> Stmt {
    let bound = |f: &str, d: usize| Some(Box::new(Expr::Call(f.into(), vec![id(from), Expr::IntLit(d.to_string())])));
    let subscripts = (1..=rank)
        .map(|d| {
            Expr::Call("@section_subscript".into(), vec![Expr::Section {
                name: to.into(),
                lower: bound("lbound", d),
                upper: bound("ubound", d),
                stride: None,
            }])
        })
        .collect();
    call("@allocate", vec![Expr::Call(to.into(), subscripts)])
}

fn keyword(e: &Expr) -> Option<(&str, &Expr)> {
    match e {
        Expr::Call(k, v) if v.len() == 1 => k.strip_prefix("@keyword:").map(|k| (k, &v[0])),
        _ => None,
    }
}

fn status(stat: Option<&Expr>, errmsg: Option<&Expr>, code: i64, message: &str) -> Result<Vec<Stmt>> {
    let mut out = vec![];
    if let Some(stat) = stat {
        let Expr::Ident(s) = stat else { bail!("STAT= requires a scalar INTEGER variable") };
        out.push(Stmt::Assign { name: s.clone(), value: Expr::IntLit(code.to_string()) });
    }
    if code != 0 {
        if let Some(errmsg) = errmsg {
            let Expr::Ident(m) = errmsg else { bail!("ERRMSG= requires a scalar CHARACTER variable") };
            out.push(Stmt::Assign { name: m.clone(), value: Expr::Str(message.into()) });
        }
    }
    Ok(out)
}

pub(crate) fn allocate(args: &[Expr], ranks: &Ranks) -> Result<Option<Vec<Stmt>>> {
    allocate_typed(args,ranks,None)
}
fn allocate_typed(args: &[Expr], ranks: &Ranks, kinds:Option<&Kinds>) -> Result<Option<Vec<Stmt>>> {
    let mut objects = vec![];
    let (mut stat, mut errmsg, mut source, mut mold) = (None, None, None, None);
    let mut controls=std::collections::HashSet::new();
    for a in args {
        if let Some((k,_))=keyword(a) {if !controls.insert(k) {bail!("duplicate ALLOCATE option `{k}=`");}}
        match keyword(a) {
            Some(("stat", v)) => stat = Some(v),
            Some(("errmsg", v)) => errmsg = Some(v),
            Some(("source", v)) => source = Some(v),
            Some(("mold", v)) => mold = Some(v),
            Some((k, _)) => bail!("unsupported ALLOCATE option `{k}=`"),
            None => objects.push(a.clone()),
        }
    }
    if source.is_some() && mold.is_some() {bail!("SOURCE= and MOLD= are mutually exclusive");}
    if stat.is_none() && errmsg.is_none() && source.is_none() && mold.is_none() {
        return Ok(None);
    }
    let mut allocations = vec![];
    // Allocation can change inquiries of a target used by SOURCE. Capture
    // these scalar values before allocating, rather than in the later copy.
    let mut source_value=source.map(|e| {
        let mut value=e.clone();
        crate::procedure_arguments::visit_expr(&mut value,&mut |e| {
            if matches!(e,Expr::Call(n,_) if matches!(n.as_str(),"allocated"|"associated")
                && !ranks.contains_key(n) && !ranks.contains_key(&format!("@procedure:{n}"))) {
                static SERIAL:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
                let temp=format!("@allocation_source_state_{}",SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
                allocations.push(Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![temp.clone()]});
                allocations.push(Stmt::Assign{name:temp.clone(),value:e.clone()});
                *e=id(&temp);
            }
        });
        value
    });
    // Materialize an array SOURCE expression before asking for its shape.
    // In particular, SIZE(function(...)) may not yet have a bound interface.
    // The ordinary array-assignment path binds the function result first.
    if let (Some(value),Some(Expr::Ident(target)),Some(kinds))=(source_value.as_ref(),objects.first(),kinds) {
        if !matches!(value,Expr::Ident(_)) && ranks.get(target).is_some_and(|r|*r>0) {
            if let Some(kind)=kinds.get(target).filter(|k|matches!(k,TypeSpec::Integer(_)|TypeSpec::IntegerKind(_)|TypeSpec::Real|TypeSpec::RealKind(_)|TypeSpec::DoublePrecision|TypeSpec::Logical|TypeSpec::LogicalKind(_))) {
                static SERIAL:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
                let temp=format!("@allocation_source_array_{}",SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
                allocations.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![id("@deferred_extent");ranks[target]]});
                allocations.push(Stmt::DummyIntent{name:temp.clone(),intent:"allocatable".into()});
                allocations.push(Stmt::Assign{name:temp.clone(),value:value.clone()});
                source_value=Some(id(&temp));
            }
        }
    }
    let shape = source_value.as_ref().or(mold);
    let mut names = vec![];
    for object in &objects {
        let name = match object {
            Expr::Ident(n) | Expr::Call(n, _) | Expr::Section { name: n, .. } => n.clone(),
            _ => bail!("ALLOCATE requires ALLOCATABLE variables"),
        };
        match (object, shape) {
            // Bounds come from an array SOURCE= or MOLD=.
            (Expr::Ident(n), Some(Expr::Ident(from))) if ranks.get(from).is_some_and(|r| *r > 0) => {
                allocations.push(allocate_like(n, from, ranks[from]));
            }
            // An array expression gives the extents, with lower bounds 1.
            (Expr::Ident(n), Some(value)) if ranks.get(n).is_some_and(|r| *r > 0) => {
                let extents = (1..=ranks[n]).map(|d| Expr::Call("size".into(), vec![value.clone(), Expr::IntLit(d.to_string())])).collect();
                allocations.push(call("@allocate", vec![Expr::Call(n.clone(), extents)]));
            }
            (Expr::Ident(n), _) if ranks.get(n).is_some_and(|r| *r > 0) => {
                bail!("ALLOCATE of array `{n}` requires bounds or an array variable as SOURCE= or MOLD=")
            }
            _ => allocations.push(call("@allocate", vec![object.clone()])),
        }
        if let Some(value) = source_value.as_ref() {
            // SOURCE initializes the allocation; it must not reallocate the
            // target or replace explicit bounds as intrinsic assignment could.
            if let Some(&rank)=ranks.get(&name).filter(|r|**r>0) {
                let indices=(0..rank).map(|_|Expr::Call("@section_subscript".into(),vec![Expr::Section{
                    name:name.clone(),lower:None,upper:None,stride:None
                }])).collect();
                allocations.push(Stmt::AssignIndex{name:name.clone(),indices,value:value.clone()});
            } else {allocations.push(Stmt::Assign { name: name.clone(), value: value.clone() });}
        }
        names.push(name);
    }
    if stat.is_none() {
        return Ok(Some(allocations));
    }
    allocations.extend(status(stat, errmsg, 0, "")?);
    // Allocating an associated POINTER is not an error.
    let arrays: Vec<Expr> = names.iter().filter(|n| ranks.get(*n).is_some_and(|r| *r > 0) && !pointer(ranks, n)).map(|n| test("allocated", n)).collect();
    if arrays.is_empty() {
        return Ok(Some(allocations));
    }
    Ok(Some(vec![when(any(arrays), status(stat, errmsg, 1, "array is already allocated")?, Some(allocations))]))
}

fn deallocate(args: &[Expr], ranks: &Ranks) -> Result<Option<Vec<Stmt>>> {
    let mut objects = vec![];
    let (mut stat, mut errmsg) = (None, None);
    let mut controls=std::collections::HashSet::new();
    for a in args {
        if let Some((k,_))=keyword(a) {if !controls.insert(k) {bail!("duplicate DEALLOCATE option `{k}=`");}}
        match keyword(a) {
            Some(("stat", v)) => stat = Some(v),
            Some(("errmsg", v)) => errmsg = Some(v),
            Some((k, _)) => bail!("unsupported DEALLOCATE option `{k}=`"),
            None => objects.push(a.clone()),
        }
    }
    if stat.is_none() && errmsg.is_none() {
        return Ok(None);
    }
    let mut body = vec![call("@deallocate", objects.clone())];
    if stat.is_none() {
        return Ok(Some(body));
    }
    body.extend(status(stat, errmsg, 0, "")?);
    let unallocated: Vec<Expr> = objects
        .iter()
        .filter_map(|o| match o {
            Expr::Ident(n) if pointer(ranks, n) => Some(Expr::Un(UnOp::Not, Box::new(test("associated", n)))),
            Expr::Ident(n) if ranks.get(n).is_some_and(|r| *r > 0) => {
                Some(Expr::Un(UnOp::Not, Box::new(test("allocated", n))))
            }
            _ => None,
        })
        .collect();
    if unallocated.is_empty() {
        return Ok(Some(body));
    }
    Ok(Some(vec![when(any(unallocated), status(stat, errmsg, 1, "array is not allocated")?, Some(body))]))
}

pub(crate) fn move_alloc(args: &[Expr], ranks: &Ranks) -> Result<Vec<Stmt>> {
    let mut from = None;
    let mut to = None;
    for (i, a) in args.iter().enumerate() {
        match (keyword(a), i) {
            (Some(("from", v)), _) => from = Some(v.clone()),
            (Some(("to", v)), _) => to = Some(v.clone()),
            (Some((k, _)), _) => bail!("unsupported MOVE_ALLOC argument `{k}=`"),
            (None, 0) => from = Some(a.clone()),
            (None, 1) => to = Some(a.clone()),
            _ => bail!("MOVE_ALLOC takes FROM and TO"),
        }
    }
    let (Some(Expr::Ident(from)), Some(Expr::Ident(to))) = (from, to) else {
        bail!("MOVE_ALLOC requires ALLOCATABLE variables FROM and TO")
    };
    let Some(&rank) = ranks.get(&from).filter(|r| **r > 0) else {
        bail!("MOVE_ALLOC currently requires ALLOCATABLE arrays")
    };
    if ranks.get(&to) != Some(&rank) {
        bail!("MOVE_ALLOC arguments `{from}` and `{to}` must have the same rank");
    }
    Ok(vec![
        when(test("allocated", &to), vec![call("@deallocate", vec![id(&to)])], None),
        when(
            test("allocated", &from),
            vec![
                allocate_like(&to, &from, rank),
                // The move also moves elements that were never set, so the
                // copy is not checked for reads of unset elements.
                call(crate::initialized::UNCHECKED_BEGIN, vec![]),
                Stmt::Assign { name: to.clone(), value: id(&from) },
                call(crate::initialized::UNCHECKED_END, vec![]),
                call(crate::initialized::COPY_INITIALIZATION, vec![id(&to),id(&from)]),
                call("@deallocate", vec![id(&from)]),
            ],
            None,
        ),
    ])
}

fn lower(body: &mut Vec<Stmt>, inherited: &Ranks, inherited_kinds:&Kinds) -> Result<()> {
    let mut ranks = inherited.clone();
    let mut kinds=inherited_kinds.clone();
    for s in body.iter() {
        match s {
            Stmt::Function{name,..} => {ranks.insert(format!("@procedure:{name}"),0);}
            Stmt::DummyIntent{name,intent} if intent=="external" || intent.starts_with("procedure") => {ranks.insert(format!("@procedure:{name}"),0);}
            Stmt::ArrayDecl { name, dims, kind } => {
                ranks.insert(name.clone(), dims.len());
                kinds.insert(name.clone(),kind.clone());
            }
            Stmt::VarDecl { names, kind } => {
                for n in names {
                    ranks.insert(n.clone(), 0);
                    kinds.insert(n.clone(),kind.clone());
                }
            }
            _ => {}
        }
    }
    // A local declaration hides a host POINTER of the same name.
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, .. } => {
                ranks.remove(&format!("@pointer:{name}"));
            }
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    ranks.remove(&format!("@pointer:{n}"));
                }
            }
            _ => {}
        }
    }
    for s in body.iter() {
        if let Stmt::DummyIntent { name, intent } = s {
            if intent == "pointer" {
                ranks.insert(format!("@pointer:{name}"), 0);
            }
        }
    }
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        let replacement = match &s {
            Stmt::CallSub { name, args } if name == "@allocate" => allocate_typed(args, &ranks,Some(&kinds))?,
            Stmt::CallSub { name, args } if name == "@deallocate" => deallocate(args, &ranks)?,
            Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => Some(move_alloc(args, &ranks)?),
            _ => None,
        };
        if let Some(statements) = replacement {
            out.extend(statements);
            continue;
        }
        match &mut s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => lower(body, &ranks,&kinds)?,
            Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => lower(body, &ranks,&kinds)?,
            Stmt::If { then_body, else_body, .. } => {
                lower(then_body, &ranks,&kinds)?;
                if let Some(b) = else_body {
                    lower(b, &ranks,&kinds)?;
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    lower(&mut c.body, &ranks,&kinds)?;
                }
                if let Some(b) = default {
                    lower(b, &ranks,&kinds)?;
                }
            }
            _ => {}
        }
        out.push(s);
    }
    *body = out;
    Ok(())
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    lower(&mut program.body, &Ranks::new(),&Kinds::new())?;
    Ok(program)
}

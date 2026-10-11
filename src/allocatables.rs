// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Bounded rank-one allocatable outputs for specialized local procedures.
//! Storage is sized from call-site array inputs or fixed local arrays; allocation state and extent
//! remain runtime values. An allocation exceeding that bound is diagnosed.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
pub(crate) type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, e: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: e,
    }
}
fn fail(cond: Expr, message: &str) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message.into())],
        }],
        else_body: None,
    }
}
pub(crate) fn metadata(body: &[Stmt], shapes: &mut Shapes) {
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(rest) = intent.strip_prefix("@bounded:") {
                if let Some((length, flag)) = rest.split_once(':') {
                    shapes.insert(
                        format!("@extent:{name}"),
                        (TypeSpec::Integer(None), vec![id(length)]),
                    );
                    shapes.insert(
                        format!("@allocated:{name}"),
                        (TypeSpec::Logical, vec![id(flag)]),
                    );
                }
            }
        }
    }
}
pub(crate) fn length(name: &str, shapes: &Shapes) -> Option<Expr> {
    shapes
        .get(&format!("@extent:{name}"))
        .map(|v| v.1[0].clone())
}
pub(crate) fn allocated(name: &str, shapes: &Shapes) -> Option<Expr> {
    shapes
        .get(&format!("@allocated:{name}"))
        .map(|v| v.1[0].clone())
}
pub(crate) fn view(name: &str, shapes: &Shapes) -> Expr {
    length(name, shapes)
        .map(|n| Expr::Call("@packed_view".into(), vec![id(name), n]))
        .unwrap_or(id(name))
}
fn capacity(e: &Expr, arrays: &HashMap<String, i64>) -> Option<i64> {
    match e {
        Expr::Ident(n) => arrays.get(n).copied(),
        Expr::Call(n, a) if n == "@array_constructor" => a.iter().try_fold(0i64, |sum, e| {
            sum.checked_add(capacity(e, arrays).unwrap_or(1))
        }),
        Expr::Call(n, a) if matches!(n.as_str(), "pack" | "@pack") => a
            .get(2)
            .and_then(|e| capacity(e, arrays))
            .or_else(|| capacity(&a[0], arrays)),
        Expr::Bin(_, a, b) => capacity(a, arrays).or_else(|| capacity(b, arrays)),
        Expr::Un(_, a) => capacity(a, arrays),
        // The backing array is a safe bound even when a section is empty or
        // its declared lower bound differs from one. Logical sizes are lowered
        // separately after explicit-bound normalization.
        Expr::Section { name, .. } => arrays.get(name).copied(),
        _ => None,
    }
}
// Capture the active elements before writing, including when the arrays overlap.
fn character_copy(name: &str, value: &Expr, len: Expr, shapes: &Shapes,
                  serial: &mut usize, out: &mut Vec<Stmt>) -> Result<()> {
    *serial += 1;
    let index = format!("@alloc_char_index_{serial}");
    let temp = format!("@alloc_char_copy_{serial}");
    let (kind, dims) = shapes[name].clone();
    let (source, rhs_len) = match value {
        Expr::Ident(n) if matches!(shapes.get(n),Some((TypeSpec::Character(_),d)) if d.len()==1) =>
            (Expr::Index(n.clone(),vec![id(&index)]), shapes[n].1[0].clone()),
        Expr::Call(tag,a) if tag=="@packed_view" && a.len()==2 => {
            let Expr::Ident(n)=&a[0] else { bail!("CHARACTER array view requires array storage"); };
            (Expr::Index(n.clone(),vec![id(&index)]),a[1].clone())
        }
        Expr::Section { name:n, lower, upper, stride } if matches!(shapes.get(n),Some((TypeSpec::Character(_),d)) if d.len()==1) => {
            let step=stride.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(1);
            if step==0 { bail!("CHARACTER array section stride must not be zero"); }
            let extent=length(n,shapes).unwrap_or(shapes[n].1[0].clone());
            let lo=lower.as_deref().cloned().unwrap_or_else(||if step<0 {extent.clone()} else {int(1)});
            let hi=upper.as_deref().cloned().unwrap_or_else(||if step<0 {int(1)} else {extent});
            let count=Expr::Call("max".into(),vec![int(0),bin(BinOp::Div,bin(BinOp::Add,bin(BinOp::Sub,hi,lo.clone()),int(step)),int(step))]);
            (Expr::Index(n.clone(),vec![bin(BinOp::Add,lo,bin(BinOp::Mul,bin(BinOp::Sub,id(&index),int(1)),int(step)))]),count)
        }
        _ => {
            let actual=crate::array_calls::scalar_actual_kind(value,shapes)?;
            if !matches!(actual,TypeSpec::Character(_)) { bail!("CHARACTER array assignment requires CHARACTER values"); }
            let scalar=format!("@alloc_char_scalar_{serial}");
            out.push(Stmt::VarDecl {kind:kind.clone(),names:vec![scalar.clone()]});
            out.push(assign(&scalar,value.clone()));
            (id(&scalar),len.clone())
        }
    };
    out.push(fail(bin(BinOp::Ne,len.clone(),rhs_len),"allocatable array assignment shapes do not conform"));
    out.push(Stmt::ArrayDecl {kind,name:temp.clone(),dims});
    out.push(Stmt::VarDecl {kind:TypeSpec::Integer(None),names:vec![index.clone()]});
    out.push(Stmt::Do {var:index.clone(),start:int(1),end:len.clone(),body:vec![Stmt::AssignIndex {name:temp.clone(),indices:vec![id(&index)],value:source}]});
    out.push(Stmt::Do {var:index.clone(),start:int(1),end:len,body:vec![Stmt::AssignIndex {name:name.into(),indices:vec![id(&index)],value:Expr::Index(temp,vec![id(&index)])}]});
    Ok(())
}
pub(crate) fn prepare(mut p: Program) -> Result<Program> {
    fn collect(body: &[Stmt], outputs: &mut HashMap<String, (Vec<usize>, Option<i64>)>) {
        for s in body {
            match s {
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let positions=params.iter().enumerate().filter_map(|(i,p)|{
                    (body.iter().any(|s|matches!(s,Stmt::DummyIntent {name,intent} if name==p && intent=="allocatable"))
                    && body.iter().any(|s|matches!(s,Stmt::DummyIntent {name,intent} if name==p && intent=="intent(out)"))).then_some(i)
                }).collect::<Vec<_>>();
                    if !positions.is_empty() {
                        let local_bound = body.iter().filter_map(|s| {
                            if let Stmt::ArrayDecl { dims, .. } = s {
                                if let [dim] = dims.as_slice() {
                                    return crate::parameters::integer_constant(dim).ok();
                                }
                            }
                            None
                        }).max();
                        outputs.insert(name.clone(), (positions, local_bound));
                    }
                }
                Stmt::Module { body, .. } => collect(body, outputs),
                _ => {}
            }
        }
    }
    let mut outputs = HashMap::new();
    collect(&p.body, &mut outputs);
    if outputs.is_empty() {
        return Ok(p);
    }
    let mut arrays = HashMap::new();
    for s in &p.body {
        if let Stmt::ArrayDecl { name, dims, .. } = s {
            if let [dim] = dims.as_slice() {
                let extent = if let Expr::Call(n, a) = dim {
                    if n == "@declared_bounds" {
                        let hi = i128::from(crate::parameters::integer_constant(&a[1])?);
                        let lo = i128::from(crate::parameters::integer_constant(&a[0])?);
                        Some(
                            i64::try_from((hi - lo + 1).max(0))
                                .context("array capacity overflows INTEGER(8)")?,
                        )
                    } else {
                        None
                    }
                } else {
                    crate::parameters::integer_constant(dim).ok()
                };
                if let Some(n) = extent {
                    arrays.insert(name.clone(), n.max(0));
                }
            }
        }
    }
    let mut bounds = HashMap::<String, i64>::new();
    fn calls(
        body: &[Stmt],
        outputs: &HashMap<String, (Vec<usize>, Option<i64>)>,
        arrays: &HashMap<String, i64>,
        bounds: &mut HashMap<String, i64>,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::CallSub { name, args } => {
                    if let Some((positions, local_bound)) = outputs.get(name) {
                        let mut available = arrays.clone();
                        available.extend(bounds.iter().map(|(n, v)| (n.clone(), *v)));
                        let bound=args.iter().enumerate().filter(|(i,_)|!positions.contains(i)).filter_map(|(_,e)|capacity(e,&available)).max().or(*local_bound)
                    .context("ALLOCATABLE output specialization requires a bounded rank-one array input or fixed local array")?;
                        for &i in positions {
                            if let Some(Expr::Ident(n)) = args.get(i) {
                                bounds
                                    .entry(n.clone())
                                    .and_modify(|v| *v = (*v).max(bound))
                                    .or_insert(bound);
                            } else {
                                bail!("ALLOCATABLE output must be an allocatable variable");
                            }
                        }
                    }
                }
                Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                    calls(body, outputs, arrays, bounds)?
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    calls(then_body, outputs, arrays, bounds)?;
                    if let Some(b) = else_body {
                        calls(b, outputs, arrays, bounds)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let allocatables = attrs_count(&p.body);
    let mut converged = false;
    for _ in 0..=allocatables + 1 {
        let previous = bounds.clone();
        calls(&p.body, &outputs, &arrays, &mut bounds)?;
        if bounds == previous {
            converged = true;
            break;
        }
    }
    if !converged {
        bail!("ALLOCATABLE output calls require a finite storage bound; growing array call cycles are not yet supported");
    }
    let attrs = p
        .body
        .iter()
        .filter_map(|s| {
            if let Stmt::DummyIntent { name, intent } = s {
                Some((name.clone(), intent.clone()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut out = vec![];
    for s in p.body {
        match s {
            Stmt::ArrayDecl { kind, name, dims } if bounds.contains_key(&name) => {
                if !attrs.iter().any(|(n, a)| n == &name && a == "allocatable") {
                    bail!("ALLOCATABLE dummy requires an ALLOCATABLE actual `{name}`");
                }
                if dims.len() != 1 || !matches!(&dims[0],Expr::Ident(n) if n=="@deferred_extent") {
                    bail!("ALLOCATABLE outputs currently require deferred-shape rank-one arrays");
                }
                if !matches!(
                    kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::Real
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                        | TypeSpec::Character(_)
                ) {
                    bail!("bounded ALLOCATABLE outputs require numeric, LOGICAL or fixed-length CHARACTER arrays");
                }
                let length = format!("@alloc_length_{name}");
                let flag = format!("@alloc_flag_{name}");
                out.push(Stmt::ArrayDecl {
                    kind,
                    name: name.clone(),
                    dims: vec![int(bounds[&name])],
                });
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(None),
                    names: vec![length.clone()],
                });
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Logical,
                    names: vec![flag.clone()],
                });
                out.push(assign(&length, int(0)));
                out.push(assign(&flag, Expr::Logical(false)));
                out.push(Stmt::DummyIntent {
                    name,
                    intent: format!("@bounded:{length}:{flag}"),
                });
            }
            Stmt::DummyIntent { name, intent }
                if bounds.contains_key(&name) && intent == "allocatable" => {}
            s => out.push(s),
        }
    }
    p.body = out;
    Ok(p)
}
fn attrs_count(body: &[Stmt]) -> usize {
    body.iter()
        .filter(|s| matches!(s,Stmt::DummyIntent {intent,..} if intent=="allocatable"))
        .count()
}
pub(crate) fn finish(mut p: Program) -> Result<Program> {
    fn expression(e: &mut Expr, shapes: &Shapes, guards: &mut Vec<Stmt>) -> Result<()> {
        fn check(n: &str, shapes: &Shapes, guards: &mut Vec<Stmt>) {
            if let Some(flag) = allocated(n, shapes) {
                guards.push(fail(
                    Expr::Un(UnOp::Not, Box::new(flag)),
                    &format!("read of unallocated array `{n}`"),
                ));
            }
        }
        match e {
            Expr::Section { name, .. } if allocated(name, shapes).is_some() => {
                bail!("sections of bounded allocatable arrays are not yet supported")
            }
            Expr::Call(tag, args) if tag == "@matrix_view" => {
                if matches!(args.first(),Some(Expr::Ident(name)) if allocated(name,shapes).is_some())
                {
                    bail!("sections of bounded allocatable arrays are not yet supported");
                }
            }
            Expr::Call(n, a) if n == "@allocated_extent" => {
                let Expr::Str(name) = &a[2] else {
                    unreachable!()
                };
                guards.push(fail(
                    Expr::Un(UnOp::Not, Box::new(a[0].clone())),
                    &format!("read of unallocated array `{name}`"),
                ));
                *e = a[1].clone();
            }
            Expr::Call(n, a) if n == "@packed_view" => {
                if let Expr::Ident(n) = &a[0] {
                    check(n, shapes, guards);
                }
                expression(&mut a[1], shapes, guards)?;
            }
            Expr::Call(n, a)
                if matches!(n.as_str(), "allocated" | "size" | "lbound" | "ubound") =>
            {
                if let Some(Expr::Ident(v)) = a.first() {
                    if let Some(len) = length(v, shapes) {
                        if n == "allocated" {
                            if a.len() != 1 {
                                bail!("ALLOCATED expects one array");
                            }
                            *e = allocated(v, shapes).unwrap();
                        } else {
                            if a.len() > 2
                                || a.get(1).is_some_and(|d| {
                                    crate::parameters::integer_constant(d).ok() != Some(1)
                                })
                            {
                                bail!("array inquiry dimension must be 1 for a rank-one allocatable array");
                            }
                            check(v, shapes, guards);
                            let value = if n == "lbound" { int(1) } else { len };
                            *e = if n != "size" && a.len() == 1 {
                                Expr::Call("@array_constructor".into(), vec![value])
                            } else {
                                value
                            };
                        }
                        return Ok(());
                    }
                }
                for e in a {
                    expression(e, shapes, guards)?;
                }
            }
            Expr::Ident(n) if length(n, shapes).is_some() => {
                check(n, shapes, guards);
                *e = view(n, shapes);
            }
            Expr::Index(n, a) | Expr::Call(n, a) => {
                if allocated(n, shapes).is_some()
                    && a.iter()
                        .any(|e| matches!(e,Expr::Call(tag,_) if tag=="@section_subscript"))
                {
                    bail!("sections of bounded allocatable arrays are not yet supported");
                }
                check(n, shapes, guards);
                for e in a {
                    expression(e, shapes, guards)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expression(a, shapes, guards)?;
                expression(b, shapes, guards)?;
            }
            Expr::Un(_, a) => expression(a, shapes, guards)?,
            _ => {}
        }
        Ok(())
    }
    fn lower(body: &mut Vec<Stmt>, inherited: &Shapes, serial: &mut usize) -> Result<()> {
        let mut shapes = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::ArrayDecl { kind, name, dims } => {
                    shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        shapes.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                _ => {}
            }
        }
        metadata(body, &mut shapes);
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            let mut guards = vec![];
            match &mut s {
                Stmt::CallSub { name, args } if name == "@allocate" => {
                    let mut handled = true;
                    for arg in args.iter() {
                        let Expr::Call(n, dims) = arg else {
                            bail!("ALLOCATE expects array bounds");
                        };
                        let Some(len) = length(n, &shapes) else {
                            handled = false;
                            break;
                        };
                        let [extent] = dims.as_slice() else {
                            bail!("ALLOCATABLE output currently supports rank one");
                        };
                        let Expr::Ident(len) = len else {
                            unreachable!()
                        };
                        let Expr::Ident(flag) = allocated(n, &shapes).unwrap() else {
                            unreachable!()
                        };
                        let mut extent = extent.clone();
                        expression(&mut extent, &shapes, &mut out)?;
                        out.push(fail(
                            id(&flag),
                            &format!("array `{n}` is already allocated"),
                        ));
                        out.push(assign(&len, Expr::Call("max".into(), vec![int(0), extent])));
                        out.push(fail(
                            bin(BinOp::Gt, id(&len), shapes[n].1[0].clone()),
                            &format!("allocation of `{n}` exceeds its bounded call-site capacity"),
                        ));
                        out.push(assign(&flag, Expr::Logical(true)));
                        out.push(Stmt::CallSub {
                            name: "@undefine".into(),
                            args: vec![id(n)],
                        });
                    }
                    if handled {
                        continue;
                    }
                }
                Stmt::Assign { name, value } if length(name, &shapes).is_some() => {
                    expression(value, &shapes, &mut out)?;
                    let len = length(name, &shapes).unwrap();
                    let flag = allocated(name, &shapes).unwrap();
                    out.push(fail(
                        Expr::Un(UnOp::Not, Box::new(flag)),
                        &format!("assignment to unallocated array `{name}`"),
                    ));
                    if matches!(shapes[name].0, TypeSpec::Character(_)) {
                        character_copy(name, value, len, &shapes, serial, &mut out)?;
                        continue;
                    }
                    let scalar = crate::array_calls::scalar_actual_kind(value, &shapes).ok();
                    if let Some(kind) = scalar {
                        *serial += 1;
                        let temp = format!("@alloc_scalar_{serial}");
                        let index = format!("@alloc_scalar_index_{serial}");
                        out.push(Stmt::VarDecl {
                            kind,
                            names: vec![temp.clone()],
                        });
                        out.push(assign(&temp, value.clone()));
                        out.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(None),
                            names: vec![index.clone()],
                        });
                        out.push(Stmt::Do {
                            var: index.clone(),
                            start: int(1),
                            end: len,
                            body: vec![Stmt::AssignIndex {
                                name: name.clone(),
                                indices: vec![id(&index)],
                                value: id(&temp),
                            }],
                        });
                        continue;
                    }
                    let start = out.len();
                    let (_, _, rhs_len) =
                        crate::reductions::argument(value, &shapes, &mut out, serial)?;
                    for s in &out[start..] {
                        match s {
                            Stmt::ArrayDecl { kind, name, dims } => {
                                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                            }
                            Stmt::VarDecl { kind, names } => {
                                for n in names {
                                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                                }
                            }
                            _ => {}
                        }
                    }
                    out.push(fail(
                        bin(BinOp::Ne, len.clone(), rhs_len),
                        "allocatable array assignment shapes do not conform",
                    ));
                    // Capture before writing, preserving overlap and self-assignment.
                    *serial += 1;
                    let index = format!("@alloc_copy_index_{serial}");
                    let temp = format!("@alloc_copy_{serial}");
                    let (kind, dims) = shapes[name].clone();
                    out.push(Stmt::ArrayDecl {
                        kind,
                        name: temp.clone(),
                        dims,
                    });
                    out.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(None),
                        names: vec![index.clone()],
                    });
                    let value = crate::reductions::argument_element(value, &id(&index), &shapes)?;
                    out.push(Stmt::Do {
                        var: index.clone(),
                        start: int(1),
                        end: len.clone(),
                        body: vec![Stmt::AssignIndex {
                            name: temp.clone(),
                            indices: vec![id(&index)],
                            value,
                        }],
                    });
                    out.push(Stmt::Do {
                        var: index.clone(),
                        start: int(1),
                        end: len,
                        body: vec![Stmt::AssignIndex {
                            name: name.clone(),
                            indices: vec![id(&index)],
                            value: Expr::Index(temp, vec![id(&index)]),
                        }],
                    });
                    continue;
                }
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    expression(value, &shapes, &mut guards)?
                }
                Stmt::AssignIndex { indices, value, .. } => {
                    for e in indices {
                        expression(e, &shapes, &mut guards)?;
                    }
                    expression(value, &shapes, &mut guards)?;
                }
                Stmt::CallSub { name, .. } if name == "@undefine" => {}
                Stmt::CallSub { args, .. } | Stmt::Print { items: args } | Stmt::Read { args } => {
                    for e in args {
                        expression(e, &shapes, &mut guards)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    expression(cond, &shapes, &mut guards)?;
                    lower(then_body, &shapes, serial)?;
                    if let Some(b) = else_body {
                        lower(b, &shapes, serial)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    expression(start, &shapes, &mut guards)?;
                    expression(end, &shapes, &mut guards)?;
                    lower(body, &shapes, serial)?;
                }
                Stmt::DoWhile { cond, body } => {
                    expression(cond, &shapes, &mut guards)?;
                    lower(body, &shapes, serial)?;
                    if !guards.is_empty() {
                        guards.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        guards.append(body);
                        *body = std::mem::take(&mut guards);
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Block { body }
                | Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Function { body, .. } => lower(body, &shapes, serial)?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    expression(expr, &shapes, &mut guards)?;
                    for c in cases {
                        lower(&mut c.body, &shapes, serial)?;
                    }
                    if let Some(b) = default {
                        lower(b, &shapes, serial)?;
                    }
                }
                _ => {}
            }
            out.extend(guards);
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    lower(&mut p.body, &Shapes::new(), &mut 0)?;
    Ok(p)
}

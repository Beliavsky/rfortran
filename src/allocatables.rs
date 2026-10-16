// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Bounded output specializations and descriptors for runtime-allocated numeric storage.
//! Heap storage uses runtime extents; bounded specializations retain capacity checks.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
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
            if intent=="@heap" {shapes.insert(format!("@heap:{name}"),(TypeSpec::Logical,vec![]));}
            if let Some(rest)=intent.strip_prefix("@bounded_lower:") {shapes.insert(format!("@lowers:{name}"),(TypeSpec::Integer(Some(8)),rest.split(':').map(id).collect()));}
            if let Some(rest) = intent.strip_prefix("@bounded_shape:") {
                shapes.insert(
                    format!("@axes:{name}"),
                    (TypeSpec::Integer(None), rest.split(':').map(id).collect()),
                );
            }
            if let Some(rest) = intent.strip_prefix("@bounded_limits:") {
                shapes.insert(
                    format!("@axis_limits:{name}"),
                    (
                        TypeSpec::Integer(None),
                        rest.split(':').map(|n| Expr::IntLit(n.into())).collect(),
                    ),
                );
            }
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
pub(crate) fn axes(name: &str, shapes: &Shapes) -> Option<Vec<Expr>> {
    shapes.get(&format!("@axes:{name}")).map(|v| v.1.clone())
}
pub(crate) fn lowers(name:&str,shapes:&Shapes)->Option<Vec<Expr>> {shapes.get(&format!("@lowers:{name}")).map(|v|v.1.clone())}
pub(crate) fn inquiry(name: &str, args: &[Expr], shapes: &Shapes) -> Result<Option<Expr>> {
    if name=="len" && args.len()==1 {
        if let Some(Expr::Ident(n))=args.first() {
            if let Some((TypeSpec::Character(width),_))=shapes.get(n) {return Ok(Some(int(width.unwrap_or(1) as i64)));}
        }
    }
    if name=="kind" && args.len()==1 {
        let array=match &args[0] {Expr::Ident(n)=>Some(n),Expr::Call(tag,args) if tag=="@packed_view"=>if let Expr::Ident(n)=&args[0]{Some(n)}else{None},_=>None};
        if let Some((kind,_))=array.and_then(|n|shapes.get(n)) {
            let bytes=match kind {TypeSpec::Integer(n)=>Some(n.unwrap_or(4)),TypeSpec::Real|TypeSpec::Logical=>Some(4),TypeSpec::DoublePrecision=>Some(8),TypeSpec::Character(_)=>Some(1),_=>None};
            if let Some(bytes)=bytes {return Ok(Some(int(i64::from(bytes))));}
        }
        return Ok(None);
    }
    let Some(Expr::Ident(array)) = args.first() else {
        return Ok(None);
    };
    let Some(length) = length(array, shapes) else {
        return Ok(None);
    };
    let dimensions = axes(array, shapes).unwrap_or_else(|| vec![length.clone()]);
    let lower=lowers(array,shapes).unwrap_or_else(||vec![int(1);dimensions.len()]);
    let bound=|axis:usize,extent:Expr| Expr::Call("@heap_bound".into(),vec![lower[axis].clone(),extent,Expr::Logical(name=="ubound")]);
    if name=="allocated" {if args.len()!=1{bail!("ALLOCATED expects one array");}return Ok(allocated(array,shapes));}
    let guard = |value: Expr| {
        allocated(array, shapes)
            .map(|flag| {
                Expr::Call(
                    "@allocated_extent".into(),
                    vec![flag, value.clone(), Expr::Str(array.clone())],
                )
            })
            .unwrap_or(value)
    };
    if name == "shape" {
        if args.len() != 1 {
            bail!("SHAPE KIND is not yet supported for runtime-sized allocatable arrays");
        }
        return Ok(Some(Expr::Call(
            "@array_constructor".into(),
            dimensions.into_iter().map(guard).collect(),
        )));
    }
    if args.len() > 2 {
        bail!("array inquiry KIND is not yet supported for runtime-sized allocatable arrays");
    }
    let axis = if let Some(dim) = args.get(1) {
        let dim = crate::parameters::integer_constant(dim)
            .context("array inquiry DIM must currently be a constant INTEGER")?;
        if dim < 1 || dim as usize > dimensions.len() {
            bail!(
                "array inquiry DIM must be between 1 and {}",
                dimensions.len()
            );
        }
        Some(dim as usize - 1)
    } else {
        None
    };
    let result = if name == "size" {
        guard(axis.map(|d| dimensions[d].clone()).unwrap_or(length))
    } else if let Some(axis) = axis {
        guard(bound(axis,dimensions[axis].clone()))
    } else {
        Expr::Call(
            "@array_constructor".into(),
            dimensions
                .into_iter()
                .enumerate().map(|(axis,extent)| guard(bound(axis,extent)))
                .collect(),
        )
    };
    Ok(Some(result))
}

// Runtime arrays retain a flat buffer or heap pointer and separate active axes.
pub(crate) fn prepare_matrices(
    body: &mut Vec<Stmt>,
    inherited: &mut Shapes,
    constants: &HashMap<String, Expr>,
) -> Result<()> {
    let globals=body.iter().filter_map(|s|if let Stmt::CallSub {name,args}=s {if name=="@module_bind" || name=="@heap_bind" {if let Some(Expr::Ident(n))=args.first() {return Some(n.clone());}}None}else{None}).collect::<HashSet<_>>();
    for s in body.iter() {
        if let Stmt::ArrayDecl { name, dims, .. } = s {
            if (dims.len() > 1 || globals.contains(name))
                && dims
                    .iter()
                    .all(|d| matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
            {
                for key in ["@allocated", "@extent", "@axes", "@axis_limits"] {
                    inherited.remove(&format!("{key}:{name}"));
                }
            }
        }
    }
    let candidates:HashMap<_,_>=body.iter().filter_map(|s|match s {
        Stmt::ArrayDecl {name,dims,..} if (dims.len()>1 || globals.contains(name)) && dims.iter().all(|d|matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
            && allocated(name,inherited).is_none()
            && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name&&intent=="@heap_dummy"))
            && body.iter().any(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==name && intent=="allocatable"))=>Some((name.clone(),dims.len())),
        _=>None,
    }).collect();
    fn used(body: &[Stmt], candidates: &HashMap<String, usize>, found: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == "@deallocate" => {
                    for e in args {
                        if let Expr::Ident(n) = e {
                            if candidates.contains_key(n) {
                                found.insert(n.clone());
                            }
                        }
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    used(then_body, candidates, found);
                    if let Some(b) = else_body {
                        used(b, candidates, found);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => used(body, candidates, found),
                Stmt::Block { body } => {
                    let mut visible = candidates.clone();
                    for s in body {
                        match s {
                            Stmt::ArrayDecl { name, .. } => {
                                visible.remove(name);
                            }
                            Stmt::VarDecl { names, .. } => {
                                for n in names {
                                    visible.remove(n);
                                }
                            }
                            _ => {}
                        }
                    }
                    used(body, &visible, found);
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        used(&c.body, candidates, found);
                    }
                    if let Some(b) = default {
                        used(b, candidates, found);
                    }
                }
                _ => {}
            }
        }
    }
    let mut wanted = HashSet::new();
    used(body, &candidates, &mut wanted);
    wanted.extend(globals.iter().filter(|n|candidates.contains_key(*n)).cloned());
    if wanted.is_empty() {
        metadata(body,inherited);
        return Ok(());
    }
    let targets:HashMap<String,usize> = candidates
        .into_iter()
        .filter(|(n, _)| wanted.contains(n))
        .collect();
    let mut shapes = inherited.clone();
    for s in body.iter() {
        if let Stmt::ArrayDecl { name, kind, dims } = s {
            shapes
                .entry(name.clone())
                .or_insert((kind.clone(), dims.clone()));
        }
    }
    let heap:HashSet<_>=body.iter().filter_map(|s|if let Stmt::DummyIntent{name,intent}=s {(intent=="@heap").then_some(name.clone())}else{None}).collect();
    let bounded_targets=targets.into_iter().filter(|(n,_)|!heap.contains(n)).collect();
    let bounds = crate::allocation_bounds::infer(body, &bounded_targets, &shapes, constants)?;
    let mut prefix = vec![];
    for s in body.iter_mut() {
        if let Stmt::ArrayDecl { name, kind, dims } = s {
            if wanted.contains(name) {
                if !matches!(
                    kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::Real
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                        | TypeSpec::Character(_)
                ) {
                    bail!("runtime-sized multidimensional allocation requires supported intrinsic elements");
                }
                let upper = bounds
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| vec![0; dims.len()]);
                if upper.iter().any(|n| *n > i64::from(i32::MAX)) {
                    bail!("bounded allocation dimension exceeds default INTEGER range");
                }
                let capacity = if heap.contains(name) {1} else {upper.iter().try_fold(1i64, |total, d| {
                    total
                        .checked_mul(*d)
                        .context("allocation storage bound overflows INTEGER(8)")
                })?};
                if capacity > i64::from(i32::MAX) {
                    bail!("bounded allocation capacity exceeds default INTEGER range");
                }
                let mut serial = 0;
                let base = loop {
                    let base = format!("@matrix_alloc_{name}_{serial}");
                    if !inherited.contains_key(&format!("{base}_length")) {
                        break base;
                    }
                    serial += 1;
                };
                let length = format!("{base}_length");
                let flag = format!("{base}_flag");
                let axis_names = (0..dims.len())
                    .map(|d| format!("{base}_axis_{d}"))
                    .collect::<Vec<_>>();
                if heap.contains(name) {
                    let lower_names=(0..dims.len()).map(|d|format!("{base}_lower_{d}")).collect::<Vec<_>>();
                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:lower_names.clone()});
                    for n in &lower_names {prefix.push(assign(n,int(1)));}
                    prefix.push(Stmt::DummyIntent{name:name.clone(),intent:format!("@bounded_lower:{}",lower_names.join(":"))});
                }
                prefix.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: axis_names.clone(),
                });
                for axis in &axis_names {
                    prefix.push(assign(axis, int(0)));
                }
                prefix.extend([
                    Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![length.clone()],
                    },
                    Stmt::VarDecl {
                        kind: TypeSpec::Logical,
                        names: vec![flag.clone()],
                    },
                    assign(&length, int(0)),
                    assign(&flag, Expr::Logical(false)),
                    Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@bounded:{length}:{flag}"),
                    },
                    Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@bounded_shape:{}", axis_names.join(":")),
                    },
                    Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!(
                            "@bounded_limits:{}",
                            upper
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(":")
                        ),
                    },
                ]);
                *dims = vec![int(capacity)];
                inherited.insert(name.clone(), (kind.clone(), dims.clone()));
            }
        }
    }
    body.retain(|s|!matches!(s,Stmt::DummyIntent {name,intent} if wanted.contains(name) && intent=="allocatable"));
    prefix.append(body);
    *body = prefix;
    metadata(body, inherited);
    Ok(())
}

fn conform_axes(a: &[Expr], b: &[Expr], guards: &mut Vec<Stmt>) -> Result<()> {
    if a.len() != b.len() {
        bail!("runtime-sized array ranks do not conform");
    }
    for (axis, (a, b)) in a.iter().zip(b).enumerate() {
        guards.push(fail(
            bin(BinOp::Ne, a.clone(), b.clone()),
            &format!(
                "runtime-sized array shapes do not conform in dimension {}",
                axis + 1
            ),
        ));
    }
    Ok(())
}

// Preserve logical rank checks before runtime arrays become flat packed views.
fn logical_shape(
    e: &Expr,
    shapes: &Shapes,
    guards: &mut Vec<Stmt>,
) -> Result<Option<(Vec<Expr>, bool)>> {
    match e {
        Expr::Ident(n) => Ok(if let Some(axes) = axes(n, shapes) {
            Some((axes, true))
        } else {
            shapes.get(n).filter(|v| !v.1.is_empty()).map(|v| {
                (
                    length(n, shapes)
                        .map(|n| vec![n])
                        .unwrap_or_else(|| v.1.clone()),
                    false,
                )
            })
        }),
        Expr::Call(n, args) if n == "@packed_view" => {
            if let Expr::Ident(array)=&args[0] {if let Some(axes)=axes(array,shapes) {if axes.len()>1{return Ok(Some((axes,true)));}}}
            Ok(Some((vec![args[1].clone()],true)))
        },
        Expr::Bin(_, a, b) => {
            let a = logical_shape(a, shapes, guards)?;
            let b = logical_shape(b, shapes, guards)?;
            if let (Some((ad, ar)), Some((bd, br))) = (&a, &b) {
                if *ar || *br {
                    conform_axes(ad, bd, guards)?;
                }
                if *ar != *br && ad.len() > 1 {
                    bail!("mixing fixed and runtime-sized multidimensional array expressions is not yet supported");
                }
            }
            Ok(a.or(b))
        }
        Expr::Un(_, a) => logical_shape(a, shapes, guards),
        Expr::Call(n, _) if matches!(n.as_str(),"size"|"shape"|"lbound"|"ubound"|"allocated"|"kind"|"@allocated_linear_element")=>Ok(None),
        Expr::Call(n, args) if !shapes.contains_key(n) => {
            let mut result: Option<(Vec<Expr>, bool)> = None;
            for arg in args {
                if let Some((dims, runtime)) = logical_shape(arg, shapes, guards)? {
                    if let Some((previous, pr)) = &result {
                        if runtime || *pr {
                            conform_axes(previous, &dims, guards)?;
                        }
                    }
                    if result.is_none() {
                        result = Some((dims, runtime));
                    }
                }
            }
            let dynamic = result
                .as_ref()
                .is_some_and(|(dims, runtime)| *runtime && dims.len() > 1);
            let reduction = matches!(
                n.as_str(),
                "sum"
                    | "product"
                    | "minval"
                    | "maxval"
                    | "any"
                    | "all"
                    | "count"
                    | "@count_default"
                    | "@count_i8"
                    | "@any_reduction"
                    | "@all_reduction"
            );
            if reduction {
                if dynamic
                    && args.len() > 1
                    && matches!(n.as_str(), "sum" | "product" | "minval" | "maxval")
                {
                    bail!("masked or DIM reductions on runtime-sized multidimensional arrays are not yet supported");
                }
                if dynamic
                    && args
                        .get(1)
                        .is_some_and(|d| !matches!(d,Expr::Call(tag,_) if tag=="@no_dim"))
                {
                    bail!("reductions with DIM on runtime-sized multidimensional arrays are not yet supported");
                }
                return Ok(None);
            }
            if crate::intrinsics::math_arity(n).is_some()
                || crate::intrinsics::integer_rounding(n) || crate::intrinsics::real_predicate(n)
                || crate::bit_intrinsics::keys(n).is_some()
                || matches!(
                    n.as_str(),
                    "abs" | "mod" | "modulo" | "real" | "int" | "dble"
                )
            {
                return Ok(result);
            }
            if dynamic {
                bail!("`{n}` with runtime-sized multidimensional arrays is not yet supported");
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn linear_index(
    name: &str,
    indices: &mut Vec<Expr>,
    shapes: &Shapes,
    checks: &mut Vec<Stmt>,
    serial: &mut usize,
) -> Result<()> {
    let Some(axes) = axes(name, shapes) else {
        return Ok(());
    };
    if indices.len() != axes.len() {
        bail!(
            "array `{name}` requires {} subscripts, got {}",
            axes.len(),
            indices.len()
        );
    }
    // A rank-dependent nested offset expression can overflow the compiler's
    // Windows stack. Keep offset calculation in a small scalar helper instead.
    *serial+=1;
    let function=format!("@alloc_offset_{serial}");
    let offset=format!("@alloc_offset_value_{serial}");let stride=format!("@alloc_stride_value_{serial}");
    let mut params=vec![];let mut actuals=vec![];let mut body=vec![];let mut terms=vec![];
    for (axis,(index,extent)) in indices.iter().zip(axes).enumerate() {
        if !matches!(crate::array_calls::scalar_actual_kind(index,shapes)?,TypeSpec::Integer(_)) {bail!("array `{name}` subscript must be scalar INTEGER");}
        let position=format!("@alloc_position_{serial}_{axis}");let limit=format!("@alloc_limit_{serial}_{axis}");
        params.extend([position.clone(),limit.clone()]);
        let position_value=if let Some(lower)=lowers(name,shapes){bin(BinOp::Add,bin(BinOp::Sub,index.clone(),lower[axis].clone()),int(1))}else{index.clone()};
        actuals.extend([position_value,extent]);
        body.push(fail(bin(BinOp::Or,bin(BinOp::Lt,id(&position),int(1)),bin(BinOp::Gt,id(&position),id(&limit))),&format!("array `{name}` dimension {}: subscript outside allocated bounds",axis+1)));
        terms.push((position,limit));
    }
    let mut declarations=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:params.iter().cloned().chain([offset.clone(),stride.clone()]).collect()}];
    declarations.extend(params.iter().map(|name|Stmt::DummyIntent{name:name.clone(),intent:"intent(in)".into()}));
    declarations.append(&mut body);body=declarations;
    body.extend([assign(&offset,int(1)),assign(&stride,int(1))]);
    for (position,limit) in terms {
        body.push(assign(&offset,bin(BinOp::Add,id(&offset),bin(BinOp::Mul,bin(BinOp::Sub,id(&position),int(1)),id(&stride)))));
        body.push(assign(&stride,bin(BinOp::Mul,id(&stride),id(&limit))));
    }
    body.push(Stmt::Return(Some(id(&offset))));
    checks.push(Stmt::Function{name:function.clone(),params,pure:true,elemental:true,return_type:Some(TypeSpec::Integer(Some(8))),body});
    let offset=Expr::Call(function,actuals);
    *indices = vec![offset];
    Ok(())
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
pub(crate) fn deallocation_targets(args: &[Expr]) -> Result<Vec<String>> {
    if args.is_empty() {
        bail!("DEALLOCATE requires at least one ALLOCATABLE object");
    }
    args.iter()
        .map(|e| match e {
            Expr::Ident(n) => Ok(n.clone()),
            Expr::Call(n, _) if n.starts_with("@keyword:") => {
                bail!("DEALLOCATE STAT/ERRMSG options are not yet supported")
            }
            _ => bail!(
                "DEALLOCATE requires whole ALLOCATABLE variables, not elements or expressions"
            ),
        })
        .collect()
}
fn capacity(e: &Expr, arrays: &HashMap<String, i64>) -> Option<i64> {
    match e {
        Expr::Ident(n) => arrays.get(n).copied(),
        Expr::Call(n,a) if n=="@packed_view" => a.first().and_then(|e|capacity(e,arrays)),
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
    if shapes.contains_key(&format!("@heap:{name}")) {crate::heap::temporary(kind,temp.clone(),len.clone(),out);}
    else {out.push(Stmt::ArrayDecl {kind,name:temp.clone(),dims});}
    out.push(Stmt::VarDecl {kind:TypeSpec::Integer(None),names:vec![index.clone()]});
    out.push(Stmt::Do {var:index.clone(),start:int(1),end:len.clone(),body:vec![Stmt::AssignIndex {name:temp.clone(),indices:vec![id(&index)],value:source}]});
    out.push(Stmt::Do {var:index.clone(),start:int(1),end:len,body:vec![Stmt::AssignIndex {name:name.into(),indices:vec![id(&index)],value:Expr::Index(temp,vec![id(&index)])}]});
    Ok(())
}
pub(crate) fn prepare(p: Program) -> Result<Program> {
    prepare_with_bounds(p, HashMap::new())
}

// Direct ALLOCATE statements can bound a function result without an output
// helper. Use the same capacity and active-length metadata as helper results.
pub(crate) fn prepare_function_result(
    p: Program, result: &str, constants: &HashMap<String, Expr>,
) -> Result<Program> {
    let shapes: Shapes = p.body.iter().filter_map(|s| match s {
        Stmt::ArrayDecl {kind,name,dims} => Some((name.clone(),(kind.clone(),dims.clone()))),
        _ => None,
    }).collect();
    let mut bounds = HashMap::new();
    if matches!(shapes.get(result),Some((TypeSpec::Character(_),dims)) if dims.len()==1) {
        let targets=HashMap::from([(result.to_owned(),1)]);
        if let Some(dims)=crate::allocation_bounds::infer(&p.body,&targets,&shapes,constants)?.get(result) {
            bounds.insert(result.to_owned(),dims[0]);
        }
    }
    prepare_with_bounds(p,bounds)
}

fn prepare_with_bounds(mut p: Program, mut bounds: HashMap<String,i64>) -> Result<Program> {
    fn collect(body: &[Stmt], outputs: &mut HashMap<String, (Vec<usize>, Option<i64>)>) {
        for s in body {
            match s {
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let positions=params.iter().enumerate().filter_map(|(i,p)|{
                    (body.iter().any(|s|matches!(s,Stmt::DummyIntent {name,intent} if name==p && intent=="allocatable"))
                    && body.iter().any(|s|matches!(s,Stmt::DummyIntent {name,intent} if name==p && intent=="intent(out)"))
                    && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p&&intent=="@heap_dummy"))).then_some(i)
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
    if outputs.is_empty() && bounds.is_empty() {
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
    // Constructor temporaries have globally unique names, including those
    // materialized inside a branch or loop rather than at program scope.
    fn constructor_arrays(body:&[Stmt], arrays:&mut HashMap<String,i64>)->Result<()> {
        for s in body {match s {
            Stmt::ArrayDecl {name,dims,..} if name.starts_with("@constructor_actual_")=>{
                if let [dim]=dims.as_slice() {arrays.insert(name.clone(),crate::parameters::integer_constant(dim)?);}
            },
            Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..}=>constructor_arrays(body,arrays)?,
            Stmt::If {then_body,else_body,..}=>{constructor_arrays(then_body,arrays)?;if let Some(body)=else_body {constructor_arrays(body,arrays)?;}},_=>{}
        }}
        Ok(())
    }
    constructor_arrays(&p.body,&mut arrays)?;
    fn prepared_outputs(body:&[Stmt],names:&mut HashSet<String>) {
        for s in body {match s {
            Stmt::DummyIntent{name,intent} if intent.starts_with("@bounded:")=>{names.insert(name.clone());},
            _=>{}
        }}
    }
    let mut prepared=HashSet::new();
    prepared_outputs(&p.body,&mut prepared);
    fn calls(
        body: &[Stmt],
        outputs: &HashMap<String, (Vec<usize>, Option<i64>)>,
        arrays: &HashMap<String, i64>,
        bounds: &mut HashMap<String, i64>,
        prepared: &HashSet<String>,
    ) -> Result<()> {
        let mut prepared=prepared.clone();
        for s in body {match s {
            Stmt::ArrayDecl{name,..}=>{prepared.remove(name);},
            Stmt::VarDecl{names,..}=>for n in names {prepared.remove(n);},_=>{}
        }}
        prepared_outputs(body,&mut prepared);
        for s in body {
            match s {
                Stmt::CallSub { name, args } => {
                    if let Some((positions, local_bound)) = outputs.get(name) {
                        if positions.iter().all(|i|matches!(args.get(*i),Some(Expr::Ident(n)) if prepared.contains(n))) {continue;}
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
                    calls(body, outputs, arrays, bounds, &prepared)?
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    calls(then_body, outputs, arrays, bounds, &prepared)?;
                    if let Some(b) = else_body {
                        calls(b, outputs, arrays, bounds, &prepared)?;
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
        calls(&p.body, &outputs, &arrays, &mut bounds, &prepared)?;
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
    fn scoped(body: &mut Vec<Stmt>, shapes: &Shapes) {
        // Later array lowering adds private declarations to loop/branch bodies.
        // A BLOCK retains their scope while source variables still refer outward.
        if shapes.keys().any(|n| n.starts_with("@axes:")) {
            *body = vec![Stmt::Block {
                body: std::mem::take(body),
            }];
        }
    }
    fn expression(
        e: &mut Expr,
        shapes: &Shapes,
        guards: &mut Vec<Stmt>,
        checks: &mut Vec<Stmt>,
        serial: &mut usize,
    ) -> Result<()> {
        if let Expr::Call(n, args) = e {
            if matches!(
                n.as_str(),
                "allocated" | "size" | "lbound" | "ubound" | "shape" | "kind" | "len"
            ) {
                if let Some(value) = inquiry(n, args, shapes)? {
                    *e = value;
                    return expression(e, shapes, guards, checks, serial);
                }
            }
        }
        if let Expr::Call(tag,args)=e {if tag=="@heap_bound" {
            for arg in &mut args[..2]{expression(arg,shapes,guards,checks,serial)?;}
            let upper=matches!(args[2],Expr::Logical(true));let name=if upper{"@heap_upper_bound"}else{"@heap_lower_bound"};
            if !checks.iter().any(|s|matches!(s,Stmt::Function{name:n,..} if n==name)) {
                let lower="@heap_bound_lower";let extent="@heap_bound_extent";
                let value=if upper{bin(BinOp::Sub,bin(BinOp::Add,id(lower),id(extent)),int(1))}else{id(lower)};
                checks.push(Stmt::Function{name:name.into(),params:vec![lower.into(),extent.into()],return_type:Some(TypeSpec::Integer(Some(8))),pure:true,elemental:true,body:vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![lower.into(),extent.into()]},Stmt::DummyIntent{name:lower.into(),intent:"intent(in)".into()},Stmt::DummyIntent{name:extent.into(),intent:"intent(in)".into()},Stmt::If{cond:bin(BinOp::Eq,id(extent),int(0)),then_body:vec![Stmt::Return(Some(int(if upper{0}else{1})))],else_body:None},Stmt::Return(Some(value))]});
            }
            *e=Expr::Call(name.into(),args[..2].to_vec());return Ok(());
        }}
        if matches!(e, Expr::Bin(..) | Expr::Call(..)) {
            logical_shape(e, shapes, guards)?;
        }
        fn check(n: &str, shapes: &Shapes, guards: &mut Vec<Stmt>) {
            if let Some(flag) = allocated(n, shapes) {
                guards.push(fail(
                    Expr::Un(UnOp::Not, Box::new(flag)),
                    &format!("read of unallocated array `{n}`"),
                ));
            }
        }
        match e {
            Expr::Call(tag,a) if tag=="@allocated_linear_element"=>{
                let Expr::Ident(name)=&a[0] else {unreachable!()};
                let name=name.clone();let mut index=a[1].clone();
                expression(&mut index,shapes,guards,checks,serial)?;
                check(&name,shapes,guards);
                *e=Expr::Index(name,vec![index]);
            }
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
                return expression(e,shapes,guards,checks,serial);
            }
            Expr::Call(n, a) if n == "@packed_view" => {
                if let Expr::Ident(n) = &a[0] {
                    check(n, shapes, guards);
                }
                expression(&mut a[1], shapes, guards, checks, serial)?;
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
                    expression(e, shapes, guards, checks, serial)?;
                }
            }
            Expr::Ident(n) if length(n, shapes).is_some() => {
                check(n, shapes, guards);
                *e = view(n, shapes);
            }
            Expr::Index(n, a) | Expr::Call(n, a) => {
                linear_index(n, a, shapes, checks, serial)?;
                if allocated(n, shapes).is_some()
                    && a.iter()
                        .any(|e| matches!(e,Expr::Call(tag,_) if tag=="@section_subscript"))
                {
                    bail!("sections of bounded allocatable arrays are not yet supported");
                }
                check(n, shapes, guards);
                for e in a {
                    expression(e, shapes, guards, checks, serial)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expression(a, shapes, guards, checks, serial)?;
                expression(b, shapes, guards, checks, serial)?;
            }
            Expr::Un(_, a) => expression(a, shapes, guards, checks, serial)?,
            _ => {}
        }
        Ok(())
    }
    fn lower(
        body: &mut Vec<Stmt>,
        inherited: &Shapes,
        serial: &mut usize,
        checks: &mut Vec<Stmt>,
    ) -> Result<()> {
        let mut shapes = if body.iter().any(|s|matches!(s,Stmt::DummyIntent{intent,..} if intent=="@native_recursive")) {Shapes::new()} else {inherited.clone()};
        for s in body.iter() {
            match s {
                Stmt::ArrayDecl { kind, name, dims } => {
                    shapes.remove(&format!("@extent:{name}"));
                    shapes.remove(&format!("@allocated:{name}"));
                    shapes.remove(&format!("@axes:{name}"));
                    shapes.remove(&format!("@axis_limits:{name}"));
                    shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        shapes.remove(&format!("@extent:{n}"));
                        shapes.remove(&format!("@allocated:{n}"));
                        shapes.remove(&format!("@axes:{n}"));
                        shapes.remove(&format!("@axis_limits:{n}"));
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
                Stmt::CallSub{name:operation,args} if operation=="@alloc_result_assign"=>{
                    let [Expr::Ident(name),value]=args.as_slice() else {bail!("invalid allocatable function result assignment");};
                    let name=name.clone();let mut value=value.clone();
                    if crate::array_calls::scalar_actual_kind(&value,&shapes).is_ok() {
                        let mut statements=vec![assign(&name,value)];
                        lower(&mut statements,&shapes,serial,checks)?;
                        out.extend(statements);continue;
                    }
                    let target_axes=axes(&name,&shapes).context("allocatable function result requires a heap descriptor")?;
                    let mut statements=vec![];
                    let rhs_axes=logical_shape(&value,&shapes,&mut statements)?.map(|v|v.0);
                    let source_lower=if let Expr::Ident(n)=&value {lowers(n,&shapes)} else {None};
                    expression(&mut value,&shapes,&mut statements,checks,serial)?;
                    if let Expr::Ident(n)=&value {
                        if shapes.get(n).is_some_and(|(_,d)|d.len()>1) {value=Expr::Call("@fixed_packed_view".into(),vec![id(n)]);}
                    }
                    let (kind,_,length)=crate::reductions::argument(&mut value,&shapes,&mut statements,serial)?;
                    let rhs_axes=rhs_axes.unwrap_or_else(||vec![length.clone()]);
                    if rhs_axes.len()!=target_axes.len() {bail!("allocatable function result assignment rank mismatch");}
                    *serial+=1;let stem=format!("@alloc_result_assignment_{serial}");
                    let snapshot=format!("{stem}_values");let index=format!("{stem}_index");
                    let mut extents=vec![];let mut bounds=vec![];
                    for (axis,extent) in rhs_axes.into_iter().enumerate() {
                        let n=format!("{stem}_extent_{axis}");
                        statements.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![n.clone()]});
                        statements.push(assign(&n,extent));extents.push(id(&n));
                        let lo=format!("{stem}_lower_{axis}");
                        statements.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![lo.clone()]});
                        statements.push(assign(&lo,source_lower.as_ref().map_or(int(1),|v|v[axis].clone())));
                        bounds.push(Expr::Call("@heap_bounds".into(),vec![id(&lo),bin(BinOp::Sub,bin(BinOp::Add,id(&lo),id(&n)),int(1))]));
                    }
                    let length=extents.iter().cloned().reduce(|a,b|bin(BinOp::Mul,a,b)).unwrap();
                    crate::heap::temporary(kind,snapshot.clone(),length.clone(),&mut statements);
                    statements.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                    let mut rhs_shapes=shapes.clone();
                    for s in &statements {match s {
                        Stmt::ArrayDecl{kind,name,dims}=>{rhs_shapes.insert(name.clone(),(kind.clone(),dims.clone()));},
                        Stmt::VarDecl{kind,names}=>for n in names {rhs_shapes.insert(n.clone(),(kind.clone(),vec![]));},
                        _=>{},
                    }}
                    metadata(&statements,&mut rhs_shapes);
                    let mut scalar=crate::reductions::argument_element(&value,&id(&index),&rhs_shapes)?;
                    fn flat_elements(e:&mut Expr,shapes:&Shapes) {
                        match e {
                            Expr::Index(n,a) if a.len()==1 && axes(n,shapes).is_some()=>{
                                *e=Expr::Call("@allocated_linear_element".into(),vec![id(n),a[0].clone()]);
                            },
                            Expr::Call(_,a)|Expr::Index(_,a)=>for e in a {flat_elements(e,shapes);},
                            Expr::Bin(_,a,b)=>{flat_elements(a,shapes);flat_elements(b,shapes);},
                            Expr::Un(_,a)=>flat_elements(a,shapes),_=>{},
                        }
                    }
                    flat_elements(&mut scalar,&rhs_shapes);
                    statements.push(Stmt::Do{var:index.clone(),start:int(1),end:length.clone(),body:vec![Stmt::AssignIndex{name:snapshot.clone(),indices:vec![id(&index)],value:scalar}]});
                    let flag=allocated(&name,&shapes).context("allocatable function result requires allocation state")?;
                    let mut resize=Expr::Un(UnOp::Not,Box::new(flag.clone()));
                    for (target,source) in target_axes.iter().zip(&extents) {resize=bin(BinOp::Or,resize,bin(BinOp::Ne,target.clone(),source.clone()));}
                    statements.push(Stmt::If{cond:resize,then_body:vec![
                        Stmt::If{cond:flag,then_body:vec![Stmt::CallSub{name:"@deallocate".into(),args:vec![id(&name)]}],else_body:None},
                        Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(name.clone(),bounds)]},
                    ],else_body:None});
                    let target_lowers=lowers(&name,&shapes).unwrap_or_else(||vec![int(1);extents.len()]);
                    let mut ordinal=bin(BinOp::Sub,id(&index),int(1));let mut indices=vec![];
                    for (extent,lower) in extents.iter().zip(target_lowers) {
                        indices.push(bin(BinOp::Add,lower,Expr::Call("mod".into(),vec![ordinal.clone(),extent.clone()])));
                        ordinal=bin(BinOp::Div,ordinal,extent.clone());
                    }
                    statements.push(Stmt::Do{var:index.clone(),start:int(1),end:length,body:vec![Stmt::AssignIndex{name,indices,value:Expr::Index(snapshot,vec![id(&index)])}]});
                    lower(&mut statements,&shapes,serial,checks)?;
                    out.push(Stmt::Block{body:statements});continue;
                }
                Stmt::CallSub { name, args } if name == "@deallocate" => {
                    for n in deallocation_targets(args)? {
                        if let Some(flag) = allocated(&n, &shapes) {
                            out.push(fail(
                                Expr::Un(UnOp::Not, Box::new(flag.clone())),
                                &format!("DEALLOCATE of unallocated array `{n}`"),
                            ));
                            let Expr::Ident(flag) = flag else {
                                bail!("DEALLOCATE requires writable allocation state");
                            };
                            out.push(assign(&flag, Expr::Logical(false)));
                            if let Some(Expr::Ident(len)) = length(&n, &shapes) {
                                out.push(assign(&len, int(0)));
                            }
                            if let Some(axes) = axes(&n, &shapes) {
                                for axis in axes {
                                    if let Expr::Ident(axis) = axis {
                                        out.push(assign(&axis, int(0)));
                                    }
                                }
                            }
                            if shapes.contains_key(&format!("@heap:{n}")) {out.push(Stmt::CallSub{name:"@heap_free".into(),args:vec![id(&n)]});}
                            out.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![id(&n)],
                            });
                        } else {
                            // Deferred-length scalar CHARACTER is lowered next.
                            out.push(Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![id(&n)],
                            });
                        }
                    }
                    continue;
                }
                Stmt::CallSub {name,..} if matches!(name.as_str(), "@module_bind" | "@heap_bind" | "@ptr_bind" | "@heap_drop" | "@heap_free") => {out.push(s);continue;}
                Stmt::CallSub{name,args} if name=="@heap_allocate" || name=="@heap_resize"=>{
                    for extent in args.iter_mut().skip(2){expression(extent,&shapes,&mut guards,checks,serial)?;}
                    out.extend(guards);out.push(s);continue;
                }
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
                        let Expr::Ident(len) = len else {
                            unreachable!()
                        };
                        let Expr::Ident(flag) = allocated(n, &shapes).unwrap() else {
                            unreachable!()
                        };
                        if let Some(axes) = axes(n, &shapes) {
                            if dims.len() != axes.len() {
                                bail!("ALLOCATE rank mismatch for `{n}`");
                            }
                            out.push(fail(
                                id(&flag),
                                &format!("array `{n}` is already allocated"),
                            ));
                            let limits = &shapes[&format!("@axis_limits:{n}")].1;
                            for (axis, ((state, extent), limit)) in
                                axes.iter().zip(dims).zip(limits).enumerate()
                            {
                                let Expr::Ident(state) = state else {
                                    unreachable!()
                                };
                                let mut extent = extent.clone();
                                if let Expr::Call(tag,bounds)=&mut extent {if tag=="@heap_bounds" {
                                    let lower=lowers(n,&shapes).context("explicit allocation bounds require runtime storage")?[axis].clone();
                                    for value in bounds.iter_mut(){expression(value,&shapes,&mut out,checks,serial)?;}
                                    out.push(Stmt::CallSub{name:"@heap_bounds".into(),args:vec![lower,id(state),bounds[0].clone(),bounds[1].clone()]});
                                }else{expression(&mut extent,&shapes,&mut out,checks,serial)?;out.push(assign(state,Expr::Call("max".into(),vec![int(0),extent])));}}
                                else{
                                    expression(&mut extent, &shapes, &mut out, checks, serial)?;
                                    out.push(assign(state,Expr::Call("max".into(), vec![int(0), extent])));
                                    if let Some(lower)=lowers(n,&shapes) {if let Expr::Ident(lower)=&lower[axis]{out.push(assign(lower,int(1)));}}
                                }
                                if !shapes.contains_key(&format!("@heap:{n}")) {out.push(fail(bin(BinOp::Gt,id(state),limit.clone()),&format!("allocation of `{n}` dimension {} exceeds its bounded capacity",axis+1)));}
                            }
                            if shapes.contains_key(&format!("@heap:{n}")) {
                                let mut args=vec![id(n),id(&len)];args.extend(axes);
                                out.push(Stmt::CallSub{name:"@heap_allocate".into(),args});
                            } else {out.push(assign(
                                &len,
                                axes.into_iter()
                                    .reduce(|a, b| bin(BinOp::Mul, a, b))
                                    .unwrap(),
                            ));}
                            out.push(assign(&flag, Expr::Logical(true)));
                            out.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![id(n)],
                            });
                            continue;
                        }
                        let [extent] = dims.as_slice() else {
                            bail!("ALLOCATABLE output currently supports rank one");
                        };
                        let mut extent = extent.clone();
                        expression(&mut extent, &shapes, &mut out, checks, serial)?;
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
                    if let Some(axes) = axes(name, &shapes) {
                        if let Some((rhs, _)) = logical_shape(value, &shapes, &mut out)? {
                            conform_axes(&axes, &rhs, &mut out)?;
                        }
                    }
                    expression(value, &shapes, &mut out, checks, serial)?;
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
                    if let Expr::Ident(array)=value {
                        if shapes.get(array).is_some_and(|(_,dims)|dims.len()>1) {
                            *value=Expr::Call("@fixed_packed_view".into(),vec![id(array)]);
                        }
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
                    if shapes.contains_key(&format!("@heap:{name}")) {crate::heap::temporary(kind,temp.clone(),len.clone(),&mut out);}else{out.push(Stmt::ArrayDecl {
                        kind,
                        name: temp.clone(),
                        dims,
                    });}
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
                Stmt::Assign { name, value } => {
                    if let Some((rhs, true)) = logical_shape(value, &shapes, &mut guards)? {
                        let lhs = &shapes
                            .get(name)
                            .context("undefined array assignment destination")?
                            .1;
                        if !lhs.is_empty() {
                            conform_axes(lhs, &rhs, &mut guards)?;
                        }
                    }
                    expression(value, &shapes, &mut guards, checks, serial)?;
                }
                Stmt::Return(Some(value)) => {
                    expression(value, &shapes, &mut guards, checks, serial)?
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    if indices.len()==1 && allocated(name,&shapes).is_some()
                        && matches!(shapes[name].0,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical) {
                        let section=match &indices[0] {
                            Expr::Section{lower,upper,stride,..}=>Some((lower.clone(),upper.clone(),stride.clone())),
                            Expr::Call(tag,args) if tag=="@section_subscript"=>match &args[0] {
                                Expr::Section{lower,upper,stride,..}=>Some((lower.clone(),upper.clone(),stride.clone())),_=>None,
                            },_=>None,
                        };
                        if let Some((lower_bound,upper,stride))=section {
                            if axes(name,&shapes).is_some_and(|d|d.len()!=1) {bail!("runtime section assignment currently requires rank one");}
                            let step=stride.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(1);
                            if step==0 {bail!("array section stride must not be zero");}
                            let len=length(name,&shapes).unwrap();
                            let lo=lower_bound.as_deref().cloned().unwrap_or_else(||if step>0 {int(1)} else {len.clone()});
                            let hi=upper.as_deref().cloned().unwrap_or_else(||if step>0 {len.clone()} else {int(1)});
                            *serial+=1;
                            let base=format!("@runtime_section_{serial}");
                            let first=format!("{base}_first");let count=format!("{base}_count");let index=format!("{base}_index");let temp=format!("{base}_values");
                            let mut statements=vec![fail(Expr::Un(UnOp::Not,Box::new(allocated(name,&shapes).unwrap())),&format!("assignment to unallocated array `{name}`")),Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![first.clone(),count.clone(),index.clone()]},
                                assign(&first,lo),assign(&count,Expr::Call("max".into(),vec![int(0),bin(BinOp::Div,bin(BinOp::Add,bin(BinOp::Sub,hi,id(&first)),int(step)),int(step))]))];
                            let mut rhs=value.clone();
                            expression(&mut rhs,&shapes,&mut statements,checks,serial)?;
                            let (element,shape,kind)=crate::reductions::elemental_element(&rhs,&id(&index),&shapes)?;
                            if shape.is_some() {
                                let dims=logical_shape(&rhs,&shapes,&mut statements)?.context("section assignment requires an array shape")?.0;
                                let extent=dims.into_iter().reduce(|a,b|bin(BinOp::Mul,a,b)).unwrap();
                                statements.push(fail(bin(BinOp::Ne,id(&count),extent.clone()),"array section assignment shapes do not conform"));
                                crate::heap::temporary(kind,temp.clone(),extent,&mut statements);
                                statements.push(Stmt::Do{var:index.clone(),start:int(1),end:id(&count),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![id(&index)],value:element}]});
                            } else {
                                statements.push(Stmt::VarDecl{kind,names:vec![temp.clone()]});statements.push(assign(&temp,rhs));
                            }
                            let rhs=if shape.is_some(){Expr::Index(temp,vec![id(&index)])}else{id(&temp)};
                            statements.push(Stmt::Do{var:index.clone(),start:int(1),end:id(&count),body:vec![Stmt::AssignIndex{name:name.clone(),indices:vec![bin(BinOp::Add,id(&first),bin(BinOp::Mul,bin(BinOp::Sub,id(&index),int(1)),int(step)))],value:rhs}]});
                            lower(&mut statements,&shapes,serial,checks)?;
                            out.extend(statements);continue;
                        }
                    }
                    linear_index(name, indices, &shapes, checks, serial)?;
                    if let Some(flag) = allocated(name, &shapes) {
                        guards.push(fail(
                            Expr::Un(UnOp::Not, Box::new(flag)),
                            &format!("assignment to unallocated array `{name}`"),
                        ));
                    }
                    for e in indices {
                        expression(e, &shapes, &mut guards, checks, serial)?;
                    }
                    expression(value, &shapes, &mut guards, checks, serial)?;
                }
                Stmt::CallSub { name, .. } if name == "@undefine" => {}
                Stmt::CallSub { name, args } if name == "random_number" && args.len() == 1 => {
                    if let Expr::Index(n, indices) | Expr::Call(n, indices) = &mut args[0] {
                        if axes(n, &shapes).is_some() {
                            linear_index(n, indices, &shapes, checks, serial)?;
                            guards.push(fail(
                                Expr::Un(UnOp::Not, Box::new(allocated(n, &shapes).unwrap())),
                                &format!("RANDOM_NUMBER destination `{n}` is unallocated"),
                            ));
                            for e in indices {
                                expression(e, &shapes, &mut guards, checks, serial)?;
                            }
                            out.extend(guards);
                            out.push(s);
                            continue;
                        }
                    }
                    let n = match &args[0] {
                        Expr::Ident(n) => Some(n.clone()),
                        Expr::Call(tag, a) if tag == "@packed_view" => {
                            if let Expr::Ident(n) = &a[0] {
                                Some(n.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(n) = n.filter(|n| length(n, &shapes).is_some()) {
                        if !matches!(shapes[&n].0, TypeSpec::Real | TypeSpec::DoublePrecision) {
                            bail!("RANDOM_NUMBER requires REAL or DOUBLE PRECISION storage");
                        }
                        out.push(fail(
                            Expr::Un(UnOp::Not, Box::new(allocated(&n, &shapes).unwrap())),
                            &format!("RANDOM_NUMBER destination `{n}` is unallocated"),
                        ));
                        *serial += 1;
                        let index = format!("@alloc_random_index_{serial}");
                        out.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(None),
                            names: vec![index.clone()],
                        });
                        out.push(Stmt::Do {
                            var: index.clone(),
                            start: int(1),
                            end: length(&n, &shapes).unwrap(),
                            body: vec![Stmt::CallSub {
                                name: "random_number".into(),
                                args: vec![Expr::Index(n, vec![id(&index)])],
                            }],
                        });
                        continue;
                    }
                    for e in args {
                        expression(e, &shapes, &mut guards, checks, serial)?;
                    }
                }
                Stmt::CallSub { args, .. } | Stmt::Print { items: args } | Stmt::Read { args } => {
                    for e in args {
                        expression(e, &shapes, &mut guards, checks, serial)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    expression(cond, &shapes, &mut guards, checks, serial)?;
                    lower(then_body, &shapes, serial, checks)?;
                    scoped(then_body, &shapes);
                    if let Some(b) = else_body {
                        lower(b, &shapes, serial, checks)?;
                        scoped(b, &shapes);
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    expression(start, &shapes, &mut guards, checks, serial)?;
                    expression(end, &shapes, &mut guards, checks, serial)?;
                    lower(body, &shapes, serial, checks)?;
                    scoped(body, &shapes);
                }
                Stmt::DoWhile { cond, body } => {
                    expression(cond, &shapes, &mut guards, checks, serial)?;
                    lower(body, &shapes, serial, checks)?;
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
                    scoped(body, &shapes);
                }
                Stmt::Block { body } => {
                    lower(body, &shapes, serial, checks)?;
                    // Give bounded BLOCK storage distinct names before the
                    // interpreter and native lowering flatten local storage.
                    if body.iter().any(|s|matches!(s,Stmt::DummyIntent {intent,..} if intent.starts_with("@bounded:"))) {
                        crate::subroutines::rename_allocation_block(body, serial)?;
                    }
                }
                Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Function { body, .. } => lower(body, &shapes, serial, checks)?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    expression(expr, &shapes, &mut guards, checks, serial)?;
                    for c in cases {
                        lower(&mut c.body, &shapes, serial, checks)?;
                        scoped(&mut c.body, &shapes);
                    }
                    if let Some(b) = default {
                        lower(b, &shapes, serial, checks)?;
                        scoped(b, &shapes);
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
    let mut checks = vec![];
    // Function specialization can already have lowered allocation blocks.
    // Continue its numbering when finishing the enclosing program.
    let mut serial=crate::subroutines::maximum_serial(&p.body,&["@alloc_"]);
    lower(&mut p.body, &Shapes::new(), &mut serial, &mut checks)?;
    p.body.extend(checks);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_specialized_blocks_preserves_outer_shape_storage() {
        // Model an already-specialized array result whose extent is captured
        // by a later FORALL allocation block. A restarted counter would rename
        // `length` to the outer extent name, making its initializer read itself.
        let p=Program {name:"allocation_names".into(),body:vec![
            Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec!["@alloc_block_1".into()]},
            assign("@alloc_block_1",int(3)),
            Stmt::Block {body:vec![
                Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec!["length".into()]},
                Stmt::VarDecl {kind:TypeSpec::Logical,names:vec!["allocated".into()]},
                assign("length",id("@alloc_block_1")),
                assign("allocated",Expr::Logical(true)),
                Stmt::ArrayDecl {kind:TypeSpec::Integer(None),name:"values".into(),dims:vec![int(3)]},
                Stmt::DummyIntent {name:"values".into(),intent:"@bounded:length:allocated".into()},
                Stmt::Print {items:vec![id("length")]},
            ]},
        ]};
        crate::initialized::instrument(finish(p).unwrap()).unwrap();
    }
}

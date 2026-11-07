// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Sequential F2023 REDUCE lowering. Table 11.1 note 1 permits reusing an
//! accumulator across iterations. Keep that accumulator separate from the
//! outside object, and combine only after the construct completes.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
mod locality;
mod calls;
mod captures;
pub(crate) mod parallel;

#[derive(Clone)]
struct Object {
    kind: TypeSpec,
    dims: Vec<Expr>,
    attrs: HashSet<String>,
    derived: bool,
    constant: Option<Expr>,
}
type Scope = HashMap<String, Object>;
fn declarations(body: &[Stmt], parent: &Scope) -> Scope {
    let mut scope = parent.clone();
    for s in body {
        let rules: Vec<(&TypeSpec, &[crate::ast::LetterRange])> = match s {
            Stmt::Implicit(crate::ast::Implicit::Rule {
                type_spec,
                letter_ranges,
            }) => vec![(type_spec, letter_ranges)],
            Stmt::Implicit(crate::ast::Implicit::Rules { rules }) => rules
                .iter()
                .map(|r| (&r.type_spec, r.letter_ranges.as_slice()))
                .collect(),
            _ => vec![],
        };
        for (kind, ranges) in rules {
            for range in ranges {
                for letter in range.start..=range.end.unwrap_or(range.start) {
                    scope.insert(
                        format!("@implicit:{letter}"),
                        Object {
                            kind: kind.clone(),
                            dims: vec![],
                            attrs: HashSet::new(),
                            derived: false, constant: None,
                        },
                    );
                }
            }
        }
    }
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    scope.insert(
                        n.clone(),
                        Object {
                            kind: kind.clone(),
                            dims: vec![],
                            attrs: HashSet::new(),
                            derived: false, constant: None,
                        },
                    );
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                scope.insert(
                    name.clone(),
                    Object {
                        kind: kind.clone(),
                        dims: dims.clone(),
                        attrs: HashSet::new(),
                        derived: false, constant: None,
                    },
                );
            }
            Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } => {
                for (n, value) in entities {
                    scope.insert(
                        n.clone(),
                        Object {
                            kind: kind.clone(),
                            dims: vec![],
                            attrs: attributes.iter().cloned().collect(),
                            derived: false, constant: if attributes.iter().any(|a|a=="parameter") {value.clone()}else{None},
                        },
                    );
                }
            }
            Stmt::DerivedDecl {
                names,
                dims,
                attributes,
                ..
            } => {
                for n in names {
                    scope.insert(
                        n.clone(),
                        Object {
                            kind: TypeSpec::Integer(None),
                            dims: dims.clone(),
                            attrs: attributes.iter().cloned().collect(),
                            derived: true, constant: None,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(o) = scope.get_mut(name) {
                o.attrs.insert(intent.clone());
            }
        }
    }
    scope
}
// Module association gives constants qualified identities. Make those values
// available to the early parallel proof. Mutable data is admitted only as
// read-only input by parallel::check; it retains its shared Fortran storage.
fn module_objects(program:&Program)->Scope {
    fn collect(body:&[Stmt],out:&mut Scope) {
        for stmt in body {
            if let Stmt::Module{body,..}=stmt {
                out.extend(declarations(body,&Scope::new()).into_iter().filter(|(n,_)|n.starts_with("@module$")));
                collect(body,out);
            }
        }
    }
    let mut out=Scope::new();
    collect(&program.body,&mut out);
    out
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn binary(op: &str) -> Option<BinOp> {
    Some(match op {
        "+" => BinOp::Add,
        "*" => BinOp::Mul,
        ".and." => BinOp::And,
        ".or." => BinOp::Or,
        ".eqv." => BinOp::Eqv,
        ".neqv." => BinOp::Neqv,
        _ => return None,
    })
}
fn combine(op: &str, a: Expr, b: Expr) -> Expr {
    match binary(op) {
        Some(op) => Expr::Bin(op, Box::new(a), Box::new(b)),
        None => Expr::Call(op.into(), vec![a, b]),
    }
}
fn identity(op: &str, kind: &TypeSpec, temporary: &str) -> Result<Expr> {
    let integer = matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_));
    let real = matches!(
        kind,
        TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision
    );
    let complex = matches!(kind, TypeSpec::Complex(_));
    let logical = matches!(kind, TypeSpec::Logical | TypeSpec::LogicalKind(_));
    let value = match op {
        "+" | "*" if integer || real || complex => int(if op == "+" { 0 } else { 1 }),
        ".and." | ".or." | ".eqv." | ".neqv." if logical => {
            Expr::Logical(matches!(op, ".and." | ".eqv."))
        }
        "iand" | "ior" | "ieor" if integer => int(if op == "iand" { -1 } else { 0 }),
        "min" | "max" if integer || real => {
            let huge = Expr::Call("huge".into(), vec![id(temporary)]);
            if op == "min" {
                huge
            } else {
                let negative = Expr::Un(crate::ast::UnOp::Neg, Box::new(huge));
                if integer {
                    Expr::sub(negative, int(1))
                } else {
                    negative
                }
            }
        }
        "+" | "*" | ".and." | ".or." | ".eqv." | ".neqv." | "iand" | "ior" | "ieor" | "min"
        | "max" => bail!("REDUCE({op}:...) is not suitable for {kind:?}"),
        _ => bail!("unsupported REDUCE operation `{op}`"),
    };
    Ok(value)
}
fn mentions(e: &Expr, names: &HashSet<String>) -> bool {
    let mut copy = e.clone();
    let mut found = false;
    visit_expr(&mut copy, &mut |e| {
        if let Expr::Ident(n)
        | Expr::Call(n, _)
        | Expr::Index(n, _)
        | Expr::Section { name: n, .. } = e
        {
            found |= names.contains(n);
        }
    });
    found
}
fn same(a: &Expr, b: &Expr) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}
// Only the reduction occurrence matching the LHS may mention any reduction
// object. In particular s=s+s and t=t+s are not valid reduction updates.
fn update(value: &Expr, target: &Expr, op: &str, names: &HashSet<String>) -> bool {
    match value {
        Expr::Bin(b, a, c) if binary(op).as_ref() == Some(b) => {
            same(a, target) && !mentions(c, names) || same(c, target) && !mentions(a, names)
        }
        Expr::Call(n, args) if n == op && binary(op).is_none() && args.len() >= 2 => {
            args.iter().filter(|a| same(a, target)).count() == 1
                && args
                    .iter()
                    .filter(|a| !same(a, target))
                    .all(|a| !mentions(a, names))
        }
        _ => false,
    }
}
fn rewrite(
    body: &mut [Stmt],
    ops: &HashMap<String, (String, String)>,
    mut offset: Option<usize>,
    depth: usize,
    loop_names: &[String],
) -> Result<()> {
    let names = ops.keys().cloned().collect();
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
            continue;
        }
        let result = (|| -> Result<()> {
            let target = match &*s {
                Stmt::Assign { name, .. } if ops.contains_key(name) => {
                    Some((name.clone(), id(name)))
                }
                Stmt::AssignIndex { name, indices, .. } if ops.contains_key(name) => Some((
                    name.clone(),
                    if indices.len() == 1 && matches!(&indices[0], Expr::Section { .. }) {
                        indices[0].clone()
                    } else {
                        Expr::Call(name.clone(), indices.clone())
                    },
                )),
                _ => None,
            };
            if let Some((name, target)) = target {
                let (op, temporary) = &ops[&name];
                let value = match &*s {
                    Stmt::Assign { value, .. } | Stmt::AssignIndex { value, .. } => value,
                    _ => unreachable!(),
                };
                if !update(value, &target, op, &names) {
                    bail!("invalid REDUCE update of `{name}`: expected matching `{op}` reduction assignment");
                }
                if let Stmt::AssignIndex { indices, .. } = &*s {
                    if indices.iter().any(|e| {
                        let mut e = e.clone();
                        // Parser sections retain the array's name alongside
                        // their bounds; that name is the designator, not an
                        // extra reference in a subscript expression.
                        visit_expr(&mut e, &mut |e| {
                            if let Expr::Section { name: n, .. } = e {
                                if n == &name {
                                    *n = String::new();
                                }
                            }
                        });
                        mentions(&e, &names)
                    }) {
                        bail!("REDUCE variable cannot appear in reduction subscripts");
                    }
                }
                match s {
                    Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                        *name = temporary.clone()
                    }
                    _ => {}
                }
                for e in expressions(s) {
                    visit_expr(e, &mut |e| match e {
                        Expr::Ident(n)
                        | Expr::Call(n, _)
                        | Expr::Index(n, _)
                        | Expr::Section { name: n, .. }
                            if n == &name =>
                        {
                            *n = temporary.clone()
                        }
                        _ => {}
                    });
                }
                return Ok(());
            }
            if let Stmt::Do { var, .. } = s {
                if names.contains(var) {
                    bail!("REDUCE variable `{var}` cannot be a DO index");
                }
            }
            for e in expressions(s) {
                if mentions(e, &names) {
                    bail!("REDUCE variable may only appear in its reduction assignment");
                }
            }
            if matches!(s, Stmt::Exit) && depth == 0 {
                bail!("EXIT cannot leave a DO CONCURRENT construct");
            }
            if matches!(s, Stmt::Return(_)) {
                bail!("RETURN cannot leave a DO CONCURRENT construct");
            }
            // Named exits need the enclosing construct stack; do not silently
            // accept a transfer that could bypass the final combination.
            if matches!(s,Stmt::CallSub{name,..} if name=="@named_exit") {
                bail!("named EXIT inside a REDUCE construct is not yet supported");
            }
            if let Stmt::CallSub { name, args } = s {
                if name == "@named_cycle"
                    && !matches!(args.first(),Some(Expr::Str(n)) if loop_names.contains(n))
                {
                    bail!("CYCLE cannot leave a DO CONCURRENT construct");
                }
            }
            let inner = depth
                + usize::from(
                    matches!(s, Stmt::Do { .. } | Stmt::DoWhile { .. })
                        || matches!(s,Stmt::Block{body} if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@do_step")),
                );
            let mut loop_names = loop_names.to_vec();
            if let Stmt::Block { body } = s {
                if let Some(Stmt::CallSub { name, args }) = body.first() {
                    if name == "@named_loop" {
                        if let Some(Expr::Str(n)) = args.first() {
                            loop_names.push(n.clone());
                        }
                    }
                }
            }
            for b in children(s) {
                rewrite(b, ops, offset, inner, &loop_names)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}

fn lower(
    args: Vec<Expr>,
    mut body: Vec<Stmt>,
    scope: &Scope,
    serial: &mut usize,
    offset: Option<usize>,
    loop_name: Option<&str>,
    implicit_none: bool,
) -> Result<Vec<Stmt>> {
    let mut args = args.into_iter();
    let Expr::Call(_, mut controls) = args.next().context("missing concurrent controls")? else {
        bail!("invalid concurrent controls")
    };
    let mut mask = args.next().context("missing concurrent mask")?;
    let mut args: Vec<_> = args.collect();
    let mut scope = scope.clone();
    let parallel = parallel::eligible(&controls, &mask, &body, &args, &scope, loop_name);
    let mut prefix = locality::prepare(
        &mut controls,
        &mut mask,
        &mut body,
        &mut args,
        &mut scope,
        serial,
        implicit_none,
    )?;
    let mut ops = HashMap::new();
    let mut suffix = vec![];
    let mut partials = vec![];
    let mut dynamic_arrays = vec![];
    let mut reduction_counts = vec![];
    for clause in args {
        let Expr::Call(tag, variables) = clause else {
            bail!("invalid reduction clause")
        };
        let op = tag
            .strip_prefix("@reduce:")
            .context("invalid reduction operation")?;
        for variable in variables {
            let Expr::Ident(n) = variable else {
                bail!("REDUCE requires a variable name")
            };
            if ops.contains_key(&n) {
                bail!("duplicate REDUCE variable `{n}`");
            }
            if controls
                .chunks(4)
                .any(|c| matches!(&c[0],Expr::Ident(index) if index==&n))
            {
                bail!("REDUCE variable `{n}` cannot be a concurrent index");
            }
            let object = scope.get(&n).with_context(|| {
                format!("REDUCE variable `{n}` requires a declared intrinsic type")
            })?;
            if object.derived {
                bail!("REDUCE variable `{n}` requires a declared intrinsic type");
            }
            for attr in [
                "parameter",
                "intent(in)",
                "in",
                "optional",
                "asynchronous",
                "volatile",
                "protected",
            ] {
                if object.attrs.contains(attr) {
                    bail!("REDUCE variable `{n}` cannot have the {attr} attribute");
                }
            }
            if object.attrs.contains("pointer") || (object.attrs.contains("allocatable") && object.dims.is_empty()) {
                bail!("REDUCE of POINTER or scalar ALLOCATABLE variable `{n}` is not yet supported");
            }
            let mut assumed_size=false;
            for dimension in &object.dims {
                let mut dimension=dimension.clone();
                visit_expr(&mut dimension,&mut |e| {
                    assumed_size |= matches!(e,Expr::Ident(n) if n=="@assumed_extent");
                });
            }
            if assumed_size { bail!("REDUCE variable `{n}` cannot be an assumed-size array"); }
            let constants = scope.iter().filter_map(|(n,o)| o.constant.as_ref().map(|e|(n.clone(),e.clone()))).collect::<HashMap<_,_>>();
            let dynamic = !object.dims.is_empty() && (object.attrs.contains("allocatable") || object.dims.iter().any(|d| {
                let bounds = match d { Expr::Call(tag,a) if tag=="@declared_bounds" => a.as_slice(), _ => std::slice::from_ref(d) };
                bounds.iter().any(|e|crate::parameters::evaluate(e,&constants).is_err())
            }));
            *serial += 1;
            let temporary = format!("@reduce_value_{serial}");
            let initial = if dynamic {
                let mold=format!("{temporary}_mold");
                prefix.push(Stmt::VarDecl{kind:object.kind.clone(),names:vec![mold.clone()]});
                prefix.push(set(&mold,identity(op,&object.kind,&mold)?));
                id(&mold)
            } else {identity(op, &object.kind, &temporary)?};
            prefix.push(if object.dims.is_empty() {
                Stmt::VarDecl {
                    kind: object.kind.clone(),
                    names: vec![temporary.clone()],
                }
            } else {
                Stmt::ArrayDecl {
                    kind: object.kind.clone(),
                    name: temporary.clone(),
                    dims: if dynamic { vec![id("@deferred_extent");object.dims.len()] } else { object.dims.clone() },
                }
            });
            if dynamic {
                prefix.push(Stmt::DummyIntent{name:temporary.clone(),intent:"allocatable".into()});
                let bounds=(1..=object.dims.len()).map(|axis|Expr::Call("@heap_bounds".into(),vec![Expr::Call("lbound".into(),vec![id(&n),int(axis as i64)]),Expr::Call("ubound".into(),vec![id(&n),int(axis as i64)])])).collect();
                prefix.push(Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(temporary.clone(),bounds)]});
                dynamic_arrays.push(temporary.clone());
            }
            if !object.dims.is_empty() {
                let count = Expr::Call("size".into(),vec![id(&temporary)]);
                // The native storage budget counts real component cells.
                reduction_counts.push(if matches!(object.kind, TypeSpec::Complex(_)) {
                    Expr::mul(Expr::Call("min".into(),vec![count,int(4097)]),int(2))
                } else { count });
            }
            prefix.push(set(&temporary, initial));
            suffix.push(set(&n, combine(op, id(&n), id(&temporary))));
            if parallel {
                partials.push((temporary.clone(), op.to_string(), object.kind.clone(), object.dims.clone()));
            }
            ops.insert(n, (op.to_string(), temporary));
        }
    }
    rewrite(
        &mut body,
        &ops,
        offset,
        0,
        &loop_name
            .map(str::to_string)
            .into_iter()
            .collect::<Vec<_>>(),
    )?;
    *serial += 1;
    let active = format!("@reduce_active_{serial}");
    prefix.push(Stmt::VarDecl {
        kind: TypeSpec::Logical,
        names: vec![active.clone()],
    });
    prefix.push(set(&active, Expr::Logical(false)));
    body.insert(0, set(&active, Expr::Logical(true)));
    // Evaluate every control once, before any iteration changes outside data.
    for c in controls.chunks_mut(4) {
        let kind = match &c[0] {
            Expr::Ident(n) => scope
                .get(n)
                .map(|o| o.kind.clone())
                .unwrap_or(TypeSpec::Integer(None)),
            _ => TypeSpec::Integer(None),
        };
        let last = if parallel && matches!(&c[3],Expr::IntLit(n) if n=="1") { 3 } else { 4 };
        for e in &mut c[1..last] {
            *serial += 1;
            let temporary = format!("@reduce_bound_{serial}");
            prefix.push(Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![temporary.clone()],
            });
            prefix.push(set(&temporary, e.clone()));
            *e = id(&temporary);
        }
    }
    let mut loop_stmt = if parallel && (controls.len() > 4 || !matches!(&controls[3],Expr::IntLit(n) if n=="1")) {
        parallel::flatten(controls, mask, body, &mut prefix, serial)
    } else {
        crate::ast::do_concurrent(controls, Some(mask), body)
    };
    // The runtime guard needs a marker-free copy for over-budget/empty arrays.
    // Capture it before adding worker metadata.
    let serial_loop = loop_stmt.clone();
    let parallel_prefix_start = prefix.len();
    let mut fold = vec![];
    let mut cleanup = vec![];
    if parallel {
        let mut metadata = vec![];
        if !partials.is_empty() {
            // The runtime caps the pool at 64. Initialize every slot so empty
            // chunks need no callback, and a backend serial fallback leaves
            // this fold inactive while retaining the ordinary accumulator.
            let worker = format!("@reduce_worker_{serial}");
            let flags = format!("@reduce_workers_active_{serial}");
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![worker.clone()] });
            prefix.push(Stmt::ArrayDecl { kind: TypeSpec::Logical, name: flags.clone(), dims: vec![int(64)] });
            prefix.push(set(&flags, Expr::Logical(false)));
            // Keep storage references as identifiers so procedure specialization
            // renames the marker together with its declarations and uses.
            metadata.extend([id(&active), id(&flags)]);
            let mut combines = vec![set(&active, Expr::Logical(true))];
            for (temporary, op, kind, dims) in partials {
                let array = format!("{temporary}_workers");
                metadata.extend([id(&temporary), id(&array)]);
                if dims.is_empty() {
                    prefix.push(Stmt::ArrayDecl { kind: kind.clone(), name: array.clone(), dims: vec![int(64)] });
                    prefix.push(set(&array, identity(&op, &kind, &temporary)?));
                    combines.push(set(&temporary, combine(&op, id(&temporary), Expr::Index(array, vec![id(&worker)]))));
                } else {
                    let count=Expr::Call("size".into(),vec![id(&temporary)]);
                    prefix.push(Stmt::ArrayDecl {kind:kind.clone(),name:array.clone(),dims:vec![id("@deferred_extent")]});
                    prefix.push(Stmt::DummyIntent{name:array.clone(),intent:"allocatable".into()});
                    prefix.push(Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(array.clone(),vec![Expr::mul(count.clone(),int(64))])]});
                    let mold=format!("{temporary}_identity");
                    prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![mold.clone()]});
                    prefix.push(set(&mold,identity(&op,&kind,&mold)?));
                    prefix.push(set(&array,id(&mold)));
                    let mut indices=vec![];
                    let mut bounds=vec![];
                    for (axis,_) in dims.iter().enumerate() {
                        *serial+=1;
                        let index=format!("@reduce_fold_{serial}");
                        prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]});
                        indices.push(id(&index));
                        bounds.push((index,Expr::Call("lbound".into(),vec![id(&temporary),int(axis as i64+1)]),Expr::Call("ubound".into(),vec![id(&temporary),int(axis as i64+1)])));
                    }
                    let mut offset=int(0);
                    let mut stride=int(1);
                    for (index,lo,hi) in &bounds {
                        offset=Expr::add(offset,Expr::mul(Expr::sub(id(index),lo.clone()),stride.clone()));
                        stride=Expr::mul(stride,Expr::add(Expr::sub(hi.clone(),lo.clone()),int(1)));
                    }
                    let slot=Expr::add(Expr::add(Expr::mul(Expr::sub(id(&worker),int(1)),count),offset),int(1));
                    let mut statements=vec![Stmt::AssignIndex{name:temporary.clone(),indices:indices.clone(),value:combine(&op,Expr::Index(temporary.clone(),indices),Expr::Index(array.clone(),vec![slot]))}];
                    for (index,lo,hi) in bounds {
                        statements=vec![Stmt::Do{var:index,start:lo,end:hi,body:statements}];
                    }
                    combines.extend(statements);
                    cleanup.push(Stmt::CallSub{name:"@deallocate".into(),args:vec![id(&array)]});
                }
            }
            fold.push(Stmt::Do { var: worker.clone(), start: int(1), end: int(64), body: vec![Stmt::If {
                cond: Expr::Index(flags, vec![id(&worker)]), then_body: combines, else_body: None,
            }] });
        }
        if let Stmt::Do { body, .. } = &mut loop_stmt {
            body.insert(0, Stmt::CallSub { name: "@parallel_iteration".into(), args: metadata });
        }
    }
    if parallel && !dynamic_arrays.is_empty() {
        // Clamp individual contributions before summing: a large array must
        // select serial execution without overflowing the budget calculation.
        let mut total=int(0);
        let mut condition=Expr::Logical(true);
        for count in reduction_counts {
            condition=Expr::Bin(BinOp::And,Box::new(condition),Box::new(Expr::Bin(BinOp::Gt,Box::new(count.clone()),Box::new(int(0)))));
            total=Expr::add(total,Expr::Call("min".into(),vec![count,int(4097)]));
        }
        condition=Expr::Bin(BinOp::And,Box::new(condition),Box::new(Expr::Bin(BinOp::Le,Box::new(total),Box::new(int(4096)))));
        let mut setup=prefix.split_off(parallel_prefix_start);
        setup.push(loop_stmt);
        setup.extend(fold);
        setup.extend(cleanup);
        prefix.push(Stmt::If{cond:condition,then_body:setup,else_body:Some(vec![serial_loop])});
    } else {
        prefix.push(loop_stmt);
        prefix.extend(fold);
        prefix.extend(cleanup);
    }
    prefix.push(Stmt::If {
        cond: id(&active),
        then_body: suffix,
        else_body: None,
    });
    for array in dynamic_arrays { prefix.push(Stmt::CallSub{name:"@deallocate".into(),args:vec![id(&array)]}); }
    Ok(prefix)
}
fn walk(
    body: &mut Vec<Stmt>,
    parent: &Scope,
    serial: &mut usize,
    mut offset: Option<usize>,
    loop_name: Option<&str>,
    implicit_none: bool,
) -> Result<()> {
    let scope = declarations(body, parent);
    let implicit_none = implicit_none
        || body
            .iter()
            .any(|s| matches!(s, Stmt::Implicit(crate::ast::Implicit::None)));
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
            continue;
        }
        // Lower before walking children, while the marker still owns its body.
        if let Stmt::Block { body } = s {
            if matches!(body.first(),Some(Stmt::CallSub{name,..}) if matches!(name.as_str(),"@concurrent_reduce"|"@do_concurrent"))
            {
                let Stmt::CallSub { args, .. } = body.remove(0) else {
                    unreachable!()
                };
                *body = lower(
                    args,
                    std::mem::take(body),
                    &scope,
                    serial,
                    offset,
                    loop_name,
                    implicit_none,
                )
                .map_err(|e| crate::source_diagnostics::at(e, offset))?;
            }
        }
        let child_name = match &*s {
            Stmt::Block { body } => match body.first() {
                Some(Stmt::CallSub { name, args }) if name == "@named_loop" => match args.first() {
                    Some(Expr::Str(n)) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let procedure_scope = if let Stmt::Function{params,..}|Stmt::Subroutine{params,..} = s {
            let mut child_scope=scope.clone();
            child_scope.retain(|n,_| !n.starts_with("@parallel_dummy:"));
            for object in child_scope.values_mut() {
                object.attrs.insert("@parallel_host".into());
            }
            let marker=Object {kind:TypeSpec::Integer(None),dims:vec![],attrs:HashSet::new(),derived:false,constant:None};
            child_scope.insert("@parallel_procedure".into(),marker.clone());
            for n in params {child_scope.insert(format!("@parallel_dummy:{n}"),marker.clone());}
            Some(child_scope)
        } else {None};
        for b in children(s) {
            walk(
                b,
                procedure_scope.as_ref().unwrap_or(&scope),
                serial,
                offset,
                child_name.as_deref(),
                implicit_none,
            )?;
        }
        // Keep a named-loop wrapper around the generated DO, not around the
        // declarations and final outside-value combination.
        if let Stmt::Block { body } = s {
            if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@named_loop")
                && body.len() == 2
            {
                if let Stmt::Block { body: generated } = &body[1] {
                    if generated.iter().any(|s|matches!(s,Stmt::VarDecl{names,..} if names.iter().any(|n|n.starts_with("@reduce_active_")))) {
                        let marker=body.remove(0);
                        let Stmt::Block{body:mut generated}=body.remove(0) else {unreachable!()};
                        let index=generated.len()-2;
                        let loop_stmt=generated.remove(index);
                        generated.insert(index,name_iteration(loop_stmt,marker));
                        *body=generated;
                    }
                }
            }
        }
    }
    Ok(())
}
// CYCLE of a multi-index concurrent construct advances to the next tuple,
// so the source construct name belongs to the innermost generated DO.
fn name_iteration(mut stmt: Stmt, marker: Stmt) -> Stmt {
    let is_loop = |s: &Stmt| {
        matches!(s, Stmt::Do { .. })
            || matches!(s,Stmt::Block{body} if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@do_step"))
    };
    let nested = match &mut stmt {
        Stmt::Do { body, .. } if body.len() == 1 && is_loop(&body[0]) => Some(&mut body[0]),
        Stmt::Block { body } if body.len() == 2 && is_loop(&body[1]) => Some(&mut body[1]),
        _ => None,
    };
    if let Some(child) = nested {
        let child_stmt = std::mem::replace(child, Stmt::Cycle);
        *child = name_iteration(child_stmt, marker);
        stmt
    } else {
        Stmt::Block {
            body: vec![marker, stmt],
        }
    }
}
pub fn resolve(mut program: Program) -> Result<Program> {
    if parallel::enabled() { captures::prepare(&mut program); }
    parallel::procedures(&program);
    let objects=if parallel::enabled(){module_objects(&program)}else{Scope::new()};
    walk(&mut program.body, &objects, &mut 0, None, None, false)?;
    parallel::mark_workspaces(&mut program);
    Ok(program)
}

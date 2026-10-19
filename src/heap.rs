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

/// Whether an assignment value may be an array, so that assigning it to an
/// unallocated ALLOCATABLE array allocates the target. Scalar values cannot.
fn array_valued(e: &Expr, arrays: &HashSet<String>) -> bool {
    match e {
        Expr::Ident(n) => arrays.contains(n),
        Expr::Section { .. } => true,
        Expr::Call(n, _) if n == "@array_constructor" => true,
        // A section or vector subscript of a local array.
        Expr::Call(n, args) if arrays.contains(n) => args.iter().any(|a| {
            matches!(a, Expr::Call(t, _) if t == "@section_subscript") || array_valued(a, arrays)
        }),
        Expr::Call(_, args) => args.iter().any(|a| array_valued(a, arrays)),
        Expr::Bin(_, a, b) => array_valued(a, arrays) || array_valued(b, arrays),
        Expr::Un(_, a) => array_valued(a, arrays),
        _ => false,
    }
}

fn array_assignment(body: &[Stmt], array: &str, declared: &HashSet<String>) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { name, value } => name == array && array_valued(value, declared),
        Stmt::If { then_body, else_body, .. } => array_assignment(then_body, array, declared)
            || else_body.as_ref().is_some_and(|b| array_assignment(b, array, declared)),
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => array_assignment(body, array, declared),
        Stmt::SelectCase { cases, default, .. } => cases.iter().any(|c| array_assignment(&c.body, array, declared))
            || default.as_ref().is_some_and(|b| array_assignment(b, array, declared)),
        _ => false,
    })
}

/// An ALLOCATABLE array dummy of a function that the function does not
/// allocate, deallocate or assign as a whole (an INTENT(IN) one cannot), and
/// whose allocation status and bounds it does not inquire or pass on, behaves
/// as an assumed-shape dummy. Treating it so lets such functions take
/// runtime-sized actuals, as the flattened ALLOCATABLE components of a derived
/// dummy do.
fn fixed_allocation_dummies(body: &mut [Stmt]) {
    fn passed(body: &[Stmt], name: &str) -> bool {
        body.iter().any(|s| match s {
            Stmt::CallSub { args, .. } => args.iter().any(|a| matches!(a, Expr::Ident(n) if n == name)),
            Stmt::If { then_body, else_body, .. } => passed(then_body, name) || else_body.as_ref().is_some_and(|b| passed(b, name)),
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => passed(body, name),
            Stmt::SelectCase { cases, default, .. } => cases.iter().any(|c| passed(&c.body, name)) || default.as_ref().is_some_and(|b| passed(b, name)),
            _ => false,
        })
    }
    // ALLOCATE, DEALLOCATE or whole-array assignment, which may reallocate.
    fn reallocated(body: &[Stmt], name: &str) -> bool {
        body.iter().any(|s| match s {
            Stmt::CallSub { name: n, args } if n == "@allocate" || n == "@deallocate" || n == "move_alloc" => args.iter()
                .any(|a| matches!(a, Expr::Ident(x) | Expr::Call(x, _) if x == name)),
            Stmt::Assign { name: n, .. } => n == name,
            Stmt::If { then_body, else_body, .. } => reallocated(then_body, name) || else_body.as_ref().is_some_and(|b| reallocated(b, name)),
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => reallocated(body, name),
            Stmt::SelectCase { cases, default, .. } => cases.iter().any(|c| reallocated(&c.body, name)) || default.as_ref().is_some_and(|b| reallocated(b, name)),
            _ => false,
        })
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { params, body, .. } => {
                let text = serde_json::to_string(&*body).unwrap_or_default();
                let has = |p: &str, attr: &str| body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == p && intent == attr));
                let inputs = params.iter().filter(|p| {
                    let ident = serde_json::to_string(&Expr::Ident(p.to_string())).unwrap_or_default();
                    has(p, "allocatable")
                        && body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == *p))
                        && ["allocated", "lbound", "ubound"].iter().all(|f| !text.contains(&format!("{{\"Call\":[\"{f}\",[{ident}")))
                        && !passed(body, p)
                        && (has(p, "intent(in)") || !reallocated(body, p))
                }).cloned().collect::<HashSet<_>>();
                body.retain(|s| !matches!(s, Stmt::DummyIntent { name, intent } if intent == "allocatable" && inputs.contains(name)));
                fixed_allocation_dummies(body);
            }
            Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => fixed_allocation_dummies(body),
            _ => {}
        }
    }
}
pub(crate) fn prepare(mut program: Program) -> Result<Program> {
    fixed_allocation_dummies(&mut program.body);
    fn numeric(kind: &TypeSpec) -> bool {
        matches!(
            kind,
            TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                | TypeSpec::Real
                | TypeSpec::DoublePrecision
                | TypeSpec::Logical
                | TypeSpec::Character(_)
                | TypeSpec::CharacterAssumed
        )
    }
    fn signatures(body: &mut [Stmt], result: &mut HashMap<String, Vec<usize>>) {
        for statement in body {
            match statement {
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let positions=params.iter().enumerate().filter_map(|(i,p)|
                    (body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,kind,dims} if name==p&&numeric(kind)&&(!matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed)||dims.len()>1 || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p && intent=="@tokenize_deferred_character")))))
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
        let declared: HashSet<String> = body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        fn allocation(body: &[Stmt], array: &str, declared: &HashSet<String>) -> bool {
            body.iter().any(|s|match s {
                Stmt::CallSub{name,args} if name=="@allocate"=>args.iter().any(|e|matches!(e,Expr::Call(n,dims) if n==array) || matches!(e,Expr::Section{name:n,..} if n==array)),
                Stmt::If{then_body,else_body,..}=>allocation(then_body,array,declared)||else_body.as_ref().is_some_and(|b|allocation(b,array,declared)),
                // Intrinsic assignment to the whole array also (re)allocates it.
                Stmt::Assign{name,value}=>name==array && array_valued(value,declared),
                Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body}=>allocation(body,array,declared),
                Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|allocation(&c.body,array,declared))||default.as_ref().is_some_and(|b|allocation(b,array,declared)),
                _=>false,
            })
        }
        fn allocate_statement(body: &[Stmt], array: &str) -> bool {
            body.iter().any(|s| match s {
                Stmt::CallSub{name,args} if name=="@allocate"=>args.iter().any(|e|matches!(e,Expr::Call(n,_)|Expr::Section{name:n,..} if n==array)),
                Stmt::If{then_body,else_body,..}=>allocate_statement(then_body,array)||else_body.as_ref().is_some_and(|b|allocate_statement(b,array)),
                Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body}=>allocate_statement(body,array),
                Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|allocate_statement(&c.body,array))||default.as_ref().is_some_and(|b|allocate_statement(b,array)),
                _=>false,
            })
        }
        // An ALLOCATE whose extents are not constant expressions may depend on
        // runtime values even after call-site specialization, so its array
        // needs runtime storage rather than a bound fixed at the call site.
        fn runtime_allocate(body: &[Stmt], array: &str) -> bool {
            body.iter().any(|s| match s {
                Stmt::CallSub{name,args} if name=="@allocate"=>args.iter().any(|e|matches!(e,Expr::Call(n,dims) if n==array
                    && dims.iter().any(|d|crate::parameters::integer_constant(d).is_err()))),
                Stmt::If{then_body,else_body,..}=>runtime_allocate(then_body,array)||else_body.as_ref().is_some_and(|b|runtime_allocate(b,array)),
                Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body}=>runtime_allocate(body,array),
                Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|runtime_allocate(&c.body,array))||default.as_ref().is_some_and(|b|runtime_allocate(b,array)),
                _=>false,
            })
        }
        let array_dummies = body
            .iter()
            .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name)));
        let arrays=body.iter().filter_map(|s|if let Stmt::ArrayDecl{kind,name,dims}=s {
            (dims.iter().all(|e|matches!(e,Expr::Ident(n) if n=="@deferred_extent"))
              && !params.contains(name)
              && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name && intent=="@heap_dummy"))
              && (passed.contains(name) || name.starts_with("@module$") || ((dims.len()>1 || !array_dummies || runtime_allocate(body,name)) && allocation(body,name,&declared))
                  // Rank-one locals of procedures with array dummies use bounded
                  // storage when ALLOCATEd; one allocated only by assignment needs
                  // runtime storage for its value's extent.
                  || (array_dummies && allocation(body,name,&declared) && !allocate_statement(body,name)) || dims.len()>1 && body.iter().any(|s|matches!(s,Stmt::CallSub{name:call,args} if call=="@deallocate"&&args.iter().any(|e|matches!(e,Expr::Ident(n) if n==name)))))
              && numeric(kind) && (!matches!(kind,TypeSpec::Character(_)) || dims.len()>1 || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name && intent=="@tokenize_deferred_character"))
                  || array_assignment(body,name,&declared)
                  || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name&&intent=="@automatic")))
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
            Stmt::Assign{name,value} if tracked.contains(name) && array_valued(value,&declared)=>{
                crate::array_calls::check_allocated(&Stmt::Print{items:vec![value.clone()]},&pending).map_err(|e|crate::source_diagnostics::at(e,offset))?;
                pending.remove(name);
            }
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
                Stmt::Function { name, body, params, .. } => {
                    let result=match body.last() {Some(Stmt::Return(Some(Expr::Ident(n))))=>n.clone(),_=>name.clone()};
                    if body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,kind,..} if name==&result && matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical)))
                        && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&result && intent=="allocatable"))
                        && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&result && intent=="@heap_dummy")) {
                        // The hidden output dummy borrows caller-owned result
                        // storage. Keep its original rank and ALLOCATABLE
                        // declaration until function-call lowering supplies it.
                        body.insert(0,Stmt::DummyIntent{name:result,intent:"@heap_dummy".into()});
                    }
                    visit(body, params, interfaces)?
                }
                Stmt::Subroutine { body, params, .. } => {
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

/// Automatic arrays such as `real :: w(n)` in procedures whose bounds depend
/// on scalar dummies become ALLOCATABLE locals allocated on entry. Procedures
/// with array, CHARACTER or POINTER dummies, and ELEMENTAL procedures, are
/// specialized per call and allocate their automatic arrays in `automatic`.
pub(crate) fn automatic_locals(mut program: Program) -> Result<Program> {
    fn specialized(params: &[String], body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::ArrayDecl { name, .. } => params.contains(name),
            Stmt::VarDecl { kind: TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed, names } => {
                names.iter().any(|n| params.contains(n))
            }
            Stmt::DummyIntent { name, intent } => intent == "pointer" && params.contains(name),
            _ => false,
        })
    }
    fn declaration(s: &Stmt) -> bool {
        matches!(s, Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } | Stmt::DerivedType { .. }
            | Stmt::DerivedDecl { .. } | Stmt::Accessibility { .. } | Stmt::DummyIntent { .. } | Stmt::VarDecl { .. }
            | Stmt::InitializedDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::Use { .. } | Stmt::Implicit(_))
    }
    fn lower(params: &[String], result: &str, body: &mut Vec<Stmt>) {
        let attributes: HashSet<&str> = body.iter().filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if matches!(intent.as_str(), "allocatable" | "pointer" | "parameter" | "save" | "target" | "@heap") => Some(name.as_str()),
            _ => None,
        }).collect();
        let mut automatic = vec![];
        for s in body.iter() {
            let Stmt::ArrayDecl { kind, name, dims } = s else { continue };
            if params.contains(name) || name == result || name.starts_with('@') || attributes.contains(name.as_str())
                || matches!(kind, TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed)
                || dims.iter().any(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent" || n == "@assumed_extent"))
                || dims.iter().all(|d| crate::vector_subscripts::fixed_extent(d).is_some()) {
                continue;
            }
            automatic.push((name.clone(), dims.clone()));
        }
        if automatic.is_empty() {
            return;
        }
        let mut allocations = vec![];
        for s in body.iter_mut() {
            if let Stmt::ArrayDecl { name, dims, .. } = s {
                if let Some((_, bounds)) = automatic.iter().find(|(n, _)| n == name) {
                    *dims = vec![Expr::Ident("@deferred_extent".into()); bounds.len()];
                }
            }
        }
        for (name, bounds) in &automatic {
            let subscripts = bounds.iter().map(|d| match d {
                Expr::Call(tag, a) if tag == "@declared_bounds" => Expr::Call("@section_subscript".into(), vec![Expr::Section {
                    name: name.clone(),
                    lower: Some(Box::new(a[0].clone())),
                    upper: Some(Box::new(a[1].clone())),
                    stride: None,
                }]),
                d => d.clone(),
            }).collect();
            allocations.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(name.clone(), subscripts)] });
        }
        // After the leading specification statements; source markers may be interleaved.
        let mut at = 0;
        for (i, s) in body.iter().enumerate() {
            if declaration(s) {
                at = i + 1;
            } else if !matches!(s, Stmt::SourceLocation(_)) {
                break;
            }
        }
        let mut tail = body.split_off(at);
        for (name, _) in &automatic {
            body.push(Stmt::DummyIntent { name: name.clone(), intent: "allocatable".into() });
            // Rank-one CHARACTER locals otherwise use bounded storage.
            body.push(Stmt::DummyIntent { name: name.clone(), intent: "@automatic".into() });
        }
        body.extend(allocations);
        body.append(&mut tail);
    }
    fn visit(body: &mut Vec<Stmt>) {
        for s in body.iter_mut() {
            match s {
                Stmt::Subroutine { elemental, params, body, .. } => {
                    if !*elemental && !specialized(params, body) {
                        lower(params, "", body);
                    }
                    visit(body);
                }
                Stmt::Function { elemental, name, params, body, .. } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                        _ => name.clone(),
                    };
                    if !*elemental && !specialized(params, body) {
                        lower(params, &result, body);
                    }
                    visit(body);
                }
                Stmt::Module { body, .. } => visit(body),
                _ => {}
            }
        }
    }
    visit(&mut program.body);
    Ok(program)
}

/// Automatic arrays of derived type, such as `type(point) :: p(n)`, become
/// ALLOCATABLE arrays allocated on entry. This runs before derived-type
/// lowering splits them into component arrays, so a bound is treated as
/// runtime when it refers to a dummy argument.
pub(crate) fn automatic_derived(mut program: Program) -> Result<Program> {
    fn uses(e: &Expr, params: &[String]) -> bool {
        match e {
            Expr::Ident(n) => params.contains(n),
            Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(|a| uses(a, params)),
            Expr::Bin(_, a, b) => uses(a, params) || uses(b, params),
            Expr::Un(_, a) => uses(a, params),
            _ => false,
        }
    }
    fn lower(params: &[String], result: &str, body: &mut Vec<Stmt>, enumerations: &HashSet<String>) {
        let mut allocations = vec![];
        for s in body.iter_mut() {
            let Stmt::DerivedDecl { type_name, names, attributes, dims, .. } = s else { continue };
            // Enumeration storage is erased to INTEGER after nominal checks.
            // Let ordinary automatic-array lowering allocate it at that point.
            if enumerations.contains(type_name) {continue;}
            if dims.is_empty() || !dims.iter().any(|d| uses(d, params))
                || names.iter().any(|n| params.contains(n) || n == result)
                || attributes.iter().any(|a| matches!(a.as_str(), "allocatable" | "pointer" | "save" | "target") || a.starts_with("intent(")) {
                continue;
            }
            for name in names.iter() {
                let subscripts = dims.iter().map(|d| match d {
                    Expr::Call(tag, a) if tag == "@declared_bounds" => Expr::Call("@section_subscript".into(), vec![Expr::Section {
                        name: name.clone(),
                        lower: Some(Box::new(a[0].clone())),
                        upper: Some(Box::new(a[1].clone())),
                        stride: None,
                    }]),
                    d => d.clone(),
                }).collect();
                allocations.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(name.clone(), subscripts)] });
            }
            *dims = vec![Expr::Ident("@deferred_extent".into()); dims.len()];
            attributes.push("allocatable".into());
        }
        if allocations.is_empty() {
            return;
        }
        let declaration = |s: &Stmt| matches!(s, Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } | Stmt::DerivedType { .. }
            | Stmt::DerivedDecl { .. } | Stmt::Accessibility { .. } | Stmt::DummyIntent { .. } | Stmt::VarDecl { .. }
            | Stmt::InitializedDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::Use { .. } | Stmt::Implicit(_));
        let mut at = 0;
        for (i, s) in body.iter().enumerate() {
            if declaration(s) {
                at = i + 1;
            } else if !matches!(s, Stmt::SourceLocation(_)) {
                break;
            }
        }
        let tail = body.split_off(at);
        body.extend(allocations);
        body.extend(tail);
    }
    fn visit(body: &mut Vec<Stmt>, enumerations: &HashSet<String>) {
        for s in body.iter_mut() {
            match s {
                Stmt::Subroutine { params, body, .. } => {
                    lower(params, "", body, enumerations);
                    visit(body, enumerations);
                }
                Stmt::Function { name, params, body, .. } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                        _ => name.clone(),
                    };
                    lower(params, &result, body, enumerations);
                    visit(body, enumerations);
                }
                Stmt::Module { body, .. } => visit(body, enumerations),
                _ => {}
            }
        }
    }
    fn enumeration_types(body: &[Stmt], names: &mut HashSet<String>) {
        for stmt in body {
            if let Stmt::DerivedType{name,bindings,..}=stmt {
                if bindings.iter().any(|(n,_)|n=="@enumeration") {names.insert(name.clone());}
            }
            let mut copy=stmt.clone();
            for child in crate::procedure_arguments::children(&mut copy) {enumeration_types(child,names);}
        }
    }
    let mut enumerations=HashSet::new();
    enumeration_types(&program.body,&mut enumerations);
    visit(&mut program.body, &enumerations);
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

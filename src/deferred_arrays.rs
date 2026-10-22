// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower a deferred-length CHARACTER array, `character(len=:), allocatable ::
//! a(:)`, to an ALLOCATABLE array of fixed length when every assignment and
//! ALLOCATE in its program unit gives it the same constant length. Assignment
//! of a constructor allocates the array to the constructor's size first.
use crate::ast::{Expr, Stmt, TypeSpec};

/// The constant CHARACTER length of an array value, and its size when known.
fn shape(e: &Expr) -> Option<(usize, Option<usize>)> {
    match e {
        Expr::Call(n, items) if n == "@typed_constructor" => {
            let Some(Expr::Str(kind)) = items.first() else { return None };
            let TypeSpec::Character(Some(length)) = serde_json::from_str(kind).ok()? else { return None };
            let plain = items[1..].iter().all(|i| !matches!(i, Expr::Call(n, _) if n == "@output_implied_do"));
            Some((length, plain.then_some(items.len() - 1)))
        }
        Expr::Call(n, items) if n == "@array_constructor" && !items.is_empty() => {
            let lengths = items.iter().map(|i| match i { Expr::Str(s) => Some(s.len()), _ => None }).collect::<Option<Vec<_>>>()?;
            lengths.iter().all(|l| *l == lengths[0]).then_some((lengths[0], Some(items.len())))
        }
        // Elemental MAX and MIN of CHARACTER arrays have the longest length.
        Expr::Call(n, args) if matches!(n.as_str(), "max" | "min") && !args.is_empty() => {
            let shapes = args.iter().map(shape).collect::<Option<Vec<_>>>()?;
            Some((shapes.iter().map(|s| s.0).max()?, shapes[0].1))
        }
        _ => None,
    }
}

/// Every assignment of `name`, and every ALLOCATE of it with a type
/// specification, with the length each gives and its source offset.
type Definition = (Option<usize>, Option<usize>);
fn definitions(body: &[Stmt], name: &str, mut at: Option<usize>, lengths: &mut Vec<Definition>) {
    for s in body {
        match s {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::Assign { name: n, value } if n == name => lengths.push((shape(value).map(|s| s.0), at)),
            Stmt::CallSub { name: n, args } if n == crate::substrings::ALLOCATE_CHARACTER
                && args[1..].iter().any(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if o == name)) =>
            {
                lengths.push((crate::parameters::integer_constant(&args[0]).ok().and_then(|l| usize::try_from(l.max(0)).ok()), at));
            }
            Stmt::If { then_body, else_body, .. } => {
                definitions(then_body, name, at, lengths);
                if let Some(b) = else_body { definitions(b, name, at, lengths); }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => definitions(body, name, at, lengths),
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { definitions(&c.body, name, at, lengths); }
                if let Some(b) = default { definitions(b, name, at, lengths); }
            }
            _ => {}
        }
    }
}

/// Allocate before assigning a constructor, and turn ALLOCATE with a type
/// specification of the arrays into a plain ALLOCATE.
fn rewrite(body: &mut Vec<Stmt>, arrays: &[String], allocated: &mut std::collections::HashSet<String>) {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        match &mut s {
            Stmt::Assign { name, value } if arrays.contains(name) => {
                if let Some((_, Some(size))) = shape(value) {
                    // CHARACTER arrays do not yet support ALLOCATED or a
                    // conditional DEALLOCATE, so the first assignment in
                    // statement order allocates, and later ones reallocate.
                    if !allocated.insert(name.clone()) {
                        out.push(Stmt::CallSub { name: "@deallocate".into(), args: vec![Expr::Ident(name.clone())] });
                    }
                    out.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(name.clone(), vec![Expr::IntLit(size.to_string())])] });
                }
            }
            Stmt::CallSub { name, args } if name == crate::substrings::ALLOCATE_CHARACTER => {
                let (mine, others): (Vec<Expr>, Vec<Expr>) = args[1..].iter().cloned()
                    .partition(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if arrays.contains(o)));
                if !mine.is_empty() {
                    out.push(Stmt::CallSub { name: "@allocate".into(), args: mine.into_iter().map(|a| match a {
                        Expr::Index(n, d) => Expr::Call(n, d), a => a }).collect() });
                    if others.is_empty() { continue; }
                    let mut kept = vec![args[0].clone()];
                    kept.extend(others);
                    *args = kept;
                }
            }
            Stmt::If { then_body, else_body, .. } => {
                rewrite(then_body, arrays, allocated);
                if let Some(b) = else_body { rewrite(b, arrays, allocated); }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => rewrite(body, arrays, allocated),
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { rewrite(&mut c.body, arrays, allocated); }
                if let Some(b) = default { rewrite(b, arrays, allocated); }
            }
            _ => {}
        }
        out.push(s);
    }
    *body = out;
}

/// The name of the variable holding the current length of the elements of
/// a deferred-length CHARACTER array whose length is set at run time.
pub(crate) fn width(name: &str) -> String {
    format!("@deferred_width${name}")
}

/// The items of a constructor, each the name of a whole array declared in
/// the program unit or None for a scalar, when no item is an array section,
/// implied DO or nested constructor.
fn items(items: &[Expr], declared: &[String]) -> Option<Vec<Option<String>>> {
    items.iter().map(|i| match i {
        Expr::Ident(n) => Some(declared.contains(n).then(|| n.clone())),
        Expr::Section { name: n, .. } => (!declared.contains(n)).then_some(None),
        Expr::Call(n, a) | Expr::Index(n, a) if declared.contains(n) => a.iter().all(|a| !matches!(a, Expr::Section { .. })).then_some(None),
        Expr::Call(n, _) => (n != "@output_implied_do" && n != "@array_constructor").then_some(None),
        _ => Some(None),
    }).collect()
}

/// The constructor items of a value, after the type of a typed constructor.
fn constructor(e: &Expr) -> Option<&[Expr]> {
    match e {
        Expr::Call(n, items) if n == "@typed_constructor" && !items.is_empty() => Some(&items[1..]),
        Expr::Call(n, items) if n == "@array_constructor" => Some(items),
        _ => None,
    }
}

/// The length and size of a value assigned to a deferred-length CHARACTER
/// array whose length is set at run time, when they can be determined.
fn runtime_shape(e: &Expr, arrays: &[String], declared: &[String]) -> Option<(Expr, Expr)> {
    let int = |n: usize| Expr::IntLit(n.to_string());
    let size = |n: &str| Expr::Call("size".into(), vec![Expr::Ident(n.into())]);
    let length = |e: &Expr| match e {
        Expr::Str(s) => int(s.len()),
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) if arrays.contains(n) => Expr::Ident(width(n)),
        _ => Expr::Call("len".into(), vec![e.clone()]),
    };
    if let Some(values) = constructor(e) {
        let plan = items(values, declared)?;
        let count = plan.iter().map(|p| p.as_deref().map_or(int(1), size))
            .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Add, Box::new(a), Box::new(b)))?;
        let typed = match e {
            Expr::Call(n, items) if n == "@typed_constructor" => {
                let Some(Expr::Str(kind)) = items.first() else { return None };
                Some(match serde_json::from_str(kind).ok()? {
                    TypeSpec::Character(Some(length)) => int(length),
                    TypeSpec::CharacterLength(e) => *e,
                    _ => return None,
                })
            }
            _ => None,
        };
        // The elements of an untyped constructor have the same length.
        return Some((typed.unwrap_or_else(|| length(&values[0])), count));
    }
    match e {
        Expr::Ident(n) => Some((length(e), size(n))),
        _ => shape(e).and_then(|(length, size)| size.map(|size| (int(length), int(size)))),
    }
}

/// Set the length and allocate a deferred-length CHARACTER array whose
/// length is set at run time: ALLOCATE with a type specification sets the
/// length, and assignment of a whole array value reallocates the array to
/// the value's length and size.
fn reallocate(body: &mut Vec<Stmt>, arrays: &[String], declared: &[String]) {
    let id = |n: &str| Expr::Ident(n.into());
    let set_width = |name: &str, length: Expr| Stmt::Assign {
        name: width(name),
        value: Expr::Call("max".into(), vec![length, Expr::IntLit("0".into())]),
    };
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        match &mut s {
            // A value that refers to the array is first assigned to a
            // temporary, since the array is deallocated before assignment.
            Stmt::Assign { name, value } if arrays.contains(name)
                && serde_json::to_string(&*value).is_ok_and(|v| v.contains(&format!("\"{name}\""))) =>
            {
                let temp = format!("@deferred_copy${name}");
                let mut block = vec![
                    Stmt::DummyIntent { name: temp.clone(), intent: "allocatable".into() },
                    Stmt::ArrayDecl { kind: crate::ast::character_length(id("@deferred_character")), name: temp.clone(), dims: vec![id("@deferred_extent")] },
                    Stmt::Assign { name: temp.clone(), value: std::mem::replace(value, id(&temp)) },
                    s,
                ];
                let mut inner = arrays.to_vec();
                inner.push(temp);
                reallocate(&mut block, &inner, declared);
                out.extend(block);
                continue;
            }
            Stmt::Assign { name, value } if arrays.contains(name) => {
                if let Some((length, size)) = runtime_shape(value, arrays, declared) {
                    let temp = format!("@deferred_value${name}");
                    out.push(Stmt::Block { body: vec![] });
                    let Some(Stmt::Block { body: block }) = out.last_mut() else { unreachable!() };
                    let (length_name, size_name) = (format!("{temp}_length"), format!("{temp}_size"));
                    block.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![length_name.clone(), size_name.clone()] });
                    block.push(Stmt::Assign { name: length_name.clone(), value: length });
                    block.push(Stmt::Assign { name: size_name.clone(), value: size });
                    block.push(Stmt::If {
                        cond: Expr::Call("allocated".into(), vec![id(name)]),
                        then_body: vec![Stmt::CallSub { name: "@deallocate".into(), args: vec![id(name)] }],
                        else_body: None,
                    });
                    block.push(set_width(name, id(&length_name)));
                    block.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(name.clone(), vec![id(&size_name)])] });
                    // A constructor with whole-array items, or whose length is
                    // not known at compile time, is assigned one element at a
                    // time.
                    let plan = constructor(value).and_then(|v| items(v, declared));
                    match (constructor(value), plan) {
                        (Some(values), Some(plan)) if shape(value).is_none() || plan.iter().any(Option::is_some) => {
                            let (position, index) = (format!("{temp}_position"), format!("{temp}_index"));
                            block.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![position.clone(), index.clone()] });
                            block.push(Stmt::Assign { name: position.clone(), value: Expr::IntLit("0".into()) });
                            let next = Stmt::Assign { name: position.clone(), value: Expr::Bin(crate::ast::BinOp::Add, Box::new(id(&position)), Box::new(Expr::IntLit("1".into()))) };
                            for (item, whole) in values.iter().zip(plan) {
                                let Some(array) = whole else {
                                    block.push(next.clone());
                                    block.push(Stmt::AssignIndex { name: name.clone(), indices: vec![id(&position)], value: item.clone() });
                                    continue;
                                };
                                let bound = |b: &str| Expr::Call(b.into(), vec![id(&array), Expr::IntLit("1".into())]);
                                block.push(Stmt::Do { var: index.clone(), start: bound("lbound"), end: bound("ubound"), body: vec![
                                    next.clone(),
                                    Stmt::AssignIndex { name: name.clone(), indices: vec![id(&position)], value: Expr::Index(array.clone(), vec![id(&index)]) },
                                ] });
                            }
                        }
                        _ => block.push(s),
                    }
                    continue;
                }
            }
            Stmt::CallSub { name, args } if name == crate::substrings::ALLOCATE_CHARACTER => {
                let (mine, others): (Vec<Expr>, Vec<Expr>) = args[1..].iter().cloned()
                    .partition(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if arrays.contains(o)));
                if !mine.is_empty() {
                    for a in &mine {
                        let (Expr::Call(o, _) | Expr::Index(o, _)) = a else { unreachable!() };
                        out.push(set_width(o, args[0].clone()));
                    }
                    out.push(Stmt::CallSub { name: "@allocate".into(), args: mine.into_iter().map(|a| match a {
                        Expr::Index(n, d) => Expr::Call(n, d), a => a }).collect() });
                    if others.is_empty() { continue; }
                    let mut kept = vec![args[0].clone()];
                    kept.extend(others);
                    *args = kept;
                }
            }
            Stmt::If { then_body, else_body, .. } => {
                reallocate(then_body, arrays, declared);
                if let Some(b) = else_body { reallocate(b, arrays, declared); }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => reallocate(body, arrays, declared),
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { reallocate(&mut c.body, arrays, declared); }
                if let Some(b) = default { reallocate(b, arrays, declared); }
            }
            _ => {}
        }
        out.push(s);
    }
    *body = out;
}

/// Lower the deferred-length CHARACTER arrays of a program unit and of the
/// units it contains. An error carries the source offset of the definition
/// whose length differs.
pub(crate) fn resolve(body: &mut Vec<Stmt>) -> Result<(), (String, Option<usize>)> {
    let declared: Vec<String> = body.iter().filter_map(|s| match s {
        Stmt::ArrayDecl { kind, name, .. } if crate::substrings::deferred(kind) => Some(name.clone()),
        _ => None,
    }).collect();
    let mut lowered = vec![];
    let mut runtime = vec![];
    for name in &declared {
        let mut lengths = vec![];
        definitions(body, name, None, &mut lengths);
        let Some((Some(first), _)) = lengths.first().copied() else { runtime.push(name.clone()); continue };
        if lengths.iter().any(|l| l.0 != Some(first)) {
            runtime.push(name.clone());
            continue;
        }
        for s in body.iter_mut() {
            if let Stmt::ArrayDecl { kind, name: n, .. } = s {
                if n == name { *kind = TypeSpec::Character(Some(first)); }
            }
        }
        lowered.push(name.clone());
    }
    if !lowered.is_empty() {
        rewrite(body, &lowered, &mut Default::default());
    }
    if !runtime.is_empty() {
        let declared: Vec<String> = body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        reallocate(body, &runtime, &declared);
    }
    for s in body.iter_mut() {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } | Stmt::Block { body } = s {
            resolve(body)?;
        }
    }
    Ok(())
}

/// The type parameter inquiries `x%len` and `x%kind` of an intrinsic-type
/// variable are LEN(x) and KIND(x). They are rewritten unless some derived
/// type has a component of that name.
pub(crate) fn type_parameter_inquiries(body: &mut Vec<Stmt>) -> Result<(), String> {
    use serde_json::{json, Value};
    fn components(body: &[Stmt], names: &mut std::collections::HashSet<String>) {
        for s in body {
            match s {
                Stmt::DerivedType { components: c, .. } => for c in c {
                    match c {
                        Stmt::VarDecl { names: n, .. } | Stmt::DerivedDecl { names: n, .. } => names.extend(n.iter().cloned()),
                        Stmt::InitializedDecl { entities, .. } => names.extend(entities.iter().map(|(n, _)| n.clone())),
                        Stmt::ArrayDecl { name, .. } => { names.insert(name.clone()); }
                        _ => {}
                    }
                },
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => components(body, names),
                _ => {}
            }
        }
    }
    let mut taken = std::collections::HashSet::new();
    components(body, &mut taken);
    let inquiries: Vec<&str> = ["len", "kind"].into_iter().filter(|n| !taken.contains(*n)).collect();
    if inquiries.is_empty() { return Ok(()); }
    fn visit(v: &mut Value, inquiries: &[&str]) {
        match v {
            Value::Object(map) => {
                if let Some(Value::String(name)) = map.get("Ident") {
                    if let Some((object, inquiry)) = name.rsplit_once('%') {
                        if inquiries.contains(&inquiry) {
                            *v = json!({"Call": [inquiry, [{"Ident": object}]]});
                            return;
                        }
                    }
                }
                for (key, value) in map.iter_mut() {
                    if key != "Str" { visit(value, inquiries); }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|i| visit(i, inquiries)),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(&*body).map_err(|e| e.to_string())?;
    visit(&mut value, &inquiries);
    *body = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(())
}

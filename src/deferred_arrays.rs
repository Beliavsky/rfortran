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
/// specification, with the length each gives.
fn definitions(body: &[Stmt], name: &str, lengths: &mut Vec<Option<usize>>) {
    for s in body {
        match s {
            Stmt::Assign { name: n, value } if n == name => lengths.push(shape(value).map(|s| s.0)),
            Stmt::CallSub { name: n, args } if n == crate::substrings::ALLOCATE_CHARACTER
                && args[1..].iter().any(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if o == name)) =>
            {
                lengths.push(crate::parameters::integer_constant(&args[0]).ok().and_then(|l| usize::try_from(l.max(0)).ok()));
            }
            Stmt::If { then_body, else_body, .. } => {
                definitions(then_body, name, lengths);
                if let Some(b) = else_body { definitions(b, name, lengths); }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => definitions(body, name, lengths),
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { definitions(&c.body, name, lengths); }
                if let Some(b) = default { definitions(b, name, lengths); }
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

/// Lower the deferred-length CHARACTER arrays of a program unit and of the
/// units it contains.
pub(crate) fn resolve(body: &mut Vec<Stmt>) -> Result<(), String> {
    let declared: Vec<String> = body.iter().filter_map(|s| match s {
        Stmt::ArrayDecl { kind, name, .. } if crate::substrings::deferred(kind) => Some(name.clone()),
        _ => None,
    }).collect();
    let mut lowered = vec![];
    for name in &declared {
        let mut lengths = vec![];
        definitions(body, name, &mut lengths);
        let Some(Some(first)) = lengths.first().copied() else { continue };
        if lengths.iter().any(|l| *l != Some(first)) {
            return Err(format!("deferred-length CHARACTER array `{name}` currently requires every assignment and ALLOCATE to give it the same constant length"));
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

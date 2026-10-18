// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower DATA statements to declaration initialization of the named
//! variables, which gives them their initial values and implied SAVE. A value
//! `r*c` repeats `c`; the repeat count may be a named constant, so it is kept
//! as `@data_repeat(r, c)` until constants are folded.
use crate::ast::{Expr, Stmt, TypeSpec};

pub(crate) const REPEAT: &str = "@data_repeat";

/// The type of `name` from a declaration in `body`, and whether it is an array.
fn declared(body: &[Stmt], name: &str) -> Option<(TypeSpec, bool)> {
    let mut found = None;
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } if names.iter().any(|n| n == name) => found = Some((kind.clone(), false)),
            Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| n == name) => found = Some((kind.clone(), false)),
            Stmt::ArrayDecl { kind, name: n, .. } if n == name => return Some((kind.clone(), true)),
            _ => {}
        }
    }
    found
}

/// Give `name` the initial value `value` in the declarations of `body`.
fn initialize(body: &mut Vec<Stmt>, name: &str, value: Expr) -> Result<(), String> {
    let Some((kind, array)) = declared(body, name) else {
        return Err(format!("DATA requires `{name}` to be declared before it, in the same program unit"));
    };
    let initializer = || Stmt::InitializedDecl { kind: kind.clone(), attributes: vec![], entities: vec![(name.to_owned(), Some(value.clone()))] };
    if array {
        // An initialized array is its InitializedDecl followed by its ArrayDecl.
        let at = body.iter().position(|s| matches!(s, Stmt::ArrayDecl { name: n, .. } if n == name)).unwrap();
        body.insert(at, initializer());
        return Ok(());
    }
    let at = body.iter().position(|s| match s {
        Stmt::VarDecl { names, .. } => names.iter().any(|n| n == name),
        Stmt::InitializedDecl { entities, .. } => entities.iter().any(|(n, _)| n == name),
        _ => false,
    }).unwrap();
    match &mut body[at] {
        Stmt::InitializedDecl { entities, .. } => {
            let slot = entities.iter_mut().find(|(n, _)| n == name).unwrap();
            if slot.1.is_some() {
                return Err(format!("`{name}` is initialized more than once"));
            }
            slot.1 = Some(value);
        }
        Stmt::VarDecl { names, .. } => {
            names.retain(|n| n != name);
            if names.is_empty() {
                body[at] = initializer();
            } else {
                body.insert(at + 1, initializer());
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// Lower the DATA statements of one program unit and of contained units.
pub(crate) fn resolve(body: &mut Vec<Stmt>) -> Result<(), String> {
    let mut sets = vec![];
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == "@data" => {
            sets.extend(args.iter().cloned());
            false
        }
        _ => true,
    });
    for set in sets {
        let Expr::Call(_, parts) = set else { unreachable!("DATA set") };
        let [Expr::Call(_, objects), Expr::Call(_, values)] = parts.as_slice() else { unreachable!("DATA set") };
        let mut values = values.clone();
        for (i, object) in objects.iter().enumerate() {
            let Expr::Ident(name) = object else {
                return Err("DATA currently supports whole variables, not array elements, sections or implied-DO lists".into());
            };
            let (_, array) = declared(body, name)
                .ok_or_else(|| format!("DATA requires `{name}` to be declared before it, in the same program unit"))?;
            let last = i + 1 == objects.len();
            if array {
                if !last {
                    return Err("DATA currently requires an array to be the last variable of its value list".into());
                }
                let items = std::mem::take(&mut values);
                initialize(body, name, Expr::Call("@array_constructor".into(), items))?;
            } else {
                if values.is_empty() {
                    return Err(format!("DATA has no value for `{name}`"));
                }
                let value = values.remove(0);
                if matches!(&value, Expr::Call(n, _) if n == REPEAT) {
                    return Err(format!("DATA gives a repeated value to scalar `{name}`"));
                }
                initialize(body, name, value)?;
            }
        }
        if !values.is_empty() {
            return Err("DATA has more values than variables".into());
        }
    }
    for s in body.iter_mut() {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
            resolve(body)?;
        }
    }
    Ok(())
}

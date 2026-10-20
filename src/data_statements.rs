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

/// Whether `body` may change `name`: by assignment, input, or as an actual
/// argument or the operand of any other call, conservatively.
fn modified(body: &[Stmt], name: &str) -> bool {
    fn mentions(e: &Expr, name: &str) -> bool {
        match e {
            Expr::Ident(n) | Expr::Section { name: n, .. } => n == name,
            Expr::Call(n, args) | Expr::Index(n, args) => n == name || args.iter().any(|a| mentions(a, name)),
            Expr::Bin(_, a, b) => mentions(a, name) || mentions(b, name),
            Expr::Un(_, a) => mentions(a, name),
            _ => false,
        }
    }
    body.iter().any(|s| match s {
        Stmt::Assign { name: n, .. } | Stmt::AssignIndex { name: n, .. } => n == name,
        Stmt::Read { args } | Stmt::CallSub { args, .. } => args.iter().any(|a| mentions(a, name)),
        Stmt::Subroutine { .. } | Stmt::Function { .. } => true,
        Stmt::If { then_body, else_body, .. } => modified(then_body, name) || else_body.as_deref().is_some_and(|b| modified(b, name)),
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => modified(body, name),
        Stmt::SelectCase { cases, default, .. } => {
            cases.iter().any(|c| modified(&c.body, name)) || default.as_deref().is_some_and(|b| modified(b, name))
        }
        _ => false,
    })
}

/// Give `name` the initial value `value` in the declarations of `body`.
fn initialize(body: &mut Vec<Stmt>, name: &str, value: Expr, procedure: bool) -> Result<(), String> {
    let Some((kind, array)) = declared(body, name) else {
        return Err(format!("DATA requires `{name}` to be declared before it, in the same program unit"));
    };
    // An array that a procedure never changes, such as a table of pivots, is
    // a named constant; its initial value is all it ever has. Procedures do
    // not otherwise support initialized local arrays, which need SAVE storage.
    let attributes = if array && procedure && !modified(body, name) { vec!["parameter".to_owned()] } else { vec![] };
    let initializer = || Stmt::InitializedDecl { kind: kind.clone(), attributes: attributes.clone(), entities: vec![(name.to_owned(), Some(value.clone()))] };
    if array {
        // An initialized array is its InitializedDecl followed by its ArrayDecl.
        let at = body.iter().position(|s| matches!(s, Stmt::ArrayDecl { name: n, .. } if n == name)).unwrap();
        // A named constant array has rank one, so a constant table of higher
        // rank keeps its values in one, which the table's name aliases.
        let size = match &body[at] {
            Stmt::ArrayDecl { dims, .. } if dims.len() > 1 && !attributes.is_empty() => dims.iter().try_fold(1i64, |size, d| {
                let extent = match d {
                    Expr::IntLit(n) => n.parse::<i64>().ok()?,
                    Expr::Call(tag, a) if tag == "@declared_bounds" => match (&a[0], &a[1]) {
                        (Expr::IntLit(l), Expr::IntLit(u)) => u.parse::<i64>().ok()? - l.parse::<i64>().ok()? + 1,
                        _ => return None,
                    },
                    _ => return None,
                };
                size.checked_mul(extent.max(0))
            }),
            _ => None,
        };
        if let Some(size) = size {
            let flat = format!("@data_table_{name}");
            body.insert(at, Stmt::CallSub {
                name: crate::equivalence::STATEMENT.into(),
                args: vec![Expr::Call("@equivalence_set".into(), vec![Expr::Ident(flat.clone()), Expr::Ident(name.to_owned())])],
            });
            body.insert(at, Stmt::ArrayDecl { kind: kind.clone(), name: flat.clone(), dims: vec![Expr::IntLit(size.to_string())] });
            body.insert(at, Stmt::InitializedDecl { kind: kind.clone(), attributes: attributes.clone(), entities: vec![(flat, Some(value))] });
            return Ok(());
        }
        if attributes.is_empty() || matches!(&body[at], Stmt::ArrayDecl { dims, .. } if dims.len() == 1) {
            body.insert(at, initializer());
        } else {
            body.insert(at, Stmt::InitializedDecl { kind: kind.clone(), attributes: vec![], entities: vec![(name.to_owned(), Some(value.clone()))] });
        }
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
    unit(body, false)
}

fn unit(body: &mut Vec<Stmt>, procedure: bool) -> Result<(), String> {
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
                initialize(body, name, Expr::Call("@array_constructor".into(), items), procedure)?;
            } else {
                if values.is_empty() {
                    return Err(format!("DATA has no value for `{name}`"));
                }
                let value = values.remove(0);
                if matches!(&value, Expr::Call(n, _) if n == REPEAT) {
                    return Err(format!("DATA gives a repeated value to scalar `{name}`"));
                }
                initialize(body, name, value, procedure)?;
            }
        }
        if !values.is_empty() {
            return Err("DATA has more values than variables".into());
        }
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => unit(body, true)?,
            Stmt::Module { body, .. } => unit(body, false)?,
            _ => {}
        }
    }
    Ok(())
}

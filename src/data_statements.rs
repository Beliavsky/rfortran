// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower DATA statements to declaration initialization of the named
//! variables, which gives them their initial values and implied SAVE. A value
//! `r*c` repeats `c`; the repeat count may be a named constant, so it is kept
//! as `@data_repeat(r, c)` until constants are folded.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec, UnOp};
use std::collections::HashMap;

pub(crate) const REPEAT: &str = "@data_repeat";
/// The name a SAVE statement without a list gives, for every local variable.
pub(crate) const SAVE_ALL: &str = "@save_all";
/// An implied-DO list of DATA objects: `@data_implied_do(var, first, last, objects...)`.
pub(crate) const IMPLIED_DO: &str = "@data_implied_do";

/// The value of an integer constant expression of literals and the named
/// constants of `body`.
pub(crate) fn integer(body: &[Stmt], e: &Expr) -> Option<i64> {
    match e {
        Expr::IntLit(n) => n.parse().ok(),
        Expr::Ident(n) => body
            .iter()
            .find_map(|s| match s {
                Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "parameter") => {
                    entities.iter().find(|(m, _)| m == n).and_then(|(_, v)| v.as_ref())
                }
                _ => None,
            })
            .filter(|v| !matches!(v, Expr::Ident(m) if m == n))
            .and_then(|v| integer(body, v)),
        Expr::Un(UnOp::Neg, a) => integer(body, a).map(|v| -v),
        Expr::Bin(op, a, b) => {
            let (a, b) = (integer(body, a)?, integer(body, b)?);
            match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div if b != 0 => Some(a / b),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The lower bound and extent of each dimension of a declared array, when
/// they are constant.
fn bounds(body: &[Stmt], dims: &[Expr]) -> Option<Vec<(i64, i64)>> {
    dims.iter()
        .map(|d| match d {
            Expr::Call(tag, a) if tag == "@declared_bounds" => {
                let (l, u) = (integer(body, &a[0])?, integer(body, &a[1])?);
                Some((l, (u - l + 1).max(0)))
            }
            d => Some((1, integer(body, d)?.max(0))),
        })
        .collect()
}

/// A variable a DATA statement initializes: a whole variable, or an array
/// element with constant subscripts.
enum Target {
    Whole(String),
    Element(String, Vec<i64>),
}

/// The variables of DATA object `object`, with the variables of enclosing
/// implied-DO lists bound to their current values.
fn targets(body: &[Stmt], object: &Expr, bound: &HashMap<String, i64>, out: &mut Vec<Target>) -> Result<(), String> {
    let value = |e: &Expr| -> Result<i64, String> {
        let mut e = e.clone();
        crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
            if let Expr::Ident(n) = e {
                if let Some(v) = bound.get(n.as_str()) {
                    *e = Expr::IntLit(v.to_string());
                }
            }
        });
        integer(body, &e).ok_or_else(|| "DATA subscripts and implied-DO bounds must be integer constants".to_string())
    };
    match object {
        Expr::Ident(n) => out.push(Target::Whole(n.clone())),
        Expr::Call(tag, args) if tag == IMPLIED_DO => {
            let Expr::Ident(var) = &args[0] else { unreachable!("implied-DO variable") };
            let (first, last) = (value(&args[1])?, value(&args[2])?);
            for i in first..=last {
                let mut inner = bound.clone();
                inner.insert(var.clone(), i);
                for o in &args[3..] {
                    targets(body, o, &inner, out)?;
                }
            }
        }
        Expr::Call(n, subscripts) | Expr::Index(n, subscripts) => {
            let subscripts = subscripts.iter().map(value).collect::<Result<_, _>>()?;
            out.push(Target::Element(n.clone(), subscripts));
        }
        _ => return Err("DATA currently supports variables, array elements and implied-DO lists, not sections".into()),
    }
    Ok(())
}

/// The next value of a DATA value list, expanding a repeat `r*c` whose count
/// is constant.
fn next_value(body: &[Stmt], values: &mut Vec<Expr>) -> Option<Expr> {
    if values.is_empty() {
        return None;
    }
    if let Expr::Call(n, a) = &values[0] {
        if n == REPEAT {
            if let Some(count) = integer(body, &a[0]) {
                let c = a[1].clone();
                if count > 1 {
                    values[0] = Expr::Call(REPEAT.into(), vec![Expr::IntLit((count - 1).to_string()), c.clone()]);
                } else {
                    values.remove(0);
                }
                return Some(c);
            }
        }
    }
    Some(values.remove(0))
}

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
        // Formatted output only reads its items.
        Stmt::CallSub { name: n, .. } if n == "@formatted_print" => false,
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
            Stmt::ArrayDecl { dims, .. } if dims.len() > 1 && !attributes.is_empty() => {
                bounds(body, dims).and_then(|b| b.iter().try_fold(1i64, |size, (_, extent)| size.checked_mul(*extent)))
            }
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

/// Whether `name` is referenced like a function in `body`, or defined like a
/// statement function, rather than being a variable.
fn called(body: &mut [Stmt], name: &str) -> bool {
    let mut found = false;
    for s in body.iter_mut() {
        if matches!(s, Stmt::AssignIndex { name: n, .. } if n == name) {
            return true;
        }
        for e in crate::procedure_arguments::expressions(s) {
            crate::procedure_arguments::visit_expr(e, &mut |e| {
                found |= matches!(e, Expr::Call(n, _) | Expr::Index(n, _) if n == name);
            });
        }
        for b in crate::procedure_arguments::children(s) {
            found |= called(b, name);
        }
    }
    found
}

/// Lower SAVE statements and attributes of a procedure, whose dummies and
/// result are `dummies`: a saved scalar is declared as initialized storage,
/// which keeps its value between calls. A main program's and a module's
/// variables keep their values anyway.
fn save(body: &mut Vec<Stmt>, dummies: Option<&[String]>) -> Result<(), String> {
    let mut names = vec![];
    let mut all = false;
    body.retain(|s| match s {
        Stmt::DummyIntent { name, intent } if intent == "save" => {
            if name == SAVE_ALL {
                all = true;
            } else {
                names.push(name.clone());
            }
            false
        }
        _ => true,
    });
    let Some(dummies) = dummies else { return Ok(()) };
    if all {
        // External procedures and named constants are not variables.
        let external: Vec<String> = body.iter().flat_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "external" => vec![name.clone()],
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "parameter") => {
                entities.iter().map(|(n, _)| n.clone()).collect()
            }
            _ => vec![],
        }).collect();
        let locals: Vec<(String, bool)> = body.iter().flat_map(|s| match s {
            Stmt::VarDecl { names, .. } => names.iter().map(|n| (n.clone(), false)).collect(),
            Stmt::ArrayDecl { name, .. } => vec![(name.clone(), true)],
            _ => vec![],
        }).filter(|(n, _)| !dummies.contains(n) && !external.contains(n)).collect();
        for (n, array) in locals {
            if !array && called(body, &n) {
                continue;
            }
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    for name in names {
        if dummies.contains(&name) {
            return Err(format!("dummy argument `{name}` cannot have the SAVE attribute"));
        }
        match declared(body, &name) {
            None => return Err(format!("SAVE requires `{name}` to be declared in the same program unit")),
            Some((_, true)) => return Err(format!("SAVE of local array `{name}` in a procedure is not yet supported")),
            Some(_) => {}
        }
        let at = body.iter().position(|s| match s {
            Stmt::VarDecl { names, .. } => names.contains(&name),
            Stmt::InitializedDecl { entities, .. } => entities.iter().any(|(n, _)| n == &name),
            _ => false,
        }).unwrap();
        match &mut body[at] {
            Stmt::InitializedDecl { attributes, .. } => {
                if !attributes.iter().any(|a| a == "save" || a == "parameter") {
                    attributes.push("save".into());
                }
            }
            Stmt::VarDecl { kind, names } => {
                let kind = kind.clone();
                names.retain(|n| n != &name);
                let saved = crate::ast::scalar_decl(kind, vec!["save".into()], vec![(name, None)]);
                if names.is_empty() {
                    body[at] = saved;
                } else {
                    body.insert(at + 1, saved);
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

/// Lower the SAVE and DATA statements of one program unit and of contained
/// units.
pub(crate) fn resolve(body: &mut Vec<Stmt>) -> Result<(), String> {
    unit(body, None)
}

fn unit(body: &mut Vec<Stmt>, dummies: Option<&[String]>) -> Result<(), String> {
    save(body, dummies)?;
    let procedure = dummies.is_some();
    let mut sets = vec![];
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == "@data" => {
            sets.extend(args.iter().cloned());
            false
        }
        _ => true,
    });
    // The values of arrays initialized element by element, in array element
    // order.
    let mut elements: HashMap<String, Vec<Option<Expr>>> = HashMap::new();
    for set in sets {
        let Expr::Call(_, parts) = set else { unreachable!("DATA set") };
        let [Expr::Call(_, objects), Expr::Call(_, values)] = parts.as_slice() else { unreachable!("DATA set") };
        let mut values = values.clone();
        let mut list = vec![];
        for object in objects {
            targets(body, object, &HashMap::new(), &mut list)?;
        }
        let count = list.len();
        for (i, target) in list.into_iter().enumerate() {
            let name = match target {
                Target::Whole(name) => name,
                Target::Element(name, subscripts) => {
                    let value = next_value(body, &mut values).ok_or_else(|| format!("DATA has no value for an element of `{name}`"))?;
                    let dims = body
                        .iter()
                        .find_map(|s| match s {
                            Stmt::ArrayDecl { name: n, dims, .. } if n == &name => Some(dims.clone()),
                            _ => None,
                        })
                        .ok_or_else(|| format!("DATA gives elements to `{name}`, which is not a declared array"))?;
                    let b = bounds(body, &dims).ok_or_else(|| format!("DATA for elements of `{name}` requires constant bounds"))?;
                    if b.len() != subscripts.len() {
                        return Err(format!("DATA gives `{name}` the wrong number of subscripts"));
                    }
                    let (mut offset, mut stride) = (0i64, 1i64);
                    for ((lower, extent), s) in b.iter().zip(&subscripts) {
                        if *s < *lower || *s >= lower + extent {
                            return Err(format!("DATA subscript of `{name}` is out of bounds"));
                        }
                        offset += (s - lower) * stride;
                        stride *= extent;
                    }
                    let slots = elements.entry(name).or_insert_with(|| vec![None; stride as usize]);
                    if slots[offset as usize].replace(value).is_some() {
                        return Err("an array element is initialized more than once".into());
                    }
                    continue;
                }
            };
            let (_, array) = declared(body, &name)
                .ok_or_else(|| format!("DATA requires `{name}` to be declared before it, in the same program unit"))?;
            let last = i + 1 == count;
            let name = &name;
            if array {
                if !last {
                    return Err("DATA currently requires an array to be the last variable of its value list".into());
                }
                let mut items = vec![];
                while let Some(v) = next_value(body, &mut values) {
                    items.push(v);
                }
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
    let mut elements: Vec<_> = elements.into_iter().collect();
    elements.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, slots) in elements {
        let values = slots
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("DATA currently requires every element of `{name}` to be given a value"))?;
        initialize(body, &name, Expr::Call("@array_constructor".into(), values), procedure)?;
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body, params, name, .. } => {
                let mut dummies = params.clone();
                dummies.push(name.clone());
                unit(body, Some(&dummies))?
            }
            Stmt::Subroutine { body, params, .. } => unit(body, Some(&params.clone()))?,
            Stmt::Module { body, .. } => unit(body, None)?,
            _ => {}
        }
    }
    Ok(())
}

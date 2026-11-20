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
            // A value that refers to the array, as [a, 'x'], is assigned
            // with reallocation, which reads the array first.
            Stmt::Assign { name, value } if arrays.contains(name)
                && !serde_json::to_string(&*value).is_ok_and(|v| v.contains(&format!("\"{name}\""))) => {
                if let Some((_, Some(size))) = shape(value) {
                    // CHARACTER arrays do not yet support a
                    // conditional DEALLOCATE, so the first assignment in
                    // statement order allocates, and later ones reallocate.
                    if !allocated.insert(name.clone()) {
                        out.push(Stmt::CallSub { name: "@deallocate".into(), args: vec![Expr::Ident(name.clone())] });
                    }
                    out.push(Stmt::CallSub { name: "@allocate".into(), args: vec![Expr::Call(name.clone(), vec![Expr::IntLit(size.to_string())])] });
                }
            }
            Stmt::CallSub { name, args } if name == crate::substrings::ALLOCATE_CHARACTER => {
                let (mut mine, mut others): (Vec<Expr>, Vec<Expr>) = args[1..].iter().cloned()
                    .partition(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if arrays.contains(o)));
                // STAT= and ERRMSG= go with the arrays when nothing else is allocated.
                if !mine.is_empty() && others.iter().all(|a| matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:"))) {
                    mine.append(&mut others);
                }
                if !mine.is_empty() {
                    // A later constructor assignment reallocates.
                    for a in &mine {
                        if let Expr::Call(n, _) | Expr::Index(n, _) = a { allocated.insert(n.clone()); }
                    }
                    out.push(Stmt::CallSub { name: "@allocate".into(), args: mine.into_iter().map(|a| match a {
                        Expr::Index(n, d) => Expr::Call(n, d), a => a }).collect() });
                    if others.is_empty() { continue; }
                    let mut kept = vec![args[0].clone()];
                    kept.extend(others);
                    *args = kept;
                }
            }
            Stmt::CallSub { name, args } if name == "@allocate" || name == "@deallocate" => {
                for a in args.iter() {
                    if let Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) = a {
                        if arrays.contains(n) {
                            if name == "@allocate" { allocated.insert(n.clone()); } else { allocated.remove(n); }
                        }
                    }
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

/// The items of a constructor, each a whole array or a section of a
/// rank-one array declared in the program unit, or None for a scalar, when
/// no item is another array section, implied DO or nested constructor.
fn items(items: &[Expr], declared: &[String]) -> Option<Vec<Option<Expr>>> {
    items.iter().map(|i| match i {
        Expr::Ident(n) => Some(declared.contains(n).then(|| i.clone())),
        Expr::Section { name: n, .. } => Some(declared.contains(n).then(|| i.clone())),
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
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } if arrays.contains(n) => Expr::Ident(width(n)),
        _ => Expr::Call("len".into(), vec![e.clone()]),
    };
    if let Some(values) = constructor(e) {
        let plan = items(values, declared)?;
        // An empty typed constructor still supplies its element length.
        // Its zero size must not prevent allocation/reallocation lowering.
        let count = plan.iter().map(|p| p.as_ref().map_or(int(1), |a| Expr::Call("size".into(), vec![a.clone()])))
            .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Add, Box::new(a), Box::new(b))).unwrap_or_else(|| int(0));
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
        return Some((typed.or_else(|| values.first().map(length))?, count));
    }
    match e {
        Expr::Bin(crate::ast::BinOp::Concat,a,b)=>{
            let left=runtime_shape(a,arrays,declared);
            let right=runtime_shape(b,arrays,declared);
            let count=left.as_ref().or(right.as_ref())?.1.clone();
            Some((Expr::Bin(crate::ast::BinOp::Add,Box::new(left.map_or_else(||length(a),|v|v.0)),Box::new(right.map_or_else(||length(b),|v|v.0))),count))
        }
        Expr::Call(n,args) if matches!(n.as_str(),"cshift"|"eoshift"|"unpack") => {
            let argument=|position:usize,key:&str| args.iter().find_map(|a|crate::named_arguments::keyword(a).filter(|(k,_)|*k==key).map(|(_,v)|v)).or_else(||args.get(position).filter(|a|crate::named_arguments::keyword(a).is_none()));
            let source=argument(0,if n=="unpack" {"vector"}else{"array"})?;
            let (width,count)=runtime_shape(source,arrays,declared)?;
            let count=if n=="unpack" {Expr::Call("size".into(),vec![argument(1,"mask")?.clone()])} else {count};
            Some((width,count))
        }
        Expr::Section{name,..} if declared.contains(name) || arrays.contains(name) => Some((length(&Expr::Ident(name.clone())),Expr::Call("size".into(),vec![e.clone()]))),
        Expr::Ident(n) if declared.contains(n) || arrays.contains(n) => Some((length(e), size(n))),
        _ => shape(e).and_then(|(length, size)| size.map(|size| (int(length), int(size)))),
    }
}

/// Set the length and allocate a deferred-length CHARACTER array whose
/// length is set at run time: ALLOCATE with a type specification sets the
/// length, and assignment of a whole array value reallocates the array to
/// the value's length and size.
/// `outer` are the arrays of an enclosing unit, which a BLOCK reads.
fn reallocate(body: &mut Vec<Stmt>, arrays: &[String], declared: &[String], outer: &[String]) {
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
                reallocate(&mut block, &inner, declared, outer);
                out.extend(block);
                continue;
            }
            Stmt::Assign { name, value } if arrays.contains(name) => {
                let widths: Vec<String> = arrays.iter().chain(outer).cloned().collect();
                if let Some((length, size)) = runtime_shape(value, &widths, declared) {
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
                                let one = || Expr::IntLit("1".into());
                                let bound = |a: &str, b: &str| Expr::Call(b.into(), vec![id(a), one()]);
                                let (array, start, end, element) = match array {
                                    Expr::Ident(a) => (a.clone(), bound(&a, "lbound"), bound(&a, "ubound"), id(&index)),
                                    // Element k of a(l:u:s) is a(l + (k - 1)*s).
                                    Expr::Section { ref name, ref lower, ref stride, .. } => {
                                        let lower = lower.as_deref().cloned().unwrap_or_else(|| bound(name, "lbound"));
                                        let stride = stride.as_deref().cloned().unwrap_or_else(one);
                                        let offset = Expr::Bin(crate::ast::BinOp::Mul,
                                            Box::new(Expr::Bin(crate::ast::BinOp::Sub, Box::new(id(&index)), Box::new(one()))), Box::new(stride));
                                        (name.clone(), one(), Expr::Call("size".into(), vec![array.clone()]),
                                            Expr::Bin(crate::ast::BinOp::Add, Box::new(lower), Box::new(offset)))
                                    }
                                    _ => unreachable!(),
                                };
                                block.push(Stmt::Do { var: index.clone(), start, end, body: vec![
                                    next.clone(),
                                    Stmt::AssignIndex { name: name.clone(), indices: vec![id(&position)], value: Expr::Index(array, vec![element]) },
                                ] });
                            }
                        }
                        _ => block.push(s),
                    }
                    continue;
                }
            }
            Stmt::CallSub { name, args } if name == crate::substrings::ALLOCATE_CHARACTER => {
                let (mut mine, mut others): (Vec<Expr>, Vec<Expr>) = args[1..].iter().cloned()
                    .partition(|a| matches!(a, Expr::Call(o, _) | Expr::Index(o, _) if arrays.contains(o)));
                // STAT= and ERRMSG= go with the arrays when nothing else is allocated.
                if !mine.is_empty() && others.iter().all(|a| matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:"))) {
                    mine.append(&mut others);
                }
                if !mine.is_empty() {
                    for a in &mine {
                        let (Expr::Call(o, _) | Expr::Index(o, _)) = a else { unreachable!() };
                        if o.starts_with("@keyword:") { continue; }
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
                reallocate(then_body, arrays, declared, outer);
                if let Some(b) = else_body { reallocate(b, arrays, declared, outer); }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => reallocate(body, arrays, declared, outer),
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { reallocate(&mut c.body, arrays, declared, outer); }
                if let Some(b) = default { reallocate(b, arrays, declared, outer); }
            }
            _ => {}
        }
        out.push(s);
    }
    *body = out;
}

/// The result variable of a function whose result is a deferred-length
/// CHARACTER array.
fn deferred_result(body: &[Stmt], name: &str) -> Option<String> {
    let result = match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(),
        _ => name.to_string(),
    };
    body.iter().any(|s| matches!(s, Stmt::ArrayDecl { kind, name, .. } if *name == result && crate::substrings::deferred(kind)))
        .then_some(result)
}

/// The functions, in all units, whose result is a deferred-length
/// CHARACTER array.
fn deferred_functions(body: &[Stmt], names: &mut Vec<String>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } => {
                if deferred_result(body, name).is_some() && !names.contains(name) { names.push(name.clone()); }
                deferred_functions(body, names);
            }
            Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => deferred_functions(body, names),
            _ => {}
        }
    }
}

/// Replace each string of a serialized AST equal to `from`, except string
/// literals, by `to`.
fn rename_value(v: &mut serde_json::Value, from: &str, to: &str) {
    use serde_json::Value;
    match v {
        Value::String(s) if s == from => *s = to.to_string(),
        Value::Array(a) => for v in a { rename_value(v, from, to); },
        Value::Object(o) => for (k, v) in o.iter_mut() {
            if k != "Str" { rename_value(v, from, to); }
        },
        _ => {}
    }
}

/// A function whose result is a deferred-length CHARACTER array becomes a
/// subroutine with the result as a last INTENT(OUT) dummy, whose length is
/// returned as that of any deferred-length dummy. Each reference to the
/// function becomes a call before its statement, giving a temporary.
fn results(body: &mut Vec<Stmt>) -> Result<(), (String, Option<usize>)> {
    let mut names = vec![];
    deferred_functions(body, &mut names);
    if names.is_empty() { return Ok(()); }
    calls(body, &names, &mut 0, None)
}

type Located<T> = Result<T, (String, Option<usize>)>;

/// Hoist the references to the functions in a serialized expression,
/// innermost first, into calls giving temporaries.
fn hoist(v: &mut serde_json::Value, names: &[String], serial: &mut usize, temps: &mut Vec<(String, Stmt)>) {
    use serde_json::Value;
    match v {
        Value::Array(a) => for v in a { hoist(v, names, serial, temps); },
        Value::Object(o) => {
            for (k, v) in o.iter_mut() { if k != "Str" { hoist(v, names, serial, temps); } }
            let Some(Value::Array(call)) = o.get("Call") else { return };
            let (Some(Value::String(f)), Some(args)) = (call.first(), call.get(1)) else { return };
            if !names.contains(f) { return; }
            let Ok(mut args) = serde_json::from_value::<Vec<Expr>>(args.clone()) else { return };
            *serial += 1;
            let temp = format!("@deferred_result_{serial}");
            args.push(Expr::Ident(temp.clone()));
            temps.push((temp.clone(), Stmt::CallSub { name: f.clone(), args }));
            *v = serde_json::json!({ "Ident": temp });
        }
        _ => {}
    }
}

/// The subroutine for a function whose result is a deferred-length
/// CHARACTER array.
fn subroutine(name: &str, params: &mut Vec<String>, body: &mut Vec<Stmt>, result: &str, at: Option<usize>) -> Located<(Vec<String>, Vec<Stmt>)> {
    use serde_json::Value;
    let mut body = std::mem::take(body);
    if matches!(body.last(), Some(Stmt::Return(Some(_)))) { body.pop(); }
    let mut v = serde_json::to_value(&body).map_err(|e| (e.to_string(), at))?;
    // Within the subroutine, the result has a name of its own.
    let dummy = if result == name { format!("@result${name}") } else { result.to_string() };
    rename_value(&mut v, result, &dummy);
    // A RETURN of the function ends the subroutine.
    fn returns(v: &mut Value) {
        match v {
            Value::Array(a) => for v in a { returns(v); },
            Value::Object(o) => {
                if o.contains_key("Return") { o.insert("Return".into(), Value::Null); }
                for (k, v) in o.iter_mut() { if k != "Str" { returns(v); } }
            }
            _ => {}
        }
    }
    returns(&mut v);
    let mut body: Vec<Stmt> = serde_json::from_value(v).map_err(|e| (e.to_string(), at))?;
    let position = body.iter().position(|s| !matches!(s, Stmt::SourceLocation(_) | Stmt::Use { .. } | Stmt::Implicit(_))).unwrap_or(body.len());
    body.insert(position, Stmt::DummyIntent { name: dummy.clone(), intent: "intent(out)".into() });
    let mut params = std::mem::take(params);
    params.push(dummy);
    Ok((params, body))
}

/// The references in a unit, which declares the temporaries.
fn calls(body: &mut Vec<Stmt>, host: &[String], serial: &mut usize, at: Option<usize>) -> Located<()> {
    let own = local_names(body);
    let mut visible: Vec<String> = host.iter().filter(|n| !own.contains(n)).cloned().collect();
    for s in body.iter() {
        if let Stmt::Use { renames, .. } = s {
            for (local, remote) in renames {
                if host.contains(remote) && !own.contains(local) && !visible.contains(local) { visible.push(local.clone()); }
            }
        }
    }
    let mut declarations = vec![];
    statements(body, &visible, serial, at, &mut declarations)?;
    // The temporaries are declared with the unit's declarations.
    let position = body.iter().position(|s| !matches!(s, Stmt::SourceLocation(_) | Stmt::Use { .. } | Stmt::Implicit(_)
        | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DummyIntent { .. }
        | Stmt::DerivedDecl { .. } | Stmt::DerivedType { .. } | Stmt::Accessibility { .. }
        | Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. })).unwrap_or(body.len());
    body.splice(position..position, declarations);
    Ok(())
}

fn statements(body: &mut Vec<Stmt>, names: &[String], serial: &mut usize, mut at: Option<usize>, declarations: &mut Vec<Stmt>) -> Located<()> {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        if let Stmt::SourceLocation(p) = s { at = Some(p); }
        let mut temps = vec![];
        if !names.is_empty() {
            let mut each = |e: &mut Expr, temps: &mut Vec<(String, Stmt)>| -> Located<()> {
                let mut v = serde_json::to_value(&*e).map_err(|e| (e.to_string(), at))?;
                hoist(&mut v, names, serial, temps);
                *e = serde_json::from_value(v).map_err(|e| (e.to_string(), at))?;
                Ok(())
            };
            match &mut s {
                Stmt::Assign { value, .. } => each(value, &mut temps)?,
                Stmt::AssignIndex { indices, value, .. } => { for i in indices { each(i, &mut temps)?; } each(value, &mut temps)?; }
                Stmt::Print { items } => for i in items { each(i, &mut temps)?; },
                Stmt::CallSub { args, .. } => for a in args { each(a, &mut temps)?; },
                Stmt::If { cond, .. } => each(cond, &mut temps)?,
                Stmt::SelectCase { expr, .. } => each(expr, &mut temps)?,
                Stmt::Return(Some(e)) => each(e, &mut temps)?,
                // The bounds are evaluated once, before the loop.
                Stmt::Do { start, end, .. } => { each(start, &mut temps)?; each(end, &mut temps)?; }
                Stmt::DoWhile { cond, .. } => {
                    each(cond, &mut temps)?;
                    if !temps.is_empty() {
                        return Err(("a deferred-length CHARACTER array function result in a DO WHILE condition is not yet supported".into(), at));
                    }
                }
                _ => {}
            }
        }
        match &mut s {
            Stmt::If { then_body, else_body, .. } => {
                statements(then_body, names, serial, at, declarations)?;
                if let Some(b) = else_body { statements(b, names, serial, at, declarations)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => statements(body, names, serial, at, declarations)?,
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases { statements(&mut c.body, names, serial, at, declarations)?; }
                if let Some(b) = default { statements(b, names, serial, at, declarations)?; }
            }
            Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => calls(body, names, serial, at)?,
            Stmt::Function { name, params, body: fbody, elemental, pure, .. } => {
                calls(fbody, names, serial, at)?;
                if let Some(result) = deferred_result(fbody, name).filter(|_| names.contains(name)) {
                    let (params, fbody) = subroutine(name, params, fbody, &result, at)?;
                    s = Stmt::Subroutine { elemental: *elemental, pure: *pure, name: name.clone(), params, body: fbody };
                }
            }
            _ => {}
        }
        for (temp, call) in temps.iter() {
            declarations.push(Stmt::DummyIntent { name: temp.clone(), intent: "allocatable".into() });
            declarations.push(Stmt::ArrayDecl { kind: crate::ast::character_length(Expr::Ident("@deferred_character".into())), name: temp.clone(), dims: vec![Expr::Ident("@deferred_extent".into())] });
            out.push(call.clone());
        }
        out.push(s);
    }
    *body = out;
    Ok(())
}

/// The deferred-length CHARACTER arrays each module declares.
type Modules = std::collections::HashMap<String, Vec<String>>;

/// The names a unit declares, which hide those of its host and modules.
fn local_names(body: &[Stmt]) -> Vec<String> {
    body.iter().flat_map(|s| match s {
        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
        Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
        _ => vec![],
    }).collect()
}

/// Lower the deferred-length CHARACTER arrays of a program unit and of the
/// units it contains. An error carries the source offset of the definition
/// whose length differs.
pub(crate) fn resolve(body: &mut Vec<Stmt>) -> Result<(), (String, Option<usize>)> {
    results(body)?;
    let modules: Modules = body.iter().filter_map(|s| match s {
        Stmt::Module { name, body } => Some((name.clone(), body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { kind, name, .. } if crate::substrings::deferred(kind) => Some(name.clone()),
            _ => None,
        }).collect())),
        _ => None,
    }).collect();
    unit(body, &[], &modules, &[], &[])
}

/// A unit sees the arrays of its host and of the modules it uses, whose
/// length any unit may set at run time, so their definitions here set it.
/// `params` are the unit's dummy arguments, whose length is always set at
/// run time, as the actual argument's. A BLOCK may assign the `outer`
/// arrays of its unit.
fn unit(body: &mut Vec<Stmt>, host: &[String], modules: &Modules, params: &[String], outer: &[String]) -> Result<(), (String, Option<usize>)> {
    let own = local_names(body);
    let mut visible: Vec<String> = host.iter().filter(|n| !own.contains(n)).cloned().collect();
    for s in body.iter() {
        if let Stmt::Use { module, only, renames, .. } = s {
            let Some(arrays) = modules.get(module) else { continue };
            for array in arrays {
                let renamed: Vec<&String> = renames.iter().filter(|(_, remote)| remote == array).map(|(local, _)| local).collect();
                let names: Vec<String> = if !renamed.is_empty() {
                    renamed.into_iter().cloned().collect()
                } else if only.as_ref().is_none_or(|o| o.contains(array)) {
                    vec![array.clone()]
                } else { vec![] };
                for n in names {
                    if !own.contains(&n) && !visible.contains(&n) { visible.push(n); }
                }
            }
        }
    }
    let declared: Vec<String> = body.iter().filter_map(|s| match s {
        Stmt::ArrayDecl { kind, name, .. } if crate::substrings::deferred(kind) => Some(name.clone()),
        _ => None,
    }).collect();
    let mut lowered = vec![];
    let mut runtime = visible.clone();
    for name in &declared {
        let mut lengths = vec![];
        definitions(body, name, None, &mut lengths);
        let Some((Some(first), _)) = lengths.first().copied() else { runtime.push(name.clone()); continue };
        if lengths.iter().any(|l| l.0 != Some(first)) || params.contains(name) {
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
        let mut declared: Vec<String> = body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        declared.extend(visible.iter().chain(outer).cloned());
        reallocate(body, &runtime, &declared, outer);
    }
    // The arrays whose length is set at run time are visible to the
    // contained units.
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => unit(body, &runtime, modules, params, &[])?,
            Stmt::Module { body, .. } => unit(body, &runtime, modules, &[], &[])?,
            // The rewriting above has reached the statements of a BLOCK,
            // and of the BLOCKs it generates.
            Stmt::Block { body } => {
                let outer: Vec<String> = outer.iter().chain(&runtime).cloned().collect();
                unit(body, &[], modules, &[], &outer)?
            }
            _ => {}
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

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Recursive derived types, whose POINTER or ALLOCATABLE scalar components
//! reach their own type, as linked lists and trees do. Their objects live in
//! storage pools: every object is an INTEGER handle, 0 for none, and each
//! component is an array of the pool indexed by handle, in the generated
//! module `rfort_pools`. So
//!
//! * `p%next%v` is `rfort_node_v(rfort_node_next(p))`;
//! * `allocate(p)` is `p = rfort_new_node()`, which takes a free slot;
//! * `p => q`, NULLIFY, ASSOCIATED and ALLOCATED work on handles;
//! * DEALLOCATE frees the object and its ALLOCATABLE components;
//! * assignment of objects copies them deeply, as intrinsic assignment does;
//! * a structure constructor makes a new object, and a type-bound procedure
//!   call passes the object's handle.
//!
//! The rest of the compiler then sees only INTEGER handles and arrays of
//! intrinsic type. A procedure's local objects are not returned to their
//! pool, a leak of storage only. Polymorphism and extension of these types
//! and components of other derived types in them are not yet supported.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

pub(crate) const MODULE: &str = "rfort_pools";

#[derive(Clone, Debug)]
enum Shape {
    Scalar,
    /// Constant (lower, upper) bounds.
    Array(Vec<(i64, i64)>),
    /// A handle of an object of the named recursive type; ALLOCATABLE ones
    /// own their object.
    Handle { target: String, owned: bool },
}

#[derive(Clone, Debug)]
struct Component {
    name: String,
    kind: TypeSpec,
    shape: Shape,
    default: Option<Expr>,
}

#[derive(Clone, Debug, Default)]
struct Boxed {
    components: Vec<Component>,
    /// Type-bound procedure names, with their procedures.
    bindings: HashMap<String, String>,
}

fn int(n: i64) -> Expr { Expr::IntLit(n.to_string()) }
fn id(n: &str) -> Expr { Expr::Ident(n.to_owned()) }
fn call(n: &str, args: Vec<Expr>) -> Expr { Expr::Call(n.to_owned(), args) }
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr { Expr::Bin(op, Box::new(a), Box::new(b)) }
fn pool(ty: &str, c: &str) -> String { format!("rfort_{ty}_{c}") }
fn sub(n: &str, args: Vec<Expr>) -> Stmt { Stmt::CallSub { name: n.to_owned(), args } }

/// The DerivedType definitions of a program, by name.
fn definitions(body: &[Stmt], out: &mut HashMap<String, (Vec<Stmt>, Vec<(String, String)>)>) {
    for s in body {
        match s {
            Stmt::DerivedType { name, components, bindings } => { out.insert(name.clone(), (components.clone(), bindings.clone())); }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => definitions(body, out),
            _ => {}
        }
    }
}

/// The types that reach themselves through POINTER or ALLOCATABLE scalar
/// components.
fn recursive_types(defs: &HashMap<String, (Vec<Stmt>, Vec<(String, String)>)>) -> HashSet<String> {
    let edges: HashMap<&String, Vec<String>> = defs.iter().map(|(n, (components, _))| {
        let targets = components.iter().filter_map(|c| match c {
            Stmt::DerivedDecl { type_name, attributes, dims, .. }
                if dims.is_empty() && attributes.iter().any(|a| a == "pointer" || a == "allocatable") => Some(type_name.clone()),
            _ => None,
        }).collect();
        (n, targets)
    }).collect();
    let mut out = HashSet::new();
    for start in defs.keys() {
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = edges[start].clone();
        while let Some(t) = stack.pop() {
            if &t == start { out.insert(start.clone()); break; }
            if !seen.insert(t.clone()) { continue; }
            if let Some(next) = edges.get(&t) { stack.extend(next.iter().cloned()); }
        }
    }
    out
}

struct Lower {
    boxed: HashMap<String, Boxed>,
    /// Components of other derived types, with their types, and those that
    /// are handles of recursive types.
    others: HashMap<String, HashMap<String, String>>,
    /// Functions whose results are objects of recursive types, and whether
    /// the result is a POINTER.
    results: HashMap<String, (String, bool)>,
    constants: HashMap<String, Expr>,
    /// The program's functions.
    functions: HashSet<String>,
    serial: usize,
}

/// A variable of a scope: its type, and whether it is a POINTER or
/// ALLOCATABLE handle, an array, a dummy argument.
#[derive(Clone, Debug)]
struct Var {
    ty: String,
    reference: bool,
    owned: bool,
}
type Vars = HashMap<String, Var>;

/// What a designator denotes after lowering.
#[derive(Clone, Debug)]
enum Place {
    /// A handle of an object of a recursive type, stored at `target`.
    Handle { value: Expr, target: Target, ty: String, reference: bool, owned: bool },
    /// A component value of an object in a pool.
    Value(Expr),
    /// A type-bound procedure of the object `handle`.
    Binding { procedure: String, handle: Expr, args: Vec<Expr> },
}

/// Where a handle is stored: a variable (or a component of an object of
/// another type, by its path) or an element of a pool array.
#[derive(Clone, Debug)]
enum Target {
    Name(String, Vec<Expr>),
    Pool(String, Expr),
}

impl Target {
    fn assign(&self, value: Expr) -> Stmt {
        match self {
            Target::Name(n, subs) if subs.is_empty() => Stmt::Assign { name: n.clone(), value },
            Target::Name(n, subs) => Stmt::AssignIndex { name: n.clone(), indices: subs.clone(), value },
            Target::Pool(p, h) => Stmt::AssignIndex { name: p.clone(), indices: vec![h.clone()], value },
        }
    }
}

/// The parts of a designator: the variable, its subscripts, and the
/// components with their subscripts.
fn parts(e: &Expr) -> Option<(String, Vec<Expr>, Vec<(String, Vec<Expr>)>)> {
    match e {
        Expr::Ident(n) => {
            let mut p = n.split('%');
            let base = p.next()?.to_owned();
            Some((base, vec![], p.map(|f| (f.to_owned(), vec![])).collect()))
        }
        Expr::Call(n, args) if !n.starts_with('@') => {
            let mut p = n.split('%');
            let base = p.next()?.to_owned();
            let mut fields: Vec<(String, Vec<Expr>)> = p.map(|f| (f.to_owned(), vec![])).collect();
            match fields.last_mut() {
                Some(last) => last.1 = args.clone(),
                None => return Some((base, args.clone(), vec![])),
            }
            Some((base, vec![], fields))
        }
        Expr::Section { name, .. } if name.contains('%') => {
            let mut p = name.split('%');
            let base = p.next()?.to_owned();
            let mut fields: Vec<(String, Vec<Expr>)> = p.map(|f| (f.to_owned(), vec![])).collect();
            let mut range = e.clone();
            if let Expr::Section { name, .. } = &mut range { name.clear(); }
            fields.last_mut()?.1 = vec![range];
            Some((base, vec![], fields))
        }
        Expr::Call(n, args) if n.starts_with("@record_component:") && !args.is_empty() => {
            let (base, subs, mut fields) = parts(&args[0])?;
            let s = args.get(1).and_then(|a| match a { Expr::Call(t, s) if t == "@component_subscripts" => Some(s.clone()), _ => None }).unwrap_or_default();
            fields.push((n["@record_component:".len()..].to_owned(), s));
            Some((base, subs, fields))
        }
        _ => None,
    }
}

impl Lower {
    /// Lower a designator of an object of a recursive type, or of something
    /// reached through one.
    fn place(&self, e: &Expr, vars: &Vars) -> Result<Option<Place>> {
        let Some((base, subs, fields)) = parts(e) else { return Ok(None) };
        let Some(var) = vars.get(&base) else { return Ok(None) };
        // The path through objects of other types, up to a handle.
        let mut ty = var.ty.clone();
        let mut path = base.clone();
        let mut path_subs = subs.clone();
        let mut i = 0;
        let mut current: Option<(Expr, Target, bool, bool)> = None;
        if self.boxed.contains_key(&ty) {
            let value = if subs.is_empty() { id(&base) } else { call(&base, subs.clone()) };
            current = Some((value, Target::Name(base.clone(), subs.clone()), var.reference, var.owned));
        } else {
            while i < fields.len() {
                let (f, s) = &fields[i];
                let next = self.others.get(&ty).and_then(|c| c.get(f)).cloned();
                let Some(next) = next else { return Ok(None) };
                path = format!("{path}%{f}");
                if !s.is_empty() { path_subs = s.clone(); }
                i += 1;
                ty = next.clone();
                if self.boxed.contains_key(&next) {
                    let value = if path_subs.is_empty() { id(&path) } else { call(&path, path_subs.clone()) };
                    current = Some((value, Target::Name(path.clone(), path_subs.clone()), true, false));
                    break;
                }
            }
        }
        let Some((mut handle, mut target, mut reference, mut owned)) = current else { return Ok(None) };
        while i < fields.len() {
            let (f, s) = &fields[i];
            let b = &self.boxed[&ty];
            let last = i + 1 == fields.len();
            let Some(c) = b.components.iter().find(|c| c.name == *f) else {
                if let (true, Some(procedure)) = (last, b.bindings.get(f)) {
                    return Ok(Some(Place::Binding { procedure: procedure.clone(), handle, args: s.clone() }));
                }
                bail!("type `{ty}` has no component `{f}`");
            };
            let array = pool(&ty, f);
            match &c.shape {
                Shape::Handle { target: t, owned: o } => {
                    target = Target::Pool(array.clone(), handle.clone());
                    handle = call(&array, vec![handle]);
                    ty = t.clone();
                    reference = true;
                    owned = *o;
                }
                Shape::Scalar => {
                    if !last { bail!("component `{f}` of type `{ty}` has no components"); }
                    if !s.is_empty() {
                        // A substring of a CHARACTER component.
                        return Ok(Some(Place::Value(call("@array_substring", vec![call(&array, vec![handle]), s[0].clone()]))));
                    }
                    return Ok(Some(Place::Value(call(&array, vec![handle]))));
                }
                Shape::Array(bounds) => {
                    if !last { bail!("component `{f}` of type `{ty}` has no components"); }
                    let mut args: Vec<Expr> = if s.is_empty() {
                        bounds.iter().map(|_| call("@section_subscript", vec![Expr::Section { name: array.clone(), lower: None, upper: None, stride: None }])).collect()
                    } else {
                        s.iter().map(|x| match x {
                            Expr::Section { lower, upper, stride, .. } => call("@section_subscript", vec![Expr::Section { name: array.clone(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() }]),
                            Expr::Call(t, a) if t == "@section_subscript" => match a.first() {
                                Some(Expr::Section { lower, upper, stride, .. }) => call("@section_subscript", vec![Expr::Section { name: array.clone(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() }]),
                                _ => x.clone(),
                            },
                            other => other.clone(),
                        }).collect()
                    };
                    args.push(handle);
                    return Ok(Some(Place::Value(call(&array, args))));
                }
            }
            i += 1;
        }
        Ok(Some(Place::Handle { value: handle, target, ty, reference, owned }))
    }

    /// The object an expression gives: its handle, its type and whether it
    /// is new (a constructor or a function result) rather than another
    /// object's.
    fn object(&mut self, e: &Expr, vars: &Vars, out: &mut Vec<Stmt>) -> Result<Option<(Expr, String, bool)>> {
        if let Some(Place::Handle { value, ty, .. }) = self.place(e, vars)? {
            return Ok(Some((value, ty, false)));
        }
        let Expr::Call(n, args) = e else { return Ok(None) };
        if self.boxed.contains_key(n) && !vars.contains_key(n) {
            let made = self.construct(n, args, vars, out)?;
            return Ok(Some((made, n.clone(), true)));
        }
        if let Some((ty, pointer)) = self.results.get(n).cloned() {
            if !vars.contains_key(n) {
                let mut e = e.clone();
                self.expr(&mut e, vars, out)?;
                return Ok(Some((e, ty, !pointer)));
            }
        }
        Ok(None)
    }

    /// A structure constructor: a new object with the component values.
    fn construct(&mut self, ty: &str, args: &[Expr], vars: &Vars, out: &mut Vec<Stmt>) -> Result<Expr> {
        let components = self.boxed[ty].components.clone();
        let mut values: Vec<Option<Expr>> = vec![None; components.len()];
        for (i, a) in args.iter().enumerate() {
            let (slot, value) = match a {
                Expr::Call(k, v) if k.starts_with("@keyword:") => {
                    let k = &k["@keyword:".len()..];
                    (components.iter().position(|c| c.name.eq_ignore_ascii_case(k)).with_context(|| format!("type `{ty}` has no component `{k}`"))?, v[0].clone())
                }
                v => (i, v.clone()),
            };
            if slot >= components.len() { bail!("too many values in a structure constructor of `{ty}`"); }
            values[slot] = Some(value);
        }
        let mut actuals = vec![];
        for (c, v) in components.iter().zip(values) {
            let value = match (v, &c.shape) {
                (Some(Expr::Call(n, a)), Shape::Handle { .. }) if n.eq_ignore_ascii_case("null") && a.is_empty() => int(0),
                (Some(v), Shape::Handle { .. }) => match self.object(&v, vars, out)? {
                    Some((h, _, _)) => h,
                    None => bail!("a structure constructor of `{ty}` needs an object or NULL() for component `{}`", c.name),
                },
                (Some(mut v), _) => { self.expr(&mut v, vars, out)?; v }
                (None, Shape::Handle { .. }) => int(0),
                (None, _) => c.default.clone().with_context(|| format!("a structure constructor of `{ty}` needs a value for component `{}`", c.name))?,
            };
            actuals.push(value);
        }
        Ok(self.temporary(call(&format!("rfort_make_{ty}"), actuals), out))
    }

    /// A temporary INTEGER with the value, so that a pool grown in computing
    /// it is not indexed before it grows.
    fn temporary(&mut self, value: Expr, out: &mut Vec<Stmt>) -> Expr {
        self.serial += 1;
        let name = format!("@pool_handle_{}", self.serial);
        out.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![name.clone()] });
        out.push(Stmt::Assign { name: name.clone(), value });
        id(&name)
    }

    /// Lower the designators in an expression; statements it needs first go
    /// to `out`.
    fn expr(&mut self, e: &mut Expr, vars: &Vars, out: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Call(n, args) if (n == "associated" || n == "allocated") && !args.is_empty() => {
                let mut handles = vec![];
                for a in args.iter() {
                    match self.place(a, vars)? {
                        Some(Place::Handle { value, .. }) => handles.push(value),
                        _ => break,
                    }
                }
                if handles.len() == args.len() {
                    let present = bin(BinOp::Ne, handles[0].clone(), int(0));
                    *e = match handles.get(1) {
                        Some(other) => bin(BinOp::And, present, bin(BinOp::Eq, handles[0].clone(), other.clone())),
                        None => present,
                    };
                    return Ok(());
                }
                for a in args.iter_mut() { self.expr(a, vars, out)?; }
            }
            Expr::Call(n, args) if n.eq_ignore_ascii_case("null") && args.is_empty() => {}
            _ => {
                match self.place(e, vars)? {
                    Some(Place::Value(v)) => { *e = v; return self.subscripts(e, vars, out); }
                    Some(Place::Handle { value, .. }) => { *e = value; return Ok(()); }
                    Some(Place::Binding { procedure, handle, args }) => {
                        let mut actuals = vec![handle];
                        for mut a in args { self.expr(&mut a, vars, out)?; actuals.push(a); }
                        *e = call(&procedure, self.by_value(actuals));
                        return Ok(());
                    }
                    None => {}
                }
                if let Expr::Call(n, args) = e {
                    if self.boxed.contains_key(n.as_str()) && !vars.contains_key(n.as_str()) {
                        let (n, args) = (n.clone(), args.clone());
                        *e = self.construct(&n, &args, vars, out)?;
                        return Ok(());
                    }
                }
                let user = matches!(e, Expr::Call(n, _) if self.functions.contains(n.as_str()) && !vars.contains_key(n.as_str()));
                match e {
                    Expr::Call(_, args) | Expr::Index(_, args) => {
                        for a in args.iter_mut() { self.expr(a, vars, out)?; }
                        // A function's pool element actuals are values: the
                        // function may grow the pool they are in.
                        if user { let taken = std::mem::take(args); *args = self.by_value(taken); }
                    }
                    Expr::Bin(_, a, b) => { self.expr(a, vars, out)?; self.expr(b, vars, out)?; }
                    Expr::Un(_, a) => self.expr(a, vars, out)?,
                    Expr::Section { lower, upper, stride, .. } => {
                        for b in [lower.as_deref_mut(), upper.as_deref_mut(), stride.as_deref_mut()].into_iter().flatten() { self.expr(b, vars, out)?; }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn subscripts(&mut self, e: &mut Expr, vars: &Vars, out: &mut Vec<Stmt>) -> Result<()> {
        if let Expr::Call(_, args) = e {
            for a in args.iter_mut() { self.expr(a, vars, out)?; }
        }
        Ok(())
    }

    /// Pool elements as values (`x + 0`), not references into the pool.
    fn by_value(&self, args: Vec<Expr>) -> Vec<Expr> {
        args.into_iter().map(|a| match &a {
            Expr::Call(n, _) if n.starts_with("rfort_") && !n.starts_with("rfort_make_") => bin(BinOp::Add, a, int(0)),
            _ => a,
        }).collect()
    }

    /// Assign the object `value` (of type `ty`; `fresh` when no other object
    /// owns it) to the handle stored at `target`.
    fn store_object(&self, target: &Target, current: Expr, reference: bool, owned: bool, ty: &str, value: Expr, fresh: bool, out: &mut Vec<Stmt>) {
        if owned {
            // An ALLOCATABLE object takes a copy of the value, or the new one.
            let new = if fresh { value } else { call(&format!("rfort_copy_{ty}"), vec![value]) };
            out.push(Stmt::Assign { name: "@pool_value".into(), value: new });
            out.push(sub(&format!("rfort_free_{ty}"), vec![current]));
            out.push(target.assign(id("@pool_value")));
        } else {
            // An object, or the target of a POINTER, takes the value's components.
            let _ = reference;
            out.push(sub(&format!("rfort_assign_{ty}"), vec![current, value.clone()]));
            if fresh { out.push(sub(&format!("rfort_free_{ty}"), vec![value])); }
        }
    }

    fn statements(&mut self, body: &mut Vec<Stmt>, vars: &Vars) -> Result<()> {
        let mut result = vec![];
        for mut s in std::mem::take(body) {
            let mut out = vec![];
            match &mut s {
                Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } => { result.push(s); continue; }
                Stmt::CallSub { name, args } if name == "@pointer_associate" && args.len() == 2 => {
                    if let Some(Place::Handle { target, .. }) = self.place(&args[0], vars)? {
                        let value = match &args[1] {
                            Expr::Call(n, a) if n.eq_ignore_ascii_case("null") && a.is_empty() => int(0),
                            v => match self.object(v, vars, &mut out)? {
                                Some((h, _, _)) => h,
                                None => bail!("pointer assignment to an object of a recursive type needs an object or NULL()"),
                            },
                        };
                        let value = if matches!(value, Expr::Call(..)) { self.temporary(value, &mut out) } else { value };
                        out.push(target.assign(value));
                        result.extend(out);
                        continue;
                    }
                }
                Stmt::CallSub { name, args } if name == "@nullify" => {
                    let mut handled = vec![];
                    for a in args.iter() {
                        if let Some(Place::Handle { target, .. }) = self.place(a, vars)? { handled.push(target.assign(int(0))); }
                    }
                    if handled.len() == args.len() { result.extend(handled); continue; }
                }
                Stmt::CallSub { name, args } if name == "@allocate" => {
                    let source = args.iter().find_map(|a| match a { Expr::Call(k, v) if k == "@keyword:source" => Some(v[0].clone()), _ => None });
                    let mut rest = vec![];
                    for a in args.iter() {
                        if matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:")) { continue; }
                        match self.place(a, vars)? {
                            Some(Place::Handle { target, ty, .. }) => {
                                let value = match &source {
                                    Some(src) => match self.object(src, vars, &mut out)? {
                                        Some((h, _, true)) => h,
                                        Some((h, _, false)) => call(&format!("rfort_copy_{ty}"), vec![h]),
                                        None => bail!("ALLOCATE of an object of type `{ty}` needs an object of the type as SOURCE"),
                                    },
                                    None => call(&format!("rfort_new_{ty}"), vec![]),
                                };
                                let value = self.temporary(value, &mut out);
                                out.push(target.assign(value));
                            }
                            _ => rest.push(a.clone()),
                        }
                    }
                    if rest.len() < args.iter().filter(|a| !matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:"))).count() {
                        if !rest.is_empty() {
                            for a in rest.iter_mut() { self.expr(a, vars, &mut out)?; }
                            out.push(Stmt::CallSub { name: "@allocate".into(), args: rest });
                        }
                        result.extend(out);
                        continue;
                    }
                }
                Stmt::CallSub { name, args } if name == "@deallocate" => {
                    let mut handled = vec![];
                    for a in args.iter() {
                        if let Some(Place::Handle { target, value, ty, .. }) = self.place(a, vars)? {
                            handled.push(sub(&format!("rfort_free_{ty}"), vec![value]));
                            handled.push(target.assign(int(0)));
                        }
                    }
                    if !handled.is_empty() {
                        if handled.len() != 2 * args.iter().filter(|a| !matches!(a, Expr::Call(k, _) if k.starts_with("@keyword:"))).count() {
                            bail!("DEALLOCATE of objects of recursive types together with other objects is not yet supported");
                        }
                        result.extend(handled);
                        continue;
                    }
                }
                Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") && args.len() == 2 => {
                    if let (Some(Place::Handle { target: from, value: f, .. }), Some(Place::Handle { target: to, value: t, ty, .. })) = (self.place(&args[0], vars)?, self.place(&args[1], vars)?) {
                        out.push(sub(&format!("rfort_free_{ty}"), vec![t]));
                        out.push(to.assign(f));
                        out.push(from.assign(int(0)));
                        result.extend(out);
                        continue;
                    }
                }
                // A type-bound subroutine call, p%method(args).
                Stmt::CallSub { name, args } if name.contains('%') => {
                    if let Some(Place::Binding { procedure, handle, .. }) = self.place(&Expr::Call(name.clone(), args.clone()), vars)? {
                        let mut actuals = vec![handle];
                        for mut a in args.clone() { self.expr(&mut a, vars, &mut out)?; actuals.push(a); }
                        let call = Stmt::CallSub { name: procedure, args: actuals };
                        self.copy_in_out(call, &mut out);
                        result.extend(out);
                        continue;
                    }
                }
                Stmt::CallSub { name, args } if !name.starts_with('@') => {
                    for a in args.iter_mut() { self.expr(a, vars, &mut out)?; }
                    let call = std::mem::replace(&mut s, Stmt::Exit);
                    self.copy_in_out(call, &mut out);
                    result.extend(out);
                    continue;
                }
                Stmt::Assign { name, value } => {
                    let target = Expr::Ident(name.clone());
                    match self.place(&target, vars)? {
                        Some(Place::Handle { target, value: current, ty, reference, owned }) => {
                            let Some((v, _, fresh)) = self.object(value, vars, &mut out)? else {
                                bail!("assignment to an object of type `{ty}` needs an object of the type");
                            };
                            let v = if matches!(v, Expr::Call(..)) { self.temporary(v, &mut out) } else { v };
                            self.store_object(&target, current, reference, owned, &ty, v, fresh, &mut out);
                            result.extend(out);
                            continue;
                        }
                        Some(Place::Value(Expr::Call(array, indices))) => {
                            let mut v = value.clone();
                            self.expr(&mut v, vars, &mut out)?;
                            let v = if contains_call(&v) { self.temporary_typed(v, &mut out) } else { v };
                            out.push(Stmt::AssignIndex { name: array, indices, value: v });
                            result.extend(out);
                            continue;
                        }
                        _ => {}
                    }
                }
                Stmt::AssignIndex { name, indices, value } => {
                    let target = Expr::Call(name.clone(), indices.clone());
                    if let Some(Place::Value(Expr::Call(array, mut idx))) = self.place(&target, vars)? {
                        for i in idx.iter_mut() { self.expr(i, vars, &mut out)?; }
                        let mut v = value.clone();
                        self.expr(&mut v, vars, &mut out)?;
                        let v = if contains_call(&v) { self.temporary_typed(v, &mut out) } else { v };
                        out.push(Stmt::AssignIndex { name: array, indices: idx, value: v });
                        result.extend(out);
                        continue;
                    }
                }
                _ => {}
            }
            for e in crate::procedure_arguments::expressions(&mut s) {
                self.expr(e, vars, &mut out)?;
            }
            for b in crate::procedure_arguments::children(&mut s) {
                self.statements(b, vars)?;
            }
            // Statements a condition needs must run each time it does.
            if let Stmt::DoWhile { cond, body } = &mut s {
                if !out.is_empty() {
                    let mut inner = std::mem::take(&mut out);
                    inner.push(Stmt::If { cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())), then_body: vec![Stmt::Exit], else_body: None });
                    inner.append(body);
                    *body = inner;
                    *cond = Expr::Logical(true);
                }
            }
            result.extend(out);
            result.push(s);
        }
        *body = result;
        Ok(())
    }

    /// A temporary for a value whose type the statement's target gives.
    fn temporary_typed(&mut self, value: Expr, out: &mut Vec<Stmt>) -> Expr {
        let _ = out;
        value
    }

    /// A CALL whose actuals are pool elements passes them through
    /// temporaries, copied back after the call: the procedure may grow the
    /// pool while it holds them.
    fn copy_in_out(&mut self, call: Stmt, out: &mut Vec<Stmt>) {
        let Stmt::CallSub { name, mut args } = call else { out.push(call); return };
        let mut back = vec![];
        for a in args.iter_mut() {
            let Expr::Call(array, indices) = a.clone() else { continue };
            if !array.starts_with("rfort_") || array.starts_with("rfort_make_") { continue; }
            let Some(kind) = self.pool_kind(&array) else { continue };
            if indices.len() != 1 { continue; }
            self.serial += 1;
            let temp = format!("@pool_actual_{}", self.serial);
            out.push(Stmt::VarDecl { kind, names: vec![temp.clone()] });
            out.push(Stmt::Assign { name: temp.clone(), value: a.clone() });
            back.push(Stmt::AssignIndex { name: array, indices, value: id(&temp) });
            *a = id(&temp);
        }
        out.push(Stmt::CallSub { name, args });
        out.extend(back);
    }

    /// The type of the elements of a scalar pool array.
    fn pool_kind(&self, array: &str) -> Option<TypeSpec> {
        for (ty, b) in &self.boxed {
            for c in &b.components {
                if pool(ty, &c.name) == array {
                    return match &c.shape {
                        Shape::Handle { .. } => Some(TypeSpec::Integer(None)),
                        Shape::Scalar => Some(c.kind.clone()),
                        Shape::Array(_) => None,
                    };
                }
            }
        }
        None
    }

    /// Lower a scope: its variables of recursive types become handles.
    fn scope(&mut self, body: &mut Vec<Stmt>, host: &Vars, params: &[String], result: Option<&str>, module: bool) -> Result<()> {
        let mut vars = host.clone();
        for s in body.iter() {
            match s {
                Stmt::DerivedDecl { type_name, names, attributes, .. } => for n in names {
                    let reference = attributes.iter().any(|a| a == "pointer" || a == "allocatable");
                    vars.insert(n.clone(), Var { ty: type_name.clone(), reference, owned: attributes.iter().any(|a| a == "allocatable") });
                },
                Stmt::VarDecl { names, .. } => for n in names { vars.remove(n); },
                Stmt::ArrayDecl { name, .. } => { vars.remove(name); }
                _ => {}
            }
        }
        // Procedures see their host's variables.
        for s in body.iter_mut() {
            match s {
                Stmt::Function { name, params, body, return_type, .. } => {
                    let r = match body.last() { Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(), _ => name.clone() };
                    self.scope(body, &vars, &params.clone(), Some(&r), false)?;
                    if let Some(Stmt::VarDecl { kind, .. }) = body.iter().find(|s| matches!(s, Stmt::VarDecl { names, .. } if names.contains(&r))) {
                        if self.results.contains_key(name) { *return_type = Some(kind.clone()); }
                    }
                }
                Stmt::Subroutine { params, body, .. } => self.scope(body, &vars, &params.clone(), None, false)?,
                Stmt::Module { body, .. } => self.scope(body, &vars, &[], None, true)?,
                _ => {}
            }
        }
        self.statements(body, &vars)?;
        // The declarations, and the objects each variable starts with.
        let mut entry = vec![];
        let mut out = vec![];
        for s in std::mem::take(body) {
            match s {
                Stmt::DerivedDecl { type_name, names, attributes, dims, polymorphic } if self.boxed.contains_key(&type_name) => {
                    let reference = attributes.iter().any(|a| a == "pointer" || a == "allocatable");
                    let allocatable = attributes.iter().any(|a| a == "allocatable");
                    for n in names {
                        let dummy = params.contains(&n);
                        if dims.is_empty() {
                            out.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![n.clone()] });
                        } else {
                            out.push(Stmt::ArrayDecl { kind: TypeSpec::Integer(None), name: n.clone(), dims: dims.clone() });
                            if allocatable || attributes.iter().any(|a| a == "pointer") {
                                out.push(Stmt::DummyIntent { name: n.clone(), intent: if allocatable { "allocatable".into() } else { "pointer".into() } });
                            }
                        }
                        for a in &attributes {
                            if a.starts_with("intent") || a == "optional" || a == "save" || a == "value" {
                                out.push(Stmt::DummyIntent { name: n.clone(), intent: a.clone() });
                            }
                        }
                        let _ = polymorphic;
                        if dummy {
                            if attributes.iter().any(|a| a == "intent(out)") {
                                if allocatable && dims.is_empty() {
                                    entry.push(sub(&format!("rfort_free_{type_name}"), vec![id(&n)]));
                                    entry.push(Stmt::Assign { name: n.clone(), value: int(0) });
                                } else if !reference && dims.is_empty() {
                                    entry.push(sub(&format!("rfort_reset_{type_name}"), vec![id(&n)]));
                                }
                            }
                            continue;
                        }
                        if module {
                            if !reference || !dims.is_empty() {
                                bail!("module variable `{n}` of recursive type `{type_name}` must currently be a scalar POINTER or ALLOCATABLE");
                            }
                            out.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names == &[n.clone()]));
                            out.push(Stmt::InitializedDecl { kind: TypeSpec::Integer(None), attributes: vec![], entities: vec![(n.clone(), Some(int(0)))] });
                            continue;
                        }
                        if !dims.is_empty() {
                            if reference { continue; }
                            // Each element of an array of objects is an object.
                            self.serial += 1;
                            let i = format!("@pool_index_{}", self.serial);
                            out.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![i.clone()] });
                            entry.push(Stmt::Do {
                                var: i.clone(), start: call("lbound", vec![id(&n), int(1)]), end: call("ubound", vec![id(&n), int(1)]),
                                body: vec![Stmt::AssignIndex { name: n.clone(), indices: vec![id(&i)], value: call(&format!("rfort_new_{type_name}"), vec![]) }],
                            });
                            if dims.len() > 1 { bail!("arrays of rank above one of recursive type `{type_name}` are not yet supported"); }
                            continue;
                        }
                        let value = if reference { int(0) } else { call(&format!("rfort_new_{type_name}"), vec![]) };
                        entry.push(Stmt::Assign { name: n.clone(), value });
                    }
                    let _ = result;
                }
                Stmt::DerivedType { name, .. } if self.boxed.contains_key(&name) => {}
                Stmt::Accessibility { public, mut names } => {
                    let all = names.is_empty();
                    names.retain(|n| !self.boxed.contains_key(n));
                    if all || !names.is_empty() { out.push(Stmt::Accessibility { public, names }); }
                }
                Stmt::Use { module, nature, only: Some(mut only), mut renames } => {
                    only.retain(|n| !self.boxed.contains_key(n));
                    renames.retain(|(_, r)| !self.boxed.contains_key(r));
                    out.push(Stmt::Use { module, nature, only: Some(only), renames });
                }
                // A component of another type that is a handle.
                Stmt::DerivedType { name, components, bindings } => {
                    let mut kept = vec![];
                    for c in components {
                        match c {
                            Stmt::DerivedDecl { type_name, names, attributes, dims, .. } if self.boxed.contains_key(&type_name) => {
                                if !dims.is_empty() || !attributes.iter().any(|a| a == "pointer" || a == "allocatable") {
                                    bail!("component of recursive type `{type_name}` in type `{name}` must currently be a scalar POINTER or ALLOCATABLE");
                                }
                                kept.push(Stmt::InitializedDecl { kind: TypeSpec::Integer(None), attributes: vec![], entities: names.into_iter().map(|n| (n, Some(int(0)))).collect() });
                            }
                            other => kept.push(other),
                        }
                    }
                    out.push(Stmt::DerivedType { name, components: kept, bindings });
                }
                other => out.push(other),
            }
        }
        // Procedures, whose results are handles, and every unit, use the pools.
        // The objects come after the declarations; a main program's body
        // also holds the definitions of modules and procedures.
        let at = out.iter().position(|s| !specification(s) && !matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }))
            .unwrap_or(out.len());
        out.splice(at..at, entry);
        *body = out;
        Ok(())
    }
}

fn substitute(e: &Expr, values: &HashMap<String, Expr>) -> Expr {
    let mut e = e.clone();
    for _ in 0..16 {
        let mut next = e.clone();
        crate::procedure_arguments::visit_expr(&mut next, &mut |x| if let Expr::Ident(n) = x { if let Some(v) = values.get(n) { *x = v.clone(); } });
        if next == e { break; }
        e = next;
    }
    e
}

fn contains_call(e: &Expr) -> bool {
    match e {
        Expr::Call(n, args) => !n.starts_with('@') || args.iter().any(contains_call),
        Expr::Bin(_, a, b) => contains_call(a) || contains_call(b),
        Expr::Un(_, a) => contains_call(a),
        _ => false,
    }
}

fn specification(s: &Stmt) -> bool {
    match s {
        Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. }
        | Stmt::DerivedType { .. } | Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::DummyIntent { .. } | Stmt::SourceLocation(_)
        | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } => true,
        Stmt::CallSub { name, .. } => name == crate::common::STATEMENT || name == crate::equivalence::STATEMENT,
        _ => false,
    }
}

/// Fortran text of an intrinsic type for the pool module.
fn type_text(kind: &TypeSpec, constants: &HashMap<String, Expr>) -> Result<String> {
    let k = |s: &str| -> Result<i64> {
        let e = match s.strip_prefix(crate::ast::KIND_EXPRESSION) {
            Some(json) => serde_json::from_str(json)?,
            None => match s.parse::<i64>() { Ok(v) => int(v), Err(_) => id(&s.to_ascii_lowercase()) },
        };
        let mut e: Expr = e;
        for _ in 0..16 {
            let mut next = e.clone();
            crate::procedure_arguments::visit_expr(&mut next, &mut |x| if let Expr::Ident(n) = x { if let Some(v) = constants.get(n) { *x = v.clone(); } });
            if next == e { break; }
            e = next;
        }
        crate::parameters::integer_constant(&e).or_else(|_| crate::parameters::constant_integer_with(&e, constants))
            .with_context(|| format!("the kind `{s}` of a component of a recursive type must be constant"))
    };
    Ok(match kind {
        TypeSpec::Integer(None) => "integer".into(),
        TypeSpec::Integer(Some(n)) => format!("integer({n})"),
        TypeSpec::IntegerKind(s) => format!("integer({})", k(s)?),
        TypeSpec::Real => "real".into(),
        TypeSpec::RealKind(s) => format!("real({})", k(s)?),
        TypeSpec::DoublePrecision => "double precision".into(),
        TypeSpec::Logical => "logical".into(),
        TypeSpec::LogicalKind(s) => format!("logical({})", k(s)?),
        TypeSpec::Complex(s) if s.is_empty() => "complex".into(),
        TypeSpec::Complex(s) => format!("complex({})", k(s)?),
        TypeSpec::Character(Some(n)) => format!("character(len={n})"),
        other => bail!("components of type {other:?} in a recursive type are not yet supported"),
    })
}

/// Fortran text of a constant initial value.
fn value_text(e: &Expr) -> Result<String> {
    Ok(match e {
        Expr::IntLit(v) | Expr::RealLit(v) => v.clone(),
        Expr::Logical(b) => if *b { ".true.".into() } else { ".false.".into() },
        Expr::Str(s) => format!("'{}'", s.replace('\'', "''")),
        Expr::Un(crate::ast::UnOp::Neg, a) => format!("(-{})", value_text(a)?),
        Expr::Call(n, items) if n == "@array_constructor" => format!("[{}]", items.iter().map(value_text).collect::<Result<Vec<_>>>()?.join(", ")),
        Expr::Bin(op, a, b) => {
            let o = match op { BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", _ => bail!("unsupported default initialization in a recursive type") };
            format!("({} {o} {})", value_text(a)?, value_text(b)?)
        }
        _ => bail!("this default initialization in a recursive type is not yet supported"),
    })
}

fn zero_text(kind: &TypeSpec) -> &'static str {
    match kind {
        TypeSpec::Logical | TypeSpec::LogicalKind(_) => ".false.",
        TypeSpec::Character(_) => "''",
        _ => "0",
    }
}

/// The Fortran source of the pool module.
fn pool_source(boxed: &HashMap<String, Boxed>, constants: &HashMap<String, Expr>) -> Result<String> {
    let mut names: Vec<&String> = boxed.keys().collect();
    names.sort();
    let mut decl = String::new();
    let mut procs = String::new();
    for ty in names {
        let b = &boxed[ty];
        decl += &format!("integer :: rfort_{ty}_size = 0, rfort_{ty}_used = 0, rfort_{ty}_free = 0\ninteger, allocatable :: rfort_{ty}_link(:)\n");
        let mut grow = format!("subroutine rfort_grow_{ty}()\ninteger :: n\ninteger, allocatable :: t_link(:)\n");
        let mut grow_body = format!("n = max(16, 2 * rfort_{ty}_size)\nallocate(t_link(0:n))\nif (rfort_{ty}_size > 0) t_link(0:rfort_{ty}_size) = rfort_{ty}_link\ncall move_alloc(t_link, rfort_{ty}_link)\n");
        let mut init = String::new();
        let mut free = String::new();
        let mut copy = String::new();
        let mut assign = String::new();
        let mut reset = String::new();
        let mut make_params = vec![];
        let mut make_decl = String::new();
        let mut make_body = String::new();
        for c in &b.components {
            let p = pool(ty, &c.name);
            let (text, bounds) = match &c.shape {
                Shape::Handle { .. } => ("integer".to_owned(), vec![]),
                Shape::Scalar => (type_text(&c.kind, constants)?, vec![]),
                Shape::Array(b) => (type_text(&c.kind, constants)?, b.clone()),
            };
            let colons: Vec<&str> = std::iter::repeat_n(":", bounds.len() + 1).collect();
            decl += &format!("{text}, allocatable :: {p}({})\n", colons.join(","));
            let t = format!("t_{}", c.name);
            grow += &format!("{text}, allocatable :: {t}({})\n", colons.join(","));
            let extents: Vec<String> = bounds.iter().map(|(l, u)| format!("{l}:{u}")).collect();
            // Slot 0 is the null object, which a reference through a null
            // handle, as in `associated(p) .and. associated(p%next)`, reads.
            let alloc_shape: Vec<String> = extents.iter().cloned().chain(std::iter::once("0:n".to_owned())).collect();
            let old_part: Vec<String> = bounds.iter().map(|_| ":".to_owned()).chain(std::iter::once(format!("0:rfort_{ty}_size"))).collect();
            grow_body += &format!("allocate({t}({}))\nif (rfort_{ty}_size > 0) {t}({}) = {p}\ncall move_alloc({t}, {p})\n", alloc_shape.join(","), old_part.join(","));
            let element = |h: &str| -> String {
                let lead: Vec<String> = bounds.iter().map(|_| ":".to_owned()).chain(std::iter::once(h.to_owned())).collect();
                format!("{p}({})", lead.join(","))
            };
            let initial = match (&c.shape, &c.default) {
                (Shape::Handle { .. }, _) => "0".to_owned(),
                (_, Some(d)) => value_text(d)?,
                (_, None) => zero_text(&c.kind).to_owned(),
            };
            init += &format!("{} = {initial}\n", element("h"));
            let arg = format!("a_{}", c.name);
            make_params.push(arg.clone());
            match &c.shape {
                Shape::Handle { target, owned: true } => {
                    free += &format!("k = {p}(h)\n{p}(h) = 0\ncall rfort_free_{target}(k)\n");
                    copy += &format!("k = rfort_copy_{target}({p}(h))\n{p}(n) = k\n");
                    assign += &format!("k = rfort_copy_{target}({p}(src))\ncall rfort_free_{target}({p}(dst))\n{p}(dst) = k\n");
                    reset += &format!("k = {p}(h)\n{p}(h) = 0\ncall rfort_free_{target}(k)\n");
                    make_decl += &format!("integer, intent(in) :: {arg}\n");
                    make_body += &format!("k = rfort_copy_{target}({arg})\n{p}(h) = k\n");
                }
                Shape::Handle { .. } => {
                    copy += &format!("{p}(n) = {p}(h)\n");
                    assign += &format!("{p}(dst) = {p}(src)\n");
                    make_decl += &format!("integer, intent(in) :: {arg}\n");
                    make_body += &format!("{p}(h) = {arg}\n");
                }
                _ => {
                    copy += &format!("{} = {}\n", element("n"), element("h"));
                    assign += &format!("{} = {}\n", element("dst"), element("src"));
                    reset += &format!("{} = {initial}\n", element("h"));
                    let dims = if bounds.is_empty() { String::new() } else { format!("({})", extents.join(",")) };
                    let text = if matches!(c.kind, TypeSpec::Character(_)) { "character(len=*)".to_owned() } else { text.clone() };
                    make_decl += &format!("{text}, intent(in) :: {arg}{dims}\n");
                    make_body += &format!("{} = {arg}\n", element("h"));
                }
            }
        }
        let null = init.replace("(h)", "(0)").replace(",h)", ",0)");
        procs += &format!("{grow}{grow_body}if (rfort_{ty}_size == 0) then\nrfort_{ty}_link(0) = 0\n{null}end if\nrfort_{ty}_size = n\nend subroutine\n");
        procs += &format!("function rfort_new_{ty}() result(h)\ninteger :: h\nif (rfort_{ty}_free /= 0) then\nh = rfort_{ty}_free\nrfort_{ty}_free = rfort_{ty}_link(h)\nelse\nif (rfort_{ty}_used == rfort_{ty}_size) call rfort_grow_{ty}()\nrfort_{ty}_used = rfort_{ty}_used + 1\nh = rfort_{ty}_used\nend if\nrfort_{ty}_link(h) = 0\n{init}end function\n");
        procs += &format!("recursive subroutine rfort_free_{ty}(h)\ninteger, intent(in) :: h\ninteger :: k\nif (h == 0) return\n{free}rfort_{ty}_link(h) = rfort_{ty}_free\nrfort_{ty}_free = h\nend subroutine\n");
        procs += &format!("recursive function rfort_copy_{ty}(h) result(n)\ninteger, intent(in) :: h\ninteger :: n, k\nn = 0\nif (h == 0) return\nn = rfort_new_{ty}()\n{copy}end function\n");
        procs += &format!("recursive subroutine rfort_assign_{ty}(dst, src)\ninteger, intent(in) :: dst, src\ninteger :: k\nif (dst == src) return\n{assign}end subroutine\n");
        procs += &format!("recursive subroutine rfort_reset_{ty}(h)\ninteger, intent(in) :: h\ninteger :: k\n{reset}end subroutine\n");
        procs += &format!("function rfort_make_{ty}({}) result(h)\n{make_decl}integer :: h, k\nh = rfort_new_{ty}()\n{make_body}end function\n", make_params.join(", "));
    }
    Ok(format!("module {MODULE}\nimplicit none\n{decl}contains\n{procs}end module\n"))
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut defs = HashMap::new();
    definitions(&program.body, &mut defs);
    // Types with FINAL procedures or polymorphic components of their own
    // type are left to the diagnostics of the lowering of derived types.
    defs.retain(|_, (components, bindings)| !bindings.iter().any(|(b, _)| b.starts_with("@final"))
        && !components.iter().any(|c| matches!(c, Stmt::DerivedDecl { polymorphic: true, .. })));
    let recursive = recursive_types(&defs);
    if recursive.is_empty() { return Ok(program); }
    // Named constants anywhere, for component kinds and bounds.
    let mut constants = HashMap::new();
    fn named(body: &[Stmt], out: &mut HashMap<String, Expr>) {
        for s in body {
            match s {
                Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) => {
                    for (n, v) in entities { if let Some(v) = v { out.insert(n.clone(), v.clone()); } }
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => named(body, out),
                _ => {}
            }
        }
    }
    named(&program.body, &mut constants);
    let mut boxed = HashMap::new();
    for ty in &recursive {
        let (components, bindings) = &defs[ty];
        let mut b = Boxed::default();
        for (n, p) in bindings {
            if !n.starts_with('@') && !p.starts_with('@') { b.bindings.insert(n.clone(), p.clone()); }
        }
        let defaults: HashMap<String, Expr> = components.iter().flat_map(|c| match c {
            Stmt::InitializedDecl { entities, .. } => entities.iter().filter_map(|(n, v)| v.clone().map(|v| (n.clone(), v))).collect::<Vec<_>>(),
            _ => vec![],
        }).collect();
        let attributes: HashSet<String> = components.iter().filter_map(|c| match c {
            Stmt::DummyIntent { name, intent } if intent == "allocatable" || intent == "pointer" => Some(name.clone()),
            _ => None,
        }).collect();
        for c in components {
            match c {
                Stmt::VarDecl { kind, names } => for n in names {
                    if attributes.contains(n) { bail!("ALLOCATABLE or POINTER component `{n}` of intrinsic type in recursive type `{ty}` is not yet supported"); }
                    b.components.push(Component { name: n.clone(), kind: kind.clone(), shape: Shape::Scalar, default: defaults.get(n).cloned() });
                },
                Stmt::InitializedDecl { kind, entities, .. } => for (n, v) in entities {
                    // An initialized array component also has its ArrayDecl.
                    if components.iter().any(|c| matches!(c, Stmt::ArrayDecl { name, .. } if name == n)) { continue; }
                    b.components.push(Component { name: n.clone(), kind: kind.clone(), shape: Shape::Scalar, default: v.clone() });
                },
                Stmt::ArrayDecl { kind, name, dims } => {
                    if attributes.contains(name) { bail!("ALLOCATABLE or POINTER array component `{name}` in recursive type `{ty}` is not yet supported"); }
                    let mut bounds = vec![];
                    for d in dims {
                        let value = |e: &Expr| crate::parameters::integer_constant(&substitute(e, &constants))
                            .with_context(|| format!("array component `{name}` of recursive type `{ty}` needs constant bounds"));
                        bounds.push(match d {
                            Expr::Call(t, a) if t == "@declared_bounds" && a.len() == 2 => (value(&a[0])?, value(&a[1])?),
                            d => (1, value(d)?),
                        });
                    }
                    b.components.push(Component { name: name.clone(), kind: kind.clone(), shape: Shape::Array(bounds), default: defaults.get(name).cloned() });
                }
                Stmt::DerivedDecl { type_name, names, attributes: a, dims, polymorphic } => {
                    let pointer = a.iter().any(|x| x == "pointer");
                    let owned = a.iter().any(|x| x == "allocatable");
                    if !recursive.contains(type_name) || !dims.is_empty() || !(pointer || owned) || *polymorphic {
                        bail!("component of type `{type_name}` in recursive type `{ty}` is not yet supported: components of a recursive type may be of intrinsic type, or scalar POINTER or ALLOCATABLE components of a recursive type");
                    }
                    for n in names {
                        b.components.push(Component { name: n.clone(), kind: TypeSpec::Integer(None), shape: Shape::Handle { target: type_name.clone(), owned }, default: None });
                    }
                }
                Stmt::DummyIntent { .. } | Stmt::SourceLocation(_) => {}
                Stmt::CallSub { name, .. } if name == crate::extensions::EXTENDS => bail!("extension of recursive type `{ty}` is not yet supported"),
                other => bail!("this component of recursive type `{ty}` is not yet supported: {other:?}"),
            }
        }
        boxed.insert(ty.clone(), b);
    }
    for (child, _) in defs.iter() {
        let _ = child;
    }
    // Extensions of recursive types are not yet supported.
    fn extends(body: &[Stmt], recursive: &HashSet<String>) -> Result<()> {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(c), Expr::Ident(p)] = args.as_slice() {
                        if recursive.contains(p) || recursive.contains(c) { bail!("extension of recursive type `{p}` is not yet supported"); }
                    }
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => extends(body, recursive)?,
                _ => {}
            }
        }
        Ok(())
    }
    extends(&program.body, &recursive)?;
    let others: HashMap<String, HashMap<String, String>> = defs.iter().filter(|(n, _)| !recursive.contains(*n)).map(|(n, (components, _))| {
        (n.clone(), components.iter().flat_map(|c| match c {
            Stmt::DerivedDecl { type_name, names, .. } => names.iter().map(|x| (x.clone(), type_name.clone())).collect::<Vec<_>>(),
            _ => vec![],
        }).collect())
    }).collect();
    // Functions whose results are objects of recursive types.
    let mut results = HashMap::new();
    fn functions(body: &[Stmt], recursive: &HashSet<String>, out: &mut HashMap<String, (String, bool)>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } => {
                    let r = match body.last() { Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(), _ => name.clone() };
                    for d in body {
                        if let Stmt::DerivedDecl { type_name, names, attributes, .. } = d {
                            if names.contains(&r) && recursive.contains(type_name) {
                                out.insert(name.clone(), (type_name.clone(), attributes.iter().any(|a| a == "pointer" || a == "allocatable")));
                            }
                        }
                    }
                    functions(body, recursive, out);
                }
                Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => functions(body, recursive, out),
                _ => {}
            }
        }
    }
    functions(&program.body, &recursive, &mut results);
    let source = pool_source(&boxed, &constants)?;
    // RFORT_DUMP_POOLS=1 prints the generated pool module.
    if std::env::var_os("RFORT_DUMP_POOLS").is_some_and(|v| v == "1") { eprintln!("{source}"); }
    let tokens = crate::lexer::lex(&source);
    let pools = crate::parser::parse_with_src(&source, &tokens, "<rfort_pools>")
        .map_err(|e: Vec<crate::errors::CompileError>| anyhow::anyhow!("internal error in the storage pools of recursive types: {:?}", e.first().map(|e| e.to_string())))?;
    let mut names = HashSet::new();
    fn function_names(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } => { out.insert(name.clone()); function_names(body, out); }
                Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => function_names(body, out),
                _ => {}
            }
        }
    }
    function_names(&program.body, &mut names);
    let mut lower = Lower { boxed, others, results, constants, functions: names, serial: 0 };
    let _ = &lower.constants;
    lower.scope(&mut program.body, &Vars::new(), &[], None, false)?;
    // Every unit uses the pools; the generated temporaries need declaring
    // where their statements are, which the scopes did.
    let use_pools = Stmt::Use { module: MODULE.into(), nature: None, only: None, renames: vec![] };
    for s in program.body.iter_mut() {
        match s {
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body.insert(0, use_pools.clone()),
            _ => {}
        }
    }
    let at = program.body.iter().position(|s| !matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::SourceLocation(_))).unwrap_or(program.body.len());
    program.body.insert(at, use_pools);
    let mut body = pools.body;
    body.append(&mut program.body);
    program.body = body;
    hoist_declarations(&mut program.body);
    Ok(program)
}

/// Move the declarations of generated temporaries, made beside the
/// statements that use them, to the start of their unit.
fn hoist_declarations(body: &mut Vec<Stmt>) {
    fn take(body: &mut Vec<Stmt>, out: &mut Vec<Stmt>) {
        body.retain(|s| {
            let generated = matches!(s, Stmt::VarDecl { names, .. } if names.iter().all(|n| n.starts_with("@pool_")));
            if generated { out.push(s.clone()); }
            !generated
        });
        for s in body.iter_mut() {
            if matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }) { continue; }
            for b in crate::procedure_arguments::children(s) { take(b, out); }
        }
    }
    let mut decls = vec![];
    take(body, &mut decls);
    let needs_value = body_mentions(body, "@pool_value");
    if needs_value { decls.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec!["@pool_value".into()] }); }
    if !decls.is_empty() {
        let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::SourceLocation(_) | Stmt::Module { .. })).unwrap_or(body.len());
        body.splice(at..at, decls);
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => hoist_declarations(body),
            _ => {}
        }
    }
}

fn body_mentions(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { name: n, .. } => n == name,
        Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } => false,
        _ => {
            let mut s = s.clone();
            crate::procedure_arguments::children(&mut s).into_iter().any(|b| body_mentions(b, name))
        }
    })
}

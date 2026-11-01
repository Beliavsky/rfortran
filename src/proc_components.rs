// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Procedure pointer components, `procedure(iface), pointer :: f`. As local
//! procedure pointers do (see crate::procedure_arguments), each becomes an
//! INTEGER tag component: 0 when disassociated, otherwise the index of its
//! target among the procedures ever associated with that component of that
//! type anywhere in the program. `x%f => g`, NULLIFY, ASSOCIATED and
//! structure constructors set or test the tag, and a call `x%f(args)` calls
//! a dispatcher with the interface's dummies that selects the target on the
//! tag, passing `x` first unless the component is NOPASS. The pass runs after
//! module entities have their program-wide names, so a dispatcher in the
//! type's module can call targets of any module.
use crate::ast::{BinOp, CaseBlock, CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
struct Component {
    /// The interface, as the program names it.
    interface: String,
    /// Whether the object is passed, and the dummy it is passed as.
    pass: bool,
    pass_arg: Option<String>,
}

#[derive(Default)]
struct Types {
    /// The procedure pointer components of each type, by name.
    components: HashMap<String, HashMap<String, Component>>,
    /// The derived-type components of each type, with their types.
    derived: HashMap<String, HashMap<String, String>>,
    /// The components of each type in order, for constructors.
    order: HashMap<String, Vec<String>>,
    parents: HashMap<String, String>,
    /// Where each type is defined: a module, or None for the program.
    owner: HashMap<String, Option<String>>,
}

impl Types {
    fn component(&self, ty: &str, c: &str) -> Option<(String, Component)> {
        let mut t = ty.to_owned();
        for _ in 0..64 {
            if let Some(found) = self.components.get(&t).and_then(|m| m.get(c)) { return Some((t, found.clone())); }
            t = self.parents.get(&t)?.clone();
        }
        None
    }
    fn derived(&self, ty: &str, c: &str) -> Option<String> {
        let mut t = ty.to_owned();
        for _ in 0..64 {
            if let Some(found) = self.derived.get(&t).and_then(|m| m.get(c)) { return Some(found.clone()); }
            t = self.parents.get(&t)?.clone();
        }
        None
    }
    fn constructor_order(&self, ty: &str) -> Vec<String> {
        let mut out = self.parents.get(ty).map(|p| self.constructor_order(p)).unwrap_or_default();
        out.extend(self.order.get(ty).cloned().unwrap_or_default());
        out
    }
}

fn collect(body: &mut Vec<Stmt>, module: Option<&str>, types: &mut Types) {
    for s in body.iter_mut() {
        match s {
            Stmt::DerivedType { name, components, .. } => {
                let mut procedures: HashMap<String, (String, bool)> = HashMap::new();
                let mut pointers = HashSet::new();
                let mut pass_args: HashMap<String, String> = HashMap::new();
                for c in components.iter() {
                    if let Stmt::DummyIntent { name: n, intent } = c {
                        if let Some(i) = intent.strip_prefix("procedure(").and_then(|r| r.strip_suffix(')')) {
                            procedures.entry(n.clone()).or_insert((String::new(), true)).0 = i.to_owned();
                        }
                        if intent == "pointer" { pointers.insert(n.clone()); }
                        if intent == "nopass" { procedures.entry(n.clone()).or_insert((String::new(), true)).1 = false; }
                        if let Some(a) = intent.strip_prefix("pass(").and_then(|r| r.strip_suffix(')')) { pass_args.insert(n.clone(), a.to_owned()); }
                    }
                }
                procedures.retain(|n, _| pointers.contains(n));
                let mut order = vec![];
                let mut derived = HashMap::new();
                for c in components.iter() {
                    match c {
                        Stmt::VarDecl { names, .. } => order.extend(names.iter().cloned()),
                        Stmt::ArrayDecl { name, .. } => order.push(name.clone()),
                        Stmt::InitializedDecl { entities, .. } => order.extend(entities.iter().map(|(n, _)| n.clone())),
                        Stmt::DerivedDecl { type_name, names, .. } => {
                            order.extend(names.iter().cloned());
                            for n in names { derived.insert(n.clone(), type_name.clone()); }
                        }
                        Stmt::DummyIntent { name: n, intent } if intent.starts_with("procedure(") && procedures.contains_key(n) => order.push(n.clone()),
                        _ => {}
                    }
                }
                order.dedup();
                if !procedures.is_empty() {
                    // Each tag takes its component's place, which positional
                    // structure constructors follow.
                    let mut placed = HashSet::new();
                    let mut out = vec![];
                    for c in std::mem::take(components) {
                        match c {
                            Stmt::DummyIntent { name: n, .. } if procedures.contains_key(&n) => {
                                if placed.insert(n.clone()) {
                                    out.push(Stmt::InitializedDecl { kind: TypeSpec::Integer(None), attributes: vec![], entities: vec![(n, Some(Expr::IntLit("0".into())))] });
                                }
                            }
                            other => out.push(other),
                        }
                    }
                    *components = out;
                    types.components.insert(name.clone(), procedures.into_iter().map(|(n, (interface, pass))| {
                        let pass_arg = pass_args.get(&n).cloned();
                        (n, Component { interface, pass, pass_arg })
                    }).collect());
                }
                types.derived.insert(name.clone(), derived);
                types.order.insert(name.clone(), order);
                types.owner.insert(name.clone(), module.map(str::to_owned));
            }
            Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() { types.parents.insert(child.clone(), parent.clone()); }
            }
            Stmt::Module { name, body } => { let m = name.clone(); collect(body, Some(&m), types); }
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => collect(body, module, types),
            _ => {}
        }
    }
}

fn procedures(body: &[Stmt], out: &mut HashMap<String, Stmt>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                out.insert(name.clone(), s.clone());
                procedures(body, out);
            }
            Stmt::Module { body, .. } => procedures(body, out),
            _ => {}
        }
    }
}

type Vars = HashMap<String, String>;

/// A designator of a procedure pointer component: its tag, the object that
/// a PASS call passes, the type and component, and where the tag is stored.
struct Place {
    tag: Expr,
    object: Expr,
    key: (String, String),
    target: (String, Vec<Expr>),
    /// A local procedure pointer, which passes no object.
    local: bool,
}

struct Lower<'a> {
    types: &'a Types,
    /// Where each component's call passes the object among the interface's
    /// dummies, with the dummy's name, or None for NOPASS.
    passes: HashMap<(String, String), Option<(usize, String)>>,
    /// The targets of each component; tags number every target of the
    /// program, so that components of different types may share them.
    targets: HashMap<(String, String), Vec<String>>,
    numbering: Vec<String>,
    /// Components associated with other components, which take their targets.
    flows: Vec<((String, String), (String, String))>,
    /// Local procedure pointers associated with components, which share a
    /// component's tags and dispatcher.
    locals: HashMap<String, (String, String)>,
    procedures: &'a HashMap<String, Stmt>,
    collecting: bool,
}

impl Lower<'_> {
    fn place(&self, e: &Expr, vars: &Vars) -> Option<Place> {
        match e {
            Expr::Ident(n) if self.locals.contains_key(n) => Some(Place {
                tag: e.clone(), object: e.clone(), key: self.locals[n].clone(), target: (n.clone(), vec![]), local: true,
            }),
            Expr::Ident(path) | Expr::Call(path, _) if path.contains('%') && !path.starts_with('@') => {
                let (object, field) = path.rsplit_once('%')?;
                let parts: Vec<&str> = object.split('%').collect();
                let mut ty = vars.get(parts[0])?.clone();
                for p in &parts[1..] { ty = self.types.derived(&ty, p)?; }
                let (owner, _) = self.types.component(&ty, field)?;
                Some(Place { tag: Expr::Ident(path.clone()), object: Expr::Ident(object.to_owned()), key: (owner, field.to_owned()), target: (path.clone(), vec![]), local: false })
            }
            Expr::Call(n, args) if n.starts_with("@record_component:") && !args.is_empty() => {
                let field = &n["@record_component:".len()..];
                let Expr::Call(array, idx) = &args[0] else { return None };
                let ty = vars.get(array)?;
                let (owner, _) = self.types.component(ty, field)?;
                Some(Place {
                    tag: Expr::Call(n.clone(), vec![args[0].clone()]),
                    object: args[0].clone(),
                    key: (owner, field.to_owned()),
                    target: (format!("{array}%{field}"), idx.clone()),
                    local: false,
                })
            }
            _ => None,
        }
    }

    fn tag(&mut self, key: &(String, String), procedure: &str) -> Result<Expr> {
        if !self.procedures.contains_key(procedure) {
            bail!("procedure pointer component `{}` target `{}` must be a procedure defined in the program", key.1, shown(procedure));
        }
        if self.collecting {
            let list = self.targets.entry(key.clone()).or_default();
            if !list.iter().any(|p| p == procedure) { list.push(procedure.to_owned()); }
            if !self.numbering.iter().any(|p| p == procedure) { self.numbering.push(procedure.to_owned()); }
        }
        let at = self.numbering.iter().position(|p| p == procedure)
            .with_context(|| format!("internal error: procedure `{procedure}` was not collected as a target"))?;
        Ok(Expr::IntLit((at + 1).to_string()))
    }

    fn dispatcher(key: &(String, String)) -> String {
        let (ty, c) = key;
        let (prefix, base) = match ty.rsplit_once('$') { Some((p, b)) => (format!("{p}$"), b.to_owned()), None => (String::new(), ty.clone()) };
        format!("{prefix}rfort_ppc_{base}_{c}")
    }

    /// The value a component is associated with: NULL(), a procedure, or
    /// another component.
    fn value(&mut self, key: &(String, String), e: &Expr, vars: &Vars) -> Result<Expr> {
        match e {
            Expr::Call(n, a) if n.eq_ignore_ascii_case("null") && a.is_empty() => Ok(Expr::IntLit("0".into())),
            Expr::Ident(p) if self.procedures.contains_key(p) => self.tag(key, p),
            other => match self.place(other, vars) {
                Some(p) => {
                    if self.collecting && p.key != *key { self.flows.push((key.clone(), p.key.clone())); }
                    Ok(p.tag)
                }
                _ => bail!("procedure pointer component `{}` must be associated with a procedure, NULL() or another procedure pointer component", key.1),
            },
        }
    }

    /// A call through the component of the element of an array of records,
    /// `v(i)%f(args)`: the dispatcher with the tag and the element.
    fn element_call(&self, p: Place, args: Vec<Expr>) -> Expr {
        let actuals = self.actuals(&p, args);
        Expr::Call(Self::dispatcher(&p.key), actuals)
    }

    /// The actuals of a dispatcher: the tag, then the call's actuals with the
    /// object: first, or by keyword as the PASS dummy it is.
    fn actuals(&self, p: &Place, args: Vec<Expr>) -> Vec<Expr> {
        let mut actuals = args;
        let pass = if p.local { None } else { self.passes.get(&p.key).cloned().flatten() };
        match pass {
            Some((0, _)) => actuals.insert(0, p.object.clone()),
            Some((_, name)) => actuals.push(Expr::Call(format!("@keyword:{name}"), vec![p.object.clone()])),
            None => {}
        }
        actuals.insert(0, p.tag.clone());
        actuals
    }

    fn expr(&mut self, e: &mut Expr, vars: &Vars) -> Result<()> {
        // v(i)%f(args), which parses as a component with subscripts.
        if let Expr::Call(n, args) = e {
            if n.starts_with("@record_component:") {
                if let Some(Expr::Call(m, call_args)) = args.get(1) {
                    if m == "@component_subscripts" {
                        if let Some(p) = self.place(&Expr::Call(n.clone(), vec![args[0].clone()]), vars) {
                            let mut call_args = call_args.clone();
                            for a in call_args.iter_mut() { self.expr(a, vars)?; }
                            *e = self.element_call(p, call_args);
                            return Ok(());
                        }
                    }
                }
            }
        }
        // Inner expressions first.
        match e {
            Expr::Call(n, args) if !n.starts_with("@record_component:") => {
                if n.eq_ignore_ascii_case("associated") {
                    if let Some(p) = args.first().and_then(|a| self.place(a, vars)) {
                        let present = Expr::Bin(BinOp::Ne, Box::new(p.tag.clone()), Box::new(Expr::IntLit("0".into())));
                        *e = match args.get(1) {
                            Some(target) => {
                                let t = self.value(&p.key, target, vars)?;
                                Expr::Bin(BinOp::And, Box::new(present), Box::new(Expr::Bin(BinOp::Eq, Box::new(p.tag), Box::new(t))))
                            }
                            None => present,
                        };
                        return Ok(());
                    }
                }
                for a in args.iter_mut() { self.expr(a, vars)?; }
            }
            Expr::Bin(_, a, b) => { self.expr(a, vars)?; self.expr(b, vars)?; return Ok(()); }
            Expr::Un(_, a) => { self.expr(a, vars)?; return Ok(()); }
            Expr::Index(_, args) => { for a in args.iter_mut() { self.expr(a, vars)?; } return Ok(()); }
            _ => {}
        }
        // A function reference through a component.
        if let Expr::Call(n, args) = e {
            if n.contains('%') && !n.starts_with('@') || self.locals.contains_key(n.as_str()) {
                if let Some(p) = self.place(&Expr::Ident(n.clone()), vars) {
                    let actuals = self.actuals(&p, std::mem::take(args));
                    *e = Expr::Call(Self::dispatcher(&p.key), actuals);
                    return Ok(());
                }
            }
            // Structure constructors give components their tags.
            if self.types.order.contains_key(n.as_str()) && !vars.contains_key(n.as_str()) {
                let ty = n.clone();
                let order = self.types.constructor_order(&ty);
                for (i, a) in args.iter_mut().enumerate() {
                    let (component, value) = match a {
                        Expr::Call(k, v) if k.starts_with("@keyword:") => (k["@keyword:".len()..].to_owned(), &mut v[0]),
                        other => match order.get(i) { Some(c) => (c.clone(), other), None => continue },
                    };
                    if let Some((owner, _)) = self.types.component(&ty, &component) {
                        *value = self.value(&(owner, component), value, vars)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn statements(&mut self, body: &mut [Stmt], vars: &Vars) -> Result<()> {
        for s in body.iter_mut() {
            match s {
                Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } => continue,
                Stmt::CallSub { name, args } if name == "@pointer_associate" && args.len() == 2 => {
                    if let Some(p) = self.place(&args[0], vars) {
                        let value = self.value(&p.key, &args[1], vars)?;
                        *s = assign(p.target, value);
                        continue;
                    }
                }
                Stmt::CallSub { name, args } if name == "@nullify" => {
                    let places: Vec<Option<Place>> = args.iter().map(|a| self.place(a, vars)).collect();
                    if places.iter().all(Option::is_some) {
                        let out: Vec<Stmt> = places.into_iter().map(|p| assign(p.unwrap().target, Expr::IntLit("0".into()))).collect();
                        *s = Stmt::Block { body: out };
                        continue;
                    }
                }
                // call v(i)%f(args) through the component of an element.
                Stmt::CallSub { name, args } if name.starts_with("@element_call:") && !args.is_empty() => {
                    let field = &name["@element_call:".len()..];
                    let tag = Expr::Call(format!("@record_component:{field}"), vec![args[0].clone()]);
                    if let Some(p) = self.place(&tag, vars) {
                        let mut rest = args[1..].to_vec();
                        for a in rest.iter_mut() { self.expr(a, vars)?; }
                        let Expr::Call(dispatch, actuals) = self.element_call(p, rest) else { unreachable!() };
                        *s = Stmt::CallSub { name: dispatch, args: actuals };
                        continue;
                    }
                }
                // A subroutine call through a component or a local pointer.
                Stmt::CallSub { name, args } if name.contains('%') && !name.starts_with('@') || self.locals.contains_key(name.as_str()) => {
                    if let Some(p) = self.place(&Expr::Ident(name.clone()), vars) {
                        for a in args.iter_mut() { self.expr(a, vars)?; }
                        let actuals = self.actuals(&p, std::mem::take(args));
                        *s = Stmt::CallSub { name: Self::dispatcher(&p.key), args: actuals };
                        continue;
                    }
                }
                _ => {}
            }
            for e in crate::procedure_arguments::expressions(s) { self.expr(e, vars)?; }
            for b in crate::procedure_arguments::children(s) { self.statements(b, vars)?; }
        }
        Ok(())
    }

    /// The key of the component that a local procedure pointer `name` is
    /// associated with, or that is associated with it.
    fn local_key(&self, body: &[Stmt], name: &str, vars: &Vars) -> Option<(String, String)> {
        for s in body {
            if let Stmt::CallSub { name: n, args } = s {
                if n == "@pointer_associate" && args.len() == 2 {
                    if matches!(&args[0], Expr::Ident(p) if p == name) {
                        if let Some(p) = self.place(&args[1], vars) { if !p.local { return Some(p.key); } }
                    }
                    if matches!(&args[1], Expr::Ident(p) if p == name) {
                        if let Some(p) = self.place(&args[0], vars) { if !p.local { return Some(p.key); } }
                    }
                }
            }
            if matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }) { continue; }
            let mut s = s.clone();
            for b in crate::procedure_arguments::children(&mut s) {
                if let Some(k) = self.local_key(b, name, vars) { return Some(k); }
            }
        }
        None
    }

    fn scope(&mut self, body: &mut Vec<Stmt>, host: &Vars, params: &[String]) -> Result<()> {
        let mut vars = host.clone();
        for s in body.iter() {
            match s {
                Stmt::DerivedDecl { type_name, names, .. } => for n in names { vars.insert(n.clone(), type_name.clone()); },
                Stmt::VarDecl { names, .. } => for n in names { vars.remove(n); },
                Stmt::ArrayDecl { name, .. } => { vars.remove(name); }
                _ => {}
            }
        }
        // Local procedure pointers that components associate become tags of
        // the components' kind.
        let saved = self.locals.clone();
        let candidates: Vec<String> = body.iter().filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent.starts_with("procedure(") => Some(name.clone()),
            _ => None,
        }).filter(|n| body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == n && intent == "pointer"))).collect();
        for n in candidates {
            if let Some(key) = self.local_key(body, &n, &vars) {
                self.locals.insert(n.clone(), key);
                body.retain(|s| !matches!(s, Stmt::DummyIntent { name, .. } if *name == n));
                // A dummy has its actual's tag; a local starts disassociated.
                if params.contains(&n) {
                    body.insert(0, Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![n.clone()] });
                } else {
                    body.insert(0, Stmt::InitializedDecl { kind: TypeSpec::Integer(None), attributes: vec![], entities: vec![(n.clone(), Some(Expr::IntLit("0".into())))] });
                }
            }
        }
        self.statements(body, &vars)?;
        for s in body.iter_mut() {
            match s {
                Stmt::Module { body, .. } => self.scope(body, &vars, &[])?,
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    let params = params.clone();
                    self.scope(body, &vars, &params)?
                }
                _ => {}
            }
        }
        self.locals = saved;
        Ok(())
    }
}

fn assign((name, indices): (String, Vec<Expr>), value: Expr) -> Stmt {
    if indices.is_empty() { Stmt::Assign { name, value } } else { Stmt::AssignIndex { name, indices, value } }
}

/// A name as the source spells it.
fn shown(name: &str) -> &str {
    name.rsplit('$').next().unwrap_or(name)
}

/// The dispatcher of a component: the interface's dummies after the tag,
/// selecting the target on the tag.
fn dispatcher(name: &str, interface: &Stmt, targets: &[String], numbering: &[String], pure: bool) -> Result<Stmt> {
    let (function, params, body, iname, return_type) = match interface {
        Stmt::Function { params, body, name, return_type, .. } => (true, params, body, name, return_type.clone()),
        Stmt::Subroutine { params, body, name, .. } => (false, params, body, name, None),
        _ => unreachable!(),
    };
    let result = match body.last() { Some(Stmt::Return(Some(Expr::Ident(r)))) => r.clone(), _ => iname.clone() };
    let tag = "rfort_tag".to_owned();
    let mut out = vec![
        Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![tag.clone()] },
        Stmt::DummyIntent { name: tag.clone(), intent: "intent(in)".into() },
    ];
    for s in body {
        let mut s = s.clone();
        let keep = match &mut s {
            Stmt::VarDecl { names, .. } => { names.retain(|n| params.contains(n) || function && *n == result); !names.is_empty() }
            Stmt::DerivedDecl { names, .. } => { names.retain(|n| params.contains(n) || function && *n == result); !names.is_empty() }
            Stmt::ArrayDecl { name, .. } | Stmt::DummyIntent { name, .. } => params.contains(name),
            Stmt::Use { .. } | Stmt::Implicit(_) => true,
            _ => false,
        };
        if keep { out.push(s); }
    }
    if function { crate::procedure_arguments::rename(&mut out, &result, name); }
    let args: Vec<Expr> = params.iter().map(|p| Expr::Ident(p.clone())).collect();
    let cases = targets.iter().map(|t| CaseBlock {
        items: vec![CaseItem::Single(Expr::IntLit((numbering.iter().position(|p| p == t).unwrap_or(0) + 1).to_string()))],
        body: vec![if function {
            Stmt::Assign { name: name.into(), value: Expr::Call(t.clone(), args.clone()) }
        } else {
            Stmt::CallSub { name: t.clone(), args: args.clone() }
        }],
    }).collect();
    out.push(Stmt::SelectCase {
        expr: Expr::Ident(tag.clone()),
        cases,
        default: Some(vec![Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str(format!("call through disassociated procedure pointer component `{}`", shown(name).trim_start_matches("rfort_ppc_")))] }]),
    });
    let mut params = params.clone();
    params.insert(0, tag);
    Ok(if function {
        Stmt::Function { name: name.into(), params, return_type, body: out, pure, elemental: false }
    } else {
        Stmt::Subroutine { name: name.into(), params, body: out, pure, elemental: false }
    })
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut types = Types::default();
    collect(&mut program.body, None, &mut types);
    if types.components.is_empty() { return Ok(program); }
    let mut all = HashMap::new();
    procedures(&program.body, &mut all);
    let mut keys: Vec<(String, String)> = types.components.iter().flat_map(|(t, cs)| cs.keys().map(move |c| (t.clone(), c.clone()))).collect();
    keys.sort();
    // The targets first (with pass positions that need the interfaces, which
    // an implicit interface takes from the first target), then the rewriting.
    let mut probe = program.body.clone();
    let mut lower = Lower { types: &types, passes: HashMap::new(), targets: HashMap::new(), numbering: vec![], flows: vec![], locals: HashMap::new(), procedures: &all, collecting: true };
    lower.scope(&mut probe, &Vars::new(), &[])?;
    // A component associated with another takes its targets.
    for _ in 0..lower.flows.len() + 1 {
        let mut changed = false;
        for (to, from) in lower.flows.clone() {
            let add = lower.targets.get(&from).cloned().unwrap_or_default();
            let list = lower.targets.entry(to).or_default();
            for t in add { if !list.contains(&t) { list.push(t); changed = true; } }
        }
        if !changed { break; }
    }
    let mut interfaces = HashMap::new();
    let mut passes = HashMap::new();
    for key in &keys {
        let component = &types.components[&key.0][&key.1];
        let owner = types.owner.get(&key.0).cloned().flatten();
        let implicit = component.interface.is_empty().then(|| lower.targets.get(key).and_then(|t| t.first()).and_then(|t| all.get(t))).flatten();
        let interface = implicit.or_else(|| match &owner {
            Some(m) => all.get(&format!("@module${m}${}", component.interface)),
            None => None,
        }).or_else(|| all.get(&component.interface))
            .or_else(|| { let found: Vec<&Stmt> = all.iter().filter(|(n, _)| shown(n) == component.interface).map(|(_, s)| s).collect(); (found.len() == 1).then(|| found[0]) })
            .with_context(|| if component.interface.is_empty() {
                format!("procedure pointer component `{}` has no interface and no target to take one from", key.1)
            } else {
                format!("procedure pointer component `{}` needs an explicit interface; `{}` is not a procedure or abstract interface of the program", key.1, component.interface)
            })?
            .clone();
        let (Stmt::Function { params, .. } | Stmt::Subroutine { params, .. }) = &interface else { unreachable!() };
        let at = match (&component.pass_arg, component.pass) {
            (_, false) => None,
            (Some(a), true) => {
                let i = params.iter().position(|p| p.eq_ignore_ascii_case(a))
                    .with_context(|| format!("PASS({a}) of procedure pointer component `{}` names no dummy of its interface", key.1))?;
                Some((i, params[i].clone()))
            }
            (None, true) => {
                if params.is_empty() { bail!("procedure pointer component `{}` passes its object, but its interface has no dummy arguments; declare it NOPASS", key.1); }
                Some((0, params[0].clone()))
            }
        };
        passes.insert(key.clone(), at);
        interfaces.insert(key.clone(), interface);
    }
    lower.passes = passes;
    lower.collecting = false;
    lower.scope(&mut program.body, &Vars::new(), &[])?;
    let targets = std::mem::take(&mut lower.targets);
    let numbering = std::mem::take(&mut lower.numbering);
    // The dispatchers, in the types' modules (or as internal procedures).
    for key in &keys {
        let owner = types.owner.get(&key.0).cloned().flatten();
        let list = targets.get(key).cloned().unwrap_or_default();
        // A dispatcher of PURE targets is PURE, as array-argument functions need.
        let pure = !list.is_empty() && list.iter().all(|t| matches!(all.get(t), Some(Stmt::Function { pure: true, .. } | Stmt::Subroutine { pure: true, .. })));
        let d = dispatcher(&Lower::dispatcher(key), &interfaces[key], &list, &numbering, pure)?;
        match &owner {
            Some(m) => {
                if let Some(Stmt::Module { body, .. }) = program.body.iter_mut().find(|s| matches!(s, Stmt::Module { name, .. } if name == m)) {
                    body.push(d);
                }
            }
            None => {
                program.contains.push(Lower::dispatcher(key));
                program.body.push(d);
            }
        }
    }
    Ok(program)
}

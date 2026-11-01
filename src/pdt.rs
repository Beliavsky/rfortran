// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Parameterized derived types with constant type parameters. Each distinct
//! set of parameter values that a program uses, as in `type(matrix(8, 3))`,
//! becomes an ordinary derived type whose components have the values in place
//! of the parameters, named like `matrix_pdt_8_3` and defined where the
//! parameterized type is. Inquiries such as `x%k` become the constant values.
//! KIND parameters are always constant.
//!
//! A type with a LEN parameter whose value is known only at run time, assumed
//! (`*`) or deferred (`:`) somewhere in the program instead has, for each set
//! of KIND values, one type whose LEN parameters are INTEGER components of
//! their names, so that `x%n` reads them. Its components that depend on them
//! are ALLOCATABLE, allocated with the object's values when the object comes
//! into being: at the start of its scope, or by ALLOCATE(t(n=...) :: x).
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

/// The parameter list of `TYPE name(p, ...)`, before the type's definition.
const PARAMETERS: &str = "@type_parameters";
/// A type specification with parameter values: `@pdt$name$` and the values.
const SPEC: &str = "@pdt$";
/// A structure constructor with type parameter values: the type with its
/// values, then the component values.
pub(crate) const CONSTRUCTOR: &str = "@pdt_constructor";
/// Where a parameterized type's concrete types go.
const INSTANCES: &str = "@pdt_instances";
/// The value of a LEN parameter of an object of a type in runtime mode.
const RUNTIME: i64 = i64::MIN;

pub(crate) fn parameters(name: &str, params: Vec<String>) -> Stmt {
    let mut args = vec![Expr::Ident(name.to_owned())];
    args.extend(params.into_iter().map(Expr::Ident));
    Stmt::CallSub { name: PARAMETERS.into(), args }
}

pub(crate) fn spec(name: &str, values: Vec<Expr>) -> String {
    format!("{SPEC}{name}${}", serde_json::to_string(&values).expect("serializable type parameters"))
}

fn decode(type_name: &str) -> Option<(String, Vec<Expr>)> {
    let rest = type_name.strip_prefix(SPEC)?;
    let (name, values) = rest.split_once('$')?;
    Some((name.to_owned(), serde_json::from_str(values).ok()?))
}

#[derive(Clone)]
struct Generic {
    /// The name the source gives the type, and the name its instances start with.
    name: String,
    display: String,
    /// The named constants and visible types where the type is defined.
    scope: Env,
    params: Vec<String>,
    kinds: HashSet<String>,
    defaults: HashMap<String, Expr>,
    /// The definition without the parameter declarations.
    components: Vec<Stmt>,
    bindings: Vec<(String, String)>,
    /// The parameterized type this one extends.
    parent: Option<String>,
    /// The type without parameters this one extends.
    plain_parent: Option<String>,
}

/// A scope: `module:name`, or the path of procedures from the program.
type Path = String;
/// A marker in a USE ONLY list or accessibility statement for the instances
/// of the type with this key.
const KEYS: &str = "@pdt_keys:";

#[derive(Default)]
struct State {
    /// Parameterized types by key: the name, or for a name that more than
    /// one scope defines, the name and the scope.
    generics: HashMap<String, Generic>,
    /// The types each module defines or makes available, by local name.
    exports: HashMap<String, HashMap<String, String>>,
    /// Concrete type names by generic, with their parameter values in order.
    instances: HashMap<String, Vec<(String, Vec<i64>)>>,
    pending: Vec<(String, Vec<i64>, String)>,
    /// The named constants of each module.
    modules: HashMap<String, HashMap<String, Expr>>,
    /// The types, with their KIND values (0 for the LEN parameters), whose
    /// LEN parameters are components.
    runtime: HashSet<(String, Vec<i64>)>,
    /// The LEN values of the last instance: the expressions they have where
    /// the type is used, None for KIND parameters and `*` or `:`.
    last_lens: Vec<Option<Expr>>,
    last_serial: usize,
}

#[derive(Clone, Default)]
struct Env {
    /// The kinds of declared variables and named constants.
    kinds: HashMap<String, i64>,
    constants: HashMap<String, Expr>,
    /// The keys of the parameterized types visible by their local names.
    aliases: HashMap<String, String>,
    /// Variables of concrete types: the generic and the parameter values.
    vars: HashMap<String, (String, Vec<i64>)>,
    /// The arrays among them.
    arrays: HashSet<String>,
}

fn named_constants(body: &[Stmt]) -> HashMap<String, Expr> {
    body.iter().flat_map(|s| match s {
        Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) =>
            entities.iter().filter_map(|(n, v)| v.clone().map(|v| (n.clone(), v))).collect::<Vec<_>>(),
        _ => vec![],
    }).collect()
}

fn substitute(e: &Expr, values: &HashMap<String, Expr>) -> Expr {
    let mut e = e.clone();
    crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
        if let Expr::Ident(n) = e {
            if let Some(v) = values.get(n) { *e = v.clone(); }
        }
    });
    e
}

/// The value of a type parameter expression, with the named constants in
/// scope, repeatedly substituted.
fn evaluate(e: &Expr, constants: &HashMap<String, Expr>) -> Result<i64> {
    let mut e = e.clone();
    for _ in 0..16 {
        let next = substitute(&e, constants);
        if next == e { break; }
        e = next;
    }
    crate::parameters::integer_constant(&e).or_else(|_| crate::parameters::constant_integer_with(&e, constants))
}

/// The kind of a literal constant, for KIND(literal): `1_8`, `1.0d0`, `1.0_dp`.
fn literal_kind(e: &Expr, constants: &HashMap<String, Expr>) -> Option<i64> {
    let (text, real) = match e {
        Expr::IntLit(t) => (t, false),
        Expr::RealLit(t) => (t, true),
        Expr::Logical(_) => return Some(4),
        _ => return None,
    };
    match text.rsplit_once('_') {
        Some((_, k)) => evaluate(&match k.parse::<i64>() { Ok(v) => Expr::IntLit(v.to_string()), Err(_) => Expr::Ident(k.to_ascii_lowercase()) }, constants).ok(),
        None if real && text.contains(['d', 'D']) => Some(8),
        None => Some(4),
    }
}

/// Replace KIND(x) of literals and of declared names by their values.
fn kinds_in(e: &mut Expr, env: &Env) {
    crate::procedure_arguments::visit_expr(e, &mut |e| {
        if let Expr::Call(n, a) = e {
            if n.eq_ignore_ascii_case("kind") && a.len() == 1 {
                let k = match &a[0] {
                    Expr::Ident(x) => env.kinds.get(x).copied(),
                    literal => literal_kind(literal, &env.constants),
                };
                if let Some(k) = k { *e = Expr::IntLit(k.to_string()); }
            }
        }
    });
}

/// The kind of a declared variable or named constant, for KIND(x).
fn declared_kind(kind: &TypeSpec, constants: &HashMap<String, Expr>) -> Option<i64> {
    match kind {
        TypeSpec::Integer(k) => Some(k.map_or(4, i64::from)),
        TypeSpec::Real | TypeSpec::Logical => Some(4),
        TypeSpec::DoublePrecision => Some(8),
        TypeSpec::Character(_) | TypeSpec::CharacterLength(_) => Some(1),
        TypeSpec::IntegerKind(k) | TypeSpec::RealKind(k) | TypeSpec::LogicalKind(k) | TypeSpec::Complex(k) => {
            let e = match k.strip_prefix(crate::ast::KIND_EXPRESSION) {
                Some(json) => serde_json::from_str(json).ok()?,
                None => match k.parse::<i64>() { Ok(v) => Expr::IntLit(v.to_string()), Err(_) => Expr::Ident(k.clone()) },
            };
            evaluate(&e, constants).ok()
        }
        _ => None,
    }
}

impl State {
    /// The parameter lists of `TYPE name(...)` and the EXTENDS relations of
    /// `body` and its units.
    fn survey(body: &[Stmt], path: &str, own: &mut HashMap<(Path, String), Vec<String>>, parents: &mut HashMap<(Path, String), String>) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == PARAMETERS => {
                    let names: Vec<String> = args.iter().map(|a| match a { Expr::Ident(n) => n.clone(), _ => unreachable!() }).collect();
                    own.insert((path.to_owned(), names[0].clone()), names[1..].to_vec());
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() { parents.insert((path.to_owned(), child.clone()), parent.clone()); }
                }
                Stmt::Module { name, body } => Self::survey(body, &format!("module:{name}"), own, parents),
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => Self::survey(body, &format!("{path}/{name}"), own, parents),
                _ => {}
            }
        }
    }

    /// Collect the parameterized types of `body` and its units, leaving a
    /// placeholder where each is defined. A type that extends a parameterized
    /// type is one too, with its parent's parameters first.
    fn collect(&mut self, body: &mut Vec<Stmt>, path: &str, own: &HashMap<(Path, String), Vec<String>>, parents: &HashMap<(Path, String), String>,
               keys: &HashMap<(Path, String), String>) -> Result<()> {
        body.retain(|s| match s {
            Stmt::CallSub { name, .. } if name == PARAMETERS => false,
            Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS =>
                !matches!(args.first(), Some(Expr::Ident(child)) if keys.contains_key(&(path.to_owned(), child.clone()))),
            _ => true,
        });
        for at in 0..body.len() {
            let Stmt::DerivedType { name, components, bindings } = &body[at] else { continue };
            let Some(key) = keys.get(&(path.to_owned(), name.clone())).cloned() else { continue };
            let name = name.clone();
            let params = own.get(&(path.to_owned(), name.clone())).cloned().unwrap_or_default();
            let mut kinds = HashSet::new();
            let mut defaults = HashMap::new();
            let mut declared = HashSet::new();
            let mut rest = vec![];
            for c in components.clone() {
                let names: Vec<(String, Option<Expr>, Vec<String>)> = match &c {
                    Stmt::VarDecl { kind: TypeSpec::Integer(_) | TypeSpec::IntegerKind(_), names } => names.iter().map(|n| (n.clone(), None, vec![])).collect(),
                    Stmt::InitializedDecl { kind: TypeSpec::Integer(_) | TypeSpec::IntegerKind(_), attributes, entities } =>
                        entities.iter().map(|(n, v)| (n.clone(), v.clone(), attributes.clone())).collect(),
                    _ => vec![],
                };
                if names.is_empty() || !names.iter().all(|(n, _, _)| params.contains(n)) {
                    if names.iter().any(|(n, _, _)| params.contains(n)) {
                        bail!("type parameters of `{name}` must be declared apart from its components");
                    }
                    rest.push(c);
                    continue;
                }
                for (n, value, attributes) in names {
                    declared.insert(n.clone());
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("kind")) { kinds.insert(n.clone()); }
                    if let Some(v) = value { defaults.insert(n, v); }
                }
            }
            if let Some(p) = params.iter().find(|p| !declared.contains(*p)) {
                bail!("type parameter `{p}` of `{name}` must be declared as an INTEGER KIND or LEN parameter");
            }
            // The parent: a parameterized type of this scope, or the only one
            // of its name, or a type without parameters.
            let declared_parent = parents.get(&(path.to_owned(), name.clone()));
            let parent = declared_parent.and_then(|p| {
                keys.get(&(path.to_owned(), p.clone())).cloned()
                    .or_else(|| { let all: Vec<&String> = keys.iter().filter(|((_, n), _)| n == p).map(|(_, k)| k).collect(); (all.len() == 1).then(|| all[0].clone()) })
            });
            let plain_parent = if parent.is_none() { declared_parent.cloned() } else { None };
            let display = if key == name { name.clone() } else { format!("{name}_{}", self.generics.len() + 1) };
            let definition = Generic { name: name.clone(), display, scope: Env::default(), params, kinds, defaults, components: rest, bindings: bindings.clone(), parent, plain_parent };
            self.generics.insert(key.clone(), definition);
            body[at] = Stmt::CallSub { name: INSTANCES.into(), args: vec![Expr::Ident(key)] };
        }
        for s in body.iter_mut() {
            match s {
                Stmt::Module { name, body } => self.collect(body, &format!("module:{name}"), own, parents, keys)?,
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => self.collect(body, &format!("{path}/{name}"), own, parents, keys)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// The parameterized types each module defines or makes available by
    /// USE, by local name.
    fn find_exports(&mut self, program: &[Stmt]) {
        let modules: Vec<(String, Vec<Stmt>)> = program.iter().filter_map(|s| match s {
            Stmt::Module { name, body } => Some((name.clone(), body.clone())),
            _ => None,
        }).collect();
        for (m, body) in &modules {
            let own: HashMap<String, String> = body.iter().filter_map(|s| match s {
                Stmt::CallSub { name, args } if name == INSTANCES => match args.first() {
                    Some(Expr::Ident(key)) => Some((self.generics[key].name.clone(), key.clone())),
                    _ => None,
                },
                _ => None,
            }).collect();
            self.exports.insert(m.clone(), own);
        }
        for _ in 0..modules.len() + 1 {
            let mut changed = false;
            for (m, body) in &modules {
                for s in body {
                    let Stmt::Use { module, .. } = s else { continue };
                    let imported = self.imported(s, module);
                    let entry = self.exports.entry(m.clone()).or_default();
                    for (l, k) in imported { if !entry.contains_key(&l) { entry.insert(l, k); changed = true; } }
                }
            }
            if !changed { break; }
        }
    }

    /// The parameterized types a USE statement makes visible, by local name.
    fn imported(&self, statement: &Stmt, module: &str) -> Vec<(String, String)> {
        let Stmt::Use { only, renames, .. } = statement else { return vec![] };
        let Some(exported) = self.exports.get(module) else { return vec![] };
        let local = |remote: &String| renames.iter().find(|(_, r)| r == remote).map_or_else(|| remote.clone(), |(l, _)| l.clone());
        exported.iter().filter(|(n, _)| match only {
            Some(list) => list.iter().any(|o| o == *n || renames.iter().any(|(l, r)| l == o && r == *n)),
            None => true,
        }).map(|(n, k)| (local(n), k.clone())).collect()
    }

    /// Give each extension its parent's parameters, defaults and KIND
    /// parameters ahead of its own.
    fn inherit(&mut self) -> Result<()> {
        let names: Vec<String> = self.generics.keys().cloned().collect();
        let mut done: HashSet<String> = HashSet::new();
        fn visit(state: &mut State, name: &str, done: &mut HashSet<String>, depth: usize) -> Result<()> {
            if done.contains(name) { return Ok(()); }
            if depth > 64 { bail!("type `{name}` extends itself"); }
            if let Some(parent) = state.generics[name].parent.clone() {
                visit(state, &parent, done, depth + 1)?;
                let p = state.generics[&parent].clone();
                let g = state.generics.get_mut(name).unwrap();
                if let Some(dup) = g.params.iter().find(|n| p.params.contains(n)) {
                    bail!("type `{name}` declares type parameter `{dup}` of its parent type");
                }
                let mut params = p.params.clone();
                params.extend(g.params.drain(..));
                g.params = params;
                g.kinds.extend(p.kinds);
                for (k, v) in p.defaults { g.defaults.entry(k).or_insert(v); }
            }
            done.insert(name.to_owned());
            Ok(())
        }
        for n in names { visit(self, &n, &mut done, 0)?; }
        Ok(())
    }

    /// The concrete type of `generic` for the given parameter values (by
    /// position or keyword; omitted ones take their defaults).
    fn instance(&mut self, generic: &str, values: &[Expr], constants: &HashMap<String, Expr>) -> Result<(String, Vec<i64>)> {
        let g = self.generics[generic].clone();
        if values.len() > g.params.len() { bail!("type `{generic}` has {} type parameters", g.params.len()); }
        let mut given: Vec<Option<Expr>> = vec![None; g.params.len()];
        let mut keywords = false;
        for (i, v) in values.iter().enumerate() {
            let (slot, value) = match v {
                Expr::Call(k, a) if k.starts_with("@keyword:") => {
                    keywords = true;
                    let k = &k["@keyword:".len()..];
                    let slot = g.params.iter().position(|p| p.eq_ignore_ascii_case(k)).with_context(|| format!("type `{generic}` has no type parameter `{k}`"))?;
                    (slot, a[0].clone())
                }
                _ if keywords => bail!("a positional type parameter value cannot follow a keyword one"),
                v => (i, v.clone()),
            };
            if given[slot].replace(value).is_some() { bail!("type parameter `{}` of `{generic}` is given twice", g.params[slot]); }
        }
        // Defaults may use earlier parameters.
        let mut bound = constants.clone();
        let mut numbers = vec![];
        let mut lens = vec![];
        for (p, v) in g.params.iter().zip(given) {
            let kind = g.kinds.contains(p);
            let (value, len) = match v {
                Some(Expr::Ident(s)) if s == "*" || s == ":" => {
                    let what = if s == "*" { "assumed" } else { "deferred" };
                    if kind { bail!("KIND type parameter `{p}` of `{generic}` cannot be {what}"); }
                    (RUNTIME, None)
                }
                Some(v) if kind => (evaluate(&v, constants).with_context(|| format!("KIND type parameter `{p}` of `{generic}` must be a constant INTEGER expression"))?, None),
                Some(v) => match evaluate(&v, constants) {
                    Ok(n) => (n, Some(Expr::IntLit(n.to_string()))),
                    Err(_) => (RUNTIME, Some(v)),
                },
                None => {
                    let d = g.defaults.get(p).with_context(|| format!("type parameter `{p}` of `{generic}` has no default and needs a value"))?;
                    match evaluate(d, &bound) {
                        Ok(n) => (n, (!kind).then(|| Expr::IntLit(n.to_string()))),
                        Err(_) if !kind => (RUNTIME, Some(substitute(d, &bound))),
                        Err(e) => return Err(e.context(format!("the default of KIND type parameter `{p}` of `{generic}` must be a constant INTEGER expression"))),
                    }
                }
            };
            if let Some(len) = &len { bound.insert(p.clone(), len.clone()); } else if value != RUNTIME { bound.insert(p.clone(), Expr::IntLit(value.to_string())); }
            numbers.push(value);
            lens.push(len);
        }
        // A type with LEN values known only at run time has them as components
        // for every object with the same KIND values.
        let kinds_key: Vec<i64> = g.params.iter().zip(&numbers).map(|(p, v)| if g.kinds.contains(p) { *v } else { 0 }).collect();
        let key = (generic.to_owned(), kinds_key);
        if numbers.contains(&RUNTIME) { self.runtime.insert(key.clone()); }
        if self.runtime.contains(&key) {
            if g.parent.is_some() || g.plain_parent.is_some() {
                bail!("LEN type parameters known only at run time are not yet supported for the extended type `{}`", g.name);
            }
            for (p, n) in g.params.iter().zip(numbers.iter_mut()) { if !g.kinds.contains(p) { *n = RUNTIME; } }
        }
        self.last_lens = lens;
        let list = self.instances.entry(generic.to_owned()).or_default();
        if let Some((name, _)) = list.iter().find(|(_, v)| *v == numbers) { return Ok((name.clone(), numbers)); }
        let suffix: Vec<String> = numbers.iter().map(|v| if *v == RUNTIME { "len".to_owned() } else if *v < 0 { format!("m{}", -v) } else { v.to_string() }).collect();
        let name = format!("{}_pdt_{}", g.display, suffix.join("_"));
        list.push((name.clone(), numbers.clone()));
        self.pending.push((generic.to_owned(), numbers.clone(), name.clone()));
        Ok((name, numbers))
    }

    /// The concrete type name for a declared type name in `env`, if it is a
    /// parameterized type.
    fn resolve_type(&mut self, type_name: &str, env: &Env) -> Result<Option<(String, String, Vec<i64>)>> {
        let (local, mut values) = match decode(type_name) { Some((n, v)) => (n, v), None => (type_name.to_owned(), vec![]) };
        for v in values.iter_mut() { kinds_in(v, env); }
        let generic = env.aliases.get(&local).cloned().unwrap_or(local.clone());
        if !self.generics.contains_key(&generic) {
            if !values.is_empty() { bail!("type `{local}` has no type parameters"); }
            return Ok(None);
        }
        let (name, numbers) = self.instance(&generic, &values, &env.constants)?;
        Ok(Some((name, generic, numbers)))
    }

    /// The statements that give an object of a runtime-mode type its LEN
    /// values and allocate the components that depend on them.
    fn create(&mut self, object: &Object, key: &str, numbers: &[i64], lens: &[Option<Expr>], env: &Env, out: &mut Vec<Stmt>) -> Result<()> {
        let g = self.generics[key].clone();
        let mut values = known(&g, numbers);
        for (p, (n, v)) in g.params.iter().zip(numbers.iter().zip(lens)) {
            if *n != RUNTIME { continue; }
            let v = v.clone().with_context(|| format!("an object of type `{}` needs a value for its LEN type parameter `{p}`", g.name))?;
            out.push(object.assign(p, v.clone()));
            values.insert(p.clone(), v);
        }
        let lens: HashSet<String> = g.params.iter().zip(numbers).filter(|(_, v)| **v == RUNTIME).map(|(p, _)| p.clone()).collect();
        for c in &g.components {
            match c {
                Stmt::VarDecl { kind: TypeSpec::CharacterLength(len), .. } | Stmt::InitializedDecl { kind: TypeSpec::CharacterLength(len), .. } if mentions(len, &lens) => {
                    let names: Vec<String> = match c {
                        Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
                        Stmt::VarDecl { names, .. } => names.clone(),
                        _ => unreachable!(),
                    };
                    for n in names {
                        out.push(Stmt::CallSub { name: crate::substrings::ALLOCATE_CHARACTER.into(), args: vec![substitute(len, &values), object.designator(&n, vec![])] });
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } if kind_mentions(kind, &lens) || dims.iter().any(|d| mentions(d, &lens)) => {
                    let dims: Vec<Expr> = dims.iter().map(|d| substitute(d, &values)).collect();
                    if let TypeSpec::CharacterLength(len) = kind {
                        out.push(Stmt::CallSub { name: crate::substrings::ALLOCATE_CHARACTER.into(), args: vec![substitute(len, &values), object.designator(name, dims)] });
                    } else {
                        out.push(Stmt::CallSub { name: "@allocate".into(), args: vec![object.designator(name, dims)] });
                    }
                }
                Stmt::DerivedDecl { type_name, names, dims, .. } => {
                    let Some((n, v)) = decode(type_name) else { continue };
                    let generic = g.scope.aliases.get(&n).cloned().unwrap_or(n.clone());
                    if !self.generics.contains_key(&generic) { continue; }
                    let mut v: Vec<Expr> = v.iter().map(|e| substitute(e, &values)).collect();
                    for x in v.iter_mut() { kinds_in(x, env); }
                    let (_, inner) = self.instance(&generic, &v, &env.constants)?;
                    let inner_lens = self.last_lens.clone();
                    if !inner.contains(&RUNTIME) { continue; }
                    if !dims.is_empty() { bail!("array components of parameterized type `{n}` with LEN values known only at run time are not yet supported"); }
                    let Object::Path(path) = object else {
                        bail!("arrays of type `{}` with components of parameterized types are not yet supported", g.name);
                    };
                    for name in names {
                        self.create(&Object::Path(format!("{path}%{name}")), &generic, &inner, &inner_lens, env, out)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// ALLOCATE(t(n=...) :: x): allocate `x`, then its components.
    fn allocations(&mut self, body: &mut Vec<Stmt>, env: &Env) -> Result<()> {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::CallSub { name, args } = &s {
                if name == "@allocate_derived_type" {
                    if let Some(Expr::Ident(t)) = args.first() {
                        if let Some((concrete, key, numbers)) = self.resolve_type(t, env)? {
                            let lens = self.last_lens.clone();
                            let objects = args[1..].to_vec();
                            if numbers.contains(&RUNTIME) {
                                out.push(Stmt::CallSub { name: "@allocate".into(), args: objects.clone() });
                                for o in &objects {
                                    let Expr::Ident(path) = o else { bail!("ALLOCATE with type parameters needs variables or components") };
                                    self.create(&Object::Path(path.clone()), &key, &numbers, &lens, env, &mut out)?;
                                }
                            } else {
                                let mut args = vec![Expr::Ident(concrete)];
                                args.extend(objects);
                                out.push(Stmt::CallSub { name: "@allocate_derived_type".into(), args });
                            }
                            continue;
                        }
                    }
                }
            }
            if !matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                for b in crate::procedure_arguments::children(&mut s) { self.allocations(b, env)?; }
            }
            out.push(s);
        }
        *body = out;
        Ok(())
    }

    fn scope(&mut self, body: &mut Vec<Stmt>, host: &Env) -> Result<()> {
        self.scope_with(body, host, &[], false)
    }

    fn scope_with(&mut self, body: &mut Vec<Stmt>, host: &Env, params: &[String], module: bool) -> Result<()> {
        let mut env = host.clone();
        for s in body.iter() {
            if let Stmt::CallSub { name, args } = s {
                if let (true, Some(Expr::Ident(key))) = (name == INSTANCES, args.first()) {
                    env.aliases.insert(self.generics[key].name.clone(), key.clone());
                }
            }
        }
        for s in body.iter_mut() {
            let Stmt::Use { module, .. } = s else { continue };
            let module = module.clone();
            let imported = self.imported(s, &module);
            // ONLY lists name the instances of the types they name, which
            // replace the types.
            if let Stmt::Use { only: Some(list), renames, .. } = s {
                for (local, key) in &imported {
                    let remote = self.generics[key].name.clone();
                    list.retain(|n| n != local && *n != remote);
                    renames.retain(|(l, r)| l != local && *r != remote);
                    let marker = format!("{KEYS}{key}");
                    if !list.contains(&marker) { list.push(marker); }
                }
            }
            env.aliases.extend(imported);
        }
        for s in body.iter() {
            let Stmt::Use { module, only, renames, .. } = s else { continue };
            if let Some(constants) = self.modules.get(module) {
                let local = |remote: &String| renames.iter().find(|(_, r)| r == remote).map_or_else(|| remote.clone(), |(l, _)| l.clone());
                let names: Vec<&String> = match only { Some(list) => list.iter().collect(), None => constants.keys().collect() };
                let found: Vec<(String, Expr)> = names.into_iter().filter_map(|n| constants.get(n).map(|v| (local(n), v.clone()))).collect();
                env.constants.extend(found);
            }
        }
        env.constants.extend(named_constants(body));
        // The scope where each type here is defined, for its components.
        for s in body.iter() {
            if let Stmt::CallSub { name, args } = s {
                if let (true, Some(Expr::Ident(key))) = (name == INSTANCES, args.first()) {
                    self.generics.get_mut(key).unwrap().scope = env.clone();
                }
            }
        }
        for s in body.iter_mut() {
            if let Stmt::Accessibility { names, .. } = s {
                let keys: Vec<String> = names.iter().filter_map(|n| env.aliases.get(n)).map(|k| format!("{KEYS}{k}")).collect();
                names.retain(|n| !env.aliases.contains_key(n));
                names.extend(keys);
            }
        }
        for s in body.iter() {
            let (kind, names): (&TypeSpec, Vec<String>) = match s {
                Stmt::VarDecl { kind, names } => (kind, names.clone()),
                Stmt::ArrayDecl { kind, name, .. } => (kind, vec![name.clone()]),
                Stmt::InitializedDecl { kind, entities, .. } => (kind, entities.iter().map(|(n, _)| n.clone()).collect()),
                _ => continue,
            };
            if let Some(k) = declared_kind(kind, &env.constants) {
                for n in names { env.kinds.insert(n, k); }
            }
        }
        let mut entry = vec![];
        for s in body.iter_mut() {
            match s {
                Stmt::DerivedDecl { type_name, names, attributes, dims, .. } => {
                    if let Some((name, generic, numbers)) = self.resolve_type(type_name, &env)? {
                        let lens = self.last_lens.clone();
                        *type_name = name;
                        for n in names.iter() {
                            env.vars.insert(n.clone(), (generic.clone(), numbers.clone()));
                            if dims.is_empty() { env.arrays.remove(n); } else { env.arrays.insert(n.clone()); }
                        }
                        if numbers.contains(&RUNTIME) {
                            // The actual of a dummy keeps its storage, which
                            // INTENT(OUT) would deallocate.
                            for a in attributes.iter_mut() { if a == "intent(out)" { *a = "intent(inout)".into(); } }
                            let reference = attributes.iter().any(|a| a == "pointer" || a == "allocatable");
                            for n in names.iter() {
                                if reference || params.contains(n) { continue; }
                                if module { bail!("module variable `{n}` of a type with LEN parameters known only at run time is not yet supported"); }
                                match dims.len() {
                                    0 => self.create(&Object::Path(n.clone()), &generic, &numbers, &lens, &env, &mut entry)?,
                                    1 => {
                                        self.last_serial += 1;
                                        let i = format!("@pdt_index_{}", self.last_serial);
                                        entry.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![i.clone()] });
                                        let mut inner = vec![];
                                        self.create(&Object::Element(n.clone(), Expr::Ident(i.clone())), &generic, &numbers, &lens, &env, &mut inner)?;
                                        let bound = |f: &str| Expr::Call(f.into(), vec![Expr::Ident(n.clone()), Expr::IntLit("1".into())]);
                                        entry.push(Stmt::Do { var: i, start: bound("lbound"), end: bound("ubound"), body: inner });
                                    }
                                    _ => bail!("arrays of rank above one of a type with LEN parameters known only at run time are not yet supported"),
                                }
                            }
                        }
                    }
                }
                Stmt::DerivedType { components, .. } => {
                    for c in components.iter_mut() {
                        if let Stmt::DerivedDecl { type_name, .. } = c {
                            if let Some((name, _, _)) = self.resolve_type(type_name, &env)? { *type_name = name; }
                        }
                    }
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let Some(Expr::Ident(parent)) = args.get_mut(1) {
                        if let Some((concrete, _, _)) = self.resolve_type(parent, &env)? { *parent = concrete; }
                    }
                }
                _ => {}
            }
        }
        let inquiries: HashMap<String, Expr> = env.vars.iter().flat_map(|(v, (g, numbers))| {
            self.generics[g].params.iter().zip(numbers).filter(|(_, n)| **n != RUNTIME)
                .map(|(p, n)| (format!("{v}%{p}"), Expr::IntLit(n.to_string()))).collect::<Vec<_>>()
        }).collect();
        if !inquiries.is_empty() {
            for s in body.iter_mut() {
                match s {
                    Stmt::VarDecl { kind, .. } | Stmt::ArrayDecl { kind, .. } | Stmt::InitializedDecl { kind, .. } => *kind = kind_of(kind, &inquiries)?,
                    _ => {}
                }
            }
        }
        self.allocations(body, &env)?;
        self.expressions(body, &env)?;
        for s in body.iter_mut() {
            match s {
                Stmt::Module { body, .. } => self.scope_with(body, &env, &[], true)?,
                Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                    let params = params.clone();
                    self.scope_with(body, &env, &params, false)?
                }
                _ => {}
            }
        }
        // Objects come into being after the declarations.
        if !entry.is_empty() {
            let (declarations, statements): (Vec<Stmt>, Vec<Stmt>) = entry.into_iter().partition(|s| matches!(s, Stmt::VarDecl { .. }));
            let at = body.iter().position(|s| !specification(s)).unwrap_or(body.len());
            body.splice(at..at, statements);
            let first = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
            body.splice(first..first, declarations);
        }
        Ok(())
    }

    /// Rewrite type parameter inquiries and constructors of parameterized
    /// types with default parameters.
    fn expressions(&mut self, body: &mut [Stmt], env: &Env) -> Result<()> {
        let mut error = None;
        for s in body.iter_mut() {
            if matches!(s, Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }) { continue; }
            if let Stmt::Block { body } = s {
                self.scope(body, env)?;
                continue;
            }
            // Constructors with type parameter values first, whose type
            // with its values the second pass would take for a constructor.
            for phase in 0..2 {
            // A component that depends on a LEN value known only at run time
            // is ALLOCATABLE, which a whole array of the objects cannot
            // select.
            // (An assignment with subscripts, as `b(2)%v = 4`, selects an element.)
            let target = match &*s { Stmt::Assign { name, .. } => Some(name.clone()), _ => None };
            if let Some(name) = target { self.check_section(&name, env)?; }
            for e in crate::procedure_arguments::expressions(s) {
                let mut names = vec![];
                crate::procedure_arguments::visit_expr(e, &mut |x| match x {
                    Expr::Ident(n) | Expr::Call(n, _) if n.contains('%') => names.push(n.clone()),
                    _ => {}
                });
                for n in names { self.check_section(&n, env)?; }
            }
            for e in crate::procedure_arguments::expressions(s) {
                crate::procedure_arguments::visit_expr(e, &mut |e| {
                    if error.is_some() { return; }
                    let constructor = matches!(e, Expr::Call(n, _) if n == CONSTRUCTOR || n == "@array_substring");
                    if constructor != (phase == 0) { return; }
                    match e {
                        Expr::Ident(_) | Expr::Call(_, _) if path(e).is_some_and(|(_, fields)| !fields.is_empty()) => {
                            let (object, fields) = path(e).unwrap();
                            if let Some(v) = self.inquire(&object, &fields, env) { *e = Expr::IntLit(v.to_string()); }
                        }
                        // t(values)(components), and t(values)(component), which
                        // parses as a substring.
                        Expr::Call(n, args) if (n == CONSTRUCTOR || n == "@array_substring") && !args.is_empty() => {
                            let Expr::Call(t, values) = &args[0] else { return };
                            let generic = env.aliases.get(t).cloned().unwrap_or(t.clone());
                            if env.vars.contains_key(t) || !self.generics.contains_key(&generic) {
                                if n == CONSTRUCTOR { error = Some(anyhow::anyhow!("`{t}` is not a parameterized derived type")); }
                                return;
                            }
                            let mut values = values.clone();
                            for v in values.iter_mut() { kinds_in(v, env); }
                            match self.instance(&generic, &values, &env.constants) {
                                Ok((name, numbers)) => {
                                    let mut all = self.runtime_values(&generic, &numbers);
                                    all.extend(args[1..].iter().cloned());
                                    *e = Expr::Call(name, all);
                                }
                                Err(err) => error = Some(err),
                            }
                        }
                        // LEN and KIND of components of parameterized types.
                        Expr::Call(n, args) if (n.eq_ignore_ascii_case("len") || n.eq_ignore_ascii_case("kind")) && args.len() == 1 => {
                            if let Some((object, fields)) = path(&args[0]).filter(|(_, f)| !f.is_empty()) {
                                let value = match self.component_type(&object, &fields, env) {
                                    Some(TypeSpec::Character(Some(len))) if n.eq_ignore_ascii_case("len") => Some(len as i64),
                                    Some(kind) if n.eq_ignore_ascii_case("kind") => declared_kind(&kind, &env.constants),
                                    _ => None,
                                };
                                if let Some(v) = value { *e = Expr::IntLit(v.to_string()); }
                            }
                        }
                        // TYPE IS (t(values)) and CLASS IS (t).
                        Expr::Call(n, args) if (n == "@type_is" || n == "@class_is") && args.len() == 1 => {
                            if let Expr::Ident(t) = &args[0] {
                                match self.resolve_type(t, env) {
                                    Ok(Some((name, _, _))) => args[0] = Expr::Ident(name),
                                    Ok(None) => {}
                                    Err(err) => error = Some(err),
                                }
                            }
                        }
                        Expr::Call(n, args) if !env.vars.contains_key(n) => {
                            let generic = env.aliases.get(n).cloned().unwrap_or(n.clone());
                            if self.generics.contains_key(&generic) {
                                match self.instance(&generic, &[], &env.constants) {
                                    Ok((name, numbers)) => {
                                        let lens = self.runtime_values(&generic, &numbers);
                                        args.splice(0..0, lens);
                                        *n = name;
                                    }
                                    Err(err) => error = Some(err.context(format!("the structure constructor `{generic}(...)` needs its type parameters' defaults; constructors with type parameter values are not yet supported"))),
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }
            }
            for b in crate::procedure_arguments::children(s) {
                self.expressions(b, env)?;
            }
        }
        match error { Some(e) => Err(e), None => Ok(()) }
    }

    fn check_section(&self, path: &str, env: &Env) -> Result<()> {
        let mut parts = path.split('%');
        let Some(base) = parts.next() else { return Ok(()) };
        let Some(field) = parts.next() else { return Ok(()) };
        if !env.arrays.contains(base) { return Ok(()); }
        let Some((key, numbers)) = env.vars.get(base) else { return Ok(()) };
        if !numbers.contains(&RUNTIME) { return Ok(()); }
        let g = &self.generics[key];
        let lens: HashSet<String> = g.params.iter().zip(numbers).filter(|(_, v)| **v == RUNTIME).map(|(p, _)| p.clone()).collect();
        let dependent = g.components.iter().any(|c| match c {
            Stmt::VarDecl { kind, names } => names.iter().any(|n| n == field) && kind_mentions(kind, &lens),
            Stmt::ArrayDecl { kind, name, dims } => name == field && (kind_mentions(kind, &lens) || dims.iter().any(|d| mentions(d, &lens))),
            _ => false,
        });
        if dependent {
            bail!("`{base}%{field}` of a whole array whose type has LEN parameters known only at run time is not yet supported; use an element, as in `{base}(i)%{field}`");
        }
        Ok(())
    }

    /// The LEN values of the last instance that are components, which a
    /// structure constructor gives first.
    fn runtime_values(&self, generic: &str, numbers: &[i64]) -> Vec<Expr> {
        let g = &self.generics[generic];
        g.params.iter().zip(numbers).zip(&self.last_lens).filter(|((_, n), _)| **n == RUNTIME)
            .map(|((p, _), v)| v.clone().unwrap_or_else(|| Expr::Ident(p.clone()))).collect()
    }

    /// The value of the type parameter that `object%c1%...%p` names, through
    /// components of parameterized types.
    fn inquire(&mut self, object: &str, fields: &[String], env: &Env) -> Option<i64> {
        let (mut key, mut numbers) = env.vars.get(object)?.clone();
        for (i, f) in fields.iter().enumerate() {
            let g = self.generics.get(&key)?.clone();
            if i + 1 == fields.len() {
                return g.params.iter().position(|p| p == f).map(|i| numbers[i]).filter(|v| *v != RUNTIME);
            }
            let values = known(&g, &numbers);
            // c%len and c%kind of an intrinsic component.
            if i + 2 == fields.len() && matches!(fields[i + 1].as_str(), "len" | "kind") {
                if let Some(kind) = intrinsic_component(&g, f, &values) {
                    return if fields[i + 1] == "len" {
                        match kind { TypeSpec::Character(Some(n)) => Some(n as i64), _ => None }
                    } else {
                        declared_kind(&kind, &g.scope.constants)
                    };
                }
            }
            let type_name = g.components.iter().find_map(|c| match c {
                Stmt::DerivedDecl { type_name, names, .. } if names.contains(f) => Some(type_name.clone()),
                _ => None,
            })?;
            let spec = match decode(&type_name) {
                Some((n, v)) => spec(&n, v.iter().map(|e| substitute(e, &values)).collect()),
                None => type_name,
            };
            let mut scope = g.scope.clone();
            scope.constants.extend(values);
            let (_, k, n) = self.resolve_type(&spec, &scope).ok()??;
            key = k;
            numbers = n;
        }
        None
    }

    /// The type of the intrinsic component that `object%c1%...%c` names.
    fn component_type(&mut self, object: &str, fields: &[String], env: &Env) -> Option<TypeSpec> {
        let (last, inner) = fields.split_last()?;
        let (mut key, mut numbers) = env.vars.get(object)?.clone();
        for f in inner {
            let g = self.generics.get(&key)?.clone();
            let values = known(&g, &numbers);
            let type_name = g.components.iter().find_map(|c| match c {
                Stmt::DerivedDecl { type_name, names, .. } if names.contains(f) => Some(type_name.clone()),
                _ => None,
            })?;
            let spec = match decode(&type_name) {
                Some((n, v)) => spec(&n, v.iter().map(|e| substitute(e, &values)).collect()),
                None => type_name,
            };
            let mut scope = g.scope.clone();
            scope.constants.extend(values);
            let (_, k, n) = self.resolve_type(&spec, &scope).ok()??;
            key = k;
            numbers = n;
        }
        let g = self.generics.get(&key)?.clone();
        let values = known(&g, &numbers);
        intrinsic_component(&g, last, &values)
    }

    /// The concrete definition of a pending instance.
    fn define(&mut self, generic: &str, numbers: &[i64], name: &str) -> Result<Vec<Stmt>> {
        let g = self.generics[generic].clone();
        let mut out = vec![];
        if let Some(parent) = &g.parent {
            let count = self.generics[parent].params.len();
            let values: Vec<Expr> = numbers[..count].iter().map(|v| Expr::IntLit(v.to_string())).collect();
            let (concrete, _) = self.instance(parent, &values, &HashMap::new())?;
            out.push(Stmt::CallSub { name: crate::extensions::EXTENDS.into(), args: vec![Expr::Ident(name.to_owned()), Expr::Ident(concrete)] });
        } else if let Some(parent) = &g.plain_parent {
            out.push(Stmt::CallSub { name: crate::extensions::EXTENDS.into(), args: vec![Expr::Ident(name.to_owned()), Expr::Ident(parent.clone())] });
        }
        let values = known(&g, numbers);
        let lens: HashSet<String> = g.params.iter().zip(numbers).filter(|(_, v)| **v == RUNTIME).map(|(p, _)| p.clone()).collect();
        let mut components = vec![];
        // LEN parameters known only at run time are components first.
        for p in g.params.iter().filter(|p| lens.contains(*p)) {
            components.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![p.clone()] });
        }
        let deferred = |n: usize| vec![Expr::Ident("@deferred_extent".into()); n];
        let allocatable = |n: &str| Stmt::DummyIntent { name: n.to_owned(), intent: "allocatable".into() };
        // Components whose sizes depend on them are ALLOCATABLE.
        let dependent: HashSet<String> = g.components.iter().flat_map(|c| match c {
            Stmt::VarDecl { kind, names } if kind_mentions(kind, &lens) => names.clone(),
            Stmt::InitializedDecl { kind, entities, .. } if kind_mentions(kind, &lens) => entities.iter().map(|(n, _)| n.clone()).collect(),
            Stmt::ArrayDecl { kind, name, dims } if kind_mentions(kind, &lens) || dims.iter().any(|d| mentions(d, &lens)) => vec![name.clone()],
            Stmt::DerivedDecl { names, dims, .. } if dims.iter().any(|d| mentions(d, &lens)) => names.clone(),
            _ => vec![],
        }).collect();
        for c in &g.components {
            let mut c = c.clone();
            match &mut c {
                Stmt::VarDecl { kind, names } if names.iter().any(|n| dependent.contains(n)) => {
                    *kind = TypeSpec::CharacterLength(Box::new(Expr::Ident("@deferred_character".into())));
                    for n in names.iter() { components.push(allocatable(n)); }
                    components.push(c);
                    continue;
                }
                Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| dependent.contains(n)) => {
                    // Their default initialization needs their storage, which
                    // the object's creation gives.
                    if matches!(kind, TypeSpec::CharacterLength(_)) {
                        let names: Vec<String> = entities.iter().map(|(n, _)| n.clone()).collect();
                        for n in &names { components.push(allocatable(n)); }
                        components.push(Stmt::VarDecl { kind: TypeSpec::CharacterLength(Box::new(Expr::Ident("@deferred_character".into()))), names });
                    }
                    continue;
                }
                Stmt::ArrayDecl { kind, name, dims } if dependent.contains(name.as_str()) => {
                    *kind = if kind_mentions(kind, &lens) { TypeSpec::CharacterLength(Box::new(Expr::Ident("@deferred_character".into()))) } else { kind_of(kind, &values)? };
                    *dims = deferred(dims.len());
                    components.push(allocatable(name));
                    components.push(c);
                    continue;
                }
                Stmt::DerivedDecl { dims, attributes, names, .. } if names.iter().any(|n| dependent.contains(n)) => {
                    *dims = deferred(dims.len());
                    if !attributes.iter().any(|a| a == "allocatable") { attributes.push("allocatable".into()); }
                }
                _ => {}
            }
            match &mut c {
                Stmt::VarDecl { kind, .. } => *kind = kind_of(kind, &values)?,
                Stmt::InitializedDecl { kind, entities, .. } => {
                    *kind = kind_of(kind, &values)?;
                    for (_, v) in entities.iter_mut() { if let Some(v) = v { *v = substitute(v, &values); } }
                }
                Stmt::ArrayDecl { kind, dims, .. } => {
                    *kind = kind_of(kind, &values)?;
                    for d in dims.iter_mut() { *d = substitute(d, &values); }
                }
                Stmt::DerivedDecl { type_name, dims, .. } => {
                    for d in dims.iter_mut() { *d = substitute(d, &values); }
                    let mut env = g.scope.clone();
                    env.constants.extend(values.clone());
                    let spec = match decode(type_name) {
                        Some((n, v)) => crate::pdt::spec(&n, v.iter().map(|e| substitute(e, &values)).collect()),
                        None => type_name.clone(),
                    };
                    if let Some((concrete, _, _)) = self.resolve_type(&spec, &env)? { *type_name = concrete; }
                }
                Stmt::Assign { value, .. } => *value = substitute(value, &values),
                _ => {}
            }
            components.push(c);
        }
        out.push(Stmt::DerivedType { name: name.to_owned(), components, bindings: g.bindings.clone() });
        Ok(out)
    }
}

/// Whether a statement belongs to a unit's specification part or defines
/// one of its units.
fn specification(s: &Stmt) -> bool {
    match s {
        Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. }
        | Stmt::DerivedType { .. } | Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::DummyIntent { .. } | Stmt::SourceLocation(_)
        | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. }
        | Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } => true,
        Stmt::CallSub { name, .. } => name == INSTANCES || name == crate::common::STATEMENT || name == crate::equivalence::STATEMENT
            || name == crate::extensions::EXTENDS || name.starts_with("@data"),
        _ => false,
    }
}

/// The parameter values of an instance that are constant.
fn known(g: &Generic, numbers: &[i64]) -> HashMap<String, Expr> {
    g.params.iter().zip(numbers).filter(|(_, v)| **v != RUNTIME).map(|(p, v)| (p.clone(), Expr::IntLit(v.to_string()))).collect()
}

/// Whether an expression uses any of `names`.
fn mentions(e: &Expr, names: &HashSet<String>) -> bool {
    let mut found = false;
    let mut e = e.clone();
    crate::procedure_arguments::visit_expr(&mut e, &mut |x| found |= matches!(x, Expr::Ident(n) if names.contains(n)));
    found
}

fn kind_mentions(kind: &TypeSpec, names: &HashSet<String>) -> bool {
    matches!(kind, TypeSpec::CharacterLength(e) if mentions(e, names))
}

/// An object whose components a runtime-mode type allocates: a variable or
/// component by its path, or an element of a rank-one array of records.
#[derive(Clone)]
enum Object {
    Path(String),
    Element(String, Expr),
}

impl Object {
    fn field(&self, c: &str) -> (String, Option<Expr>) {
        match self {
            Object::Path(p) => (format!("{p}%{c}"), None),
            Object::Element(v, i) => (c.to_owned(), Some(Expr::Call(v.clone(), vec![i.clone()]))),
        }
    }
    fn assign(&self, c: &str, value: Expr) -> Stmt {
        match self {
            Object::Path(p) => Stmt::Assign { name: format!("{p}%{c}"), value },
            Object::Element(v, i) => Stmt::AssignIndex { name: format!("{v}%{c}"), indices: vec![i.clone()], value },
        }
    }
    /// The designator of component `c`, with array bounds `dims`.
    fn designator(&self, c: &str, dims: Vec<Expr>) -> Expr {
        match self.field(c) {
            (path, None) if dims.is_empty() => Expr::Ident(path),
            (path, None) => Expr::Call(path, dims),
            (field, Some(element)) => {
                let mut args = vec![element];
                if !dims.is_empty() { args.push(Expr::Call("@component_subscripts".into(), dims)); }
                Expr::Call(format!("@record_component:{field}"), args)
            }
        }
    }
}

/// The concrete type of intrinsic component `name` of a parameterized type.
fn intrinsic_component(g: &Generic, name: &str, values: &HashMap<String, Expr>) -> Option<TypeSpec> {
    g.components.iter().find_map(|c| match c {
        Stmt::VarDecl { kind, names } if names.iter().any(|n| n == name) => Some(kind.clone()),
        Stmt::ArrayDecl { kind, name: n, .. } if n == name => Some(kind.clone()),
        Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| n == name) => Some(kind.clone()),
        _ => None,
    }).and_then(|k| kind_of(&k, values).ok())
}

/// The variable and component names of a designator `x%a%b` or
/// `v(i)%a%b`, which become `Ident("x%a%b")` or nested record components.
fn path(e: &Expr) -> Option<(String, Vec<String>)> {
    match e {
        Expr::Ident(n) => {
            let mut parts = n.split('%').map(str::to_owned);
            Some((parts.next()?, parts.collect()))
        }
        Expr::Call(n, args) if n.starts_with("@record_component:") && args.len() == 1 => {
            let (object, mut fields) = match &args[0] {
                Expr::Call(v, _) if !v.starts_with('@') => (v.clone(), vec![]),
                inner => path(inner)?,
            };
            fields.push(n["@record_component:".len()..].to_owned());
            Some((object, fields))
        }
        _ => None,
    }
}

/// A component type with the parameter values in place of the parameters.
fn kind_of(kind: &TypeSpec, values: &HashMap<String, Expr>) -> Result<TypeSpec> {
    let text = |s: &str| -> Result<String> {
        if let Some(v) = values.get(s) { return Ok(crate::ast::kind_text(v.clone())); }
        if let Some(json) = s.strip_prefix(crate::ast::KIND_EXPRESSION) {
            let e: Expr = serde_json::from_str(json)?;
            let e = substitute(&e, values);
            return Ok(match crate::parameters::integer_constant(&e) {
                Ok(v) => v.to_string(),
                Err(_) => crate::ast::kind_text(e),
            });
        }
        Ok(s.to_owned())
    };
    Ok(match kind {
        TypeSpec::IntegerKind(s) => crate::ast::integer_kind(Expr::IntLit(text(s)?)),
        TypeSpec::RealKind(s) => TypeSpec::RealKind(text(s)?),
        TypeSpec::Complex(s) => TypeSpec::Complex(text(s)?),
        TypeSpec::LogicalKind(s) => TypeSpec::LogicalKind(text(s)?),
        TypeSpec::CharacterLength(e) => {
            let e = substitute(e, values);
            match crate::parameters::integer_constant(&e) {
                Ok(n) => TypeSpec::Character(Some(n.max(0) as usize)),
                Err(_) => TypeSpec::CharacterLength(Box::new(e)),
            }
        }
        other => other.clone(),
    })
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut state = State::default();
    let (mut own, mut parents) = (HashMap::new(), HashMap::new());
    State::survey(&program.body, "", &mut own, &mut parents);
    if own.is_empty() { return Ok(program); }
    // Extensions of parameterized types are parameterized.
    let mut generic: HashSet<(Path, String)> = own.keys().cloned().collect();
    loop {
        let more: Vec<(Path, String)> = parents.iter()
            .filter(|((path, c), p)| !generic.contains(&(path.clone(), c.clone())) && generic.iter().any(|(_, n)| n == *p))
            .map(|(k, _)| k.clone()).collect();
        if more.is_empty() { break; }
        generic.extend(more);
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, n) in &generic { *counts.entry(n.as_str()).or_default() += 1; }
    let keys: HashMap<(Path, String), String> = generic.iter()
        .map(|(path, n)| ((path.clone(), n.clone()), if counts[n.as_str()] == 1 { n.clone() } else { format!("{n}@{path}") }))
        .collect();
    state.collect(&mut program.body, "", &own, &parents, &keys)?;
    state.inherit()?;
    state.find_exports(&program.body);
    state.modules = program.body.iter().filter_map(|s| match s {
        Stmt::Module { name, body } => Some((name.clone(), named_constants(body))),
        _ => None,
    }).collect();
    // A first run finds the types with LEN values known only at run time,
    // so that all their objects have the same representation.
    let mut probe = program.body.clone();
    state.scope(&mut probe, &Env::default())?;
    state.instances.clear();
    state.pending.clear();
    state.scope(&mut program.body, &Env::default())?;
    // Define the instances, which may use further instances.
    let mut definitions: HashMap<String, Vec<Vec<Stmt>>> = HashMap::new();
    let mut count = 0;
    while let Some((generic, numbers, name)) = state.pending.pop() {
        count += 1;
        if count > 10_000 { bail!("parameterized type `{generic}` has too many instances"); }
        let d = state.define(&generic, &numbers, &name)?;
        definitions.entry(generic).or_default().push(d);
    }
    let names: HashMap<String, Vec<String>> = state.instances.iter().map(|(g, list)| (g.clone(), list.iter().map(|(n, _)| n.clone()).collect())).collect();
    place(&mut program.body, &mut definitions, &names);
    Ok(program)
}

/// Put each type's instances where it was defined, and extend USE ONLY lists
/// and accessibility statements that name it to its instances.
fn place(body: &mut Vec<Stmt>, definitions: &mut HashMap<String, Vec<Vec<Stmt>>>, names: &HashMap<String, Vec<String>>) {
    let mut i = 0;
    while i < body.len() {
        if let Stmt::CallSub { name, args } = &body[i] {
            if name == INSTANCES {
                let Some(Expr::Ident(generic)) = args.first() else { unreachable!() };
                let mut groups = definitions.remove(generic).unwrap_or_default();
                groups.reverse();
                let list: Vec<Stmt> = groups.into_iter().flatten().collect();
                let count = list.len();
                body.splice(i..=i, list);
                i += count;
                continue;
            }
        }
        match &mut body[i] {
            // The markers become the instances' names.
            Stmt::Use { only: Some(list), .. } | Stmt::Accessibility { names: list, .. } => {
                let mut out = vec![];
                let mut marked = false;
                for n in list.drain(..) {
                    match n.strip_prefix(KEYS) {
                        Some(key) => {
                            marked = true;
                            for e in names.get(key).into_iter().flatten() { if !out.contains(e) { out.push(e.clone()); } }
                        }
                        None => out.push(n),
                    }
                }
                *list = out;
                // PUBLIC :: t of a type without instances names nothing; an
                // empty list would set the default accessibility instead.
                if marked && list.is_empty() && matches!(body[i], Stmt::Accessibility { .. }) {
                    body.remove(i);
                    continue;
                }
            }
            Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => place(body, definitions, names),
            _ => {}
        }
        i += 1;
    }
}

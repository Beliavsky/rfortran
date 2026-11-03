// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Tagged storage for unlimited polymorphic (`CLASS(*)`) ALLOCATABLE objects
//! of any rank and for `CLASS(t)` ALLOCATABLE arrays.
//!
//! Each such object becomes an integer tag and one ordinary ALLOCATABLE of
//! the object's rank for each dynamic type it can have: 0 means unallocated,
//! and i means the i-th storage holds the value. The dynamic types of a
//! `CLASS(t)` object are the nonabstract extensions of `t`; those of a
//! `CLASS(*)` object are the types the program allocates or assigns to such
//! objects. Every object of one declared type has the same layout, so a
//! dummy argument receives the tag and the storages of its actual.
//!
//! ALLOCATE, DEALLOCATE, MOVE_ALLOC, assignment to the whole object and
//! ALLOCATED work on the tag and the selected storage. SELECT TYPE becomes a
//! SELECT CASE on the tag whose branches name the storage directly. Any
//! other statement that names the object runs once for each dynamic type,
//! with the object replaced by that type's storage.
use super::*;
use crate::ast::{BinOp, CaseBlock, TypeSpec};
use std::collections::BTreeSet;

const DEFERRED: &str = "@deferred_extent";

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn set(n: &str, v: Expr) -> Stmt {
    Stmt::Assign { name: n.into(), value: v }
}
fn stop(message: String) -> Vec<Stmt> {
    vec![Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str(message)] }]
}
fn keyword(e: &Expr) -> Option<(&str, &Expr)> {
    crate::named_arguments::keyword(e)
}
fn compare(op: BinOp, tag: &str, value: usize) -> Expr {
    Expr::Bin(op, Box::new(id(tag)), Box::new(int(value)))
}
fn describe(kind: &str) -> String {
    match intrinsic::decode(kind) {
        Some(TypeSpec::Integer(None)) => "INTEGER".into(),
        Some(TypeSpec::Integer(Some(k))) => format!("INTEGER({k})"),
        Some(TypeSpec::Real) => "REAL".into(),
        Some(TypeSpec::DoublePrecision) => "REAL(8)".into(),
        Some(TypeSpec::Complex(k)) => format!("COMPLEX({k})"),
        Some(TypeSpec::Logical) => "LOGICAL".into(),
        Some(TypeSpec::Character(_)) => "CHARACTER".into(),
        Some(k) => format!("{k:?}"),
        None => format!("TYPE({kind})"),
    }
}

/// Whether a declared object gets tagged storage.
fn candidate(v: &Object) -> bool {
    v.polymorphic && v.attrs.iter().any(|a| a == "allocatable") && !v.attrs.iter().any(|a| a == "pointer")
        && (v.kind == "*" || v.rank > 0)
}

#[derive(Clone)]
struct Tagged {
    display: String,
    declared: String,
    rank: usize,
    tag: String,
    readonly: bool,
    /// Dynamic types and their storage, in layout order.
    variants: Vec<(String, String)>,
}
type Tags = HashMap<String, Tagged>;

/// A procedure with tagged dummy arguments: its interface, and the declared
/// type and rank of each tagged dummy by position.
struct Procedure {
    signature: crate::named_arguments::Signature,
    dummies: HashMap<usize, (String, usize, bool)>,
}

struct Lower<'a> {
    defs: &'a Definitions,
    /// The dynamic types of CLASS(*) objects.
    unlimited: Vec<String>,
    procedures: HashMap<String, Procedure>,
    serial: usize,
}

/// The type of a SOURCE=, MOLD= or assigned value, encoded as SELECT TYPE
/// encodes kinds.
fn value_kind(e: &Expr, vars: &Objects, defs: &Definitions) -> Result<String> {
    let kind = match e {
        Expr::Ident(n) => match vars.get(n) {
            Some(v) if v.polymorphic => bail!("a polymorphic SOURCE=, MOLD= or value `{n}` is not yet supported here"),
            Some(v) => Some(v.kind.clone()),
            None => None,
        },
        Expr::Call(n, args) | Expr::Index(n, args) if vars.get(n).is_some_and(|v| intrinsic::decode(&v.kind).is_none()) => {
            let v = &vars[n];
            if v.polymorphic { bail!("a polymorphic SOURCE=, MOLD= or value `{n}` is not yet supported here"); }
            let _ = args;
            Some(v.kind.clone())
        }
        Expr::Call(n, _) if defs.fields.contains_key(n) => Some(n.clone()),
        Expr::Call(n, args) if n == "@typed_constructor" => match args.first() {
            Some(Expr::Str(t)) => serde_json::from_str::<TypeSpec>(t).ok().map(|t| intrinsic::encode(&t)),
            _ => None,
        },
        Expr::Call(n, args) if n == "@array_constructor" || ["reshape", "spread", "transpose", "pack", "cshift", "eoshift", "merge"].contains(&n.as_str()) => {
            match args.first().map(|a| keyword(a).map_or(a, |(_, v)| v)) {
                Some(first) => Some(value_kind(first, vars, defs)?),
                None => None,
            }
        }
        // An implied DO: the control, then the items.
        Expr::Call(n, args) if n.contains("implied_do") => match args.get(4) {
            Some(item) => Some(value_kind(item, vars, defs)?),
            None => None,
        }
        _ => None,
    };
    let kind = match kind {
        Some(k) => k,
        None => match intrinsic::actual(e, vars)? {
            Some((k, _)) => intrinsic::encode(&k),
            None => bail!("cannot determine the type of the value for a polymorphic ALLOCATABLE object; use a type specification"),
        },
    };
    if matches!(intrinsic::decode(&kind), Some(TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed)) {
        bail!("CHARACTER values of polymorphic ALLOCATABLE objects are not yet supported");
    }
    Ok(kind)
}

/// The type a type specification of ALLOCATE names, if any.
fn spec_kind(name: &str, args: &[Expr]) -> Result<Option<String>> {
    Ok(match (name, args.first()) {
        ("@allocate_derived_type", Some(Expr::Ident(k))) => Some(k.clone()),
        ("@allocate_intrinsic_type", Some(e)) => {
            let kind = intrinsic::guard_kind(e)?.context("invalid ALLOCATE type specification")?;
            if matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed) {
                bail!("CHARACTER values of polymorphic ALLOCATABLE objects are not yet supported");
            }
            Some(intrinsic::encode(&kind))
        }
        _ => None,
    })
}

fn allocation(name: &str) -> bool {
    matches!(name, "@allocate" | "@allocate_derived_type" | "@allocate_intrinsic_type")
}

/// The object an ALLOCATE or DEALLOCATE item names.
fn item(a: &Expr) -> Option<&str> {
    match a {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } if !n.starts_with('@') => Some(n),
        _ => None,
    }
}

/// Collect the dynamic types of CLASS(*) objects: the types allocated,
/// assigned or moved into them anywhere in the program.
fn collect(body: &[Stmt], inherited: &Objects, defs: &Definitions, out: &mut BTreeSet<String>) -> Result<()> {
    let vars = objects(body, inherited);
    let unlimited = |n: &str| vars.get(n).is_some_and(|v| candidate(v) && v.kind == "*");
    let mut location = None;
    for s in body {
        if let Some(p) = crate::source_diagnostics::statement_offset(s) {
            location = Some(p);
        }
        (|| -> Result<()> {
            match s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    return collect(body, &vars, defs, out);
                }
                Stmt::CallSub { name, args } if allocation(name) => {
                    let start = usize::from(name != "@allocate");
                    if args[start..].iter().filter_map(item).any(unlimited) {
                        if let Some(k) = spec_kind(name, args)? {
                            out.insert(k);
                        }
                        for a in args {
                            if let Some(("source" | "mold", v)) = keyword(a) {
                                if !matches!(v, Expr::Ident(n) if vars.get(n).is_some_and(candidate)) {
                                    out.insert(value_kind(v, &vars, defs)?);
                                }
                            }
                        }
                    }
                }
                Stmt::Assign { name, value } if unlimited(name) => {
                    if !matches!(value, Expr::Ident(n) if vars.get(n).is_some_and(candidate)) {
                        out.insert(value_kind(value, &vars, defs)?);
                    }
                }
                Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => {
                    if let [from, to] = args.as_slice() {
                        let (from, to) = (keyword(from).map_or(from, |(_, v)| v), keyword(to).map_or(to, |(_, v)| v));
                        if matches!(to, Expr::Ident(n) if unlimited(n)) && !matches!(from, Expr::Ident(n) if vars.get(n).is_some_and(candidate)) {
                            out.insert(value_kind(from, &vars, defs)?);
                        }
                    }
                }
                _ => {}
            }
            let mut copy = s.clone();
            for b in children(&mut copy) {
                if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
                    collect_nested(b, &vars, defs, out)?;
                }
            }
            Ok(())
        })()
        .map_err(|e| crate::source_diagnostics::at(e, location))?;
    }
    Ok(())
}

/// A nested statement body shares its scope's declarations.
fn collect_nested(body: &[Stmt], vars: &Objects, defs: &Definitions, out: &mut BTreeSet<String>) -> Result<()> {
    collect(body, vars, defs, out)
}


fn procedures(body: &[Stmt], inherited: &Objects, out: &mut HashMap<String, Procedure>) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let vars = objects(body, inherited);
                let dummies: HashMap<_, _> = params.iter().enumerate().filter_map(|(i, p)| {
                    let v = vars.get(p).filter(|v| candidate(v))?;
                    Some((i, (v.kind.clone(), v.rank, v.attrs.iter().any(|a| a == "intent(in)"))))
                }).collect();
                if !dummies.is_empty() {
                    out.insert(name.clone(), Procedure { signature: crate::named_arguments::Signature::new(params, body), dummies });
                }
                procedures(body, &vars, out);
            }
            Stmt::Module { body, .. } => procedures(body, inherited, out),
            _ => {}
        }
    }
}

/// Whether a statement, nested bodies included, names `n`.
fn uses(s: &Stmt, n: &str) -> bool {
    crate::leak_warnings::mentions(s, n)
}

impl Lower<'_> {
    fn layout(&self, declared: &str) -> Vec<String> {
        if declared == "*" {
            return self.unlimited.clone();
        }
        let mut kinds: Vec<String> = self.defs.fields.keys()
            .filter(|k| !self.defs.abstracts.contains(*k) && self.defs.distance(k, declared).is_some())
            .cloned().collect();
        kinds.sort();
        kinds
    }

    fn storage_declarations(&self, t: &Tagged, intent: Option<&str>, save: bool) -> Vec<Stmt> {
        let mut out = vec![if save {
            Stmt::InitializedDecl { kind: TypeSpec::Integer(None), attributes: vec!["save".into()], entities: vec![(t.tag.clone(), Some(int(0)))] }
        } else {
            Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![t.tag.clone()] }
        }];
        if let Some(intent) = intent {
            out.push(Stmt::DummyIntent { name: t.tag.clone(), intent: intent.into() });
        }
        let dims = vec![id(DEFERRED); t.rank];
        for (kind, storage) in &t.variants {
            match intrinsic::decode(kind) {
                Some(k) => {
                    // Attributes precede the declaration, as parsed.
                    out.push(Stmt::DummyIntent { name: storage.clone(), intent: "allocatable".into() });
                    out.push(if t.rank == 0 {
                        Stmt::VarDecl { kind: k, names: vec![storage.clone()] }
                    } else {
                        Stmt::ArrayDecl { kind: k, name: storage.clone(), dims: dims.clone() }
                    });
                    if let Some(intent) = intent {
                        out.push(Stmt::DummyIntent { name: storage.clone(), intent: intent.into() });
                        // Storage of another dynamic type is unallocated.
                        out.push(Stmt::DummyIntent { name: storage.clone(), intent: "@tagged_storage".into() });
                    }
                    if save {
                        out.push(Stmt::DummyIntent { name: storage.clone(), intent: "save".into() });
                    }
                }
                None => {
                    let mut attributes = vec!["allocatable".to_string()];
                    attributes.extend(intent.map(String::from));
                    if save { attributes.push("save".into()); }
                    out.push(Stmt::DerivedDecl { type_name: kind.clone(), names: vec![storage.clone()], attributes, polymorphic: false, dims: dims.clone() });
                }
            }
        }
        out
    }

    /// DEALLOCATE whichever storage holds the value, and clear the tag.
    fn release(&self, t: &Tagged, unallocated: Vec<Stmt>) -> Vec<Stmt> {
        vec![self.cases(t, |_, storage| vec![Stmt::CallSub { name: "@deallocate".into(), args: vec![id(storage)] }], unallocated),
             set(&t.tag, int(0))]
    }

    fn cases(&self, t: &Tagged, mut branch: impl FnMut(&str, &str) -> Vec<Stmt>, default: Vec<Stmt>) -> Stmt {
        Stmt::SelectCase {
            expr: id(&t.tag),
            cases: t.variants.iter().enumerate().map(|(i, (k, s))| CaseBlock { items: vec![CaseItem::Single(int(i + 1))], body: branch(k, s) }).collect(),
            default: Some(default),
        }
    }

    fn index(&self, t: &Tagged, kind: &str) -> Result<usize> {
        t.variants.iter().position(|(k, _)| k == kind).map(|i| i + 1).with_context(|| {
            if t.declared == "*" {
                format!("{} is not a dynamic type of `{}`", describe(kind), t.display)
            } else {
                format!("{} must extend declared type `{}` of `{}`", describe(kind), t.declared, t.display)
            }
        })
    }

    fn scope(&mut self, body: &mut Vec<Stmt>, params: &mut Vec<String>, inherited: &Tags, outer: &Objects, function: Option<&str>, module: bool) -> Result<()> {
        let vars = objects(body, outer);
        let local: HashSet<String> = objects(body, &Objects::new()).into_keys().collect();
        let mut tags: Tags = inherited.iter().filter(|(n, _)| !local.contains(*n)).map(|(n, t)| (n.clone(), t.clone())).collect();
        let mut found: Vec<(String, Object)> = vars.iter()
            .filter(|(n, v)| local.contains(*n) && !n.contains('%') && candidate(v))
            .map(|(n, v)| (n.clone(), v.clone())).collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let mut generated: HashMap<String, Vec<Stmt>> = HashMap::new();
        let mut entry = vec![];
        for (n, v) in &found {
            if module {
                bail!("module polymorphic ALLOCATABLE objects are not yet supported");
            }
            if Some(n.as_str()) == function || body.iter().any(|s| matches!(s, Stmt::Return(Some(Expr::Ident(r))) if r == n)) {
                bail!("polymorphic ALLOCATABLE function results are not yet supported");
            }
            self.serial += 1;
            let stem = format!("tagged{}", self.serial);
            let t = Tagged {
                display: n.clone(),
                declared: v.kind.clone(),
                rank: v.rank,
                tag: format!("{stem}$tag"),
                readonly: v.attrs.iter().any(|a| a == "intent(in)"),
                variants: self.layout(&v.kind).into_iter().enumerate().map(|(i, k)| (k, format!("{stem}$v{i}"))).collect(),
            };
            let dummy = params.contains(n);
            let intent = dummy.then(|| if t.readonly { "intent(in)" } else { "intent(inout)" });
            let save = !dummy && v.attrs.iter().any(|a| a == "save");
            generated.insert(n.clone(), self.storage_declarations(&t, intent, save));
            if !dummy && !save {
                entry.push(set(&t.tag, int(0)));
            } else if v.attrs.iter().any(|a| a == "intent(out)") {
                entry.extend(self.release(&t, vec![]));
            }
            tags.insert(n.clone(), t);
        }
        if !found.is_empty() {
            let mut cleaned = vec![];
            for mut s in std::mem::take(body) {
                match &mut s {
                    Stmt::DerivedDecl { names, .. } if names.iter().any(|n| generated.contains_key(n)) => {
                        let mut kept = vec![];
                        for n in std::mem::take(names) {
                            match generated.remove(&n) {
                                Some(declarations) => cleaned.extend(declarations),
                                None => kept.push(n),
                            }
                        }
                        *names = kept;
                        if names.is_empty() { continue; }
                    }
                    Stmt::DummyIntent { name, .. } if found.iter().any(|(n, _)| n == name) => continue,
                    _ => {}
                }
                cleaned.push(s);
            }
            // Generated initialization follows the specification part.
            let at = cleaned.iter().position(|s| !matches!(s,
                Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } | Stmt::DerivedType { .. } | Stmt::DerivedDecl { .. }
                | Stmt::Accessibility { .. } | Stmt::DummyIntent { .. } | Stmt::VarDecl { .. } | Stmt::InitializedDecl { .. }
                | Stmt::ArrayDecl { .. } | Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_) | Stmt::Module { .. }
                | Stmt::Function { .. } | Stmt::Subroutine { .. }) && !matches!(s, Stmt::CallSub { name, .. } if name == "@extends"))
                .unwrap_or(cleaned.len());
            cleaned.splice(at..at, entry);
            *body = cleaned;
            *params = std::mem::take(params).into_iter().flat_map(|p| match tags.get(&p).filter(|_| found.iter().any(|(n, _)| n == &p)) {
                Some(t) => std::iter::once(t.tag.clone()).chain(t.variants.iter().map(|(_, s)| s.clone())).collect(),
                None => vec![p],
            }).collect();
        }
        let vars = objects(body, outer);
        self.body(body, &tags, &vars)
    }

    fn body(&mut self, body: &mut Vec<Stmt>, tags: &Tags, vars: &Objects) -> Result<()> {
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) {
                location = Some(p);
            }
            let lowered = (|| -> Result<Vec<Stmt>> {
                match &mut s {
                    Stmt::Function { name, params, body, .. } => {
                        let name = name.clone();
                        self.scope(body, params, tags, vars, Some(&name), false)?;
                        return Ok(vec![s]);
                    }
                    Stmt::Subroutine { params, body, .. } => {
                        self.scope(body, params, tags, vars, None, false)?;
                        return Ok(vec![s]);
                    }
                    Stmt::Module { body, .. } => {
                        self.scope(body, &mut vec![], tags, vars, None, true)?;
                        return Ok(vec![s]);
                    }
                    _ => {}
                }
                self.statement(s.clone(), tags, vars)
            })()
            .map_err(|e| crate::source_diagnostics::at(e, location))?;
            out.extend(lowered);
        }
        *body = out;
        Ok(())
    }

    fn statement(&mut self, mut s: Stmt, tags: &Tags, vars: &Objects) -> Result<Vec<Stmt>> {
        if tags.is_empty() || !tags.keys().any(|n| uses(&s, n)) {
            // Plain ALLOCATE(type :: x) of a nonpolymorphic object.
            if let Stmt::CallSub { name, args } = &mut s {
                if name == "@allocate_intrinsic_type" {
                    args.remove(0);
                    *name = "@allocate".into();
                }
            }
            let mut copy = s;
            for b in children(&mut copy) {
                self.body(b, tags, vars)?;
            }
            return Ok(vec![copy]);
        }
        // ALLOCATED needs only the tag.
        for e in expressions(&mut s) {
            visit_expr(e, &mut |e| {
                if let Expr::Call(n, a) = e {
                    if n == "allocated" && a.len() == 1 {
                        if let Expr::Ident(v) = keyword(&a[0]).map_or(&a[0], |(_, v)| v) {
                            if let Some(t) = tags.get(v) {
                                *e = compare(BinOp::Ne, &t.tag, 0);
                            }
                        }
                    }
                }
            });
        }
        match &s {
            Stmt::CallSub { name, args } if allocation(name) => return self.allocate(name, args, tags, vars),
            Stmt::CallSub { name, args } if name == "@deallocate" => return self.deallocate(args, tags),
            Stmt::CallSub { name, args } if name.eq_ignore_ascii_case("move_alloc") => {
                if let Some(out) = self.move_alloc(args, tags, vars)? {
                    return Ok(out);
                }
            }
            Stmt::Assign { name, value } if tags.contains_key(name) => return self.assign(name, value, tags, vars),
            Stmt::SelectCase { expr, .. } if selection(expr).is_some_and(|(n, _)| tags.contains_key(n)) => {
                return self.select_type(s, tags, vars);
            }
            _ => {}
        }
        // Statements that hold bodies evaluate tagged values in their headers
        // once, at the original control point.
        match &mut s {
            Stmt::If { cond, then_body, else_body } => {
                let mut prefix = vec![];
                if tags.keys().any(|n| uses(&Stmt::Assign { name: String::new(), value: cond.clone() }, n)) {
                    let temp = self.temp(TypeSpec::Logical, cond.clone(), &mut prefix, tags, vars)?;
                    *cond = id(&temp);
                }
                self.body(then_body, tags, vars)?;
                if let Some(b) = else_body {
                    self.body(b, tags, vars)?;
                }
                prefix.push(s);
                return Ok(prefix);
            }
            Stmt::DoWhile { cond, body } => {
                self.body(body, tags, vars)?;
                if tags.keys().any(|n| uses(&Stmt::Assign { name: String::new(), value: cond.clone() }, n)) {
                    // The condition is evaluated before each iteration.
                    let mut inner = vec![];
                    let temp = self.temp(TypeSpec::Logical, cond.clone(), &mut inner, tags, vars)?;
                    let declaration = inner.remove(0);
                    inner.push(Stmt::If { cond: Expr::Un(crate::ast::UnOp::Not, Box::new(id(&temp))), then_body: vec![Stmt::Exit], else_body: None });
                    inner.append(body);
                    return Ok(vec![declaration, Stmt::DoWhile { cond: Expr::Logical(true), body: inner }]);
                }
                return Ok(vec![s]);
            }
            Stmt::Do { start, end, body, .. } => {
                let mut prefix = vec![];
                for bound in [start, end] {
                    if tags.keys().any(|n| uses(&Stmt::Assign { name: String::new(), value: bound.clone() }, n)) {
                        let temp = self.temp(TypeSpec::Integer(Some(8)), bound.clone(), &mut prefix, tags, vars)?;
                        *bound = id(&temp);
                    }
                }
                self.body(body, tags, vars)?;
                prefix.push(s);
                return Ok(prefix);
            }
            Stmt::SelectCase { expr, cases, default } => {
                if tags.keys().any(|n| uses(&Stmt::Assign { name: String::new(), value: expr.clone() }, n)) {
                    bail!("a SELECT CASE expression that names a polymorphic ALLOCATABLE object is not yet supported");
                }
                for c in cases.iter_mut() {
                    self.body(&mut c.body, tags, vars)?;
                }
                if let Some(b) = default {
                    self.body(b, tags, vars)?;
                }
                return Ok(vec![s]);
            }
            Stmt::Block { body } => {
                if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name.starts_with('@'))
                    && tags.keys().any(|n| uses(&body[0], n)) {
                    bail!("a loop control that names a polymorphic ALLOCATABLE object is not yet supported");
                }
                self.body(body, tags, vars)?;
                return Ok(vec![s]);
            }
            _ => {}
        }
        self.dispatch(s, tags, vars)
    }

    /// Run a simple statement once for each dynamic type of an object it
    /// names, after expanding tagged actual arguments.
    fn dispatch(&mut self, mut s: Stmt, tags: &Tags, vars: &Objects) -> Result<Vec<Stmt>> {
        self.arguments(&mut s, tags)?;
        let Some((n, t)) = tags.iter().filter(|(n, _)| uses(&s, n)).min_by_key(|(n, _)| (*n).clone()) else {
            return Ok(vec![s]);
        };
        let t = t.clone();
        let n = n.clone();
        // Any other use runs once for each dynamic type.
        let mut cases = vec![];
        for (i, (_, storage)) in t.variants.iter().enumerate() {
            let mut branch = vec![s.clone()];
            rename(&mut branch, &n, storage);
            let mut lowered = vec![];
            for b in branch {
                lowered.extend(self.statement(b, tags, vars)?);
            }
            cases.push(CaseBlock { items: vec![CaseItem::Single(int(i + 1))], body: lowered });
        }
        Ok(vec![Stmt::SelectCase { expr: id(&t.tag), cases, default: Some(stop(format!("use of unallocated polymorphic object `{}`", t.display))) }])
    }

    /// A temporary that holds the value of an expression naming tagged
    /// objects.
    fn temp(&mut self, kind: TypeSpec, value: Expr, prefix: &mut Vec<Stmt>, tags: &Tags, vars: &Objects) -> Result<String> {
        self.serial += 1;
        let temp = format!("@tagged_value{}", self.serial);
        prefix.push(Stmt::VarDecl { kind, names: vec![temp.clone()] });
        prefix.extend(self.statement(set(&temp, value), tags, vars)?);
        Ok(temp)
    }

    fn arguments(&self, s: &mut Stmt, tags: &Tags) -> Result<()> {
        let expand = |name: &str, args: &mut Vec<Expr>| -> Result<()> {
            let Some(p) = self.procedures.get(name) else { return Ok(()) };
            p.signature.reorder(name, args)?;
            let mut out = vec![];
            for (i, a) in std::mem::take(args).into_iter().enumerate() {
                let Some((declared, rank, readonly)) = p.dummies.get(&i) else {
                    out.push(a);
                    continue;
                };
                let Expr::Ident(actual) = &a else {
                    bail!("a polymorphic ALLOCATABLE dummy argument of `{name}` requires a polymorphic ALLOCATABLE variable");
                };
                let t = tags.get(actual).with_context(|| format!("a polymorphic ALLOCATABLE dummy argument of `{name}` requires a polymorphic ALLOCATABLE variable, not `{actual}`"))?;
                if &t.declared != declared || t.rank != *rank {
                    bail!("polymorphic ALLOCATABLE actual `{actual}` and its dummy argument must have the same declared type and rank");
                }
                if t.readonly && !readonly {
                    bail!("cannot pass INTENT(IN) polymorphic object `{actual}` to a writable ALLOCATABLE dummy argument");
                }
                out.push(id(&t.tag));
                out.extend(t.variants.iter().map(|(_, s)| id(s)));
            }
            *args = out;
            Ok(())
        };
        if let Stmt::CallSub { name, args } = s {
            let name = name.clone();
            expand(&name, args)?;
        }
        for e in expressions(s) {
            let mut error = None;
            visit_expr(e, &mut |e| {
                if let Expr::Call(name, args) = e {
                    let name = name.clone();
                    if error.is_none() {
                        if let Err(err) = expand(&name, args) {
                            error = Some(err);
                        }
                    }
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
        }
        Ok(())
    }

    fn allocate(&mut self, name: &str, args: &[Expr], tags: &Tags, vars: &Objects) -> Result<Vec<Stmt>> {
        let spec = spec_kind(name, args)?;
        let start = usize::from(name != "@allocate");
        let mut options = vec![];
        let mut objects = vec![];
        for a in &args[start..] {
            if keyword(a).is_some() { options.push(a.clone()) } else { objects.push(a.clone()) }
        }
        let value = |k: &str| options.iter().find_map(|a| keyword(a).filter(|(key, _)| *key == k).map(|(_, v)| v.clone()));
        let (source, mold, stat) = (value("source"), value("mold"), value("stat"));
        let mut out = vec![];
        let plain: Vec<Expr> = objects.iter().filter(|a| item(a).map_or(true, |n| !tags.contains_key(n))).cloned().collect();
        if !plain.is_empty() {
            let mut args: Vec<Expr> = if name == "@allocate_derived_type" { vec![args[0].clone()] } else { vec![] };
            args.extend(plain);
            args.extend(options.iter().cloned());
            out.extend(self.dispatch(Stmt::CallSub { name: if name == "@allocate_derived_type" { name.into() } else { "@allocate".into() }, args }, tags, vars)?);
        }
        let others: Vec<Expr> = options.iter().filter(|a| !matches!(keyword(a), Some(("source" | "mold", _)))).cloned().collect();
        for a in objects.iter().filter(|a| item(a).is_some_and(|n| tags.contains_key(n))) {
            let t = &tags[item(a).unwrap()];
            if t.readonly {
                bail!("cannot ALLOCATE INTENT(IN) polymorphic object `{}`", t.display);
            }
            let target = |storage: &str| match a {
                Expr::Call(_, bounds) | Expr::Index(_, bounds) => Expr::Call(storage.into(), bounds.clone()),
                Expr::Section { lower, upper, stride, .. } => Expr::Section { name: storage.into(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() },
                _ => id(storage),
            };
            let store = |storage: &str, option: Option<Expr>| -> Vec<Stmt> {
                let mut args = vec![target(storage)];
                args.extend(option);
                args.extend(others.iter().cloned());
                vec![Stmt::CallSub { name: "@allocate".into(), args }]
            };
            let wrap = |index: usize, mut allocate: Vec<Stmt>| -> Vec<Stmt> {
                let tagged = set(&t.tag, int(index));
                match &stat {
                    Some(stat) => {
                        let Expr::Ident(s) = stat else { return stop("STAT= of a polymorphic ALLOCATE must be a variable".into()) };
                        allocate.push(Stmt::If { cond: compare(BinOp::Eq, s, 0), then_body: vec![tagged], else_body: None });
                        vec![Stmt::If { cond: compare(BinOp::Ne, &t.tag, 0), then_body: vec![set(s, int(1))], else_body: Some(allocate) }]
                    }
                    None => {
                        let mut out = vec![Stmt::If { cond: compare(BinOp::Ne, &t.tag, 0),
                            then_body: stop(format!("ALLOCATE of already allocated polymorphic object `{}`", t.display)), else_body: None }];
                        out.extend(allocate);
                        out.push(tagged);
                        out
                    }
                }
            };
            let given = source.as_ref().map(|v| ("source", v)).or(mold.as_ref().map(|v| ("mold", v)));
            match given {
                Some((key, Expr::Ident(w))) if tags.contains_key(w) => {
                    let from = tags[w].clone();
                    let mut branches = vec![];
                    for (k, storage) in &from.variants {
                        branches.push(match t.variants.iter().position(|(kk, _)| kk == k) {
                            Some(i) => wrap(i + 1, store(&t.variants[i].1, Some(Expr::Call(format!("@keyword:{key}"), vec![id(storage)])))),
                            None => stop(format!("dynamic type {} of `{w}` is not a type `{}` can have", describe(k), t.display)),
                        });
                    }
                    let mut branches = branches.into_iter();
                    out.push(self.cases(&from, |_, _| branches.next().unwrap(), stop(format!("unallocated polymorphic SOURCE= or MOLD= `{w}`"))));
                }
                Some((key, v)) => {
                    if tags.keys().any(|n| uses(&Stmt::Assign { name: String::new(), value: v.clone() }, n)) {
                        bail!("a SOURCE= or MOLD= expression that names a polymorphic ALLOCATABLE object is not yet supported");
                    }
                    let kind = value_kind(v, vars, self.defs)?;
                    let i = self.index(t, &kind)?;
                    let option = Expr::Call(format!("@keyword:{key}"), vec![v.clone()]);
                    out.extend(wrap(i, store(&t.variants[i - 1].1, Some(option))));
                }
                None => {
                    let kind = match &spec {
                        Some(k) => k.clone(),
                        None if t.declared != "*" && !self.defs.abstracts.contains(&t.declared) => t.declared.clone(),
                        None => bail!("ALLOCATE of polymorphic `{}` requires a type specification, SOURCE= or MOLD=", t.display),
                    };
                    let i = self.index(t, &kind)?;
                    out.extend(wrap(i, store(&t.variants[i - 1].1, None)));
                }
            }
        }
        Ok(out)
    }

    fn deallocate(&mut self, args: &[Expr], tags: &Tags) -> Result<Vec<Stmt>> {
        let mut out = vec![];
        let options: Vec<Expr> = args.iter().filter(|a| keyword(a).is_some()).cloned().collect();
        let stat = options.iter().find_map(|a| keyword(a).filter(|(k, _)| *k == "stat").map(|(_, v)| v.clone()));
        let plain: Vec<Expr> = args.iter().filter(|a| keyword(a).is_none() && item(a).map_or(true, |n| !tags.contains_key(n))).cloned().collect();
        if !plain.is_empty() {
            let mut args = plain;
            args.extend(options.iter().cloned());
            out.push(Stmt::CallSub { name: "@deallocate".into(), args });
        }
        for a in args.iter().filter(|a| keyword(a).is_none() && item(a).is_some_and(|n| tags.contains_key(n))) {
            let t = &tags[item(a).unwrap()];
            if t.readonly {
                bail!("cannot DEALLOCATE INTENT(IN) polymorphic object `{}`", t.display);
            }
            let unallocated = match &stat {
                Some(Expr::Ident(s)) => vec![set(s, int(1))],
                Some(_) => bail!("STAT= of a polymorphic DEALLOCATE must be a variable"),
                None => stop(format!("DEALLOCATE of unallocated polymorphic object `{}`", t.display)),
            };
            if let Some(Expr::Ident(s)) = &stat {
                out.push(set(s, int(0)));
            }
            out.extend(self.release(t, unallocated));
        }
        Ok(out)
    }

    fn move_alloc(&mut self, args: &[Expr], tags: &Tags, vars: &Objects) -> Result<Option<Vec<Stmt>>> {
        let arg = |key: &str, i: usize| args.iter().find(|a| matches!(keyword(a), Some((k, _)) if k == key))
            .or_else(|| args.get(i).filter(|a| keyword(a).is_none()))
            .map(|a| keyword(a).map_or(a, |(_, v)| v).clone());
        let (Some(from), Some(to)) = (arg("from", 0), arg("to", 1)) else { return Ok(None) };
        let name = |e: &Expr| match e { Expr::Ident(n) => Some(n.clone()), _ => None };
        let (f, d) = (name(&from).and_then(|n| tags.get(&n).cloned()), name(&to).and_then(|n| tags.get(&n).cloned()));
        let call = |a: &str, b: &str| Stmt::CallSub { name: "move_alloc".into(), args: vec![id(a), id(b)] };
        match (f, d) {
            (None, None) => Ok(None),
            (Some(f), Some(d)) => {
                if f.declared != d.declared || f.rank != d.rank {
                    bail!("MOVE_ALLOC between polymorphic objects requires the same declared type and rank");
                }
                let mut out = self.release(&d, vec![]);
                out.push(self.cases(&f, |k, storage| {
                    let target = &d.variants.iter().find(|(kk, _)| kk == k).unwrap().1;
                    vec![call(storage, target)]
                }, vec![]));
                out.push(set(&d.tag, id(&f.tag)));
                out.push(set(&f.tag, int(0)));
                Ok(Some(out))
            }
            (None, Some(d)) => {
                let Some(source) = name(&from) else { bail!("MOVE_ALLOC FROM= must be a variable") };
                let i = self.index(&d, &value_kind(&from, vars, self.defs)?)?;
                let mut out = self.release(&d, vec![]);
                out.push(Stmt::If {
                    cond: Expr::Call("allocated".into(), vec![from.clone()]),
                    then_body: vec![call(&source, &d.variants[i - 1].1), set(&d.tag, int(i))],
                    else_body: None,
                });
                Ok(Some(out))
            }
            (Some(f), None) => bail!("MOVE_ALLOC from polymorphic `{}` requires a polymorphic TO= of the same declared type", f.display),
        }
    }

    fn assign(&mut self, name: &str, value: &Expr, tags: &Tags, vars: &Objects) -> Result<Vec<Stmt>> {
        let t = tags[name].clone();
        if t.readonly {
            bail!("cannot define INTENT(IN) polymorphic object `{name}`");
        }
        let replace = |this: &Self, i: usize, value: Expr| -> Vec<Stmt> {
            let mut release = this.release(&t, vec![]);
            release.pop();
            let storage = &t.variants[i - 1].1;
            if t.rank > 1 {
                // Assignment reallocates only rank-one arrays, so a higher
                // rank takes the shape of the value through SOURCE=.
                return vec![
                    Stmt::If { cond: compare(BinOp::Ne, &t.tag, 0), then_body: release, else_body: None },
                    Stmt::CallSub { name: "@allocate".into(), args: vec![id(storage), Expr::Call("@keyword:source".into(), vec![value])] },
                    set(&t.tag, int(i)),
                ];
            }
            vec![
                Stmt::If { cond: compare(BinOp::Ne, &t.tag, i), then_body: release, else_body: None },
                set(storage, value),
                set(&t.tag, int(i)),
            ]
        };
        if let Expr::Ident(w) = value {
            if let Some(from) = tags.get(w).cloned() {
                if w == name {
                    return Ok(vec![]);
                }
                let mut branches = vec![];
                for (k, storage) in &from.variants {
                    branches.push(match t.variants.iter().position(|(kk, _)| kk == k) {
                        Some(i) => replace(self, i + 1, id(storage)),
                        None => stop(format!("dynamic type {} of `{w}` is not a type `{name}` can have", describe(k))),
                    });
                }
                let mut branches = branches.into_iter();
                return Ok(vec![self.cases(&from, |_, _| branches.next().unwrap(), stop(format!("assignment from unallocated polymorphic object `{w}`")))]);
            }
        }
        let i = self.index(&t, &value_kind(value, vars, self.defs)?)?;
        let mut out = vec![];
        for s in replace(self, i, value.clone()) {
            out.extend(self.statement(s, tags, vars)?);
        }
        Ok(out)
    }

    fn select_type(&mut self, s: Stmt, tags: &Tags, vars: &Objects) -> Result<Vec<Stmt>> {
        let Stmt::SelectCase { expr, cases: guards, default } = s else { unreachable!() };
        let (selector, alias) = selection(&expr).unwrap();
        let t = tags[selector].clone();
        let mut guarded = vec![];
        for c in &guards {
            let (class, kind) = guard(&c.items[0])?;
            if !self.defs.fields.contains_key(&kind) && intrinsic::decode(&kind).is_none() {
                bail!("unknown derived type `{kind}` in SELECT TYPE guard");
            }
            if t.declared != "*" && self.defs.distance(&kind, &t.declared).is_none() {
                bail!("SELECT TYPE guard `{kind}` must extend declared type `{}`", t.declared);
            }
            guarded.push((class, kind, c.body.clone()));
        }
        let mut cases = vec![];
        for (i, (kind, storage)) in t.variants.iter().enumerate() {
            let mut best: Option<(usize, &Vec<Stmt>)> = None;
            for (class, guard_kind, body) in &guarded {
                let d = if *class {
                    self.defs.distance(kind, guard_kind).map(|d| d + 1)
                } else {
                    (kind == guard_kind || intrinsic::matches(kind, guard_kind)).then_some(0)
                };
                if let Some(d) = d {
                    if best.map_or(true, |(b, _)| d < b) {
                        best = Some((d, body));
                    }
                }
            }
            let mut branch = match best {
                Some((_, body)) => {
                    let mut body = body.clone();
                    rename(&mut body, alias, storage);
                    body
                }
                None => {
                    let mut body = default.clone().unwrap_or_default();
                    if alias != selector {
                        rename(&mut body, alias, selector);
                    }
                    body
                }
            };
            self.body(&mut branch, tags, vars)?;
            cases.push(CaseBlock { items: vec![CaseItem::Single(int(i + 1))], body: branch });
        }
        Ok(vec![Stmt::SelectCase { expr: id(&t.tag), cases, default: Some(stop(format!("SELECT TYPE on unallocated polymorphic object `{}`", t.display))) }])
    }
}

pub(super) fn resolve(mut program: Program) -> Result<Program> {
    fn any(body: &[Stmt], inherited: &Objects) -> bool {
        let vars = objects(body, inherited);
        vars.values().any(candidate) || body.iter().any(|s| {
            let mut copy = s.clone();
            matches!(s, Stmt::CallSub { name, .. } if name == "@allocate_intrinsic_type")
                || children(&mut copy).into_iter().any(|b| any(b, &vars))
        })
    }
    if !any(&program.body, &Objects::new()) {
        return Ok(program);
    }
    intrinsic::prepare(&mut program.body, &HashMap::new())?;
    let mut defs = Definitions::default();
    defs.collect(&program.body);
    let mut unlimited = BTreeSet::new();
    collect(&program.body, &Objects::new(), &defs, &mut unlimited)?;
    let mut signatures = HashMap::new();
    procedures(&program.body, &Objects::new(), &mut signatures);
    let mut lower = Lower { defs: &defs, unlimited: unlimited.into_iter().collect(), procedures: signatures, serial: 0 };
    lower.scope(&mut program.body, &mut vec![], &Tags::new(), &Objects::new(), None, false)?;
    Ok(program)
}

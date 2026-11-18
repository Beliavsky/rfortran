// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! NAMELIST groups expand to checked scalar transfers, preserving omitted objects.
use crate::ast::{BinOp as B, Expr as E, Stmt as S, TypeSpec as T};
use anyhow::{bail, ensure, Result};
use std::collections::{HashMap, HashSet};
fn i(n: i64) -> E {
    E::IntLit(n.to_string())
}
fn v(n: &str) -> E {
    E::Ident(n.into())
}
fn text(n: &str) -> E {
    E::Str(n.into())
}
fn bin(op: B, a: E, b: E) -> E {
    E::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, e: E) -> S {
    S::Assign {
        name: n.into(),
        value: e,
    }
}
fn call(n: &str, args: Vec<E>) -> S {
    S::CallSub {
        name: n.into(),
        args,
    }
}
fn branch(cond: E, body: Vec<S>) -> S {
    S::If {
        cond,
        then_body: body,
        else_body: None,
    }
}
fn decl(kind: T, n: &str) -> S {
    S::VarDecl {
        kind,
        names: vec![n.into()],
    }
}
pub fn tokens(tokens: &mut Vec<crate::lexer::Token>, input: &str) {
    use crate::lexer::TokenKind as K;
    let mut n = 0;
    while n + 1 < tokens.len() {
        if matches!(&tokens[n].kind,K::Ident(s) if s=="namelist")
            && matches!(tokens[n + 1].kind, K::Slash)
            && (n == 0
                || !crate::lexer::same_statement(
                    &input[tokens[n - 1].span.end..tokens[n].span.start],
                ))
        {
            let mut end = n + 1;
            while end < tokens.len()
                && crate::lexer::same_statement(
                    &input[tokens[end - 1].span.end..tokens[end].span.start],
                )
            {
                end += 1;
            }
            let src = tokens[n + 1..end]
                .iter()
                .map(|t| input[t.span.clone()].to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            tokens[n].kind = K::ProcedureDeclaration(format!("namelist|{src}"));
            tokens[n].span.end = tokens[end - 1].span.end;
            tokens.drain(n + 1..end);
        }
        n += 1;
    }
}
pub fn declaration(src: &str) -> std::result::Result<Vec<S>, &'static str> {
    let mut parts = src.split('/');
    if !parts.next().unwrap_or("").trim().is_empty() {
        return Err("invalid NAMELIST declaration");
    }
    let mut out = vec![];
    while let Some(group) = parts.next() {
        let group = group.trim();
        let members = parts.next().ok_or("NAMELIST requires /group/ members")?;
        let names = members
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>();
        let identifier = |n: &str| {
            n.starts_with(|c: char| c.is_ascii_alphabetic())
                && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !identifier(group) || names.is_empty() || names.iter().any(|n| !identifier(n)) {
            return Err("NAMELIST requires a group name and variable names");
        }
        let mut args = vec![text(group)];
        args.extend(names.into_iter().map(v));
        out.push(call("@namelist_decl", args));
    }
    Ok(out)
}
/// Begins `[marker(name), NAMELIST transfer, other READ or WRITE]` for a
/// statement such as `read(10, name, iostat=ios)`: a positional name in the
/// format position, with other controls, is a NAMELIST group only when a
/// visible NAMELIST statement declares it, which resolve() decides.
pub(crate) const CANDIDATE: &str = "@namelist_candidate";

/// The READ or WRITE `normal`, or both readings when the second positional
/// control may name a NAMELIST group (see CANDIDATE).
pub(crate) fn candidate(
    controls: &[E],
    items: &[E],
    read: bool,
    normal: std::result::Result<S, &'static str>,
) -> std::result::Result<S, &'static str> {
    let keyword = |e: &E| matches!(e, E::Call(n, _) if n.starts_with("@read_control:") || n.starts_with("@write_control:"));
    let (Some(E::Ident(group)), Some(first)) = (controls.get(1), controls.first()) else { return normal };
    if !items.is_empty() || controls.len() < 3 || keyword(first) || !controls[2..].iter().all(keyword) {
        return normal;
    }
    let Ok(group_transfer) = controls_inner(controls.to_vec(), vec![], read) else { return normal };
    let Ok(normal) = normal else { return Ok(group_transfer) };
    Ok(S::Block { body: vec![call(CANDIDATE, vec![v(group)]), group_transfer, normal] })
}

pub fn controls(
    controls: Vec<E>,
    items: Vec<E>,
    read: bool,
) -> std::result::Result<S, &'static str> {
    controls_inner(controls, items, read)
}

fn controls_inner(
    controls: Vec<E>,
    items: Vec<E>,
    read: bool,
) -> std::result::Result<S, &'static str> {
    if !items.is_empty() {
        return Err("NAMELIST transfer cannot have an input/output item list");
    }
    let mut values: HashMap<String, E> = HashMap::new();
    let mut named = false;
    for (pos, e) in controls.into_iter().enumerate() {
        let (key, e) = match e {
            E::Call(n, a)
                if n.starts_with("@read_control:") || n.starts_with("@write_control:") =>
            {
                named = true;
                (n.split(':').nth(1).unwrap().to_string(), a[0].clone())
            }
            e => {
                if named {
                    return Err("positional controls must precede named controls");
                }
                (
                    (match pos {
                        0 => "unit",
                        1 => "nml",
                        _ => return Err("too many NAMELIST controls"),
                    })
                    .into(),
                    e,
                )
            }
        };
        if !matches!(key.as_str(), "unit" | "nml" | "iostat" | "iomsg" | "err" | "end") {
            return Err("NAMELIST currently supports UNIT, NML, IOSTAT, IOMSG, ERR and END");
        }
        if key == "end" && !read {
            return Err("NAMELIST WRITE must not specify END=");
        }
        if matches!(key.as_str(), "iostat" | "iomsg") && !matches!(e, E::Ident(_)) {
            return Err("NAMELIST IOSTAT and IOMSG require scalar variables");
        }
        if values.insert(key, e).is_some() {
            return Err("duplicate NAMELIST control");
        }
    }
    let mut unit = values.remove("unit").ok_or("NAMELIST requires UNIT")?;
    if matches!(&unit,E::Call(n,_) if n=="@write_star") {
        unit = i(if read { 5 } else { 6 });
    }
    let E::Ident(group) = values.remove("nml").ok_or("NAMELIST requires NML")? else {
        return Err("NML requires a NAMELIST group name");
    };
    let user = values.remove("iostat");
    let message = values.remove("iomsg").unwrap_or(i(0));
    // ERR= and END= branch on the status, as for other READ and WRITE
    // statements (see input::IO_BRANCHES); without IOSTAT= it is a temporary.
    let branches: Vec<E> = ["err", "end"].iter()
        .filter_map(|k| values.remove(*k).map(|label| E::Call(format!("@io_spec:{k}"), vec![label])))
        .collect();
    let status = match (&user, branches.is_empty()) {
        (Some(status), _) => status.clone(),
        (None, true) => i(0),
        (None, false) => v(crate::input::BRANCH_STATUS),
    };
    let transfer = call(
        if read {
            "@namelist_read"
        } else {
            "@namelist_write"
        },
        vec![unit, text(&group), status.clone(), message],
    );
    if branches.is_empty() {
        return Ok(transfer);
    }
    let mut args = vec![status, E::Logical(user.is_some())];
    args.extend(branches);
    Ok(S::Block { body: vec![transfer, call(crate::input::IO_BRANCHES, args)] })
}
type Symbols = HashMap<String, (T, Vec<E>, Vec<String>)>;
/// NAMELIST groups by local name: the group's own name, which input and
/// output use, and its members as (local designator, label) pairs. The
/// members of a USE-associated group are accessed through hidden USE renames,
/// since the scope need not import them itself.
type Groups = HashMap<String, (String, Vec<(String, String)>)>;
/// Replace the leading member designator of `field` by its label.
fn label(field: &str, members: &[(String, String)]) -> String {
    for (access, shown) in members {
        if let Some(rest) = field.strip_prefix(access.as_str()) {
            if rest.is_empty() || rest.starts_with('%') || rest.starts_with('(') {
                return format!("{shown}{rest}");
            }
        }
    }
    field.to_string()
}
/// Derived objects: name -> (type name, dims, attributes).
type Objects = HashMap<String, (String, Vec<E>, Vec<String>)>;
/// A component: name, intrinsic type or derived type name, dims, attributes.
type Component = (String, Result<T, String>, Vec<E>, Vec<String>);
#[derive(Default)]
struct Types {
    components: HashMap<String, Vec<Component>>,
    parents: HashMap<String, String>,
    /// The variables, derived objects and groups of each module.
    modules: HashMap<String, (Symbols, Objects, Groups)>,
}
impl Types {
    fn collect(&mut self, body: &[S]) {
        for s in body {
            match s {
                S::DerivedType { name, components, .. } => {
                    let mut out: Vec<Component> = vec![];
                    let mut put = |n: &str, kind: Option<Result<T, String>>, dims: Option<&Vec<E>>, attrs: &[String]| {
                        let slot = match out.iter().position(|c| c.0 == n) {
                            Some(p) => p,
                            None => {
                                out.push((n.to_string(), Err(String::new()), vec![], vec![]));
                                out.len() - 1
                            }
                        };
                        let c = &mut out[slot];
                        if let Some(k) = kind {
                            c.1 = k;
                        }
                        if let Some(d) = dims {
                            c.2 = d.clone();
                        }
                        c.3.extend(attrs.iter().cloned());
                    };
                    for c in components {
                        match c {
                            S::VarDecl { kind, names } => names.iter().for_each(|n| put(n, Some(Ok(kind.clone())), None, &[])),
                            S::ArrayDecl { kind, name, dims } => put(name, Some(Ok(kind.clone())), Some(dims), &[]),
                            S::InitializedDecl { kind, attributes, entities } => {
                                entities.iter().for_each(|(n, _)| put(n, Some(Ok(kind.clone())), None, attributes))
                            }
                            S::DerivedDecl { type_name, names, attributes, dims, .. } => names.iter()
                                .for_each(|n| put(n, Some(Err(type_name.clone())), (!dims.is_empty()).then_some(dims), attributes)),
                            _ => {}
                        }
                    }
                    self.components.insert(name.clone(), out);
                }
                S::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [E::Ident(child), E::Ident(parent)] = args.as_slice() {
                        self.parents.insert(child.clone(), parent.clone());
                    }
                }
                S::Module { body, .. } | S::Function { body, .. } | S::Subroutine { body, .. } | S::Block { body } => self.collect(body),
                _ => {}
            }
        }
    }
    /// The intrinsic leaves of type `ty`, including inherited components, in
    /// component order, and whether they are all its leaves: the leaves of
    /// an array of derived type are addressed separately. Each component's
    /// designators are pushed to `out`. Within an element of an array of
    /// derived type, `record` holds the array's designator, the element's
    /// subscripts and the component path; `rec` names the element number.
    fn leaves(&self, prefix: &str, record: Option<(&str, &[E], &str)>, ty: &str, rec: &str, out: &mut Vec<Entry>, depth: usize) -> Result<(Vec<Leaf>, bool)> {
        ensure!(depth < 32, "NAMELIST member `{prefix}` has too deeply nested components");
        let mut leaves = vec![];
        let mut complete = true;
        if let Some(parent) = self.parents.get(ty) {
            let (inherited, all) = self.leaves(prefix, record, parent, rec, out, depth + 1)?;
            leaves.extend(inherited);
            complete &= all;
        }
        let components = self.components.get(ty)
            .ok_or_else(|| anyhow::anyhow!("NAMELIST member `{prefix}` has derived type `{ty}`, whose definition is not visible"))?;
        for (n, kind, dims, attrs) in components {
            let name = format!("{prefix}%{n}");
            // `x()%a` denotes a component of each element of `x` in messages.
            let shown = name.replace("()", "");
            ensure!(!attrs.iter().any(|a| matches!(a.as_str(), "allocatable" | "pointer")),
                "NAMELIST component `{shown}` with the ALLOCATABLE or POINTER attribute is not yet supported");
            ensure!(dims.iter().all(|d| !matches!(d, E::Ident(n) if n.starts_with('@'))),
                "NAMELIST component `{shown}` currently requires a fixed-size array");
            let inner = record.map(|(base, index, path)| {
                (base.to_string(), index.to_vec(), if path.is_empty() { n.clone() } else { format!("{path}%{n}") })
            });
            match kind {
                Ok(kind) => {
                    let leaf = Leaf { field: name.clone(), kind: kind.clone(), dims: dims.clone(), record: inner };
                    out.push(Entry::leaf(&leaf));
                    leaves.push(leaf);
                }
                Err(sub) if dims.is_empty() => {
                    let context = inner.as_ref().map(|(b, i, p)| (b.as_str(), i.as_slice(), p.as_str()));
                    let (parts, all) = self.leaves(&name, context, sub, rec, out, depth + 1)?;
                    if all {
                        out.push(Entry::whole(&name, parts.clone()));
                    }
                    leaves.extend(parts);
                    complete &= all;
                }
                Err(sub) => {
                    ensure!(record.is_none(), "NAMELIST component `{shown}`, an array of derived type within an array of derived type, is not yet supported");
                    self.records(&name, sub, dims, rec, out, depth + 1)?;
                    complete = false;
                }
            }
        }
        Ok((leaves, complete))
    }
    /// Push the designators of the array `name` of derived type `ty`: `x()%a`
    /// selects an element by the designator's outer subscripts, while `x%a`
    /// and `x` take values for every element in array element order. The
    /// element number is computed at run time into `rec`, so the generated
    /// code does not grow with the number of elements.
    fn records(&self, name: &str, ty: &str, dims: &[E], rec: &str, out: &mut Vec<Entry>, depth: usize) -> Result<()> {
        let element = format!("{name}()");
        let index = indices(dims, v(rec));
        let mut parts = vec![];
        let (leaves, complete) = self.leaves(&element, Some((name, &index, "")), ty, rec, &mut parts, depth + 1)?;
        for part in parts {
            let suffix = part.field[element.len()..].to_string();
            out.push(Entry { records: Some((dims.to_vec(), true)), output: false, ..part.clone() });
            out.push(Entry { field: format!("{name}{suffix}"), dims: vec![], records: Some((dims.to_vec(), false)), ..part });
        }
        if complete {
            out.push(Entry { field: name.into(), leaves, dims: dims.to_vec(), scaled: true, records: Some((dims.to_vec(), false)), output: false, label: String::new() });
        }
        Ok(())
    }
}
/// An intrinsic variable or component that NAMELIST transfers, under its
/// input/output designator `field`. A component of an element of an array of
/// derived type has `record`: the array, the element subscripts and the path.
#[derive(Clone)]
struct Leaf {
    field: String,
    kind: T,
    dims: Vec<E>,
    record: Option<(String, Vec<E>, String)>,
}
impl Leaf {
    fn plain(name: &str, kind: &T, dims: &[E]) -> Leaf {
        Leaf { field: name.into(), kind: kind.clone(), dims: dims.to_vec(), record: None }
    }
    /// Element `position` (in array element order) of the leaf.
    fn value(&self, position: E) -> E {
        let Some((base, index, path)) = &self.record else {
            return element(&self.field, &self.dims, position);
        };
        let mut args = vec![E::Call(base.clone(), index.clone())];
        if !self.dims.is_empty() {
            args.push(E::Call("@component_subscripts".into(), indices(&self.dims, position)));
        }
        E::Call(format!("@record_component:{path}"), args)
    }
    fn assign(&self, position: E, value: E) -> S {
        let Some((base, index, path)) = &self.record else {
            return match element(&self.field, &self.dims, position) {
                E::Ident(name) => S::Assign { name, value },
                E::Call(name, indices) => S::AssignIndex { name, indices, value },
                _ => unreachable!(),
            };
        };
        let mut all = index.clone();
        if !self.dims.is_empty() {
            all.push(E::Call("@component_subscripts".into(), indices(&self.dims, position)));
        }
        S::AssignIndex { name: format!("{base}%{path}"), indices: all, value }
    }
}
/// A designator accepted in NAMELIST input. A whole derived object takes its
/// leaves' values in component order.
#[derive(Clone)]
struct Entry {
    field: String,
    leaves: Vec<Leaf>,
    /// The bounds that the designator's final subscripts address.
    dims: Vec<E>,
    /// The final subscripts select a whole element of an array of derived
    /// type, which takes the values of all of its leaves.
    scaled: bool,
    /// For an array of derived type, its bounds and whether the designator's
    /// outer subscripts select the element (`x(2)%a`); otherwise the values
    /// define the leaves of every element in turn (`x%a`).
    records: Option<(Vec<E>, bool)>,
    /// NAMELIST output lists this designator.
    output: bool,
    /// The designator as input and output spell it.
    label: String,
}
impl Entry {
    fn leaf(leaf: &Leaf) -> Entry {
        Entry { field: leaf.field.clone(), leaves: vec![leaf.clone()], dims: leaf.dims.clone(), scaled: false, records: None, output: true, label: String::new() }
    }
    fn whole(field: &str, leaves: Vec<Leaf>) -> Entry {
        Entry { field: field.into(), leaves, dims: vec![], scaled: false, records: None, output: false, label: String::new() }
    }
    /// The number of values of the object or of one element.
    fn values(&self) -> E {
        self.leaves.iter().fold(i(0), |n, leaf| bin(B::Add, n, count(&leaf.dims)))
    }
}
fn bounds(d: &E) -> (E, E) {
    match d {
        E::Call(n, a) if n == "@declared_bounds" => (a[0].clone(), a[1].clone()),
        E::Section { lower, upper, .. } => (
            lower.as_deref().cloned().unwrap_or(i(1)),
            upper.as_deref().cloned().unwrap_or(i(0)),
        ),
        _ => (i(1), d.clone()),
    }
}
fn size(d: &E) -> E {
    let (lo, hi) = bounds(d);
    E::Call(
        "max".into(),
        vec![i(0), bin(B::Add, bin(B::Sub, hi, lo), i(1))],
    )
}
/// The bounds of NAMELIST member `name`: those of an ALLOCATABLE, POINTER
/// or assumed-shape array are the ones it has when the statement executes.
fn runtime_bounds(name: &str, dims: &[E], attrs: &[String]) -> Vec<E> {
    let runtime = attrs.iter().any(|a| matches!(a.as_str(), "allocatable" | "pointer"))
        || dims.iter().any(|d| matches!(d, E::Ident(n) if n.starts_with('@')) || matches!(d, E::Section { upper: None, .. }));
    if !runtime { return dims.to_vec(); }
    (1..=dims.len()).map(|k| E::Call("@declared_bounds".into(), vec![
        E::Call("lbound".into(), vec![v(name), i(k as i64)]),
        E::Call("ubound".into(), vec![v(name), i(k as i64)])])).collect()
}
fn count(dims: &[E]) -> E {
    dims.iter().fold(i(1), |n, d| bin(B::Mul, n, size(d)))
}
fn element(name: &str, dims: &[E], index: E) -> E {
    if dims.is_empty() {
        return v(name);
    }
    E::Call(name.into(), indices(dims, index))
}
/// The subscripts of element `index`, in array element order, of an array.
fn indices(dims: &[E], index: E) -> Vec<E> {
    let mut offset = bin(B::Sub, index, i(1));
    let mut indices = vec![];
    for d in dims {
        let (lo, _) = bounds(d);
        indices.push(bin(
            B::Add,
            lo,
            E::Call("mod".into(), vec![offset.clone(), size(d)]),
        ));
        offset = bin(B::Div, offset, size(d));
    }
    indices
}
/// Record the variables, derived objects and NAMELIST groups that
/// `statements` declare or USE-associate.
/// Returns the USE-associated group names.
fn declare(statements: &mut Vec<S>, types: &Types, symbols: &mut Symbols, objects: &mut Objects, groups: &mut Groups) -> HashSet<String> {
    let mut local = HashSet::new();
    let mut imported = HashSet::new();
    let existing: HashSet<String> = statements.iter().flat_map(|s| match s {
        S::Use { renames, .. } => renames.iter().map(|(l, _)| l.clone()).collect(),
        _ => vec![],
    }).collect();
    let mut hidden: Vec<(usize, S)> = vec![];
    for (position, s) in statements.iter().enumerate() {
        if let S::Use { module, only, renames, .. } = s {
            let Some((m_symbols, m_objects, m_groups)) = types.modules.get(module) else { continue };
            let names: HashSet<&String> = m_symbols.keys().chain(m_objects.keys()).chain(m_groups.keys()).collect();
            for remote in names {
                if only.as_ref().is_some_and(|o| !o.contains(remote)) {
                    continue;
                }
                let mut aliases: Vec<&String> = renames.iter().filter(|(_, r)| r == remote).map(|(l, _)| l).collect();
                if aliases.is_empty() || only.as_ref().is_some_and(|o| o.iter().filter(|n| *n == remote).count() > aliases.len()) {
                    aliases.push(remote);
                }
                for alias in aliases {
                    if let Some(x) = m_symbols.get(remote) {
                        objects.remove(alias);
                        symbols.insert(alias.clone(), x.clone());
                    }
                    if let Some(x) = m_objects.get(remote) {
                        symbols.remove(alias);
                        objects.insert(alias.clone(), x.clone());
                    }
                    if let Some((group_name, members)) = m_groups.get(remote) {
                        // Each member is reached through a hidden rename of
                        // the module's entity.
                        let mut renamed = vec![];
                        for (access, shown) in members {
                            let hidden_name = format!("@nml_use_{module}_{}", access.trim_start_matches('@'));
                            if let Some(x) = m_symbols.get(access) {
                                symbols.insert(hidden_name.clone(), x.clone());
                            }
                            if let Some(x) = m_objects.get(access) {
                                objects.insert(hidden_name.clone(), x.clone());
                            }
                            if !existing.contains(&hidden_name) && !hidden.iter().any(|(_, h)| matches!(h, S::Use { renames, .. } if renames[0].0 == hidden_name)) {
                                hidden.push((position, S::Use {
                                    module: module.clone(),
                                    nature: None,
                                    only: Some(vec![access.clone()]),
                                    renames: vec![(hidden_name.clone(), access.clone())],
                                }));
                            }
                            renamed.push((hidden_name, shown.clone()));
                        }
                        groups.insert(alias.clone(), (group_name.clone(), renamed));
                        imported.insert(alias.clone());
                    }
                }
            }
        }
    }
        for s in statements.iter() {
        match s {
            S::DerivedDecl { type_name, names, attributes, dims, .. } => {
                for n in names {
                    symbols.remove(n);
                    objects.insert(n.clone(), (type_name.clone(), dims.clone(), attributes.clone()));
                }
            }
            S::VarDecl { kind, names } => {
                for n in names {
                    objects.remove(n);
                    symbols.insert(n.clone(), (kind.clone(), vec![], vec![]));
                }
            }
            S::ArrayDecl { kind, name, dims } => {
                symbols.insert(name.clone(), (kind.clone(), dims.clone(), vec![]));
            }
            S::InitializedDecl {
                kind,
                attributes,
                entities,
            } => {
                for (n, _) in entities {
                    symbols.insert(n.clone(), (kind.clone(), vec![], attributes.clone()));
                }
            }
            S::CallSub { name, args } if name == "@namelist_decl" => {
                let E::Str(group) = &args[0] else {
                    unreachable!()
                };
                // A local NAMELIST statement starts a new group, except that
                // it extends a USE-associated group, as other compilers allow.
                if local.insert(group.clone()) && !imported.contains(group) {
                    groups.insert(group.clone(), (group.clone(), vec![]));
                }
                let members = &mut groups.get_mut(group).unwrap().1;
                for e in &args[1..] {
                    let E::Ident(n) = e else { unreachable!() };
                    if !members.iter().any(|(m, _)| m == n) {
                        members.push((n.clone(), n.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    for (position, s) in hidden.into_iter().rev() {
        statements.insert(position + 1, s);
    }
    imported
}
pub fn resolve(mut program: crate::ast::Program) -> Result<crate::ast::Program> {
    fn body(
        statements: &mut Vec<S>,
        inherited: &Symbols,
        inherited_objects: &Objects,
        types: &Types,
        host: &Groups,
        serial: &mut usize,
    ) -> Result<()> {
        let mut symbols = inherited.clone();
        let mut objects = inherited_objects.clone();
        let mut groups = host.clone();
        let imported = declare(statements, types, &mut symbols, &mut objects, &mut groups);
        for stmt in statements.iter() {
            if let S::DummyIntent { name, intent } = stmt {
                if let Some((_, _, attrs)) = symbols.get_mut(name) {
                    attrs.push(intent.clone());
                }
            }
        }
        let mut offset = None;
        for s in statements.iter_mut() {
            if let S::SourceLocation(n) = s {
                offset = Some(*n);
                continue;
            }
            let result = (|| -> Result<()> {
                // Choose a reading of a statement that may name a group.
                if let S::Block { body } = s {
                    let chosen = match body.first() {
                        Some(S::CallSub { name, args }) if name == CANDIDATE => {
                            let [E::Ident(group)] = args.as_slice() else { unreachable!() };
                            Some(if groups.contains_key(group) { 1 } else { 2 })
                        }
                        _ => None,
                    };
                    if let Some(chosen) = chosen {
                        let taken = body.swap_remove(chosen);
                        *s = taken;
                    }
                }
                if let S::CallSub { name, args } = s {
                    if name == "@namelist_decl" {
                        // The group remains an entity for USE ONLY and renames.
                        let E::Str(group) = &args[0] else { unreachable!() };
                        *s = match imported.contains(group) {
                            true => S::Block { body: vec![] },
                            false => crate::imports::declaration(group.clone()),
                        };
                        return Ok(());
                    }
                    // A positional second control denotes a group only when a
                    // visible NAMELIST declaration establishes that meaning.
                    if name == "@write" || name == "@formatted_read" {
                        if let Some(E::Ident(group)) = args.get(1) {
                            if groups.contains_key(group) {
                                ensure!(
                                    args.len() == 2,
                                    "NAMELIST does not permit a transfer item list"
                                );
                                *s = call(
                                    if name == "@write" {
                                        "@namelist_write"
                                    } else {
                                        "@namelist_read"
                                    },
                                    vec![args[0].clone(), text(group), i(0), i(0)],
                                );
                            }
                        }
                    }
                }
                if let S::CallSub { name, args } = s {
                    if name == "@namelist_read" || name == "@namelist_write" {
                        let read = name == "@namelist_read";
                        let E::Str(group) = &args[1] else {
                            unreachable!()
                        };
                        let (group_name, members) = groups
                            .get(group)
                            .ok_or_else(|| anyhow::anyhow!("unknown NAMELIST group `{group}`"))?;
                        let mut entries = vec![];
                        for (n, _) in members {
                            if let Some((ty, dims, attrs)) = objects.get(n) {
                                let dims = &runtime_bounds(n, dims, attrs);
                                ensure!(!read || !attrs.iter().any(|a| a == "intent(in)"), "NAMELIST READ cannot define INTENT(IN) member `{n}`");
                                let rec = format!("@nml_{}_rec", *serial + 1);
                                if dims.is_empty() {
                                    let mut parts = vec![];
                                    let (leaves, complete) = types.leaves(n, None, ty, &rec, &mut parts, 0)?;
                                    if complete {
                                        entries.push(Entry::whole(n, leaves));
                                    }
                                    entries.extend(parts);
                                } else {
                                    types.records(n, ty, dims, &rec, &mut entries, 0)?;
                                }
                                continue;
                            }
                            let (kind, dims, attrs) = symbols.get(n).ok_or_else(|| {
                                anyhow::anyhow!(
                                    "NAMELIST member `{n}` requires a declaration"
                                )
                            })?;
                            ensure!(!attrs.iter().any(|a|a=="parameter"),"NAMELIST member `{n}` must be a variable");
                            entries.push(Entry::leaf(&Leaf::plain(n, kind, &runtime_bounds(n, dims, attrs))));
                        }
                        if read {
                            for (n, _) in members.iter().filter(|(n, _)| symbols.contains_key(n)) {
                                ensure!(
                                    !symbols[n].2.iter().any(|a| a == "intent(in)"),
                                    "NAMELIST READ cannot define INTENT(IN) member `{n}`"
                                );
                            }
                        }
                        *serial += 1;
                        let mut args = args.clone();
                        args[1] = text(group_name);
                        let mut entries = entries;
                        for entry in &mut entries {
                            entry.label = label(&entry.field, members);
                        }
                        *s = transfer(&args, read, &entries, &symbols, *serial, members)?;
                        return Ok(());
                    }
                }
                for child in crate::procedure_arguments::children(s) {
                    body(child, &symbols, &objects, types, &groups, serial)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
    let mut types = Types::default();
    types.collect(&program.body);
    for s in program.body.iter_mut() {
        if let S::Module { name, body } = s {
            let (mut symbols, mut objects, mut groups) = Default::default();
            declare(body, &types, &mut symbols, &mut objects, &mut groups);
            types.modules.insert(name.clone(), (symbols, objects, groups));
        }
    }
    body(&mut program.body, &Symbols::new(), &Objects::new(), &types, &Groups::new(), &mut 0)?;
    Ok(program)
}
fn transfer(
    args: &[E],
    read: bool,
    entries: &[Entry],
    symbols: &Symbols,
    serial: usize,
    members: &[(String, String)],
) -> Result<S> {
    let [unit, E::Str(group), status, message] = args else {
        unreachable!()
    };
    ensure!(
        matches!(status, E::Ident(_) | E::IntLit(_)),
        "NAMELIST IOSTAT requires an INTEGER variable"
    );
    ensure!(
        matches!(message, E::Ident(_) | E::IntLit(_)),
        "NAMELIST IOMSG requires a CHARACTER variable"
    );
    // The status of ERR= or END= without IOSTAT= is declared by the label
    // pass (see input::BRANCH_STATUS).
    if let E::Ident(n) = status {
        ensure!(
            n == crate::input::BRANCH_STATUS ||
            matches!(symbols.get(n),Some((T::Integer(_)|T::IntegerKind(_),d,_)) if d.is_empty()),
            "NAMELIST IOSTAT requires an INTEGER scalar variable"
        );
    }
    if let E::Ident(n) = message {
        ensure!(
            matches!(symbols.get(n),Some((T::Character(_)|T::CharacterLength(_),d,_)) if d.is_empty()),
            "NAMELIST IOMSG requires a CHARACTER scalar variable"
        );
    }
    let temp = |s: &str| format!("@nml_{serial}_{s}");
    let (code, h, field, token, index) = (
        temp("code"),
        temp("handle"),
        temp("field"),
        temp("token"),
        temp("index"),
    );
    let mut out = vec![decl(T::Integer(Some(8)), &code), set(&code, i(0))];
    if read {
        out.extend([
            decl(T::Integer(Some(8)), &h),
            decl(T::Integer(Some(8)), &index),
            decl(T::Character(Some(128)), &field),
            decl(T::Character(Some(4096)), &token),
        ]);
        let mut internal = matches!(unit,E::Ident(n) if matches!(symbols.get(n),Some((T::Character(_)|T::CharacterLength(_),d,_)) if d.is_empty()));
        let mut unit = unit.clone();
        // An element of a CHARACTER array is a one-record internal file: its
        // copy is read.
        if let E::Index(n, subscripts) | E::Call(n, subscripts) = &unit {
            if let Some((T::Character(Some(width)), dims, _)) = symbols.get(n) {
                if !dims.is_empty() && subscripts.len() == dims.len() && !subscripts.iter().any(|e| matches!(e, E::Section { .. }) || matches!(e, E::Call(c, _) if c == "@vector_subscript")) {
                    let copy = temp("element");
                    out.extend([decl(T::Character(Some(*width)), &copy), set(&copy, unit.clone())]);
                    unit = v(&copy);
                    internal = true;
                }
            }
        }
        // A CHARACTER array internal file: its records are joined, each
        // ending with a newline, into one buffer.
        if let E::Ident(n) = &unit {
            if let Some((kind, dims, _)) = symbols.get(n).filter(|(_, d, _)| !d.is_empty()) {
                if let T::Character(_) | T::CharacterLength(_) = kind {
                    let T::Character(Some(width)) = kind else {
                        bail!("NAMELIST READ from a CHARACTER array internal file currently requires a constant length");
                    };
                    let mut total = 1i64;
                    for d in dims {
                        let (E::IntLit(lo), E::IntLit(hi)) = bounds(d) else {
                            bail!("NAMELIST READ from a CHARACTER array internal file currently requires constant bounds");
                        };
                        let (lo, hi): (i64, i64) = (lo.parse()?, hi.parse()?);
                        total *= (hi - lo + 1).max(0);
                    }
                    let joined = temp("records");
                    let (k, at) = (temp("record_number"), temp("record_at"));
                    let size = (*width as i64 + 1) * total.max(1);
                    ensure!(size <= 1 << 24, "NAMELIST READ internal file is too large");
                    let range = |name: &str, lower: E, upper: E| E::Section { name: name.into(), lower: Some(Box::new(lower)), upper: Some(Box::new(upper)), stride: None };
                    out.extend([
                        decl(T::Character(Some(size as usize)), &joined),
                        decl(T::Integer(Some(8)), &k),
                        decl(T::Integer(Some(8)), &at),
                        set(&joined, text("")),
                        S::Do {
                            var: k.clone(),
                            start: i(1),
                            end: i(total),
                            body: vec![
                                set(&at, bin(B::Add, bin(B::Mul, bin(B::Sub, v(&k), i(1)), i(*width as i64 + 1)), i(1))),
                                S::AssignIndex {
                                    name: joined.clone(),
                                    indices: vec![range("", v(&at), bin(B::Add, v(&at), i(*width as i64 - 1)))],
                                    value: element(n, dims, v(&k)),
                                },
                                // Each element is a record: a comment ends with it.
                                S::AssignIndex {
                                    name: joined.clone(),
                                    indices: vec![range("", bin(B::Add, v(&at), i(*width as i64)), bin(B::Add, v(&at), i(*width as i64)))],
                                    value: text("
"),
                                },
                            ],
                        },
                    ]);
                    unit = v(&joined);
                    internal = true;
                }
            }
        }
        out.push(call(
            if internal {
                "@nml_internal_begin"
            } else {
                "@nml_begin"
            },
            vec![v(&h), v(&code), unit, text(group)],
        ));
        let mut work = vec![
            call(
                "@nml_next",
                vec![v(&code), v(&index), v(&field), v(&token), v(&h)],
            ),
            branch(bin(B::Lt, v(&code), i(0)), vec![S::Exit]),
        ];
        let mut dispatch = vec![set(&code, i(3))];
        // The dispatch selects a leaf (by number) and the position of the
        // value within it. Each type then converts the value once, and assigns
        // it to the selected leaf.
        let (sub, selected) = (temp("sub"), temp("leaf"));
        out.extend([decl(T::Integer(Some(8)), &sub), decl(T::Integer(Some(8)), &selected)]);
        let mut kinds: Vec<(String, Vec<Leaf>)> = vec![];
        for leaf in entries.iter().flat_map(|e| &e.leaves) {
            let key = format!("{:?}", leaf.kind);
            match kinds.iter_mut().find(|(k, _)| *k == key) {
                Some((_, leaves)) => {
                    if !leaves.iter().any(|l| l.field == leaf.field) {
                        leaves.push(leaf.clone());
                    }
                }
                None => kinds.push((key, vec![leaf.clone()])),
            }
        }
        let number = |leaf: &Leaf| -> i64 {
            let mut n = 0;
            for (_, leaves) in &kinds {
                for l in leaves {
                    n += 1;
                    if l.field == leaf.field {
                        return n;
                    }
                }
            }
            unreachable!()
        };
        let mut stores = vec![];
        let mut first_number = 1;
        for (k, (_, leaves)) in kinds.iter().enumerate() {
            let buffer = format!("@nml_{serial}_value_{k}");
            out.push(decl(leaves[0].kind.clone(), &buffer));
            let mut chain: Option<Vec<S>> = None;
            for (j, leaf) in leaves.iter().enumerate() {
                let body = vec![leaf.assign(v(&sub), v(&buffer))];
                chain = Some(match chain {
                    None => body,
                    Some(rest) => vec![S::If {
                        cond: bin(B::Eq, v(&selected), i(first_number + j as i64)),
                        then_body: body,
                        else_body: Some(rest),
                    }],
                });
            }
            let last_number = first_number + leaves.len() as i64 - 1;
            stores.push(branch(
                bin(B::And, bin(B::Ge, v(&selected), i(first_number)), bin(B::Le, v(&selected), i(last_number))),
                vec![
                    call("@internal_read", vec![v(&token), text("iostat"), v(&code), v(&buffer)]),
                    branch(bin(B::Eq, v(&code), i(0)), chain.unwrap_or_default()),
                    branch(bin(B::Lt, v(&code), i(0)), vec![set(&code, i(5))]),
                ],
            ));
            first_number = last_number + 1;
        }
        let (rec, position) = (temp("rec"), temp("position"));
        out.extend([decl(T::Integer(Some(8)), &rec), set(&rec, i(1)), decl(T::Integer(Some(8)), &position)]);
        for entry in entries.iter().rev() {
            let dims = &entry.dims;
            let per = entry.values();
            let mut matched = vec![call(
                "@nml_layout",
                vec![
                    v(&code),
                    v(&index),
                    v(&h),
                    i(0),
                    i(dims.len() as i64),
                    if entry.scaled { per.clone() } else { i(0) },
                    i(0),
                ],
            )];
            for (axis, d) in dims.iter().enumerate() {
                let (lo, hi) = bounds(d);
                matched.push(call(
                    "@nml_layout",
                    vec![
                        v(&code),
                        v(&index),
                        v(&h),
                        i(axis as i64 + 1),
                        i(dims.len() as i64),
                        lo,
                        hi,
                    ],
                ));
            }
            if let Some((elements, true)) = &entry.records {
                for (axis, d) in elements.iter().enumerate() {
                    let (lo, hi) = bounds(d);
                    matched.push(call(
                        "@nml_layout",
                        vec![v(&code), v(&rec), v(&h), i(-(axis as i64) - 1), i(elements.len() as i64), lo, hi],
                    ));
                }
            }
            let total = match &entry.records {
                Some((elements, false)) => bin(B::Mul, count(elements), per.clone()),
                _ => per.clone(),
            };
            matched.push(branch(
                bin(
                    B::Or,
                    bin(B::Lt, v(&index), i(1)),
                    bin(B::Gt, v(&index), total),
                ),
                vec![set(&code, i(4))],
            ));
            // A null advances the array position without defining its object.
            // Handle it before conversion: conversion errors may also be 2.
            matched.push(branch(
                bin(B::Eq, v(&code), i(2)),
                vec![set(&code, i(0)), S::Cycle],
            ));
            // Values spanning an array of derived type define each element
            // in turn.
            let mut store = match &entry.records {
                Some((_, false)) => vec![
                    set(&rec, bin(B::Add, bin(B::Div, bin(B::Sub, v(&index), i(1)), per.clone()), i(1))),
                    set(&position, bin(B::Add, E::Call("mod".into(), vec![bin(B::Sub, v(&index), i(1)), per.clone()]), i(1))),
                ],
                _ => vec![set(&position, v(&index))],
            };
            store.extend(if entry.leaves.len() > 1 {
                // Select the last leaf whose values start before `position`.
                let mut chain: Option<Vec<S>> = None;
                let mut first = i(0);
                for leaf in &entry.leaves {
                    let body = vec![
                        set(&sub, bin(B::Sub, v(&position), first.clone())),
                        set(&selected, i(number(leaf))),
                    ];
                    chain = Some(match chain {
                        None => body,
                        Some(rest) => vec![S::If {
                            cond: bin(B::Gt, v(&position), first.clone()),
                            then_body: body,
                            else_body: Some(rest),
                        }],
                    });
                    first = bin(B::Add, first, count(&leaf.dims));
                }
                chain.unwrap_or_default()
            } else {
                vec![set(&sub, v(&position)), set(&selected, i(number(&entry.leaves[0])))]
            });
            matched.push(branch(bin(B::Eq, v(&code), i(0)), store));

            dispatch = vec![S::If {
                cond: bin(B::Eq, v(&field), text(&entry.label)),
                then_body: matched,
                else_body: Some(dispatch),
            }];
        }
        work.push(set(&selected, i(0)));
        work.push(branch(
            bin(
                B::Or,
                bin(B::Eq, v(&code), i(0)),
                bin(B::Eq, v(&code), i(2)),
            ),
            dispatch,
        ));
        work.push(branch(bin(B::Eq, v(&code), i(0)), stores));
        out.push(S::DoWhile {
            cond: bin(B::Eq, v(&code), i(0)),
            body: work,
        });
        out.push(call("@nml_end", vec![v(&h)]));
        out.push(branch(bin(B::Eq, v(&code), i(-2)), vec![set(&code, i(0))]));
    } else {
        // An internal file: a scalar CHARACTER variable receives the whole
        // group as one record, and each element of a CHARACTER array one
        // output record, as in gfortran.
        let internal = match unit {
            E::Ident(n) => match symbols.get(n) {
                Some((T::Character(_) | T::CharacterLength(_), dims, _)) => Some((n.clone(), !dims.is_empty())),
                _ => None,
            },
            _ => None,
        };
        let unit_temp = temp("unit");
        let (piece, cursor, line, used) = (temp("piece"), temp("cursor"), temp("line"), temp("used"));
        // CHARACTER values are delimited as the unit's DELIM says, with
        // quotes by default (see unformatted_records::NAMELIST_DELIM).
        let (delimiter, quote, separator) = (temp("delimiter"), temp("quote"), temp("separator"));
        out.extend([decl(T::Integer(Some(8)), &delimiter), decl(T::Character(Some(1)), &quote), decl(T::Character(Some(1)), &separator)]);
        match &internal {
            None => out.extend([
                decl(T::Integer(Some(8)), &unit_temp),
                set(&unit_temp, unit.clone()),
                // Values follow their name on its line, as in gfortran.
                decl(T::Character(Some(8192)), &piece),
                decl(T::Character(Some(8192)), &line),
                decl(T::Integer(Some(8)), &used),
                set(&line, text("")),
                set(&used, i(0)),
                call("@file_inquire_connection", vec![v(&delimiter), v(&unit_temp), i(crate::unformatted_records::NAMELIST_DELIM)]),
            ]),
            Some((name, array)) => {
                out.extend([decl(T::Character(Some(8192)), &piece), decl(T::Integer(Some(8)), &cursor)]);
                if *array {
                    out.extend([
                        decl(T::Character(Some(8192)), &line),
                        decl(T::Integer(Some(8)), &used),
                        set(&line, text("")),
                        set(&used, i(0)),
                        set(&cursor, bin(B::Sub, E::Call("lbound".into(), vec![v(name), i(1)]), i(1))),
                    ]);
                } else {
                    out.extend([set(name, text("")), set(&cursor, i(0))]);
                }
                out.push(set(&delimiter, i(2)));
            }
        }
        out.extend([
            set(&quote, text("\"")),
            branch(bin(B::Eq, v(&delimiter), i(1)), vec![set(&quote, text("'"))]),
            set(&separator, text(",")),
            branch(bin(B::Eq, v(&delimiter), i(0)), vec![set(&separator, text(" "))]),
        ]);
        let unit = v(&unit_temp);
        let record = temp("record");
        out.push(decl(T::Character(Some(8192)), &record));
        let range = |name: &str, lower: E, upper: E| E::Section { name: name.into(), lower: Some(Box::new(lower)), upper: Some(Box::new(upper)), stride: None };
        // Store the pending line of a CHARACTER array internal file in its
        // next element.
        let flush = |name: &str| {
            vec![
                set(&cursor, bin(B::Add, v(&cursor), i(1))),
                S::If {
                    cond: bin(B::Gt, v(&cursor), E::Call("ubound".into(), vec![v(name), i(1)])),
                    then_body: vec![set(&code, i(5))],
                    else_body: Some(vec![S::AssignIndex { name: name.into(), indices: vec![v(&cursor)], value: v(&line) }]),
                },
                set(&line, text("")),
                set(&used, i(0)),
            ]
        };
        // `new` starts a record: a CHARACTER array internal file holds the
        // values of an item in the record of its label while they fit.
        let emit_record = |e: E, new: bool| {
            let body = match &internal {
                None => {
                    // A record holds a name and its values while they fit in
                    // WIDTH columns.
                    const WIDTH: i64 = 80;
                    let n = E::Call("len_trim".into(), vec![v(&piece)]);
                    let end = bin(B::Add, v(&used), n.clone());
                    let start = if new { bin(B::Gt, v(&used), i(0)) } else { bin(B::And, bin(B::Gt, end.clone(), i(WIDTH)), bin(B::Gt, v(&used), i(0))) };
                    vec![
                        set(&piece, e),
                        branch(start, vec![
                            call("@nml_record", vec![v(&code), unit.clone(), range(&line, i(1), v(&used))]),
                            set(&line, text("")),
                            set(&used, i(0)),
                        ]),
                        branch(bin(B::Gt, n.clone(), i(0)), vec![
                            S::AssignIndex { name: line.clone(), indices: vec![range("", bin(B::Add, v(&used), i(1)), end.clone())], value: range(&piece, i(1), n) },
                            set(&used, end),
                        ]),
                    ]
                }
                Some((name, true)) => {
                    let n = E::Call("len_trim".into(), vec![v(&piece)]);
                    let end = bin(B::Add, v(&used), n.clone());
                    let width = E::Call("len".into(), vec![v(name)]);
                    let full = bin(B::Gt, end.clone(), width.clone());
                    let start = if new { bin(B::Or, full, bin(B::Gt, v(&used), i(0))) } else { bin(B::And, full, bin(B::Gt, v(&used), i(0))) };
                    vec![
                        set(&piece, e),
                        branch(start, flush(name)),
                        S::If {
                            cond: bin(B::Gt, n.clone(), width),
                            then_body: vec![set(&code, i(5))],
                            else_body: Some(vec![
                                S::AssignIndex { name: line.clone(), indices: vec![range("", bin(B::Add, v(&used), i(1)), end.clone())], value: range(&piece, i(1), n) },
                                set(&used, end),
                            ]),
                        },
                    ]
                }
                Some((name, false)) => {
                    let n = E::Call("len_trim".into(), vec![v(&piece)]);
                    let end = bin(B::Add, v(&cursor), n.clone());
                    vec![
                        set(&piece, e),
                        S::If {
                            cond: bin(B::Gt, end.clone(), E::Call("len".into(), vec![v(name)])),
                            then_body: vec![set(&code, i(5))],
                            else_body: Some(vec![
                                S::AssignIndex {
                                    name: name.clone(),
                                    indices: vec![range("", bin(B::Add, v(&cursor), i(1)), end.clone())],
                                    value: range(&piece, i(1), n),
                                },
                                set(&cursor, end),
                            ]),
                        },
                    ]
                }
            };
            branch(bin(B::Eq, v(&code), i(0)), body)
        };
        let emit = |e: E| emit_record(e, true);
        out.push(emit(text(&format!("&{}", group.to_ascii_uppercase()))));
        // Output lists each intrinsic leaf; a derived object's components
        // appear as `x%a=` designators.
        // The components of an array of derived type are listed as `x%a=`
        // with the values of every element.
        let rec = temp("rec");
        let position = temp("position");
        out.extend([decl(T::Integer(Some(8)), &rec), set(&rec, i(1)), decl(T::Integer(Some(8)), &position)]);
        for (m, entry) in entries.iter().filter(|e| e.output).enumerate() {
            let leaf = &entry.leaves[0];
            let (n, kind) = (&entry.label, &leaf.kind);
            let ix = format!("@nml_{serial}_item_{m}");
            out.push(decl(T::Integer(Some(8)), &ix));
            let per = count(&leaf.dims);
            let (mut body, total) = match &entry.records {
                Some((elements, false)) => (vec![
                    set(&rec, bin(B::Add, bin(B::Div, bin(B::Sub, v(&ix), i(1)), per.clone()), i(1))),
                    set(&position, bin(B::Add, E::Call("mod".into(), vec![bin(B::Sub, v(&ix), i(1)), per.clone()]), i(1))),
                ], bin(B::Mul, count(elements), per)),
                _ => (vec![set(&position, v(&ix))], per),
            };
            // Each element of an array of derived type is labeled with its
            // subscripts, as `tr(2)%w=`: `tr%w` is not a valid designator
            // when the component is an array.
            if let (Some((elements, false)), Some((base, index, path))) = (&entry.records, &leaf.record) {
                let format = format!("(' {}(',{},')%{}=')", label(base, members).to_ascii_uppercase(), vec!["i0"; elements.len()].join(",',',"), path.to_ascii_uppercase());
                let mut label = vec![v(&record), text(&format)];
                label.extend(index.iter().cloned());
                body.push(branch(bin(B::Eq, v(&position), i(1)), vec![
                    call("@write", label),
                    emit(E::Call("trim".into(), vec![v(&record)])),
                ]));
            }
            let item = leaf.value(v(&position));
            let format;
            let mut values = vec![];
            match kind {
                T::Integer(_)|T::IntegerKind(_)=>{format="(i0,a)";values.push(item);},
                T::Real|T::RealKind(_)|T::DoublePrecision=>{format="(es26.17,a)";values.push(item);},
                T::Logical|T::LogicalKind(_)=>{format="(l1,a)";values.push(item);},
                T::Complex(_)=>{format="('(',es26.17,',',es26.17,')',a)";values.extend([E::Call("real".into(),vec![item.clone()]),E::Call("aimag".into(),vec![item])]);},
                T::Character(Some(width))=>{
                    ensure!(*width<=4000,"NAMELIST CHARACTER output currently supports lengths up to 4000");
                    let escaped=format!("@nml_{serial}_escaped_{m}");let pos=format!("@nml_{serial}_pos_{m}");let ch=format!("@nml_{serial}_ch_{m}");let copy=format!("@nml_{serial}_copy_{m}");
                    out.extend([decl(T::Character(Some(2*width+2)),&escaped),decl(T::Character(Some(*width)),&copy),decl(T::Integer(Some(8)),&pos),decl(T::Integer(Some(8)),&ch)]);
                    let sub=|n:&str,start:E,end:E|E::Section{name:n.into(),lower:Some(Box::new(start)),upper:Some(Box::new(end)),stride:None};
                    let put=|value:E|S::AssignIndex{name:escaped.clone(),indices:vec![E::Section{name:String::new(),lower:Some(Box::new(v(&pos))),upper:Some(Box::new(v(&pos))),stride:None}],value};
                    let advance=||set(&pos,bin(B::Add,v(&pos),i(1)));
                    // DELIM='NONE' writes the characters alone.
                    let delimited=bin(B::Ne,v(&delimiter),i(0));
                    body.extend([set(&copy,item),set(&escaped,text("")),set(&pos,i(1)),
                        branch(delimited.clone(),vec![put(v(&quote)),advance()]),
                        S::Do{var:ch.clone(),start:i(1),end:i(*width as i64),body:vec![put(sub(&copy,v(&ch),v(&ch))),advance(),
                            branch(bin(B::And,delimited.clone(),bin(B::Eq,sub(&copy,v(&ch),v(&ch)),v(&quote))),vec![put(v(&quote)),advance()])]},
                        branch(delimited,vec![put(v(&quote)),advance()])]);
                    format="(a,a)";values.push(sub(&escaped,i(1),bin(B::Sub,v(&pos),i(1))));
                },_=>bail!("NAMELIST output currently requires an intrinsic type with a fixed CHARACTER length"),
            }
            values.push(v(&separator));
            let mut write = vec![v(&record), text(format)];
            write.extend(values);
            body.push(call("@write", write));
            body.push(emit_record(E::Call("trim".into(), vec![v(&record)]), false));
            if !matches!(entry.records, Some((_, false))) {
                out.push(emit(text(&format!(" {}=", n.to_ascii_uppercase()))));
            }
            out.push(S::Do {
                var: ix,
                start: i(1),
                end: total,
                body: vec![branch(bin(B::Eq, v(&code), i(0)), body)],
            });
        }
        out.push(emit(text(" /")));
        match &internal {
            Some((name, true)) => out.push(branch(bin(B::And, bin(B::Eq, v(&code), i(0)), bin(B::Gt, v(&used), i(0))), flush(name))),
            None => out.push(branch(bin(B::And, bin(B::Eq, v(&code), i(0)), bin(B::Gt, v(&used), i(0))),
                vec![call("@nml_record", vec![v(&code), unit.clone(), range(&line, i(1), v(&used))])])),
            _ => {}
        }
    }
    if let E::Ident(n) = message {
        let mut error = vec![set(
            n,
            text("NAMELIST input/output error (invalid name, value, bounds or unit)"),
        )];
        error.push(branch(
            bin(B::Eq, v(&code), i(-1)),
            vec![set(n, text("End of file while reading NAMELIST"))],
        ));
        out.push(branch(bin(B::Ne, v(&code), i(0)), error));
    }
    if let E::Ident(n) = status {
        out.push(set(n, v(&code)));
    } else {
        out.push(branch(
            bin(B::Ne, v(&code), i(0)),
            vec![call(
                "@error_stop",
                vec![text("NAMELIST input/output error")],
            )],
        ));
    }
    Ok(S::Block { body: out })
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Scalar SELECT TYPE and bound dispatch with private dynamic-type specialization.
//! Retain the original declared-type dummy and add borrowed component views:
//! generic resolution outside a guard must still see the declared parent.
use crate::ast::{CaseItem, CaseOrDefault, Expr, Program, Stmt};
use crate::lexer::{Token, TokenKind};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
const SELECT: &str = "@select_type";

pub(crate) fn tokens(tokens: &mut Vec<Token>, source: &str) {
    let mut i = 0;
    while i < tokens.len() {
        let text = source[tokens[i].span.clone()].to_ascii_lowercase();
        let head = i == 0
            || !crate::lexer::same_statement(&source[tokens[i - 1].span.end..tokens[i].span.start]);
        if text == "type" && i > 0 && matches!(tokens[i - 1].kind, TokenKind::KwSelect) {
            tokens[i].kind = TokenKind::KwType;
        }
        let next = tokens
            .get(i + 1)
            .filter(|t| crate::lexer::same_statement(&source[tokens[i].span.end..t.span.start]))
            .map(|t| source[t.span.clone()].to_ascii_lowercase());
        if head && matches!(text.as_str(), "type" | "class") {
            let guard = if next.as_deref() == Some("is")
                && tokens
                    .get(i + 2)
                    .is_some_and(|t| matches!(t.kind, TokenKind::LParen))
            {
                Some(if text == "type" {
                    TokenKind::KwTypeIs
                } else {
                    TokenKind::KwClassIs
                })
            } else if text == "class" && next.as_deref() == Some("default") {
                Some(TokenKind::KwClassDefault)
            } else {
                None
            };
            if let Some(kind) = guard {
                tokens[i].kind = kind;
                tokens[i].span.end = tokens[i + 1].span.end;
                tokens.remove(i + 1);
            }
        }
        i += 1;
    }
}
pub(crate) fn construct(
    selector: (String, String),
    blocks: Vec<CaseOrDefault>,
) -> std::result::Result<Stmt, &'static str> {
    let mut cases = vec![];
    let mut default = None;
    for b in blocks {
        match b {
            CaseOrDefault::Case(c) => cases.push(c),
            CaseOrDefault::Default(b) => {
                if default.replace(b).is_some() {
                    return Err("duplicate CLASS DEFAULT guard");
                }
            }
        }
    }
    Ok(Stmt::SelectCase {
        expr: Expr::Call(
            SELECT.into(),
            vec![Expr::Ident(selector.1), Expr::Str(selector.0)],
        ),
        cases,
        default,
    })
}
pub(crate) fn selection(e: &Expr) -> Option<(&str, &str)> {
    if let Expr::Call(n, a) = e {
        if n == SELECT {
            if let [Expr::Ident(n), Expr::Str(alias)] = a.as_slice() {
                return Some((n, alias));
            }
        }
    }
    None
}
fn guard(item: &CaseItem) -> Result<(bool, &str)> {
    if let CaseItem::Single(Expr::Call(n, a)) = item {
        if matches!(n.as_str(), "@type_is" | "@class_is") {
            if let [Expr::Ident(t)] = a.as_slice() {
                return Ok((n == "@class_is", t));
            }
        }
    }
    bail!("invalid SELECT TYPE guard")
}
#[derive(Clone)]
struct Object {
    kind: String,
    polymorphic: bool,
    rank: usize,
    attrs: Vec<String>,
}
type Objects = HashMap<String, Object>;
fn objects(body: &[Stmt], inherited: &Objects) -> Objects {
    let mut vars = inherited.clone();
    for s in body {
        match s {
            Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic,
                dims,
            } => {
                for n in names {
                    vars.insert(
                        n.clone(),
                        Object {
                            kind: type_name.clone(),
                            polymorphic: *polymorphic,
                            rank: dims.len(),
                            attrs: attributes.clone(),
                        },
                    );
                }
            }
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    vars.remove(n);
                }
            }
            Stmt::ArrayDecl { name, .. } => {
                vars.remove(name);
            }
            Stmt::InitializedDecl { entities, .. } => {
                for (n, _) in entities {
                    vars.remove(n);
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(v) = vars.get_mut(name) {
                if !v.attrs.contains(intent) {
                    v.attrs.push(intent.clone());
                }
            }
        }
    }
    vars
}
#[derive(Default)]
struct Definitions {
    parents: HashMap<String, String>,
    bound_targets: HashSet<String>,
    abstracts: HashSet<String>,
    fields: HashMap<String, HashMap<String, Option<String>>>,
    bindings: HashMap<String, HashMap<String, String>>,
}
impl Definitions {
    fn collect(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == crate::abstract_types::ABSTRACT => {
                    if let [Expr::Ident(n)] = args.as_slice() {
                        self.abstracts.insert(n.clone());
                    }
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() {
                        self.parents.insert(child.clone(), parent.clone());
                    }
                }
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    let mut fields = HashMap::new();
                    for s in components {
                        match s {
                            Stmt::VarDecl { names, .. } => {
                                for n in names {
                                    fields.insert(n.clone(), None);
                                }
                            }
                            Stmt::ArrayDecl { name, .. } => {
                                fields.insert(name.clone(), None);
                            }
                            Stmt::InitializedDecl { entities, .. } => {
                                for (n, _) in entities {
                                    fields.insert(n.clone(), None);
                                }
                            }
                            Stmt::DerivedDecl {
                                type_name, names, ..
                            } => {
                                for n in names {
                                    fields.insert(n.clone(), Some(type_name.clone()));
                                }
                            }
                            _ => {}
                        }
                    }
                    self.bound_targets.extend(
                        bindings
                            .iter()
                            .filter(|(n, _)| !n.starts_with('@'))
                            .map(|(_, target)| target.clone()),
                    );
                    self.fields.insert(name.clone(), fields);
                    self.bindings.insert(
                        name.clone(),
                        bindings
                            .iter()
                            .filter(|(n, _)| !n.starts_with('@'))
                            .cloned()
                            .collect(),
                    );
                }
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. }
                | Stmt::Block { body } => self.collect(body),
                _ => {}
            }
        }
    }
    fn distance(&self, child: &str, parent: &str) -> Option<usize> {
        let mut t = child;
        let mut seen = HashSet::new();
        let mut d = 0;
        loop {
            if t == parent {
                return Some(d);
            }
            if !seen.insert(t) {
                return None;
            }
            t = self.parents.get(t)?;
            d += 1;
        }
    }
    fn binding(&self, kind: &str, method: &str) -> Option<String> {
        let mut kind = kind;
        let mut seen = HashSet::new();
        loop {
            if let Some(target) = self.bindings.get(kind).and_then(|b| b.get(method)) {
                return Some(target.clone());
            }
            if !seen.insert(kind) {
                return None;
            }
            kind = self.parents.get(kind)?;
        }
    }
    fn field(&self, ty: &str, name: &str) -> Option<Option<String>> {
        let mut ty = ty;
        let mut seen = HashSet::new();
        loop {
            if let Some(value) = self.fields.get(ty).and_then(|f| f.get(name)) {
                return Some(value.clone());
            }
            if self.bindings.get(ty).is_some_and(|b| b.contains_key(name)) {
                return Some(None);
            }
            if !seen.insert(ty) {
                return None;
            }
            let parent = self.parents.get(ty)?;
            if parent.rsplit('$').next() == Some(name) {
                return Some(Some(parent.clone()));
            }
            ty = parent;
        }
    }
    fn component(&self, name: &str, vars: &Objects) -> Result<()> {
        let Some((root, rest)) = name.split_once('%') else {
            return Ok(());
        };
        let Some(obj) = vars.get(root) else {
            return Ok(());
        };
        let mut ty = obj.kind.clone();
        for part in rest.split('%') {
            let Some(field) = self.field(&ty, part) else {
                bail!("component or binding `{part}` is not declared in type `{ty}`");
            };
            if let Some(kind) = field {
                ty = kind;
            } else {
                break;
            }
        }
        Ok(())
    }
    fn validate(&self, body: &[Stmt], inherited: &Objects) -> Result<()> {
        let vars = objects(body, inherited);
        let mut location = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                location = Some(p);
            }
            let result = (|| -> Result<()> {
                if let Stmt::Function {
                    pure: true,
                    params,
                    body,
                    ..
                }
                | Stmt::Subroutine {
                    pure: true,
                    params,
                    body,
                    ..
                } = s
                {
                    let local = objects(body, &vars);
                    if params.iter().any(|n| {
                        local.get(n).is_some_and(|v| {
                            v.polymorphic && v.attrs.iter().any(|a| a == "intent(out)")
                        })
                    }) {
                        bail!("PURE procedure cannot have a polymorphic INTENT(OUT) dummy");
                    }
                }
                if let Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } = s
                {
                    if let Some((selector, alias)) = selection(expr) {
                        let obj = vars.get(selector).context(
                            "SELECT TYPE selector must be a polymorphic scalar dummy argument",
                        )?;
                        if !obj.polymorphic {
                            bail!("SELECT TYPE selector `{selector}` must be polymorphic");
                        }
                        if obj.rank != 0 {
                            bail!("array SELECT TYPE selectors are not yet supported");
                        }
                        let mut seen = HashSet::new();
                        for c in cases {
                            let (class, kind) = guard(&c.items[0])?;
                            if !self.fields.contains_key(kind) {
                                bail!("unknown derived type `{kind}` in SELECT TYPE guard");
                            }
                            if self.distance(kind, &obj.kind).is_none() {
                                bail!(
                                    "SELECT TYPE guard `{kind}` must extend declared type `{}`",
                                    obj.kind
                                );
                            }
                            if !seen.insert((class, kind)) {
                                bail!("duplicate SELECT TYPE guard for `{kind}`");
                            }
                            let mut branch = vars.clone();
                            branch.insert(
                                alias.into(),
                                Object {
                                    kind: kind.into(),
                                    polymorphic: class,
                                    rank: 0,
                                    attrs: obj.attrs.clone(),
                                },
                            );
                            self.validate(&c.body, &branch)?;
                        }
                        if let Some(b) = default {
                            let mut branch = vars.clone();
                            branch.insert(alias.into(), obj.clone());
                            self.validate(b, &branch)?;
                        }
                        return Ok(());
                    }
                }
                if let Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } = s {
                    let root = name.split('%').next().unwrap_or(name);
                    if vars
                        .get(root)
                        .is_some_and(|v| v.attrs.iter().any(|a| a == "intent(in)"))
                    {
                        bail!("cannot define INTENT(IN) entity `{name}`");
                    }
                }
                match s {
                    Stmt::Assign { name, .. }
                    | Stmt::AssignIndex { name, .. }
                    | Stmt::CallSub { name, .. } => self.component(name, &vars)?,
                    _ => {}
                }
                let mut s = s.clone();
                for e in expressions(&mut s) {
                    let mut error = None;
                    visit_expr(e, &mut |e| {
                        if let Expr::Ident(n)
                        | Expr::Call(n, _)
                        | Expr::Index(n, _)
                        | Expr::Section { name: n, .. } = e
                        {
                            if let Err(e) = self.component(n, &vars) {
                                error = Some(e);
                            }
                        }
                    });
                    if let Some(e) = error {
                        return Err(e);
                    }
                }
                for b in children(&mut s) {
                    self.validate(b, &vars)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Template {
    statement: Stmt,
    params: Vec<String>,
    dummies: Vec<(usize, String, Object)>,
    host: Objects,
}
fn collect(
    body: &[Stmt],
    inherited: &Objects,
    templates: &mut HashMap<String, Template>,
) -> Result<()> {
    let vars = objects(body, inherited);
    for s in body {
        match s {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => {
                let local = objects(body, &vars);
                let dummies = params
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| {
                        local
                            .get(n)
                            .filter(|v| v.polymorphic)
                            .map(|v| (i, n.clone(), v.clone()))
                    })
                    .collect::<Vec<_>>();
                if !dummies.is_empty() {
                    templates.insert(
                        name.clone(),
                        Template {
                            statement: s.clone(),
                            params: params.clone(),
                            dummies,
                            host: vars.clone(),
                        },
                    );
                }
                collect(body, &vars, templates)?;
            }
            Stmt::Module { body, .. } | Stmt::Block { body } => collect(body, &vars, templates)?,
            _ => {}
        }
    }
    Ok(())
}
// Borrowed views are parameters of the specialized procedure, all associated
// with the same caller storage. Never copy the selector into a local record.
#[derive(Clone)]
struct Dynamic {
    kind: String,
    value: Expr,
}
type Dynamics = HashMap<String, Dynamic>;
#[derive(Clone)]
struct Extra {
    position: usize,
    parameter: String,
    kind: String,
    polymorphic: bool,
    attrs: Vec<String>,
    initialize: bool,
}
struct Lowerer {
    templates: HashMap<String, Template>,
    defs: Definitions,
    cache: HashMap<(String, Vec<String>), String>,
    extras: HashMap<String, Vec<Extra>>,
    generated: HashMap<String, Vec<Stmt>>,
    wrappers: HashMap<String, Stmt>,
    names: Vec<(String, String)>,
    serial: usize,
}
fn rename(body: &mut [Stmt], from: &str, to: &str) {
    fn name(n: &mut String, from: &str, to: &str) {
        if n == from {
            *n = to.into();
        } else if n.strip_prefix(from).is_some_and(|s| s.starts_with('%')) {
            *n = format!("{to}{}", &n[from.len()..]);
        }
    }
    for s in body {
        match s {
            Stmt::Assign { name: n, .. }
            | Stmt::AssignIndex { name: n, .. }
            | Stmt::CallSub { name: n, .. }
            | Stmt::DummyIntent { name: n, .. } => name(n, from, to),
            _ => {}
        }
        for e in expressions(s) {
            visit_expr(e, &mut |e| match e {
                Expr::Ident(n)
                | Expr::Call(n, _)
                | Expr::Index(n, _)
                | Expr::Section { name: n, .. } => name(n, from, to),
                _ => {}
            });
        }
        for b in children(s) {
            rename(b, from, to);
        }
    }
}
impl Lowerer {
    fn specialize(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        vars: &Objects,
        dynamic: &Dynamics,
    ) -> Result<String> {
        let t = self.templates[name].clone();
        let source = match &t.statement {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body,
            _ => unreachable!(),
        };
        crate::named_arguments::Signature::new(&t.params, source).reorder(name, args)?;
        args.resize_with(t.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        let mut actuals = vec![];
        let mut kinds = vec![];
        for (position, dummy, formal) in &t.dummies {
            if formal.rank != 0 {
                bail!("polymorphic call specialization currently supports scalar CLASS dummies only (`{dummy}`)");
            }
            if formal
                .attrs
                .iter()
                .any(|a| matches!(a.as_str(), "pointer" | "allocatable" | "value"))
            {
                bail!("polymorphic call specialization does not yet support POINTER, ALLOCATABLE or VALUE CLASS dummies");
            }
            let actual = &args[*position];
            if crate::named_arguments::omitted(actual) {
                kinds.push(formal.kind.clone());
                actuals.push(actual.clone());
                continue;
            }
            let Expr::Ident(n) = actual else {
                bail!("polymorphic call specialization currently requires a scalar variable actual for `{dummy}`");
            };
            if vars
                .get(n)
                .is_some_and(|v| v.attrs.iter().any(|a| a == "intent(in)"))
                && formal
                    .attrs
                    .iter()
                    .any(|a| matches!(a.as_str(), "intent(out)" | "intent(inout)"))
            {
                bail!("cannot pass INTENT(IN) entity `{n}` to writable CLASS dummy `{dummy}`");
            }
            let (kind, value) = if let Some(d) = dynamic.get(n) {
                (d.kind.clone(), d.value.clone())
            } else {
                let obj = vars
                    .get(n)
                    .context("CLASS dummy requires a derived-type actual")?;
                if obj.rank != 0 {
                    bail!("scalar CLASS dummy `{dummy}` requires a scalar actual");
                }
                if obj.polymorphic {
                    bail!(
                        "cannot determine dynamic type of polymorphic actual `{n}` for polymorphic calls"
                    );
                }
                (obj.kind.clone(), actual.clone())
            };
            if self.defs.distance(&kind, &formal.kind).is_none() {
                bail!(
                    "actual type `{kind}` does not extend CLASS dummy type `{}`",
                    formal.kind
                );
            }
            kinds.push(kind);
            actuals.push(value);
        }
        let key = (name.to_owned(), kinds.clone());
        let unique = if let Some(n) = self.cache.get(&key) {
            n.clone()
        } else {
            self.serial += 1;
            let unique = format!("{name}$type{}", self.serial);
            self.cache.insert(key, unique.clone());
            self.names.push((name.into(), unique.clone()));
            let mut statement = t.statement.clone();
            let (params, body) = match &mut statement {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    *name = unique.clone();
                    (params, body)
                }
                _ => unreachable!(),
            };
            // Record flattening propagates attributes from DerivedDecl. Merge
            // separately written attributes before expanding component dummies.
            let declared = objects(source, &t.host);
            let dummy_names: HashSet<_> = t.dummies.iter().map(|(_, n, _)| n.clone()).collect();
            *body = std::mem::take(body)
                .into_iter()
                .flat_map(|s| {
                    if let Stmt::DerivedDecl {
                        type_name,
                        names,
                        attributes,
                        polymorphic,
                        dims,
                    } = &s
                    {
                        return names
                            .iter()
                            .map(|n| {
                                let mut attrs = if dummy_names.contains(n) {
                                    declared[n].attrs.clone()
                                } else {
                                    attributes.clone()
                                };
                                if dummy_names.contains(n) {
                                    for a in &mut attrs {
                                        if a == "intent(out)" {
                                            *a = "intent(inout)".into();
                                        }
                                    }
                                }
                                Stmt::DerivedDecl {
                                    type_name: type_name.clone(),
                                    names: vec![n.clone()],
                                    attributes: attrs,
                                    polymorphic: *polymorphic,
                                    dims: dims.clone(),
                                }
                            })
                            .collect::<Vec<_>>();
                    }
                    vec![s]
                })
                .collect();
            body.retain(|s|!matches!(s,Stmt::DummyIntent{name,intent} if dummy_names.contains(name) && matches!(intent.as_str(),"optional"|"intent(in)"|"intent(out)"|"intent(inout)"|"target")));
            let mut local = Dynamics::new();
            let mut extras = vec![];
            for ((position, dummy, formal), kind) in t.dummies.iter().zip(&kinds) {
                let parameter = format!("{dummy}$dynamic");
                extras.push(Extra {
                    position: *position,
                    parameter: parameter.clone(),
                    kind: kind.clone(),
                    polymorphic: false,
                    attrs: formal.attrs.clone(),
                    initialize: true,
                });
                local.insert(
                    dummy.clone(),
                    Dynamic {
                        kind: kind.clone(),
                        value: Expr::Ident(parameter),
                    },
                );
            }
            self.select(body, &local, &t, &mut extras)?;
            for e in &extras {
                let (_, original, _) = t.dummies.iter().find(|(i, _, _)| *i == e.position).unwrap();
                local.insert(e.parameter.clone(), local[original].clone());
                params.push(e.parameter.clone());
                // Initialize the complete dynamic object once for INTENT(OUT).
                // Other views borrow it without repeating default initialization.
                let attrs = e
                    .attrs
                    .iter()
                    .map(|a| {
                        if a == "intent(out)" && !e.initialize {
                            "intent(inout)".into()
                        } else {
                            a.clone()
                        }
                    })
                    .collect();
                body.insert(
                    0,
                    Stmt::DerivedDecl {
                        type_name: e.kind.clone(),
                        names: vec![e.parameter.clone()],
                        attributes: attrs,
                        polymorphic: e.polymorphic,
                        dims: vec![],
                    },
                );
            }
            self.extras.insert(unique.clone(), extras);
            let implicit = matches!(&t.statement,Stmt::Function{body,..} if !matches!(body.last(),Some(Stmt::Return(Some(Expr::Ident(n)))) if n!=name));
            self.body(body, &t.host, &local)?;
            if implicit {
                crate::procedure_arguments::rename(body, name, &unique);
            }
            self.generated
                .entry(name.into())
                .or_default()
                .push(statement);
            unique
        };
        for extra in &self.extras[&unique] {
            let index = t
                .dummies
                .iter()
                .position(|(i, _, _)| *i == extra.position)
                .unwrap();
            args.push(actuals[index].clone());
        }
        Ok(unique)
    }
    fn select(
        &mut self,
        body: &mut Vec<Stmt>,
        dynamic: &Dynamics,
        template: &Template,
        extras: &mut Vec<Extra>,
    ) -> Result<()> {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::SelectCase {
                expr,
                cases,
                default,
            } = &mut s
            {
                if let Some((selector, alias)) = selection(expr) {
                    let selector = selector.to_owned();
                    let alias = alias.to_owned();
                    let actual = dynamic
                        .get(&selector)
                        .context("SELECT TYPE selector must be a supported CLASS dummy")?
                        .clone();
                    let mut chosen = None;
                    let mut best = usize::MAX;
                    for c in cases {
                        let (class, kind) = guard(&c.items[0])?;
                        let distance = if class {
                            self.defs.distance(&actual.kind, kind).map(|d| d + 1)
                        } else {
                            (actual.kind == kind).then_some(0)
                        };
                        if let Some(distance) = distance {
                            if distance < best {
                                best = distance;
                                chosen = Some((kind.to_owned(), class, c.body.clone()));
                            }
                        }
                    }
                    let position = template
                        .dummies
                        .iter()
                        .find(|(_, n, _)| n == &selector)
                        .map(|(i, _, _)| *i)
                        .or_else(|| {
                            extras
                                .iter()
                                .find(|e| e.parameter == selector)
                                .map(|e| e.position)
                        })
                        .context("SELECT TYPE on local associate names is not yet supported")?;
                    let (_, _, formal) = template
                        .dummies
                        .iter()
                        .find(|(i, _, _)| *i == position)
                        .unwrap();
                    let mut local = dynamic.clone();
                    let mut selected = if let Some((kind, class, mut branch)) = chosen {
                        let view = format!("{selector}$view{}", extras.len());
                        extras.push(Extra {
                            position,
                            parameter: view.clone(),
                            kind,
                            polymorphic: class,
                            attrs: formal.attrs.clone(),
                            initialize: false,
                        });
                        rename(&mut branch, &alias, &view);
                        local.insert(view, actual);
                        branch
                    } else {
                        let mut b = default.take().unwrap_or_default();
                        rename(&mut b, &alias, &selector);
                        b
                    };
                    self.select(&mut selected, &local, template, extras)?;
                    out.push(Stmt::Block { body: selected });
                    continue;
                }
            }
            for b in children(&mut s) {
                self.select(b, dynamic, template, extras)?;
            }
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    fn call(
        &mut self,
        name: &mut String,
        args: &mut Vec<Expr>,
        vars: &Objects,
        dynamic: &Dynamics,
    ) -> Result<()> {
        let mut parent_dynamic = None;
        if let Some((object, method)) = name.rsplit_once('%') {
            let mut view = vars.get(object).cloned().map(|v| (v, None));
            // An explicit parent component is nonpolymorphic. It deliberately
            // bypasses an override, including dispatch from inside that call.
            if view.is_none() {
                if let Some((root, rest)) = object.split_once('%') {
                    if let Some(declared) = vars.get(root) {
                        let mut kind = declared.kind.clone();
                        let mut parent = true;
                        for component in rest.split('%') {
                            match self.defs.field(&kind, component) {
                                Some(Some(next))
                                    if self.defs.distance(&kind, &next).is_some_and(|d| d > 0) =>
                                {
                                    kind = next
                                }
                                _ => {
                                    parent = false;
                                    break;
                                }
                            }
                        }
                        if parent {
                            let root_value = dynamic
                                .get(root)
                                .map(|d| d.value.clone())
                                .unwrap_or_else(|| Expr::Ident(root.into()));
                            if let Expr::Ident(root_value) = root_value {
                                let value = Expr::Ident(format!("{root_value}%{rest}"));
                                let mut obj = declared.clone();
                                obj.kind = kind;
                                obj.polymorphic = false;
                                obj.rank = 0;
                                view = Some((obj, Some(value)));
                            }
                        }
                    }
                }
            }
            if let Some((declared, parent)) = view {
                if self.defs.binding(&declared.kind, method).is_some() {
                    if declared.rank != 0 {
                        bail!("dynamic binding dispatch currently requires a scalar passed object");
                    }
                    let (kind, value) = if let Some(value) = parent {
                        let Expr::Ident(key) = &value else {
                            unreachable!()
                        };
                        let mut map = dynamic.clone();
                        map.insert(
                            key.clone(),
                            Dynamic {
                                kind: declared.kind.clone(),
                                value: value.clone(),
                            },
                        );
                        parent_dynamic = Some(map);
                        (declared.kind.clone(), value)
                    } else if let Some(actual) = dynamic.get(object) {
                        (actual.kind.clone(), actual.value.clone())
                    } else {
                        if declared.polymorphic {
                            bail!("cannot determine dynamic type of CLASS object `{object}` for binding dispatch");
                        }
                        (declared.kind.clone(), Expr::Ident(object.into()))
                    };
                    let target = self
                        .defs
                        .binding(&kind, method)
                        .with_context(|| format!("type `{kind}` has no binding `{method}`"))?;
                    if declared.attrs.iter().any(|a| a == "intent(in)")
                        && self.templates.get(&target).is_some_and(|t| {
                            t.dummies.iter().any(|(i, _, v)| {
                                *i == 0
                                    && v.attrs.iter().any(|a| {
                                        matches!(a.as_str(), "intent(out)" | "intent(inout)")
                                    })
                            })
                        })
                    {
                        bail!("cannot pass INTENT(IN) object `{object}` to a writable passed-object dummy");
                    }
                    args.insert(0, value);
                    *name = target;
                }
            }
        }
        if self.templates.contains_key(name) {
            *name =
                self.specialize(name, args, vars, parent_dynamic.as_ref().unwrap_or(dynamic))?;
        }
        Ok(())
    }
    fn expression(&mut self, e: &mut Expr, vars: &Objects, dynamic: &Dynamics) -> Result<()> {
        match e {
            Expr::Call(n, args) => {
                self.call(n, args, vars, dynamic)?;
                for a in args {
                    self.expression(a, vars, dynamic)?;
                }
            }
            Expr::Index(_, a) => {
                for e in a {
                    self.expression(e, vars, dynamic)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, vars, dynamic)?;
                self.expression(b, vars, dynamic)?;
            }
            Expr::Un(_, a) => self.expression(a, vars, dynamic)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, vars, dynamic)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut [Stmt], inherited: &Objects, dynamic: &Dynamics) -> Result<()> {
        let vars = objects(body, inherited);
        let mut location = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                location = Some(p);
            }
            let result = (|| -> Result<()> {
                if matches!(s,Stmt::Function{name,..}|Stmt::Subroutine{name,..} if self.templates.contains_key(name))
                {
                    return Ok(());
                }
                if matches!(s,Stmt::SelectCase{expr,..} if selection(expr).is_some()) {
                    bail!("SELECT TYPE selector must be a supported scalar CLASS dummy argument");
                }
                if let Stmt::CallSub { name, args } = s {
                    self.call(name, args, &vars, dynamic)?;
                }
                for e in expressions(s) {
                    self.expression(e, &vars, dynamic)?;
                }
                for b in children(s) {
                    self.body(b, &vars, dynamic)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
    fn binding_wrappers(&mut self) -> Result<()> {
        let targets = self.defs.bound_targets.iter().cloned().collect::<Vec<_>>();
        for name in targets {
            let Some(t) = self.templates.get(&name).cloned() else {
                continue;
            };
            if t.dummies
                .iter()
                .any(|(_, _, v)| self.defs.abstracts.contains(&v.kind))
            {
                continue;
            }
            let source = match &t.statement {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body,
                _ => unreachable!(),
            };
            let vars = objects(source, &t.host);
            let dynamic = t
                .dummies
                .iter()
                .map(|(_, n, v)| {
                    (
                        n.clone(),
                        Dynamic {
                            kind: v.kind.clone(),
                            value: Expr::Ident(n.clone()),
                        },
                    )
                })
                .collect();
            let mut args = t
                .params
                .iter()
                .map(|n| Expr::Ident(n.clone()))
                .collect::<Vec<_>>();
            let target = self.specialize(&name, &mut args, &vars, &dynamic)?;
            let mut wrapper = t.statement.clone();
            let body = match &mut wrapper {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body,
                _ => unreachable!(),
            };
            let result = match &t.statement {
                Stmt::Function { body, .. } => Some(match body.last() {
                    Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                    _ => name.clone(),
                }),
                _ => None,
            };
            let keep = |n: &String| t.params.contains(n) || result.as_ref() == Some(n);
            *body = source
                .iter()
                .filter_map(|s| match s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        polymorphic,
                        dims,
                        attributes,
                    } => {
                        let names = names
                            .iter()
                            .filter(|n| keep(n))
                            .cloned()
                            .collect::<Vec<_>>();
                        if names.is_empty() {
                            None
                        } else {
                            Some(Stmt::DerivedDecl {
                                type_name: type_name.clone(),
                                names,
                                polymorphic: *polymorphic,
                                dims: dims.clone(),
                                attributes: attributes
                                    .iter()
                                    .map(|a| {
                                        if a == "intent(out)" {
                                            "intent(inout)".into()
                                        } else {
                                            a.clone()
                                        }
                                    })
                                    .collect(),
                            })
                        }
                    }
                    Stmt::VarDecl { kind, names } => {
                        let names = names
                            .iter()
                            .filter(|n| keep(n))
                            .cloned()
                            .collect::<Vec<_>>();
                        if names.is_empty() {
                            None
                        } else {
                            Some(Stmt::VarDecl {
                                kind: kind.clone(),
                                names,
                            })
                        }
                    }
                    Stmt::ArrayDecl { name, .. } | Stmt::DummyIntent { name, .. } if keep(name) => {
                        Some(s.clone())
                    }
                    Stmt::InitializedDecl { attributes, .. }
                        if attributes.iter().any(|a| a == "parameter") =>
                    {
                        Some(s.clone())
                    }
                    Stmt::Implicit(_) | Stmt::Use { .. } | Stmt::SourceLocation(_) => {
                        Some(s.clone())
                    }
                    _ => None,
                })
                .collect();
            let class_params: HashSet<_> = t.dummies.iter().map(|(_, n, _)| n.clone()).collect();
            *body=std::mem::take(body).into_iter().flat_map(|s| {
                if let Stmt::DerivedDecl{type_name,names,attributes,polymorphic,dims}=&s {
                    return names.iter().map(|n| {
                        let attrs=if class_params.contains(n){vars[n].attrs.clone()}else{attributes.clone()};
                        Stmt::DerivedDecl{type_name:type_name.clone(),names:vec![n.clone()],attributes:attrs.into_iter().map(|a|if a=="intent(out)"{"intent(inout)".into()}else{a}).collect(),polymorphic:*polymorphic,dims:dims.clone()}
                    }).collect::<Vec<_>>();
                }
                if matches!(&s,Stmt::DummyIntent{name,intent} if class_params.contains(name) && matches!(intent.as_str(),"optional"|"intent(in)"|"intent(out)"|"intent(inout)"|"target")){vec![]}else{vec![s]}
            }).collect();
            if let Some(result) = result {
                body.push(Stmt::Assign {
                    name: result.clone(),
                    value: Expr::Call(target, args),
                });
                body.push(Stmt::Return(Some(Expr::Ident(result))));
            } else {
                body.push(Stmt::CallSub { name: target, args });
            }
            self.wrappers.insert(name, wrapper);
        }
        Ok(())
    }
    fn install(&mut self, body: &mut Vec<Stmt>) {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = &s {
                if self.templates.contains_key(name) {
                    let name = name.clone();
                    // Concrete TYPE IS views can call ordinary existing bindings.
                    // Their original entry point is still named by the type table.
                    if let Some(wrapper) = self.wrappers.remove(&name) {
                        out.push(wrapper);
                    } else {
                        out.push(crate::imports::declaration(name.clone()));
                    }
                    for mut definition in self.generated.remove(&name).unwrap_or_default() {
                        for b in children(&mut definition) {
                            self.install(b);
                        }
                        out.push(definition);
                    }
                    continue;
                }
            }
            for b in children(&mut s) {
                self.install(b);
            }
            out.push(s);
        }
        *body = out;
    }
}
fn needed(body: &[Stmt]) -> bool {
    // Borrow source bodies here: cloning whole procedures on the fast path is
    // unnecessary overhead for programs that contain no SELECT TYPE at all.
    body.iter().any(|s| match s {
        Stmt::SelectCase {
            expr,
            cases,
            default,
        } => {
            selection(expr).is_some()
                || cases.iter().any(|c| needed(&c.body))
                || default.as_ref().is_some_and(|b| needed(b))
        }
        Stmt::Function { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Module { body, .. }
        | Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. } => needed(body),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => needed(then_body) || else_body.as_ref().is_some_and(|b| needed(b)),
        _ => false,
    })
}
fn bound_dispatch_needed(body: &[Stmt], inherited: &Objects, defs: &Definitions) -> bool {
    let vars = objects(body, inherited);
    for s in body {
        if let Stmt::CallSub { name, .. } = s {
            if let Some((object, method)) = name.rsplit_once('%') {
                if vars
                    .get(object)
                    .or_else(|| object.split_once('%').and_then(|(root, _)| vars.get(root)))
                    .is_some_and(|v| {
                        v.polymorphic && v.rank == 0 && defs.binding(&v.kind, method).is_some()
                    })
                {
                    return true;
                }
            }
        }
        let mut copy = s.clone();
        let mut found = false;
        for e in expressions(&mut copy) {
            visit_expr(e, &mut |e| {
                if let Expr::Call(name, _) = e {
                    if let Some((object, method)) = name.rsplit_once('%') {
                        if vars
                            .get(object)
                            .or_else(|| object.split_once('%').and_then(|(root, _)| vars.get(root)))
                            .is_some_and(|v| {
                                v.polymorphic
                                    && v.rank == 0
                                    && defs.binding(&v.kind, method).is_some()
                            })
                        {
                            found = true;
                        }
                    }
                }
            });
        }
        if found
            || children(&mut copy)
                .into_iter()
                .any(|b| bound_dispatch_needed(b, &vars, defs))
        {
            return true;
        }
    }
    false
}
pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut defs = Definitions::default();
    defs.collect(&program.body);
    if !needed(&program.body)
        && (defs.bound_targets.is_empty()
            || !bound_dispatch_needed(&program.body, &Objects::new(), &defs))
    {
        return Ok(program);
    }
    defs.validate(&program.body, &Objects::new())?;
    let mut templates = HashMap::new();
    collect(&program.body, &Objects::new(), &mut templates)?;
    fn check_generics(body: &[Stmt], templates: &HashMap<String, Template>) -> Result<()> {
        let mut location = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                location = Some(p);
            }
            if matches!(s,Stmt::GenericInterface{procedures,..} if procedures.iter().any(|n|templates.contains_key(n)))
            {
                return Err(crate::source_diagnostics::at(anyhow::anyhow!("generic interfaces containing polymorphic call specializations are not yet supported; call the specific procedure"),location));
            }
            let mut s = s.clone();
            for b in children(&mut s) {
                check_generics(b, templates)?;
            }
        }
        Ok(())
    }
    check_generics(&program.body, &templates)?;
    let mut lower = Lowerer {
        templates,
        defs,
        cache: HashMap::new(),
        extras: HashMap::new(),
        generated: HashMap::new(),
        wrappers: HashMap::new(),
        names: vec![],
        serial: 0,
    };
    lower.body(&mut program.body, &Objects::new(), &Dynamics::new())?;
    lower.binding_wrappers()?;
    let wrapper_entries: HashSet<_> = lower.wrappers.keys().cloned().collect();
    lower.install(&mut program.body);
    let original: HashSet<_> = program.contains.iter().cloned().collect();
    program
        .contains
        .retain(|n| !lower.templates.contains_key(n) || wrapper_entries.contains(n));
    program.contains.extend(
        lower
            .names
            .into_iter()
            .filter(|(n, _)| original.contains(n))
            .map(|(_, n)| n),
    );
    crate::block_scopes::resolve(program)
}

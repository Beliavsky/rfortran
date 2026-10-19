// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Keep enumeration identity in the existing nominal-type AST through module
//! association and generic resolution. Erase it only after checking uses.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

pub fn definition(
    name: String,
    access: Option<bool>,
    items: Vec<(usize, Vec<String>)>,
    end: Option<String>,
) -> std::result::Result<Vec<Stmt>, &'static str> {
    if end.as_ref().is_some_and(|n| n != &name) {
        return Err("END ENUMERATION TYPE name does not match its definition");
    }
    let count = items.iter().map(|(_, n)| n.len()).sum::<usize>();
    let mut seen = HashSet::from([name.clone()]);
    if items
        .iter()
        .flat_map(|(_, n)| n)
        .any(|n| !seen.insert(n.clone()))
    {
        return Err("enumeration type and enumerator names must be distinct");
    }
    let mut bindings = vec![("@enumeration".into(), format!("{count}:{}", items[0].0))];
    if let Some(access) = access {
        bindings.push(("@enumeration_access".into(), access.to_string()));
    }
    let mut out = vec![Stmt::DerivedType {
        name: name.clone(),
        components: vec![],
        bindings,
    }];
    if let Some(public) = access {
        out.push(Stmt::Accessibility {
            public,
            names: vec![name.clone()],
        });
    }
    let mut ordinal = 0;
    for (pos, names) in items {
        for enumerator in names {
            ordinal += 1;
            out.push(Stmt::SourceLocation(pos));
            if let Some(public) = access {
                out.push(Stmt::Accessibility {
                    public,
                    names: vec![enumerator.clone()],
                });
            }
            out.push(Stmt::DerivedDecl {
                type_name: name.clone(),
                names: vec![enumerator.clone()],
                attributes: vec!["parameter".into()],
                polymorphic: false,
                dims: vec![],
            });
            out.push(Stmt::Assign {
                name: enumerator,
                value: Expr::Call(name.clone(), vec![int(ordinal)]),
            });
        }
    }
    Ok(out)
}
fn marker(s: &Stmt) -> Option<(&String, &String)> {
    match s {
        Stmt::DerivedType { name, bindings, .. } => bindings
            .iter()
            .find(|(k, _)| k == "@enumeration")
            .map(|(_, v)| (name, v)),
        _ => None,
    }
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn section(e: &Expr) -> bool {
    matches!(e, Expr::Section { .. }) || matches!(e,Expr::Call(n,_) if n=="@section_subscript")
}
fn count(text: &str) -> usize {
    text.split(':').next().unwrap().parse().unwrap()
}

pub fn validate_definitions(p: &Program) -> Result<()> {
    fn walk(body: &[Stmt], module: bool) -> Result<()> {
        let enum_names: HashSet<_> = body
            .iter()
            .filter(|s| marker(s).is_some())
            .flat_map(|s| {
                let (name, _) = marker(s).unwrap();
                std::iter::once(name.clone())
                    .chain(
                        body.iter()
                            .filter_map(move |s| match s {
                                Stmt::DerivedDecl {
                                    type_name,
                                    names,
                                    attributes,
                                    ..
                                } if type_name == name
                                    && attributes.iter().any(|a| a == "parameter") =>
                                {
                                    Some(names.clone())
                                }
                                _ => None,
                            })
                            .flatten(),
                    )
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut seen = HashSet::new();
        let mut offset = None;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(*p);
                continue;
            }
            let result = (|| -> Result<()> {
                if marker(s).is_some()
                    && !module
                    && matches!(s,Stmt::DerivedType{bindings,..} if bindings.iter().any(|(n,_)|n=="@enumeration_access"))
                {
                    bail!("enumeration type accessibility is only permitted in a module");
                }
                let names = match s {
                    Stmt::DerivedType { name, .. }
                    | Stmt::ArrayDecl { name, .. }
                    | Stmt::Function { name, .. }
                    | Stmt::Subroutine { name, .. } => vec![name],
                    Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                        names.iter().collect()
                    }
                    Stmt::InitializedDecl { entities, .. } => {
                        entities.iter().map(|(n, _)| n).collect()
                    }
                    _ => vec![],
                };
                for n in names {
                    if enum_names.contains(n) && !seen.insert(n.clone()) {
                        bail!("duplicate declaration of enumeration name `{n}`");
                    }
                }
                let mut copy = s.clone();
                for b in children(&mut copy) {
                    walk(b, matches!(s, Stmt::Module { .. }))?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
    walk(&p.body, false)
}

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Enum(String, usize),
    Integer(usize),
    Derived(String, usize),
    Other,
    Unknown,
}
impl Ty {
    fn description(&self) -> String {
        fn name(n: &str) -> String {
            n.strip_prefix("@module$")
                .and_then(|s| s.split_once('$'))
                .map_or_else(|| n.to_string(), |(m, n)| format!("{m}::{n}"))
        }
        match self {
            Self::Enum(n, r) => format!("enumeration `{}` (rank {r})", name(n)),
            Self::Integer(r) => format!("INTEGER (rank {r})"),
            Self::Derived(n, r) => format!("TYPE({}) (rank {r})", name(n)),
            Self::Other => "a non-enumeration type".into(),
            Self::Unknown => "an unknown type".into(),
        }
    }
    fn enumeration(&self) -> Option<&str> {
        if let Self::Enum(n, _) = self {
            Some(n)
        } else {
            None
        }
    }
    fn rank(&self) -> usize {
        match self {
            Self::Enum(_, r) | Self::Integer(r) | Self::Derived(_, r) => *r,
            _ => 0,
        }
    }
    fn ranked(self, rank: usize) -> Self {
        match self {
            Self::Enum(n, _) => Self::Enum(n, rank),
            Self::Integer(_) => Self::Integer(rank),
            Self::Derived(n, _) => Self::Derived(n, rank),
            t => t,
        }
    }
}
#[derive(Clone)]
struct Scope {
    vars: HashMap<String, Ty>,
    kinds: HashMap<String, TypeSpec>,
    constants: HashMap<String, i64>,
    parameters: HashSet<String>,
    optional: HashSet<String>,
    readonly: HashSet<String>,
    implicit: [Option<bool>; 26],
}
impl Default for Scope {
    fn default() -> Self {
        Self {
            vars: HashMap::new(),
            kinds: HashMap::new(),
            constants: HashMap::new(),
            parameters: HashSet::new(),
            optional: HashSet::new(),
            readonly: HashSet::new(),
            implicit: std::array::from_fn(|i| Some((8..14).contains(&i))),
        }
    }
}
#[derive(Clone)]
struct Routine {
    params: Vec<String>,
    types: Vec<Ty>,
    result: Ty,
    elemental: bool,
}
struct Context {
    types: HashMap<String, String>,
    fields: HashMap<String, HashMap<String, Ty>>,
    field_order: HashMap<String, Vec<String>>,
    field_kinds: HashMap<String, HashMap<String, TypeSpec>>,
    procedures: HashMap<String, Routine>,
    generics: HashSet<String>,
    strict: bool,
    serial: usize,
    helpers: Vec<Stmt>,
    location: Option<usize>,
    globals: Scope,
    external: HashSet<(String, Option<usize>)>,
}
impl Context {
    fn object_kind(&self, n: &str, scope: &Scope) -> Option<TypeSpec> {
        if let Some(kind) = scope.kinds.get(n).or_else(|| self.globals.kinds.get(n)) {
            return Some(kind.clone());
        }
        if let Some((object, field)) = n.rsplit_once('%') {
            if let Ty::Derived(ty, _) = self.name(object, scope) {
                return self.field_kinds.get(&ty)?.get(field).cloned();
            }
        }
        None
    }
    fn stat_kind(&self, e: &Expr, scope: &Scope) -> Result<TypeSpec> {
        let n = match e {
            Expr::Ident(n) | Expr::Section { name: n, .. } => n,
            Expr::Call(n, args) | Expr::Index(n, args) if self.name(n, scope).rank() > 0 => {
                for a in args {
                    if self.expression(&mut a.clone(), scope, false)?.rank() > 0 && !section(a) {
                        bail!("NEXT/PREVIOUS STAT cannot have a vector subscript");
                    }
                }
                n
            }
            _ => bail!("NEXT/PREVIOUS STAT must be a writable INTEGER variable"),
        };
        if scope.parameters.contains(n) || self.globals.parameters.contains(n) {
            bail!("NEXT/PREVIOUS STAT must be writable, not a named constant");
        }
        let root = n.split('%').next().unwrap_or(n);
        if scope.readonly.contains(root) {
            bail!("NEXT/PREVIOUS STAT cannot be an INTENT(IN) variable");
        }
        let kind = match self
            .object_kind(n, scope)
            .unwrap_or(TypeSpec::Integer(None))
        {
            TypeSpec::IntegerKind(n) => {
                let selector = if let Some(encoded) = n.strip_prefix(crate::ast::KIND_EXPRESSION) {
                    serde_json::from_str(encoded)?
                } else {
                    Expr::Ident(n.clone())
                };
                let k = self
                    .constant(&selector, scope)
                    .or_else(|| n.parse().ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!("STAT INTEGER kind must be a supported constant")
                    })?;
                TypeSpec::Integer(Some(u8::try_from(k)?))
            }
            k => k,
        };
        Ok(kind)
    }
    fn step_helper(&mut self, operation: &str, ty: &str, stat: Option<TypeSpec>) -> String {
        self.serial += 1;
        let name = format!("@enumeration_step_{}", self.serial);
        let a = Expr::Ident("a".into());
        let boundary = if operation == "next" {
            count(&self.types[ty]) as i64
        } else {
            1
        };
        let with_stat = stat.is_some();
        let mut body = vec![
            crate::simple::marker(),
            Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: vec!["a".into(), "result".into()],
            },
            Stmt::DummyIntent {
                name: "a".into(),
                intent: "intent(in)".into(),
            },
        ];
        if let Some(p) = self.location {
            body.push(Stmt::SourceLocation(p));
        }
        let failed = if let Some(kind) = stat {
            body.push(Stmt::VarDecl {
                kind,
                names: vec!["stat".into()],
            });
            body.push(Stmt::DummyIntent {
                name: "stat".into(),
                intent: "intent(out)".into(),
            });
            body.push(Stmt::Assign {
                name: "stat".into(),
                value: int(0),
            });
            vec![Stmt::Assign {
                name: "stat".into(),
                value: int(1),
            }]
        } else {
            vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(format!(
                    "@enumeration_boundary:{}: no {} enumerator for `{}`",
                    operation.to_uppercase(),
                    operation,
                    crate::source_diagnostics::user_names(&format!("`{ty}`")).trim_matches('`')
                ))],
            }]
        };
        body.push(Stmt::Assign {
            name: "result".into(),
            value: a.clone(),
        });
        body.push(Stmt::If {
            cond: Expr::eq(a.clone(), int(boundary)),
            then_body: failed,
            else_body: Some(vec![Stmt::Assign {
                name: "result".into(),
                value: Expr::add(a, int(if operation == "next" { 1 } else { -1 })),
            }]),
        });
        body.push(Stmt::Return(Some(Expr::Ident("result".into()))));
        let params = if with_stat {
            vec!["a".into(), "stat".into()]
        } else {
            vec!["a".into()]
        };
        self.helpers.push(Stmt::Function {
            name: name.clone(),
            params,
            body,
            return_type: Some(TypeSpec::Integer(None)),
            pure: true,
            elemental: true,
        });
        name
    }
    fn external(&self, s: &Stmt) -> bool {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                self.external.contains(&(
                    name.clone(),
                    body.iter()
                        .find_map(crate::source_diagnostics::statement_offset),
                ))
            }
            _ => false,
        }
    }
    fn nominal(&self, n: &str, rank: usize) -> Ty {
        if self.types.contains_key(n) {
            Ty::Enum(n.into(), rank)
        } else {
            Ty::Derived(n.into(), rank)
        }
    }
    fn scalar(kind: &TypeSpec, rank: usize) -> Ty {
        if matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)) {
            Ty::Integer(rank)
        } else {
            Ty::Other
        }
    }
    fn scope(&self, body: &[Stmt], parent: &Scope) -> Scope {
        let mut scope = parent.clone();
        // A local declaration can shadow an OPTIONAL or read-only host object.
        for s in body {
            let names = match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
                Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                Stmt::InitializedDecl { entities, .. } => {
                    entities.iter().map(|(n, _)| n.clone()).collect()
                }
                _ => vec![],
            };
            for n in names {
                scope.optional.remove(&n);
                scope.readonly.remove(&n);
            }
        }
        for s in body {
            use crate::ast::Implicit;
            let rules = match s {
                Stmt::Implicit(Implicit::None) => {
                    scope.implicit = [None; 26];
                    continue;
                }
                Stmt::Implicit(Implicit::Rule {
                    type_spec,
                    letter_ranges,
                }) => vec![(type_spec, letter_ranges)],
                Stmt::Implicit(Implicit::Rules { rules }) => rules
                    .iter()
                    .map(|r| (&r.type_spec, &r.letter_ranges))
                    .collect(),
                _ => continue,
            };
            for (kind, ranges) in rules {
                let integer = matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_));
                for range in ranges {
                    for c in range.start..=range.end.unwrap_or(range.start) {
                        if c.is_ascii_alphabetic() {
                            scope.implicit[(c.to_ascii_lowercase() as u8 - b'a') as usize] =
                                Some(integer);
                        }
                    }
                }
            }
        }
        for s in body {
            match s {
                Stmt::DummyIntent { name, intent } if intent == "optional" => {
                    scope.optional.insert(name.clone());
                }
                Stmt::DummyIntent { name, intent } if intent == "intent(in)" => {
                    scope.readonly.insert(name.clone());
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        scope.kinds.insert(n.clone(), kind.clone());
                        scope.vars.insert(n.clone(), Self::scalar(kind, 0));
                        scope.constants.remove(n);
                        scope.parameters.remove(n);
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    scope.kinds.insert(name.clone(), kind.clone());
                    scope
                        .vars
                        .insert(name.clone(), Self::scalar(kind, dims.len()));
                }
                Stmt::InitializedDecl {
                    kind,
                    attributes,
                    entities,
                } => {
                    for (n, _) in entities {
                        scope.kinds.insert(n.clone(), kind.clone());
                        scope.vars.insert(n.clone(), Self::scalar(kind, 0));
                        scope.constants.remove(n);
                        if attributes.iter().any(|a| a == "parameter") {
                            scope.parameters.insert(n.clone());
                        }
                    }
                }
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    attributes,
                    ..
                } => {
                    for n in names {
                        scope
                            .vars
                            .insert(n.clone(), self.nominal(type_name, dims.len()));
                        scope.constants.remove(n);
                        scope.parameters.remove(n);
                        if attributes.iter().any(|a| a == "parameter") {
                            scope.parameters.insert(n.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        // Constants can depend on earlier constants, including imported
        // enumerators. Leave other constant-expression diagnostics to folding.
        for _ in 0..=scope.parameters.len() {
            let before = scope.constants.len();
            for s in body {
                match s {
                    Stmt::Assign { name, value } if scope.parameters.contains(name) => {
                        if let Some(v) = self.constant(value, &scope) {
                            scope.constants.insert(name.clone(), v);
                        }
                    }
                    Stmt::InitializedDecl { entities, .. } => {
                        for (n, e) in entities {
                            if scope.parameters.contains(n) {
                                if let Some(v) = e.as_ref().and_then(|e| self.constant(e, &scope)) {
                                    scope.constants.insert(n.clone(), v);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if before == scope.constants.len() {
                break;
            }
        }
        scope
    }
    fn name(&self, n: &str, scope: &Scope) -> Ty {
        if let Some(t) = scope.vars.get(n) {
            return t.clone();
        }
        if let Some(t) = self.globals.vars.get(n) {
            return t.clone();
        }
        if let Some((object, field)) = n.rsplit_once('%') {
            if let Ty::Derived(ty, rank) = self.name(object, scope) {
                if let Some(t) = self.fields.get(&ty).and_then(|f| f.get(field)) {
                    return t.clone().ranked(rank.max(t.rank()));
                }
            }
        }
        match n
            .bytes()
            .next()
            .filter(u8::is_ascii_lowercase)
            .and_then(|c| scope.implicit[(c - b'a') as usize])
        {
            Some(true) => Ty::Integer(0),
            Some(false) => Ty::Other,
            None => Ty::Unknown,
        }
    }
    fn constant(&self, e: &Expr, scope: &Scope) -> Option<i64> {
        if let Expr::Call(n, args) = e {
            if matches!(n.as_str(), "next" | "previous")
                && args.len() == 1
                && !self.procedures.contains_key(n)
                && !self.generics.contains(n)
            {
                let ty = self.expression(&mut args[0].clone(), scope, false).ok()?;
                let ty = ty.enumeration()?;
                let value = self.constant(&args[0], scope)?;
                let value = value + if n == "next" { 1 } else { -1 };
                return (value >= 1 && value <= count(&self.types[ty]) as i64).then_some(value);
            }
        }
        let mut e = e.clone();
        visit_expr(&mut e, &mut |e| match e {
            Expr::Ident(n)
                if scope.constants.contains_key(n) || self.globals.constants.contains_key(n) =>
            {
                *e = int(*scope
                    .constants
                    .get(n)
                    .or_else(|| self.globals.constants.get(n))
                    .unwrap())
            }
            Expr::Call(n, a)
                if (self.types.contains_key(n)
                    || n == "@enumeration_ordinal"
                    || n == "int" && !self.procedures.contains_key(n))
                    && a.len() == 1 =>
            {
                if let Expr::IntLit(_) = &a[0] {
                    *e = a[0].clone();
                }
            }
            Expr::Call(n, a) if n == "huge" && a.len() == 1 && !self.procedures.contains_key(n) => {
                if let Ok(Some(TypeSpec::Integer(k))) =
                    crate::inquiries::model_type(&a[0], &|_| None)
                {
                    if let Ok(value) = crate::inquiries::integer_huge(k) {
                        *e = int(value);
                    }
                }
            }
            _ => {}
        });
        crate::parameters::integer_constant(&e).ok()
    }
    fn collect(&mut self, body: &[Stmt], parent: &Scope) -> Result<()> {
        let scope = self.scope(body, parent);
        for s in body {
            let empty = Scope::default();
            let external = self.external(s);
            match s {
                Stmt::DerivedType {
                    name, components, ..
                } if marker(s).is_none() => {
                    self.field_kinds.insert(
                        name.clone(),
                        self.scope(components, &Scope::default()).kinds,
                    );
                    self.fields
                        .insert(name.clone(), self.scope(components, &Scope::default()).vars);
                    let mut order = vec![];
                    for component in components {
                        let names = match component {
                            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                                names.clone()
                            }
                            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                            Stmt::InitializedDecl { entities, .. } => {
                                entities.iter().map(|(n, _)| n.clone()).collect()
                            }
                            _ => vec![],
                        };
                        for n in names {
                            if !order.contains(&n) {
                                order.push(n);
                            }
                        }
                    }
                    self.field_order.insert(name.clone(), order);
                }
                Stmt::GenericInterface { name, .. } => {
                    self.generics.insert(name.clone());
                }
                Stmt::Function {
                    name,
                    params,
                    body,
                    return_type,
                    elemental,
                    ..
                } => {
                    let inner = self.scope(body, if external { &empty } else { &scope });
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                        _ => name,
                    };
                    self.procedures.insert(
                        name.clone(),
                        Routine {
                            params: params.clone(),
                            types: params.iter().map(|p| self.name(p, &inner)).collect(),
                            result: inner
                                .vars
                                .get(result)
                                .cloned()
                                .or_else(|| return_type.as_ref().map(|k| Self::scalar(k, 0)))
                                .unwrap_or(Ty::Unknown),
                            elemental: *elemental,
                        },
                    );
                }
                Stmt::Subroutine {
                    name,
                    params,
                    body,
                    elemental,
                    ..
                } => {
                    let inner = self.scope(body, if external { &empty } else { &scope });
                    self.procedures.insert(
                        name.clone(),
                        Routine {
                            params: params.clone(),
                            types: params.iter().map(|p| self.name(p, &inner)).collect(),
                            result: Ty::Unknown,
                            elemental: *elemental,
                        },
                    );
                }
                _ => {}
            }
            let mut s = s.clone();
            let module = matches!(s, Stmt::Module { .. });
            let empty = Scope::default();
            for b in children(&mut s) {
                self.collect(b, if module || external { &empty } else { &scope })?;
            }
        }
        Ok(())
    }
    fn check_call(&self, n: &str, args: &[Expr], types: &[Ty]) -> Result<Ty> {
        if let Some(p) = self.procedures.get(n) {
            for (i, actual) in types.iter().enumerate() {
                let position = crate::named_arguments::keyword(&args[i])
                    .and_then(|(key, _)| p.params.iter().position(|n| n == key))
                    .unwrap_or(i);
                if let Some(expected) = p.types.get(position) {
                    if actual.enumeration().is_some() || expected.enumeration().is_some() {
                        if actual.enumeration() != expected.enumeration()
                            && (self.strict || *actual != Ty::Unknown)
                        {
                            bail!("enumeration type mismatch in argument to `{n}`: expected {}, got {}",expected.description(),actual.description());
                        }
                        if actual.rank() != expected.rank()
                            && !(p.elemental && expected.rank() == 0)
                        {
                            bail!("enumeration argument rank mismatch in call to `{n}`");
                        }
                    }
                }
            }
            return Ok(if p.elemental {
                p.result
                    .clone()
                    .ranked(types.iter().map(Ty::rank).max().unwrap_or(0))
            } else {
                p.result.clone()
            });
        }
        if types.iter().any(|t| t.enumeration().is_some())
            && !self.generics.contains(n)
            && !n.starts_with('@')
        {
            bail!("enumeration argument to `{n}` requires a supported intrinsic or an explicit procedure interface");
        }
        Ok(Ty::Unknown)
    }
    fn expression(&self, e: &mut Expr, scope: &Scope, prepare: bool) -> Result<Ty> {
        if let Expr::Call(n, args) = e {
            if n == "@enumeration_reference" && args.len() == 3 {
                self.expression(&mut args[2], scope, false)?;
                if let (Expr::Str(ty), Expr::IntLit(rank)) = (&args[0], &args[1]) {
                    return Ok(Ty::Enum(ty.clone(), rank.parse().unwrap()));
                }
            }
        }
        let ty = match e {
            Expr::Ident(n) => {
                let ty = self.name(n, scope);
                if prepare {
                    if let (Some(t), Some(value)) = (
                        ty.enumeration(),
                        scope
                            .constants
                            .get(n)
                            .or_else(|| self.globals.constants.get(n)),
                    ) {
                        *e = Expr::Call(t.to_string(), vec![int(*value)]);
                    }
                }
                ty
            }
            Expr::IntLit(_) => Ty::Integer(0),
            Expr::RealLit(_) | Expr::Str(_) | Expr::Logical(_) => Ty::Other,
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    if self.expression(e, scope, prepare)?.enumeration().is_some() {
                        bail!("section bounds and stride must be INTEGER, not enumeration values");
                    }
                }
                self.name(name, scope).ranked(1)
            }
            Expr::Bin(op, a, b) => {
                let left = self.expression(a, scope, prepare)?;
                let right = self.expression(b, scope, prepare)?;
                if left.enumeration().is_some() || right.enumeration().is_some() {
                    if !self.strict && (left == Ty::Unknown || right == Ty::Unknown) {
                        return Ok(Ty::Other);
                    }
                    if left.enumeration() != right.enumeration() {
                        bail!("comparison operands must have the same enumeration type");
                    }
                    if !matches!(
                        op,
                        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                    ) {
                        bail!("intrinsic arithmetic and logical operations are not defined for enumeration values");
                    }
                    if prepare {
                        **a = Expr::Call("@enumeration_ordinal".into(), vec![(**a).clone()]);
                        **b = Expr::Call("@enumeration_ordinal".into(), vec![(**b).clone()]);
                    }
                    Ty::Other
                } else {
                    if matches!(
                        op,
                        BinOp::Eq
                            | BinOp::Ne
                            | BinOp::Lt
                            | BinOp::Le
                            | BinOp::Gt
                            | BinOp::Ge
                            | BinOp::And
                            | BinOp::Or
                            | BinOp::Eqv
                            | BinOp::Neqv
                    ) {
                        Ty::Other
                    } else if matches!(left, Ty::Integer(_)) && matches!(right, Ty::Integer(_)) {
                        Ty::Integer(left.rank().max(right.rank()))
                    } else {
                        Ty::Other
                    }
                }
            }
            Expr::Un(_, a) => {
                let ty = self.expression(a, scope, prepare)?;
                if ty.enumeration().is_some() {
                    bail!("intrinsic unary operations are not defined for enumeration values");
                }
                ty
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if n == "@conditional" {
                    let mut result = Ty::Unknown;
                    let length = args.len();
                    for (i, arg) in args.iter_mut().enumerate() {
                        let ty = self.expression(arg, scope, prepare)?;
                        if i % 2 == 0 && i + 1 < length {
                            if ty.enumeration().is_some() {
                                bail!("conditional condition must be LOGICAL, not an enumeration value");
                            }
                        } else if !crate::conditionals::nil(arg) {
                            if result != Ty::Unknown {
                                self.compatible(&result, &ty)?;
                            }
                            if result == Ty::Unknown {
                                result = ty;
                            }
                        }
                    }
                    return Ok(result);
                }
                // The first argument is a type name, not a data expression.
                if n == "@record_constructor" {
                    if let Some(Expr::Ident(name)) = args.first() {
                        if self.types.contains_key(name) {
                            let name = name.clone();
                            for item in &mut args[1..] {
                                let actual = self.expression(item, scope, prepare)?;
                                if actual.enumeration() != Some(name.as_str())
                                    && (self.strict || actual != Ty::Unknown)
                                {
                                    bail!("typed array constructor requires elements of enumeration type `{name}`, got {}", actual.description());
                                }
                                if actual.rank() > 1 {
                                    bail!("array constructor elements must be scalar or rank one");
                                }
                            }
                            return Ok(Ty::Enum(name, 1));
                        }
                    }
                }
                if n == "@output_implied_do" && args.len() >= 5 {
                    let mut inner = scope.clone();
                    if let Expr::Ident(index) = &args[0] {
                        inner.vars.insert(index.clone(), Ty::Integer(0));
                    }
                    for e in &mut args[1..4] {
                        if self.expression(e, scope, prepare)?.enumeration().is_some() {
                            bail!("implied-DO bounds must be INTEGER, not enumeration values");
                        }
                    }
                    let mut result = Ty::Unknown;
                    for e in &mut args[4..] {
                        let t = self.expression(e, &inner, prepare)?;
                        if result.enumeration().is_some() || t.enumeration().is_some() {
                            if result != Ty::Unknown && result.enumeration() != t.enumeration() {
                                bail!("array constructor elements must have the same enumeration type");
                            }
                        }
                        result = t;
                    }
                    return Ok(result.ranked(1));
                }
                let array = self.name(n, scope);
                let types = args
                    .iter_mut()
                    .map(|a| self.expression(a, scope, prepare && array.rank() == 0))
                    .collect::<Result<Vec<_>>>()?;
                if array.rank() > 0 {
                    if types
                        .iter()
                        .zip(args.iter())
                        .any(|(t, a)| t.enumeration().is_some() && !section(a))
                    {
                        bail!("array subscripts must be INTEGER, not enumeration values");
                    }
                    let rank = types
                        .iter()
                        .zip(args.iter())
                        .filter(|(t, a)| t.rank() > 0 || section(a))
                        .count();
                    array.ranked(rank)
                } else if self.types.contains_key(n) {
                    if !self.strict && types.len() == 1 && types[0] == Ty::Unknown {
                        return Ok(Ty::Enum(n.clone(), 0));
                    }
                    if types.len() != 1 || types[0] != Ty::Integer(0) {
                        bail!("enumeration constructor `{n}` requires one scalar INTEGER ordinal");
                    }
                    if let Some(v) = self.constant(&args[0], scope) {
                        if v < 1 || v > count(&self.types[n]) as i64 {
                            bail!(
                                "enumeration constructor `{n}` ordinal {v} is outside 1..{}",
                                count(&self.types[n])
                            );
                        }
                    }
                    Ty::Enum(n.clone(), 0)
                } else if n == "@declared_bounds" {
                    if types.iter().any(|t| t.enumeration().is_some()) {
                        bail!("array bounds must be INTEGER, not enumeration values");
                    }
                    Ty::Integer(0)
                } else if n.starts_with("@keyword:") {
                    types.first().cloned().unwrap_or(Ty::Unknown)
                } else if self.procedures.contains_key(n) || self.generics.contains(n) {
                    self.check_call(n, args, &types)?
                } else if let Some(fields) = self.fields.get(n) {
                    let order = &self.field_order[n];
                    for (i, (arg, actual)) in args.iter().zip(&types).enumerate() {
                        if crate::named_arguments::omitted(arg) {
                            continue;
                        }
                        let field = crate::named_arguments::keyword(arg)
                            .map(|(key, _)| key)
                            .or_else(|| order.get(i).map(String::as_str));
                        if let Some((field, expected)) =
                            field.and_then(|f| fields.get(f).map(|t| (f, t)))
                        {
                            self.compatible(expected, actual).map_err(|e| {
                                anyhow::anyhow!("constructor `{n}` component `{field}`: {e}")
                            })?;
                        }
                    }
                    Ty::Derived(n.clone(), 0)
                } else if n == "@array_constructor" {
                    let enums = types.iter().filter_map(Ty::enumeration).collect::<Vec<_>>();
                    if let Some(first) = enums.first() {
                        if types.iter().any(|t| {
                            t.enumeration() != Some(*first) && (self.strict || *t != Ty::Unknown)
                        }) {
                            bail!("array constructor elements must have the same enumeration type");
                        }
                        Ty::Enum(first.to_string(), 1)
                    } else {
                        types.first().cloned().unwrap_or(Ty::Unknown).ranked(1)
                    }
                } else if n == "@section_subscript" {
                    types.first().cloned().unwrap_or(Ty::Unknown)
                } else if n == "int" {
                    for (i, (a, t)) in args.iter().zip(&types).enumerate() {
                        let kind = crate::named_arguments::keyword(a)
                            .map_or(i == 1, |(key, _)| key == "kind");
                        if kind && t.enumeration().is_some() {
                            bail!("INT KIND must be a scalar INTEGER, not an enumeration value");
                        }
                    }
                    Ty::Integer(types.iter().map(Ty::rank).max().unwrap_or(0))
                } else if n == "@enumeration_ordinal" {
                    Ty::Integer(types.first().map(Ty::rank).unwrap_or(0))
                } else if matches!(
                    n.as_str(),
                    "huge" | "mod" | "modulo" | "min" | "max" | "abs" | "sign"
                ) && types.iter().all(|t| matches!(t, Ty::Integer(_)))
                {
                    Ty::Integer(types.iter().map(Ty::rank).max().unwrap_or(0))
                } else if n == "huge" && types.first().and_then(Ty::enumeration).is_some() {
                    if args.len() != 1 {
                        bail!("HUGE expects one argument");
                    }
                    let ty = types[0].enumeration().unwrap().to_string();
                    if prepare {
                        *e = Expr::Call(ty.clone(), vec![int(count(&self.types[&ty]) as i64)]);
                    }
                    Ty::Enum(ty, 0)
                } else if matches!(n.as_str(), "next" | "previous") {
                    if !(1..=2).contains(&types.len()) {
                        bail!("{n} expects A and optional STAT");
                    }
                    if types[0].enumeration().is_none() && (self.strict || types[0] != Ty::Unknown)
                    {
                        bail!("{n} A must have enumeration type");
                    }
                    if let Some(stat) = types.get(1) {
                        if !matches!(stat, Ty::Integer(_)) {
                            bail!("{n} STAT must be an INTEGER variable");
                        }
                        let kind = self.stat_kind(&args[1], scope)?;
                        if matches!(kind, TypeSpec::Integer(Some(1))) {
                            bail!("{n} STAT must have a decimal exponent range of at least four");
                        }
                        if types[0].rank() > 0 && stat.rank() != types[0].rank() {
                            bail!("{n} STAT must be a conformable array when A is an array");
                        }
                    }
                    types[0]
                        .clone()
                        .ranked(types.iter().map(Ty::rank).max().unwrap_or(0))
                } else if n == "merge" && types.iter().take(2).any(|t| t.enumeration().is_some()) {
                    if types.get(2).is_some_and(|t| t.enumeration().is_some()) {
                        bail!("MERGE MASK must be LOGICAL, not an enumeration value");
                    }
                    if types.len() != 3 || types[0].enumeration() != types[1].enumeration() {
                        bail!("MERGE sources must have the same enumeration type");
                    }
                    types[0]
                        .clone()
                        .ranked(types.iter().map(Ty::rank).max().unwrap_or(0))
                } else if matches!(
                    n.as_str(),
                    "size" | "shape" | "lbound" | "ubound" | "rank" | "storage_size"
                ) {
                    for (i, (a, t)) in args.iter().zip(&types).enumerate() {
                        let source = crate::named_arguments::keyword(a)
                            .map_or(i == 0, |(key, _)| matches!(key, "a" | "source" | "array"));
                        if !source && t.enumeration().is_some() {
                            bail!("{n}: DIM and KIND arguments must be INTEGER, not enumeration values");
                        }
                    }
                    Ty::Integer(usize::from(n == "shape"))
                } else if matches!(n.as_str(), "allocated" | "associated" | "present") {
                    Ty::Other
                } else {
                    self.check_call(n, args, &types)?
                }
            }
        };
        if prepare {
            if let Some(name) = ty.enumeration() {
                let variable = match e {
                    Expr::Ident(_) | Expr::Section { .. } => true,
                    Expr::Call(n, _) | Expr::Index(n, _) => self.name(n, scope).rank() > 0,
                    _ => false,
                };
                if variable {
                    *e = Expr::Call(
                        "@enumeration_reference".into(),
                        vec![
                            Expr::Str(name.to_string()),
                            int(ty.rank() as i64),
                            e.clone(),
                        ],
                    );
                }
            }
        }
        Ok(ty)
    }
    fn compatible(&self, target: &Ty, value: &Ty) -> Result<()> {
        if target.enumeration().is_some() || value.enumeration().is_some() {
            if target.enumeration() != value.enumeration() && (self.strict || *value != Ty::Unknown)
            {
                bail!(
                    "assignment requires the same enumeration type on both sides: {} and {}",
                    target.description(),
                    value.description()
                );
            }
            if value.rank() != 0 && value.rank() != target.rank() {
                bail!("enumeration assignment ranks do not conform");
            }
        }
        Ok(())
    }
    fn validate_body(
        &self,
        body: &mut [Stmt],
        parent: &Scope,
        prepare: bool,
        mut offset: Option<usize>,
    ) -> Result<()> {
        let scope = self.scope(body, parent);
        let mut initialized = HashSet::new();
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(*p);
                continue;
            }
            let result = (|| -> Result<()> {
                if let Stmt::VarDecl { kind, .. }
                | Stmt::ArrayDecl { kind, .. }
                | Stmt::InitializedDecl { kind, .. } = s
                {
                    match kind {
                        TypeSpec::IntegerKind(n)
                        | TypeSpec::RealKind(n)
                        | TypeSpec::LogicalKind(n)
                        | TypeSpec::Complex(n) => {
                            let ty = if let Some(encoded) =
                                n.strip_prefix(crate::ast::KIND_EXPRESSION)
                            {
                                let mut expr: Expr = serde_json::from_str(encoded)?;
                                self.expression(&mut expr, &scope, false)?
                            } else {
                                self.name(n, &scope)
                            };
                            if ty.enumeration().is_some() {
                                bail!("kind selector must be INTEGER, not an enumeration value");
                            }
                        }
                        TypeSpec::CharacterLength(e) => {
                            if self
                                .expression(&mut e.clone(), &scope, false)?
                                .enumeration()
                                .is_some()
                            {
                                bail!("CHARACTER length must be INTEGER, not an enumeration value");
                            }
                        }
                        _ => {}
                    }
                }
                if marker(s).is_some()
                    && matches!(s,Stmt::DerivedType{components,..} if components.iter().any(|s|matches!(s,Stmt::DummyIntent{name,..} if name==crate::extensions::EXTENDS)))
                {
                    bail!("enumeration types cannot be extended");
                }
                if let Stmt::DerivedDecl {
                    type_name,
                    polymorphic,
                    attributes,
                    ..
                } = s
                {
                    if self.types.contains_key(type_name) {
                        if *polymorphic {
                            bail!("enumeration objects cannot be polymorphic");
                        }
                        if attributes
                            .iter()
                            .any(|a| matches!(a.as_str(), "pointer" | "allocatable"))
                        {
                            bail!(
                                "POINTER and ALLOCATABLE enumeration objects are not yet supported"
                            );
                        }
                    }
                }
                match s {
                    Stmt::ArrayDecl { dims, .. } | Stmt::DerivedDecl { dims, .. } => {
                        for e in dims {
                            if self.expression(e, &scope, false)?.enumeration().is_some() {
                                bail!("array bounds must be INTEGER, not enumeration values");
                            }
                        }
                    }
                    Stmt::InitializedDecl { entities, .. } => {
                        for (name, value) in entities {
                            if let Some(value) = value {
                                let target = self.name(name, &scope);
                                let ty = self.expression(value, &scope, prepare)?;
                                self.compatible(&target, &ty)?;
                            }
                        }
                    }
                    Stmt::Assign { name, value } => {
                        let t = self.name(name, &scope);
                        let v = self.expression(value, &scope, prepare)?;
                        self.compatible(&t, &v)?;
                        if scope.parameters.contains(name)
                            && t.enumeration().is_some()
                            && !initialized.insert(name.clone())
                        {
                            bail!("cannot assign to enumeration named constant `{name}`");
                        }
                    }
                    Stmt::AssignIndex {
                        name,
                        indices,
                        value,
                    } => {
                        let rank = indices.iter().filter(|e| section(e)).count();
                        let t = self.name(name, &scope).ranked(rank);
                        let v = self.expression(value, &scope, prepare)?;
                        self.compatible(&t, &v)?;
                        for e in indices {
                            if self.expression(e, &scope, false)?.enumeration().is_some()
                                && !section(e)
                            {
                                bail!("array subscripts must be INTEGER, not enumeration values");
                            }
                        }
                        if scope.parameters.contains(name) && t.enumeration().is_some() {
                            bail!("cannot assign to enumeration named constant `{name}`");
                        }
                    }
                    Stmt::SelectCase { expr, cases, .. } => {
                        let t = self.expression(expr, &scope, prepare)?;
                        for case in cases {
                            for item in &mut case.items {
                                let values = match item {
                                    CaseItem::Single(e) => vec![e],
                                    CaseItem::Range(a, b) => vec![a, b],
                                };
                                for e in values {
                                    if matches!(e,Expr::Call(n,_) if n=="@case_unbounded") {
                                        continue;
                                    }
                                    let v = self.expression(e, &scope, prepare)?;
                                    self.compatible(&t, &v)?;
                                }
                            }
                        }
                    }
                    Stmt::CallSub { name, args } => {
                        let types = args
                            .iter_mut()
                            .map(|e| self.expression(e, &scope, prepare))
                            .collect::<Result<Vec<_>>>()?;
                        if types.iter().any(|t| t.enumeration().is_some())
                            && name.starts_with('@')
                            && !self.procedures.contains_key(name)
                            && !self.generics.contains(name)
                        {
                            if matches!(
                                name.as_str(),
                                "@formatted_print" | "@write" | "@formatted_write_noadvance"
                            ) || name.starts_with("@read")
                                || name.starts_with("@io_")
                            {
                                bail!("direct input/output of enumeration values is not yet supported; use INT for output");
                            }
                            bail!("construct `{name}` does not yet support enumeration operands");
                        }
                        self.check_call(name, args, &types)?;
                    }
                    Stmt::Print { items } | Stmt::Read { args: items } => {
                        for e in items {
                            if self.expression(e, &scope, prepare)?.enumeration().is_some() {
                                bail!("list-directed input/output is not permitted for enumeration values; use INT for output");
                            }
                        }
                    }
                    // The final Return(Ident(result)) also records the named
                    // result for later ABI selection; keep that designator.
                    Stmt::Return(Some(e)) => {
                        self.expression(e, &scope, false)?;
                    }
                    Stmt::Do {
                        var, start, end, ..
                    } => {
                        if self.name(var, &scope).enumeration().is_some() {
                            bail!("enumeration variable cannot be a DO index");
                        }
                        for e in [start, end] {
                            if self.expression(e, &scope, prepare)?.enumeration().is_some() {
                                bail!("DO bounds must be INTEGER, not enumeration values");
                            }
                        }
                    }
                    Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => {
                        if self
                            .expression(cond, &scope, prepare)?
                            .enumeration()
                            .is_some()
                        {
                            bail!("condition must be LOGICAL, not an enumeration value");
                        }
                    }
                    Stmt::DerivedType { .. }
                    | Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. } => {}
                    _ => {
                        for e in expressions(s) {
                            self.expression(e, &scope, prepare)?;
                        }
                    }
                }
                let module = matches!(s, Stmt::Module { .. });
                let external = self.external(s);
                let empty = Scope::default();
                if let Stmt::DerivedType { components, .. } = s {
                    self.validate_body(components, &scope, prepare, offset)?;
                }
                for b in children(s) {
                    self.validate_body(
                        b,
                        if module || external { &empty } else { &scope },
                        prepare,
                        offset,
                    )?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
    fn erase_expr(&mut self, e: &mut Expr, scope: &Scope) -> Result<()> {
        if let Expr::Call(n, args) = e {
            if matches!(n.as_str(), "next" | "previous")
                && !self.procedures.contains_key(n)
                && !self.generics.contains(n)
            {
                let ty = self.expression(&mut args[0].clone(), scope, false)?;
                if let Some(ty) = ty.enumeration() {
                    if args.len() == 1 {
                        if let Some(value) =
                            self.constant(&Expr::Call(n.clone(), args.clone()), scope)
                        {
                            *e = int(value);
                            return Ok(());
                        }
                    }
                    let stat_kind = args.get(1).map(|e| self.stat_kind(e, scope)).transpose()?;
                    let helper = self.step_helper(n, ty, stat_kind);
                    let absent = if let Some(Expr::Ident(stat)) = args.get(1) {
                        if scope.optional.contains(stat) {
                            Some((stat.clone(), self.step_helper(n, ty, None)))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    for a in args.iter_mut() {
                        self.erase_expr(a, scope)?;
                    }
                    if let Some((stat, absent)) = absent {
                        *e = Expr::Call(
                            "@conditional".into(),
                            vec![
                                Expr::Call("present".into(), vec![Expr::Ident(stat)]),
                                Expr::Call(helper, args.clone()),
                                Expr::Call(absent, vec![args[0].clone()]),
                            ],
                        );
                        return Ok(());
                    }
                    *n = helper;
                    return Ok(());
                }
            }
        }
        if let Expr::Call(n, args) = e {
            if n == "@record_constructor"
                && matches!(args.first(), Some(Expr::Ident(t)) if self.types.contains_key(t))
            {
                let mut items = args.split_off(1);
                for item in &mut items {
                    self.erase_expr(item, scope)?;
                }
                *e = if items.is_empty() {
                    crate::empty_constructors::parsed(TypeSpec::Integer(None), items)
                } else {
                    Expr::Call("@array_constructor".into(), items)
                };
                return Ok(());
            }
            if n == "@enumeration_ordinal" && args.len() == 1 {
                *e = args.pop().unwrap();
                return self.erase_expr(e, scope);
            }
            if n == "@enumeration_reference" && args.len() == 3 {
                *e = args.pop().unwrap();
                return self.erase_expr(e, scope);
            }
        }
        let original = e.clone();
        if let Some(value) = self.constant(&original, scope) {
            if matches!(&original,Expr::Call(n,_) if self.types.contains_key(n)) {
                *e = int(value);
                return Ok(());
            }
        }
        match e {
            Expr::Call(n, args) | Expr::Index(n, args) => {
                for a in args.iter_mut() {
                    self.erase_expr(a, scope)?;
                }
                if self.types.contains_key(n) {
                    self.serial += 1;
                    let helper = format!("@enum_constructor_{}", self.serial);
                    let size = count(&self.types[n]);
                    let message = format!(
                        "enumeration constructor `{}` ordinal outside 1..{size}",
                        crate::source_diagnostics::user_names(&format!("`{n}`")).trim_matches('`')
                    );
                    self.helpers.push(Stmt::Function {
                        name: helper.clone(),
                        params: vec!["ordinal".into()],
                        return_type: Some(TypeSpec::Integer(None)),
                        pure: true,
                        elemental: false,
                        body: vec![
                            Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: vec!["ordinal".into()],
                            },
                            Stmt::DummyIntent {
                                name: "ordinal".into(),
                                intent: "intent(in)".into(),
                            },
                            Stmt::If {
                                cond: Expr::or(
                                    Expr::lt(Expr::Ident("ordinal".into()), int(1)),
                                    Expr::gt(Expr::Ident("ordinal".into()), int(size as i64)),
                                ),
                                then_body: vec![Stmt::CallSub {
                                    name: "@error_stop".into(),
                                    args: vec![Expr::Str(message)],
                                }],
                                else_body: None,
                            },
                            Stmt::Return(Some(Expr::Ident("ordinal".into()))),
                        ],
                    });
                    *n = helper;
                }
            }
            Expr::Bin(_, a, b) => {
                self.erase_expr(a, scope)?;
                self.erase_expr(b, scope)?;
            }
            Expr::Un(_, a) => self.erase_expr(a, scope)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.erase_expr(a, scope)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn erase_body(&mut self, body: &mut Vec<Stmt>, parent: &Scope, in_module: bool) -> Result<()> {
        let scope = self.scope(body, parent);
        let mut initializers = HashMap::new();
        let mut skip = HashSet::new();
        for (i, s) in body.iter().enumerate() {
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                ..
            } = s
            {
                if self.types.contains_key(type_name)
                    && attributes.iter().any(|a| a == "parameter" || a == "save")
                {
                    if let Some(Stmt::Assign { name, value }) = body.get(i + 1) {
                        if names.contains(name) {
                            initializers.insert(name.clone(), value.clone());
                            skip.insert(i + 1);
                        }
                    }
                }
            }
        }
        let mut out = vec![];
        for (index, mut s) in std::mem::take(body).into_iter().enumerate() {
            if let Stmt::SourceLocation(offset) = &s {
                self.location = Some(*offset);
            }
            let external = self.external(&s);
            if marker(&s).is_some() {
                continue;
            }
            if skip.contains(&index) {
                continue;
            }
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                dims,
                ..
            } = &s
            {
                if self.types.contains_key(type_name) {
                    for n in names {
                        for attr in attributes {
                            if attr.starts_with("intent(")
                                || matches!(attr.as_str(), "optional" | "target" | "value")
                            {
                                out.push(Stmt::DummyIntent {
                                    name: n.clone(),
                                    intent: attr.clone(),
                                });
                            }
                        }
                        if initializers.contains_key(n) {
                            let mut value = initializers[n].clone();
                            self.erase_expr(&mut value, &scope)?;
                            let mut attrs: Vec<_> = attributes
                                .iter()
                                .filter(|a| {
                                    !(attributes.iter().any(|a| a == "parameter")
                                        && a.as_str() == "save")
                                })
                                .cloned()
                                .collect();
                            if in_module && !attributes.iter().any(|a| a == "parameter") {
                                attrs.push("@module_initializer".into());
                            }
                            out.push(Stmt::InitializedDecl {
                                kind: TypeSpec::Integer(None),
                                attributes: attrs,
                                entities: vec![(n.clone(), Some(value))],
                            });
                        } else if dims.is_empty() {
                            out.push(Stmt::VarDecl {
                                kind: TypeSpec::Integer(None),
                                names: vec![n.clone()],
                            });
                        }
                        if !dims.is_empty() {
                            out.push(Stmt::ArrayDecl {
                                kind: TypeSpec::Integer(None),
                                name: n.clone(),
                                dims: dims.clone(),
                            });
                        }
                    }
                    continue;
                }
            }
            if let Stmt::Function {
                name,
                body,
                return_type,
                ..
            } = &mut s
            {
                let empty = Scope::default();
                let inner = self.scope(body, if external { &empty } else { &scope });
                let result = match body.last() {
                    Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                    _ => name,
                };
                if self.name(result, &inner).enumeration().is_some() {
                    *return_type = Some(TypeSpec::Integer(None));
                }
            }
            for e in expressions(&mut s) {
                self.erase_expr(e, &scope)?;
            }
            let module = matches!(s, Stmt::Module { .. });
            let empty = Scope::default();
            if let Stmt::DerivedType { components, .. } = &mut s {
                // SAVE is parser metadata for a derived component default,
                // not an attribute on the lowered intrinsic component.
                let defaults: HashSet<String> = components
                    .iter()
                    .filter_map(|c| {
                        if let Stmt::DerivedDecl {
                            type_name,
                            names,
                            attributes,
                            ..
                        } = c
                        {
                            if self.types.contains_key(type_name)
                                && attributes.iter().any(|a| a == "save")
                            {
                                return Some(names.clone());
                            }
                        }
                        None
                    })
                    .flatten()
                    .collect();
                self.erase_body(components, &scope, false)?;
                for c in components {
                    if let Stmt::InitializedDecl {
                        attributes,
                        entities,
                        ..
                    } = c
                    {
                        if entities.iter().all(|(n, _)| defaults.contains(n)) {
                            attributes.retain(|a| a != "save");
                        }
                    }
                }
            }
            for b in children(&mut s) {
                self.erase_body(b, if module || external { &empty } else { &scope }, module)?;
            }
            out.push(s);
        }
        *body = out;
        Ok(())
    }
}
fn context(p: &Program, strict: bool) -> Result<Context> {
    fn definitions(body: &[Stmt], types: &mut HashMap<String, String>) -> Result<()> {
        for s in body {
            if let Some((name, tag)) = marker(s) {
                if types.get(name).is_some_and(|v| v != tag) {
                    bail!("distinct local enumeration definitions with the same name `{name}` are not yet supported");
                }
                types.insert(name.clone(), tag.clone());
            }
            let mut s = s.clone();
            for b in children(&mut s) {
                definitions(b, types)?;
            }
        }
        Ok(())
    }
    let mut c = Context {
        types: HashMap::new(),
        fields: HashMap::new(),
        field_order: HashMap::new(),
        field_kinds: HashMap::new(),
        procedures: HashMap::new(),
        generics: HashSet::new(),
        strict,
        serial: 0,
        helpers: vec![],
        location: None,
        globals: Scope::default(),
        external: p
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. }
                    if !p.contains.contains(name) =>
                {
                    Some((
                        name.clone(),
                        body.iter()
                            .find_map(crate::source_diagnostics::statement_offset),
                    ))
                }
                _ => None,
            })
            .collect(),
    };
    definitions(&p.body, &mut c.types)?;
    if !c.types.is_empty() {
        for s in &p.body {
            if let Stmt::Module { body, .. } = s {
                let vars = c.scope(body, &Scope::default());
                c.globals.vars.extend(vars.vars);
                c.globals.kinds.extend(vars.kinds);
                c.globals.constants.extend(vars.constants);
                c.globals.parameters.extend(vars.parameters);
            }
        }
        c.collect(&p.body, &Scope::default())?;
    }
    Ok(c)
}
pub fn prepare(mut p: Program) -> Result<Program> {
    let c = context(&p, false)?;
    if !c.types.is_empty() {
        c.validate_body(&mut p.body, &Scope::default(), true, None)?;
    }
    Ok(p)
}
pub fn lower(mut p: Program) -> Result<Program> {
    let mut c = context(&p, true)?;
    if !c.types.is_empty() {
        c.validate_body(&mut p.body, &Scope::default(), true, None)?;
        c.erase_body(&mut p.body, &Scope::default(), false)?;
        p.body.extend(c.helpers);
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_procedure_does_not_inherit_main_enumerators() {
        let source="enumeration type colour\nenumerator red,green\nend enumeration type\nend\nreal function f()\nred=2.0\nf=red\nend function";
        let p =
            crate::parser::parse_with_src(source, &crate::lexer::lex(source), "scope.f90").unwrap();
        let p = super::lower(super::prepare(p).unwrap()).unwrap();
        assert!(p.body.iter().any(|s|matches!(s,crate::ast::Stmt::Function{name,return_type:Some(crate::ast::TypeSpec::Real),..} if name=="f")));
    }
}

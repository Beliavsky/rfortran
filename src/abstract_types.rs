// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate abstract type contracts before rank/type specialization discards
//! uncalled source procedures. Interface bodies are declarations, never code.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
pub(crate) const ABSTRACT: &str = "@abstract_type";
const INTERFACE: &str = "@abstract_interface";

pub(crate) fn deferred_bindings(
    interface: String,
    attrs: Vec<String>,
    names: Vec<String>,
) -> std::result::Result<Vec<(String, String)>, &'static str> {
    if !attrs.iter().any(|a| a == "deferred") {
        return Err("an explicit type-bound interface requires DEFERRED");
    }
    for a in &attrs {
        match a.as_str() {
            "deferred" | "public" | "private" | "pass" => {}
            "non_overridable" => return Err("DEFERRED and NON_OVERRIDABLE cannot be combined"),
            "nopass" => return Err("NOPASS deferred bindings are not yet supported"),
            _ => return Err("unsupported deferred binding attribute"),
        }
    }
    Ok(names
        .into_iter()
        .flat_map(|name| {
            [
                (name.clone(), interface.clone()),
                (format!("@deferred:{name}"), interface.clone()),
            ]
        })
        .collect())
}
pub(crate) fn interface(encoded: &str) -> std::result::Result<Vec<Stmt>, &'static str> {
    let (base, source) = encoded
        .split_once('|')
        .ok_or("invalid abstract interface encoding")?;
    let base = base
        .parse::<usize>()
        .map_err(|_| "invalid abstract interface location")?;
    // IMPORT affects visibility; these interface declarations are parsed in
    // isolation, then associated with their host by the module resolver. Blank
    // IMPORT without changing byte positions used for source diagnostics.
    let mut tokens = crate::lexer::lex(source);
    let mut ranges = vec![];
    for (i, t) in tokens.iter().enumerate() {
        if matches!(&t.kind,crate::lexer::TokenKind::Ident(n) if n=="import")
            && (i == 0
                || !crate::lexer::same_statement(&source[tokens[i - 1].span.end..t.span.start]))
        {
            let mut end = t.span.end;
            for next in &tokens[i + 1..] {
                if !crate::lexer::same_statement(&source[end..next.span.start]) {
                    break;
                }
                end = next.span.end;
            }
            ranges.push(t.span.start..end);
        }
    }
    let mut bytes = source.as_bytes().to_vec();
    for range in ranges {
        for b in &mut bytes[range] {
            if !matches!(*b, b'\r' | b'\n') {
                *b = b' ';
            }
        }
    }
    let source = String::from_utf8(bytes).map_err(|_| "invalid abstract interface source")?;
    tokens = crate::lexer::lex(&source);
    let mut program = crate::parser::parse_with_src(&source, &tokens, "<abstract interface>")
        .map_err(|_| "invalid abstract interface body")?;
    fn offsets(body: &mut [Stmt], base: usize) {
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                *p += base;
            }
            for b in children(s) {
                offsets(b, base);
            }
        }
    }
    offsets(&mut program.body, base);
    let mut out = vec![];
    for mut s in program.body {
        let function = matches!(&s, Stmt::Function { .. });
        match &mut s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                out.extend(crate::ast::procedure_declaration(&format!(
                    "abstract|{name}={}",
                    if function { "function" } else { "subroutine" }
                )));
                if crate::simple::is_simple(body) {
                    out.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: "@simple_interface".into(),
                    });
                }
                body.insert(
                    0,
                    Stmt::DummyIntent {
                        name: INTERFACE.into(),
                        intent: "true".into(),
                    },
                );
                out.push(s);
            }
            Stmt::SourceLocation(_) => {}
            _ => return Err("abstract interface bodies must declare procedures"),
        }
    }
    Ok(out)
}
fn prototype(body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| matches!(s,Stmt::DummyIntent{name,..} if name==INTERFACE))
}
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Intrinsic(TypeSpec),
    Derived(String, bool),
    Procedure(String),
    Unknown,
}
#[derive(Clone, Debug)]
struct Argument {
    kind: Kind,
    dims: Vec<Expr>,
    attrs: HashSet<String>,
}
impl Argument {
    fn unknown() -> Self {
        Self {
            kind: Kind::Unknown,
            dims: vec![],
            attrs: HashSet::new(),
        }
    }
}
#[derive(Clone)]
struct Signature {
    params: Vec<(String, Argument)>,
    result: Option<Argument>,
    pure: bool,
    elemental: bool,
    simple: bool,
    constants: HashMap<String, Expr>,
}
fn signature(statement: &Stmt, constants: HashMap<String, Expr>) -> Option<Signature> {
    let (name, params, body, header, pure, elemental, function) = match statement {
        Stmt::Function {
            name,
            params,
            body,
            return_type,
            pure,
            elemental,
        } => (
            name,
            params,
            body,
            return_type.clone(),
            *pure,
            *elemental,
            true,
        ),
        Stmt::Subroutine {
            name,
            params,
            body,
            pure,
            elemental,
        } => (name, params, body, None, *pure, *elemental, false),
        _ => return None,
    };
    let mut objects: HashMap<String, Argument> = HashMap::new();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    objects.insert(
                        n.clone(),
                        Argument {
                            kind: Kind::Intrinsic(kind.clone()),
                            dims: vec![],
                            attrs: HashSet::new(),
                        },
                    );
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                objects.insert(
                    name.clone(),
                    Argument {
                        kind: Kind::Intrinsic(kind.clone()),
                        dims: dims.clone(),
                        attrs: HashSet::new(),
                    },
                );
            }
            Stmt::DerivedDecl {
                type_name,
                names,
                polymorphic,
                dims,
                attributes,
            } => {
                for n in names {
                    objects.insert(
                        n.clone(),
                        Argument {
                            kind: Kind::Derived(type_name.clone(), *polymorphic),
                            dims: dims.clone(),
                            attrs: attributes.iter().cloned().collect(),
                        },
                    );
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            let obj = objects
                .entry(name.clone())
                .or_insert_with(Argument::unknown);
            if intent.starts_with("procedure(") {
                obj.kind = Kind::Procedure(intent.clone());
            } else {
                obj.attrs.insert(intent.clone());
            }
        }
    }
    let result = if function {
        let result_name = match body.last() {
            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
            _ => name,
        };
        let mut result = objects
            .get(result_name)
            .cloned()
            .unwrap_or_else(Argument::unknown);
        if result.kind == Kind::Unknown {
            if let Some(kind) = header {
                result.kind = Kind::Intrinsic(kind);
            }
        }
        Some(result)
    } else {
        None
    };
    Some(Signature {
        params: params
            .iter()
            .map(|n| {
                (
                    n.clone(),
                    objects.get(n).cloned().unwrap_or_else(Argument::unknown),
                )
            })
            .collect(),
        result,
        pure,
        elemental,
        simple: crate::simple::is_simple(body),
        constants,
    })
}
#[derive(Clone)]
struct Binding {
    target: String,
    deferred: bool,
    non_overridable: bool,
    position: Option<usize>,
}
#[derive(Clone)]
struct Definition {
    bindings: HashMap<String, Binding>,
    position: Option<usize>,
}
#[derive(Default)]
struct Contracts {
    abstracts: HashSet<String>,
    parents: HashMap<String, String>,
    types: HashMap<String, Definition>,
    procedures: HashMap<String, Signature>,
    interfaces: HashSet<String>,
    constants: HashMap<String, Expr>,
}
impl Contracts {
    fn collect(&mut self, body: &[Stmt]) {
        let mut position = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                position = Some(p);
            }
            match s {
                Stmt::CallSub { name, args } if name == ABSTRACT => {
                    if let [Expr::Ident(n)] = args.as_slice() {
                        self.abstracts.insert(n.clone());
                    }
                }
                Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                    if let [Expr::Ident(n), Expr::Ident(parent)] = args.as_slice() {
                        self.parents.insert(n.clone(), parent.clone());
                    }
                }
                Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } if attributes.iter().any(|a| a == "parameter") => {
                    for (n, v) in entities {
                        if let Some(v) = v {
                            self.constants.insert(n.clone(), v.clone());
                        }
                    }
                }
                Stmt::DerivedType {
                    name,
                    bindings,
                    components,
                } => {
                    for c in components {
                        if let Stmt::DummyIntent { name: n, intent } = c {
                            if n == crate::extensions::EXTENDS {
                                self.parents.insert(name.clone(), intent.clone());
                            }
                        }
                    }
                    self.types.insert(
                        name.clone(),
                        Definition {
                            position,
                            bindings: bindings
                                .iter()
                                .filter(|(n, _)| !n.starts_with('@'))
                                .map(|(n, target)| {
                                    let deferred = bindings
                                        .iter()
                                        .any(|(tag, _)| tag == &format!("@deferred:{n}"));
                                    let non_overridable = bindings
                                        .iter()
                                        .any(|(tag, _)| tag == &format!("@non_overridable:{n}"));
                                    let position = bindings
                                        .iter()
                                        .find(|(tag, _)| tag == &format!("@binding_location:{n}"))
                                        .and_then(|(_, p)| p.parse().ok())
                                        .or(position);
                                    (
                                        n.clone(),
                                        Binding {
                                            target: target.clone(),
                                            deferred,
                                            non_overridable,
                                            position,
                                        },
                                    )
                                })
                                .collect(),
                        },
                    );
                }
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    if prototype(body) {
                        self.interfaces.insert(name.clone());
                    }
                    let mut constants = self.constants.clone();
                    for s in body {
                        if let Stmt::InitializedDecl {
                            attributes,
                            entities,
                            ..
                        } = s
                        {
                            if attributes.iter().any(|a| a == "parameter") {
                                for (n, v) in entities {
                                    if let Some(v) = v {
                                        constants.insert(n.clone(), v.clone());
                                    }
                                }
                            }
                        }
                    }
                    self.procedures
                        .insert(name.clone(), signature(s, constants).unwrap());
                    self.collect(body);
                }
                _ => {
                    let mut copy = s.clone();
                    for b in children(&mut copy) {
                        self.collect(b);
                    }
                }
            }
        }
    }
    fn normalized(&self, kind: &Kind, constants: &HashMap<String, Expr>) -> Kind {
        match kind {
            Kind::Intrinsic(k) => {
                let k = crate::parameters::concrete_kind_with(k.clone(), constants)
                    .unwrap_or_else(|_| k.clone());
                Kind::Intrinsic(match k {
                    TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)),
                    TypeSpec::Character(None) => TypeSpec::Character(Some(1)),
                    TypeSpec::RealKind(ref n) if n == "4" => TypeSpec::Real,
                    TypeSpec::RealKind(ref n) if n == "8" => TypeSpec::DoublePrecision,
                    TypeSpec::LogicalKind(ref n) if n == "4" => TypeSpec::Logical,
                    TypeSpec::CharacterLength(ref e) => {
                        crate::parameters::constant_integer_with(e, constants)
                            .ok()
                            .and_then(|n| usize::try_from(n).ok())
                            .map(|n| TypeSpec::Character(Some(n)))
                            .unwrap_or(k)
                    }
                    other => other,
                })
            }
            other => other.clone(),
        }
    }
    fn argument(
        &self,
        a: &Argument,
        b: &Argument,
        context: &str,
        parent: &Signature,
        child: &Signature,
    ) -> Result<()> {
        if self.normalized(&a.kind, &parent.constants) != self.normalized(&b.kind, &child.constants)
        {
            bail!("{context}: type or kind mismatch");
        }
        if a.dims.len() != b.dims.len() {
            bail!("{context}: rank mismatch");
        }
        for attr in [
            "intent(in)",
            "intent(out)",
            "intent(inout)",
            "optional",
            "pointer",
            "allocatable",
            "value",
            "contiguous",
        ] {
            if a.attrs.contains(attr) != b.attrs.contains(attr) {
                bail!("{context}: {attr} attribute mismatch");
            }
        }
        for (a, b) in a.dims.iter().zip(&b.dims) {
            let category = |e: &Expr| match e {
                Expr::Ident(n) if n == "@deferred_extent" || n == "@assumed_extent" => {
                    Some(n.clone())
                }
                _ => None,
            };
            if category(a) != category(b) {
                bail!("{context}: array shape category mismatch");
            }
            if let (Ok(a), Ok(b)) = (
                crate::parameters::constant_integer_with(a, &parent.constants),
                crate::parameters::constant_integer_with(b, &child.constants),
            ) {
                if a != b {
                    bail!("{context}: explicit array shape mismatch");
                }
            }
        }
        Ok(())
    }
    fn passed(&self, signature: &Signature, owner: &str, binding: &str) -> Result<()> {
        let Some((_, arg)) = signature.params.first() else {
            bail!("binding `{binding}` requires a first passed-object dummy");
        };
        if !matches!(&arg.kind,Kind::Derived(t,true) if t==owner) || !arg.dims.is_empty() {
            bail!("binding `{binding}` requires a scalar CLASS({owner}) passed-object dummy");
        }
        if ["pointer", "allocatable", "value", "optional"]
            .iter()
            .any(|a| arg.attrs.contains(*a))
        {
            bail!("binding `{binding}` passed-object dummy cannot be POINTER, ALLOCATABLE, VALUE or OPTIONAL");
        }
        Ok(())
    }
    fn compatible(&self, parent: &Signature, child: &Signature, binding: &str) -> Result<()> {
        if parent.params.len() != child.params.len() {
            bail!("override of `{binding}` has a different number of dummy arguments");
        }
        if parent.result.is_some() != child.result.is_some() {
            bail!("override of `{binding}` must preserve function/subroutine category");
        }
        if parent.pure && !child.pure {
            bail!("override of PURE binding `{binding}` must be PURE");
        }
        if parent.simple && !child.simple {
            bail!("override of SIMPLE binding `{binding}` must be SIMPLE");
        }
        if parent.elemental != child.elemental {
            bail!("override of `{binding}` must preserve ELEMENTAL");
        }
        for (i, ((pn, p), (cn, c))) in parent.params.iter().zip(&child.params).enumerate() {
            let mut p = p.clone();
            let mut c = c.clone();
            if i == 0 {
                p.kind = Kind::Unknown;
                c.kind = Kind::Unknown;
            } else if pn != cn {
                bail!("override of `{binding}` must preserve dummy name `{pn}`");
            }
            self.argument(
                &p,
                &c,
                &format!("override of `{binding}`, dummy `{cn}`"),
                parent,
                child,
            )?;
        }
        if let (Some(p), Some(c)) = (&parent.result, &child.result) {
            self.argument(
                p,
                c,
                &format!("override of `{binding}`, function result"),
                parent,
                child,
            )?;
        }
        Ok(())
    }
    fn effective(
        &self,
        name: &str,
        done: &mut HashMap<String, HashMap<String, Binding>>,
        active: &mut HashSet<String>,
    ) -> Result<HashMap<String, Binding>> {
        if let Some(b) = done.get(name) {
            return Ok(b.clone());
        }
        let definition = self
            .types
            .get(name)
            .with_context(|| format!("unknown parent type `{name}`"))?;
        if !active.insert(name.into()) {
            bail!("cyclic type extension involving `{name}`");
        }
        let mut inherited = if let Some(p) = self.parents.get(name) {
            self.effective(p, done, active)?
        } else {
            HashMap::new()
        };
        for (method, binding) in &definition.bindings {
            let result = (|| -> Result<()> {
                if binding.deferred && !self.abstracts.contains(name) {
                    bail!("DEFERRED binding `{method}` requires ABSTRACT type `{name}`");
                }
                if !binding.deferred && self.interfaces.contains(&binding.target) {
                    bail!("nondeferred binding `{method}` requires an implementation, not an abstract interface");
                }
                let sig = self.procedures.get(&binding.target).with_context(|| {
                    format!(
                        "binding `{method}` requires a known explicit interface `{}`",
                        binding.target
                    )
                })?;
                self.passed(sig, name, method)?;
                if let Some(parent) = inherited.get(method) {
                    if parent.non_overridable {
                        bail!("cannot override NON_OVERRIDABLE binding `{method}`");
                    }
                    let original = &self.procedures[&parent.target];
                    self.compatible(original, sig, method)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, binding.position))?;
            inherited.insert(method.clone(), binding.clone());
        }
        if !self.abstracts.contains(name) {
            if let Some((method, _)) = inherited.iter().find(|(_, b)| b.deferred) {
                return Err(crate::source_diagnostics::at(anyhow::anyhow!("concrete type `{name}` must implement inherited DEFERRED binding `{method}`"),definition.position));
            }
        }
        active.remove(name);
        done.insert(name.into(), inherited.clone());
        Ok(inherited)
    }
    fn objects(&self, body: &[Stmt]) -> Result<()> {
        self.objects_in(body, &HashMap::new())
    }
    fn objects_in(&self, body: &[Stmt], inherited: &HashMap<String, String>) -> Result<()> {
        let mut declared = inherited.clone();
        for s in body {
            if let Stmt::DerivedDecl {
                type_name, names, ..
            } = s
            {
                for n in names {
                    declared.insert(n.clone(), type_name.clone());
                }
            }
        }
        for s in body {
            if let Stmt::VarDecl { kind, names } = s {
                let referenced = if let TypeSpec::Referenced { source, .. } = kind {
                    if let Expr::Ident(n) = source.as_ref() {
                        declared.get(n).cloned()
                    } else {
                        None
                    }
                } else {
                    None
                };
                for n in names {
                    if let Some(kind) = &referenced {
                        declared.insert(n.clone(), kind.clone());
                    } else {
                        declared.remove(n);
                    }
                }
            }
        }
        let mut position = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                position = Some(p);
            }
            let result = (|| -> Result<()> {
                if let Stmt::DerivedType { components, .. } = s {
                    self.objects_in(components, &declared)?;
                }
                if let Stmt::VarDecl {
                    kind:
                        TypeSpec::Referenced {
                            source,
                            polymorphic: false,
                        },
                    ..
                }
                | Stmt::ArrayDecl {
                    kind:
                        TypeSpec::Referenced {
                            source,
                            polymorphic: false,
                        },
                    ..
                } = s
                {
                    if matches!(source.as_ref(),Expr::Ident(n) if declared.get(n).is_some_and(|t|self.abstracts.contains(t)))
                    {
                        bail!("TYPEOF cannot declare a nonpolymorphic object of an ABSTRACT type");
                    }
                }
                if let Stmt::DerivedDecl {
                    type_name,
                    polymorphic: false,
                    ..
                } = s
                {
                    if self.abstracts.contains(type_name) {
                        bail!(
                            "cannot declare a nonpolymorphic object of ABSTRACT type `{type_name}`"
                        );
                    }
                }
                let mut copy = s.clone();
                for e in expressions(&mut copy) {
                    let mut error = None;
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(n, a) = e {
                            if self.abstracts.contains(n) {
                                error =
                                    Some(anyhow::anyhow!("cannot construct ABSTRACT type `{n}`"));
                            }
                            if n == "@type_is"
                                && matches!(a.as_slice(),[Expr::Ident(t)] if self.abstracts.contains(t))
                            {
                                error = Some(anyhow::anyhow!(
                                    "TYPE IS guard cannot name an ABSTRACT type; use CLASS IS"
                                ));
                            }
                        }
                    });
                    if let Some(e) = error {
                        return Err(e);
                    }
                }
                for b in children(&mut copy) {
                    self.objects_in(b, &declared)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, position))?;
        }
        Ok(())
    }
}
fn strip_interfaces(body: &mut Vec<Stmt>) {
    for s in body {
        if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
            if prototype(body) {
                *s = crate::imports::declaration(name.clone());
                continue;
            }
        }
        for b in children(s) {
            strip_interfaces(b);
        }
    }
}
pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut contracts = Contracts::default();
    contracts.collect(&program.body);
    let deferred = contracts
        .types
        .values()
        .any(|t| t.bindings.values().any(|b| b.deferred));
    if !contracts.abstracts.is_empty() || deferred {
        contracts.objects(&program.body)?;
        let mut done = HashMap::new();
        for name in contracts.types.keys() {
            contracts.effective(name, &mut done, &mut HashSet::new())?;
        }
    }
    strip_interfaces(&mut program.body);
    Ok(program)
}
pub(crate) fn finish(mut program: Program) -> Result<Program> {
    let mut contracts = Contracts::default();
    contracts.collect(&program.body);
    if !contracts.abstracts.is_empty() {
        contracts.objects(&program.body)?;
    }
    fn clear(body: &mut Vec<Stmt>) {
        body.retain(|s| !matches!(s,Stmt::CallSub{name,..} if name==ABSTRACT));
        for s in body {
            if let Stmt::DerivedType { bindings, .. } = s {
                bindings.retain(|(n, _)| {
                    !n.starts_with("@deferred:")
                        && !n.starts_with("@non_overridable:")
                        && !n.starts_with("@binding_location:")
                });
            }
            for b in children(s) {
                clear(b);
            }
        }
    }
    clear(&mut program.body);
    Ok(program)
}

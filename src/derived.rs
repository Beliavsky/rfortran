// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower scalar derived values to named component storage and local procedures.
//! CLASS dummies have the declared concrete type; extension/dynamic dispatch
//! and an external derived-value ABI are deliberately not implemented here.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

/// Each component of a derived object becomes its own variable named
/// `object%component`, so array, ALLOCATABLE and POINTER components reuse the
/// ordinary array and pointer lowering.
#[derive(Clone)]
struct Component {
    name: String,
    kind: TypeSpec,
    default: Option<Expr>,
    /// Empty for scalars; `@deferred_extent` entries for deferred shape.
    dims: Vec<Expr>,
    /// `allocatable` and/or `pointer`.
    attrs: Vec<String>,
    /// The type of a derived-type component, stored as its own components.
    derived: Option<String>,
}
impl Component {
    fn scalar(name: &str, kind: &TypeSpec, default: Option<Expr>) -> Self {
        Self { name: name.into(), kind: kind.clone(), default, dims: vec![], attrs: vec![], derived: None }
    }
    fn allocatable(&self) -> bool { self.attrs.iter().any(|a| a == "allocatable") }
    fn pointer(&self) -> bool { self.attrs.iter().any(|a| a == "pointer") }
    /// Plain scalar components are copied through temporaries, so constructor
    /// values may read the target object.
    fn plain_scalar(&self) -> bool { self.dims.is_empty() && self.attrs.is_empty() && self.derived.is_none() }
}
fn call(name: &str, args: Vec<Expr>) -> Stmt { Stmt::CallSub { name: name.into(), args } }
/// Whether `e` refers to the object `target` or one of its components.
fn mentions(e: &Expr, target: &str) -> bool {
    let named = |n: &str| n == target || n.strip_prefix(target).is_some_and(|rest| rest.starts_with('%'));
    match e {
        Expr::Ident(n) => named(n),
        Expr::Call(n, args) | Expr::Index(n, args) => named(n) || args.iter().any(|a| mentions(a, target)),
        Expr::Section { name, lower, upper, stride } => named(name)
            || [lower, upper, stride].into_iter().flatten().any(|e| mentions(e, target)),
        Expr::Bin(_, a, b) => mentions(a, target) || mentions(b, target),
        Expr::Un(_, a) => mentions(a, target),
        _ => false,
    }
}
fn test(name: &str, object: &str) -> Expr { Expr::Call(name.into(), vec![Expr::Ident(object.into())]) }
fn when(cond: Expr, then_body: Vec<Stmt>, else_body: Option<Vec<Stmt>>) -> Stmt {
    Stmt::If { cond, then_body, else_body }
}
#[derive(Clone)]
struct Definition {
    components: Vec<Component>,
    bindings: HashMap<String, String>,
}
#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    params: Vec<(String, Option<String>)>,
    result: Option<(String, String)>,
    function: bool,
    inputs: HashSet<String>,
    used_fields: HashMap<String, Vec<String>>,
}
// Track data objects separately from globally collected procedure signatures.
// Parent scopes are inherited; local declarations and dummies shadow them.
#[derive(Clone, Default)]
struct Objects {
    derived: HashMap<String, String>,
    arrays: HashSet<String>,
    /// Objects declared CLASS.
    polymorphic: HashSet<String>,
}
impl Objects {
    fn new() -> Self { Self::default() }
}
impl std::ops::Deref for Objects {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target { &self.derived }
}
impl std::ops::DerefMut for Objects {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.derived }
}
struct Lowerer {
    types: HashMap<String, Definition>,
    procedures: HashMap<String, Procedure>,
    serial: usize,
    /// Lowering a module's specification part or a procedure body.
    in_module: bool,
    in_procedure: bool,
    /// Module derived objects, whose names module association made unique.
    globals: Objects,
    /// The parent of each extended type.
    parents: HashMap<String, String>,
}

fn references(body: &[Stmt], names: &mut HashSet<String>) {
    fn expression(e: &Expr, names: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => {
                names.insert(n.clone());
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                names.insert(n.clone());
                for a in args {
                    expression(a, names);
                }
            }
            Expr::Bin(_, a, b) => {
                expression(a, names);
                expression(b, names);
            }
            Expr::Un(_, a) => expression(a, names),
            Expr::Section { name, lower, upper, stride } => {
                names.insert(name.clone());
                for e in [lower, upper, stride].into_iter().flatten() {
                    expression(e, names);
                }
            }
            _ => {}
        }
    }
    for s in body {
        match s {
            Stmt::Assign { name, value } => {
                names.insert(name.clone());
                expression(value, names);
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                names.insert(name.clone());
                for e in indices {
                    expression(e, names);
                }
                expression(value, names);
            }
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                if let Stmt::CallSub { name, .. } = s {
                    names.insert(name.clone());
                }
                for e in items {
                    expression(e, names);
                }
            }
            Stmt::Return(Some(e)) => expression(e, names),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, names);
                references(then_body, names);
                if let Some(b) = else_body {
                    references(b, names);
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                expression(start, names);
                expression(end, names);
                references(body, names);
            }
            Stmt::DoWhile { cond, body } => {
                expression(cond, names);
                references(body, names);
            }
            Stmt::Block { body } => references(body, names),
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                expression(expr, names);
                for c in cases {
                    references(&c.body, names);
                }
                if let Some(b) = default {
                    references(b, names);
                }
            }
            _ => {}
        }
    }
}

impl Lowerer {
    fn collect_types(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    if self.types.contains_key(name) {
                        bail!("duplicate derived type name `{name}`; distinct scopes with the same type name are not yet supported");
                    }
                    let mut fields: Vec<Component> = vec![];
                    let mut attrs: Vec<(String, String)> = vec![];
                    for c in components {
                        match c {
                            Stmt::SourceLocation(_) => {},
                            Stmt::DummyIntent { name: n, intent } if n == crate::extensions::EXTENDS => {
                                self.parents.insert(name.clone(), intent.clone());
                            }
                            Stmt::VarDecl { kind, names } => {
                                for n in names {
                                    fields.push(Component::scalar(n, kind, None));
                                }
                            }
                            Stmt::InitializedDecl {
                                kind,
                                attributes,
                                entities,
                            } => {
                                if !attributes.is_empty() {
                                    bail!("component attributes are not yet supported");
                                }
                                for (n, value) in entities {
                                    fields.push(Component::scalar(n, kind, value.clone()));
                                }
                            }
                            // An initialized array component is parsed as an
                            // InitializedDecl followed by its ArrayDecl.
                            Stmt::ArrayDecl { kind, name: n, dims } => {
                                match fields.iter_mut().find(|f| &f.name == n && f.dims.is_empty() && f.default.is_some()) {
                                    Some(f) => f.dims = dims.clone(),
                                    None => fields.push(Component { dims: dims.clone(), ..Component::scalar(n, kind, None) }),
                                }
                            }
                            Stmt::DummyIntent { name: n, intent } if intent == "allocatable" || intent == "pointer" || intent == "@record_slots" => {
                                attrs.push((n.clone(), intent.clone()));
                            }
                            Stmt::DummyIntent { intent, .. } => {
                                bail!("{} components are not yet supported", intent.to_uppercase())
                            }
                            // A derived component's default initializer, which the
                            // parser emits after the declaration it marks SAVE.
                            Stmt::Assign { name: n, value } => {
                                let f = fields.iter_mut().find(|f| &f.name == n && f.derived.is_some())
                                    .with_context(|| format!("initializer for unknown component `{n}`"))?;
                                let mut value = value.clone();
                                // Module renaming does not reach type definitions,
                                // so the constructor may use the local type name.
                                if let (Expr::Call(c, _), Some(ty)) = (&mut value, &f.derived) {
                                    if ty.rsplit('$').next() == Some(c.as_str()) {
                                        *c = ty.clone();
                                    }
                                }
                                f.default = Some(value);
                            }
                            Stmt::DerivedDecl { type_name, names, attributes, dims, .. } => {
                                if !self.types.contains_key(type_name) {
                                    bail!("component type `{type_name}` must be defined before type `{name}`");
                                }
                                if !dims.is_empty() {
                                    bail!("array components of derived type are not yet supported");
                                }
                                if attributes.iter().any(|a| a != "save") {
                                    bail!("ALLOCATABLE or POINTER components of derived type are not yet supported");
                                }
                                for n in names {
                                    fields.push(Component {
                                        derived: Some(type_name.clone()),
                                        ..Component::scalar(n, &TypeSpec::Integer(None), None)
                                    });
                                }
                            }
                            _ => bail!(
                                "derived types currently support intrinsic components only"
                            ),
                        }
                    }
                    for (n, a) in attrs {
                        let f = fields.iter_mut().find(|f| f.name == n)
                            .with_context(|| format!("attribute for unknown component `{n}`"))?;
                        f.attrs.push(a);
                    }
                    for f in &fields {
                        if f.derived.is_some() && !f.attrs.is_empty() {
                            bail!("ALLOCATABLE or POINTER components of derived type are not yet supported");
                        }
                        if f.allocatable() && f.dims.is_empty() {
                            bail!("ALLOCATABLE scalar component `{}` is not yet supported", f.name);
                        }
                        if f.allocatable() && f.pointer() {
                            bail!("component `{}` cannot be both ALLOCATABLE and POINTER", f.name);
                        }
                    }
                    let mut lengths=vec![];
                    for f in &mut fields {
                        if crate::substrings::deferred(&f.kind) && !f.dims.is_empty() && f.allocatable() {
                            let length=format!("@deferred_length_{}",f.name);
                            lengths.push(Component::scalar(&length,&TypeSpec::Integer(Some(8)),Some(Expr::IntLit("0".into()))));
                            f.kind=TypeSpec::Character(Some(4096));
                            f.attrs.push(format!("@deferred_array_length:{length}"));
                        }
                    }
                    fields.extend(lengths);
                    let mut names = HashSet::new();
                    for f in &fields {
                        if !names.insert(f.name.clone()) {
                            bail!("duplicate component `{}`", f.name);
                        }
                    }
                    let mut methods = HashMap::new();
                    for (n, target) in bindings.iter().filter(|(n, _)| !n.starts_with('@')) {
                        if !names.insert(n.clone()) {
                            bail!("duplicate component or binding `{n}`");
                        }
                        methods.insert(n.clone(), target.clone());
                    }
                    self.types.insert(
                        name.clone(),
                        Definition {
                            components: fields,
                            bindings: methods,
                        },
                    );
                }
                Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } => self.collect_types(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn objects(&self, body: &[Stmt], inherited: &Objects) -> Result<Objects> {
        let mut objects = inherited.clone();
        for s in body {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        objects.remove(n);
                        objects.arrays.remove(n);
                    }
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        objects.remove(n);
                        objects.arrays.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    objects.remove(name);
                    objects.arrays.insert(name.clone());
                }
                _ => {}
            }
        }
        for s in body {
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic,
                ..
            } = s
            {
                if !self.types.contains_key(type_name) {
                    bail!("unknown derived type `{type_name}`");
                }
                for n in names {
                    if *polymorphic {
                        objects.polymorphic.insert(n.clone());
                    } else {
                        objects.polymorphic.remove(n);
                    }
                }
                if attributes.iter().any(|a| !a.starts_with("intent(") && !matches!(a.as_str(), "parameter" | "save" | "public" | "private" | "optional" | "target")) {
                    bail!("derived objects currently support INTENT, OPTIONAL, PARAMETER, SAVE and TARGET attributes only");
                }
                for n in names {
                    objects.arrays.remove(n);
                    self.register(&mut objects, n, type_name);
                }
            }
        }
        Ok(objects)
    }
    fn collect_procedures(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    if matches!(s,Stmt::Function{pure:true,..}|Stmt::Subroutine{pure:true,..}) && body.iter().any(|s|matches!(s,Stmt::DerivedDecl{polymorphic:true,attributes,..} if attributes.iter().any(|a|a=="intent(out)"))) {bail!("PURE procedure cannot have a polymorphic INTENT(OUT) dummy");}
                    let objects = self.objects(body, &Objects::new())?;
                    let result_name = match s {
                        Stmt::Function { .. } => match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) if objects.contains_key(n) => {
                                Some(n.clone())
                            }
                            _ => objects.contains_key(name).then_some(name.clone()),
                        },
                        _ => None,
                    };
                    let result = result_name.map(|n| (n.clone(), objects[&n].clone()));
                    let mut inputs = HashSet::new();
                    for d in body {
                        if let Stmt::DerivedDecl {
                            names, attributes, ..
                        } = d
                        {
                            if attributes.iter().any(|a| a == "intent(in)") {
                                inputs.extend(names.iter().cloned());
                            }
                        }
                    }
                    let mut used = HashSet::new();
                    references(body, &mut used);
                    let mut used_fields = HashMap::new();
                    for p in params {
                        if let Some(ty) = objects.get(p) {
                            let definition = &self.types[ty];
                            // Both parts of a COMPLEX dummy are passed, so that
                            // separately compiled callers agree on its arguments.
                            let all = !inputs.contains(p)
                                || crate::complex::type_kind(ty).is_some()
                                || used.contains(p)
                                || definition
                                    .bindings
                                    .keys()
                                    .any(|b| used.contains(&format!("{p}%{b}")));
                            let leaves = self.leaves(ty);
                            let carrier = leaves.first().cloned();
                            let mut selected: Vec<_> = leaves.into_iter().filter(|leaf| {
                                all || leaf.match_indices('%').map(|(i, _)| &leaf[..i]).chain(std::iter::once(leaf.as_str()))
                                    .any(|prefix| used.contains(&format!("{p}%{prefix}")))
                            }).collect();
                            // Even an unused ELEMENTAL input supplies shape and
                            // conformance information. Preserve one component,
                            // while avoiding reads of other unused components.
                            if selected.is_empty() && matches!(s, Stmt::Function { elemental: true, .. } | Stmt::Subroutine { elemental: true, .. }) {
                                selected.extend(carrier);
                            }
                            used_fields.insert(p.clone(), selected);
                        }
                    }
                    self.procedures.insert(
                        name.clone(),
                        Procedure {
                            signature: crate::named_arguments::Signature::new(params, body),
                            params: params
                                .iter()
                                .map(|n| (n.clone(), objects.get(n).cloned()))
                                .collect(),
                            result,
                            function: matches!(s, Stmt::Function { .. }),
                            inputs,
                            used_fields,
                        },
                    );
                    self.collect_procedures(body)?;
                }
                Stmt::Module { body, .. } => {
                    self.objects(body, &Objects::new())?;
                    self.collect_procedures(body)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Whether `ty` is `ancestor` or extends it.
    fn extends(&self, ty: &str, ancestor: &str) -> bool {
        let mut t = ty;
        loop {
            if t == ancestor {
                return true;
            }
            match self.parents.get(t) {
                Some(p) => t = p,
                None => return false,
            }
        }
    }
    /// Component paths of a type down to non-derived storage, e.g. `p%x`.
    fn leaves(&self, ty: &str) -> Vec<String> {
        let mut out = vec![];
        for c in &self.types[ty].components {
            match &c.derived {
                Some(sub) => out.extend(self.leaves(sub).into_iter().map(|l| format!("{}%{l}", c.name))),
                None => out.push(c.name.clone()),
            }
        }
        out
    }
    fn field_names(&self, object: &str, ty: &str) -> Vec<String> {
        self.leaves(ty).into_iter().map(|l| format!("{object}%{l}")).collect()
    }
    /// Record an object and each of its derived-type components as objects.
    fn register(&self, objects: &mut Objects, name: &str, ty: &str) {
        objects.insert(name.to_string(), ty.to_string());
        for c in &self.types[ty].components {
            if let Some(sub) = &c.derived {
                self.register(objects, &format!("{name}%{}", c.name), sub);
            }
        }
    }
    fn component(&self, name: &str, objects: &Objects) -> Result<()> {
        if let Some((object, field)) = name.rsplit_once('%') {
            let ty = objects
                .get(object)
                .with_context(|| format!("`{object}` is not a declared derived object"))?;
            if !self.types[ty].components.iter().any(|c| c.name == field) {
                bail!("type `{ty}` has no component `{field}`");
            }
        }
        Ok(())
    }
    fn bound_call(&self, name: &mut String, args: &mut Vec<Expr>, objects: &Objects) -> Result<()> {
        if let Some((object, method)) = name.rsplit_once('%') {
            let ty = objects
                .get(object)
                .with_context(|| format!("`{object}` is not a derived object"))?;
            if self.types[ty].components.iter().any(|c| c.name == method && !c.dims.is_empty()) {
                // An element or section of an array component, not a binding.
                return Ok(());
            }
            let target = self.types[ty]
                .bindings
                .get(method)
                .with_context(|| format!("type `{ty}` has no binding `{method}`"))?;
            let procedure = self.procedures.get(target).with_context(|| {
                format!("binding `{method}` refers to undefined procedure `{target}`")
            })?;
            if !procedure.params.first().and_then(|(_, t)| t.as_deref()).is_some_and(|t| self.extends(ty, t)) {
                bail!("binding `{method}` requires a first passed-object dummy of type `{ty}`");
            }
            // A CLASS object may have an extended dynamic type whose binding
            // overrides this one, which needs dynamic dispatch.
            if objects.polymorphic.contains(object) {
                if let Some(other) = self.types.iter().find(|(t, d)| *t != ty && self.extends(t, ty)
                    && d.bindings.get(method).is_some_and(|b| b != target)).map(|(t, _)| t.clone())
                {
                    bail!("calling binding `{method}` of CLASS(`{ty}`) object `{object}` needs dynamic dispatch, because type `{other}` overrides it; this is not yet supported");
                }
            }
            args.insert(0, Expr::Ident(object.into()));
            *name = target.clone();
        }
        Ok(())
    }
    fn actuals(
        &mut self,
        name: &str,
        mut args: Vec<Expr>,
        objects: &Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Vec<Expr>> {
        // Intrinsic data expressions and ordinary procedure calls need no derived
        // argument association. In particular, array subscripts must never be
        // checked against an unrelated procedure's globally collected signature.
        let proc = self.procedures.get(name).filter(|p| {
            !objects.arrays.contains(name)
                && (p.result.is_some() || p.params.iter().any(|(_, ty)| ty.is_some()))
        }).cloned();
        let Some(proc) = proc else {
            let mut args = args;
            for a in &mut args {
                self.expression(a, objects, prefix)?;
            }
            return Ok(args);
        };
        proc.signature.reorder(name, &mut args)?;
        // Keep omitted slots when flattening derived inputs or appending result
        // components. Later OPTIONAL specialization removes the absent dummies.
        args.resize_with(proc.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        let mut result = vec![];
        for (mut arg, (param, ty)) in args.into_iter().zip(&proc.params) {
            if let Some(ty) = ty {
                // An omitted OPTIONAL derived argument omits each component.
                if crate::named_arguments::omitted(&arg) {
                    result.extend(proc.used_fields[param].iter().map(|_| Expr::Call(crate::named_arguments::OMITTED.into(), vec![])));
                    continue;
                }
                if let Expr::Call(tag, fields) = &arg {
                    if tag == "@elemental_record" {
                        if !matches!(fields.first(), Some(Expr::Str(actual)) if actual == ty) {
                            bail!("ELEMENTAL derived argument `{param}` requires type `{ty}`");
                        }
                        for field in &proc.used_fields[param] {
                            let mut value = fields[1..].iter().find_map(|e| match e {
                                Expr::Call(tag, values) if tag == &format!("@record_field:{field}") => values.first().cloned(),
                                _ => None,
                            }).with_context(|| format!("missing ELEMENTAL record component `{field}`"))?;
                            self.expression(&mut value, objects, prefix)?;
                            result.push(value);
                        }
                        continue;
                    }
                }
                let object = if let Expr::Ident(n) = &arg {
                    // An extension has its parent's components, by name.
                    if objects.get(n).is_some_and(|t| self.extends(t, ty)) {
                        Some(n.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                let object = if let Some(n) = object {
                    n
                } else {
                    if !proc.inputs.contains(param) {
                        bail!("writable derived argument `{param}` requires a variable of type `{ty}`");
                    }
                    // A constructor/function actual has its own component storage.
                    self.serial += 1;
                    let temp = format!("@derived_actual_{}", self.serial);
                    self.declare(&temp, &ty, &[], false, prefix)?;
                    self.assign(&temp, &ty, arg, objects, prefix)?;
                    temp
                };
                result.extend(
                    proc.used_fields[param]
                        .iter()
                        .map(|f| Expr::Ident(format!("{object}%{f}"))),
                );
            } else {
                self.expression(&mut arg, objects, prefix)?;
                result.push(arg);
            }
        }
        Ok(result)
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        objects: &Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        match e {
            Expr::Ident(n) => {
                self.component(n, objects)?;
                if objects.contains_key(n) {
                    bail!("derived value `{n}` requires a constructor, derived assignment, or typed procedure argument");
                }
            }
            // PRESENT of an OPTIONAL derived dummy is the presence of its
            // first component, which is passed with the others.
            Expr::Call(n, args) if n == "present" && matches!(args.as_slice(), [Expr::Ident(p)] if objects.contains_key(p)) => {
                let [Expr::Ident(p)] = args.as_slice() else { unreachable!() };
                let leaf = self.leaves(&objects[p]).into_iter().next().context("PRESENT of a derived dummy requires a component")?;
                *args = vec![Expr::Ident(format!("{p}%{leaf}"))];
            }
            Expr::Call(n, args) => {
                self.bound_call(n, args, objects)?;
                if !objects.arrays.contains(n) && (self.types.contains_key(n)
                    || self.procedures.get(n).is_some_and(|p| p.result.is_some()))
                {
                    bail!(
                        "derived-valued expression `{n}` requires a derived assignment or argument"
                    );
                }
                *args = self.actuals(n, std::mem::take(args), objects, prefix)?;
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, objects, prefix)?;
                self.expression(b, objects, prefix)?;
            }
            Expr::Un(_, a) => self.expression(a, objects, prefix)?,
            Expr::Index(n, args) => {
                self.component(n, objects)?;
                for a in args {
                    self.expression(a, objects, prefix)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn declare(
        &self,
        name: &str,
        ty: &str,
        attrs: &[String],
        dummy: bool,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let intent = attrs.iter().find(|a| a.starts_with("intent("));
        for c in &self.types[ty].components {
            let field = format!("{name}%{}", c.name);
            if let Some(sub) = &c.derived {
                self.declare(&field, sub, attrs, dummy, out)?;
                if !dummy || intent.is_some_and(|i| i == "intent(out)") {
                    if let Some(value) = &c.default {
                        let mut values = vec![];
                        if !self.initial_values(&field, sub, Some(value.clone()), true, &Objects::new(), &mut values)? {
                            bail!("default initialization of component `{}` currently requires scalar components", c.name);
                        }
                        out.extend(values.into_iter().filter_map(|(name, _, value)| value.map(|value| Stmt::Assign { name, value })));
                    }
                }
                continue;
            }
            for a in &c.attrs {
                if dummy && a=="@record_slots" {continue;}
                if let Some(length)=a.strip_prefix("@deferred_array_length:") {
                    let length=format!("{name}%{length}");
                    out.push(Stmt::DummyIntent{name:field.clone(),intent:format!("@character_spec_length:{length}")});
                    out.push(Stmt::DummyIntent{name:field.clone(),intent:format!("@deferred_array_length:{length}")});
                    continue;
                }
                out.push(Stmt::DummyIntent { name: field.clone(), intent: a.clone() });
            }
            // A nonpointer subobject of a TARGET object is a valid target too.
            if attrs.iter().any(|a| a == "target") && !c.pointer() {
                out.push(Stmt::DummyIntent { name: field.clone(), intent: "target".into() });
            }
            // Each component of an OPTIONAL derived dummy is an OPTIONAL dummy.
            if dummy && attrs.iter().any(|a| a == "optional") {
                out.push(Stmt::DummyIntent { name: field.clone(), intent: "optional".into() });
            }
            if c.dims.is_empty() {
                out.push(Stmt::VarDecl {
                    kind: c.kind.clone(),
                    names: vec![field.clone()],
                });
            } else {
                out.push(Stmt::ArrayDecl { kind: c.kind.clone(), name: field.clone(), dims: c.dims.clone() });
            }
            if let Some(i) = intent {
                out.push(Stmt::DummyIntent {
                    name: field.clone(),
                    intent: i.clone(),
                });
            }
            if !dummy || intent.is_some_and(|i| i == "intent(out)") {
                if let Some(value) = &c.default {
                    out.push(Stmt::Assign {
                        name: field,
                        value: value.clone(),
                    });
                }
            }
        }
        Ok(())
    }
    fn has_array_components(&self, ty: &str) -> bool {
        self.types[ty].components.iter().any(|field| !field.dims.is_empty()
            || field.derived.as_ref().is_some_and(|child| self.has_array_components(child)))
    }
    fn owns_record_slots(&self, ty:&str) -> bool {
        self.types[ty].components.iter().any(|field|field.attrs.iter().any(|a|a=="@record_slots")
            || field.derived.as_ref().is_some_and(|child|self.owns_record_slots(child)))
    }
    fn value_type<'a>(&'a self, value: &Expr, objects: &'a Objects) -> Option<&'a str> {
        match value {
            Expr::Ident(n) => objects.get(n).map(String::as_str),
            Expr::Call(n, args) => {
                if let Some((name, _)) = self.types.get_key_value(n) { return Some(name); }
                if let Some((_, ty)) = self.procedures.get(n).and_then(|p| p.result.as_ref()) { return Some(ty); }
                if n == "merge" && !self.procedures.contains_key(n) {
                    return args.first().and_then(|v| self.value_type(v, objects));
                }
                None
            }
            _ => None,
        }
    }
    fn assign(
        &mut self,
        target: &str,
        ty: &str,
        mut value: Expr,
        objects: &Objects,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        if self.owns_record_slots(ty) {
            if matches!(&value,Expr::Ident(n) if n==target) {return Ok(());}
            bail!("intrinsic assignment of containers whose array elements own allocatable components is not yet supported");
        }
        if let Expr::Call(n, args) = &mut value {
            self.bound_call(n, args, objects)?;
        }
        if let Expr::Call(n, args) = &value {
            if let Some(proc) = self
                .procedures
                .get(n)
                .cloned()
                .filter(|p| !objects.arrays.contains(n) && p.result.is_some())
            {
                if proc.result.as_ref().unwrap().1 != ty {
                    bail!("derived result type mismatch assigning `{target}`");
                }
                let mut args = self.actuals(n, args.clone(), objects, out)?;
                // The result components are written as the function runs, so
                // when an argument refers to the target, as in `r = f(r)`, the
                // result goes through a temporary.
                if args.iter().any(|a| mentions(a, target)) {
                    self.serial += 1;
                    let temp = format!("@derived_result_value_{}", self.serial);
                    self.declare(&temp, ty, &[], false, out)?;
                    args.extend(self.field_names(&temp, ty).into_iter().map(Expr::Ident));
                    out.push(Stmt::CallSub { name: format!("@derived_result_{n}"), args });
                    let mut temps = objects.clone();
                    temps.insert(temp.clone(), ty.to_owned());
                    return self.assign(target, ty, Expr::Ident(temp), &temps, out);
                }
                args.extend(self.field_names(target, ty).into_iter().map(Expr::Ident));
                out.push(Stmt::CallSub {
                    name: format!("@derived_result_{n}"),
                    args,
                });
                return Ok(());
            }
            if n == "merge" && !self.procedures.contains_key(n) && !objects.arrays.contains(n) {
                let [first, second, mask] = args.as_slice() else {
                    bail!("MERGE expects TSOURCE, FSOURCE and MASK");
                };
                if let (Some(first), Some(second)) = (self.value_type(first, objects), self.value_type(second, objects)) {
                    if first != second { bail!("MERGE sources must have the same derived type: `{first}` and `{second}`"); }
                }
                // Select the complete record rather than applying MERGE to
                // each component. Array/ALLOCATABLE/POINTER components keep
                // their ordinary derived-assignment semantics this way.
                let mut mask = mask.clone();
                self.expression(&mut mask, objects, out)?;
                self.serial += 1;
                let condition = format!("@derived_merge_mask_{}", self.serial);
                out.push(Stmt::VarDecl { kind: TypeSpec::Logical, names: vec![condition.clone()] });
                out.push(Stmt::Assign { name: condition.clone(), value: mask });
                let mut yes = vec![];
                let mut no = vec![];
                self.assign(target, ty, first.clone(), objects, &mut yes)?;
                self.assign(target, ty, second.clone(), objects, &mut no)?;
                out.push(when(Expr::Ident(condition), yes, Some(no)));
                return Ok(());
            }
        }
        enum Source {
            Copy(String),
            Value(Expr),
            Absent,
        }
        let fields = self.types[ty].components.clone();
        let constructor=matches!(&value,Expr::Call(n,_) if n==ty);
        let sources: Vec<Source> =
            match value {
                // The parent part `c%parent` of an extended object is `c`.
                Expr::Ident(n) if objects.get(&n).is_some_and(|t| self.extends(t, ty)) => fields
                    .iter()
                    .map(|f| Source::Copy(format!("{n}%{}", f.name)))
                    .collect(),
                Expr::Call(n, args) if n == ty => {
                    if args.len() > fields.len() {
                        bail!("too many components in constructor `{ty}`");
                    }
                    // Omitted components, including keyword gaps, take their defaults.
                    let mut args = args.into_iter();
                    let mut sources = vec![];
                    for c in &fields {
                        sources.push(match args.next().filter(|a| !crate::named_arguments::omitted(a)) {
                            Some(a) => Source::Value(a),
                            None => match &c.default {
                                Some(d) => Source::Value(d.clone()),
                                None if c.allocatable() || c.pointer() => Source::Absent,
                                None => bail!("missing constructor component `{}`", c.name),
                            },
                        });
                    }
                    sources
                }
                _ => bail!("derived assignment to `{target}` requires a value of type `{ty}`"),
            };
        // Scalar values go through temporaries and are written last; array,
        // ALLOCATABLE and POINTER components are written before them, so every
        // value still reads the target's old scalar components.
        let mut writes = vec![];
        let mut arrays = vec![];
        for (field, source) in fields.iter().zip(sources) {
            // The component assignment computes its deferred length. A
            // constructor's synthetic default must not overwrite that length.
            if constructor && field.name.starts_with("@deferred_length_") {continue;}
            let to = format!("{target}%{}", field.name);
            if let Some(sub) = &field.derived {
                let value = match source {
                    Source::Copy(n) => Expr::Ident(n),
                    Source::Value(v) => v,
                    Source::Absent => bail!("missing constructor component `{}`", field.name),
                };
                if matches!(&value, Expr::Ident(n) if *n == to) {
                    continue;
                }
                let mut nested = vec![];
                self.assign(&to, sub, value, objects, &mut nested)?;
                arrays.extend(nested);
                continue;
            }
            let mut value = match source {
                Source::Copy(n) if n == to => continue,
                Source::Copy(n) if !field.plain_scalar() => {
                    arrays.extend(self.copy_component(field, &to, &n));
                    continue;
                }
                Source::Copy(n) => Expr::Ident(n),
                Source::Value(v) => v,
                Source::Absent => {
                    arrays.push(if field.pointer() {
                        call("@nullify", vec![Expr::Ident(to)])
                    } else {
                        when(test("allocated", &to), vec![call("@deallocate", vec![Expr::Ident(to)])], None)
                    });
                    continue;
                }
            };
            self.expression(&mut value, objects, out)?;
            if !field.plain_scalar() {
                self.component_value(field, &to, value, &mut arrays, out)?;
                continue;
            }
            self.serial += 1;
            let temp = format!("@derived_value_{}", self.serial);
            out.push(Stmt::VarDecl {
                kind: field.kind.clone(),
                names: vec![temp.clone()],
            });
            out.push(Stmt::Assign {
                name: temp.clone(),
                value,
            });
            writes.push(Stmt::Assign {
                name: to,
                value: Expr::Ident(temp),
            });
        }
        out.extend(arrays);
        out.extend(writes);
        Ok(())
    }
    /// ALLOCATE `to` with the bounds of the allocated array `from`, then copy it.
    fn allocate_like(to: &str, from: &str, rank: usize) -> Vec<Stmt> {
        let bound = |f: &str, d: usize| {
            Some(Box::new(Expr::Call(f.into(), vec![Expr::Ident(from.into()), Expr::IntLit(d.to_string())])))
        };
        let subscripts = (1..=rank)
            .map(|d| Expr::Call("@section_subscript".into(), vec![Expr::Section {
                name: to.into(),
                lower: bound("lbound", d),
                upper: bound("ubound", d),
                stride: None,
            }]))
            .collect();
        vec![
            call("@allocate", vec![Expr::Call(to.into(), subscripts)]),
            Stmt::Assign { name: to.into(), value: Expr::Ident(from.into()) },
        ]
    }
    /// Intrinsic assignment of a non-scalar component from the same component
    /// of another object: deep copy for ALLOCATABLE, association for POINTER.
    fn copy_component(&self, field: &Component, to: &str, from: &str) -> Vec<Stmt> {
        let target = || Expr::Ident(to.into());
        if field.pointer() {
            vec![when(
                test("associated", from),
                vec![call("@pointer_associate", vec![target(), Expr::Ident(from.into())])],
                Some(vec![call("@nullify", vec![target()])]),
            )]
        } else if field.allocatable() {
            vec![
                when(test("allocated", to), vec![call("@deallocate", vec![target()])], None),
                when(test("allocated", from), Self::allocate_like(to, from, field.dims.len()), None),
            ]
        } else {
            vec![Stmt::Assign { name: to.into(), value: Expr::Ident(from.into()) }]
        }
    }
    /// Constructor or default value for a non-scalar component.
    fn component_value(
        &mut self,
        field: &Component,
        to: &str,
        value: Expr,
        arrays: &mut Vec<Stmt>,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let null = matches!(&value, Expr::Call(n, a) if n.eq_ignore_ascii_case("null") && a.is_empty());
        let target = || Expr::Ident(to.into());
        if field.pointer() {
            arrays.push(if null {
                call("@nullify", vec![target()])
            } else {
                call("@pointer_associate", vec![target(), value])
            });
        } else if field.allocatable() {
            if matches!(&value, Expr::Ident(n) if n == to) {
                return Ok(());
            }
            let release = when(test("allocated", to), vec![call("@deallocate", vec![target()])], None);
            match value {
                _ if null => arrays.push(release),
                Expr::Ident(n) => {
                    arrays.push(release);
                    arrays.extend(Self::allocate_like(to, &n, field.dims.len()));
                }
                // A literal constructor gives its extent without a runtime shape.
                Expr::Call(n, items)
                    if n == "@array_constructor" && field.dims.len() == 1 && items.iter().all(|e| {
                        let e = match e { Expr::Un(_, e) => e.as_ref(), e => e };
                        matches!(e, Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) | Expr::Str(_))
                    }) =>
                {
                    arrays.push(release);
                    let size = Expr::IntLit(items.len().to_string());
                    arrays.push(call("@allocate", vec![Expr::Call(to.into(), vec![size])]));
                    arrays.push(Stmt::Assign { name: to.into(), value: Expr::Call(n, items) });
                }
                // Intrinsic assignment (re)allocates heap-backed arrays to the
                // shape of the value after evaluating it, so it may read `to`.
                value => arrays.push(Stmt::Assign { name: to.into(), value }),
            }
        } else {
            self.serial += 1;
            let temp = format!("@derived_value_{}", self.serial);
            out.push(Stmt::ArrayDecl { kind: field.kind.clone(), name: temp.clone(), dims: field.dims.clone() });
            out.push(Stmt::Assign { name: temp.clone(), value });
            arrays.push(Stmt::Assign { name: to.into(), value: Expr::Ident(temp) });
        }
        Ok(())
    }
    /// A PARAMETER or SAVE derived object: each component becomes an
    /// initialized declaration, so named constants and persistent locals reuse
    /// the scalar lowering. Components without a value keep default
    /// initialization.
    fn initialized(
        &mut self,
        name: &str,
        ty: &str,
        attributes: &[String],
        value: Option<Expr>,
        objects: &Objects,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let parameter = attributes.iter().any(|a| a == "parameter");
        let mut fields = vec![];
        if !self.initial_values(name, ty, value.clone(), value.is_some(), objects, &mut fields)? {
            if parameter || self.in_procedure {
                bail!("{} derived object `{name}` currently requires scalar components",
                    if parameter { "PARAMETER" } else { "saved local" });
            }
            // Main-program and module storage persists anyway, so a
            // declaration and an assignment give the same value.
            self.declare(name, ty, &[], false, out)?;
            if let Some(value) = value {
                self.assign(name, ty, value, objects, out)?;
            }
            return Ok(());
        }
        let mut attrs = vec![if parameter { "parameter" } else { "save" }.to_string()];
        attrs.extend(attributes.iter().filter(|a| matches!(a.as_str(), "public" | "private")).cloned());
        if self.in_module && !parameter {
            attrs.push("@module_initializer".into());
        }
        for (field, kind, value) in fields {
            if parameter && value.is_none() {
                bail!("PARAMETER `{name}` requires a value for component `{field}`");
            }
            if value.is_none() && !self.in_procedure {
                out.push(Stmt::VarDecl { kind, names: vec![field] });
                continue;
            }
            out.push(Stmt::InitializedDecl { kind, attributes: attrs.clone(), entities: vec![(field, value)] });
        }
        Ok(())
    }
    /// Collect `(component, kind, value)` for each scalar leaf of `object`;
    /// false when a component is an array, ALLOCATABLE or POINTER.
    fn initial_values(
        &self,
        object: &str,
        ty: &str,
        value: Option<Expr>,
        explicit: bool,
        objects: &Objects,
        out: &mut Vec<(String, TypeSpec, Option<Expr>)>,
    ) -> Result<bool> {
        let args = match value {
            None => vec![],
            Some(Expr::Call(n, args)) if n == ty => args,
            // Another named constant or object of the type: its components.
            Some(Expr::Ident(n)) if objects.get(&n).is_some_and(|t| t == ty) => self.types[ty]
                .components.iter().map(|c| Expr::Ident(format!("{n}%{}", c.name))).collect(),
            Some(_) => bail!("the initializer of `{object}` must be a constructor of type `{ty}`"),
        };
        let components = &self.types[ty].components;
        if args.len() > components.len() {
            bail!("too many components in constructor `{ty}`");
        }
        let mut args = args.into_iter();
        for c in components {
            let field = format!("{object}%{}", c.name);
            let given = args.next().filter(|a| !crate::named_arguments::omitted(a));
            if let Some(sub) = &c.derived {
                let given = given.or_else(|| c.default.clone());
                if !self.initial_values(&field, sub, given, explicit, objects, out)? {
                    return Ok(false);
                }
                continue;
            }
            if !c.plain_scalar() {
                return Ok(false);
            }
            let value = given.or_else(|| c.default.clone());
            if explicit && value.is_none() {
                bail!("missing constructor component `{}`", c.name);
            }
            out.push((field, c.kind.clone(), value));
        }
        Ok(true)
    }
    /// A derived object in an output list stands for its components in
    /// declaration order, nested derived components included.
    fn output_items(&mut self, items: &mut Vec<Expr>, objects: &Objects, prefix: &mut Vec<Stmt>) -> Result<()> {
        self.transfer_items(items, objects, prefix, false)
    }
    /// Input expands writable component designators, never a record temporary.
    fn transfer_items(&mut self, items: &mut Vec<Expr>, objects: &Objects,
        prefix: &mut Vec<Stmt>, input: bool) -> Result<()> {
        let marker = if input { "@input_implied_do" } else { "@output_implied_do" };
        let mut out = vec![];
        for e in std::mem::take(items) {
            match e {
                Expr::Call(n, mut args) if input && matches!(n.as_str(), "@input_implied_do" | "@output_implied_do") => {
                    let mut values = args.split_off(4);
                    self.transfer_items(&mut values, objects, prefix, input)?;
                    args.extend(values);
                    out.push(Expr::Call(marker.into(), args));
                }
                Expr::Ident(n) if objects.contains_key(&n) => {
                    let ty = objects[&n].clone();
                    if self.has_dynamic_component(&ty) {
                        let direction = if input { "input" } else { "output" };
                        bail!("{direction} of derived object `{n}` with an ALLOCATABLE or POINTER component requires defined input/output");
                    }
                    // The fields `c$f` of an array component `c` of derived
                    // type are written element by element.
                    let leaves = self.leaves(&ty);
                    let mut i = 0;
                    while i < leaves.len() {
                        let group = |leaf: &str| leaf.rsplit_once('$').map(|(c, _)| c.to_owned());
                        let Some(key) = group(&leaves[i]) else {
                            out.push(Expr::Ident(format!("{n}%{}", leaves[i])));
                            i += 1;
                            continue;
                        };
                        let mut j = i;
                        while j < leaves.len() && group(&leaves[j]).as_deref() == Some(key.as_str()) {
                            j += 1;
                        }
                        let dims = self.leaf_component(&ty, &leaves[i]).map(|c| c.dims.clone()).unwrap_or_default();
                        let extents = dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()
                            .context("I/O of a derived array component requires constant extents")?;
                        self.serial += 1;
                        let index = format!("@derived_output_{}", self.serial);
                        prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![index.clone()] });
                        let mut ordinal = Expr::Bin(crate::ast::BinOp::Sub, Box::new(Expr::Ident(index.clone())), Box::new(Expr::IntLit("1".into())));
                        let mut subscripts = vec![];
                        for extent in &extents {
                            let extent = Expr::IntLit(extent.max(&0).to_string());
                            subscripts.push(Expr::Bin(crate::ast::BinOp::Add,
                                Box::new(Expr::Call("mod".into(), vec![ordinal.clone(), extent.clone()])),
                                Box::new(Expr::IntLit("1".into()))));
                            ordinal = Expr::Bin(crate::ast::BinOp::Div, Box::new(ordinal), Box::new(extent));
                        }
                        let count: i64 = extents.iter().map(|e| (*e).max(0)).product();
                        let mut args = vec![Expr::Ident(index), Expr::IntLit("1".into()), Expr::IntLit(count.to_string()), Expr::IntLit("1".into())];
                        args.extend(leaves[i..j].iter().map(|leaf| Expr::Index(format!("{n}%{leaf}"), subscripts.clone())));
                        out.push(Expr::Call(marker.into(), args));
                        i = j;
                    }
                }
                e => out.push(e),
            }
        }
        *items = out;
        Ok(())
    }
    /// The component that stores a leaf path of `ty`, such as `p%root$id`.
    fn leaf_component(&self, ty: &str, leaf: &str) -> Option<&Component> {
        match leaf.split_once('%') {
            Some((c, rest)) => {
                let sub = self.types[ty].components.iter().find(|x| x.name == c)?.derived.as_deref()?;
                self.leaf_component(sub, rest)
            }
            None => self.types[ty].components.iter().find(|x| x.name == leaf),
        }
    }
    fn has_dynamic_component(&self, ty: &str) -> bool {
        self.types[ty].components.iter().any(|c| {
            !c.attrs.is_empty() || c.derived.as_deref().is_some_and(|sub| self.has_dynamic_component(sub))
        })
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Objects, params: &[String]) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut inherited = inherited.clone();
        for n in params {
            inherited.remove(n);
            inherited.arrays.remove(n);
        }
        let objects = self.objects(body, &inherited)?;
        // The parser emits a declaration initializer as an assignment directly
        // after the declaration, which it also marks SAVE.
        let mut initializers = HashMap::new();
        let mut i = 0;
        while i + 1 < body.len() {
            if let (Stmt::DerivedDecl { names, attributes, .. }, Stmt::Assign { name, .. }) = (&body[i], &body[i + 1]) {
                if names.len() == 1 && &names[0] == name && attributes.iter().any(|a| a == "save" || a == "parameter") {
                    let Stmt::Assign { name, value } = body.remove(i + 1) else { unreachable!() };
                    initializers.insert(name, value);
                }
            }
            i += 1;
        }
        let mut output = vec![];
        for mut s in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) { statement_location.set(Some(p)); }
            let mut prefix = vec![];
            match &mut s {
                Stmt::DerivedType { .. } => continue,
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    attributes,
                    ..
                } => {
                    for n in names {
                        if attributes.iter().any(|a| a == "save" || a == "parameter") {
                            self.initialized(n, type_name, attributes, initializers.remove(n.as_str()), &objects, &mut output)?;
                        } else {
                            self.declare(n, type_name, attributes, params.contains(n), &mut output)?;
                        }
                    }
                    continue;
                }
                Stmt::Assign { name, value } if objects.contains_key(name) => {
                    self.assign(name, &objects[name], value.clone(), &objects, &mut output)?;
                    continue;
                }
                Stmt::Assign { name, value } => {
                    self.component(name, &objects)?;
                    self.expression(value, &objects, &mut prefix)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.component(name, &objects)?;
                    for e in indices {
                        self.expression(e, &objects, &mut prefix)?;
                    }
                    self.expression(value, &objects, &mut prefix)?;
                }
                // Accessibility of a derived object applies to its components.
                Stmt::Accessibility { names, .. } => {
                    let fields = names.iter().filter_map(|n| objects.get(n).map(|ty| self.field_names(n, ty)))
                        .flatten().collect::<Vec<_>>();
                    names.extend(fields);
                }
                Stmt::Print { items } => {
                    self.output_items(items, &objects, &mut prefix)?;
                    for e in items {
                        self.expression(e, &objects, &mut prefix)?;
                    }
                }
                Stmt::Read { args: items } => {
                    self.transfer_items(items, &objects, &mut prefix, true)?;
                    for e in items {
                        self.expression(e, &objects, &mut prefix)?;
                    }
                }
                Stmt::CallSub { name, args } => {
                    if let Some(first) = crate::input::transfer_start(name) {
                        let mut items = args.split_off(first.min(args.len()));
                        self.transfer_items(&mut items, &objects, &mut prefix, true)?;
                        args.extend(items);
                    }
                    if matches!(name.as_str(), "@formatted_print" | "@formatted_write_noadvance" | "@write" | "@unformatted_write") {
                        // Unformatted items follow the unit, control, status and position.
                        let mut first = match name.as_str() { "@write" => 2, "@unformatted_read" | "@unformatted_write" => 4, _ => 1 };
                        if name == "@write" {
                            while matches!(args.get(first), Some(Expr::Call(n, _)) if n.starts_with("@write_option:")) {
                                first += 1;
                            }
                        }
                        let mut items = args.split_off(first.min(args.len()));
                        self.output_items(&mut items, &objects, &mut prefix)?;
                        args.extend(items);
                    }
                    self.bound_call(name, args, &objects)?;
                    if self.procedures.get(name).is_some_and(|p| p.function) {
                        bail!("function `{name}` cannot be invoked with CALL");
                    }
                    *args = self.actuals(name, std::mem::take(args), &objects, &mut prefix)?;
                }
                Stmt::Return(Some(e)) => self.expression(e, &objects, &mut prefix)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &objects, &mut prefix)?;
                    self.body(then_body, &objects, &[])?;
                    if let Some(b) = else_body {
                        self.body(b, &objects, &[])?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &objects, &mut prefix)?;
                    self.expression(end, &objects, &mut prefix)?;
                    self.body(body, &objects, &[])?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &objects, &mut prefix)?;
                    if !prefix.is_empty() {
                        bail!("derived temporaries in DO WHILE conditions are not yet supported");
                    }
                    self.body(body, &objects, &[])?;
                }
                Stmt::Block { body } => self.body(body, &objects, &[])?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &objects, &mut prefix)?;
                    for case in cases {
                        self.body(&mut case.body, &objects, &[])?;
                    }
                    if let Some(body) = default {
                        self.body(body, &objects, &[])?;
                    }
                }
                Stmt::Module { body, .. } => {
                    let outer = std::mem::replace(&mut self.in_module, true);
                    let globals = self.globals.clone();
                    self.body(body, &globals, &[])?;
                    self.in_module = outer;
                }
                Stmt::Function {
                    name,
                    params,
                    body,
                    pure,
                    ..
                }
                | Stmt::Subroutine {
                    name,
                    params,
                    body,
                    pure,
                    ..
                } => {
                    let proc = self.procedures.get(name)
                        .with_context(|| format!("missing derived-lowering signature for procedure `{name}`"))?
                        .clone();
                    let outer = (self.in_module, self.in_procedure);
                    (self.in_module, self.in_procedure) = (false, true);
                    let array_record_abi = proc.params.iter().any(|(_, ty)|
                        ty.as_ref().is_some_and(|ty| self.has_array_components(ty)));
                    let old_params = params.clone();
                    let mut new_params = vec![];
                    for (n, ty) in &proc.params {
                        if ty.is_some() {
                            new_params
                                .extend(proc.used_fields[n].iter().map(|f| format!("{n}%{f}")));
                        } else {
                            new_params.push(n.clone());
                        }
                    }
                    if let Some((result, ty)) = &proc.result {
                        if matches!(body.last(),Some(Stmt::Return(Some(Expr::Ident(n)))) if n==result)
                        {
                            body.pop();
                        }
                        fn has_return(body: &[Stmt]) -> bool {
                            body.iter().any(|s| match s {
                                Stmt::Return(_) => true,
                                Stmt::If {
                                    then_body,
                                    else_body,
                                    ..
                                } => {
                                    has_return(then_body)
                                        || else_body.as_ref().is_some_and(|b| has_return(b))
                                }
                                Stmt::Do { body, .. }
                                | Stmt::DoWhile { body, .. }
                                | Stmt::Block { body } => has_return(body),
                                Stmt::SelectCase { cases, default, .. } => {
                                    cases.iter().any(|c| has_return(&c.body))
                                        || default.as_ref().is_some_and(|b| has_return(b))
                                }
                                _ => false,
                            })
                        }
                        // The function becomes a subroutine that writes its result's
                        // components as it runs, so a RETURN returns no value.
                        if has_return(body) {
                            fn plain(body: &mut [Stmt]) {
                                for s in body {
                                    // Internal procedures own their RETURN statements;
                                    // converting the host's result ABI must not alter them.
                                    if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                                        continue;
                                    }
                                    if let Stmt::Return(value) = s {
                                        *value = None;
                                    }
                                    for b in crate::procedure_arguments::children(s) {
                                        plain(b);
                                    }
                                }
                            }
                            plain(body);
                        }
                        self.body(body, &objects, &old_params)?;
                        (self.in_module, self.in_procedure) = outer;
                        for n in self.field_names(result, ty) {
                            new_params.push(n.clone());
                            body.push(Stmt::DummyIntent {
                                name: n,
                                intent: "intent(out)".into(),
                            });
                        }
                        s = Stmt::Subroutine {
                            elemental: false,
                            name: format!("@derived_result_{name}"),
                            params: new_params,
                            body: std::mem::take(body),
                            pure: *pure,
                        };
                    } else {
                        self.body(body, &objects, &old_params)?;
                        (self.in_module, self.in_procedure) = outer;
                        *params = new_params;
                        // ELEMENTAL source dummies are scalar records. Their
                        // component ABI may have array dummies after flattening.
                        if array_record_abi {
                            match &mut s {
                                Stmt::Function { elemental, .. } | Stmt::Subroutine { elemental, .. } => *elemental = false,
                                _ => unreachable!(),
                            }
                        }
                    }
                }
                _ => {}
            }
            output.extend(prefix);
            output.push(s);
        }
        *body = output;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
pub fn resolve(program: Program) -> Result<Program> {
    let program = crate::allocation_options::resolve(resolve_types(program)?)?;
    crate::simple::validate_calls(&program)?;
    crate::deferred_character_components::resolve(program)
}
fn resolve_types(mut program: Program) -> Result<Program> {
    program=crate::input::prepare_formatted(program);
    crate::source_diagnostics::validate_function_results(&program)?;
    crate::enumerations::validate_definitions(&program)?;
    crate::conditionals::validate_source(&program)?;
    crate::parameters::resolve_intrinsic_modules(&mut program)?;
    program = crate::block_scopes::resolve(program)?;
    crate::parameters::fold_kind_inquiries(&mut program);
    crate::imports::validate(&program)?;
    program=crate::modules::associate(program)?;
    program=crate::multiple_subscripts::resolve(program)?;
    program=crate::concurrent_reduce::resolve(program)?;
    crate::simple::validate(&program)?;
    program = crate::extensions::resolve(program)?;
    program=crate::referenced_types::resolve(program)?;
    program = crate::procedure_arguments::resolve(program)?;
    program = crate::heap::automatic_derived(program)?;
    crate::imports::validate(&program)?;
    crate::referenced_types::clear_provenance(&mut program.body);
    program=crate::tokenize::prepare(program)?;
    program = crate::named_arguments::resolve(program, false)?;
    program=crate::enumerations::prepare(program)?;
    if crate::conditionals::contains(&program) {
        crate::operators::validate_conditionals(&program)?;
        program=crate::parameters::resolve_constants(program)?;
    }
    program = crate::operators::resolve(program)?;
    program = crate::enumerations::lower(program)?;
    // Enum scalar dummies/results retain nominal identity through dispatch and
    // become supported INTEGER storage before ELEMENTAL storage validation.
    crate::elemental::validate_program(&program)?;
    if crate::conditionals::contains(&program) {program=crate::heap::automatic_locals(program)?;}
    program = crate::conditionals::resolve(program)?;
    program = crate::named_arguments::resolve(program, true)?;
    program = crate::intrinsic_rewrites::resolve(program)?;
    program = crate::matmul_runtime::resolve(program)?;
    program = crate::complex::resolve(program)?;
    program = crate::derived_arrays::resolve(program)?;
    let mut lower = Lowerer {
        types: HashMap::new(),
        procedures: HashMap::new(),
        serial: 0,
        in_module: false,
        in_procedure: false,
        globals: Objects::new(),
        parents: HashMap::new(),
    };
    lower.collect_types(&program.body)?;
    lower.collect_procedures(&program.body)?;
    if lower.types.is_empty() {
        // Still diagnose undeclared TYPE/CLASS declarations.
        fn reject_unknown(body: &[Stmt]) -> Result<()> {
            for s in body {
                match s {
                    Stmt::DerivedDecl { type_name, .. } => {
                        bail!("unknown derived type `{type_name}`")
                    }
                    Stmt::Module { body, .. }
                    | Stmt::Function { body, .. }
                    | Stmt::Subroutine { body, .. }
                    | Stmt::Block { body }
                    | Stmt::Do { body, .. }
                    | Stmt::DoWhile { body, .. } => reject_unknown(body)?,
                    Stmt::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        reject_unknown(then_body)?;
                        if let Some(b) = else_body {
                            reject_unknown(b)?;
                        }
                    }
                    Stmt::SelectCase { cases, default, .. } => {
                        for c in cases {
                            reject_unknown(&c.body)?;
                        }
                        if let Some(b) = default {
                            reject_unknown(b)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        reject_unknown(&program.body)?;
        return Ok(program);
    }
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            let objects = lower.objects(body, &Objects::new())?;
            lower.globals.derived.extend(objects.derived);
        }
    }
    let globals = lower.globals.clone();
    lower.body(&mut program.body, &globals, &[])?;
    crate::complex::hoist(&mut program.body);
    Ok(program)
}

#[cfg(test)]
mod tests {
    fn parse(source: &str) -> crate::ast::Program {
        crate::parser::parse_with_src(source, &crate::lexer::lex(source), "derived.f90").unwrap()
    }
    fn lower(source: &str) -> anyhow::Result<crate::ast::Program> {
        crate::sections::resolve(crate::reductions::resolve(crate::array_calls::resolve(
            crate::parameters::resolve(super::resolve(parse(source))?)?,
        )?)?)
    }
    #[test]
    fn lowers_derived_examples_and_preserves_initialization() {
        for source in [
            include_str!("../tests/50_derived_date.f90"),
            include_str!("../tests/51_derived_edges.f90"),
            include_str!("../tests/52_derived_components.f90"),
        ] {
            crate::initialized::instrument(lower(source).unwrap()).unwrap();
        }
        let p = lower(include_str!("../tests/53_derived_uninitialized.f90")).unwrap();
        assert!(crate::initialized::instrument(p)
            .unwrap_err()
            .to_string()
            .contains("p%second"));
    }
    #[test]
    fn rejects_invalid_components_constructors_and_bindings() {
        for (statement, expected) in [
            ("a%unknown=3", "no component"),
            ("call a%missing()", "no binding"),
            ("a=pair(1,2,3)", "too many components"),
            ("a=pair()", "missing constructor component"),
            ("a=other(1)", "requires a value of type"),
        ] {
            let source = format!(
                "type :: pair\ninteger :: x,y\nend type pair\ntype(pair)::a\n{statement}\nend"
            );
            let e = lower(&source).unwrap_err();
            assert!(format!("{e:#}").contains(expected), "{e:#}");
        }
        let source = "type(missing)::x\nend";
        assert!(lower(source)
            .unwrap_err()
            .to_string()
            .contains("unknown derived type"));
        let source = include_str!("../tests/52_derived_components.f90")
            .replace("subroutine reset(self)", "pure subroutine reset(self)");
        assert!(lower(&source)
            .unwrap_err()
            .to_string()
            .contains("polymorphic INTENT(OUT)"));
    }
    #[test]
    fn pure_component_writes_remain_checked() {
        let source = include_str!("../tests/50_derived_date.f90").replace(
            "valid = self%month >= 1",
            "self%month=2\nvalid = self%month >= 1",
        );
        assert!(lower(&source)
            .unwrap_err()
            .to_string()
            .contains("cannot modify dummy argument"));
    }
}

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
                            Stmt::DummyIntent { name: n, intent } if intent == "allocatable" || intent == "pointer" => {
                                attrs.push((n.clone(), intent.clone()));
                            }
                            Stmt::DummyIntent { intent, .. } => {
                                bail!("{} components are not yet supported", intent.to_uppercase())
                            }
                            Stmt::DerivedDecl { type_name, names, attributes, dims, .. } => {
                                if !self.types.contains_key(type_name) {
                                    bail!("component type `{type_name}` must be defined before type `{name}`");
                                }
                                if !dims.is_empty() {
                                    bail!("array components of derived type are not yet supported");
                                }
                                if !attributes.is_empty() {
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
                    let mut names = HashSet::new();
                    for f in &fields {
                        if !names.insert(f.name.clone()) {
                            bail!("duplicate component `{}`", f.name);
                        }
                    }
                    let mut methods = HashMap::new();
                    for (n, target) in bindings {
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
                ..
            } = s
            {
                if !self.types.contains_key(type_name) {
                    bail!("unknown derived type `{type_name}`");
                }
                if attributes.iter().any(|a| !a.starts_with("intent(")) {
                    bail!("derived objects currently support INTENT attributes only");
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
                            let all = !inputs.contains(p)
                                || used.contains(p)
                                || definition
                                    .bindings
                                    .keys()
                                    .any(|b| used.contains(&format!("{p}%{b}")));
                            let leaves = self.leaves(ty);
                            used_fields.insert(
                                p.clone(),
                                leaves
                                    .into_iter()
                                    .filter(|leaf| {
                                        all || leaf.match_indices('%').map(|(i, _)| &leaf[..i]).chain(std::iter::once(leaf.as_str()))
                                            .any(|prefix| used.contains(&format!("{p}%{prefix}")))
                                    })
                                    .collect(),
                            );
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
            if procedure.params.first().and_then(|(_, ty)| ty.as_deref()) != Some(ty.as_str()) {
                bail!("binding `{method}` requires a first passed-object dummy of type `{ty}`");
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
                let object = if let Expr::Ident(n) = &arg {
                    if objects.get(n) == Some(ty) {
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
                continue;
            }
            for a in &c.attrs {
                out.push(Stmt::DummyIntent { name: field.clone(), intent: a.clone() });
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
    fn assign(
        &mut self,
        target: &str,
        ty: &str,
        mut value: Expr,
        objects: &Objects,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
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
                args.extend(self.field_names(target, ty).into_iter().map(Expr::Ident));
                out.push(Stmt::CallSub {
                    name: format!("@derived_result_{n}"),
                    args,
                });
                return Ok(());
            }
        }
        enum Source {
            Copy(String),
            Value(Expr),
            Absent,
        }
        let fields = self.types[ty].components.clone();
        let sources: Vec<Source> =
            match value {
                Expr::Ident(n) if objects.get(&n).is_some_and(|t| t == ty) => fields
                    .iter()
                    .map(|f| Source::Copy(format!("{n}%{}", f.name)))
                    .collect(),
                Expr::Call(n, args) if n == ty => {
                    if args.len() > fields.len() {
                        bail!("too many components in constructor `{ty}`");
                    }
                    let mut sources: Vec<Source> = args.into_iter().map(Source::Value).collect();
                    for c in fields.iter().skip(sources.len()) {
                        sources.push(match &c.default {
                            Some(d) => Source::Value(d.clone()),
                            None if c.allocatable() || c.pointer() => Source::Absent,
                            None => bail!("missing constructor component `{}`", c.name),
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
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Objects, params: &[String]) -> Result<()> {
        let mut inherited = inherited.clone();
        for n in params {
            inherited.remove(n);
            inherited.arrays.remove(n);
        }
        let objects = self.objects(body, &inherited)?;
        let mut output = vec![];
        for mut s in std::mem::take(body) {
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
                        self.declare(n, type_name, attributes, params.contains(n), &mut output)?;
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
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        self.expression(e, &objects, &mut prefix)?;
                    }
                }
                Stmt::CallSub { name, args } => {
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
                Stmt::Module { body, .. } => self.body(body, &Objects::new(), &[])?,
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
                        if has_return(body) {
                            bail!("explicit early RETURN in a derived-result function is not yet supported");
                        }
                        self.body(body, &objects, &old_params)?;
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
                        *params = new_params;
                    }
                }
                _ => {}
            }
            output.extend(prefix);
            output.push(s);
        }
        *body = output;
        Ok(())
    }
}
pub fn resolve(program: Program) -> Result<Program> {
    crate::allocation_options::resolve(resolve_types(program)?)
}
fn resolve_types(mut program: Program) -> Result<Program> {
    crate::parameters::resolve_intrinsic_modules(&mut program)?;
    crate::imports::validate(&program)?;
    program=crate::modules::associate(program)?;
    program = crate::procedure_arguments::resolve(program)?;
    program = crate::heap::automatic_derived(program)?;
    crate::elemental::validate_program(&program)?;
    crate::imports::validate(&program)?;
    program = crate::named_arguments::resolve(program, false)?;
    program = crate::operators::resolve(program)?;
    program = crate::named_arguments::resolve(program, true)?;
    program = crate::intrinsic_rewrites::resolve(program)?;
    program = crate::complex::resolve(program)?;
    program = crate::derived_arrays::resolve(program)?;
    let mut lower = Lowerer {
        types: HashMap::new(),
        procedures: HashMap::new(),
        serial: 0,
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
    lower.body(&mut program.body, &Objects::new(), &[])?;
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

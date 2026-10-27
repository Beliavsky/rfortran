// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Check component accessibility after USE association assigns type identities,
//! before specialization and flattening erase the defining module and receiver.
use crate::ast::{CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

type Objects = HashMap<String, String>;
#[derive(Clone)]
struct Field {
    name: String,
    derived: Option<String>,
    private: bool,
    binding: bool,
}
struct Definition {
    module: Option<String>,
    fields: Vec<Field>,
    bindings: Vec<Field>,
    parent: Option<String>,
}
#[derive(Default)]
struct Validator {
    types: HashMap<String, Definition>,
    parents: HashMap<String, String>,
    generics: HashSet<String>,
    results: HashMap<String, String>,
}
fn spelling(name: &str) -> &str {
    name.rsplit('$').next().unwrap_or(name)
}

impl Validator {
    fn collect(
        &mut self,
        body: &mut [Stmt],
        module: Option<&str>,
        module_spec: bool,
    ) -> Result<()> {
        let mut location = None;
        for statement in body {
            if let Stmt::SourceLocation(p) = statement {
                location = Some(*p);
            }
            let result = (|| -> Result<()> {
                match statement {
                    Stmt::Module { name, body } => self.collect(body, Some(name), true)?,
                    Stmt::Function { name, body, .. } => {
                        let result = body
                            .last()
                            .and_then(|s| match s {
                                Stmt::Return(Some(Expr::Ident(n))) => Some(n.as_str()),
                                _ => None,
                            })
                            .unwrap_or(name);
                        if let Some(ty) = body.iter().find_map(|s| match s {
                            Stmt::DerivedDecl {
                                type_name, names, ..
                            } if names.iter().any(|n| n == result) => Some(type_name.clone()),
                            _ => None,
                        }) {
                            self.results.insert(name.clone(), ty);
                        }
                        self.collect(body, module, false)?;
                    }
                    Stmt::DerivedType {
                        name,
                        components,
                        bindings,
                    } => {
                        let default_private = components.iter().any(|s| {
                            matches!(s,
                            Stmt::Accessibility { public:false, names } if names.is_empty())
                        });
                        let mut access = HashMap::new();
                        let mut fields = Vec::new();
                        for c in components.iter() {
                            if let Stmt::Accessibility { public, names } = c {
                                for n in names {
                                    access.insert(n.clone(), !*public);
                                }
                            }
                        }
                        for c in components.iter() {
                            let mut add =
                                |n: &String, derived: Option<String>, attrs: &[String]| {
                                    let private = access.get(n).copied().unwrap_or_else(|| {
                                        if attrs.iter().any(|a| a == "private") {
                                            true
                                        } else if attrs.iter().any(|a| a == "public") {
                                            false
                                        } else {
                                            default_private
                                        }
                                    });
                                    // Initialized arrays have two declaration entries.
                                    if !fields.iter().any(|f: &Field| f.name == *n) {
                                        fields.push(Field {
                                            name: n.clone(),
                                            derived,
                                            private,
                                            binding: false,
                                        });
                                    }
                                };
                            match c {
                                Stmt::VarDecl { names, .. } => {
                                    for n in names {
                                        add(n, None, &[]);
                                    }
                                }
                                Stmt::ArrayDecl { name, .. } => add(name, None, &[]),
                                Stmt::InitializedDecl {
                                    entities,
                                    attributes,
                                    ..
                                } => {
                                    for (n, _) in entities {
                                        add(n, None, attributes);
                                    }
                                }
                                Stmt::DerivedDecl {
                                    names,
                                    type_name,
                                    attributes,
                                    ..
                                } => {
                                    for n in names {
                                        add(n, Some(type_name.clone()), attributes);
                                    }
                                }
                                _ => {}
                            }
                        }
                        if !module_spec && (default_private || fields.iter().any(|f| f.private)) {
                            bail!("PRIVATE components require a derived type defined in a module");
                        }
                        let private_bindings = bindings
                            .iter()
                            .any(|(n, a)| n == "@binding_default_access" && a == "@private");
                        let members: Vec<Field> = bindings
                            .iter()
                            .filter_map(|(n, _)| {
                                let name = n.strip_prefix("@generic:").unwrap_or(n);
                                if name.starts_with('@') {
                                    return None;
                                }
                                let private = bindings
                                    .iter()
                                    .find(|(n, _)| n == &format!("@binding_access:{name}"))
                                    .map(|(_, a)| a == "@private")
                                    .unwrap_or(private_bindings);
                                Some(Field {
                                    name: name.into(),
                                    derived: None,
                                    private,
                                    binding: true,
                                })
                            })
                            .collect();
                        self.types.insert(
                            name.clone(),
                            Definition {
                                module: module.map(str::to_owned),
                                fields,
                                bindings: members,
                                parent: None,
                            },
                        );
                    }
                    Stmt::CallSub { name, args } if name == crate::extensions::EXTENDS => {
                        if let [Expr::Ident(child), Expr::Ident(parent)] = args.as_slice() {
                            self.parents.insert(child.clone(), parent.clone());
                        }
                    }
                    Stmt::GenericInterface { name, .. } => {
                        self.generics.insert(name.clone());
                    }
                    _ => {
                        for b in crate::procedure_arguments::children(statement) {
                            self.collect(b, module, false)?;
                        }
                    }
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }

    fn field<'a>(&'a self, ty: &str, field: &str) -> Option<(&'a Definition, &'a Field)> {
        let mut next = ty;
        let mut seen = HashSet::new();
        while seen.insert(next) {
            let def = self.types.get(next)?;
            if let Some(f) = def
                .fields
                .iter()
                .chain(&def.bindings)
                .find(|f| f.name == field)
            {
                return Some((def, f));
            }
            next = def.parent.as_deref()?;
        }
        None
    }
    fn accessible(&self, def: &Definition, field: &Field, module: Option<&str>) -> Result<()> {
        if field.private && def.module.as_deref() != module {
            bail!(
                "{} `{}` is PRIVATE to module `{}`",
                if field.binding {
                    "binding"
                } else {
                    "component"
                },
                field.name,
                def.module.as_deref().unwrap_or("<unknown>")
            );
        }
        Ok(())
    }
    // Returns the derived type of the last component, if it is itself derived.
    fn path(&self, name: &str, objects: &Objects, module: Option<&str>) -> Result<Option<String>> {
        let mut parts = name.split('%');
        let Some(mut ty) = parts.next().and_then(|n| objects.get(n)).cloned() else {
            return Ok(None);
        };
        for part in parts {
            if let Some(def) = self.types.get(&ty) {
                if let Some(parent) = &def.parent {
                    if spelling(parent) == part {
                        ty = parent.clone();
                        continue;
                    }
                }
            }
            let Some((def, field)) = self.field(&ty, part) else {
                return Ok(None);
            };
            self.accessible(def, field, module)?;
            let Some(nested) = &field.derived else {
                return Ok(None);
            };
            ty = nested.clone();
        }
        Ok(Some(ty))
    }
    fn constructor(&self, name: &str, args: &[Expr], module: Option<&str>) -> Result<()> {
        if self.generics.contains(name) {
            return Ok(());
        } // User-defined constructor interface.
        let Some(def) = self.types.get(name) else {
            return Ok(());
        };
        // A structure constructor has the parent component first, followed by
        // its own components. Only supplied private components are inaccessible;
        // an omitted component with a default remains legal.
        let parent = def.parent.as_ref().map(|p| spelling(p));
        for (index, arg) in args.iter().enumerate() {
            let keyword = crate::named_arguments::keyword(arg).map(|(n, _)| n);
            let field = if let Some(key) = keyword {
                def.fields.iter().find(|f| f.name == key)
            } else {
                index
                    .checked_sub(usize::from(parent.is_some()))
                    .and_then(|i| def.fields.get(i))
            };
            if let Some(field) = field {
                self.accessible(def, field, module)?;
            }
        }
        Ok(())
    }
    fn expr(&self, expr: &Expr, objects: &Objects, module: Option<&str>) -> Result<Option<String>> {
        match expr {
            Expr::Ident(n) => self.path(n, objects, module),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                let mut ty = self.path(n, objects, module)?;
                if let Some(field) = n.strip_prefix("@record_component:") {
                    if let Some(receiver) = args.first() {
                        if let Some(receiver_type) = self.expr(receiver, objects, module)? {
                            let mut local = Objects::new();
                            local.insert("@receiver".into(), receiver_type);
                            ty = self.path(&format!("@receiver%{field}"), &local, module)?;
                        }
                    }
                } else if !objects.contains_key(n) {
                    self.constructor(n, args, module)?;
                    if self.types.contains_key(n) {
                        ty = Some(n.clone());
                    } else if let Some(result) = self.results.get(n) {
                        ty = Some(result.clone());
                    }
                }
                for a in args {
                    self.expr(a, objects, module)?;
                }
                Ok(ty)
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, objects, module)?;
                self.expr(b, objects, module)?;
                Ok(None)
            }
            Expr::Un(_, a) => {
                self.expr(a, objects, module)?;
                Ok(None)
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                let ty = self.path(name, objects, module)?;
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expr(e, objects, module)?;
                }
                Ok(ty)
            }
            _ => Ok(None),
        }
    }
    fn transfer_type(
        &self,
        ty: &str,
        module: Option<&str>,
        seen: &mut HashSet<String>,
    ) -> Result<()> {
        if !seen.insert(ty.into()) {
            return Ok(());
        }
        if let Some(def) = self.types.get(ty) {
            if let Some(parent) = &def.parent {
                self.transfer_type(parent, module, seen)?;
            }
            for field in &def.fields {
                self.accessible(def, field, module)?;
                if let Some(nested) = &field.derived {
                    self.transfer_type(nested, module, seen)?;
                }
            }
        }
        Ok(())
    }
    fn transfer(&self, items: &[Expr], objects: &Objects, module: Option<&str>) -> Result<()> {
        for item in items {
            if let Some(ty) = self.expr(item, objects, module)? {
                self.transfer_type(&ty, module, &mut HashSet::new())?;
            }
        }
        Ok(())
    }
    fn body(
        &self,
        body: &mut [Stmt],
        inherited: &Objects,
        module: Option<&str>,
        initial: Option<usize>,
    ) -> Result<()> {
        let mut objects = inherited.clone();
        // A local intrinsic declaration shadows an enclosing derived object.
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        objects.remove(n);
                    }
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        objects.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    objects.remove(name);
                }
                Stmt::DerivedDecl {
                    names, type_name, ..
                } => {
                    for n in names {
                        objects.insert(n.clone(), type_name.clone());
                    }
                }
                _ => {}
            }
        }
        // TYPEOF/CLASSOF still carry their source expression at this stage.
        // Preserve its derived identity and check access to the reference too.
        let mut position = initial;
        for s in body.iter() {
            if let Stmt::SourceLocation(p) = s {
                position = Some(*p);
            }
            let (kind, names): (Option<&TypeSpec>, Vec<&String>) = match s {
                Stmt::VarDecl { kind, names } => (Some(kind), names.iter().collect()),
                Stmt::ArrayDecl { kind, name, .. } => (Some(kind), vec![name]),
                Stmt::InitializedDecl { kind, entities, .. } => {
                    (Some(kind), entities.iter().map(|(n, _)| n).collect())
                }
                _ => (None, vec![]),
            };
            if let Some(TypeSpec::Referenced { source, .. }) = kind {
                let ty = self
                    .expr(source, &objects, module)
                    .map_err(|e| crate::source_diagnostics::at(e, position))?;
                for name in names {
                    if let Some(ty) = &ty {
                        objects.insert(name.clone(), ty.clone());
                    }
                }
            }
        }
        let mut location = initial;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                location = Some(*p);
            }
            let result = (|| -> Result<()> {
                match s {
                    Stmt::Module { name, body } => {
                        return self.body(body, &Objects::new(), Some(name), location)
                    }
                    Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                        let mut host = objects.clone();
                        for p in params {
                            host.remove(p);
                        }
                        return self.body(body, &host, module, location);
                    }
                    Stmt::SelectCase {
                        expr,
                        cases,
                        default,
                    } if crate::select_type::selection(expr).is_some() => {
                        let (selector, alias) = crate::select_type::selection(expr).unwrap();
                        let declared = self.path(selector, &objects, module)?;
                        for case in cases {
                            let mut local = objects.clone();
                            if let Some(ty) = case.items.iter().find_map(|item| match item {
                                CaseItem::Single(Expr::Call(n, args))
                                    if n == "@type_is" || n == "@class_is" =>
                                {
                                    args.first().and_then(|a| {
                                        if let Expr::Ident(t) = a {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    })
                                }
                                _ => None,
                            }) {
                                local.insert(alias.into(), ty.clone());
                            }
                            self.body(&mut case.body, &local, module, location)?;
                        }
                        if let Some(body) = default {
                            let mut local = objects.clone();
                            if let Some(ty) = declared {
                                local.insert(alias.into(), ty);
                            }
                            self.body(body, &local, module, location)?;
                        }
                        return Ok(());
                    }
                    Stmt::VarDecl {
                        kind: TypeSpec::CharacterLength(e),
                        ..
                    }
                    | Stmt::ArrayDecl {
                        kind: TypeSpec::CharacterLength(e),
                        ..
                    }
                    | Stmt::InitializedDecl {
                        kind: TypeSpec::CharacterLength(e),
                        ..
                    } => {
                        self.expr(e, &objects, module)?;
                    }
                    Stmt::DerivedType { components, .. } => {
                        // Defaults are evaluated in the defining module. Do not
                        // treat the component declarations as module objects.
                        for component in components {
                            for e in crate::procedure_arguments::expressions(component) {
                                self.expr(e, &objects, module)?;
                            }
                        }
                        return Ok(());
                    }
                    Stmt::Print { items } | Stmt::Read { args: items } => {
                        self.transfer(items, &objects, module)?;
                    }
                    Stmt::CallSub { name, args } => {
                        self.path(name, &objects, module)?;
                        let first =
                            crate::input::transfer_start(name).or_else(|| match name.as_str() {
                                "@formatted_print" | "@formatted_write_noadvance" => Some(1),
                                "@write" => Some(2),
                                "@unformatted_write" => Some(4),
                                _ => None,
                            });
                        if let Some(first) = first {
                            self.transfer(&args[first.min(args.len())..], &objects, module)?;
                        }
                    }
                    Stmt::Assign { name, .. }
                    | Stmt::AssignIndex { name, .. }
                    | Stmt::Do { var: name, .. } => {
                        self.path(name, &objects, module)?;
                    }
                    _ => {}
                }
                for e in crate::procedure_arguments::expressions(s) {
                    self.expr(e, &objects, module)?;
                }
                for b in crate::procedure_arguments::children(s) {
                    self.body(b, &objects, module, location)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
}

fn clear(body: &mut [Stmt]) {
    for s in body {
        if let Stmt::DerivedType {
            components,
            bindings,
            ..
        } = s
        {
            bindings.retain(|(n, _)| {
                n != "@binding_default_access" && !n.starts_with("@binding_access:")
            });
            components.retain(|s| !matches!(s, Stmt::Accessibility { .. }));
            for component in components {
                if let Stmt::InitializedDecl { attributes, .. }
                | Stmt::DerivedDecl { attributes, .. } = component
                {
                    attributes.retain(|a| a != "public" && a != "private");
                }
            }
        } else {
            for b in crate::procedure_arguments::children(s) {
                clear(b);
            }
        }
    }
}

pub(crate) fn validate(program: &mut Program) -> Result<()> {
    let mut check = Validator::default();
    check.collect(&mut program.body, None, false)?;
    for (ty, parent) in &check.parents {
        if let Some(def) = check.types.get_mut(ty) {
            def.parent = Some(parent.clone());
        }
    }
    if check
        .types
        .values()
        .any(|d| d.fields.iter().chain(&d.bindings).any(|f| f.private))
    {
        check.body(&mut program.body, &Objects::new(), None, None)?;
    }
    clear(&mut program.body);
    Ok(())
}

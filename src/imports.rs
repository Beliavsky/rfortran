// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Check ONLY association before lowering erases module/type declarations.
//! Module association subsequently assigns each defining entity a scoped identity.
use crate::ast::{CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

type Symbols = HashMap<String, String>; // local spelling -> defining module/name
type Modules = HashMap<String, Symbols>;

fn declared(body: &[Stmt]) -> HashSet<String> {
    let mut names = HashSet::new();
    for s in body {
        match s {
            Stmt::VarDecl { names: ns, .. } | Stmt::DerivedDecl { names: ns, .. } => {
                names.extend(ns.iter().cloned())
            }
            Stmt::InitializedDecl { entities, .. } => {
                names.extend(entities.iter().map(|(n, _)| n.clone()))
            }
            Stmt::ArrayDecl { name, .. }
            | Stmt::DerivedType { name, .. }
            | Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::GenericInterface { name, .. } => {
                names.insert(name.clone());
            }
            Stmt::OperatorInterface { operator, .. } => {
                names.insert(format!("operator({operator})"));
            }
            _ => {}
        }
    }
    names
}

fn imported(body: &[Stmt], modules: &Modules) -> Result<Symbols> {
    let mut selected = Symbols::new();
    for s in body {
        if let Stmt::Use { module, only, renames, .. } = s {
            let Some(exports) = modules.get(module) else {
                if only.is_some() {
                    bail!("USE ONLY requires module `{module}` earlier in the same source group");
                }
                continue; // Preserve the existing separate-object path for unrestricted USE.
            };
            if let Some(names) = only {
                for name in names {
                    if !exports.contains_key(name) {
                        bail!("`{name}` is not a public entity of module `{module}` in USE ONLY");
                    }
                }
            }
            for (_,remote) in renames {if !exports.contains_key(remote) {bail!("`{remote}` is not a public entity of module `{module}` in USE");}}
            for (remote, origin) in exports {
                if only.as_ref().is_some_and(|names| !names.contains(remote)) {
                    continue;
                }
                let mut aliases=renames.iter().filter_map(|(local,n)|(n==remote).then_some(local)).collect::<Vec<_>>();
                if aliases.is_empty() || only.as_ref().is_some_and(|names|names.iter().filter(|n|*n==remote).count()>aliases.len()) {aliases.push(remote);}
                for name in aliases {
                if let Some(previous) = selected.insert(name.clone(), origin.clone()) {
                    if previous != *origin
                        && !name.starts_with("operator(")
                        && !(previous.starts_with("generic:") && origin.starts_with("generic:"))
                    {
                        bail!("ambiguous imported name `{name}` from `{previous}` and `{origin}`");
                    }
                }}
            }
        }
    }
    Ok(selected)
}

struct Validator {
    modules: Modules,
    module_names: HashSet<String>,
}
impl Validator {
    fn name(&self, name: &str, visible: &HashSet<String>) -> Result<()> {
        let root = name.split('%').next().unwrap_or(name);
        if self.module_names.contains(root) && !visible.contains(root) {
            bail!("module entity `{root}` is not accessible in this scope; check USE ONLY and PUBLIC/PRIVATE");
        }
        Ok(())
    }
    fn kind(&self, kind: &TypeSpec, visible: &HashSet<String>) -> Result<()> {
        if let TypeSpec::RealKind(n) | TypeSpec::IntegerKind(n) = kind {
            self.name(n, visible)?;
        }
        Ok(())
    }
    fn expression(&self, expr: &Expr, visible: &HashSet<String>) -> Result<()> {
        match expr {
            Expr::Ident(n) => self.name(n, visible)?,
            Expr::RealLit(s) => {
                if let Some((_, k)) = s.split_once('_') {
                    self.name(k, visible)?;
                }
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                self.name(n, visible)?;
                for e in args {
                    self.expression(e, visible)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, visible)?;
                self.expression(b, visible)?;
            }
            Expr::Un(_, a) => self.expression(a, visible)?,
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                self.name(name, visible)?;
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, visible)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&self, body: &[Stmt], inherited: &HashSet<String>) -> Result<()> {
        let locals = declared(body);
        let imports = imported(body, &self.modules)?;
        for n in imports.keys() {
            if locals.contains(n)
                && !(imports[n].starts_with("generic:")
                    && body
                        .iter()
                        .any(|s| matches!(s, Stmt::GenericInterface { name, .. } if name == n)))
            {
                bail!("local declaration `{n}` conflicts with a USE-associated entity");
            }
        }
        let mut visible = inherited.clone();
        visible.extend(locals);
        visible.extend(imports.into_keys());
        for s in body {
            match s {
                Stmt::Module { body, .. } => self.body(body, &HashSet::new())?,
                Stmt::Function { params, body, .. } => {
                    let mut local = visible.clone();
                    local.extend(params.iter().cloned());
                    self.body(body, &local)?;
                }
                Stmt::Subroutine { params, body, .. } => {
                    let mut local = visible.clone();
                    local.extend(params.iter().cloned());
                    self.body(body, &local)?;
                }
                Stmt::VarDecl { kind, .. } => self.kind(kind, &visible)?,
                Stmt::InitializedDecl { kind, entities, .. } => {
                    self.kind(kind, &visible)?;
                    for (_, value) in entities {
                        if let Some(e) = value {
                            self.expression(e, &visible)?;
                        }
                    }
                }
                Stmt::ArrayDecl { kind, dims, .. } => {
                    self.kind(kind, &visible)?;
                    for e in dims {
                        self.expression(e, &visible)?;
                    }
                }
                Stmt::DerivedDecl { type_name, .. } => self.name(type_name, &visible)?,
                Stmt::DerivedType {
                    components,
                    bindings,
                    ..
                } => {
                    self.body(components, &visible)?;
                    for (_, procedure) in bindings {
                        self.name(procedure, &visible)?;
                    }
                }
                Stmt::OperatorInterface { procedures, .. }
                | Stmt::GenericInterface { procedures, .. } => {
                    for n in procedures {
                        self.name(n, &visible)?;
                    }
                }
                Stmt::Assign { name, value } => {
                    self.name(name, &visible)?;
                    self.expression(value, &visible)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.name(name, &visible)?;
                    self.expression(value, &visible)?;
                    for e in indices {
                        self.expression(e, &visible)?;
                    }
                }
                Stmt::Print { items }
                | Stmt::Read { args: items }
                | Stmt::CallSub { args: items, .. } => {
                    if let Stmt::CallSub { name, .. } = s {
                        self.name(name, &visible)?;
                    }
                    for e in items {
                        self.expression(e, &visible)?;
                    }
                }
                Stmt::Return(Some(e)) => self.expression(e, &visible)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &visible)?;
                    self.body(then_body, &visible)?;
                    if let Some(b) = else_body {
                        self.body(b, &visible)?;
                    }
                }
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    self.name(var, &visible)?;
                    self.expression(start, &visible)?;
                    self.expression(end, &visible)?;
                    self.body(body, &visible)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &visible)?;
                    self.body(body, &visible)?;
                }
                Stmt::Block { body } => self.body(body, &visible)?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &visible)?;
                    for case in cases {
                        for item in &case.items {
                            match item {
                                CaseItem::Single(e) => self.expression(e, &visible)?,
                                CaseItem::Range(a, b) => {
                                    self.expression(a, &visible)?;
                                    self.expression(b, &visible)?;
                                }
                            }
                        }
                        self.body(&case.body, &visible)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &visible)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn validate(program: &Program) -> Result<()> {
    fn has_only(body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::Use { only: Some(_), .. } | Stmt::GenericInterface { .. } => true,
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => has_only(body),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => has_only(then_body) || else_body.as_ref().is_some_and(|b| has_only(b)),
            Stmt::SelectCase { cases, default, .. } => {
                cases.iter().any(|c| has_only(&c.body))
                    || default.as_ref().is_some_and(|b| has_only(b))
            }
            _ => false,
        })
    }
    if !has_only(&program.body) {
        return Ok(());
    }
    let mut validator = Validator {
        modules: Modules::new(),
        module_names: HashSet::new(),
    };
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let local = declared(body);
            validator.module_names.extend(local.iter().cloned());
            let mut symbols = imported(body, &validator.modules)?;
            for n in local {
                let generic = body
                    .iter()
                    .any(|s| matches!(s, Stmt::GenericInterface { name, .. } if name == &n));
                symbols.insert(
                    n.clone(),
                    format!("{}{name}::{n}", if generic { "generic:" } else { "" }),
                );
            }
            let mut default_public = true;
            let mut access = HashMap::new();
            for s in body {
                match s {
                    Stmt::Accessibility { public, names } => {
                        if names.is_empty() {
                            default_public = *public;
                        }
                        for n in names {
                            access.insert(n.clone(), *public);
                        }
                    }
                    Stmt::InitializedDecl {
                        attributes,
                        entities,
                        ..
                    } => {
                        for attr in attributes {
                            if attr == "public" || attr == "private" {
                                for (n, _) in entities {
                                    access.insert(n.clone(), attr == "public");
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            symbols.retain(|n, _| access.get(n).copied().unwrap_or(default_public));
            validator.modules.insert(name.clone(), symbols);
        }
    }
    validator.body(&program.body, &HashSet::new())
}

#[cfg(test)]
mod tests {
    fn check(source: &str) -> anyhow::Result<crate::ast::Program> {
        let tokens = crate::lexer::lex(source);
        let program = crate::parser::parse_with_src(source, &tokens, "only.f90").unwrap();
        crate::derived::resolve(program)
    }

    const MODULE: &str = "module m\nimplicit none\nprivate\npublic :: n, f, box\ninteger, parameter :: n=4, hidden=7\ntype :: box\ninteger :: v\nend type box\ncontains\ninteger function f(i)\ninteger :: i\nf=i+hidden\nend function f\nend module m\n";

    #[test]
    fn selected_imports_and_reexports() {
        let p = check(include_str!("../tests/60_use_only.f90")).unwrap();
        crate::parameters::resolve(p).unwrap();
        check(&format!("{MODULE}use m, only: n\nimplicit none\ninteger :: hidden\nhidden=8\nprint *, hidden,n\nend")).unwrap();
    }

    #[test]
    fn missing_private_and_excluded_entities_are_rejected() {
        for (main, expected) in [
            ("use m, only: absent\nend", "not a public entity"),
            ("use m, only: hidden\nend", "not a public entity"),
            (
                "use m, only: n\nimplicit none\nprint *, f(2)\nend",
                "`f` is not accessible",
            ),
            (
                "use m, only: f\nimplicit none\nprint *, n\nend",
                "`n` is not accessible",
            ),
            (
                "use m, only: n\nimplicit none\ntype(box) :: b\nend",
                "`box` is not accessible",
            ),
            (
                "use m, only: n\ninteger :: n\nend",
                "conflicts with a USE-associated",
            ),
        ] {
            let err = check(&format!("{MODULE}{main}")).unwrap_err().to_string();
            assert!(err.contains(expected), "{main}: {err}");
        }
    }

    #[test]
    fn selected_operators_are_visible() {
        let source = include_str!("../tests/56_derived_operators.f90");
        let selected = source.replace("use year_month_mod", "use year_month_mod, only: year_month, operator(+), operator(-), operator(<), operator(<=), operator(==), operator(>=), operator(>)");
        check(&selected).unwrap();
        let excluded = source.replace("use year_month_mod", "use year_month_mod, only: year_month");
        assert!(check(&excluded).is_err());
    }

    #[test]
    fn duplicate_association_and_local_only_identifier() {
        check(&format!("{MODULE}module bridge\nuse m, only: n\nend module bridge\nuse m, only: n\nuse bridge, only: n\ninteger :: only\nonly=n\nprint *, only\nend")).unwrap();
        let err = check(&format!("{MODULE}module other\ninteger, parameter :: n=5\nend module other\nuse m, only: n\nuse other, only: n\nprint *, n\nend")).unwrap_err().to_string();
        assert!(err.contains("ambiguous imported name `n`"), "{err}");
    }
}

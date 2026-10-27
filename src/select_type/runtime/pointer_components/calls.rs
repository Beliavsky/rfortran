// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Explicit-interface specialization carries a component's target environment
//! by reference alongside its identity tag. The environment is private to a
//! call specialization; it is not an external ABI or owning target storage.
use super::*;

pub(super) const ENVIRONMENT: &str = "@pointer_component_environment";

fn owner(defs: &Definitions, kind: &str) -> Result<bool> {
    let mut fields = Members::new();
    members(defs, kind, "", 0, false, &mut fields, &mut HashSet::new())?;
    Ok(!fields.is_empty())
}

fn collect(
    body: &mut Vec<Stmt>,
    defs: &Definitions,
    templates: &mut HashMap<String, (Stmt, String)>,
    host: &str,
) -> Result<()> {
    let mut kept = vec![];
    for mut s in std::mem::take(body) {
        if let Stmt::Subroutine {
            name, params, body, ..
        }
        | Stmt::Function {
            name, params, body, ..
        } = &s
        {
            let vars = super::super::super::objects(body, &Objects::new());
            let mut owns = false;
            for p in params {
                if let Some(v) = vars.get(p) {
                    owns |= owner(defs, &v.kind)?;
                }
            }
            if owns {
                templates.insert(name.clone(), (s, host.into()));
                continue;
            }
        }
        if !matches!(s, Stmt::DerivedType { .. }) {
            let nested = match &s {
                Stmt::Function { name, .. }
                | Stmt::Subroutine { name, .. }
                | Stmt::Module { name, .. } => name.clone(),
                _ => host.into(),
            };
            for child in children(&mut s) {
                collect(child, defs, templates, &nested)?;
            }
        }
        kept.push(s);
    }
    *body = kept;
    Ok(())
}

fn substitute(body: &mut [Stmt], from: &str, to: &str) {
    fn name(n: &mut String, from: &str, to: &str) {
        if n == from || n.strip_prefix(from).is_some_and(|s| s.starts_with('%')) {
            *n = format!("{to}{}", &n[from.len()..]);
        }
    }
    for s in body {
        if let Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } = s {
            if params.iter().any(|n| n == from)
                || super::super::super::objects(body, &Objects::new()).contains_key(from)
            {
                continue;
            }
        }
        // Retain formal declarations: the original actual still satisfies the
        // user interface. Executable target references use the canonical alias.
        match s {
            Stmt::Assign { name: n, .. }
            | Stmt::AssignIndex { name: n, .. }
            | Stmt::Do { var: n, .. } => name(n, from, to),
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
        for child in children(s) {
            substitute(child, from, to);
        }
    }
}

struct Calls {
    templates: HashMap<String, (Stmt, String)>,
    generated: HashMap<String, Vec<Stmt>>,
    internal: HashSet<String>,
    new_internal: Vec<String>,
    serial: usize,
    stack: Vec<String>,
}
impl Calls {
    /// Trace association operands through explicit interfaces. Unrelated TARGET
    /// declarations must not become dispatch alternatives (or value reads).
    fn referenced_targets(
        &self,
        body: &[Stmt],
        names: &HashMap<String, String>,
        stack: &mut HashSet<String>,
        out: &mut HashSet<String>,
    ) -> Result<()> {
        let resolve = |n: &str| names.get(n).cloned().unwrap_or_else(|| n.into());
        let mut call = |name: &str, args: &[Expr]| -> Result<()> {
            if name == "@pointer_associate" {
                if let Some(Expr::Ident(n)) = args.get(1) {
                    out.insert(resolve(n));
                }
            }
            if let Some((template, _)) = self.templates.get(name) {
                if stack.insert(name.into()) {
                    let (params, body) = match template {
                        Stmt::Function { params, body, .. }
                        | Stmt::Subroutine { params, body, .. } => (params, body),
                        _ => unreachable!(),
                    };
                    let mut args = args.to_vec();
                    crate::named_arguments::Signature::new(params, body)
                        .reorder(name, &mut args)?;
                    let mut bindings = names.clone();
                    for n in super::super::super::objects(body, &Objects::new()).keys() {
                        bindings.remove(n);
                    }
                    for (p, a) in params.iter().zip(args) {
                        if let Expr::Ident(n) = a {
                            bindings.insert(p.clone(), resolve(&n));
                        }
                    }
                    self.referenced_targets(body, &bindings, stack, out)?;
                    stack.remove(name);
                }
            }
            Ok(())
        };
        for s in body {
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                continue;
            }
            if let Stmt::CallSub { name, args } = s {
                call(name, args)?;
            }
            let mut s = s.clone();
            for e in expressions(&mut s) {
                let mut error = None;
                visit_expr(e, &mut |e| {
                    if let Expr::Call(name, args) = e {
                        if let Err(e) = call(name, args) {
                            error = Some(e);
                        }
                    }
                });
                if let Some(e) = error {
                    return Err(e);
                }
            }
        }
        for s in body {
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                continue;
            }
            for child in children(&mut s.clone()) {
                self.referenced_targets(child, names, stack, out)?;
            }
        }
        Ok(())
    }
    fn call(
        &mut self,
        name: &mut String,
        args: &mut Vec<Expr>,
        targets: &[(String, String)],
        host: &str,
        locals: &HashSet<String>,
    ) -> Result<()> {
        let Some((template, destination)) = self.templates.get(name).cloned() else {
            return Ok(());
        };
        if self.stack.contains(name) {
            bail!("recursive procedures with polymorphic POINTER components require a persistent component descriptor interface");
        }
        let original = name.clone();
        let mut procedure = template;
        let function = matches!(procedure, Stmt::Function { .. });
        let readonly = matches!(procedure, Stmt::Function { pure: true, .. });
        let (clone_name, params, body) = match &mut procedure {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => (name, params, body),
            _ => unreachable!(),
        };
        crate::named_arguments::Signature::new(params, body).reorder(name, args)?;
        let vars = super::super::super::objects(body, &Objects::new());
        let host_references: HashSet<_> = targets
            .iter()
            .filter(|(_, n)| !vars.contains_key(n) && body.iter().any(|s| uses(s, n)))
            .map(|(_, n)| n.clone())
            .collect();
        let mut invoked = HashSet::new();
        crate::procedure_arguments::invoked(body, &mut invoked);
        if invoked.contains(&original) {
            bail!("recursive procedures with polymorphic POINTER components require a persistent component descriptor interface");
        }
        self.serial += 1;
        *clone_name = format!("@pointer_owner_call{}", self.serial);
        // Functions without RESULT use their original name as result storage.
        crate::procedure_arguments::rename(body, &original, clone_name);
        if function && !matches!(body.last(), Some(Stmt::Return(Some(_)))) {
            body.push(Stmt::Return(Some(id(clone_name))));
        }
        let hidden: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(i, _)| format!("@pointer_owner_target{i:08}"))
            .collect();
        for (p, actual) in params.iter().zip(args.iter()) {
            if vars
                .get(p)
                .is_some_and(|v| v.attrs.iter().any(|a| a == "target"))
            {
                if let Expr::Ident(n) = actual {
                    if let Some(i) = targets.iter().position(|(_, t)| t == n) {
                        substitute(body, p, &hidden[i]);
                    }
                }
            }
        }
        for ((_, target), hidden) in targets.iter().zip(&hidden) {
            if host_references.contains(target) {
                if locals.contains(target) && destination != host {
                    bail!("shadowed host TARGET storage in an owner procedure requires a persistent component descriptor interface");
                }
                substitute(body, target, hidden);
            }
        }
        for ((kind, target), hidden) in targets.iter().zip(&hidden) {
            params.push(hidden.clone());
            args.push(id(target));
            body.insert(
                0,
                Stmt::DerivedDecl {
                    type_name: kind.clone(),
                    names: vec![hidden.clone()],
                    attributes: vec![
                        "target".into(),
                        if readonly {
                            "intent(in)"
                        } else {
                            "intent(inout)"
                        }
                        .into(),
                        "@referenced_type".into(),
                    ],
                    polymorphic: false,
                    dims: vec![],
                },
            );
        }
        body.insert(
            0,
            Stmt::CallSub {
                name: ENVIRONMENT.into(),
                args: hidden.iter().map(|n| id(n)).collect(),
            },
        );
        *name = clone_name.clone();
        self.stack.push(original.clone());
        self.scope(body, &Declarations::new(), params, clone_name)?;
        self.stack.pop();
        if self.internal.contains(&original) {
            self.new_internal.push(clone_name.clone());
        }
        self.generated
            .entry(destination)
            .or_default()
            .push(procedure);
        Ok(())
    }
    fn statements(
        &mut self,
        body: &mut [Stmt],
        targets: &[(String, String)],
        host: &str,
        locals: &HashSet<String>,
    ) -> Result<()> {
        let mut location = None;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                location = Some(*p);
            }
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                continue;
            }
            (|| -> Result<()> {
                if let Stmt::CallSub { name, args } = s {
                    self.call(name, args, targets, host, locals)?;
                }
                for e in expressions(s) {
                    let mut error = None;
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(name, args) = e {
                            if let Err(e) = self.call(name, args, targets, host, locals) {
                                error = Some(e);
                            }
                        }
                    });
                    if let Some(e) = error {
                        return Err(e);
                    }
                }
                for child in children(s) {
                    self.statements(child, targets, host, locals)?;
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
    fn scope(
        &mut self,
        body: &mut Vec<Stmt>,
        parent: &Declarations,
        params: &[String],
        host: &str,
    ) -> Result<()> {
        let declared = declarations(body, parent);
        let environment = body.iter().find_map(|s| match s {
            Stmt::CallSub { name, args } if name == ENVIRONMENT => Some(args),
            _ => None,
        });
        let mut referenced = HashSet::new();
        if environment.is_none() {
            self.referenced_targets(body, &HashMap::new(), &mut HashSet::new(), &mut referenced)?;
        }
        let mut targets = vec![];
        for (n, ds) in &declared {
            let has = |attribute: &str| {
                ds.iter().any(|s| match s {
                    Stmt::DerivedDecl { attributes, .. } => {
                        attributes.iter().any(|a| a == attribute)
                    }
                    Stmt::DummyIntent { intent, .. } => intent == attribute,
                    _ => false,
                })
            };
            for s in ds {
                if let Stmt::DerivedDecl {
                    type_name,
                    dims,
                    polymorphic: false,
                    ..
                } = s
                {
                    if dims.is_empty() && has("target") && !has("pointer") && !has("allocatable") {
                        if environment
                            .map_or_else(|| referenced.contains(n), |args| args.contains(&id(n)))
                        {
                            targets.push((type_name.clone(), n.clone()));
                        } else if environment.is_some()
                            && !parent.contains_key(n)
                            && !params.contains(n)
                        {
                            bail!("local TARGET storage in a procedure with polymorphic POINTER component dummies is not yet supported");
                        }
                    }
                }
            }
        }
        for s in body.iter_mut() {
            match s {
                Stmt::Function {
                    name, body, params, ..
                }
                | Stmt::Subroutine {
                    name, body, params, ..
                } => self.scope(body, &declared, params, name)?,
                Stmt::Module { name, body, .. } => self.scope(body, &declared, &[], name)?,
                _ => {}
            }
        }
        let locals = super::super::super::objects(body, &Objects::new())
            .into_keys()
            .collect();
        self.statements(body, &targets, host, &locals)?;
        // Every proxy in this activation uses the same ordered identity table.
        body.insert(
            0,
            Stmt::CallSub {
                name: ENVIRONMENT.into(),
                args: targets.iter().map(|(_, n)| id(n)).collect(),
            },
        );
        Ok(())
    }
}

pub(super) fn prepare(program: &mut Program, defs: &Definitions) -> Result<()> {
    let mut calls = Calls {
        templates: HashMap::new(),
        generated: HashMap::new(),
        internal: program.contains.iter().cloned().collect(),
        new_internal: vec![],
        serial: 0,
        stack: vec![],
    };
    collect(&mut program.body, defs, &mut calls.templates, "")?;
    calls.scope(&mut program.body, &Declarations::new(), &[], "")?;
    fn attach(body: &mut Vec<Stmt>, host: &str, generated: &mut HashMap<String, Vec<Stmt>>) {
        for s in body.iter_mut() {
            match s {
                Stmt::Function { name, body, .. }
                | Stmt::Subroutine { name, body, .. }
                | Stmt::Module { name, body, .. } => attach(body, name, generated),
                _ => {}
            }
        }
        body.extend(generated.remove(host).unwrap_or_default());
    }
    attach(&mut program.body, "", &mut calls.generated);
    if !calls.generated.is_empty() {
        bail!(
            "nested owner-procedure templates require a persistent component descriptor interface"
        );
    }
    program.contains.extend(calls.new_internal);
    Ok(())
}

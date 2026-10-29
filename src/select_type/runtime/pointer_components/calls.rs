// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Explicit-interface specialization borrows automatic targets alongside the
//! identity tag. Persistent targets use a shared static-storage identity table
//! instead; the private environment is not an external ABI or owning storage.
use super::*;

pub(in crate::select_type::runtime) const ENVIRONMENT: &str = "@pointer_component_environment";

fn persistent_actual(e: &Expr, owners: &HashSet<String>) -> bool {
    let n = match e {
        Expr::Ident(n) | Expr::Index(n, _) | Expr::Section { name: n, .. } => n,
        Expr::Call(n, args) if n.starts_with("@record_component:") => {
            return args.first().is_some_and(|e| persistent_actual(e, owners));
        }
        Expr::Call(n, _) => n,
        _ => return false,
    };
    owners.contains(n.split('%').next().unwrap_or(n))
}

pub(super) fn owner(defs: &Definitions, kind: &str) -> Result<bool> {
    let mut fields = Members::new();
    members(defs, kind, "", 0, false, false, &mut fields, &mut HashSet::new())?;
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
                if matches!(
                    s,
                    Stmt::Function { pure: true, .. } | Stmt::Subroutine { pure: true, .. }
                ) && params.iter().any(|p| {
                    vars.get(p).is_some_and(|v| {
                        v.polymorphic && v.attrs.iter().any(|a| a == "intent(out)")
                    })
                }) {
                    bail!("PURE procedure cannot have a polymorphic INTENT(OUT) dummy");
                }
                let declaration = crate::imports::declaration(name.clone());
                templates.insert(name.clone(), (s, host.into()));
                kept.push(declaration);
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

pub(super) fn substitute(body: &mut [Stmt], from: &str, to: &str) {
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
            Stmt::CallSub { name: n, .. } if n.strip_prefix(from).is_some_and(|s| s.starts_with('%')) => name(n, from, to),
            _ => {}
        }
        for e in expressions(s) {
            visit_expr(e, &mut |e| match e {
                Expr::Call(n, args) if n == "present" => {
                    // Presence belongs to the original OPTIONAL interface,
                    // not to the always-present borrowed target environment.
                    for arg in args {
                        visit_expr(arg, &mut |e| {
                            if matches!(e, Expr::Ident(n) if n == to) { *e = id(from); }
                        });
                    }
                }
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
    persistent: Declarations,
    owners: Declarations,
    interfaces: Option<crate::operators::OwnerInterfaces>,
    owner_kinds: HashSet<String>,
    templates: HashMap<String, (Stmt, String)>,
    generated: HashMap<String, Vec<Stmt>>,
    internal: HashSet<String>,
    new_internal: Vec<String>,
    serial: usize,
    stack: Vec<String>,
}
impl Calls {
    fn concrete(
        &self,
        body: &mut Vec<Stmt>,
        params: &[String],
        args: &[Expr],
        scope: &crate::operators::OwnerScope,
    ) -> Result<()> {
        let Some(interfaces) = &self.interfaces else {
            return Ok(());
        };
        let mut out = vec![];
        for s in std::mem::take(body) {
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic: true,
                dims,
            } = &s
            {
                if self.owner_kinds.contains(type_name) {
                    for name in names {
                        let actual = params
                            .iter()
                            .position(|p| p == name)
                            .and_then(|i| args.get(i));
                        if let Some(actual) = actual.filter(|a| !crate::named_arguments::omitted(a))
                        {
                            if interfaces.dynamic(actual, scope) {
                                bail!("runtime polymorphic owners require a persistent component descriptor interface");
                            }
                            let Some((kind, rank)) = interfaces.derived(actual, scope)? else {
                                bail!("owner dummy `{name}` requires a derived-type actual");
                            };
                            if rank != dims.len() || !interfaces.extends(&kind, type_name) {
                                bail!("owner actual does not match CLASS dummy `{name}`");
                            }
                            let mut attributes = attributes.clone();
                            attributes.push("@referenced_type".into());
                            out.push(Stmt::DerivedDecl {
                                type_name: kind,
                                names: vec![name.clone()],
                                attributes,
                                polymorphic: false,
                                dims: dims.clone(),
                            });
                        } else {
                            out.push(Stmt::DerivedDecl {
                                type_name: type_name.clone(),
                                names: vec![name.clone()],
                                attributes: attributes.clone(),
                                polymorphic: true,
                                dims: dims.clone(),
                            });
                        }
                    }
                    continue;
                }
            }
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    /// Trace association operands through explicit interfaces. Unrelated TARGET
    /// declarations must not become dispatch alternatives (or value reads).
    fn referenced_targets(
        &self,
        body: &[Stmt],
        names: &HashMap<String, String>,
        stack: &mut HashSet<String>,
        out: &mut HashSet<String>,
        scope: &crate::operators::OwnerScope,
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
                    for (p, a) in params.iter().zip(args.iter()) {
                        if let Expr::Ident(n) = a {
                            bindings.insert(p.clone(), resolve(&n));
                        }
                    }
                    let mut source = body.clone();
                    self.concrete(&mut source, params, &args, scope)?;
                    let nested = self
                        .interfaces
                        .as_ref()
                        .map(|r| r.scope(&source, name))
                        .unwrap_or_default();
                    if let Some(r) = &self.interfaces {
                        r.normalize(&mut source, &nested)?;
                    }
                    self.referenced_targets(&source, &bindings, stack, out, &nested)?;
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
                self.referenced_targets(child, names, stack, out, scope)?;
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
        scope: &crate::operators::OwnerScope,
        persistent_owners: &HashSet<String>,
        epochs: &HashMap<String, allocated::Info>,
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
        // Hidden required arguments follow the source arguments, including
        // explicit placeholders for trailing absent OPTIONAL dummies.
        args.resize_with(params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        self.concrete(body, params, args, scope)?;
        let vars = super::super::super::objects(body, &Objects::new());
        for (p, actual) in params.iter().zip(args.iter()) {
            if vars.get(p).is_some_and(|v| self.owner_kinds.contains(&v.kind))
                && persistent_actual(actual, persistent_owners) {
                body.push(Stmt::DummyIntent { name: p.clone(), intent: persistent::OWNER.into() });
            }
        }
        if let Some(interfaces) = &self.interfaces {
            for (p, actual) in params.iter().zip(args.iter()) {
                if vars
                    .get(p)
                    .is_some_and(|v| self.owner_kinds.contains(&v.kind))
                    && interfaces.dynamic(actual, scope)
                {
                    bail!("runtime polymorphic owners require a persistent component descriptor interface");
                }
                if vars.get(p).is_some_and(|v| {
                    self.owner_kinds.contains(&v.kind)
                        && v.attrs
                            .iter()
                            .any(|a| a == "intent(out)" || a == "intent(inout)")
                }) && interfaces.readonly(actual, scope)
                {
                    bail!("cannot pass INTENT(IN) owner to writable dummy `{p}`");
                }
            }
        }
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
        let persistent: Vec<_> = targets
            .iter()
            .filter(|(_, n)| self.persistent.contains_key(n))
            .cloned()
            .collect();
        for (p, actual) in params.iter().zip(args.iter()) {
            if vars.get(p).is_some_and(|v| v.attrs.iter().any(|a| a == "target")) {
                if let Expr::Ident(n) = actual {
                    if self.persistent.contains_key(n) {
                        substitute(body, p, n);
                    }
                }
            }
        }
        let targets: Vec<_> = targets
            .iter()
            .filter(|(_, n)| !self.persistent.contains_key(n))
            .cloned()
            .collect();
        let hidden: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(i, _)| format!("@pointer_owner_target{i:08}"))
            .collect();
        // Component views of a TARGET owner dummy name the same allocation
        // as the caller's view. Use that canonical borrowed target rather
        // than treating the callee's synthetic view as local TARGET storage.
        let mut component_aliases = vec![];
        let component_views=declarations(body,&Declarations::new());
        for (p, actual) in params.iter().zip(args.iter()) {
            let Expr::Ident(actual) = actual else { continue; };
            if !vars.get(p).is_some_and(|v| v.attrs.iter().any(|a| a == "target")) {
                continue;
            }
            for (alias, v) in &vars {
                if let Some(array)=component_views.get(alias).and_then(|d|allocated::info(d)).and_then(|i|i.array) {
                    if array.owner==*p {
                        if let Some((target,_))=epochs.iter().find(|(_,i)|i.array.as_ref().is_some_and(|a|a.owner==*actual && a.field==array.field && a.rank==array.rank)) {
                            let canonical=if self.persistent.contains_key(target){Some(target.clone())}else{targets.iter().position(|(_,n)|n==target).map(|i|hidden[i].clone())};
                            if let Some(canonical)=canonical {component_aliases.push((alias.clone(),canonical));}
                        }
                    }
                    continue;
                }
                if !v.attrs.iter().any(|a| a == "@allocated_component_alias") { continue; }
                let Some(backing) = v.attrs.iter().find_map(|a| a.strip_prefix("@allocated_target_storage:")) else { continue; };
                let Some(field) = backing.strip_prefix(p).filter(|s| s.starts_with('%')) else { continue; };
                let backing = format!("{actual}{field}");
                if let Some((target, _)) = epochs.iter().find(|(_, i)| i.backing.as_ref() == Some(&backing)) {
                    let canonical = if self.persistent.contains_key(target) {
                        Some(target.clone())
                    } else {
                        targets.iter().position(|(_, n)| n == target).map(|i| hidden[i].clone())
                    };
                    if let Some(canonical) = canonical { component_aliases.push((alias.clone(), canonical)); }
                }
            }
        }
        for (alias, canonical) in component_aliases {
            substitute(body, &alias, &canonical);
            body.retain(|s| !matches!(s, Stmt::DerivedDecl { names, .. } if names.contains(&alias)));
        }
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
            let mut metadata = vec![];
            let allocation=epochs.get(target);
            if let Some(array) = allocation.and_then(|i| i.array.as_ref()) {
                let formal=format!("{hidden}$owner");
                params.push(formal.clone());args.push(id(&array.owner));
                let mut attrs=vec!["target".into(),"@referenced_type".into(),if readonly || array.readonly {"intent(in)"}else{"intent(inout)"}.into()];
                if array.allocatable {attrs.push("allocatable".into());}
                body.insert(0,Stmt::DerivedDecl{type_name:array.owner_type.clone(),names:vec![formal.clone()],attributes:attrs,polymorphic:false,dims:vec![id("@deferred_extent");array.rank]});
                let mut view=array.clone();view.owner=formal;
                metadata.extend([view.attribute(),allocated::backing_attribute(&format!("{hidden}$storage")),allocated::epoch_attribute(&format!("{hidden}$epoch"))]);
            } else if let Some(allocation) = allocation {
                let formal = format!("{hidden}$epoch");
                params.push(formal.clone()); args.push(id(&allocation.epoch));
                body.insert(0, Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec![formal.clone()]});
                body.insert(0, Stmt::DummyIntent {name:formal.clone(),intent:if readonly {"intent(in)"}else{"intent(inout)"}.into()});
                metadata.push(allocated::epoch_attribute(&formal));
            }
            if allocation.is_some_and(|i|i.array.is_some()) {
                // The owner array is the persistent allocation environment.
            } else if let Some(backing)=allocation.and_then(|i|i.backing.as_ref()) {
                // Pass the descriptor, including an unallocated moved-from
                // variable, without evaluating its nonexistent scalar element.
                let formal=format!("{hidden}$storage");
                metadata.push(allocated::backing_attribute(&formal));
                params.push(formal.clone());args.push(id(backing));
                body.insert(0,Stmt::DerivedDecl{type_name:kind.clone(),names:vec![formal],attributes:vec!["allocatable".into(),"target".into(),"@scalar_heap".into(),"@referenced_type".into(),if readonly {"intent(in)"}else{"intent(inout)"}.into()],polymorphic:false,dims:vec![id("@deferred_extent")]});
            } else {
                params.push(hidden.clone());
                args.push(id(target));
            }
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
                    ].into_iter().chain(metadata).collect(),
                    polymorphic: false,
                    dims: vec![],
                },
            );
        }
        body.insert(
            0,
            Stmt::CallSub {
                name: ENVIRONMENT.into(),
                args: persistent
                    .iter()
                    .map(|(_, n)| id(n))
                    .chain(hidden.iter().map(|n| id(n)))
                    .collect(),
            },
        );
        *name = clone_name.clone();
        self.stack.push(original.clone());
        self.scope(body, &self.persistent.clone(), params, clone_name, &original)?;
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
        scope: &crate::operators::OwnerScope,
        persistent_owners: &HashSet<String>,
        epochs: &HashMap<String, allocated::Info>,
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
                    self.call(name, args, targets, host, locals, scope, persistent_owners, epochs)?;
                }
                for e in expressions(s) {
                    let mut error = None;
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(name, args) = e {
                            if let Err(e) = self.call(name, args, targets, host, locals, scope, persistent_owners, epochs) {
                                error = Some(e);
                            }
                        }
                    });
                    if let Some(e) = error {
                        return Err(e);
                    }
                }
                for child in children(s) {
                    self.statements(child, targets, host, locals, scope, persistent_owners, epochs)?;
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
        origin: &str,
    ) -> Result<()> {
        let scope = self
            .interfaces
            .as_ref()
            .map(|r| r.scope(body, origin))
            .unwrap_or_default();
        if let Some(r) = &self.interfaces {
            r.normalize(body, &scope)?;
        }
        let declared = declarations(body, parent);
        let environment = body.iter().find_map(|s| match s {
            Stmt::CallSub { name, args } if name == ENVIRONMENT => Some(args),
            _ => None,
        });
        let mut referenced = HashSet::new();
        if environment.is_none() {
            self.referenced_targets(
                body,
                &HashMap::new(),
                &mut HashSet::new(),
                &mut referenced,
                &scope,
            )?;
        }
        let mut targets = vec![];
        let local=super::super::super::objects(body,&Objects::new());
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
                    if dims.is_empty() && has("target") && !has("pointer") && (!has("allocatable") || allocated::info(ds).is_some()) {
                        let component_dummy=allocated::info(ds).and_then(|i|i.backing).is_some_and(|b|params.iter().any(|p|b==*p || b.strip_prefix(p).is_some_and(|f|f.starts_with('%'))));
                        if component_dummy || environment.map_or_else(
                            || self.persistent.contains_key(n) || referenced.contains(n) || (local.contains_key(n) && allocated::info(ds).is_some()),
                            |args| args.contains(&id(n)),
                        )
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
                } => self.scope(body, &declared, params, name, name)?,
                Stmt::Module { name, body, .. } => self.scope(body, &declared, &[], name, name)?,
                _ => {}
            }
        }
        let locals = super::super::super::objects(body, &Objects::new())
            .into_keys()
            .collect();
        let mut persistent_owners: HashSet<_> = self.owners.keys().cloned().collect();
        persistent_owners.extend(super::super::super::objects(body, &Objects::new())
            .into_iter().filter(|(_, v)| v.attrs.iter().any(|a| a == persistent::OWNER)).map(|(n, _)| n));
        let epochs = declared.iter().filter_map(|(n,d)| allocated::info(d).map(|i|(n.clone(),i))).collect();
        self.statements(body, &targets, host, &locals, &scope, &persistent_owners, &epochs)?;
        body.extend(self.owners.keys().cloned().map(crate::imports::reference));
        body.extend(self.persistent.keys().cloned().map(crate::imports::reference));
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

pub(super) fn prepare(program: &mut Program, defs: &mut Definitions) -> Result<()> {
    let source = program.clone();
    let mut owner_kinds = HashSet::new();
    for kind in defs.fields.keys() {
        if owner(defs, kind)? {
            owner_kinds.insert(kind.clone());
        }
    }
    let mut calls = Calls {
        persistent: persistent::declarations(program),
        owners: persistent::owners(program, defs)?,
        interfaces: None,
        owner_kinds,
        templates: HashMap::new(),
        generated: HashMap::new(),
        internal: program.contains.iter().cloned().collect(),
        new_internal: vec![],
        serial: 0,
        stack: vec![],
    };
    collect(&mut program.body, defs, &mut calls.templates, "")?;
    let owned: HashSet<_> = calls.templates.keys().cloned().collect();
    if !owned.is_empty() {
        calls.interfaces = Some(crate::operators::OwnerInterfaces::new(
            &source,
            owned.clone(),
            defs.parents.clone(),
        )?);
    }
    let mut globals = calls.persistent.clone();
    globals.extend(calls.owners.clone());
    calls.scope(&mut program.body, &globals, &[], "", "")?;
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
    fn retire(
        body: &mut [Stmt],
        owned: &HashSet<String>,
        interfaces: Option<&crate::operators::OwnerInterfaces>,
    ) {
        for s in body {
            match s {
                Stmt::GenericInterface { procedures, .. } => {
                    procedures.retain(|n| !owned.contains(n))
                }
                Stmt::DerivedType { name, bindings, .. } => bindings.retain(|(key, n)| {
                    let generic = key.strip_prefix("@generic:").or_else(|| {
                        key.strip_prefix("@generic_binding:")
                            .and_then(|s| s.split_once(':'))
                            .map(|(g, _)| g)
                    });
                    !owned.contains(n)
                        && !generic
                            .is_some_and(|g| interfaces.is_some_and(|r| r.owns_generic(name, g)))
                }),
                _ => {}
            }
            for b in children(s) {
                retire(b, owned, interfaces);
            }
        }
    }
    retire(&mut program.body, &owned, calls.interfaces.as_ref());
    for bindings in defs.bindings.values_mut() {
        bindings.retain(|_, n| !owned.contains(n));
    }
    defs.bound_targets.retain(|n| !owned.contains(n));
    program.contains.extend(calls.new_internal);
    Ok(())
}

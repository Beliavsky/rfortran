// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Resolve owner interfaces while their original signatures and bindings still
//! exist. This uses the normal generic matcher and a typed snapshot; it does
//! not run operator lowering on the compiler's source tree prematurely.
use super::*;
use crate::procedure_arguments::{children, expressions, visit_expr};

#[derive(Clone, Default)]
pub(crate) struct OwnerScope {
    vars: Vars,
    declared: Vars,
    visible: Bindings,
    readonly: HashSet<String>,
    dynamic: HashSet<String>,
    constants: HashMap<String, Expr>,
}
pub(crate) struct OwnerInterfaces {
    resolver: Resolver,
    scopes: HashMap<String, OwnerScope>,
    parents: HashMap<String, String>,
    classes: HashMap<String, HashSet<usize>>,
    owned: HashSet<String>,
    dynamic_fields: HashSet<(String, String)>,
    generic_binding_names: HashMap<(String, String), Vec<String>>,
}
impl OwnerInterfaces {
    pub(crate) fn new(
        program: &Program,
        owned: HashSet<String>,
        parents: HashMap<String, String>,
    ) -> Result<Self> {
        let mut snapshot = crate::extensions::resolve(program.clone())?;
        // Constant lowering removes PARAMETER declarations. Retain their
        // folded values (including imported literal-kind aliases) in this
        // private snapshot for argument inference on the original expressions.
        fn retain_constants(body: &mut Vec<Stmt>) {
            let mut names = HashSet::new();
            for s in body.iter_mut() {
                if let Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } = s
                {
                    if attributes.iter().any(|a| a == "parameter") {
                        names.extend(entities.iter().map(|(n, _)| n.clone()));
                    }
                }
                if matches!(
                    s,
                    Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }
                ) {
                    for b in children(s) {
                        retain_constants(b);
                    }
                } else {
                    for e in expressions(s) {
                        visit_expr(e, &mut |e| {
                            if let Expr::IntLit(n) | Expr::RealLit(n) = e {
                                if let Some((_, kind)) = n.split_once('_') {
                                    if kind.parse::<u8>().is_err() {
                                        names.insert(kind.into());
                                    }
                                }
                            }
                        });
                    }
                }
            }
            let mut names: Vec<_> = names.into_iter().collect();
            names.sort();
            let at = body.len() - usize::from(matches!(body.last(), Some(Stmt::Return(_))));
            body.splice(
                at..at,
                names.into_iter().map(|n| Stmt::CallSub {
                    name: "@owner_interface_constant".into(),
                    args: vec![Expr::Str(n.clone()), Expr::Ident(n)],
                }),
            );
        }
        retain_constants(&mut snapshot.body);
        let snapshot = crate::parameters::resolve_constants(snapshot)?;
        let mut resolver = Resolver::default();
        resolver.collect(&snapshot.body)?;
        for (child, parent) in &parents {
            resolver
                .components
                .entry(child.clone())
                .or_default()
                .insert(
                    parent.rsplit('$').next().unwrap().into(),
                    Ty::Derived(parent.clone()),
                );
        }
        resolver.collect_exports(&snapshot.body)?;
        fn scopes(
            r: &Resolver,
            body: &[Stmt],
            host: &str,
            parent: &OwnerScope,
            out: &mut HashMap<String, OwnerScope>,
            classes: &mut HashMap<String, HashSet<usize>>,
        ) -> Result<()> {
            let mut scope = OwnerScope {
                vars: declarations(body, &parent.vars),
                declared: declarations(body, &parent.declared),
                visible: r.visible(body, &parent.visible)?,
                readonly: parent.readonly.clone(),
                dynamic: parent.dynamic.clone(),
                constants: parent.constants.clone(),
            };
            for n in declarations(body, &Vars::new()).keys() {
                scope.constants.remove(n);
            }
            for s in body {
                if let Stmt::CallSub { name, args } = s {
                    if name == "@owner_interface_constant" {
                        if let [Expr::Str(n), value] = args.as_slice() {
                            scope.constants.insert(n.clone(), value.clone());
                        }
                    }
                }
                if let Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } = s
                {
                    if attributes.iter().any(|a| a == "parameter") {
                        scope.constants.extend(
                            entities
                                .iter()
                                .filter_map(|(n, e)| e.as_ref().map(|e| (n.clone(), e.clone()))),
                        );
                    }
                }
            }
            for s in body {
                match s {
                    Stmt::DerivedDecl { names, .. } | Stmt::VarDecl { names, .. } => {
                        for n in names {
                            scope.readonly.remove(n);
                            scope.dynamic.remove(n);
                        }
                    }
                    Stmt::ArrayDecl { name, .. } => {
                        scope.readonly.remove(name);
                        scope.dynamic.remove(name);
                    }
                    _ => {}
                }
            }
            OwnerInterfaces::readonly_declarations(body, &mut scope);
            out.insert(host.into(), scope.clone());
            for s in body {
                match s {
                    Stmt::Function {
                        name, params, body, ..
                    }
                    | Stmt::Subroutine {
                        name, params, body, ..
                    } => {
                        classes.insert(name.clone(),params.iter().enumerate().filter_map(|(i,p)|body.iter().any(|s|matches!(s,Stmt::DerivedDecl{names,polymorphic:true,..} if names.contains(p))).then_some(i)).collect());
                        scopes(r, body, name, &scope, out, classes)?;
                    }
                    Stmt::Module { name, body } => scopes(r, body, name, &scope, out, classes)?,
                    _ => {}
                }
            }
            Ok(())
        }
        let mut out = Self {
            resolver,
            scopes: HashMap::new(),
            parents,
            classes: HashMap::new(),
            owned,
            dynamic_fields: HashSet::new(),
            generic_binding_names: HashMap::new(),
        };
        fn dynamic_fields(body: &[Stmt], out: &mut HashSet<(String, String)>) {
            for s in body {
                if let Stmt::DerivedType {
                    name, components, ..
                } = s
                {
                    for s in components {
                        if let Stmt::DerivedDecl {
                            names,
                            polymorphic: true,
                            ..
                        } = s
                        {
                            out.extend(names.iter().map(|n| (name.clone(), n.clone())));
                        }
                    }
                } else {
                    for b in children(&mut s.clone()) {
                        dynamic_fields(b, out);
                    }
                }
            }
        }
        dynamic_fields(&snapshot.body, &mut out.dynamic_fields);
        fn generic_names(body: &[Stmt], out: &mut HashMap<(String, String), Vec<String>>) {
            for s in body {
                if let Stmt::DerivedType { name, bindings, .. } = s {
                    for (key, _) in bindings {
                        if let Some((generic, binding)) = key
                            .strip_prefix("@generic_binding:")
                            .and_then(|s| s.split_once(':'))
                        {
                            let names = out.entry((name.clone(), generic.into())).or_default();
                            if !names.iter().any(|n| n == binding) {
                                names.push(binding.into());
                            }
                        }
                    }
                } else {
                    for b in children(&mut s.clone()) {
                        generic_names(b, out);
                    }
                }
            }
        }
        generic_names(&snapshot.body, &mut out.generic_binding_names);
        scopes(
            &out.resolver,
            &snapshot.body,
            "",
            &OwnerScope::default(),
            &mut out.scopes,
            &mut out.classes,
        )?;
        let globals: HashMap<_, _> = out
            .scopes
            .values()
            .flat_map(|s| s.constants.iter())
            .filter(|(n, _)| n.starts_with("@module$"))
            .map(|(n, e)| (n.clone(), e.clone()))
            .collect();
        for scope in out.scopes.values_mut() {
            scope.constants.extend(globals.clone());
        }
        Ok(out)
    }
    pub(crate) fn scope(&self, body: &[Stmt], origin: &str) -> OwnerScope {
        let mut scope = self.scopes.get(origin).cloned().unwrap_or_default();
        // Only derived declarations change during owner specialization. Keep
        // normalized intrinsic kinds from the source snapshot.
        for s in body {
            if matches!(s, Stmt::DerivedDecl { .. }) {
                scope.vars = declarations(std::slice::from_ref(s), &scope.vars);
                for (name, ty) in declarations(std::slice::from_ref(s), &Vars::new()) {
                    scope.declared.entry(name).or_insert(ty);
                }
            }
        }
        Self::readonly_declarations(body, &mut scope);
        scope
    }
    fn readonly_declarations(body: &[Stmt], scope: &mut OwnerScope) {
        for s in body {
            if let Stmt::DerivedDecl {
                names, polymorphic, ..
            } = s
            {
                for n in names {
                    if *polymorphic {
                        scope.dynamic.insert(n.clone());
                    } else {
                        scope.dynamic.remove(n);
                    }
                }
            }
            match s {
                Stmt::DerivedDecl {
                    names, attributes, ..
                } if attributes
                    .iter()
                    .any(|a| a == "intent(in)" || a == "parameter") =>
                {
                    scope.readonly.extend(names.iter().cloned())
                }
                Stmt::DummyIntent { name, intent } if intent == "intent(in)" => {
                    scope.readonly.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    pub(crate) fn dynamic(&self, e: &Expr, scope: &OwnerScope) -> bool {
        let (base, path) = match e {
            Expr::Ident(n) | Expr::Index(n, _) => {
                let (base, path) = n.split_once('%').unwrap_or((n, ""));
                (Expr::Ident(base.into()), path)
            }
            Expr::Call(n, args) if n.starts_with("@record_component:") => {
                let Some(base) = args.first() else {
                    return false;
                };
                if self.dynamic(base, scope) {
                    return true;
                }
                (base.clone(), n.trim_start_matches("@record_component:"))
            }
            Expr::Call(n, _) => {
                let (base, path) = n.split_once('%').unwrap_or((n, ""));
                (Expr::Ident(base.into()), path)
            }
            _ => return false,
        };
        if matches!(&base,Expr::Ident(n) if scope.dynamic.contains(n)) {
            return true;
        }
        let Ok(Some((mut kind, _))) = self.derived(&base, scope) else {
            return false;
        };
        for field in path.split('%').filter(|s| !s.is_empty()) {
            if self.dynamic_fields.contains(&(kind.clone(), field.into())) {
                return true;
            }
            match self
                .resolver
                .components
                .get(&kind)
                .and_then(|c| c.get(field))
            {
                Some(Ty::Derived(k)) => kind = k.clone(),
                _ => return false,
            }
        }
        false
    }
    pub(crate) fn readonly(&self, e: &Expr, scope: &OwnerScope) -> bool {
        match e {
            Expr::Ident(n) | Expr::Index(n, _) => {
                scope.readonly.contains(n.split('%').next().unwrap())
            }
            Expr::Call(n, a) if n.starts_with("@record_component:") => {
                a.first().is_some_and(|e| self.readonly(e, scope))
            }
            Expr::Call(n, _) => scope.readonly.contains(n.split('%').next().unwrap()),
            _ => false,
        }
    }
    pub(crate) fn derived(&self, e: &Expr, scope: &OwnerScope) -> Result<Option<(String, usize)>> {
        let ty = self.expression_type(e, scope)?;
        Ok(match ty {
            Ty::Derived(k) => Some((k, 0)),
            Ty::Array(t, r) => {
                if let Ty::Derived(k) = *t {
                    Some((k, r))
                } else {
                    None
                }
            }
            _ => None,
        })
    }
    fn expression_type(&self, e: &Expr, scope: &OwnerScope) -> Result<Ty> {
        self.type_using(e, scope, &scope.vars)
    }
    fn declared_type(&self, e: &Expr, scope: &OwnerScope) -> Result<Ty> {
        self.type_using(e, scope, &scope.declared)
    }
    fn type_using(&self, e: &Expr, scope: &OwnerScope, vars: &Vars) -> Result<Ty> {
        let mut e = e.clone();
        let mut result = Ok(());
        visit_expr(&mut e, &mut |e| {
            if result.is_ok() {
                if let Expr::Call(n, args) = e {
                    if n == "@typed_constructor" || n == "@intrinsic_type" {
                        if let Some(Expr::Str(encoded)) = args.first_mut() {
                            if let Ok(mut kind) = serde_json::from_str::<TypeSpec>(encoded) {
                                if matches!(
                                    kind,
                                    TypeSpec::IntegerKind(_)
                                        | TypeSpec::RealKind(_)
                                        | TypeSpec::LogicalKind(_)
                                        | TypeSpec::Complex(_)
                                ) {
                                    result = crate::parameters::resolve_kind(
                                        &mut kind,
                                        &scope.constants,
                                    );
                                }
                                if result.is_ok() {
                                    *encoded = serde_json::to_string(&kind)
                                        .expect("serializable constructor type");
                                }
                            }
                        }
                    }
                }
            }
            if result.is_ok() {
                if let Expr::Ident(n) = e {
                    if let Some(value) = scope.constants.get(n) {
                        *e = value.clone();
                    }
                }
                result = crate::parameters::normalize_literal(e, &scope.constants);
            }
        });
        result?;
        self.resolver.expression(&mut e, vars, &scope.visible)
    }
    pub(crate) fn extends(&self, actual: &str, formal: &str) -> bool {
        let mut k = actual;
        let mut seen = HashSet::new();
        loop {
            if k == formal {
                return true;
            }
            if !seen.insert(k) {
                return false;
            }
            match self.parents.get(k) {
                Some(p) => k = p,
                None => return false,
            }
        }
    }
    fn compatible(&self, n: &str, i: usize, formal: &Ty, actual: &Ty) -> bool {
        if formal == actual {
            return true;
        }
        if !self.classes.get(n).is_some_and(|c| c.contains(&i)) {
            return false;
        }
        match (formal, actual) {
            (Ty::Derived(f), Ty::Derived(a)) => self.extends(a, f),
            (Ty::Array(f, fr), Ty::Array(a, ar)) if fr == ar => self.compatible(n, i, f, a),
            _ => false,
        }
    }
    pub(crate) fn owns_generic(&self, kind: &str, generic: &str) -> bool {
        self.generic_binding_names
            .get(&(kind.into(), generic.into()))
            .is_some_and(|names| {
                names.iter().any(|n| {
                    self.resolver
                        .methods
                        .get(kind)
                        .and_then(|m| m.get(n))
                        .is_some_and(|n| self.owned.contains(n))
                })
            })
    }
    fn route(
        &self,
        name: &mut String,
        args: &mut Vec<Expr>,
        subroutine: bool,
        scope: &OwnerScope,
        receiver: Option<Expr>,
    ) -> Result<bool> {
        // Array element references can shadow a host-associated generic.
        if receiver.is_none() && matches!(scope.declared.get(name), Some(Ty::Array(..))) {
            return Ok(false);
        }
        let (receiver, method) = if let Some(e) = receiver {
            (Some(e), name.clone())
        } else if let Some((object, method)) = name.rsplit_once('%') {
            (Some(Expr::Ident(object.into())), method.into())
        } else {
            (None, name.clone())
        };
        let mut actuals = args.clone();
        let mut virtual_binding = None;
        let candidates = if let Some(receiver) = receiver {
            let Ty::Derived(kind) = self.declared_type(&receiver, scope)? else {
                return Ok(false);
            };
            let methods = self.resolver.methods.get(&kind);
            let generic = self
                .resolver
                .generic_methods
                .get(&kind)
                .and_then(|m| m.get(&format!("generic:{method}")));
            let mut identities = vec![];
            let candidates = if let Some(names) = self
                .generic_binding_names
                .get(&(kind.clone(), method.clone()))
            {
                names
                    .iter()
                    .map(|n| {
                        let target =
                            methods.and_then(|m| m.get(n)).cloned().with_context(|| {
                                format!("unknown specific binding `{n}` in generic `{method}`")
                            })?;
                        identities.push((target.clone(), n.clone()));
                        Ok(target)
                    })
                    .collect::<Result<Vec<_>>>()?
            } else if let Some(g) = generic {
                if g.iter().any(|n| self.owned.contains(n)) {
                    bail!("owner generic `{method}` lacks binding identity metadata; recompile its module interface");
                }
                g.clone()
            } else {
                let candidates: Vec<_> = methods
                    .and_then(|m| m.get(&method))
                    .cloned()
                    .into_iter()
                    .collect();
                identities.extend(candidates.iter().map(|p| (p.clone(), method.clone())));
                candidates
            };
            if !candidates.iter().any(|n| self.owned.contains(n)) {
                return Ok(false);
            }
            if self.dynamic(&receiver, scope) {
                bail!("runtime polymorphic owner receivers require a persistent component descriptor interface");
            }
            let Some((dynamic, 0)) = self.derived(&receiver, scope)? else {
                return Ok(false);
            };
            virtual_binding = Some((dynamic, identities));
            actuals.insert(0, receiver);
            candidates
        } else {
            let Some(candidates) = scope.visible.get(&format!("generic:{method}")) else {
                return Ok(false);
            };
            if !candidates.iter().any(|n| self.owned.contains(n)) {
                return Ok(false);
            }
            candidates.clone()
        };
        let types = actuals
            .iter()
            .map(|e| self.declared_type(e, scope))
            .collect::<Result<Vec<_>>>()?;
        let visible = Bindings::from([(format!("generic:{method}"), candidates)]);
        let Some(target) = self.resolver.select_generic_using(
            &method,
            &mut actuals,
            &types,
            subroutine,
            &visible,
            &|n, i, f, a| self.compatible(n, i, f, a),
        )?
        else {
            // A named generic can extend an intrinsic. Leave the valid
            // intrinsic fallback for its normal later lowering.
            return Ok(false);
        };
        *name = if let Some((dynamic, identities)) = virtual_binding {
            let binding = identities
                .iter()
                .find(|(p, _)| p == &target)
                .map(|(_, b)| b);
            binding
                .and_then(|b| self.resolver.methods.get(&dynamic).and_then(|m| m.get(b)))
                .cloned()
                .unwrap_or(target)
        } else {
            target
        };
        *args = actuals;
        Ok(true)
    }
    fn expression(&self, e: &mut Expr, scope: &OwnerScope) -> Result<Option<String>> {
        let Expr::Call(name, args) = e else {
            return Ok(None);
        };
        if let Some(path) = name.strip_prefix("@record_component:") {
            if let (Some(receiver), Some(Expr::Call(marker, actuals))) = (args.first(), args.get(1))
            {
                if marker == "@component_subscripts" {
                    let (receiver, method) = if let Some((path, method)) = path.rsplit_once('%') {
                        (
                            Expr::Call(format!("@record_component:{path}"), vec![receiver.clone()]),
                            method,
                        )
                    } else {
                        (receiver.clone(), path)
                    };
                    let mut target = method.to_owned();
                    let mut values = actuals.clone();
                    if self.route(&mut target, &mut values, false, scope, Some(receiver))? {
                        *e = Expr::Call(target.clone(), values);
                        return Ok(Some(target));
                    }
                }
            }
        } else if self.route(name, args, false, scope, None)? {
            return Ok(Some(name.clone()));
        }
        Ok(None)
    }
    pub(crate) fn normalize(&self, body: &mut Vec<Stmt>, scope: &OwnerScope) -> Result<()> {
        let mut location = None;
        let mut resolved = HashSet::new();
        for s in body.iter_mut() {
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
                for e in expressions(s) {
                    let mut result = Ok(());
                    visit_expr(e, &mut |e| {
                        if result.is_ok() {
                            match self.expression(e, scope) {
                                Ok(Some(n)) => {
                                    resolved.insert(n);
                                }
                                Ok(None) => {}
                                Err(e) => result = Err(e),
                            }
                        }
                    });
                    result?;
                }
                if let Stmt::CallSub { name, args } = s {
                    if let Some(path) = name.strip_prefix("@element_call:") {
                        if let Some(receiver) = args.first() {
                            let (receiver, method) =
                                if let Some((path, method)) = path.rsplit_once('%') {
                                    (
                                        Expr::Call(
                                            format!("@record_component:{path}"),
                                            vec![receiver.clone()],
                                        ),
                                        method,
                                    )
                                } else {
                                    (receiver.clone(), path)
                                };
                            let mut target = method.to_owned();
                            let mut values = args[1..].to_vec();
                            if self.route(&mut target, &mut values, true, scope, Some(receiver))? {
                                resolved.insert(target.clone());
                                *name = target;
                                *args = values;
                            }
                        }
                    } else if self.route(name, args, true, scope, None)? {
                        resolved.insert(name.clone());
                    }
                }
                for child in children(s) {
                    self.normalize(child, scope)?;
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        // Preserve source-validated generic/binding accessibility until the
        // later imports pass. These compiler declarations are then discarded.
        let mut resolved: Vec<_> = resolved.into_iter().collect();
        resolved.sort();
        body.extend(resolved.into_iter().map(crate::imports::reference));
        Ok(())
    }
}

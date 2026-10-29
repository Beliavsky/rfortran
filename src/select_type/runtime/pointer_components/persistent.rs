// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Persistent concrete targets have program-wide identities, independent of a
//! call's borrowed environment. Promote saved local TARGETs to private module
//! storage; retain their original lifetime, default values and shared identity.
//! This is the static-storage part of the descriptor interface, not permission
//! for an automatic local target to escape its activation.
use super::*;

pub(super) const OWNER: &str = "@persistent_pointer_owner";

pub(super) fn owners(program: &Program, defs: &Definitions) -> Result<Declarations> {
    let mut out = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            for (n, ds) in super::super::declarations(body, &Declarations::new()) {
                let mut owned = false;
                for s in &ds {
                    if let Stmt::DerivedDecl { type_name, .. } = s {
                        owned |= super::calls::owner(defs, type_name)?;
                    }
                }
                if owned {
                    out.insert(n, ds);
                }
            }
        }
    }
    Ok(out)
}

pub(in crate::select_type::runtime) fn declarations(program: &Program) -> Declarations {
    let mut out = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            let objects = super::super::super::objects(body, &Objects::new());
            let declared = super::super::declarations(body, &Declarations::new());
            for (n, v) in objects {
                if v.rank == 0
                    && !v.polymorphic
                    && (v.attrs.iter().any(|a| a == "target") || declared.get(&n).is_some_and(|d|super::allocated::info(d).is_some()))
                    && !v.attrs.iter().any(|a| a == "pointer")
                {
                    if let Some(d) = declared.get(&n) {
                        out.insert(n, d.clone());
                    }
                }
            }
        }
    }
    // Include the allocation descriptor and epoch used by static target views.
    let mut extra = vec![];
    for ds in out.values() { if let Some(i) = super::allocated::info(ds) {
        if let Some(a) = i.array {extra.push(a.owner);continue;}
        extra.push(i.epoch);
        if let Some(b) = i.backing { extra.push(b); }
    }}
    for s in &program.body { if let Stmt::Module { body, .. } = s {
        let ds = super::super::declarations(body, &Declarations::new());
        for n in &extra { if let Some(d) = ds.get(n) { out.insert(n.clone(), d.clone()); } }
    }}
    out
}

pub(super) fn prepare(program: &mut Program, defs: &Definitions) -> Result<()> {
    let mut owner_kinds = HashSet::new();
    for kind in defs.fields.keys() {
        if super::calls::owner(defs, kind)? {
            owner_kinds.insert(kind.clone());
        }
    }
    type Initializers = HashMap<(String, String), Expr>;
    type Shapes = HashMap<(String, String), Vec<Expr>>;
    fn initializers(body: &[Stmt], host: &str, out: &mut Initializers, bounds: &mut Shapes) {
        for pair in body.windows(2) {
            if let (
                Stmt::DerivedDecl {
                    names, attributes, ..
                },
                Stmt::Assign { name, value },
            ) = (&pair[0], &pair[1])
            {
                if names.len() == 1 && names[0] == *name && attributes.iter().any(|a| a == "save") {
                    out.insert((host.into(), name.clone()), value.clone());
                }
            }
        }
        for s in body {
            match s {
                Stmt::DerivedDecl {
                    names,
                    dims,
                    attributes,
                    ..
                } if !dims.is_empty()
                    && attributes.iter().any(|a| a == "save")
                    && !attributes
                        .iter()
                        .any(|a| a == "allocatable" || a == "pointer") =>
                {
                    for n in names {
                        bounds.insert((host.into(), n.clone()), dims.clone());
                    }
                }
                Stmt::CallSub { name, args } if name == "@saved_owner_shape" => {
                    if let Some(Expr::Str(n)) = args.first() {
                        bounds.insert((host.into(), n.clone()), args[1..].to_vec());
                    }
                }
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    initializers(body, &format!("{host}${name}"), out, bounds)
                }
                Stmt::Module { name, body } => initializers(body, name, out, bounds),
                _ => {}
            }
        }
    }
    // Reuse constant lowering in the defining scope before promoting storage.
    // This preserves local PARAMETERs and imported kind/literal constants in
    // declaration constructors without moving user declarations between scopes.
    let mut folded = Initializers::new();
    let mut bounds = Shapes::new();
    initializers(&program.body, "", &mut folded, &mut bounds);
    if !folded.is_empty() || !bounds.is_empty() {
        // Derived specification expressions deliberately survive ordinary
        // constant lowering. Evaluate copies as executable marker arguments,
        // where the defining procedure's PARAMETER environment is still known.
        fn retain_bounds(body: &mut Vec<Stmt>) {
            let mut markers = vec![];
            for s in body.iter_mut() {
                if let Stmt::DerivedDecl {
                    names,
                    dims,
                    attributes,
                    ..
                } = s
                {
                    if !dims.is_empty() && attributes.iter().any(|a| a == "save") {
                        for n in names {
                            let mut args = vec![Expr::Str(n.clone())];
                            args.extend(dims.clone());
                            markers.push(Stmt::CallSub {
                                name: "@saved_owner_shape".into(),
                                args,
                            });
                        }
                    }
                }
                if matches!(
                    s,
                    Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }
                ) {
                    for child in children(s) {
                        retain_bounds(child);
                    }
                }
            }
            let at = body.len() - usize::from(matches!(body.last(), Some(Stmt::Return(_))));
            body.splice(at..at, markers);
        }
        let mut snapshot = program.clone();
        retain_bounds(&mut snapshot.body);
        let snapshot = crate::parameters::resolve_constants(snapshot)?;
        folded.clear();
        bounds.clear();
        initializers(&snapshot.body, "", &mut folded, &mut bounds);
    }
    fn scope(
        body: &mut Vec<Stmt>,
        host: &str,
        module: Option<&str>,
        procedure: bool,
        params: &[String],
        storage: &mut Vec<Stmt>,
        folded: &Initializers,
        owner_kinds: &HashSet<String>,
        bounds: &Shapes,
    ) -> Result<()> {
        if procedure || host.is_empty() {
            let vars = super::super::super::objects(body, &Objects::new());
            let saved: Vec<_> = vars
                .into_iter()
                .filter(|(n, v)| {
                    !params.contains(n)
                        && !v.polymorphic
                        && ((v.rank == 0 && v.attrs.iter().any(|a| a == "target" || a == "allocatable"))
                            || (v.rank > 0 && v.attrs.iter().any(|a|a=="target") && body.iter().any(|s|matches!(s,Stmt::DerivedDecl{names,..} if names.contains(n))))
                            || owner_kinds.contains(&v.kind))
                        && v.attrs.iter().any(|a| a == "save")
                        && !v.attrs.iter().any(|a| a == "pointer")
                        && (!v.attrs.iter().any(|a| a == "allocatable") || (v.rank == 0 && !owner_kinds.contains(&v.kind)))
                })
                .map(|(n, _)| n)
                .collect();
            let mut saved = saved;
            saved.sort();
            for n in saved {
                let target_attribute = body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == &n && intent == "target"));
                // The name is based on lexical identity, never on the number
                // or order of call specializations. The module spelling also
                // gives native separate compilation a stable storage symbol.
                let canonical = format!(
                    "@module${}$@pointer_saved${host}${n}",
                    module.unwrap_or("@pointer_saved")
                );
                let initializer = body.windows(2).position(|pair| {
                    matches!(&pair[0], Stmt::DerivedDecl { names, .. } if names.len() == 1 && names[0] == n)
                        && matches!(&pair[1], Stmt::Assign { name, .. } if name == &n)
                }).map(|i| body.remove(i + 1));
                let mut declaration = None;
                body.retain_mut(|s| match s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        attributes,
                        polymorphic,
                        dims,
                    } if names.contains(&n) => {
                        let mut attributes = attributes.clone();
                        if target_attribute && !attributes.iter().any(|a| a == "target") {
                            attributes.push("target".into());
                        }
                        if !dims.is_empty() {
                            // Module arrays already have static lifetime.
                            attributes.retain(|a| a != "save");
                        }
                        attributes.push("@referenced_type".into());
                        declaration = Some(Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![canonical.clone()],
                            attributes,
                            polymorphic: *polymorphic,
                            dims: bounds
                                .get(&(host.into(), n.clone()))
                                .cloned()
                                .unwrap_or_else(|| dims.clone()),
                        });
                        names.retain(|x| x != &n);
                        !names.is_empty()
                    }
                    Stmt::DummyIntent { name, intent }
                        if name == &n && matches!(intent.as_str(), "save" | "target") =>
                    {
                        false
                    }
                    _ => true,
                });
                if let Some(d) = declaration {
                    // Declaration initialization is the adjacent assignment
                    // emitted by the parser. Executable assignments stay in
                    // the procedure, and consequently run on every call.
                    storage.push(d);
                    if let Some(mut s) = initializer {
                        if let Stmt::Assign { value, .. } = &mut s {
                            if let Some(initial) = folded.get(&(host.into(), n.clone())) {
                                *value = initial.clone();
                            }
                        }
                        super::calls::substitute(std::slice::from_mut(&mut s), &n, &canonical);
                        storage.push(s);
                    }
                    super::calls::substitute(body, &n, &canonical);
                    body.push(crate::imports::reference(canonical));
                }
            }
        }
        for s in body.iter_mut() {
            match s {
                Stmt::Function {
                    name,
                    body,
                    params,
                    pure,
                    ..
                }
                | Stmt::Subroutine {
                    name,
                    body,
                    params,
                    pure,
                    ..
                } => {
                    if *pure
                        && super::super::super::objects(body, &Objects::new())
                            .values()
                            .any(|v| {
                                v.attrs.iter().any(|a| a == "save")
                                    && (v.attrs.iter().any(|a| a == "target") || (v.rank==0 && v.attrs.iter().any(|a|a=="allocatable"))
                                        || owner_kinds.contains(&v.kind))
                            })
                    {
                        bail!("PURE procedure cannot have saved or declaration-initialized local variables");
                    }
                    scope(
                        body,
                        &format!("{host}${name}"),
                        module,
                        true,
                        params,
                        storage,
                        folded,
                        owner_kinds,
                        bounds,
                    )?;
                }
                Stmt::Module { name, body } => scope(
                    body,
                    name,
                    Some(name),
                    false,
                    &[],
                    storage,
                    folded,
                    owner_kinds,
                    bounds,
                )?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut storage = vec![];
    scope(
        &mut program.body,
        "",
        None,
        false,
        &[],
        &mut storage,
        &folded,
        &owner_kinds,
        &bounds,
    )?;
    if !storage.is_empty() {
        program.body.insert(
            0,
            Stmt::Module {
                name: "_pointer_saved".into(),
                body: storage,
            },
        );
    }
    Ok(())
}

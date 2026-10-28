// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Persistent concrete targets have program-wide identities, independent of a
//! call's borrowed environment. Promote saved local TARGETs to private module
//! storage; retain their original lifetime, default values and shared identity.
//! This is the static-storage part of the descriptor interface, not permission
//! for an automatic local target to escape its activation.
use super::*;

pub(in crate::select_type::runtime) fn declarations(program: &Program) -> Declarations {
    let mut out = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            let objects = super::super::super::objects(body, &Objects::new());
            let declared = super::super::declarations(body, &Declarations::new());
            for (n, v) in objects {
                if v.rank == 0
                    && !v.polymorphic
                    && v.attrs.iter().any(|a| a == "target")
                    && !v.attrs.iter().any(|a| a == "pointer" || a == "allocatable")
                {
                    if let Some(d) = declared.get(&n) {
                        out.insert(n, d.clone());
                    }
                }
            }
        }
    }
    out
}

pub(super) fn prepare(program: &mut Program) -> Result<()> {
    type Initializers = HashMap<(String, String), Expr>;
    fn initializers(body: &[Stmt], host: &str, out: &mut Initializers) {
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
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    initializers(body, &format!("{host}${name}"), out)
                }
                Stmt::Module { name, body } => initializers(body, name, out),
                _ => {}
            }
        }
    }
    // Reuse constant lowering in the defining scope before promoting storage.
    // This preserves local PARAMETERs and imported kind/literal constants in
    // declaration constructors without moving user declarations between scopes.
    let mut folded = Initializers::new();
    initializers(&program.body, "", &mut folded);
    if !folded.is_empty() {
        let snapshot = crate::parameters::resolve_constants(program.clone())?;
        folded.clear();
        initializers(&snapshot.body, "", &mut folded);
    }
    fn scope(
        body: &mut Vec<Stmt>,
        host: &str,
        module: Option<&str>,
        procedure: bool,
        params: &[String],
        storage: &mut Vec<Stmt>,
        folded: &Initializers,
    ) -> Result<()> {
        if procedure {
            let vars = super::super::super::objects(body, &Objects::new());
            let saved: Vec<_> = vars
                .into_iter()
                .filter(|(n, v)| {
                    !params.contains(n)
                        && !v.polymorphic
                        && v.rank == 0
                        && v.attrs.iter().any(|a| a == "target")
                        && v.attrs.iter().any(|a| a == "save")
                        && !v.attrs.iter().any(|a| a == "pointer" || a == "allocatable")
                })
                .map(|(n, _)| n)
                .collect();
            let mut saved = saved;
            saved.sort();
            for n in saved {
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
                        attributes.push("@referenced_type".into());
                        declaration = Some(Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![canonical.clone()],
                            attributes,
                            polymorphic: *polymorphic,
                            dims: dims.clone(),
                        });
                        names.retain(|x| x != &n);
                        !names.is_empty()
                    }
                    Stmt::DummyIntent { name, intent } if name == &n && intent == "save" => false,
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
                                    && v.attrs.iter().any(|a| a == "target")
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
                    )?;
                }
                Stmt::Module { name, body } => {
                    scope(body, name, Some(name), false, &[], storage, folded)?
                }
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

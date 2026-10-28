// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Preserve source interfaces, then pass scalar allocation storage and identity.
use super::*;

#[derive(Clone)]
struct Interface {
    signature: crate::named_arguments::Signature,
    descriptors: Vec<(usize, String, bool)>,
}
fn collect(body: &[Stmt], out: &mut HashMap<String, Interface>, prepared: bool) {
    for s in body {
        if let Stmt::Subroutine {
            name, params, body, ..
        }
        | Stmt::Function {
            name, params, body, ..
        } = s
        {
            let vars = super::super::super::super::objects(body, &Objects::new());
            let descriptors = params
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let v = vars.get(p)?;
                    (!v.polymorphic
                        && v.rank == 0
                        && v.attrs.iter().any(|a| a == "allocatable")
                        && (!prepared || v.attrs.iter().any(|a| a == "@allocated_scalar_dummy")))
                    .then(|| (i, v.kind.clone(), v.attrs.iter().any(|a| a == "intent(in)")))
                })
                .collect::<Vec<_>>();
            if !descriptors.is_empty() {
                out.insert(
                    name.clone(),
                    Interface {
                        signature: crate::named_arguments::Signature {
                            params: params.clone(),
                            optional: params
                                .iter()
                                .filter(|p| {
                                    vars.get(*p)
                                        .is_some_and(|v| v.attrs.iter().any(|a| a == "optional"))
                                })
                                .cloned()
                                .collect(),
                        },
                        descriptors,
                    },
                );
            }
            collect(body, out, prepared);
        } else if let Stmt::Module { body, .. } = s {
            collect(body, out, prepared);
        }
    }
}
pub(super) fn normalize(program: &mut Program, defs: &Definitions) -> Result<()> {
    let mut interfaces = HashMap::new();
    collect(&program.body, &mut interfaces, false);
    if interfaces.is_empty() {
        return Ok(());
    }
    let resolver = crate::operators::OwnerInterfaces::new(
        program,
        interfaces.keys().cloned().collect(),
        defs.parents.clone(),
    )?;
    fn scope(
        body: &mut Vec<Stmt>,
        origin: &str,
        resolver: &crate::operators::OwnerInterfaces,
    ) -> Result<()> {
        let context = resolver.scope(body, origin);
        resolver.normalize(body, &context)?;
        for s in body {
            match s {
                Stmt::Function { name, body, .. }
                | Stmt::Subroutine { name, body, .. }
                | Stmt::Module { name, body, .. } => scope(body, name, resolver)?,
                _ => {}
            }
        }
        Ok(())
    }
    scope(&mut program.body, "", &resolver)
}

pub(super) fn lower(program: &mut Program) -> Result<()> {
    let mut interfaces = HashMap::new();
    collect(&program.body, &mut interfaces, true);
    if interfaces.is_empty() {
        return Ok(());
    }
    fn actuals(
        name: &str,
        args: &mut Vec<Expr>,
        declared: &Declarations,
        interfaces: &HashMap<String, Interface>,
    ) -> Result<()> {
        let Some(interface) = interfaces.get(name) else {
            return Ok(());
        };
        interface.signature.reorder(name, args)?;
        args.resize_with(interface.signature.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        let mut hidden = vec![];
        for (i, expected, readonly) in &interface.descriptors {
            let actual = &args[*i];
            if crate::named_arguments::omitted(actual) {
                hidden.push(actual.clone());
                continue;
            }
            if matches!(actual,Expr::Ident(n)|Expr::Call(n,_) if n.contains('%') || n.starts_with("@record_component:"))
            {
                bail!("ALLOCATABLE scalar component actuals are not yet supported");
            }
            let Expr::Ident(n) = actual else {
                bail!("ALLOCATABLE scalar dummy `{}` of `{name}` requires an ALLOCATABLE scalar actual",interface.signature.params[*i]);
            };
            let ds = declared.get(n).with_context(|| {
                format!(
                    "ALLOCATABLE scalar dummy `{}` requires an ALLOCATABLE scalar actual",
                    interface.signature.params[*i]
                )
            })?;
            let information = info(ds).filter(|i| i.backing.is_some()).with_context(|| {
                format!(
                    "ALLOCATABLE scalar dummy `{}` requires an ALLOCATABLE scalar actual",
                    interface.signature.params[*i]
                )
            })?;
            let declaration = ds
                .iter()
                .find_map(|s| match s {
                    Stmt::DerivedDecl {
                        type_name,
                        attributes,
                        dims,
                        ..
                    } => Some((type_name, attributes, dims)),
                    _ => None,
                })
                .context("missing ALLOCATABLE scalar declaration")?;
            if declaration.0 != expected || !declaration.2.is_empty() {
                bail!(
                    "ALLOCATABLE scalar dummy `{}` requires the same declared type and rank",
                    interface.signature.params[*i]
                );
            }
            if !readonly
                && (declaration.1.iter().any(|a| a == "intent(in)")
                    || ds
                        .iter()
                        .any(|s| matches!(s,Stmt::DummyIntent{intent,..} if intent=="intent(in)")))
            {
                bail!(
                    "cannot pass INTENT(IN) ALLOCATABLE scalar `{n}` to writable dummy `{}`",
                    interface.signature.params[*i]
                );
            }
            args[*i] = id(information.backing.as_ref().unwrap());
            hidden.push(id(&information.epoch));
        }
        args.extend(hidden);
        Ok(())
    }
    fn scope(
        body: &mut Vec<Stmt>,
        parent: &Declarations,
        interfaces: &HashMap<String, Interface>,
    ) -> Result<()> {
        let declared = declarations(body, parent);
        let mut location = None;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                location = Some(*p);
            }
            if let Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } = s
            {
                if let Some(interface) = interfaces.get(name) {
                    let local = declarations(body, &Declarations::new());
                    let mut hidden = vec![];
                    for (i, _, _) in &interface.descriptors {
                        let info = info(&local[&params[*i]])
                            .context("missing ALLOCATABLE dummy storage")?;
                        params[*i] = info.backing.unwrap();
                        hidden.push(info.epoch);
                    }
                    params.extend(hidden);
                }
                scope(body, &declared, interfaces)?;
                continue;
            }
            if let Stmt::Module { body, .. } = s {
                scope(body, &declared, interfaces)?;
                continue;
            }
            let mut result = Ok(());
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if result.is_ok() {
                        if let Expr::Call(name, args) = e {
                            result = actuals(name, args, &declared, interfaces);
                        }
                    }
                });
            }
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
            if let Stmt::CallSub { name, args } = s {
                actuals(name, args, &declared, interfaces)
                    .map_err(|e| crate::source_diagnostics::at(e, location))?;
            }
            for child in children(s) {
                scope(child, &declared, interfaces)?;
            }
        }
        Ok(())
    }
    let mut globals = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            globals.extend(declarations(body, &Declarations::new()));
        }
    }
    scope(&mut program.body, &globals, &interfaces)
}

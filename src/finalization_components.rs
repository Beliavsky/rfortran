// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Concrete scalar owned record components share rank-one descriptor storage.
use super::*;

pub(super) fn normalize(program: &mut Program, types: &HashMap<String, Definition>) -> Result<()> {
    fn paths(name: &str, ty: &str, types: &HashMap<String, Definition>, out: &mut Vec<String>) {
        if let Some(d) = types.get(ty) {
            for (field, child, attrs, rank) in &d.components {
                if attrs.iter().any(|a| a == "pointer") {
                    continue;
                }
                let path = format!("{name}%{field}");
                if attrs.iter().any(|a| a == "allocatable") {
                    if *rank == 0 {
                        out.push(path);
                    }
                } else if *rank == 0 {
                    paths(&path, child, types, out);
                }
            }
        }
    }
    fn reference(n: &str, paths: &[String]) -> Option<(String, Option<String>)> {
        paths.iter().find_map(|p| {
            if n == p {
                Some((p.clone(), None))
            } else {
                n.strip_prefix(&format!("{p}%"))
                    .map(|f| (p.clone(), Some(f.into())))
            }
        })
    }
    fn expression(e: &mut Expr, paths: &[String], whole: bool) {
        match e {
            Expr::Ident(n) => {
                if let Some((p, field)) = reference(n, paths) {
                    if whole && field.is_none() {
                        return;
                    }
                    let element = Expr::Call(p, vec![Expr::IntLit("1".into())]);
                    *e = if let Some(f) = field {
                        Expr::Call(format!("@record_component:{f}"), vec![element])
                    } else {
                        element
                    };
                }
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if let Some((p, field)) = reference(n, paths) {
                    if let Some(field) = field {
                        let mut a = vec![Expr::Call(p, vec![Expr::IntLit("1".into())])];
                        if !args.is_empty() {
                            a.push(Expr::Call("@component_subscripts".into(), args.clone()));
                        }
                        *e = Expr::Call(format!("@record_component:{field}"), a);
                        return;
                    }
                }
                let inquiry = matches!(n.as_str(), "allocated" | "present");
                for arg in args {
                    expression(arg, paths, inquiry);
                }
            }
            Expr::Bin(_, a, b) => {
                expression(a, paths, false);
                expression(b, paths, false);
            }
            Expr::Un(_, a) => expression(a, paths, false),
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    expression(e, paths, false);
                }
            }
            _ => {}
        }
    }
    fn scope(
        body: &mut [Stmt],
        inherited: &[String],
        types: &HashMap<String, Definition>,
    ) -> Result<()> {
        let mut owned = inherited.to_vec();
        for s in body.iter() {
            let names = match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
                Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                Stmt::InitializedDecl { entities, .. } => {
                    entities.iter().map(|(n, _)| n.clone()).collect()
                }
                _ => vec![],
            };
            owned.retain(|path| !names.iter().any(|n| path.starts_with(&format!("{n}%"))));
        }
        for s in body.iter() {
            if let Stmt::DerivedDecl {
                type_name,
                names,
                dims,
                ..
            } = s
            {
                if dims.is_empty() {
                    for name in names {
                        paths(name, type_name, types, &mut owned);
                    }
                }
            }
        }
        for s in body {
            if let Stmt::DerivedType { components, .. } = s {
                for c in components {
                    if let Stmt::DerivedDecl {
                        attributes,
                        dims,
                        polymorphic: false,
                        ..
                    } = c
                    {
                        if dims.is_empty() && attributes.iter().any(|a| a == "allocatable") {
                            dims.push(id("@deferred_extent"));
                            attributes.push("@scalar_heap".into());
                        }
                    }
                }
                continue;
            }
            match s {
                Stmt::Assign { name, value } => {
                    expression(value, &owned, false);
                    if let Some((p, field)) = reference(name, &owned) {
                        let assignment = Stmt::AssignIndex {
                            name: field.as_ref().map_or(p.clone(), |f| format!("{p}%{f}")),
                            indices: vec![Expr::IntLit("1".into())],
                            value: value.clone(),
                        };
                        *s = if field.is_none() {
                            Stmt::Block {
                                body: vec![
                                    guarded(
                                        Expr::Un(
                                            crate::ast::UnOp::Not,
                                            Box::new(Expr::Call("allocated".into(), vec![id(&p)])),
                                        ),
                                        vec![call(
                                            "@allocate",
                                            vec![Expr::Call(p, vec![Expr::IntLit("1".into())])],
                                        )],
                                    ),
                                    assignment,
                                ],
                            }
                        } else {
                            assignment
                        };
                    }
                }
                Stmt::CallSub { name, args } if name == "@allocate" || name == "@deallocate" || name=="@move_allocation_storage" || name=="move_alloc" => {
                    for arg in args {
                        if name == "@allocate"
                            && matches!(arg,Expr::Ident(n) if reference(n,&owned).is_some_and(|(_,f)|f.is_none()))
                        {
                            let Expr::Ident(n) = arg else { unreachable!() };
                            *arg = Expr::Call(n.clone(), vec![Expr::IntLit("1".into())]);
                        } else {
                            expression(arg, &owned, true);
                        }
                    }
                }
                _ => {
                    for e in crate::procedure_arguments::expressions(s) {
                        expression(e, &owned, false);
                    }
                }
            }
            for child in crate::procedure_arguments::children(s) {
                scope(child, &owned, types)?;
            }
        }
        Ok(())
    }
    scope(&mut program.body, &[], types)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Scalar component allocations expose the same virtual views as root targets.
//! Their generation belongs to the owner, rather than to a call's temporary.
use super::*;
const ALIAS: &str = "@allocated_component_alias";
fn epoch_field(field: &str) -> String {
    format!("@allocated_component_epoch_{field}")
}
type Paths = HashMap<String, Vec<(String, String, String)>>;

pub(super) fn declarations(declared: &mut Declarations) {
    let mut extra = vec![];
    for ds in declared.values() {
        for s in ds {
            if let Stmt::DerivedDecl {
                type_name,
                attributes,
                ..
            } = s
            {
                if attributes.iter().any(|a| a == ALIAS) && !array_target(ds) {
                    if let Some(i) = info(ds) {
                        let backing = i.backing.unwrap();
                        extra.push((
                            backing.clone(),
                            Stmt::DerivedDecl {
                                type_name: type_name.clone(),
                                names: vec![backing],
                                attributes: vec![
                                    "allocatable".into(),
                                    "target".into(),
                                    "@scalar_heap".into(),
                                    "@referenced_type".into(),
                                ],
                                polymorphic: false,
                                dims: vec![id("@deferred_extent")],
                            },
                        ));
                        extra.push((
                            i.epoch.clone(),
                            Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: vec![i.epoch],
                            },
                        ));
                    }
                }
            }
        }
    }
    for (n, s) in extra {
        declared.entry(n).or_insert_with(|| vec![s]);
    }
}

pub(super) fn prepare(program: &mut Program, defs: &mut Definitions) -> Result<()> {
    crate::finalization::prepare_components(program)?;
    // Only the concrete scalar component layouts already normalized to an
    // owned one-element descriptor participate in this view interface.
    fn schemas(body: &mut [Stmt], defs: &mut Definitions, out: &mut Paths) {
        for s in body {
            if let Stmt::DerivedType {
                name, components, ..
            } = s
            {
                let mut found = vec![];
                let mut fields = vec![];
                for c in components.iter() {
                    if let Stmt::DerivedDecl {
                        type_name,
                        names,
                        attributes,
                        dims,
                        polymorphic: false,
                    } = c
                    {
                        for field in names {
                            if attributes.iter().any(|a| a == "allocatable")
                                && attributes.iter().any(|a| a == "@scalar_heap")
                            {
                                found.push((field.clone(), type_name.clone(), epoch_field(field)));
                                fields.push(epoch_field(field));
                            } else if dims.is_empty()
                                && !attributes
                                    .iter()
                                    .any(|a| a == "allocatable" || a == "pointer")
                            {
                                found.push((field.clone(), type_name.clone(), String::new()));
                            }
                        }
                    }
                }
                for field in fields {
                    components.push(Stmt::InitializedDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        attributes: vec![],
                        entities: vec![(field.clone(), Some(int(0)))],
                    });
                    defs.fields.get_mut(name).unwrap().insert(field, None);
                }
                out.insert(name.clone(), found);
            } else {
                for b in children(s) {
                    schemas(b, defs, out);
                }
            }
        }
    }
    let mut paths = Paths::new();
    schemas(&mut program.body, defs, &mut paths);
    // Inherited components use the same accessible field path as components
    // declared directly in the owner's type.
    for kind in paths.keys().cloned().collect::<Vec<_>>() {
        let mut parent = defs.parents.get(&kind);
        let mut seen = HashSet::from([kind.clone()]);
        let mut inherited = vec![];
        while let Some(base) = parent {
            if !seen.insert(base.clone()) {
                break;
            }
            inherited.extend(paths.get(base).into_iter().flatten().cloned());
            parent = defs.parents.get(base);
        }
        let fields = paths.get_mut(&kind).unwrap();
        for field in inherited {
            if !fields.iter().any(|f| f.0 == field.0) {
                fields.push(field);
            }
        }
    }
    fn collect(root: &str, kind: &str, paths: &Paths, out: &mut Vec<(String, String, String)>) {
        collect_inner(root, kind, paths, out, &mut HashSet::new());
    }
    fn collect_inner(
        root: &str,
        kind: &str,
        paths: &Paths,
        out: &mut Vec<(String, String, String)>,
        seen: &mut HashSet<String>,
    ) {
        // Invalid nonallocatable recursive layouts are diagnosed by type
        // validation; do not recurse indefinitely while constructing views.
        if !seen.insert(kind.into()) {
            return;
        }
        for (field, ty, epoch) in paths.get(kind).into_iter().flatten() {
            let path = format!("{root}%{field}");
            if epoch.is_empty() {
                collect_inner(&path, ty, paths, out, seen);
            } else {
                out.push((path, ty.clone(), format!("{root}%{epoch}")));
            }
        }
        seen.remove(kind);
    }
    fn scope(body: &mut Vec<Stmt>, parent: &HashMap<String, String>, paths: &Paths) -> Result<()> {
        let mut aliases = parent.clone();
        for root in super::super::super::super::objects(body, &Objects::new()).keys() {
            aliases.retain(|path, _| !path.strip_prefix(root).is_some_and(|s| s.starts_with('%')));
        }
        let mut generated = vec![];
        for s in body.iter() {
            if let Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                dims,
                polymorphic: false,
            } = s
            {
                if !dims.is_empty() || info(std::slice::from_ref(s)).is_some() {
                    continue;
                }
                for root in names {
                    let mut components = vec![];
                    collect(root, type_name, paths, &mut components);
                    for (backing, kind, epoch) in components {
                        let field = &backing[root.len() + 1..];
                        let alias = if root.starts_with("@module$") {
                            format!("{root}$@component_target${}", field.replace('%', "$"))
                        } else {
                            format!("@component_target${root}${}", field.replace('%', "$"))
                        };
                        let mut attrs = vec![
                            "allocatable".into(),
                            "@referenced_type".into(),
                            ALIAS.into(),
                            backing_attribute(&backing),
                            epoch_attribute(&epoch),
                        ];
                        attrs.extend(
                            attributes
                                .iter()
                                .filter(|a| a.as_str() == "target" || a.starts_with("intent("))
                                .cloned(),
                        );
                        for statement in body.iter() {
                            if let Stmt::DummyIntent { name, intent } = statement {
                                if name == root
                                    && (intent == "target" || intent.starts_with("intent("))
                                {
                                    attrs.push(intent.clone());
                                }
                            }
                        }
                        generated.push(Stmt::DerivedDecl {
                            type_name: kind,
                            names: vec![alias.clone()],
                            attributes: attrs,
                            polymorphic: false,
                            dims: vec![],
                        });
                        aliases.insert(backing, alias);
                    }
                }
            }
        }
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if matches!(s, Stmt::DerivedType { .. }) {
                out.push(s);
                continue;
            }
            if matches!(
                s,
                Stmt::Module { .. } | Stmt::Subroutine { .. } | Stmt::Function { .. }
            ) {
                for b in children(&mut s) {
                    scope(b, &aliases, paths)?;
                }
                out.push(s);
                continue;
            }
            let copied = match &s {
                Stmt::Assign { name, .. } => Some(name.clone()),
                Stmt::CallSub { name, args } if name == crate::operators::INTRINSIC_ASSIGNMENT => {
                    args.first().and_then(|e| {
                        if let Expr::Ident(n) = e {
                            Some(n.clone())
                        } else {
                            None
                        }
                    })
                }
                _ => None,
            };
            for e in expressions(&mut s) {
                visit_expr(e, &mut |e| match e {
                    Expr::Ident(n) => {
                        if let Some(a) = aliases.get(n) {
                            *n = a.clone();
                        }
                    }
                    Expr::Call(n, args) if matches!(args.as_slice(),[Expr::IntLit(i)] if i=="1") => {
                        if let Some(a) = aliases.get(n) {
                            *e = id(a);
                        }
                    }
                    _ => {}
                });
            }
            if let Stmt::AssignIndex {
                name,
                indices,
                value,
            } = &mut s
            {
                if matches!(indices.as_slice(),[Expr::IntLit(i)] if i=="1") {
                    if let Some(alias) = aliases.get(name) {
                        s = Stmt::Assign {
                            name: alias.clone(),
                            value: value.clone(),
                        };
                    }
                }
            }
            for b in children(&mut s) {
                scope(b, &aliases, paths)?;
            }
            out.push(s);
            if let Some(name) = copied {
                let mut owned = aliases
                    .iter()
                    .filter(|(path, _)| path.starts_with(&format!("{name}%")))
                    .collect::<Vec<_>>();
                owned.sort();
                for (path, alias) in owned {
                    let (root, field) = path.rsplit_once('%').unwrap();
                    let epoch = format!("{root}%{}", epoch_field(field));
                    out.push(Stmt::If {
                        cond: Expr::Call("allocated".into(), vec![id(alias)]),
                        then_body: vec![Stmt::CallSub {
                            name: "@heap_identity".into(),
                            args: vec![id(&epoch)],
                        }],
                        else_body: Some(vec![Stmt::Assign {
                            name: epoch,
                            value: int(0),
                        }]),
                    });
                }
            }
        }
        generated.extend(out);
        *body = generated;
        Ok(())
    }
    let mut globals = HashMap::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            for s in body {
                if let Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    polymorphic: false,
                    ..
                } = s
                {
                    if dims.is_empty() && info(std::slice::from_ref(s)).is_none() {
                        for root in names {
                            let mut c = vec![];
                            collect(root, type_name, &paths, &mut c);
                            for (p, _, _) in c {
                                globals.insert(
                                    p.clone(),
                                    format!(
                                        "{root}$@component_target${}",
                                        p[root.len() + 1..].replace('%', "$")
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    super::array_components::prepare(program, &paths)?;
    crate::finalization::prepare_components(program)?;
    scope(&mut program.body, &globals, &paths)
}

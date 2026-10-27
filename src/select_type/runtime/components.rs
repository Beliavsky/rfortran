// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Owned scalar polymorphic components use the ordinary type tag and a
//! concrete rank-one allocation per possible dynamic type, inside the owner.
use super::*;

pub(super) fn stem(path: &str) -> String {
    let (owner, field) = path.rsplit_once('%').unwrap();
    format!("{owner}%@polymorphic_{field}")
}
impl Definitions {
    pub(in crate::select_type) fn component_objects(&self, mut roots: Objects) -> Objects {
        if !self.has_polymorphic_components {
            return roots;
        }
        fn add(
            defs: &Definitions,
            out: &mut Objects,
            name: &str,
            obj: &super::super::Object,
            seen: &mut HashSet<String>,
        ) {
            if obj.rank != 0 || !seen.insert(obj.kind.clone()) {
                return;
            }
            if let Some(parent) = defs.parents.get(&obj.kind) {
                let mut base = obj.clone();
                base.kind = parent.clone();
                add(defs, out, name, &base, seen);
            }
            for (field, member) in defs.members.get(&obj.kind).into_iter().flatten() {
                let path = format!("{name}%{field}");
                let mut member = member.clone();
                if obj.attrs.iter().any(|a| a == "intent(in)") {
                    member.attrs.push("intent(in)".into());
                }
                if member.polymorphic && member.attrs.iter().any(|a| a == "allocatable") {
                    let layout = variants(defs, &member.kind);
                    let storage_stem = stem(&path);
                    for (i, kind) in layout.into_iter().enumerate() {
                        out.insert(
                            format!("{storage_stem}$storage{i}"),
                            super::super::Object {
                                kind,
                                polymorphic: false,
                                rank: 1,
                                attrs: member.attrs.clone(),
                            },
                        );
                    }
                    out.insert(path, member);
                } else {
                    add(defs, out, &path, &member, seen);
                }
            }
            seen.remove(&obj.kind);
        }
        for (name, obj) in roots.clone() {
            add(self, &mut roots, &name, &obj, &mut HashSet::new());
        }
        roots
    }
}
fn variants(defs: &Definitions, base: &str) -> Vec<String> {
    let mut kinds = defs
        .fields
        .keys()
        .filter(|k| !defs.abstracts.contains(*k) && defs.distance(k, base).is_some())
        .cloned()
        .collect::<Vec<_>>();
    kinds.sort();
    kinds
}
fn recursive(defs: &Definitions, ty: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(ty.into()) {
        return true;
    }
    let inherited = defs
        .parents
        .get(ty)
        .is_some_and(|p| recursive(defs, p, seen));
    let found = inherited
        || defs.members.get(ty).into_iter().flatten().any(|(_, v)| {
            v.polymorphic && v.attrs.iter().any(|a| a == "allocatable")
                || recursive(defs, &v.kind, seen)
        });
    seen.remove(ty);
    found
}
pub(super) fn prepare(body: &mut [Stmt], defs: &mut Definitions) -> Result<()> {
    for s in body {
        match s {
            Stmt::DerivedType {
                name, components, ..
            } => {
                let mut out = vec![];
                for c in std::mem::take(components) {
                    if let Stmt::DerivedDecl {
                        type_name,
                        names,
                        attributes,
                        polymorphic: true,
                        dims,
                    } = &c
                    {
                        if attributes.iter().any(|a| a == "allocatable") {
                            if !dims.is_empty() {
                                bail!("polymorphic ALLOCATABLE component arrays are not yet supported");
                            }
                            if type_name == "*" {
                                bail!("unlimited polymorphic ALLOCATABLE components are not yet supported");
                            }
                            let kinds = variants(defs, type_name);
                            for ty in &kinds {
                                if recursive(defs, ty, &mut HashSet::new()) {
                                    bail!("recursive polymorphic component layouts are not yet supported");
                                }
                            }
                            for n in names {
                                let stem = format!("@polymorphic_{n}");
                                let tag = format!("{stem}$tag");
                                out.push(Stmt::InitializedDecl {
                                    kind: TypeSpec::Integer(None),
                                    attributes: vec![],
                                    entities: vec![(tag.clone(), Some(int(0)))],
                                });
                                defs.fields.get_mut(name).unwrap().insert(tag, None);
                                for (i, ty) in kinds.iter().enumerate() {
                                    let storage = format!("{stem}$storage{i}");
                                    out.push(Stmt::DerivedDecl {
                                        type_name: ty.clone(),
                                        names: vec![storage.clone()],
                                        attributes: vec![
                                            "allocatable".into(),
                                            "@scalar_heap".into(),
                                            "@referenced_type".into(),
                                        ],
                                        polymorphic: false,
                                        dims: vec![id("@deferred_extent")],
                                    });
                                    defs.fields
                                        .get_mut(name)
                                        .unwrap()
                                        .insert(storage, Some(ty.clone()));
                                }
                            }
                            continue;
                        }
                    }
                    out.push(c);
                }
                *components = out;
            }
            s => {
                for b in children(s) {
                    prepare(b, defs)?;
                }
            }
        }
    }
    Ok(())
}

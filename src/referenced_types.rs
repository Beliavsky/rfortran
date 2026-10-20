// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Resolve Fortran 2023 TYPEOF/CLASSOF declarations before type lowering.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use crate::sema::ImplicitTyping;
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
enum Kind {
    Intrinsic(TypeSpec),
    Derived(String),
    Procedure,
}
#[derive(Clone)]
struct Entity {
    kind: Kind,
    rank: usize,
    optional: bool,
    constant: Option<Expr>,
}
type Entities = HashMap<String, Entity>;
type Types = HashMap<String, Vec<Stmt>>;

fn attributes(body: &[Stmt], name: &str) -> Vec<String> {
    body.iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name: n, intent } if n == name => Some(intent.clone()),
            _ => None,
        })
        .collect()
}
fn source_entity(
    source: &Expr,
    entities: &Entities,
    types: &Types,
    implicit: &ImplicitTyping,
) -> Result<Entity> {
    fn named(
        name: &str,
        entities: &Entities,
        types: &Types,
        implicit: &ImplicitTyping,
    ) -> Result<Entity> {
        let mut path = name.split('%');
        let base = path.next().unwrap();
        if !entities.contains_key(base) && types.contains_key(base) {
            bail!("TYPEOF/CLASSOF requires a data object, not a type name");
        }
        let mut entity = entities
            .get(base)
            .cloned()
            .or_else(|| {
                // Canonical module names and component paths cannot be implicitly typed.
                (!base.starts_with('@'))
                    .then(|| implicit.get_implicit_type(base))
                    .flatten()
                    .map(|kind| Entity {
                        kind: Kind::Intrinsic(kind),
                        rank: 0,
                        optional: false,
                        constant: None,
                    })
            })
            .with_context(|| {
                format!("TYPEOF/CLASSOF reference `{base}` has no previously declared type")
            })?;
        if matches!(entity.kind, Kind::Procedure) {
            bail!("TYPEOF/CLASSOF requires a data object, not a procedure");
        }
        for field in path {
            entity = component(&entity, field, types)?;
        }
        Ok(entity)
    }
    fn component(base: &Entity, field: &str, types: &Types) -> Result<Entity> {
        let Kind::Derived(ty) = &base.kind else {
            bail!("TYPEOF/CLASSOF component reference requires a derived-type object");
        };
        let fields = types
            .get(ty)
            .with_context(|| format!("unknown derived type `{ty}` in TYPEOF/CLASSOF"))?;
        let mut parent = None;
        for s in fields {
            let kind = match s {
                Stmt::VarDecl { kind, names } if names.iter().any(|n| n == field) => {
                    Some((Kind::Intrinsic(kind.clone()), 0))
                }
                Stmt::ArrayDecl { kind, name, dims } if name == field => {
                    Some((Kind::Intrinsic(kind.clone()), dims.len()))
                }
                Stmt::InitializedDecl { kind, entities, .. }
                    if entities.iter().any(|(n, _)| n == field) =>
                {
                    Some((Kind::Intrinsic(kind.clone()), 0))
                }
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } if names.iter().any(|n| n == field) => {
                    Some((Kind::Derived(type_name.clone()), dims.len()))
                }
                Stmt::DummyIntent { name, intent } if name == crate::extensions::EXTENDS => {
                    parent = Some(intent);
                    None
                }
                _ => None,
            };
            if let Some((kind, rank)) = kind {
                return Ok(Entity {
                    kind,
                    rank,
                    optional: base.optional,
                    constant: None,
                });
            }
        }
        if let Some(parent) = parent {
            return component(
                &Entity {
                    kind: Kind::Derived(parent.clone()),
                    rank: 0,
                    optional: base.optional,
                    constant: None,
                },
                field,
                types,
            );
        }
        bail!("unknown component `{field}` of type `{ty}` in TYPEOF/CLASSOF")
    }
    match source {
        Expr::Ident(n) => named(n, entities, types, implicit),
        Expr::Call(tag, args) if tag.starts_with("@record_component:") => {
            let mut value = source_entity(&args[0], entities, types, implicit)?;
            for field in tag["@record_component:".len()..].split('%') {
                value = component(&value, field, types)?;
            }
            Ok(value)
        }
        Expr::Call(n, args) | Expr::Index(n, args) => {
            let mut entity = named(n, entities, types, implicit)?;
            if entity.rank == 0 || args.len() != entity.rank {
                bail!("TYPEOF/CLASSOF requires a data reference, not a function call or scalar subscript");
            }
            entity.rank = 0;
            Ok(entity)
        }
        Expr::Section { name, .. } => {
            let entity = named(name, entities, types, implicit)?;
            if entity.rank != 1 {
                bail!("TYPEOF/CLASSOF section must reference an array");
            }
            Ok(entity)
        }
        _ => bail!("TYPEOF/CLASSOF requires a data reference, not an expression"),
    }
}
fn resolve_kind(
    kind: &TypeSpec,
    entities: &Entities,
    types: &Types,
    implicit: &ImplicitTyping,
) -> Result<(Kind, bool)> {
    let constants = entities
        .iter()
        .filter_map(|(n, e)| e.constant.clone().map(|v| (n.clone(), v)))
        .collect();
    let TypeSpec::Referenced {
        source,
        polymorphic,
    } = kind
    else {
        let kind = crate::parameters::concrete_kind_with(kind.clone(), &constants)
            .unwrap_or_else(|_| kind.clone());
        return Ok((Kind::Intrinsic(kind), false));
    };
    let mut entity = source_entity(source, entities, types, implicit)?;
    if matches!(&entity.kind,Kind::Derived(n) if n=="*") {
        if *polymorphic {
            bail!(
                "CLASSOF of CLASS(*) requires unlimited polymorphism, which is not yet supported"
            );
        }
        bail!("TYPEOF reference must not be unlimited polymorphic");
    }
    if let Kind::Intrinsic(ref kind) = entity.kind {
        if *polymorphic {
            bail!("CLASSOF reference must not have intrinsic type");
        }
        if entity.optional
            && (matches!(kind, TypeSpec::CharacterAssumed) || crate::substrings::deferred(kind))
        {
            bail!("TYPEOF/CLASSOF OPTIONAL reference must not have an assumed or deferred type parameter");
        }
        if matches!(
            kind,
            TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_)
        ) && !crate::substrings::deferred(kind)
        {
            entity.kind = Kind::Intrinsic(TypeSpec::CharacterLength(Box::new(Expr::Call(
                "len".into(),
                vec![*source.clone()],
            ))));
        }
    }
    Ok((entity.kind, *polymorphic))
}

fn scope(
    body: &mut Vec<Stmt>,
    inherited: &Entities,
    inherited_types: &Types,
    inherited_implicit: &ImplicitTyping,
    params: &[String],
    root_contains: Option<&[String]>,
) -> Result<(Entities, Types)> {
    let declared_attributes: HashMap<String, Vec<String>> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, .. } => Some((name.clone(), attributes(body, name))),
            _ => None,
        })
        .collect();
    let attrs_for = |name: &str| declared_attributes.get(name).cloned().unwrap_or_default();
    let mut entities = inherited.clone();
    let mut types = inherited_types.clone();
    let mut implicit = inherited_implicit.clone();
    let mut local = HashSet::new();
    for s in body.iter() {
        match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                local.extend(names.iter().cloned())
            }
            Stmt::InitializedDecl { entities, .. } => {
                local.extend(entities.iter().map(|(n, _)| n.clone()))
            }
            Stmt::ArrayDecl { name, .. }
            | Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. } => {
                local.insert(name.clone());
            }
            Stmt::Implicit(Implicit::None) => implicit.apply_implicit_none(),
            Stmt::Implicit(Implicit::Rule {
                type_spec,
                letter_ranges,
            }) => implicit.add_rule(type_spec.clone(), letter_ranges.clone(), 0..0),
            Stmt::Implicit(Implicit::Rules { rules }) => {
                for rule in rules {
                    implicit.add_rule(rule.type_spec.clone(), rule.letter_ranges.clone(), 0..0);
                }
            }
            _ => {}
        }
    }
    for name in &local {
        entities.remove(name);
    }
    for s in body.iter() {
        if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
            entities.insert(
                name.clone(),
                Entity {
                    kind: Kind::Procedure,
                    rank: 0,
                    optional: false,
                    constant: None,
                },
            );
        }
    }
    let mut offset = None;
    let mut out = vec![];
    // All declarations in this scope are resolved before its contained procedures.
    for mut s in std::mem::take(body) {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(p);
            out.push(Stmt::SourceLocation(p));
            continue;
        }
        let result = (|| -> Result<()> {
            let mut after = vec![];
            let declaration = match &s {
                Stmt::VarDecl { kind, names } => {
                    Some((kind, names.clone(), vec![], vec![], vec![]))
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    Some((kind, vec![name.clone()], dims.clone(), vec![], vec![]))
                }
                Stmt::InitializedDecl {
                    kind,
                    attributes,
                    entities,
                } => Some((
                    kind,
                    entities.iter().map(|(n, _)| n.clone()).collect(),
                    vec![],
                    attributes.clone(),
                    entities.clone(),
                )),
                _ => None,
            };
            if let Some((kind, names, dims, attrs, initializers)) = declaration {
                let (resolved, polymorphic) = resolve_kind(kind, &entities, &types, &implicit)?;
                let referenced = matches!(kind, TypeSpec::Referenced { .. });
                if polymorphic {
                    for name in &names {
                        let all = attrs_for(name);
                        if !params.contains(name)
                            && !attrs
                                .iter()
                                .chain(&all)
                                .any(|a| matches!(a.as_str(), "allocatable" | "pointer"))
                        {
                            bail!("CLASSOF entity `{name}` must be a dummy argument or have ALLOCATABLE or POINTER");
                        }
                    }
                }
                let parameter = attrs.iter().any(|a| a == "parameter");
                match &resolved {
                    Kind::Procedure => {
                        unreachable!("procedure rejected as a declaration reference")
                    }
                    Kind::Intrinsic(ty) => match &mut s {
                        Stmt::VarDecl { kind, .. }
                        | Stmt::ArrayDecl { kind, .. }
                        | Stmt::InitializedDecl { kind, .. } => *kind = ty.clone(),
                        _ => unreachable!(),
                    },
                    Kind::Derived(type_name) => {
                        let mut all = attrs;
                        for name in &names {
                            for a in attrs_for(name) {
                                if !all.contains(&a) {
                                    all.push(a);
                                }
                            }
                        }
                        if initializers.iter().any(|(_, v)| v.is_some())
                            && !all.iter().any(|a| a == "parameter")
                        {
                            all.push("save".into());
                        }
                        // The type is obtained through a visible object, rather
                        // than by naming the (possibly private) type directly.
                        all.push("@referenced_type".into());
                        s = Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: names.clone(),
                            attributes: all,
                            polymorphic,
                            dims: dims.clone(),
                        };
                    }
                }
                for name in names {
                    let optional = attrs_for(&name).iter().any(|a| a == "optional");
                    let constants = entities
                        .iter()
                        .filter_map(|(n, e)| e.constant.clone().map(|v| (n.clone(), v)))
                        .collect();
                    let constant = if parameter {
                        initializers
                            .iter()
                            .find(|(n, _)| n == &name)
                            .and_then(|(_, v)| v.as_ref())
                            .and_then(|v| {
                                crate::parameters::constant_integer_with(v, &constants).ok()
                            })
                            .map(|v| Expr::IntLit(v.to_string()))
                    } else {
                        None
                    };
                    entities.insert(
                        name,
                        Entity {
                            kind: resolved.clone(),
                            rank: dims.len(),
                            optional,
                            constant,
                        },
                    );
                }
                if referenced {
                    for (name, value) in initializers {
                        if let Some(value) = value {
                            if matches!(resolved, Kind::Derived(_)) {
                                after.push(Stmt::Assign { name, value });
                            }
                        }
                    }
                }
            } else {
                match &mut s {
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        ..
                    } => {
                        for name in names {
                            entities.insert(
                                name.clone(),
                                Entity {
                                    kind: Kind::Derived(type_name.clone()),
                                    rank: dims.len(),
                                    optional: attrs_for(name).iter().any(|a| a == "optional"),
                                    constant: None,
                                },
                            );
                        }
                    }
                    Stmt::DerivedType {
                        name, components, ..
                    } => {
                        scope(components, &entities, &types, &implicit, &[], None)?;
                        types.insert(name.clone(), components.clone());
                    }
                    Stmt::Module { body, .. } => {
                        let imports = entities
                            .iter()
                            .filter(|(n, _)| n.starts_with("@module$"))
                            .map(|(n, e)| (n.clone(), e.clone()))
                            .collect();
                        let (module_entities, module_types) =
                            scope(body, &imports, &types, &ImplicitTyping::new(), &[], None)?;
                        entities.extend(
                            module_entities
                                .into_iter()
                                .filter(|(n, _)| n.starts_with("@module$")),
                        );
                        types.extend(
                            module_types
                                .into_iter()
                                .filter(|(n, _)| n.starts_with("@module$")),
                        );
                    }
                    _ => {}
                }
            }
            out.push(s);
            out.extend(after);
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    for s in &mut out {
        match s {
            Stmt::Function {
                name,
                body,
                params,
                return_type,
                ..
            } => {
                let mut inherited: Entities = if root_contains.is_some_and(|c| !c.contains(name)) {
                    entities
                        .iter()
                        .filter(|(n, _)| n.starts_with("@module$"))
                        .map(|(n, e)| (n.clone(), e.clone()))
                        .collect()
                } else {
                    entities.clone()
                };
                inherited.remove(name);
                let procedure_implicit = if root_contains.is_some_and(|c| !c.contains(name)) {
                    ImplicitTyping::new()
                } else {
                    implicit.clone()
                };
                let (function_entities, _) =
                    scope(body, &inherited, &types, &procedure_implicit, params, None)?;
                if matches!(return_type, Some(TypeSpec::Referenced { .. })) {
                    let result = body
                        .last()
                        .and_then(|s| match s {
                            Stmt::Return(Some(Expr::Ident(n))) => Some(n),
                            _ => None,
                        })
                        .unwrap_or(name);
                    *return_type = function_entities.get(result).and_then(|e| match &e.kind {
                        Kind::Intrinsic(k) => Some(k.clone()),
                        _ => None,
                    });
                }
            }
            Stmt::Subroutine {
                name, body, params, ..
            } => {
                let inherited: Entities = if root_contains.is_some_and(|c| !c.contains(name)) {
                    entities
                        .iter()
                        .filter(|(n, _)| n.starts_with("@module$"))
                        .map(|(n, e)| (n.clone(), e.clone()))
                        .collect()
                } else {
                    entities.clone()
                };
                let procedure_implicit = if root_contains.is_some_and(|c| !c.contains(name)) {
                    ImplicitTyping::new()
                } else {
                    implicit.clone()
                };
                scope(body, &inherited, &types, &procedure_implicit, params, None)?;
            }
            Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                scope(body, &entities, &types, &implicit, params, None)?;
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    scope(&mut c.body, &entities, &types, &implicit, params, None)?;
                }
                if let Some(body) = default {
                    scope(body, &entities, &types, &implicit, params, None)?;
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                scope(then_body, &entities, &types, &implicit, params, None)?;
                if let Some(body) = else_body {
                    scope(body, &entities, &types, &implicit, params, None)?;
                }
            }
            _ => {}
        }
    }
    *body = out;
    Ok((entities, types))
}
fn contains(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::VarDecl { kind, .. }
        | Stmt::ArrayDecl { kind, .. }
        | Stmt::InitializedDecl { kind, .. } => matches!(kind, TypeSpec::Referenced { .. }),
        Stmt::DerivedType {
            components: body, ..
        }
        | Stmt::Module { body, .. }
        | Stmt::Function { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. } => contains(body),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => contains(then_body) || else_body.as_ref().is_some_and(|b| contains(b)),
        Stmt::SelectCase { cases, default, .. } => {
            cases.iter().any(|c| contains(&c.body)) || default.as_ref().is_some_and(|b| contains(b))
        }
        _ => false,
    })
}
/// Provenance is needed only while validating the generated type names.
pub(crate) fn clear_provenance(body: &mut [Stmt]) {
    for statement in body {
        if let Stmt::DerivedDecl { attributes, .. } = statement {
            attributes.retain(|a| a != "@referenced_type");
        }
        if let Stmt::DerivedType { components, .. } = statement {
            clear_provenance(components);
        }
        for body in crate::procedure_arguments::children(statement) {
            clear_provenance(body);
        }
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    if !contains(&program.body) {
        return Ok(program);
    }
    scope(
        &mut program.body,
        &Entities::new(),
        &Types::new(),
        &ImplicitTyping::new(),
        &[],
        Some(&program.contains),
    )?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolved(source: &str) -> Program {
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "typeof.f90")
            .unwrap();
        let p = crate::modules::associate(p).unwrap();
        super::resolve(p).unwrap()
    }
    #[test]
    fn typeof_discards_polymorphism_and_classof_preserves_it() {
        let p=resolved("module m\ntype::t\ninteger::n\nend type\ncontains\nsubroutine s(a,b)\nclass(t),intent(in)::a\nclassof(a),intent(in)::b\ntypeof(a)::copy\nend subroutine\nend module");
        let Stmt::Module { body, .. } = &p.body[0] else {
            panic!("module")
        };
        let body = body
            .iter()
            .find_map(|s| match s {
                Stmt::Subroutine { body, .. } => Some(body),
                _ => None,
            })
            .unwrap();
        assert!(body.iter().any(|s|matches!(s,Stmt::DerivedDecl{names,polymorphic:true,..} if names.iter().any(|n|n=="b"))));
        assert!(body.iter().any(|s|matches!(s,Stmt::DerivedDecl{names,polymorphic:false,..} if names.iter().any(|n|n=="copy"))));
    }
    #[test]
    fn typeof_copies_type_without_shape_or_parameter_attribute() {
        let p = resolved("implicit none\ninteger,parameter::a(2)=[1,2]\ntypeof(a)::b\nend");
        assert!(p.body.iter().any(|s|matches!(s,Stmt::VarDecl{kind:TypeSpec::Integer(_),names} if names.iter().any(|n|n=="b"))));
        assert!(!p.body.iter().any(
            |s| matches!(s,Stmt::DummyIntent{name,intent} if name=="b" && intent=="parameter")
        ));
    }
}

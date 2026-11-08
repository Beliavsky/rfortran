// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Locality semantics shared by the serial fallback and future parallel lowering.
use super::*;
pub(super) struct PrivateArray {
    pub name: String,
    pub runtime: bool,
    pub components: i64,
}
pub(super) struct Prepared {
    pub prefix: Vec<Stmt>,
    pub arrays: Vec<PrivateArray>,
    pub cleanup: Vec<Stmt>,
}

fn implicit_object(name: &str, scope: &Scope) -> Object {
    let letter = name.chars().next().unwrap_or('a');
    scope
        .get(&format!("@implicit:{letter}"))
        .cloned()
        .unwrap_or(Object {
            kind: if ('i'..='n').contains(&letter) {
                TypeSpec::Integer(None)
            } else {
                TypeSpec::Real
            },
            dims: vec![],
            attrs: HashSet::new(),
            derived: false, constant: None,
        })
}

fn declaration(name: &str, object: &Object) -> Stmt {
    if object.dims.is_empty() {
        Stmt::VarDecl {
            kind: object.kind.clone(),
            names: vec![name.into()],
        }
    } else {
        Stmt::ArrayDecl {
            kind: object.kind.clone(),
            name: name.into(),
            dims: object.dims.clone(),
        }
    }
}

/// Check source references before introducing compiler temporaries. BLOCK
/// locals were already given distinct names by block_scopes. A nested
/// concurrent construct introduces its own index and LOCAL entities.
fn default_none(body: &[Stmt], scope: &Scope, allowed: &HashSet<String>) -> Result<()> {
    fn check(n: &str, scope: &Scope, allowed: &HashSet<String>) -> Result<()> {
        let root = n.split('%').next().unwrap_or(n);
        if scope
            .get(root)
            .is_some_and(|o| !o.attrs.contains("parameter"))
            && !allowed.contains(root)
        {
            bail!("DEFAULT(NONE) requires a locality specifier for `{root}`");
        }
        Ok(())
    }
    for statement in body {
        let mut s = statement.clone();
        if let Stmt::Block { body } = &s {
            if let Some(Stmt::CallSub { name, args }) = body.first() {
                if matches!(name.as_str(), "@do_concurrent" | "@concurrent_reduce") {
                    let mut nested = allowed.clone();
                    if let Some(Expr::Call(_, controls)) = args.first() {
                        for c in controls.chunks(4) {
                            if let Expr::Ident(n) = &c[0] {
                                nested.insert(n.clone());
                            }
                            for e in &c[1..] {
                                default_none(&[set("", e.clone())], scope, allowed)?;
                            }
                        }
                    }
                    for clause in &args[2..] {
                        if let Expr::Call(tag, names) = clause {
                            for e in names {
                                if let Expr::Ident(n) = e {
                                    if tag != "@locality:local" {
                                        check(n, scope, allowed)?;
                                    }
                                    if matches!(
                                        tag.as_str(),
                                        "@locality:local" | "@locality:local_init"
                                    ) {
                                        nested.insert(n.clone());
                                    }
                                }
                            }
                        }
                    }
                    if let Some(mask) = args.get(1) {
                        default_none(&[set("", mask.clone())], scope, &nested)?;
                    }
                    default_none(&body[1..], scope, &nested)?;
                    continue;
                }
            }
        }
        if let Stmt::Assign { name, .. }
        | Stmt::AssignIndex { name, .. }
        | Stmt::Do { var: name, .. } = &s
        {
            let name = name.split('%').next().unwrap_or(name);
            if !scope.contains_key(name)
                && !allowed.contains(name)
                && !name.is_empty()
                && !name.starts_with('@')
                && !name.contains("$block")
            {
                bail!("DEFAULT(NONE) requires a locality specifier for `{name}`");
            }
            check(name, scope, allowed)?;
        }
        let mut error = None;
        for e in expressions(&mut s) {
            // Hidden inputs preserve reads inside an accessible PURE callee;
            // they are not source references subject to this loop's locality
            // list. Do not require users to name private/USE-excluded data.
            visit_expr(e, &mut |e| {
                if matches!(e,Expr::Call(tag,_) if tag.starts_with("@keyword:@parallel_read_")) {
                    *e=int(0);
                }
            });
            visit_expr(e, &mut |e| {
                if let Expr::Ident(n) = &*e {
                    let root = n.split('%').next().unwrap_or(n);
                    if error.is_none()
                        && !scope.contains_key(root)
                        && !allowed.contains(root)
                        && !root.starts_with('@')
                        && !root.contains("$block")
                    {
                        error = Some(anyhow::anyhow!(
                            "DEFAULT(NONE) requires a locality specifier for `{root}`"
                        ));
                    }
                }
                if let Expr::Ident(n)
                | Expr::Call(n, _)
                | Expr::Index(n, _)
                | Expr::Section { name: n, .. } = e
                {
                    if error.is_none() {
                        error = check(n, scope, allowed).err();
                    }
                }
            });
        }
        if let Some(e) = error {
            return Err(e);
        }
        for b in children(&mut s) {
            default_none(b, scope, allowed)?;
        }
    }
    Ok(())
}

pub(super) fn validate(controls: &[Expr], body: &[Stmt], clauses: &[Expr],
                       scope: &Scope, implicit_none: bool) -> Result<()> {
    let mut indices = HashSet::new();
    for c in controls.chunks(4) {
        let Expr::Ident(n) = &c[0] else {
            bail!("invalid concurrent index");
        };
        if !indices.insert(n.clone()) {
            bail!("duplicate DO CONCURRENT index `{n}`");
        }
        if let Some(o) = scope.get(n) {
            if o.derived
                || !o.dims.is_empty()
                || !matches!(o.kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_))
            {
                bail!("DO CONCURRENT index `{n}` must have scalar INTEGER type");
            }
        } else if implicit_none {
            bail!("DO CONCURRENT index `{n}` is not declared under IMPLICIT NONE");
        } else if !matches!(
            implicit_object(n, scope).kind,
            TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)
        ) {
            bail!("DO CONCURRENT index `{n}` must have scalar INTEGER type");
        }
    }
    for c in controls.chunks(4) {
        if c[1..].iter().any(|e| mentions(e, &indices)) {
            bail!(
                "DO CONCURRENT bounds and stride cannot reference an index of the same construct"
            );
        }
    }
    let mut locality = HashMap::new();
    let mut default = false;
    for clause in clauses.iter() {
        let Expr::Call(tag, variables) = clause else {
            bail!("invalid concurrent locality clause");
        };
        if tag == "@locality:default_none" {
            if default {
                bail!("duplicate DEFAULT(NONE)");
            }
            default = true;
            continue;
        }
        for e in variables {
            let Expr::Ident(n) = e else {
                bail!("locality requires a variable name");
            };
            if let Some(previous) = locality.insert(n.clone(), tag.clone()) {
                if previous.starts_with("@reduce:") && tag.starts_with("@reduce:") {
                    bail!("duplicate REDUCE variable `{n}`");
                }
                bail!("duplicate locality specifier for `{n}`");
            }
            if indices.contains(n) {
                if tag.starts_with("@reduce:") {
                    bail!("REDUCE variable `{n}` cannot be a concurrent index");
                }
                bail!("concurrent index `{n}` cannot appear in a locality specifier");
            }
            if !tag.starts_with("@reduce:") {
                let object = scope
                    .get(n)
                    .with_context(|| format!("locality variable `{n}` requires a declaration"))?;
                if object.attrs.contains("parameter") {
                    bail!("locality specifier requires a variable, not PARAMETER `{n}`");
                }
            }
        }
    }
    if default {
        let mut allowed = indices.clone();
        allowed.extend(locality.keys().cloned());
        default_none(body, scope, &allowed)?;
    }
    Ok(())
}

pub(super) fn prepare(
    controls: &mut [Expr],
    mask: &mut Expr,
    body: &mut Vec<Stmt>,
    clauses: &mut Vec<Expr>,
    scope: &mut Scope,
    serial: &mut usize,
) -> Result<Prepared> {
    let mut prefix = vec![];
    let mut each = vec![];
    let mut private_arrays = vec![];
    let mut cleanup = vec![];
    let constants=scope.iter().filter_map(|(n,o)|o.constant.as_ref()
        .filter(|_|o.attrs.contains("parameter")).map(|e|(n.clone(),e.clone()))).collect();
    // Preserve source clause order for reproducible generated code.
    for clause in clauses.iter() {
        let Expr::Call(tag, variables) = clause else {
            unreachable!()
        };
        if !matches!(tag.as_str(), "@locality:local" | "@locality:local_init") {
            continue;
        }
        for variable in variables {
            let Expr::Ident(n) = variable else {
                unreachable!()
            };
            let object = &scope[n];
            if object.derived {
                bail!("LOCAL/LOCAL_INIT of derived-type variable `{n}` is not yet supported");
            }
            for attr in [
                "intent(in)",
                "in",
                "optional",
                "allocatable",
                "pointer",
                "asynchronous",
                "volatile",
                "protected",
            ] {
                if object.attrs.contains(attr) {
                    bail!("LOCAL/LOCAL_INIT variable `{n}` with {attr} is not supported");
                }
            }
            let runtime=object.dims.iter().any(|dim|match dim {
                Expr::Call(tag,a) if tag=="@declared_bounds" =>
                    a.iter().any(|e|crate::parameters::evaluate(e,&constants).is_err()),
                _=>crate::parameters::evaluate(dim,&constants).is_err(),
            });
            if tag == "@locality:local" {
                let names = HashSet::from([n.clone()]);
                if mentions(mask, &names)
                    || controls
                        .chunks(4)
                        .any(|c| c[1..].iter().any(|e| mentions(e, &names)))
                {
                    bail!("LOCAL variable `{n}` cannot appear in the concurrent header");
                }
            }
            *serial += 1;
            let temporary = format!("{n}$block_concurrent{serial}");
            let mut declare=|name:&str| {
                if runtime {
                    prefix.push(Stmt::ArrayDecl{kind:object.kind.clone(),name:name.into(),dims:vec![id("@deferred_extent");object.dims.len()]});
                    prefix.push(Stmt::DummyIntent{name:name.into(),intent:"allocatable".into()});
                    // Inherit the established shape, not a reevaluation of
                    // specification expressions that may since have changed.
                    let bounds=(1..=object.dims.len()).map(|axis|Expr::Call("@heap_bounds".into(),vec![
                        Expr::Call("lbound".into(),vec![id(n),int(axis as i64)]),
                        Expr::Call("ubound".into(),vec![id(n),int(axis as i64)])])).collect();
                    prefix.push(Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(name.into(),bounds)]});
                    cleanup.push(Stmt::CallSub{name:"@deallocate".into(),args:vec![id(name)]});
                } else {prefix.push(declaration(name,object));}
            };
            declare(&temporary);
            if !object.dims.is_empty() {private_arrays.push(PrivateArray{
                name:temporary.clone(),runtime,components:if matches!(object.kind,TypeSpec::Complex(_)) {2}else{1}
            });}
            if tag == "@locality:local_init" {
                let saved = format!("@concurrent_initial_{serial}");
                declare(&saved);
                prefix.push(if runtime {section_assignment(&saved,object.dims.len(),Expr::Index(n.clone(),whole_sections(n,object.dims.len())))}
                    else {set(&saved,id(n))});
                each.push(if runtime {section_assignment(&temporary,object.dims.len(),id(&saved))}
                    else {set(&temporary,id(&saved))});
            } else {
                each.push(Stmt::CallSub {
                    name: "@undefine".into(),
                    args: vec![id(&temporary)],
                });
            }
            if runtime {
                fn fixed_shape(body:&mut [Stmt],name:&str,rank:usize) {
                    for s in body {
                        if let Stmt::Assign{name:n,value}=s {
                            if n==name {*s=section_assignment(name,rank,value.clone());}
                        }
                        for b in children(s) {fixed_shape(b,name,rank);}
                    }
                }
                fixed_shape(body,n,object.dims.len());
            }
            crate::block_scopes::rename(body, n, &temporary);
        }
    }
    // Index names are construct entities, not definitions of the outside
    // objects. Rename their uses only in the mask and iteration body.
    for c in controls.chunks_mut(4) {
        let Expr::Ident(n) = &c[0] else {
            unreachable!()
        };
        let n = n.clone();
        let object = scope
            .get(&n)
            .cloned()
            .unwrap_or_else(|| implicit_object(&n, scope));
        *serial += 1;
        let index = format!("{n}$block_concurrent{serial}");
        prefix.push(declaration(&index, &object));
        scope.insert(index.clone(), object);
        crate::block_scopes::rename(body, &n, &index);
        visit_expr(mask, &mut |e| {
            if let Expr::Ident(v) = e {
                if v == &n {
                    *v = index.clone();
                }
            }
        });
        c[0] = id(&index);
    }
    each.append(body);
    *body = each;
    clauses.retain(|e| !matches!(e, Expr::Call(n, _) if n.starts_with("@locality:")));
    Ok(Prepared{prefix,arrays:private_arrays,cleanup})
}

fn section_assignment(name:&str,rank:usize,value:Expr)->Stmt {
    Stmt::AssignIndex{name:name.into(),indices:whole_sections(name,rank),value}
}
fn whole_sections(name:&str,rank:usize)->Vec<Expr> {
    (0..rank).map(|_|Expr::Call("@section_subscript".into(),vec![Expr::Section{
        name:name.into(),lower:None,upper:None,stride:None
    }])).collect()
}

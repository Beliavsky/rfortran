// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Locality semantics shared by the serial fallback and future parallel lowering.
use super::*;

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

pub(super) fn prepare(
    controls: &mut [Expr],
    mask: &mut Expr,
    body: &mut Vec<Stmt>,
    clauses: &mut Vec<Expr>,
    scope: &mut Scope,
    serial: &mut usize,
    implicit_none: bool,
) -> Result<(Vec<Stmt>, Vec<String>)> {
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
    let mut prefix = vec![];
    let mut each = vec![];
    let mut private_arrays = vec![];
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
            for dim in &object.dims {
                let mut runtime = false;
                visit_expr(&mut dim.clone(), &mut |e| {
                    if let Expr::Ident(v) = e {
                        runtime |= v.starts_with('@')
                            || scope.get(v).is_some_and(|o| !o.attrs.contains("parameter"));
                    }
                });
                if runtime {
                    bail!(
                        "LOCAL/LOCAL_INIT array `{n}` currently requires constant explicit bounds"
                    );
                }
            }
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
            prefix.push(declaration(&temporary, object));
            if !object.dims.is_empty() {private_arrays.push(temporary.clone());}
            if tag == "@locality:local_init" {
                let saved = format!("@concurrent_initial_{serial}");
                prefix.push(declaration(&saved, object));
                prefix.push(set(&saved, id(n)));
                each.push(set(&temporary, id(&saved)));
            } else {
                each.push(Stmt::CallSub {
                    name: "@undefine".into(),
                    args: vec![id(&temporary)],
                });
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
    Ok((prefix, private_arrays))
}

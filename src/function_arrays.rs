// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Pass fixed-size read-only array arguments through the existing scalar ABI.
//! This bounded lowering copies each input element; actual arrays must be defined.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Arrays = HashMap<String, (TypeSpec, Vec<Expr>)>;
#[derive(Clone)]
struct ArrayParam {
    name: String,
    kind: TypeSpec,
    dims: Vec<i64>,
    elements: Vec<String>,
}
#[derive(Clone)]
struct Signature {
    params: Vec<String>,
    arrays: HashMap<String, ArrayParam>,
}
fn extent(dims: &[i64]) -> Result<usize> {
    let mut total = 1usize;
    for &n in dims {
        if n < 0 {
            bail!("negative function array extent");
        }
        total = total
            .checked_mul(n as usize)
            .context("function array extent overflow")?;
    }
    if total > 1024 {
        bail!("array function arguments currently support at most 1024 elements per dummy");
    }
    Ok(total)
}
fn indices(mut ordinal: usize, dims: &[i64]) -> Vec<Expr> {
    dims.iter()
        .map(|&n| {
            let value = ordinal % n as usize + 1;
            ordinal /= n as usize;
            Expr::IntLit(value.to_string())
        })
        .collect()
}
fn collect(
    body: &[Stmt],
    signatures: &mut HashMap<String, Signature>,
    serial: &mut usize,
) -> Result<()> {
    for s in body {
        match s {
            Stmt::Function {
                name,
                params,
                body,
                pure,
                ..
            } => {
                let mut arrays = HashMap::new();
                for s in body {
                    if let Stmt::ArrayDecl {
                        name: param,
                        kind,
                        dims,
                    } = s
                    {
                        if !params.contains(param) {
                            continue;
                        }
                        if !pure {
                            bail!("array-argument function `{name}` currently requires PURE and INTENT(IN) dummies");
                        }
                        if !body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == param && intent == "intent(in)")) {
                            bail!("array dummy `{param}` in function `{name}` requires INTENT(IN)");
                        }
                        if body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == param && matches!(intent.as_str(), "optional" | "allocatable"))) {
                            bail!("OPTIONAL/ALLOCATABLE array dummy `{param}` in function `{name}` is not yet supported");
                        }
                        if !matches!(
                            kind,
                            TypeSpec::Integer(_)
                                | TypeSpec::Real
                                | TypeSpec::DoublePrecision
                                | TypeSpec::Logical
                        ) {
                            bail!(
                                "function array dummy `{param}` requires a numeric or LOGICAL type"
                            );
                        }
                        let dims = dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>()
                            .with_context(|| format!("function array dummy `{param}` currently requires fixed constant extents"))?;
                        let count = extent(&dims)?;
                        *serial += 1;
                        let elements = (0..count)
                            .map(|i| format!("@function_array_{}_{}", serial, i))
                            .collect();
                        arrays.insert(
                            param.clone(),
                            ArrayParam {
                                name: param.clone(),
                                kind: kind.clone(),
                                dims,
                                elements,
                            },
                        );
                    }
                }
                if !arrays.is_empty()
                    && signatures
                        .insert(
                            name.clone(),
                            Signature {
                                params: params.clone(),
                                arrays,
                            },
                        )
                        .is_some()
                {
                    bail!("ambiguous array-argument function `{name}`");
                }
                collect(body, signatures, serial)?;
            }
            Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => {
                collect(body, signatures, serial)?
            }
            _ => {}
        }
    }
    Ok(())
}
fn expression(
    e: &mut Expr,
    arrays: &Arrays,
    signatures: &HashMap<String, Signature>,
) -> Result<()> {
    match e {
        Expr::Call(name, args) => {
            for arg in args.iter_mut() {
                expression(arg, arrays, signatures)?;
            }
            if arrays.contains_key(name) {
                return Ok(());
            }
            if let Some(sig) = signatures.get(name) {
                if args.len() != sig.params.len() {
                    bail!(
                        "array-argument function `{name}` currently requires all {} arguments",
                        sig.params.len()
                    );
                }
                let mut expanded = Vec::new();
                for (param, actual) in sig.params.iter().zip(args.iter()) {
                    if let Some(dummy) = sig.arrays.get(param) {
                        let Expr::Ident(actual_name) = actual else {
                            bail!("function array dummy `{param}` currently requires a whole-array actual");
                        };
                        let (kind, dims) = arrays
                            .get(actual_name)
                            .context("function array actual must be a declared array")?;
                        let dims = dims
                            .iter()
                            .map(crate::parameters::integer_constant)
                            .collect::<Result<Vec<_>>>()
                            .context("function array actual must have fixed constant extents")?;
                        let same_kind = kind == &dummy.kind
                            || matches!(
                                (kind, &dummy.kind),
                                (
                                    TypeSpec::Integer(None | Some(4)),
                                    TypeSpec::Integer(None | Some(4))
                                )
                            );
                        if !same_kind || dims != dummy.dims {
                            bail!("type or shape mismatch for function array dummy `{param}`");
                        }
                        for i in 0..dummy.elements.len() {
                            expanded.push(Expr::Index(actual_name.clone(), indices(i, &dims)));
                        }
                    } else {
                        expanded.push(actual.clone());
                    }
                }
                *args = expanded;
            }
        }
        Expr::Index(_, args) => {
            for arg in args {
                expression(arg, arrays, signatures)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, arrays, signatures)?;
            expression(b, arrays, signatures)?;
        }
        Expr::Un(_, a) => expression(a, arrays, signatures)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                expression(e, arrays, signatures)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn lower(
    body: &mut [Stmt],
    inherited: &Arrays,
    signatures: &HashMap<String, Signature>,
) -> Result<()> {
    let mut arrays = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, kind, dims } => {
                arrays.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    arrays.remove(n);
                }
            }
            _ => {}
        }
    }
    for s in body {
        match s {
            Stmt::Assign { value, .. } => expression(value, &arrays, signatures)?,
            Stmt::AssignIndex { indices, value, .. } => {
                for e in indices {
                    expression(e, &arrays, signatures)?;
                }
                expression(value, &arrays, signatures)?;
            }
            Stmt::Return(Some(e)) => expression(e, &arrays, signatures)?,
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expression(e, &arrays, signatures)?;
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, &arrays, signatures)?;
                lower(then_body, &arrays, signatures)?;
                if let Some(b) = else_body {
                    lower(b, &arrays, signatures)?;
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                expression(start, &arrays, signatures)?;
                expression(end, &arrays, signatures)?;
                lower(body, &arrays, signatures)?;
            }
            Stmt::DoWhile { cond, body } => {
                expression(cond, &arrays, signatures)?;
                lower(body, &arrays, signatures)?;
            }
            Stmt::Block { body } => lower(body, &arrays, signatures)?,
            Stmt::Module { body, .. } => lower(body, &Arrays::new(), signatures)?,
            Stmt::Subroutine { body, .. } => lower(body, &arrays, signatures)?,
            Stmt::Function {
                name, params, body, ..
            } => {
                lower(body, &arrays, signatures)?;
                if let Some(sig) = signatures.get(name) {
                    let mut new_params = Vec::new();
                    let mut prefix = Vec::new();
                    for p in &sig.params {
                        if let Some(dummy) = sig.arrays.get(p) {
                            for (i, scalar) in dummy.elements.iter().enumerate() {
                                new_params.push(scalar.clone());
                                prefix.push(Stmt::VarDecl {
                                    kind: dummy.kind.clone(),
                                    names: vec![scalar.clone()],
                                });
                                prefix.push(Stmt::DummyIntent {
                                    name: scalar.clone(),
                                    intent: "intent(in)".into(),
                                });
                                prefix.push(Stmt::AssignIndex {
                                    name: dummy.name.clone(),
                                    indices: indices(i, &dummy.dims),
                                    value: Expr::Ident(scalar.clone()),
                                });
                            }
                        } else {
                            new_params.push(p.clone());
                        }
                    }
                    body.retain(|s| !matches!(s,Stmt::DummyIntent{name,..} if sig.arrays.contains_key(name)));
                    let (mut declarations, copies): (Vec<_>, Vec<_>) = prefix
                        .into_iter()
                        .partition(|s| !matches!(s, Stmt::AssignIndex { .. }));
                    let first_exec = body
                        .iter()
                        .position(|s| {
                            !matches!(
                                s,
                                Stmt::VarDecl { .. }
                                    | Stmt::ArrayDecl { .. }
                                    | Stmt::DummyIntent { .. }
                                    | Stmt::Implicit(_)
                                    | Stmt::Use { .. }
                                    | Stmt::SourceLocation(_)
                            )
                        })
                        .unwrap_or(body.len());
                    let mut executable = body.split_off(first_exec);
                    declarations.append(body);
                    declarations.push(Stmt::Block { body: copies });
                    declarations.append(&mut executable);
                    *body = declarations;
                    *params = new_params;
                }
            }
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                expression(expr, &arrays, signatures)?;
                for c in cases {
                    lower(&mut c.body, &arrays, signatures)?;
                }
                if let Some(b) = default {
                    lower(b, &arrays, signatures)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn resolve(mut program: Program) -> Result<Program> {
    let mut signatures = HashMap::new();
    collect(&program.body, &mut signatures, &mut 0)?;
    lower(&mut program.body, &Arrays::new(), &signatures)?;
    Ok(program)
}

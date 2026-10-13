// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate timing intrinsic outputs and lower them to ordinary assignments.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
fn assign(target: &Expr, value: Expr) -> Stmt {
    match target {
        Expr::Ident(name) => Stmt::Assign {
            name: name.clone(),
            value,
        },
        Expr::Index(name, indices) | Expr::Call(name, indices) => Stmt::AssignIndex {
            name: name.clone(),
            indices: indices.clone(),
            value,
        },
        _ => unreachable!("validated output"),
    }
}
pub fn resolve(mut p: Program) -> Result<Program> {
    fn collect(body: &[Stmt], procedures: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    procedures.insert(name.clone());
                    collect(body, procedures);
                }
                Stmt::Module { body, .. } | Stmt::Block { body } => collect(body, procedures),
                _ => {}
            }
        }
    }
    fn scope(
        body: &mut Vec<Stmt>,
        inherited: &HashMap<String, (TypeSpec, usize)>,
        inherited_readonly: &HashSet<String>,
        procedures: &HashSet<String>,
        pure: bool,
        serial: &mut usize,
    ) -> Result<()> {
        let mut vars = inherited.clone();
        let mut readonly = inherited_readonly.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        readonly.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    readonly.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        readonly.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        vars.insert(n.clone(), (kind.clone(), 0));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    vars.insert(name.clone(), (kind.clone(), dims.len()));
                }
                Stmt::InitializedDecl {
                    kind,
                    entities,
                    attributes,
                } => {
                    for (name, _) in entities {
                        vars.entry(name.clone())
                            .or_insert_with(|| (kind.clone(), 0));
                        if attributes.iter().any(|a| a == "parameter") {
                            readonly.insert(name.clone());
                        }
                    }
                }
                Stmt::DummyIntent { name, intent }
                    if matches!(intent.as_str(), "parameter" | "intent(in)") =>
                {
                    readonly.insert(name.clone());
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(offset) = &s {
                location = Some(*offset);
            }
            let result = match &mut s {
                Stmt::CallSub { name, args }
                    if matches!(name.as_str(), "cpu_time" | "system_clock")
                        && !procedures.contains(name) =>
                {
                    lower_call(name, args, &vars, &readonly, pure, serial).map(|statements| {
                        s = Stmt::Block { body: statements };
                    })
                }
                Stmt::Function { body, pure, .. } | Stmt::Subroutine { body, pure, .. } => {
                    scope(body, &vars, &readonly, procedures, *pure, serial)
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => {
                    scope(body, &vars, &readonly, procedures, pure, serial)
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scope(then_body, &vars, &readonly, procedures, pure, serial)?;
                    if let Some(body) = else_body {
                        scope(body, &vars, &readonly, procedures, pure, serial)?;
                    }
                    Ok(())
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        scope(&mut case.body, &vars, &readonly, procedures, pure, serial)?;
                    }
                    if let Some(body) = default {
                        scope(body, &vars, &readonly, procedures, pure, serial)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
            result.map_err(|error| crate::source_diagnostics::at(error, location))?;
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    let mut procedures = HashSet::new();
    collect(&p.body, &mut procedures);
    scope(
        &mut p.body,
        &HashMap::new(),
        &HashSet::new(),
        &procedures,
        false,
        &mut 0,
    )?;
    Ok(p)
}

fn lower_call(
    name: &str,
    args: &mut Vec<Expr>,
    vars: &HashMap<String, (TypeSpec, usize)>,
    readonly: &HashSet<String>,
    pure: bool,
    serial: &mut usize,
) -> Result<Vec<Stmt>> {
    let intrinsic = name.to_uppercase();
    if pure {
        bail!("{intrinsic} is not permitted in a PURE procedure");
    }
    crate::named_arguments::intrinsic(name)
        .unwrap()
        .reorder(name, args)?;
    let mut kinds = vec![];
    for (slot, arg) in args.iter().enumerate() {
        if crate::named_arguments::omitted(arg) {
            kinds.push(None);
            continue;
        }
        let variable = match arg {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => n,
            _ => bail!("{intrinsic} requires a writable scalar variable"),
        };
        let (kind, rank) = vars
            .get(variable)
            .context("timing intrinsic requires a declared writable scalar variable")?;
        let scalar = match arg {
            Expr::Ident(_) => *rank == 0,
            Expr::Index(_, indices) | Expr::Call(_, indices) => {
                *rank > 0
                    && indices.len() == *rank
                    && indices.iter().all(|index| scalar_index(index, vars))
            }
            _ => false,
        };
        if !scalar {
            bail!("{intrinsic} requires a scalar output");
        }
        if readonly.contains(variable) {
            bail!("{intrinsic} requires a writable variable");
        }
        let real = matches!(kind, TypeSpec::Real | TypeSpec::DoublePrecision);
        if name == "cpu_time" && !real {
            bail!("CPU_TIME TIME requires a REAL variable");
        }
        if name == "system_clock"
            && !(matches!(kind, TypeSpec::Integer(None | Some(4 | 8))) || slot == 1 && real)
        {
            bail!(
                "SYSTEM_CLOCK {} requires {}",
                ["COUNT", "COUNT_RATE", "COUNT_MAX"][slot],
                if slot == 1 {
                    "INTEGER kind 4 or 8, or REAL"
                } else {
                    "INTEGER kind 4 or 8"
                }
            );
        }
        kinds.push(Some(kind.clone()));
    }
    let mut out = vec![];
    // Bind subscript expressions before writing any output, as procedure actuals do.
    for arg in args.iter_mut() {
        if crate::named_arguments::omitted(arg) {
            continue;
        }
        if let Expr::Index(_, indices) | Expr::Call(_, indices) = arg {
            for index in indices {
                let temp = temporary(serial, "subscript");
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![temp.clone()],
                });
                out.push(Stmt::Assign {
                    name: temp.clone(),
                    value: index.clone(),
                });
                *index = Expr::Ident(temp);
            }
        }
    }
    if name == "cpu_time" {
        let value = Expr::Call("@cpu_seconds".into(), vec![]);
        let value = if matches!(kinds[0], Some(TypeSpec::Real)) {
            Expr::Call("real".into(), vec![value])
        } else {
            value
        };
        out.push(assign(&args[0], value));
        return Ok(out);
    }
    if args.is_empty() {
        return Ok(out);
    }
    // Select COUNT's representation first, then MAX, then an integer RATE.
    let kind = [0, 2, 1]
        .into_iter()
        .find_map(|slot| match kinds.get(slot) {
            Some(Some(TypeSpec::Integer(bytes))) => Some(bytes.unwrap_or(4)),
            _ => None,
        })
        .unwrap_or(4);
    if kinds
        .iter()
        .flatten()
        .any(|k| matches!(k, TypeSpec::Integer(bytes) if bytes.unwrap_or(4) != kind))
    {
        bail!("SYSTEM_CLOCK integer arguments must have the same kind");
    }
    let temp = temporary(serial, "snapshot");
    out.push(Stmt::VarDecl {
        kind: TypeSpec::Integer(Some(8)),
        names: vec![temp.clone()],
    });
    out.push(Stmt::Assign {
        name: temp.clone(),
        value: Expr::Call("@clock_count".into(), vec![Expr::IntLit(kind.to_string())]),
    });
    if !crate::named_arguments::omitted(&args[0]) {
        out.push(assign(&args[0], Expr::Ident(temp.clone())));
    }
    let mut success = vec![];
    let mut failure = vec![];
    for slot in 1..args.len() {
        if crate::named_arguments::omitted(&args[slot]) {
            continue;
        }
        let value = if slot == 1 {
            crate::timing::rate(i64::from(kind))
        } else {
            crate::timing::maximum(i64::from(kind))
        };
        let real = matches!(
            kinds[slot],
            Some(TypeSpec::Real | TypeSpec::DoublePrecision)
        );
        let literal = |n: i64| {
            if real {
                Expr::RealLit(format!("{n}.0"))
            } else {
                Expr::IntLit(n.to_string())
            }
        };
        success.push(assign(&args[slot], literal(value)));
        failure.push(assign(&args[slot], literal(0)));
    }
    out.push(Stmt::If {
        cond: Expr::Bin(
            crate::ast::BinOp::Ge,
            Box::new(Expr::Ident(temp)),
            Box::new(Expr::IntLit("0".into())),
        ),
        then_body: success,
        else_body: Some(failure),
    });
    Ok(out)
}
fn temporary(serial: &mut usize, label: &str) -> String {
    let name = format!("@clock_{label}_{}", *serial);
    *serial += 1;
    name
}
fn scalar_index(expr: &Expr, vars: &HashMap<String, (TypeSpec, usize)>) -> bool {
    match expr {
        Expr::Ident(n) => !vars.get(n).is_some_and(|(_, rank)| *rank != 0),
        Expr::Bin(_, a, b) => scalar_index(a, vars) && scalar_index(b, vars),
        Expr::Un(_, a) => scalar_index(a, vars),
        Expr::Call(n, _)
            if n.starts_with("@section")
                || n == "@range"
                || n == "@array_constructor"
                || n == "@typed_constructor" =>
        {
            false
        }
        Expr::Call(n, args) | Expr::Index(n, args)
            if vars.get(n).is_some_and(|(_, rank)| *rank > 0) =>
        {
            args.iter().all(|a| scalar_index(a, vars))
        }
        Expr::Call(_, args) => args.iter().all(|a| scalar_index(a, vars)),
        _ => true,
    }
}

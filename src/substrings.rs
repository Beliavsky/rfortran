// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve fixed-bound scalar substrings before array-section lowering.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Characters = HashMap<String, usize>;
fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn slice(name: &str, start: usize, length: usize) -> Expr {
    Expr::Call(
        "@substring".into(),
        vec![Expr::Ident(name.into()), int(start), int(length)],
    )
}
fn bounds(section: &Expr, length: usize) -> Result<(usize, usize)> {
    let Expr::Section {
        lower,
        upper,
        stride,
        ..
    } = section
    else {
        bail!("scalar CHARACTER subscripts require a substring range");
    };
    if stride.is_some() {
        bail!("CHARACTER substrings do not allow a stride");
    }
    let constant = |e: &Expr| {
        crate::parameters::integer_constant(e)
            .context("substring bounds must currently be constant integers")
    };
    let lower = lower.as_deref().map(constant).transpose()?.unwrap_or(1);
    let upper = upper
        .as_deref()
        .map(constant)
        .transpose()?
        .unwrap_or(length as i64);
    if lower < 1 || lower > length as i64 + 1 || upper < 0 || upper > length as i64 {
        bail!("substring bounds {lower}:{upper} are outside CHARACTER length {length}");
    }
    Ok(((lower - 1) as usize, (upper - lower + 1).max(0) as usize))
}
fn expression(e: &mut Expr, chars: &Characters) -> Result<()> {
    match e {
        Expr::Section { name, .. } if chars.contains_key(name) => {
            let name = name.clone();
            let (start, length) = bounds(e, chars[&name])?;
            *e = slice(&name, start, length);
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                expression(e, chars)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, chars)?;
            expression(b, chars)?;
        }
        Expr::Un(_, a) => expression(a, chars)?,
        _ => {}
    }
    Ok(())
}
fn body(statements: &mut Vec<Stmt>, inherited: &Characters, serial: &mut usize) -> Result<()> {
    let mut chars = inherited.clone();
    for s in statements.iter() {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    chars.remove(n);
                    if let TypeSpec::Character(length) = kind {
                        chars.insert(n.clone(), length.unwrap_or(1));
                    }
                }
            }
            Stmt::ArrayDecl { name, .. } => {
                chars.remove(name);
            }
            _ => {}
        }
    }
    let mut output = Vec::new();
    for mut s in std::mem::take(statements) {
        if let Stmt::AssignIndex {
            name,
            indices,
            value,
        } = &mut s
        {
            if let Some(&total) = chars.get(name) {
                let [section] = indices.as_slice() else {
                    bail!("scalar CHARACTER assignment requires one substring range");
                };
                let (start, length) = bounds(section, total)?;
                expression(value, &chars)?;
                *serial += 1;
                let temp = format!("@substring_value_{serial}");
                output.push(Stmt::VarDecl {
                    kind: TypeSpec::Character(Some(length)),
                    names: vec![temp.clone()],
                });
                output.push(Stmt::Assign {
                    name: temp.clone(),
                    value: value.clone(),
                });
                // Empty destinations perform no write. Full destinations need
                // no read of the previous value; partial ones preserve it.
                if length > 0 || total == 0 {
                    let mut value = Expr::Ident(temp);
                    if start > 0 {
                        value = Expr::Bin(
                            BinOp::Concat,
                            Box::new(slice(name, 0, start)),
                            Box::new(value),
                        );
                    }
                    if start + length < total {
                        value = Expr::Bin(
                            BinOp::Concat,
                            Box::new(value),
                            Box::new(slice(name, start + length, total - start - length)),
                        );
                    }
                    output.push(Stmt::Assign {
                        name: name.clone(),
                        value,
                    });
                }
                continue;
            }
        }
        match &mut s {
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => expression(value, &chars)?,
            Stmt::AssignIndex { indices, value, .. } => {
                for e in indices {
                    expression(e, &chars)?;
                }
                expression(value, &chars)?;
            }
            Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expression(e, &chars)?;
                }
            }
            Stmt::Read { args } => {
                if args
                    .iter()
                    .any(|e| matches!(e,Expr::Section{name,..} if chars.contains_key(name)))
                {
                    bail!("READ into a substring is not yet supported");
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, &chars)?;
                body(then_body, &chars, serial)?;
                if let Some(b) = else_body {
                    body(b, &chars, serial)?;
                }
            }
            Stmt::Do {
                start,
                end,
                body: b,
                ..
            } => {
                expression(start, &chars)?;
                expression(end, &chars)?;
                body(b, &chars, serial)?;
            }
            Stmt::DoWhile { cond, body: b } => {
                expression(cond, &chars)?;
                body(b, &chars, serial)?;
            }
            Stmt::Block { body: b }
            | Stmt::Module { body: b, .. }
            | Stmt::Function { body: b, .. }
            | Stmt::Subroutine { body: b, .. } => body(b, &chars, serial)?,
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                expression(expr, &chars)?;
                for c in cases {
                    body(&mut c.body, &chars, serial)?;
                }
                if let Some(b) = default {
                    body(b, &chars, serial)?;
                }
            }
            _ => {}
        }
        output.push(s);
    }
    *statements = output;
    Ok(())
}
pub fn resolve(mut program: Program) -> Result<Program> {
    body(&mut program.body, &Characters::new(), &mut 0)?;
    Ok(program)
}

/// Generated declarations belong to the enclosing procedure, even when their
/// captures execute conditionally or inside a loop.
pub fn hoist(body: &mut Vec<Stmt>) {
    fn walk(body: &mut Vec<Stmt>, declarations: &mut Vec<Stmt>) {
        let mut output = Vec::new();
        for mut s in std::mem::take(body) {
            if matches!(&s, Stmt::VarDecl {names,..} if names.iter().all(|n|n.starts_with("@substring_")))
            {
                declarations.push(s);
                continue;
            }
            match &mut s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => hoist(body),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    walk(then_body, declarations);
                    if let Some(b) = else_body {
                        walk(b, declarations);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    walk(body, declarations)
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        walk(&mut c.body, declarations);
                    }
                    if let Some(b) = default {
                        walk(b, declarations);
                    }
                }
                _ => {}
            }
            output.push(s);
        }
        *body = output;
    }
    let mut declarations = Vec::new();
    walk(body, &mut declarations);
    declarations.append(body);
    *body = declarations;
}

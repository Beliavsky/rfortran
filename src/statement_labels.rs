// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate statement labels and lower supported forward READ END= transfers.
//! Other I/O branch controls remain intact for their feature checks.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;

fn number(value: &Expr) -> Result<u32> {
    let Expr::IntLit(text) = value else {
        bail!("statement label must be a decimal integer literal");
    };
    if text.len() > 5 || !text.bytes().all(|c| c.is_ascii_digit()) {
        bail!("statement label `{text}` must contain one to five decimal digits");
    }
    let number: u32 = text.parse().context("invalid statement label")?;
    if number == 0 {
        bail!("statement label must be between 1 and 99999");
    }
    Ok(number)
}

fn references(value: &Expr, targets: &mut Vec<u32>) -> Result<()> {
    match value {
        Expr::Call(name, args)
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "@io_spec:err" | "@io_spec:end" | "@io_spec:eor"
            ) =>
        {
            let [value] = args.as_slice() else {
                bail!("invalid I/O branch label");
            };
            targets.push(number(value)?);
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for value in args {
                references(value, targets)?;
            }
        }
        Expr::Bin(_, a, b) => {
            references(a, targets)?;
            references(b, targets)?;
        }
        Expr::Un(_, value) => references(value, targets)?,
        _ => {}
    }
    Ok(())
}

fn collect(body: &[Stmt], labels: &mut HashSet<u32>, targets: &mut Vec<u32>) -> Result<()> {
    for (position, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::CallSub { name, args } if name == "@statement_label" => {
                let [value] = args.as_slice() else {
                    bail!("invalid statement label marker");
                };
                let label = number(value)?;
                if !labels.insert(label) {
                    bail!("duplicate statement label {label} in the same program unit");
                }
                if matches!(body.get(position + 1), Some(Stmt::Implicit(_))) {
                    bail!("labels on specification statements are not yet supported");
                }
            }
            Stmt::CallSub { name, args } => {
                for arg in args {
                    references(arg, targets)?;
                }
                let control = match name.as_str() {
                    "@empty_read" | "@internal_read" => args.get(1).zip(args.get(2)),
                    "@formatted_read_status" => args.get(2).zip(args.get(3)),
                    _ => None,
                };
                if let Some((Expr::Str(control), target)) = control {
                    if matches!(control.to_ascii_lowercase().as_str(), "err" | "end" | "eor") {
                        targets.push(number(target)?);
                    }
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect(then_body, labels, targets)?;
                if let Some(body) = else_body {
                    collect(body, labels, targets)?;
                }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                collect(body, labels, targets)?
            }
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases {
                    collect(&case.body, labels, targets)?;
                }
                if let Some(body) = default {
                    collect(body, labels, targets)?;
                }
            }
            // Contained procedures and modules have independent label namespaces.
            _ => {}
        }
    }
    Ok(())
}

fn clean(body: &mut Vec<Stmt>) -> Result<()> {
    body.retain(|stmt| !matches!(stmt, Stmt::CallSub { name, .. } if matches!(name.as_str(), "@statement_label" | "@continue")));
    for stmt in body {
        match stmt {
            Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } => unit(body)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                clean(then_body)?;
                if let Some(body) = else_body {
                    clean(body)?;
                }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                clean(body)?
            }
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases {
                    clean(&mut case.body)?;
                }
                if let Some(body) = default {
                    clean(body)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// Forward END= transfers to a following statement in this or an enclosing
// statement sequence become named exits from a single-iteration region. The
// named-loop pass unwinds intervening loops identically on both backends.
fn replace_end(
    stmt: &mut Stmt,
    target: u32,
    region: &str,
    temps: &mut Vec<String>,
) -> Result<bool> {
    if let Stmt::CallSub { name, args } = stmt {
        let offset = match name.as_str() {
            "@empty_read" | "@internal_read" => Some(1),
            "@formatted_read_status" => Some(2),
            _ => None,
        };
        if let Some(offset) = offset {
            if matches!(args.get(offset),Some(Expr::Str(control)) if control.eq_ignore_ascii_case("end"))
                && number(&args[offset + 1])? == target
            {
                let code = format!("@read_end_status_{region}_{}", temps.len());
                temps.push(code.clone());
                args[offset] = Expr::Str("iostat".into());
                args[offset + 1] = Expr::Ident(code.clone());
                let read = stmt.clone();
                *stmt = Stmt::Block {
                    body: vec![
                        read,
                        Stmt::If {
                            cond: Expr::Bin(
                                BinOp::Eq,
                                Box::new(Expr::Ident(code.clone())),
                                Box::new(Expr::IntLit("-1".into())),
                            ),
                            then_body: vec![Stmt::CallSub {
                                name: "@named_exit".into(),
                                args: vec![Expr::Str(region.into())],
                            }],
                            else_body: None,
                        },
                        Stmt::If {
                            cond: Expr::Bin(
                                BinOp::Ne,
                                Box::new(Expr::Ident(code)),
                                Box::new(Expr::IntLit("0".into())),
                            ),
                            then_body: vec![Stmt::CallSub {
                                name: "@error_stop".into(),
                                args: vec![Expr::Str("input error in READ with END=".into())],
                            }],
                            else_body: None,
                        },
                    ],
                };
                return Ok(true);
            }
        }
    }
    let mut found = false;
    match stmt {
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for stmt in then_body {
                found |= replace_end(stmt, target, region, temps)?;
            }
            if let Some(body) = else_body {
                for stmt in body {
                    found |= replace_end(stmt, target, region, temps)?;
                }
            }
        }
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
            for stmt in body {
                found |= replace_end(stmt, target, region, temps)?;
            }
        }
        Stmt::SelectCase { cases, default, .. } => {
            for case in cases {
                for stmt in &mut case.body {
                    found |= replace_end(stmt, target, region, temps)?;
                }
            }
            if let Some(body) = default {
                for stmt in body {
                    found |= replace_end(stmt, target, region, temps)?;
                }
            }
        }
        _ => {}
    }
    Ok(found)
}
// A synthetic loop must not capture an existing unqualified transfer to an
// enclosing user loop. Named transfers already unwind synthetic loops safely.
fn escaping_transfer(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match stmt {
        Stmt::Exit | Stmt::Cycle => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            escaping_transfer(then_body)
                || else_body
                    .as_ref()
                    .is_some_and(|body| escaping_transfer(body))
        }
        Stmt::Block { body } => escaping_transfer(body),
        Stmt::SelectCase { cases, default, .. } => {
            cases.iter().any(|case| escaping_transfer(&case.body))
                || default.as_ref().is_some_and(|body| escaping_transfer(body))
        }
        // Transfers in a nested DO still target that DO.
        _ => false,
    })
}

fn forward_regions(body: &mut Vec<Stmt>, temps: &mut Vec<String>) -> Result<()> {
    let mut position = 0;
    while position < body.len() {
        let label = match &body[position] {
            Stmt::CallSub { name, args } if name == "@statement_label" => Some(number(&args[0])?),
            _ => None,
        };
        if let Some(label) = label {
            let region = format!("@read_end_region_{label}");
            let mut first = None;
            for (i, stmt) in body[..position].iter_mut().enumerate() {
                if replace_end(stmt, label, &region, temps)? && first.is_none() {
                    first = Some(i);
                }
            }
            if let Some(first) = first {
                if escaping_transfer(&body[first..position]) {
                    bail!("use named EXIT/CYCLE when a forward READ END= region contains a transfer to an enclosing loop");
                }
                let mut segment: Vec<_> = body.drain(first..position).collect();
                segment.push(Stmt::Exit);
                body.insert(
                    first,
                    crate::named_loops::parsed(
                        Stmt::DoWhile {
                            cond: Expr::Logical(true),
                            body: segment,
                        },
                        Some(region.clone()),
                        Some(region),
                    ),
                );
                position = first + 1;
            }
        }
        position += 1;
    }
    for stmt in body {
        match stmt {
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                forward_regions(then_body, temps)?;
                if let Some(body) = else_body {
                    forward_regions(body, temps)?;
                }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                forward_regions(body, temps)?
            }
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases {
                    forward_regions(&mut case.body, temps)?;
                }
                if let Some(body) = default {
                    forward_regions(body, temps)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn unresolved_end(body: &[Stmt]) -> bool {
    body.iter().any(|stmt|match stmt {
        Stmt::CallSub{name,args}=>{
            let offset=match name.as_str(){"@empty_read"|"@internal_read"=>Some(1),"@formatted_read_status"=>Some(2),_=>None};
            offset.is_some_and(|i|matches!(args.get(i),Some(Expr::Str(control)) if control.eq_ignore_ascii_case("end")))
        }
        Stmt::If{then_body,else_body,..}=>unresolved_end(then_body)||else_body.as_ref().is_some_and(|b|unresolved_end(b)),
        Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body}=>unresolved_end(body),
        Stmt::SelectCase{cases,default,..}=>cases.iter().any(|c|unresolved_end(&c.body))||default.as_ref().is_some_and(|b|unresolved_end(b)),
        _=>false
    })
}

fn unit(body: &mut Vec<Stmt>) -> Result<()> {
    let mut labels = HashSet::new();
    let mut targets = vec![];
    collect(body, &mut labels, &mut targets)?;
    for target in targets {
        if !labels.contains(&target) {
            bail!("undefined statement label {target} in this program unit");
        }
    }
    let mut temps = vec![];
    forward_regions(body, &mut temps)?;
    if unresolved_end(body) {
        bail!("READ END= currently requires an list-directed or scalar CHARACTER formatted READ and a forward label outside intervening loops");
    }
    if !temps.is_empty() {
        body.insert(
            0,
            Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: temps,
            },
        );
    }
    clean(body)
}

/// Give each READ, WRITE and PRINT whose format is a statement label the
/// specification of that FORMAT statement, and remove the FORMAT statements.
/// Labels are local to a program unit; contained procedures have their own.
pub(crate) fn formats(body: &mut Vec<Stmt>) -> std::result::Result<(), String> {
    fn collect(body: &[Stmt], formats: &mut std::collections::HashMap<String, String>) {
        for (i, s) in body.iter().enumerate() {
            match s {
                Stmt::CallSub { name, args } if name == "@format_statement" => {
                    if let (Some(Stmt::CallSub { name: l, args: label }), [Expr::Str(spec)]) = (i.checked_sub(1).map(|j| &body[j]), args.as_slice()) {
                        if l == "@statement_label" {
                            if let [Expr::IntLit(n)] = label.as_slice() {
                                formats.insert(n.trim_start_matches('0').to_owned(), spec.clone());
                            }
                        }
                    }
                }
                Stmt::If { then_body, else_body, .. } => {
                    collect(then_body, formats);
                    if let Some(b) = else_body { collect(b, formats); }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => collect(body, formats),
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases { collect(&c.body, formats); }
                    if let Some(b) = default { collect(b, formats); }
                }
                _ => {}
            }
        }
    }
    fn replace(body: &mut Vec<Stmt>, formats: &std::collections::HashMap<String, String>) -> std::result::Result<(), String> {
        body.retain(|s| !matches!(s, Stmt::CallSub { name, .. } if name == "@format_statement"));
        for s in body.iter_mut() {
            match s {
                Stmt::CallSub { name, args } => {
                    let position = match name.as_str() {
                        "@formatted_print" => 0,
                        "@write" | "@formatted_read" | "@formatted_read_status" | "@formatted_write_noadvance" => 1,
                        _ => continue,
                    };
                    if let Some(format @ Expr::IntLit(_)) = args.get_mut(position) {
                        let Expr::IntLit(n) = &*format else { unreachable!() };
                        let spec = formats.get(n.trim_start_matches('0'))
                            .ok_or_else(|| format!("format label {n} does not label a FORMAT statement in this program unit"))?;
                        *format = Expr::Str(spec.clone());
                    }
                }
                Stmt::If { then_body, else_body, .. } => {
                    replace(then_body, formats)?;
                    if let Some(b) = else_body { replace(b, formats)?; }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => replace(body, formats)?,
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases { replace(&mut c.body, formats)?; }
                    if let Some(b) = default { replace(b, formats)?; }
                }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => self::formats(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut table = std::collections::HashMap::new();
    collect(body, &mut table);
    replace(body, &table)
}

pub fn resolve(mut program: Program) -> Result<Program> {
    unit(&mut program.body)?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_io_targets_after_validating_and_removing_label_markers() {
        let source="program main\nopen(20,file='data.txt',status='old',action='read',err=1234)\n1234 continue\nend program main";
        let program =
            crate::parser::parse_with_src(source, &crate::lexer::lex(source), "labels.f90")
                .unwrap();
        let program = resolve(program).unwrap();
        assert!(program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,args} if name=="@io_open"
            && args.iter().any(|a|matches!(a,Expr::Call(n,v) if n=="@io_spec:err" && v==&vec![Expr::IntLit("1234".into())])))));
        assert!(!program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if matches!(name.as_str(),"@continue"|"@statement_label"))));
    }
}

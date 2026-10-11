// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate statement labels within each program unit before removing no-op
//! markers. I/O branch controls remain intact for their feature checks.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;

fn number(value: &Expr) -> Result<u32> {
    let Expr::IntLit(text) = value else { bail!("statement label must be a decimal integer literal"); };
    if text.len() > 5 || !text.bytes().all(|c| c.is_ascii_digit()) {
        bail!("statement label `{text}` must contain one to five decimal digits");
    }
    let number: u32 = text.parse().context("invalid statement label")?;
    if number == 0 { bail!("statement label must be between 1 and 99999"); }
    Ok(number)
}

fn references(value: &Expr, targets: &mut Vec<u32>) -> Result<()> {
    match value {
        Expr::Call(name, args) if matches!(name.to_ascii_lowercase().as_str(), "@io_spec:err" | "@io_spec:end" | "@io_spec:eor") => {
            let [value] = args.as_slice() else { bail!("invalid I/O branch label"); };
            targets.push(number(value)?);
        }
        Expr::Call(_, args) | Expr::Index(_, args) => for value in args { references(value, targets)?; },
        Expr::Bin(_, a, b) => { references(a, targets)?; references(b, targets)?; }
        Expr::Un(_, value) => references(value, targets)?,
        _ => {}
    }
    Ok(())
}

fn collect(body: &[Stmt], labels: &mut HashSet<u32>, targets: &mut Vec<u32>) -> Result<()> {
    for (position, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::CallSub { name, args } if name == "@statement_label" => {
                let [value] = args.as_slice() else { bail!("invalid statement label marker"); };
                let label = number(value)?;
                if !labels.insert(label) { bail!("duplicate statement label {label} in the same program unit"); }
                if matches!(body.get(position+1), Some(Stmt::Implicit(_))) {
                    bail!("labels on specification statements are not yet supported");
                }
            }
            Stmt::CallSub { name, args } => {
                for arg in args { references(arg, targets)?; }
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
            Stmt::If { then_body, else_body, .. } => {
                collect(then_body, labels, targets)?;
                if let Some(body) = else_body { collect(body, labels, targets)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => collect(body, labels, targets)?,
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { collect(&case.body, labels, targets)?; }
                if let Some(body) = default { collect(body, labels, targets)?; }
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
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => unit(body)?,
            Stmt::If { then_body, else_body, .. } => {
                clean(then_body)?;
                if let Some(body) = else_body { clean(body)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => clean(body)?,
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { clean(&mut case.body)?; }
                if let Some(body) = default { clean(body)?; }
            }
            _ => {}
        }
    }
    Ok(())
}

fn unit(body: &mut Vec<Stmt>) -> Result<()> {
    let mut labels = HashSet::new();
    let mut targets = vec![];
    collect(body, &mut labels, &mut targets)?;
    for target in targets {
        if !labels.contains(&target) { bail!("undefined statement label {target} in this program unit"); }
    }
    clean(body)
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
        let program=crate::parser::parse_with_src(source,&crate::lexer::lex(source),"labels.f90").unwrap();
        let program=resolve(program).unwrap();
        assert!(program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,args} if name=="@io_open"
            && args.iter().any(|a|matches!(a,Expr::Call(n,v) if n=="@io_spec:err" && v==&vec![Expr::IntLit("1234".into())])))));
        assert!(!program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if matches!(name.as_str(),"@continue"|"@statement_label"))));
    }
}

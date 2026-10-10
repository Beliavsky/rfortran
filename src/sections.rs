//! Expand constant one-dimensional array sections in list-directed output.
use crate::ast::{CaseItem, Expr, Program, Stmt};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Arrays = HashMap<String, Vec<Expr>>;

pub fn resolve(mut program: Program) -> Result<Program> {
    resolve_body(&mut program.body, &Arrays::new())?;
    Ok(program)
}

fn constant(e: &Expr) -> Result<i64> {
    crate::parameters::integer_constant(e)
        .context("array section bounds, strides, and extents must currently be constant integers")
}

pub(crate) fn expand(e: Expr, arrays: &Arrays, output: &mut Vec<Expr>) -> Result<()> {
    if let Some((_,count)) = crate::multidim::view_element(&e,&Expr::IntLit("1".into())) {
        for i in 1..=count {
            output.push(crate::multidim::view_element(&e,&Expr::IntLit(i.to_string())).unwrap().0);
        }
    } else if let Expr::Section { name, lower, upper, stride } = e {
        let dims = arrays.get(&name.to_ascii_lowercase())
            .with_context(|| format!("array section `{name}` requires a declared array"))?;
        if dims.len() != 1 { bail!("only one-dimensional array sections are currently supported"); }
        let extent = constant(&dims[0])?;
        if extent < 0 { bail!("negative array extent for `{name}` is not supported"); }
        let step = stride.as_deref().map(constant).transpose()?.unwrap_or(1);
        if step == 0 { bail!("array section stride must not be zero"); }
        // Omitted bounds use the declared bounds, regardless of stride sign.
        let first = lower.as_deref().map(constant).transpose()?.unwrap_or(1);
        let end = upper.as_deref().map(constant).transpose()?.unwrap_or(extent);
        let (mut index, end, step) = (i128::from(first), i128::from(end), i128::from(step));
        while if step > 0 { index <= end } else { index >= end } {
            if index < 1 || index > i128::from(extent) {
                bail!("array section of `{name}` selects index {index} outside 1:{extent}");
            }
            output.push(Expr::Index(name.clone(), vec![Expr::IntLit(index.to_string())]));
            index += step;
        }
    } else {
        reject_section(&e)?;
        output.push(e);
    }
    Ok(())
}

fn reject_section(e: &Expr) -> Result<()> {
    match e {
        Expr::Call(tag,_) if tag == "@matrix_view" => bail!("multidimensional section is unsupported in this context"),
        Expr::Section { .. } => bail!("array sections are currently supported only as direct PRINT items"),
        Expr::Bin(_, a, b) => { reject_section(a)?; reject_section(b)?; }
        Expr::Un(_, a) => reject_section(a)?,
        Expr::Call(_, args) | Expr::Index(_, args) => for arg in args { reject_section(arg)?; },
        _ => {}
    }
    Ok(())
}

fn resolve_body(body: &mut [Stmt], inherited: &Arrays) -> Result<()> {
    let mut arrays = inherited.clone();
    for stmt in body.iter() {
        match stmt {
            Stmt::ArrayDecl { name, dims, .. } => { arrays.insert(name.to_ascii_lowercase(), dims.clone()); }
            Stmt::VarDecl { names, .. } => for name in names { arrays.remove(&name.to_ascii_lowercase()); },
            _ => {}
        }
    }
    for stmt in body {
        match stmt {
            Stmt::Print { items } => {
                let mut expanded = Vec::new();
                for item in std::mem::take(items) { expand(item, &arrays, &mut expanded)?; }
                *items = expanded;
            }
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => reject_section(value)?,
            Stmt::AssignIndex { indices, value, .. } => {
                for index in indices { reject_section(index)?; }
                reject_section(value)?;
            }
            Stmt::ArrayDecl { dims, .. } => for dim in dims { reject_section(dim)?; },
            Stmt::CallSub { args, .. } | Stmt::Read { args } => for arg in args { reject_section(arg)?; },
            Stmt::If { cond, then_body, else_body } => {
                reject_section(cond)?;
                resolve_body(then_body, &arrays)?;
                if let Some(body) = else_body { resolve_body(body, &arrays)?; }
            }
            Stmt::Do { start, end, body, .. } => {
                reject_section(start)?; reject_section(end)?;
                resolve_body(body, &arrays)?;
            }
            Stmt::DoWhile { cond, body } => { reject_section(cond)?; resolve_body(body, &arrays)?; }
            Stmt::Block { body } | Stmt::Module { body, .. } => resolve_body(body, &arrays)?,
            Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                let mut local = arrays.clone();
                for param in params { local.remove(&param.to_ascii_lowercase()); }
                resolve_body(body, &local)?;
            }
            Stmt::SelectCase { expr, cases, default } => {
                reject_section(expr)?;
                for case in cases {
                    for item in &case.items {
                        match item {
                            CaseItem::Single(e) => reject_section(e)?,
                            CaseItem::Range(a, b) => { reject_section(a)?; reject_section(b)?; }
                        }
                    }
                    resolve_body(&mut case.body, &arrays)?;
                }
                if let Some(body) = default { resolve_body(body, &arrays)?; }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(declarations: &str, statement: &str) -> Result<Program> {
        let source = format!("program main\nimplicit none\n{declarations}\n{statement}\nend program main");
        let tokens = crate::lexer::lex(&source);
        let parsed = crate::parser::parse_with_src(&source, &tokens, "section.f90")
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        resolve(crate::parameters::resolve(parsed)?)
    }

    #[test]
    fn expands_section_bounds_and_strides() {
        for (section, expected) in [
            ("a(2:)", vec![2, 3, 4]), ("a(:2)", vec![1, 2]),
            ("a(:)", vec![1, 2, 3, 4]), ("a(2:3)", vec![2, 3]),
            ("a(::2)", vec![1, 3]), ("a(2::2)", vec![2, 4]),
            ("a(1:4:2)", vec![1, 3]), ("a(::-1)", vec![]),
            ("a(4:1:-1)", vec![4, 3, 2, 1]),
            ("a(4:1:-2)", vec![4, 2]), ("a(3:2)", vec![]),
            ("a(2:3:-1)", vec![]), ("a(n-1:n)", vec![3, 4]),
        ] {
            let p = program("integer, parameter :: n = 4\nreal :: a(n)", &format!("print *, {section}")).unwrap();
            let Stmt::Print { items } = p.body.last().unwrap() else { panic!() };
            let indices: Vec<i64> = items.iter().map(|e| {
                let Expr::Index(name, indices) = e else { panic!("{e:?}") };
                assert_eq!(name, "a");
                constant(&indices[0]).unwrap()
            }).collect();
            assert_eq!(indices, expected, "{section}");
        }
    }

    #[test]
    fn rejects_unsupported_or_invalid_sections() {
        for (declarations, statement, error) in [
            ("real :: a(4)", "print *, a(::0)", "stride must not be zero"),
            ("real :: a(4)", "print *, a(0:2)", "outside 1:4"),
            ("real :: a(4)", "print *, a(3:5)", "outside 1:4"),
            ("real :: a(4)", "print *, a(1.5:)", "constant integers"),
            ("real :: a(4)\ninteger :: i", "print *, a(i:)", "constant integers"),
            ("real :: a", "print *, a(:)", "declared array"),
            ("real :: a(4)", "print *, sum(a(:))", "direct PRINT items"),
            ("real :: a(4)", "call random_number(a(:))", "direct PRINT items"),
        ] {
            let result = program(declarations, statement).unwrap_err();
            assert!(format!("{result:#}").contains(error), "{result:#}");
        }
    }
}

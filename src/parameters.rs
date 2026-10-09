//! Resolve supported named constants before semantic analysis and lowering.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

type Constants = HashMap<String, i64>;

pub fn resolve(mut program: Program) -> Result<Program> {
    crate::purity::validate(&program)?;
    resolve_scope(&mut program.body, &Constants::new(), &[])?;
    Ok(program)
}

fn evaluate(expr: &Expr, constants: &Constants) -> Result<i64> {
    let value = match expr {
        Expr::IntLit(s) => s.parse().ok(),
        Expr::Ident(name) => {
            return constants
                .get(&name.to_ascii_lowercase())
                .copied()
                .with_context(|| {
                    format!("`{name}` is not a previously defined integer PARAMETER")
                });
        }
        Expr::Un(UnOp::Neg, e) => evaluate(e, constants)?.checked_neg(),
        Expr::Bin(op, left, right) => {
            let a = evaluate(left, constants)?;
            let b = evaluate(right, constants)?;
            match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div => {
                    if b == 0 {
                        bail!("division by zero in PARAMETER initializer");
                    }
                    a.checked_div(b)
                }
                BinOp::Pow if b < 0 => {
                    if a == 0 {
                        bail!("zero raised to a negative power in PARAMETER initializer");
                    }
                    Some(match a {
                        1 => 1,
                        -1 => {
                            if b % 2 == 0 {
                                1
                            } else {
                                -1
                            }
                        }
                        _ => 0,
                    })
                }
                BinOp::Pow => {
                    let exponent = u32::try_from(b).context("PARAMETER exponent is too large")?;
                    a.checked_pow(exponent)
                }
                _ => bail!("unsupported operator in integer PARAMETER initializer"),
            }
        }
        _ => bail!("PARAMETER initializer must be a supported integer constant expression"),
    };
    value.context("integer overflow in PARAMETER initializer")
}

pub(crate) fn integer_constant(expr: &Expr) -> Result<i64> {
    evaluate(expr, &Constants::new())
}

fn resolve_scope(body: &mut Vec<Stmt>, inherited: &Constants, arguments: &[String]) -> Result<()> {
    let mut constants = inherited.clone();
    let mut locals: HashSet<String> = arguments.iter().map(|s| s.to_ascii_lowercase()).collect();
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { names, .. } => {
                locals.extend(names.iter().map(|s| s.to_ascii_lowercase()))
            }
            Stmt::ArrayDecl { name, .. } => {
                locals.insert(name.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    for name in &locals {
        constants.remove(name);
    }
    // Local parameter names also hide host constants, even in their initializers.
    for stmt in body.iter() {
        if let Stmt::InitializedDecl { entities, .. } = stmt {
            for (name, _) in entities {
                constants.remove(&name.to_ascii_lowercase());
            }
        }
    }
    for stmt in body.iter() {
        if let Stmt::InitializedDecl {
            kind,
            attributes,
            entities,
        } = stmt
        {
            if !attributes
                .iter()
                .any(|a| a.eq_ignore_ascii_case("parameter"))
            {
                bail!("declaration initialization without PARAMETER is not yet supported");
            }
            if !matches!(kind, TypeSpec::Integer(None)) {
                bail!("only default INTEGER PARAMETER declarations are currently supported");
            }
            if attributes
                .iter()
                .any(|a| !a.eq_ignore_ascii_case("parameter"))
            {
                bail!("additional attributes on PARAMETER declarations are not yet supported");
            }
            for (name, initializer) in entities {
                let key = name.to_ascii_lowercase();
                if !locals.insert(key.clone()) {
                    bail!("duplicate declaration of PARAMETER `{name}`");
                }
                let initializer = initializer
                    .as_ref()
                    .with_context(|| format!("PARAMETER `{name}` requires an initializer"))?;
                let value = evaluate(initializer, &constants)
                    .with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?;
                constants.insert(key, value);
            }
        }
    }
    body.retain(|s| !matches!(s, Stmt::InitializedDecl { .. }));
    rewrite_body(body, &constants)
}

fn variable(name: &str, constants: &Constants) -> Result<()> {
    if constants.contains_key(&name.to_ascii_lowercase()) {
        bail!("cannot modify PARAMETER `{name}`");
    }
    Ok(())
}

fn rewrite_expr(expr: &mut Expr, constants: &Constants) -> Result<()> {
    match expr {
        Expr::Section { name, lower, upper, stride } => {
            if constants.contains_key(&name.to_ascii_lowercase()) { bail!("PARAMETER `{name}` is not an array"); }
            for bound in [lower, upper, stride].into_iter().flatten() { rewrite_expr(bound, constants)?; }
        }
        Expr::Ident(name) => {
            if let Some(value) = constants.get(&name.to_ascii_lowercase()) {
                *expr = Expr::IntLit(value.to_string());
            }
        }
        Expr::Bin(_, left, right) => {
            rewrite_expr(left, constants)?;
            rewrite_expr(right, constants)?;
        }
        Expr::Un(_, inner) => rewrite_expr(inner, constants)?,
        Expr::Call(name, args) | Expr::Index(name, args) => {
            if constants.contains_key(&name.to_ascii_lowercase()) {
                bail!("scalar PARAMETER `{name}` cannot be called or indexed");
            }
            for arg in args {
                rewrite_expr(arg, constants)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn rewrite_body(body: &mut [Stmt], constants: &Constants) -> Result<()> {
    for stmt in body {
        match stmt {
            Stmt::Print { items } => {
                for e in items {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Assign { name, value } => {
                variable(name, constants)?;
                rewrite_expr(value, constants)?;
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                variable(name, constants)?;
                for e in indices {
                    rewrite_expr(e, constants)?;
                }
                rewrite_expr(value, constants)?;
            }
            Stmt::ArrayDecl { dims, .. } => {
                for e in dims {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Read { args } => {
                for e in args {
                    match e {
                        Expr::Ident(name) | Expr::Call(name, _) | Expr::Index(name, _) => {
                            variable(name, constants)?
                        }
                        _ => {}
                    }
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::CallSub { name, args } => {
                if constants.contains_key(&name.to_ascii_lowercase()) {
                    bail!("PARAMETER `{name}` cannot be called");
                }
                for e in args {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Return(Some(e)) => rewrite_expr(e, constants)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(then_body, constants)?;
                if let Some(body) = else_body {
                    rewrite_body(body, constants)?;
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                variable(var, constants)?;
                rewrite_expr(start, constants)?;
                rewrite_expr(end, constants)?;
                rewrite_body(body, constants)?;
            }
            Stmt::DoWhile { cond, body } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(body, constants)?;
            }
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                rewrite_expr(expr, constants)?;
                for case in cases {
                    for item in &mut case.items {
                        match item {
                            CaseItem::Single(e) => rewrite_expr(e, constants)?,
                            CaseItem::Range(a, b) => {
                                rewrite_expr(a, constants)?;
                                rewrite_expr(b, constants)?;
                            }
                        }
                    }
                    rewrite_body(&mut case.body, constants)?;
                }
                if let Some(body) = default {
                    rewrite_body(body, constants)?;
                }
            }
            Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                resolve_scope(body, constants, params)?
            }
            Stmt::Block { body } => resolve_scope(body, constants, &[])?,
            Stmt::Module { body, .. } => resolve_scope(body, &Constants::new(), &[])?,
            Stmt::InitializedDecl { .. } => {
                bail!("PARAMETER declaration must be in a specification part")
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_source(declarations: &str, statements: &str) -> Result<Program> {
        let src = format!(
            "program main\nimplicit none\n{declarations}\n{statements}\nend program main\n"
        );
        let tokens = crate::lexer::lex(&src);
        let program = crate::parser::parse_with_src(&src, &tokens, "parameter.f90")
            .map_err(|errors| anyhow::anyhow!("{errors:?}"))?;
        resolve(program)
    }

    #[test]
    fn evaluates_named_integer_constants() {
        let program =
            resolve_source("integer, PARAMETER :: N = 5, m = n**2 + 1", "print *, N, m").unwrap();
        let Stmt::Print { items } = program.body.last().unwrap() else {
            panic!("expected PRINT")
        };
        assert!(matches!(&items[0], Expr::IntLit(s) if s == "5"));
        assert!(matches!(&items[1], Expr::IntLit(s) if s == "26"));
    }

    #[test]
    fn rejects_invalid_parameters_and_modifications() {
        for (decl, stmt, message) in [
            (
                "integer, parameter :: n = 5",
                "n = 6",
                "cannot modify PARAMETER",
            ),
            (
                "integer, parameter :: n = 5",
                "read(*,*) n",
                "cannot modify PARAMETER",
            ),
            (
                "integer, parameter :: n = 5",
                "do n = 1, 3\nprint *, n\nend do",
                "cannot modify PARAMETER",
            ),
            ("integer, parameter :: n", "", "requires an initializer"),
            (
                "integer :: i\ninteger, parameter :: n = i",
                "",
                "previously defined",
            ),
            ("integer, parameter :: n = 1/0", "", "division by zero"),
            ("integer, parameter :: n = 2**63", "", "overflow"),
            (
                "integer, parameter :: n = 5\ninteger :: n",
                "",
                "duplicate declaration",
            ),
        ] {
            let error = format!("{:#}", resolve_source(decl, stmt).unwrap_err());
            assert!(error.contains(message), "{error}");
        }
    }
}

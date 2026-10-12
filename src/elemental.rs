// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Declaration checks for ELEMENTAL functions and scalar subroutine calls.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub fn result_kind(name: &str, declared: &Option<TypeSpec>, body: &[Stmt]) -> Option<TypeSpec> {
    if declared.is_some() {
        return declared.clone();
    }
    let result = match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
        _ => name,
    };
    body.iter().find_map(|s| match s {
        Stmt::VarDecl { kind, names } if names.iter().any(|n| n == result) => Some(kind.clone()),
        _ => None,
    })
}
fn numeric(kind: &TypeSpec) -> bool {
    matches!(
        kind,
        TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
            | TypeSpec::IntegerKind(_)
            | TypeSpec::Real
            | TypeSpec::RealKind(_)
            | TypeSpec::DoublePrecision
    )
}
fn scalar(kind: &TypeSpec) -> bool {
    numeric(kind)
        || matches!(
            kind,
            TypeSpec::Logical
                | TypeSpec::Character(_)
                | TypeSpec::CharacterLength(_)
                | TypeSpec::CharacterAssumed
        )
}
pub(crate) fn inline_character(
    elemental: bool,
    name: &str,
    params: &[String],
    declared: &Option<TypeSpec>,
    body: &[Stmt],
) -> bool {
    let character = |kind: &TypeSpec| {
        matches!(
            kind,
            TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed
        )
    };
    elemental && (result_kind(name,declared,body).as_ref().is_some_and(character)
        || body.iter().any(|s|matches!(s,Stmt::VarDecl{kind,names} if character(kind)&&names.iter().any(|n|params.contains(n)))))
}

pub fn validate_subroutine(name: &str, params: &[String], body: &[Stmt]) -> Result<()> {
    for param in params {
        if body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if name == param)) {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("ELEMENTAL subroutine `{name}` requires scalar dummy arguments")));
        }
        if !body.iter().any(|s| matches!(s, Stmt::VarDecl { kind, names } if names.contains(param)
            && (numeric(kind) || matches!(kind, TypeSpec::Logical | TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed)))) {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("ELEMENTAL dummy `{param}` requires a supported explicitly declared scalar type")));
        }
        let attributes: Vec<_> = body.iter().filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if name == param => Some(intent.as_str()), _ => None,
        }).collect();
        if attributes.iter().filter(|a| a.starts_with("intent(")).count() != 1 {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("ELEMENTAL dummy `{param}` requires one explicit INTENT")));
        }
        if attributes.iter().any(|a| !matches!(*a, "intent(in)" | "intent(out)" | "intent(inout)" | "optional")) {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("unsupported attribute on ELEMENTAL dummy `{param}`")));
        }
    }
    fn expression(e: &Expr, params: &[String], body: &[Stmt]) -> Result<()> {
        match e {
            Expr::Call(n, args) if n == "present" => {
                let [Expr::Ident(param)] = args.as_slice() else { bail!("PRESENT requires an OPTIONAL dummy argument"); };
                if !params.contains(param) || !body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == param && intent == "optional")) {
                    bail!("PRESENT requires an OPTIONAL dummy argument");
                }
            }
            Expr::Call(_, args) | Expr::Index(_, args) => for e in args { expression(e, params, body)?; },
            Expr::Bin(_, a, b) => { expression(a, params, body)?; expression(b, params, body)?; }
            Expr::Un(_, a) => expression(a, params, body)?,
            _ => {}
        }
        Ok(())
    }
    fn walk(statements: &[Stmt], params: &[String], declarations: &[Stmt]) -> Result<()> {
        for s in statements {
            match s {
                Stmt::Assign { value, .. } => expression(value, params, declarations)?,
                Stmt::CallSub { args, .. } => for e in args { expression(e, params, declarations)?; },
                Stmt::If { cond, then_body, else_body } => {
                    expression(cond, params, declarations)?;
                    walk(then_body, params, declarations)?;
                    if let Some(body) = else_body { walk(body, params, declarations)?; }
                }
                Stmt::Do { start, end, body, .. } => {
                    expression(start, params, declarations)?; expression(end, params, declarations)?;
                    walk(body, params, declarations)?;
                }
                Stmt::DoWhile { cond, body } => { expression(cond, params, declarations)?; walk(body, params, declarations)?; }
                Stmt::Block { body } => walk(body, params, declarations)?,
                Stmt::Return(Some(e)) => expression(e, params, declarations)?,
                Stmt::AssignIndex { indices, value, .. } => {
                    for e in indices { expression(e, params, declarations)?; }
                    expression(value, params, declarations)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    walk(body, params, body)
}
pub fn functions(program: &Program) -> HashMap<String, TypeSpec> {
    fn walk(body: &[Stmt], out: &mut HashMap<String, TypeSpec>) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    elemental: true,
                    return_type,
                    body,
                    ..
                } => {
                    if let Some(kind) = result_kind(name, return_type, body) {
                        out.insert(name.clone(), kind);
                    }
                    walk(body, out);
                }
                Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => walk(body, out),
                _ => {}
            }
        }
    }
    let mut out = HashMap::new();
    walk(&program.body, &mut out);
    out
}
pub fn validate(
    name: &str,
    params: &[String],
    declared: &Option<TypeSpec>,
    body: &[Stmt],
) -> Result<()> {
    let result = match body.last() {
        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.as_str(),
        _ => name,
    };
    if body
        .iter()
        .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name) || name == result))
    {
        let target=body.iter().find_map(|s|match s {Stmt::ArrayDecl{name,..} if params.contains(name)||name==result=>Some(name.as_str()),_=>None}).unwrap();
        return Err(crate::source_diagnostics::declaration(body,target,None,format!("ELEMENTAL function `{name}` requires scalar dummy arguments and a scalar result")));
    }
    if !result_kind(name, declared, body)
        .as_ref()
        .is_some_and(scalar)
    {
        return Err(crate::source_diagnostics::declaration(body,result,None,format!("ELEMENTAL function `{name}` requires an explicitly declared supported scalar result")));
    }
    for param in params {
        if !body.iter().any(
            |s| matches!(s,Stmt::VarDecl{kind,names} if names.contains(param) && scalar(kind)),
        ) {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("ELEMENTAL dummy argument `{param}` requires an explicitly declared supported scalar type")));
        }
        if !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name == param && intent == "intent(in)")) {
            return Err(crate::source_diagnostics::declaration(body,param,None,format!("ELEMENTAL function dummy argument `{param}` requires INTENT(IN)")));
        }
        if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name == param && intent != "intent(in)" && intent != "optional" && !intent.starts_with('@'))) {
            let attribute=body.iter().find_map(|s|match s {Stmt::DummyIntent{name,intent} if name==param&&intent!="intent(in)"&&intent!="optional"&&!intent.starts_with('@')=>Some(intent.as_str()),_=>None}).unwrap();
            let message=format!("attribute `{attribute}` on dummy argument `{param}` is not yet supported in ELEMENTAL function `{name}`");
            return Err(crate::source_diagnostics::declaration(body,param,Some(attribute),message));
        }
    }
    Ok(())
}

pub fn validate_program(program: &Program) -> Result<()> {
    fn walk(body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    elemental: true,
                    params,
                    return_type,
                    body,
                    ..
                } => {
                    validate(name, params, return_type, body)?;
                    walk(body)?;
                }
                Stmt::Subroutine { name, elemental: true, params, body, .. } => {
                    validate_subroutine(name, params, body)?;
                    walk(body)?;
                }
                Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => walk(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    walk(&program.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(source: &str) -> Program {
        crate::parser::parse_with_src(source, &crate::lexer::lex(source), "elemental.f90").unwrap()
    }
    #[test]
    fn validates_elemental_declarations_and_purity() {
        for (decl, statement, expected) in [
            ("integer :: i,j", "j=i", "requires INTENT(IN)"),
            (
                "integer,intent(out)::i\ninteger::j",
                "j=1",
                "requires INTENT(IN)",
            ),
            (
                "integer,intent(in)::i(2)\ninteger::j",
                "j=1",
                "requires scalar",
            ),
            (
                "integer,intent(in)::i\ninteger::j(2)",
                "j=1",
                "requires scalar",
            ),
            (
                "integer,pointer,intent(in)::i\ninteger::j",
                "j=i",
                "attribute `pointer`",
            ),
            (
                "integer,intent(in)::i\ninteger::j",
                "print *,i\nj=i",
                "external I/O",
            ),
            (
                "integer,intent(in)::i\ninteger::j",
                "i=2\nj=i",
                "cannot modify dummy",
            ),
            (
                "integer,intent(in)::i\ninteger::j",
                "j=unknown(i)",
                "impure or unknown",
            ),
            (
                "integer,intent(in)::i",
                "j=.true.",
                "supported scalar result",
            ),
        ] {
            let p = parse(&format!(
                "elemental function f(i) result(j)\n{decl}\n{statement}\nend function f"
            ));
            let error = crate::purity::validate(&p).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
        let p = parse(include_str!("../tests/70_elemental_functions.f90"));
        crate::purity::validate(&p).unwrap();
        assert_eq!(functions(&p).len(), 3);
    }
    #[test]
    fn rejects_nonconforming_elemental_actual_arguments() {
        for statement in ["print *,f(a,b)", "a=f(a,b)", "print *,sum(f(a,b))"] {
            let p=parse(&format!("integer::a(2,3),b(3,2)\na=1\nb=2\n{statement}\ncontains\nelemental integer function f(x,y)\ninteger,intent(in)::x,y\nf=x+y\nend function f\nend"));
            let p = crate::parameters::resolve(crate::derived::resolve(p).unwrap()).unwrap();
            let error = crate::array_calls::resolve(p).unwrap_err().to_string();
            assert!(error.contains("nonconforming"), "{error}");
        }
    }
    #[test]
    fn rejects_resolved_unsupported_elemental_kinds() {
        let p=parse("integer,parameter::k=16\ncontains\nelemental function f(i) result(j)\ninteger(kind=k),intent(in)::i\ninteger(kind=k)::j\nj=i\nend function f\nend");
        let result=crate::derived::resolve(p).and_then(crate::parameters::resolve).and_then(crate::array_calls::resolve);
        assert!(result.unwrap_err().to_string().contains("supported scalar result"));
    }

    #[test]
    fn lowers_elemental_array_calls() {
        let p = parse(include_str!("../tests/70_elemental_functions.f90"));
        let p = crate::array_calls::resolve(
            crate::parameters::resolve(crate::derived::resolve(p).unwrap()).unwrap(),
        )
        .unwrap();
        crate::sections::resolve(crate::reductions::resolve(p).unwrap()).unwrap();
    }
}

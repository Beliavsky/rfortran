//! Metadata and declaration checks for the supported ELEMENTAL function subset.
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
        bail!("ELEMENTAL function `{name}` requires scalar dummy arguments and a scalar result");
    }
    if !result_kind(name, declared, body)
        .as_ref()
        .is_some_and(numeric)
    {
        bail!("ELEMENTAL function `{name}` currently requires an explicitly declared numeric scalar result");
    }
    for param in params {
        if !body.iter().any(
            |s| matches!(s,Stmt::VarDecl{kind,names} if names.contains(param) && numeric(kind)),
        ) {
            bail!("ELEMENTAL dummy argument `{param}` currently requires an explicitly declared numeric scalar type");
        }
        if !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name == param && intent == "intent(in)")) {
            bail!("ELEMENTAL function dummy argument `{param}` requires INTENT(IN)");
        }
        if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name == param && intent != "intent(in)")) {
            bail!("additional attributes on ELEMENTAL dummy argument `{param}` are not yet supported");
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
                "integer,optional,intent(in)::i\ninteger::j",
                "j=i",
                "additional attributes",
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
                "integer,intent(in)::i\nlogical::j",
                "j=.true.",
                "numeric scalar result",
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
        assert!(result.unwrap_err().to_string().contains("numeric scalar result"));
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

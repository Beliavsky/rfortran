// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Source-bearing early compiler errors, independent of diagnostic rendering.
use crate::{
    ast::Stmt,
    lexer::{Token, TokenKind},
};
use std::ops::Range;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct LocatedError {
    message: String,
    range: Range<usize>,
    highlight: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourceOrigin {
    pub name: String,
    pub range: Range<usize>,
}

/// Check source declarations before backend specialization can obscure a
/// missing function result type with an unrelated unsupported-feature error.
pub(crate) fn validate_function_results(program: &crate::ast::Program) -> anyhow::Result<()> {
    use crate::ast::{Expr, Implicit};
    // A local IMPLICIT statement can override part of the inherited mapping.
    // Only whether each initial letter has a type matters for this check.
    fn mapping(body: &[Stmt], mut inherited: [bool; 26]) -> [bool; 26] {
        for stmt in body {
            let rules = match stmt {
                Stmt::Implicit(Implicit::None) => { inherited = [false; 26]; continue; }
                Stmt::Implicit(Implicit::Rule { letter_ranges, .. }) => vec![letter_ranges],
                Stmt::Implicit(Implicit::Rules { rules }) => rules.iter().map(|r| &r.letter_ranges).collect(),
                _ => continue,
            };
            for ranges in rules {
                for range in ranges {
                    for letter in range.start.to_ascii_lowercase()..=range.end.unwrap_or(range.start).to_ascii_lowercase() {
                        if letter.is_ascii_lowercase() { inherited[(letter as u8-b'a') as usize] = true; }
                    }
                }
            }
        }
        inherited
    }
    fn walk(body: &[Stmt], inherited: [bool; 26], main_contains: Option<&[String]>) -> anyhow::Result<()> {
        let scope = mapping(body, inherited);
        let mut offset = None;
        for stmt in body {
            if let Stmt::SourceLocation(p) = stmt { offset = Some(*p); continue; }
            match stmt {
                Stmt::Function { name, return_type, body, .. } => {
                    // Top-level external procedures do not inherit the main
                    // program's IMPLICIT statements; CONTAINS procedures do.
                    let parent = if main_contains.is_some_and(|names| !names.contains(name)) { [true;26] } else { scope };
                    let local = mapping(body, parent);
                    let result = match body.last() { Some(Stmt::Return(Some(Expr::Ident(n)))) => n, _ => name };
                    let declared = return_type.is_some() || body.iter().any(|s| match s {
                        Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.contains(result),
                        Stmt::ArrayDecl { name, .. } => name == result,
                        Stmt::InitializedDecl { entities, .. } => entities.iter().any(|(n,_)| n == result),
                        _ => false,
                    });
                    let implicitly_typed = result.bytes().next().filter(u8::is_ascii_lowercase).is_some_and(|c| local[(c-b'a') as usize]);
                    if !declared && !implicitly_typed {
                        let message = format!("function result `{result}` has no declared type under IMPLICIT NONE");
                        let start = body.iter().find_map(statement_offset).or(offset);
                        if let Some(start) = start {
                            let end = body.iter().filter_map(statement_offset).find(|p| *p > start).unwrap_or(usize::MAX);
                            return Err(LocatedError { message, range: start..end, highlight: Some(result.clone()) }.into());
                        }
                        anyhow::bail!(message);
                    }
                    walk(body, parent, None)?;
                }
                Stmt::Subroutine { name, body, .. } => {
                    let parent = if main_contains.is_some_and(|names| !names.contains(name)) { [true;26] } else { scope };
                    walk(body, parent, None)?;
                }
                Stmt::Module { body, .. } => walk(body, [true;26], None)?,
                Stmt::Block { body } => walk(body, scope, None)?,
                _ => {},
            }
        }
        Ok(())
    }
    walk(&program.body, [true;26], Some(&program.contains))
}

#[cfg(test)]
mod result_type_tests {
    fn validate(source: &str) -> anyhow::Result<()> {
        let p=crate::parser::parse_with_src(source,&crate::lexer::lex(source),"result.f90").unwrap();
        super::validate_function_results(&p)
    }
    #[test]
    fn preserves_implicit_scope_and_explicit_result_types() {
        for source in [
            "function f()\nf=1.0\nend function",
            "implicit none\nend\nfunction f()\nf=1.0\nend function",
            "module m\nimplicit none\ncontains\nfunction f()\nimplicit real(f)\nf=1.0\nend function\nend module",
            "module m\nimplicit none\ncontains\nfunction f()\nimplicit real(a-e),integer(i-n)\nf=1.0\nend function\nend module",
            "function f()\nimplicit none(external)\nf=1.0\nend function",
            "module m\nimplicit none\ncontains\nreal function f()\nf=1.0\nend function\nend module",
            "function f() result(r)\nimplicit none\nreal::r(2)\nr=1\nend function",
        ] {
            let result=validate(source);
            // A partial local IMPLICIT mapping leaves F without a type.
            if source.contains("implicit real(a-e)") { assert!(result.is_err(),"{source}"); }
            else {assert!(result.is_ok(),"{source}: {result:?}");}
        }
    }
}
/// Module lowering gives symbols internal identities; diagnostics use the
/// entity names the programmer wrote instead of exposing those identities.
pub(crate) fn user_names(message: &str) -> String {
    message.split('`').enumerate().map(|(i, part)| {
        let part = if i % 2 == 1 && part.starts_with("@module$") {
            part.rsplit('$').next().unwrap_or(part)
        } else if i % 2 == 1 { crate::block_scopes::original(part) } else { part };
        if i == 0 { part.to_owned() } else { format!("`{part}") }
    }).collect()
}
/// The source offset a statement marker records, for locating errors.
pub(crate) fn statement_offset<S: std::borrow::Borrow<crate::ast::Stmt>>(stmt: &S) -> Option<usize> {
    match stmt.borrow() { crate::ast::Stmt::SourceLocation(p) => Some(*p), _ => None }
}
pub(crate) fn at(error: anyhow::Error, offset: Option<usize>) -> anyhow::Error {
    if error.downcast_ref::<LocatedError>().is_some() {
        return error;
    }
    if let Some(offset) = offset {
        LocatedError {
            message: format!("{error:#}"),
            range: offset..offset,
            highlight: None,
        }
        .into()
    } else {
        error
    }
}
pub(crate) fn declaration(
    body: &[Stmt],
    name: &str,
    attribute: Option<&str>,
    message: String,
) -> anyhow::Error {
    let mut offset = None;
    for (i, stmt) in body.iter().enumerate() {
        if let Stmt::SourceLocation(pos) = stmt {
            offset = Some(*pos);
            continue;
        }
        let matches = match stmt {
            Stmt::DummyIntent { name: n, intent } => {
                n == name && attribute.is_none_or(|a| intent == a)
            }
            Stmt::VarDecl { names, .. } => attribute.is_none() && names.iter().any(|n| n == name),
            Stmt::ArrayDecl { name: n, .. } => attribute.is_none() && n == name,
            _ => false,
        };
        if matches {
            if let Some(start) = offset {
                let end = body[i + 1..]
                    .iter()
                    .find_map(|s| match s {
                        Stmt::SourceLocation(pos) if *pos > start => Some(*pos),
                        _ => None,
                    })
                    .unwrap_or(usize::MAX);
                return LocatedError {
                    message,
                    range: start..end,
                    highlight: Some(attribute.unwrap_or(name).into()),
                }
                .into();
            }
        }
    }
    at(
        anyhow::anyhow!(message),
        body.iter().find_map(|s| {
            if let Stmt::SourceLocation(pos) = s {
                Some(*pos)
            } else {
                None
            }
        }),
    )
}
fn range(error: &LocatedError, src: &str, tokens: &[Token]) -> Range<usize> {
    let start = error.range.start.min(src.len());
    let end = error.range.end.min(src.len());
    if let Some(hint) = &error.highlight {
        if let Some(token) = tokens.iter().find(|t| {
            t.span.start >= start
                && t.span.start < end
                && matches!(&t.kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case(hint))
        }) {
            return token.span.clone();
        }
    }
    tokens
        .iter()
        .find(|t| t.span.start >= start)
        .map(|t| t.span.clone())
        .unwrap_or(src.len()..src.len())
}
/// Render only errors which carry a trustworthy source offset. Legacy errors
/// retain their existing text until their producing pass supplies a location.
pub(crate) fn report(
    error: anyhow::Error,
    src: &str,
    tokens: &[Token],
    filename: &str,
    origins: Option<&[SourceOrigin]>,
) -> anyhow::Error {
    let Some(located) = error.downcast_ref::<LocatedError>() else {
        return error;
    };
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFile,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut span = range(located, src, tokens);
    let mut name = filename;
    let mut source = src;
    if let Some(origin) = origins.and_then(|origins| {
        origins
            .iter()
            .find(|origin| origin.range.contains(&span.start))
    }) {
        name = &origin.name;
        source = &src[origin.range.clone()];
        span = span.start - origin.range.start..span.end.min(origin.range.end) - origin.range.start;
    }
    let message = user_names(&located.message);
    let diagnostic = Diagnostic::error()
        .with_message(message)
        .with_labels(vec![Label::primary((), span)]);
    let file = SimpleFile::new(name, source);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diagnostic);
    anyhow::anyhow!("compile failed")
}

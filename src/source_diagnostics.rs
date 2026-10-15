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
    // Module lowering gives symbols internal identities; diagnostics use the
    // entity names the programmer wrote instead of exposing those identities.
    let message = located.message.split('`').enumerate().map(|(i, part)| {
        let part = if i % 2 == 1 && part.starts_with("@module$") {
            part.rsplit('$').next().unwrap_or(part)
        } else { part };
        if i == 0 { part.to_owned() } else { format!("`{part}") }
    }).collect::<String>();
    let diagnostic = Diagnostic::error()
        .with_message(message)
        .with_labels(vec![Label::primary((), span)]);
    let file = SimpleFile::new(name, source);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diagnostic);
    anyhow::anyhow!("compile failed")
}

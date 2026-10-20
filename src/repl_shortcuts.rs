// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! Expand interactive shortcuts to standard Fortran before parsing/evaluation.
use crate::ast::{Program, Stmt};
use crate::lexer::{lex, TokenKind};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;

pub(super) fn command(text: &str) -> String {
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    let word = &text[..end];
    let Some(name) = word.strip_prefix('.').or_else(|| word.strip_prefix(':')) else {
        return text.into();
    };
    // Do not mistake Fortran literals/operators (.5, .true., .not.) for commands.
    if !name.chars().all(|c| c.is_ascii_alphabetic() || c == '!') {
        return text.into();
    }
    let name = match name.to_ascii_lowercase().as_str() {
        "clear" | "reset" => "reset".to_string(),
        "quit!" => "quit".into(),
        other => other.to_string(),
    };
    format!(":{name}{}", &text[end..])
}

pub(super) fn expand(source: &str, session: &super::Session) -> Result<Option<String>> {
    let source = source.trim();
    let end = source.find(char::is_whitespace).unwrap_or(source.len());
    let shortcut = source[..end].to_ascii_lowercase();
    if !matches!(shortcut.as_str(), "let" | "const") {
        return Ok(None);
    }
    let rest = source[end..].trim();
    let tokens = lex(rest);
    if matches!(tokens.first().map(|t| &t.kind), Some(TokenKind::Eq)) {
        return Ok(None);
    }
    if tokens.is_empty() {
        return Ok(None);
    } // A variable named LET/CONST can be printed.
    let prepared = super::dynamic::prepare(
        Program {
            name: "<repl-shortcut>".into(),
            body: session.declarations.clone(),
            contains: vec![],
        },
        &session.allocations,
        &session.names,
        false,
    )?;
    let mut headers = prepared.program.body;
    let mut known = session.names.clone();
    let mut depth = 0;
    let mut chunks = vec![];
    let mut start = 0;
    for token in &tokens {
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => {
                chunks.push(&rest[start..token.span.start]);
                start = token.span.end;
            }
            _ => {}
        }
    }
    chunks.push(&rest[start..]);
    let mut output = String::new();
    for chunk in chunks {
        let chunk = chunk.trim();
        let mut depth = 0;
        let equal = lex(chunk)
            .into_iter()
            .find_map(|t| {
                match t.kind {
                    TokenKind::LParen | TokenKind::LBracket => depth += 1,
                    TokenKind::RParen | TokenKind::RBracket => depth -= 1,
                    TokenKind::Eq if depth == 0 => return Some(t.span),
                    _ => {}
                }
                None
            })
            .context("LET/CONST requires name = expression")?;
        let lhs = chunk[..equal.start].trim();
        let rhs = chunk[equal.end..].trim();
        let wrapped = super::wrapped(chunk, false);
        let parsed = crate::parser::parse_with_src(&wrapped, &lex(&wrapped), "<repl-shortcut>")
            .map_err(|_| anyhow::anyhow!("invalid LET/CONST assignment"))?;
        let executable: Vec<_> = parsed
            .body
            .iter()
            .filter(|s| !matches!(s, Stmt::SourceLocation(_) | Stmt::Implicit(_)))
            .collect();
        let (name, value, explicit_shape) = match executable.as_slice() {
            [Stmt::Assign { name, value }] => (name, value, false),
            [Stmt::AssignIndex { name, value, .. }] if shortcut == "const" => (name, value, true),
            _ => bail!(
                "LET requires a new variable name; CONST accepts a name or an explicit array shape"
            ),
        };
        if !known.insert(name.clone()) {
            bail!(
                "{} name `{name}` is already declared; use ordinary assignment for variables",
                shortcut.to_ascii_uppercase()
            );
        }
        let (kind, extent) = super::dynamic::infer(value, &headers)
            .with_context(|| format!("could not infer type for `{name}` from `{rhs}`"))?;
        super::scalar(&kind)?;
        let kind = super::export::kind_text(&kind)?;
        let declaration = if shortcut == "const" {
            if explicit_shape && extent.is_none() {
                bail!("CONST array declaration requires an array initializer");
            }
            let lhs = if explicit_shape {
                lhs.to_string()
            } else if let Some(n) = extent {
                format!("{name}({n})")
            } else {
                name.clone()
            };
            format!("{kind}, parameter :: {lhs} = {rhs}\n")
        } else if extent.is_some() {
            format!("{kind}, allocatable :: {name}(:)\n{name} = {rhs}\n")
        } else {
            format!("{kind} :: {name}\n{name} = {rhs}\n")
        };
        let wrapped = super::wrapped(&declaration, false);
        let parsed = crate::parser::parse_with_src(&wrapped, &lex(&wrapped), "<repl-shortcut>")
            .map_err(|_| anyhow::anyhow!("could not expand LET/CONST declaration"))?;
        headers.extend(parsed.body.into_iter().filter(|s| {
            matches!(
                s,
                Stmt::VarDecl { .. }
                    | Stmt::ArrayDecl { .. }
                    | Stmt::InitializedDecl { .. }
                    | Stmt::DummyIntent { .. }
            )
        }));
        output.push_str(&declaration);
    }
    Ok(Some(output))
}

pub(super) fn selected(arguments: &str, names: &HashSet<String>) -> Result<Vec<String>> {
    let mut selected: Vec<_> = if arguments.trim().is_empty() {
        names.iter().cloned().collect()
    } else {
        arguments
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .collect()
    };
    for name in &selected {
        if !names.contains(name) {
            bail!("unknown REPL variable `{name}`");
        }
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Declaration advice on original source, before initializer/attribute lowering.
use crate::ast::{Program, Stmt};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

struct Warning {
    category: &'static str,
    message: String,
    span: Range<usize>,
    note: &'static str,
}

fn word(token: &Token, name: &str) -> bool {
    matches!(&token.kind, TokenKind::Ident(n) | TokenKind::AttrStatement(n) if n == name)
}

// Attribute words inside kind/length expressions are names, not attributes.
fn attribute(tokens: &[Token], name: &str) -> bool {
    let mut depth = 0;
    for token in tokens {
        match token.kind {
            TokenKind::DColon => break,
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => depth -= 1,
            _ if depth == 0 && word(token, name) => return true,
            _ => {}
        }
    }
    false
}

fn name_span(tokens: &[Token], name: &str) -> Option<Range<usize>> {
    tokens.iter().find(|t| matches!(&t.kind, TokenKind::Ident(n) if n == name)).map(|t| t.span.clone())
}

// Find the original entity initializer, including pointer => NULL() which the
// parser removes. Nested commas in bounds, constructors and calls are skipped.
fn initializer(tokens: &[Token], name: &str) -> Option<Range<usize>> {
    let start = tokens.iter().position(|t| matches!(t.kind, TokenKind::DColon))? + 1;
    let mut depth = 0;
    let mut entity = None;
    let mut first = true;
    for token in &tokens[start..] {
        if first {
            if let TokenKind::Ident(n) = &token.kind {
                entity = (n == name).then(|| token.span.clone());
            }
            first = false;
        }
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { first = true; entity = None; }
            TokenKind::Eq | TokenKind::Arrow if depth == 0 && entity.is_some() => return entity,
            _ => {}
        }
    }
    None
}

fn statement_tokens<'a>(tokens: &'a [Token], start: usize, end: usize, src: &str) -> &'a [Token] {
    let first = tokens.partition_point(|t| t.span.start < start);
    let last = tokens.partition_point(|t| t.span.start < end);
    let tokens = &tokens[first..last];
    for (i, pair) in tokens.windows(2).enumerate() {
        let gap = &src[pair[0].span.end..pair[1].span.start];
        let line = gap.split('\n').next().unwrap_or(gap).split('!').next().unwrap_or("");
        if line.contains(';') || gap.contains('\n') && !line.trim_end().ends_with('&') {
            return &tokens[..i + 1];
        }
    }
    tokens
}

fn analyze(body: &[Stmt], procedure: Option<(&str, &[String])>, tokens: &[Token], src: &str, out: &mut Vec<Warning>) {
    // SourceLocation survives parser normalization of SAVE/DATA statements.
    let positions: Vec<_> = body.iter().enumerate().filter_map(|(i, s)| match s {
        Stmt::SourceLocation(p) => Some((i, *p)), _ => None,
    }).collect();
    let groups: Vec<_> = positions.iter().enumerate().map(|(j, (i, pos))| {
        let (last, end) = positions.get(j + 1).copied().unwrap_or((body.len(), usize::MAX));
        (&body[i + 1..last], statement_tokens(tokens, *pos, end, src))
    }).collect();
    if let Some((procedure_name, params)) = procedure {
        let mut saved = HashSet::new();
        let mut save_all = false;
        let mut exempt = HashSet::new();
        let mut spans = HashMap::new();
        for (_, ts) in &groups {
            if matches!(ts.first().map(|t| &t.kind), Some(TokenKind::KwSaveAll)) { save_all = true; }
            if ts.first().is_some_and(|t| word(t, "save")) {
                saved.extend(ts.iter().skip(1).filter_map(|t| match &t.kind {
                    TokenKind::Ident(n) => Some(n.clone()), _ => None,
                }));
            }
        }
        for statement in body {
            if let Stmt::DerivedDecl { names, attributes, .. } = statement {
                if attributes.iter().any(|a| a.starts_with("intent(") || matches!(a.as_str(), "pointer" | "value")) {
                    exempt.extend(names.iter().cloned());
                }
            }
            if let Stmt::DummyIntent { name, intent } = statement {
                if intent.starts_with("intent(") || intent == "external" || intent.starts_with("procedure(")
                    || intent.starts_with("@interface:") || intent == "pointer" {
                    exempt.insert(name.clone());
                }
            }
        }
        let mut seen = HashSet::new();
        for (statements, ts) in &groups {
            for statement in *statements {
                let names: Vec<&String> = match statement {
                    Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.iter().collect(),
                    Stmt::ArrayDecl { name, .. } => vec![name],
                    Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n).collect(),
                    _ => vec![],
                };
                for name in names {
                    if let Some(span) = name_span(ts, name) { spans.entry(name.clone()).or_insert(span); }
                    if attribute(ts, "value") || attribute(ts, "pointer") { exempt.insert(name.clone()); }
                    if !save_all && !saved.contains(name) && !attribute(ts, "save")
                        && !attribute(ts, "parameter") && !params.contains(name) {
                        if let Some(span) = initializer(ts, name) {
                            if seen.insert(name.clone()) {
                                out.push(Warning {
                                    category: "implicit-save",
                                    message: format!("local variable `{name}` of procedure `{procedure_name}` has implicit SAVE because of declaration initialization"),
                                    span,
                                    note: "add explicit SAVE to document persistent state, or initialize in an executable statement for initialization on every invocation",
                                });
                            }
                        }
                    }
                }
            }
        }
        let header = groups.first().map(|(_, ts)| *ts).unwrap_or(&[]);
        let mut invoked = HashSet::new();
        crate::procedure_arguments::invoked(body, &mut invoked);
        for param in params {
            if exempt.contains(param) || !spans.contains_key(param) && invoked.contains(param) { continue; }
            if let Some(span) = spans.get(param).cloned().or_else(|| name_span(header, param)) {
                out.push(Warning {
                    category: "missing-intent",
                    message: format!("dummy argument `{param}` of procedure `{procedure_name}` has no explicit INTENT"),
                    span,
                    note: "specify INTENT(IN), INTENT(OUT), or INTENT(INOUT) according to the procedure's contract; INTENT(OUT) makes the argument undefined on entry",
                });
            }
        }
    }
    for statement in body {
        match statement {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => analyze(body, Some((name, params)), tokens, src, out),
            Stmt::Module { body, .. } => analyze(body, None, tokens, src, out),
            Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => analyze(body, procedure.map(|(n, _)| (n, &[][..])), tokens, src, out),
            Stmt::If { then_body, else_body, .. } => {
                analyze(then_body, procedure.map(|(n, _)| (n, &[][..])), tokens, src, out);
                if let Some(body) = else_body { analyze(body, procedure.map(|(n, _)| (n, &[][..])), tokens, src, out); }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { analyze(&case.body, procedure.map(|(n, _)| (n, &[][..])), tokens, src, out); }
                if let Some(body) = default { analyze(body, procedure.map(|(n, _)| (n, &[][..])), tokens, src, out); }
            }
            _ => {}
        }
    }
}

pub fn check(program: &Program, src: &str, tokens: &[Token], filename: &str,
    origins: Option<&[crate::source_diagnostics::SourceOrigin]>, settings: &SemaSettings,
    options: &[String]) -> Result<()> {
    let mut intent_settings = settings.clone();
    intent_settings.wall = false; // Advice about an omitted attribute is opt-in.
    let controls = HashMap::from([
        ("implicit-save", crate::unused::selected("implicit-save", false, settings, options)),
        ("missing-intent", crate::unused::selected("missing-intent", false, &intent_settings, options)),
    ]);
    if !controls.values().any(|(enabled, _)| *enabled) { return Ok(()); }
    let mut warnings = vec![];
    analyze(&program.body, None, tokens, src, &mut warnings);
    warnings.sort_by_key(|w| w.span.start);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut failed = false;
    for warning in warnings {
        let (enabled, error) = controls[warning.category];
        if !enabled { continue; }
        let (file, source, span) = if let Some(origin) = origins.unwrap_or(&[]).iter().find(|o| o.range.contains(&warning.span.start)) {
            (origin.name.as_str(), &src[origin.range.clone()],
                warning.span.start-origin.range.start..warning.span.end.min(origin.range.end)-origin.range.start)
        } else { (filename, src, warning.span) };
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("{} [{}]", warning.message, warning.category))
            .with_labels(vec![Label::primary((), span)])
            .with_notes(vec![warning.note.into()]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(),
            &SimpleFile::new(file, source), &diagnostic)?;
        failed |= error;
    }
    if failed { bail!("declaration warnings treated as errors"); }
    Ok(())
}

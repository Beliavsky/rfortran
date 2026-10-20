// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! Keep source history, independently of the lowered code used for evaluation.
//! Hoist declarations but keep mutable initializers at their execution point.
use crate::ast::{Expr, Stmt, TypeSpec};
use crate::lexer::{lex, TokenKind};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::Path;

#[derive(Default)]
pub(super) struct History {
    declarations: Vec<String>,
    executable: Vec<String>,
}

impl History {
    pub(super) fn submission(source: &str, body: &[Stmt]) -> Result<Self> {
        let mut history = Self::default();
        let mut groups: Vec<(usize, Vec<&Stmt>)> = vec![];
        for stmt in body {
            if let Stmt::SourceLocation(offset) = stmt {
                if groups.last().is_none_or(|(start, _)| start != offset) {
                    groups.push((*offset, vec![]));
                }
            } else {
                groups
                    .last_mut()
                    .context("missing REPL source location")?
                    .1
                    .push(stmt);
            }
        }
        // submit receives the parser wrapper: IMPLICIT NONE, input, END.
        let end = source
            .len()
            .checked_sub("end\n".len())
            .context("missing REPL source wrapper")?;
        for (i, (start, stmts)) in groups.iter().enumerate() {
            if stmts.is_empty() || stmts.iter().all(|s| matches!(s, Stmt::Implicit(_))) {
                continue;
            }
            let stop = groups.get(i + 1).map_or(end, |(start, _)| *start);
            let text = source
                .get(*start..stop)
                .context("invalid REPL source span")?
                .trim();
            if stmts.iter().all(|s| {
                matches!(
                    s,
                    Stmt::VarDecl { .. }
                        | Stmt::ArrayDecl { .. }
                        | Stmt::InitializedDecl { .. }
                        | Stmt::DummyIntent { .. }
                )
            }) {
                let parameter = stmts.iter().any(|s| matches!(s,
                    Stmt::InitializedDecl {attributes,..} if attributes.iter().any(|a| a=="parameter")));
                if parameter {
                    history.declarations.push(text.into());
                } else {
                    let initialized: Vec<_> = stmts
                        .iter()
                        .flat_map(|s| match s {
                            Stmt::InitializedDecl { entities, .. } => entities
                                .iter()
                                .filter_map(|(name, value)| value.as_ref().map(|_| name.clone()))
                                .collect(),
                            _ => vec![],
                        })
                        .collect();
                    let (declaration, assignments) = split_initializers(text, &initialized)?;
                    history.declarations.push(declaration);
                    history.executable.extend(assignments);
                }
            } else {
                history.executable.push(text.into());
            }
        }
        Ok(history)
    }

    pub(super) fn inferred(&mut self, declarations: &[Stmt]) -> Result<()> {
        let allocatable: HashSet<_> = declarations
            .iter()
            .filter_map(|s| match s {
                Stmt::DummyIntent { name, intent } if intent == "allocatable" => {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        for stmt in declarations {
            match stmt {
                Stmt::VarDecl { kind, names } => {
                    self.declarations
                        .push(format!("{} :: {}", kind_text(kind)?, names.join(", ")))
                }
                Stmt::ArrayDecl { kind, name, dims } if allocatable.contains(name.as_str()) => {
                    if !dims
                        .iter()
                        .all(|d| matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
                    {
                        bail!("unexpected inferred array shape in REPL export");
                    }
                    self.declarations.push(format!(
                        "{}, allocatable :: {name}({})",
                        kind_text(kind)?,
                        vec![":"; dims.len()].join(",")
                    ));
                }
                Stmt::DummyIntent { .. } => {}
                _ => bail!("unsupported inferred declaration in REPL export"),
            }
        }
        Ok(())
    }

    pub(super) fn append(&mut self, mut submission: Self) {
        self.declarations.append(&mut submission.declarations);
        self.executable.append(&mut submission.executable);
    }

    pub(super) fn source(&self, names: &HashSet<String>) -> String {
        let mut program = "repl_session".to_string();
        while names.contains(&program) {
            program.push_str("_export");
        }
        let mut source =
            format!("! Exported from an rfort REPL session.\nprogram {program}\nimplicit none\n");
        for declaration in &self.declarations {
            source.push_str(declaration);
            source.push('\n');
        }
        if !self.executable.is_empty() {
            source.push('\n');
        }
        for statement in &self.executable {
            source.push_str(statement);
            source.push('\n');
        }
        source.push_str(&format!("end program {program}\n"));
        source
    }

    pub(super) fn save(&self, path: &Path, names: &HashSet<String>) -> Result<()> {
        let source = self.source(names);
        // Write beside the destination, then replace it only once complete.
        // A failed write must not truncate a previously saved session.
        struct Temporary(std::path::PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = Temporary(path.with_file_name(
            format!(".rfort-save-{}-{}-{}.tmp", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
        ));
        (|| -> Result<()> {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary.0)?;
            file.write_all(source.as_bytes())?;
            drop(file);
            std::fs::rename(&temporary.0, path)?;
            Ok(())
        })()
        .with_context(|| format!("could not save REPL session to {}", path.display()))
    }
}

pub(super) fn kind_text(kind: &TypeSpec) -> Result<String> {
    Ok(match kind {
        TypeSpec::Integer(None) => "integer".into(),
        TypeSpec::Integer(Some(k)) => format!("integer(kind={k})"),
        TypeSpec::IntegerKind(k) => format!("integer(kind={k})"),
        TypeSpec::Real => "real".into(),
        TypeSpec::RealKind(k) => format!("real(kind={k})"),
        TypeSpec::DoublePrecision => "double precision".into(),
        TypeSpec::Logical => "logical".into(),
        TypeSpec::LogicalKind(k) => format!("logical(kind={k})"),
        TypeSpec::Character(length) => format!("character(len={})", length.unwrap_or(1)),
        _ => bail!("unsupported inferred type in REPL export"),
    })
}

fn split_initializers(source: &str, names: &[String]) -> Result<(String, Vec<String>)> {
    if names.is_empty() {
        return Ok((source.into(), vec![]));
    }
    let mut depth = 0;
    let mut start = None;
    let mut ranges = vec![];
    for token in lex(source) {
        match token.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Eq if depth == 0 => start = Some((token.span.start, token.span.end)),
            TokenKind::Comma if depth == 0 => {
                if let Some((equal, value)) = start.take() {
                    ranges.push((equal, value, token.span.start));
                }
            }
            _ => {}
        }
    }
    if let Some((equal, value)) = start {
        ranges.push((equal, value, source.len()));
    }
    if ranges.len() != names.len() {
        bail!("could not separate REPL declaration initializers for export");
    }
    let mut declaration = String::new();
    let mut cursor = 0;
    let mut assignments = vec![];
    for ((equal, value, end), name) in ranges.into_iter().zip(names) {
        declaration.push_str(&source[cursor..equal]);
        assignments.push(format!("{name} = {}", source[value..end].trim()));
        cursor = end;
    }
    declaration.push_str(&source[cursor..]);
    Ok((declaration.trim().to_string(), assignments))
}

pub(super) fn save_path(command: &str) -> Result<&Path> {
    let name = command.strip_prefix(":save").unwrap_or("").trim();
    if name.is_empty() {
        bail!("usage: :save filename.f90");
    }
    let name = if name.starts_with('"') {
        name.strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .context("missing closing quote in :save filename")?
    } else {
        name
    };
    if name.is_empty() {
        bail!("usage: :save filename.f90");
    }
    Ok(Path::new(name))
}

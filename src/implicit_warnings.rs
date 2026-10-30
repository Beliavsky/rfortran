// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Diagnose implicit data declarations, and declare implicitly typed
//! variables explicitly so that lowering sees their types.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::procedure_arguments::{children, expressions};
use crate::sema::{ImplicitTyping, SemaSettings};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

struct Scope {
    parent: Option<usize>,
    // False denotes a data entity whose type has not been explicitly declared.
    names: HashMap<String, bool>,
    implicit: Vec<Option<TypeSpec>>,
    // A BLOCK is not a scoping unit for implicit typing: its implicitly
    // typed variables belong to the enclosing unit.
    block: bool,
    // A USE without ONLY of a module whose contents are unknown here, such as
    // an intrinsic module, may supply any undeclared name, in this scope or
    // its hosts, so no implicit declarations are inserted.
    opaque: bool,
}
struct Analysis {
    scopes: Vec<Scope>,
    modules: HashMap<String, Vec<Stmt>>,
    tokens: Vec<Token>,
    seen: HashSet<(usize, String)>,
    warnings: Vec<(String, TypeSpec, Range<usize>)>,
    // Each implicitly typed variable with the scope that owns it.
    implicit: Vec<(usize, String, TypeSpec)>,
    // Insert declarations of implicitly typed variables into their scopes.
    declare: bool,
    // Names used with parentheses or in CALL: procedures or statement
    // functions, never implicitly typed scalar variables, so not declared.
    called: HashSet<String>,
}

fn declarations(body: &[Stmt]) -> HashMap<String, bool> {
    let mut names = HashMap::new();
    for s in body {
        match s {
            Stmt::VarDecl { names: ns, .. } | Stmt::DerivedDecl { names: ns, .. } => {
                names.extend(ns.iter().map(|n| (n.clone(), true)));
            }
            Stmt::InitializedDecl { entities, .. } => {
                names.extend(entities.iter().map(|(n, _)| (n.clone(), true)));
            }
            Stmt::ArrayDecl { name, .. }
            | Stmt::DerivedType { name, .. }
            | Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. }
            | Stmt::GenericInterface { name, .. } => {
                names.insert(name.clone(), true);
            }
            Stmt::DummyIntent { name, intent }
                if intent == "external"
                    || intent.starts_with("procedure(")
                    || intent.starts_with("@interface:") =>
            {
                names.insert(name.clone(), true);
            }
            _ => {}
        }
    }
    names
}
impl Analysis {
    fn imports(&self, body: &[Stmt], visiting: &mut HashSet<String>) -> HashMap<String, bool> {
        let mut names = HashMap::new();
        for s in body {
            if let Stmt::Use {
                module,
                only,
                renames,
                ..
            } = s
            {
                let mut exports = HashMap::new();
                if visiting.insert(module.clone()) {
                    if let Some(body) = self.modules.get(module) {
                        exports = declarations(body);
                        exports.extend(self.imports(body, visiting));
                        let default_public = !body.iter().any(|s| {
                            matches!(s,
                            Stmt::Accessibility { public: false, names } if names.is_empty())
                        });
                        exports.retain(|n, _| {
                            body.iter()
                                .rev()
                                .find_map(|s| match s {
                                    Stmt::Accessibility { public, names } if names.contains(n) => {
                                        Some(*public)
                                    }
                                    _ => None,
                                })
                                .unwrap_or(default_public)
                        });
                    }
                    visiting.remove(module);
                }
                // ONLY names are associated even for compiler-provided intrinsic modules.
                if let Some(only) = only {
                    for n in only {
                        let local = renames
                            .iter()
                            .find(|(_, remote)| remote == n)
                            .map_or(n, |(local, _)| local);
                        names.insert(local.clone(), true);
                    }
                } else {
                    for (n, _) in exports {
                        let local = renames
                            .iter()
                            .find(|(_, remote)| remote == &n)
                            .map_or(&n, |(local, _)| local);
                        names.insert(local.clone(), true);
                    }
                }
                names.extend(renames.iter().map(|(local, _)| (local.clone(), true)));
                if module == "iso_fortran_env" {
                    for n in [
                        "int8",
                        "int16",
                        "int32",
                        "int64",
                        "real32",
                        "real64",
                        "real128",
                        "input_unit",
                        "output_unit",
                        "error_unit",
                        "iostat_end",
                        "iostat_eor",
                    ] {
                        if only.is_none() {
                            names.insert(n.into(), true);
                        }
                    }
                }
            }
        }
        names
    }
    fn lookup(&self, scope: usize, name: &str) -> Option<(usize, bool)> {
        self.scopes[scope]
            .names
            .get(name)
            .map(|&explicit| (scope, explicit))
            .or_else(|| self.scopes[scope].parent.and_then(|p| self.lookup(p, name)))
    }
    fn reference(&mut self, scope: usize, name: &str, offset: usize) {
        let name = name.split('%').next().unwrap_or(name);
        if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return;
        }
        let mut unit = scope;
        while self.scopes[unit].block {
            let Some(parent) = self.scopes[unit].parent else { break };
            unit = parent;
        }
        let (owner, explicit) = self.lookup(scope, name).unwrap_or((unit, false));
        if explicit || self.seen.contains(&(owner, name.to_owned())) {
            return;
        }
        let letter = (name.as_bytes()[0].to_ascii_lowercase() - b'a') as usize;
        let Some(kind) = self.scopes[owner].implicit[letter].clone() else {
            return;
        };
        self.scopes[owner].names.insert(name.into(), false);
        self.seen.insert((owner, name.into()));
        self.implicit.push((owner, name.into(), kind.clone()));
        // AST identifiers do not carry spans. Locate this use in its source statement.
        let span = self
            .tokens
            .iter()
            .enumerate()
            .find(|(i, t)| {
                t.span.start >= offset
                    && matches!(&t.kind, TokenKind::Ident(n) if n == name)
                    && !(matches!(self.tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Eq))
                        && i.checked_sub(1).is_some_and(|p| {
                            matches!(self.tokens[p].kind, TokenKind::LParen | TokenKind::Comma)
                        }))
            })
            .map(|(_, t)| t.span.clone())
            .unwrap_or(offset..offset);
        self.warnings.push((name.into(), kind, span));
    }
    fn expr(&mut self, scope: usize, e: &Expr, offset: usize) {
        if let Expr::Call(n, _) | Expr::Index(n, _) = e {
            self.called.insert(n.clone());
        }
        match e {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Section { name: n, .. } => {
                self.reference(scope, n, offset);
            }
            // A CALL name denotes a procedure unless it is a known data entity.
            // Never warn about an intrinsic, keyword label or component name.
            Expr::Call(n, _) if n.contains('%') => self.reference(scope, n, offset),
            _ => {}
        }
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for a in args {
                    self.expr(scope, a, offset);
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(scope, a, offset);
                self.expr(scope, b, offset);
            }
            Expr::Un(_, a) => self.expr(scope, a, offset),
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expr(scope, e, offset);
                }
            }
            _ => {}
        }
    }
    fn scope(
        &mut self,
        body: &mut Vec<Stmt>,
        parent: Option<usize>,
        params: &[String],
        result: Option<&str>,
        contained: Option<&[String]>,
        block: bool,
    ) {
        let mut names: HashMap<_, _> = params.iter().map(|n| (n.clone(), false)).collect();
        if let Some(result) = result {
            names.insert(result.into(), false);
        }
        names.extend(self.imports(body, &mut HashSet::new()));
        names.extend(declarations(body));
        let mut implicit = parent
            .map(|p| self.scopes[p].implicit.clone())
            .unwrap_or_else(|| {
                let defaults = ImplicitTyping::new();
                (b'a'..=b'z')
                    .map(|c| defaults.get_implicit_type(&(c as char).to_string()))
                    .collect()
            });
        fn apply(
            map: &mut [Option<TypeSpec>],
            kind: &TypeSpec,
            ranges: &[crate::ast::LetterRange],
        ) {
            for range in ranges {
                for c in range.start..=range.end.unwrap_or(range.start) {
                    if c.is_ascii_alphabetic() {
                        map[(c.to_ascii_lowercase() as u8 - b'a') as usize] = Some(kind.clone());
                    }
                }
            }
        }
        for s in body.iter() {
            match s {
                Stmt::Implicit(Implicit::None) => implicit.fill(None),
                Stmt::Implicit(Implicit::Rule {
                    type_spec,
                    letter_ranges,
                }) => apply(&mut implicit, type_spec, letter_ranges),
                Stmt::Implicit(Implicit::Rules { rules }) => {
                    for rule in rules {
                        apply(&mut implicit, &rule.type_spec, &rule.letter_ranges);
                    }
                }
                _ => {}
            }
        }
        let id = self.scopes.len();
        let opaque = parent.is_some_and(|p| self.scopes[p].opaque)
            || body.iter().any(|s| matches!(s, Stmt::Use { module, only: None, .. } if !self.modules.contains_key(module)));
        self.scopes.push(Scope {
            parent,
            names,
            implicit,
            block,
            opaque,
        });
        let offset = body
            .iter()
            .find_map(|s| {
                if let Stmt::SourceLocation(p) = s {
                    Some(*p)
                } else {
                    None
                }
            })
            .unwrap_or(0);
        for n in params {
            self.reference(id, n, offset);
        }
        self.body(body, id, offset, contained);
        if self.declare && !self.scopes[id].opaque {
            // Contained procedures were analyzed within body, so every
            // implicitly typed variable of this scope is known. A function
            // result keeps the type its function gives it.
            let declarations: Vec<Stmt> = self.implicit.iter()
                .filter(|(owner, name, _)| *owner == id && result != Some(name.as_str()) && !self.called.contains(name))
                .map(|(_, name, kind)| Stmt::VarDecl { kind: kind.clone(), names: vec![name.clone()] })
                .collect();
            if !declarations.is_empty() {
                let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
                body.splice(at..at, declarations);
            }
        }
    }
    fn body(
        &mut self,
        body: &mut [Stmt],
        scope: usize,
        mut offset: usize,
        contained: Option<&[String]>,
    ) {
        // Analyze the host before its procedures, so host implicit bindings are visible.
        for s in body.iter_mut() {
            match s {
                Stmt::SourceLocation(p) => {
                    offset = *p;
                    continue;
                }
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => continue,
                Stmt::Block { body } => {
                    self.scope(body, Some(scope), &[], None, None, true);
                    continue;
                }
                Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                    self.reference(scope, name, offset)
                }
                Stmt::Do { var, .. } => self.reference(scope, var, offset),
                Stmt::CallSub { name, .. } => { self.called.insert(name.clone()); }
                _ => {}
            }
            if let Stmt::VarDecl { kind, .. }
            | Stmt::ArrayDecl { kind, .. }
            | Stmt::InitializedDecl { kind, .. } = s
            {
                if let TypeSpec::CharacterLength(e) | TypeSpec::Referenced { source: e, .. } = kind
                {
                    self.expr(scope, e, offset);
                }
            }
            for e in expressions(s) {
                self.expr(scope, e, offset);
            }
            for b in children(s) {
                self.body(b, scope, offset, None);
            }
        }
        for s in body {
            match s {
                Stmt::Function {
                    name, params, body, ..
                } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                        _ => name.clone(),
                    };
                    let host = contained.is_none_or(|c| c.contains(name)).then_some(scope);
                    self.scope(body, host, params, Some(&result), None, false);
                }
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let host = contained.is_none_or(|c| c.contains(name)).then_some(scope);
                    self.scope(body, host, params, None, None, false);
                }
                Stmt::Module { body, .. } => self.scope(body, None, &[], None, None, false),
                _ => {}
            }
        }
    }
}

pub fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    let (enabled, error) = crate::unused::selected("implicit-typing", false, settings, options);
    if !enabled {
        return Ok(());
    }
    let mut analysis = Analysis {
        scopes: vec![],
        modules: program
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Module { name, body } => Some((name.clone(), body.clone())),
                _ => None,
            })
            .collect(),
        tokens: crate::lexer::lex(src),
        seen: HashSet::new(),
        warnings: vec![],
        implicit: vec![],
        called: HashSet::new(),
        declare: false,
    };
    analysis.scope(
        &mut program.body.clone(),
        None,
        &[],
        None,
        Some(&program.contains),
        false,
    );
    analysis.warnings.sort_by_key(|(_, _, span)| span.start);
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFile,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut failed = false;
    for (name, kind, span) in analysis.warnings {
        // Loaded module sources have already been checked when they were compiled.
        if span.start >= own_len {
            continue;
        }
        let (file, source, span) =
            if let Some(origin) = origins.iter().find(|o| o.range.contains(&span.start)) {
                (
                    origin.name.as_str(),
                    origin.source(src),
                    origin.span(span.clone()),
                )
            } else {
                (filename, src, span)
            };
        let diagnostic = if error {
            Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(format!(
            "variable `{name}` is implicitly typed as {} [implicit-typing]",
            crate::sema::show_type(&kind)
        ))
        .with_labels(vec![Label::primary((), span)])
        .with_notes(vec![
            "declare its type explicitly; IMPLICIT NONE prevents implicit typing".into(),
        ]);
        emit_to_io_write(
            &mut StandardStream::stderr(ColorChoice::Auto),
            &Config::default(),
            &SimpleFile::new(file, source),
            &diagnostic,
        )?;
        failed |= error;
    }
    if failed {
        bail!("implicit typing warnings treated as errors");
    }
    Ok(())
}

/// Declare every implicitly typed variable explicitly, with the type its
/// scoping unit's IMPLICIT rules give it, so that every later pass sees the
/// type: `i = 7.9` stores 7 and `x = 7` stores 7.0.
pub fn declare(mut program: Program) -> Program {
    let mut analysis = Analysis {
        scopes: vec![],
        modules: program
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Module { name, body } => Some((name.clone(), body.clone())),
                _ => None,
            })
            .collect(),
        tokens: vec![],
        seen: HashSet::new(),
        warnings: vec![],
        implicit: vec![],
        called: HashSet::new(),
        declare: true,
    };
    let contains = program.contains.clone();
    analysis.scope(&mut program.body, None, &[], None, Some(&contains), false);
    program
}

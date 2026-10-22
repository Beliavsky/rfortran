// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Logical operators do not make allocation/association inquiries into guards.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

type Names = HashSet<String>;
type Facts = HashSet<(&'static str, String)>;
fn fixed_character(kind: &TypeSpec) -> bool {
    match kind {
        TypeSpec::Character(_) => true,
        TypeSpec::CharacterLength(e) => !matches!(e.as_ref(), Expr::Ident(n) if n == "@deferred_character"),
        _ => false,
    }
}
#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    descriptor_args: Names,
    pure: bool,
}
#[derive(Clone, Default)]
struct Scope {
    names: Names,
    fixed_character: Names,
    procedures: HashMap<String, Procedure>,
    storage: HashMap<String, &'static str>,
    generics: Names,
}
impl Scope {
    fn remove(&mut self, name: &str) {
        self.names.remove(name);
        self.fixed_character.remove(name);
        self.procedures.remove(name);
        self.storage.remove(name);
        self.generics.remove(name);
    }
    fn intrinsic(&self, name: &str) -> bool { !self.names.contains(name) }
    fn declarations(&mut self, body: &[Stmt], params: &[String]) -> Names {
        let mut local: Names = params.iter().cloned().collect();
        for s in body {
            match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => local.extend(names.iter().cloned()),
                Stmt::ArrayDecl { name, .. } | Stmt::DerivedType { name, .. }
                | Stmt::Function { name, .. } | Stmt::Subroutine { name, .. }
                | Stmt::GenericInterface { name, .. } => { local.insert(name.clone()); }
                Stmt::InitializedDecl { entities, .. } => local.extend(entities.iter().map(|(n, _)| n.clone())),
                Stmt::DummyIntent { name, intent } if intent == "external" || intent.starts_with("procedure(") => { local.insert(name.clone()); }
                _ => {}
            }
        }
        for name in &local { self.remove(name); }
        self.names.extend(local.iter().cloned());
        for s in body {
            match s {
                Stmt::GenericInterface { name, .. } => { self.generics.insert(name.clone()); }
                Stmt::DummyIntent { name, intent } if intent == "allocatable" || intent == "pointer" => {
                    self.storage.insert(name.clone(), if intent == "allocatable" { "allocated" } else { "associated" });
                }
                Stmt::DerivedDecl { names, attributes, .. } => {
                    let query = if attributes.iter().any(|a| a == "allocatable") { Some("allocated") }
                        else if attributes.iter().any(|a| a == "pointer") { Some("associated") } else { None };
                    if let Some(query) = query { for name in names { self.storage.insert(name.clone(), query); } }
                }
                Stmt::Function { name, params, body, pure, .. }
                | Stmt::Subroutine { name, params, body, pure, .. } => {
                    let signature = crate::named_arguments::Signature::new(params, body);
                    let mut descriptor_args = signature.optional.clone();
                    for s in body {
                        match s {
                            Stmt::DummyIntent { name, intent } if intent == "allocatable" || intent == "pointer" => { descriptor_args.insert(name.clone()); }
                            Stmt::DerivedDecl { names, attributes, .. } if attributes.iter().any(|a| a == "allocatable" || a == "pointer") => descriptor_args.extend(names.iter().cloned()),
                            _ => {}
                        }
                    }
                    self.procedures.insert(name.clone(), Procedure { signature, descriptor_args, pure: *pure });
                }
                Stmt::VarDecl { kind, names } if fixed_character(kind) => self.fixed_character.extend(names.iter().cloned()),
                Stmt::ArrayDecl { kind, name, .. } if fixed_character(kind) => { self.fixed_character.insert(name.clone()); }
                Stmt::InitializedDecl { kind, entities, .. } if fixed_character(kind) => self.fixed_character.extend(entities.iter().map(|(n, _)| n.clone())),
                _ => {}
            }
        }
        local
    }
    fn rename_import(&mut self, local: &str, remote: &str, exports: &Scope) {
        self.remove(local);
        self.names.insert(local.into());
        if exports.fixed_character.contains(remote) { self.fixed_character.insert(local.into()); }
        if let Some(p) = exports.procedures.get(remote) { self.procedures.insert(local.into(), p.clone()); }
        if let Some(query) = exports.storage.get(remote) { self.storage.insert(local.into(), query); }
        if exports.generics.contains(remote) { self.generics.insert(local.into()); }
    }
}
fn facts(e: &Expr, truth: bool, scope: &Scope) -> Facts {
    ["allocated", "associated"].into_iter().filter(|n| scope.intrinsic(n)).flat_map(|query| {
        crate::optional_warnings::inquiry_facts(e, truth, query).into_iter().map(move |name| (query, name))
    }).collect()
}
fn argument(e: &Expr) -> &Expr { crate::named_arguments::keyword(e).map_or(e, |(_, value)| value) }
fn reference(name: &str, target: &str) -> bool { name == target || name.strip_prefix(target).is_some_and(|tail| tail.starts_with('%')) }
fn expressions(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().collect(),
        Expr::Bin(_, a, b) => vec![a, b],
        Expr::Un(_, e) => vec![e],
        Expr::Section { lower, upper, stride, .. } => [lower, upper, stride].into_iter().filter_map(|e| e.as_deref()).collect(),
        _ => vec![],
    }
}
fn may_change(e: &Expr, scope: &Scope) -> bool {
    if let Expr::Call(name, _) = e {
        if scope.generics.contains(name) { return true; }
        if scope.intrinsic(name) && matches!(name.as_str(), "kind" | "rank") { return false; }
        if let Some(p) = scope.procedures.get(name) {
            if !p.pure { return true; }
        } else if !scope.names.contains(name) && !name.starts_with('@')
            && crate::intrinsics::purity(name) != Some(crate::intrinsics::Purity::Pure) {
            return true;
        }
    }
    expressions(e).iter().any(|e| may_change(e, scope))
}
fn changes_storage(name: &str, scope: &Scope) -> bool {
    matches!(name, "@allocate" | "@deallocate" | "@nullify" | "@pointer_associate" | "move_alloc")
        || !name.starts_with('@') && (scope.names.contains(name) || crate::intrinsics::purity(name).is_none())
}
fn changes(body: &[Stmt], scope: &Scope) -> bool {
    body.iter().any(|s| {
        if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) { return false; }
        if matches!(s, Stmt::CallSub { name, .. } if changes_storage(name, scope)) { return true; }
        let mut s = s.clone();
        crate::procedure_arguments::expressions(&mut s).into_iter().any(|e| may_change(e, scope))
            || crate::procedure_arguments::children(&mut s).into_iter().any(|b| changes(b, scope))
    })
}
fn inquiries(e: &Expr, scope: &Scope, found: &mut Facts) {
    if let Expr::Call(name, _) = e {
        if scope.intrinsic(name) && matches!(name.as_str(), "kind" | "rank") { return; }
        if matches!(name.as_str(), "allocated" | "associated") && scope.intrinsic(name) {
            found.extend(facts(e, true, scope));
        }
    }
    for e in expressions(e) { inquiries(e, scope, found); }
}
fn unsafe_use(e: &Expr, target: &(&'static str, String), proven: &Facts, scope: &Scope) -> bool {
    if proven.contains(target) { return false; }
    match e {
        Expr::Ident(n) => reference(n, &target.1),
        Expr::Index(n, args) => reference(n, &target.1) || args.iter().any(|e| unsafe_use(e, target, proven, scope)),
        Expr::Section { name, .. } => reference(name, &target.1) || expressions(e).iter().any(|e| unsafe_use(e, target, proven, scope)),
        Expr::Call(name, args) if name == "@conditional" && !args.is_empty() => {
            let mut remaining = proven.clone();
            for pair in args[..args.len()-1].chunks_exact(2) {
                if unsafe_use(&pair[0], target, &remaining, scope) { return true; }
                let mut yes = remaining.clone(); yes.extend(facts(&pair[0], true, scope));
                if unsafe_use(&pair[1], target, &yes, scope) { return true; }
                remaining.extend(facts(&pair[0], false, scope));
            }
            unsafe_use(args.last().unwrap(), target, &remaining, scope)
        }
        Expr::Call(name, args) => {
            if scope.intrinsic(name) && matches!(name.as_str(), "kind" | "rank") { return false; }
            if reference(name, &target.1) { return true; } // An array element.
            args.iter().enumerate().any(|(i, e)| {
                let value = argument(e);
                let key = crate::named_arguments::keyword(e).map(|(key, _)| key);
                let safe = if scope.intrinsic(name) && matches!(name.as_str(), "allocated" | "present" | "kind" | "rank") {
                    true
                } else if scope.intrinsic(name) && name == "associated" {
                    key.map_or(i == 0, |key| key == "pointer")
                } else if scope.intrinsic(name) && name == "len" {
                    matches!(value, Expr::Ident(n) if scope.fixed_character.contains(n))
                } else if let Some(p) = scope.procedures.get(name) {
                    key.or_else(|| p.signature.params.get(i).map(String::as_str)).is_some_and(|key| p.descriptor_args.contains(key))
                } else {
                    // No interface: do not assume a whole descriptor actual is
                    // read. Its element/section/value expressions still are.
                    scope.generics.contains(name) || !scope.names.contains(name) && !name.starts_with('@') && crate::intrinsics::purity(name).is_none()
                };
                if safe && matches!(value, Expr::Ident(_)) { false }
                else { unsafe_use(value, target, proven, scope) }
            })
        }
        _ => expressions(e).iter().any(|e| unsafe_use(e, target, proven, scope)),
    }
}

struct Analysis<'a> {
    modules: HashMap<String, &'a [Stmt]>,
    tokens: &'a [Token],
    offsets: Vec<usize>,
    warnings: Vec<(usize, &'static str, String)>,
    dead: HashSet<usize>,
}
impl Analysis<'_> {
    fn bind(&self, body: &[Stmt], inherited: &Scope, params: &[String], visiting: &mut Names) -> (Scope, Names) {
        let mut scope = inherited.clone();
        for s in body {
            if let Stmt::Use { module, only, renames, .. } = s {
                if visiting.insert(module.clone()) {
                    if let Some(body) = self.modules.get(module) {
                        let (exports, _) = self.bind(body, &Scope::default(), &[], visiting);
                        let default_public = !body.iter().any(|s| matches!(s, Stmt::Accessibility { public: false, names } if names.is_empty()));
                        for remote in &exports.names {
                            let public = body.iter().rev().find_map(|s| match s {
                                Stmt::Accessibility { public, names } if names.contains(remote) => Some(*public), _ => None,
                            }).unwrap_or(default_public);
                            if !public { continue; }
                            if only.as_ref().is_some_and(|names| !names.contains(remote)) { continue; }
                            let aliases: Vec<_> = renames.iter().filter(|(_, n)| n == remote).map(|(n, _)| n.as_str()).collect();
                            if aliases.is_empty() { scope.rename_import(remote, remote, &exports); }
                            else { for local in aliases { scope.rename_import(local, remote, &exports); } }
                        }
                    }
                    visiting.remove(module);
                }
            }
        }
        let local = scope.declarations(body, params);
        (scope, local)
    }
    fn expression(&mut self, e: &Expr, scope: &Scope, proven: &Facts, offset: usize) {
        if let Expr::Call(name, args) = e {
            if scope.intrinsic(name) && matches!(name.as_str(), "kind" | "rank") { return; }
            if name == "@conditional" && !args.is_empty() {
                let mut remaining = proven.clone();
                for pair in args[..args.len()-1].chunks_exact(2) {
                    self.expression(&pair[0], scope, &remaining, offset);
                    let mut yes = remaining.clone(); yes.extend(facts(&pair[0], true, scope));
                    self.expression(&pair[1], scope, &yes, offset);
                    remaining.extend(facts(&pair[0], false, scope));
                }
                self.expression(args.last().unwrap(), scope, &remaining, offset);
                return;
            }
        }
        if matches!(e, Expr::Bin(BinOp::And | BinOp::Or, _, _)) {
            let mut candidates = Facts::new();
            inquiries(e, scope, &mut candidates);
            for target in candidates {
                if unsafe_use(e, &target, proven, scope) {
                    self.warnings.push((offset, target.0, target.1));
                }
            }
        }
        for e in expressions(e) { self.expression(e, scope, proven, offset); }
    }
    fn scope(&mut self, body: &[Stmt], inherited: &Scope, params: &[String], mut proven: Facts) -> Option<Facts> {
        let (scope, local) = self.bind(body, inherited, params, &mut Names::new());
        proven.retain(|(_, n)| !local.iter().any(|local| reference(n, local)));
        let result = self.body(body, &scope, proven);
        result.map(|mut facts| { facts.retain(|(_, n)| !local.iter().any(|local| reference(n, local))); facts })
    }
    fn body(&mut self, body: &[Stmt], scope: &Scope, mut proven: Facts) -> Option<Facts> {
        // Definitions are checked even after a host STOP/RETURN or in an
        // uncalled procedure; execution flow must not hide their diagnostics.
        for s in body {
            match s {
                Stmt::Module { body, .. } => { self.scope(body, &Scope::default(), &[], Facts::new()); }
                Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => { self.scope(body, scope, params, Facts::new()); }
                _ => {}
            }
        }
        let mut offset = 0;
        for statement in body {
            match statement {
                Stmt::SourceLocation(p) => { offset = *p; continue; }
                Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. } => continue,
                _ => {}
            }
            if self.dead.contains(&(statement as *const Stmt as usize)) { continue; }
            if let Stmt::DoWhile { body, .. } = statement {
                if changes(body, scope) { proven.clear(); }
            }
            let mut copy = statement.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                if may_change(e, scope) { proven.clear(); }
                self.expression(e, scope, &proven, offset);
            }
            match statement {
                Stmt::If { cond, then_body, else_body } => {
                    let mut yes = proven.clone(); yes.extend(facts(cond, true, scope));
                    let mut no = proven.clone(); no.extend(facts(cond, false, scope));
                    let yes = self.body(then_body, scope, yes);
                    let no = else_body.as_ref().map_or(Some(no.clone()), |b| self.body(b, scope, no));
                    proven = match (yes, no) {
                        (Some(a), Some(b)) => a.intersection(&b).cloned().collect(),
                        (Some(a), None) | (None, Some(a)) => a,
                        (None, None) => return None,
                    };
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                    // A call on one iteration can invalidate a proof on the next.
                    let modified = changes(body, scope);
                    let mut inside = if modified { Facts::new() } else { proven.clone() };
                    if let Stmt::DoWhile { cond, .. } = statement { inside.extend(facts(cond, true, scope)); }
                    let after = self.body(body, scope, inside);
                    if modified { proven.clear(); }
                    else if let Some(after) = after { proven = proven.intersection(&after).cloned().collect(); }
                }
                Stmt::Block { body } => {
                    let after = self.scope(body, scope, &[], proven.clone())?;
                    // A block-local shadow must not remove an outer object's proof.
                    let (block_scope, locals) = self.bind(body, scope, &[], &mut Names::new());
                    let mut retained: Facts = if changes(body, &block_scope) { Facts::new() }
                        else { proven.iter().filter(|(_, n)| locals.iter().any(|local| reference(n, local))).cloned().collect() };
                    retained.extend(after);
                    proven = retained;
                }
                Stmt::SelectCase { cases, default, .. } => {
                    let mut after = vec![];
                    for case in cases { if let Some(f) = self.body(&case.body, scope, proven.clone()) { after.push(f); } }
                    if let Some(body) = default {
                        if let Some(f) = self.body(body, scope, proven.clone()) { after.push(f); }
                    } else { after.push(proven.clone()); }
                    if after.is_empty() { return None; }
                    proven = after.into_iter().reduce(|a, b| a.intersection(&b).cloned().collect()).unwrap();
                }
                Stmt::CallSub { name, .. } if name == "@stop" || name == "@error_stop" => return None,
                Stmt::CallSub { name, args } => {
                    if changes_storage(name, scope) { proven.clear(); }
                    // A successful allocation without STAT either returns an
                    // allocated/associated object or terminates on failure.
                    if name == "@allocate" && !args.iter().any(|e| crate::named_arguments::keyword(e).is_some_and(|(k, _)| k == "stat")) {
                        for e in args {
                            if crate::named_arguments::keyword(e).is_some() { continue; }
                            let n = match e { Expr::Ident(n) | Expr::Call(n, _) | Expr::Section { name: n, .. } => Some(n), _ => None };
                            if let Some(n) = n { for query in ["allocated", "associated"] { proven.insert((query, n.clone())); } }
                        }
                    }
                }
                Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                    proven.retain(|(_, n)| n == name || !reference(n, name));
                    // A completed intrinsic assignment leaves an allocatable
                    // allocated, or requires a pointer target to be associated.
                    if let Some(query) = scope.storage.get(name) { proven.insert((query, name.clone())); }
                }
                Stmt::Return(_) | Stmt::Exit | Stmt::Cycle => return None,
                _ => {}
            }
        }
        Some(proven)
    }
    fn span(&self, offset: usize, name: &str) -> Range<usize> {
        let root = name.split('%').next().unwrap_or(name);
        let end = self.offsets.iter().copied().find(|&p| p > offset).unwrap_or(usize::MAX);
        let mut fallback = None;
        for (i, token) in self.tokens.iter().enumerate().filter(|(_, t)| t.span.start >= offset && t.span.start < end) {
            if !matches!(&token.kind, TokenKind::Ident(n) if n == root) { continue; }
            fallback.get_or_insert_with(|| token.span.clone());
            let mut depth = 0;
            let mut inquiry = false;
            for j in (0..i).rev() {
                match self.tokens[j].kind {
                    TokenKind::RParen => depth += 1,
                    TokenKind::LParen if depth > 0 => depth -= 1,
                    TokenKind::LParen => {
                        inquiry = j > 0 && matches!(&self.tokens[j-1].kind, TokenKind::Ident(n) if matches!(n.as_str(), "allocated" | "associated"));
                        break;
                    }
                    _ => {}
                }
            }
            if !inquiry { return token.span.clone(); }
        }
        fallback.unwrap_or(offset..offset)
    }
}

pub fn check(program: &Program, src: &str, own_len: usize, filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin], settings: &SemaSettings, options: &[String]) -> Result<()> {
    let (enabled, error) = crate::unused::selected("short-circuit", false, settings, options);
    if !enabled { return Ok(()); }
    fn collect<'a>(body: &'a [Stmt], a: &mut Analysis<'a>) {
        for s in body {
            match s {
                Stmt::SourceLocation(p) => a.offsets.push(*p),
                Stmt::Module { name, body } => { a.modules.insert(name.clone(), body); collect(body, a); }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body }
                | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => collect(body, a),
                Stmt::If { then_body, else_body, .. } => { collect(then_body, a); if let Some(b) = else_body { collect(b, a); } }
                Stmt::SelectCase { cases, default, .. } => { for c in cases { collect(&c.body, a); } if let Some(b) = default { collect(b, a); } }
                _ => {}
            }
        }
    }
    let tokens = crate::lexer::lex(src);
    let mut a = Analysis { modules: HashMap::new(), tokens: &tokens, offsets: vec![], warnings: vec![],
        dead: crate::reachability::analyze(program).dead };
    collect(&program.body, &mut a);
    a.offsets.sort_unstable(); a.offsets.dedup();
    a.scope(&program.body, &Scope::default(), &[], Facts::new());
    a.warnings.sort(); a.warnings.dedup();
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut failed = false;
    for (offset, query, name) in &a.warnings {
        if *offset >= own_len { continue; }
        let span = a.span(*offset, name);
        let (file, source, span) = if let Some(origin) = origins.iter().find(|o| o.range.contains(&span.start)) {
            (origin.name.as_str(), &src[origin.range.clone()],
                span.start-origin.range.start..span.end.min(origin.range.end)-origin.range.start)
        } else { (filename, src, span) };
        let state = if *query == "allocated" { "allocation" } else { "association" };
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("access to '{}' in the same logical expression as {} may require valid {} [short-circuit]", name, query.to_ascii_uppercase(), state))
            .with_labels(vec![Label::primary((), span)])
            .with_notes(vec!["Fortran .AND. and .OR. do not guarantee short-circuit evaluation; use a nested IF or a conditional expression to guard the access".into()]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(),
            &SimpleFile::new(file, source), &diagnostic)?;
        failed |= error;
    }
    if failed { bail!("short-circuit warnings treated as errors"); }
    Ok(())
}

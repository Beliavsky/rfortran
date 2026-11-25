// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wunneeded-target`, in -Wextra: a TARGET variable that no pointer can be
//! associated with. TARGET tells the compiler that a pointer may alias the
//! variable, which can prevent optimization, and tells a reader to look for
//! that pointer.
//!
//! Candidates are the TARGET local variables of the main program and of
//! procedures, other than dummy arguments and function results, and the
//! PRIVATE TARGET variables of modules. A use that may associate a pointer
//! with the variable, or a part of it, prevents the warning: the target of
//! `=>` (pointer assignment, a pointer initializer, or an ASSOCIATE
//! selector), ASSOCIATED with two arguments, C_LOC, C_FUNLOC and LOC, COMMON
//! and EQUIVALENCE, and the variable or a part of it as an actual argument
//! of a procedure whose dummy may be POINTER or TARGET (any procedure whose
//! interface is not in the file) or of a structure constructor. Uses in
//! internal procedures count as uses of a host variable. The token checks
//! cover the whole file, so a use of the same name elsewhere also prevents
//! the warning.
use crate::allocatable_warnings::{arrays, signatures, Context, Signatures, Uses};
use crate::ast::{Program, Stmt};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "unneeded-target";

#[derive(Clone, Copy, PartialEq)]
enum Scope<'a> {
    Main,
    Procedure(&'a str),
    Module,
}

struct Report {
    name: String,
    declaration: usize,
}

/// The TARGET variables a scope's own statements declare, with the offsets
/// of their declarations.
fn candidates(body: &[Stmt], scope: Scope, params: &[String]) -> Vec<(String, usize)> {
    let mut targets = vec![];
    let mut declared: HashMap<String, usize> = HashMap::new();
    let mut access: HashMap<String, bool> = HashMap::new();
    let mut default_public = true;
    let mut offset = 0;
    let target = |a: &String| a.eq_ignore_ascii_case("target");
    for s in body {
        let mut declare = |n: &String, offset: usize| { declared.entry(n.clone()).or_insert(offset); };
        match s {
            Stmt::SourceLocation(n) => offset = *n,
            Stmt::DummyIntent { name, intent } if intent == "target" => targets.push(name.clone()),
            Stmt::InitializedDecl { attributes, entities, .. } => {
                for (n, _) in entities {
                    declare(n, offset);
                    if attributes.iter().any(target) { targets.push(n.clone()); }
                }
            }
            Stmt::VarDecl { names, .. } => names.iter().for_each(|n| declare(n, offset)),
            Stmt::ArrayDecl { name, .. } => declare(name, offset),
            Stmt::DerivedDecl { names, attributes, .. } => {
                for n in names {
                    declare(n, offset);
                    if attributes.iter().any(target) { targets.push(n.clone()); }
                }
            }
            Stmt::Accessibility { public, names } if names.is_empty() => default_public = *public,
            Stmt::Accessibility { public, names } => names.iter().for_each(|n| { access.insert(n.clone(), *public); }),
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    targets.into_iter().filter(|p| seen.insert(p.clone())).filter_map(|p| {
        let &at = declared.get(&p)?;
        if params.contains(&p) { return None; }
        if let Scope::Procedure(name) = scope { if p == name { return None; } }
        if scope == Scope::Module && *access.get(&p).unwrap_or(&default_public) { return None; }
        Some((p, at))
    }).collect()
}

/// The names that tokens of the file may associate with a pointer: the
/// name after `=>`, names inside C_LOC, C_FUNLOC and LOC, and names in
/// EQUIVALENCE and COMMON statements.
fn token_uses(src: &str, tokens: &[Token]) -> HashSet<String> {
    let mut out = HashSet::new();
    let ident = |t: &Token| match &t.kind { TokenKind::Ident(n) => Some(n.to_ascii_lowercase()), _ => None };
    for (i, t) in tokens.iter().enumerate() {
        if matches!(t.kind, TokenKind::Arrow) {
            if let Some(n) = tokens.get(i + 1).and_then(ident) { out.insert(n); }
        }
        if matches!(t.kind, TokenKind::KwEquivalence | TokenKind::KwCommon) {
            for t in crate::declaration_warnings::statement_tokens(tokens, t.span.start, usize::MAX, src) {
                if let Some(n) = ident(t) { out.insert(n); }
            }
            continue;
        }
        let Some(name) = ident(t) else { continue };
        if matches!(name.as_str(), "c_loc" | "c_funloc" | "loc") && matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
            let mut depth = 0;
            for t in &tokens[i + 1..] {
                match t.kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => { depth -= 1; if depth == 0 { break; } }
                    _ => {}
                }
                if let Some(n) = ident(t) { out.insert(n); }
            }
        }
    }
    out
}

fn unit(body: &[Stmt], scope: Scope, params: &[String], ctx: &Context, used: &HashSet<String>, out: &mut Vec<Report>) {
    for (p, declaration) in candidates(body, scope, params) {
        if used.contains(&p.to_ascii_lowercase()) { continue; }
        // A function result is returned through `Return(Some(result))`.
        let returned = crate::leak_warnings::mentions(&Stmt::Block { body: body.iter().filter(|s| matches!(s, Stmt::Return(Some(_)))).cloned().collect() }, &p);
        if returned { continue; }
        let mut uses = Uses::default();
        ctx.scan(body, &p, &mut uses);
        if uses.pointer { continue; }
        out.push(Report { name: p, declaration });
    }
    for s in body {
        if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
            unit(body, Scope::Procedure(name), params, ctx, used, out);
        }
    }
}

fn analyze(program: &Program, src: &str, tokens: &[Token]) -> Vec<Report> {
    let mut signatures_ = Signatures::new();
    let mut generics = HashMap::new();
    signatures(&program.body, &mut signatures_, &mut generics);
    let mut array_names = HashSet::new();
    arrays(&program.body, &mut array_names);
    let ctx = Context { signatures: &signatures_, generics: &generics, arrays: &array_names, parts: true };
    let used = token_uses(src, tokens);
    let mut out = vec![];
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    unit(&main, Scope::Main, &[], &ctx, &used, &mut out);
    for s in &program.body {
        match s {
            Stmt::Module { body, .. } => unit(body, Scope::Module, &[], &ctx, &used, &mut out),
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } if external(s) => {
                unit(body, Scope::Procedure(name), params, &ctx, &used, &mut out);
            }
            _ => {}
        }
    }
    out.sort_by_key(|r| r.declaration);
    out
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // In -Wextra, not -Wall.
    let mut extra_only = settings.clone();
    extra_only.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &extra_only, options);
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let reports = analyze(program, src, &tokens);
    let warnings = reports.into_iter().filter(|r| r.declaration < own_len).map(|r| {
        let p = &r.name;
        let ts = crate::declaration_warnings::statement_tokens(&tokens, r.declaration, usize::MAX, src);
        let pos = ts.iter().find(|t| matches!(&t.kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case(p)))
            .or(ts.first()).map_or(r.declaration, |t| t.span.start);
        (pos, format!("`{p}` has the TARGET attribute, but no pointer is associated with it [{CATEGORY}]"),
            vec![format!("remove TARGET: without it, the compiler may assume that no pointer aliases `{p}`")])
    }).collect();
    if crate::open_warnings::emit(src, own_len, filename, origins, &tokens, error, warnings)? {
        bail!("{CATEGORY} warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn names(src: &str) -> Vec<String> {
        let tokens = crate::lexer::lex(src);
        let program = crate::parser::parse_with_src(src, &tokens, "t.f90").unwrap_or_else(|e| panic!("{src}: {e:?}"));
        super::analyze(&program, src, &tokens).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn reported() {
        for (src, expected) in [
            ("implicit none\ninteger, target :: i\ni = 3\nprint *, i\nend\n", vec!["i"]),
            // A TARGET statement; values passed to intrinsics and to a
            // procedure whose dummy is neither POINTER nor TARGET.
            ("program t\nreal :: a(3)\ntarget :: a\na = 1\nprint *, sum(a)\ncall show(a(2))\ncontains\nsubroutine show(x)\nreal, intent(in) :: x\nprint *, x\nend subroutine\nend program\n", vec!["a"]),
            // A local of a procedure, and a private module variable.
            ("subroutine s\nreal, target :: x\nx = 1\nprint *, x\nend subroutine\n", vec!["x"]),
            ("module m\nprivate\nreal, target, save :: g(3)\npublic :: init\ncontains\nsubroutine init\ng = 0\nend subroutine\nend module\n", vec!["g"]),
        ] {
            assert_eq!(names(src), expected, "{src}");
        }
    }

    #[test]
    fn may_be_pointed_to() {
        for src in [
            // Pointer assignment, to the whole variable, an element and a component.
            "real, target :: a(3)\nreal, pointer :: p(:)\np => a\nend\n",
            "real, target :: a(3)\nreal, pointer :: p\np => a(2)\nend\n",
            "type t\nreal :: v\nend type\ntype(t), target :: x\nreal, pointer :: p\np => x%v\nend\n",
            // A pointer initializer, ASSOCIATE, two-argument ASSOCIATED.
            "integer, target, save :: i\ncall s\ncontains\nsubroutine s\ninteger, pointer :: p => i\nprint *, p\nend subroutine\nend\n",
            "real, target :: a(3)\nreal, pointer :: p(:)\nassociate (b => a)\np => b\nend associate\nend\n",
            "real, target :: a(3)\nreal, pointer :: p(:)\nprint *, associated(p, a)\nend\n",
            // C_LOC, EQUIVALENCE, COMMON.
            "use iso_c_binding\nreal, target :: a(3)\ntype(c_ptr) :: c\nc = c_loc(a)\nend\n",
            "real, target :: a(3)\nreal :: b(3)\nequivalence (a, b)\nend\n",
            "real, target :: a(3)\ncommon /c/ a\nend\n",
            // An unknown procedure, a TARGET or POINTER dummy, a constructor.
            "real, target :: a(3)\ncall t(a)\nend\n",
            "real, target :: a(3)\ncall t(a(2))\ncontains\nsubroutine t(x)\nreal, target :: x\nprint *, x\nend subroutine\nend\n",
            "type n\ninteger, pointer :: p\nend type\ninteger, target :: i\ntype(n) :: x\nx = n(i)\nend\n",
            // In an internal procedure.
            "real, target :: a(3)\ncall s\ncontains\nsubroutine s\nreal, pointer :: p(:)\np => a\nend subroutine\nend\n",
            // A dummy argument, a function result, a public module variable.
            "subroutine s(x)\nreal, target :: x\nprint *, x\nend subroutine\n",
            "function f() result(r)\nreal, target :: r\nr = 1\nend function\n",
            "module m\nreal, target :: g(3)\nend module\n",
        ] {
            assert!(names(src).is_empty(), "{src}: {:?}", names(src));
        }
    }
}

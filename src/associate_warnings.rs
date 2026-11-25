// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wcould-be-associate`, in -Wextra: a POINTER that is associated once,
//! with a variable or a part of one, and then only used for that variable,
//! could be the name of an ASSOCIATE construct. An ASSOCIATE name cannot be
//! associated again or left dangling, and its selector does not need TARGET.
//!
//! Candidates are the POINTER local variables of the main program and of
//! procedures, other than dummy arguments, function results and saved or
//! initialized pointers. The scope must associate the pointer in exactly one
//! statement, `p => t`, at its top level (not in a construct), where `t` is
//! a variable, an element, a section or a component; it must not use the
//! pointer before that statement, nor nullify, allocate, deallocate or test
//! it with ASSOCIATED, use it as a target, pass it to a procedure whose dummy
//! may be POINTER or TARGET, or name it in COMMON. Internal procedures must
//! not use it, and the scope must have no GO TO. ASSOCIATE needs Fortran
//! 2003, so `-std=f95` turns the warning off.
use crate::allocatable_warnings::{arrays, initialized, is_null, root, signatures, whole, Context, Signatures, Uses};
use crate::ast::{Expr, Program, Stmt};
use crate::lexer::{Token, TokenKind};
use crate::leak_warnings::{gotos, mentions};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "could-be-associate";

struct Report {
    pointer: String,
    /// The offset of the pointer assignment.
    at: usize,
}

/// The POINTER variables a scope's own statements declare, with the offsets
/// of their declarations.
fn pointers(body: &[Stmt], params: &[String], function: Option<&str>) -> Vec<(String, usize)> {
    let mut found = vec![];
    let mut declared: HashMap<String, usize> = HashMap::new();
    let mut saved = HashSet::new();
    let mut offset = 0;
    let is = |a: &String, w: &str| a.eq_ignore_ascii_case(w);
    for s in body {
        let mut declare = |n: &String, offset: usize| { declared.entry(n.clone()).or_insert(offset); };
        match s {
            Stmt::SourceLocation(n) => offset = *n,
            Stmt::DummyIntent { name, intent } if intent == "pointer" => found.push(name.clone()),
            Stmt::DummyIntent { name, intent } if intent == "save" || *intent == crate::data_statements::SAVED_STORAGE => { saved.insert(name.clone()); }
            Stmt::InitializedDecl { attributes, entities, .. } => {
                for (n, _) in entities {
                    declare(n, offset);
                    saved.insert(n.clone());
                    if attributes.iter().any(|a| is(a, "pointer")) { found.push(n.clone()); }
                }
            }
            Stmt::VarDecl { names, .. } => names.iter().for_each(|n| declare(n, offset)),
            Stmt::ArrayDecl { name, .. } => declare(name, offset),
            Stmt::DerivedDecl { names, attributes, .. } => {
                for n in names {
                    declare(n, offset);
                    if attributes.iter().any(|a| is(a, "pointer")) { found.push(n.clone()); }
                    if attributes.iter().any(|a| is(a, "save")) { saved.insert(n.clone()); }
                }
            }
            _ => {}
        }
    }
    if saved.contains(crate::data_statements::SAVE_ALL) { return vec![]; }
    let mut seen = HashSet::new();
    found.into_iter().filter(|p| seen.insert(p.clone())).filter_map(|p| {
        let &at = declared.get(&p)?;
        if params.contains(&p) || function == Some(p.as_str()) || saved.contains(&p) { return None; }
        Some((p, at))
    }).collect()
}

/// The number of statements in `body`, at any depth, that associate `p`
/// with `=>`.
fn associations(body: &[Stmt], p: &str) -> usize {
    let mut count = 0;
    for s in body {
        if let Stmt::CallSub { name, args } = s {
            if name == "@pointer_associate" && args.first().and_then(root) == Some(p) { count += 1; }
        }
        let mut copy = s.clone();
        for b in crate::procedure_arguments::children(&mut copy) {
            count += associations(b, p);
        }
    }
    count
}

/// Whether `e` designates a variable or a part of one, not a function value.
fn designator(e: &Expr, ctx: &Context) -> bool {
    match e {
        Expr::Ident(_) | Expr::Section { .. } => true,
        Expr::Call(n, _) | Expr::Index(n, _) => n.contains('%') || ctx.arrays.contains(n.as_str()) && !ctx.signatures.contains_key(n.as_str()),
        _ => false,
    }
}

fn unit(body: &[Stmt], params: &[String], function: Option<&str>, ctx: &Context, src: &str, tokens: &[Token], out: &mut Vec<Report>) {
    let executable = |s: &Stmt| !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. });
    let candidates = pointers(body, params, function);
    if !candidates.is_empty() && !gotos(body) {
        for (p, declaration) in candidates {
            if initialized(src, tokens, declaration, &p) || associations(body, &p) != 1 { continue; }
            // The association, at the top level.
            let Some(k) = body.iter().position(|s| matches!(s, Stmt::CallSub { name, args }
                if name == "@pointer_associate" && args.len() == 2 && whole(&args[0], &p) && !is_null(&args[1]) && designator(&args[1], ctx))) else { continue };
            let Stmt::CallSub { args, .. } = &body[k] else { continue };
            // A target that depends on the pointer itself.
            if crate::allocatable_warnings::names(&args[1], &p) { continue; }
            if body[..k].iter().filter(|s| executable(s)).any(|s| mentions(s, &p) && !matches!(s, Stmt::DummyIntent { .. })) { continue; }
            if body.iter().filter(|s| !executable(s)).any(|s| mentions(s, &p)) { continue; }
            let rest: Vec<Stmt> = body.iter().enumerate().filter(|(i, s)| *i != k && executable(s)).map(|(_, s)| s.clone()).collect();
            let mut uses = Uses::default();
            ctx.scan(&rest, &p, &mut uses);
            if uses.pointer || uses.allocated || uses.nullified || uses.associated { continue; }
            if !rest.iter().any(|s| mentions(s, &p)) { continue; }
            let at = body[..k].iter().rev().find_map(|s| match s { Stmt::SourceLocation(n) => Some(*n), _ => None }).unwrap_or(declaration);
            out.push(Report { pointer: p, at });
        }
    }
    for s in body {
        if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
            let function = matches!(s, Stmt::Function { .. }).then_some(name.as_str());
            unit(body, params, function, ctx, src, tokens, out);
        }
    }
}

fn analyze(program: &Program, src: &str, tokens: &[Token]) -> Vec<Report> {
    let mut signatures_ = Signatures::new();
    let mut generics = HashMap::new();
    signatures(&program.body, &mut signatures_, &mut generics);
    let mut array_names = HashSet::new();
    arrays(&program.body, &mut array_names);
    let ctx = Context { signatures: &signatures_, generics: &generics, arrays: &array_names, parts: false };
    let mut out = vec![];
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    unit(&main, &[], None, &ctx, src, tokens, &mut out);
    for s in &program.body {
        match s {
            Stmt::Module { body, .. } => {
                for s in body {
                    if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
                        let function = matches!(s, Stmt::Function { .. }).then_some(name.as_str());
                        unit(body, params, function, &ctx, src, tokens, &mut out);
                    }
                }
            }
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } if external(s) => {
                let function = matches!(s, Stmt::Function { .. }).then_some(name.as_str());
                unit(body, params, function, &ctx, src, tokens, &mut out);
            }
            _ => {}
        }
    }
    out.sort_by_key(|r| r.at);
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
    standard: Option<crate::standards::Standard>,
) -> Result<()> {
    if standard.is_some_and(|s| s < "f2003".parse().unwrap()) { return Ok(()); }
    // In -Wextra, not -Wall.
    let mut extra_only = settings.clone();
    extra_only.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &extra_only, options);
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let reports = analyze(program, src, &tokens);
    let warnings = reports.into_iter().filter(|r| r.at < own_len).map(|r| {
        let p = &r.pointer;
        let ts = crate::declaration_warnings::statement_tokens(&tokens, r.at, usize::MAX, src);
        let pos = ts.iter().find(|t| matches!(t.kind, TokenKind::Arrow)).or(ts.first()).map_or(r.at, |t| t.span.start);
        let text = match (ts.first(), ts.last()) {
            (Some(a), Some(b)) => src[a.span.start..b.span.end].to_string(),
            _ => format!("{p} => ..."),
        };
        (pos, format!("POINTER `{p}` is associated only here and could be an ASSOCIATE name [{CATEGORY}]"),
            vec![format!("replace `{text}` by `associate ({text})`, end the construct after the last use of `{p}`, and remove the POINTER declaration of `{p}`; the selector then does not need TARGET")])
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
        super::analyze(&program, src, &tokens).into_iter().map(|r| r.pointer).collect()
    }

    #[test]
    fn reported() {
        for (src, expected) in [
            ("implicit none\ninteger, target :: i\ninteger, pointer :: p\ni = 3\np => i\np = p*10\nprint *, i, p\nend\n", vec!["p"]),
            // An element, a section and a component, in a procedure.
            ("subroutine s(a)\nreal, intent(inout), target :: a(:)\nreal, pointer :: p, q(:)\np => a(1)\nq => a(2:)\np = sum(q)\nend subroutine\n", vec!["p", "q"]),
            ("type t\nreal :: v(3)\nend type\ntype(t), target :: x\nreal, pointer :: p(:)\np => x%v\np = 1\nprint *, x%v\nend\n", vec!["p"]),
            // Passed to a procedure whose dummy is neither POINTER nor TARGET.
            ("program m\nreal, target :: a(3)\nreal, pointer :: p(:)\np => a\ncall show(p)\ncontains\nsubroutine show(x)\nreal, intent(in) :: x(:)\nprint *, x\nend subroutine\nend program\n", vec!["p"]),
            // A pointer used as a target is not reported; the other is,
            // since `associate (q => p)` names the same target.
            ("real, target :: a\nreal, pointer :: p, q\np => a\nq => p\nq = 1\nend\n", vec!["q"]),
        ] {
            assert_eq!(names(src), expected, "{src}");
        }
    }

    #[test]
    fn needs_a_pointer() {
        for src in [
            // Associated twice, in a loop, or conditionally.
            "real, target :: a, b\nreal, pointer :: p\np => a\np = 1\np => b\np = 2\nend\n",
            "real, target :: a(3)\nreal, pointer :: p\ninteger :: i\ndo i = 1, 3\np => a(i)\np = i\nend do\nend\n",
            "real, target :: a, b\nreal, pointer :: p\nlogical :: c\nc = .true.\nif (c) then\np => a\nelse\np => b\nend if\np = 1\nend\n",
            // NULLIFY, ALLOCATE, ASSOCIATED, NULL().
            "real, target :: a\nreal, pointer :: p\np => a\np = 1\nnullify (p)\nend\n",
            "real, target :: a\nreal, pointer :: p\np => a\np = 1\nallocate (p)\nend\n",
            "real, target :: a\nreal, pointer :: p\np => a\nprint *, associated(p)\nend\n",
            "real, pointer :: p\np => null()\nend\n",
            // A function result as target, used before.
            "real, pointer :: p(:)\np => f()\nprint *, p\ncontains\nfunction f()\nreal, pointer :: f(:)\nallocate (f(2))\nf = 1\nend function\nend\n",
            "real, target :: a\nreal, pointer :: p\nprint *, associated(p)\np => a\np = 1\nend\n",
            // An unknown procedure, an internal procedure, a dummy.
            "real, target :: a\nreal, pointer :: p\np => a\ncall t(p)\nend\n",
            "real, target :: a\nreal, pointer :: p\np => a\ncall s\ncontains\nsubroutine s\np = 1\nend subroutine\nend\n",
            "subroutine s(p, a)\nreal, pointer :: p\nreal, target :: a\np => a\np = 1\nend subroutine\n",
            // A saved or initialized pointer, a module variable, GO TO.
            "subroutine s(a)\nreal, target :: a\nreal, pointer, save :: p\np => a\np = 1\nend subroutine\n",
            "real, target :: a\nreal, pointer :: p => null()\np => a\np = 1\nend\n",
            "module m\nreal, pointer :: p\nreal, target :: a\ncontains\nsubroutine s\np => a\np = 1\nend subroutine\nend module\n",
            "real, target :: a\nreal, pointer :: p\np => a\n10 p = p + 1\nif (p < 3) go to 10\nend\n",
        ] {
            assert!(names(src).is_empty(), "{src}: {:?}", names(src));
        }
    }
}

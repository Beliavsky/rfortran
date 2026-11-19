// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wcould-be-allocatable`, in -Wextra: a POINTER variable that is only
//! allocated, deallocated, nullified, tested with one-argument ASSOCIATED
//! and used for its values could be ALLOCATABLE, which frees its storage
//! automatically and cannot alias other variables.
//!
//! Candidates are the local variables of the main program and of
//! procedures, other than dummy arguments and function results, and the
//! PRIVATE variables of modules, that some statement ALLOCATEs. A use that
//! needs a pointer disqualifies the variable: `=>` with a target other than
//! NULL() or with bounds, the variable or a part of it as the target of
//! `=>`, ASSOCIATED with two arguments, the whole variable as an actual
//! argument of a procedure whose dummy is not known to be neither POINTER
//! nor TARGET, COMMON, and an ALLOCATE that some path reaches while the
//! variable holds storage the procedure allocated (an error for an
//! ALLOCATABLE). Procedures with GO TO are not analyzed, so their variables
//! are not reported. Uses in internal procedures count as uses of a host
//! variable.
use crate::ast::{Expr, Program, Stmt};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "could-be-allocatable";

/// For each procedure name, its dummy arguments and those that are POINTER
/// or TARGET; None when several procedures have the name.
type Signatures = HashMap<String, Option<(Vec<String>, HashSet<String>)>>;

fn signatures(body: &[Stmt], out: &mut Signatures, generics: &mut HashMap<String, Vec<String>>) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let mut pointers = HashSet::new();
                for s in body {
                    match s {
                        Stmt::DummyIntent { name, intent } if intent == "pointer" || intent == "target" => { pointers.insert(name.clone()); }
                        Stmt::DerivedDecl { names, attributes, .. }
                            if attributes.iter().any(|a| a.eq_ignore_ascii_case("pointer") || a.eq_ignore_ascii_case("target")) => {
                            pointers.extend(names.iter().cloned());
                        }
                        _ => {}
                    }
                }
                out.entry(name.clone()).and_modify(|e| *e = None).or_insert(Some((params.clone(), pointers)));
                signatures(body, out, generics);
            }
            Stmt::GenericInterface { name, procedures, .. } => generics.entry(name.clone()).or_default().extend(procedures.iter().cloned()),
            Stmt::Module { body, .. } => signatures(body, out, generics),
            _ => {}
        }
    }
}

fn root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Some(n.split('%').next().unwrap_or(n)),
        _ => None,
    }
}

fn names(e: &Expr, p: &str) -> bool {
    let mut found = false;
    crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |e| found |= root(e) == Some(p));
    found
}

fn whole(e: &Expr, p: &str) -> bool {
    matches!(e, Expr::Ident(n) if n == p)
}

fn is_null(e: &Expr) -> bool {
    matches!(e, Expr::Call(n, args) if n.eq_ignore_ascii_case("null") && args.is_empty())
}

struct Context<'a> {
    signatures: &'a Signatures,
    generics: &'a HashMap<String, Vec<String>>,
    arrays: &'a HashSet<String>,
}

/// How a candidate is used.
#[derive(Default)]
struct Uses {
    allocated: bool,
    associated: bool,
    nullified: bool,
    /// A use that needs a POINTER.
    pointer: bool,
}

impl Context<'_> {
    /// Whether the dummy argument of `procedure` that the actual argument at
    /// `position` (or with `keyword`) associates with is known to be
    /// neither POINTER nor TARGET.
    fn plain_dummy(&self, procedure: &str, position: usize, keyword: Option<&str>) -> bool {
        let specific = |name: &str| self.signatures.get(name).and_then(|s| s.as_ref()).is_some_and(|(params, pointers)| {
            let dummy = keyword.map(str::to_owned).or_else(|| params.get(position).cloned());
            dummy.is_some_and(|d| params.contains(&d) && !pointers.contains(&d))
        });
        match self.generics.get(procedure) {
            Some(specifics) => !self.signatures.contains_key(procedure) && !specifics.is_empty() && specifics.iter().all(|s| specific(s)),
            None => specific(procedure),
        }
    }

    /// The actual arguments of a reference to `procedure`.
    fn actuals(&self, procedure: &str, args: &[Expr], p: &str, uses: &mut Uses) {
        for (i, a) in args.iter().enumerate() {
            let (keyword, value) = match crate::named_arguments::keyword(a) {
                Some((k, v)) => (Some(k), v),
                None => (None, a),
            };
            if !whole(value, p) { continue; }
            if procedure == "associated" {
                if args.len() == 1 { uses.associated = true } else { uses.pointer = true }
            } else if !(crate::leak_warnings::INTRINSICS.contains(&procedure) || self.arrays.contains(procedure) && !self.signatures.contains_key(procedure)
                || self.plain_dummy(procedure, i, keyword)) {
                uses.pointer = true;
            }
        }
    }

    fn scan(&self, body: &[Stmt], p: &str, uses: &mut Uses) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == "@pointer_associate" => {
                    if whole(&args[0], p) {
                        if args.len() == 2 && is_null(&args[1]) { uses.nullified = true } else { uses.pointer = true }
                    }
                    if args[1..].iter().any(|a| names(a, p)) { uses.pointer = true; }
                }
                Stmt::CallSub { name, args } if name == "@nullify" && args.iter().any(|a| whole(a, p)) => uses.nullified = true,
                Stmt::CallSub { name, args } if name == "@allocate" && args.iter().any(|a| root(a) == Some(p) && !matches!(a, Expr::Ident(n) | Expr::Call(n, _) if n.contains('%'))) => {
                    uses.allocated = true;
                }
                Stmt::CallSub { name, args } if name == crate::common::STATEMENT && args.iter().any(|a| names(a, p)) => uses.pointer = true,
                Stmt::CallSub { name, args } if !name.starts_with('@') => self.actuals(name, args, p, uses),
                _ => {}
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(e, &mut |e| {
                    if let Expr::Call(n, args) | Expr::Index(n, args) = e {
                        if !n.starts_with('@') { self.actuals(n, args, p, uses); }
                    }
                });
            }
            for b in crate::procedure_arguments::children(&mut copy) {
                self.scan(b, p, uses);
            }
        }
    }
}

/// The procedures of a body, nested ones included.
fn procedures(body: &[Stmt]) -> Vec<&[Stmt]> {
    let mut out = vec![];
    for s in body {
        if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } = s {
            out.push(body.as_slice());
            out.extend(procedures(body));
        }
    }
    out
}

/// The kind of scope a candidate belongs to.
#[derive(Clone, Copy, PartialEq)]
enum Scope<'a> {
    Main,
    Procedure(&'a str),
    Module,
}

struct Report {
    name: String,
    declaration: usize,
    scope: String,
    procedure: bool,
    saved: bool,
    contiguous: bool,
    uses: Uses,
}

/// The candidates declared by a scope's own statements, with the offsets of
/// their declarations.
fn candidates(body: &[Stmt], scope: Scope, params: &[String]) -> Vec<(String, usize, bool, bool)> {
    let mut pointers = vec![];
    let mut saved = HashSet::new();
    let mut contiguous = HashSet::new();
    let mut declared: HashMap<String, usize> = HashMap::new();
    let mut access: HashMap<String, bool> = HashMap::new();
    let mut default_public = true;
    let mut offset = 0;
    for s in body {
        let mut declare = |n: &String, offset: usize| { declared.entry(n.clone()).or_insert(offset); };
        match s {
            Stmt::SourceLocation(n) => offset = *n,
            Stmt::DummyIntent { name, intent } => match intent.as_str() {
                "pointer" => pointers.push(name.clone()),
                "save" => { saved.insert(name.clone()); }
                "contiguous" => { contiguous.insert(name.clone()); }
                _ => {}
            },
            Stmt::InitializedDecl { attributes, entities, .. } => {
                for (n, _) in entities {
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("save")) { saved.insert(n.clone()); }
                    declare(n, offset);
                }
            }
            Stmt::VarDecl { names, .. } => names.iter().for_each(|n| declare(n, offset)),
            Stmt::ArrayDecl { name, .. } => declare(name, offset),
            Stmt::DerivedDecl { names, attributes, .. } => {
                for n in names {
                    declare(n, offset);
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("pointer")) { pointers.push(n.clone()); }
                    if attributes.iter().any(|a| a.eq_ignore_ascii_case("save")) { saved.insert(n.clone()); }
                }
            }
            Stmt::Accessibility { public, names } if names.is_empty() => default_public = *public,
            Stmt::Accessibility { public, names } => names.iter().for_each(|n| { access.insert(n.clone(), *public); }),
            _ => {}
        }
    }
    let all_saved = saved.contains(crate::data_statements::SAVE_ALL);
    let mut seen = HashSet::new();
    pointers.into_iter().filter(|p| seen.insert(p.clone())).filter_map(|p| {
        let &at = declared.get(&p)?;
        if params.contains(&p) { return None; }
        if let Scope::Procedure(name) = scope { if p == name { return None; } }
        if scope == Scope::Module && *access.get(&p).unwrap_or(&default_public) { return None; }
        Some((p.clone(), at, all_saved || saved.contains(&p), contiguous.contains(&p)))
    }).collect()
}

/// Whether the declaration of `p` at `at` initializes it with `=>`.
fn initialized(src: &str, tokens: &[Token], at: usize, p: &str) -> bool {
    let ts = crate::declaration_warnings::statement_tokens(tokens, at, usize::MAX, src);
    let Some(i) = ts.iter().position(|t| matches!(&t.kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case(p))) else { return false };
    let mut depth = 0;
    for t in &ts[i + 1..] {
        match t.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => depth -= 1,
            TokenKind::Arrow if depth == 0 => return true,
            _ if depth == 0 => return false,
            _ => {}
        }
    }
    false
}

fn unit(body: &[Stmt], scope: Scope, params: &[String], ctx: &Context, out: &mut Vec<Report>) {
    for (p, declaration, saved, contiguous) in candidates(body, scope, params) {
        // A function result is returned through `Return(Some(result))`.
        let returned = crate::leak_warnings::mentions(&Stmt::Block { body: body.iter().filter(|s| matches!(s, Stmt::Return(Some(_)))).cloned().collect() }, &p);
        if returned { continue; }
        let mut uses = Uses::default();
        ctx.scan(body, &p, &mut uses);
        if uses.pointer || !uses.allocated { continue; }
        // The bodies that use the variable must be analyzable, and none may
        // allocate it twice.
        let own: Vec<&[Stmt]> = std::iter::once(body).chain(procedures(body)).collect();
        let users: Vec<&[Stmt]> = own.into_iter().filter(|b| b.iter().any(|s| crate::leak_warnings::mentions(s, &p))).collect();
        if users.iter().any(|b| crate::leak_warnings::gotos(b) || crate::leak_warnings::reallocates(b, &p)) { continue; }
        let names = match scope {
            Scope::Main => "the main program".to_string(),
            Scope::Procedure(name) => format!("`{name}`"),
            Scope::Module => "the module".to_string(),
        };
        out.push(Report { name: p, declaration, scope: names, procedure: matches!(scope, Scope::Procedure(_)), saved, contiguous, uses });
    }
    for s in body {
        if let Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } = s {
            unit(body, Scope::Procedure(name), params, ctx, out);
        }
    }
}

fn analyze(program: &Program) -> Vec<Report> {
    let mut signatures_ = Signatures::new();
    let mut generics = HashMap::new();
    signatures(&program.body, &mut signatures_, &mut generics);
    fn arrays(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::ArrayDecl { name, .. } => { out.insert(name.clone()); }
                Stmt::DerivedDecl { names, dims, .. } if !dims.is_empty() => out.extend(names.iter().cloned()),
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => arrays(body, out),
                _ => {}
            }
        }
    }
    let mut array_names = HashSet::new();
    arrays(&program.body, &mut array_names);
    let ctx = Context { signatures: &signatures_, generics: &generics, arrays: &array_names };
    let mut out = vec![];
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    unit(&main, Scope::Main, &[], &ctx, &mut out);
    for s in &program.body {
        match s {
            Stmt::Module { body, .. } => unit(body, Scope::Module, &[], &ctx, &mut out),
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } if external(s) => {
                unit(body, Scope::Procedure(name), params, &ctx, &mut out);
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
    let reports = analyze(program);
    if reports.is_empty() { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let warnings = reports.into_iter().filter(|r| r.declaration < own_len).map(|r| {
        let p = &r.name;
        let ts = crate::declaration_warnings::statement_tokens(&tokens, r.declaration, usize::MAX, src);
        let pos = ts.iter().find(|t| matches!(&t.kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case(p)))
            .or(ts.first()).map_or(r.declaration, |t| t.span.start);
        let mut notes = vec![];
        if r.uses.associated { notes.push(format!("replace ASSOCIATED({p}) by ALLOCATED({p})")); }
        if r.uses.nullified {
            notes.push(format!("replace NULLIFY({p}) and `{p} => NULL()` by `if (allocated({p})) deallocate ({p})`, or remove them where `{p}` is not allocated"));
        }
        if r.contiguous { notes.push("drop CONTIGUOUS: an ALLOCATABLE array is always contiguous".into()); }
        let saved = r.saved || initialized(src, &tokens, r.declaration, p);
        if r.procedure && saved {
            notes.push(format!("`{p}` is saved, by SAVE or its `=> NULL()` initialization: declare it ALLOCATABLE, SAVE to keep its storage between calls of {}", r.scope));
        } else if r.procedure {
            notes.push(format!("a local ALLOCATABLE is deallocated automatically when {} returns", r.scope));
        }
        (pos, format!("POINTER `{p}` could be ALLOCATABLE: it is only allocated, deallocated and used for its values [{CATEGORY}]"), notes)
    }).collect();
    if crate::open_warnings::emit(src, own_len, filename, origins, &tokens, error, warnings)? {
        bail!("{CATEGORY} warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn names(src: &str) -> Vec<String> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|e| panic!("{src}: {e:?}"));
        super::analyze(&program).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn reported() {
        for (src, expected) in [
            // A local array, with NULLIFY, ASSOCIATED, values, intrinsics.
            ("subroutine s(n)\ninteger, intent(in) :: n\nreal, pointer :: p(:)\nnullify (p)\nallocate (p(n))\np = 1\nif (associated(p)) print *, sum(p), size(p)\ndeallocate (p)\nend subroutine\n", vec!["p"]),
            // A scalar of the main program, passed to a known procedure.
            ("program t\ninteger, pointer :: k\nallocate (k)\nk = 3\ncall show(k)\ncontains\nsubroutine show(i)\ninteger, intent(in) :: i\nprint *, i\nend subroutine\nend program\n", vec!["k"]),
            // A private module variable allocated in a module procedure.
            ("module m\nimplicit none\nprivate\nreal, pointer :: g(:) => null()\npublic :: init\ncontains\nsubroutine init(n)\ninteger, intent(in) :: n\nif (.not. associated(g)) allocate (g(n))\ng = 0\nend subroutine\nend module\n", vec!["g"]),
            // Freed before it is allocated again in a loop.
            ("subroutine s\nreal, pointer :: p(:)\ninteger :: i\ndo i = 1, 3\nallocate (p(i))\np = i\nprint *, p\ndeallocate (p)\nend do\nend subroutine\n", vec!["p"]),
        ] {
            assert_eq!(names(src), expected, "{src}");
        }
    }

    #[test]
    fn needs_a_pointer() {
        for src in [
            // Associated with a target, or the target of another pointer.
            "subroutine s\nreal, pointer :: p(:)\nreal, target :: a(3)\nallocate (p(3))\np => a\nend subroutine\n",
            "subroutine s\nreal, pointer :: p(:), q(:)\nallocate (p(3))\nq => p\nprint *, q\nend subroutine\n",
            // ASSOCIATED with two arguments, bounds remapping.
            "subroutine s\nreal, pointer :: p(:), q(:)\nallocate (p(3), q(3))\nprint *, associated(p, q)\nend subroutine\n",
            // Passed to an unknown procedure or a POINTER dummy.
            "subroutine s\nreal, pointer :: p(:)\nallocate (p(3))\ncall t(p)\nend subroutine\n",
            "subroutine s\nreal, pointer :: p(:)\nallocate (p(3))\ncall t(p)\ncontains\nsubroutine t(x)\nreal, pointer :: x(:)\nx => null()\nend subroutine\nend subroutine\n",
            // A dummy argument, a function result, a public module variable.
            "subroutine s(p)\nreal, pointer :: p(:)\nallocate (p(3))\nend subroutine\n",
            "function f() result(p)\nreal, pointer :: p(:)\nallocate (p(3))\nend function\n",
            "module m\nreal, pointer :: g(:)\ncontains\nsubroutine init\nallocate (g(3))\nend subroutine\nend module\n",
            // Allocated again while it holds storage, never allocated.
            "subroutine s\nreal, pointer :: p(:)\nallocate (p(3))\nallocate (p(4))\nend subroutine\n",
            "subroutine s\nreal, pointer :: p(:)\nprint *, associated(p)\nend subroutine\n",
        ] {
            assert!(names(src).is_empty(), "{src}: {:?}", names(src));
        }
    }
}

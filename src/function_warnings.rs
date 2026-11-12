// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wcould-be-function`, opt-in: a subroutine whose only effect is to
//! define one INTENT(OUT) argument, which a function result can return.
//! Every other dummy argument is INTENT(IN), and the subroutine could be
//! PURE: the purity check rfort applies to PURE procedures accepts it, so
//! it performs no input/output, executes no STOP, saves nothing, defines no
//! variable outside itself and calls no impure procedure.
//!
//! Not reported: subroutines named in a generic or operator interface or a
//! type-bound binding, since such an interface or binding may require a
//! subroutine, subroutines passed as actual arguments, subroutines with
//! alternate returns or OPTIONAL, POINTER or procedure dummies, and
//! subroutines that reference procedures of intrinsic modules other than
//! the ones the purity check knows.
use crate::ast::{Expr, Program, Stmt};
use crate::lexer::{lex, Token};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::HashSet;

const CATEGORY: &str = "could-be-function";

#[derive(Debug, PartialEq)]
struct Report {
    offset: usize,
    name: String,
    output: String,
    inputs: Vec<String>,
    /// Whether the output is an array.
    array: bool,
}

/// Names that must stay subroutines: those in interfaces and bindings, and
/// those passed as actual arguments.
fn fixed_names(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::GenericInterface { procedures, .. } | Stmt::OperatorInterface { procedures, .. } => {
                out.extend(procedures.iter().cloned());
            }
            Stmt::DerivedType { bindings, .. } => {
                for (binding, target) in bindings {
                    out.insert(binding.clone());
                    out.insert(target.clone());
                }
            }
            _ => {}
        }
        let mut s = s.clone();
        for e in crate::procedure_arguments::expressions(&mut s) {
            crate::procedure_arguments::visit_expr(e, &mut |e| {
                if let Expr::Call(_, args) | Expr::Index(_, args) = e {
                    for a in args.iter() {
                        let a = match a {
                            Expr::Call(k, v) if k.starts_with("@keyword:") && v.len() == 1 => &v[0],
                            a => a,
                        };
                        if let Expr::Ident(n) = a {
                            out.insert(n.clone());
                        }
                    }
                }
            });
        }
        if let Stmt::CallSub { args, .. } = &s {
            for a in args {
                if let Expr::Ident(n) = a {
                    out.insert(n.clone());
                }
            }
        }
        for b in crate::procedure_arguments::children(&mut s) {
            fixed_names(b, out);
        }
    }
}

/// The intents and attributes a body gives a dummy.
fn attributes<'a>(body: &'a [Stmt], name: &str) -> Vec<&'a str> {
    body.iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name: n, intent } if n == name => Some(intent.as_str()),
            _ => None,
        })
        .collect()
}

/// The output and inputs of a subroutine that has one INTENT(OUT) dummy and
/// only INTENT(IN) dummies otherwise.
fn shape(params: &[String], body: &[Stmt]) -> Option<(String, Vec<String>)> {
    let mut output = None;
    let mut inputs = vec![];
    for p in params {
        if p == "*" {
            return None;
        }
        let attrs = attributes(body, p);
        if attrs.iter().any(|a| matches!(*a, "optional" | "pointer" | "external" | "value") || a.starts_with("procedure") || a.starts_with('@')) {
            return None;
        }
        match attrs.iter().find(|a| a.starts_with("intent(")) {
            Some(&"intent(in)") => inputs.push(p.clone()),
            Some(&"intent(out)") if output.is_none() => output = Some(p.clone()),
            _ => return None,
        }
    }
    // A dummy procedure is declared only by an interface or EXTERNAL; each
    // dummy here has a data declaration.
    let declared = |p: &String| body.iter().any(|s| match s {
        Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.contains(p),
        Stmt::ArrayDecl { name, .. } => name == p,
        _ => false,
    });
    if !params.iter().all(declared) {
        return None;
    }
    output.map(|o| (o, inputs))
}

/// The candidate subroutines, as paths of body indices, with their reports.
fn candidates(body: &[Stmt], path: &mut Vec<usize>, fixed: &HashSet<String>, out: &mut Vec<(Vec<usize>, Report)>) {
    let mut offset = 0;
    for (i, s) in body.iter().enumerate() {
        match s {
            Stmt::SourceLocation(at) => offset = *at,
            Stmt::Subroutine { name, params, body: b, .. } => {
                if !fixed.contains(name) && !b.iter().any(|s| matches!(s, Stmt::CallSub { name, .. } if name == "@entry")) {
                    if let Some((output, inputs)) = shape(params, b) {
                        let offset = b.iter().find_map(|s| match s { Stmt::SourceLocation(at) => Some(*at), _ => None }).unwrap_or(offset);
                        let mut p = path.clone();
                        p.push(i);
                        let array = b.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, .. } if *name == output));
                        out.push((p, Report { offset, name: name.clone(), output, inputs, array }));
                    }
                }
                path.push(i);
                candidates(b, path, fixed, out);
                path.pop();
            }
            Stmt::Function { body: b, .. } | Stmt::Module { body: b, .. } => {
                path.push(i);
                candidates(b, path, fixed, out);
                path.pop();
            }
            _ => {}
        }
    }
}

/// Mark the subroutine at `path` PURE.
fn mark_pure(body: &mut [Stmt], path: &[usize]) {
    let Some((&first, rest)) = path.split_first() else { return };
    match &mut body[first] {
        Stmt::Subroutine { pure, .. } if rest.is_empty() => *pure = true,
        Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } | Stmt::Module { body, .. } => mark_pure(body, rest),
        _ => {}
    }
}

/// The statement at `path`.
fn at<'a>(body: &'a [Stmt], path: &[usize]) -> Option<&'a Stmt> {
    let (&first, rest) = path.split_first()?;
    let s = body.get(first)?;
    if rest.is_empty() {
        return Some(s);
    }
    match s {
        Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } | Stmt::Module { body, .. } => at(body, rest),
        _ => None,
    }
}

fn reports(program: &Program) -> Vec<Report> {
    let mut fixed = HashSet::new();
    fixed_names(&program.body, &mut fixed);
    let mut found = vec![];
    candidates(&program.body, &mut vec![], &fixed, &mut found);
    found
        .into_iter()
        .filter(|(path, _)| {
            // Only the subroutine, marked PURE, is checked.
            let mut copy = program.clone();
            mark_pure(&mut copy.body, path);
            at(&copy.body, path).is_some_and(|s| crate::purity::validate_procedure(&copy, s).is_ok())
        })
        .map(|(_, r)| r)
        .collect()
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
    let mut quiet = settings.clone();
    quiet.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &quiet, options);
    if !enabled {
        return Ok(());
    }
    let mut found = reports(program);
    found.retain(|r| r.offset < own_len);
    if found.is_empty() {
        return Ok(());
    }
    let tokens: Vec<Token> = lex(src);
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config},
    };
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for r in &found {
        let (file, at) = locate(crate::leak_warnings::span(src, &tokens, r.offset, Some(&r.name)));
        let inputs = r.inputs.join(", ");
        let mut notes = vec![format!("suggestion: `pure function {}({inputs}) result({})`, with `call {}(...)` becoming `{} = {}({inputs})`",
            r.name, r.output, r.name, r.output, r.name)];
        if r.array {
            notes.push(format!("the result `{}` needs an explicit shape computed from the inputs, or the ALLOCATABLE attribute", r.output));
        }
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!(
                "subroutine `{}` could be a function: its only effect is to define INTENT(OUT) `{}` [{CATEGORY}]",
                r.name, r.output
            ))
            .with_labels(vec![Label::primary(file, at).with_message("could be a function")])
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error {
        bail!("{CATEGORY} warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn found(src: &str) -> Vec<String> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        super::reports(&program).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn subroutines_that_could_be_functions() {
        let src = "module m\nimplicit none\ninteger :: count = 0\ncontains\n\
            subroutine area(w, h, a)\nreal, intent(in) :: w, h\nreal, intent(out) :: a\na = w * h\nend subroutine area\n\
            subroutine noisy(x, y)\nreal, intent(in) :: x\nreal, intent(out) :: y\ny = x\nprint *, y\nend subroutine noisy\n\
            subroutine counts(x, y)\nreal, intent(in) :: x\nreal, intent(out) :: y\ny = x\ncount = count + 1\nend subroutine counts\n\
            subroutine two(x, y, z)\nreal, intent(in) :: x\nreal, intent(out) :: y, z\ny = x\nz = x\nend subroutine two\n\
            subroutine both(x, y)\nreal, intent(inout) :: x\nreal, intent(out) :: y\ny = x\nend subroutine both\n\
            subroutine nointent(x, y)\nreal :: x\nreal, intent(out) :: y\ny = x\nend subroutine nointent\n\
            subroutine scale(v, s, w)\nreal, intent(in) :: v(:), s\nreal, intent(out) :: w(size(v))\nw = s * v\nend subroutine scale\n\
            end module m\n";
        assert_eq!(found(src), ["area", "scale"]);
    }

    #[test]
    fn interfaces_and_actual_arguments_keep_subroutines() {
        let src = "module m\nimplicit none\ninterface assignment(=)\nmodule procedure assign\nend interface\n\
            type t\ninteger :: v\nend type\ncontains\n\
            subroutine assign(a, b)\ntype(t), intent(out) :: a\ninteger, intent(in) :: b\na%v = b\nend subroutine assign\n\
            subroutine twice(x, y)\nreal, intent(in) :: x\nreal, intent(out) :: y\ny = 2 * x\nend subroutine twice\n\
            subroutine apply(f)\nexternal f\nreal :: r\ncall f(1.0, r)\nend subroutine apply\n\
            subroutine run()\ncall apply(twice)\nend subroutine run\nend module m\n";
        assert!(found(src).is_empty(), "{:?}", found(src));
    }
}

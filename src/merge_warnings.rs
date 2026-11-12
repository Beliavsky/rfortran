// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Warnings about MERGE, whose TSOURCE and FSOURCE arguments the standard
//! allows to be evaluated whatever MASK is.
//!
//! `-Wmerge-guard`, in -Wall: an argument that may be invalid when MASK does
//! not select it, because MASK tests what the argument depends on, as in
//! `merge(a(i), 0, i > 0)`, `merge(x, 0.0, present(x))` and
//! `merge(1/n, 0, n /= 0)`. MASK tests a name with PRESENT, ASSOCIATED or
//! ALLOCATED, or compares it; an argument that references a name tested
//! with PRESENT, ASSOCIATED or ALLOCATED, or that uses a compared name in a
//! subscript or a divisor, is reported.
//!
//! `-Wmerge-evaluation`, opt-in: an argument that computes something, which
//! is computed even when MASK does not select it. Constants, variables,
//! components, and elements and sections with constant or variable
//! subscripts are not reported; operations and function references are.
use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};
use crate::lexer::{lex, Token};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::HashSet;

const GUARD: &str = "merge-guard";
const EVALUATION: &str = "merge-evaluation";

#[derive(Debug, PartialEq)]
struct Report {
    category: &'static str,
    offset: usize,
    /// TSOURCE or FSOURCE.
    argument: &'static str,
    /// For merge-guard, the name the mask tests and how.
    tested: Option<(String, &'static str)>,
}

/// What a scope knows about names: its arrays, the procedures of the file,
/// and whether a module it uses is not in the file, so that an undeclared
/// name may be one of that module's arrays.
#[derive(Clone, Default)]
struct Scope {
    arrays: HashSet<String>,
    unknown_module: bool,
}

struct Context<'a> {
    procedures: &'a HashSet<String>,
    modules: &'a [(String, HashSet<String>)],
    reports: Vec<Report>,
}

/// The arrays a specification part declares.
fn declared_arrays(body: &[Stmt]) -> HashSet<String> {
    body.iter()
        .flat_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            Stmt::DerivedDecl { names, dims, .. } if !dims.is_empty() => names.clone(),
            _ => vec![],
        })
        .collect()
}

/// The base name of a designator: `a` for `a%b%c`.
fn base(name: &str) -> &str {
    name.split('%').next().unwrap_or(name)
}

/// The names an expression references, with the bases of components.
fn names(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n) => out.push(base(n).to_string()),
        Expr::Call(n, args) | Expr::Index(n, args) => {
            if !n.starts_with('@') {
                out.push(base(n).to_string());
            }
            for a in args {
                names(a, out);
            }
        }
        Expr::Bin(_, a, b) => {
            names(a, out);
            names(b, out);
        }
        Expr::Un(_, a) => names(a, out),
        Expr::Section { name, lower, upper, stride } => {
            out.push(base(name).to_string());
            for x in [lower, upper, stride].into_iter().flatten() {
                names(x, out);
            }
        }
        _ => {}
    }
}

fn relational(op: &BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

/// The names that a mask tests with PRESENT, ASSOCIATED or ALLOCATED, and
/// the names it compares. A name in a subscript of the mask is not
/// compared: the mask itself evaluates that element.
fn tests(context: &Context, scope: &Scope, mask: &Expr, inquired: &mut Vec<(String, &'static str)>, compared: &mut Vec<String>) {
    match mask {
        Expr::Call(n, args) if matches!(n.as_str(), "present" | "associated" | "allocated") => {
            let how = match n.as_str() {
                "present" => "PRESENT",
                "associated" => "ASSOCIATED",
                _ => "ALLOCATED",
            };
            if let Some(a) = args.first() {
                let a = match a {
                    Expr::Call(k, v) if k.starts_with("@keyword:") && v.len() == 1 => &v[0],
                    a => a,
                };
                if let Expr::Ident(x) | Expr::Call(x, _) | Expr::Index(x, _) | Expr::Section { name: x, .. } = a {
                    inquired.push((x.clone(), how));
                }
            }
        }
        Expr::Bin(op, a, b) if relational(op) => {
            operands(context, scope, a, compared);
            operands(context, scope, b, compared);
        }
        Expr::Bin(_, a, b) => {
            tests(context, scope, a, inquired, compared);
            tests(context, scope, b, inquired, compared);
        }
        Expr::Un(_, a) => tests(context, scope, a, inquired, compared),
        _ => {}
    }
}

/// The names of a compared operand, outside subscripts of arrays.
fn operands(context: &Context, scope: &Scope, e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n) => out.push(base(n).to_string()),
        Expr::Call(n, args) | Expr::Index(n, args) => {
            if context.array(n, scope) == Some(true) {
                out.push(base(n).to_string());
            } else {
                for a in args {
                    operands(context, scope, a, out);
                }
            }
        }
        Expr::Bin(_, a, b) => {
            operands(context, scope, a, out);
            operands(context, scope, b, out);
        }
        Expr::Un(_, a) => operands(context, scope, a, out),
        Expr::Section { name, .. } => out.push(base(name).to_string()),
        _ => {}
    }
}

impl Context<'_> {
    fn array(&self, name: &str, scope: &Scope) -> Option<bool> {
        if name.contains('%') || scope.arrays.contains(name) {
            Some(true)
        } else if self.procedures.contains(name) || crate::intrinsics::purity(name).is_some() || !scope.unknown_module {
            Some(false)
        } else {
            None
        }
    }

    /// Whether `e` computes something: anything but a constant, a variable
    /// or component, or an element or section with constant or variable
    /// subscripts. A reference that may be an element of an array of a
    /// module outside the file is not reported.
    fn computes(&self, e: &Expr, scope: &Scope) -> bool {
        let simple = |x: &Expr| match x {
            Expr::IntLit(_) | Expr::Ident(_) => true,
            Expr::Un(UnOp::Neg, a) => matches!(**a, Expr::IntLit(_)),
            _ => false,
        };
        match e {
            Expr::IntLit(_) | Expr::RealLit(_) | Expr::Str(_) | Expr::Logical(_) | Expr::Ident(_) => false,
            Expr::Un(UnOp::Neg, a) => !matches!(**a, Expr::IntLit(_) | Expr::RealLit(_)),
            Expr::Call(n, args) | Expr::Index(n, args) if !n.starts_with('@') => match self.array(n, scope) {
                Some(true) => !args.iter().all(|a| match a {
                    Expr::Section { lower, upper, stride, .. } => {
                        [lower, upper, stride].into_iter().flatten().all(|b| simple(b))
                    }
                    a => simple(a),
                }),
                Some(false) => true,
                None => !args.iter().all(simple),
            },
            Expr::Section { lower, upper, stride, .. } => !([lower, upper, stride].into_iter().flatten().all(|b| simple(b))),
            // A constructor of values that compute nothing, and NULL().
            Expr::Call(n, args) if n == "@array_constructor" => args.iter().any(|a| self.computes(a, scope)),
            Expr::Call(n, args) if n == "@typed_constructor" => args.iter().skip(1).any(|a| self.computes(a, scope)),
            Expr::Call(n, args) if n == "null" && args.is_empty() => false,
            _ => true,
        }
    }

    /// The name in `e` that `mask` tests, if `e` depends on it in a way that
    /// is invalid when the test fails: a name tested with PRESENT,
    /// ASSOCIATED or ALLOCATED, or a compared name in a subscript of an
    /// array or a divisor.
    fn guarded(&self, e: &Expr, mask: &Expr, scope: &Scope) -> Option<(String, &'static str)> {
        let mut inquired = vec![];
        let mut compared = vec![];
        tests(self, scope, mask, &mut inquired, &mut compared);
        let mut referenced = vec![];
        names(e, &mut referenced);
        for (x, how) in inquired {
            if referenced.iter().any(|r| *r == base(&x)) {
                return Some((x, how));
            }
        }
        let mut sensitive = vec![];
        self.sensitive(e, scope, &mut sensitive);
        compared.into_iter().find(|c| sensitive.contains(c)).map(|c| (c, "compares"))
    }

    /// The names used in subscripts of arrays and in divisors.
    fn sensitive(&self, e: &Expr, scope: &Scope, out: &mut Vec<String>) {
        match e {
            Expr::Call(n, args) | Expr::Index(n, args) => {
                let element = !n.starts_with('@') && self.array(n, scope) == Some(true);
                for a in args {
                    if element {
                        names(a, out);
                    }
                    self.sensitive(a, scope, out);
                }
            }
            Expr::Section { lower, upper, stride, .. } => {
                for x in [lower, upper, stride].into_iter().flatten() {
                    names(x, out);
                }
            }
            Expr::Bin(op, a, b) => {
                if matches!(op, BinOp::Div) {
                    names(b, out);
                }
                self.sensitive(a, scope, out);
                self.sensitive(b, scope, out);
            }
            Expr::Un(_, a) => self.sensitive(a, scope, out),
            _ => {}
        }
    }

    fn expression(&mut self, e: &Expr, scope: &Scope, offset: usize) {
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for a in args {
                    self.expression(a, scope, offset);
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, scope, offset);
                self.expression(b, scope, offset);
            }
            Expr::Un(_, a) => self.expression(a, scope, offset),
            Expr::Section { lower, upper, stride, .. } => {
                for x in [lower, upper, stride].into_iter().flatten() {
                    self.expression(x, scope, offset);
                }
            }
            _ => {}
        }
        let Expr::Call(n, args) = e else { return };
        if n != "merge" || args.len() != 3 {
            return;
        }
        // Positional arguments, then keywords.
        let mut slots: [Option<&Expr>; 3] = [None, None, None];
        for (i, a) in args.iter().enumerate() {
            match a {
                Expr::Call(k, v) if v.len() == 1 && k.starts_with("@keyword:") => {
                    let at = match &k["@keyword:".len()..] {
                        "tsource" => 0,
                        "fsource" => 1,
                        "mask" => 2,
                        _ => return,
                    };
                    slots[at] = Some(&v[0]);
                }
                a => slots[i] = Some(a),
            }
        }
        let [Some(t), Some(f), Some(mask)] = slots else { return };
        // A constant mask selects at compile time.
        if matches!(mask, Expr::Logical(_)) {
            return;
        }
        for (argument, a) in [("TSOURCE", t), ("FSOURCE", f)] {
            // Both, for when only -Wmerge-evaluation is selected.
            if let Some(tested) = self.guarded(a, mask, scope) {
                self.reports.push(Report { category: GUARD, offset, argument, tested: Some(tested) });
            }
            if self.computes(a, scope) {
                self.reports.push(Report { category: EVALUATION, offset, argument, tested: None });
            }
        }
    }

    fn body(&mut self, body: &[Stmt], host: &Scope) {
        let mut scope = host.clone();
        // A local scalar hides an array of the host.
        for s in body {
            if let Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } = s {
                for n in names {
                    scope.arrays.remove(n);
                }
            }
        }
        scope.arrays.extend(declared_arrays(body));
        for s in body {
            if let Stmt::Use { module, .. } = s {
                match self.modules.iter().find(|(m, _)| m == module) {
                    Some((_, arrays)) => scope.arrays.extend(arrays.iter().cloned()),
                    None => scope.unknown_module = true,
                }
            }
        }
        let mut offset = 0;
        for s in body {
            match s {
                Stmt::SourceLocation(at) => offset = *at,
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => self.body(body, &scope),
                Stmt::Module { body, .. } => self.body(body, &Scope::default()),
                // Initializations are constant expressions, evaluated when
                // compiling.
                Stmt::InitializedDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::VarDecl { .. } | Stmt::DerivedDecl { .. } => {}
                s => {
                    let mut s = s.clone();
                    for e in crate::procedure_arguments::expressions(&mut s) {
                        self.expression(e, &scope, offset);
                    }
                    for b in crate::procedure_arguments::children(&mut s) {
                        self.body(b, &scope);
                    }
                }
            }
        }
    }
}

fn procedures(body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::Function { name, body, .. } => {
                out.insert(name.clone());
                procedures(body, out);
            }
            Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => procedures(body, out),
            _ => {}
        }
    }
}

fn reports(program: &Program) -> Vec<Report> {
    let mut functions = HashSet::new();
    procedures(&program.body, &mut functions);
    let modules: Vec<(String, HashSet<String>)> = program
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Module { name, body } => Some((name.clone(), declared_arrays(body))),
            _ => None,
        })
        .collect();
    let mut context = Context { procedures: &functions, modules: &modules, reports: vec![] };
    context.body(&program.body, &Scope::default());
    context.reports
}

#[allow(clippy::too_many_arguments)]
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
    let (guard, guard_error) = crate::unused::selected(GUARD, false, settings, options);
    let mut quiet = settings.clone();
    quiet.wall = false;
    let (evaluation, evaluation_error) = crate::unused::selected(EVALUATION, false, &quiet, options);
    if !(guard || evaluation) {
        return Ok(());
    }
    let mut found = reports(program);
    found.retain(|r| r.offset < own_len && if r.category == GUARD { guard } else { evaluation });
    // A reported guard covers the evaluation of the same argument.
    let guarded: Vec<(usize, &str)> = found.iter().filter(|r| r.category == GUARD).map(|r| (r.offset, r.argument)).collect();
    found.retain(|r| r.category == GUARD || !guarded.contains(&(r.offset, r.argument)));
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
    let conditional = standard.is_none_or(|s| s >= "f2023".parse().unwrap());
    let alternative = if conditional {
        "use an IF construct, or a conditional expression such as `(mask ? tsource : fsource)`, which evaluates only the selected value"
    } else {
        "use an IF construct, which evaluates only the selected value"
    };
    let mut error = false;
    for r in &found {
        let (file, at) = locate(crate::leak_warnings::span(src, &tokens, r.offset, Some("merge")));
        let (message, label) = match &r.tested {
            Some((name, "compares")) => (
                format!("the {} argument of MERGE uses `{name}`, which the mask tests, in a subscript or divisor; the standard allows the argument to be evaluated even when the mask does not select it [{GUARD}]", r.argument),
                "may be evaluated when invalid",
            ),
            Some((name, how)) => (
                format!("the {} argument of MERGE references `{name}`, which the mask tests with {how}; the standard allows the argument to be evaluated even when the mask does not select it [{GUARD}]", r.argument),
                "may be evaluated when invalid",
            ),
            None => (
                format!("the {} argument of MERGE is computed even when the mask does not select it [{EVALUATION}]", r.argument),
                "computed whatever the mask is",
            ),
        };
        let is_error = if r.category == GUARD { guard_error } else { evaluation_error };
        error |= is_error;
        let d = if is_error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(vec![Label::primary(file, at).with_message(label)])
            .with_notes(vec![format!("{alternative}; -Wno-{} disables this warning", r.category)]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error {
        bail!("MERGE warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The reports, with an evaluation report of a guarded argument dropped,
    /// as when both categories are selected.
    fn found(src: &str) -> Vec<(&'static str, &'static str)> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        let all = super::reports(&program);
        let guarded: Vec<_> = all.iter().filter(|r| r.category == super::GUARD).map(|r| (r.offset, r.argument)).collect();
        all.iter().filter(|r| r.category == super::GUARD || !guarded.contains(&(r.offset, r.argument)))
            .map(|r| (r.category, r.argument)).collect()
    }

    #[test]
    fn guards() {
        let decl = "integer :: i, n, a(3)\nreal :: x\n";
        assert_eq!(found(&format!("{decl}print *, merge(a(i), 0, i > 0)\nend\n")), [("merge-guard", "TSOURCE")]);
        assert_eq!(found(&format!("{decl}print *, merge(0, 1/n, n == 0)\nend\n")), [("merge-guard", "FSOURCE")]);
        assert_eq!(found("subroutine s(x)\nreal, optional :: x\nprint *, merge(x, 0.0, present(x))\nend subroutine\n"), [("merge-guard", "TSOURCE")]);
        // The mask evaluates the element itself.
        assert!(found(&format!("{decl}print *, merge(a(i), 0, a(i) > 0)
end
")).is_empty());
        assert_eq!(found("subroutine s(a, i)\ninteger :: a(:), i\nprint *, merge(a(i), -1, i >= 1 .and. i <= size(a))\nend subroutine\n"), [("merge-guard", "TSOURCE")]);
    }

    #[test]
    fn evaluation() {
        let decl = "integer :: i, j, a(3)\nreal :: x\ntype t\ninteger :: v\nend type\ntype(t) :: q\n";
        for simple in ["merge(a(i), 0, x > 0)", "merge(x, -1.0, i > 0)", "merge(q%v, -1, .true.)", "merge(a(1:j), a, i > 0)",
                       "merge(tsource='yes', fsource='no', mask=i > 0)", "merge([1, 2], [3, 4], i > 0)", "merge(x + 1, x, .true.)"] {
            assert!(found(&format!("{decl}print *, {simple}\nend\n")).is_empty(), "{simple}");
        }
        assert_eq!(found(&format!("{decl}print *, merge(x+1, x, i > 1)\nend\n")), [("merge-evaluation", "TSOURCE")]);
        assert_eq!(found(&format!("{decl}print *, merge(fsource=sqrt(x), tsource=0.0, mask=x < 0)\nend\n")), [("merge-evaluation", "FSOURCE")]);
        assert_eq!(found(&format!("{decl}print *, merge(a(i+1), 0, x > 0)\nend\n")), [("merge-evaluation", "TSOURCE")]);
    }
}

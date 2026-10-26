// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Conservative presence checks on the source AST, before OPTIONAL specialization.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

type Names = HashSet<String>;
type Interfaces = HashMap<String, crate::named_arguments::Signature>;

fn optionals(body: &[Stmt]) -> Names {
    crate::named_arguments::Signature::new(&[], body).optional
}
fn declarations(body: &[Stmt]) -> Names {
    body.iter()
        .flat_map(|s| match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            Stmt::InitializedDecl { entities, .. } => {
                entities.iter().map(|(n, _)| n.clone()).collect()
            }
            _ => vec![],
        })
        .collect()
}
fn interfaces(body: &[Stmt]) -> Interfaces {
    body.iter()
        .filter_map(|s| match s {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => Some((
                name.clone(),
                crate::named_arguments::Signature::new(params, body),
            )),
            _ => None,
        })
        .collect()
}
fn facts(e: &Expr, truth: bool) -> Names {
    inquiry_facts(e, truth, "present")
}
pub(crate) fn inquiry_facts(e: &Expr, truth: bool, inquiry: &str) -> Names {
    match e {
        Expr::Call(n, args) if n == inquiry && truth => {
            let key = match inquiry { "allocated" => "array", "associated" => "pointer", _ => "a" };
            match args.iter().enumerate().find_map(|(i, arg)| match crate::named_arguments::keyword(arg) {
                Some((name, value)) if name == key => Some(value),
                None if i == 0 => Some(arg),
                _ => None,
            }) {
            Some(arg) => match arg {
                Expr::Ident(n) => HashSet::from([n.clone()]),
                _ => Names::new(),
            },
            _ => Names::new(),
        }},
        Expr::Un(UnOp::Not, e) => inquiry_facts(e, !truth, inquiry),
        Expr::Bin(BinOp::And, a, b) if truth => {
            inquiry_facts(a, true, inquiry).union(&inquiry_facts(b, true, inquiry)).cloned().collect()
        }
        Expr::Bin(BinOp::Or, a, b) if !truth => {
            inquiry_facts(a, false, inquiry).union(&inquiry_facts(b, false, inquiry)).cloned().collect()
        }
        _ => Names::new(),
    }
}
fn uses(e: &Expr, optional: &Names, present: &Names, known: &Interfaces, found: &mut Names) {
    let mut reference = |n: &str| {
        if optional.contains(n) && !present.contains(n) {
            found.insert(n.to_owned());
        }
    };
    match e {
        Expr::Ident(n) => reference(n),
        Expr::Index(n, args) => {
            reference(n);
            for e in args {
                uses(e, optional, present, known, found);
            }
        }
        Expr::Section {
            name,
            lower,
            upper,
            stride,
        } => {
            reference(name);
            for e in [lower, upper, stride].into_iter().flatten() {
                uses(e, optional, present, known, found);
            }
        }
        Expr::Call(n, args) if n == "present" => {
            // PRESENT itself is valid for an absent dummy.
            for e in args {
                let e = crate::named_arguments::keyword(e).map_or(e, |(_, value)| value);
                if !matches!(e, Expr::Ident(_)) {
                    uses(e, optional, present, known, found);
                }
            }
        }
        Expr::Call(n,args) if n=="@conditional" => {
            let mut remaining=present.clone();
            for pair in args[..args.len()-1].chunks_exact(2) {
                uses(&pair[0],optional,&remaining,known,found);
                let mut selected=remaining.clone();selected.extend(facts(&pair[0],true));
                uses(&pair[1],optional,&selected,known,found);
                remaining.extend(facts(&pair[0],false));
            }
            uses(args.last().unwrap(),optional,&remaining,known,found);
        }
        Expr::Call(n, args) => {
            reference(n); // Array elements are also parsed as calls.
            for (i, e) in args.iter().enumerate() {
                let (key, value) = crate::named_arguments::keyword(e)
                    .map_or((None, e), |(key, value)| (Some(key), value));
                // Forwarding a whole absent dummy to another OPTIONAL dummy
                // is valid; sections, elements and expressions still reference it.
                let intrinsic = crate::named_arguments::intrinsic(n);
                let forwarding = known
                    .get(n)
                    .or(intrinsic.as_ref())
                    .is_some_and(|signature| {
                        key.or_else(|| signature.params.get(i).map(String::as_str))
                            .is_some_and(|p| signature.optional.contains(p))
                    });
                if !forwarding || !matches!(value, Expr::Ident(_)) {
                    uses(value, optional, present, known, found);
                }
            }
        }
        Expr::Bin(_, a, b) => {
            // Fortran logical operators do not guarantee short-circuit evaluation.
            uses(a, optional, present, known, found);
            uses(b, optional, present, known, found);
        }
        Expr::Un(_, e) => uses(e, optional, present, known, found),
        _ => {}
    }
}

struct Analysis<'a> {
    tokens: &'a [Token],
    offsets: Vec<usize>,
    modules: HashMap<String, Interfaces>,
    warnings: Vec<(String, String, Range<usize>)>,
}
impl Analysis<'_> {
    fn record(&mut self, found: Names, procedure: &str, offset: usize) {
        let end = self
            .offsets
            .iter()
            .copied()
            .find(|&p| p > offset)
            .unwrap_or(usize::MAX);
        for name in found {
            let span = self
                .tokens
                .iter()
                .enumerate()
                .find_map(|(i, t)| {
                    if t.span.start < offset || t.span.start >= end {
                        return None;
                    }
                    if !matches!(&t.kind,TokenKind::Ident(n) if n==&name) {
                        return None;
                    }
                    // Prefer a value reference over PRESENT's argument on the same line.
                    if i >= 2
                        && matches!(self.tokens[i - 1].kind, TokenKind::LParen)
                        && matches!(&self.tokens[i-2].kind,TokenKind::Ident(n) if n=="present")
                    {
                        return None;
                    }
                    Some(t.span.clone())
                })
                .unwrap_or(offset..offset);
            self.warnings.push((name, procedure.to_owned(), span));
        }
    }
    fn scope(
        &mut self,
        body: &[Stmt],
        inherited: &Names,
        present: Names,
        parent: &Interfaces,
        procedure: &str,
    ) -> Option<Names> {
        let locals = declarations(body);
        let mut optional: Names = inherited.difference(&locals).cloned().collect();
        optional.extend(optionals(body));
        let mut known = parent.clone();
        for s in body {
            if let Stmt::Use {
                module,
                only,
                renames,
                ..
            } = s
            {
                if let Some(exports) = self.modules.get(module) {
                    for (name, signature) in exports {
                        let local = renames
                            .iter()
                            .find(|(_, remote)| remote == name)
                            .map(|(local, _)| local)
                            .unwrap_or(name);
                        if only
                            .as_ref()
                            .is_none_or(|names| names.contains(local) || names.contains(name))
                        {
                            known.insert(local.clone(), signature.clone());
                        }
                    }
                }
            }
        }
        known.extend(interfaces(body));
        self.body(body, &optional, present, &known, procedure)
    }
    fn body(
        &mut self,
        body: &[Stmt],
        optional: &Names,
        mut present: Names,
        known: &Interfaces,
        procedure: &str,
    ) -> Option<Names> {
        let mut offset = 0;
        let mut reachable = true;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = *p;
                continue;
            }
            match s {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let visible = optional
                        .iter()
                        .filter(|n| !params.contains(n))
                        .cloned()
                        .collect();
                    self.scope(body, &visible, Names::new(), known, name);
                    continue;
                }
                Stmt::Module { body, .. } => {
                    self.scope(
                        body,
                        &Names::new(),
                        Names::new(),
                        &Interfaces::new(),
                        procedure,
                    );
                    continue;
                }
                _ => {}
            }
            if !reachable {
                continue;
            }
            let mut found = Names::new();
            let mut expr = |e: &Expr| uses(e, optional, &present, known, &mut found);
            match s {
                Stmt::Assign { name, value } => {
                    expr(&Expr::Ident(name.clone()));
                    expr(value);
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    expr(&Expr::Ident(name.clone()));
                    for e in indices {
                        expr(e);
                    }
                    expr(value);
                }
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        expr(e);
                    }
                }
                Stmt::CallSub { name, args } => expr(&Expr::Call(name.clone(), args.clone())),
                Stmt::Return(Some(e)) => expr(e),
                Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => expr(cond),
                Stmt::Do {
                    var, start, end, ..
                } => {
                    expr(&Expr::Ident(var.clone()));
                    expr(start);
                    expr(end);
                }
                Stmt::SelectCase { expr: e, cases, .. } => {
                    expr(e);
                    for c in cases {
                        for item in &c.items {
                            match item {
                                crate::ast::CaseItem::Single(e) => expr(e),
                                crate::ast::CaseItem::Range(a, b) => {
                                    expr(a);
                                    expr(b);
                                }
                            }
                        }
                    }
                }
                Stmt::VarDecl { kind, .. }
                | Stmt::ArrayDecl { kind, .. }
                | Stmt::InitializedDecl { kind, .. } => {
                    if let TypeSpec::CharacterLength(e) = kind {
                        expr(e);
                    }
                    if let Stmt::ArrayDecl { dims, .. } = s {
                        for e in dims {
                            expr(e);
                        }
                    }
                    if let Stmt::InitializedDecl { entities, .. } = s {
                        for (_, e) in entities {
                            if let Some(e) = e {
                                expr(e);
                            }
                        }
                    }
                }
                _ => {}
            }
            self.record(found, procedure, offset);
            match s {
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let mut yes = present.clone();
                    yes.extend(facts(cond, true));
                    let mut no = present.clone();
                    no.extend(facts(cond, false));
                    let yes = self.body(then_body, optional, yes, known, procedure);
                    let no = else_body.as_ref().map_or(Some(no.clone()), |b| {
                        self.body(b, optional, no, known, procedure)
                    });
                    match (yes, no) {
                        (Some(a), Some(b)) => present = a.intersection(&b).cloned().collect(),
                        (Some(a), None) | (None, Some(a)) => present = a,
                        (None, None) => reachable = false,
                    }
                }
                Stmt::Do { body, .. } => {
                    self.body(body, optional, present.clone(), known, procedure);
                }
                Stmt::DoWhile { cond, body } => {
                    let mut yes = present.clone();
                    yes.extend(facts(cond, true));
                    self.body(body, optional, yes, known, procedure);
                }
                Stmt::Block { body } => {
                    match self.scope(body, optional, present.clone(), known, procedure) {
                        Some(facts) => present.extend(facts.intersection(optional).cloned()),
                        None => reachable = false,
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    // Keep incoming facts after SELECT, conservatively.
                    for c in cases {
                        self.body(&c.body, optional, present.clone(), known, procedure);
                    }
                    if let Some(b) = default {
                        self.body(b, optional, present.clone(), known, procedure);
                    }
                }
                Stmt::Return(_) | Stmt::Exit | Stmt::Cycle => reachable = false,
                Stmt::CallSub { name, .. } if name == "@stop" || name == "@error_stop" => {
                    reachable = false
                }
                _ => {}
            }
        }
        reachable.then_some(present)
    }
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    tokens: &[Token],
    filename: &str,
    origins: Option<&[crate::source_diagnostics::SourceOrigin]>,
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    let (enabled, error) = crate::unused::selected("optional-argument", false, settings, options);
    if !enabled {
        return Ok(());
    }
    fn offsets(body: &[Stmt], out: &mut Vec<usize>, modules: &mut HashMap<String, Interfaces>) {
        for s in body {
            match s {
                Stmt::SourceLocation(p) => out.push(*p),
                Stmt::Module { name, body } => {
                    modules.insert(name.clone(), interfaces(body));
                    offsets(body, out, modules);
                }
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => offsets(body, out, modules),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    offsets(then_body, out, modules);
                    if let Some(b) = else_body {
                        offsets(b, out, modules);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        offsets(&c.body, out, modules);
                    }
                    if let Some(b) = default {
                        offsets(b, out, modules);
                    }
                }
                _ => {}
            }
        }
    }
    let mut a = Analysis {
        tokens,
        offsets: vec![],
        modules: HashMap::new(),
        warnings: vec![],
    };
    offsets(&program.body, &mut a.offsets, &mut a.modules);
    a.offsets.sort_unstable();
    a.offsets.dedup();
    a.scope(
        &program.body,
        &Names::new(),
        Names::new(),
        &Interfaces::new(),
        &program.name,
    );
    a.warnings.sort_by_key(|(_, _, span)| span.start);
    a.warnings.dedup();
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFile,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    for (name, procedure, span) in &a.warnings {
        let (filename, source, span) = if let Some(origin) =
            origins.and_then(|o| o.iter().find(|o| o.range.contains(&span.start)))
        {
            (
                origin.name.as_str(),
                origin.source(src),
                origin.span(span.clone()),
            )
        } else {
            (filename, src, span.clone())
        };
        let diagnostic=if error {Diagnostic::error()} else {Diagnostic::warning()}
            .with_message(format!("OPTIONAL argument `{name}` of procedure `{procedure}` may be used when not present [optional-argument]"))
            .with_labels(vec![Label::primary((),span)]);
        let _ = emit_to_io_write(
            &mut stderr,
            &Config::default(),
            &SimpleFile::new(filename, source),
            &diagnostic,
        );
    }
    if error && !a.warnings.is_empty() {
        bail!("optional argument warnings treated as errors");
    }
    Ok(())
}

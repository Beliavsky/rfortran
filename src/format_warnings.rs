// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Best-effort checks of known formats, before I/O lowering loses item types.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use crate::format_spec as spec;
use crate::sema::{ImplicitTyping, SemaSettings};
use anyhow::{bail, Result};
use std::collections::HashMap;

type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
const LIMIT: usize = 10000;
struct Analysis {
    functions: Shapes,
    globals: Shapes,
    warnings: Vec<(usize, String)>,
}
#[derive(Clone)]
struct Item {
    tag: Option<u64>,
    number: usize,
    component: Option<usize>,
}
fn tag(kind: &TypeSpec) -> Option<u64> {
    Some(match kind {
        TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) => 1,
        TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision => 2,
        TypeSpec::Logical | TypeSpec::LogicalKind(_) => 3,
        TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => 4,
        _ => return None,
    })
}
fn declarations(body: &[Stmt], shapes: &mut Shapes) {
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            // A derived declaration hides any same-named host intrinsic object.
            Stmt::DerivedDecl { names, dims, .. } => {
                for n in names {
                    // Retain an unknown declared type instead of implicitly
                    // typing this object or exposing a same-named host object.
                    shapes.insert(
                        n.clone(),
                        (
                            TypeSpec::Referenced {
                                source: Box::new(Expr::Ident(n.clone())),
                                polymorphic: false,
                            },
                            dims.clone(),
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}
impl Analysis {
    fn functions(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                        _ => name,
                    };
                    let mut local = Shapes::new();
                    declarations(body, &mut local);
                    if let Some(shape) = local
                        .get(result)
                        .cloned()
                        .or_else(|| return_type.clone().map(|k| (k, vec![])))
                    {
                        self.functions.insert(name.clone(), shape);
                    }
                }
                Stmt::Module { body, .. } => {
                    let mut globals = Shapes::new();
                    declarations(body, &mut globals);
                    self.globals
                        .extend(globals.into_iter().filter(|(n, _)| n.starts_with('@')));
                    self.functions(body);
                }
                _ => {}
            }
        }
    }
    fn kind(&self, e: &Expr, shapes: &Shapes, implicit: &ImplicitTyping) -> Option<TypeSpec> {
        match e {
            Expr::Call(n, _) | Expr::Index(n, _) if shapes.contains_key(n) => {
                Some(shapes[n].0.clone())
            }
            Expr::Ident(n) => shapes.get(n).map(|(k, _)| k.clone()).or_else(|| {
                (!n.contains('%') && !n.starts_with('@'))
                    .then(|| implicit.get_implicit_type(n))
                    .flatten()
            }),
            Expr::Call(n, _) if self.functions.contains_key(n) => Some(self.functions[n].0.clone()),
            Expr::Call(n, _) if n == "@complex_literal" => Some(TypeSpec::Complex("4".into())),
            Expr::Bin(_, a, b)
                if matches!(self.kind(a, shapes, implicit), Some(TypeSpec::Complex(_)))
                    || matches!(self.kind(b, shapes, implicit), Some(TypeSpec::Complex(_))) =>
            {
                if matches!(
                    e,
                    Expr::Bin(crate::ast::BinOp::Eq | crate::ast::BinOp::Ne, _, _)
                ) {
                    Some(TypeSpec::Logical)
                } else {
                    Some(TypeSpec::Complex("4".into()))
                }
            }
            _ => crate::array_calls::scalar_actual_kind(e, shapes).ok(),
        }
    }
    fn append(
        &self,
        e: &Expr,
        number: usize,
        shapes: &Shapes,
        implicit: &ImplicitTyping,
        out: &mut Vec<Item>,
    ) -> Option<()> {
        if out.len() >= LIMIT {
            return None;
        }
        if let Expr::Call(n, args) = e {
            if n == "@array_constructor" {
                for e in args {
                    self.append(e, number, shapes, implicit, out)?;
                }
                return Some(());
            }
            if n == "@output_implied_do" || n == "@input_implied_do" {
                let [Expr::Ident(var), start, end, step, rest @ ..] = args.as_slice() else {
                    return None;
                };
                let (mut i, end, step) = (
                    crate::parameters::integer_constant(start).ok()?,
                    crate::parameters::integer_constant(end).ok()?,
                    crate::parameters::integer_constant(step).ok()?,
                );
                if step == 0 {
                    return None;
                }
                let mut iterations = 0;
                while if step > 0 { i <= end } else { i >= end } {
                    if iterations >= LIMIT {
                        return None;
                    }
                    iterations += 1;
                    for e in rest {
                        let mut e = e.clone();
                        crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
                            if matches!(e, Expr::Ident(n) if n == var) {
                                *e = Expr::IntLit(i.to_string());
                            }
                        });
                        self.append(&e, number, shapes, implicit, out)?;
                    }
                    i = i.checked_add(step)?;
                }
                return Some(());
            }
        }
        let dims: HashMap<_, _> = shapes
            .iter()
            .map(|(n, (_, d))| (n.clone(), d.clone()))
            .collect();
        let expanded = crate::input_items::expand(&[e.clone()], &dims, &|e| {
            crate::parameters::integer_constant(e).ok()
        })
        .ok()?;
        for value in expanded {
            if crate::input_items::index(&value).is_some() {
                continue;
            }
            if matches!(value, Expr::Bin(..) | Expr::Un(..)) {
                let mut array_expression = false;
                crate::procedure_arguments::visit_expr(&mut value.clone(), &mut |e| {
                    if let Expr::Ident(n) = e {
                        array_expression |= shapes.get(n).is_some_and(|(_, d)| !d.is_empty());
                    }
                });
                if array_expression {
                    return None;
                }
            }
            // Unknown function results might be arrays; do not guess their length.
            let kind = self.kind(&value, shapes, implicit);
            if kind.is_none() {
                return None;
            }
            if kind.as_ref().and_then(tag).is_none() && !matches!(kind, Some(TypeSpec::Complex(_)))
            {
                return None;
            }
            if let Expr::Call(n, _) = &value {
                if self.functions.get(n).is_some_and(|(_, d)| !d.is_empty()) {
                    return None;
                }
            }
            if matches!(kind, Some(TypeSpec::Complex(_))) {
                for component in 1..=2 {
                    out.push(Item {
                        tag: Some(2),
                        number,
                        component: Some(component),
                    });
                }
            } else {
                out.push(Item {
                    tag: kind.as_ref().and_then(tag),
                    number,
                    component: None,
                });
            }
            if out.len() >= LIMIT {
                return None;
            }
        }
        Some(())
    }
    fn statement(
        &mut self,
        name: &str,
        args: &[Expr],
        shapes: &Shapes,
        implicit: &ImplicitTyping,
        offset: usize,
    ) {
        let (format, first, input) = match name {
            "@formatted_print" => (0, 1, false),
            "@write" | "@formatted_write_noadvance" => (1, 2, false),
            "@formatted_read" => (1, 2, true),
            "@formatted_read_status" => (1, 4, true),
            _ => return,
        };
        let Some(format) = args
            .get(format)
            .and_then(|e| crate::parameters::character_constant(e).ok())
        else {
            return;
        };
        let mut items = vec![];
        for (number, e) in args.iter().skip(first).filter(|e|
            !matches!(e, Expr::Call(n, _) if n.starts_with("@write_option:") || n.starts_with("@io_spec:"))).enumerate() {
            if self.append(e, number+1, shapes, implicit, &mut items).is_none() { break; }
        }
        // Syntax errors are already diagnosed by the compiler's format validator.
        let Ok(mut parser) = spec::GroupParser::new(format.as_bytes(), items.len()) else {
            return;
        };
        let mut index = 0;
        while let Ok(Some(d)) = parser.next() {
            if !spec::is_data(d.code) {
                continue;
            }
            for _ in 0..d.repeat.min(items.len().saturating_sub(index)) {
                let item = &items[index];
                index += 1;
                let Some(tag) = item.tag else { continue };
                // BOZ editing also permits REAL/COMPLEX and LOGICAL data in
                // Fortran. The runtime currently renders INTEGER BOZ only;
                // that implementation limit is not a source type mismatch.
                // https://www.intel.com/content/www/us/en/docs/fortran-compiler/developer-guide-reference/2023-1/b-editing.html
                if spec::matches_type(d.code, tag)
                    || matches!(d.code, b'b' | b'o' | b'z') && matches!(tag, 1..=3)
                {
                    continue;
                }
                let descriptor = match d.code {
                    spec::ES => "ES".into(),
                    spec::EN => "EN".into(),
                    spec::AT => "AT".into(),
                    c => (c as char).to_ascii_uppercase().to_string(),
                };
                let descriptor = format!(
                    "{descriptor}{}{}",
                    d.width,
                    if d.has_precision {
                        format!(".{}", d.precision)
                    } else {
                        String::new()
                    }
                );
                let actual = ["unknown", "INTEGER", "REAL", "LOGICAL", "CHARACTER"][tag as usize];
                let expected = match d.code {
                    b'i' => "INTEGER",
                    b'b' | b'o' | b'z' => "numeric or LOGICAL data",
                    b'l' => "LOGICAL",
                    b'a' | spec::AT => "CHARACTER",
                    _ => "REAL",
                };
                let component = item
                    .component
                    .map_or(String::new(), |c| format!(", COMPLEX component {c}"));
                self.warnings.push((offset, format!("format descriptor {descriptor} expects {expected}, but {} item {}{component} is {actual} [format]", if input {"input"} else {"output"}, item.number)));
            }
        }
    }
    fn scope(
        &mut self,
        body: &mut [Stmt],
        parent: &Shapes,
        inherited: &ImplicitTyping,
        contained: Option<&[String]>,
        params: &[String],
    ) {
        let outer_functions = self.functions.clone();
        for n in params {
            self.functions.remove(n);
        }
        self.functions(body);
        let mut shapes = parent.clone();
        for n in params {
            shapes.remove(n);
        }
        declarations(body, &mut shapes);
        let mut implicit = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::Implicit(Implicit::None) => implicit.apply_implicit_none(),
                Stmt::Implicit(Implicit::Rule {
                    type_spec,
                    letter_ranges,
                }) => implicit.add_rule(type_spec.clone(), letter_ranges.clone(), 0..0),
                Stmt::Implicit(Implicit::Rules { rules }) => {
                    for r in rules {
                        implicit.add_rule(r.type_spec.clone(), r.letter_ranges.clone(), 0..0);
                    }
                }
                _ => {}
            }
        }
        let mut offset = 0;
        for s in body {
            match s {
                Stmt::SourceLocation(p) => offset = *p,
                Stmt::CallSub { name, args } => {
                    self.statement(name, args, &shapes, &implicit, offset)
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let host = contained.is_none_or(|c| c.contains(name));
                    if host {
                        self.scope(body, &shapes, &implicit, None, params);
                    } else {
                        self.scope(
                            body,
                            &self.globals.clone(),
                            &ImplicitTyping::new(),
                            None,
                            params,
                        );
                    }
                }
                Stmt::Module { body, .. } => self.scope(
                    body,
                    &self.globals.clone(),
                    &ImplicitTyping::new(),
                    None,
                    &[],
                ),
                _ => {
                    for child in crate::procedure_arguments::children(s) {
                        self.scope(child, &shapes, &implicit, None, &[]);
                    }
                }
            }
        }
        self.functions = outer_functions;
    }
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
    let (enabled, error) = crate::unused::selected("format", false, settings, options);
    if !enabled {
        return Ok(());
    }
    let mut normalized = program.clone();
    // Work on a copy: diagnostics must not alter the compilation pipeline.
    let _ = crate::statement_labels::formats(&mut normalized.body);
    if let Ok(associated) = crate::modules::associate(normalized.clone()) {
        normalized = crate::parameters::resolve_constants(associated.clone()).unwrap_or(associated);
    }
    let mut a = Analysis {
        functions: Shapes::new(),
        globals: Shapes::new(),
        warnings: vec![],
    };
    a.functions(&normalized.body);
    a.scope(
        &mut normalized.body,
        &a.globals.clone(),
        &ImplicitTyping::new(),
        Some(&normalized.contains),
        &[],
    );
    a.warnings.sort();
    a.warnings.dedup();
    let tokens = crate::lexer::lex(src);
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
    for (pos, message) in a.warnings {
        if pos >= own_len {
            continue;
        }
        let span = tokens
            .iter()
            .find(|t| t.span.start >= pos)
            .map(|t| t.span.clone())
            .unwrap_or(pos..pos);
        let (name, source, span) = if let Some(o) = origins.iter().find(|o| o.range.contains(&pos))
        {
            (
                o.name.as_str(),
                &src[o.range.clone()],
                span.start - o.range.start..span.end.min(o.range.end) - o.range.start,
            )
        } else {
            (filename, src, span)
        };
        let d = if error {
            codespan_reporting::diagnostic::Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(message)
        .with_labels(vec![Label::primary((), span)]);
        emit_to_io_write(
            &mut StandardStream::stderr(ColorChoice::Auto),
            &Config::default(),
            &SimpleFile::new(name, source),
            &d,
        )?;
        failed |= error;
    }
    if failed {
        bail!("format warnings treated as errors");
    }
    Ok(())
}

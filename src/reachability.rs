// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Conservative source reachability, shared by dead-code and unused diagnostics.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

type Constants = HashMap<String, Expr>;
pub(crate) struct Analysis {
    /// Addresses in the borrowed source AST; never use after mutating that AST.
    pub dead: HashSet<usize>,
    warnings: Vec<(usize, &'static str)>,
    modules: HashMap<String, Vec<Stmt>>,
}

fn specification(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::SourceLocation(_)
            | Stmt::VarDecl { .. }
            | Stmt::ArrayDecl { .. }
            | Stmt::InitializedDecl { .. }
            | Stmt::DerivedDecl { .. }
            | Stmt::DerivedType { .. }
            | Stmt::Use { .. }
            | Stmt::Implicit(_)
            | Stmt::DummyIntent { .. }
            | Stmt::Accessibility { .. }
            | Stmt::GenericInterface { .. }
            | Stmt::OperatorInterface { .. }
    )
}
impl Analysis {
    fn constants(
        &self,
        body: &[Stmt],
        inherited: &Constants,
        params: &[String],
        visiting: &mut HashSet<String>,
    ) -> Constants {
        let mut constants = inherited.clone();
        for n in params {
            constants.remove(n);
        }
        for s in body {
            match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                    for n in names {
                        constants.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. }
                | Stmt::Function { name, .. }
                | Stmt::Subroutine { name, .. } => {
                    constants.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        constants.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body {
            match s {
                Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => {
                    constants.insert(format!("@model_procedure:{name}"), Expr::Logical(true));
                }
                Stmt::DummyIntent { name, intent }
                    if intent == "external"
                        || intent.starts_with("procedure(")
                        || intent.starts_with("@interface:") =>
                {
                    constants.insert(format!("@model_procedure:{name}"), Expr::Logical(true));
                }
                _ => {}
            }
        }
        for s in body {
            if let Stmt::Use {
                module,
                only,
                renames,
                ..
            } = s
            {
                if !visiting.insert(module.clone()) {
                    continue;
                }
                if let Some(body) = self.modules.get(module) {
                    let exports = self.constants(body, &Constants::new(), &[], visiting);
                    let default_public = !body.iter().any(|s| {
                        matches!(s,
                        Stmt::Accessibility { public: false, names } if names.is_empty())
                    });
                    for (remote, value) in exports {
                        let procedure = remote.strip_prefix("@model_procedure:");
                        let remote = procedure.unwrap_or(&remote);
                        let public = body
                            .iter()
                            .rev()
                            .find_map(|s| match s {
                                Stmt::Accessibility { public, names }
                                    if names.iter().any(|n| n == remote) =>
                                {
                                    Some(*public)
                                }
                                _ => None,
                            })
                            .unwrap_or(default_public);
                        let local = renames
                            .iter()
                            .find(|(_, r)| r == remote)
                            .map_or(remote, |(local, _)| local.as_str());
                        if public
                            && only
                                .as_ref()
                                .is_none_or(|names| names.iter().any(|n| n == local || n == remote))
                        {
                            let local = if procedure.is_some() {
                                format!("@model_procedure:{local}")
                            } else {
                                local.to_owned()
                            };
                            constants.insert(local, value);
                        }
                    }
                }
                visiting.remove(module);
            }
        }
        for s in body {
            if let Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } = s
            {
                if !attributes.iter().any(|a| a == "parameter") {
                    continue;
                }
                for (name, value) in entities {
                    let Some(value) = value else { continue };
                    let value = (|| {
                        let value = self
                            .fold(value, &constants)
                            .ok_or_else(|| anyhow::anyhow!("not a constant expression"))?;
                        if matches!(kind, TypeSpec::CharacterAssumed) {
                            return crate::parameters::character_constant(&value).map(Expr::Str);
                        }
                        let kind = crate::parameters::concrete_kind_with(kind.clone(), &constants)?;
                        crate::parameters::initial_value(&kind, &value)
                    })();
                    if let Ok(value) = value {
                        constants.insert(name.clone(), value);
                    }
                }
            }
        }
        constants
    }
    fn fold(&self, expr: &Expr, constants: &Constants) -> Option<Expr> {
        let mut expr = expr.clone();
        let mut user_call = false;
        crate::procedure_arguments::visit_expr(&mut expr, &mut |e| {
            if let Expr::Call(name, _) = e {
                user_call |= constants.contains_key(&format!("@model_procedure:{name}"));
            }
        });
        if user_call {
            return None;
        }
        crate::parameters::constant_expression_with(&expr, constants).ok()
    }
    fn condition(&self, expr: &Expr, constants: &Constants) -> Option<bool> {
        let expr = self.fold(expr, constants)?;
        crate::parameters::logical_constant(&expr, constants).ok()
    }
    fn scope(
        &mut self,
        body: &[Stmt],
        host: &Constants,
        params: &[String],
        contained: Option<&[String]>,
    ) {
        let constants = self.constants(body, host, params, &mut HashSet::new());
        self.body(body, &constants, true, None);
        // Procedure definitions are independent of executable control flow in the host.
        for s in body {
            match s {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let mut host = if contained.is_none_or(|c| c.contains(name)) {
                        constants.clone()
                    } else {
                        Constants::new()
                    };
                    host.remove(name);
                    if let Some(Stmt::Return(Some(Expr::Ident(result)))) = body.last() {
                        host.remove(result);
                    }
                    self.scope(body, &host, params, None);
                }
                Stmt::Module { body, .. } => self.scope(body, &Constants::new(), &[], None),
                _ => {}
            }
        }
    }
    /// Returns whether execution may reach the end of this statement sequence.
    fn body(
        &mut self,
        body: &[Stmt],
        constants: &Constants,
        mut reachable: bool,
        reason: Option<&'static str>,
    ) -> bool {
        let mut offset = None;
        let mut located = false;
        let mut reported = false;
        let mut reason =
            reason.unwrap_or("unreachable statement after an unconditional transfer of control");
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(*p);
                located = true;
                continue;
            }
            if specification(s)
                || matches!(
                    s,
                    Stmt::Module { .. } | Stmt::Function { .. } | Stmt::Subroutine { .. }
                )
            {
                continue;
            }
            // A label may be reached by GOTO or an I/O branch elsewhere in the unit.
            if matches!(s, Stmt::CallSub { name, .. } if name == "@statement_label") {
                reachable = true;
                reported = false;
                continue;
            }
            if !reachable {
                self.dead.insert(s as *const Stmt as usize);
                if !reported && located {
                    if let Some(p) = offset {
                        self.warnings.push((p, reason));
                    }
                    reported = true;
                }
                located = false;
                continue;
            }
            reachable = match s {
                Stmt::Return(_) | Stmt::Exit | Stmt::Cycle => false,
                Stmt::CallSub { name, .. }
                    if matches!(
                        name.as_str(),
                        "@stop" | "@error_stop" | "@named_exit" | "@named_cycle" | "@goto"
                    ) =>
                {
                    false
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let known = self.condition(cond, constants);
                    let yes = self.body(
                        then_body,
                        constants,
                        known != Some(false),
                        Some("unreachable THEN branch: condition is always false"),
                    );
                    let no = else_body.as_ref().map_or(known != Some(true), |body| {
                        self.body(
                            body,
                            constants,
                            known != Some(true),
                            Some("unreachable ELSE branch: condition is always true"),
                        )
                    });
                    yes || no
                }
                Stmt::Block { body } => {
                    let constants = self.constants(body, constants, &[], &mut HashSet::new());
                    self.body(body, &constants, true, None)
                }
                Stmt::Do { body, .. } => {
                    self.body(body, constants, true, None);
                    true
                }
                Stmt::DoWhile { cond, body } => {
                    self.body(
                        body,
                        constants,
                        self.condition(cond, constants) != Some(false),
                        Some("unreachable loop body: condition is always false"),
                    );
                    true
                }
                Stmt::SelectCase { cases, default, .. } => {
                    let mut falls_through = default
                        .as_ref()
                        .is_none_or(|b| self.body(b, constants, true, None));
                    for case in cases {
                        falls_through |= self.body(&case.body, constants, true, None);
                    }
                    falls_through
                }
                _ => true,
            };
            if !reachable {
                reason = "unreachable statement after an unconditional transfer of control";
                reported = false;
            }
            located = false;
        }
        reachable
    }
}
pub(crate) fn analyze(program: &Program) -> Analysis {
    let mut a = Analysis {
        dead: HashSet::new(),
        warnings: vec![],
        modules: program
            .body
            .iter()
            .filter_map(|s| {
                if let Stmt::Module { name, body } = s {
                    Some((name.clone(), body.clone()))
                } else {
                    None
                }
            })
            .collect(),
    };
    a.scope(
        &program.body,
        &Constants::new(),
        &[],
        Some(&program.contains),
    );
    a
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
    let (enabled, error) = crate::unused::selected("unreachable-code", false, settings, options);
    if !enabled {
        return Ok(());
    }
    let mut analysis = analyze(program);
    analysis.warnings.sort_by_key(|(p, _)| *p);
    analysis.warnings.dedup();
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
    for (pos, message) in analysis.warnings {
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
                o.source(src),
                o.span(span),
            )
        } else {
            (filename, src, span)
        };
        let diagnostic = if error {
            Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(format!("{message} [unreachable-code]"))
        .with_labels(vec![Label::primary((), span)]);
        emit_to_io_write(
            &mut StandardStream::stderr(ColorChoice::Auto),
            &Config::default(),
            &SimpleFile::new(name, source),
            &diagnostic,
        )?;
        failed |= error;
    }
    if failed {
        bail!("unreachable code warnings treated as errors");
    }
    Ok(())
}

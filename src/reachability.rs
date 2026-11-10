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
    /// DO loops without loop control that nothing can end, for -Winfinite-loop.
    pub loops: Vec<usize>,
    /// Each procedure's body, or None when several procedures share the name.
    procedures: HashMap<String, Option<Vec<Stmt>>>,
    /// The location of the statement being analyzed, for statements nested
    /// in constructs that carry no location of their own.
    location: Option<usize>,
}

/// The ways a loop body can end the loop. `leaves`: control may continue
/// after the loop, by EXIT, a named EXIT, GO TO or an I/O branch. `stops`:
/// STOP, ERROR STOP, or a call that may stop the program. `returns`: RETURN.
#[derive(Default, Clone, Copy)]
struct Ways {
    leaves: bool,
    stops: bool,
    returns: bool,
}

fn merge(a: Ways, b: Ways) -> Ways {
    Ways { leaves: a.leaves || b.leaves, stops: a.stops || b.stops, returns: a.returns || b.returns }
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
    /// Whether a procedure may stop the program, directly or through the
    /// procedures it calls. A procedure outside the program might.
    fn may_stop(&self, name: &str, visiting: &mut HashSet<String>) -> bool {
        match self.procedures.get(name) {
            Some(Some(body)) => visiting.insert(name.to_owned()) && self.ways(body, false, visiting).stops,
            Some(None) => true,
            None => !(crate::intrinsics::purity(name).is_some() || crate::leak_warnings::INTRINSICS.contains(&name)),
        }
    }
    /// How statements can end the loop whose body they are in; `level`
    /// whether they are directly in that loop rather than in a nested one.
    fn ways(&self, body: &[Stmt], level: bool, visiting: &mut HashSet<String>) -> Ways {
        let mut w = Ways::default();
        for s in body {
            match s {
                Stmt::Exit => w.leaves |= level,
                Stmt::Return(_) => w.returns = true,
                Stmt::CallSub { name, .. } if matches!(name.as_str(), "@named_exit" | "@goto") => w.leaves = true,
                Stmt::CallSub { name, .. } if matches!(name.as_str(), "@stop" | "@error_stop") => w.stops = true,
                // The GNU extensions EXIT and ABORT end the program.
                Stmt::CallSub { name, .. } if matches!(name.as_str(), "exit" | "abort") => w.stops = true,
                Stmt::CallSub { name, .. } if !name.starts_with('@') => w.stops |= self.may_stop(name, visiting),
                Stmt::If { then_body, else_body, .. } => {
                    for b in std::iter::once(then_body).chain(else_body) {
                        let inner = self.ways(b, level, visiting);
                        w = merge(w, inner);
                    }
                }
                Stmt::Block { body } => {
                    let inner = self.ways(body, level, visiting);
                    w = merge(w, inner);
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for b in cases.iter().map(|c| &c.body).chain(default) {
                        let inner = self.ways(b, level, visiting);
                        w = merge(w, inner);
                    }
                }
                // EXIT in a nested loop leaves only that loop.
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                    let inner = self.ways(body, false, visiting);
                    w = merge(w, inner);
                }
                _ => {}
            }
            // An I/O statement with ERR=, END= or EOR= may branch out.
            if crate::action_warnings::branches(std::slice::from_ref(s)) {
                w.leaves = true;
            }
            // A function reference to a procedure of the program that may stop.
            if !matches!(s, Stmt::If { .. } | Stmt::Block { .. } | Stmt::SelectCase { .. } | Stmt::Do { .. } | Stmt::DoWhile { .. }) {
                let mut names = vec![];
                let mut copy = s.clone();
                for e in crate::procedure_arguments::expressions(&mut copy) {
                    crate::procedure_arguments::visit_expr(e, &mut |e| {
                        if let Expr::Call(n, _) = e { names.push(n.clone()); }
                    });
                }
                for n in names {
                    if self.procedures.contains_key(&n) && self.may_stop(&n, &mut visiting.clone()) {
                        w.stops = true;
                    }
                }
            }
        }
        w
    }
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
        let mut offset = self.location;
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
            // Statements nested in a construct take its location.
            self.location = offset;
            let mut endless = false;
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
                    let always = self.condition(cond, constants) == Some(true);
                    let at = offset;
                    self.body(
                        body,
                        constants,
                        self.condition(cond, constants) != Some(false),
                        Some("unreachable loop body: condition is always false"),
                    );
                    // A DO without loop control, or DO WHILE (.true.), ends
                    // only by a transfer out of its body.
                    if always {
                        let w = self.ways(body, true, &mut HashSet::new());
                        if !(w.leaves || w.stops || w.returns) {
                            if let Some(p) = at {
                                self.loops.push(p);
                            }
                        }
                        if !w.leaves {
                            endless = true;
                        }
                        w.leaves
                    } else {
                        true
                    }
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
                reason = if endless {
                    "unreachable statement after a loop that does not end"
                } else {
                    "unreachable statement after an unconditional transfer of control"
                };
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
        loops: vec![],
        procedures: HashMap::new(),
        location: None,
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
    fn procedures(body: &[Stmt], out: &mut HashMap<String, Option<Vec<Stmt>>>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    out.entry(name.clone()).and_modify(|e| *e = None).or_insert_with(|| Some(body.clone()));
                    procedures(body, out);
                }
                Stmt::Module { body, .. } => procedures(body, out),
                _ => {}
            }
        }
    }
    procedures(&program.body, &mut a.procedures);
    a.scope(
        &program.body,
        &Constants::new(),
        &[],
        Some(&program.contains),
    );
    a
}
/// Report DO loops without loop control that nothing in their body can end.
fn infinite_loops(loops: &[usize], src: &str, own_len: usize, filename: &str,
                  origins: &[crate::source_diagnostics::SourceOrigin], error: bool) -> Result<()> {
    let mut loops: Vec<usize> = loops.iter().copied().filter(|p| *p < own_len).collect();
    loops.sort();
    loops.dedup();
    if loops.is_empty() {
        return Ok(());
    }
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFile,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let tokens = crate::lexer::lex(src);
    for pos in loops {
        let statement = crate::declaration_warnings::statement_tokens(&tokens, pos, usize::MAX, src);
        let span = match (statement.first(), statement.last()) {
            (Some(a), Some(b)) => a.span.start..b.span.end,
            _ => pos..pos,
        };
        let (name, source, span) = match origins.iter().find(|o| o.range.contains(&pos)) {
            Some(o) => (o.name.as_str(), o.source(src), o.span(span)),
            None => (filename, src, span),
        };
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message("this DO loop has no EXIT, RETURN, STOP or branch out of it, so it never ends [infinite-loop]")
            .with_labels(vec![Label::primary((), span).with_message("the loop begins here")])
            .with_notes(vec!["add an EXIT condition, or use `!#allow(infinite-loop)` if the program is meant to run until stopped".to_owned(),
                "-Wno-infinite-loop disables this warning".to_owned()]);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(),
            &SimpleFile::new(name, source), &diagnostic)?;
    }
    if error {
        bail!("infinite-loop warnings treated as errors");
    }
    Ok(())
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
    let (loops_enabled, loops_error) = crate::unused::selected("infinite-loop", false, settings, options);
    if !enabled && !loops_enabled {
        return Ok(());
    }
    let mut analysis = analyze(program);
    if !enabled {
        analysis.warnings.clear();
    }
    if loops_enabled {
        infinite_loops(&analysis.loops, src, own_len, filename, origins, loops_error)?;
    }
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

#[cfg(test)]
mod tests {
    /// The number of loops reported as never ending, and of unreachable-code warnings.
    fn counts(src: &str) -> (usize, usize) {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        let a = super::analyze(&program);
        (a.loops.len(), a.warnings.len())
    }

    #[test]
    fn loops_that_never_end() {
        // The xloop_no_exit.f90 example, DO WHILE (.true.), and EXIT of an inner loop only.
        assert_eq!(counts("real :: x\ndo\ncall random_number(x)\nprint *, x\nend do\nend\n"), (1, 0));
        assert_eq!(counts("integer :: i\ni = 0\ndo while (.true.)\ni = i + 1\nend do\nprint *, i\nend\n"), (1, 1));
        assert_eq!(counts("integer :: i\ndo\ndo\nexit\nend do\nend do\nend\n"), (1, 0));
    }

    #[test]
    fn loops_that_can_end() {
        for src in [
            "integer :: i\ni = 0\ndo\ni = i + 1\nif (i > 3) exit\nend do\nprint *, i\nend\n",
            "integer :: i\nouter: do\ndo\nexit outer\nend do\nend do outer\nend\n",
            "integer :: i\ndo\nread (*, *, end=10) i\nend do\n10 continue\nend\n",
            "integer :: i\ndo\ngo to 10\nend do\n10 continue\nend\n",
            "integer :: i\ndo i = 1, 3\nprint *, i\nend do\nend\n",
            // A procedure of the program that may stop, and one outside it.
            "do\ncall finish()\nend do\ncontains\nsubroutine finish()\nstop\nend subroutine\nend\n",
            "do\ncall elsewhere()\nend do\nend\n",
            "subroutine s()\ndo\nreturn\nend do\nend subroutine\n",
        ] {
            assert_eq!(counts(src).0, 0, "{src}");
        }
    }

    #[test]
    fn code_after_a_loop_that_only_stops_is_unreachable() {
        // STOP ends the program, so the PRINT after the loop never runs; a
        // procedure that does not stop does not end the loop.
        assert_eq!(counts("integer :: i\ni = 0\ndo\ni = i + 1\nif (i > 3) stop\nend do\nprint *, i\nend\n"), (0, 1));
        assert_eq!(counts("do\ncall show()\nend do\ncontains\nsubroutine show()\nprint *, 1\nend subroutine\nend\n"), (1, 0));
    }
}

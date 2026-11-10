// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wearly-return`, opt-in: a RETURN, conditional or not, after which
//! executable statements of the procedure can still run, such as a guard
//! clause `if (n <= 0) return` or a RETURN inside a loop. A RETURN that is
//! the last executable statement is not reported, nor one followed only by
//! END IF, END SELECT, END BLOCK, END ASSOCIATE, CONTINUE and FORMAT
//! statements and by other branches of the constructs it is in. Code after
//! an unconditional RETURN is also reported by -Wunreachable-code.
use crate::indentation_warnings::{classify, words, Kind, Role};
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::ops::Range;

const CATEGORY: &str = "early-return";

struct Statement {
    label: Option<u64>,
    words: Vec<String>,
    /// The RETURN keyword, when the statement is or ends with one.
    ret: Option<Range<usize>>,
}

struct Warning {
    span: Range<usize>,
    procedure: (String, String),
    in_loop: bool,
}

/// The index of the parenthesis closing the one at `open`.
fn closing(w: &[String], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, x) in w.iter().enumerate().skip(open) {
        match x.as_str() {
            "(" => depth += 1,
            ")" => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}

/// Whether `w` is RETURN or a logical IF whose action is RETURN.
fn returns(w: &[String]) -> bool {
    match w.first().map(String::as_str) {
        Some("return") => true,
        Some("if") if w.get(1).is_some_and(|x| x == "(") =>
            closing(w, 1).is_some_and(|c| w.get(c + 1).is_some_and(|x| x == "return")),
        _ => false,
    }
}

/// Whether an executable statement of the procedure can run after the
/// RETURN in statement `at`, were the RETURN not taken.
fn more_follows(statements: &[Statement], at: usize, terminators: &[u64]) -> bool {
    // Nesting depth inside a branch being skipped, as after ELSE or CASE.
    let mut skip = 0usize;
    for s in &statements[at + 1..] {
        if skip == 0 && s.label.is_some_and(|l| terminators.contains(&l)) { return true; }
        let role = classify(&s.words, Some(Kind::Select));
        if skip > 0 {
            match role {
                Role::Open(Kind::Unit, _) => return false,
                Role::Open(..) => skip += 1,
                Role::Close(Some(k)) if k != Kind::Unit => skip -= 1,
                Role::Close(_) => return false,
                _ => {}
            }
            continue;
        }
        match role {
            Role::Close(Some(Kind::Do | Kind::Forall | Kind::Where)) => return true,
            Role::Close(None | Some(Kind::Unit)) | Role::Contains | Role::Open(Kind::Unit, _) => return false,
            Role::Close(Some(_)) => {}
            Role::Else(_) | Role::Case => skip = 1,
            // Another entry point needs the RETURN before it.
            _ if s.words.first().is_some_and(|x| x == "entry") => return false,
            _ if matches!(s.words.first().map(String::as_str), Some("continue" | "format" | "data")) => {}
            _ => return true,
        }
    }
    false
}

/// The kind and name of a procedure opened by `w`.
fn procedure(w: &[String]) -> Option<(String, String)> {
    let at = w.iter().position(|x| matches!(x.as_str(), "function" | "subroutine" | "procedure"))?;
    Some((w[at].clone(), w.get(at + 1)?.clone()))
}

fn analyze(src: &str, tokens: &[Token], own_len: usize) -> Vec<Warning> {
    let mut statements = vec![];
    let mut i = 0;
    while i < tokens.len() && tokens[i].span.start < own_len {
        let ts = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        i += ts.len().max(1);
        let Some(start) = ts.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))) else { continue };
        let label = ts[..start].iter().find_map(|t| src[t.span.clone()].trim().parse::<u64>().ok());
        let (head, last) = (&ts[start], &ts[ts.len() - 1]);
        let w = words(&src[head.span.start..last.span.end.max(head.span.end)]);
        let ret = returns(&w).then(|| ts[start..].iter().rev()
            .find(|t| src[t.span.clone()].eq_ignore_ascii_case("return"))
            .map_or(head.span.clone(), |t| t.span.clone()));
        statements.push(Statement { label, words: w, ret });
    }
    // Labels ending labeled DO loops.
    let terminators: Vec<u64> = statements.iter().filter_map(|s| {
        let w = if s.words.len() > 2 && s.words[1] == ":" { &s.words[2..] } else { &s.words[..] };
        (w.first().is_some_and(|x| x == "do")).then(|| w.get(1)?.parse().ok()).flatten()
    }).collect();
    let mut out = vec![];
    // Open constructs, with each procedure's kind and name.
    // Open constructs, with each procedure's kind and name and each labeled
    // DO loop's terminating label.
    let mut stack: Vec<(Kind, Option<(String, String)>, Option<u64>)> = vec![];
    for (at, s) in statements.iter().enumerate() {
        let role = classify(&s.words, stack.last().map(|f| f.0));
        match &role {
            Role::Open(kind, label) => stack.push((*kind, if *kind == Kind::Unit { procedure(&s.words) } else { None }, *label)),
            Role::Close(kind) => {
                let wanted = kind.unwrap_or(Kind::Unit);
                if let Some(p) = stack.iter().rposition(|f| f.0 == wanted) { stack.truncate(p); }
            }
            _ => {}
        }
        // Loops sharing a terminating statement all end after it.
        let ends = s.label.filter(|_| !matches!(role, Role::Close(_)))
            .and_then(|l| stack.iter().position(|f| f.0 == Kind::Do && f.2 == Some(l)));
        // The innermost program unit, which must be a procedure.
        let unit = stack.iter().rposition(|f| f.0 == Kind::Unit);
        let procedure = unit.and_then(|u| stack[u].1.clone());
        if let (Some(span), Some(unit), Some(procedure)) = (s.ret.clone(), unit, procedure) {
            if !stack.iter().any(|f| f.0 == Kind::Interface) && more_follows(&statements, at, &terminators) {
                let in_loop = stack[unit..].iter().any(|f| f.0 == Kind::Do);
                out.push(Warning { span, procedure, in_loop });
            }
        }
        if let Some(p) = ends { stack.truncate(p); }
    }
    out
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // Opt-in: -Wall does not enable this advice.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
    if !enabled { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    let warnings = analyze(src, &tokens, own_len);
    if warnings.is_empty() { return Ok(()); }
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for w in &warnings {
        let (file, at) = locate(w.span.clone());
        let (kind, name) = &w.procedure;
        let mut notes = vec![];
        if w.in_loop { notes.push("it also ends the enclosing loop".to_owned()); }
        notes.push("a single exit at the end, reached through IF ... ELSE or a named BLOCK left with EXIT, keeps the flow in one place".to_owned());
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("RETURN before the end of {kind} `{name}` [{CATEGORY}]"))
            .with_labels(vec![Label::primary(file, at).with_message(format!("later statements of `{name}` can still run"))])
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The lines of the reported RETURNs, and whether each is in a loop.
    fn reported(src: &str) -> Vec<(usize, bool)> {
        let tokens = crate::lexer::lex(src);
        super::analyze(src, &tokens, src.len()).into_iter()
            .map(|w| (src[..w.span.start].matches('\n').count() + 1, w.in_loop)).collect()
    }

    #[test]
    fn final_returns_are_not_reported() {
        let src = "module m\ncontains\nsubroutine a(x)\nreal :: x\nx = 1\nreturn\nend subroutine a\n\
            subroutine b(x)\nreal :: x\nif (x > 0) then\n   x = 1\n   return\nelse if (x < 0) then\n   x = 2\nelse\n   return\nend if\nend subroutine b\n\
            integer function f(n)\ninteger :: n\nselect case (n)\ncase (1)\n   f = 1\n   return\ncase default\n   f = 2\nend select\n10 continue\nend function f\n\
            subroutine c()\ninterface\n   subroutine d()\n   end subroutine d\nend interface\nreturn\ncontains\nsubroutine e()\nend subroutine e\nend subroutine c\nend module m\n";
        assert_eq!(reported(src), []);
    }

    #[test]
    fn early_returns() {
        let src = "subroutine a(n, x)\ninteger :: n, i\nreal :: x\nif (n <= 0) return\nx = 1\n\
            do i = 1, n\n   if (i > 3) return\nend do\nx = 2\ndo 10 i = 1, n\n   if (i > 4) return\n10 continue\n\
            if (x > 0) then\n   return\nend if\nx = 3\nend subroutine a\n";
        assert_eq!(reported(src), [(4, false), (7, true), (11, true), (14, false)]);
    }
}

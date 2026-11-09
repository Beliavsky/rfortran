// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wuse-without-only`, opt-in: a USE statement without ONLY imports every
//! public entity of its module. The warning suggests the ONLY list that the
//! scope needs, or says that the USE imports nothing that is used.
//!
//! A scope needs an entity when its name appears anywhere in the scope,
//! including procedures it contains, which inherit the USE by host
//! association. A defined operator is needed where it is applied, an
//! intrinsic operator such as operator(+) wherever that operator appears,
//! assignment(=) wherever there is an assignment, and defined input/output
//! wherever there is input or output. A name that appears for another reason
//! (a component or a keyword argument of the same spelling) is kept, which
//! is harmless: the suggested list may be longer than necessary but never
//! omits a needed entity. A module with a public default re-exports what it
//! uses, so its users in the program count too.
//!
//! Only modules defined in the program or loaded from .rmod files are
//! checked; intrinsic modules are not. A module consisting only of USE and
//! accessibility statements re-exports its modules on purpose and is skipped.
use crate::ast::{Program, Stmt};
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{BTreeSet, HashSet};

const CATEGORY: &str = "use-without-only";

/// Everything a scope's statements mention: each string in the AST, the
/// identifiers within those strings, and the AST node names.
#[derive(Default)]
struct Words {
    strings: HashSet<String>,
    words: HashSet<String>,
    nodes: HashSet<String>,
}
impl Words {
    fn add(&mut self, value: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                for w in s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()) {
                    // The kind of a literal such as 1.0_dp or 5_ik.
                    if let Some((_, kind)) = w.split_once('_').filter(|_| w.starts_with(|c: char| c.is_ascii_digit())) {
                        self.words.insert(kind.to_ascii_lowercase());
                    }
                    self.words.insert(w.to_ascii_lowercase());
                }
                self.strings.insert(s.clone());
            }
            serde_json::Value::Array(items) => for v in items { self.add(v); },
            serde_json::Value::Object(fields) => for (k, v) in fields {
                self.nodes.insert(k.clone());
                self.add(v);
            },
            _ => {}
        }
    }
    fn extend(&mut self, other: &Words) {
        self.strings.extend(other.strings.iter().cloned());
        self.words.extend(other.words.iter().cloned());
        self.nodes.extend(other.nodes.iter().cloned());
    }
}

struct Use<'a> {
    module: &'a str,
    only: Option<&'a Vec<String>>,
    renames: &'a [(String, String)],
    offset: Option<usize>,
}

struct Scope<'a> {
    /// The module this scope defines, if it is a module.
    module: Option<&'a str>,
    default_public: bool,
    /// Only USE and accessibility statements: a module that re-exports.
    reexports: bool,
    uses: Vec<Use<'a>>,
    words: Words,
}

fn scope<'a>(body: &'a [Stmt], module: Option<&'a str>, top: bool) -> Scope<'a> {
    let mut uses = vec![];
    let mut at = None;
    let mut words = Words::default();
    for s in body {
        match s {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::Use { module, only, renames, .. } => uses.push(Use { module, only: only.as_ref(), renames, offset: at }),
            // Modules in the program body are scopes of their own.
            Stmt::Module { .. } if top => {}
            _ => if let Ok(value) = serde_json::to_value(s) { words.add(&value); },
        }
    }
    Scope {
        module,
        default_public: !body.iter().any(|s| matches!(s, Stmt::Accessibility { public: false, names } if names.is_empty())),
        reexports: module.is_some() && body.iter().all(|s| matches!(s,
            Stmt::Use { .. } | Stmt::Accessibility { .. } | Stmt::SourceLocation(_) | Stmt::Implicit(_))),
        uses,
        words,
    }
}

/// Every scope that can contain USE statements: the main program (the
/// statements of the program body), modules, procedures and BLOCK constructs.
fn scopes<'a>(body: &'a [Stmt], out: &mut Vec<Scope<'a>>) {
    for s in body {
        match s {
            Stmt::Module { name, body } => {
                out.push(scope(body, Some(name), false));
                scopes(body, out);
            }
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => {
                if body.iter().any(|s| matches!(s, Stmt::Use { .. })) { out.push(scope(body, None, false)); }
                scopes(body, out);
            }
            _ => {}
        }
    }
}

/// The words of the scopes that use `module`, and of their users in turn
/// when they re-export it through a public default.
fn users(module: &str, scopes: &[Scope], seen: &mut HashSet<String>, out: &mut Words) {
    if !seen.insert(module.to_owned()) { return; }
    for scope in scopes {
        for u in scope.uses.iter().filter(|u| u.module == module) {
            match u.only {
                Some(names) => {
                    for n in names.iter().chain(u.renames.iter().map(|(_, remote)| remote)) {
                        out.words.insert(n.clone());
                        out.strings.insert(n.clone());
                    }
                }
                None => {
                    out.extend(&scope.words);
                    if let Some(m) = scope.module.filter(|_| scope.default_public) { users(m, scopes, seen, out); }
                }
            }
        }
    }
}

/// The AST names of the expressions that apply an intrinsic operator.
fn operator_nodes(operator: &str) -> &'static [&'static str] {
    match operator {
        "+" => &["Add"],
        "-" => &["Sub", "Neg"],
        "*" => &["Mul"],
        "/" => &["Div"],
        "**" => &["Pow"],
        "//" => &["Concat"],
        "==" | ".eq." => &["Eq"],
        "/=" | ".ne." => &["Ne"],
        "<" | ".lt." => &["Lt"],
        "<=" | ".le." => &["Le"],
        ">" | ".gt." => &["Gt"],
        ">=" | ".ge." => &["Ge"],
        ".and." => &["And"],
        ".or." => &["Or"],
        ".not." => &["Not"],
        ".eqv." => &["Eqv"],
        ".neqv." => &["Neqv"],
        _ => &[],
    }
}

fn needed(entity: &str, words: &Words) -> bool {
    if let Some(operator) = entity.strip_prefix("operator(").and_then(|o| o.strip_suffix(')')) {
        let intrinsic = operator_nodes(operator);
        return if intrinsic.is_empty() {
            words.strings.contains(&format!("@defined_operator:{operator}"))
        } else {
            intrinsic.iter().any(|n| words.strings.contains(*n))
        };
    }
    if entity.starts_with("assignment(") { return words.nodes.iter().any(|n| n.starts_with("Assign")); }
    if entity.contains('(') {
        // Defined input/output, such as write(formatted).
        return words.nodes.contains("Print") || ["read", "write", "print"].iter().any(|w| words.words.contains(*w));
    }
    words.words.contains(entity)
}

struct Report {
    offset: usize,
    module: String,
    /// The suggested ONLY list, or None when nothing is used.
    only: Option<Vec<String>>,
    count: usize,
    /// The scope is a module with a public default, so users outside the
    /// program may rely on the entities it re-exports.
    reexported: bool,
}

fn analyze(program: &Program) -> Vec<Report> {
    let exports = crate::imports::module_exports(program);
    let mut all = vec![scope(&program.body, None, true)];
    scopes(&program.body, &mut all);
    let mut reports = vec![];
    for scope in all.iter().filter(|s| !s.reexports) {
        let reexported = scope.module.is_some() && scope.default_public;
        let mut words = None;
        for u in scope.uses.iter().filter(|u| u.only.is_none()) {
            let (Some(entities), Some(offset)) = (exports.get(u.module), u.offset) else { continue };
            let words = words.get_or_insert_with(|| {
                let mut words = Words::default();
                words.extend(&scope.words);
                if let Some(m) = scope.module.filter(|_| reexported) { users(m, &all, &mut HashSet::new(), &mut words); }
                words
            });
            let renamed: HashSet<&str> = u.renames.iter().map(|(_, remote)| remote.as_str()).collect();
            let (mut operators, mut names) = (BTreeSet::new(), BTreeSet::new());
            for e in entities.iter().filter(|e| !renamed.contains(e.as_str()) && needed(e, words)) {
                if e.contains('(') { operators.insert(e.clone()); } else { names.insert(e.clone()); }
            }
            let mut only: Vec<String> = names.into_iter().collect();
            only.extend(u.renames.iter().map(|(local, remote)| format!("{local} => {remote}")));
            only.extend(operators);
            reports.push(Report { offset, module: u.module.to_owned(), count: entities.len(),
                only: (!only.is_empty()).then_some(only), reexported });
        }
    }
    reports.sort_by_key(|r| r.offset);
    reports
}

/// `prefix, only: a, b, c`, continued with `&` when it is long.
fn suggestion(prefix: &str, only: &[String]) -> String {
    let mut lines = vec![format!("{prefix}, only:")];
    for (i, item) in only.iter().enumerate() {
        let item = if i + 1 < only.len() { format!("{item},") } else { item.clone() };
        let last = lines.last_mut().unwrap();
        if last.len() + 1 + item.len() > 76 && i > 0 {
            last.push_str(" &");
            lines.push(format!("    {item}"));
        } else {
            last.push(' ');
            last.push_str(&item);
        }
    }
    lines.join("\n")
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
    // Opt-in: -Wall does not enable this style advice.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
    if !enabled { return Ok(()); }
    let mut reports = analyze(program);
    reports.retain(|r| r.offset < own_len);
    if reports.is_empty() { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    for r in &reports {
        let statement = crate::declaration_warnings::statement_tokens(&tokens, r.offset, usize::MAX, src);
        let (Some(first), Some(last)) = (statement.first(), statement.last()) else { continue };
        // The suggestion keeps the statement's spelling up to the module name.
        let name = statement.iter().skip(1).find(|t| matches!(&t.kind, TokenKind::Ident(n) if *n == r.module));
        let prefix = &src[first.span.start..name.map_or(last.span.end, |t| t.span.end)];
        let (file, at) = locate(first.span.start..last.span.end);
        let mut notes = vec![];
        let (message, label) = match &r.only {
            Some(only) => {
                notes.push(format!("suggestion: {}", suggestion(prefix, only)));
                (format!("USE of {} without ONLY imports all {} of its public entities [{CATEGORY}]", r.module, r.count),
                 format!("{} of them used here", only.len()))
            }
            None => {
                notes.push("remove the USE statement".to_owned());
                (format!("USE of {} imports nothing that is used [{CATEGORY}]", r.module), "no entity of the module is used".to_owned())
            }
        };
        if r.reexported {
            notes.push("this module re-exports the entities it uses; users of it outside this program are not taken into account".to_owned());
        }
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(vec![Label::primary(file, at).with_message(label)])
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each USE without ONLY: its module and the suggested list, or None.
    fn reports(src: &str) -> Vec<(String, Option<Vec<String>>)> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        super::analyze(&program).into_iter().map(|r| (r.module, r.only)).collect()
    }
    fn only(module: &str, names: &[&str]) -> (String, Option<Vec<String>>) {
        (module.into(), Some(names.iter().map(|s| s.to_string()).collect()))
    }

    const STATS: &str = "module stats\nimplicit none\nprivate\npublic :: dp, mean, sd, unused_fn\ninteger, parameter :: dp = kind(1.0d0)\ncontains\nreal(dp) function mean(x)\nreal(dp), intent(in) :: x(:)\nmean = sum(x)/size(x)\nend function\nreal(dp) function sd(x)\nreal(dp), intent(in) :: x(:)\nsd = sqrt(sum((x-mean(x))**2)/size(x))\nend function\nreal(dp) function unused_fn()\nunused_fn = 0\nend function\nend module\n";

    #[test]
    fn suggests_the_entities_used() {
        let src = format!("{STATS}program p\nuse stats\nimplicit none\nreal(dp) :: x(3) = [1, 2, 3]\nprint *, mean(x)\ncall show()\ncontains\nsubroutine show()\nprint *, sd(x)\nend subroutine\nend program\n");
        assert_eq!(reports(&src), [only("stats", &["dp", "mean", "sd"])]);
    }

    #[test]
    fn unused_modules_and_renames() {
        let src = format!("{STATS}program p\nuse stats\nprint *, 1\nend program\n");
        assert_eq!(reports(&src), [("stats".into(), None)]);
        let src = format!("{STATS}program p\nuse stats, average => mean\nprint *, average([1.0_dp])\nend program\n");
        assert_eq!(reports(&src), [only("stats", &["dp", "average => mean"])]);
        // USE with ONLY is not reported.
        assert!(reports(&format!("{STATS}program p\nuse stats, only: mean\nend program\n")).is_empty());
    }

    #[test]
    fn operators_and_assignment() {
        let module = "module vecs\nimplicit none\ntype :: vec\nreal :: x = 0\nend type\ninterface operator(.dot.)\nmodule procedure dotv\nend interface\ninterface operator(+)\nmodule procedure addv\nend interface\ncontains\nreal function dotv(a, b)\ntype(vec), intent(in) :: a, b\ndotv = a%x*b%x\nend function\ntype(vec) function addv(a, b)\ntype(vec), intent(in) :: a, b\naddv%x = a%x + b%x\nend function\nend module\n";
        let src = format!("{module}program p\nuse vecs\ntype(vec) :: a, b\nprint *, a .dot. b\nend program\n");
        assert_eq!(reports(&src), [only("vecs", &["vec", "operator(.dot.)"])]);
        let src = format!("{module}program p\nuse vecs\ntype(vec) :: a, b\na = a + b\nend program\n");
        assert_eq!(reports(&src), [only("vecs", &["vec", "operator(+)"])]);
    }

    #[test]
    fn reexports_count_their_users() {
        let src = format!("{STATS}module wrapper\nuse stats\nimplicit none\ncontains\nsubroutine report(x)\nreal(dp), intent(in) :: x(:)\nprint *, mean(x)\nend subroutine\nend module\nprogram p\nuse wrapper\nprint *, sd([1.0_dp, 2.0_dp])\ncall report([1.0_dp])\nend program\n");
        assert_eq!(reports(&src), [only("stats", &["dp", "mean", "sd"]), only("wrapper", &["dp", "report", "sd"])]);
        // A private default stops re-export, and a module of only USE statements is skipped.
        let src = format!("{STATS}module wrapper\nuse stats\nimplicit none\nprivate\npublic :: report\ncontains\nsubroutine report(x)\nreal(dp), intent(in) :: x(:)\nprint *, mean(x)\nend subroutine\nend module\nmodule facade\nuse stats\nend module\nprogram p\nuse wrapper, only: report\nend program\n");
        assert_eq!(reports(&src), [only("stats", &["dp", "mean"])]);
    }

    #[test]
    fn long_suggestions_are_continued() {
        let only: Vec<String> = (1..=12).map(|i| format!("entity_number_{i}")).collect();
        let text = super::suggestion("use big_module", &only);
        assert!(text.lines().count() > 1, "{text}");
        assert!(text.lines().all(|l| l.len() <= 80), "{text}");
        assert!(text.lines().rev().skip(1).all(|l| l.ends_with(" &")), "{text}");
    }
}

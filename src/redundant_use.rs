// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wredundant-use`, in -Wstyle: a USE statement, or names in its ONLY
//! list, that import nothing new. Either the host already makes the same
//! entities visible under the same names through its own USE of the module,
//! as with `use iso_fortran_env, only: int32` in a module and again in one
//! of its procedures, or an earlier USE in the same scope does.
//!
//! A host is a module for its procedures, a procedure for its internal
//! procedures, and a main program for the procedures after its CONTAINS.
//! An interface body has no host: its USE statements are needed.
//! USE without ONLY makes every entity visible under its own name, except
//! the ones it renames. A rename to another local name imports something
//! new, and so does a USE without ONLY after a USE with ONLY. Intrinsic
//! modules are checked like the program's own.
use crate::ast::{Program, Stmt};
use crate::lexer::{lex, Token};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "redundant-use";

/// What the USE statements of a scope and its hosts make visible from one
/// module: everything under its own name except the renamed entities, or
/// the listed (local, module) pairs.
#[derive(Clone, Default)]
struct Visible {
    all: bool,
    renamed: HashSet<String>,
    pairs: HashSet<(String, String)>,
}

impl Visible {
    fn contains(&self, local: &str, remote: &str) -> bool {
        self.pairs.contains(&(local.to_owned(), remote.to_owned()))
            || (self.all && local == remote && !self.renamed.contains(remote))
    }
}

/// The modules visible in a scope, with where each USE came from: the host
/// (true) or this scope (false).
type Imports = HashMap<String, (Visible, bool)>;

/// The (local, module) pairs of a USE statement with an ONLY list.
fn pairs(only: &[String], renames: &[(String, String)]) -> Vec<(String, String)> {
    let renamed: HashSet<&str> = renames.iter().map(|(_, r)| r.as_str()).collect();
    let mut out: Vec<(String, String)> = renames.to_vec();
    for n in only {
        if !renamed.contains(n.as_str()) {
            out.push((n.clone(), n.clone()));
        }
    }
    out
}

#[derive(Debug, PartialEq)]
struct Report {
    offset: usize,
    module: String,
    /// The local names that are already visible; all of the statement's
    /// names when `whole`.
    names: Vec<String>,
    whole: bool,
    /// The ONLY list without them, when some remain.
    kept: Vec<String>,
    from_host: bool,
    /// Whether nothing else in the host uses the names, so that removing
    /// the host's USE instead would keep them local.
    host_only_here: bool,
}

/// The identifiers a list of statements mentions, apart from USE statements.
fn words(body: &[Stmt], skip: Option<usize>) -> HashSet<String> {
    let mut out = HashSet::new();
    fn add(value: &serde_json::Value, out: &mut HashSet<String>) {
        match value {
            serde_json::Value::String(s) => {
                for w in s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()) {
                    out.insert(w.to_ascii_lowercase());
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| add(v, out)),
            serde_json::Value::Object(fields) => fields.values().for_each(|v| add(v, out)),
            _ => {}
        }
    }
    for (i, s) in body.iter().enumerate() {
        if Some(i) == skip || matches!(s, Stmt::Use { .. }) {
            continue;
        }
        if let Ok(value) = serde_json::to_value(s) {
            add(&value, &mut out);
        }
    }
    out
}

/// Check the USE statements of `body`, a scope whose hosts make `inherited`
/// visible, and then the procedures it contains. `host` is the body of the
/// host and the index of this scope in it, for the note about the host's USE.
fn scope(body: &[Stmt], inherited: &Imports, host: Option<(&[Stmt], usize)>, contains: Option<&[String]>, out: &mut Vec<Report>) {
    let mut imports: Imports = inherited.iter().map(|(m, (v, _))| (m.clone(), (v.clone(), true))).collect();
    let mut offset = 0;
    for s in body {
        match s {
            Stmt::SourceLocation(at) => offset = *at,
            Stmt::Use { module, only, renames, .. } => {
                let key = module.to_ascii_lowercase();
                let entry = imports.entry(key).or_default();
                let from_host = entry.1;
                let visible = &mut entry.0;
                match only {
                    Some(only) => {
                        let listed = pairs(only, renames);
                        let mut seen = HashSet::new();
                        let (redundant, kept): (Vec<_>, Vec<_>) = listed.iter().partition(|(l, r)| {
                            !seen.insert((l.clone(), r.clone())) || visible.contains(l, r)
                        });
                        if !redundant.is_empty() {
                            let names: Vec<String> = redundant.iter().map(|(l, _)| l.clone()).collect();
                            let used = host.map(|(b, i)| words(b, Some(i))).unwrap_or_default();
                            let host_only_here = from_host && names.iter().all(|n| !used.contains(&n.to_ascii_lowercase()));
                            let kept: Vec<String> = kept.iter().map(|(l, r)| if l == r { l.clone() } else { format!("{l} => {r}") }).collect();
                            out.push(Report { offset, module: module.clone(), whole: kept.is_empty(), names, kept, from_host, host_only_here });
                        }
                        for (l, r) in listed {
                            visible.pairs.insert((l, r));
                        }
                    }
                    None => {
                        let renamed_already = renames.iter().all(|(l, r)| visible.contains(l, r));
                        let same_renames = renames.iter().map(|(_, r)| r.clone()).collect::<HashSet<_>>() == visible.renamed;
                        if visible.all && renamed_already && same_renames {
                            out.push(Report { offset, module: module.clone(), names: vec![], whole: true, kept: vec![],
                                from_host, host_only_here: false });
                        }
                        if !visible.all {
                            visible.renamed = renames.iter().map(|(_, r)| r.clone()).collect();
                        }
                        visible.all = true;
                        for (l, r) in renames {
                            visible.pairs.insert((l.clone(), r.clone()));
                        }
                    }
                }
                entry.1 = false;
            }
            _ => {}
        }
    }
    for (i, s) in body.iter().enumerate() {
        match s {
            Stmt::Function { name, body: b, .. } | Stmt::Subroutine { name, body: b, .. } => {
                // In a main program, only the procedures after CONTAINS are
                // internal; the others are external.
                let internal = contains.is_none_or(|c| c.iter().any(|n| n.eq_ignore_ascii_case(name)));
                // An interface body inherits nothing from its host.
                let interface = body.iter().any(|s| matches!(s, Stmt::DummyIntent { name: n, intent } if n == name && intent.starts_with("@interface")))
                    || b.iter().any(|s| matches!(s, Stmt::DummyIntent { name, .. } if name == crate::c_interop::IMPORT));
                if internal && !interface {
                    scope(b, &imports, Some((body, i)), None, out);
                } else {
                    scope(b, &Imports::new(), None, None, out);
                }
            }
            Stmt::Module { body: b, .. } => scope(b, &Imports::new(), None, None, out),
            _ => {}
        }
    }
}

fn reports(program: &Program) -> Vec<Report> {
    let mut out = vec![];
    scope(&program.body, &Imports::new(), None, Some(&program.contains), &mut out);
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
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
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
        let (file, at) = locate(crate::leak_warnings::span(src, &tokens, r.offset, None));
        let source = if r.from_host { "the host already imports" } else { "an earlier USE in this scope already imports" };
        let listed = r.names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
        let message = match (r.whole, r.names.is_empty()) {
            (true, true) => format!("USE of `{}` imports nothing new: {source} all of it [{CATEGORY}]", r.module),
            (true, false) => format!("USE of `{}` imports nothing new: {source} {listed} [{CATEGORY}]", r.module),
            (false, _) => format!("{listed} in this USE of `{}`: {source} {} [{CATEGORY}]", r.module,
                if r.names.len() == 1 { "it" } else { "them" }),
        };
        let mut notes = vec![if r.whole {
            "suggestion: delete this USE statement".to_owned()
        } else {
            format!("suggestion: use {}, only: {}", r.module, r.kept.join(", "))
        }];
        if r.host_only_here {
            notes.push(format!("nothing else in the host uses {listed}; removing the host's import instead keeps {} local to this procedure",
                if r.names.len() == 1 { "it" } else { "them" }));
        }
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(message)
            .with_labels(vec![Label::primary(file, at).with_message("redundant")])
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
    /// (line, names, whole, from_host, host_only_here) of each report.
    fn found(src: &str) -> Vec<(usize, Vec<String>, bool, bool, bool)> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        super::reports(&program)
            .into_iter()
            .map(|r| (src[..r.offset].matches('\n').count() + 1, r.names, r.whole, r.from_host, r.host_only_here))
            .collect()
    }
    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn procedure_repeats_module_import() {
        let src = "module m\nuse iso_fortran_env, only: int32\nimplicit none\ncontains\nsubroutine sub()\n\
            use iso_fortran_env, only: int32\nprint*,int32\nend subroutine sub\nend module m\n";
        assert_eq!(found(src), [(6, names(&["int32"]), true, true, true)]);
        // Another procedure uses the host's import: no note.
        let src = "module m\nuse iso_fortran_env, only: int32, int64\nimplicit none\ncontains\nsubroutine sub()\n\
            use iso_fortran_env, only: int32, real64\nprint*,int32, real64\nend subroutine sub\n\
            subroutine other()\nprint *, int32\nend subroutine other\nend module m\n";
        assert_eq!(found(src), [(6, names(&["int32"]), false, true, false)]);
    }

    #[test]
    fn same_scope_and_whole_module_imports() {
        let src = "program p\nuse iso_fortran_env, only: int32\nuse iso_fortran_env, only: int32, int64\nprint *, int32, int64\nend program p\n";
        assert_eq!(found(src), [(3, names(&["int32"]), false, false, false)]);
        let src = "module a\ninteger :: x\nend module a\nmodule b\nuse a\ncontains\nsubroutine s()\nuse a\nuse a, only: x\nprint *, x\nend subroutine s\nend module b\n";
        assert_eq!(found(src), [(8, names(&[]), true, true, false), (9, names(&["x"]), true, false, false)]);
    }

    #[test]
    fn new_imports_are_not_reported() {
        for src in [
            // A rename to another local name, and USE without ONLY after ONLY.
            "module a\ninteger :: x\nend module a\nmodule b\nuse a, only: x\ncontains\nsubroutine s()\nuse a, only: y => x\nuse a\nprint *, y\nend subroutine s\nend module b\n",
            // A renamed entity is not visible under its own name.
            "module a\ninteger :: x\nend module a\nmodule b\nuse a, z => x\ncontains\nsubroutine s()\nuse a, only: x\nprint *, x\nend subroutine s\nend module b\n",
            // An interface body does not inherit its host's USE.
            "module m
use iso_c_binding
contains
function k() result(r)
type(c_funptr) :: r
interface
function fun() bind(C)
use iso_c_binding
real(c_float) fun
end function fun
end interface
r = c_funloc(fun)
end function k
end module m
",
            "module m
use iso_fortran_env, only: real64
interface
function f(x)
use iso_fortran_env, only: real64
real(real64) :: f, x
end function f
end interface
end module m
",
            // An external procedure does not inherit the program's USE.
            "program p\nuse iso_fortran_env, only: int32\nprint *, int32\nend program p\nsubroutine e()\nuse iso_fortran_env, only: int32\nprint *, int32\nend subroutine e\n",
        ] {
            assert!(found(src).is_empty(), "{src}: {:?}", found(src));
        }
    }

    #[test]
    fn main_program_internal_procedure() {
        let src = "program p\nuse iso_fortran_env, only: int32\ncall s()\ncontains\nsubroutine s()\nuse iso_fortran_env, only: int32\nprint *, int32\nend subroutine s\nend program p\n";
        assert_eq!(found(src), [(6, names(&["int32"]), true, true, true)]);
    }
}

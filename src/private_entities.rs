// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! A name that cannot be found in a scope is often a module entity that the
//! module keeps PRIVATE. The PRIVATE entities of the program's modules,
//! including separately compiled ones, are recorded before lowering, and an
//! error naming one of them gets a note that says how to make it PUBLIC.
use crate::ast::Program;
use std::cell::RefCell;

thread_local! {
    /// (module, entity, whether PRIVATE only by the module's default)
    static PRIVATE: RefCell<Vec<(String, String, bool)>> = const { RefCell::new(Vec::new()) };
}

/// Record the PRIVATE entities of the modules of `program`, after the
/// modules of separately compiled files are added and before lowering.
pub(crate) fn record(program: &Program) {
    let private = crate::imports::private_entities(program);
    PRIVATE.with(|p| *p.borrow_mut() = private);
}

/// Errors that say a name cannot be found or is not accessible.
const UNRESOLVED: &[&str] = &[
    "undefined variable",
    "undefined function",
    "undefined subroutine",
    "unknown derived type",
    "unknown function",
    "unknown subroutine",
    "unknown procedure",
    "is not a public entity of module",
    "is not accessible in this scope",
    "has no IMPLICIT type",
    "is not a previously defined integer PARAMETER",
    "no matching operator(",
];

/// The notes for an error `message` that names PRIVATE module entities.
pub(crate) fn notes(message: &str) -> Vec<String> {
    let message = crate::source_diagnostics::user_names(message);
    if !UNRESOLVED.iter().any(|m| message.contains(m)) {
        return vec![];
    }
    let quoted: Vec<&str> = message.split('`').skip(1).step_by(2).collect();
    // "`x` is not a public entity of module `m`" concerns `m` only.
    let module = message.contains("is not a public entity of module").then(|| quoted.get(1).copied()).flatten();
    // "no matching operator(.op.) for operand types ..."
    let operator = message.split_once("no matching operator(")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(op, _)| format!("operator({op})"));
    let Some(name) = operator.or_else(|| quoted.first().map(|n| n.to_string())) else { return vec![] };
    let name = name.to_ascii_lowercase();
    PRIVATE.with(|p| {
        p.borrow().iter()
            .filter(|(m, n, _)| *n == name && module.is_none_or(|module| module.eq_ignore_ascii_case(m)))
            .map(|(m, n, by_default)| {
                let why = if *by_default { String::from(", whose entities are PRIVATE by default") } else { String::new() };
                format!("help: `{n}` is PRIVATE in module `{m}`{why}; to use it outside the module, add `public :: {n}` to module `{m}`")
            })
            .collect()
    })
}

/// `message` followed by its notes, one per line.
pub(crate) fn with_notes(message: String) -> String {
    let notes = notes(&message);
    if notes.is_empty() {
        return message;
    }
    std::iter::once(message).chain(notes.into_iter().map(|n| format!("  = {n}"))).collect::<Vec<_>>().join("\n")
}

/// A compile error without a source location, with the notes for a PRIVATE
/// entity it names. Called on the thread that compiled, which recorded them.
#[allow(dead_code)]
pub(crate) fn annotate(error: anyhow::Error) -> anyhow::Error {
    let message = crate::source_diagnostics::user_names(&format!("{error:?}"));
    if notes(&message).is_empty() {
        return error;
    }
    anyhow::anyhow!(with_notes(message))
}

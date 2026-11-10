// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Style warnings after Clerman and Spector, "Modern Fortran: Style and
//! Usage". All are opt-in, and `-Wstyle` enables them together with the
//! other style checks; `logical-comparison` is also in -Wextra.
//!
//! - `old-relational`: `.lt.`, `.le.`, `.eq.`, `.ne.`, `.gt.`, `.ge.`.
//! - `logical-comparison`: comparisons with `.true.` or `.false.`.
//! - `missing-double-colon`: type declarations without `::`.
//! - `labeled-do`: DO loops ended by a labeled statement.
//! - `double-precision`: DOUBLE PRECISION and DOUBLE COMPLEX.
//! - `bare-end`: END statements of program units and derived types
//!   without the name.
//! - `keyword-case`: keywords written in more than one case in a file.
//! - `tabs` and `line-length` (132 characters, or N with
//!   `-Wline-length=N`): free-form source layout.
//! - `unnamed-exit`: EXIT and CYCLE without a construct name.
//! - `goto`: GO TO statements.
//! - `obsolescent`: obsolescent and deleted features, such as computed
//!   GO TO, arithmetic IF, COMMON, EQUIVALENCE, ENTRY and alternate returns.
//! - `stop-in-procedure`: STOP in a function or subroutine.
//! - `assumed-size`: assumed-size dummy arrays.
//! - `redundant-implicit`: IMPLICIT NONE in a procedure or submodule whose
//!   host's IMPLICIT NONE already applies. Interface bodies do not inherit
//!   the host's implicit rules and are not reported.
//! - `function-result`: a FUNCTION statement with the result type in its
//!   prefix, as in `real function f(x)`, or without a RESULT clause; the
//!   preferred form is `function f(x) result(y)` with `y` declared in the
//!   body.
//! - `could-be-one-line-if` and its reverse, `one-line-if`, neither in
//!   -Wstyle: an IF block holding one action statement, which a logical
//!   IF can express on one line, and a logical IF, which can be written as
//!   a block.
//! - `could-be-intent-in`, in -Wextra: an INTENT(INOUT) dummy argument
//!   that nothing in the procedure can modify. Anything that might modify
//!   it counts: assignment, READ, ALLOCATE and other statements naming it,
//!   passing it to a procedure that is not intrinsic, and the control list
//!   of a WRITE. POINTER and TARGET dummies are not reported, nor
//!   polymorphic ones, since an interface the procedure must match may
//!   require INOUT.
//! - `unused-only`, in -Wextra: names in a USE ONLY list that the scope
//!   never uses. Module-level USE is checked only in modules whose default
//!   accessibility is PRIVATE, since a public module may re-export them.
//! - `name-case`: a name written in a case different from its declaration,
//!   or from its first appearance if it is not declared, within a scope.
//!   Host-associated and USE-associated names are compared with their
//!   declarations, and components with their declarations in types.
//!
//! The checks read the tokens of each statement and a stack of open
//! constructs shared with -Windentation.
use crate::indentation_warnings::{classify, words, Kind, Role};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::ops::Range;

pub(crate) const CATEGORIES: &[&str] = &[
    "old-relational", "logical-comparison", "missing-double-colon", "labeled-do", "double-precision", "bare-end",
    "keyword-case", "tabs", "line-length", "unnamed-exit", "goto", "obsolescent", "stop-in-procedure", "assumed-size",
    "redundant-implicit", "name-case", "function-result", "could-be-one-line-if", "one-line-if",
    "could-be-intent-in", "unused-only", "semicolon", "trailing-whitespace", "declaration-order", "argument-order",
];

struct Warning {
    category: &'static str,
    message: String,
    labels: Vec<(Range<usize>, String)>,
    related: Vec<(Range<usize>, String)>,
    notes: Vec<String>,
}

impl Warning {
    fn new(category: &'static str, message: String, span: Range<usize>, label: impl Into<String>) -> Self {
        Warning { category, message, labels: vec![(span, label.into())], related: vec![], notes: vec![] }
    }
    fn note(mut self, note: impl Into<String>) -> Self { self.notes.push(note.into()); self }
}

/// A program unit or derived type: what its END statement should name.
struct Unit {
    kind: String,
    name: String,
    args: Vec<String>,
    bind: bool,
}

struct Frame {
    kind: Kind,
    label: Option<u64>,
    span: Range<usize>,
    /// The construct name of a DO loop.
    name: Option<String>,
    unit: Option<Unit>,
    /// EXIT and CYCLE statements without a construct name.
    bare: Vec<(Range<usize>, String)>,
    /// For a program unit: the IMPLICIT NONE rules in effect (TYPE and
    /// EXTERNAL bits) with the statement giving them, the scope of its
    /// names, the scopes of modules it uses without ONLY, and whether it
    /// inherits from the unit enclosing it.
    implicit: (u8, Option<Range<usize>>),
    scope: Option<usize>,
    uses: Vec<usize>,
    inherits: bool,
    /// For a function without RESULT: its name, and the warning that
    /// collects the uses of the name as the result variable.
    rename: Option<(String, usize)>,
    usage: Usage,
}

/// For a program unit: its INTENT(INOUT) dummies, the names something may
/// modify, its arrays, its USE ONLY names, and whether it is a module with
/// default PRIVATE accessibility.
#[derive(Default)]
struct Usage {
    inout: Vec<(String, Range<usize>)>,
    modified: std::collections::HashSet<String>,
    arrays: std::collections::HashSet<String>,
    only: Vec<Imported>,
    private: bool,
    order: Order,
}

/// For a procedure: its declarations and the attributes of its dummies, for
/// -Wdeclaration-order and -Wargument-order.
#[derive(Default)]
struct Order {
    /// The function result's name, and whether a RESULT clause gives it.
    result: Option<String>,
    result_clause: bool,
    decls: Vec<Declared>,
    attrs: std::collections::HashMap<String, Attributes>,
    /// Whether the executable part has begun.
    started: bool,
    statements: usize,
}

/// An entity of a type declaration statement, or a dummy procedure
/// declared by an interface body.
struct Declared {
    name: String,
    /// The names its declaration references, as bounds, lengths and kinds.
    refs: std::collections::HashSet<String>,
    span: Range<usize>,
    statement: usize,
    /// The statement's text through `::`, and the entity's text.
    prefix: Option<String>,
    text: String,
}

#[derive(Default)]
struct Attributes {
    intent: Option<String>,
    optional: bool,
    value: bool,
    polymorphic: bool,
}

/// Statements of the specification part, other than type declarations.
const SPECIFICATION: &[&str] = &[
    "use", "import", "implicit", "parameter", "dimension", "save", "external", "intrinsic", "public", "private",
    "data", "format", "common", "equivalence", "namelist", "pointer", "target", "allocatable", "contiguous",
    "volatile", "asynchronous", "protected", "bind", "procedure", "generic", "entry", "include", "enum",
    "enumerator", "final", "sequence", "codimension", "intent", "optional", "value", "module",
];

/// The attributes a statement's words give, up to `end`.
fn attributes_of(w: &[String], end: usize, a: &mut Attributes) {
    let w = &w[..end.min(w.len())];
    for (i, x) in w.iter().enumerate() {
        match x.as_str() {
            "intent" if w.get(i + 1).is_some_and(|y| y == "(") => {
                if let Some(c) = closing_word(w, i + 1) { a.intent = Some(w[i + 2..c].concat()); }
            }
            "optional" => a.optional = true,
            "value" => a.value = true,
            _ => {}
        }
    }
}

/// Record a statement of a procedure's specification part in `o`.
fn order_statement(src: &str, ts: &[Token], w: &[String], args: &[String], o: &mut Order) {
    if o.started { return; }
    let first = w.first().map(String::as_str).unwrap_or("");
    let colon = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon));
    let typed = declaration(w) || (first == "procedure" && colon.is_some());
    if !typed {
        // INTENT, OPTIONAL and VALUE statements.
        if matches!(first, "intent" | "optional" | "value") {
            let at = w.iter().position(|x| x == "::").map_or_else(|| if first == "intent" { closing_word(w, 1).map_or(w.len(), |c| c + 1) } else { 1 }, |c| c + 1);
            for name in w[at.min(w.len())..].iter().filter(|x| is_word(x)) {
                if args.contains(name) { attributes_of(w, at, o.attrs.entry(name.clone()).or_default()); }
            }
        } else if !SPECIFICATION.contains(&first) {
            o.started = true;
        }
        return;
    }
    let entities = match colon { Some(c) => c + 1, None => spec_end(ts) };
    if entities >= ts.len() { return; }
    let word_end = w.iter().position(|x| x == "::").unwrap_or(w.len());
    let common: std::collections::HashSet<String> = ts[..entities].iter()
        .filter(|t| matches!(t.kind, TokenKind::Ident(_))).map(|t| text(src, t).to_ascii_lowercase()).collect();
    let prefix = colon.map(|c| src[ts[0].span.start..ts[c].span.end].split_whitespace().collect::<Vec<_>>().join(" "));
    o.statements += 1;
    for e in top_level(&ts[entities..]) {
        let Some(name_token) = e.first().filter(|t| matches!(t.kind, TokenKind::Ident(_))) else { continue };
        let name = text(src, name_token).to_ascii_lowercase();
        let mut refs = common.clone();
        refs.extend(e[1..].iter().filter(|t| matches!(t.kind, TokenKind::Ident(_))).map(|t| text(src, t).to_ascii_lowercase()));
        refs.remove(&name);
        if args.contains(&name) {
            let a = o.attrs.entry(name.clone()).or_default();
            attributes_of(w, word_end, a);
            a.polymorphic |= first == "class";
        }
        let text = src[e[0].span.start..e[e.len() - 1].span.end].split_whitespace().collect::<Vec<_>>().join(" ");
        o.decls.push(Declared { name, refs, span: name_token.span.clone(), statement: o.statements, prefix: prefix.clone(), text });
    }
}

/// `-Wdeclaration-order`: dummies, then the function result, then locals.
fn declaration_order(unit: &Unit, o: &Order, out: &mut Vec<Warning>) {
    let kind_of = |n: &str| if unit.args.iter().any(|a| a == n) { Some(0) } else if o.result.as_deref() == Some(n) { Some(1) } else { None };
    // Locals that the declarations of dummies or the result need, directly
    // or through other locals, may come first.
    let mut needed: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let locals: std::collections::HashSet<&str> = o.decls.iter().filter(|d| kind_of(&d.name).is_none()).map(|d| d.name.as_str()).collect();
    let mut frontier: Vec<&str> = o.decls.iter().filter(|d| kind_of(&d.name).is_some())
        .flat_map(|d| d.refs.iter().map(String::as_str)).filter(|n| locals.contains(n)).collect();
    while let Some(n) = frontier.pop() {
        if !needed.insert(n) { continue; }
        for d in o.decls.iter().filter(|d| d.name == n) {
            frontier.extend(d.refs.iter().map(String::as_str).filter(|r| locals.contains(r)));
        }
    }
    let rank = |d: &Declared| match kind_of(&d.name) {
        Some(k) => Some(k),
        None if needed.contains(d.name.as_str()) => None,
        None => Some(2),
    };
    let describe = |d: &Declared| match kind_of(&d.name) {
        Some(0) => format!("dummy argument `{}`", d.name),
        Some(_) => format!("function result `{}`", d.name),
        None => format!("local variable `{}`", d.name),
    };
    // The first declaration after one of a later group, in an earlier statement.
    let mut highest: Option<(usize, &Declared)> = None;
    let mut late = None;
    let mut statement = 0;
    let mut pending: Option<(usize, &Declared)> = None;
    for d in &o.decls {
        if d.statement != statement {
            if let Some((r, p)) = pending.take() { if highest.is_none_or(|(h, _)| r > h) { highest = Some((r, p)); } }
            statement = d.statement;
        }
        let Some(r) = rank(d) else { continue };
        if let Some((h, earlier)) = highest {
            if r < h && late.is_none() { late = Some((d, earlier)); }
        }
        if pending.is_none_or(|(p, _)| r > p) { pending = Some((r, d)); }
    }
    if let Some((d, earlier)) = late {
        let what = describe(d);
        let message = format!("{what} is declared after {} [declaration-order]", describe(earlier));
        let mut w = Warning::new("declaration-order", message, d.span.clone(), "declared here");
        w.related.push((earlier.span.clone(), "declared earlier".to_owned()));
        out.push(w.note(if o.result.is_some() {
            "declare the dummy arguments first, then the function result, then local variables; a local needed in their declarations, such as a named constant used as a bound or kind, may come first"
        } else {
            "declare the dummy arguments before local variables; a local needed in their declarations, such as a named constant used as a bound or kind, may come first"
        }));
    }
    // A statement declaring entities of more than one group.
    let mut mixed = o.decls.iter().filter_map(|d| rank(d).map(|r| (d.statement, r, d))).collect::<Vec<_>>();
    mixed.sort_by_key(|(s, r, _)| (*s, *r));
    for group in mixed.chunk_by(|a, b| a.0 == b.0) {
        if group.first().map(|g| g.1) == group.last().map(|g| g.1) { continue; }
        let (a, b) = (group[0].2, group[group.len() - 1].2);
        let message = format!("one declaration declares {} and {} [declaration-order]", describe(a), describe(b));
        let mut w = Warning::new("declaration-order", message, b.span.clone(), "declared with it");
        w.related.push((a.span.clone(), describe(a)));
        if let Some(prefix) = &a.prefix {
            let mut parts = vec![];
            for r in 0..3 {
                let names: Vec<&str> = o.decls.iter().filter(|d| d.statement == a.statement && rank(d) == Some(r)).map(|d| d.text.as_str()).collect();
                if !names.is_empty() { parts.push(format!("`{prefix} {}`", names.join(", "))); }
            }
            w = w.note(format!("suggestion: split it into {}", parts.join(" and ")));
        }
        out.push(w.note("declare dummy arguments, the function result and local variables in separate statements"));
        break;
    }
}

/// `-Wargument-order`: required INTENT(IN) arguments, then other required
/// arguments, then OPTIONAL ones. The warning is kept with the procedure's
/// name, since defined assignment and defined I/O are exempt.
fn argument_order(unit: &Unit, o: &Order, header: &Range<usize>) -> Option<Warning> {
    let args: Vec<&String> = unit.args.iter().filter(|a| is_word(a)).collect();
    // The passed-object dummy of a type-bound procedure comes first.
    let skip = usize::from(args.first().is_some_and(|a| o.attrs.get(*a).is_some_and(|x| x.polymorphic)));
    let args = &args[skip..];
    let attrs = |a: &str| o.attrs.get(a);
    let optional = |a: &str| attrs(a).is_some_and(|x| x.optional);
    let input = |a: &str| attrs(a).is_some_and(|x| x.intent.as_deref() == Some("in") || (x.intent.is_none() && x.value));
    let output = |a: &str| attrs(a).is_some_and(|x| matches!(x.intent.as_deref(), Some("out" | "inout")));
    let mut problems = vec![];
    if let Some(i) = (0..args.len()).find(|&i| !optional(args[i]) && input(args[i]) && args[..i].iter().any(|a| !optional(a) && output(a))) {
        let before = args[..i].iter().find(|a| !optional(a) && output(a)).unwrap();
        problems.push(format!("INTENT(IN) argument `{}` follows output argument `{before}`", args[i]));
    }
    if let Some(i) = (0..args.len()).find(|&i| !optional(args[i]) && args[..i].iter().any(|a| optional(a))) {
        let before = args[..i].iter().find(|a| optional(a)).unwrap();
        problems.push(format!("required argument `{}` follows OPTIONAL argument `{before}`", args[i]));
    }
    if problems.is_empty() { return None; }
    // Required inputs, other required arguments, then OPTIONAL ones, each
    // in its original order. A dummy without INTENT stays with the inputs
    // unless it follows an output.
    let mut keyed: Vec<(u8, &String)> = vec![];
    let mut seen_output = false;
    for a in args {
        seen_output |= !optional(a) && output(a);
        let key = if optional(a) { 2 } else if input(a) || (!output(a) && !seen_output) { 0 } else { 1 };
        keyed.push((key, a));
    }
    keyed.sort_by_key(|(k, _)| *k);
    let mut list: Vec<&str> = unit.args[..skip].iter().map(String::as_str).collect();
    list.extend(keyed.iter().map(|(_, a)| a.as_str()));
    let result = match (&o.result, o.result_clause) { (Some(r), true) => format!(" result({r})"), _ => String::new() };
    let message = format!("arguments of {} `{}` are out of order: {} [argument-order]", unit.kind, unit.name, problems[0]);
    let mut w = Warning::new("argument-order", message, header.clone(), "argument list");
    if problems.len() > 1 { w = w.note(problems[1].clone()); }
    w = w.note(format!("suggestion: {} {}({}){result}", unit.kind, unit.name, list.join(", ")));
    Some(w.note("put required INTENT(IN) arguments first, then the other required arguments, then OPTIONAL ones; positional calls must change to match, while calls by keyword need not"))
}

/// A name imported by USE ONLY.
struct Imported {
    name: String,
    span: Range<usize>,
    used: bool,
    /// The USE statement: its location, its module, its text up to the ONLY
    /// list, and its ONLY items as written.
    statement: Range<usize>,
    module: String,
    prefix: String,
    items: Vec<String>,
}

/// Statements that may define the variables they name.
const DEFINING: &[&str] = &[
    "call", "read", "allocate", "deallocate", "nullify", "open", "close", "inquire", "wait", "flush", "rewind",
    "backspace", "endfile", "forall", "where", "associate", "select", "selecttype", "selectrank", "change",
    "event", "lock", "unlock", "form", "sync", "namelist", "data", "equivalence", "common",
];

fn is_word(x: &str) -> bool {
    x.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
}

/// The names `w` may modify, for -Wcould-be-intent-in; `arrays` are the
/// unit's array names, whose subscripts are not arguments.
fn modified_names(w: &[String], arrays: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    let a = action(w);
    let first = a.first().map(String::as_str).unwrap_or("");
    if declaration(w) || matches!(first, "use" | "implicit" | "import" | "intent" | "dimension" | "public" | "private") { return; }
    if DEFINING.contains(&first) {
        out.extend(a.iter().filter(|x| is_word(x)).cloned());
        return;
    }
    // DO defines its loop variables, as in `do i = 1, n` and DO CONCURRENT.
    if first == "do" {
        for (k, x) in a.iter().enumerate() {
            if x == "=" && k > 0 && is_word(&a[k - 1]) { out.insert(a[k - 1].clone()); }
        }
    }
    // WRITE defines what its control list names, such as an internal file.
    if first == "write" && a.get(1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(a, 1) { out.extend(a[2..c].iter().filter(|x| is_word(x)).cloned()); }
    }
    // An assignment, or a pointer assignment, defines the root of its target.
    let mut depth = 0;
    for x in a {
        match x.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth -= 1,
            "=" | "=>" if depth == 0 && first != "do" => { if is_word(first) { out.insert(first.to_owned()); } break; }
            _ => {}
        }
    }
    // Arguments of procedures that are not intrinsic, and the object of a
    // type-bound procedure.
    for (i, x) in a.iter().enumerate() {
        if x != "(" || i == 0 { continue; }
        let callee = &a[i - 1];
        if !is_word(callee) || arrays.contains(callee) || crate::intrinsics::purity(callee).is_some()
            || matches!(callee.as_str(), "if" | "while" | "case" | "print" | "write" | "kind" | "len") { continue; }
        if let Some(c) = closing_word(a, i) { out.extend(a[i + 1..c].iter().filter(|x| is_word(x)).cloned()); }
        // `obj%method(...)`: the object is passed too.
        let mut j = i - 1;
        while j >= 2 && a[j - 1] == "%" { j -= 2; }
        if j < i - 1 { out.insert(a[j].clone()); }
    }
}

/// The INTENT(INOUT) dummies a declaration statement declares, and its arrays.
fn intent_declarations(src: &str, ts: &[Token], w: &[String], args: &[String], usage: &mut Usage) {
    let colon = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon));
    let typed = declaration(w);
    let attributes = |name: &str| colon.is_some_and(|c| ts[..c].iter().any(|t| text(src, t).eq_ignore_ascii_case(name)));
    let entities = match colon { Some(c) => c + 1, None if typed => spec_end(ts), None => return };
    let shared_shape = attributes("dimension");
    let inout = attributes("inout") || ts[..entities].iter().zip(ts[..entities].iter().skip(1)).any(|(a, b)|
        text(src, a).eq_ignore_ascii_case("in") && text(src, b).eq_ignore_ascii_case("out"));
    // A pointer to a TARGET dummy may modify it.
    let excluded = attributes("pointer") || attributes("target") || w.first().is_some_and(|x| x == "class");
    for e in top_level(&ts[entities.min(ts.len())..]) {
        let Some(first) = e.first() else { continue };
        let name = text(src, first).to_ascii_lowercase();
        if typed && (shared_shape || matches!(e.get(1).map(|t| &t.kind), Some(TokenKind::LParen))) { usage.arrays.insert(name.clone()); }
        if inout && !excluded && args.contains(&name) { usage.inout.push((name, first.span.clone())); }
    }
}

/// Record a USE ONLY statement's names in `usage`.
fn only_names(src: &str, ts: &[Token], w: &[String], span: Range<usize>, usage: &mut Usage) {
    if !w.iter().any(|x| x == "only") { return; }
    let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::Colon)) else { return };
    let module = ts.iter().skip(1).find(|t| matches!(t.kind, TokenKind::Ident(_)) && !text(src, t).eq_ignore_ascii_case("intrinsic"))
        .map(|t| text(src, t).to_owned()).unwrap_or_default();
    let items: Vec<&[Token]> = top_level(&ts[colon + 1..]).into_iter().filter(|i| !i.is_empty()).collect();
    let prefix = src[ts[0].span.start..ts[colon].span.end].split_whitespace().collect::<Vec<_>>().join(" ").replace("& ", "");
    let written: Vec<String> = items.iter().map(|i| src[i[0].span.start..i[i.len() - 1].span.end].split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    for item in &items {
        // Operators and assignment cannot be checked by name.
        if item.iter().any(|t| matches!(text(src, t).to_ascii_lowercase().as_str(), "operator" | "assignment")) { continue; }
        let Some(local) = item.first().filter(|t| matches!(t.kind, TokenKind::Ident(_))) else { continue };
        usage.only.push(Imported {
            name: text(src, local).to_ascii_lowercase(), span: local.span.clone(), used: false,
            statement: span.clone(), module: module.clone(), prefix: prefix.clone(), items: written.clone(),
        });
    }
}

/// Report a closed unit's INOUT dummies that nothing modifies and its
/// unused USE ONLY names.
fn usage_warnings(f: &Frame, out: &mut Vec<Warning>) {
    let Some(unit) = &f.unit else { return };
    for (name, span) in &f.usage.inout {
        if f.usage.modified.contains(name) { continue; }
        out.push(Warning::new("could-be-intent-in",
            format!("dummy argument `{name}` of {} `{}` is INTENT(INOUT) but is never modified [could-be-intent-in]", unit.kind, unit.name),
            span.clone(), "could be INTENT(IN)")
            .note("INTENT(IN) documents that the procedure only reads it, and lets the compiler check that")
            .note("keep INOUT if an interface the procedure must match requires it"));
    }
    if unit.kind == "module" && !f.usage.private { return; }
    let mut statements: Vec<Range<usize>> = f.usage.only.iter().map(|i| i.statement.clone()).collect();
    statements.dedup();
    for statement in statements {
        let items: Vec<&Imported> = f.usage.only.iter().filter(|i| i.statement == statement).collect();
        let unused: Vec<&&Imported> = items.iter().filter(|i| !i.used).collect();
        if unused.is_empty() { continue; }
        let names: Vec<String> = unused.iter().map(|i| format!("`{}`", i.name)).collect();
        let module = &items[0].module;
        let message = if names.len() == 1 {
            format!("{} is imported from `{module}` but never used [unused-only]", names[0])
        } else {
            format!("{} are imported from `{module}` but never used [unused-only]", names.join(", "))
        };
        let mut w = Warning { category: "unused-only", message,
            labels: unused.iter().map(|i| (i.span.clone(), "never used".to_owned())).collect(), related: vec![], notes: vec![] };
        let kept: Vec<&String> = items[0].items.iter().filter(|text| {
            let local = text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or("").to_ascii_lowercase();
            !unused.iter().any(|i| i.name == local)
        }).collect();
        w = w.note(if kept.is_empty() {
            "suggestion: remove this USE statement".to_owned()
        } else {
            format!("suggestion: {} {}", items[0].prefix, kept.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
        });
        out.push(w);
    }
}

/// The IMPLICIT NONE rules a statement gives

/// A name's spelling where it is declared, or first appears.
struct Entry {
    spelling: String,
    span: Range<usize>,
    declared: bool,
    /// Other spellings, where they appear.
    others: Vec<(Range<usize>, String)>,
}

/// Words that the lexer can leave as names but are keywords, attribute
/// names or specifiers; their case is -Wkeyword-case's concern.
const KEYWORD_NAMES: &[&str] = &[
    "in", "out", "inout", "intent", "allocatable", "pointer", "target", "optional", "value", "save", "parameter",
    "public", "private", "protected", "volatile", "asynchronous", "contiguous", "external", "intrinsic", "bind", "c",
    "name", "len", "kind", "only", "none", "result", "recursive", "pure", "elemental", "impure", "simple", "then",
    "else", "elseif", "end", "do", "while", "concurrent", "if", "select", "case", "default", "where", "forall",
    "associate", "block", "critical", "exit", "cycle", "stop", "error", "return", "call", "go", "to", "goto",
    "continue", "format", "open", "close", "read", "write", "print", "inquire", "rewind", "backspace", "endfile",
    "flush", "wait", "deferred", "nopass", "pass", "non_overridable", "abstract", "extends", "sequence", "import",
    "implicit", "use", "contains", "module", "procedure", "function", "subroutine", "program", "submodule",
    "interface", "operator", "assignment", "generic", "final", "enum", "enumerator", "type", "class", "is", "real",
    "integer", "complex", "logical", "character", "double", "precision", "allocate", "deallocate", "nullify", "data",
    "common", "equivalence", "namelist", "entry", "sync", "all", "images", "memory", "lock", "unlock", "event",
    "post", "team", "change", "rank", "local", "local_init", "shared", "reduce", "dimension", "codimension",
    "non_intrinsic", "doubleprecision", "doublecomplex", "selectcase", "selecttype", "enddo", "endif",
];

/// Scopes of names: 0 holds components, 1 module and program names, 2 names
/// outside any program unit; then one per program unit.
struct Names {
    scopes: Vec<std::collections::HashMap<String, Entry>>,
    /// Each module's scope and IMPLICIT NONE rules, for USE and submodules.
    modules: std::collections::HashMap<String, (usize, (u8, Option<Range<usize>>))>,
    ambiguous: std::collections::HashSet<String>,
}

const COMPONENTS: usize = 0;
const UNITS: usize = 1;
const OUTSIDE: usize = 2;

impl Names {
    fn new() -> Self {
        Names { scopes: (0..3).map(|_| Default::default()).collect(), modules: Default::default(), ambiguous: Default::default() }
    }
    fn scope(&mut self) -> usize {
        self.scopes.push(Default::default());
        self.scopes.len() - 1
    }
    /// Record `spelling` of a name in `scope`: its reference spelling if
    /// it is new there, otherwise compared with that.
    fn note(&mut self, scope: usize, spelling: &str, span: Range<usize>, declared: bool) {
        let key = spelling.to_ascii_lowercase();
        if scope == COMPONENTS && self.ambiguous.contains(&key) { return; }
        match self.scopes[scope].get_mut(&key) {
            Some(e) if e.spelling != spelling => {
                if scope == COMPONENTS && declared && e.declared {
                    // Components of different types may be spelled differently.
                    self.ambiguous.insert(key.clone());
                    self.scopes[scope].remove(&key);
                } else if declared && !e.declared && e.others.is_empty() {
                    // A declaration after uses becomes the reference.
                    let old = std::mem::replace(e, Entry { spelling: spelling.to_owned(), span, declared, others: vec![] });
                    e.others.push((old.span, old.spelling));
                } else {
                    e.others.push((span, spelling.to_owned()));
                }
            }
            Some(e) => {
                if declared && !e.declared { e.declared = true; e.span = span; }
            }
            None => { self.scopes[scope].insert(key, Entry { spelling: spelling.to_owned(), span, declared, others: vec![] }); }
        }
    }
    /// The scopes a name in the innermost unit can come from, innermost first.
    fn chain(stack: &[Frame]) -> Vec<usize> {
        let mut out = vec![];
        for f in stack.iter().rev() {
            let Some(scope) = f.scope else { continue };
            out.push(scope);
            out.extend(f.uses.iter().copied());
            if !f.inherits { return out; }
        }
        out.push(OUTSIDE);
        out
    }
    /// A use of a name: compared in the scope that has it, or recorded in
    /// the innermost scope.
    fn resolve(&mut self, stack: &[Frame], spelling: &str, span: Range<usize>) {
        let key = spelling.to_ascii_lowercase();
        let chain = Names::chain(stack);
        let scope = chain.iter().copied().find(|s| self.scopes[*s].contains_key(&key)).unwrap_or(chain[0]);
        self.note(scope, spelling, span, false);
    }
}

fn name_of(src: &str, t: &Token) -> Option<String> {
    match &t.kind {
        TokenKind::Ident(_) => {
            let written = text(src, t);
            (!KEYWORD_NAMES.contains(&written.to_ascii_lowercase().as_str())).then(|| written.to_owned())
        }
        _ => None,
    }
}

/// The names in a statement: declarations define names in the innermost
/// scope, or in the components for a type; other names are uses. Keyword
/// arguments and specifiers are skipped.
fn statement_names(src: &str, ts: &[Token], w: &[String], stack: &mut [Frame], names: &mut Names) {
    let mut declared = vec![false; ts.len()];
    if declaration(w) {
        let entities = match ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)) { Some(c) => c + 1, None => spec_end(ts) };
        let mut at = entities;
        for e in top_level(&ts[entities.min(ts.len())..]) {
            if !e.is_empty() && at < ts.len() { declared[at] = true; }
            at += e.len() + 1;
        }
    }
    let in_type = stack.last().is_some_and(|f| f.kind == Kind::Type);
    // USE: the module, and the names imported from it.
    if w.first().is_some_and(|x| x == "use") {
        let module = ts.iter().skip(1).find_map(|t| name_of(src, t).map(|n| (n, t.span.clone())));
        let Some((module, module_span)) = module else { return };
        names.note(UNITS, &module, module_span, false);
        let source = names.modules.get(&module.to_ascii_lowercase()).map(|m| m.0);
        let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::Colon)) else {
            if let (Some(source), Some(f)) = (source, stack.iter_mut().rev().find(|f| f.scope.is_some())) { f.uses.push(source); }
            return;
        };
        let target = Names::chain(stack)[0];
        for item in top_level(&ts[colon + 1..]) {
            let idents: Vec<(String, Range<usize>)> = item.iter().filter_map(|t| name_of(src, t).map(|n| (n, t.span.clone()))).collect();
            let renamed = item.iter().any(|t| matches!(t.kind, TokenKind::Arrow));
            match (renamed, idents.as_slice()) {
                (true, [(local, l), (remote, r)]) => {
                    if let Some(source) = source { names.note(source, remote, r.clone(), false); }
                    names.note(target, local, l.clone(), true);
                }
                (false, [(name, span)]) => {
                    // Later uses compare with the module's spelling.
                    let found = source.and_then(|s| names.scopes[s].get(&name.to_ascii_lowercase()).map(|e| (s, e.spelling.clone(), e.span.clone())));
                    match found {
                        Some((source, spelling, at)) => {
                            names.note(source, name, span.clone(), false);
                            names.note(target, &spelling, at, true);
                        }
                        None => names.note(target, name, span.clone(), true),
                    }
                }
                _ => {}
            }
        }
        return;
    }
    let mut depth = 0i32;
    for (k, t) in ts.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            _ => {}
        }
        let Some(spelling) = name_of(src, t) else { continue };
        if depth > 0 && matches!(ts.get(k + 1).map(|n| &n.kind), Some(TokenKind::Eq)) { continue; }
        if k > 0 && matches!(ts[k - 1].kind, TokenKind::Percent) {
            names.note(COMPONENTS, &spelling, t.span.clone(), false);
        } else if declared[k] {
            let scope = if in_type { COMPONENTS } else { Names::chain(stack)[0] };
            names.note(scope, &spelling, t.span.clone(), true);
        } else {
            names.resolve(stack, &spelling, t.span.clone());
        }
    }
}

/// The names in the first statement of a program unit or derived type,
/// after its frame is pushed: the unit's own name in the enclosing scope (or
/// with the module and program names), and its dummy arguments and result
/// in its own scope.
fn header_names(src: &str, ts: &[Token], stack: &mut [Frame], names: &mut Names) {
    let Some(top) = stack.last() else { return };
    let Some(unit) = &top.unit else { return };
    let (kind, own, name, unit_kind) = (top.kind, top.scope, unit.name.clone(), unit.kind.clone());
    let outer = &stack[..stack.len() - 1];
    let parent = Names::chain(outer)[0];
    let Some(at) = ts.iter().position(|t| name_of(src, t).is_some_and(|n| n.eq_ignore_ascii_case(&name))) else { return };
    let mut depth = 0i32;
    for (k, t) in ts.iter().enumerate() {
        match t.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => depth -= 1,
            _ => {}
        }
        let Some(spelling) = name_of(src, t) else { continue };
        if depth > 0 && matches!(ts.get(k + 1).map(|n| &n.kind), Some(TokenKind::Eq)) { continue; }
        if k < at {
            // A submodule's parent, or the type of a function result.
            if unit_kind == "submodule" { names.note(UNITS, &spelling, t.span.clone(), false); }
            else { names.resolve(outer, &spelling, t.span.clone()); }
        } else if k == at {
            match (kind, unit_kind.as_str()) {
                (Kind::Unit, "module" | "program" | "submodule" | "block data") => names.note(UNITS, &spelling, t.span.clone(), true),
                _ => {
                    names.note(parent, &spelling, t.span.clone(), true);
                    if let Some(own) = own { names.note(own, &spelling, t.span.clone(), true); }
                }
            }
        } else if kind == Kind::Type {
            names.resolve(outer, &spelling, t.span.clone());
        } else if let Some(own) = own {
            names.note(own, &spelling, t.span.clone(), true);
        }
    }
}

fn name_warnings(names: &Names, out: &mut Vec<Warning>) {
    for (scope, map) in names.scopes.iter().enumerate() {
        for e in map.values().filter(|e| !e.others.is_empty()) {
            let first = &e.others[0].1;
            let what = if scope == COMPONENTS { "component " } else { "" };
            let (verb, where_) = if e.declared { ("is declared as", "declared here") } else { ("is first written", "first written here") };
            let mut w = Warning {
                category: "name-case",
                message: format!("{what}`{first}` {verb} `{}` [name-case]", e.spelling),
                labels: e.others.iter().take(5).map(|(s, _)| (s.clone(), format!("write `{}`", e.spelling))).collect(),
                related: vec![(e.span.clone(), where_.to_owned())],
                notes: vec![],
            };
            if e.others.len() > 5 { w = w.note(format!("{} more uses are spelled differently", e.others.len() - 5)); }
            out.push(w.note("Fortran ignores case, but different spellings of one name read as different names"));
        }
    }
}

/// A FUNCTION statement with the result type in the prefix or without a
/// RESULT clause. Returns the function's name when RESULT is missing, so
/// that uses of the name as the result variable can be labeled.
fn function_result(src: &str, ts: &[Token], w: &[String], out: &mut Vec<Warning>) -> Option<String> {
    let at = w.iter().position(|x| x == "function")?;
    // A separate module procedure takes its characteristics from its interface.
    if w[..at].iter().any(|x| x == "module") { return None; }
    let name = w.get(at + 1)?.clone();
    let has_result = w[at..].iter().any(|x| x == "result");
    let types = ["integer", "real", "complex", "logical", "character", "double", "doubleprecision", "doublecomplex", "type", "class"];
    let function = ts.iter().position(|t| text(src, t).eq_ignore_ascii_case("function"))?;
    let spec = ts[..function].iter().position(|t| types.contains(&text(src, t).to_ascii_lowercase().as_str()));
    let spec_range = spec.map(|i| (i, i + spec_end(&ts[i..function])));
    if spec_range.is_none() && has_result { return None; }
    let spec_text = spec_range.map(|(a, b)| src[ts[a].span.start..ts[b - 1].span.end].to_owned());
    // Assumed length (character(len=*)) is obsolescent and has no RESULT form.
    if spec_text.as_deref().is_some_and(|t| t.replace(' ', "").contains('*')) && spec_text.as_deref().is_some_and(|t| t.to_ascii_lowercase().starts_with("character")) {
        return None;
    }
    let last = ts[ts.len() - 1].span.end;
    let header = first_line(src, ts[0].span.start, last);
    let result_name = if has_result {
        let r = w.iter().position(|x| x == "result")?;
        w.get(r + 2)?.clone()
    } else {
        format!("{name}_result")
    };
    let message = match (spec_text.is_some(), has_result) {
        (true, false) => format!("function `{name}` gives its result type in the FUNCTION statement and has no RESULT clause [function-result]"),
        (true, true) => format!("function `{name}` gives its result type in the FUNCTION statement [function-result]"),
        _ => format!("function `{name}` has no RESULT clause [function-result]"),
    };
    let mut warning = Warning::new("function-result", message, header.clone(), "here");
    if !src[ts[0].span.start..last].contains('\n') {
        let mut rewritten = match spec_range {
            Some((a, b)) => format!("{}{}", &src[ts[0].span.start..ts[a].span.start], &src[ts[b.min(ts.len() - 1)].span.start..last]),
            None => src[ts[0].span.start..last].to_owned(),
        };
        if !has_result { rewritten.push_str(&format!(" result({result_name})")); }
        warning = warning.note(format!("suggestion: {}", rewritten.trim()));
    }
    if let Some(t) = &spec_text { warning = warning.note(format!("then declare `{t} :: {result_name}` with the arguments")); }
    if !has_result { warning = warning.note(format!("and rename `{name}` to `{result_name}` where the body uses it as the result")); }
    out.push(warning.note("declaring the result in the body allows attributes such as ALLOCATABLE and DIMENSION, and RESULT keeps the function's name for calls"));
    (!has_result).then_some(name)
}

/// A statement, for checks of statement sequences.
struct Seen {
    role: Role,
    words: Vec<String>,
    labeled: bool,
    /// The source from the first token after any label to the last token.
    range: Range<usize>,
    /// For an IF ... THEN statement, where THEN begins.
    then: Option<usize>,
}

/// Whether a statement can be the action of a logical IF.
fn action_statement(s: &Seen) -> bool {
    let first = s.words.first().map(String::as_str).unwrap_or("");
    matches!(s.role, Role::Plain) && !s.labeled
        && !matches!(first, "if" | "format" | "entry" | "data" | "implicit" | "use" | "import" | "contains" | "end")
        && !declaration(&s.words)
}

/// Whether the source lines holding `range` contain a comment or `;`.
fn commented(src: &str, range: Range<usize>) -> bool {
    let start = src[..range.start].rfind('\n').map_or(0, |p| p + 1);
    let end = src[range.end..].find('\n').map_or(src.len(), |p| range.end + p);
    src[start..end].split('\n').any(|line| {
        let code = code_part(line.trim_end_matches('\r'));
        code.len() < line.trim_end_matches('\r').len() || code.contains(';')
    })
}

/// IF blocks holding one action statement, and logical IFs.
fn one_line_ifs(src: &str, seen: &[Seen], limit: usize, out: &mut Vec<Warning>) {
    for (i, s) in seen.iter().enumerate() {
        // `if (c) then`, one action statement, `end if`.
        if let (Role::Open(Kind::If, _), Some(then), Some(body), Some(end)) = (&s.role, s.then, seen.get(i + 1), seen.get(i + 2)) {
            let closes = matches!(end.role, Role::Close(Some(Kind::If))) && !end.labeled;
            if closes && construct_name(&s.words).is_none() && action_statement(body)
                && !src[body.range.clone()].contains('\n') && !commented(src, s.range.start..end.range.end) {
                let head = src[s.range.start..then].trim_end();
                let line = format!("{head} {}", &src[body.range.clone()]);
                let indent = column(src, s.range.start) - 1;
                if indent + line.chars().count() <= limit {
                    out.push(Warning::new("could-be-one-line-if",
                        "IF block with a single statement could be a one-line IF [could-be-one-line-if]".to_owned(),
                        s.range.start..then + 4, "here").note(format!("suggestion: {line}")));
                }
            }
        }
        // A logical IF: `if (c) action`, not an arithmetic IF.
        if s.words.first().is_some_and(|x| x == "if") && s.words.get(1).is_some_and(|x| x == "(") && matches!(s.role, Role::Plain) {
            let Some(c) = closing_word(&s.words, 1) else { continue };
            let Some(next) = s.words.get(c + 1) else { continue };
            if next.bytes().all(|b| b.is_ascii_digit()) && s.words.get(c + 2).is_some_and(|x| x == ",") { continue; }
            // The parenthesized condition ends at the matching `)` in the source.
            let text = &src[s.range.clone()];
            let mut depth = 0;
            let mut close = None;
            for (k, ch) in text.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => { depth -= 1; if depth == 0 { close = Some(k); break; } }
                    _ => {}
                }
            }
            let Some(close) = close else { continue };
            let head = &text[..=close];
            let action = text[close + 1..].trim();
            if head.contains(['\'', '"']) || head.contains('\n') { continue; }
            let pad = " ".repeat(column(src, s.range.start) - 1);
            out.push(Warning::new("one-line-if", "one-line IF [one-line-if]".to_owned(), s.range.start..s.range.start + close + 1, "here")
                .note(format!("suggestion:\n{head} then\n{pad}   {action}\n{pad}end if"))
                .note("a block IF leaves room for more statements, and for a breakpoint on the action"));
        }
    }
}

/// The 1-based column of byte `at`.
fn column(src: &str, at: usize) -> usize {
    let start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    src[start..at].chars().count() + 1
}

/// The IMPLICIT NONE rules a statement gives: TYPE (1) and EXTERNAL (2).
fn implicit_none(w: &[String]) -> Option<u8> {
    if !(w.first().is_some_and(|x| x == "implicit") && w.get(1).is_some_and(|x| x == "none")) { return None; }
    let mut bits = 0;
    for x in &w[2..] {
        match x.as_str() { "type" => bits |= 1, "external" => bits |= 2, _ => {} }
    }
    Some(if bits == 0 { 1 } else { bits })
}


fn text<'a>(src: &'a str, t: &Token) -> &'a str { &src[t.span.clone()] }

/// The index of the token closing the parenthesis at `open`.
fn closing(ts: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(open) {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => { depth -= 1; if depth == 0 { return Some(i); } }
            _ => {}
        }
    }
    None
}

/// The index of the word closing the parenthesis at `open`.
fn closing_word(w: &[String], open: usize) -> Option<usize> {
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

/// Split tokens at commas outside parentheses and brackets.
fn top_level(ts: &[Token]) -> Vec<&[Token]> {
    let (mut out, mut depth, mut begin) = (vec![], 0i32, 0);
    for (i, t) in ts.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { out.push(&ts[begin..i]); begin = i + 1; }
            _ => {}
        }
    }
    out.push(&ts[begin..]);
    out
}

/// The words of a logical IF's action, or all the words.
fn action(w: &[String]) -> &[String] {
    if w.first().is_some_and(|x| x == "if") && w.get(1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, 1) {
            if w.get(c + 1).is_some_and(|x| x != "then") { return &w[c + 1..]; }
        }
    }
    w
}

/// Whether `w` is a type declaration statement.
fn declaration(w: &[String]) -> bool {
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let typed = match at(0) {
        "integer" | "real" | "complex" | "logical" | "character" | "doubleprecision" | "doublecomplex" => true,
        "double" => matches!(at(1), "precision" | "complex"),
        "type" | "class" => at(1) == "(",
        _ => false,
    };
    typed && !matches!(at(1), "=" | "=>" | "%") && !matches!(classify(w, None), Role::Open(..))
}

/// The index of the first token after the type specification at the start
/// of `ts`, as in `real(dp)`, `character*8` or `double precision`.
fn spec_end(ts: &[Token]) -> usize {
    let mut i = if matches!(ts[0].kind, TokenKind::KwDouble) { 2 } else { 1 };
    match ts.get(i).map(|t| &t.kind) {
        Some(TokenKind::LParen) => i = closing(ts, i).map_or(ts.len(), |c| c + 1),
        Some(TokenKind::Star) => {
            i += 1;
            if matches!(ts.get(i).map(|t| &t.kind), Some(TokenKind::LParen)) {
                i = closing(ts, i).map_or(ts.len(), |c| c + 1);
            } else {
                i += 1;
            }
        }
        _ => {}
    }
    i.min(ts.len())
}

/// The `*` ending the bounds in parentheses at `open`, for an assumed size.
fn assumed_size(ts: &[Token], open: usize) -> Option<Range<usize>> {
    let close = closing(ts, open)?;
    let last = *top_level(&ts[open + 1..close]).last()?;
    let star = last.last().filter(|t| matches!(t.kind, TokenKind::Star))?;
    let n = last.len();
    (n == 1 || matches!(last[n - 2].kind, TokenKind::Colon)).then(|| star.span.clone())
}

/// The unit opened by `w`: its kind, name, dummy arguments and BIND(C).
fn unit(w: &[String], kind: Kind) -> Option<Unit> {
    let at = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let w = if w.len() > 2 && w[1] == ":" { &w[2..] } else { w };
    let bind = w.iter().any(|x| x == "bind");
    if kind == Kind::Type {
        let name = match w.iter().position(|x| x == "::") { Some(p) => w.get(p + 1)?, None => w.get(1)? };
        return Some(Unit { kind: "type".into(), name: name.clone(), args: vec![], bind });
    }
    let (kind, at_name) = if let Some(p) = w.iter().position(|x| matches!(x.as_str(), "function" | "subroutine")) {
        (w[p].clone(), p + 1)
    } else {
        match (at(0), at(1)) {
            ("module", "procedure") => ("procedure".to_owned(), 2),
            ("program" | "module", _) => (w[0].clone(), 1),
            ("submodule", _) => ("submodule".to_owned(), closing_word(w, 1)? + 1),
            ("block", "data") => ("block data".to_owned(), 2),
            ("blockdata", _) => ("block data".to_owned(), 1),
            _ => return None,
        }
    };
    let name = w.get(at_name).filter(|x| x.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()))?.clone();
    let mut args = vec![];
    if w.get(at_name + 1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, at_name + 1) {
            args = w[at_name + 2..c].iter().filter(|x| x.as_str() != ",").cloned().collect();
        }
    }
    Some(Unit { kind, name, args, bind })
}

fn construct_name(w: &[String]) -> Option<String> {
    (w.len() > 2 && w[1] == ":").then(|| w[0].clone())
}

/// Report the EXIT and CYCLE statements without a construct name of a
/// closed DO loop.
fn unnamed(frame: &Frame, out: &mut Vec<Warning>) {
    if frame.kind != Kind::Do || frame.bare.is_empty() { return; }
    let mut kinds: Vec<&str> = frame.bare.iter().map(|(_, k)| k.as_str()).collect();
    kinds.sort();
    kinds.dedup();
    let what = kinds.iter().map(|k| k.to_ascii_uppercase()).collect::<Vec<_>>().join(" and ");
    let mut w = Warning {
        category: "unnamed-exit",
        message: format!("{what} without a construct name in this DO loop [unnamed-exit]"),
        labels: frame.bare.iter().map(|(s, k)| (s.clone(), format!("`{k}` applies to the innermost DO loop"))).collect(),
        related: vec![(frame.span.clone(), "this loop".to_owned())],
        notes: vec![],
    };
    let k = kinds[0];
    w.notes.push(match &frame.name {
        Some(n) => format!("write `{k} {n}` so that the loop is named where control leaves it"),
        None => format!("name the loop, as in `name: do`, and write `{k} name`; a named EXIT or CYCLE keeps its meaning if loops are added around it"),
    });
    out.push(w);
}

/// The source of a statement's first line, from `start`.
fn first_line(src: &str, start: usize, end: usize) -> Range<usize> {
    let line_end = src[start..].find('\n').map_or(src.len(), |p| start + p).min(end);
    let code = src[start..line_end].trim_end_matches(['\r', ' ', '\t']);
    start..start + code.len().max(1)
}

/// The code of a line before any comment, outside character literals.
fn code_part(line: &str) -> &str {
    let mut quote = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '!') => return &line[..i],
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), _) if q == c => quote = None,
            _ => {}
        }
    }
    line
}

fn relational(kind: &TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::EqEq => "==",
        TokenKind::Ne => "/=",
        TokenKind::Lt => "<",
        TokenKind::Le => "<=",
        TokenKind::Gt => ">",
        TokenKind::Ge => ">=",
        _ => return None,
    })
}

fn statement_checks(src: &str, ts: &[Token], w: &[String], span: Range<usize>, stack: &mut [Frame], out: &mut Vec<Warning>) {
    let a = action(w);
    let at = |i: usize| a.get(i).map(String::as_str).unwrap_or("");
    let find = |word: &str| ts.iter().rev().find(|t| text(src, t).replace([' ', '\t'], "").eq_ignore_ascii_case(word)).map(|t| t.span.clone());
    // Old relational operators, with the statement rewritten.
    let old: Vec<(&Token, &str)> = ts.iter().filter_map(|t| relational(&t.kind).filter(|_| text(src, t).starts_with('.')).map(|r| (t, r))).collect();
    if !old.is_empty() {
        let message = if old.len() == 1 {
            format!("`{}` is the old spelling of `{}` [old-relational]", text(src, old[0].0).to_ascii_lowercase(), old[0].1)
        } else {
            "old spellings of relational operators [old-relational]".to_owned()
        };
        let mut warning = Warning { category: "old-relational", message,
            labels: old.iter().map(|(t, r)| (t.span.clone(), format!("use {r}"))).collect(), related: vec![], notes: vec![] };
        let end = ts.last().map_or(span.end, |t| t.span.end);
        if !src[span.start..end].contains('\n') {
            let mut rewritten = String::new();
            let mut from = span.start;
            for (t, r) in &old {
                rewritten.push_str(&src[from..t.span.start]);
                rewritten.push_str(r);
                from = t.span.end;
            }
            rewritten.push_str(&src[from..end]);
            warning = warning.note(format!("suggestion: {rewritten}"));
        }
        out.push(warning);
    }
    // Comparisons with a LOGICAL literal.
    for (i, t) in ts.iter().enumerate() {
        let Some(positive) = (match t.kind {
            TokenKind::EqEq | TokenKind::Eqv => Some(true),
            TokenKind::Ne | TokenKind::Neqv => Some(false),
            _ => None,
        }) else { continue };
        let literal = |j: Option<usize>| j.and_then(|j| ts.get(j)).filter(|x| matches!(x.kind, TokenKind::True | TokenKind::False));
        let Some(lit) = literal(Some(i + 1)).or_else(|| literal(i.checked_sub(1))) else { continue };
        let truth = matches!(lit.kind, TokenKind::True);
        let lit_text = text(src, lit).to_ascii_lowercase();
        let keep = positive == truth;
        let mut warning = Warning::new("logical-comparison", format!("comparison with `{lit_text}` is redundant [logical-comparison]"),
            t.span.start.min(lit.span.start)..t.span.end.max(lit.span.end),
            if keep { "test the LOGICAL value itself" } else { "test `.not.` of the LOGICAL value" });
        // The other operand, when it is a single name.
        let lone = |j: usize| ts.get(j).filter(|x| matches!(x.kind, TokenKind::Ident(_)));
        let boundary = |j: Option<usize>| j.and_then(|j| ts.get(j)).is_none_or(|x| matches!(x.kind,
            TokenKind::LParen | TokenKind::RParen | TokenKind::Comma | TokenKind::And | TokenKind::Or | TokenKind::Not | TokenKind::KwThen));
        if lit.span.start > t.span.start {
            if let Some(name) = i.checked_sub(1).and_then(lone).filter(|_| boundary(i.checked_sub(2)) && boundary(Some(i + 2))) {
                let name = text(src, name);
                warning = warning.note(format!("suggestion: {}", if keep { name.to_owned() } else { format!(".not. {name}") }));
            }
        }
        if matches!(t.kind, TokenKind::EqEq | TokenKind::Ne) {
            warning = warning.note("`==` and `/=` do not apply to LOGICAL operands in standard Fortran; `.eqv.` and `.neqv.` do");
        }
        out.push(warning);
    }
    // DOUBLE PRECISION and DOUBLE COMPLEX.
    for (i, t) in ts.iter().enumerate() {
        let word = text(src, t).to_ascii_lowercase();
        let next = ts.get(i + 1).map(|n| text(src, n).to_ascii_lowercase());
        let (end, complex) = match (word.as_str(), next.as_deref()) {
            ("double", Some("precision")) => (ts[i + 1].span.end, false),
            ("double", Some("complex")) => (ts[i + 1].span.end, true),
            ("doubleprecision", _) => (t.span.end, false),
            ("doublecomplex", _) => (t.span.end, true),
            _ => continue,
        };
        let (written, use_) = if complex { ("DOUBLE COMPLEX", "complex(dp)") } else { ("DOUBLE PRECISION", "real(dp)") };
        out.push(Warning::new("double-precision", format!("{written} type [double-precision]"), t.span.start..end, format!("use {use_}"))
            .note("with `integer, parameter :: dp = kind(1.0d0)`, or `real64` from ISO_FORTRAN_ENV, one constant sets the precision of the program"));
        break;
    }
    // Declarations without `::`.
    if declaration(w) && !w.iter().any(|x| x == "::") {
        let end = spec_end(ts);
        if end < ts.len() {
            let last = ts[ts.len() - 1].span.end;
            let mut warning = Warning::new("missing-double-colon", "declaration without `::` [missing-double-colon]".to_owned(),
                ts[0].span.start..ts[end - 1].span.end, "add `::` after the type");
            if !src[ts[0].span.start..last].contains('\n') {
                warning = warning.note(format!("suggestion: {} :: {}", &src[ts[0].span.start..ts[end - 1].span.end], &src[ts[end].span.start..last]));
            }
            out.push(warning.note("`::` is required with attributes or initialization; using it always keeps declarations uniform"));
        }
    }
    // The innermost procedure.
    let procedure = stack.iter().rev().find_map(|f| f.unit.as_ref().filter(|_| f.kind == Kind::Unit));
    let in_procedure = procedure.filter(|u| matches!(u.kind.as_str(), "function" | "subroutine" | "procedure"));
    // GO TO and obsolescent or deleted features.
    let obsolete = |message: &str, note: &str| Warning::new("obsolescent", format!("{message} [obsolescent]"), span.clone(), "here").note(note);
    let goto = at(0) == "goto" || at(0) == "go" && at(1) == "to";
    if goto {
        let target = if at(0) == "goto" { at(1) } else { at(2) };
        if target == "(" {
            out.push(obsolete("computed GO TO is obsolescent", "use SELECT CASE"));
        } else if target.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()) {
            out.push(obsolete("assigned GO TO was deleted from the standard", "use SELECT CASE or a procedure"));
        } else {
            let at_goto = find("goto").unwrap_or(span.clone());
            out.push(Warning::new("goto", "GO TO statement [goto]".to_owned(), at_goto, "jumps to a label")
                .note("use a structured construct: EXIT or CYCLE in loops, a named BLOCK left with EXIT for forward jumps, or IF ... ELSE"));
        }
    }
    let first = w.first().map(String::as_str).unwrap_or("");
    if first == "if" && w.get(1).is_some_and(|x| x == "(") {
        if let Some(c) = closing_word(w, 1) {
            if w.get(c + 1).is_some_and(|x| x.bytes().all(|b| b.is_ascii_digit())) && w.get(c + 2).is_some_and(|x| x == ",") {
                out.push(obsolete("arithmetic IF is obsolescent", "use IF ... ELSE IF ... ELSE with comparisons with zero"));
            }
        }
    }
    match (first, w.get(1).map(String::as_str)) {
        ("common", _) => out.push(obsolete("COMMON is obsolescent", "put shared data in a module")),
        ("equivalence", _) => out.push(obsolete("EQUIVALENCE is obsolescent", "use separate variables, TRANSFER for reinterpretation, or pointers")),
        ("entry", _) => out.push(obsolete("ENTRY is obsolescent", "write a separate module procedure for each entry point")),
        ("block", Some("data")) | ("blockdata", _) => out.push(obsolete("BLOCK DATA is obsolescent", "initialize module variables in their declarations")),
        ("forall", _) => out.push(obsolete("FORALL is obsolescent", "use DO CONCURRENT or array assignments")),
        ("pause", _) => out.push(obsolete("PAUSE was deleted from the standard", "use READ to wait for input")),
        ("assign", _) if w.get(2).is_some_and(|x| x == "to") => out.push(obsolete("ASSIGN was deleted from the standard", "use an integer variable and SELECT CASE")),
        ("character", Some("*")) => out.push(obsolete("the CHARACTER*length form is obsolescent", "write character(len=n)")),
        _ => {}
    }
    if at(0) == "return" && a.len() > 1 {
        out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
    }
    if first == "call" && w.windows(3).any(|x| matches!(x[0].as_str(), "(" | ",") && x[1] == "*" && x[2].bytes().all(|b| b.is_ascii_digit())) {
        out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
    }
    if matches!(classify(w, None), Role::Open(Kind::Unit, _)) && w.iter().any(|x| x == "subroutine") {
        if let Some(p) = w.iter().position(|x| x == "subroutine") {
            if w.get(p + 2).is_some_and(|x| x == "(") && closing_word(w, p + 2).is_some_and(|c| w[p + 3..c].iter().any(|x| x == "*")) {
                out.push(obsolete("alternate return is obsolescent", "return a status argument and test it after the call"));
            }
        }
    }
    // STOP in a procedure.
    if at(0) == "stop" {
        if let Some(u) = in_procedure {
            out.push(Warning::new("stop-in-procedure", format!("STOP in {} `{}` ends the whole program [stop-in-procedure]", u.kind, u.name),
                find("stop").unwrap_or(span.clone()), "ends the program")
                .note("return an error status argument so the caller can decide, or use ERROR STOP for an unrecoverable error so the exit status shows the failure"));
        }
    }
    // Assumed-size dummy arrays.
    if let Some(u) = in_procedure.filter(|u| !u.bind) {
        let dimension = first == "dimension";
        if declaration(w) || dimension {
            let colon = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon));
            let (attrs, entities) = match colon {
                Some(c) => (&ts[..c], &ts[c + 1..]),
                None if dimension => (&ts[..0], &ts[1.min(ts.len())..]),
                None => { let e = spec_end(ts); (&ts[..0], &ts[e..]) }
            };
            let shared = attrs.iter().position(|t| text(src, t).eq_ignore_ascii_case("dimension"))
                .filter(|&p| matches!(attrs.get(p + 1).map(|t| &t.kind), Some(TokenKind::LParen)))
                .and_then(|p| assumed_size(attrs, p + 1));
            for e in top_level(entities) {
                let Some(first) = e.first() else { continue };
                let name = text(src, first).to_ascii_lowercase();
                if !u.args.contains(&name) { continue; }
                let star = match e.get(1).map(|t| &t.kind) {
                    Some(TokenKind::LParen) => assumed_size(e, 1),
                    _ => shared.clone(),
                };
                if let Some(star) = star {
                    out.push(Warning::new("assumed-size", format!("dummy argument `{name}` is an assumed-size array [assumed-size]"), star, "assumed size")
                        .note(format!("declare it assumed-shape, as in `{name}(:)`, so that its size is known and bounds can be checked; the procedure then needs an explicit interface, as in a module")));
                }
            }
        }
    }
    // EXIT and CYCLE without a construct name, for the innermost DO loop.
    if matches!(at(0), "exit" | "cycle") && a.len() == 1 {
        let boundary = stack.iter().rposition(|f| f.kind == Kind::Unit).map_or(0, |p| p + 1);
        if let Some(p) = stack[boundary..].iter().rposition(|f| f.kind == Kind::Do) {
            let span = find(at(0)).unwrap_or(span.clone());
            stack[boundary + p].bare.push((span, at(0).to_owned()));
        }
    }
}

/// The END statement `w` of `unit` without its name.
fn bare_end(src: &str, ts: &[Token], w: &[String], unit: &Unit, out: &mut Vec<Warning>) {
    if w.last().is_some_and(|x| *x == unit.name) { return; }
    let span = first_line(src, ts[0].span.start, ts[ts.len() - 1].span.end);
    out.push(Warning::new("bare-end", format!("END statement without the name of {} `{}` [bare-end]", unit.kind, unit.name), span,
        format!("write `end {} {}`", unit.kind, unit.name))
        .note("naming the unit at its END lets the compiler check that the END closes the unit intended"));
}

/// The keyword tokens of a statement in keyword positions: the leading
/// keywords, after any construct name, those before `::`, and a final THEN.
/// Names spelled like keywords elsewhere in a statement are not counted.
fn keyword_positions(ts: &[Token], out: &mut Vec<Token>) {
    let keyword = |t: &Token| format!("{:?}", t.kind).starts_with("Kw");
    let start = if ts.len() > 2 && matches!(ts[1].kind, TokenKind::Colon) && matches!(ts[0].kind, TokenKind::Ident(_)) { 2 } else { 0 };
    out.extend(ts[start..].iter().take(3).take_while(|t| keyword(t)).cloned());
    if let Some(colon) = ts.iter().position(|t| matches!(t.kind, TokenKind::DColon)) {
        out.extend(ts[start..colon].iter().skip(1).filter(|t| keyword(t)).cloned());
    }
    if let Some(last) = ts.last().filter(|t| matches!(t.kind, TokenKind::KwThen)) {
        if ts.len() > 1 { out.push(last.clone()); }
    }
}

fn keyword_case(src: &str, tokens: &[Token], out: &mut Vec<Warning>) {
    fn style(t: &str) -> &'static str {
        if !t.bytes().any(|b| b.is_ascii_uppercase()) { "lowercase" }
        else if !t.bytes().any(|b| b.is_ascii_lowercase()) { "uppercase" }
        else if t.split_whitespace().all(|w| w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && !w.chars().skip(1).any(|c| c.is_ascii_uppercase())) { "capitalized" }
        else { "mixed-case" }
    }
    let mut uses: Vec<(&'static str, Range<usize>)> = vec![];
    for t in tokens {
        let written = text(src, t);
        if written.len() < 2
            || !written.bytes().all(|b| b.is_ascii_alphabetic() || b == b' ' || b == b'_') { continue; }
        uses.push((style(written), t.span.clone()));
    }
    let mut counts: Vec<(&str, usize)> = vec![];
    for (s, _) in &uses {
        match counts.iter_mut().find(|(c, _)| c == s) { Some((_, n)) => *n += 1, None => counts.push((s, 1)) }
    }
    if counts.len() < 2 { return; }
    // The most common case, preferring the first written on ties.
    let most = counts.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let main = counts.iter().find(|(_, n)| *n == most).map(|(s, _)| *s).unwrap_or("lowercase");
    let summary = counts.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
    let others: Vec<&(&str, Range<usize>)> = uses.iter().filter(|(s, _)| *s != main).collect();
    let mut warning = Warning {
        category: "keyword-case",
        message: format!("keywords are written in more than one case: {summary} [keyword-case]"),
        labels: others.iter().take(5).map(|(s, span)| (span.clone(), format!("{s}; most keywords are {main}"))).collect(),
        related: vec![], notes: vec![],
    };
    if others.len() > 5 { warning = warning.note(format!("{} more keywords are not {main}", others.len() - 5)); }
    out.push(warning.note("write keywords in one case throughout the file"));
}

/// A summary warning for a file, labeling the first five places.
fn summary(category: &'static str, message: String, places: &[Range<usize>], label: &str, more: &str, note: &str) -> Warning {
    let mut w = Warning { category, message: format!("{message} [{category}]"),
        labels: places.iter().take(5).map(|s| (s.clone(), label.to_owned())).collect(), related: vec![], notes: vec![] };
    if places.len() > 5 { w = w.note(format!("{} more {more}", places.len() - 5)); }
    w.note(note)
}

fn lines(n: usize) -> String { format!("{n} line{}", if n == 1 { "" } else { "s" }) }

/// Lines with more than one statement, apart from a CASE, TYPE IS, CLASS IS
/// or RANK selector followed by one statement.
fn semicolons(src: &str, seen: &[Seen], out: &mut Vec<Warning>) {
    let mut places = vec![];
    let mut k = 0;
    while k < seen.len() {
        let mut end = k + 1;
        while end < seen.len() && !src[seen[end - 1].range.end..seen[end].range.start].contains('\n') { end += 1; }
        let line = &seen[k..end];
        let branch = line.len() == 2 && line[0].role == Role::Case;
        if line.len() > 1 && !branch {
            for pair in line.windows(2) {
                let gap = pair[0].range.end..pair[1].range.start;
                if let Some(p) = src[gap.clone()].find(';') { places.push(gap.start + p..gap.start + p + 1); }
            }
        }
        k = end;
    }
    if places.is_empty() { return; }
    let n = places.iter().map(|p| src[..p.start].matches('\n').count()).collect::<std::collections::BTreeSet<_>>().len();
    out.push(summary("semicolon", format!("{} {} more than one statement", lines(n), if n == 1 { "has" } else { "have" }),
        &places, "statement separator", "separators",
        "write one statement per line; a CASE, TYPE IS, CLASS IS or RANK selector followed by one statement is not reported"));
}

fn layout(src: &str, own_len: usize, tabs: bool, limit: Option<usize>, out: &mut Vec<Warning>) {
    let (mut trailing, mut ending) = (vec![], vec![]);
    let (mut tabbed, mut long) = (vec![], vec![]);
    let mut start = 0;
    for line in src[..own_len.min(src.len())].split('\n') {
        let line_text = line.trim_end_matches('\r');
        let kept = line_text.trim_end_matches([' ', '\t']);
        if kept.len() < line_text.len() { trailing.push(start + kept.len()..start + line_text.len()); }
        let code = code_part(line_text).trim_end();
        if code.ends_with(';') && !code.trim_end_matches(';').trim_end().is_empty() {
            ending.push(start + code.len() - 1..start + code.len());
        }
        if tabs {
            if let Some(p) = code_part(line_text).find('\t') { tabbed.push(start + p..start + p + 1); }
        }
        if let Some(limit) = limit {
            let chars = line_text.chars().count();
            if chars > limit {
                let from = line_text.char_indices().nth(limit).map_or(line_text.len(), |(i, _)| i);
                long.push((start + from..start + line_text.len(), chars));
            }
        }
        start += line.len() + 1;
    }
    if !trailing.is_empty() {
        out.push(summary("trailing-whitespace", format!("{} {} trailing whitespace", lines(trailing.len()), if trailing.len() == 1 { "has" } else { "have" }),
            &trailing, "trailing whitespace", "lines have trailing whitespace", "remove spaces and tabs at the ends of lines"));
    }
    if !ending.is_empty() {
        out.push(summary("semicolon", format!("{} {} with a semicolon", lines(ending.len()), if ending.len() == 1 { "ends" } else { "end" }),
            &ending, "trailing semicolon", "lines end with a semicolon", "a semicolon only separates statements on one line; remove it"));
    }
    if !tabbed.is_empty() {
        let n = tabbed.len();
        let mut w = Warning { category: "tabs",
            message: format!("{n} line{} contain{} tab characters [tabs]", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" }),
            labels: tabbed.iter().take(5).map(|s| (s.clone(), "tab".to_owned())).collect(), related: vec![], notes: vec![] };
        if n > 5 { w = w.note(format!("{} more lines contain tabs", n - 5)); }
        out.push(w.note("tab is not in the Fortran character set, and editors and compilers expand it differently; indent with spaces"));
    }
    if let (false, Some(limit)) = (long.is_empty(), limit) {
        let n = long.len();
        let mut w = Warning { category: "line-length",
            message: format!("{n} line{} longer than {limit} characters [line-length]", if n == 1 { " is" } else { "s are" }),
            labels: long.iter().take(5).map(|(s, c)| (s.clone(), format!("{c} characters"))).collect(), related: vec![], notes: vec![] };
        if n > 5 { w = w.note(format!("{} more lines are too long", n - 5)); }
        if limit == 132 {
            w = w.note("132 characters is the free-form limit before Fortran 2023; gfortran rejects longer lines without -ffree-line-length-none");
        }
        out.push(w.note("continue long statements with `&`"));
    }
}

fn analyze(src: &str, tokens: &[Token], own_len: usize, fixed_form: bool, line_limit: Option<usize>, tabs: bool) -> Vec<Warning> {
    let mut out = vec![];
    let mut stack: Vec<Frame> = vec![];
    let mut keywords = vec![];
    let mut names = Names::new();
    let mut seen: Vec<Seen> = vec![];
    let mut fixed_order: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut argument_warnings: Vec<(String, Warning)> = vec![];
    let mut i = 0;
    while i < tokens.len() && tokens[i].span.start < own_len {
        let all = crate::declaration_warnings::statement_tokens(tokens, tokens[i].span.start, usize::MAX, src);
        i += all.len().max(1);
        let Some(start) = all.iter().position(|t| !matches!(t.kind, TokenKind::StatementLabel(_))) else { continue };
        let label = all[..start].iter().find_map(|t| text(src, t).trim().parse::<u64>().ok());
        let ts = &all[start..];
        // Skip zero-width tokens the lexer inserts.
        let ts: Vec<Token> = ts.iter().filter(|t| !t.span.is_empty()).cloned().collect();
        if ts.is_empty() { continue; }
        keyword_positions(&ts, &mut keywords);
        let w = words(&src[ts[0].span.start..ts[ts.len() - 1].span.end]);
        let span = first_line(src, ts[0].span.start, ts[ts.len() - 1].span.end);
        let role = classify(&w, stack.last().map(|f| f.kind));
        seen.push(Seen {
            role: classify(&w, stack.last().map(|f| f.kind)), words: w.clone(), labeled: label.is_some(),
            range: ts[0].span.start..ts[ts.len() - 1].span.end,
            then: matches!(role, Role::Open(Kind::If, _)).then(|| ts[ts.len() - 1].span.start),
        });
        statement_checks(src, &ts, &w, span.clone(), &mut stack, &mut out);
        // Declarations of the innermost procedure, for -Wdeclaration-order
        // and -Wargument-order.
        if let Some(f) = stack.last_mut().filter(|f| f.kind == Kind::Unit) {
            let args = f.unit.as_ref().map(|u| u.args.clone()).unwrap_or_default();
            match role {
                Role::Open(Kind::Interface | Kind::Type | Kind::Enum, _) | Role::Close(_) => {}
                Role::Open(Kind::Unit, _) => {}
                Role::Open(..) | Role::Contains => f.usage.order.started = true,
                _ => order_statement(src, &ts, &w, &args, &mut f.usage.order),
            }
        }
        if let (Role::Open(Kind::Unit, _), [.., parent, interface]) = (&role, stack.as_mut_slice()) {
            if interface.kind == Kind::Interface && parent.kind == Kind::Unit && !parent.usage.order.started {
                if let (Some(u), Some(p)) = (unit(&w, Kind::Unit), parent.unit.as_ref()) {
                    if p.args.contains(&u.name) {
                        parent.usage.order.statements += 1;
                        let statement = parent.usage.order.statements;
                        parent.usage.order.decls.push(Declared { name: u.name.clone(), refs: Default::default(), span: span.clone(), statement, prefix: None, text: u.name });
                    }
                }
            }
        }
        // Procedures of defined assignment and defined I/O, whose argument
        // order is fixed.
        if w.first().is_some_and(|x| x == "generic") && w.iter().any(|x| matches!(x.as_str(), "assignment" | "read" | "write")) {
            if let Some(p) = w.iter().position(|x| x == "=>") { fixed_order.extend(w[p + 1..].iter().filter(|x| is_word(x)).cloned()); }
        }
        if stack.last().is_some_and(|f| f.kind == Kind::Interface && f.name.as_deref() == Some("@fixed-order"))
            && matches!(w.first().map(String::as_str), Some("module" | "procedure")) {
            let at = w.iter().position(|x| x == "procedure").map_or(w.len(), |p| p + 1);
            fixed_order.extend(w[at..].iter().filter(|x| is_word(x) && x.as_str() != "::").cloned());
        }
        let opens_scope = matches!(role, Role::Open(Kind::Unit | Kind::Type, _));
        // Dummies, arrays and USE ONLY names of the innermost unit; what may
        // be modified and what is used, for it and the units enclosing it.
        if !matches!(role, Role::Open(Kind::Unit, _)) && !stack.iter().any(|f| f.kind == Kind::Interface) {
            if let Some(f) = stack.iter_mut().rev().find(|f| f.kind == Kind::Unit) {
                let args = f.unit.as_ref().map(|u| u.args.clone()).unwrap_or_default();
                intent_declarations(src, &ts, &w, &args, &mut f.usage);
                if w.first().is_some_and(|x| x == "use") { only_names(src, &ts, &w, span.clone(), &mut f.usage); }
                if w == ["private"] { f.usage.private = true; }
            }
            let arrays: std::collections::HashSet<String> = stack.iter().filter(|f| f.kind == Kind::Unit)
                .flat_map(|f| f.usage.arrays.iter().cloned()).collect();
            let mut modified = std::collections::HashSet::new();
            modified_names(&w, &arrays, &mut modified);
            if w.first().is_some_and(|x| x != "use") {
                for f in stack.iter_mut().filter(|f| f.kind == Kind::Unit) {
                    f.usage.modified.extend(modified.iter().cloned());
                    for i in f.usage.only.iter_mut() {
                        if w.contains(&i.name) { i.used = true; }
                    }
                }
            }
        }
        // Uses of a function's name as its result variable, to be renamed.
        if !matches!(role, Role::Close(_) | Role::Open(Kind::Unit, _)) {
            if let Some((name, index)) = stack.iter().rev().find(|f| f.kind == Kind::Unit).and_then(|f| f.rename.clone()) {
                for t in ts.iter().filter(|t| matches!(t.kind, TokenKind::Ident(_)) && text(src, t).eq_ignore_ascii_case(&name)) {
                    if out[index].labels.len() < 8 {
                        out[index].labels.push((t.span.clone(), format!("rename to `{name}_result`")));
                    }
                }
            }
        }
        if !opens_scope { statement_names(src, &ts, &w, &mut stack, &mut names); }
        // IMPLICIT NONE that the host's already gives.
        if let Some(bits) = implicit_none(&w) {
            if let Some(f) = stack.iter_mut().rev().find(|f| f.kind == Kind::Unit) {
                if let (true, Some(host)) = (f.implicit.0 & bits == bits, f.implicit.1.clone()) {
                    out.push(Warning {
                        category: "redundant-implicit",
                        message: "IMPLICIT NONE is already in effect from the host [redundant-implicit]".to_owned(),
                        labels: vec![(span.clone(), "redundant".to_owned())],
                        related: vec![(host, "the host's IMPLICIT NONE applies here too".to_owned())],
                        notes: vec!["module and internal procedures inherit the implicit rules of their host; interface bodies do not, so IMPLICIT NONE is needed in those".to_owned()],
                    });
                }
                f.implicit = (f.implicit.0 | bits, Some(span.clone()));
            }
        }
        // Loops ended by this labeled statement.
        if let Some(outermost) = label.and_then(|l| stack.iter().position(|f| f.kind == Kind::Do && f.label == Some(l))) {
            let ended = stack.split_off(outermost);
            let labeled = ended.iter().filter(|f| f.label == label).count();
            let action = !matches!(action(&w).first().map(String::as_str), Some("continue" | "enddo"))
                && !(w.first().is_some_and(|x| x == "end") && w.get(1).is_some_and(|x| x == "do"));
            for f in ended.iter().rev() {
                unnamed(f, &mut out);
                if f.label == label {
                    let l = label.unwrap_or(0);
                    let mut warning = Warning {
                        category: "labeled-do",
                        message: format!("DO loop ended by the statement labeled {l} [labeled-do]"),
                        labels: vec![(f.span.clone(), "labeled DO".to_owned())],
                        related: vec![(span.clone(), "ends here".to_owned())], notes: vec![],
                    };
                    if labeled > 1 { warning = warning.note("loops sharing a terminal statement are obsolescent"); }
                    else if action { warning = warning.note("a loop ended by a statement other than CONTINUE or END DO is obsolescent"); }
                    out.push(warning.note("write the loop without the label, ending it with END DO"));
                }
            }
            continue;
        }
        match role {
            Role::Open(kind, do_label) => {
                let unit = matches!(kind, Kind::Unit | Kind::Type).then(|| unit(&w, kind)).flatten();
                // A procedure inherits from the unit enclosing it, unless it
                // is an interface body; a submodule from its parent module.
                let host = stack.iter().rposition(|f| matches!(f.kind, Kind::Unit | Kind::Interface))
                    .filter(|&p| stack[p].kind == Kind::Unit);
                let submodule = unit.as_ref().filter(|u| u.kind == "submodule")
                    .and_then(|_| w.get(2)).and_then(|parent| names.modules.get(parent).cloned());
                let (inherits, implicit, uses) = match (kind, host, submodule) {
                    (Kind::Unit, _, Some((scope, implicit))) => (false, implicit, vec![scope]),
                    (Kind::Unit, Some(p), None) => (true, stack[p].implicit.clone(), vec![]),
                    (Kind::Type, ..) => (true, (0, None), vec![]),
                    _ => (false, (0, None), vec![]),
                };
                let scope = (kind == Kind::Unit).then(|| names.scope());
                let rename = (kind == Kind::Unit).then(|| function_result(src, &ts, &w, &mut out)).flatten()
                    .map(|name| (name, out.len() - 1));
                let mut usage = Usage::default();
                if let Some(u) = unit.as_ref().filter(|u| u.kind == "function") {
                    let clause = w.iter().position(|x| x == "result").and_then(|r| w.get(r + 2)).cloned();
                    usage.order.result_clause = clause.is_some();
                    usage.order.result = Some(clause.unwrap_or_else(|| u.name.clone()));
                }
                let fixed = kind == Kind::Interface && (w.iter().any(|x| x == "assignment")
                    || w.windows(3).any(|x| matches!(x[0].as_str(), "read" | "write") && x[1] == "(" && matches!(x[2].as_str(), "formatted" | "unformatted")));
                stack.push(Frame {
                    kind, label: do_label, span: span.clone(), name: if fixed { Some("@fixed-order".into()) } else { construct_name(&w) }, unit, bare: vec![],
                    implicit, scope, uses, inherits, rename, usage,
                });
                if opens_scope { header_names(src, &ts, &mut stack, &mut names); }
            }
            Role::Close(kind) => {
                let wanted = kind.unwrap_or(Kind::Unit);
                if let Some(p) = stack.iter().rposition(|f| f.kind == wanted) {
                    let in_interface = stack[..p].iter().any(|f| f.kind == Kind::Interface);
                    for f in stack.split_off(p).iter().rev() {
                        if let (true, Some(u)) = (f.kind == wanted && f.kind == Kind::Unit, &f.unit) {
                            if matches!(u.kind.as_str(), "function" | "subroutine") {
                                declaration_order(u, &f.usage.order, &mut out);
                                if !in_interface {
                                    if let Some(w) = argument_order(u, &f.usage.order, &f.span) { argument_warnings.push((u.name.clone(), w)); }
                                }
                            }
                        }
                        unnamed(f, &mut out);
                        if f.kind == wanted {
                            if let Some(u) = &f.unit { bare_end(src, &ts, &w, u, &mut out); }
                        }
                        if f.kind == wanted && f.kind == Kind::Unit { usage_warnings(f, &mut out); }
                        if let (Some(u), Some(scope)) = (f.unit.as_ref().filter(|u| u.kind == "module"), f.scope) {
                            names.modules.insert(u.name.clone(), (scope, f.implicit.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.extend(argument_warnings.into_iter().filter(|(n, _)| !fixed_order.contains(n)).map(|(_, w)| w));
    name_warnings(&names, &mut out);
    one_line_ifs(src, &seen, line_limit.unwrap_or(132), &mut out);
    keywords.dedup_by(|a, b| a.span == b.span);
    keyword_case(src, &keywords, &mut out);
    semicolons(src, &seen, &mut out);
    if !fixed_form { layout(src, own_len, tabs, line_limit, &mut out); }
    out
}

/// Whether `line-length` is enabled, with its limit, and whether as an error.
fn line_length(settings: &SemaSettings, options: &[String]) -> (Option<usize>, bool) {
    let (mut enabled, mut error) = crate::unused::selected("line-length", false, settings, options);
    let mut limit = 132;
    for option in options {
        let (on, as_error, name) = match option.strip_prefix("no-") {
            Some(n) => (false, false, n),
            None => match option.strip_prefix("error=") { Some(n) => (true, true, n), None => (true, false, option.as_str()) },
        };
        if let Some(n) = name.strip_prefix("line-length=").and_then(|n| n.parse().ok()) {
            limit = n;
            enabled = on;
            error |= as_error;
        }
    }
    (enabled.then_some(limit), error)
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
    fixed_form: bool,
) -> Result<()> {
    // Opt-in: -Wall does not enable these.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let controls: Vec<(&str, (bool, bool))> = CATEGORIES.iter().map(|c| (*c, crate::unused::selected(c, false, &opt_in, options))).collect();
    let (limit, limit_error) = line_length(&opt_in, options);
    let enabled = |c: &str| if c == "line-length" { limit.is_some() } else { controls.iter().any(|(n, (on, _))| *n == c && *on) };
    let as_error = |c: &str| if c == "line-length" { limit_error } else { controls.iter().any(|(n, (_, e))| *n == c && *e) };
    if !CATEGORIES.iter().any(|c| enabled(c)) { return Ok(()); }
    let tokens: Vec<Token> = crate::lexer::lex_source(src);
    let mut warnings = analyze(src, &tokens, own_len, fixed_form, limit, enabled("tabs"));
    warnings.retain(|w| enabled(w.category));
    if warnings.is_empty() { return Ok(()); }
    warnings.sort_by_key(|w| w.labels.first().map_or(0, |l| l.0.start));
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    let mut failed = vec![];
    for w in &warnings {
        let mut labels = vec![];
        for (span, label) in &w.labels {
            let (file, at) = locate(span.clone());
            labels.push(Label::primary(file, at).with_message(label.clone()));
        }
        for (span, label) in &w.related {
            let (file, at) = locate(span.clone());
            labels.push(Label::secondary(file, at).with_message(label.clone()));
        }
        let error = as_error(w.category);
        let diagnostic = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(w.message.clone()).with_labels(labels).with_notes(w.notes.clone());
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &diagnostic)?;
        if error && !failed.contains(&w.category) { failed.push(w.category); }
    }
    if !failed.is_empty() { bail!("{} warnings treated as errors", failed.join(", ")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Each warning as (category, line, message without the category).
    fn warnings(src: &str) -> Vec<(&'static str, usize, String)> {
        let tokens = crate::lexer::lex_source(src);
        // `one-line-if` reports every logical IF; its test reads it directly.
        super::analyze(src, &tokens, src.len(), false, Some(132), true).into_iter().filter(|w| w.category != "one-line-if").map(|w| {
            let at = w.labels.first().map_or(0, |l| l.0.start);
            let message = w.message.rsplit_once(" [").map_or(w.message.clone(), |(m, _)| m.to_owned());
            (w.category, src[..at].matches('\n').count() + 1, message)
        }).collect()
    }
    fn categories(src: &str) -> Vec<&'static str> {
        warnings(src).into_iter().map(|w| w.0).collect()
    }

    #[test]
    fn modern_code_is_not_reported() {
        let src = "module m\nimplicit none\ncontains\nsubroutine s(x, n, ok)\nreal, intent(in) :: x(:)\n\
            integer, intent(in) :: n\nlogical, intent(in) :: ok\ninteger :: i\ncharacter(len=*), parameter :: c = 'a.lt.b'\n\
            outer: do i = 1, n\n   if (x(i) < 0 .and. ok) exit outer\n   if (x(i) == 0) cycle outer\nend do outer\nend subroutine s\n\
            end module m\n";
        assert_eq!(warnings(src), []);
    }

    #[test]
    fn style_findings() {
        let src = "subroutine s(x, y, n, flag)\ninteger n, i\nreal x(*), y(n, *)\nlogical flag\ndouble precision d\n\
            if (n .lt. 0 .or. flag .eqv. .true.) stop\ndo 10 i = 1, n\n   if (x(i) > 0) exit\n   x(i) = 0\n10 continue\n\
            go to 20\n20 continue\nend\n";
        assert_eq!(categories(src), ["missing-double-colon", "missing-double-colon", "assumed-size", "assumed-size",
            "missing-double-colon", "double-precision", "missing-double-colon", "old-relational", "logical-comparison",
            "stop-in-procedure", "unnamed-exit", "labeled-do", "goto", "declaration-order", "declaration-order", "bare-end"]);
        let w = warnings(src);
        // `integer n, i` declares a dummy with a local, before `x` and `y`.
        assert!(w.iter().any(|x| x.2 == "dummy argument `x` is declared after local variable `i`"));
        assert!(w.iter().any(|x| x.2 == "one declaration declares dummy argument `n` and local variable `i`"));
        assert!(w.iter().any(|x| x.2 == "`.lt.` is the old spelling of `<`"));
        assert!(w.iter().any(|x| x.2 == "END statement without the name of subroutine `s`"));
        assert!(w.iter().any(|x| x.2 == "comparison with `.true.` is redundant"));
    }

    #[test]
    fn obsolescent_features() {
        let src = "subroutine s(k, *)\ninteger :: k, a(2), b(2)\ncommon /blk/ a\nequivalence (a, b)\n\
            go to (1, 2), k\n1 if (k) 2, 2, 3\n2 return 1\n3 continue\nend subroutine s\n";
        let w: Vec<String> = warnings(src).into_iter().filter(|x| x.0 == "obsolescent").map(|x| x.2).collect();
        assert_eq!(w, ["alternate return is obsolescent", "COMMON is obsolescent", "EQUIVALENCE is obsolescent",
            "computed GO TO is obsolescent", "arithmetic IF is obsolescent", "alternate return is obsolescent"]);
    }

    #[test]
    fn redundant_implicit_none() {
        let src = "module m\nimplicit none\ninterface\n   subroutine ext(x)\n      implicit none\n      real :: x\n   end subroutine ext\nend interface\n\
            contains\nsubroutine sub(x)\nimplicit none\nreal, intent(in) :: x\nprint *, x\ncontains\nsubroutine inner()\nimplicit none (type, external)\n\
            end subroutine inner\nend subroutine sub\nend module m\nsubmodule (m) sm\nimplicit none\nend submodule sm\n\
            program p\nuse m\nimplicit none\nend program p\n";
        let lines: Vec<usize> = warnings(src).into_iter().filter(|w| w.0 == "redundant-implicit").map(|w| w.1).collect();
        // Not the interface body (line 5), nor `(type, external)`, which adds EXTERNAL (line 16).
        assert_eq!(lines, [11, 21]);
    }

    #[test]
    fn name_case() {
        let src = "module m\nimplicit none\ntype :: point\n   real :: xVal\nend type point\ninteger :: nMax = 3\n\
            contains\nsubroutine sub(Arg)\nreal :: arg\ntype(Point) :: p\np%XVAL = arg + NMAX\ncall sub2(ARG)\nend subroutine sub\n\
            subroutine sub2(y)\nreal :: Y\nend subroutine sub2\nend module m\nprogram main\nuse m, only: Sub\nimplicit none\n\
            real :: x\nX = 2.3\nopen(Unit=10, file='a')\ncall sub(x)\nend program main\n";
        let mut got: Vec<String> = warnings(src).into_iter().filter(|w| w.0 == "name-case").map(|w| w.2).collect();
        got.sort();
        assert_eq!(got, ["`NMAX` is declared as `nMax`", "`Point` is declared as `point`", "`Sub` is declared as `sub`",
            "`X` is declared as `x`", "`Y` is declared as `y`", "`arg` is declared as `Arg`", "component `XVAL` is declared as `xVal`"]);
    }

    #[test]
    fn function_result_forms() {
        let src = "module m\nimplicit none\ncontains\nfunction a(x) result(y)\nreal, intent(in) :: x\nreal :: y\ny = x\nend function a\n\
            real function b(x) result(y)\nreal, intent(in) :: x\ny = x\nend function b\n\
            pure real(kind=8) function c(x)\nreal(kind=8), intent(in) :: x\nc = 2*x\nend function c\n\
            function d(x)\nreal, intent(in) :: x\nreal :: d\nd = x\nend function d\nend module m\n";
        let w: Vec<(usize, String)> = warnings(src).into_iter().filter(|w| w.0 == "function-result").map(|w| (w.1, w.2)).collect();
        assert_eq!(w, [(9, "function `b` gives its result type in the FUNCTION statement".to_owned()),
            (13, "function `c` gives its result type in the FUNCTION statement and has no RESULT clause".to_owned()),
            (17, "function `d` has no RESULT clause".to_owned())]);
        let tokens = crate::lexer::lex_source(src);
        let all = super::analyze(src, &tokens, src.len(), false, None, false);
        let c = all.iter().find(|w| w.message.contains("`c`")).expect("c");
        assert!(c.notes.iter().any(|n| n == "suggestion: pure function c(x) result(c_result)"), "{:?}", c.notes);
        assert!(c.notes.iter().any(|n| n == "then declare `real(kind=8) :: c_result` with the arguments"), "{:?}", c.notes);
        // The header, then `c = 2*x` in the body.
        assert_eq!(c.labels.len(), 2);
    }

    #[test]
    fn one_line_ifs() {
        let src = "program p\nreal :: x\nx = 4\nif (x > 0) then\n   x = sqrt(x)\nend if\nif (x > 1) then ! note\n   x = 1\nend if\n\
            if (x > 2) then\n   x = 2\n   x = 3\nend if\ncheck: if (x > 3) then\n   x = 4\nend if check\nif (x < 0) print *, x\n\
            if (x) 10, 10, 10\n10 continue\nend program p\n";
        let tokens = crate::lexer::lex_source(src);
        let all = super::analyze(src, &tokens, src.len(), false, None, false);
        let w: Vec<(&str, usize)> = all.iter().filter(|w| w.category.contains("one-line-if"))
            .map(|w| (w.category, src[..w.labels[0].0.start].matches('\n').count() + 1)).collect();
        assert_eq!(w, [("could-be-one-line-if", 4), ("one-line-if", 17)]);
        assert!(all.iter().any(|w| w.notes.iter().any(|n| n == "suggestion: if (x > 0) x = sqrt(x)")));
        assert!(all.iter().any(|w| w.notes.iter().any(|n| n == "suggestion:\nif (x < 0) then\n   print *, x\nend if")));
    }

    #[test]
    fn intent_in_candidates() {
        let src = "module m\nimplicit none\ncontains\nsubroutine s(a, b, c, d, e, f, g)\nreal, intent(inout) :: a, b, c, d, e\n\
            real, intent(inout) :: f(:)\ncharacter(len=*), intent(inout) :: g\ninteger :: i\nb = a\ncall t(c)\nread (*, *) d\n\
            do i = 1, size(f)\n   print *, f(i), e\nend do\nwrite (g, '(a)') 'x'\nend subroutine s\nsubroutine t(x)\nreal, intent(inout) :: x\n\
            x = 1\nend subroutine t\nend module m\n";
        let mut got: Vec<String> = warnings(src).into_iter().filter(|w| w.0 == "could-be-intent-in").map(|w| w.2).collect();
        got.sort();
        assert_eq!(got, ["dummy argument `a` of subroutine `s` is INTENT(INOUT) but is never modified",
            "dummy argument `e` of subroutine `s` is INTENT(INOUT) but is never modified",
            "dummy argument `f` of subroutine `s` is INTENT(INOUT) but is never modified"]);
    }

    #[test]
    fn unused_only_names() {
        let src = "module a\nimplicit none\ninteger :: x = 1, y = 2, z = 3\nend module a\nmodule b\nuse a, only: x, y\nimplicit none\nend module b\n\
            module c\nuse a, only: x, y\nimplicit none\nprivate\npublic :: x\nend module c\n\
            program p\nuse a, only: x, y, w => z\nimplicit none\nprint *, x\nend program p\n";
        let w: Vec<String> = warnings(src).into_iter().filter(|w| w.0 == "unused-only").map(|w| w.2).collect();
        // Not module b, which may re-export; module c's `y`; the program's `y` and `w`.
        assert_eq!(w, ["`y` is imported from `a` but never used", "`y`, `w` are imported from `a` but never used"]);
        let tokens = crate::lexer::lex_source(src);
        let all = super::analyze(src, &tokens, src.len(), false, None, false);
        assert!(all.iter().any(|w| w.notes.iter().any(|n| n == "suggestion: use a, only: x")));
    }

    #[test]
    fn declaration_and_argument_order() {
        let src = "module m\nimplicit none\ntype :: t\ncontains\nprocedure :: assign\ngeneric :: assignment(=) => assign\nend type t\ncontains\n\
subroutine s(n, x, y)\ninteger, parameter :: wp = kind(1.0d0)\ninteger :: i\ninteger, intent(in) :: n\nreal(wp), intent(in) :: x(n)\nreal(wp), intent(out) :: y\ny = sum(x)\ni = 1\nend subroutine s\n\
function f(a, b, tol) result(r)\nreal, intent(out) :: a\nreal, intent(in), optional :: tol\nreal, intent(in) :: b\nreal :: r, tmp\ntmp = b\na = tmp\nr = tmp\nend function f\n\
subroutine assign(lhs, rhs)\nclass(t), intent(out) :: lhs\ntype(t), intent(in) :: rhs\nend subroutine assign\n\
subroutine g(z, k)\nreal, intent(out) :: z\ninteger, intent(in) :: k\nend subroutine g\nend module m\n";
        let w: Vec<_> = super::analyze(src, &crate::lexer::lex_source(src), src.len(), false, None, false).into_iter()
            .filter(|w| matches!(w.category, "declaration-order" | "argument-order"))
            .map(|w| (src[..w.labels[0].0.start].matches('\n').count() + 1, w.message.rsplit_once(" [").unwrap().0.to_owned(), w.notes)).collect();
        let messages: Vec<(usize, &str)> = w.iter().map(|(l, m, _)| (*l, m.as_str())).collect();
        assert_eq!(messages, [
            (12, "dummy argument `n` is declared after local variable `i`"),
            (22, "one declaration declares function result `r` and local variable `tmp`"),
            (18, "arguments of function `f` are out of order: INTENT(IN) argument `b` follows output argument `a`"),
            (31, "arguments of subroutine `g` are out of order: INTENT(IN) argument `k` follows output argument `z`"),
        ].as_slice().iter().map(|(l, m)| (*l, *m)).collect::<Vec<_>>().as_slice());
        assert!(w[2].2.iter().any(|n| n == "suggestion: function f(b, a, tol) result(r)"), "{:?}", w[2].2);
    }

    #[test]
    fn semicolons_and_trailing_whitespace() {
        let src = "program main\nimplicit none\ninteger :: i, n\nreal :: x, y\nx = 1.0; y = 2.0\nn = 1;\nselect case (n)\n   case (1); y = sin(x)\n   case (2); y = cos(x); x = 0\n   case default; y = x\nend select\ndo i = 1, n; print *, i; end do  \nprint *, x, y ! a; b\nend program main\n";
        let w: Vec<_> = super::analyze(src, &crate::lexer::lex_source(src), src.len(), false, None, false).into_iter()
            .filter(|w| matches!(w.category, "semicolon" | "trailing-whitespace")).map(|w| {
                let lines: Vec<usize> = w.labels.iter().map(|l| src[..l.0.start].matches('\n').count() + 1).collect();
                (w.category, w.message, lines)
            }).collect();
        assert_eq!(w, [
            ("semicolon", "3 lines have more than one statement [semicolon]".to_owned(), vec![5, 9, 9, 12, 12]),
            ("trailing-whitespace", "1 line has trailing whitespace [trailing-whitespace]".to_owned(), vec![12]),
            ("semicolon", "1 line ends with a semicolon [semicolon]".to_owned(), vec![6]),
        ]);
    }

    #[test]
    fn keyword_case_and_layout() {
        let src = format!("program p\nINTEGER :: i\ndo i = 1, 2\n\tprint *, i\nend do\nprint *, '{}'\nend program p\n", "x".repeat(140));
        let w = warnings(&src);
        assert!(w.iter().any(|x| x.0 == "keyword-case" && x.2.starts_with("keywords are written in more than one case")), "{w:?}");
        assert!(w.iter().any(|x| x.0 == "tabs" && x.1 == 4));
        assert!(w.iter().any(|x| x.0 == "line-length" && x.1 == 6));
        // Names spelled like keywords are not keywords.
        let src = "program p\ninteger :: DEFAULT, LEN\nDEFAULT = 1\nLEN = DEFAULT\nprint *, LEN\nend program p\n";
        assert!(!warnings(src).iter().any(|x| x.0 == "keyword-case"), "{:?}", warnings(src));
    }
}

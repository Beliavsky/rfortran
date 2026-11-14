// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Parenthesized variables as actual arguments.
//!
//! `(x)` is an expression, not the variable `x`: passed to a procedure, the
//! dummy is associated with a copy of its value, so that `f((i), i)` gives the
//! first dummy the original value of `i` even when the procedure defines the
//! second. The parser marks a parenthesized variable `@paren(x)`. Here each
//! such actual argument of a user procedure becomes a copy made just before
//! its statement, when the variable's type is known in the scope; every other
//! mark is removed, leaving the variable itself.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use std::collections::HashMap;

/// The mark of a parenthesized variable.
pub const PAREN: &str = "@paren";

/// The declared or implicit type, and the dimensions, of a scope's variables.
struct Scope {
    types: HashMap<String, (TypeSpec, Vec<Expr>)>,
    /// Names whose type is not simply declared, as initialized or derived.
    other: std::collections::HashSet<String>,
    implicit: Option<Vec<(char, char, TypeSpec)>>,
}

impl Scope {
    /// `implicit`: whether an undeclared name can only be implicitly typed,
    /// as in a main program or external procedure without USE.
    fn new(body: &[Stmt], implicit: bool) -> Self {
        let mut types = HashMap::new();
        let mut other = std::collections::HashSet::new();
        let uses = body.iter().any(|s| matches!(s, Stmt::Use { .. }));
        let mut implicit = (implicit && !uses).then(|| vec![('a', 'h', TypeSpec::Real), ('i', 'n', TypeSpec::Integer(None)), ('o', 'z', TypeSpec::Real)]);
        for s in body {
            match s {
                Stmt::InitializedDecl { entities, .. } => for (n, _) in entities { other.insert(n.to_ascii_lowercase()); },
                Stmt::DerivedDecl { names, .. } => for n in names { other.insert(n.to_ascii_lowercase()); },
                Stmt::VarDecl { kind, names } => for n in names { types.entry(n.to_ascii_lowercase()).or_insert((kind.clone(), vec![])); },
                Stmt::ArrayDecl { kind, name, dims } => { types.insert(name.to_ascii_lowercase(), (kind.clone(), dims.clone())); }
                Stmt::Implicit(Implicit::None) => implicit = None,
                Stmt::Implicit(Implicit::Rule { type_spec, letter_ranges }) => {
                    if let Some(rules) = &mut implicit {
                        for r in letter_ranges { rules.insert(0, (r.start, r.end.unwrap_or(r.start), type_spec.clone())); }
                    }
                }
                Stmt::Implicit(Implicit::Rules { rules: list }) => {
                    if let Some(rules) = &mut implicit {
                        for rule in list { for r in &rule.letter_ranges { rules.insert(0, (r.start, r.end.unwrap_or(r.start), rule.type_spec.clone())); } }
                    }
                }
                _ => {}
            }
        }
        Scope { types, other, implicit }
    }
    /// Whether `n` is declared as an array, whose subscripts are not actual
    /// arguments; a scalar declaration of a referenced name is a function's.
    fn array(&self, n: &str) -> bool { self.types.get(&n.to_ascii_lowercase()).is_some_and(|(_, d)| !d.is_empty()) }
    /// The type of a variable that this scope declares, or implicitly types
    /// when it declares nothing else of that name.
    fn kind(&self, n: &str) -> Option<(TypeSpec, Vec<Expr>)> {
        if let Some(t) = self.types.get(&n.to_ascii_lowercase()) { return Some(t.clone()); }
        if self.other.contains(&n.to_ascii_lowercase()) { return None; }
        let first = n.chars().next()?.to_ascii_lowercase();
        self.implicit.as_ref()?.iter().find(|(a, b, _)| (*a..=*b).contains(&first)).map(|(_, _, k)| (k.clone(), vec![]))
    }
}

pub(crate) fn resolve(program: &mut Program) {
    let mut serial = 0;
    let contained = program.contains.clone();
    let scope_types = Scope::new(&program.body, true);
    for s in program.body.iter_mut() {
        if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
            let implicit = !contained.iter().any(|c| c.eq_ignore_ascii_case(name));
            scope(body, implicit, &mut serial);
        }
    }
    statements(&mut program.body, &scope_types, &mut serial);
}

fn scope(body: &mut Vec<Stmt>, implicit: bool, serial: &mut usize) {
    let scope_types = Scope::new(body, implicit);
    statements(body, &scope_types, serial);
}

fn statements(body: &mut Vec<Stmt>, scope_types: &Scope, serial: &mut usize) {
    let mut i = 0;
    while i < body.len() {
        match &mut body[i] {
            // Top-level procedures are done by resolve; those in a module or
            // another procedure see host names, so need declarations.
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => scope(body, false, serial),
            Stmt::DoWhile { cond, body } => { strip(cond); statements(body, scope_types, serial); }
            s => {
                let mut copies = vec![];
                // Only statements evaluated once take copies before them.
                let once = matches!(s, Stmt::Assign { .. } | Stmt::AssignIndex { .. } | Stmt::CallSub { .. } | Stmt::Print { .. });
                if let Stmt::CallSub { name, args } = s {
                    if !name.starts_with('@') {
                        for a in args.iter_mut() { copy(a, scope_types, once, &mut copies, serial); }
                    }
                }
                for e in crate::procedure_arguments::expressions(s) {
                    crate::procedure_arguments::visit_expr(e, &mut |e| {
                        if let Expr::Call(n, args) = e {
                            if !n.starts_with('@') && !scope_types.array(n) && crate::intrinsics::purity(n).is_none() {
                                for a in args.iter_mut() { copy(a, scope_types, once, &mut copies, serial); }
                            }
                        }
                    });
                    strip(e);
                }
                for child in crate::procedure_arguments::children(s) { statements(child, scope_types, serial); }
                if !copies.is_empty() {
                    let statement = std::mem::replace(&mut body[i], Stmt::Block { body: vec![] });
                    copies.push(statement);
                    body[i] = Stmt::Block { body: copies };
                }
            }
        }
        i += 1;
    }
}

/// Make the actual argument `(x)` a copy of `x` when its type is known.
fn copy(arg: &mut Expr, scope_types: &Scope, once: bool, copies: &mut Vec<Stmt>, serial: &mut usize) {
    let Expr::Call(tag, inner) = arg else { return };
    if tag != PAREN || !once { return; }
    let [Expr::Ident(n)] = inner.as_slice() else { return };
    let Some((kind, dims)) = scope_types.kind(n) else { return };
    if dims.iter().any(|d| !matches!(d, Expr::IntLit(_))) || matches!(kind, TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_)) { return; }
    *serial += 1;
    let temp = format!("@paren_copy_{serial}");
    copies.push(if dims.is_empty() { Stmt::VarDecl { kind, names: vec![temp.clone()] } } else { Stmt::ArrayDecl { kind, name: temp.clone(), dims } });
    copies.push(Stmt::Assign { name: temp.clone(), value: Expr::Ident(n.clone()) });
    *arg = Expr::Ident(temp);
}

/// Remove the marks that remain: elsewhere `(x)` has the value of `x`.
fn strip(e: &mut Expr) {
    crate::procedure_arguments::visit_expr(e, &mut |e| {
        if let Expr::Call(tag, inner) = e {
            if tag == PAREN && inner.len() == 1 { *e = inner.pop().unwrap(); }
        }
    });
}

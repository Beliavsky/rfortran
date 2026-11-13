// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Implied-DO indices with an INTEGER type-spec (Fortran 2008), as in
//! `[(i**2, integer :: i = 1, n)]`.
//!
//! Such an index is local to its implied-DO: it needs no declaration and
//! leaves a variable of the same name in the enclosing scope unchanged. The
//! parser marks the index `@typed_implied_do_index:<kind>(i)`. Each one
//! becomes a fresh variable of that type, declared in the enclosing scope and
//! used only inside the implied-DO's items.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::procedure_arguments::{children, expressions, visit_expr};

/// The prefix of a typed index; the INTEGER kind follows, empty for the default kind.
pub const TAG: &str = "@typed_implied_do_index:";
/// The prefix of the variables that the indices become.
pub const LOCAL: &str = "@implied_do_index_";

pub(crate) fn resolve(program: &mut Program) {
    let mut serial = 0;
    scope(&mut program.body, &mut serial);
}

// Whether a statement belongs to the specification part, where the new
// declarations go.
fn declaration(s: &Stmt) -> bool {
    matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. }
        | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. } | Stmt::DummyIntent { .. }
        | Stmt::DerivedType { .. } | Stmt::SourceLocation(_))
}

fn scope(body: &mut Vec<Stmt>, serial: &mut usize) {
    let mut i = 0;
    while i < body.len() {
        let mut declarations = vec![];
        statement(&mut body[i], serial, &mut declarations);
        if !declarations.is_empty() {
            // Before a declaration that uses the index; otherwise after the
            // last declaration, so that kind parameters are already defined.
            let at = if declaration(&body[i]) { i } else { body[..i].iter().rposition(declaration).map_or(0, |p| p + 1) };
            let count = declarations.len();
            body.splice(at..at, declarations);
            i += count;
        }
        i += 1;
    }
}

fn statement(s: &mut Stmt, serial: &mut usize, declarations: &mut Vec<Stmt>) {
    for e in expressions(s) {
        visit_expr(e, &mut |e| index(e, serial, declarations));
    }
    match s {
        // New scopes keep their indices' declarations.
        Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } | Stmt::Block { body } => scope(body, serial),
        _ => for child in children(s) {
            for s in child.iter_mut() { statement(s, serial, declarations); }
        },
    }
}

// Give a typed index of an implied-DO a declared variable of its own.
fn index(e: &mut Expr, serial: &mut usize, declarations: &mut Vec<Stmt>) {
    let Expr::Call(name, args) = e else { return };
    if name != "@output_implied_do" || args.len() < 4 { return; }
    let Expr::Call(tag, var) = &args[0] else { return };
    let Some(kind) = tag.strip_prefix(TAG) else { return };
    let [Expr::Ident(var)] = var.as_slice() else { return };
    let kind = if kind.is_empty() {
        TypeSpec::Integer(None)
    } else {
        kind.parse::<u8>().map_or_else(|_| TypeSpec::IntegerKind(kind.to_owned()), |k| TypeSpec::Integer(Some(k)))
    };
    *serial += 1;
    let local = format!("{LOCAL}{serial}_{var}");
    let var = var.clone();
    for item in &mut args[4..] {
        visit_expr(item, &mut |e| if matches!(e, Expr::Ident(n) if n.eq_ignore_ascii_case(&var)) { *e = Expr::Ident(local.clone()); });
    }
    args[0] = Expr::Ident(local.clone());
    declarations.push(Stmt::VarDecl { kind, names: vec![local] });
}

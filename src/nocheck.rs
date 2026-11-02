// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-nocheck`: compiled code without run-time checks of uninitialized reads
//! or array subscripts. Applied after initialization instrumentation, so the
//! compile-time errors for definitely unassigned variables remain.
//!
//! Initialization flags are deleted, and reads of them become 1, unless a
//! procedure receives them: allocation, MOVE_ALLOC, record and module storage
//! keep their flags, though nothing checks them. Subscript guards are replaced
//! by the index arithmetic they wrap.
use crate::ast::{BinOp, Expr, Program, Stmt};
use crate::procedure_arguments::{children, expressions, visit_expr};
use std::collections::{BTreeMap, BTreeSet};

const CHECK: &str = "@check_initialized";

fn flag(name: &str) -> bool { name.starts_with("@initialized_") }

/// Guard procedures that check a subscript and return an expression of
/// their parameters: the parameters and that expression.
fn guards(body: &[Stmt], found: &mut BTreeMap<String, (Vec<String>, Expr)>) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } if name.starts_with("@matrix_bound_") || name.starts_with("@lower_bound_index_") => {
                if let Some(Stmt::Return(Some(e))) = body.last() { found.insert(name.clone(), (params.clone(), e.clone())); }
            }
            Stmt::Module { body, .. } => guards(body, found),
            _ => {}
        }
    }
}

/// Flags that a procedure receives, which therefore keep their storage.
fn passed(body: &mut [Stmt], kept: &mut BTreeSet<String>) {
    for s in body.iter_mut() {
        let call = matches!(s, Stmt::CallSub { name, .. } if name != CHECK);
        for e in expressions(s) {
            if call { visit_expr(e, &mut |e| if let Expr::Ident(n) | Expr::Index(n, _) = e { if flag(n) { kept.insert(n.clone()); } }); }
            visit_expr(e, &mut |e| if let Expr::Call(_, args) = e {
                for a in args.iter() { if let Expr::Ident(n) = a { if flag(n) { kept.insert(n.clone()); } } }
            });
        }
        for b in children(s) { passed(b, kept); }
    }
}

fn inline(e: &mut Expr, guards: &BTreeMap<String, (Vec<String>, Expr)>, kept: &BTreeSet<String>) {
    visit_expr(e, &mut |e| {
        let replacement = match e {
            Expr::Ident(n) | Expr::Index(n, _) if flag(n) && !kept.contains(n) => Some(Expr::IntLit("1".into())),
            Expr::Call(n, args) if guards.contains_key(n.as_str()) => {
                let (params, result) = &guards[n.as_str()];
                let mut result = result.clone();
                visit_expr(&mut result, &mut |r| if let Expr::Ident(p) = r {
                    if let Some(i) = params.iter().position(|q| q == p) { if let Some(a) = args.get(i) { *r = a.clone(); } }
                });
                Some(result)
            }
            // The element offset of an allocatable array, from (position,
            // extent) pairs: 1 + sum of (position - 1) * stride.
            Expr::Call(n, args) if n.starts_with("@alloc_offset_") && !args.is_empty() && args.len() % 2 == 0 => {
                let bin = |op, a, b| Expr::Bin(op, Box::new(a), Box::new(b));
                let mut offset = Expr::IntLit("1".into());
                let mut stride: Option<Expr> = None;
                for pair in args.chunks(2) {
                    let position = bin(BinOp::Sub, pair[0].clone(), Expr::IntLit("1".into()));
                    let term = match &stride { Some(s) => bin(BinOp::Mul, position, s.clone()), None => position };
                    offset = bin(BinOp::Add, offset, term);
                    stride = Some(match stride { Some(s) => bin(BinOp::Mul, s, pair[1].clone()), None => pair[1].clone() });
                }
                Some(offset)
            }
            _ => None,
        };
        if let Some(r) = replacement { *e = r; }
    });
}

fn rewrite(body: &mut Vec<Stmt>, guards: &BTreeMap<String, (Vec<String>, Expr)>, kept: &BTreeSet<String>) {
    let removable = |n: &str| flag(n) && !kept.contains(n);
    let mut output = Vec::with_capacity(body.len());
    for mut s in std::mem::take(body) {
        match &mut s {
            Stmt::CallSub { name, .. } if name == CHECK => continue,
            Stmt::ArrayDecl { name, .. } | Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } if removable(name) => continue,
            Stmt::VarDecl { names, .. } => {
                names.retain(|n| !removable(n));
                if names.is_empty() { continue; }
            }
            _ => {}
        }
        for e in expressions(&mut s) { inline(e, guards, kept); }
        let had_body = matches!(&s, Stmt::Do { body, .. } if !body.is_empty());
        for b in children(&mut s) { rewrite(b, guards, kept); }
        // A generated loop that only cleared or set flags is now empty.
        if had_body && matches!(&s, Stmt::Do { var, body, .. } if body.is_empty() && var.starts_with('@')) { continue; }
        output.push(s);
    }
    *body = output;
}

// The browser build interprets programs and never strips checks.
#[allow(dead_code)]
pub(crate) fn strip(mut program: Program) -> Program {
    let mut found = BTreeMap::new();
    guards(&program.body, &mut found);
    let mut kept = BTreeSet::new();
    passed(&mut program.body, &mut kept);
    rewrite(&mut program.body, &found, &kept);
    program
}

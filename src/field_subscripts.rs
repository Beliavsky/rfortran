// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Subscripts after the component of an array of records: `a%foo(i)` is
//! element `i` of `foo` in every element of `a`, as `a(:)%foo(i)` is, not
//! `a(i)%foo`. The assignment `a%foo(i) = v` parses with its subscripts
//! marked, since it would otherwise read as `a(i)%foo = v`; with the
//! declarations known, a whole array `a` takes the `a(:)%foo(i)` form, and
//! a scalar record `x` keeps `x%foo(i)` (unmarked). References `a%foo(i)` in
//! expressions take the same form.
use crate::ast::{Expr, Program, Stmt};
use std::collections::HashMap;

/// The marker of the subscripts of `name%component(subscripts) = value`.
pub(crate) const MARK: &str = "@field_subscripts";

/// The arrays of records of a scope, with their ranks.
type Arrays = HashMap<String, usize>;

fn whole(rank: usize) -> Vec<Expr> {
    vec![Expr::Section { name: String::new(), lower: None, upper: None, stride: None }; rank]
}

fn expr(e: &mut Expr, arrays: &Arrays) {
    crate::procedure_arguments::visit_expr(e, &mut |e| {
        let Expr::Call(n, subs) = e else { return };
        let Some((base, field)) = n.split_once('%') else { return };
        if field.contains('%') { return; }
        let Some(&rank) = arrays.get(base) else { return };
        let element = crate::ast::section_or_call(base.to_owned(), whole(rank));
        let args = vec![element, Expr::Call("@component_subscripts".into(), std::mem::take(subs))];
        *e = Expr::Call(format!("@record_component:{field}"), args);
    });
}

fn body(body: &mut Vec<Stmt>, host: &Arrays) {
    let mut arrays = host.clone();
    for s in body.iter() {
        match s {
            Stmt::DerivedDecl { names, dims, .. } => for n in names {
                if dims.is_empty() { arrays.remove(n); } else { arrays.insert(n.clone(), dims.len()); }
            },
            Stmt::VarDecl { names, .. } => for n in names { arrays.remove(n); },
            Stmt::ArrayDecl { name, .. } => { arrays.remove(name); }
            _ => {}
        }
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body: b, .. } | Stmt::Subroutine { body: b, .. } | Stmt::Module { body: b, .. } => { self::body(b, &arrays); continue; }
            Stmt::Block { body: b } => { self::body(b, &arrays); continue; }
            Stmt::AssignIndex { name, indices, value } => {
                if let [Expr::Call(m, subs)] = indices.as_slice() {
                    if m == MARK {
                        let subs = subs.clone();
                        let base = name.split_once('%').map(|(b, f)| (b, f.contains('%')));
                        *indices = match base.and_then(|(b, deeper)| (!deeper).then(|| arrays.get(b)).flatten()) {
                            Some(&rank) => {
                                let mut all = whole(rank);
                                all.push(Expr::Call("@component_subscripts".into(), subs));
                                all
                            }
                            None => subs,
                        };
                    }
                }
                expr(value, &arrays);
                for i in indices.iter_mut() { expr(i, &arrays); }
                continue;
            }
            _ => {}
        }
        for e in crate::procedure_arguments::expressions(s) { expr(e, &arrays); }
        for b in crate::procedure_arguments::children(s) { self::body(b, &arrays); }
    }
}

pub(crate) fn resolve(program: &mut Program) {
    body(&mut program.body, &Arrays::new());
}

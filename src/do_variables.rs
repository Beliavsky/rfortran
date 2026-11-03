// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! A DO variable must not be redefined inside its loop (Fortran 2018
//! 11.1.7.4.3): by assignment, as the variable of a nested DO or implied DO,
//! as a READ input item, or as an IOSTAT=, STAT= or similar status variable.
//! ASSOCIATE names are already replaced by their selectors, so assignment
//! through an associate name of the DO variable is found too.
use crate::ast::{Expr, Program, Stmt};
use anyhow::Result;

/// Control keywords whose variable an I/O or allocation statement defines.
const DEFINED: &[&str] = &["iostat", "stat", "size", "iomsg", "errmsg", "newunit", "nextrec", "number", "pos", "recl"];

fn whole(e: &Expr, v: &str) -> bool {
    matches!(e, Expr::Ident(n) if n.eq_ignore_ascii_case(v))
}

/// Whether an expression, as a READ item or a statement argument, defines `v`:
/// the variable of an implied DO, or a defined control.
fn defines(e: &Expr, v: &str, item: bool) -> bool {
    match e {
        Expr::Ident(_) => item && whole(e, v),
        Expr::Call(n, args) if n.contains("implied_do") => {
            args.first().is_some_and(|a| whole(a, v)) || args.iter().any(|a| defines(a, v, item))
        }
        Expr::Call(n, args) => ["@io_spec:", "@keyword:", "@write_option:"].iter()
            .filter_map(|p| n.strip_prefix(p))
            .any(|key| DEFINED.contains(&key) && args.first().is_some_and(|a| whole(a, v))),
        _ => false,
    }
}

/// The READ input items and status variable of a READ statement.
fn read_defines(name: &str, args: &[Expr], v: &str) -> bool {
    let (status, items): (Option<(&Expr, &Expr)>, &[Expr]) = match (name, args) {
        ("@unformatted_read", [_, c, s, _, items @ ..]) => (Some((c, s)), items),
        ("@internal_read" | "@empty_read", [_, c, s, items @ ..]) => (Some((c, s)), items),
        ("@formatted_read_status", [_, _, c, s, items @ ..]) => (Some((c, s)), items),
        ("@formatted_read", [_, _, items @ ..]) => (None, items),
        _ => return false,
    };
    status.is_some_and(|(c, s)| matches!(c, Expr::Str(k) if k == "iostat") && whole(s, v))
        || items.iter().any(|e| defines(e, v, true))
}

/// The DO variables of a statement that starts a loop.
fn loop_variables(s: &Stmt) -> Vec<&str> {
    match s {
        Stmt::Do { var, .. } => vec![var.as_str()],
        Stmt::Block { body } => match body.first() {
            Some(Stmt::CallSub { name, args }) if name == "@do_step" => args.chunks_exact(4).filter_map(|c| match &c[0] {
                Expr::Ident(n) => Some(n.as_str()),
                _ => None,
            }).collect(),
            _ => vec![],
        },
        _ => vec![],
    }
}

fn line(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].matches('\n').count() + 1
}

struct Checker<'a> {
    src: &'a str,
    /// Active DO variables and the offsets of their DO statements.
    active: Vec<(String, usize)>,
}

impl Checker<'_> {
    fn redefined(&self, s: &Stmt) -> Option<&(String, usize)> {
        self.active.iter().find(|(v, _)| match s {
            Stmt::Assign { name, .. } => name.eq_ignore_ascii_case(v),
            Stmt::Read { args } => args.iter().any(|e| defines(e, v, true)),
            Stmt::CallSub { name, args } if name.contains("read") => read_defines(name, args, v) || args.iter().any(|e| defines(e, v, false)),
            // A procedure's dummy arguments can have the names of controls.
            Stmt::CallSub { name, args } => name.starts_with('@') && args.iter().any(|e| defines(e, v, false)),
            Stmt::Print { items } => items.iter().any(|e| defines(e, v, false)),
            _ => loop_variables(s).iter().any(|n| n.eq_ignore_ascii_case(v)),
        })
    }

    fn body(&mut self, body: &[Stmt], mut offset: usize) -> Result<()> {
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if let Some((v, start)) = self.redefined(s) {
                return Err(crate::source_diagnostics::named(
                    format!("DO variable `{v}` of the loop at line {} cannot be redefined inside the loop", line(self.src, *start)),
                    offset, v));
            }
            match s {
                // A procedure has its own variables, though a contained one
                // can change a host DO variable through host association.
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    let saved = std::mem::take(&mut self.active);
                    self.body(body, offset)?;
                    self.active = saved;
                }
                _ => {
                    let vars: Vec<String> = loop_variables(s).into_iter().map(str::to_string).collect();
                    let depth = self.active.len();
                    self.active.extend(vars.into_iter().map(|v| (v, offset)));
                    let mut copy = s.clone();
                    let nested = crate::procedure_arguments::children(&mut copy);
                    for b in nested {
                        // The control of an explicit-step DO is its first statement.
                        let skip = usize::from(matches!(s, Stmt::Block { .. }) && depth < self.active.len());
                        self.body(&b[skip..], offset)?;
                    }
                    self.active.truncate(depth);
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn check(program: &Program, src: &str) -> Result<()> {
    Checker { src, active: vec![] }.body(&program.body, 0)
}

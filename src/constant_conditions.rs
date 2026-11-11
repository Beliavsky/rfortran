// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wconstant-condition`, in -Wextra: a condition, or part of one, whose
//! value is known at compile time, so that part of the test has no effect,
//! as `.true. .or. (x > y)`. It reports literals combined with other
//! operands, comparisons of an INTEGER, LOGICAL or CHARACTER variable with
//! itself, comparisons of literals, and comparisons of one INTEGER variable
//! with constants that contradict or cover every value, as
//! `n > 5 .and. n < 3`. A whole condition that is a literal, as
//! `if (.false.)` or `do while (.true.)`, is deliberate and not reported.
//!
//! `-Wconstant-condition-all` also reports conditions whose value comes
//! from a named constant, as `debug .or. n > 2` with `debug` a PARAMETER,
//! which are usually settings rather than mistakes.
use super::*;
use crate::ast::UnOp;
use super::array_sizes::text;

/// A known LOGICAL value, and whether a named constant gives it.
#[derive(Clone, Copy)]
struct Known {
    value: bool,
    named: bool,
}

/// Sets of integers, as sorted disjoint closed intervals.
type Set = Vec<(i128, i128)>;
const LOW: i128 = i64::MIN as i128 - 1;
const HIGH: i128 = i64::MAX as i128 + 1;

fn intersect(a: &Set, b: &Set) -> Set {
    let mut out = vec![];
    for &(p, q) in a {
        for &(r, s) in b {
            let (lo, hi) = (p.max(r), q.min(s));
            if lo <= hi { out.push((lo, hi)); }
        }
    }
    out.sort();
    out
}

fn complement(a: &Set) -> Set {
    let mut out = vec![];
    let mut next = LOW;
    for &(p, q) in a {
        if p > next { out.push((next, p - 1)); }
        next = next.max(q + 1);
    }
    if next <= HIGH { out.push((next, HIGH)); }
    out
}

fn union(a: &Set, b: &Set) -> Set {
    complement(&intersect(&complement(a), &complement(b)))
}

fn universe(a: &Set) -> bool {
    complement(a).is_empty()
}

impl<'a> Analysis<'a> {
    fn integer_kind(&self, scope: usize, n: &str) -> Option<TypeSpec> {
        let d = &self.decls[self.resolve(scope, n)?];
        d.kind.clone()
    }

    /// Whether `e` names a variable, or an element, of a type whose
    /// comparison with itself is always decided: not REAL or COMPLEX.
    fn self_comparable(&self, e: &Expr, scope: usize) -> bool {
        let (name, subscripts): (&str, &[Expr]) = match e {
            Expr::Ident(n) => (n, &[]),
            Expr::Index(n, s) => (n, s),
            _ => return false,
        };
        let simple = subscripts.iter().all(|s| matches!(s, Expr::Ident(_) | Expr::IntLit(_)));
        let Some(id) = self.resolve(scope, name) else { return false };
        let d = &self.decls[id];
        simple && d.procedure.is_none() && !d.attrs.contains("parameter")
            && matches!(d.kind, Some(TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Logical | TypeSpec::LogicalKind(_)
                | TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed))
    }

    /// Whether `e` refers to a named constant.
    fn mentions_constant(&self, e: &Expr, scope: usize) -> bool {
        let mut found = false;
        crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| {
            if let Expr::Ident(n) = x {
                found |= self.resolve(scope, n).is_some_and(|id| self.decls[id].attrs.contains("parameter"));
            }
        });
        found
    }

    /// The integers of variable `var` for which comparison `e` holds, if
    /// `e` is a test of one INTEGER variable against constants.
    fn comparison_set(&self, e: &Expr, scope: usize, var: &mut Option<String>, named: &mut bool) -> Option<Set> {
        match e {
            Expr::Un(UnOp::Not, x) => self.comparison_set(x, scope, var, named).map(|s| complement(&s)),
            Expr::Bin(BinOp::And, a, b) => Some(intersect(&self.comparison_set(a, scope, var, named)?, &self.comparison_set(b, scope, var, named)?)),
            Expr::Bin(BinOp::Or, a, b) => Some(union(&self.comparison_set(a, scope, var, named)?, &self.comparison_set(b, scope, var, named)?)),
            Expr::Bin(op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), a, b) => {
                let integer = |n: &str| matches!(self.integer_kind(scope, n), Some(TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)))
                    && self.resolve(scope, n).is_some_and(|id| !self.decls[id].attrs.contains("parameter"));
                let (name, k, flipped) = match (&**a, &**b) {
                    (Expr::Ident(n), c) if integer(n) => (n, c, false),
                    (c, Expr::Ident(n)) if integer(n) => (n, c, true),
                    _ => return None,
                };
                let k = self.integer(k, scope, &mut HashSet::new())? as i128;
                *named |= self.mentions_constant(if flipped { a } else { b }, scope);
                match var {
                    Some(v) if v != name => return None,
                    _ => *var = Some(name.clone()),
                }
                // Turn `k op n` into `n op' k`.
                let op = match (op, flipped) {
                    (BinOp::Lt, true) => BinOp::Gt,
                    (BinOp::Le, true) => BinOp::Ge,
                    (BinOp::Gt, true) => BinOp::Lt,
                    (BinOp::Ge, true) => BinOp::Le,
                    (op, _) => op.clone(),
                };
                Some(match op {
                    BinOp::Eq => vec![(k, k)],
                    BinOp::Ne => vec![(LOW, k - 1), (k + 1, HIGH)],
                    BinOp::Lt => vec![(LOW, k - 1)],
                    BinOp::Le => vec![(LOW, k)],
                    BinOp::Gt => vec![(k + 1, HIGH)],
                    _ => vec![(k, HIGH)],
                })
            }
            _ => None,
        }
    }

    /// The value of a LOGICAL expression known at compile time.
    fn known(&self, e: &Expr, scope: usize) -> Option<Known> {
        match e {
            Expr::Logical(v) => Some(Known { value: *v, named: false }),
            Expr::Ident(n) => {
                let d = &self.decls[self.resolve(scope, n)?];
                if !d.attrs.contains("parameter") { return None; }
                match d.constant.as_ref()? {
                    Expr::Logical(v) => Some(Known { value: *v, named: true }),
                    c => self.known(&c.clone(), d.scope).map(|k| Known { named: true, ..k }),
                }
            }
            Expr::Un(UnOp::Not, x) => self.known(x, scope).map(|k| Known { value: !k.value, ..k }),
            Expr::Bin(op @ (BinOp::And | BinOp::Or | BinOp::Eqv | BinOp::Neqv), a, b) => {
                let (x, y) = (self.known(a, scope), self.known(b, scope));
                let named = x.is_some_and(|k| k.named) || y.is_some_and(|k| k.named);
                match (op, x, y) {
                    (BinOp::Or, Some(k), _) | (BinOp::Or, _, Some(k)) if k.value => Some(Known { value: true, named: k.named }),
                    (BinOp::And, Some(k), _) | (BinOp::And, _, Some(k)) if !k.value => Some(Known { value: false, named: k.named }),
                    (_, Some(x), Some(y)) => Some(Known {
                        value: match op { BinOp::And => x.value && y.value, BinOp::Or => x.value || y.value, BinOp::Eqv => x.value == y.value, _ => x.value != y.value },
                        named,
                    }),
                    _ => {
                        // Comparisons of one variable that decide the result.
                        let (mut var, mut named) = (None, false);
                        let set = self.comparison_set(e, scope, &mut var, &mut named)?;
                        if set.is_empty() { Some(Known { value: false, named }) } else if universe(&set) { Some(Known { value: true, named }) } else { None }
                    }
                }
            }
            Expr::Bin(op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), a, b) => {
                // A variable compared with itself.
                if a == b && self.self_comparable(a, scope) {
                    return Some(Known { value: matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge), named: false });
                }
                let compare = |o: std::cmp::Ordering| match op {
                    BinOp::Eq => o.is_eq(), BinOp::Ne => o.is_ne(), BinOp::Lt => o.is_lt(), BinOp::Le => o.is_le(), BinOp::Gt => o.is_gt(), _ => o.is_ge(),
                };
                match (&**a, &**b) {
                    (Expr::Str(x), Expr::Str(y)) => {
                        // Shorter strings compare as if padded with blanks.
                        let n = x.len().max(y.len());
                        let (x, y) = (format!("{x:n$}"), format!("{y:n$}"));
                        Some(Known { value: compare(x.cmp(&y)), named: false })
                    }
                    _ => {
                        let constant = |e: &Expr| matches!(e, Expr::IntLit(_)) || matches!(e, Expr::Un(UnOp::Neg, x) if matches!(**x, Expr::IntLit(_)))
                            || matches!(e, Expr::Ident(n) if self.resolve(scope, n).is_some_and(|id| self.decls[id].attrs.contains("parameter")));
                        if !constant(a) || !constant(b) { return None; }
                        let (x, y) = (self.integer(a, scope, &mut HashSet::new())?, self.integer(b, scope, &mut HashSet::new())?);
                        Some(Known { value: compare(x.cmp(&y)), named: self.mentions_constant(e, scope) })
                    }
                }
            }
            _ => None,
        }
    }

    /// The report for condition `e`: its category and message.
    fn constant_report(&self, e: &Expr, scope: usize) -> Option<(&'static str, String)> {
        let category = |named: bool| if named { "constant-condition-all" } else { "constant-condition" };
        let truth = |v: bool| if v { "true" } else { "false" };
        let literal = |v: bool| if v { ".true." } else { ".false." };
        let from = |k: Known, part: &Expr| if let (true, Expr::Ident(n)) = (k.named, part) {
            format!(", since `{n}` is a named constant with value {}", literal(k.value))
        } else if k.named && !matches!(part, Expr::Logical(_)) {
            let mut names = vec![];
            crate::procedure_arguments::visit_expr(&mut part.clone(), &mut |x| {
                if let Expr::Ident(n) = x {
                    if self.resolve(scope, n).is_some_and(|id| self.decls[id].attrs.contains("parameter")) && !names.contains(n) { names.push(n.clone()); }
                }
            });
            format!(", since {} {} a named constant", names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(" and "), if names.len() == 1 { "is" } else { "are" })
        } else {
            String::new()
        };
        match e {
            Expr::Bin(op @ (BinOp::And | BinOp::Or), a, b) => {
                let (x, y) = (self.known(a, scope), self.known(b, scope));
                let decides = matches!(op, BinOp::Or);
                for (k, part, other) in [(x, a, b), (y, b, a)] {
                    let Some(k) = k else { continue };
                    let whole = text(e);
                    return Some(if k.value == decides {
                        (category(k.named), format!("`{whole}` is always {}{}; `{}` has no effect", truth(decides), from(k, part), text(other)))
                    } else if let (true, Expr::Ident(n)) = (k.named, &**part) {
                        (category(true), format!("`{n}` is a named constant with value {}, so it has no effect in `{whole}`", literal(k.value)))
                    } else if matches!(**part, Expr::Logical(_)) {
                        (category(k.named), format!("`{}` has no effect in `{whole}`", text(part)))
                    } else {
                        (category(k.named), format!("`{}` is always {}{} and has no effect in `{whole}`", text(part), truth(k.value), from(k, part)))
                    });
                }
                if let Some(k) = self.known(e, scope) {
                    return Some((category(k.named), format!("`{}` is always {}{}", text(e), truth(k.value), from(k, e))));
                }
                self.constant_report(a, scope).or_else(|| self.constant_report(b, scope))
            }
            Expr::Un(UnOp::Not, x) => self.constant_report(x, scope),
            Expr::Logical(_) => None,
            _ => {
                let k = self.known(e, scope)?;
                // A named constant alone, as `if (debug)`, is a setting.
                if let (Expr::Ident(n), true) = (e, k.named) {
                    return Some(("constant-condition-all", format!("`{n}` is a named constant with value {}, so the condition is always {}", literal(k.value), truth(k.value))));
                }
                Some((category(k.named), format!("`{}` is always {}{}", text(e), truth(k.value), from(k, e))))
            }
        }
    }

    fn report_condition(&mut self, e: &Expr, scope: usize, offset: usize) {
        // A literal alone is deliberate, as `if (.false.)` or `do while (.true.)`.
        if matches!(e, Expr::Logical(_)) || matches!(e, Expr::Un(UnOp::Not, x) if matches!(**x, Expr::Logical(_))) {
            return;
        }
        let Some((category, message)) = self.constant_report(e, scope) else { return };
        let mut first = None;
        crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| if let (Expr::Ident(n), None) = (x, &first) { first = Some(n.clone()) });
        let span = match &first {
            Some(n) => self.span(offset, n),
            None => self.statement_tokens(offset).first().map_or(offset..offset, |t| t.span.clone()),
        };
        self.warnings.push(Advice { category, span, message, related: None, suggestion: None,
            note: "a condition known at compile time is often a leftover from debugging or a mistake; remove the constant part, or the test it hides" });
    }

    pub(super) fn constant_conditions(&mut self, body: &'a [Stmt], scope: usize, mut offset: usize) {
        for s in body {
            if let Stmt::SourceLocation(p) = s { offset = *p; continue; }
            match s {
                // IF (.true.) without ELSE runs its body unconditionally.
                Stmt::If { cond, else_body: None, .. } if matches!(cond, Expr::Logical(true))
                    || matches!(cond, Expr::Un(UnOp::Not, x) if matches!(**x, Expr::Logical(false))) => {
                    let ts = self.statement_tokens(offset);
                    let span = ts.first().map_or(offset..offset, |t| t.span.clone());
                    let else_if = ts.first().is_some_and(|t| self.src[t.span.clone()].to_ascii_lowercase().starts_with("else"));
                    let (message, note) = if else_if {
                        ("`else if (.true.)` is always true; write `else`".to_owned(), "an ELSE IF whose condition is always true is an ELSE")
                    } else {
                        ("`if (.true.)` is always true; the IF has no effect".to_owned(), "remove the IF and keep the statements it controls")
                    };
                    self.warnings.push(Advice { category: "constant-condition", span, message, related: None, suggestion: None, note });
                }
                Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => self.report_condition(cond, scope, offset),
                Stmt::CallSub { name, args } if name == "@where" => if let Some(m) = args.first() { self.report_condition(m, scope, offset) },
                Stmt::Assign { value: v @ Expr::Bin(BinOp::And | BinOp::Or, _, _), .. } => self.report_condition(v, scope, offset),
                _ => {}
            }
            // MERGE masks and the conditions of conditional expressions.
            let mut copy = s.clone();
            let mut conditions = vec![];
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| match x {
                    Expr::Call(n, a) if n == "merge" && a.len() == 3 => conditions.push(a[2].clone()),
                    Expr::Call(n, a) if n == "@conditional" && a.len() >= 3 => {
                        for i in (0..a.len() - 1).step_by(2) { conditions.push(a[i].clone()); }
                    }
                    _ => {}
                });
            }
            for c in conditions { self.report_condition(&c, scope, offset); }
            match s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    let inner = self.bodies.get(&(body.as_ptr() as usize)).copied().unwrap_or(scope);
                    self.constant_conditions(body, inner, offset);
                }
                _ => for b in children(s) { self.constant_conditions(b, scope, offset) },
            }
        }
    }
}

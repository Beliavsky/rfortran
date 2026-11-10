// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wduplicate-constant`, in -Wstyle: a named constant defined in more than
//! one scope, such as `pi` in several procedures of a module or in several
//! modules. It considers scalar REAL and COMPLEX constants, and INTEGER
//! constants that are kind parameters, such as `dp = kind(1.0d0)`; other
//! INTEGER, LOGICAL and CHARACTER constants sharing a name are usually
//! unrelated. Values are compared after evaluation, at the kind of each
//! constant, so `3.141592653589793_dp` and `4*atan(1.0_dp)` are the same.
use super::*;
use std::collections::BTreeMap;

/// Kind constants of ISO_FORTRAN_ENV and ISO_C_BINDING.
const KIND_NAMES: &[&str] = &[
    "real16", "real32", "real64", "real128", "int8", "int16", "int32", "int64", "c_float", "c_double",
    "c_long_double", "c_int", "c_short", "c_long", "c_long_long", "c_int8_t", "c_int16_t", "c_int32_t",
    "c_int64_t", "c_size_t", "c_intptr_t", "c_bool", "c_char", "c_float_complex", "c_double_complex",
];

#[derive(Clone, Copy)]
enum Value {
    Real(f64, f64),
    Integer(i64),
    Unknown,
}

struct Constant {
    value: Value,
    /// The bytes of the constant's kind, where known.
    bytes: Option<u8>,
}

/// How a later definition compares with the first.
enum Comparison {
    Same,
    Different,
    OtherKind,
    Unknown,
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 4.0 * f64::EPSILON * a.abs().max(b.abs())
}

impl<'a> Analysis<'a> {
    fn kind_parameter(&self, e: &Expr) -> bool {
        let mut found = false;
        crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| match x {
            Expr::Call(n, _) if matches!(n.as_str(), "kind" | "selected_real_kind" | "selected_int_kind"
                | "selected_char_kind" | "selected_logical_kind") => found = true,
            Expr::Ident(n) if KIND_NAMES.contains(&n.as_str()) => found = true,
            _ => {}
        });
        found
    }

    /// The value of a named constant this check considers.
    fn duplicate_candidate(&self, id: usize) -> Option<Constant> {
        let d = &self.decls[id];
        if !d.attrs.contains("parameter") || !d.dims.is_empty() || d.dummy || d.attrs.contains("@warning_enum_member") {
            return None;
        }
        let e = d.constant.as_ref()?;
        let kind = |k: &str| self.kind_number(d.scope, k);
        let (bytes, real) = match d.kind.as_ref()? {
            TypeSpec::Real => (Some(4), true),
            TypeSpec::DoublePrecision => (Some(8), true),
            TypeSpec::RealKind(k) | TypeSpec::Complex(k) => (kind(k), true),
            TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) if self.kind_parameter(e) => (None, false),
            _ => return None,
        };
        let value = if real {
            match self.constant_number(e, d.scope, &mut HashSet::new(), true) {
                // Rounded to the constant's own kind.
                Some((x, y)) if bytes == Some(4) => Value::Real((x as f32) as f64, (y as f32) as f64),
                Some((x, y)) => Value::Real(x, y),
                None => Value::Unknown,
            }
        } else {
            self.integer(e, d.scope, &mut HashSet::new()).map_or(Value::Unknown, Value::Integer)
        };
        Some(Constant { value, bytes })
    }

    /// The initializer of a named constant as written.
    fn initializer_text(&self, id: usize) -> Option<String> {
        let d = &self.decls[id];
        let ts = self.statement_tokens(d.span.start);
        let at = ts.iter().position(|t| t.span == d.span)?;
        if !matches!(ts.get(at + 1)?.kind, TokenKind::Eq) { return None; }
        let mut depth = 0;
        let mut end = ts.len();
        for (i, t) in ts.iter().enumerate().skip(at + 2) {
            match t.kind {
                TokenKind::LParen | TokenKind::LBracket => depth += 1,
                TokenKind::RParen | TokenKind::RBracket => depth -= 1,
                TokenKind::Comma if depth == 0 => { end = i; break; }
                _ => {}
            }
        }
        Some(self.token_text(&ts[at + 2..end]))
    }

    pub(super) fn duplicate_constants(&mut self, own_len: usize) {
        let mut groups: BTreeMap<String, Vec<(usize, Constant)>> = BTreeMap::new();
        for id in 0..self.decls.len() {
            let d = &self.decls[id];
            if d.span.start >= own_len || self.scopes[d.scope].kind == ScopeKind::Block { continue; }
            if let Some(c) = self.duplicate_candidate(id) {
                groups.entry(d.name.to_ascii_lowercase()).or_default().push((id, c));
            }
        }
        for (_, mut group) in groups {
            if group.len() < 2 { continue; }
            group.sort_by_key(|(id, _)| self.decls[*id].span.start);
            let count = group.len();
            for k in 1..count {
                let (id, c) = &group[k];
                // The first definition of the same kind, or else the first.
                let (first, base) = group[..k].iter().find(|(_, b)| b.bytes == c.bytes).unwrap_or(&group[0]);
                let first_text = self.initializer_text(*first).unwrap_or_default();
                let comparison = match (base.value, c.value) {
                    _ if base.bytes.is_some() && c.bytes.is_some() && base.bytes != c.bytes => Comparison::OtherKind,
                    (Value::Real(a, b), Value::Real(x, y)) => if close(a, x) && close(b, y) { Comparison::Same } else { Comparison::Different },
                    (Value::Integer(a), Value::Integer(x)) => if a == x { Comparison::Same } else { Comparison::Different },
                    _ if self.decls[*first].constant == self.decls[*id].constant => Comparison::Same,
                    _ => Comparison::Unknown,
                };
                let name = self.decls[*id].name.clone();
                let text = self.initializer_text(*id).unwrap_or_default();
                let (message, note) = match comparison {
                    Comparison::Different => (
                        format!("named constant `{name}` = {text} has a different value from its definition elsewhere, {first_text}"),
                        "definitions of one constant that disagree, such as a shorter value of pi, give different results in different procedures; define it once in a module and USE it",
                    ),
                    Comparison::OtherKind => (
                        format!("named constant `{name}` is also defined elsewhere with another kind"),
                        "constants that differ only in kind are clearer with names that say so, such as `pi_sp` and `pi_dp`, defined once in a module",
                    ),
                    Comparison::Same | Comparison::Unknown => (
                        format!("named constant `{name}` is defined in {count} places"),
                        "define the constant once in a module and USE it, so that every procedure has the same value",
                    ),
                };
                self.warnings.push(Advice {
                    category: "duplicate-constant", span: self.decls[*id].span.clone(), message, suggestion: None,
                    related: Some((self.decls[*first].span.clone(), format!("first defined here as {first_text}"))), note,
                });
            }
        }
    }
}

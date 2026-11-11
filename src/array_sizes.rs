// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Arrays whose sizes depend on the data, in -Wextra.
//!
//! Within a procedure or main program, an array defined once, outside any
//! construct, gets a size from its definition: `x = pack(a, mask)` gives 0
//! to `size(a)` elements, `count(mask)`; `allocate(x(n))` gives `n`; and
//! `x = y` gives the size of `y`. Arrays with constant bounds have theirs.
//! Arrays defined more than once, or inside a construct, are not tracked.
//!
//! `-Wmaybe-nonconforming`: an elemental operation on two tracked arrays
//! whose sizes may differ, as `pack(x, tf) - pack(x, .not. tf)`.
//!
//! `-Wmaybe-out-of-bounds`: a constant subscript of a PACK result, which may
//! have fewer elements, as `x1(1)` when no element is selected.
//!
//! An array is not reported when some comparison, DO bound or call
//! argument of the scope uses its size, or the PACK mask's count.
use super::*;
use crate::ast::UnOp;

/// The size of a tracked array, with its text for messages.
#[derive(Clone)]
enum Size {
    Exact { text: String, value: Option<i64> },
    /// The elements of `source` that `mask` selects: 0 to `max`.
    Packed { mask: String, source: String, max: Option<i64>, definition: String },
}

impl Size {
    fn text(&self) -> String {
        match self {
            Size::Exact { text, .. } => text.clone(),
            Size::Packed { mask, .. } => format!("count({mask})"),
        }
    }
    fn same(&self, other: &Size) -> bool {
        match (self, other) {
            (Size::Exact { text: a, value: x }, Size::Exact { text: b, value: y }) => a == b || x.is_some() && x == y,
            (Size::Packed { mask: a, source: s, .. }, Size::Packed { mask: b, source: t, .. }) => a == b && s == t,
            _ => false,
        }
    }
}

/// An expression as written, for messages.
pub(super) fn text(e: &Expr) -> String {
    match e {
        Expr::Ident(n) => n.clone(),
        Expr::IntLit(s) | Expr::RealLit(s) => s.clone(),
        Expr::Str(s) => format!("\"{s}\""),
        Expr::Logical(b) => if *b { ".true.".into() } else { ".false.".into() },
        Expr::Un(UnOp::Neg, x) => format!("-{}", text(x)),
        Expr::Un(UnOp::Not, x) => format!(".not. {}", text(x)),
        Expr::Bin(op, a, b) => {
            let o = match op {
                BinOp::Add => " + ", BinOp::Sub => " - ", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Pow => "**",
                BinOp::Eq => " == ", BinOp::Ne => " /= ", BinOp::Lt => " < ", BinOp::Le => " <= ", BinOp::Gt => " > ",
                BinOp::Ge => " >= ", BinOp::And => " .and. ", BinOp::Or => " .or. ", BinOp::Eqv => " .eqv. ",
                BinOp::Neqv => " .neqv. ", BinOp::Concat => " // ",
            };
            format!("{}{o}{}", text(a), text(b))
        }
        Expr::Call(n, a) | Expr::Index(n, a) => {
            let args: Vec<String> = a.iter().map(|x| match x {
                Expr::Call(k, v) if k.starts_with("@keyword:") => format!("{}={}", &k[9..], v.first().map(text).unwrap_or_default()),
                x => text(x),
            }).collect();
            format!("{n}({})", args.join(", "))
        }
        _ => "...".into(),
    }
}

/// Whether statement `s` may change the size of array `name`: it assigns,
/// allocates, deallocates, reads or passes the whole array.
fn defines(s: &Stmt, name: &str) -> bool {
    let named = |e: &Expr| matches!(e, Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) if n == name);
    match s {
        // Assigning a scalar to an allocated array keeps its size.
        Stmt::Assign { name: n, value } => n == name && !matches!(value, Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) | Expr::Str(_))
            && !matches!(value, Expr::Un(_, x) if matches!(**x, Expr::IntLit(_) | Expr::RealLit(_))),
        Stmt::Read { .. } => {
            let mut copy = s.clone();
            crate::procedure_arguments::expressions(&mut copy).into_iter().any(|e| named(e))
        }
        Stmt::CallSub { name: n, args } if !matches!(n.as_str(), "@formatted_print" | "@write" | "@formatted_write_noadvance" | "@write_star") =>
            args.iter().any(|a| named(a) || matches!(a, Expr::Call(k, v) if k.starts_with("@keyword:") && v.iter().any(named))),
        _ => false,
    }
}

impl<'a> Analysis<'a> {
    /// The size of an array with constant bounds.
    fn constant_size(&self, scope: usize, n: &str) -> Option<i64> {
        let d = &self.decls[self.resolve(scope, n)?];
        if d.dims.is_empty() || d.procedure.is_some() { return None; }
        d.dims.iter().try_fold(1i64, |p, x| p.checked_mul(self.integer(x, scope, &mut HashSet::new())?))
    }

    /// The value of a subscript known at compile time, such as `3` or
    /// `size(x)` for `x` of constant size.
    fn known_value(&self, e: &Expr, scope: usize) -> Option<i64> {
        match e {
            Expr::Call(f, a) if f == "size" => match a.as_slice() {
                [Expr::Ident(n)] => self.constant_size(scope, n),
                _ => None,
            },
            _ => self.integer(e, scope, &mut HashSet::new()),
        }
    }

    /// The size a definition gives, if this check tracks it.
    fn defined_size(&self, value: &Expr, scope: usize, sizes: &HashMap<String, Size>) -> Option<Size> {
        let known = |e: &Expr| -> Option<Size> {
            match e {
                Expr::Ident(n) => sizes.get(n).cloned().or_else(|| self.constant_size(scope, n).map(|v| Size::Exact { text: v.to_string(), value: Some(v) })),
                _ => None,
            }
        };
        match value {
            Expr::Call(n, a) if n == "pack" && !self.user_call(value, scope) => {
                let positional: Vec<&Expr> = a.iter().filter(|x| !matches!(x, Expr::Call(k, _) if k.starts_with("@keyword:"))).collect();
                let keyword = |k: &str| a.iter().find_map(|x| match x { Expr::Call(n, v) if n.strip_prefix("@keyword:") == Some(k) => v.first(), _ => None });
                let source = keyword("array").or(positional.first().copied())?;
                let mask = keyword("mask").or(positional.get(1).copied())?;
                if keyword("vector").or(positional.get(2).copied()).is_some() {
                    return None;
                }
                let max = match known(source) { Some(Size::Exact { value, .. }) => value, _ => None };
                if matches!(mask, Expr::Logical(true)) {
                    return known(source);
                }
                Some(Size::Packed { mask: text(mask), source: text(source), max, definition: text(value) })
            }
            Expr::Ident(_) => known(value),
            Expr::Bin(_, x, y) => self.defined_size(x, scope, sizes).or_else(|| self.defined_size(y, scope, sizes)),
            _ => None,
        }
    }

    /// The names whose sizes, or PACK counts as `count(mask)`, a comparison,
    /// DO bound or call argument uses.
    fn guarded(body: &[Stmt], out: &mut HashSet<String>) {
        fn sizes(e: &Expr, out: &mut HashSet<String>) {
            crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |y| {
                if let Expr::Call(f, a) = y {
                    match (f.as_str(), a.first()) {
                        ("size" | "ubound" | "lbound" | "shape", Some(Expr::Ident(n))) => { out.insert(n.clone()); }
                        ("count", Some(m)) => { out.insert(format!("count({})", text(m))); }
                        _ => {}
                    }
                }
            });
        }
        for s in body {
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| match x {
                    Expr::Bin(BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, _, _) => sizes(x, out),
                    Expr::Call(f, a) if !f.starts_with('@') && crate::intrinsics::purity(f).is_none() => for y in a.iter() { sizes(y, out) },
                    _ => {}
                });
            }
            match s {
                Stmt::Do { start, end, .. } => { sizes(start, out); sizes(end, out); }
                Stmt::CallSub { name, args } if !name.starts_with('@') => for a in args { sizes(a, out) },
                _ => {}
            }
            for b in children(s) { Self::guarded(b, out); }
        }
    }

    pub(super) fn array_sizes(&mut self, body: &'a [Stmt], scope: usize, nonconforming: bool, bounds: bool) {
        for s in body {
            if let Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } = s {
                let inner = self.bodies.get(&(body.as_ptr() as usize)).copied().unwrap_or(scope);
                self.array_sizes(body, inner, nonconforming, bounds);
            }
        }
        if self.scopes[scope].kind == ScopeKind::Module { return; }
        // Local arrays, and how many statements may change their sizes.
        let arrays: Vec<String> = self.scopes[scope].names.iter()
            .filter(|(_, &id)| { let d = &self.decls[id]; d.scope == scope && !d.dims.is_empty() && d.procedure.is_none() })
            .map(|(n, _)| n.clone()).collect();
        if arrays.is_empty() { return; }
        fn count(body: &[Stmt], name: &str) -> usize {
            body.iter().map(|s| usize::from(defines(s, name)) + children(s).into_iter().map(|b| count(b, name)).sum::<usize>()).sum()
        }
        let mut guards = HashSet::new();
        Self::guarded(body, &mut guards);
        let mut sizes: HashMap<String, Size> = HashMap::new();
        let mut offset = 0;
        for s in body {
            if let Stmt::SourceLocation(p) = s { offset = *p; continue; }
            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) { continue; }
            // Uses, in this statement and the statements it contains.
            self.size_uses(std::slice::from_ref(s), scope, offset, &sizes, &guards, nonconforming, bounds);
            // A definition outside any construct, of an array defined once.
            let tracked = |n: &str| arrays.iter().any(|a| a == n) && count(body, n) == 1;
            match s {
                Stmt::Assign { name, value } if tracked(name) => {
                    if let Some(size) = self.defined_size(value, scope, &sizes) { sizes.insert(name.clone(), size); }
                }
                Stmt::CallSub { name, args } if name == "@allocate" => {
                    for a in args {
                        if let Expr::Call(n, d) | Expr::Index(n, d) = a {
                            if let ([e], true) = (d.as_slice(), tracked(n)) {
                                if !matches!(e, Expr::Section { .. }) && !matches!(e, Expr::Call(k, _) if k.starts_with('@')) {
                                    sizes.insert(n.clone(), Size::Exact { text: text(e), value: self.integer(e, scope, &mut HashSet::new()) });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Report the uses of tracked arrays in `body`.
    #[allow(clippy::too_many_arguments)]
    fn size_uses(&mut self, body: &[Stmt], scope: usize, mut offset: usize, sizes: &HashMap<String, Size>, guards: &HashSet<String>, nonconforming: bool, bounds: bool) {
        for s in body {
            if let Stmt::SourceLocation(p) = s { offset = *p; continue; }
            let mut copy = s.clone();
            let mut found: Vec<(&'static str, String, String)> = vec![];
            for e in crate::procedure_arguments::expressions(&mut copy) {
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| { let x: &Expr = x; match x {
                    Expr::Bin(op, a, b) if nonconforming && !matches!(op, BinOp::Concat) => {
                        let (Expr::Ident(p), Expr::Ident(q)) = (&**a, &**b) else { return };
                        let size = |n: &str| sizes.get(n).cloned().or_else(|| self.constant_size(scope, n).map(|v| Size::Exact { text: v.to_string(), value: Some(v) }));
                        let (Some(sp), Some(sq)) = (size(p), size(q)) else { return };
                        if p == q || sp.same(&sq) || !(matches!(sp, Size::Packed { .. }) || matches!(sq, Size::Packed { .. })) && sp.text() == sq.text() { return; }
                        // Two constant sizes that differ are a definite error.
                        if let (Size::Exact { value: Some(_), .. }, Size::Exact { value: Some(_), .. }) = (&sp, &sq) { return; }
                        if [p, q].iter().any(|n| guards.contains(*n)) || [&sp, &sq].iter().any(|z| guards.contains(&z.text())) { return; }
                        found.push(("maybe-nonconforming", p.clone(), format!(
                            "`{}` combines arrays whose sizes may differ: `{p}` has {} elements and `{q}` has {}",
                            text(x), sp.text(), sq.text())));
                    }
                    Expr::Index(n, a) | Expr::Call(n, a) if bounds && a.len() == 1 => {
                        let Some(Size::Packed { mask, source, max, definition }) = sizes.get(n) else { return };
                        if guards.contains(n) || guards.contains(&format!("count({mask})")) { return; }
                        let Some(k) = self.known_value(&a[0], scope) else { return };
                        if k < 1 { return; }
                        let written = text(x);
                        let shown = if matches!(a[0], Expr::IntLit(_)) { format!("`{written}` is") } else { format!("`{written}` is `{n}({k})`,") };
                        let when = match max {
                            Some(m) if k > *m => format!("always out of bounds: `{n}` has at most {m} elements"),
                            Some(m) if k == *m => format!("out of bounds unless all {m} elements of `{mask}` are true"),
                            _ if k == 1 => format!("out of bounds when `{n}` is empty"),
                            _ => format!("out of bounds when fewer than {k} elements of `{source}` are selected"),
                        };
                        let range = match max { Some(m) => format!("0 to {m}"), None => format!("0 to size({source})") };
                        found.push(("maybe-out-of-bounds", n.clone(), format!("{shown} {when}; `{n} = {definition}` gives {range} elements")));
                    }
                    _ => {}
                }});
            }
            for (category, name, message) in found {
                let note = if category == "maybe-out-of-bounds" {
                    "PACK gives as many elements as the mask selects; check the size first"
                } else {
                    "arrays combined element by element must have the same size; check the sizes first"
                };
                self.warnings.push(Advice { category, span: self.span(offset, &name), message, related: None, suggestion: None, note });
            }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }) {
                for b in children(s) { self.size_uses(b, scope, offset, sizes, guards, nonconforming, bounds); }
            }
        }
    }
}

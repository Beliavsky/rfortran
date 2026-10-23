// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Numeric information loss, pointer defaults, loop ranges, and finite enums.
use super::*;
use crate::ast::{CaseItem, UnOp};

pub(super) struct TypeInfo<'a> {
    pub scope: usize,
    pub name: String,
    pub components: &'a [Stmt],
    pub enumeration: bool,
}
#[derive(Clone, Copy, PartialEq, Debug)]
struct Number {
    class: u8,
    bytes: u8,
} // INTEGER, REAL, COMPLEX
impl Number {
    fn label(self) -> String {
        format!(
            "{}({})",
            ["INTEGER", "REAL", "COMPLEX"][self.class as usize],
            self.bytes
        )
    }
    fn spec(self) -> TypeSpec {
        match self.class {
            0 => TypeSpec::Integer(Some(self.bytes)),
            1 if self.bytes == 8 => TypeSpec::DoublePrecision,
            1 => TypeSpec::Real,
            _ => TypeSpec::Complex(self.bytes.to_string()),
        }
    }
    fn combine(self, b: Self) -> Self {
        let class = self.class.max(b.class);
        let bytes = if self.class == b.class {
            self.bytes.max(b.bytes)
        } else if self.class == 0 {
            b.bytes
        } else if b.class == 0 {
            self.bytes
        } else {
            self.bytes.max(b.bytes)
        };
        Self { class, bytes }
    }
}
type Intervals = HashMap<usize, (i64, i64)>;

impl<'a> Analysis<'a> {
    pub(super) fn intrinsic_integer(&self, scope: usize, name: &str) -> Option<i64> {
        let s = &self.scopes[scope];
        for (module, only, renames) in &s.uses {
            if !matches!(
                module.as_str(),
                "iso_fortran_env" | "@intrinsic_iso_fortran_env"
            ) || self.modules.contains_key(module)
            {
                continue;
            }
            let remote = renames
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, r)| r.as_str())
                .unwrap_or(name);
            if remote == name && renames.iter().any(|(l, r)| r == name && l != name) {
                continue;
            }
            if only
                .as_ref()
                .is_some_and(|v| !v.iter().any(|n| n == name || n == remote))
            {
                continue;
            }
            return Some(match remote {
                "int8" => 1,
                "int16" => 2,
                "int32" | "real32" => 4,
                "int64" | "real64" => 8,
                "real128" => -1,
                _ => continue,
            });
        }
        s.parent.and_then(|p| self.intrinsic_integer(p, name))
    }
    fn type_lookup(&self, scope: usize, name: &str) -> Option<usize> {
        fn find(
            a: &Analysis<'_>,
            scope: usize,
            name: &str,
            seen: &mut HashSet<(usize, String)>,
        ) -> Option<usize> {
            if !seen.insert((scope, name.into())) {
                return None;
            }
            let s = &a.scopes[scope];
            if let Some(&t) = s.types.get(name) {
                return Some(t);
            }
            for (m, only, renames) in &s.uses {
                let remote = renames
                    .iter()
                    .find(|(l, _)| l == name)
                    .map(|(_, r)| r.as_str())
                    .unwrap_or(name);
                if remote == name && renames.iter().any(|(l, r)| r == name && l != name) {
                    continue;
                }
                if only
                    .as_ref()
                    .is_some_and(|v| !v.iter().any(|n| n == name || n == remote))
                {
                    continue;
                }
                if let Some(&m) = a.modules.get(m) {
                    if a.scopes[m]
                        .access
                        .get(remote)
                        .copied()
                        .unwrap_or(a.scopes[m].default_public)
                    {
                        if let Some(t) = find(a, m, remote, seen) {
                            return Some(t);
                        }
                    }
                }
            }
            s.parent.and_then(|p| find(a, p, name, seen))
        }
        find(self, scope, name, &mut HashSet::new())
    }
    fn kind_number(&self, scope: usize, text: &str) -> Option<u8> {
        let expr = if let Some(text) = text.strip_prefix(crate::ast::KIND_EXPRESSION) {
            serde_json::from_str(text).ok()?
        } else if text.parse::<i64>().is_ok() {
            Expr::IntLit(text.into())
        } else {
            Expr::Ident(text.into())
        };
        let n = self.integer(&expr, scope, &mut HashSet::new())?;
        matches!(n, 1 | 2 | 4 | 8).then_some(n as u8)
    }
    fn number_kind(&self, kind: &TypeSpec, scope: usize) -> Option<Number> {
        let (class, bytes) = match kind {
            TypeSpec::Integer(k) => (0, k.unwrap_or(4)),
            TypeSpec::IntegerKind(k) => (0, self.kind_number(scope, k)?),
            TypeSpec::Real => (1, 4),
            TypeSpec::DoublePrecision => (1, 8),
            TypeSpec::RealKind(k) => (1, self.kind_number(scope, k)?),
            TypeSpec::Complex(k) => (2, self.kind_number(scope, k)?),
            _ => return None,
        };
        (if class == 0 {
            matches!(bytes, 1 | 2 | 4 | 8)
        } else {
            matches!(bytes, 4 | 8)
        })
        .then_some(Number { class, bytes })
    }
    // Resolve the declared type/shape of a component without confusing its
    // spelling with a same-named entity in another derived type.
    fn data_type(
        &self,
        scope: usize,
        name: &str,
    ) -> Option<(Option<TypeSpec>, Option<usize>, Vec<Expr>, Names, usize)> {
        let mut parts = name.split('%');
        let d = &self.decls[self.resolve(scope, parts.next()?)?];
        let mut kind = d.kind.clone();
        let mut nominal = d
            .nominal
            .as_ref()
            .and_then(|n| self.type_lookup(d.scope, n));
        let mut dims = d.dims.clone();
        let mut attrs = d.attrs.clone();
        let mut owner = d.scope;
        for part in parts {
            let t = &self.types[nominal?];
            owner = t.scope;
            kind = None;
            nominal = None;
            dims.clear();
            attrs.clear();
            let mut found = false;
            for s in t.components {
                match s {
                    Stmt::VarDecl { kind: k, names } if names.iter().any(|n| n == part) => {
                        kind = Some(k.clone());
                        found = true;
                    }
                    Stmt::ArrayDecl {
                        kind: k,
                        name,
                        dims: d,
                    } if name == part => {
                        kind = Some(k.clone());
                        dims = d.clone();
                        found = true;
                    }
                    Stmt::InitializedDecl {
                        kind: k,
                        attributes,
                        entities,
                    } if entities.iter().any(|(n, _)| n == part) => {
                        kind = Some(k.clone());
                        attrs.extend(attributes.iter().cloned());
                        found = true;
                    }
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        attributes,
                        dims: d,
                        ..
                    } if names.iter().any(|n| n == part) => {
                        nominal = self.type_lookup(owner, type_name);
                        attrs.extend(attributes.iter().cloned());
                        dims = d.clone();
                        found = true;
                    }
                    Stmt::DummyIntent { name, intent } if name == part => {
                        attrs.insert(intent.clone());
                    }
                    _ => {}
                }
            }
            if !found {
                return None;
            }
        }
        Some((kind, nominal, dims, attrs, owner))
    }
    fn call_args(&self, name: &str, args: &[Expr]) -> Vec<Expr> {
        let mut args = args.to_vec();
        if let Some(sig) = crate::named_arguments::intrinsic(name) {
            let _ = sig.reorder(name, &mut args);
        }
        args
    }
    fn numeric_type(&self, e: &Expr, scope: usize) -> Option<Number> {
        match value(e) {
            Expr::IntLit(s) => Some(Number {
                class: 0,
                bytes: s
                    .split_once('_')
                    .map(|(_, k)| self.kind_number(scope, k))
                    .unwrap_or(Some(4))?,
            }),
            Expr::RealLit(s) => Some(Number {
                class: 1,
                bytes: s
                    .split_once('_')
                    .map(|(_, k)| self.kind_number(scope, k))
                    .unwrap_or(Some(if s.contains(['d', 'D']) { 8 } else { 4 }))?,
            }),
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Section { name: n, .. } => {
                let (k, _, _, _, owner) = self.data_type(scope, n)?;
                self.number_kind(&k?, owner)
            }
            Expr::Un(UnOp::Neg, x) => self.numeric_type(x, scope),
            Expr::Bin(op, x, y)
                if matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) =>
            {
                let (x, y) = (self.numeric_type(x, scope)?, self.numeric_type(y, scope)?);
                Some(if matches!(op, BinOp::Pow) && y.class == 0 {
                    x
                } else {
                    x.combine(y)
                })
            }
            Expr::Call(n, a) => {
                if let Some((Some(k), _, dims, _, owner)) = self.data_type(scope, n) {
                    if !dims.is_empty() {
                        return self.number_kind(&k, owner);
                    }
                }
                if let Some(id) = self.resolve(scope, n) {
                    let d = &self.decls[id];
                    if let Some(p) = d.procedure {
                        let scope = self.procedures[p].scope;
                        return self.scopes[scope].names.values().find_map(|id| {
                            let d = &self.decls[*id];
                            d.result
                                .then(|| self.number_kind(d.kind.as_ref()?, scope))
                                .flatten()
                        });
                    }
                    if !d.dims.is_empty() || d.attrs.contains("external") {
                        let (k, _, _, _, owner) = self.data_type(scope, n)?;
                        return self.number_kind(&k?, owner);
                    }
                }
                if n == "@typed_constructor" {
                    return self.number_kind(&crate::empty_constructors::kind(e).ok()??, scope);
                }
                if n == "@array_constructor" {
                    return a.first().and_then(|e| self.numeric_type(e, scope));
                }
                if n == "@integer_literal" && a.len() == 2 {
                    return Some(Number {
                        class: 0,
                        bytes: self
                            .integer(&a[1], scope, &mut HashSet::new())?
                            .try_into()
                            .ok()?,
                    });
                }
                if n == "@complex_literal" {
                    return Some(Number {
                        class: 2,
                        bytes: a
                            .iter()
                            .filter_map(|e| self.numeric_type(e, scope))
                            .filter(|n| n.class > 0)
                            .map(|n| n.bytes)
                            .max()
                            .unwrap_or(4),
                    });
                }
                let args = self.call_args(n, a);
                let input = args.first().and_then(|e| self.numeric_type(e, scope));
                let kind = |slot: usize, default: u8| {
                    args.get(slot)
                        .filter(|e| !crate::named_arguments::omitted(e))
                        .map(|e| {
                            self.integer(e, scope, &mut HashSet::new())
                                .and_then(|n| n.try_into().ok())
                        })
                        .unwrap_or(Some(default))
                };
                match n.as_str() {
                    "int" | "nint" | "floor" | "ceiling" => {
                        return Some(Number {
                            class: 0,
                            bytes: kind(1, 4)?,
                        })
                    }
                    "real" => {
                        return Some(Number {
                            class: 1,
                            bytes: kind(1, input.filter(|n| n.class == 2).map_or(4, |n| n.bytes))?,
                        })
                    }
                    "dble" => return Some(Number { class: 1, bytes: 8 }),
                    "cmplx" => {
                        return Some(Number {
                            class: 2,
                            bytes: kind(2, 4)?,
                        })
                    }
                    "abs" | "aimag" => {
                        let x = input?;
                        return Some(Number {
                            class: if x.class == 2 { 1 } else { x.class },
                            ..x
                        });
                    }
                    "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos"
                    | "atan" | "conjg" | "aint" | "anint" | "@unary_plus" | "sum" | "product"
                    | "maxval" | "minval" | "norm2" => return input,
                    _ => {}
                }
                // Share the compiler's intrinsic/generic inference for the
                // remaining supported operations, with concrete scoped kinds.
                let mut declarations = HashMap::new();
                let normalized = self.inference_expr(e, scope, &mut declarations)?;
                let (kind, _) = self
                    .inference
                    .as_ref()?
                    .kind_rank(&normalized, &declarations)
                    .ok()??;
                self.number_kind(&kind, scope)
            }
            _ => None,
        }
    }
    fn inference_expr(
        &self,
        e: &Expr,
        scope: usize,
        decls: &mut HashMap<String, (TypeSpec, usize)>,
    ) -> Option<Expr> {
        Some(match e {
            Expr::Ident(n) => {
                let (k, _, dims, _, owner) = self.data_type(scope, n)?;
                let ntype = self.number_kind(&k?, owner)?;
                decls.insert(n.clone(), (ntype.spec(), dims.len()));
                e.clone()
            }
            Expr::IntLit(s) => Expr::Call(
                "@integer_literal".into(),
                vec![
                    Expr::IntLit(s.split('_').next()?.into()),
                    Expr::IntLit(self.numeric_type(e, scope)?.bytes.to_string()),
                ],
            ),
            Expr::RealLit(s) => {
                let literal = Expr::RealLit(s.split('_').next()?.replace(['d', 'D'], "e"));
                if self.numeric_type(e, scope)?.bytes == 8 {
                    Expr::Call("dble".into(), vec![literal])
                } else {
                    literal
                }
            }
            Expr::Call(n, a) => {
                let mut args = vec![];
                for arg in self.call_args(n, a) {
                    if let Some(i) = self.integer(&arg, scope, &mut HashSet::new()) {
                        args.push(Expr::IntLit(i.to_string()));
                    } else {
                        args.push(self.inference_expr(&arg, scope, decls)?);
                    }
                }
                Expr::Call(n.clone(), args)
            }
            Expr::Bin(op, x, y) => Expr::Bin(
                op.clone(),
                Box::new(self.inference_expr(x, scope, decls)?),
                Box::new(self.inference_expr(y, scope, decls)?),
            ),
            Expr::Un(op, x) => {
                Expr::Un(op.clone(), Box::new(self.inference_expr(x, scope, decls)?))
            }
            _ => e.clone(),
        })
    }
    fn constant_number(
        &self,
        e: &Expr,
        scope: usize,
        seen: &mut HashSet<usize>,
        rounded: bool,
    ) -> Option<(f64, f64)> {
        if self.numeric_type(e, scope)?.class == 0 {
            return self
                .integer(e, scope, &mut HashSet::new())
                .map(|n| (n as f64, 0.0));
        }
        let v = match value(e) {
            Expr::RealLit(s) => (
                s.split('_').next()?.replace(['d', 'D'], "e").parse().ok()?,
                0.0,
            ),
            Expr::Ident(n) => {
                let id = self.resolve(scope, n)?;
                if !seen.insert(id) {
                    return None;
                }
                let d = &self.decls[id];
                let v = self.constant_number(d.constant.as_ref()?, d.scope, seen, rounded);
                seen.remove(&id);
                v?
            }
            Expr::Un(UnOp::Neg, x) => {
                let (x, y) = self.constant_number(x, scope, seen, rounded)?;
                (-x, -y)
            }
            Expr::Bin(op, x, y) => {
                let (a, b) = self.constant_number(x, scope, seen, rounded)?;
                let (c, d) = self.constant_number(y, scope, seen, rounded)?;
                match op {
                    BinOp::Add => (a + c, b + d),
                    BinOp::Sub => (a - c, b - d),
                    BinOp::Mul => (a * c - b * d, a * d + b * c),
                    BinOp::Div if b == 0.0 && d == 0.0 => (a / c, 0.0),
                    BinOp::Div => {
                        let den = c * c + d * d;
                        ((a * c + b * d) / den, (b * c - a * d) / den)
                    }
                    BinOp::Pow if b == 0.0 && d == 0.0 => (a.powf(c), 0.0),
                    _ => return None,
                }
            }
            Expr::Call(n, a) if self.resolve(scope, n).is_none() => {
                let args = self.call_args(n, a);
                let (x, y) = self.constant_number(args.first()?, scope, seen, rounded)?;
                match n.as_str() {
                    "@complex_literal" => (
                        x,
                        self.constant_number(args.get(1)?, scope, seen, rounded)?.0,
                    ),
                    "cmplx" => (
                        x,
                        args.get(1)
                            .filter(|e| !crate::named_arguments::omitted(e))
                            .map(|e| self.constant_number(e, scope, seen, rounded).map(|v| v.0))
                            .unwrap_or(Some(y))?,
                    ),
                    "real" | "dble" => (x, 0.0),
                    "aimag" => (y, 0.0),
                    "conjg" => (x, -y),
                    "abs" => (x.hypot(y), 0.0),
                    "@unary_plus" => (x, y),
                    _ if y == 0.0 => {
                        let e = Expr::Call(n.clone(), vec![Expr::RealLit(x.to_string())]);
                        (crate::parameters::real_constant(&e).ok()?, 0.0)
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        let v = if rounded && self.numeric_type(e, scope)?.bytes == 4 {
            ((v.0 as f32) as f64, (v.1 as f32) as f64)
        } else {
            v
        };
        (v.0.is_finite() && v.1.is_finite()).then_some(v)
    }
    fn rhs_span(&self, offset: usize) -> Range<usize> {
        let ts = self.statement_tokens(offset);
        let start = ts
            .iter()
            .position(|t| matches!(t.kind, TokenKind::DColon))
            .map_or(0, |i| i + 1);
        ts[start..]
            .iter()
            .position(|t| matches!(t.kind, TokenKind::Eq))
            .and_then(|i| ts.get(start + i + 1))
            .map(|t| t.span.clone())
            .unwrap_or(offset..offset)
    }
    fn safe_conversion(&self, e: &Expr, scope: usize, from: Number, to: Number) -> bool {
        // Invalid/unsupported kinds belong to the existing type diagnostics.
        if !matches!(to.bytes, 1 | 2 | 4 | 8) || !matches!(from.bytes, 1 | 2 | 4 | 8) {
            return true;
        }
        if let Expr::Call(n, a) = e {
            if n == "@array_constructor" {
                return a.iter().all(|e| self.safe_conversion(e, scope, from, to));
            }
            if n == "@typed_constructor" {
                return a[1..]
                    .iter()
                    .all(|e| self.safe_conversion(e, scope, from, to));
            }
        }
        if from.class == 0 {
            if let Some(i) = self.integer(e, scope, &mut HashSet::new()) {
                if to.class == 0 {
                    let bits = u32::from(to.bytes) * 8;
                    return i128::from(i) >= -(1i128 << (bits - 1))
                        && i128::from(i) < (1i128 << (bits - 1));
                }
                return if to.bytes == 4 {
                    (i as f32) as i128 == i128::from(i)
                } else {
                    (i as f64) as i128 == i128::from(i)
                };
            }
            return to.class > 0
                && u32::from(from.bytes) * 8 - 1 <= if to.bytes == 4 { 24 } else { 53 };
        }
        let Some((mut re, mut im)) = self.constant_number(e, scope, &mut HashSet::new(), true)
        else {
            return false;
        };
        if from.bytes == 4 {
            re = (re as f32) as f64;
            im = (im as f32) as f64;
        }
        if to.class < 2 && im != 0.0 {
            return false;
        }
        if to.class == 0 {
            let limit = 2f64.powi(i32::from(to.bytes) * 8 - 1);
            return re.fract() == 0.0 && re >= -limit && re < limit;
        }
        to.bytes >= from.bytes
            || (re as f32) as f64 == re && (to.class < 2 || (im as f32) as f64 == im)
    }
    fn precision_issue(&self, e: &Expr, scope: usize, expected: Number) -> Option<Number> {
        let actual = self.numeric_type(e, scope)?;
        if actual.class == 0 {
            return None;
        }
        if let Expr::Call(n, a) = e {
            if n == "@array_constructor" || n == "@typed_constructor" {
                let (items, context) = if n == "@typed_constructor" {
                    (
                        &a[1..],
                        if actual.bytes < expected.bytes {
                            actual
                        } else {
                            expected
                        },
                    )
                } else {
                    (a.as_slice(), expected)
                };
                return items
                    .iter()
                    .find_map(|e| self.precision_issue(e, scope, context));
            }
        }
        if let Expr::Call(n, _) = e {
            if self.resolve(scope, n).is_none()
                && matches!(n.as_str(), "kind" | "rank" | "size" | "lbound" | "ubound")
            {
                return None;
            }
        }
        let explicit = matches!(e,Expr::Call(n,_) if self.resolve(scope,n).is_none() && matches!(n.as_str(),"real"|"dble"|"cmplx"));
        let computed = matches!(
            e,
            Expr::RealLit(_) | Expr::Bin(..) | Expr::Un(..) | Expr::Call(..)
        );
        if computed && !explicit && actual.bytes < expected.bytes {
            let exact = self
                .constant_number(e, scope, &mut HashSet::new(), false)
                .is_some_and(|(x, y)| {
                    actual.bytes == 8 || (x as f32) as f64 == x && (y as f32) as f64 == y
                });
            if !exact {
                return Some(actual);
            }
        }
        // A conversion intentionally narrowing its input does not request a
        // higher-precision computation. Widening conversions still reveal
        // lower-precision work in their arguments.
        let context = Number {
            class: actual.class,
            bytes: actual.bytes.max(expected.bytes),
        };
        let context = if explicit && actual.bytes < expected.bytes {
            actual
        } else {
            context
        };
        for child in expressions(e) {
            if let Some(kind) = self.precision_issue(value(child), scope, context) {
                return Some(kind);
            }
        }
        None
    }
    fn numeric_assignment(
        &mut self,
        name: &str,
        e: &Expr,
        to: Number,
        scope: usize,
        offset: usize,
    ) {
        let Some(from) = self.numeric_type(e, scope) else {
            return;
        };
        let risky = from.class > to.class
            || from.class == 0 && to.class == 0 && from.bytes > to.bytes
            || from.class > 0 && to.class > 0 && from.bytes > to.bytes
            || from.class == 0 && to.class > 0;
        if risky && !self.safe_conversion(e, scope, from, to) {
            self.warnings.push(Advice{category:"conversion",span:self.rhs_span(offset),
                message:format!("implicit conversion from {} to {} in assignment to `{name}` may lose information",from.label(),to.label()),related:None,
                note:"use an explicit conversion when the loss is intentional, or choose a destination type and kind that preserve the value"});
        }
        if to.class > 0 {
            if let Some(low) = self
                .precision_issue(e, scope, to)
                .filter(|low| low.bytes < to.bytes)
            {
                self.warnings.push(Advice{category:"precision-loss",span:self.rhs_span(offset),
                    message:format!("expression assigned to `{name}` uses {} precision before widening to {}",low.label(),to.label()),related:None,
                    note:"give literals and computations the intended kind; conversion after a calculation cannot restore lost precision"});
            }
        }
    }
    pub(super) fn pointer_initializers(&mut self) {
        for t in &self.types {
            let mut offset = 0;
            let mut pending = HashMap::new();
            let mut pointers = HashSet::new();
            for s in t.components {
                if let Stmt::SourceLocation(n) = s {
                    offset = *n;
                    continue;
                }
                if let Stmt::DummyIntent { name, intent } = s {
                    if intent == "pointer" {
                        pointers.insert(name.clone());
                    }
                }
                if let Stmt::DerivedDecl {
                    names, attributes, ..
                } = s
                {
                    if attributes.iter().any(|a| a == "pointer") {
                        pointers.extend(names.iter().cloned());
                    }
                }
                if let Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } = s
                {
                    if attributes.iter().any(|a| a == "pointer") {
                        pointers.extend(entities.iter().map(|(n, _)| n.clone()));
                    }
                }
                let names: Vec<&String> = match s {
                    Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                        names.iter().collect()
                    }
                    Stmt::ArrayDecl { name, .. } => vec![name],
                    Stmt::InitializedDecl { entities, .. } => {
                        entities.iter().map(|(n, _)| n).collect()
                    }
                    _ => vec![],
                };
                for n in names {
                    pending.insert(n.clone(), offset);
                }
            }
            for (name, offset) in pending {
                let ts = self.statement_tokens(offset);
                if !pointers.contains(&name) {
                    continue;
                }
                if crate::declaration_warnings::initializer(ts, &name).is_some() {
                    continue;
                }
                self.warnings.push(Advice{category:"pointer-init",span:self.span(offset,&name),
                    message:format!("pointer component `{}%{name}` has no default initialization",t.name),related:None,
                    note:"consider => NULL(); without initialization its association status is undefined, so ASSOCIATED is not a safe first test"});
            }
        }
    }
    fn enum_type(&self, e: &Expr, scope: usize) -> Option<usize> {
        match value(e) {
            Expr::Ident(n) | Expr::Index(n, _) => self
                .data_type(scope, n)?
                .1
                .filter(|t| self.types[*t].enumeration),
            Expr::Call(n, _) => {
                if let Some(t) = self
                    .type_lookup(scope, n)
                    .filter(|t| self.types[*t].enumeration)
                {
                    return Some(t);
                }
                let d = &self.decls[self.resolve(scope, n)?];
                if let Some(p) = d.procedure {
                    let ps = self.procedures[p].scope;
                    return self.scopes[ps].names.values().find_map(|id| {
                        let d = &self.decls[*id];
                        if !d.result {
                            return None;
                        }
                        self.type_lookup(ps, d.nominal.as_deref()?)
                            .filter(|t| self.types[*t].enumeration)
                    });
                }
                self.data_type(scope, n)?
                    .1
                    .filter(|t| self.types[*t].enumeration)
            }
            _ => None,
        }
    }
    fn enum_value(
        &self,
        e: &Expr,
        scope: usize,
        wanted: usize,
        seen: &mut HashSet<usize>,
    ) -> Option<i64> {
        match value(e) {
            Expr::Ident(n) => {
                let id = self.resolve(scope, n)?;
                let d = &self.decls[id];
                if !d.attrs.contains("parameter") || !seen.insert(id) {
                    return None;
                }
                let result = self.enum_value(d.constant.as_ref()?, d.scope, wanted, seen);
                seen.remove(&id);
                result
            }
            Expr::Call(n, a) if self.type_lookup(scope, n) == Some(wanted) && a.len() == 1 => {
                self.bound_integer(&a[0], scope)
            }
            _ => self.bound_integer(e, scope),
        }
    }
    fn enum_selection(&mut self, s: &Stmt, scope: usize, offset: usize) {
        let Stmt::SelectCase {
            expr,
            cases,
            default,
        } = s
        else {
            return;
        };
        if default.is_some() {
            return;
        }
        let Some(tid) = self.enum_type(expr, scope) else {
            return;
        };
        let t = &self.types[tid];
        let mut members = vec![];
        for d in &self.decls {
            if d.scope != t.scope
                || !d.attrs.contains("parameter")
                || !d.attrs.contains("@enum_member") && !d.attrs.contains("@warning_enum_member")
                || d.nominal
                    .as_ref()
                    .and_then(|n| self.type_lookup(d.scope, n))
                    != Some(tid)
            {
                continue;
            }
            let Some(v) = d
                .constant
                .as_ref()
                .and_then(|e| self.enum_value(e, d.scope, tid, &mut HashSet::new()))
            else {
                return;
            };
            members.push((d.name.clone(), v, d.span.start));
        }
        if members.is_empty() {
            return;
        }
        let mut ranges = vec![];
        for c in cases {
            for item in &c.items {
                let range = match item {
                    CaseItem::Single(e) => self
                        .enum_value(e, scope, tid, &mut HashSet::new())
                        .map(|n| (n, n)),
                    CaseItem::Range(l, u) => {
                        let bound = |e: &Expr, default| {
                            if matches!(e,Expr::Call(n,_) if n=="@case_unbounded") {
                                Some(default)
                            } else {
                                self.enum_value(e, scope, tid, &mut HashSet::new())
                            }
                        };
                        bound(l, i64::MIN).zip(bound(u, i64::MAX))
                    }
                };
                let Some(range) = range else { return };
                ranges.push(range);
            }
        }
        members.sort_by_key(|m| m.2);
        let missing = members
            .iter()
            .filter(|(_, v, _)| !ranges.iter().any(|(l, u)| l <= v && v <= u))
            .map(|(n, _, _)| format!("`{n}`"))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            self.warnings.push(Advice{category:"enum-exhaustive",span:offset..offset,
            message:format!("SELECT CASE for enumeration `{}` does not cover {}",t.name,missing.join(", ")),related:None,
            note:"add cases for the missing values, or CASE DEFAULT to handle the remaining values deliberately"});
        }
    }
    fn dimensions(&self, name: &str, scope: usize) -> Option<Vec<(i64, i64)>> {
        let (_, _, dims, attrs, owner) = self.data_type(scope, name)?;
        if dims.is_empty() || attrs.contains("pointer") || attrs.contains("allocatable") {
            return None;
        }
        dims.iter()
            .map(|d| match d {
                Expr::Call(n, b) if n == "@declared_bounds" && b.len() == 2 => Some((
                    self.integer(&b[0], owner, &mut HashSet::new())?,
                    self.integer(&b[1], owner, &mut HashSet::new())?,
                )),
                d => Some((1, self.integer(d, owner, &mut HashSet::new())?)),
            })
            .collect()
    }
    fn bound_integer(&self, e: &Expr, scope: usize) -> Option<i64> {
        if let Some(n) = self.integer(e, scope, &mut HashSet::new()) {
            return Some(n);
        }
        match e {
            Expr::Bin(op, x, y) => {
                let (x, y) = (self.bound_integer(x, scope)?, self.bound_integer(y, scope)?);
                return match op {
                    BinOp::Add => x.checked_add(y),
                    BinOp::Sub => x.checked_sub(y),
                    BinOp::Mul => x.checked_mul(y),
                    BinOp::Div => x.checked_div(y),
                    _ => None,
                };
            }
            Expr::Un(UnOp::Neg, x) => return self.bound_integer(x, scope)?.checked_neg(),
            _ => {}
        }
        if let Expr::Call(n, a) = e {
            if self.resolve(scope, n).is_none()
                && matches!(n.as_str(), "size" | "lbound" | "ubound")
            {
                let a = self.call_args(n, a);
                let Expr::Ident(name) = a.first()? else {
                    return None;
                };
                let dims = self.dimensions(name, scope)?;
                if a.get(1)
                    .is_some_and(|e| !crate::named_arguments::omitted(e))
                {
                    let dim = self.integer(&a[1], scope, &mut HashSet::new())?;
                    let (l, u) = dims.get(usize::try_from(dim.checked_sub(1)?).ok()?)?;
                    return match n.as_str() {
                        "lbound" => Some(*l),
                        "ubound" => Some(*u),
                        _ => u.checked_sub(*l)?.checked_add(1).map(|n| n.max(0)),
                    };
                }
                if n == "size" {
                    return dims.iter().try_fold(1i64, |p, (l, u)| {
                        p.checked_mul(u.checked_sub(*l)?.checked_add(1)?.max(0))
                    });
                }
            }
        }
        None
    }
    fn interval(&self, e: &Expr, scope: usize, env: &Intervals) -> Option<(i64, i64)> {
        if let Some(n) = self.bound_integer(e, scope) {
            return Some((n, n));
        }
        match value(e) {
            Expr::Ident(n) => env.get(&self.resolve(scope, n)?).copied(),
            Expr::Un(UnOp::Neg, x) => {
                let (l, u) = self.interval(x, scope, env)?;
                Some((u.checked_neg()?, l.checked_neg()?))
            }
            Expr::Call(n, a) if n == "@unary_plus" && a.len() == 1 => {
                self.interval(&a[0], scope, env)
            }
            Expr::Bin(op, x, y) => {
                let (l, u) = self.interval(x, scope, env)?;
                let (m, v) = self.interval(y, scope, env)?;
                match op {
                    BinOp::Add => Some((l.checked_add(m)?, u.checked_add(v)?)),
                    BinOp::Sub => Some((l.checked_sub(v)?, u.checked_sub(m)?)),
                    BinOp::Mul => {
                        let vals = [
                            l.checked_mul(m)?,
                            l.checked_mul(v)?,
                            u.checked_mul(m)?,
                            u.checked_mul(v)?,
                        ];
                        Some((*vals.iter().min()?, *vals.iter().max()?))
                    }
                    BinOp::Div if m == v && m != 0 => {
                        let (a, b) = (l.checked_div(m)?, u.checked_div(m)?);
                        Some((a.min(b), a.max(b)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
    fn guard(&self, e: &Expr, truth: bool, scope: usize, env: &mut Intervals) -> bool {
        if let Expr::Un(UnOp::Not, x) = e {
            return self.guard(x, !truth, scope, env);
        }
        if let Expr::Bin(op, x, y) = e {
            if matches!((op, truth), (BinOp::And, true) | (BinOp::Or, false)) {
                return self.guard(x, truth, scope, env) && self.guard(y, truth, scope, env);
            }
            let op = match (op, truth) {
                (BinOp::Lt, true) | (BinOp::Ge, false) => BinOp::Lt,
                (BinOp::Le, true) | (BinOp::Gt, false) => BinOp::Le,
                (BinOp::Gt, true) | (BinOp::Le, false) => BinOp::Gt,
                (BinOp::Ge, true) | (BinOp::Lt, false) => BinOp::Ge,
                (BinOp::Eq, true) | (BinOp::Ne, false) => BinOp::Eq,
                (BinOp::Ne, true) | (BinOp::Eq, false) => BinOp::Ne,
                _ => return true,
            };
            // Normalize simple affine guards to variable OP constant.
            fn affine(a: &Analysis<'_>, e: &Expr, scope: usize) -> Option<(usize, i64)> {
                match e {
                    Expr::Ident(n) => Some((a.resolve(scope, n)?, 0)),
                    Expr::Bin(BinOp::Add, x, y) => {
                        if let Some((id, k)) = affine(a, x, scope) {
                            Some((id, k.checked_add(a.bound_integer(y, scope)?)?))
                        } else {
                            let (id, k) = affine(a, y, scope)?;
                            Some((id, k.checked_add(a.bound_integer(x, scope)?)?))
                        }
                    }
                    Expr::Bin(BinOp::Sub, x, y) => {
                        let (id, k) = affine(a, x, scope)?;
                        Some((id, k.checked_sub(a.bound_integer(y, scope)?)?))
                    }
                    _ => None,
                }
            }
            let (id, c, op) = if let (Some((id, k)), Some(c)) =
                (affine(self, x, scope), self.bound_integer(y, scope))
            {
                let Some(c) = c.checked_sub(k) else {
                    return true;
                };
                (id, c, op)
            } else if let (Some(c), Some((id, k))) =
                (self.bound_integer(x, scope), affine(self, y, scope))
            {
                let Some(c) = c.checked_sub(k) else {
                    return true;
                };
                (
                    id,
                    c,
                    match op {
                        BinOp::Lt => BinOp::Gt,
                        BinOp::Le => BinOp::Ge,
                        BinOp::Gt => BinOp::Lt,
                        BinOp::Ge => BinOp::Le,
                        op => op,
                    },
                )
            } else {
                return true;
            };
            if let Some((l, u)) = env.get_mut(&id) {
                match op {
                    BinOp::Lt => {
                        let Some(c) = c.checked_sub(1) else {
                            return false;
                        };
                        *u = (*u).min(c);
                    }
                    BinOp::Le => *u = (*u).min(c),
                    BinOp::Gt => {
                        let Some(c) = c.checked_add(1) else {
                            return false;
                        };
                        *l = (*l).max(c);
                    }
                    BinOp::Ge => *l = (*l).max(c),
                    BinOp::Eq => {
                        *l = (*l).max(c);
                        *u = (*u).min(c);
                    }
                    BinOp::Ne => {
                        if *l == c {
                            let Some(n) = l.checked_add(1) else {
                                return false;
                            };
                            *l = n;
                        }
                        if *u == c {
                            let Some(n) = u.checked_sub(1) else {
                                return false;
                            };
                            *u = n;
                        }
                    }
                    _ => {}
                }
                return *l <= *u;
            }
        }
        true
    }
    fn check_indices(
        &mut self,
        name: &str,
        args: &[Expr],
        scope: usize,
        offset: usize,
        env: &Intervals,
    ) {
        if env.is_empty() {
            return;
        }
        let Some(dims) = self.dimensions(name, scope) else {
            return;
        };
        if dims.len() != args.len() {
            return;
        }
        for (axis, (index, (lower, upper))) in args.iter().zip(dims).enumerate() {
            if matches!(index, Expr::Section { .. } | Expr::Call(..)) {
                continue;
            }
            // Constant invalid accesses retain the existing hard error.
            if self.bound_integer(index, scope).is_some() {
                continue;
            }
            let Some((l, u)) = self.interval(index, scope, env) else {
                continue;
            };
            if l < lower || u > upper {
                self.warnings.push(Advice{category:"do-subscript",span:self.span(offset,root(name)),
                message:format!("loop-derived subscript range {l}:{u} for `{name}` dimension {} may exceed bounds {lower}:{upper}",axis+1),
                related:self.resolve(scope,name).map(|id|(self.decls[id].span.clone(),"array declared here".into())),
                note:"check the loop bounds, offsets, and guards; the range describes possible values, not proof that every value is accessed"});
            }
        }
    }
    fn index_expressions(&mut self, e: &Expr, scope: usize, offset: usize, env: &Intervals) {
        match e {
            Expr::Index(n, a) | Expr::Call(n, a)
                if self
                    .data_type(scope, n)
                    .is_some_and(|(_, _, d, _, _)| !d.is_empty()) =>
            {
                self.check_indices(n, a, scope, offset, env)
            }
            Expr::Call(n, _)
                if self.resolve(scope, n).is_none() && matches!(n.as_str(), "kind" | "rank") =>
            {
                return
            }
            _ => {}
        }
        for child in expressions(e) {
            self.index_expressions(child, scope, offset, env);
        }
    }
    fn loop_range(
        &self,
        var: &str,
        start: &Expr,
        end: &Expr,
        step: i64,
        scope: usize,
        env: &Intervals,
    ) -> Option<Intervals> {
        if step == 0 {
            return None;
        }
        let (a, b) = (
            self.interval(start, scope, env)?,
            self.interval(end, scope, env)?,
        );
        if a.0 != a.1 || b.0 != b.1 {
            let (lo, hi) = if step > 0 { (a.0, b.1) } else { (b.0, a.1) };
            if lo > hi {
                return Some(HashMap::from([(usize::MAX, (1, 0))]));
            }
            let mut result = env.clone();
            result.insert(self.resolve(scope, var)?, (lo, hi));
            return Some(result);
        }
        let (start, end) = (i128::from(a.0), i128::from(b.0));
        let step = i128::from(step);
        let mut result = env.clone();
        if step > 0 && start > end || step < 0 && start < end {
            return Some(HashMap::from([(usize::MAX, (1, 0))]));
        }
        let last = start + (end - start) / step * step;
        result.insert(
            self.resolve(scope, var)?,
            (
                start.min(last).try_into().ok()?,
                start.max(last).try_into().ok()?,
            ),
        );
        Some(result)
    }
    fn transfer(body: &[Stmt]) -> bool {
        matches!(
            body.iter()
                .rev()
                .find(|s| !matches!(s, Stmt::SourceLocation(_))),
            Some(Stmt::Return(_) | Stmt::Exit | Stmt::Cycle)
        ) || matches!(body.iter().rev().find(|s|!matches!(s,Stmt::SourceLocation(_))),Some(Stmt::CallSub{name,..}) if matches!(name.as_str(),"@stop"|"@error_stop"|"@goto"))
    }
    pub(super) fn value_walk(
        &mut self,
        body: &'a [Stmt],
        scope: usize,
        mut offset: usize,
        inherited: &Intervals,
    ) {
        let scope = self
            .bodies
            .get(&(body.as_ptr() as usize))
            .copied()
            .unwrap_or(scope);
        if inherited.contains_key(&usize::MAX) {
            return;
        }
        let mut env = inherited.clone();
        // Non-unit DO loops are represented by a BLOCK and @do_step marker.
        for s in body {
            if let Stmt::CallSub { name, args } = s {
                if name == "@do_step" {
                    if let [Expr::Ident(var), start, end, step] = args.as_slice() {
                        if let Some(e) = self
                            .bound_integer(step, scope)
                            .and_then(|step| self.loop_range(var, start, end, step, scope, &env))
                        {
                            env = e;
                        }
                        if env.contains_key(&usize::MAX) {
                            return;
                        }
                    }
                }
            }
        }
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if let Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } = s
            {
                self.value_walk(body, scope, offset, &Intervals::new());
                continue;
            }
            if self.dead.contains(&(s as *const Stmt as usize)) {
                continue;
            }
            match s {
                Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. } => {
                    if let Some((Some(k), _, _, _, owner)) = self.data_type(scope, name) {
                        if let Some(to) = self.number_kind(&k, owner) {
                            self.numeric_assignment(name, value, to, scope, offset);
                        }
                    }
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    if let Some(to) = self.number_kind(kind, scope) {
                        for (n, e) in entities {
                            if let Some(e) = e {
                                self.numeric_assignment(n, e, to, scope, offset);
                            }
                        }
                    }
                }
                Stmt::SelectCase { .. } => self.enum_selection(s, scope, offset),
                Stmt::DerivedType { components, .. } => {
                    self.value_walk(components, scope, offset, &Intervals::new());
                    continue;
                }
                _ => {}
            }
            if let Stmt::AssignIndex { name, indices, .. } = s {
                self.check_indices(name, indices, scope, offset, &env);
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                self.index_expressions(e, scope, offset, &env);
            }
            match s {
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    let next = self
                        .loop_range(var, start, end, 1, scope, &env)
                        .unwrap_or_else(|| {
                            let mut e = env.clone();
                            if let Some(id) = self.resolve(scope, var) {
                                e.remove(&id);
                            }
                            e
                        });
                    self.value_walk(body, scope, offset, &next);
                    // Values changed by a loop are not carried into later code.
                    env.retain(|id, _| !self.procedure_writes(body, scope, *id));
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let mut yes = env.clone();
                    let yes_possible = self.guard(cond, true, scope, &mut yes);
                    let mut no = env.clone();
                    let no_possible = self.guard(cond, false, scope, &mut no);
                    if yes_possible {
                        self.value_walk(then_body, scope, offset, &yes);
                    }
                    if let Some(b) = else_body {
                        if no_possible {
                            self.value_walk(b, scope, offset, &no);
                        }
                    }
                    if Self::transfer(then_body) && no_possible {
                        env = no;
                    } else if else_body.as_ref().is_some_and(|b| Self::transfer(b)) && yes_possible
                    {
                        env = yes;
                    } else {
                        env.retain(|id, _| {
                            !self.procedure_writes(then_body, scope, *id)
                                && !else_body
                                    .as_ref()
                                    .is_some_and(|b| self.procedure_writes(b, scope, *id))
                        });
                    }
                    // A guard narrows the incoming values; it does not retain
                    // facts about variables subsequently assigned in a branch.
                    env.retain(|id, _| {
                        !self.procedure_writes(then_body, scope, *id)
                            && !else_body
                                .as_ref()
                                .is_some_and(|b| self.procedure_writes(b, scope, *id))
                    });
                }
                Stmt::Assign { name, value } if !env.is_empty() => {
                    if let Some(id) = self.resolve(scope, name) {
                        if let Some(r) = self.interval(value, scope, &env) {
                            env.insert(id, r);
                        } else {
                            env.remove(&id);
                        }
                    }
                }
                Stmt::CallSub { name, .. } if !name.starts_with('@') => env.clear(),
                Stmt::CallSub { name, .. } if name.contains("read") => env.clear(),
                _ => {
                    for b in children(s) {
                        self.value_walk(b, scope, offset, &env);
                        env.retain(|id, _| !self.procedure_writes(b, scope, *id));
                    }
                }
            }
        }
    }
}

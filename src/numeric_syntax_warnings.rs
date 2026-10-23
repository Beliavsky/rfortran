// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Conservative constant magnitude proofs do not depend on host underflow.
use super::*;

#[derive(Clone, Copy)]
struct Magnitude {
    sign: i8,
    log2: f64,
}
impl Magnitude {
    fn zero() -> Self {
        Self { sign: 0, log2: 0.0 }
    }
    fn float(n: f64) -> Option<Self> {
        n.is_finite().then(|| {
            if n == 0.0 {
                Self::zero()
            } else {
                Self {
                    sign: if n < 0.0 { -1 } else { 1 },
                    log2: n.abs().log2(),
                }
            }
        })
    }
    fn literal(text: &str) -> Option<Self> {
        let text = text
            .split('_')
            .next()?
            .to_ascii_lowercase()
            .replace('d', "e");
        let (number, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let negative = number.starts_with('-');
        let number = number.trim_start_matches(['+', '-']);
        let decimal = number.find('.').unwrap_or(number.len());
        let digits = number.bytes().filter(|c| *c != b'.').collect::<Vec<_>>();
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let Some(first) = digits.iter().position(|c| *c != b'0') else {
            return Some(Self::zero());
        };
        let leading = digits[first..]
            .iter()
            .take(16)
            .fold(0f64, |n, c| n * 10.0 + f64::from(*c - b'0'));
        let count = (digits.len() - first).min(16);
        let exponent = exponent
            .parse::<i64>()
            .map(|n| n.clamp(-1_000_000, 1_000_000))
            .unwrap_or_else(|_| {
                if exponent.starts_with('-') {
                    -1_000_000
                } else {
                    1_000_000
                }
            });
        let power = exponent as f64 + decimal as f64 - first as f64 - count as f64;
        Some(Self {
            sign: if negative { -1 } else { 1 },
            log2: leading.log2() + power * std::f64::consts::LOG2_10,
        })
    }
}
impl<'a> Analysis<'a> {
    fn magnitude(&self, e: &Expr, scope: usize, seen: &mut HashSet<usize>) -> Option<Magnitude> {
        let kind = self.numeric_type(e, scope)?;
        if kind.class == 2 {
            return None;
        }
        if kind.class == 0 {
            return Magnitude::float(self.integer(e, scope, &mut HashSet::new())? as f64);
        }
        let m = match value(e) {
            Expr::RealLit(text) => Magnitude::literal(text)?,
            Expr::Ident(n) => {
                let id = self.resolve(scope, n)?;
                if !seen.insert(id) {
                    return None;
                }
                let d = &self.decls[id];
                // A stored PARAMETER already rounded to zero is zero at its
                // uses, not a new underflow at every reference.
                let result = if self
                    .constant_number(e, scope, &mut HashSet::new(), true)
                    .is_some_and(|(x, y)| x == 0.0 && y == 0.0)
                {
                    Some(Magnitude::zero())
                } else {
                    self.magnitude(d.constant.as_ref()?, d.scope, seen)
                };
                seen.remove(&id);
                return result;
            }
            Expr::Un(UnOp::Neg, x) => {
                let mut m = self.magnitude(x, scope, seen)?;
                m.sign = -m.sign;
                m
            }
            Expr::Bin(op, x, y) => {
                let (x, y) = (
                    self.magnitude(x, scope, seen)?,
                    self.magnitude(y, scope, seen)?,
                );
                match op {
                    BinOp::Mul => {
                        if x.sign == 0 || y.sign == 0 {
                            Magnitude::zero()
                        } else {
                            Magnitude {
                                sign: x.sign * y.sign,
                                log2: x.log2 + y.log2,
                            }
                        }
                    }
                    BinOp::Div if y.sign != 0 => {
                        if x.sign == 0 {
                            Magnitude::zero()
                        } else {
                            Magnitude {
                                sign: x.sign * y.sign,
                                log2: x.log2 - y.log2,
                            }
                        }
                    }
                    BinOp::Add | BinOp::Sub => {
                        let mut y = y;
                        if matches!(op, BinOp::Sub) {
                            y.sign = -y.sign;
                        }
                        if x.sign == 0 {
                            y
                        } else if y.sign == 0 {
                            x
                        } else if x.sign == y.sign {
                            let hi = x.log2.max(y.log2);
                            let lo = x.log2.min(y.log2);
                            Magnitude {
                                sign: x.sign,
                                log2: hi + (1.0 + 2f64.powf(lo - hi)).log2(),
                            }
                        } else {
                            // Cancellation needs a trustworthy nonzero result.
                            Magnitude::float(
                                self.constant_number(e, scope, &mut HashSet::new(), false)?
                                    .0,
                            )?
                        }
                    }
                    BinOp::Pow if y.sign != 0 => {
                        let Expr::Bin(_, _, exponent) = e else {
                            return None;
                        };
                        let exponent = self.integer(exponent, scope, &mut HashSet::new())?;
                        if x.sign == 0 {
                            if exponent > 0 {
                                Magnitude::zero()
                            } else {
                                return None;
                            }
                        } else {
                            Magnitude {
                                sign: if x.sign < 0 && exponent % 2 != 0 {
                                    -1
                                } else {
                                    1
                                },
                                log2: x.log2 * exponent as f64,
                            }
                        }
                    }
                    BinOp::Pow if y.sign == 0 => Magnitude { sign: 1, log2: 0.0 },
                    _ => return None,
                }
            }
            Expr::Call(n, args) if self.resolve(scope, n).is_none() => {
                let args = self.call_args(n, args);
                match n.as_str() {
                    "real" | "dble" | "abs" | "@unary_plus" => {
                        self.magnitude(args.first()?, scope, seen)?
                    }
                    "tiny" => {
                        let k = self.numeric_type(args.first()?, scope)?;
                        Magnitude {
                            sign: 1,
                            log2: if k.bytes == 4 {
                                -126.0
                            } else if k.bytes == 8 {
                                -1022.0
                            } else {
                                return None;
                            },
                        }
                    }
                    "huge" => {
                        let k = self.numeric_type(args.first()?, scope)?;
                        Magnitude {
                            sign: 1,
                            log2: match k.bytes {
                                4 => (f32::MAX as f64).log2(),
                                8 => f64::MAX.log2(),
                                _ => return None,
                            },
                        }
                    }
                    "exp" => {
                        let (x, y) =
                            self.constant_number(args.first()?, scope, &mut HashSet::new(), false)?;
                        if y != 0.0 {
                            return None;
                        }
                        Magnitude {
                            sign: 1,
                            log2: x * std::f64::consts::LOG2_E,
                        }
                    }
                    _ => {
                        let (x, y) = self.constant_number(e, scope, &mut HashSet::new(), false)?;
                        if y != 0.0 {
                            return None;
                        }
                        Magnitude::float(x)?
                    }
                }
            }
            _ => return None,
        };
        (m.log2.is_finite()
            && (m.sign == 0 || m.log2 <= if kind.bytes == 4 { 128.0 } else { 1024.0 }))
        .then_some(m)
    }
    fn underflow_expression(
        &mut self,
        e: &Expr,
        scope: usize,
        offset: usize,
        expected: Option<Number>,
    ) -> bool {
        let inquiry = matches!(e,Expr::Call(n,_) if self.resolve(scope,n).is_none()&&(
            crate::inquiries::numeric_model(n)||matches!(n.as_str(),"kind"|"rank"|"tiny"|"huge"|"bit_size"|"storage_size")));
        let actual = self.numeric_type(e, scope);
        let context = actual
            .filter(|n| n.class > 0)
            .map(|n| Number {
                bytes: expected
                    .filter(|e| e.class > 0)
                    .map_or(n.bytes, |e| n.bytes.min(e.bytes)),
                ..n
            })
            .or(expected);
        if let Some(k) = context.filter(|k| k.class > 0 && matches!(k.bytes, 4 | 8)) {
            if let Some(m) = self.magnitude(e, scope, &mut HashSet::new()) {
                let threshold = if k.bytes == 4 { -150.0 } else { -1075.0 };
                // Keep approximate logarithms away from halfway boundaries.
                if m.sign != 0 && m.log2 < threshold - 1e-6 {
                    self.warnings.push(Advice{category:"underflow",span:self.rhs_span(offset),
                        message:format!("nonzero constant rounds to zero in {}",Number{class:1,bytes:k.bytes}.label()),related:None,
                        note:"choose a kind with greater exponent range or rescale the calculation; a subnormal value that remains nonzero is not diagnosed"});
                    return true;
                }
            }
        }
        // Inquiry arguments supply type information and are not evaluated.
        if inquiry {
            return false;
        }
        let child_context = if matches!(e,Expr::Call(n,_) if matches!(n.as_str(),"@typed_constructor"|"@array_constructor"|"@complex_literal"|"cmplx"|"real"|"dble"))
        {
            context
        } else {
            None
        };
        for child in expressions(e) {
            if self.underflow_expression(value(child), scope, offset, child_context) {
                return true;
            }
        }
        false
    }
    fn character_case(&self, e: &Expr, scope: usize, seen: &mut HashSet<usize>) -> Option<String> {
        let mut e = e.clone();
        fn replace(
            a: &Analysis<'_>,
            e: &mut Expr,
            scope: usize,
            seen: &mut HashSet<usize>,
        ) -> Option<()> {
            if let Expr::Ident(n) = e {
                let id = a.resolve(scope, n)?;
                let d = &a.decls[id];
                if !seen.insert(id) {
                    return None;
                }
                let s = a.character_case(d.constant.as_ref()?, d.scope, seen);
                seen.remove(&id);
                *e = Expr::Str(s?);
                return Some(());
            }
            if let Expr::Call(n, _) = e {
                if a.resolve(scope, n).is_some() {
                    return None;
                }
            }
            match e {
                Expr::Call(_, v) | Expr::Index(_, v) => {
                    for e in v {
                        replace(a, e, scope, seen)?;
                    }
                }
                Expr::Bin(_, x, y) => {
                    replace(a, x, scope, seen)?;
                    replace(a, y, scope, seen)?;
                }
                Expr::Un(_, e) => replace(a, e, scope, seen)?,
                _ => {}
            }
            Some(())
        }
        replace(self, &mut e, scope, seen)?;
        crate::parameters::character_constant(&e).ok()
    }
    fn case_headers(&self, offset: usize) -> Vec<Range<usize>> {
        let start = self.tokens.partition_point(|t| t.span.start < offset);
        let mut depth = 0;
        let mut heads = vec![];
        for i in start..self.tokens.len() {
            if matches!(self.tokens[i].kind, TokenKind::KwSelect) {
                if i > 0 && matches!(self.tokens[i - 1].kind, TokenKind::KwEnd) {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if self
                    .tokens
                    .get(i + 1)
                    .is_some_and(|t| matches!(t.kind, TokenKind::KwCase))
                {
                    depth += 1;
                }
            }
            if depth == 1
                && matches!(self.tokens[i].kind, TokenKind::KwCase)
                && !i
                    .checked_sub(1)
                    .is_some_and(|i| matches!(self.tokens[i].kind, TokenKind::KwSelect))
                && !self
                    .tokens
                    .get(i + 1)
                    .is_some_and(|t| matches!(t.kind, TokenKind::KwDefault))
            {
                heads.push(self.tokens[i].span.clone());
            }
        }
        heads
    }
    fn empty_cases(&mut self, s: &Stmt, scope: usize, offset: usize) {
        let Stmt::SelectCase { expr, cases, .. } = s else {
            return;
        };
        let heads = self.case_headers(offset);
        let enumeration = self.enum_type(expr, scope);
        for (i, c) in cases.iter().enumerate() {
            for item in &c.items {
                let CaseItem::Range(l, u) = item else {
                    continue;
                };
                let integer = if let Some(t) = enumeration {
                    self.enum_value(l, scope, t, &mut HashSet::new())
                        .zip(self.enum_value(u, scope, t, &mut HashSet::new()))
                } else {
                    self.integer(l, scope, &mut HashSet::new())
                        .zip(self.integer(u, scope, &mut HashSet::new()))
                };
                let description = if let Some((l, u)) = integer {
                    if l <= u {
                        continue;
                    }
                    format!("{l}:{u}")
                } else if let (Some(l), Some(u)) = (
                    self.character_case(l, scope, &mut HashSet::new()),
                    self.character_case(u, scope, &mut HashSet::new()),
                ) {
                    if !l.is_ascii() || !u.is_ascii() {
                        continue;
                    }
                    let n = l.len().max(u.len());
                    let mut lb = l.as_bytes().to_vec();
                    let mut ub = u.as_bytes().to_vec();
                    lb.resize(n, b' ');
                    ub.resize(n, b' ');
                    if lb <= ub {
                        continue;
                    }
                    format!("{l:?}:{u:?}")
                } else {
                    continue;
                };
                self.warnings.push(Advice{category:"empty-case",span:heads.get(i).cloned().unwrap_or(offset..offset),
                message:format!("CASE range {description} is empty and cannot match any selector value"),related:None,
                note:"check whether the lower and upper bounds were reversed; an empty CASE range is legal but its branch cannot be selected"});
            }
        }
    }
    pub(super) fn syntax_checks(&mut self, body: &'a [Stmt], scope: usize, mut offset: usize) {
        let scope = self
            .bodies
            .get(&(body.as_ptr() as usize))
            .copied()
            .unwrap_or(scope);
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if let Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. }
            | Stmt::DerivedType {
                components: body, ..
            } = s
            {
                self.syntax_checks(body, scope, offset);
                continue;
            }
            self.empty_cases(s, scope, offset);
            if !self.dead.contains(&(s as *const Stmt as usize)) {
                match s {
                    Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. } => {
                        let to = self
                            .data_type(scope, name)
                            .and_then(|(k, _, _, _, owner)| self.number_kind(&k?, owner));
                        self.underflow_expression(value, scope, offset, to);
                    }
                    Stmt::InitializedDecl { kind, entities, .. } => {
                        let to = self.number_kind(kind, scope);
                        for (_, e) in entities {
                            if let Some(e) = e {
                                self.underflow_expression(e, scope, offset, to);
                            }
                        }
                    }
                    _ => {
                        let mut copy = s.clone();
                        for e in crate::procedure_arguments::expressions(&mut copy) {
                            self.underflow_expression(e, scope, offset, None);
                        }
                    }
                }
            }
            for b in children(s) {
                self.syntax_checks(b, scope, offset);
            }
        }
    }
}

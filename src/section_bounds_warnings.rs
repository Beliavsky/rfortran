// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wsection-bounds`, in -Wextra: an ASSOCIATE selector or a pointer target
//! that is a section spanning a whole array, such as `a(:)`, when the array
//! has a lower bound other than 1. A section's lower bounds are 1, so
//! `associate (x => a(:))` gives `x` the bounds 1:5 of an `a(0:4)`, while
//! `associate (x => a)` keeps 0:4; the same holds for `p => a(:)` and
//! `p => a`. Writing `a(:)` for `a` then renumbers the elements silently.
//!
//! A section of part of the array, such as `a(2:3)`, is not reported: bounds
//! starting at 1 are expected there. Nor is a pointer assignment with a
//! bounds specification or remapping, `p(0:) => a(:)`, nor an actual
//! argument, which takes bounds starting at 1 from `a` and `a(:)` alike.
//!
//! The check reads tokens. Bounds are known for explicit-shape and
//! assumed-shape arrays whose lower bounds are integer literals or named
//! constants with an integer literal value.
use crate::lexer::{Token, TokenKind};
use crate::open_warnings::{constants, items, statements};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::HashMap;

const CATEGORY: &str = "section-bounds";

/// A declared dimension: the text of its lower and upper bounds. The lower
/// bound is None when it is not known (a deferred shape), and the upper
/// bound when it is not declared (assumed shape or size).
#[derive(Clone)]
struct Dimension {
    lower: Option<String>,
    upper: Option<String>,
}

type Arrays = HashMap<String, Vec<Dimension>>;

/// The source text of tokens, without spaces, in lower case.
fn text(src: &str, tokens: &[Token]) -> String {
    tokens.iter().map(|t| src[t.span.clone()].to_ascii_lowercase()).collect()
}

/// Tokens split at the colons and commas outside parentheses: `sep` picks one.
fn split<'a>(tokens: &'a [Token], colon: bool) -> Vec<&'a [Token]> {
    let mut out = vec![];
    let (mut depth, mut start) = (0i32, 0);
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket | TokenKind::ComplexOpen => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Colon if colon && depth == 0 => { out.push(&tokens[start..i]); start = i + 1; }
            TokenKind::Comma if !colon && depth == 0 => { out.push(&tokens[start..i]); start = i + 1; }
            _ => {}
        }
    }
    out.push(&tokens[start..]);
    out
}

/// The tokens inside the parentheses that open at `open`, and the index
/// after the closing one.
fn group(tokens: &[Token], open: usize) -> Option<(&[Token], usize)> {
    let mut depth = 0;
    for i in open..tokens.len() {
        match tokens[i].kind {
            TokenKind::LParen | TokenKind::ComplexOpen => depth += 1,
            TokenKind::RParen => {
                depth -= 1;
                if depth == 0 { return Some((&tokens[open + 1..i], i + 1)); }
            }
            _ => {}
        }
    }
    None
}

/// The dimensions of an array specification such as `0:4, n, -1:`.
fn dimensions(src: &str, spec: &[Token]) -> Vec<Dimension> {
    split(spec, false).into_iter().map(|d| {
        let parts = split(d, true);
        let bound = |t: &[Token]| (!t.is_empty() && !matches!(t, [Token { kind: TokenKind::Star, .. }])).then(|| text(src, t));
        match parts.as_slice() {
            [upper] => Dimension { lower: Some("1".into()), upper: bound(upper) },
            [lower, upper] => Dimension { lower: bound(lower), upper: bound(upper) },
            _ => Dimension { lower: None, upper: None },
        }
    }).collect()
}

/// Records the entities a type declaration or DIMENSION statement declares.
fn declare(src: &str, s: &[Token], arrays: &mut Arrays) {
    let kind = |i: usize| s.get(i).map(|t| &t.kind);
    let typed = match kind(0) {
        Some(TokenKind::KwInteger | TokenKind::KwReal | TokenKind::KwComplex | TokenKind::KwCharacter | TokenKind::KwLogical) => 1,
        Some(TokenKind::KwDouble) if matches!(kind(1), Some(TokenKind::KwPrecision)) => 2,
        Some(TokenKind::KwType | TokenKind::KwClass) if matches!(kind(1), Some(TokenKind::LParen)) => 1,
        Some(TokenKind::KwDimension) => 1,
        _ => return,
    };
    if s.iter().any(|t| matches!(t.kind, TokenKind::KwFunction | TokenKind::KwSubroutine)) { return; }
    let dimension_statement = matches!(kind(0), Some(TokenKind::KwDimension));
    let mut shared = None;
    let entities = match s.iter().position(|t| matches!(t.kind, TokenKind::DColon)) {
        Some(colons) => {
            for attribute in split(&s[typed..colons], false) {
                if let [Token { kind: TokenKind::KwDimension, .. }, rest @ ..] = attribute {
                    if let Some((spec, _)) = group(rest, 0) { shared = Some(dimensions(src, spec)); }
                }
            }
            &s[colons + 1..]
        }
        None if dimension_statement => &s[1..],
        None => {
            // `real*8 a(3)`, `character(len=4) c(2)`: skip the type parameters.
            let mut at = typed;
            if matches!(kind(at), Some(TokenKind::LParen)) {
                let Some((_, next)) = group(s, at) else { return };
                at = next;
            }
            if matches!(kind(at), Some(TokenKind::Star)) {
                at += 1;
                if matches!(kind(at), Some(TokenKind::LParen)) {
                    let Some((_, next)) = group(s, at) else { return };
                    at = next;
                } else {
                    at += 1;
                }
            }
            s.get(at..).unwrap_or_default()
        }
    };
    for entity in split(entities, false) {
        let Some(Token { kind: TokenKind::Ident(name), .. }) = entity.first() else { continue };
        let own = matches!(entity.get(1).map(|t| &t.kind), Some(TokenKind::LParen))
            .then(|| group(entity, 1)).flatten().map(|(spec, _)| dimensions(src, spec));
        match own.or_else(|| shared.clone()) {
            Some(dims) => { arrays.insert(name.clone(), dims); }
            // A scalar of the same name hides an array of the host.
            None if !dimension_statement => { arrays.remove(name); }
            None => {}
        }
    }
}

/// The value of a lower bound: an integer literal, possibly negated, or a
/// named constant with an integer literal value.
fn value(bound: &str, constants: &HashMap<String, Option<i64>>) -> Option<i64> {
    let (negative, digits) = match bound.strip_prefix('-') { Some(rest) => (true, rest), None => (false, bound.strip_prefix('+').unwrap_or(bound)) };
    let magnitude = match digits.parse::<i64>() {
        Ok(v) => v,
        Err(_) => (*constants.get(digits)?)?,
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// For a selector or target spanning the whole of an array with a lower
/// bound other than 1: the array and its lower bounds.
fn whole_section(src: &str, selector: &[Token], arrays: &Arrays, constants: &HashMap<String, Option<i64>>) -> Option<(String, Vec<i64>)> {
    let [Token { kind: TokenKind::Ident(name), .. }, Token { kind: TokenKind::LParen, .. }, ..] = selector else { return None };
    let (subscripts, end) = group(selector, 1)?;
    if end != selector.len() { return None; }
    let dims = arrays.get(name)?;
    let subscripts = split(subscripts, false);
    if subscripts.len() != dims.len() { return None; }
    let mut lowers = vec![];
    for (subscript, dim) in subscripts.iter().zip(dims) {
        let parts = split(subscript, true);
        let (first, last) = match parts.as_slice() {
            [first, last] => (first, last),
            [first, last, stride] if text(src, stride) == "1" => (first, last),
            _ => return None,
        };
        let lower = dim.lower.as_ref()?;
        if !first.is_empty() && text(src, first) != *lower { return None; }
        if !last.is_empty() && Some(text(src, last)) != dim.upper { return None; }
        lowers.push(value(lower, constants)?);
    }
    lowers.iter().any(|l| *l != 1).then(|| (name.clone(), lowers))
}

/// The ASSOCIATE selectors and pointer targets that renumber a whole array:
/// the position, the associate name or pointer, the selector's text, the
/// array, its lower bounds, and whether it is a pointer assignment.
fn analyze(src: &str, tokens: &[Token]) -> Vec<(usize, String, String, String, Vec<i64>, bool)> {
    let constants = constants(src, tokens);
    let mut scopes: Vec<Arrays> = vec![Arrays::new()];
    let mut out = vec![];
    let mut definition = false;
    for s in statements(src, tokens) {
        // A statement label or construct name comes first.
        let s = match s { [Token { kind: TokenKind::Integer(_), .. }, rest @ ..] if !rest.is_empty() => rest, _ => s };
        let kind = |i: usize| s.get(i).map(|t| &t.kind);
        // Program units: a procedure sees the arrays of its host.
        let begins = match kind(0) {
            Some(TokenKind::KwProgram | TokenKind::KwBlockData) => true,
            Some(TokenKind::KwModule) => !matches!(kind(1), Some(TokenKind::Ident(n)) if n == "procedure"),
            Some(TokenKind::Ident(n)) if n == "submodule" => true,
            Some(TokenKind::KwEnd) => false,
            _ => s.iter().any(|t| matches!(t.kind, TokenKind::KwFunction | TokenKind::KwSubroutine)),
        };
        if begins {
            let host = scopes.last().cloned().unwrap_or_default();
            scopes.push(host);
            continue;
        }
        let ends = match kind(0) {
            Some(TokenKind::KwEnd) => matches!(kind(1), None | Some(TokenKind::KwSubroutine | TokenKind::KwFunction | TokenKind::KwProgram | TokenKind::KwModule))
                || matches!(kind(1), Some(TokenKind::Ident(n)) if n == "submodule" || n == "procedure"),
            Some(TokenKind::BareEnd | TokenKind::KwEndBlockData | TokenKind::EndUnitName(_)) => true,
            _ => false,
        };
        if ends {
            if scopes.len() > 1 { scopes.pop(); }
            continue;
        }
        // Components of a derived type definition are not variables.
        match (kind(0), kind(1)) {
            (Some(TokenKind::KwType), Some(TokenKind::LParen)) => {}
            (Some(TokenKind::KwType), _) => { definition = true; continue; }
            (Some(TokenKind::KwEnd), Some(TokenKind::KwType)) => { definition = false; continue; }
            _ if definition => continue,
            _ => {}
        }
        let arrays = scopes.last_mut().unwrap();
        declare(src, s, arrays);
        let arrays = &*arrays;
        let named = match s { [Token { kind: TokenKind::Ident(_), .. }, Token { kind: TokenKind::Colon, .. }, rest @ ..] => rest, _ => s };
        if let [Token { kind: TokenKind::KwAssociate, .. }, Token { kind: TokenKind::LParen, .. }, ..] = named {
            for item in items(named, 1) {
                let [Token { kind: TokenKind::Ident(associate), .. }, Token { kind: TokenKind::Arrow, .. }, selector @ ..] = item else { continue };
                if let Some((array, lowers)) = whole_section(src, selector, arrays, &constants) {
                    out.push((selector[0].span.start, associate.clone(), text(src, selector), array, lowers, false));
                }
            }
            continue;
        }
        // A pointer assignment, possibly the action of a one-line IF.
        let statement = match s {
            [Token { kind: TokenKind::KwIf, .. }, Token { kind: TokenKind::LParen, .. }, ..] => match group(s, 1) {
                Some((_, after)) => &s[after..],
                None => continue,
            },
            _ => s,
        };
        let [Token { kind: TokenKind::Ident(_), .. }, ..] = statement else { continue };
        let Some(arrow) = statement.iter().position(|t| matches!(t.kind, TokenKind::Arrow)) else { continue };
        let pointer = &statement[..arrow];
        // `p(0:) => a(:)` gives the bounds itself; `x(i)%p` is skipped too.
        if pointer.iter().any(|t| matches!(t.kind, TokenKind::LParen)) { continue; }
        if pointer.iter().any(|t| !matches!(t.kind, TokenKind::Ident(_) | TokenKind::Percent)) { continue; }
        let target = &statement[arrow + 1..];
        if let Some((array, lowers)) = whole_section(src, target, arrays, &constants) {
            out.push((target[0].span.start, text(src, pointer), text(src, target), array, lowers, true));
        }
    }
    out
}

fn bounds(values: &[i64]) -> String {
    let list: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    if list.len() == 1 { list[0].clone() } else { format!("({})", list.join(", ")) }
}

pub(crate) fn check(
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // In -Wextra, not -Wall.
    let mut extra_only = settings.clone();
    extra_only.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &extra_only, options);
    if !enabled { return Ok(()); }
    let tokens = crate::lexer::lex(src);
    let warnings = analyze(src, &tokens).into_iter().map(|(pos, name, selector, array, lowers, pointer)| {
        let ones = bounds(&vec![1; lowers.len()]);
        let plural = if lowers.len() == 1 { "bound" } else { "bounds" };
        let message = format!("{} `{selector}` is an array section, so `{name}` has lower {plural} {ones}, not {} as `{array}` has [{CATEGORY}]",
            if pointer { "target" } else { "selector" }, bounds(&lowers));
        let note = if pointer {
            format!("`{name} => {array}` keeps the bounds of `{array}`")
        } else {
            format!("associate `{name}` with `{array}` to keep its bounds")
        };
        (pos, message, vec![note])
    }).collect();
    if crate::open_warnings::emit(src, own_len, filename, origins, &tokens, error, warnings)? {
        bail!("{CATEGORY} warnings treated as errors");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::analyze;

    fn reports(src: &str) -> Vec<(String, String, Vec<i64>, bool)> {
        analyze(src, &crate::lexer::lex(src)).into_iter().map(|(_, n, s, _, l, p)| (n, s, l, p)).collect()
    }

    #[test]
    fn whole_sections_of_arrays_not_starting_at_one() {
        let src = "program p\ninteger, parameter :: n0 = 0\nreal :: a(0:4), c(5)\nreal, dimension(-1:1,2), target :: d\nreal, target :: e(n0:3)\nreal, pointer :: q(:), r(:,:)\nassociate (x => a(:), y => c(:), z => d(-1:1,:))\nend associate\nq => e(:)\nif (.true.) r => d(:,1:2)\nend program\n";
        assert_eq!(reports(src), vec![
            ("x".into(), "a(:)".into(), vec![0], false),
            ("z".into(), "d(-1:1,:)".into(), vec![-1, 1], false),
            ("q".into(), "e(:)".into(), vec![0], true),
            ("r".into(), "d(:,1:2)".into(), vec![-1, 1], true),
        ]);
    }

    #[test]
    fn partial_sections_whole_arrays_and_remapping() {
        let src = "real, target :: a(0:4)\nreal, pointer :: q(:)\nassociate (x => a, y => a(1:4), z => a(0:4:2), w => a(:)+1)\nend associate\nq => a\nq(0:) => a(:)\nend\n";
        assert!(reports(src).is_empty());
    }

    #[test]
    fn scopes_and_unknown_bounds() {
        // An internal procedure sees the host's array; a local scalar hides it.
        let src = "program p\nreal :: a(0:4)\ncall s\ncall t\ncontains\nsubroutine s\nassociate (x => a(:))\nend associate\nend subroutine\nsubroutine t\nreal :: a\nend subroutine\nend program\nsubroutine u(b, m)\ninteger :: m\nreal :: b(m:), c(0:)\nreal, allocatable :: d(:)\nassociate (x => b(:), y => c(:), z => d(:))\nend associate\nend subroutine\n";
        assert_eq!(reports(src), vec![
            ("x".into(), "a(:)".into(), vec![0], false),
            ("y".into(), "c(:)".into(), vec![0], false),
        ]);
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Best-effort checks of known formats, before I/O lowering loses item types.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use crate::format_spec as spec;
use crate::sema::{ImplicitTyping, SemaSettings};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
const LIMIT: usize = 10000;
struct Analysis {
    functions: Shapes,
    globals: Shapes,
    /// Position, category, message and notes.
    warnings: Vec<(usize, &'static str, String, Vec<String>)>,
    /// Statements whose format is a named constant or FORMAT statement that
    /// other statements also use, by position.
    shared: HashSet<usize>,
    /// Statements whose format is a literal, by position.
    literal: HashSet<usize>,
    /// Whether -Wcould-be-at is enabled, and its candidates.
    at_enabled: bool,
    at: Vec<AtCandidate>,
}

/// A list item for -Wcould-be-at: `trim(...)`, an implied-DO of
/// `trim(s(i))` that is `s` or `s(a:b)`, a value without trailing blanks
/// that AT leaves as A does, or any other item.
#[derive(Clone, PartialEq)]
enum AtItem {
    Trim,
    TrimLoop { whole: bool },
    Plain,
    Other,
}

/// An output statement whose `trim` calls the AT edit descriptor can
/// replace: the A descriptors to change, in the format's text.
struct AtCandidate {
    offset: usize,
    format: String,
    converted: Vec<std::ops::Range<usize>>,
    items: Vec<AtItem>,
}

fn at_item(e: &Expr, shapes: &Shapes) -> (AtItem, Option<usize>) {
    let constant = |e: &Expr| crate::parameters::integer_constant(e).ok();
    let character = |k: &TypeSpec| matches!(k, TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed);
    match e {
        Expr::Call(n, a) if n == "trim" && a.len() == 1 => (AtItem::Trim, Some(1)),
        Expr::Str(s) if !s.ends_with(' ') => (AtItem::Plain, Some(1)),
        Expr::Call(n, a) if n == "@output_implied_do" => {
            let [Expr::Ident(v), start, end, step, Expr::Call(t, inner)] = a.as_slice() else { return (AtItem::Other, None) };
            let mentions = |e: &Expr| {
                let mut found = false;
                crate::procedure_arguments::visit_expr(&mut e.clone(), &mut |x| found |= matches!(x, Expr::Ident(n) if n == v));
                found
            };
            if t != "trim" || constant(step) != Some(1) || mentions(start) || mentions(end) {
                return (AtItem::Other, None);
            }
            let [Expr::Index(name, s) | Expr::Call(name, s)] = inner.as_slice() else { return (AtItem::Other, None) };
            let Some((kind, dims)) = shapes.get(name) else { return (AtItem::Other, None) };
            if dims.len() != 1 || !character(kind) || !matches!(s.as_slice(), [Expr::Ident(i)] if i == v) {
                return (AtItem::Other, None);
            }
            let count = constant(start).zip(constant(end)).map(|(a, b)| (b - a + 1).max(0) as usize);
            let whole = constant(start) == Some(1) && constant(end).is_some() && constant(end) == constant(&dims[0]);
            (AtItem::TrimLoop { whole }, count)
        }
        _ => (AtItem::Other, None),
    }
}

/// The A descriptors that every `trim` item of the list meets, if each can
/// become AT: plain `a` met only by items AT writes as A does. `counts` are
/// the items' lengths; an unknown one is tried for each size up to a bound.
fn at_descriptors(format: &str, items: &[AtItem], counts: &[Option<usize>]) -> Option<Vec<std::ops::Range<usize>>> {
    let unknown: Vec<usize> = (0..items.len()).filter(|&i| counts[i].is_none()).collect();
    if unknown.len() > 3 {
        return None;
    }
    let body = format.trim().strip_prefix('(')?.strip_suffix(')')?;
    let bound = (2 * data_count(body.as_bytes(), &mut false)?.max(1) + 2).min([40, 40, 12, 6][unknown.len()]);
    let base = format.as_ptr() as usize;
    let mut users: HashMap<(usize, usize), HashSet<usize>> = HashMap::new();
    let mut convertible: HashMap<(usize, usize), bool> = HashMap::new();
    let mut sizes = vec![0usize; unknown.len()];
    loop {
        let mut list = vec![];
        for (i, c) in counts.iter().enumerate() {
            let n = c.unwrap_or_else(|| sizes[unknown.iter().position(|&u| u == i).unwrap()]);
            list.extend(std::iter::repeat_n(i, n));
        }
        let mut parser = spec::GroupParser::new(format.as_bytes(), list.len()).ok()?;
        let mut index = 0;
        while let Some(d) = parser.next().ok()? {
            if !spec::is_data(d.code) {
                continue;
            }
            let at = (d.raw.as_ptr() as usize - base, d.raw.len());
            convertible.insert(at, d.code == b'a' && d.raw.last().is_some_and(|c| c.eq_ignore_ascii_case(&b'a'))
                && d.raw[..d.raw.len() - 1].iter().all(u8::is_ascii_digit));
            for _ in 0..d.repeat.min(list.len().saturating_sub(index)) {
                users.entry(at).or_default().insert(list[index]);
                index += 1;
            }
        }
        let mut k = 0;
        while k < sizes.len() && sizes[k] == bound {
            sizes[k] = 0;
            k += 1;
        }
        if k == sizes.len() {
            break;
        }
        sizes[k] += 1;
    }
    let trimmed = |i: &usize| matches!(items[*i], AtItem::Trim | AtItem::TrimLoop { .. });
    let mut converted = vec![];
    for (at, met) in &users {
        if !met.iter().any(trimmed) {
            continue;
        }
        if !convertible[at] || met.iter().any(|i| items[*i] == AtItem::Other) {
            return None;
        }
        converted.push(at.0..at.0 + at.1);
    }
    converted.sort_by_key(|r| r.start);
    (!converted.is_empty()).then_some(converted)
}

/// Remove continuation marks, so that a statement reads as one line.
fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_end_matches('\r');
        let line = if out.is_empty() { line } else { line.trim_start().trim_start_matches('&') };
        match line.trim_end().strip_suffix('&') {
            Some(kept) if lines.peek().is_some() => out.push_str(kept),
            _ => out.push_str(line),
        }
    }
    out.trim().to_owned()
}

/// A descriptor as written in messages, such as `F8.3`.
fn descriptor_text(d: &spec::Descriptor<'_>) -> String {
    let name = match d.code {
        spec::ES => "ES".into(),
        spec::EN => "EN".into(),
        spec::AT => "AT".into(),
        c => (c as char).to_ascii_uppercase().to_string(),
    };
    format!("{name}{}{}", d.width, if d.has_precision { format!(".{}", d.precision) } else { String::new() })
}

const TYPE_NAMES: [&str; 5] = ["unknown", "INTEGER", "REAL", "LOGICAL", "CHARACTER"];

/// The first item a descriptor of the wrong type meets: its index, the
/// descriptor, and whether format reversion came first.
fn first_mismatch(format: &str, items: &[Item]) -> Option<(usize, String, bool)> {
    let mut parser = spec::GroupParser::new(format.as_bytes(), items.len()).ok()?;
    let (mut index, mut reverted) = (0, false);
    while let Ok(Some(d)) = parser.next() {
        reverted |= d.code == spec::REVERSION;
        if !spec::is_data(d.code) {
            continue;
        }
        for _ in 0..d.repeat.min(items.len().saturating_sub(index)) {
            index += 1;
            let Some(tag) = items[index - 1].tag else { continue };
            if spec::matches_type(d.code, tag) || matches!(d.code, b'b' | b'o' | b'z') && matches!(tag, 1..=3) {
                continue;
            }
            return Some((index - 1, descriptor_text(&d), reverted));
        }
    }
    None
}

/// An item of an I/O list for the sizes check: items of known length, or
/// an array of unknown size, given by its size variable.
enum Part {
    Fixed(Vec<Item>),
    Array { size: usize, element: Vec<Item> },
}

/// How an I/O list of known length uses a constant format.
enum Layout {
    Fits,
    /// The first data edit descriptor without an item: its place in the
    /// format, the items left for it if it has a repeat count, whether it
    /// is outside any group, and whether a colon edit precedes it.
    Unused { at: std::ops::Range<usize>, repeat: Option<usize>, top: bool, colon: bool },
    /// Items left after the first pass, the descriptors of that pass, and
    /// whether a descriptor or group has a repeat count above 1.
    Reversion { descriptors: usize, repeated: bool },
}

/// The data edit descriptors a pass through `body` gives.
fn data_count(body: &[u8], repeated: &mut bool) -> Option<usize> {
    let mut p = spec::Parser::items(body);
    let mut n = 0usize;
    loop {
        if p.rest().is_empty() {
            return Some(n);
        }
        if let Some((inner, repeat, _)) = p.next_group().ok()? {
            *repeated |= repeat > 1;
            n = n.checked_add(data_count(inner, repeated)?.checked_mul(repeat)?)?;
        } else {
            let d = p.next().ok()??;
            if spec::is_data(d.code) {
                *repeated |= d.repeat > 1;
                n = n.checked_add(d.repeat)?;
            }
        }
    }
}

fn layout(format: &str, count: usize) -> Option<Layout> {
    let bytes = format.as_bytes();
    let open = bytes.iter().position(|b| !b.is_ascii_whitespace())?;
    let close = bytes.iter().rposition(|b| !b.is_ascii_whitespace())?;
    if bytes[open] != b'(' || bytes[close] != b')' || close <= open {
        return None;
    }
    let base = bytes.as_ptr() as usize;
    let mut p = spec::Parser::items(&bytes[open + 1..close]);
    let (mut consumed, mut colon, mut repeated) = (0usize, false, false);
    loop {
        let rest = p.rest();
        if rest.is_empty() {
            break;
        }
        let at = rest.as_ptr() as usize - base;
        if let Some((inner, repeat, unlimited)) = p.next_group().ok()? {
            // An unlimited group takes any number of items.
            if unlimited {
                return Some(Layout::Fits);
            }
            repeated |= repeat > 1;
            let n = data_count(inner, &mut repeated)?.checked_mul(repeat)?;
            if n > 0 && consumed + n > count {
                let end = p.rest().as_ptr() as usize - base;
                return Some(Layout::Unused { at: at..end, repeat: None, top: consumed >= count, colon });
            }
            consumed += n;
        } else {
            let d = p.next().ok()??;
            if d.code == b':' {
                colon = true;
            } else if spec::is_data(d.code) {
                repeated |= d.repeat > 1;
                if consumed + d.repeat > count {
                    let repeat = (consumed < count).then(|| count - consumed);
                    return Some(Layout::Unused { at: at..at + d.raw.len(), repeat, top: true, colon });
                }
                consumed += d.repeat;
            }
        }
    }
    Some(if consumed < count { Layout::Reversion { descriptors: consumed, repeated } } else { Layout::Fits })
}

/// The FORMAT statement label an I/O statement at `offset` names, as in
/// `print 20, x`, `write (*, 20) x` or `read (u, fmt=20) x`. The parser has
/// already replaced the label by the format.
fn format_label(src: &str, offset: usize) -> Option<String> {
    let line = src.get(offset..)?.lines().next()?;
    let w = crate::indentation_warnings::words(line);
    let number = |x: &String| x.bytes().all(|b| b.is_ascii_digit()).then(|| x.trim_start_matches('0').to_owned());
    match w.first().map(String::as_str)? {
        "print" | "read" if w.get(1).is_some_and(|x| x != "(") => number(w.get(1)?),
        "write" | "read" => {
            if let Some(f) = w.iter().position(|x| x == "fmt") {
                return number(w.get(f + 2)?);
            }
            // The second item of the control list.
            let (mut depth, mut items) = (0, 0);
            for (i, x) in w.iter().enumerate().skip(1) {
                match x.as_str() {
                    "(" => depth += 1,
                    ")" => { depth -= 1; if depth == 0 { return None; } }
                    "," if depth == 1 => { items += 1; if items == 1 { return number(w.get(i + 1)?).filter(|_| w.get(i + 2).is_some_and(|y| y == ")" || y == ",")); } }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

/// The positions of formatted I/O statements whose format is a named
/// constant, or a FORMAT statement of the unit, that another statement uses.
fn shared_formats(body: &mut [Stmt], src: &str) -> (HashSet<usize>, HashSet<usize>) {
    fn walk(body: &mut [Stmt], unit: usize, serial: &mut usize, mut offset: usize, uses: &mut HashMap<String, Vec<usize>>, src: &str) {
        for s in body {
            match s {
                Stmt::SourceLocation(p) => offset = *p,
                Stmt::CallSub { name, args } => {
                    let position = match name.as_str() {
                        "@formatted_print" => 0,
                        "@write" | "@formatted_read" | "@formatted_read_status" | "@formatted_write_noadvance" => 1,
                        _ => continue,
                    };
                    let key = match args.get(position) {
                        Some(Expr::Ident(n)) => n.clone(),
                        _ => match format_label(src, offset) {
                            Some(l) => format!("{unit}:{l}"),
                            None if matches!(args.get(position), Some(Expr::Str(_))) => "@literal".to_owned(),
                            None => continue,
                        },
                    };
                    uses.entry(key).or_default().push(offset);
                }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    *serial += 1;
                    walk(body, *serial, serial, offset, uses, src);
                }
                _ => {
                    for child in crate::procedure_arguments::children(s) {
                        walk(child, unit, serial, offset, uses, src);
                    }
                }
            }
        }
    }
    let mut uses: HashMap<String, Vec<usize>> = HashMap::new();
    walk(body, 0, &mut 0, 0, &mut uses, src);
    let literal = uses.remove("@literal").unwrap_or_default().into_iter().collect();
    (uses.into_values().filter(|v| v.len() > 1).flatten().collect(), literal)
}
#[derive(Clone)]
struct Item {
    tag: Option<u64>,
    number: usize,
    component: Option<usize>,
    /// For an element of an array of unknown size, its position.
    element: Option<usize>,
}
fn tag(kind: &TypeSpec) -> Option<u64> {
    Some(match kind {
        TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) => 1,
        TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision => 2,
        TypeSpec::Logical | TypeSpec::LogicalKind(_) => 3,
        TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => 4,
        _ => return None,
    })
}
fn declarations(body: &[Stmt], shapes: &mut Shapes) {
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            // A derived declaration hides any same-named host intrinsic object.
            Stmt::DerivedDecl { names, dims, .. } => {
                for n in names {
                    // Retain an unknown declared type instead of implicitly
                    // typing this object or exposing a same-named host object.
                    shapes.insert(
                        n.clone(),
                        (
                            TypeSpec::Referenced {
                                source: Box::new(Expr::Ident(n.clone())),
                                polymorphic: false,
                            },
                            dims.clone(),
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}
impl Analysis {
    fn functions(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                        _ => name,
                    };
                    let mut local = Shapes::new();
                    declarations(body, &mut local);
                    if let Some(shape) = local
                        .get(result)
                        .cloned()
                        .or_else(|| return_type.clone().map(|k| (k, vec![])))
                    {
                        self.functions.insert(name.clone(), shape);
                    }
                }
                Stmt::Module { body, .. } => {
                    let mut globals = Shapes::new();
                    declarations(body, &mut globals);
                    self.globals
                        .extend(globals.into_iter().filter(|(n, _)| n.starts_with('@')));
                    self.functions(body);
                }
                _ => {}
            }
        }
    }
    fn kind(&self, e: &Expr, shapes: &Shapes, implicit: &ImplicitTyping) -> Option<TypeSpec> {
        match e {
            Expr::Call(n, _) | Expr::Index(n, _) if shapes.contains_key(n) => {
                Some(shapes[n].0.clone())
            }
            Expr::Ident(n) => shapes.get(n).map(|(k, _)| k.clone()).or_else(|| {
                (!n.contains('%') && !n.starts_with('@'))
                    .then(|| implicit.get_implicit_type(n))
                    .flatten()
            }),
            Expr::Call(n, _) if self.functions.contains_key(n) => Some(self.functions[n].0.clone()),
            Expr::Call(n, _) if n == "@complex_literal" => Some(TypeSpec::Complex("4".into())),
            Expr::Bin(_, a, b)
                if matches!(self.kind(a, shapes, implicit), Some(TypeSpec::Complex(_)))
                    || matches!(self.kind(b, shapes, implicit), Some(TypeSpec::Complex(_))) =>
            {
                if matches!(
                    e,
                    Expr::Bin(crate::ast::BinOp::Eq | crate::ast::BinOp::Ne, _, _)
                ) {
                    Some(TypeSpec::Logical)
                } else {
                    Some(TypeSpec::Complex("4".into()))
                }
            }
            _ => crate::array_calls::scalar_actual_kind(e, shapes).ok(),
        }
    }
    fn append(
        &self,
        e: &Expr,
        number: usize,
        shapes: &Shapes,
        implicit: &ImplicitTyping,
        out: &mut Vec<Item>,
    ) -> Option<()> {
        if out.len() >= LIMIT {
            return None;
        }
        if let Expr::Call(n, args) = e {
            if n == "@array_constructor" {
                for e in args {
                    self.append(e, number, shapes, implicit, out)?;
                }
                return Some(());
            }
            if n == "@output_implied_do" || n == "@input_implied_do" {
                let [Expr::Ident(var), start, end, step, rest @ ..] = args.as_slice() else {
                    return None;
                };
                let (mut i, end, step) = (
                    crate::parameters::integer_constant(start).ok()?,
                    crate::parameters::integer_constant(end).ok()?,
                    crate::parameters::integer_constant(step).ok()?,
                );
                if step == 0 {
                    return None;
                }
                let mut iterations = 0;
                while if step > 0 { i <= end } else { i >= end } {
                    if iterations >= LIMIT {
                        return None;
                    }
                    iterations += 1;
                    for e in rest {
                        let mut e = e.clone();
                        crate::procedure_arguments::visit_expr(&mut e, &mut |e| {
                            if matches!(e, Expr::Ident(n) if n == var) {
                                *e = Expr::IntLit(i.to_string());
                            }
                        });
                        self.append(&e, number, shapes, implicit, out)?;
                    }
                    i = i.checked_add(step)?;
                }
                return Some(());
            }
        }
        // A vector subscript, as in `v([1, 3])`, gives an array whose length
        // this check does not count.
        if let Expr::Index(_, args) | Expr::Call(_, args) = e {
            let vector = |a: &Expr| match a {
                Expr::Call(n, _) => n == "@array_constructor",
                Expr::Ident(n) => shapes.get(n).is_some_and(|(_, d)| !d.is_empty()),
                _ => false,
            };
            if args.iter().any(vector) && shapes.get(match e { Expr::Index(n, _) | Expr::Call(n, _) => n, _ => unreachable!() }).is_some_and(|(_, d)| !d.is_empty()) {
                return None;
            }
        }
        let dims: HashMap<_, _> = shapes
            .iter()
            .map(|(n, (_, d))| (n.clone(), d.clone()))
            .collect();
        let expanded = crate::input_items::expand(&[e.clone()], &dims, &|e| {
            crate::parameters::integer_constant(e).ok()
        })
        .ok()?;
        for value in expanded {
            if crate::input_items::index(&value).is_some() {
                continue;
            }
            if matches!(value, Expr::Bin(..) | Expr::Un(..)) {
                let mut array_expression = false;
                crate::procedure_arguments::visit_expr(&mut value.clone(), &mut |e| {
                    if let Expr::Ident(n) = e {
                        array_expression |= shapes.get(n).is_some_and(|(_, d)| !d.is_empty());
                    }
                });
                if array_expression {
                    return None;
                }
            }
            // Unknown function results might be arrays; do not guess their length.
            let kind = self.kind(&value, shapes, implicit);
            if kind.is_none() {
                return None;
            }
            if kind.as_ref().and_then(tag).is_none() && !matches!(kind, Some(TypeSpec::Complex(_)))
            {
                return None;
            }
            if let Expr::Call(n, _) = &value {
                if self.functions.get(n).is_some_and(|(_, d)| !d.is_empty()) {
                    return None;
                }
            }
            if matches!(kind, Some(TypeSpec::Complex(_))) {
                for component in 1..=2 {
                    out.push(Item {
                        tag: Some(2),
                        number,
                        component: Some(component),
                        element: None,
                    });
                }
            } else {
                out.push(Item {
                    tag: kind.as_ref().and_then(tag),
                    number,
                    component: None,
                    element: None,
                });
            }
            if out.len() >= LIMIT {
                return None;
            }
        }
        Some(())
    }
    fn statement(
        &mut self,
        name: &str,
        args: &[Expr],
        shapes: &Shapes,
        implicit: &ImplicitTyping,
        offset: usize,
    ) {
        let (format, first, input) = match name {
            "@formatted_print" => (0, 1, false),
            "@write" | "@formatted_write_noadvance" => (1, 2, false),
            "@formatted_read" => (1, 2, true),
            "@formatted_read_status" => (1, 4, true),
            _ => return,
        };
        let Some(format) = args
            .get(format)
            .and_then(|e| crate::parameters::character_constant(e).ok())
        else {
            return;
        };
        let mut items = vec![];
        let mut complete = true;
        for (number, e) in args.iter().skip(first).filter(|e|
            !matches!(e, Expr::Call(n, _) if n.starts_with("@write_option:") || n.starts_with("@io_spec:"))).enumerate() {
            if self.append(e, number+1, shapes, implicit, &mut items).is_none() { complete = false; break; }
        }
        if complete && !items.is_empty() {
            self.layout_warnings(&format, items.len(), input, offset);
        }
        if self.at_enabled && !input && self.literal.contains(&offset) {
            let list: Vec<&Expr> = args.iter().skip(first).filter(|e|
                !matches!(e, Expr::Call(n, _) if n.starts_with("@write_option:") || n.starts_with("@io_spec:"))).collect();
            let mut classes = vec![];
            let mut counts = vec![];
            for (i, e) in list.iter().enumerate() {
                let (class, mut count) = at_item(e, shapes);
                if class == AtItem::Other {
                    let mut known = vec![];
                    count = self.append(e, i + 1, shapes, implicit, &mut known).map(|_| known.len());
                    if count.is_none() { classes.clear(); break; }
                }
                classes.push(class);
                counts.push(count);
            }
            if classes.len() == list.len() && classes.iter().any(|c| matches!(c, AtItem::Trim | AtItem::TrimLoop { .. })) {
                if let Some(converted) = at_descriptors(&format, &classes, &counts) {
                    self.at.push(AtCandidate { offset, format: format.clone(), converted, items: classes });
                }
            }
        }
        if !complete {
            let list: Vec<&Expr> = args.iter().skip(first).filter(|e|
                !matches!(e, Expr::Call(n, _) if n.starts_with("@write_option:") || n.starts_with("@io_spec:"))).collect();
            self.size_dependent(&format, &list, shapes, implicit, input, offset, items.len());
        }
        // Syntax errors are already diagnosed by the compiler's format validator.
        let Ok(mut parser) = spec::GroupParser::new(format.as_bytes(), items.len()) else {
            return;
        };
        let mut index = 0;
        let mut reverted = false;
        // Each list item once, at its first mismatch: an array is one item.
        let mut reported = HashSet::new();
        while let Ok(Some(d)) = parser.next() {
            reverted |= d.code == spec::REVERSION;
            if !spec::is_data(d.code) {
                continue;
            }
            for _ in 0..d.repeat.min(items.len().saturating_sub(index)) {
                let item = &items[index];
                index += 1;
                let Some(tag) = item.tag else { continue };
                // BOZ editing also permits REAL/COMPLEX and LOGICAL data in
                // Fortran. The runtime currently renders INTEGER BOZ only;
                // that implementation limit is not a source type mismatch.
                // https://www.intel.com/content/www/us/en/docs/fortran-compiler/developer-guide-reference/2023-1/b-editing.html
                if spec::matches_type(d.code, tag)
                    || matches!(d.code, b'b' | b'o' | b'z') && matches!(tag, 1..=3)
                {
                    continue;
                }
                let descriptor = match d.code {
                    spec::ES => "ES".into(),
                    spec::EN => "EN".into(),
                    spec::AT => "AT".into(),
                    c => (c as char).to_ascii_uppercase().to_string(),
                };
                let descriptor = format!(
                    "{descriptor}{}{}",
                    d.width,
                    if d.has_precision {
                        format!(".{}", d.precision)
                    } else {
                        String::new()
                    }
                );
                let actual = ["unknown", "INTEGER", "REAL", "LOGICAL", "CHARACTER"][tag as usize];
                let expected = match d.code {
                    b'i' => "INTEGER",
                    b'b' | b'o' | b'z' => "numeric or LOGICAL data",
                    b'l' => "LOGICAL",
                    b'a' | spec::AT => "CHARACTER",
                    _ => "REAL",
                };
                let component = item
                    .component
                    .map_or(String::new(), |c| format!(", COMPLEX component {c}"));
                if !reported.insert(item.number) {
                    continue;
                }
                let after = if reverted { ", after format reversion" } else { "" };
                self.warnings.push((offset, "format", format!("format descriptor {descriptor} expects {expected}, but {} item {}{component} is {actual}{after} [format]", if input {"input"} else {"output"}, item.number), vec![]));
            }
        }
    }
    /// A list with arrays of unknown size that the format fits for some
    /// sizes but not others, as `(f0.4,1x,i0)` for `x, size(x)`. Sizes up to
    /// twice the format's data edit descriptors, past which the pattern
    /// repeats, are tried. `prefix` items were checked already.
    #[allow(clippy::too_many_arguments)]
    fn size_dependent(&mut self, format: &str, list: &[&Expr], shapes: &Shapes, implicit: &ImplicitTyping, input: bool, offset: usize, prefix: usize) {
        let mut parts = vec![];
        let mut names: Vec<String> = vec![];
        for (i, e) in list.iter().enumerate() {
            let mut fixed = vec![];
            if self.append(e, i + 1, shapes, implicit, &mut fixed).is_some() {
                parts.push(Part::Fixed(fixed));
                continue;
            }
            // A whole array keeps one size wherever it appears; a section
            // has its own.
            let (name, whole) = match e {
                Expr::Ident(n) => (n, true),
                Expr::Section { name, .. } => (name, false),
                _ => return,
            };
            let Some((kind, dims)) = shapes.get(name) else { return };
            if dims.is_empty() {
                return;
            }
            let number = i + 1;
            let element = match kind {
                TypeSpec::Complex(_) => (1..=2).map(|c| Item { tag: Some(2), number, component: Some(c), element: None }).collect(),
                k => match tag(k) { Some(t) => vec![Item { tag: Some(t), number, component: None, element: None }], None => return },
            };
            let size = match names.iter().position(|n| whole && n == name) {
                Some(p) => p,
                None => { names.push(if whole { name.clone() } else { format!("{name}(...)") }); names.len() - 1 }
            };
            parts.push(Part::Array { size, element });
        }
        if names.is_empty() || names.len() > 3 {
            return;
        }
        let Some(body) = format.trim().strip_prefix('(').and_then(|f| f.strip_suffix(')')) else { return };
        let Some(descriptors) = data_count(body.as_bytes(), &mut false) else { return };
        let bound = (2 * descriptors.max(1) + 2).min([40, 12, 6][names.len() - 1]);
        let mut works = vec![];
        let mut fails = vec![];
        let mut sizes = vec![0usize; names.len()];
        loop {
            let mut items = vec![];
            for part in &parts {
                match part {
                    Part::Fixed(f) => items.extend(f.iter().cloned()),
                    Part::Array { size, element } => {
                        for k in 0..sizes[*size] {
                            items.extend(element.iter().map(|x| Item { element: Some(k + 1), ..x.clone() }));
                        }
                    }
                }
            }
            match first_mismatch(format, &items) {
                None => works.push(sizes.clone()),
                Some((index, descriptor, reverted)) => fails.push((sizes.clone(), items[index].clone(), descriptor, reverted, index)),
            }
            // The next combination of sizes.
            let mut k = 0;
            while k < sizes.len() && sizes[k] == bound {
                sizes[k] = 0;
                k += 1;
            }
            if k == sizes.len() {
                break;
            }
            sizes[k] += 1;
        }
        if fails.is_empty() {
            return;
        }
        // Failures within the items before the first array were reported.
        if works.is_empty() && fails.iter().all(|f| f.4 < prefix) {
            return;
        }
        // The example: the smallest failing sizes with every array nonempty,
        // or else the first.
        let example = fails.iter().filter(|f| f.0.iter().all(|&n| n > 0)).min_by_key(|f| f.0.iter().sum::<usize>()).unwrap_or(&fails[0]);
        let (sizes, item, descriptor, reverted, _) = example;
        let elements = |n: usize| format!("{n} element{}", if n == 1 { "" } else { "s" });
        let quoted: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
        let fit = if names.len() == 1 {
            let good: Vec<usize> = works.iter().map(|w| w[0]).collect();
            match good.as_slice() {
                [] => format!("for no size of {}", quoted[0]),
                [0] => format!("for no nonempty size of {}", quoted[0]),
                [n] => format!("only when {} has {}", quoted[0], elements(*n)),
                g if g.len() <= 4 => format!("only when {} has {} elements", quoted[0],
                    g.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ").rsplitn(2, ", ").collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" or ")),
                _ => format!("only for some sizes of {}", quoted[0]),
            }
        } else if works.is_empty() {
            format!("for no sizes of {}", quoted.join(" and "))
        } else {
            format!("only for some sizes of {}", quoted.join(" and "))
        };
        let with = names.iter().zip(sizes).map(|(n, &k)| format!("{} in `{n}`", elements(k))).collect::<Vec<_>>().join(" and ");
        let actual = TYPE_NAMES[item.tag.unwrap_or(0) as usize];
        let at = match (item.element, item.component) {
            (Some(e), Some(c)) => format!(" (element {e}, COMPLEX component {c})"),
            (Some(e), None) => format!(" (element {e})"),
            (None, Some(c)) => format!(" (COMPLEX component {c})"),
            _ => String::new(),
        };
        let after = if *reverted { ", after format reversion" } else { "" };
        self.warnings.push((offset, "format", format!(
            "the format matches the {} list {fit}; with {with}, {} item {}{at} is {actual} but meets {descriptor}{after} [format]",
            if input { "input" } else { "output" }, if input { "input" } else { "output" }, item.number),
            vec![
                "put the array last, in an unlimited group, as in `(i0,*(1x,f0.4))` for `size(x), x`, or write it in a statement of its own".to_owned(),
                format!("sizes up to {bound} were tried"),
            ]));
    }
    /// `-Wformat-reversion` and `-Wformat-unused`.
    fn layout_warnings(&mut self, format: &str, count: usize, input: bool, offset: usize) {
        let items = |n: usize| format!("{n} item{}", if n == 1 { "" } else { "s" });
        let descriptors = |n: usize| format!("{n} data edit descriptor{}", if n == 1 { "" } else { "s" });
        match layout(format, count) {
            Some(Layout::Reversion { descriptors: n, repeated: false }) => {
                let (verb, effect) = if input { ("read", "moves to the next record") } else { ("written", "starts a new record") };
                self.warnings.push((offset, "format-reversion", format!(
                    "the format `{}` has {} for {}; the other {} {verb} by format reversion, which {effect} [format-reversion]",
                    format.trim(), descriptors(n), items(count), if count - n == 1 { "item is".to_owned() } else { format!("{} items are", count - n) }),
                    vec![
                        "to keep the items in one record, use an unlimited repeat, such as `(*(g0,:,1x))`, or a repeat count".to_owned(),
                        "a format whose descriptors have a repeat count, such as `(10f8.3)`, is taken to revert on purpose".to_owned(),
                    ]));
            }
            // A repeat count, as in `(10f8.3)`, is taken as a limit, so a
            // partly used one is not reported.
            Some(Layout::Unused { at, repeat: None, top: true, colon: false }) if !self.shared.contains(&offset) => {
                let mut total_repeated = false;
                let trimmed = format.trim();
                let total = trimmed.strip_prefix('(').and_then(|f| f.strip_suffix(')'))
                    .and_then(|body| data_count(body.as_bytes(), &mut total_repeated)).unwrap_or(0);
                // Everything from the first unused descriptor on is unused.
                let close = format.rfind(')').unwrap_or(format.len());
                let unused = format[at.start..close].trim().trim_end_matches(',').trim();
                let what = format!("`{unused}` {}", if unused.contains(',') { "are" } else { "is" });
                let kept = format[..at.start].trim_end().trim_end_matches(',').trim_end();
                let mut notes = vec![format!("suggestion: \"{kept})\"")];
                notes.push(format!("{} stops at the first data edit descriptor without an item; check that no item is missing",
                    if input { "input" } else { "output" }));
                self.warnings.push((offset, "format-unused", format!(
                    "{what} never used: the format has {} for {} [format-unused]", descriptors(total), items(count)), notes));
            }
            _ => {}
        }
    }
    fn scope(
        &mut self,
        body: &mut [Stmt],
        parent: &Shapes,
        inherited: &ImplicitTyping,
        contained: Option<&[String]>,
        params: &[String],
    ) {
        let outer_functions = self.functions.clone();
        for n in params {
            self.functions.remove(n);
        }
        self.functions(body);
        let mut shapes = parent.clone();
        for n in params {
            shapes.remove(n);
        }
        declarations(body, &mut shapes);
        let mut implicit = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::Implicit(Implicit::None) => implicit.apply_implicit_none(),
                Stmt::Implicit(Implicit::Rule {
                    type_spec,
                    letter_ranges,
                }) => implicit.add_rule(type_spec.clone(), letter_ranges.clone(), 0..0),
                Stmt::Implicit(Implicit::Rules { rules }) => {
                    for r in rules {
                        implicit.add_rule(r.type_spec.clone(), r.letter_ranges.clone(), 0..0);
                    }
                }
                _ => {}
            }
        }
        let mut offset = 0;
        for s in body {
            match s {
                Stmt::SourceLocation(p) => offset = *p,
                Stmt::CallSub { name, args } => {
                    self.statement(name, args, &shapes, &implicit, offset)
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let host = contained.is_none_or(|c| c.contains(name));
                    if host {
                        self.scope(body, &shapes, &implicit, None, params);
                    } else {
                        self.scope(
                            body,
                            &self.globals.clone(),
                            &ImplicitTyping::new(),
                            None,
                            params,
                        );
                    }
                }
                Stmt::Module { body, .. } => self.scope(
                    body,
                    &self.globals.clone(),
                    &ImplicitTyping::new(),
                    None,
                    &[],
                ),
                _ => {
                    for child in crate::procedure_arguments::children(s) {
                        self.scope(child, &shapes, &implicit, None, &[]);
                    }
                }
            }
        }
        self.functions = outer_functions;
    }
}
/// The -Wcould-be-at warning for a candidate, if the statement rewritten
/// with AT is shorter.
fn at_suggestion(c: &AtCandidate, src: &str, tokens: &[crate::lexer::Token]) -> Option<(usize, &'static str, String, Vec<String>)> {
    use crate::lexer::TokenKind as K;
    let ts = crate::declaration_warnings::statement_tokens(tokens, c.offset, usize::MAX, src);
    let text = |t: &crate::lexer::Token| &src[t.span.clone()];
    let f = ts.iter().position(|t| matches!(&t.kind, K::Str(s) if *s == c.format))?;
    let literal = text(&ts[f]);
    let quote = literal.chars().next()?;
    // Doubled quotes would move the descriptors' positions.
    if literal[1..literal.len() - 1].contains(quote) {
        return None;
    }
    // The output list: after the format's comma in PRINT, after the control
    // list in WRITE.
    let start = if text(&ts[0]).eq_ignore_ascii_case("print") {
        f + 2
    } else {
        let mut depth = 0;
        let mut end = None;
        for (i, t) in ts.iter().enumerate().skip(1) {
            match t.kind {
                K::LParen => depth += 1,
                K::RParen => { depth -= 1; if depth == 0 { end = Some(i + 1); break; } }
                _ => {}
            }
        }
        end?
    };
    let list = ts.get(start..)?;
    let mut items: Vec<&[crate::lexer::Token]> = vec![];
    let (mut depth, mut begin) = (0, 0);
    for (i, t) in list.iter().enumerate() {
        match t.kind {
            K::LParen | K::LBracket => depth += 1,
            K::RParen | K::RBracket => depth -= 1,
            K::Comma if depth == 0 => { items.push(&list[begin..i]); begin = i + 1; }
            _ => {}
        }
    }
    items.push(&list[begin..]);
    if items.len() != c.items.len() || items.iter().any(|i| i.is_empty()) {
        return None;
    }
    let span = |t: &[crate::lexer::Token]| t[0].span.start..t[t.len() - 1].span.end;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = vec![];
    for (item, class) in items.iter().zip(&c.items) {
        match class {
            AtItem::Trim => {
                if !text(&item[0]).eq_ignore_ascii_case("trim") || item.len() < 4 { return None; }
                edits.push((span(item), src[span(&item[2..item.len() - 1])].to_owned()));
            }
            AtItem::TrimLoop { whole } => {
                // (trim(name(v)), v = a, b)
                let name = text(item.get(3)?);
                let eq = item.iter().position(|t| matches!(t.kind, K::Eq))?;
                let mut depth = 0;
                let mut comma = None;
                for (i, t) in item.iter().enumerate().skip(eq) {
                    match t.kind {
                        K::LParen | K::LBracket => depth += 1,
                        K::RParen | K::RBracket => depth -= 1,
                        K::Comma if depth == 0 => { comma = Some(i); break; }
                        _ => {}
                    }
                }
                let comma = comma?;
                let section = if *whole {
                    name.to_owned()
                } else {
                    format!("{name}({}:{})", &src[span(&item[eq + 1..comma])], &src[span(&item[comma + 1..item.len() - 1])])
                };
                edits.push((span(item), section));
            }
            _ => {}
        }
    }
    let mut format = c.format.clone();
    for r in c.converted.iter().rev() {
        format.insert(r.end, 't');
    }
    edits.push((ts[f].span.clone(), format!("{quote}{format}{quote}")));
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let whole = span(ts);
    let mut statement = src[whole.clone()].to_owned();
    for (r, new) in edits {
        statement.replace_range(r.start - whole.start..r.end - whole.start, &new);
    }
    let (before, after) = (one_line(&src[whole]), one_line(&statement));
    (after.len() < before.len()).then(|| (c.offset, "could-be-at", "the AT edit descriptor can replace `trim` in this output list [could-be-at]".to_owned(), vec![
        format!("suggestion: {after}"),
        "AT, new in Fortran 2023, writes a CHARACTER value without its trailing blanks; compilers without Fortran 2023 support reject it".to_owned(),
    ]))
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
    standard: Option<crate::standards::Standard>,
) -> Result<()> {
    // -Wformat is in -Wall; -Wformat-reversion and -Wformat-unused are opt-in.
    let mut optional = settings.clone();
    optional.wall = false;
    let mut controls: HashMap<&str, (bool, bool)> = [("format", settings), ("format-reversion", &optional), ("format-unused", &optional)]
        .into_iter().map(|(c, s)| (c, crate::unused::selected(c, false, s, options))).collect();
    // -Wcould-be-at is opt-in, and in -Wstyle for Fortran 2023, which has AT.
    let f2023 = standard.is_some_and(|s| s >= "f2023".parse().expect("standard"));
    controls.insert("could-be-at", crate::unused::selected_as("could-be-at", false, &optional, options, f2023));
    if !controls.values().any(|c| c.0) {
        return Ok(());
    }
    let (shared, literal) = shared_formats(&mut program.body.clone(), src);
    let mut normalized = program.clone();
    // Work on a copy: diagnostics must not alter the compilation pipeline.
    let _ = crate::statement_labels::formats(&mut normalized.body);
    if let Ok(associated) = crate::modules::associate(normalized.clone()) {
        normalized = crate::parameters::resolve_constants(associated.clone()).unwrap_or(associated);
    }
    let mut a = Analysis {
        functions: Shapes::new(),
        globals: Shapes::new(),
        warnings: vec![],
        shared,
        literal,
        at_enabled: controls["could-be-at"].0,
        at: vec![],
    };
    a.functions(&normalized.body);
    a.scope(
        &mut normalized.body,
        &a.globals.clone(),
        &ImplicitTyping::new(),
        Some(&normalized.contains),
        &[],
    );
    let tokens = crate::lexer::lex(src);
    for c in std::mem::take(&mut a.at) {
        if let Some(w) = at_suggestion(&c, src, &tokens) {
            a.warnings.push(w);
        }
    }
    a.warnings.sort();
    a.warnings.dedup();
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFile,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut failed = false;
    for (pos, category, message, notes) in a.warnings {
        let (enabled, error) = controls[category];
        if pos >= own_len || !enabled {
            continue;
        }
        let span = tokens
            .iter()
            .find(|t| t.span.start >= pos)
            .map(|t| t.span.clone())
            .unwrap_or(pos..pos);
        let (name, source, span) = if let Some(o) = origins.iter().find(|o| o.range.contains(&pos))
        {
            (
                o.name.as_str(),
                o.source(src),
                o.span(span),
            )
        } else {
            (filename, src, span)
        };
        let d = if error {
            codespan_reporting::diagnostic::Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(message)
        .with_labels(vec![Label::primary((), span)])
        .with_notes(notes);
        emit_to_io_write(
            &mut StandardStream::stderr(ColorChoice::Auto),
            &Config::default(),
            &SimpleFile::new(name, source),
            &d,
        )?;
        failed |= error;
    }
    if failed {
        bail!("format warnings treated as errors");
    }
    Ok(())
}

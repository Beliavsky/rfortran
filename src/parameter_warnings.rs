// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! `-Wcould-be-parameter`, opt-in: a local scalar variable that is defined
//! once, from a constant expression, and never changed could be a named
//! constant. After the rules of the xparam.py checker.
//!
//! The variable is a local of a main program or procedure, of intrinsic type,
//! without attributes, and not a dummy argument or function result. Its one
//! definition is an initialization in its declaration, or an assignment at
//! the top level of the executable part (not inside IF, a loop, SELECT or
//! BLOCK) that precedes every other reference. The value is a constant
//! expression: literals, named constants, intrinsic operators, intrinsics
//! allowed in constant expressions, and other variables accepted by this
//! check. Every other appearance must be one the analysis knows cannot
//! define the variable; anything else (READ, DO, an actual argument whose
//! dummy is not known to be INTENT(IN) or VALUE, an assignment in an
//! internal procedure, DATA, NAMELIST, ...) counts as a definition.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::lexer::{lex, Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

const CATEGORY: &str = "could-be-parameter";

/// Intrinsic functions permitted in constant expressions, as in xparam.py.
const CONSTANT_INTRINSICS: &[&str] = &[
    "abs", "achar", "acos", "adjustl", "adjustr", "aimag", "aint", "anint", "asin", "atan", "atan2",
    "bit_size", "ceiling", "char", "cmplx", "conjg", "cos", "cosh", "digits", "dim", "dble",
    "epsilon", "exp", "floor", "huge", "iachar", "iand", "ibclr", "ibits", "ibset", "ichar",
    "ieor", "index", "int", "ior", "kind", "len", "len_trim", "log", "log10", "max", "maxexponent",
    "merge", "min", "minexponent", "mod", "modulo", "new_line", "nint", "not", "precision", "radix",
    "range", "real", "repeat", "scan", "selected_char_kind", "selected_int_kind",
    "selected_real_kind", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "tiny", "trim", "verify",
];

/// The identifiers a statement mentions anywhere.
fn words(s: &Stmt) -> HashSet<String> {
    fn add(v: &serde_json::Value, out: &mut HashSet<String>) {
        match v {
            serde_json::Value::String(s) => out.extend(s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty()).map(str::to_ascii_lowercase)),
            serde_json::Value::Array(items) => for v in items { add(v, out); },
            serde_json::Value::Object(fields) => for v in fields.values() { add(v, out); },
            _ => {}
        }
    }
    let mut out = HashSet::new();
    if let Ok(v) = serde_json::to_value(s) { add(&v, &mut out); }
    out
}

fn declaration(s: &Stmt) -> bool {
    matches!(s, Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. }
        | Stmt::DummyIntent { .. } | Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_) | Stmt::DerivedType { .. }
        | Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } | Stmt::Accessibility { .. })
}

/// The dummy arguments and their INTENT or VALUE attributes of each
/// procedure with a unique name in the program.
type Signatures = HashMap<String, Option<(Vec<String>, HashMap<String, String>)>>;
fn signatures(body: &[Stmt], out: &mut Signatures) {
    for s in body {
        match s {
            Stmt::Function { name, params, body, .. } | Stmt::Subroutine { name, params, body, .. } => {
                let intents = body.iter().filter_map(|s| match s {
                    Stmt::DummyIntent { name, intent } if intent.starts_with("intent(") || intent == "value" => Some((name.clone(), intent.clone())),
                    _ => None,
                }).collect();
                let entry = (params.clone(), intents);
                out.entry(name.clone()).and_modify(|e| *e = None).or_insert(Some(entry));
                signatures(body, out);
            }
            Stmt::Module { body, .. } => signatures(body, out),
            _ => {}
        }
    }
}

/// The variable an expression designates, if it is one: `x`, `x(i)`, `x(1:2)`.
fn designator(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Some(n.split('%').next().unwrap_or(n)),
        Expr::Call(n, args) if n.starts_with("@keyword:") && args.len() == 1 => designator(&args[0]),
        _ => None,
    }
}

struct Scan<'a> {
    candidates: &'a HashSet<String>,
    signatures: &'a Signatures,
    /// Definitions of each candidate, and whether each is a plain top-level
    /// assignment.
    definitions: HashMap<String, usize>,
}

impl Scan<'_> {
    fn define(&mut self, name: &str) {
        let name = name.split('%').next().unwrap_or(name);
        if self.candidates.contains(name) { *self.definitions.entry(name.to_owned()).or_default() += 1; }
    }
    /// An actual argument: a definition unless the dummy is INTENT(IN) or VALUE.
    fn actual(&mut self, procedure: &str, position: usize, arg: &Expr) {
        if let Some(n) = designator(arg).filter(|n| self.candidates.contains(*n)) {
            let keyword = match arg { Expr::Call(t, _) => t.strip_prefix("@keyword:"), _ => None };
            let input = self.signatures.get(procedure).and_then(|s| s.as_ref()).is_some_and(|(params, intents)| {
                let dummy = keyword.map(str::to_owned).or_else(|| params.get(position).cloned());
                dummy.and_then(|d| intents.get(&d)).is_some_and(|i| i == "intent(in)" || i == "value")
            });
            if !input { self.define(n); }
        }
        self.expression(arg);
    }
    fn expression(&mut self, e: &Expr) {
        match e {
            // An implied DO defines its control variable, the first argument;
            // its bounds and items are read.
            Expr::Call(tag, args) if tag.contains("implied_do") => {
                match args.first().and_then(designator) {
                    Some(n) => self.define(n),
                    // An unfamiliar form: every name it mentions may be defined.
                    None => {
                        let mentioned = words(&Stmt::Print { items: vec![e.clone()] });
                        for n in self.candidates.iter().filter(|n| mentioned.contains(*n)).cloned().collect::<Vec<_>>() { self.define(&n); }
                    }
                }
                for a in args.iter().skip(1) { self.expression(a); }
            }
            Expr::Call(f, args) | Expr::Index(f, args) => {
                // Intrinsic and generated calls do not define their arguments;
                // a user function might.
                let user = matches!(e, Expr::Call(..)) && !f.starts_with('@') && crate::intrinsics::purity(f).is_none();
                for (i, a) in args.iter().enumerate() {
                    if user { self.actual(f, i, a); } else { self.expression(a); }
                }
            }
            Expr::Bin(_, a, b) => { self.expression(a); self.expression(b); }
            Expr::Un(_, a) => self.expression(a),
            Expr::Section { lower, upper, stride, .. } => for b in [lower, upper, stride].into_iter().flatten() { self.expression(b); },
            _ => {}
        }
    }
    fn statements(&mut self, body: &[Stmt]) {
        for s in body { self.statement(s); }
    }
    fn statement(&mut self, s: &Stmt) {
        match s {
            _ if declaration(s) => {}
            Stmt::Assign { name, value } => { self.define(name); self.expression(value); }
            Stmt::AssignIndex { name, indices, value } => {
                self.define(name);
                for e in indices { self.expression(e); }
                self.expression(value);
            }
            Stmt::Do { var, start, end, body } => {
                self.define(var);
                self.expression(start);
                self.expression(end);
                self.statements(body);
            }
            Stmt::DoWhile { cond, body } => { self.expression(cond); self.statements(body); }
            Stmt::If { cond, then_body, else_body } => {
                self.expression(cond);
                self.statements(then_body);
                if let Some(b) = else_body { self.statements(b); }
            }
            Stmt::SelectCase { expr, cases, default } => {
                self.expression(expr);
                for c in cases {
                    for item in &c.items {
                        match item {
                            crate::ast::CaseItem::Single(e) => self.expression(e),
                            crate::ast::CaseItem::Range(a, b) => { self.expression(a); self.expression(b); }
                        }
                    }
                    self.statements(&c.body);
                }
                if let Some(b) = default { self.statements(b); }
            }
            Stmt::Block { body } => self.statements(body),
            Stmt::Print { items } => for e in items { self.expression(e); },
            Stmt::Return(e) => if let Some(e) = e { self.expression(e); },
            Stmt::CallSub { name, args } if matches!(name.as_str(), "@write" | "@formatted_write_noadvance" | "@unformatted_write") => {
                for (i, a) in args.iter().enumerate() {
                    match a {
                        // An internal file is written.
                        _ if i == 0 => if let Some(n) = designator(a) { self.define(n); } else { self.expression(a); },
                        // IOSTAT=, IOMSG= and SIZE= are defined; other controls are read.
                        Expr::Call(option, values) if option.starts_with("@write_option:") => {
                            let key = &option["@write_option:".len()..];
                            for v in values {
                                match designator(v) {
                                    Some(n) if matches!(key, "iostat" | "iomsg" | "size" | "id") => self.define(n),
                                    _ => self.expression(v),
                                }
                            }
                        }
                        _ => self.expression(a),
                    }
                }
            }
            // ALLOCATE and DEALLOCATE define their objects, STAT= and ERRMSG=,
            // and read bounds, SOURCE= and MOLD=.
            Stmt::CallSub { name, args } if name == "@allocate" || name == "@deallocate" => {
                for a in args {
                    match a {
                        Expr::Call(key, values) if key.starts_with("@keyword:") => {
                            let defined = matches!(&key["@keyword:".len()..], "stat" | "errmsg");
                            for v in values {
                                match designator(v) {
                                    Some(n) if defined => self.define(n),
                                    _ => self.expression(v),
                                }
                            }
                        }
                        Expr::Call(object, bounds) | Expr::Index(object, bounds) => {
                            self.define(object);
                            for b in bounds { self.expression(b); }
                        }
                        _ => match designator(a) { Some(n) => self.define(n), None => self.expression(a) },
                    }
                }
            }
            Stmt::CallSub { name, args } if !name.starts_with('@') => {
                for (i, a) in args.iter().enumerate() { self.actual(name, i, a); }
            }
            // An internal procedure defines a host variable it assigns,
            // unless it declares its own.
            Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                let own: HashSet<String> = params.iter().cloned().chain(body.iter().flat_map(|s| match s {
                    Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
                    Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                    Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.clone()).collect(),
                    _ => vec![],
                })).collect();
                let visible: HashSet<String> = self.candidates.difference(&own).cloned().collect();
                let mut inner = Scan { candidates: &visible, signatures: self.signatures, definitions: HashMap::new() };
                inner.statements(body);
                for (n, k) in inner.definitions { *self.definitions.entry(n).or_default() += k; }
            }
            // Anything else that mentions a candidate may define it.
            _ => {
                let mentioned = words(s);
                for n in self.candidates.iter().filter(|n| mentioned.contains(*n)).cloned().collect::<Vec<_>>() { self.define(&n); }
            }
        }
    }
}

fn constant(e: &Expr, constants: &HashSet<String>) -> bool {
    match e {
        Expr::IntLit(_) | Expr::Str(_) | Expr::Logical(_) => true,
        // A kind suffix such as 1.0_dp names a constant.
        Expr::RealLit(s) => s.split_once('_').is_none_or(|(_, k)| k.bytes().all(|b| b.is_ascii_digit()) || constants.contains(k)),
        Expr::Ident(n) => constants.contains(n),
        Expr::Bin(_, a, b) => constant(a, constants) && constant(b, constants),
        Expr::Un(_, a) => constant(a, constants),
        Expr::Call(f, args) if f.starts_with("@keyword:") || f == "@array_constructor" || f == "@no_pad" || f == "@no_order"
            || CONSTANT_INTRINSICS.contains(&f.as_str()) && !constants.contains(f) =>
            args.iter().all(|a| constant(a, constants)),
        // An element of a named constant array.
        Expr::Index(n, args) | Expr::Call(n, args) if constants.contains(n) => args.iter().all(|a| constant(a, constants)),
        _ => false,
    }
}

fn supported(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision
        | TypeSpec::Complex(_) | TypeSpec::Logical | TypeSpec::LogicalKind(_) | TypeSpec::Character(Some(_)) | TypeSpec::CharacterLength(_))
}

/// The named constants a scope declares, and the names of its variables.
fn declared(body: &[Stmt]) -> (HashSet<String>, HashSet<String>) {
    let (mut constants, mut variables) = (HashSet::new(), HashSet::new());
    for s in body {
        match s {
            Stmt::InitializedDecl { attributes, entities, .. } => for (n, _) in entities {
                if attributes.iter().any(|a| a == "parameter") { constants.insert(n.clone()); } else { variables.insert(n.clone()); }
            },
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => variables.extend(names.iter().cloned()),
            Stmt::ArrayDecl { name, .. } => { variables.insert(name.clone()); }
            _ => {}
        }
    }
    for c in &constants { variables.remove(c); }
    (constants, variables)
}

/// The values of the INTEGER named constants a scope declares, added to
/// those of its hosts, for array extents and subscripts.
type Integers = HashMap<String, Expr>;
fn integers(body: &[Stmt], hosts: &Integers) -> Integers {
    let mut out = hosts.clone();
    for s in body {
        if let Stmt::InitializedDecl { attributes, entities, .. } = s {
            if !attributes.iter().any(|a| a == "parameter") { continue; }
            for (n, v) in entities {
                match v.as_ref().map(|v| crate::parameters::evaluate(v, &out)) {
                    Some(Ok(value)) => { out.insert(n.clone(), Expr::IntLit(value.to_string())); }
                    _ => { out.remove(n); }
                }
            }
        }
    }
    out
}

/// The lower bound and extent of each dimension of a declared shape.
fn shape(dims: &[Expr], integers: &Integers) -> Option<Vec<(i64, i64)>> {
    if dims.is_empty() { return None; }
    dims.iter().map(|d| match d {
        Expr::Call(tag, b) if tag == "@declared_bounds" && b.len() == 2 => {
            let lower = crate::parameters::evaluate(&b[0], integers).ok()?;
            let upper = crate::parameters::evaluate(&b[1], integers).ok()?;
            Some((lower, (upper - lower + 1).max(0)))
        }
        d => Some((1, crate::parameters::evaluate(d, integers).ok()?.max(0))),
    }).collect()
}

/// The elements, as offsets in array element order, that subscripts
/// select, listed in array element order of the section.
fn positions(indices: &[Expr], shape: &[(i64, i64)], integers: &Integers) -> Option<Vec<usize>> {
    if indices.len() != shape.len() { return None; }
    let mut per_dimension = vec![];
    for (index, &(lower, extent)) in indices.iter().zip(shape) {
        let upper = lower + extent - 1;
        let values: Vec<i64> = match index {
            // A rank-one section is written as the section itself.
            Expr::Section { .. } | Expr::Call(..) if matches!(index, Expr::Section { .. })
                || matches!(index, Expr::Call(tag, s) if tag == "@section_subscript" && s.len() == 1) => {
                let section = match index { Expr::Call(_, s) => &s[0], s => s };
                let Expr::Section { lower: l, upper: u, stride, .. } = section else { return None };
                let bound = |e: &Option<Box<Expr>>, default: i64| e.as_ref().map_or(Some(default), |e| crate::parameters::evaluate(e, integers).ok());
                let (first, last, step) = (bound(l, lower)?, bound(u, upper)?, bound(stride, 1)?);
                if step == 0 { return None; }
                let mut v = vec![];
                let mut i = first;
                while if step > 0 { i <= last } else { i >= last } { v.push(i); i += step; }
                v
            }
            e => vec![crate::parameters::evaluate(e, integers).ok()?],
        };
        if values.iter().any(|v| *v < lower || *v > upper) { return None; }
        per_dimension.push(values);
    }
    // The first subscript varies fastest: build from the last dimension in.
    let mut strides = vec![1usize; shape.len()];
    for d in 1..shape.len() { strides[d] = strides[d - 1] * shape[d - 1].1 as usize; }
    let mut out = vec![0usize];
    for d in (0..shape.len()).rev() {
        out = out.iter().flat_map(|o| per_dimension[d].iter().map(move |v| (o, v)))
            .map(|(o, v)| o + (v - shape[d].0) as usize * strides[d]).collect();
    }
    // `out` now varies the last dimension slowest and the first fastest.
    Some(out)
}

/// The values a right-hand side gives `n` elements: the items of an array
/// constructor, or a scalar for each. None when the value is not plainly
/// one of these.
fn element_values(value: &Expr, n: usize, arrays: &HashSet<String>) -> Option<Vec<Expr>> {
    fn scalar(e: &Expr, arrays: &HashSet<String>) -> bool {
        match e {
            Expr::Ident(n) => !arrays.contains(n),
            Expr::Section { .. } => false,
            Expr::Call(tag, _) if tag == "@array_constructor" || tag.contains("implied_do") || tag == "@section_subscript" => false,
            Expr::Index(_, args) | Expr::Call(_, args) => args.iter().all(|a| scalar(a, arrays)),
            Expr::Bin(_, a, b) => scalar(a, arrays) && scalar(b, arrays),
            Expr::Un(_, a) => scalar(a, arrays),
            _ => true,
        }
    }
    match value {
        Expr::Call(tag, items) if tag == "@array_constructor" =>
            (items.len() == n && items.iter().all(|i| scalar(i, arrays))).then(|| items.clone()),
        v if scalar(v, arrays) => Some(vec![v.clone(); n]),
        _ => None,
    }
}

struct Report {
    name: String,
    declaration: usize,
    /// The defining assignments, when the variable is not initialized in its declaration.
    assignments: Vec<usize>,
    /// For an array defined by element or section assignments: each
    /// assignment and the elements it defines.
    pieces: Vec<(usize, Vec<usize>)>,
    /// The extents of an array defined by pieces.
    extents: Vec<i64>,
    /// Named constants its value uses that are defined after its declaration.
    later: Vec<String>,
}

/// The defining statements of a candidate: their values to check for
/// constancy, and for an array defined in pieces, the elements each defines.
#[derive(Default)]
struct Definition {
    values: Vec<Expr>,
    statements: Vec<Option<usize>>,
    pieces: Vec<Vec<usize>>,
    initialized: bool,
    /// Every element of an array defined in pieces, once each.
    elements: Option<Vec<Option<Expr>>>,
}

/// One main program or procedure: its statements (internal procedures
/// included), dummy arguments and result names, and the named constants and
/// variables of its hosts.
#[allow(clippy::too_many_arguments)]
fn unit(body: &[Stmt], excluded: &HashSet<String>, host_constants: &HashSet<String>, host_variables: &HashSet<String>,
        integers: &Integers, array_names: &HashSet<String>, signatures: &Signatures, out: &mut Vec<Report>) {
    let (own_constants, own_variables) = declared(body);
    let mut constants: HashSet<String> = host_constants.difference(&own_variables).cloned().collect();
    constants.extend(own_constants);
    for v in host_variables { if !own_variables.contains(v) { constants.remove(v); } }
    // Candidates: locals of intrinsic type without attributes; arrays need
    // a constant shape.
    let mut shapes: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
    let mut unshaped: HashSet<&String> = HashSet::new();
    for s in body {
        if let Stmt::ArrayDecl { kind, name, dims } = s {
            match shape(dims, integers).filter(|_| supported(kind)) {
                Some(s) => { shapes.insert(name.clone(), s); }
                None => { unshaped.insert(name); }
            }
        }
    }
    let attributed: HashSet<&String> = body.iter().filter_map(|s| match s { Stmt::DummyIntent { name, .. } => Some(name), _ => None }).collect();
    let mut at = None;
    let mut decl_at = HashMap::new();
    let mut initial = HashMap::new();
    let mut candidates = HashSet::new();
    for s in body {
        match s {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::VarDecl { kind, names } if supported(kind) => for n in names {
                candidates.insert(n.clone());
                decl_at.entry(n.clone()).or_insert(at);
            },
            Stmt::ArrayDecl { name, .. } if shapes.contains_key(name) => {
                candidates.insert(name.clone());
                decl_at.entry(name.clone()).or_insert(at);
            }
            Stmt::InitializedDecl { kind, attributes, entities } if supported(kind) && attributes.iter().all(|a| a.starts_with('@')) => for (n, v) in entities {
                candidates.insert(n.clone());
                decl_at.entry(n.clone()).or_insert(at);
                if let Some(v) = v { initial.insert(n.clone(), v.clone()); }
            },
            _ => {}
        }
    }
    candidates.retain(|n| !unshaped.contains(n) && !attributed.contains(n) && !excluded.contains(n));
    initial.retain(|n, _| candidates.contains(n));
    if candidates.is_empty() { return; }
    let mut scan = Scan { candidates: &candidates, signatures, definitions: HashMap::new() };
    scan.statements(body);
    let counts = scan.definitions;
    // The defining assignments of each candidate without an initialization:
    // top-level statements before which the candidate is not otherwise
    // mentioned. A scalar has one; an array has a whole-array assignment,
    // or element and section assignments with constant subscripts.
    let mut definitions: HashMap<String, Definition> = HashMap::new();
    let mut mentioned: HashSet<String> = HashSet::new();
    let mut at = None;
    for s in body {
        let defining = match s {
            Stmt::Assign { name, value } if candidates.contains(name) && !initial.contains_key(name) && !mentioned.contains(name) => Some((name, value, None)),
            Stmt::AssignIndex { name, indices, value } if shapes.contains_key(name) && candidates.contains(name)
                && !initial.contains_key(name) && !mentioned.contains(name) => Some((name, value, Some(indices))),
            _ => None,
        };
        match (s, defining) {
            (Stmt::SourceLocation(p), _) => at = Some(*p),
            (_, Some((name, value, indices))) => {
                let d = definitions.entry(name.clone()).or_default();
                d.statements.push(at);
                match (shapes.get(name), indices) {
                    // A scalar, or a whole array, has one definition.
                    (None, _) | (Some(_), None) => { d.values.push(value.clone()); mentioned.insert(name.clone()); }
                    (Some(shape), Some(indices)) => {
                        let size: i64 = shape.iter().map(|(_, e)| e).product();
                        let elements = d.elements.get_or_insert_with(|| vec![None; size as usize]);
                        let piece = positions(indices, shape, integers);
                        let values = piece.as_ref().and_then(|p| element_values(value, p.len(), array_names));
                        match (piece, values) {
                            (Some(piece), Some(values)) if piece.iter().all(|&p| elements[p].is_none()) => {
                                for (&p, v) in piece.iter().zip(values) { elements[p] = Some(v); }
                                d.pieces.push(piece);
                            }
                            // Overlapping or unrecognized subscripts end the analysis of this array.
                            _ => { mentioned.insert(name.clone()); d.values.push(Expr::Ident("@unknown".into())); }
                        }
                    }
                }
                let mut words = words(&Stmt::Print { items: vec![value.clone()] });
                words.remove(name);
                mentioned.extend(words);
            }
            (Stmt::Function { .. } | Stmt::Subroutine { .. }, _) => {}
            (s, _) if declaration(s) => {}
            (s, _) => mentioned.extend(words(s)),
        }
    }
    for (n, v) in &initial {
        if !counts.contains_key(n) {
            definitions.insert(n.clone(), Definition { values: vec![v.clone()], initialized: true, ..Default::default() });
        }
    }
    // Every definition must be one of these; an array defined in pieces
    // must have every element defined.
    definitions.retain(|n, d| {
        let complete = d.elements.as_ref().is_none_or(|e| e.iter().all(Option::is_some));
        complete && counts.get(n).copied().unwrap_or(0) == if d.initialized { 0 } else { d.statements.len() }
    });
    for d in definitions.values_mut() {
        if let Some(elements) = d.elements.take() { d.values.extend(elements.into_iter().flatten()); }
    }
    // Where the scope's named constants are defined, including those
    // accepted here, which move to their declarations.
    let mut defined_at: HashMap<String, usize> = HashMap::new();
    let mut at = None;
    for s in body {
        match s {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "parameter") => {
                if let Some(p) = at { for (n, _) in entities { defined_at.entry(n.clone()).or_insert(p); } }
            }
            _ => {}
        }
    }
    // Accept definitions whose values are constant, given the constants accepted so far.
    loop {
        let ready: Vec<String> = definitions.iter().filter(|(_, d)| d.values.iter().all(|v| constant(v, &constants)))
            .map(|(n, _)| n.clone()).collect();
        if ready.is_empty() { break; }
        for name in ready {
            let d = definitions.remove(&name).unwrap();
            constants.insert(name.clone());
            let Some(Some(declaration)) = decl_at.get(&name).copied() else { continue };
            // An assignment without a recorded location is reported at the declaration.
            let assignments: Vec<usize> = d.statements.iter().map(|a| a.unwrap_or(declaration)).collect();
            let pieces = if d.pieces.is_empty() { vec![] } else { assignments.iter().copied().zip(d.pieces).collect() };
            let extents = shapes.get(&name).map(|s| s.iter().map(|(_, e)| *e).collect()).unwrap_or_default();
            let used = words(&Stmt::Print { items: d.values.clone() });
            let mut later: Vec<String> = used.into_iter().filter(|w| defined_at.get(w).is_some_and(|&p| p > declaration)).collect();
            later.sort();
            defined_at.insert(name.clone(), declaration);
            out.push(Report { name, declaration, assignments, pieces, extents, later });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn procedures(body: &[Stmt], host_constants: &HashSet<String>, host_variables: &HashSet<String>, host_integers: &Integers,
              array_names: &HashSet<String>, signatures: &Signatures, out: &mut Vec<Report>) {
    for s in body {
        let (name, params, inner, result) = match s {
            Stmt::Function { name, params, body, .. } => (name, params, body, match body.last() {
                Some(Stmt::Return(Some(Expr::Ident(n)))) => Some(n.clone()),
                _ => None,
            }),
            Stmt::Subroutine { name, params, body, .. } => (name, params, body, None),
            _ => continue,
        };
        let excluded: HashSet<String> = params.iter().cloned().chain([name.clone()]).chain(result).collect();
        let integers = integers(inner, host_integers);
        unit(inner, &excluded, host_constants, host_variables, &integers, array_names, signatures, out);
        let (constants, variables) = declared(inner);
        let constants: HashSet<String> = host_constants.iter().cloned().chain(constants).collect();
        let variables: HashSet<String> = host_variables.iter().cloned().chain(variables).chain(params.iter().cloned()).collect();
        procedures(inner, &constants, &variables, &integers, array_names, signatures, out);
    }
}

fn analyze(program: &Program) -> Vec<Report> {
    let mut known = Signatures::new();
    signatures(&program.body, &mut known);
    let signatures = known;
    // Every name declared as an array anywhere, so that a right-hand side
    // naming one is not taken for a scalar.
    fn arrays(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::ArrayDecl { name, .. } => { out.insert(name.clone()); }
                Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => arrays(body, out),
                _ => {}
            }
        }
    }
    let mut array_names = HashSet::new();
    arrays(&program.body, &mut array_names);
    // The named constants of every module; a scope that declares a variable
    // of the same name hides it.
    let mut module_constants = HashSet::new();
    let mut module_integers = Integers::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            module_constants.extend(declared(body).0);
            module_integers = integers(body, &module_integers);
        }
    }
    let mut out = vec![];
    // The main program: the program body outside modules and external
    // procedures. Its internal procedures, which `contains` names, may
    // define its variables.
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    let main_integers = integers(&main, &module_integers);
    unit(&main, &HashSet::new(), &module_constants, &HashSet::new(), &main_integers, &array_names, &signatures, &mut out);
    let (constants, variables) = declared(&main);
    let constants: HashSet<String> = module_constants.iter().cloned().chain(constants).collect();
    procedures(&main, &constants, &variables, &main_integers, &array_names, &signatures, &mut out);
    let externals: Vec<Stmt> = program.body.iter().filter(|s| external(s)).cloned().collect();
    procedures(&externals, &module_constants, &HashSet::new(), &module_integers, &array_names, &signatures, &mut out);
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            let (constants, variables) = declared(body);
            let constants: HashSet<String> = module_constants.iter().cloned().chain(constants).collect();
            let integers = integers(body, &module_integers);
            procedures(body, &constants, &variables, &integers, &array_names, &signatures, &mut out);
        }
    }
    out.sort_by_key(|r| (r.declaration, r.assignments.first().copied()));
    out
}

/// The source text of `tokens`.
fn text(src: &str, tokens: &[Token]) -> String {
    match (tokens.first(), tokens.last()) {
        (Some(a), Some(b)) => src[a.span.start..b.span.end].split_whitespace().collect::<Vec<_>>().join(" ").replace("& ", ""),
        _ => String::new(),
    }
}

/// The tokens after the first `=` outside parentheses and brackets, up to
/// a comma outside them.
fn value_tokens(tokens: &[Token]) -> &[Token] {
    let mut depth = 0i32;
    let Some(eq) = tokens.iter().position(|t| {
        match t.kind { TokenKind::LParen | TokenKind::LBracket => depth += 1, TokenKind::RParen | TokenKind::RBracket => depth -= 1, _ => {} }
        depth == 0 && matches!(t.kind, TokenKind::Eq)
    }) else { return &[] };
    let rest = &tokens[eq + 1..];
    depth = 0;
    let end = rest.iter().position(|t| {
        match t.kind { TokenKind::LParen | TokenKind::LBracket => depth += 1, TokenKind::RParen | TokenKind::RBracket => depth -= 1, _ => {} }
        depth == 0 && matches!(t.kind, TokenKind::Comma)
    }).unwrap_or(rest.len());
    &rest[..end]
}

/// Split tokens at commas outside parentheses and brackets.
fn split_items(tokens: &[Token]) -> Vec<&[Token]> {
    let (mut out, mut depth, mut begin) = (vec![], 0i32, 0);
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            TokenKind::Comma if depth == 0 => { out.push(&tokens[begin..i]); begin = i + 1; }
            _ => {}
        }
    }
    out.push(&tokens[begin..]);
    out
}

/// The tokens of each item of an array constructor `[a, b]` or `(/a, b/)`,
/// or None when the value is not a constructor.
fn constructor_items(tokens: &[Token]) -> Option<Vec<&[Token]>> {
    let inner = match tokens {
        [open, inner @ .., close] if matches!(open.kind, TokenKind::LBracket) && matches!(close.kind, TokenKind::RBracket) => inner,
        [open, slash, inner @ .., slash2, close] if matches!(open.kind, TokenKind::LParen) && matches!(slash.kind, TokenKind::Slash)
            && matches!(slash2.kind, TokenKind::Slash) && matches!(close.kind, TokenKind::RParen) => inner,
        _ => return None,
    };
    Some(split_items(inner))
}

/// The value of an array defined in pieces: a constructor in array element
/// order, reshaped for rank two or more, with a continuation line for each
/// column.
fn assembled(src: &str, pieces: &[(&[Token], &Vec<usize>)], extents: &[i64], shape: &str) -> Option<String> {
    let size: i64 = extents.iter().product();
    let mut items: Vec<Option<String>> = vec![None; size as usize];
    for (value, positions) in pieces {
        let texts: Vec<String> = match constructor_items(value) {
            Some(parts) => parts.iter().map(|p| text(src, p)).collect(),
            None => vec![text(src, value); positions.len()],
        };
        if texts.len() != positions.len() { return None; }
        for (&p, t) in positions.iter().zip(texts) { items[p] = Some(t); }
    }
    let items: Vec<String> = items.into_iter().collect::<Option<_>>()?;
    let column = if extents.len() > 1 { extents[0].max(1) as usize } else { 8 };
    let lines: Vec<String> = items.chunks(column).map(|c| c.join(", ")).collect();
    let body = if lines.len() == 1 { lines[0].clone() } else { format!(" &\n    {}", lines.join(", &\n    ")) };
    Some(if extents.len() > 1 { format!("reshape([{body}], [{shape}])") } else { format!("[{body}]") })
}

pub(crate) fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    // Opt-in: -Wall does not enable this advice.
    let mut opt_in = settings.clone();
    opt_in.wall = false;
    let (enabled, error) = crate::unused::selected(CATEGORY, false, &opt_in, options);
    if !enabled { return Ok(()); }
    let mut reports = analyze(program);
    reports.retain(|r| r.declaration < own_len);
    if reports.is_empty() { return Ok(()); }
    let tokens: Vec<Token> = lex(src);
    use codespan_reporting::{diagnostic::{Diagnostic, Label}, files::SimpleFiles,
        term::{emit_to_io_write, termcolor::{ColorChoice, StandardStream}, Config}};
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins.iter().map(|o| files.add(o.name.as_str(), o.source(src))).collect::<Vec<_>>();
    let locate = |span: std::ops::Range<usize>| match origins.iter().enumerate().find(|(_, o)| o.range.contains(&span.start)) {
        Some((i, o)) => (ids[i], o.span(span)),
        None => (fallback, span.start.min(src.len())..span.end.min(src.len())),
    };
    let statement = |offset: usize| crate::declaration_warnings::statement_tokens(&tokens, offset, usize::MAX, src);
    for r in &reports {
        let decl = statement(r.declaration);
        let Some(name_at) = decl.iter().position(|t| matches!(&t.kind, TokenKind::Ident(n) if *n == r.name)).filter(|&i| i > 0) else { continue };
        // The type, without attributes: the text before `::` or the name.
        let type_end = decl.iter().position(|t| matches!(t.kind, TokenKind::DColon)).unwrap_or(name_at);
        let mut depth = 0i32;
        let type_end = decl[..type_end].iter().position(|t| {
            match t.kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
            depth == 0 && matches!(t.kind, TokenKind::Comma)
        }).unwrap_or(type_end);
        let type_text = text(src, &decl[..type_end]);
        // The entity as declared, with its own length such as a*3 or its shape.
        let mut depth = 0i32;
        let entity_end = decl[name_at..].iter().position(|t| {
            match t.kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
            depth == 0 && matches!(t.kind, TokenKind::Comma | TokenKind::Eq | TokenKind::Arrow)
        }).map_or(decl.len(), |i| name_at + i);
        let entity_tokens = &decl[name_at..entity_end];
        let entity = text(src, entity_tokens);
        let assignments: Vec<&[Token]> = r.assignments.iter().filter(|&&a| a != r.declaration).map(|&a| statement(a)).collect();
        if assignments.len() != r.assignments.len() { continue; }
        let value = if !r.pieces.is_empty() {
            // The shape: each declared extent, or its value for explicit bounds.
            let dims = entity_tokens.iter().position(|t| matches!(t.kind, TokenKind::LParen))
                .map(|open| split_items(&entity_tokens[open + 1..entity_tokens.len() - 1])).unwrap_or_default();
            let shape: Vec<String> = dims.iter().zip(&r.extents).map(|(d, e)|
                if d.iter().any(|t| matches!(t.kind, TokenKind::Colon)) { e.to_string() } else { text(src, d) }).collect();
            let pieces: Vec<(&[Token], &Vec<usize>)> = r.pieces.iter().map(|(a, p)| (value_tokens(statement(*a)), p)).collect();
            match assembled(src, &pieces, &r.extents, &shape.join(", ")) { Some(v) => v, None => continue }
        } else {
            match assignments.first() {
                Some(a) => text(src, value_tokens(a)),
                None => text(src, value_tokens(&decl[name_at..])),
            }
        };
        if value.is_empty() { continue; }
        let (file, at) = locate(decl[name_at].span.clone());
        let mut labels = vec![Label::primary(file, at).with_message("declared here")];
        let mut notes = vec![format!("suggestion: {type_text}, parameter :: {entity} = {value}")];
        let others = decl.iter().skip(type_end).filter(|t| matches!(&t.kind, TokenKind::Ident(n) if *n != r.name)).count();
        for statement in &assignments {
            let (file, at) = locate(statement[0].span.start..statement[statement.len() - 1].span.end);
            labels.push(Label::secondary(file, at).with_message(if assignments.len() > 1 { "one of its definitions" } else { "its only definition" }));
        }
        let how = match assignments.len() {
            0 => format!("`{}` is initialized with a constant and never modified", r.name),
            1 if r.pieces.is_empty() => format!("`{}` is assigned once, from a constant, and never modified", r.name),
            _ => format!("each element of `{}` is assigned once, from a constant, and never modified", r.name),
        };
        match assignments.len() {
            0 => {}
            1 => notes.push("and remove the assignment".to_owned()),
            _ => notes.push("and remove the assignments".to_owned()),
        }
        if others > 0 { notes.push(format!("declare `{}` in a separate statement from the other entities of its declaration", r.name)); }
        if !r.later.is_empty() {
            let names: Vec<String> = r.later.iter().map(|n| format!("`{n}`")).collect();
            notes.push(format!("its value uses {}, defined after its declaration; place the declaration after {}",
                names.join(", "), if names.len() > 1 { "them" } else { "it" }));
        }
        notes.push(format!("-Wno-{CATEGORY} disables this warning"));
        let d = if error { Diagnostic::error() } else { Diagnostic::warning() }
            .with_message(format!("{how}; it could be a named constant [{CATEGORY}]"))
            .with_labels(labels)
            .with_notes(notes);
        emit_to_io_write(&mut StandardStream::stderr(ColorChoice::Auto), &Config::default(), &files, &d)?;
    }
    if error { bail!("{CATEGORY} warnings treated as errors"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn names(src: &str) -> Vec<String> {
        let program = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "t.f90").unwrap_or_else(|_| panic!("{src}"));
        let mut out: Vec<String> = super::analyze(&program).into_iter().map(|r| r.name).collect();
        out.sort();
        out
    }

    #[test]
    fn xparam_example() {
        // From xparam.f90: x2 depends on x1; x3 is random; x4 is allocatable.
        let src = "program xparam\nimplicit none\ninteger :: n\nreal :: x1, x2, x3\nreal, allocatable :: x4(:)\ncharacter (len=10) :: msg\nx1 = 3.1\nx2 = 2*x1\ncall random_number(x3)\nn = 5\nallocate (x4(n))\nmsg = \"hello\"\nend program xparam\n";
        assert_eq!(names(src), ["msg", "n", "x1", "x2"]);
    }

    #[test]
    fn definitions_that_disqualify() {
        for src in [
            // Assigned twice, in a loop, conditionally, by READ, as a DO variable.
            "real :: x\nx = 1\nx = 2\nprint *, x\nend\n",
            "real :: x\ninteger :: i\ndo i = 1, 3\nx = 2.1\nend do\nend\n",
            "real :: x\nlogical :: c\nc = .true.\nif (c) x = 1\nprint *, x\nend\n",
            "real :: x\nx = 1\nread *, x\nend\n",
            "integer :: i\ni = 1\ndo i = 1, 3\nend do\nend\n",
            // Read before it is set, and passed to a procedure of unknown intent.
            "real :: x, y\ny = x\nx = 1\nprint *, y\nend\n",
            "real :: x\nx = 1\ncall s(x)\nend\n",
            // Changed by an internal procedure, or written with IOSTAT=.
            "real :: x\nx = 1\ncall reset()\ncontains\nsubroutine reset()\nx = 0\nend subroutine\nend\n",
            "integer :: k\nk = 0\nwrite (*, *, iostat=k) 1\nend\n",
            // An implied-DO index.
            "integer :: i
i = 1
print *, [(2*i, i=1,3)]
end
",
            // Not a constant expression.
            "real :: x, y\ncall random_number(y)\nx = 2*y\nprint *, x\nend\n",
        ] {
            assert!(!names(src).contains(&"x".to_owned()) && !names(src).contains(&"i".to_owned()) && !names(src).contains(&"k".to_owned()), "{src}: {:?}", names(src));
        }
    }

    #[test]
    fn arrays() {
        // Defined by sections and elements covering every element once, by a
        // whole-array assignment, and by an initialization.
        let src = "program p
integer, parameter :: nb = 2
real(8) :: q(2,nb), v(3), w(2), z(2,2)
integer :: s(2) = [1, 2], k
q(:,1) = [1d0, 2d0]
q(1:2,2) = 3d0
v(1) = 1
v(2:3) = [2, 3]
w = [5, 6]
z(:,1) = 1
k = 1
z(:,2) = 2
z(1,1) = 3
print *, q, v, w, s, z, k
end program
";
        assert_eq!(names(src), ["k", "q", "s", "v", "w"]);
        assert_eq!(names("integer :: x(2) = 7
print *, x
end
"), ["x"]);
        for src in [
            // An element left undefined, an element defined twice, a run-time subscript, a later change.
            "real :: a(3)
a(1:2) = 1
print *, a
end
",
            "real :: a(2)
a(:) = 1
a(2) = 2
print *, a
end
",
            "real :: a(2)
integer :: i
i = 1
a(i) = 1
a(2) = 2
print *, a
end
",
            "real :: a(2)
integer :: i
a = [1, 2]
do i = 1, 2
a(i) = 0
end do
end
",
            // A value that is an array not known to be constant.
            "real :: a(2), b(2)
call random_number(b)
a = b
print *, a
end
",
        ] {
            assert!(!names(src).contains(&"a".to_owned()), "{src}: {:?}", names(src));
        }
    }

    #[test]
    fn section_positions() {
        let integers = super::Integers::new();
        let section = |l: Option<i64>, u: Option<i64>| crate::ast::Expr::Call("@section_subscript".into(), vec![crate::ast::Expr::Section {
            name: "p".into(), lower: l.map(|v| Box::new(crate::ast::Expr::IntLit(v.to_string()))),
            upper: u.map(|v| Box::new(crate::ast::Expr::IntLit(v.to_string()))), stride: None }]);
        let int = |v: i64| crate::ast::Expr::IntLit(v.to_string());
        // p(4,5): column 2 is elements 4..7; row 3 is 2, 6, 10, 14, 18.
        let shape = [(1, 4), (1, 5)];
        assert_eq!(super::positions(&[section(None, None), int(2)], &shape, &integers), Some(vec![4, 5, 6, 7]));
        assert_eq!(super::positions(&[int(3), section(None, None)], &shape, &integers), Some(vec![2, 6, 10, 14, 18]));
        assert_eq!(super::positions(&[section(Some(2), Some(3)), section(Some(4), Some(5))], &shape, &integers), Some(vec![13, 14, 17, 18]));
        assert_eq!(super::positions(&[int(5), int(1)], &shape, &integers), None);
    }

    #[test]
    fn accepted_forms() {
        // Initialized in the declaration, an INTENT(IN) argument, a module constant,
        // and an internal procedure that only reads it.
        let src = "module m\ninteger, parameter :: dp = kind(1d0)\ncontains\nsubroutine show(a)\nreal(dp), intent(in) :: a\nprint *, a\nend subroutine\nend module\nprogram p\nuse m\nreal(dp) :: pi = 3.141592653589793_dp, two\ntwo = 2*pi\ncall show(two)\ncall report()\ncontains\nsubroutine report()\nprint *, pi\nend subroutine\nend program\n";
        assert_eq!(names(src), ["pi", "two"]);
        // A procedure's local, but not its dummy or result.
        let src = "real function area(r) result(a)\nreal, intent(in) :: r\nreal :: pi\npi = acos(-1.0)\na = pi*r**2\nend function\n";
        assert_eq!(names(src), ["pi"]);
    }
}

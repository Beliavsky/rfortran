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
        Expr::Call(f, args) if f.starts_with("@keyword:") || CONSTANT_INTRINSICS.contains(&f.as_str()) && !constants.contains(f) =>
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

struct Report {
    name: String,
    declaration: usize,
    /// The defining assignment, when the variable is not initialized in its declaration.
    assignment: Option<usize>,
}

/// One main program or procedure: its statements (internal procedures
/// included), dummy arguments and result names, and the named constants and
/// variables of its hosts.
fn unit(body: &[Stmt], excluded: &HashSet<String>, host_constants: &HashSet<String>, host_variables: &HashSet<String>,
        signatures: &Signatures, out: &mut Vec<Report>) {
    let (own_constants, own_variables) = declared(body);
    let mut constants: HashSet<String> = host_constants.difference(&own_variables).cloned().collect();
    constants.extend(own_constants);
    for v in host_variables { if !own_variables.contains(v) { constants.remove(v); } }
    // Candidates: scalar locals of intrinsic type without attributes.
    let arrays: HashSet<&String> = body.iter().filter_map(|s| match s { Stmt::ArrayDecl { name, .. } => Some(name), _ => None }).collect();
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
            Stmt::InitializedDecl { kind, attributes, entities } if supported(kind) && attributes.iter().all(|a| a.starts_with('@')) => for (n, v) in entities {
                candidates.insert(n.clone());
                decl_at.entry(n.clone()).or_insert(at);
                if let Some(v) = v { initial.insert(n.clone(), v.clone()); }
            },
            _ => {}
        }
    }
    candidates.retain(|n| !arrays.contains(n) && !attributed.contains(n) && !excluded.contains(n));
    initial.retain(|n, _| candidates.contains(n));
    if candidates.is_empty() { return; }
    let mut scan = Scan { candidates: &candidates, signatures, definitions: HashMap::new() };
    scan.statements(body);
    let definitions = scan.definitions;
    // The defining assignment of each candidate without an initialization:
    // a top-level statement before which the candidate is not mentioned.
    // Each value is kept with its assignment's location, or None for an
    // initialization.
    let mut values: HashMap<String, (Expr, Option<Option<usize>>)> = HashMap::new();
    let mut mentioned: HashSet<String> = HashSet::new();
    let mut at = None;
    for s in body {
        match s {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::Assign { name, value } if candidates.contains(name) && !initial.contains_key(name) && !mentioned.contains(name) => {
                values.entry(name.clone()).or_insert((value.clone(), Some(at)));
                mentioned.extend(words(&Stmt::Print { items: vec![value.clone()] }));
                mentioned.insert(name.clone());
            }
            Stmt::Function { .. } | Stmt::Subroutine { .. } => {}
            _ if declaration(s) => {}
            _ => mentioned.extend(words(s)),
        }
    }
    for (n, v) in &initial {
        if !definitions.contains_key(n) { values.insert(n.clone(), (v.clone(), None)); }
    }
    // An initialized variable has no other definition; an assigned one has
    // only that assignment.
    values.retain(|n, (_, assignment)| definitions.get(n).copied().unwrap_or(0) == usize::from(assignment.is_some()));
    // Accept values that are constant, given the constants accepted so far.
    loop {
        let ready: Vec<String> = values.iter().filter(|(_, (v, _))| constant(v, &constants)).map(|(n, _)| n.clone()).collect();
        if ready.is_empty() { break; }
        for name in ready {
            let (_, assignment) = values.remove(&name).unwrap();
            constants.insert(name.clone());
            let Some(Some(declaration)) = decl_at.get(&name).copied() else { continue };
            // An assignment without a recorded location is reported at the declaration.
            let assignment = assignment.map(|a| a.unwrap_or(declaration));
            out.push(Report { name, declaration, assignment });
        }
    }
}

fn procedures(body: &[Stmt], host_constants: &HashSet<String>, host_variables: &HashSet<String>, signatures: &Signatures, out: &mut Vec<Report>) {
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
        unit(inner, &excluded, host_constants, host_variables, signatures, out);
        let (constants, variables) = declared(inner);
        let constants: HashSet<String> = host_constants.iter().cloned().chain(constants).collect();
        let variables: HashSet<String> = host_variables.iter().cloned().chain(variables).chain(params.iter().cloned()).collect();
        procedures(inner, &constants, &variables, signatures, out);
    }
}

fn analyze(program: &Program) -> Vec<Report> {
    let mut known = Signatures::new();
    signatures(&program.body, &mut known);
    let signatures = known;
    // The named constants of every module; a scope that declares a variable
    // of the same name hides it.
    let mut module_constants = HashSet::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s { module_constants.extend(declared(body).0); }
    }
    let mut out = vec![];
    // The main program: the program body outside modules and external
    // procedures. Its internal procedures, which `contains` names, may
    // define its variables.
    let external = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if !program.contains.contains(name));
    let main: Vec<Stmt> = program.body.iter().filter(|s| !matches!(s, Stmt::Module { .. }) && !external(s)).cloned().collect();
    unit(&main, &HashSet::new(), &module_constants, &HashSet::new(), &signatures, &mut out);
    let (constants, variables) = declared(&main);
    let constants: HashSet<String> = module_constants.iter().cloned().chain(constants).collect();
    procedures(&main, &constants, &variables, &signatures, &mut out);
    let externals: Vec<Stmt> = program.body.iter().filter(|s| external(s)).cloned().collect();
    procedures(&externals, &module_constants, &HashSet::new(), &signatures, &mut out);
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            let (constants, variables) = declared(body);
            let constants: HashSet<String> = module_constants.iter().cloned().chain(constants).collect();
            procedures(body, &constants, &variables, &signatures, &mut out);
        }
    }
    out.sort_by_key(|r| (r.declaration, r.assignment));
    out
}

/// The source text of `tokens`.
fn text(src: &str, tokens: &[Token]) -> String {
    match (tokens.first(), tokens.last()) {
        (Some(a), Some(b)) => src[a.span.start..b.span.end].split_whitespace().collect::<Vec<_>>().join(" ").replace("& ", ""),
        _ => String::new(),
    }
}

/// The tokens after the first `=` at parenthesis depth zero, up to a comma
/// at depth zero.
fn value_tokens(tokens: &[Token]) -> &[Token] {
    let mut depth = 0i32;
    let Some(eq) = tokens.iter().position(|t| {
        match t.kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
        depth == 0 && matches!(t.kind, TokenKind::Eq)
    }) else { return &[] };
    let rest = &tokens[eq + 1..];
    depth = 0;
    let end = rest.iter().position(|t| {
        match t.kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
        depth == 0 && matches!(t.kind, TokenKind::Comma)
    }).unwrap_or(rest.len());
    &rest[..end]
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
        let (value, assignment) = match r.assignment {
            Some(a) if a != r.declaration => (text(src, value_tokens(statement(a))), Some(statement(a))),
            _ => (text(src, value_tokens(&decl[name_at..])), None),
        };
        if value.is_empty() { continue; }
        let (file, at) = locate(decl[name_at].span.clone());
        let mut labels = vec![Label::primary(file, at).with_message("declared here")];
        // The entity as declared, with its own length such as a*3.
        let mut depth = 0i32;
        let entity_end = decl[name_at..].iter().position(|t| {
            match t.kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
            depth == 0 && matches!(t.kind, TokenKind::Comma | TokenKind::Eq | TokenKind::Arrow)
        }).map_or(decl.len(), |i| name_at + i);
        let entity = text(src, &decl[name_at..entity_end]);
        let mut notes = vec![format!("suggestion: {type_text}, parameter :: {entity} = {value}")];
        let others = decl.iter().skip(type_end).filter(|t| matches!(&t.kind, TokenKind::Ident(n) if *n != r.name)).count();
        let how = match assignment {
            Some(statement) => {
                let (file, at) = locate(statement[0].span.start..statement[statement.len() - 1].span.end);
                labels.push(Label::secondary(file, at).with_message("its only definition"));
                format!("`{}` is assigned once, from a constant, and never modified", r.name)
            }
            None => format!("`{}` is initialized with a constant and never modified", r.name),
        };
        if assignment.is_some() { notes.push("and remove the assignment".to_owned()); }
        if others > 0 { notes.push(format!("declare `{}` in a separate statement from the other entities of its declaration", r.name)); }
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
            // An implied-DO index, and an array initialized in its declaration.
            "integer :: i
i = 1
print *, [(2*i, i=1,3)]
end
",
            "integer :: x(2) = 7
print *, x
end
",
            // Not a constant expression.
            "real :: x, y\ncall random_number(y)\nx = 2*y\nprint *, x\nend\n",
        ] {
            assert!(!names(src).contains(&"x".to_owned()) && !names(src).contains(&"i".to_owned()) && !names(src).contains(&"k".to_owned()), "{src}: {:?}", names(src));
        }
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

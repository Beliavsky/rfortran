// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Partial revision checks on the source AST and original token spelling.
//! Keep this ahead of lowering, including for definitions loaded from .rmod.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Standard(u16);
impl std::str::FromStr for Standard {
    type Err = String;
    fn from_str(text: &str) -> std::result::Result<Self, Self::Err> {
        match text {
            "f90" => Ok(Self(1990)),
            "f95" => Ok(Self(1995)),
            "f2003" => Ok(Self(2003)),
            "f2008" => Ok(Self(2008)),
            "f2018" => Ok(Self(2018)),
            "f2023" => Ok(Self(2023)),
            _ => Err(format!("unknown Fortran standard '{text}'")),
        }
    }
}
impl fmt::Display for Standard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fortran {}",
            if self.0 == 1990 {
                "90".into()
            } else if self.0 == 1995 {
                "95".into()
            } else {
                self.0.to_string()
            }
        )
    }
}

struct Feature {
    key: &'static str,
    name: &'static str,
    first: u16,
    obsolete: Option<u16>,
    deleted: Option<u16>,
}
macro_rules! feature {
    ($key:literal, $name:literal, $year:literal) => {
        Feature {
            key: $key,
            name: $name,
            first: $year,
            obsolete: None,
            deleted: None,
        }
    };
}
// Central metadata for recognized features. Add revision checks here rather
// than independent year comparisons scattered through backend lowering.
const FEATURES: &[Feature] = &[
    feature!("pure", "PURE procedures", 1995),
    feature!("simple", "SIMPLE procedures", 2023),
    feature!("elemental", "ELEMENTAL procedures", 1995),
    Feature {
        key: "forall",
        name: "FORALL",
        first: 1995,
        obsolete: Some(2018),
        deleted: None,
    },
    feature!(
        "component_init",
        "derived-type component default initialization",
        1995
    ),
    feature!("brackets", "square-bracket array constructors", 2003),
    feature!("typed_constructor", "typed array constructors", 2003),
    feature!("associate", "ASSOCIATE constructs", 2003),
    feature!("polymorphic", "polymorphic declarations", 2003),
    feature!("type_bound", "type-bound procedures", 2003),
    feature!(
        "procedure",
        "PROCEDURE declarations and abstract interfaces",
        2003
    ),
    feature!(
        "intrinsic_module",
        "intrinsic modules and USE module nature",
        2003
    ),
    feature!("deferred_character", "deferred CHARACTER length", 2003),
    feature!(
        "allocatable_dummy",
        "ALLOCATABLE dummy arguments and function results",
        2003
    ),
    feature!("allocatable_scalar", "ALLOCATABLE scalars", 2003),
    feature!(
        "allocatable_component",
        "ALLOCATABLE derived-type components",
        2003
    ),
    feature!(
        "attribute",
        "VALUE, VOLATILE, ASYNCHRONOUS or PROTECTED attributes",
        2003
    ),
    feature!("iomsg", "IOMSG specifiers", 2003),
    feature!("stream", "stream file access", 2003),
    feature!("flush", "FLUSH statements", 2003),
    feature!("block", "BLOCK constructs", 2008),
    feature!("concurrent", "DO CONCURRENT", 2008),
    feature!("impure", "IMPURE procedure prefixes", 2008),
    feature!("contiguous", "CONTIGUOUS attributes", 2008),
    feature!("newunit", "NEWUNIT specifiers", 2008),
    feature!("rank", "arrays of rank greater than seven", 2008),
    feature!("error_stop", "ERROR STOP", 2008),
    feature!("pure_error_stop", "ERROR STOP in a PURE procedure", 2018),
];
fn intrinsic_year(name: &str) -> Option<u16> {
    Some(match name {
        "cpu_time" | "null" => 1995,
        "command_argument_count"
        | "get_command"
        | "get_command_argument"
        | "get_environment_variable"
        | "move_alloc"
        | "new_line"
        | "selected_char_kind"
        | "is_iostat_end"
        | "is_iostat_eor" => 2003,
        "acosh"
        | "asinh"
        | "atanh"
        | "bessel_j0"
        | "bessel_j1"
        | "bessel_jn"
        | "bessel_y0"
        | "bessel_y1"
        | "bessel_yn"
        | "erf"
        | "erfc"
        | "erfc_scaled"
        | "gamma"
        | "log_gamma"
        | "hypot"
        | "norm2"
        | "findloc"
        | "parity"
        | "storage_size"
        | "execute_command_line"
        | "compiler_version"
        | "compiler_options"
        | "bge"
        | "bgt"
        | "ble"
        | "blt"
        | "dshiftl"
        | "dshiftr"
        | "iall"
        | "iany"
        | "iparity"
        | "leadz"
        | "trailz"
        | "maskl"
        | "maskr"
        | "merge_bits"
        | "popcnt"
        | "poppar"
        | "shifta"
        | "shiftl"
        | "shiftr" => 2008,
        "random_init" | "rank" | "out_of_range" => 2018,
        "acospi"
        | "asinpi"
        | "atanpi"
        | "cospi"
        | "sinpi"
        | "tanpi"
        | "acosd"
        | "asind"
        | "atand"
        | "cosd"
        | "sind"
        | "tand"
        | "selected_logical_kind"
        | "split"
        | "tokenize"
        | "reduce" => 2023,
        _ => return None,
    })
}

struct Issue {
    offset: usize,
    message: String,
    warning: bool,
}
struct Checker<'a> {
    standard: Standard,
    source: &'a str,
    tokens: &'a [Token],
    issues: Vec<Issue>,
    headers: HashSet<usize>,
    modules: HashMap<String, HashSet<String>>,
    env_names: HashSet<String>,
    module_env: HashMap<String, HashSet<String>>,
}
impl Checker<'_> {
    fn env_imports(&self, body: &[Stmt], inherited: &HashSet<String>) -> HashSet<String> {
        let mut names = inherited.clone();
        for stmt in body {
            match stmt {
                Stmt::VarDecl { names: local, .. } | Stmt::DerivedDecl { names: local, .. } => {
                    for n in local {
                        names.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. }
                | Stmt::Function { name, .. }
                | Stmt::Subroutine { name, .. } => {
                    names.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        names.remove(n);
                    }
                }
                _ => {}
            }
        }
        for stmt in body {
            if let Stmt::Use {
                module,
                only,
                renames,
                nature,
            } = stmt
            {
                let intrinsic = module == "iso_fortran_env"
                    && (nature.as_deref() == Some("intrinsic")
                        || !self.modules.contains_key(module));
                let exports = if intrinsic {
                    ["logical8", "logical16", "logical32", "logical64", "real16"]
                        .map(String::from)
                        .into_iter()
                        .collect()
                } else {
                    self.module_env.get(module).cloned().unwrap_or_default()
                };
                let selected = only
                    .clone()
                    .unwrap_or_else(|| exports.iter().cloned().collect());
                for remote in selected {
                    if exports.contains(&remote) {
                        names.insert(
                            renames
                                .iter()
                                .find(|(_, r)| *r == remote)
                                .map_or(remote.clone(), |(l, _)| l.clone()),
                        );
                    }
                }
                for (local, remote) in renames {
                    if exports.contains(remote) {
                        names.insert(local.clone());
                    }
                }
            }
        }
        names
    }
    fn format(&mut self, expr: &Expr, offset: usize) {
        if let Expr::Str(text) = expr {
            if crate::format_spec::contains_at(text.as_bytes()) {
                self.require("AT edit descriptor", 2023, offset);
            }
        }
    }
    fn feature(&mut self, key: &str, offset: usize) {
        let f = FEATURES
            .iter()
            .find(|f| f.key == key)
            .expect("known standard feature");
        self.require(f.name, f.first, offset);
        if f.deleted.is_some_and(|year| self.standard.0 >= year) {
            self.issues.push(Issue {
                offset,
                warning: false,
                message: format!(
                    "{} was deleted in {}; selected standard is {}",
                    f.name,
                    Standard(f.deleted.unwrap()),
                    self.standard
                ),
            });
        } else if f.obsolete.is_some_and(|year| self.standard.0 >= year) {
            self.issues.push(Issue {
                offset,
                warning: true,
                message: format!("{} is obsolescent in {} [-std]", f.name, self.standard),
            });
        }
    }
    fn require(&mut self, name: &str, first: u16, offset: usize) {
        if self.standard.0 < first {
            self.issues.push(Issue {
                offset,
                warning: false,
                message: format!(
                    "{name} requires {}; selected standard is {}",
                    Standard(first),
                    self.standard
                ),
            });
        }
    }
    fn header(&mut self, offset: usize) {
        if !self.headers.insert(offset) {
            return;
        }
        let start = self.tokens.partition_point(|t| t.span.start < offset);
        let mut end = start;
        while end < self.tokens.len()
            && (end == start
                || crate::lexer::same_statement(
                    &self.source[self.tokens[end - 1].span.end..self.tokens[end].span.start],
                ))
        {
            end += 1;
        }
        let tokens = &self.tokens[start..end];
        let io = tokens.iter().any(|t| {
            matches!(
                t.kind,
                TokenKind::FileControl(_)
                    | TokenKind::KwWrite
                    | TokenKind::KwRead
                    | TokenKind::KwReadEmpty
                    | TokenKind::KwWriteEmpty
            )
        });
        for (index, t) in tokens.iter().enumerate() {
            let key = match &t.kind {
                TokenKind::FormatSpec(text) if crate::format_spec::contains_at(text.as_bytes()) => {
                    self.require("AT edit descriptor", 2023, t.span.start);
                    None
                }
                TokenKind::KwPure => Some("pure"),
                TokenKind::KwSimple => Some("simple"),
                TokenKind::KwElemental => Some("elemental"),
                TokenKind::KwImpure => Some("impure"),
                TokenKind::KwForall => Some("forall"),
                TokenKind::KwAssociate => Some("associate"),
                TokenKind::KwClass => Some("polymorphic"),
                TokenKind::KwBlock => Some("block"),
                TokenKind::KwError => Some("error_stop"),
                TokenKind::LBracket if self.source[t.span.clone()].starts_with('[') => {
                    Some("brackets")
                }
                TokenKind::ProcedureDeclaration(_)
                    if {
                        let text = self.source[t.span.clone()].to_ascii_lowercase();
                        text.starts_with("procedure") || text.starts_with("abstract")
                    } =>
                {
                    Some("procedure")
                }
                TokenKind::FilePosition(n) | TokenKind::FileControl(n) if n == "flush" => {
                    Some("flush")
                }
                TokenKind::Ident(n)
                    if n == "concurrent"
                        && index > 0
                        && matches!(tokens[index - 1].kind, TokenKind::KwDo) =>
                {
                    Some("concurrent")
                }
                TokenKind::Ident(n)
                    if io
                        && matches!(
                            tokens.get(index + 1).map(|t| &t.kind),
                            Some(TokenKind::Eq)
                        ) =>
                {
                    match n.as_str() {
                        "newunit" => Some("newunit"),
                        "iomsg" => Some("iomsg"),
                        "access" if matches!(tokens.get(index+2).map(|t|&t.kind),Some(TokenKind::Str(s)) if s.eq_ignore_ascii_case("stream")) => {
                            Some("stream")
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(key) = key {
                self.feature(key, t.span.start);
            }
        }
    }
    fn typ(&mut self, kind: &TypeSpec, offset: usize) {
        if let TypeSpec::LogicalKind(name)
        | TypeSpec::RealKind(name)
        | TypeSpec::IntegerKind(name) = kind
        {
            if self.env_names.contains(name) {
                self.require("Fortran 2023 ISO_FORTRAN_ENV kind constants", 2023, offset);
            }
            if let Some(encoded) = name.strip_prefix(crate::ast::KIND_EXPRESSION) {
                if let Ok(expr) = serde_json::from_str::<Expr>(encoded) {
                    self.expr(&expr, offset, &HashSet::new());
                }
            }
        }
        if crate::substrings::deferred(kind) {
            self.feature("deferred_character", offset);
        }
    }
    fn expr(&mut self, e: &Expr, offset: usize, names: &HashSet<String>) {
        match e {
            Expr::Ident(n) if self.env_names.contains(n) => {
                self.require("Fortran 2023 ISO_FORTRAN_ENV kind constants", 2023, offset)
            }
            Expr::Call(n, args) => {
                if n=="@conditional" {self.require("conditional expressions and arguments",2023,offset);}
                if matches!(
                    n.as_str(),
                    "@typed_constructor" | "@typed_character_constructor"
                ) {
                    self.feature("typed_constructor", offset);
                }
                if !names.contains(n) {
                    if let Some(year) = intrinsic_year(n) {
                        self.require(&format!("intrinsic '{n}'"), year, offset);
                    }
                }
                for a in args {
                    self.expr(a, offset, names);
                }
            }
            Expr::Index(_, args) => {
                for a in args {
                    self.expr(a, offset, names);
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, offset, names);
                self.expr(b, offset, names);
            }
            Expr::Un(_, a) => self.expr(a, offset, names),
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expr(a, offset, names);
                }
            }
            _ => {}
        }
    }
    fn scope_names(&self, body: &[Stmt], inherited: &HashSet<String>) -> HashSet<String> {
        let mut names = inherited.clone();
        for s in body {
            match s {
                Stmt::VarDecl { names: local, .. } | Stmt::DerivedDecl { names: local, .. } => {
                    names.extend(local.iter().cloned())
                }
                Stmt::InitializedDecl { entities, .. } => {
                    names.extend(entities.iter().map(|(n, _)| n.clone()))
                }
                Stmt::ArrayDecl { name, .. }
                | Stmt::Function { name, .. }
                | Stmt::Subroutine { name, .. }
                | Stmt::GenericInterface { name, .. } => {
                    names.insert(name.clone());
                }
                Stmt::DummyIntent { name, intent } if intent == "external" => {
                    names.insert(name.clone());
                }
                Stmt::Use {
                    module,
                    only,
                    renames,
                    ..
                } => {
                    if let Some(exports) = self.modules.get(module) {
                        let renamed = renames
                            .iter()
                            .map(|(local, remote)| (remote, local))
                            .collect::<HashMap<_, _>>();
                        let selected = only
                            .clone()
                            .unwrap_or_else(|| exports.iter().cloned().collect());
                        for remote in &selected {
                            if exports.contains(remote) {
                                names
                                    .insert(renamed.get(remote).copied().unwrap_or(remote).clone());
                            }
                        }
                        names.extend(renames.iter().map(|(local, _)| local.clone()));
                    }
                }
                _ => {}
            }
        }
        names
    }
    fn body(
        &mut self,
        body: &[Stmt],
        inherited: &HashSet<String>,
        mut offset: usize,
        pure: bool,
        component: bool,
        dummies: &HashSet<String>,
    ) {
        let empty_env = HashSet::new();
        let scope_env = self.env_imports(
            body,
            if inherited.is_empty() {
                &empty_env
            } else {
                &self.env_names
            },
        );
        let previous_env = std::mem::replace(&mut self.env_names, scope_env);
        let names = self.scope_names(body, inherited);
        let arrays = body
            .iter()
            .filter_map(|s| {
                if let Stmt::ArrayDecl { name, .. } = s {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>();
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = *p;
                self.header(offset);
                continue;
            }
            match s {
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    let procedure_offset = body
                        .iter()
                        .find_map(crate::source_diagnostics::statement_offset)
                        .unwrap_or(offset);
                    let is_pure = matches!(
                        s,
                        Stmt::Function { pure: true, .. } | Stmt::Subroutine { pure: true, .. }
                    );
                    if crate::simple::is_simple(body) { self.feature("simple", procedure_offset); }
                    if is_pure {
                        self.feature("pure", procedure_offset);
                    }
                    if matches!(
                        s,
                        Stmt::Function {
                            elemental: true,
                            ..
                        } | Stmt::Subroutine {
                            elemental: true,
                            ..
                        }
                    ) {
                        self.feature("elemental", procedure_offset);
                    }
                    if let Stmt::Function {
                        return_type: Some(kind),
                        ..
                    } = s
                    {
                        self.typ(kind, procedure_offset);
                    }
                    let mut arguments = params.iter().cloned().collect::<HashSet<_>>();
                    if matches!(s, Stmt::Function { .. }) {
                        for stmt in body {
                            if let Stmt::Return(Some(Expr::Ident(result))) = stmt {
                                arguments.insert(result.clone());
                            }
                        }
                    }
                    self.body(body, &names, procedure_offset, is_pure, false, &arguments);
                }
                Stmt::DerivedType {
                    components,
                    bindings,
                    ..
                } => {
                    if !bindings.is_empty() {
                        self.feature("type_bound", offset);
                    }
                    self.body(components, &names, offset, pure, true, &HashSet::new());
                }
                Stmt::Module { body, .. } => {
                    self.body(body, &HashSet::new(), offset, false, false, &HashSet::new())
                }
                Stmt::Use {
                    module,
                    nature,
                    only,
                    renames,
                } if nature.is_some()
                    || !self.modules.contains_key(module)
                        && matches!(
                            module.as_str(),
                            "iso_fortran_env"
                                | "iso_c_binding"
                                | "ieee_arithmetic"
                                | "ieee_exceptions"
                                | "ieee_features"
                        ) =>
                {
                    self.feature("intrinsic_module", offset);
                    if module == "iso_fortran_env"
                        && (nature.as_deref() == Some("intrinsic")
                            || !self.modules.contains_key(module))
                        && only
                            .iter()
                            .flatten()
                            .chain(renames.iter().map(|(_, r)| r))
                            .any(|n| {
                                matches!(
                                    n.as_str(),
                                    "logical8" | "logical16" | "logical32" | "logical64" | "real16"
                                )
                            })
                    {
                        self.require("Fortran 2023 ISO_FORTRAN_ENV kind constants", 2023, offset);
                    }
                }
                Stmt::DerivedDecl {
                    names: declared,
                    polymorphic,
                    attributes,
                    dims,
                    ..
                } => {
                    if *polymorphic {
                        self.feature("polymorphic", offset);
                    }
                    if attributes.iter().any(|a| a == "allocatable")
                        && (component
                            || dims.is_empty()
                            || declared.iter().any(|n| dummies.contains(n)))
                    {
                        self.feature(
                            if component {
                                "allocatable_component"
                            } else if declared.iter().any(|n| dummies.contains(n)) {
                                "allocatable_dummy"
                            } else {
                                "allocatable_scalar"
                            },
                            offset,
                        );
                    }
                    if dims.len() > 7 {
                        self.feature("rank", offset);
                    }
                }
                Stmt::DummyIntent { name, intent } => match intent.as_str() {
                    "@simple_interface" => self.feature("simple", offset),
                    "allocatable" if component => self.feature("allocatable_component", offset),
                    "allocatable" if dummies.contains(name) => {
                        self.feature("allocatable_dummy", offset)
                    }
                    "allocatable" if !arrays.contains(name) => {
                        self.feature("allocatable_scalar", offset)
                    }
                    "value" | "volatile" | "asynchronous" | "protected" => {
                        self.feature("attribute", offset)
                    }
                    "contiguous" => self.feature("contiguous", offset),
                    _ => {}
                },
                Stmt::VarDecl { kind, .. } => self.typ(kind, offset),
                Stmt::ArrayDecl { kind, dims, .. } => {
                    self.typ(kind, offset);
                    if dims.len() > 7 {
                        self.feature("rank", offset);
                    }
                    for d in dims {
                        self.expr(d, offset, &names);
                    }
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    self.typ(kind, offset);
                    if component && entities.iter().any(|(_, e)| e.is_some()) {
                        self.feature("component_init", offset);
                    }
                    for (_, e) in entities {
                        if let Some(e) = e {
                            self.expr(e, offset, &names);
                        }
                    }
                }
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    self.expr(value, offset, &names)
                }
                Stmt::AssignIndex { indices, value, .. } => {
                    for e in indices {
                        self.expr(e, offset, &names);
                    }
                    self.expr(value, offset, &names);
                }
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        self.expr(e, offset, &names);
                    }
                }
                Stmt::CallSub { name, args } => {
                    let format = match name.as_str() {
                        "@formatted_print" | "@formatted_write_noadvance" => args.first(),
                        "@write" => args.get(1),
                        _ => None,
                    };
                    if let Some(format) = format {
                        self.format(format, offset);
                    }
                    if name == "@error_stop" {
                        self.feature(
                            if pure {
                                "pure_error_stop"
                            } else {
                                "error_stop"
                            },
                            offset,
                        );
                    }
                    if !names.contains(name) {
                        if let Some(year) = intrinsic_year(name) {
                            self.require(&format!("intrinsic '{name}'"), year, offset);
                        }
                    }
                    for e in args {
                        self.expr(e, offset, &names);
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(cond, offset, &names);
                    self.body(then_body, &names, offset, pure, component, dummies);
                    if let Some(body) = else_body {
                        self.body(body, &names, offset, pure, component, dummies);
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expr(start, offset, &names);
                    self.expr(end, offset, &names);
                    self.body(body, &names, offset, pure, component, dummies);
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(cond, offset, &names);
                    self.body(body, &names, offset, pure, component, dummies);
                }
                Stmt::Block { body } => self.body(body, &names, offset, pure, component, dummies),
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(expr, offset, &names);
                    for case in cases {
                        self.body(&case.body, &names, offset, pure, component, dummies);
                    }
                    if let Some(body) = default {
                        self.body(body, &names, offset, pure, component, dummies);
                    }
                }
                _ => {}
            }
        }
        self.env_names = previous_env;
    }
}

pub(crate) fn check(
    program: &Program,
    source: &str,
    tokens: &[Token],
    filename: &str,
    origins: Option<&[crate::source_diagnostics::SourceOrigin]>,
    standard: Option<Standard>,
    werror: bool,
) -> Result<()> {
    let Some(standard) = standard else {
        return Ok(());
    };
    let mut checker = Checker {
        standard,
        source,
        tokens,
        issues: vec![],
        headers: HashSet::new(),
        modules: HashMap::new(),
        env_names: HashSet::new(),
        module_env: HashMap::new(),
    };
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let names = checker.scope_names(body, &HashSet::new());
            checker.modules.insert(name.clone(), names);
        }
    }
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let names = checker.env_imports(body, &HashSet::new());
            checker.module_env.insert(name.clone(), names);
        }
    }
    checker.body(
        &program.body,
        &HashSet::new(),
        0,
        false,
        false,
        &HashSet::new(),
    );
    checker
        .issues
        .sort_by(|a, b| (a.offset, &a.message).cmp(&(b.offset, &b.message)));
    checker
        .issues
        .dedup_by(|a, b| a.offset == b.offset && a.message == b.message);
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFile,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let mut failed = false;
    for issue in checker.issues {
        let offset = issue.offset.min(source.len());
        let end = tokens
            .iter()
            .find(|t| t.span.start >= offset)
            .map_or(offset, |t| t.span.end);
        let (name, text, span) = if let Some(origin) =
            origins.and_then(|o| o.iter().find(|o| o.range.contains(&offset)))
        {
            (
                origin.name.as_str(),
                &source[origin.range.clone()],
                offset - origin.range.start..end.min(origin.range.end) - origin.range.start,
            )
        } else {
            (filename, source, offset..end)
        };
        let error = !issue.warning || werror;
        let diagnostic = (if error {
            Diagnostic::error()
        } else {
            Diagnostic::warning()
        })
        .with_message(issue.message)
        .with_labels(vec![Label::primary((), span)]);
        let _ = emit_to_io_write(
            &mut stderr,
            &Config::default(),
            &SimpleFile::new(name, text),
            &diagnostic,
        );
        failed |= error;
    }
    if failed {
        bail!("standard compatibility checks failed");
    }
    Ok(())
}

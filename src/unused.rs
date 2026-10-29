// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Unused declarations are checked on the source AST, before lowering erases them.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

const CATEGORIES: &[&str] = &[
    "unused-type",
    "unused-variable",
    "unused-but-set-variable",
    "unused-parameter",
    "unused-dummy-argument",
    "unused-function",
    "optional-argument",
    "implicit-typing",
    "unreachable-code",
    "format",
    "implicit-save",
    "missing-intent",
    "short-circuit",
    "shadow",
    "shadow-dummy-argument",
    "dead-store",
    "integer-division",
    "unchecked-status",
    "target-lifetime",
    "aliasing",
    "conversion",
    "precision-loss",
    "pointer-init",
    "do-subscript",
    "enum-exhaustive",
    "implicit-interface",
    "intrinsic-shadow",
    "keyword-name",
    "underflow",
    "empty-case",
    "unused-label",
];
fn canonical(name: &str) -> &str {
    match name {
        "unused-constant" => "unused-parameter",
        "unused-procedure" => "unused-function",
        other => other,
    }
}
pub fn validate_options(options: &[String]) -> Result<()> {
    for option in options {
        let name = option
            .strip_prefix("no-")
            .or_else(|| option.strip_prefix("error="))
            .unwrap_or(option);
        let name = canonical(name);
        if !CATEGORIES.contains(&name) && !matches!(name, "unused" | "unused-private") {
            bail!("unknown warning option `{option}`");
        }
    }
    Ok(())
}
pub(crate) fn selected(
    category: &str,
    private: bool,
    settings: &SemaSettings,
    options: &[String],
) -> (bool, bool) {
    let old = match category {
        "unused-dummy-argument" => "unused_param",
        "unused-function" => "unused_function",
        "unused-variable" | "unused-but-set-variable" => "unused_local",
        _ => category,
    };
    let program_variable = matches!(category, "unused-variable" | "unused-but-set-variable");
    let legacy_error = program_variable
        && (settings.deny.contains("unused_program_var")
            || settings.error.contains("unused_program_var"));
    let mut enabled = legacy_error
        || settings.wall
        || settings.deny.contains(old)
        || settings.error.contains(old)
        || settings.deny.contains(category)
        || settings.error.contains(category);
    let mut error = legacy_error
        || settings.werror
        || settings.deny.contains(old)
        || settings.error.contains(old)
        || settings.deny.contains(category)
        || settings.error.contains(category);
    for option in options {
        let name = canonical(
            option
                .strip_prefix("no-")
                .or_else(|| option.strip_prefix("error="))
                .unwrap_or(option),
        );
        if name == category
            || name == "unused" && category.starts_with("unused-")
            || name == "unused-private" && private && category.starts_with("unused-")
            || name == "unused-variable" && category == "unused-but-set-variable"
        {
            enabled = !option.starts_with("no-");
            if option.starts_with("error=") {
                error = true;
            }
        }
    }
    if settings.allow.contains(old)
        || settings.allow.contains(category)
        || program_variable && settings.allow.contains("unused_program_var")
    {
        enabled = false;
    }
    (enabled, error)
}
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Variable,
    Constant,
    Type,
    Procedure,
    Dummy,
    Generic,
}
struct Decl {
    name: String,
    kind: Kind,
    scope: usize,
    span: Range<usize>,
    public: bool,
    read: bool,
    written: bool,
    output: bool,
    result: bool,
    edges: HashSet<usize>,
}
struct Scope {
    parent: Option<usize>,
    owner: Option<usize>,
    module: bool,
    names: HashMap<String, usize>,
    uses: Vec<(String, Option<Vec<String>>, Vec<(String,String)>)>,
}
struct Analysis<'a> {
    tokens: &'a [Token],
    scopes: Vec<Scope>,
    decls: Vec<Decl>,
    bodies: HashMap<usize, usize>,
    modules: HashMap<String, usize>,
    roots: HashSet<usize>,
    dead: HashSet<usize>,
}
impl<'a> Analysis<'a> {
    fn scope(
        &mut self,
        parent: Option<usize>,
        owner: Option<usize>,
        module: bool,
        body: &[Stmt],
    ) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent,
            owner,
            module,
            names: HashMap::new(),
            uses: vec![],
        });
        self.bodies.insert(body.as_ptr() as usize, id);
        id
    }
    fn declare(&mut self, scope: usize, name: &str, kind: Kind, offset: usize) -> usize {
        let span = self
            .tokens
            .iter()
            .find(|t| {
                t.span.start >= offset
                    && matches!(&t.kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case(name))
            })
            .map(|t| t.span.clone())
            .unwrap_or(offset..offset);
        if let Some(&id) = self.scopes[scope].names.get(name) {
            if self.decls[id].kind == Kind::Dummy || kind == Kind::Constant {
                self.decls[id].span = span.clone();
            }
            if matches!(
                kind,
                Kind::Constant | Kind::Procedure | Kind::Type | Kind::Generic
            ) {
                self.decls[id].kind = kind;
                self.decls[id].span = span;
            }
            return id;
        }
        let id = self.decls.len();
        self.decls.push(Decl {
            name: name.into(),
            kind,
            scope,
            span,
            public: false,
            read: false,
            written: false,
            output: kind == Kind::Dummy,
            result: false,
            edges: HashSet::new(),
        });
        self.scopes[scope].names.insert(name.into(), id);
        id
    }
    fn gather(&mut self, body: &[Stmt], scope: usize) {
        let mut offset = 0;
        for stmt in body {
            match stmt {
                Stmt::SourceLocation(pos) => offset = *pos,
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => {
                    for name in names {
                        self.declare(scope, name, Kind::Variable, offset);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    self.declare(scope, name, Kind::Variable, offset);
                }
                Stmt::InitializedDecl {
                    entities,
                    attributes,
                    ..
                } => {
                    for (name, _) in entities {
                        self.declare(
                            scope,
                            name,
                            if attributes.iter().any(|a| a == "parameter") {
                                Kind::Constant
                            } else {
                                Kind::Variable
                            },
                            offset,
                        );
                    }
                }
                Stmt::DerivedType { name, .. } => {
                    self.declare(scope, name, Kind::Type, offset);
                }
                Stmt::GenericInterface { name, .. } => {
                    self.declare(scope, name, Kind::Generic, offset);
                }
                Stmt::OperatorInterface { operator, .. } => {
                    self.declare(
                        scope,
                        &format!("operator({operator})"),
                        Kind::Generic,
                        offset,
                    );
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let declaration_offset = body
                        .iter()
                        .find_map(|s| {
                            if let Stmt::SourceLocation(pos) = s {
                                Some(*pos)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(offset);
                    let owner = self.declare(scope, name, Kind::Procedure, declaration_offset);
                    let child = self.scope(Some(scope), Some(owner), false, body);
                    for name in params {
                        self.declare(child, name, Kind::Dummy, offset);
                    }
                    self.gather(body, child);
                    if let Stmt::Function { .. } = stmt {
                        let result = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(result)))) => result,
                            _ => name,
                        };
                        if let Some(&id) = self.scopes[child].names.get(result) {
                            self.decls[id].result = true;
                        }
                    }
                }
                Stmt::Module { name, body } => {
                    let child = self.scope(Some(scope), None, true, body);
                    self.modules.insert(name.clone(), child);
                    self.gather(body, child);
                }
                Stmt::Block { body } => {
                    let child = self.scope(Some(scope), self.scopes[scope].owner, false, body);
                    self.gather(body, child);
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => self.gather(body, scope),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.gather(then_body, scope);
                    if let Some(body) = else_body {
                        self.gather(body, scope);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        self.gather(&c.body, scope);
                    }
                    if let Some(body) = default {
                        self.gather(body, scope);
                    }
                }
                Stmt::Use { module, only, renames, .. } => {
                    self.scopes[scope].uses.push((module.clone(), only.clone(), renames.clone()))
                }
                Stmt::DummyIntent { name, intent } => {
                    if let Some(&id) = self.scopes[scope].names.get(name) {
                        if intent == "intent(out)" {
                            self.decls[id].output = true;
                        }
                    }
                }
                _ => {}
            }
        }
        for stmt in body {
            if let Stmt::DummyIntent { name, intent } = stmt {
                if let Some(&id) = self.scopes[scope].names.get(name) {
                    if intent == "intent(out)" {
                        self.decls[id].output = true;
                    }
                    if intent == "parameter" {
                        self.decls[id].kind = Kind::Constant;
                    }
                }
            }
        }
        for stmt in body {
            if let Stmt::DerivedDecl {
                names, attributes, ..
            } = stmt
            {
                if attributes.iter().any(|a| a == "intent(out)") {
                    for name in names {
                        if let Some(&id) = self.scopes[scope].names.get(name) {
                            self.decls[id].output = true;
                        }
                    }
                }
            }
        }
        if self.scopes[scope].module {
            let mut default = true;
            let mut access = HashMap::new();
            for stmt in body {
                if let Stmt::Accessibility { public, names } = stmt {
                    if names.is_empty() {
                        default = *public;
                    } else {
                        for name in names {
                            access.insert(name.clone(), *public);
                        }
                    }
                }
            }
            for id in self.scopes[scope].names.values().copied() {
                self.decls[id].public =
                    access.get(&self.decls[id].name).copied().unwrap_or(default);
            }
        }
    }
    fn lookup(&self, scope: usize, name: &str) -> Option<usize> {
        if let Some(&id) = self.scopes[scope].names.get(name) {
            return Some(id);
        }
        for (module, only, renames) in &self.scopes[scope].uses {
            let remote=renames.iter().find_map(|(local,remote)|if local==name {Some(remote.as_str())}else{None}).unwrap_or(name);
            if remote==name && renames.iter().any(|(local,remote)|remote==name&&local!=name)
                && !only.as_ref().is_some_and(|names|names.iter().filter(|n|n.as_str()==name).count()>renames.iter().filter(|(_,remote)|remote==name).count()) {continue;}
            if only
                .as_ref()
                .is_some_and(|names| !names.iter().any(|n| n == remote))
            {
                continue;
            }
            if let Some(&m) = self.modules.get(module) {
                if let Some(&id) = self.scopes[m].names.get(remote) {
                    if self.decls[id].public {
                        return Some(id);
                    }
                }
            }
        }
        self.scopes[scope].parent.and_then(|p| self.lookup(p, name))
    }
    fn reference(&mut self, scope: usize, owner: Option<usize>, name: &str, write: bool) {
        let name = name.split('%').next().unwrap_or(name);
        if let Some(id) = self.lookup(scope, name) {
            if write {
                self.decls[id].written = true;
            } else {
                self.decls[id].read = true;
            }
            if let Some(owner) = owner {
                self.decls[owner].edges.insert(id);
            } else {
                self.roots.insert(id);
            }
        }
    }
    fn words(&mut self, scope: usize, owner: Option<usize>, text: &str) {
        for word in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if word.starts_with(|c: char| c.is_ascii_alphabetic()) {
                self.reference(scope, owner, word, false);
            } else if let Some((_, suffix)) = word.split_once('_') {
                self.reference(scope, owner, suffix, false);
            }
        }
    }
    fn kind(&mut self, scope: usize, owner: Option<usize>, kind: &TypeSpec) {
        match kind {
            TypeSpec::IntegerKind(n) | TypeSpec::RealKind(n) | TypeSpec::Complex(n) => self.words(scope, owner, n),
            TypeSpec::CharacterLength(e) | TypeSpec::Referenced {source:e,..} => self.expr(scope, owner, e),
            _ => {}
        }
    }
    fn expr(&mut self, scope: usize, owner: Option<usize>, expr: &Expr) {
        match expr {
            Expr::Ident(n) => self.reference(scope, owner, n, false),
            Expr::IntLit(s) | Expr::RealLit(s) => self.words(scope, owner, s),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                self.reference(scope, owner, n, false);
                if n.contains('%') {
                    // A binding is an edge of its declared type; the object's declaration records that edge.
                    self.reference(scope, owner, n.split('%').next().unwrap(), false);
                }
                if let Some(operator) = n.strip_prefix("@defined_operator:") {
                    self.reference(scope, owner, &format!("operator({operator})"), false);
                }
                for arg in args {
                    self.expr(scope, owner, arg);
                }
            }
            Expr::Bin(op, a, b) => {
                use crate::ast::BinOp::*;
                let key = match op {
                    Add => "+",
                    Sub => "-",
                    Mul => "*",
                    Pow => "**",
                    Concat => "//",
                    And => ".and.",
                    Or => ".or.",
                    Eqv => ".eqv.",
                    Neqv => ".neqv.",
                    Div => "/",
                    Eq => "==",
                    Ne => "/=",
                    Lt => "<",
                    Le => "<=",
                    Gt => ">",
                    Ge => ">=",
                };
                self.reference(scope, owner, &format!("operator({key})"), false);
                self.expr(scope, owner, a);
                self.expr(scope, owner, b);
            }
            Expr::Un(op, e) => {
                let key = match op {
                    crate::ast::UnOp::Neg => "-",
                    crate::ast::UnOp::Not => ".not.",
                };
                self.reference(scope, owner, &format!("operator({key})"), false);
                self.expr(scope, owner, e);
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                self.reference(scope, owner, name, false);
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expr(scope, owner, e);
                }
            }
            _ => {}
        }
    }
    fn target(&mut self, scope: usize, owner: Option<usize>, expr: &Expr) {
        match expr {
            Expr::Ident(n) => self.reference(scope, owner, n, true),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                self.reference(scope, owner, n, true);
                for e in args {
                    self.expr(scope, owner, e);
                }
            }
            _ => self.expr(scope, owner, expr),
        }
    }
    fn walk(&mut self, body: &[Stmt], scope: usize, owner: Option<usize>) {
        for stmt in body {
            if self.dead.contains(&(stmt as *const Stmt as usize)) { continue; }
            match stmt {
                Stmt::VarDecl { kind, .. } => self.kind(scope, owner, kind),
                Stmt::ArrayDecl { kind, dims, .. } => {
                    self.kind(scope, owner, kind);
                    for e in dims {
                        self.expr(scope, owner, e);
                    }
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (name, e) in entities {
                        let id = self.lookup(scope, name).unwrap();
                        self.kind(scope, Some(id), kind);
                        if let Some(e) = e {
                            self.expr(scope, Some(id), e);
                            if self.decls[id].kind == Kind::Variable {
                                self.decls[id].written = true;
                            }
                        }
                    }
                }
                Stmt::DerivedDecl {
                    type_name, names, dims, ..
                } => {
                    for e in dims {self.expr(scope,owner,e);}
                    for name in names {
                        let id = self.lookup(scope, name);
                        self.reference(scope, id, type_name, false);
                        // A type in a live procedure's declaration/signature is used,
                        // even when its corresponding variable or dummy is unused.
                        self.reference(scope, owner, type_name, false);
                    }
                }
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    let id = self.lookup(scope, name);
                    for component in components {
                        match component {
                            Stmt::VarDecl { kind, .. } => self.kind(scope, id, kind),
                            Stmt::InitializedDecl { kind, entities, .. } => {
                                self.kind(scope, id, kind);
                                for (_, e) in entities {
                                    if let Some(e) = e {
                                        self.expr(scope, id, e);
                                    }
                                }
                            }
                            Stmt::ArrayDecl { kind, dims, .. } => {
                                self.kind(scope, id, kind);
                                for e in dims {
                                    self.expr(scope, id, e);
                                }
                            }
                            _ => {}
                        }
                    }
                    for (_, procedure) in bindings {
                        self.reference(scope, id, procedure, false);
                    }
                }
                Stmt::GenericInterface {
                    name, procedures, ..
                } => {
                    let id = self.lookup(scope, name);
                    for p in procedures {
                        self.reference(scope, id, p, false);
                    }
                }
                Stmt::OperatorInterface {
                    operator,
                    procedures,
                    ..
                } => {
                    let id = self.lookup(scope, &format!("operator({operator})"));
                    for p in procedures {
                        self.reference(scope, id, p, false);
                    }
                }
                Stmt::Assign { name, value } => {
                    self.reference(scope, owner, name, true);
                    self.expr(scope, owner, value);
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.reference(scope, owner, name, true);
                    for e in indices {
                        self.expr(scope, owner, e);
                    }
                    self.expr(scope, owner, value);
                }
                Stmt::Print { items } => {
                    for e in items {
                        self.expr(scope, owner, e);
                    }
                }
                Stmt::Read { args } => {
                    for e in args {
                        self.target(scope, owner, e);
                    }
                }
                Stmt::Return(Some(e)) => self.expr(scope, owner, e),
                Stmt::CallSub { name, args } => {
                    self.reference(scope, owner, name, false);
                    let user_procedure = self.lookup(scope, name).is_some_and(|id| {
                        matches!(self.decls[id].kind, Kind::Procedure | Kind::Generic)
                    });
                    let outputs = !user_procedure
                        && matches!(
                            name.as_str(),
                            "random_number" | "cpu_time" | "system_clock" | "date_and_time" | "getcwd"
                        );
                    for (index,e) in args.iter().enumerate() {
                        let keyword=crate::named_arguments::keyword(e);
                        let environment_output=!user_procedure && matches!(name.as_str(),"get_environment_variable"|"get_command_argument") && keyword.map_or((1..=3).contains(&index),|(key,_)|matches!(key,"value"|"length"|"status"));
                        let e = keyword.map_or(e, |(_, e)| e);
                        if outputs || environment_output {
                            self.target(scope, owner, e);
                        } else {
                            self.expr(scope, owner, e);
                        }
                    }
                }
                Stmt::Function {
                    body, return_type, ..
                } => {
                    let child = self.bodies[&(body.as_ptr() as usize)];
                    let id = self.scopes[child].owner;
                    if let Some(kind) = return_type {
                        self.kind(scope, id, kind);
                    }
                    self.walk(body, child, id);
                }
                Stmt::Subroutine { body, .. } => {
                    let child = self.bodies[&(body.as_ptr() as usize)];
                    self.walk(body, child, self.scopes[child].owner);
                }
                Stmt::Module { body, .. } => {
                    let child = self.bodies[&(body.as_ptr() as usize)];
                    self.walk(body, child, None);
                }
                Stmt::Block { body } => {
                    let child = self.bodies[&(body.as_ptr() as usize)];
                    self.walk(body, child, owner);
                }
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    self.reference(scope, owner, var, true);
                    self.reference(scope, owner, var, false);
                    self.expr(scope, owner, start);
                    self.expr(scope, owner, end);
                    self.walk(body, scope, owner);
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(scope, owner, cond);
                    self.walk(body, scope, owner);
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(scope, owner, cond);
                    self.walk(then_body, scope, owner);
                    if let Some(body) = else_body {
                        self.walk(body, scope, owner);
                    }
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(scope, owner, expr);
                    for case in cases {
                        for item in &case.items {
                            match item {
                                crate::ast::CaseItem::Single(e) => self.expr(scope, owner, e),
                                crate::ast::CaseItem::Range(a, b) => {
                                    self.expr(scope, owner, a);
                                    self.expr(scope, owner, b);
                                }
                            }
                        }
                        self.walk(&case.body, scope, owner);
                    }
                    if let Some(body) = default {
                        self.walk(body, scope, owner);
                    }
                }
                _ => {}
            }
        }
    }
}
pub fn check(
    program: &Program,
    src: &str,
    tokens: &[Token],
    own_len: usize,
    filename: &str,
    origins: Option<&[crate::source_diagnostics::SourceOrigin]>,
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    validate_options(options)?;
    if !settings.wall && options.is_empty() && settings.deny.is_empty() && settings.error.is_empty()
    {
        return Ok(());
    }
    let mut a = Analysis {
        tokens,
        scopes: vec![],
        decls: vec![],
        bodies: HashMap::new(),
        modules: HashMap::new(),
        roots: HashSet::new(),
        dead: crate::reachability::analyze(program).dead,
    };
    let root = a.scope(None, None, false, &program.body);
    a.gather(&program.body, root);
    a.walk(&program.body, root, None);
    let has_main = program.name != "<anon>";
    if !has_main {
        for id in a.scopes[root].names.values().copied() {
            if a.decls[id].kind == Kind::Procedure {
                a.decls[id].public = true;
            }
        }
    }

    for (id, d) in a.decls.iter().enumerate() {
        if d.public {
            a.roots.insert(id);
        }
    }
    let mut reached = HashSet::new();
    let mut pending = a.roots.iter().copied().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if reached.insert(id) {
            pending.extend(a.decls[id].edges.iter().copied());
        }
    }
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
    let mut ids = (0..a.decls.len()).collect::<Vec<_>>();
    ids.sort_by_key(|&id| a.decls[id].span.start);
    for id in ids {
        let d = &a.decls[id];
        if d.span.start >= own_len { continue; }
        let module = a.scopes[d.scope].module;
        if d.public
            || d.result
            || d.name.starts_with('@')
            || d.kind == Kind::Generic
            || d.kind == Kind::Dummy && d.output && d.written
        {
            continue;
        }
        let used = if module {
            reached.contains(&id) && (d.kind != Kind::Variable || d.read)
        } else if d.kind == Kind::Procedure {
            reached.contains(&id)
        } else {
            d.read
        };
        if used {
            continue;
        }
        let (category, noun) = match d.kind {
            Kind::Type => ("unused-type", "derived type"),
            Kind::Constant => ("unused-parameter", "named constant"),
            Kind::Procedure => ("unused-function", "procedure"),
            Kind::Dummy => ("unused-dummy-argument", "dummy argument"),
            _ => {
                if d.written {
                    ("unused-but-set-variable", "variable")
                } else {
                    ("unused-variable", "variable")
                }
            }
        };
        let (enabled, error) = selected(category, module, settings, options);
        if !enabled {
            continue;
        }
        let message = format!(
            "{}{} `{}` {} [{}]",
            if module { "private module " } else { "" },
            noun,
            d.name,
            if d.kind == Kind::Variable && d.written {
                "is assigned but never read"
            } else {
                "is never used"
            },
            category
        );
        let (name, source, span) = if let Some(origin) =
            origins.and_then(|o| o.iter().find(|o| o.range.contains(&d.span.start)))
        {
            (
                origin.name.as_str(),
                origin.source(src),
                origin.span(d.span.clone()),
            )
        } else {
            (filename, src, d.span.clone())
        };
        let diagnostic = if error {
            Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(message)
        .with_labels(vec![Label::primary((), span)]);
        let _ = emit_to_io_write(
            &mut stderr,
            &Config::default(),
            &SimpleFile::new(name, source),
            &diagnostic,
        );
        failed |= error;
    }
    if failed {
        bail!("unused declaration warnings treated as errors");
    }
    Ok(())
}

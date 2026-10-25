// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Conservative source advice: scope, overwritten values, status, and storage.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::sema::SemaSettings;
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
#[path = "call_label_warnings.rs"]
mod call_label_warnings;
#[path = "value_warnings.rs"]
mod value_warnings;

type Names = HashSet<String>;
#[derive(Clone, Copy, PartialEq)]
enum ScopeKind {
    Main,
    Module,
    Procedure,
    Block,
}
struct Scope {
    parent: Option<usize>,
    kind: ScopeKind,
    names: HashMap<String, usize>,
    types: HashMap<String, usize>,
    uses: Vec<(String, Option<Vec<String>>, Vec<(String, String)>)>,
    default_public: bool,
    access: HashMap<String, bool>,
}
struct Decl {
    name: String,
    scope: usize,
    span: Range<usize>,
    attrs: Names,
    kind: Option<TypeSpec>,
    dims: Vec<Expr>,
    constant: Option<Expr>,
    dummy: bool,
    result: bool,
    procedure: Option<usize>,
    nominal: Option<String>,
}
struct Procedure<'a> {
    name: String,
    elemental: bool,
    params: Vec<String>,
    scope: usize,
    body: &'a [Stmt],
}
struct Advice {
    category: &'static str,
    span: Range<usize>,
    message: String,
    related: Option<(Range<usize>, String)>,
    note: &'static str,
}
struct Analysis<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    scopes: Vec<Scope>,
    decls: Vec<Decl>,
    modules: HashMap<String, usize>,
    procedures: Vec<Procedure<'a>>,
    bodies: HashMap<usize, usize>,
    dead: HashSet<usize>,
    warnings: Vec<Advice>,
    types: Vec<value_warnings::TypeInfo<'a>>,
    inference: Option<crate::operators::ExpressionTypes>,
    interface_errors: Vec<Advice>,
}
fn root(name: &str) -> &str {
    name.split('%').next().unwrap_or(name)
}
fn children(s: &Stmt) -> Vec<&[Stmt]> {
    match s {
        Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => vec![body],
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            let mut v = vec![then_body.as_slice()];
            if let Some(b) = else_body {
                v.push(b);
            }
            v
        }
        Stmt::SelectCase { cases, default, .. } => {
            let mut v: Vec<_> = cases.iter().map(|c| c.body.as_slice()).collect();
            if let Some(b) = default {
                v.push(b);
            }
            v
        }
        _ => vec![],
    }
}
fn expressions(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Call(_, a) | Expr::Index(_, a) => a.iter().collect(),
        Expr::Bin(_, a, b) => vec![a, b],
        Expr::Un(_, e) => vec![e],
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => [lower, upper, stride]
            .into_iter()
            .filter_map(|x| x.as_deref())
            .collect(),
        _ => vec![],
    }
}
fn value(e: &Expr) -> &Expr {
    crate::named_arguments::keyword(e).map_or(e, |(_, v)| v)
}
fn ts_arrow(ts: &[Token]) -> bool {
    ts.iter().any(|t| matches!(t.kind, TokenKind::Arrow))
}
impl<'a> Analysis<'a> {
    fn gather(&mut self, body: &'a [Stmt], scope: usize, contains: &[String], mut offset: usize) {
        self.bodies.insert(body.as_ptr() as usize, scope);
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            let mut declared = vec![];
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        declared.push(self.declare(scope, n, offset, Some(kind.clone())));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    let id = self.declare(scope, name, offset, Some(kind.clone()));
                    self.decls[id].dims = dims.clone();
                    declared.push(id);
                }
                Stmt::InitializedDecl {
                    kind,
                    attributes,
                    entities,
                } => {
                    for (n, e) in entities {
                        let id = self.declare(scope, n, offset, Some(kind.clone()));
                        self.decls[id].attrs.extend(attributes.iter().cloned());
                        if attributes.iter().any(|a| a == "parameter") {
                            self.decls[id].constant = e.clone();
                        } else if e.is_some() {
                            self.decls[id].attrs.insert("save".into());
                        }
                        declared.push(id);
                    }
                }
                Stmt::DerivedDecl {
                    type_name,
                    polymorphic,
                    names,
                    attributes,
                    dims,
                    ..
                } => {
                    for n in names {
                        let id = self.declare(scope, n, offset, None);
                        self.decls[id].attrs.extend(attributes.iter().cloned());
                        self.decls[id].dims = dims.clone();
                        self.decls[id].nominal = Some(type_name.clone());
                        if *polymorphic {
                            self.decls[id].attrs.insert("@polymorphic".into());
                        }
                        if attributes.iter().any(|a| a == "parameter")
                            && self
                                .statement_tokens(offset)
                                .iter()
                                .any(|t| matches!(t.kind, TokenKind::KwEnumerator))
                        {
                            self.decls[id].attrs.insert("@warning_enum_member".into());
                        }
                        declared.push(id);
                    }
                }
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    let id = self.types.len();
                    self.scopes[scope].types.insert(name.clone(), id);
                    self.types.push(value_warnings::TypeInfo {
                        scope,
                        name: name.clone(),
                        components,
                        enumeration: bindings.iter().any(|(n, _)| n == "@enumeration"),
                    });
                }
                Stmt::GenericInterface { name, .. } => {
                    let id = self.declare(scope, name, offset, None);
                    self.decls[id].attrs.insert("@generic".into());
                }
                Stmt::Assign { name, value } => {
                    if let Some(&id) = self.scopes[scope].names.get(name) {
                        if self.decls[id].attrs.contains("parameter") {
                            self.decls[id].constant = Some(value.clone());
                        }
                    }
                }
                Stmt::DummyIntent { name, intent } => {
                    let id = self.declare(scope, name, offset, None);
                    self.decls[id].attrs.insert(intent.clone());
                }
                Stmt::Use {
                    module,
                    nature,
                    only,
                    renames,
                    ..
                } => self.scopes[scope].uses.push((
                    if nature.as_deref() == Some("intrinsic") && matches!(module.as_str(), "iso_fortran_env" | "ieee_arithmetic") {
                        format!("@intrinsic_{module}")
                    } else {
                        module.clone()
                    },
                    only.clone(),
                    renames.clone(),
                )),
                Stmt::Accessibility { public, names } => {
                    if names.is_empty() {
                        self.scopes[scope].default_public = *public;
                    }
                    for n in names {
                        self.scopes[scope].access.insert(n.clone(), *public);
                    }
                }
                Stmt::Module { name, body } => {
                    let child = self.scope(None, ScopeKind::Module, body);
                    self.modules.insert(name.clone(), child);
                    self.gather(body, child, &[], offset);
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let id = self.declare(scope, name, offset, None);
                    let parent =
                        if self.scopes[scope].kind == ScopeKind::Main && !contains.contains(name) {
                            None
                        } else {
                            Some(scope)
                        };
                    let child = self.scope(parent, ScopeKind::Procedure, body);
                    self.decls[id].procedure = Some(self.procedures.len());
                    self.procedures.push(Procedure {
                        name: name.clone(),
                        elemental: matches!(
                            s,
                            Stmt::Function {
                                elemental: true,
                                ..
                            } | Stmt::Subroutine {
                                elemental: true,
                                ..
                            }
                        ),
                        params: params.clone(),
                        scope: child,
                        body,
                    });
                    for p in params {
                        let id = self.declare(child, p, offset, None);
                        self.decls[id].dummy = true;
                    }
                    self.gather(body, child, &[], offset);
                    if let Stmt::Function { return_type, .. } = s {
                        let result = body
                            .iter()
                            .rev()
                            .find_map(|s| match s {
                                Stmt::Return(Some(Expr::Ident(n))) => Some(n.as_str()),
                                _ => None,
                            })
                            .unwrap_or(name);
                        let id = self.declare(child, result, offset, return_type.clone());
                        self.decls[id].result = true;
                    }
                }
                Stmt::Block { body } => {
                    let child = self.scope(Some(scope), ScopeKind::Block, body);
                    self.gather(body, child, &[], offset);
                }
                _ => {
                    for b in children(s) {
                        self.gather(b, scope, contains, offset);
                    }
                }
            }
            for id in declared {
                let ts = self.statement_tokens(offset);
                let attrs = [
                    "target",
                    "pointer",
                    "allocatable",
                    "save",
                    "parameter",
                    "value",
                    "volatile",
                    "asynchronous",
                    "external",
                ]
                .into_iter()
                .filter(|a| crate::declaration_warnings::attribute(ts, a))
                .map(str::to_string)
                .collect::<Vec<_>>();
                self.decls[id].attrs.extend(attrs);
                if self.decls[id].attrs.contains("pointer")
                    && ts_arrow(self.statement_tokens(offset))
                {
                    self.decls[id].attrs.insert("save".into());
                }
            }
        }
        if body
            .iter()
            .any(|s| matches!(s,Stmt::CallSub{name,..} if name==crate::equivalence::STATEMENT))
        {
            for id in self.scopes[scope].names.values() {
                self.decls[*id].attrs.insert("storage-associated".into());
            }
        }
    }
    fn shadows(&mut self) {
        for d in &self.decls {
            if d.procedure.is_some()
                || d.result
                || !matches!(
                    self.scopes[d.scope].kind,
                    ScopeKind::Procedure | ScopeKind::Block
                )
            {
                continue;
            }
            let Some(original) = self.scopes[d.scope]
                .parent
                .and_then(|p| self.resolve(p, &d.name))
            else {
                continue;
            };
            let original = &self.decls[original];
            if original.procedure.is_some() {
                continue;
            }
            self.warnings.push(Advice {
                category: if d.dummy {
                    "shadow-dummy-argument"
                } else {
                    "shadow"
                },
                span: d.span.clone(),
                message: format!(
                    "{} `{}` shadows a declaration in an enclosing scope",
                    if d.dummy {
                        "dummy argument"
                    } else {
                        "local declaration"
                    },
                    d.name
                ),
                related: Some((original.span.clone(), "original declaration".into())),
                note:
                    "use distinct names when both entities are needed; shadowing is legal Fortran",
            });
        }
    }
    fn scope(&mut self, parent: Option<usize>, kind: ScopeKind, body: &[Stmt]) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent,
            kind,
            names: HashMap::new(),
            types: HashMap::new(),
            uses: vec![],
            default_public: true,
            access: HashMap::new(),
        });
        self.bodies.insert(body.as_ptr() as usize, id);
        id
    }
    fn statement_tokens(&self, offset: usize) -> &[Token] {
        crate::declaration_warnings::statement_tokens(&self.tokens, offset, usize::MAX, self.src)
    }
    fn span(&self, offset: usize, name: &str) -> Range<usize> {
        let ts = self.statement_tokens(offset);
        let first = ts
            .iter()
            .position(|t| matches!(t.kind, TokenKind::DColon))
            .map_or(0, |i| i + 1);
        ts[first..]
            .iter()
            .find(|t| matches!(&t.kind,TokenKind::Ident(n) if n == root(name)))
            .or_else(|| {
                ts.iter()
                    .find(|t| matches!(&t.kind,TokenKind::Ident(n) if n == root(name)))
            })
            .map(|t| t.span.clone())
            .unwrap_or(offset..offset)
    }
    fn declare(
        &mut self,
        scope: usize,
        name: &str,
        offset: usize,
        kind: Option<TypeSpec>,
    ) -> usize {
        let span = self.span(offset, name);
        if let Some(&id) = self.scopes[scope].names.get(name) {
            if kind.is_some() {
                self.decls[id].kind = kind;
                self.decls[id].span = span;
            }
            return id;
        }
        let id = self.decls.len();
        self.decls.push(Decl {
            name: name.into(),
            scope,
            span,
            attrs: Names::new(),
            kind,
            dims: vec![],
            constant: None,
            dummy: false,
            result: false,
            procedure: None,
            nominal: None,
        });
        self.scopes[scope].names.insert(name.into(), id);
        id
    }
    fn resolve(&self, scope: usize, name: &str) -> Option<usize> {
        self.resolve_inner(scope, root(name), &mut HashSet::new())
    }
    fn resolve_inner(
        &self,
        scope: usize,
        name: &str,
        visiting: &mut HashSet<(usize, String)>,
    ) -> Option<usize> {
        if !visiting.insert((scope, name.into())) {
            return None;
        }
        let s = &self.scopes[scope];
        if let Some(id) = s.names.get(name) {
            return Some(*id);
        }
        for (module, only, renames) in &s.uses {
            let remote = renames
                .iter()
                .find(|(local, _)| local == name)
                .map(|(_, remote)| remote.as_str())
                .unwrap_or(name);
            if remote == name && renames.iter().any(|(local, r)| r == name && local != name) {
                continue;
            }
            if only
                .as_ref()
                .is_some_and(|v| !v.iter().any(|n| n == name || n == remote))
            {
                continue;
            }
            if let Some(&m) = self.modules.get(module) {
                let public = self.scopes[m]
                    .access
                    .get(remote)
                    .copied()
                    .unwrap_or(self.scopes[m].default_public);
                if public {
                    if let Some(id) = self.resolve_inner(m, remote, visiting) {
                        return Some(id);
                    }
                }
            }
        }
        s.parent.and_then(|p| self.resolve_inner(p, name, visiting))
    }
    fn data_reads(&self, e: &Expr, scope: usize, out: &mut HashSet<usize>) {
        match e {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Section { name: n, .. } => {
                if let Some(id) = self.resolve(scope, n) {
                    if self.decls[id].procedure.is_none() {
                        out.insert(id);
                    }
                }
            }
            Expr::Call(n, _) => {
                if let Some(id) = self.resolve(scope, n) {
                    if !self.decls[id].dims.is_empty() {
                        out.insert(id);
                    }
                }
            }
            _ => {}
        }
        for e in expressions(e) {
            self.data_reads(e, scope, out);
        }
    }
    fn reads(&self, s: &Stmt, scope: usize) -> HashSet<usize> {
        let mut s = s.clone();
        let mut out = HashSet::new();
        for e in crate::procedure_arguments::expressions(&mut s) {
            self.data_reads(e, scope, &mut out);
        }
        out
    }
    fn user_call(&self, e: &Expr, scope: usize) -> bool {
        if let Expr::Call(n, _) = e {
            if !n.starts_with('@') {
                if let Some(id) = self.resolve(scope, n) {
                    if self.decls[id].procedure.is_some()
                        || self.decls[id].attrs.contains("@generic")
                        || self.decls[id].attrs.contains("external")
                    {
                        return true;
                    }
                } else if crate::intrinsics::purity(n).is_none() {
                    return true;
                }
            }
        }
        expressions(e).iter().any(|e| self.user_call(e, scope))
    }
    fn ordinary_scalar(&self, id: usize, scope: usize) -> bool {
        let d = &self.decls[id];
        d.scope == scope
            && d.dims.is_empty()
            && d.kind.is_some()
            && d.procedure.is_none()
            && !d.dummy
            && !d.result
            && self.scopes[scope].kind != ScopeKind::Module
            && ![
                "save",
                "parameter",
                "pointer",
                "target",
                "allocatable",
                "volatile",
                "asynchronous",
                "storage-associated",
            ]
            .iter()
            .any(|a| d.attrs.contains(*a))
    }
    fn automatic_target(&self, id: usize) -> bool {
        let d = &self.decls[id];
        matches!(
            self.scopes[d.scope].kind,
            ScopeKind::Procedure | ScopeKind::Block
        ) && (!d.dummy || d.attrs.contains("value"))
            && !d.attrs.contains("save")
            && !d.attrs.contains("parameter")
    }
    fn ancestor(&self, outer: usize, mut inner: usize) -> bool {
        loop {
            if outer == inner {
                return true;
            }
            match self.scopes[inner].parent {
                Some(p) => inner = p,
                None => return false,
            }
        }
    }
    fn integer(&self, e: &Expr, scope: usize, visiting: &mut HashSet<usize>) -> Option<i64> {
        let mut folded = e.clone();
        fn substitute(
            e: &mut Expr,
            a: &Analysis<'_>,
            scope: usize,
            visiting: &mut HashSet<usize>,
        ) -> Option<()> {
            if let Expr::IntLit(text) = e {
                if let Some((number, kind)) = text.split_once('_') {
                    let kind = if kind.parse::<i64>().is_ok() {
                        Expr::IntLit(kind.into())
                    } else {
                        Expr::Ident(kind.into())
                    };
                    let kind = a.integer(&kind, scope, visiting)?;
                    *e = Expr::Call(
                        "@integer_literal".into(),
                        vec![Expr::IntLit(number.into()), Expr::IntLit(kind.to_string())],
                    );
                }
                return Some(());
            }
            if let Expr::RealLit(text) = e {
                if let Some((number, kind)) = text.split_once('_') {
                    let kind = if kind.parse::<i64>().is_ok() {
                        Expr::IntLit(kind.into())
                    } else {
                        Expr::Ident(kind.into())
                    };
                    *e = Expr::RealLit(format!("{number}_{}", a.integer(&kind, scope, visiting)?));
                }
                return Some(());
            }
            if let Expr::Ident(n) = e {
                let Some(id) = a.resolve(scope, n) else {
                    *e = Expr::IntLit(a.intrinsic_integer(scope, n)?.to_string());
                    return Some(());
                };
                let d = &a.decls[id];
                let c = d.constant.as_ref()?;
                if !visiting.insert(id) {
                    return None;
                }
                let v = a.integer(c, d.scope, visiting);
                visiting.remove(&id);
                *e = Expr::IntLit(v?.to_string());
                return Some(());
            }
            match e {
                Expr::Call(n, args) if n == "@integer_literal" => {
                    if let Some(v) = args.first_mut() {
                        substitute(v, a, scope, visiting)?;
                    }
                    return Some(());
                }
                Expr::Call(n, _) if !n.starts_with('@') && a.resolve(scope, n).is_some() => {
                    // Do not fold a user procedure as a same-named intrinsic.
                    return None;
                }
                Expr::Call(_, args) | Expr::Index(_, args) => {
                    for e in args {
                        substitute(e, a, scope, visiting)?;
                    }
                }
                Expr::Bin(_, x, y) => {
                    substitute(x, a, scope, visiting)?;
                    substitute(y, a, scope, visiting)?;
                }
                Expr::Un(_, x) => substitute(x, a, scope, visiting)?,
                _ => {}
            }
            Some(())
        }
        substitute(&mut folded, self, scope, visiting)?;
        crate::parameters::folded_integer_constant(&folded).ok()
    }
    fn divisions(
        &mut self,
        e: &Expr,
        scope: usize,
        offset: usize,
        cursor: &mut usize,
        suppressed: bool,
    ) {
        let suppressed = suppressed
            || matches!(e,Expr::Call(n,_) if self.resolve(scope,n).is_none() && matches!(n.as_str(),"kind"|"rank"));
        if let Expr::Bin(op, x, y) = e {
            // Source order matters for continued lines and multiple divisions.
            self.divisions(x, scope, offset, cursor, suppressed);
            if matches!(op, BinOp::Div) {
                let span = self
                    .statement_tokens(offset)
                    .iter()
                    .filter(|t| matches!(t.kind, TokenKind::Slash))
                    .nth(*cursor)
                    .map(|t| t.span.clone())
                    .unwrap_or(offset..offset);
                *cursor += 1;
                if let (false, Some(x), Some(y)) = (
                    suppressed,
                    self.integer(x, scope, &mut HashSet::new()),
                    self.integer(y, scope, &mut HashSet::new()),
                ) {
                    if y != 0 && x.checked_rem(y).is_some_and(|r| r != 0) {
                        self.warnings.push(Advice {category:"integer-division",span,message:format!("integer division {x}/{y} truncates its result to {}",x/y),
                        related:None,note:"use a real operand when a fractional result is intended; integer division truncates toward zero"});
                    }
                }
            }
            self.divisions(y, scope, offset, cursor, suppressed);
            return;
        }
        for e in expressions(e) {
            self.divisions(e, scope, offset, cursor, suppressed);
        }
    }
    fn status_outputs(
        &self,
        s: &Stmt,
        scope: usize,
        offset: usize,
    ) -> Vec<(usize, String, Range<usize>)> {
        let Stmt::CallSub { name, args } = s else {
            return vec![];
        };
        let io = name.starts_with('@')
            && (name.contains("read")
                || name == "@write"
                || name.starts_with("@io_")
                || name.starts_with("@file_")
                || matches!(
                    name.as_str(),
                    "@open"
                        | "@close"
                        | "@inquire"
                        | "@rewind"
                        | "@backspace"
                        | "@endfile"
                        | "@flush"
                ));
        let allocation = matches!(name.as_str(), "@allocate" | "@deallocate");
        let intrinsic = self
            .resolve(scope, name)
            .is_none()
            .then(|| {
                if name == "execute_command_line" {
                    Some(crate::named_arguments::Signature {
                        params: ["command", "wait", "exitstat", "cmdstat", "cmdmsg"]
                            .map(str::to_string)
                            .into(),
                        optional: HashSet::new(),
                    })
                } else {
                    crate::named_arguments::intrinsic(name)
                }
            })
            .flatten();
        let mut found = vec![];
        if io || allocation {
            let key = if allocation { "stat" } else { "iostat" };
            let ts = self.statement_tokens(offset);
            for (i, w) in ts.windows(3).enumerate() {
                if matches!(&w[0].kind,TokenKind::Ident(n) if n==key)
                    && matches!(w[1].kind, TokenKind::Eq)
                {
                    if let TokenKind::Ident(n) = &w[2].kind {
                        // Only plain scalar destinations: an element/component
                        // needs storage-path analysis rather than a name match.
                        if ts.get(i + 3).is_some_and(|t| {
                            !matches!(t.kind, TokenKind::Comma | TokenKind::RParen)
                        }) {
                            continue;
                        }
                        if let Some(id) = self.resolve(scope, n) {
                            found.push((id, key.into(), w[2].span.clone()));
                        }
                    }
                }
            }
        }
        if let Some(signature) = intrinsic {
            for (i, e) in args.iter().enumerate() {
                let key = crate::named_arguments::keyword(e)
                    .map(|(k, _)| k)
                    .or_else(|| signature.params.get(i).map(String::as_str));
                if let Some(key @ ("status" | "cmdstat" | "exitstat")) = key {
                    if let Expr::Ident(n) = value(e) {
                        if let Some(id) = self.resolve(scope, n) {
                            found.push((id, key.into(), self.span(offset, n)));
                        }
                    }
                }
            }
        }
        found
    }
    fn future_use(&self, future: &[(&Stmt, usize, usize)], id: usize) -> bool {
        for &(s, scope, offset) in future {
            if self.dead.contains(&(s as *const Stmt as usize)) {
                continue;
            }
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::SourceLocation(_)
            ) {
                continue;
            }
            if let Stmt::Assign { name, value } = s {
                let mut reads = HashSet::new();
                self.data_reads(value, scope, &mut reads);
                if reads.contains(&id) {
                    return true;
                }
                if self.resolve(scope, name) == Some(id) {
                    return false;
                }
            } else {
                let outputs = self.status_outputs(s, scope, offset);
                if outputs.iter().any(|(n, _, _)| *n == id) {
                    let count = self
                        .statement_tokens(offset)
                        .iter()
                        .filter(
                            |t| matches!(&t.kind,TokenKind::Ident(n) if n==&self.decls[id].name),
                        )
                        .count();
                    return count > outputs.iter().filter(|(n, _, _)| *n == id).count();
                }
                if self.reads(s, scope).contains(&id) {
                    return true;
                }
            }
            let mut copy = s.clone();
            if crate::procedure_arguments::expressions(&mut copy)
                .into_iter()
                .any(|e| self.user_call(e, scope))
            {
                return true;
            }
            if let Stmt::CallSub { name, .. } = s {
                if !name.starts_with('@') {
                    return true;
                }
            }
            let subs = children(s);
            if !subs.is_empty() {
                // Any possible inspection, overwrite, or opaque call in a
                // control-flow region is enough to avoid claiming a dead status.
                for b in subs {
                    if self.region_mentions(b, scope, id) {
                        return true;
                    }
                }
            }
        }
        false
    }
    fn region_mentions(&self, body: &[Stmt], scope: usize, id: usize) -> bool {
        let scope = self
            .bodies
            .get(&(body.as_ptr() as usize))
            .copied()
            .unwrap_or(scope);
        body.iter().any(|s|{
            if matches!(s,Stmt::Function{..}|Stmt::Subroutine{..}|Stmt::Module{..}){return false;}
            if self.reads(s,scope).contains(&id){return true;}
            if matches!(s,Stmt::Assign{name,..}|Stmt::AssignIndex{name,..} if self.resolve(scope,name)==Some(id)){return true;}
            if matches!(s,Stmt::CallSub{name,..} if !name.starts_with('@')){return true;}
            children(s).iter().any(|b|self.region_mentions(b,scope,id))
        })
    }
    fn storage<'b>(&self, e: &'b Expr, scope: usize) -> Option<(usize, &'b str, Vec<&'b Expr>)> {
        let (n, indices) = match value(e) {
            Expr::Ident(n) => (n, vec![]),
            Expr::Index(n, a) => (n, a.iter().collect()),
            Expr::Call(n, a)
                if self
                    .resolve(scope, n)
                    .is_some_and(|id| !self.decls[id].dims.is_empty()) =>
            {
                (n, a.iter().collect())
            }
            Expr::Section { name, .. } => (name, vec![value(e)]),
            _ => return None,
        };
        let id = self.resolve(scope, n)?;
        if self.decls[id].procedure.is_some() || self.decls[id].attrs.contains("pointer") {
            return None;
        }
        Some((id, n, indices))
    }
    fn overlap(&self, x: &Expr, y: &Expr, scope: usize) -> bool {
        let (Some((a, an, ai)), Some((b, bn, bi))) =
            (self.storage(x, scope), self.storage(y, scope))
        else {
            return false;
        };
        if a != b
            || !(an == bn
                || an.strip_prefix(bn).is_some_and(|n| n.starts_with('%'))
                || bn.strip_prefix(an).is_some_and(|n| n.starts_with('%')))
        {
            return false;
        }
        fn interval(a: &Analysis<'_>, e: &Expr, scope: usize) -> Option<(i64, i64)> {
            if let Expr::Section {
                lower: Some(l),
                upper: Some(u),
                stride,
                ..
            } = e
            {
                if stride
                    .as_ref()
                    .is_some_and(|s| a.integer(s, scope, &mut HashSet::new()) != Some(1))
                {
                    return None;
                }
                Some((
                    a.integer(l, scope, &mut HashSet::new())?,
                    a.integer(u, scope, &mut HashSet::new())?,
                ))
            } else {
                let n = a.integer(e, scope, &mut HashSet::new())?;
                Some((n, n))
            }
        }
        // A known empty selection cannot share an element with anything.
        if ai
            .iter()
            .chain(bi.iter())
            .any(|e| interval(self, e, scope).is_some_and(|(l, u)| l > u))
        {
            return false;
        }
        if self.decls[a].dims.iter().any(|d| {
            match d {
                Expr::Call(n,b) if n=="@declared_bounds" && b.len()==2 => {
                    matches!((self.integer(&b[0],scope,&mut HashSet::new()),self.integer(&b[1],scope,&mut HashSet::new())),(Some(l),Some(u)) if l>u)
                }
                d=>self.integer(d,scope,&mut HashSet::new()).is_some_and(|n|n<=0),
            }
        }) {return false;}
        if ai.is_empty() || bi.is_empty() {
            return true;
        }
        if ai.len() != bi.len() {
            return false;
        }
        ai.iter().zip(bi).all(|(x, y)| {
            match (interval(self, x, scope), interval(self, y, scope)) {
                (Some((l, u)), Some((m, v))) => l <= u && m <= v && l <= v && m <= u,
                _ => false, // Unknown subscripts are not proof of overlap.
            }
        })
    }
    fn procedure_reads(&self, body: &[Stmt], scope: usize, id: usize) -> bool {
        body.iter().any(|s| {
            if matches!(
                s,
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }
            ) {
                return false;
            }
            self.reads(s, scope).contains(&id)
                || children(s).iter().any(|b| {
                    self.procedure_reads(
                        b,
                        *self.bodies.get(&(b.as_ptr() as usize)).unwrap_or(&scope),
                        id,
                    )
                })
        })
    }
    fn procedure_writes(&self, body: &[Stmt], scope: usize, id: usize) -> bool {
        body.iter().any(|s| {
            if matches!(
                s,
                Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. }
            ) {
                return false;
            }
            let writes = match s {
                Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                    self.resolve(scope, name) == Some(id)
                }
                Stmt::Read { args } => args
                    .iter()
                    .any(|e| self.storage(e, scope).is_some_and(|(n, _, _)| n == id)),
                Stmt::CallSub { name, args } if !name.starts_with('@') || name.contains("read") => {
                    args.iter()
                        .any(|e| self.storage(e, scope).is_some_and(|(n, _, _)| n == id))
                }
                _ => false,
            };
            writes
                || children(s).iter().any(|b| {
                    self.procedure_writes(
                        b,
                        *self.bodies.get(&(b.as_ptr() as usize)).unwrap_or(&scope),
                        id,
                    )
                })
        })
    }
    fn grouped_actual(&self, name: &str, argument: usize, offset: usize) -> bool {
        // The source AST erases grouping parentheses. An actual such as (x)
        // is an expression temporary, not the variable x for alias analysis.
        let ts = self.statement_tokens(offset);
        for (i, t) in ts.iter().enumerate() {
            if !matches!(&t.kind,TokenKind::Ident(n) if n==name)
                || !ts
                    .get(i + 1)
                    .is_some_and(|t| matches!(t.kind, TokenKind::LParen))
            {
                continue;
            }
            let mut depth = 0;
            let mut slot = 0;
            let mut first = i + 2;
            for j in i + 2..ts.len() {
                let end = depth == 0 && matches!(ts[j].kind, TokenKind::Comma | TokenKind::RParen);
                if end {
                    if slot == argument {
                        let start = if ts
                            .get(first + 1)
                            .is_some_and(|t| matches!(t.kind, TokenKind::Eq))
                        {
                            first + 2
                        } else {
                            first
                        };
                        if start < j && matches!(ts[start].kind, TokenKind::LParen) {
                            return true;
                        }
                    }
                    if matches!(ts[j].kind, TokenKind::RParen) {
                        break;
                    }
                    slot += 1;
                    first = j + 1;
                }
                match ts[j].kind {
                    TokenKind::LParen | TokenKind::LBracket => depth += 1,
                    TokenKind::RParen | TokenKind::RBracket => depth -= 1,
                    _ => {}
                }
            }
        }
        false
    }
    fn aliasing(&mut self, name: &str, args: &[Expr], scope: usize, offset: usize) {
        let Some(p) = self
            .resolve(scope, name)
            .and_then(|id| self.decls[id].procedure)
        else {
            return;
        };
        let p = &self.procedures[p];
        let Ok(slots) =
            crate::named_arguments::Signature::new(&p.params, p.body).associate(name, args)
        else {
            return;
        };
        for i in 0..slots.len() {
            for j in i + 1..slots.len() {
                let (Some(x), Some(y)) = (slots[i], slots[j]) else {
                    continue;
                };
                if self.grouped_actual(name, x, offset) || self.grouped_actual(name, y, offset) {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    self.resolve(p.scope, &p.params[i]),
                    self.resolve(p.scope, &p.params[j]),
                ) else {
                    continue;
                };
                let excluded = |id: usize| {
                    ["pointer", "target", "value"]
                        .iter()
                        .any(|s| self.decls[id].attrs.contains(*s))
                };
                if excluded(a) || excluded(b) {
                    continue;
                }
                let writes = |id: usize| {
                    self.decls[id].attrs.contains("intent(out)")
                        || self.decls[id].attrs.contains("intent(inout)")
                            && self.procedure_writes(p.body, p.scope, id)
                };
                if !((writes(a) && self.procedure_reads(p.body, p.scope, b))
                    || (writes(b) && self.procedure_reads(p.body, p.scope, a)))
                {
                    continue;
                }
                if !self.overlap(&args[x], &args[y], scope) {
                    continue;
                }
                self.warnings.push(Advice{category:"aliasing",span:self.span(offset,name),
                message:format!("overlapping actual arguments for `{}` and `{}` of `{name}` may conflict with INTENT(OUT/INOUT)",p.params[i],p.params[j]),
                related:Some((self.decls[if writes(a){a}else{b}].span.clone(),"dummy argument that may be defined".into())),
                note:"use separate storage or a temporary when one dummy argument is defined and another reads overlapping storage"});
            }
        }
    }
    fn call_expressions(&mut self, e: &Expr, scope: usize, offset: usize) {
        if let Expr::Call(n, args) = e {
            self.aliasing(n, args, scope, offset);
        }
        for e in expressions(e) {
            self.call_expressions(e, scope, offset);
        }
    }
    fn pointer(
        &mut self,
        s: &Stmt,
        scope: usize,
        offset: usize,
        aliases: &mut HashMap<usize, usize>,
    ) {
        let Stmt::CallSub { name, args } = s else {
            return;
        };
        if name != "@pointer_associate" || args.len() != 2 {
            return;
        }
        let Expr::Ident(p) = &args[0] else { return };
        let Some(dest) = self.resolve(scope, p) else {
            return;
        };
        aliases.remove(&dest);
        let n = match &args[1] {
            Expr::Ident(n)
            | Expr::Index(n, _)
            | Expr::Call(n, _)
            | Expr::Section { name: n, .. } => n,
            _ => return,
        };
        let Some(mut target) = self.resolve(scope, n) else {
            return;
        };
        if self.decls[target].attrs.contains("pointer") {
            let Some(&underlying) = aliases.get(&target) else {
                return;
            };
            target = underlying;
        }
        aliases.insert(dest, target);
        if !self.automatic_target(target) {
            return;
        }
        let d = &self.decls[dest];
        let t = &self.decls[target];
        let escaping = d.result
            || d.dummy
            || d.attrs.contains("save")
            || matches!(
                self.scopes[d.scope].kind,
                ScopeKind::Module | ScopeKind::Main
            )
            || d.scope != t.scope && self.ancestor(d.scope, t.scope);
        if escaping {
            self.warnings.push(Advice{category:"target-lifetime",span:self.span(offset,p),
            message:format!("pointer `{p}` may outlive automatic target `{}`",t.name),
            related:Some((t.span.clone(),"target ceases to exist when its scope ends".into())),
            note:"associate an escaping pointer with storage whose lifetime extends beyond the call or block"});
        }
    }
    fn walk(
        &mut self,
        body: &'a [Stmt],
        scope: usize,
        mut offset: usize,
        tail: &[(&'a Stmt, usize, usize)],
        loop_tests: &HashSet<usize>,
    ) {
        let scope = self
            .bodies
            .get(&(body.as_ptr() as usize))
            .copied()
            .unwrap_or(scope);
        let mut statements = vec![];
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
            } else {
                statements.push((s, scope, offset));
            }
        }
        let mut stores: HashMap<usize, Range<usize>> = HashMap::new();
        let mut aliases = HashMap::new();
        for (i, &(s, _, offset)) in statements.iter().enumerate() {
            if let Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } = s
            {
                self.walk(body, scope, offset, &[], &HashSet::new());
                continue;
            }
            if self.dead.contains(&(s as *const Stmt as usize)) {
                continue;
            }
            let mut future = statements[i + 1..].to_vec();
            future.extend_from_slice(tail);
            for (id, key, span) in self.status_outputs(s, scope, offset) {
                if self.ordinary_scalar(id, scope)
                    && !loop_tests.contains(&id)
                    && !self.future_use(&future, id)
                {
                    self.warnings.push(Advice{category:"unchecked-status",span,
                        message:format!("{key} result `{}` is not inspected or forwarded before it is overwritten or leaves scope",self.decls[id].name),
                        related:None,note:"check or forward the status to handle a failed operation; this check recognizes uses, not the correctness of error handling"});
                }
            }
            let mut copy = s.clone();
            let exprs = crate::procedure_arguments::expressions(&mut copy);
            let opaque = exprs.iter().any(|e| self.user_call(e, scope))
                || matches!(s,Stmt::CallSub{name,..} if !name.starts_with('@'));
            let mut slash_cursor = 0;
            for e in exprs {
                self.divisions(e, scope, offset, &mut slash_cursor, false);
                self.call_expressions(e, scope, offset);
            }
            if let Stmt::CallSub { name, args } = s {
                self.aliasing(name, args, scope, offset);
            }
            if opaque {
                stores.clear();
                aliases.clear();
            }
            for id in self.reads(s, scope) {
                stores.remove(&id);
            }
            if let Stmt::Assign { name, .. } = s {
                if let Some(id) = self
                    .resolve(scope, name)
                    .filter(|id| self.ordinary_scalar(*id, scope))
                {
                    let span = self.span(offset, name);
                    if let Some(old) = stores.insert(id, span.clone()) {
                        self.warnings.push(Advice{category:"dead-store",span:old,
                        message:format!("value assigned to `{name}` is overwritten before it is read"),
                        related:Some((span,"overwriting assignment".into())),note:"remove the earlier assignment if its value and evaluation are unnecessary"});
                    }
                    if opaque {
                        stores.remove(&id);
                    }
                }
            }
            self.pointer(s, scope, offset, &mut aliases);
            if let Stmt::CallSub { name, .. } = s {
                if name.starts_with('@') && name != "@pointer_associate" {
                    stores.clear();
                    aliases.clear();
                }
            }
            let nested = children(s);
            if !nested.is_empty() {
                stores.clear();
                aliases.clear();
                let mut tests = loop_tests.clone();
                if matches!(s, Stmt::DoWhile { .. }) {
                    tests.extend(self.reads(s, scope));
                }
                for b in nested {
                    self.walk(b, scope, offset, &future, &tests);
                }
            }
            if matches!(s, Stmt::Return(_) | Stmt::Exit | Stmt::Cycle) {
                stores.clear();
                aliases.clear();
            }
        }
    }
}

pub fn check(
    program: &Program,
    src: &str,
    own_len: usize,
    filename: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
    settings: &SemaSettings,
    options: &[String],
) -> Result<()> {
    let mut optional = settings.clone();
    optional.wall = false;
    let controls = [
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
        "underflow",
        "empty-case",
        "unused-label",
    ]
    .into_iter()
    .map(|c| {
        let s = if matches!(
            c,
            "shadow"
                | "shadow-dummy-argument"
                | "dead-store"
                | "unchecked-status"
                | "conversion"
                | "implicit-interface"
                | "intrinsic-shadow"
        ) {
            &optional
        } else {
            settings
        };
        (c, crate::unused::selected(c, false, s, options))
    })
    .collect::<HashMap<_, _>>();
    let advice_enabled = controls.values().any(|c| c.0);
    let mut a = Analysis {
        src,
        tokens: crate::lexer::lex(src),
        scopes: vec![],
        decls: vec![],
        modules: HashMap::new(),
        procedures: vec![],
        bodies: HashMap::new(),
        dead: crate::reachability::analyze(program).dead,
        warnings: vec![],
        types: vec![],
        inference: advice_enabled
            .then(|| crate::operators::ExpressionTypes::new(program).ok())
            .flatten(),
        interface_errors: vec![],
    };
    // Module procedure interfaces must be available before the lowering pass
    // injects their implementations, including renamed and re-exported USEs.
    let mut ieee_body = crate::parameters::ieee_module_body()?;
    ieee_body.retain(|s| matches!(s, Stmt::GenericInterface { .. } | Stmt::Accessibility { .. }));
    ieee_body.insert(0, Stmt::SourceLocation(src.len()));
    let ieee = vec![Stmt::Module { name: "@intrinsic_ieee_arithmetic".into(), body: ieee_body }];
    let intrinsic_scope = a.scope(None, ScopeKind::Main, &ieee);
    a.gather(&ieee, intrinsic_scope, &[], src.len());
    let scope = a.scope(None, ScopeKind::Main, &program.body);
    a.gather(&program.body, scope, &program.contains, 0);
    if !a.modules.contains_key("ieee_arithmetic") {
        let ieee_scope = a.modules["@intrinsic_ieee_arithmetic"];
        a.modules.insert("ieee_arithmetic".into(), ieee_scope);
    }
    a.call_checks(&program.body, scope, 0);
    if !advice_enabled && a.interface_errors.is_empty() {
        return Ok(());
    }
    if advice_enabled {
        a.shadows();
        a.walk(&program.body, scope, 0, &[], &HashSet::new());
        a.pointer_initializers();
        a.value_walk(&program.body, scope, 0, &HashMap::new());
        a.syntax_value_checks(&program.body, scope, 0);
        a.unused_labels(&program.body, 0);
        a.intrinsic_shadows();
    }
    a.warnings.sort_by_key(|w| w.span.start);
    a.warnings
        .dedup_by(|x, y| x.category == y.category && x.span == y.span && x.message == y.message);
    use codespan_reporting::{
        diagnostic::{Diagnostic, Label},
        files::SimpleFiles,
        term::{
            emit_to_io_write,
            termcolor::{ColorChoice, StandardStream},
            Config,
        },
    };
    let mut files = SimpleFiles::new();
    let fallback = files.add(filename, src);
    let ids = origins
        .iter()
        .map(|o| files.add(o.name.as_str(), &src[o.range.clone()]))
        .collect::<Vec<_>>();
    let locate = |span: Range<usize>| {
        if let Some((i, o)) = origins
            .iter()
            .enumerate()
            .find(|(_, o)| o.range.contains(&span.start))
        {
            (
                ids[i],
                span.start - o.range.start..span.end.min(o.range.end) - o.range.start,
            )
        } else {
            (fallback, span.start.min(src.len())..span.end.min(src.len()))
        }
    };
    let required_failed = a.interface_errors.iter().any(|w| w.span.start < own_len);
    let mut failed = false;
    for (w, required) in a
        .warnings
        .into_iter()
        .map(|w| (w, false))
        .chain(a.interface_errors.into_iter().map(|w| (w, true)))
    {
        let (enabled, error) = if required {
            (true, true)
        } else {
            controls[w.category]
        };
        if !enabled || w.span.start >= own_len {
            continue;
        }
        let (file, span) = locate(w.span);
        let mut labels = vec![Label::primary(file, span)];
        if let Some((span, message)) = w.related {
            let (file, span) = locate(span);
            labels.push(Label::secondary(file, span).with_message(message));
        }
        let d = if error {
            Diagnostic::error()
        } else {
            Diagnostic::warning()
        }
        .with_message(format!("{} [{}]", w.message, w.category))
        .with_labels(labels)
        .with_notes(vec![w.note.into()]);
        emit_to_io_write(
            &mut StandardStream::stderr(ColorChoice::Auto),
            &Config::default(),
            &files,
            &d,
        )?;
        failed |= error;
    }
    if failed {
        if required_failed {
            bail!("explicit procedure interface required");
        }
        bail!("source advice warnings treated as errors");
    }
    Ok(())
}

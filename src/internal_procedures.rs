// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lift internal procedures with an explicit, borrowed host environment.
//! Data/initialization references use the same storage ABI as pointer accesses;
//! recursive and sibling calls forward the environment rather than copy data.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use crate::named_arguments::{Signature, OMITTED};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, HashMap, HashSet};

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn product(values: &[Expr]) -> Expr {
    match values.len() {
        0 => int(1),
        1 => values[0].clone(),
        n => bin(
            BinOp::Mul,
            product(&values[..n / 2]),
            product(&values[n / 2..]),
        ),
    }
}
#[derive(Clone)]
struct Variable {
    kind: TypeSpec,
    rank: usize,
    attributes: HashSet<String>,
}
#[derive(Clone)]
struct Capture {
    variable: Variable,
    data: String,
    flags: String,
    label: String,
    axes: Vec<(String, String)>,
}
impl Capture {
    fn fields(&self) -> Vec<String> {
        let mut fields = vec![self.data.clone(), self.flags.clone(), self.label.clone()];
        for (lower, extent) in &self.axes {
            fields.extend([lower.clone(), extent.clone()]);
        }
        fields
    }
    fn suffix(&self) -> &'static str {
        match self.variable.kind {
            TypeSpec::Integer(_) => "i",
            TypeSpec::Real => "r4",
            TypeSpec::DoublePrecision => "r8",
            TypeSpec::Logical => "l",
            _ => unreachable!(),
        }
    }
}
#[derive(Clone)]
struct Procedure {
    name: String,
    signature: Signature,
    inputs: HashSet<String>,
}
fn declarations(body: &[Stmt]) -> BTreeMap<String, Variable> {
    let mut vars = BTreeMap::new();
    for stmt in body {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                for name in names {
                    vars.insert(
                        name.clone(),
                        Variable {
                            kind: kind.clone(),
                            rank: 0,
                            attributes: HashSet::new(),
                        },
                    );
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                vars.insert(
                    name.clone(),
                    Variable {
                        kind: kind.clone(),
                        rank: dims.len(),
                        attributes: HashSet::new(),
                    },
                );
            }
            _ => {}
        }
    }
    for stmt in body {
        if let Stmt::DummyIntent { name, intent } = stmt {
            if let Some(v) = vars.get_mut(name) {
                v.attributes.insert(intent.clone());
            }
        }
    }
    vars
}
fn references(
    body: &[Stmt],
    locals: &HashSet<String>,
    host: &BTreeMap<String, Variable>,
    used: &mut HashSet<String>,
) {
    fn name(
        n: &str,
        locals: &HashSet<String>,
        host: &BTreeMap<String, Variable>,
        used: &mut HashSet<String>,
    ) {
        if host.contains_key(n) && !locals.contains(n) {
            used.insert(n.into());
        }
    }
    fn expr(
        e: &Expr,
        locals: &HashSet<String>,
        host: &BTreeMap<String, Variable>,
        used: &mut HashSet<String>,
    ) {
        match e {
            Expr::Ident(n) => name(n, locals, host, used),
            Expr::Call(n, a) | Expr::Index(n, a) => {
                name(n, locals, host, used);
                for e in a {
                    expr(e, locals, host, used);
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, locals, host, used);
                expr(b, locals, host, used);
            }
            Expr::Un(_, e) => expr(e, locals, host, used),
            Expr::Section {
                name: n,
                lower,
                upper,
                stride,
            } => {
                name(n, locals, host, used);
                for e in [lower, upper, stride].into_iter().flatten() {
                    expr(e, locals, host, used);
                }
            }
            _ => {}
        }
    }
    let mut locals = locals.clone();
    locals.extend(declarations(body).into_keys());
    for stmt in body {
        match stmt {
            Stmt::Assign { name: n, value } => {
                name(n, &locals, host, used);
                expr(value, &locals, host, used);
            }
            Stmt::AssignIndex {
                name: n,
                indices,
                value,
            } => {
                name(n, &locals, host, used);
                for e in indices {
                    expr(e, &locals, host, used);
                }
                expr(value, &locals, host, used);
            }
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expr(e, &locals, host, used);
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, &locals, host, used);
                references(then_body, &locals, host, used);
                if let Some(b) = else_body {
                    references(b, &locals, host, used);
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                name(var, &locals, host, used);
                expr(start, &locals, host, used);
                expr(end, &locals, host, used);
                references(body, &locals, host, used);
            }
            Stmt::DoWhile { cond, body } => {
                expr(cond, &locals, host, used);
                references(body, &locals, host, used);
            }
            Stmt::Block { body } => references(body, &locals, host, used),
            Stmt::SelectCase {
                expr: e,
                cases,
                default,
            } => {
                expr(e, &locals, host, used);
                for c in cases {
                    references(&c.body, &locals, host, used);
                }
                if let Some(b) = default {
                    references(b, &locals, host, used);
                }
            }
            Stmt::Return(Some(e)) => expr(e, &locals, host, used),
            Stmt::ArrayDecl { dims, .. } => {
                for e in dims {
                    expr(e, &locals, host, used);
                }
            }
            _ => {}
        }
    }
}

struct Rewrite<'a> {
    captures: &'a BTreeMap<String, Capture>,
    procedures: &'a HashMap<String, Procedure>,
    serial: &'a mut usize,
    name_capacity: usize,
}
impl Rewrite<'_> {
    fn direct_capture(&self, e: &Expr, locals: &HashSet<String>) -> bool {
        if let Some((_, value)) = crate::named_arguments::keyword(e) {
            return self.direct_capture(value, locals);
        }
        match e {
            Expr::Ident(n)
            | Expr::Call(n, _)
            | Expr::Index(n, _)
            | Expr::Section { name: n, .. } => self.capture(n, locals).is_some(),
            _ => false,
        }
    }
    fn writable_actuals(&self, name: &str, args: &[Expr], locals: &HashSet<String>) -> Result<()> {
        if let Some(p) = self.procedures.get(name) {
            for (param, slot) in p
                .signature
                .params
                .iter()
                .zip(p.signature.associate(name, args)?)
            {
                if let Some(slot) = slot {
                    if self.direct_capture(&args[slot], locals) && !p.inputs.contains(param) {
                        bail!("passing a host-associated object to writable dummy `{param}` is not yet supported");
                    }
                }
            }
        } else if !name.starts_with('@') && args.iter().any(|e| self.direct_capture(e, locals)) {
            bail!("passing host-associated objects directly to ordinary subroutines is not yet supported");
        }
        Ok(())
    }
    fn temp(&mut self, kind: TypeSpec, before: &mut Vec<Stmt>) -> String {
        *self.serial += 1;
        let name = format!("@host_value_{}", self.serial);
        before.push(Stmt::VarDecl {
            kind,
            names: vec![name.clone()],
        });
        name
    }
    fn capture(&self, name: &str, locals: &HashSet<String>) -> Option<Capture> {
        if locals.contains(name) {
            None
        } else {
            self.captures.get(name).cloned()
        }
    }
    fn offset(
        &mut self,
        name: &str,
        c: &Capture,
        indices: &[Expr],
        before: &mut Vec<Stmt>,
    ) -> Result<Expr> {
        if indices.len() != c.axes.len() {
            bail!(
                "host-associated array `{name}` requires {} subscripts",
                c.axes.len()
            );
        }
        if indices.is_empty() {
            return Ok(int(0));
        }
        let offset = self.temp(TypeSpec::Integer(Some(8)), before);
        before.push(set(&offset, int(0)));
        let mut stride = int(1);
        for (axis, (index, (lower, extent))) in indices.iter().zip(&c.axes).enumerate() {
            let subscript = self.temp(TypeSpec::Integer(Some(8)), before);
            before.push(set(&subscript, index.clone()));
            before.push(Stmt::If {
                cond: bin(
                    BinOp::Or,
                    bin(BinOp::Lt, id(&subscript), id(lower)),
                    bin(
                        BinOp::Ge,
                        id(&subscript),
                        bin(BinOp::Add, id(lower), id(extent)),
                    ),
                ),
                then_body: vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(format!(
                        "host-associated array `{name}` subscript outside bounds"
                    ))],
                }],
                else_body: None,
            });
            before.push(set(
                &offset,
                bin(
                    BinOp::Add,
                    id(&offset),
                    bin(
                        BinOp::Mul,
                        bin(BinOp::Sub, id(&subscript), id(lower)),
                        stride,
                    ),
                ),
            ));
            stride = product(
                &c.axes[..axis + 1]
                    .iter()
                    .map(|(_, e)| id(e))
                    .collect::<Vec<_>>(),
            );
        }
        Ok(id(&offset))
    }
    fn read(
        &mut self,
        name: &str,
        c: &Capture,
        indices: &[Expr],
        before: &mut Vec<Stmt>,
    ) -> Result<Expr> {
        let offset = self.offset(name, c, indices, before)?;
        before.push(Stmt::CallSub {
            name: "@check_initialized".into(),
            args: vec![
                call("@ptr_load_i", vec![id(&c.flags), offset.clone()]),
                call(
                    "@ptr_name",
                    vec![id(&c.label), int(self.name_capacity as i64)],
                ),
                if c.axes.is_empty() {
                    int(0)
                } else {
                    bin(BinOp::Add, offset.clone(), int(1))
                },
            ],
        });
        let value = self.temp(c.variable.kind.clone(), before);
        before.push(set(
            &value,
            call(
                &format!("@ptr_load_{}", c.suffix()),
                vec![id(&c.data), offset],
            ),
        ));
        Ok(id(&value))
    }
    fn store(&self, name: &str, c: &Capture, offset: Expr, value: Expr) -> Result<Stmt> {
        if c.variable.attributes.contains("intent(in)") {
            bail!("cannot define host-associated INTENT(IN) variable `{name}`");
        }
        Ok(Stmt::CallSub {
            name: format!("@ptr_store_{}", c.suffix()),
            args: vec![id(&c.data), id(&c.flags), offset, value],
        })
    }
    fn arguments(&self, name: &mut String, args: &mut Vec<Expr>) -> Result<()> {
        if let Some(p) = self.procedures.get(name) {
            p.signature.reorder(name, args)?;
            args.resize_with(p.signature.params.len(), || call(OMITTED, vec![]));
            for c in self.captures.values() {
                args.extend(c.fields().iter().map(|n| id(n)));
            }
            *name = p.name.clone();
        }
        Ok(())
    }
    fn expr(
        &mut self,
        e: &mut Expr,
        locals: &HashSet<String>,
        before: &mut Vec<Stmt>,
    ) -> Result<()> {
        match e {
            Expr::Ident(n) => {
                if let Some(c) = self.capture(n, locals) {
                    if !c.axes.is_empty() {
                        bail!("whole-array host-associated values are not yet supported; use subscripts or an array inquiry for `{n}`");
                    }
                    *e = self.read(n, &c, &[], before)?;
                }
            }
            Expr::Call(n, args)
                if !self.procedures.contains_key(n)
                    && matches!(n.as_str(), "size" | "shape" | "lbound" | "ubound")
                    && matches!(args.first(),Some(Expr::Ident(a)) if self.capture(a,locals).is_some()) =>
            {
                let Expr::Ident(array) = &args[0] else {
                    unreachable!()
                };
                let c = self.capture(array, locals).unwrap();
                if c.axes.is_empty() {
                    bail!("{n} requires a host-associated array");
                }
                let values: Vec<_> = c
                    .axes
                    .iter()
                    .map(|(lower, extent)| match n.as_str() {
                        "size" | "shape" => id(extent),
                        "lbound" => id(lower),
                        _ => bin(BinOp::Sub, bin(BinOp::Add, id(lower), id(extent)), int(1)),
                    })
                    .collect();
                *e = if let Some(dim) = args.get(1) {
                    let dim = crate::parameters::integer_constant(dim)?;
                    if dim < 1 || dim as usize > values.len() {
                        bail!("host-associated array inquiry DIM outside rank");
                    }
                    values[dim as usize - 1].clone()
                } else if n == "size" {
                    product(&values)
                } else {
                    call("@array_constructor", values)
                };
            }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                if self.procedures.contains_key(n) {
                    self.writable_actuals(n, args, locals)?;
                }
                for a in args.iter_mut() {
                    self.expr(a, locals, before)?;
                }
                if !self.procedures.contains_key(n) {
                    if let Some(c) = self.capture(n, locals) {
                        *e = self.read(n, &c, args, before)?;
                        return Ok(());
                    }
                }
                self.arguments(n, args)?;
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, locals, before)?;
                self.expr(b, locals, before)?;
            }
            Expr::Un(_, e) => self.expr(e, locals, before)?,
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if self.capture(name, locals).is_some() {
                    bail!("sections of host-associated arrays are not yet supported");
                }
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expr(e, locals, before)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &HashSet<String>) -> Result<()> {
        let mut locals = inherited.clone();
        locals.extend(declarations(body).into_keys());
        let mut output = vec![];
        let mut statement_location = None;
        for mut stmt in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location = Some(p); }
            (|| -> anyhow::Result<()> {
            let mut before = vec![];
            let mut after = vec![];
            if matches!(&stmt,Stmt::Read {args} if args.iter().any(|e|self.direct_capture(e,&locals)))
            {
                bail!("READ into host-associated objects is not yet supported");
            }
            if let Stmt::CallSub { name, args } = &stmt {
                self.writable_actuals(name, args, &locals)?;
                let outputs = match name.as_str() {
                    "@internal_read" => args.get(2..),
                    "@formatted_read" => args.get(2..),
                    "@formatted_read_status" => args.get(3..),
                    "@empty_read" => args.get(2..),
                    _ => None,
                };
                if outputs.is_some_and(|a| a.iter().any(|e| self.direct_capture(e, &locals))) {
                    bail!("READ definitions of host-associated objects are not yet supported");
                }
                if args.iter().any(|e|matches!(e,Expr::Call(n,a) if matches!(n.as_str(),"@io_spec:newunit"|"@io_spec:name"|"@keyword:stat") && a.iter().any(|e|self.direct_capture(e,&locals)))) {
                    bail!("I/O or allocation definitions of host-associated objects are not yet supported");
                }
            }
            match &mut stmt {
                Stmt::Assign { name, value } => {
                    self.expr(value, &locals, &mut before)?;
                    if let Some(c) = self.capture(name, &locals) {
                        if !c.axes.is_empty() {
                            bail!("whole-array assignment to host-associated `{name}` is not yet supported");
                        }
                        stmt = self.store(name, &c, int(0), value.clone())?;
                    }
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    for e in indices.iter_mut() {
                        self.expr(e, &locals, &mut before)?;
                    }
                    if let Some(c) = self.capture(name, &locals) {
                        let offset = self.offset(name, &c, indices, &mut before)?;
                        self.expr(value, &locals, &mut before)?;
                        stmt = self.store(name, &c, offset, value.clone())?;
                    } else {
                        self.expr(value, &locals, &mut before)?;
                    }
                }
                Stmt::Print { items }
                | Stmt::Read { args: items }
                | Stmt::CallSub { args: items, .. } => {
                    for e in items.iter_mut() {
                        self.expr(e, &locals, &mut before)?;
                    }
                    if let Stmt::CallSub { name, args } = &mut stmt {
                        self.arguments(name, args)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(cond, &locals, &mut before)?;
                    self.body(then_body, &locals)?;
                    if let Some(b) = else_body {
                        self.body(b, &locals)?;
                    }
                }
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    self.expr(start, &locals, &mut before)?;
                    self.expr(end, &locals, &mut before)?;
                    let control = self.capture(var, &locals);
                    self.body(body, &locals)?;
                    if let Some(c) = control {
                        if !c.axes.is_empty() || !matches!(c.variable.kind, TypeSpec::Integer(_)) {
                            bail!("host-associated DO variable must be scalar INTEGER");
                        }
                        let name = var.clone();
                        let temp = self.temp(c.variable.kind.clone(), &mut before);
                        body.insert(0, self.store(&name, &c, int(0), id(&temp))?);
                        after.push(self.store(&name, &c, int(0), id(&temp))?);
                        *var = temp;
                    }
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(cond, &locals, &mut before)?;
                    self.body(body, &locals)?;
                    if !before.is_empty() {
                        before.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        before.append(body);
                        *body = std::mem::take(&mut before);
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Block { body } => self.body(body, &locals)?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(expr, &locals, &mut before)?;
                    for c in cases {
                        self.body(&mut c.body, &locals)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &locals)?;
                    }
                }
                Stmt::Return(Some(e)) => self.expr(e, &locals, &mut before)?,
                Stmt::ArrayDecl { dims, .. } => {
                    for e in dims {
                        self.expr(e, &locals, &mut before)?;
                    }
                }
                _ => {}
            }
            output.extend(before);
            output.push(stmt);
            output.extend(after);
                    Ok(())
            })().map_err(|e| crate::source_diagnostics::at(e, statement_location))?;
        }
        *body = output;
        Ok(())
    }
}

fn hoist(body: &mut Vec<Stmt>) {
    fn collect(body: &mut Vec<Stmt>, decls: &mut Vec<Stmt>) {
        body.retain_mut(|stmt| {
            if matches!(stmt,Stmt::VarDecl {names,..} if names.iter().all(|n|n.starts_with("@host_value_"))) {
                decls.push(stmt.clone());return false;
            }
            match stmt {
                Stmt::If {then_body,else_body,..}=>{collect(then_body,decls);if let Some(b)=else_body {collect(b,decls);}},
                Stmt::Do {body,..}|Stmt::DoWhile {body,..}|Stmt::Block {body}=>collect(body,decls),
                Stmt::SelectCase {cases,default,..}=>{for c in cases {collect(&mut c.body,decls);}if let Some(b)=default {collect(b,decls);}},
                _=>{}
            }
            true
        });
    }
    let mut decls = vec![];
    collect(body, &mut decls);
    decls.append(body);
    *body = decls;
}
/// The type that IMPLICIT statements, or the default rules (I to N INTEGER,
/// otherwise REAL), give `name`; None under IMPLICIT NONE.
fn implicit_type(implicit: &[Stmt], name: &str) -> Option<TypeSpec> {
    use crate::ast::Implicit;
    let letter = name.chars().next()?.to_ascii_lowercase();
    let mut kind = if ('i'..='n').contains(&letter) { TypeSpec::Integer(None) } else { TypeSpec::Real };
    for s in implicit {
        let Stmt::Implicit(rule) = s else { continue };
        let rules: Vec<(&TypeSpec, &[crate::ast::LetterRange])> = match rule {
            Implicit::None => return None,
            Implicit::Rule { type_spec, letter_ranges } => vec![(type_spec, letter_ranges)],
            Implicit::Rules { rules } => rules.iter().map(|r| (&r.type_spec, r.letter_ranges.as_slice())).collect(),
        };
        for (type_spec, ranges) in rules {
            if ranges.iter().any(|r| (r.start.to_ascii_lowercase()..=r.end.unwrap_or(r.start).to_ascii_lowercase()).contains(&letter)) {
                kind = type_spec.clone();
            }
        }
    }
    Some(kind)
}
/// The source offset of the statement declaring `name` in a host body.
fn declaration_offset(body: &[Stmt], name: &str) -> Option<usize> {
    let mut at = None;
    for stmt in body {
        match stmt {
            Stmt::SourceLocation(p) => at = Some(*p),
            Stmt::VarDecl { names, .. } if names.iter().any(|n| n == name) => return at,
            Stmt::ArrayDecl { name: n, .. } if n == name => return at,
            _ => {}
        }
    }
    None
}
fn procedure(
    body: &mut Vec<Stmt>,
    parent_params: &[String],
    serial: &mut usize,
    lifted: &mut Vec<Stmt>,
    name_capacity: usize,
    inherited_implicit: &[Stmt],
) -> Result<()> {
    let own_implicit = body
        .iter()
        .filter(|s| matches!(s, Stmt::Implicit(_)))
        .cloned()
        .collect::<Vec<_>>();
    let implicit = if own_implicit.is_empty() {
        inherited_implicit.to_vec()
    } else {
        own_implicit
    };
    let mut host = declarations(body);
    // Until implicit host typing is lowered explicitly, reject captures of
    // those entities rather than treating writes as helper-local variables.
    fn definitions(body: &[Stmt], names: &mut HashSet<String>) {
        for stmt in body {
            match stmt {
                Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => {
                    names.insert(name.clone());
                }
                Stmt::Do { var, body, .. } => {
                    names.insert(var.clone());
                    definitions(body, names);
                }
                Stmt::DoWhile { body, .. } | Stmt::Block { body } => definitions(body, names),
                // READ destinations.
                Stmt::CallSub { name, args } => {
                    if let Some(start) = crate::input::transfer_start(name) {
                        for a in args.iter().skip(start) {
                            if let Expr::Ident(n) = a { names.insert(n.clone()); }
                        }
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    definitions(then_body, names);
                    if let Some(b) = else_body {
                        definitions(b, names);
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        definitions(&c.body, names);
                    }
                    if let Some(b) = default {
                        definitions(b, names);
                    }
                }
                _ => {}
            }
        }
    }
    let mut defined = parent_params.iter().cloned().collect();
    definitions(body, &mut defined);
    for name in defined {
        host.entry(name).or_insert_with(|| Variable {
            kind: TypeSpec::Integer(None),
            rank: 0,
            attributes: HashSet::from(["@implicit_host".into()]),
        });
    }
    let mut helpers = vec![];
    body.retain(|stmt| {
        if matches!(stmt, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
            helpers.push(stmt.clone());
            false
        } else {
            true
        }
    });
    if helpers.is_empty() {
        return Ok(());
    }
    let mut used = HashSet::new();
    let mut procedures = HashMap::new();
    for helper in &helpers {
        let (name, params, body) = match helper {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => (name, params, body),
            _ => unreachable!(),
        };
        if body
            .iter()
            .any(|s| matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }))
        {
            bail!("internal procedures inside another internal procedure are not yet supported");
        }
        if let Stmt::Function { return_type, .. } = helper {
            let result = match body.last() {
                Some(Stmt::Return(Some(Expr::Ident(result)))) => result.as_str(),
                _ => name.as_str(),
            };
            if return_type.is_none() && !declarations(body).contains_key(result) {
                bail!("internal function `{name}` currently requires an explicitly declared result type");
            }
        }
        references(body, &params.iter().cloned().collect(), &host, &mut used);
        *serial += 1;
        let mangled = format!("@internal_{}_{}", serial, name);
        if procedures
            .insert(
                name.clone(),
                Procedure {
                    name: mangled,
                    signature: Signature::new(params, body),
                    inputs: body
                        .iter()
                        .filter_map(|s| match s {
                            Stmt::DummyIntent { name, intent } if intent == "intent(in)" => {
                                Some(name.clone())
                            }
                            _ => None,
                        })
                        .collect(),
                },
            )
            .is_some()
        {
            bail!("duplicate internal procedure `{name}`");
        }
    }
    let mut captures = BTreeMap::new();
    let mut bindings = HashMap::new();
    let mut prefix = vec![];
    // An implicitly typed host variable that an internal procedure uses gets
    // an explicit declaration of its implicit type, so it is captured like
    // any declared variable.
    let mut implicit_declarations = vec![];
    for (name, v) in host.iter_mut().filter(|(n, _)| used.contains(*n)) {
        if !v.attributes.remove("@implicit_host") { continue; }
        let Some(kind) = implicit_type(&implicit, name) else {
            bail!("host-associated variable `{name}` has no type under IMPLICIT NONE");
        };
        v.kind = kind.clone();
        implicit_declarations.push(Stmt::VarDecl { kind, names: vec![name.clone()] });
    }
    if !implicit_declarations.is_empty() {
        let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::Implicit(_) | Stmt::SourceLocation(_))).unwrap_or(body.len());
        body.splice(at..at, implicit_declarations);
    }
    for (name, v) in host.iter().filter(|(n, _)| used.contains(*n)) {
        let declared = declaration_offset(body, name);
        if !matches!(
            v.kind,
            TypeSpec::Integer(None | Some(4) | Some(8))
                | TypeSpec::Real
                | TypeSpec::DoublePrecision
                | TypeSpec::Logical
        ) || v
            .attributes
            .iter()
            .any(|a| matches!(a.as_str(), "pointer" | "allocatable" | "optional"))
        {
            return Err(crate::source_diagnostics::at(anyhow::anyhow!("host association for `{name}` currently supports INTEGER(4/8), REAL(4/8), or LOGICAL storage without OPTIONAL, POINTER, or ALLOCATABLE"), declared));
        }
        if v.rank > 15 {
            bail!("host-associated arrays support rank at most 15");
        }
        *serial += 1;
        let stem = format!("@host_capture_{}", serial);
        let c = Capture {
            variable: v.clone(),
            data: format!("{stem}_data"),
            flags: format!("{stem}_flags"),
            label: format!("{stem}_name"),
            axes: (1..=v.rank)
                .map(|d| (format!("{stem}_lower_{d}"), format!("{stem}_extent_{d}")))
                .collect(),
        };
        prefix.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: c.fields(),
        });
        prefix.push(Stmt::DummyIntent {
            name: name.clone(),
            intent: "target".into(),
        });
        let mut binding = vec![Stmt::CallSub {
            name: "@ptr_bind".into(),
            args: vec![id(&c.data), id(&c.flags), id(&c.label), id(name)],
        }];
        for (axis, (lower, extent)) in c.axes.iter().enumerate() {
            binding.push(set(
                lower,
                call("lbound", vec![id(name), int(axis as i64 + 1)]),
            ));
            binding.push(set(
                extent,
                call("size", vec![id(name), int(axis as i64 + 1)]),
            ));
        }
        bindings.insert(name.clone(), binding);
        captures.insert(name.clone(), c);
    }
    // Rewrite host calls while leaving its own data variables as real storage.
    let host_locals = host.keys().cloned().collect();
    Rewrite {
        captures: &captures,
        procedures: &procedures,
        serial,
        name_capacity,
    }
    .body(body, &host_locals)?;
    let mut statement_location = None;
    for stmt in std::mem::take(body) {
        if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location = Some(p); }
        (|| -> anyhow::Result<()> {
        let declared = match &stmt {
            Stmt::VarDecl { names, .. } => names.clone(),
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            _ => vec![],
        };
        prefix.push(stmt);
        for n in declared {
            if let Some(binding) = bindings.remove(&n) {
                prefix.extend(binding);
            }
        }
            Ok(())
        })().map_err(|e| crate::source_diagnostics::at(e, statement_location))?;
    }
    *body = prefix;
    for mut helper in helpers {
        if let Stmt::Function {
            name,
            params,
            body,
            return_type,
            ..
        } = &mut helper
        {
            if body
                .iter()
                .any(|s| matches!(s,Stmt::VarDecl {names,..} if names.contains(name)))
                && !matches!(body.last(), Some(Stmt::Return(Some(_))))
            {
                let normalized = crate::ast::function_with_result(
                    name.clone(),
                    params.clone(),
                    name.clone(),
                    std::mem::take(body),
                );
                let Stmt::Function {
                    body: normalized,
                    return_type: normalized_type,
                    ..
                } = normalized
                else {
                    unreachable!()
                };
                *body = normalized;
                if return_type.is_none() {
                    *return_type = normalized_type;
                }
            }
        }
        let (name, params, body) = match &mut helper {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => (name, params, body),
            _ => unreachable!(),
        };
        let original = name.clone();
        if !body.iter().any(|s| matches!(s, Stmt::Implicit(_))) {
            body.splice(0..0, implicit.clone());
        }
        // The result variable keeps its source name when the procedure is lifted.
        let fields = captures
            .values()
            .flat_map(Capture::fields)
            .collect::<Vec<_>>();
        Rewrite {
            captures: &captures,
            procedures: &procedures,
            serial,
            name_capacity,
        }
        .body(body, &params.iter().cloned().collect())?;
        hoist(body);
        *name = procedures[&original].name.clone();
        params.extend(fields.clone());
        let mut declarations = vec![Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: fields.clone(),
        }];
        declarations.extend(fields.into_iter().map(|name| Stmt::DummyIntent {
            name,
            intent: "intent(in)".into(),
        }));
        declarations.append(body);
        *body = declarations;
        lifted.push(helper);
    }
    Ok(())
}
// Names used as values rather than called, such as procedure actual arguments.
fn bare_names(body: &[Stmt], out: &mut HashSet<String>) {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => { out.insert(n.to_ascii_lowercase()); }
            Expr::Call(_, args) | Expr::Index(_, args) => for a in args { expr(a, out); },
            Expr::Bin(_, a, b) => { expr(a, out); expr(b, out); }
            Expr::Un(_, a) => expr(a, out),
            _ => {}
        }
    }
    let mut body = body.to_vec();
    for s in &mut body {
        for e in crate::procedure_arguments::expressions(s) { expr(e, out); }
        for child in crate::procedure_arguments::children(s) { bare_names(child, out); }
    }
}

// Every name that a body uses as a variable, array or procedure.
fn names_in(body: &[Stmt], out: &mut HashSet<String>) {
    fn expr(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(n) => { out.insert(n.to_ascii_lowercase()); }
            Expr::Call(n, args) | Expr::Index(n, args) => {
                out.insert(n.to_ascii_lowercase());
                for a in args { expr(a, out); }
            }
            Expr::Bin(_, a, b) => { expr(a, out); expr(b, out); }
            Expr::Un(_, a) => expr(a, out),
            Expr::Section { name, lower, upper, stride } => {
                out.insert(name.to_ascii_lowercase());
                for e in [lower, upper, stride].into_iter().flatten() { expr(e, out); }
            }
            _ => {}
        }
    }
    let mut body = body.to_vec();
    for s in &mut body {
        if let Stmt::CallSub { name, .. } = s { out.insert(name.to_ascii_lowercase()); }
        for e in crate::procedure_arguments::expressions(s) { expr(e, out); }
        for child in crate::procedure_arguments::children(s) { names_in(child, out); }
    }
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    fn maximum(body: &[Stmt]) -> usize {
        let own = declarations(body)
            .keys()
            .map(String::len)
            .max()
            .unwrap_or(0);
        body.iter().fold(own, |m, s| {
            m.max(match s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => maximum(body),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => maximum(then_body).max(else_body.as_ref().map_or(0, |b| maximum(b))),
                _ => 0,
            })
        })
    }
    fn units(
        body: &mut [Stmt],
        serial: &mut usize,
        lifted: &mut Vec<Stmt>,
        capacity: usize,
        implicit: &[Stmt],
    ) -> Result<()> {
        for stmt in body {
            match stmt {
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    procedure(body, params, serial, lifted, capacity, implicit)?
                }
                Stmt::Module { body, .. } => {
                    let own = body
                        .iter()
                        .filter(|s| matches!(s, Stmt::Implicit(_)))
                        .cloned()
                        .collect::<Vec<_>>();
                    units(
                        body,
                        serial,
                        lifted,
                        capacity,
                        if own.is_empty() { implicit } else { &own },
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    let capacity = maximum(&p.body).saturating_add(128);
    let mut lifted = vec![];
    let mut serial = 0;
    if !p.contains.is_empty() {
        // The main program is the host of the procedures it CONTAINS: gather
        // its statements and those procedures into one host body.
        // Procedures with array, CHARACTER or POINTER dummies, and ELEMENTAL
        // procedures, are expanded at their call sites later, where they see
        // the main program's variables directly. So are the procedures they
        // refer to.
        let expanded = |s: &Stmt| match s {
            Stmt::Function { params, body, elemental, .. } | Stmt::Subroutine { params, body, elemental, .. } => *elemental
                || body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if intent == "pointer" && params.contains(name))
                    || matches!(s, Stmt::ArrayDecl { name, .. } if params.contains(name))
                    || matches!(s, Stmt::VarDecl { kind: TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_), names } if names.iter().any(|n| params.contains(n)))),
            _ => false,
        };
        let mut lifted_names: HashSet<String> = p.body.iter().filter_map(|s| match s {
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } if p.contains.contains(&name.to_ascii_lowercase()) && !expanded(s) => Some(name.to_ascii_lowercase()),
            _ => None,
        }).collect();
        let mut referenced = HashSet::new();
        for s in &p.body {
            if let Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } = s {
                if p.contains.contains(&name.to_ascii_lowercase()) && expanded(s) {
                    names_in(body, &mut referenced);
                }
            }
        }
        // A procedure passed as an actual argument keeps its name, as does
        // one that another program unit refers to, such as a specialized copy
        // of a module procedure that received it as an actual argument.
        let mut actuals = HashSet::new();
        for s in &p.body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } if p.contains.contains(&name.to_ascii_lowercase()) => {
                    let mut own = HashSet::new();
                    bare_names(body, &mut own);
                    own.remove(&name.to_ascii_lowercase());
                    actuals.extend(own);
                }
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => names_in(body, &mut referenced),
                s => bare_names(std::slice::from_ref(s), &mut actuals),
            }
        }
        lifted_names.retain(|n| !referenced.contains(n) && !actuals.contains(n));
        if lifted_names.is_empty() { p.contains.clear(); }
        let internal = |s: &Stmt| matches!(s, Stmt::Function { name, .. } | Stmt::Subroutine { name, .. }
            if lifted_names.contains(&name.to_ascii_lowercase()));
        let unit = |s: &Stmt| matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. });
        if !p.contains.is_empty() {
        let (mut host, others): (Vec<Stmt>, Vec<Stmt>) = std::mem::take(&mut p.body).into_iter().partition(|s| internal(s) || !unit(s));
        procedure(&mut host, &[], &mut serial, &mut lifted, capacity, &[])?;
        p.body = others;
        p.body.extend(host);
        p.contains.clear();
        }
    }
    units(&mut p.body, &mut serial, &mut lifted, capacity, &[])?;
    p.body.extend(lifted);
    Ok(p)
}

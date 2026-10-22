// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize local OPTIONAL procedures by associated argument presence.
//! Values remain runtime arguments; no optional-argument descriptor ABI is used.
use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

type Names = HashSet<String>;
fn optionals(body: &[Stmt]) -> Names {
    body.iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "optional" => Some(name.clone()),
            _ => None,
        })
        .collect()
}
// Preserve source-level use information when a presence variant discards the
// only branch which reads a parameter. This metadata affects warnings only.
fn parameter_used(body: &[Stmt],param:&str)->bool {
    fn expr(e:&Expr,param:&str)->bool {match e {
        Expr::Ident(n)=>n==param,
        Expr::Call(n,args)|Expr::Index(n,args)=>n==param||args.iter().any(|e|expr(e,param)),
        Expr::Section{name,lower,upper,stride}=>name==param||[lower,upper,stride].into_iter().flatten().any(|e|expr(e,param)),
        Expr::Bin(_,a,b)=>expr(a,param)||expr(b,param),Expr::Un(_,a)=>expr(a,param),_=>false
    }}
    body.iter().any(|s|match s {
        Stmt::Assign{value,..}|Stmt::Return(Some(value))=>expr(value,param),
        Stmt::AssignIndex{indices,value,..}=>expr(value,param)||indices.iter().any(|e|expr(e,param)),
        Stmt::Print{items}|Stmt::CallSub{args:items,..}=>items.iter().any(|e|expr(e,param)),
        Stmt::If{cond,then_body,else_body}=>expr(cond,param)||parameter_used(then_body,param)||else_body.as_ref().is_some_and(|b|parameter_used(b,param)),
        Stmt::Do{start,end,body,..}=>expr(start,param)||expr(end,param)||parameter_used(body,param),
        Stmt::DoWhile{cond,body}=>expr(cond,param)||parameter_used(body,param),
        Stmt::Block{body}=>parameter_used(body,param),
        Stmt::SelectCase{expr:e,cases,default}=>expr(e,param)||cases.iter().any(|c|parameter_used(&c.body,param))||default.as_ref().is_some_and(|b|parameter_used(b,param)),
        _=>false
    })
}
fn collect(body: &mut Vec<Stmt>, templates: &mut HashMap<String, Stmt>) {
    body.retain_mut(|s| match s {
        Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. }
            if !optionals(body).is_empty() =>
        {
            templates.insert(name.clone(), s.clone());
            false
        }
        Stmt::Module { body, .. } => {
            collect(body, templates);
            true
        }
        _ => true,
    });
}
struct Specializer {
    templates: HashMap<String, Stmt>,
    cache: HashMap<(String, Vec<bool>), String>,
    generated: Vec<Stmt>,
    // The nesting of the body being specialized, 1 at a procedure's top
    // level, and whether a top-level statement before the current one may
    // leave the procedure.
    nesting: usize,
    left: bool,
}
/// Whether a statement contains RETURN, STOP or ERROR STOP.
fn leaves(s: &Stmt) -> bool {
    serde_json::to_string(s).is_ok_and(|t| t.contains("\"Return\"") || t.contains("\"@stop\"") || t.contains("\"@error_stop\""))
}
impl Specializer {
    fn call(
        &mut self,
        name: &mut String,
        args: &mut Vec<Expr>,
        absent: &Names,
        optional: &Names,
    ) -> Result<()> {
        let Some(mut template) = self.templates.get(name).cloned() else {
            for arg in args {
                self.expr(arg, absent, optional)?;
            }
            return Ok(());
        };
        let function = matches!(template, Stmt::Function { .. });
        let (params, body) = match &mut template {
            Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                (params, body)
            }
            _ => unreachable!(),
        };
        if function && params.iter().any(|p| !body.iter().any(|s| matches!(s,Stmt::DummyIntent {name,intent} if name == p && intent == "intent(in)"))) {
            bail!("OPTIONAL function specialization currently requires INTENT(IN) dummy arguments");
        }
        let callee_optional = optionals(body);
        if callee_optional.iter().any(|n| !params.contains(n)) {
            bail!("OPTIONAL attribute requires a dummy argument");
        }
        if args.len() > params.len() {
            bail!("too many arguments to `{name}`");
        }
        let mut missing = Names::new();
        let mut mask = Vec::new();
        let mut actuals = Vec::new();
        for (i, param) in params.iter().enumerate() {
            let supplied = args
                .get(i)
                .filter(|e| !crate::named_arguments::omitted(e)
                    && !matches!(e,Expr::Ident(n) if absent.contains(n)));
            mask.push(supplied.is_some());
            if let Some(actual) = supplied {
                if matches!(actual,Expr::Call(n,_) if n.starts_with("@keyword:")) {
                    bail!("OPTIONAL specialization currently requires positional arguments");
                }
                let mut actual = actual.clone();
                self.expr(&mut actual, absent, optional)?;
                actuals.push(actual);
            } else {
                if !callee_optional.contains(param) {
                    bail!("missing required argument `{param}` of `{name}`");
                }
                missing.insert(param.clone());
            }
        }
        let key = (name.clone(), mask);
        if let Some(variant) = self.cache.get(&key) {
            *name = variant.clone();
            *args = actuals;
            return Ok(());
        }
        let variant = format!("@optional_{}", self.cache.len());
        // Register before visiting calls: recursive presence variants form a
        // finite graph and reuse this entry rather than expanding indefinitely.
        self.cache.insert(key, variant.clone());
        let original_reads:Vec<_>=params.iter().filter(|p|!missing.contains(*p)&&parameter_used(body,p)).cloned().collect();
        params.retain(|n| !missing.contains(n));
        for stmt in body.iter_mut() {
            if let Stmt::VarDecl { names, .. } = stmt {
                names.retain(|n| !missing.contains(n));
            }
        }
        body.retain(|s| !matches!(s,Stmt::VarDecl {names,..} if names.is_empty())
            && !matches!(s,Stmt::ArrayDecl {name,..} if missing.contains(name))
            && !matches!(s,Stmt::DummyIntent {name,intent} if missing.contains(name) || intent == "optional"));
        if function {for param in original_reads {body.insert(0,Stmt::DummyIntent{name:param,intent:"@optional_source_read".into()});}}
        let result = match &template {
            Stmt::Function { body, .. } => Some(match body.last() {
                Some(Stmt::Return(Some(e))) => e.clone(),
                _ => Expr::Ident(name.clone()),
            }),
            _ => None,
        };
        match &mut template {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                *name = variant.clone();
                let saved = (self.nesting, self.left);
                (self.nesting, self.left) = (0, false);
                let specialized = self.body(body, &missing, &callee_optional, result.as_ref());
                (self.nesting, self.left) = saved;
                specialized?;
                if let Some(result) = result {
                    body.push(Stmt::Return(Some(result)));
                }
            }
            _ => unreachable!(),
        }
        self.generated.push(template);
        *name = variant;
        *args = actuals;
        Ok(())
    }
    fn expr(&mut self, e: &mut Expr, absent: &Names, optional: &Names) -> Result<()> {
        match e {
            Expr::Ident(n) if absent.contains(n) => {
                bail!("reference to absent OPTIONAL dummy `{n}`")
            }
            Expr::Call(n, args) if n == "present" => {
                let [Expr::Ident(p)] = args.as_slice() else {
                    bail!("PRESENT requires an OPTIONAL dummy argument");
                };
                if !optional.contains(p) {
                    bail!("PRESENT requires an OPTIONAL dummy argument");
                }
                *e = Expr::Logical(!absent.contains(p));
            }
            Expr::Call(n, args) => {
                if absent.contains(n) {
                    bail!("reference to absent OPTIONAL dummy `{n}`");
                }
                self.call(n, args, absent, optional)?;
            }
            Expr::Index(n, args) => {
                if absent.contains(n) {
                    bail!("reference to absent OPTIONAL dummy `{n}`");
                }
                for a in args {
                    self.expr(a, absent, optional)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if absent.contains(name) {
                    bail!("reference to absent OPTIONAL dummy `{name}`");
                }
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expr(a, absent, optional)?;
                }
            }
            Expr::Bin(op, a, b) => {
                self.expr(a, absent, optional)?;
                self.expr(b, absent, optional)?;
                if let (Expr::Logical(a), Expr::Logical(b)) = (a.as_ref(), b.as_ref()) {
                    let value = match op {
                        BinOp::And => Some(*a && *b),
                        BinOp::Or => Some(*a || *b),
                        BinOp::Eqv => Some(a == b),
                        BinOp::Neqv => Some(a != b),
                        _ => None,
                    };
                    if let Some(value) = value {
                        *e = Expr::Logical(value);
                    }
                }
            }
            Expr::Un(op, a) => {
                self.expr(a, absent, optional)?;
                if let (UnOp::Not, Expr::Logical(value)) = (op, a.as_ref()) {
                    *e = Expr::Logical(!value);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(
        &mut self,
        body: &mut Vec<Stmt>,
        absent: &Names,
        optional: &Names,
        result: Option<&Expr>,
    ) -> Result<()> {
        self.nesting += 1;
        let outcome = self.statements(body, absent, optional, result);
        self.nesting -= 1;
        outcome
    }
    fn statements(
        &mut self,
        body: &mut Vec<Stmt>,
        absent: &Names,
        optional: &Names,
        result: Option<&Expr>,
    ) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut output = Vec::new();
        for mut stmt in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location.set(Some(p)); }
            // Some(true) ends the body after a chosen branch that returns,
            // Some(false) continues with the next statement.
            let flow = (|| -> Result<Option<bool>> {
            match &mut stmt {
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(cond, absent, optional)?;
                    if let Expr::Logical(value) = cond {
                        let mut chosen = if *value {
                            std::mem::take(then_body)
                        } else {
                            else_body.take().unwrap_or_default()
                        };
                        // The chosen branch always executes.
                        self.statements(&mut chosen, absent, optional, result)?;
                        let returns = matches!(chosen.last(), Some(Stmt::Return(_)));
                        output.extend(chosen);
                        return Ok(Some(returns));
                    }
                    self.body(then_body, absent, optional, result)?;
                    if let Some(b) = else_body {
                        self.body(b, absent, optional, result)?;
                    }
                }
                Stmt::CallSub { name, args } => self.call(name, args, absent, optional)?,
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        self.expr(e, absent, optional)?;
                    }
                }
                Stmt::Assign { name, value } => {
                    if absent.contains(name) {
                        bail!("assignment to absent OPTIONAL dummy `{name}`");
                    }
                    self.expr(value, absent, optional)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    if absent.contains(name) {
                        bail!("assignment to absent OPTIONAL dummy `{name}`");
                    }
                    for e in indices {
                        self.expr(e, absent, optional)?;
                    }
                    self.expr(value, absent, optional)?;
                }
                Stmt::ArrayDecl { dims, .. } => {
                    for e in dims {
                        self.expr(e, absent, optional)?;
                    }
                }
                Stmt::Return(e) => {
                    if e.is_none() {
                        *e = result.cloned();
                    }
                    if let Some(e) = e {
                        self.expr(e, absent, optional)?;
                    }
                }
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    if absent.contains(var) {
                        bail!("assignment to absent OPTIONAL dummy `{var}`");
                    }
                    self.expr(start, absent, optional)?;
                    self.expr(end, absent, optional)?;
                    self.body(body, absent, optional, result)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(cond, absent, optional)?;
                    self.body(body, absent, optional, result)?;
                }
                Stmt::Block { body } => self.body(body, absent, optional, result)?,
                Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } => {
                    self.body(body, &Names::new(), &Names::new(), None)?
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(expr, absent, optional)?;
                    for case in cases {
                        self.body(&mut case.body, absent, optional, result)?;
                    }
                    if let Some(body) = default {
                        self.body(body, absent, optional, result)?;
                    }
                }
                _ => {}
            }
            Ok(None)
            })();
            match flow {
                Ok(Some(true)) => break,
                Ok(Some(false)) => continue,
                Ok(None) => {}
                // Referencing an absent dummy is an error only when the
                // statement executes, as under `if (n > 0)` with `n` set
                // only when the argument is present. A reference that
                // always executes is diagnosed at compile time.
                Err(e) if e.to_string().contains("absent OPTIONAL dummy") && (self.nesting > 1 || self.left) => {
                    stmt = Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str(e.to_string())] };
                }
                Err(e) => return Err(e),
            }
            let returns = matches!(stmt, Stmt::Return(_));
            if self.nesting == 1 && leaves(&stmt) { self.left = true; }
            output.push(stmt);
            if returns {
                break;
            }
        }
        *body = output;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
pub fn resolve(mut program: Program) -> Result<Program> {
    let mut pass = Specializer {
        templates: HashMap::new(),
        cache: HashMap::new(),
        generated: vec![],
        nesting: 0,
        left: false,
    };
    collect(&mut program.body, &mut pass.templates);
    if pass.templates.is_empty() {
        return Ok(program);
    }
    pass.body(&mut program.body, &Names::new(), &Names::new(), None)?;
    program.body.extend(pass.generated);
    Ok(program)
}

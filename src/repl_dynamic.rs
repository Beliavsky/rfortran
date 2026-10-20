// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! REPL-owned allocation and optional declaration inference. Each assignment
//! gets fixed backing storage for this transaction; old storage remains alive
//! while its RHS is evaluated. No shared interpreter heap changes are needed.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone)]
pub(super) struct Allocation {
    pub kind: TypeSpec,
    pub extent: Option<i64>,
}
pub(super) type Allocations = BTreeMap<String, Allocation>;

pub(super) struct Prepared {
    pub program: Program,
    pub allocations: Allocations,
    pub inferred: Vec<Stmt>,
    pub bindings: HashMap<String, String>,
}

fn declaration(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::VarDecl { .. }
            | Stmt::ArrayDecl { .. }
            | Stmt::InitializedDecl { .. }
            | Stmt::DummyIntent { .. }
            | Stmt::Implicit(_)
    )
}

/// Normalize kinds/constants with the compiler, then reuse its array-expression
/// materializer for element type and extent. The probe never executes source.
fn infer(value: &Expr, headers: &[Stmt]) -> Result<(TypeSpec, Option<i64>)> {
    let mut body: Vec<_> = headers.iter().filter(|s| declaration(s)).cloned().collect();
    body.push(Stmt::Print {
        items: vec![value.clone()],
    });
    let probe = crate::parameters::resolve(Program {
        name: "<repl-type>".into(),
        body,
        contains: vec![],
    })?;
    let mut shapes = HashMap::new();
    for stmt in &probe.body {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                for name in names {
                    shapes.insert(name.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            _ => {}
        }
    }
    let value = probe
        .body
        .iter()
        .rev()
        .find_map(|s| match s {
            Stmt::Print { items } => items.first(),
            _ => None,
        })
        .context("missing REPL type probe")?;
    let shape = crate::multidim::fixed_actual_shape(value, &shapes)?;
    if shape.as_ref().is_some_and(|dims| dims.len() != 1) {
        bail!("automatic and allocatable REPL arrays currently require rank one");
    }
    if shape.is_some() {
        let (kind, count, _) =
            crate::reductions::argument(&mut value.clone(), &shapes, &mut vec![], &mut 0)?;
        if !(0..=10_000).contains(&count) {
            bail!("REPL allocation requires a known extent of at most 10000 elements");
        }
        super::arrays::kind(&kind)?;
        Ok((kind, Some(count)))
    } else {
        let scalar_kind = crate::array_calls::scalar_actual_kind(value, &shapes).or_else(|_| {
            let (_, extent, kind) =
                crate::reductions::elemental_element(value, &Expr::IntLit("1".into()), &shapes)?;
            if extent.is_some() {
                bail!("scalar REPL inference received an array expression");
            }
            Ok(kind)
        });
        match scalar_kind {
            Ok(kind) => Ok((kind, None)),
            Err(_) => {
                // Reductions and intrinsic calls may introduce typed scalar
                // temporaries. Ask the compiler to lower the scalar probe.
                let lowered = crate::reductions::resolve(probe)?;
                let mut shapes = HashMap::new();
                for stmt in &lowered.body {
                    match stmt {
                        Stmt::VarDecl { kind, names } => {
                            for n in names {
                                shapes.insert(n.clone(), (kind.clone(), vec![]));
                            }
                        }
                        Stmt::ArrayDecl { kind, name, dims } => {
                            shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                        }
                        _ => {}
                    }
                }
                let value = lowered
                    .body
                    .iter()
                    .rev()
                    .find_map(|s| match s {
                        Stmt::Print { items } => items.first(),
                        _ => None,
                    })
                    .context("missing lowered REPL type probe")?;
                Ok((
                    crate::array_calls::scalar_actual_kind(value, &shapes)?,
                    None,
                ))
            }
        }
    }
}

struct Lower {
    allocations: Allocations,
    bindings: HashMap<String, String>,
    headers: Vec<Stmt>,
    known: HashSet<String>,
    inferred: Vec<Stmt>,
    serial: usize,
    auto: bool,
}
impl Lower {
    fn target(&self, name: &str) -> Result<String> {
        if self
            .allocations
            .get(name)
            .is_some_and(|a| a.extent.is_none())
        {
            bail!("allocatable array `{name}` is not allocated");
        }
        Ok(self
            .bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.into()))
    }
    fn expr(&self, e: &mut Expr) -> Result<()> {
        if let Expr::Call(name, args) = e {
            if name == "allocated" {
                let [argument] = args.as_slice() else {
                    bail!("ALLOCATED requires one allocatable array argument");
                };
                let argument = match argument {
                    Expr::Call(keyword, values)
                        if keyword == "@keyword:array" && values.len() == 1 =>
                    {
                        &values[0]
                    }
                    other => other,
                };
                let Expr::Ident(name) = argument else {
                    bail!("ALLOCATED requires an allocatable array name (or ARRAY=name)");
                };
                let allocation = self
                    .allocations
                    .get(name)
                    .with_context(|| format!("`{name}` is not an allocatable REPL array"))?;
                *e = Expr::Logical(allocation.extent.is_some());
                return Ok(());
            }
            if name == "kind"
                && matches!(args.as_slice(), [Expr::Ident(n)] if self.allocations.get(n).is_some_and(|a|a.extent.is_none()))
            {
                return Ok(()); // KIND inquires about the declaration, not allocation.
            }
        }
        match e {
            Expr::Ident(n) => *n = self.target(n)?,
            Expr::Index(n, args) | Expr::Call(n, args) => {
                if self.allocations.contains_key(n) {
                    *n = self.target(n)?;
                }
                for arg in args {
                    self.expr(arg)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                *name = self.target(name)?;
                for bound in [lower, upper, stride].into_iter().flatten() {
                    self.expr(bound)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a)?;
                self.expr(b)?;
            }
            Expr::Un(_, a) => self.expr(a)?,
            _ => {}
        }
        Ok(())
    }
    fn storage(&mut self, name: &str, count: i64, out: &mut Vec<Stmt>) -> String {
        self.serial += 1;
        let storage = format!("@repl_alloc_{}", self.serial);
        let allocation = self.allocations.get_mut(name).unwrap();
        allocation.extent = Some(count);
        let decl = Stmt::ArrayDecl {
            kind: allocation.kind.clone(),
            name: storage.clone(),
            dims: vec![Expr::IntLit(count.to_string())],
        };
        self.headers.push(decl.clone());
        out.push(decl);
        self.bindings.insert(name.into(), storage.clone());
        storage
    }
    fn allocation(&mut self, name: &str, value: &Expr, out: &mut Vec<Stmt>) -> Result<String> {
        let (kind, extent) = infer(value, &self.headers)?;
        let numeric = |kind: &TypeSpec| {
            matches!(
                kind,
                TypeSpec::Integer(_)
                    | TypeSpec::IntegerKind(_)
                    | TypeSpec::Real
                    | TypeSpec::RealKind(_)
                    | TypeSpec::DoublePrecision
            )
        };
        let destination = &self.allocations[name].kind;
        if !(numeric(destination) && numeric(&kind)
            || matches!(destination, TypeSpec::Logical | TypeSpec::LogicalKind(_))
                && matches!(kind, TypeSpec::Logical | TypeSpec::LogicalKind(_)))
        {
            bail!("type mismatch in assignment to allocatable array `{name}`");
        }
        match extent {
            Some(count) => Ok(self.storage(name, count, out)),
            None => self.target(name), // Scalar expansion requires existing allocation.
        }
    }
    fn statements(&mut self, body: Vec<Stmt>, top: bool) -> Result<Vec<Stmt>> {
        let mut out = vec![];
        for mut s in body {
            match &mut s {
                Stmt::DummyIntent { name, intent }
                    if intent == "allocatable" && self.allocations.contains_key(name) =>
                {
                    continue
                }
                Stmt::ArrayDecl { name, kind, dims } if self.allocations.contains_key(name) => {
                    let a = &self.allocations[name];
                    *kind = a.kind.clone();
                    *dims = vec![Expr::IntLit(a.extent.unwrap_or(0).to_string())];
                }
                Stmt::Assign { name, value } => {
                    self.expr(value)?;
                    if !self.known.contains(name) && !(self.auto && top) {
                        bail!("undefined variable `{name}`; declare it first or enable :auto on for top-level assignments");
                    }
                    if !self.known.contains(name) && self.auto && top {
                        if name.starts_with("rfort_repl_snapshot_") {
                            bail!("name prefix `rfort_repl_snapshot_` is reserved by the REPL");
                        }
                        let (kind, extent) = infer(value, &self.headers)?;
                        super::scalar(&kind)?;
                        let decl = match extent {
                            Some(_) => {
                                self.allocations.insert(
                                    name.clone(),
                                    Allocation {
                                        kind: kind.clone(),
                                        extent: None,
                                    },
                                );
                                self.inferred.push(Stmt::DummyIntent {
                                    name: name.clone(),
                                    intent: "allocatable".into(),
                                });
                                Stmt::ArrayDecl {
                                    kind,
                                    name: name.clone(),
                                    dims: vec![Expr::Ident("@deferred_extent".into())],
                                }
                            }
                            None => Stmt::VarDecl {
                                kind,
                                names: vec![name.clone()],
                            },
                        };
                        self.inferred.push(decl.clone());
                        self.known.insert(name.clone());
                        if extent.is_none() {
                            self.headers.push(decl.clone());
                            out.push(decl);
                        }
                    }
                    if self.allocations.contains_key(name) {
                        if !top {
                            bail!("whole allocatable-array assignment in a REPL block is not yet supported; assign at the prompt");
                        }
                        *name = self.allocation(name, value, &mut out)?;
                    }
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.expr(value)?;
                    for index in indices {
                        self.expr(index)?;
                    }
                    *name = self.target(name)?;
                }
                Stmt::CallSub { name, args } if name == "@allocate" || name == "@deallocate" => {
                    if !top {
                        bail!("REPL ALLOCATE/DEALLOCATE currently require a top-level statement");
                    }
                    for arg in args {
                        if name == "@deallocate" {
                            let Expr::Ident(n) = arg else {
                                bail!("REPL DEALLOCATE currently accepts array names only");
                            };
                            let a = self
                                .allocations
                                .get_mut(n)
                                .with_context(|| format!("`{n}` is not allocatable"))?;
                            if a.extent.is_none() {
                                bail!("allocatable array `{n}` is not allocated");
                            }
                            a.extent = None;
                            self.bindings.remove(n);
                        } else {
                            let Expr::Call(n, dims) = arg else {
                                bail!("REPL ALLOCATE requires an array extent");
                            };
                            let a = self
                                .allocations
                                .get(n)
                                .with_context(|| format!("`{n}` is not allocatable"))?;
                            if a.extent.is_some() {
                                bail!("allocatable array `{n}` is already allocated");
                            }
                            if dims.len() != 1 {
                                bail!("REPL ALLOCATE currently requires rank one");
                            }
                            // Resolve named constants through the same type probe.
                            let mut probe_body: Vec<_> = self
                                .headers
                                .iter()
                                .filter(|s| declaration(s))
                                .cloned()
                                .collect();
                            probe_body.push(Stmt::Print {
                                items: dims.clone(),
                            });
                            let probe = crate::parameters::resolve(Program {
                                name: "<repl-allocate>".into(),
                                body: probe_body,
                                contains: vec![],
                            })?;
                            let bounds = probe
                                .body
                                .iter()
                                .rev()
                                .find_map(|s| match s {
                                    Stmt::Print { items } => Some(items),
                                    _ => None,
                                })
                                .unwrap();
                            let count = crate::parameters::integer_constant(&bounds[0]).context("REPL ALLOCATE extent currently requires a constant integer expression")?.max(0);
                            if count > 10_000 {
                                bail!("REPL allocation exceeds snapshot limit of 10000 elements");
                            }
                            self.storage(n, count, &mut out);
                        }
                    }
                    continue;
                }
                Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expr(e)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(cond)?;
                    if matches!(cond, Expr::Logical(false)) {
                        then_body.clear();
                    } else {
                        *then_body = self.statements(std::mem::take(then_body), false)?;
                    }
                    if matches!(cond, Expr::Logical(true)) {
                        *else_body = None;
                    } else if let Some(body) = else_body {
                        *body = self.statements(std::mem::take(body), false)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expr(start)?;
                    self.expr(end)?;
                    *body = self.statements(std::mem::take(body), false)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(cond)?;
                    *body = self.statements(std::mem::take(body), false)?;
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(expr)?;
                    for case in cases {
                        case.body = self.statements(std::mem::take(&mut case.body), false)?;
                    }
                    if let Some(body) = default {
                        *body = self.statements(std::mem::take(body), false)?;
                    }
                }
                _ => {}
            }
            out.push(s);
        }
        Ok(out)
    }
}

pub(super) fn prepare(
    mut program: Program,
    previous: &Allocations,
    known: &HashSet<String>,
    auto: bool,
) -> Result<Prepared> {
    let mut allocations = previous.clone();
    let alloc_names: HashSet<_> = program
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "allocatable" => Some(name.clone()),
            _ => None,
        })
        .collect();
    for s in &program.body {
        if let Stmt::ArrayDecl { name, kind, dims } = s {
            if alloc_names.contains(name) {
                super::arrays::kind(kind)?;
                if !matches!(dims.as_slice(), [Expr::Ident(n)] if n == "@deferred_extent") {
                    bail!(
                        "REPL ALLOCATABLE declarations currently require rank one with shape (:)"
                    );
                }
                allocations.entry(name.clone()).or_insert(Allocation {
                    kind: kind.clone(),
                    extent: None,
                });
            }
        }
    }
    for name in &alloc_names {
        if !allocations.contains_key(name) {
            bail!("REPL ALLOCATABLE `{name}` requires a rank-one array declaration");
        }
    }
    let mut headers = vec![];
    for s in &program.body {
        if declaration(s) {
            match s {
                Stmt::DummyIntent { intent, .. } if intent == "allocatable" => {}
                Stmt::ArrayDecl { kind, name, .. } if allocations.contains_key(name) => headers
                    .push(Stmt::ArrayDecl {
                        kind: kind.clone(),
                        name: name.clone(),
                        dims: vec![Expr::IntLit(
                            allocations[name].extent.unwrap_or(0).to_string(),
                        )],
                    }),
                _ => headers.push(s.clone()),
            }
        }
    }
    let bindings = allocations
        .iter()
        .filter(|(_, a)| a.extent.is_some())
        .map(|(n, _)| (n.clone(), n.clone()))
        .collect();
    let mut lower = Lower {
        allocations,
        bindings,
        headers,
        known: known.clone(),
        inferred: vec![],
        serial: 0,
        auto,
    };
    program.body = lower.statements(program.body, true)?;
    Ok(Prepared {
        program,
        allocations: lower.allocations,
        inferred: lower.inferred,
        bindings: lower.bindings,
    })
}

pub(super) fn variables(declarations: &[Stmt], allocations: &Allocations) -> Vec<String> {
    let mut lines = BTreeMap::new();
    for stmt in declarations {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                for name in names {
                    lines.insert(
                        name.clone(),
                        format!(
                            "{} :: {name}",
                            crate::sema::show_type(kind).to_ascii_lowercase()
                        ),
                    );
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                let text = if let Some(a) = allocations.get(name) {
                    format!(
                        "{}, allocatable :: {name}(:) ! {}",
                        crate::sema::show_type(kind).to_ascii_lowercase(),
                        a.extent
                            .map_or("unallocated".into(), |n| format!("allocated, size={n}"))
                    )
                } else {
                    format!(
                        "{} :: {name}({})",
                        crate::sema::show_type(kind).to_ascii_lowercase(),
                        dims.iter()
                            .map(|d| match d {
                                Expr::IntLit(n) | Expr::Ident(n) => n.clone(),
                                _ => "?".into(),
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                };
                lines.insert(name.clone(), text);
            }
            Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } => {
                for (name, _) in entities {
                    lines.entry(name.clone()).or_insert_with(|| {
                        format!(
                            "{}{} :: {name}",
                            crate::sema::show_type(kind).to_ascii_lowercase(),
                            if attributes.iter().any(|a| a == "parameter") {
                                ", parameter"
                            } else {
                                ""
                            }
                        )
                    });
                }
            }
            _ => {}
        }
    }
    lines.into_values().collect()
}

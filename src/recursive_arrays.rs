// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Native recursive subroutines borrow numeric array storage through the same
//! checked descriptors as POINTER accesses. Presence variants already exist.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}
fn children(s: &mut Stmt, f: &mut impl FnMut(&mut Vec<Stmt>)) {
    match s {
        Stmt::Module { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Function { body, .. }
        | Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. } => f(body),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            f(then_body);
            if let Some(b) = else_body {
                f(b);
            }
        }
        Stmt::SelectCase { cases, default, .. } => {
            for c in cases {
                f(&mut c.body);
            }
            if let Some(b) = default {
                f(b);
            }
        }
        _ => {}
    }
}
#[derive(Clone)]
struct Borrow {
    fields: Vec<String>,
    rank: usize,
    kind: TypeSpec,
    input: bool,
    output: bool,
}
impl Borrow {
    fn new(stem: &str, rank: usize, kind: TypeSpec, input: bool, output: bool) -> Self {
        Self {
            fields: ["data", "flags", "name", "state", "origin"]
                .into_iter()
                .map(|s| format!("{stem}_{s}"))
                .chain((0..rank).flat_map(|i| {
                    ["lower", "extent", "stride"].map(move |s| format!("{stem}_{s}{i}"))
                }))
                .collect(),
            rank,
            kind,
            input,
            output,
        }
    }
}
#[derive(Clone)]
struct Procedure {
    native: String,
    params: Vec<String>,
    body: Vec<Stmt>,
    borrowed: BTreeMap<String, Borrow>,
}
fn calls(body: &[Stmt], out: &mut HashSet<String>) {
    for mut s in body.to_vec() {
        if let Stmt::CallSub { name, .. } = &s {
            out.insert(name.clone());
        }
        children(&mut s, &mut |b| calls(b, out));
    }
}
fn reaches(
    from: &str,
    target: &str,
    graph: &HashMap<String, HashSet<String>>,
    seen: &mut HashSet<String>,
) -> bool {
    if !seen.insert(from.into()) {
        return false;
    }
    graph.get(from).is_some_and(|edges| {
        edges
            .iter()
            .any(|n| n == target || reaches(n, target, graph, seen))
    })
}
fn collect(body: &[Stmt], out: &mut BTreeMap<String, (Vec<String>, Vec<Stmt>)>) {
    for s in body {
        match s {
            Stmt::Subroutine {
                name, params, body, ..
            } => {
                out.insert(name.clone(), (params.clone(), body.clone()));
            }
            Stmt::Module { body, .. } => collect(body, out),
            _ => {}
        }
    }
}
fn formals(body: &[Stmt], params: &[String]) -> Vec<Stmt> {
    let mut result = vec![];
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                let names = names
                    .iter()
                    .filter(|n| params.contains(n))
                    .cloned()
                    .collect::<Vec<_>>();
                if !names.is_empty() {
                    result.push(Stmt::VarDecl {
                        kind: kind.clone(),
                        names,
                    });
                }
            }
            Stmt::ArrayDecl { name, .. } | Stmt::DummyIntent { name, .. }
                if params.contains(name) =>
            {
                result.push(s.clone())
            }
            Stmt::SourceLocation(_) => result.push(s.clone()),
            _ => {}
        }
    }
    result
}
fn rewrite_calls(
    body: &mut Vec<Stmt>,
    current: &Procedure,
    procedures: &HashMap<String, Procedure>,
) -> Result<()> {
    for s in body {
        if let Stmt::CallSub { name, args } = s {
            if let Some(callee) = procedures.get(name) {
                if args.len() != callee.params.len() {
                    bail!("wrong argument count in recursive call `{name}`");
                }
                let actuals: HashMap<_, _> = callee
                    .params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect();
                let mut forwarded = vec![];
                let mut possible = true;
                for p in &callee.params {
                    if let Some(dest) = callee.borrowed.get(p) {
                        let Expr::Ident(a) = &actuals[p] else {
                            possible = false;
                            break;
                        };
                        let Some(source) = current.borrowed.get(a) else {
                            possible = false;
                            break;
                        };
                        if source.rank != dest.rank {
                            bail!("rank mismatch in recursive argument `{p}`");
                        }
                        if !crate::pointers::same_kind(&source.kind, &dest.kind) {
                            bail!("type or kind mismatch in recursive argument `{p}`");
                        }
                        if source.input && !dest.input {
                            bail!("cannot pass INTENT(IN) variable as writable recursive argument `{p}`");
                        }
                        // Dummies are rebased at every association. Descriptor
                        // origin/strides retain the selected storage coordinates.
                        forwarded.extend(source.fields[..5].iter().map(|n| id(n)));
                        for axis in 0..dest.rank {
                            let lower = callee
                                .body
                                .iter()
                                .find_map(|s| match s {
                                    Stmt::ArrayDecl { name, dims, .. } if name == p => {
                                        Some(match &dims[axis] {
                                            Expr::Call(tag, a) if tag == "@declared_bounds" => {
                                                a[0].clone()
                                            }
                                            _ => int(1),
                                        })
                                    }
                                    _ => None,
                                })
                                .unwrap_or_else(|| int(1));
                            let mut lower = lower;
                            substitute(&mut lower, &actuals);
                            forwarded.extend([
                                lower,
                                id(&source.fields[6 + axis * 3]),
                                id(&source.fields[7 + axis * 3]),
                            ]);
                        }
                    } else {
                        forwarded.push(actuals[p].clone());
                    }
                }
                if possible {
                    *name = callee.native.clone();
                    *args = forwarded;
                }
            }
        }
        let mut error = None;
        children(s, &mut |b| {
            if error.is_none() {
                error = rewrite_calls(b, current, procedures).err();
            }
        });
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}
fn substitute(e: &mut Expr, names: &HashMap<String, Expr>) {
    match e {
        Expr::Ident(n) => {
            if let Some(value) = names.get(n) {
                *e = value.clone();
            }
        }
        Expr::Call(_, a) | Expr::Index(_, a) => {
            for e in a {
                substitute(e, names);
            }
        }
        Expr::Bin(_, a, b) => {
            substitute(a, names);
            substitute(b, names);
        }
        Expr::Un(_, a) => substitute(a, names),
        _ => {}
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut definitions = BTreeMap::new();
    collect(&p.body, &mut definitions);
    let graph = definitions
        .iter()
        .map(|(n, (_, b))| {
            let mut c = HashSet::new();
            calls(b, &mut c);
            (n.clone(), c)
        })
        .collect::<HashMap<_, _>>();
    let cycles = definitions
        .keys()
        .filter(|n| reaches(n, n, &graph, &mut HashSet::new()))
        .cloned()
        .collect::<HashSet<_>>();
    let array_cycles = cycles
        .iter()
        .filter(|n| {
            let (params, body) = &definitions[*n];
            body.iter()
                .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name)))
        })
        .cloned()
        .collect::<Vec<_>>();
    let selected = definitions
        .keys()
        .filter(|n| {
            let (params, body) = &definitions[*n];
            let array = body
                .iter()
                .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name)));
            // Presence variants leading into a recursive cycle also benefit
            // from native lowering. Ordinary allocatable/CHARACTER wrappers
            // keep their existing call-site specialization instead.
            let bridge = array
                && !body.iter().any(|s| match s {
                    Stmt::DummyIntent { name, intent } => {
                        params.contains(name)
                            && matches!(intent.as_str(), "allocatable" | "pointer")
                    }
                    Stmt::VarDecl { kind, names } => {
                        names.iter().any(|n| params.contains(n))
                            && !matches!(
                                kind,
                                TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                                    | TypeSpec::Real
                                    | TypeSpec::DoublePrecision
                                    | TypeSpec::Logical
                            )
                    }
                    Stmt::ArrayDecl { kind, name, .. } => {
                        params.contains(name)
                            && !matches!(
                                kind,
                                TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                                    | TypeSpec::Real
                                    | TypeSpec::DoublePrecision
                                    | TypeSpec::Logical
                            )
                    }
                    _ => false,
                });
            array_cycles.iter().any(|a| {
                *n == a
                    || (bridge && reaches(n, a, &graph, &mut HashSet::new()))
                    || (cycles.contains(*n)
                        && reaches(n, a, &graph, &mut HashSet::new())
                        && reaches(a, n, &graph, &mut HashSet::new()))
            })
        })
        .cloned()
        .collect::<HashSet<_>>();
    if selected.is_empty() {
        return Ok(p);
    }
    let mut procedures = HashMap::new();
    for (serial, (name, (params, body))) in definitions
        .iter()
        .filter(|(n, _)| selected.contains(*n))
        .enumerate()
    {
        let mut borrowed = BTreeMap::new();
        for param in params {
            let (kind, rank) = body
                .iter()
                .find_map(|s| match s {
                    Stmt::ArrayDecl { kind, name, dims } if name == param => {
                        Some((kind.clone(), dims.len()))
                    }
                    Stmt::VarDecl { kind, names } if names.contains(param) => {
                        Some((kind.clone(), 0))
                    }
                    _ => None,
                })
                .with_context(|| {
                    format!("recursive dummy `{param}` requires an explicit declaration")
                })?;
            let input=body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && intent=="intent(in)"));
            if rank > 0 || !input {
                if !matches!(
                    kind,
                    TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
                        | TypeSpec::Real
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                ) {
                    bail!("recursive borrowed dummy `{param}` currently requires INTEGER, REAL or LOGICAL storage");
                }
                if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && matches!(intent.as_str(),"allocatable"|"pointer"))) {
                    bail!("recursive ALLOCATABLE/POINTER dummy `{param}` is not yet supported");
                }
                borrowed.insert(
                    param.clone(),
                    Borrow::new(&format!("@recursive_{serial}_{param}"), rank, kind, input,
                        body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && intent=="intent(out)"))),
                );
            }
        }
        procedures.insert(
            name.clone(),
            Procedure {
                native: format!("@native_recursive_{serial}"),
                params: params.clone(),
                body: body.clone(),
                borrowed,
            },
        );
    }
    fn replace(
        body: &mut Vec<Stmt>,
        procedures: &HashMap<String, Procedure>,
        generated: &mut Vec<Stmt>,
    ) -> Result<()> {
        for s in body {
            if let Stmt::Subroutine {
                name,
                params,
                body,
                elemental,
                pure,
            } = s
            {
                if let Some(procedure) = procedures.get(name) {
                    let mut wrapper = formals(body, params);
                    let mut native_body = body.clone();
                    let mut native_params = vec![];
                    let mut actuals = vec![];
                    for param in params.iter() {
                        if let Some(borrow) = procedure.borrowed.get(param) {
                            native_params.extend(borrow.fields.clone());
                            native_body.push(Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: borrow.fields.clone(),
                            });
                            for f in &borrow.fields {
                                native_body.push(Stmt::DummyIntent {
                                    name: f.clone(),
                                    intent: "intent(in)".into(),
                                });
                            }
                            for decl in &mut native_body {
                                if let Stmt::ArrayDecl { name, dims, .. } = decl {
                                    if name == param {
                                        *dims = vec![id("@deferred_extent"); borrow.rank];
                                    }
                                }
                            }
                            native_body.push(Stmt::DummyIntent {
                                name: param.clone(),
                                intent: "pointer".into(),
                            });
                            native_body.push(Stmt::DummyIntent {
                                name: param.clone(),
                                intent: format!("@borrowed_descriptor:{}", borrow.fields.join(",")),
                            });
                            if borrow.output {
                                native_body.insert(
                                    0,
                                    Stmt::CallSub {
                                        name: "@borrowed_out".into(),
                                        args: vec![id(param)],
                                    },
                                );
                            }
                            wrapper.push(Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: borrow.fields[..3].to_vec(),
                            });
                            wrapper.push(Stmt::CallSub {
                                name: "@ptr_bind".into(),
                                args: borrow.fields[..3]
                                    .iter()
                                    .map(|f| id(f))
                                    .chain([id(param)])
                                    .collect(),
                            });
                            actuals.extend(borrow.fields[..3].iter().map(|f| id(f)));
                            actuals.extend([int(1), int(0)]);
                            for axis in 1..=borrow.rank {
                                actuals.extend([
                                    call("lbound", vec![id(param), int(axis as i64)]),
                                    call("size", vec![id(param), int(axis as i64)]),
                                    (1..axis)
                                        .map(|i| call("size", vec![id(param), int(i as i64)]))
                                        .reduce(|a, b| {
                                            Expr::Bin(
                                                crate::ast::BinOp::Mul,
                                                Box::new(a),
                                                Box::new(b),
                                            )
                                        })
                                        .unwrap_or_else(|| int(1)),
                                ]);
                            }
                        } else {
                            native_params.push(param.clone());
                            actuals.push(id(param));
                        }
                    }
                    native_body.push(Stmt::DummyIntent {
                        name: procedure.native.clone(),
                        intent: "@native_recursive".into(),
                    });
                    rewrite_calls(&mut native_body, procedure, procedures)?;
                    generated.push(Stmt::Subroutine {
                        name: procedure.native.clone(),
                        params: native_params,
                        body: native_body,
                        elemental: *elemental,
                        pure: *pure,
                    });
                    wrapper.push(Stmt::CallSub {
                        name: procedure.native.clone(),
                        args: actuals,
                    });
                    *body = wrapper;
                    continue;
                }
            }
            if let Stmt::Module { body, .. } = s {
                replace(body, procedures, generated)?;
            }
        }
        Ok(())
    }
    let mut generated = vec![];
    replace(&mut p.body, &procedures, &mut generated)?;
    p.body.extend(generated);
    Ok(p)
}

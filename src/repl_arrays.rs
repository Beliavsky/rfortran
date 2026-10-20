// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! Adapt array state to the interpreter's existing scalar snapshot interface.
//! Storage is captured after lowering, where fixed arrays use flat indices.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) type Values = HashMap<String, Vec<Option<Expr>>>;

pub(super) fn kind(kind: &TypeSpec) -> Result<()> {
    match kind {
        TypeSpec::Integer(_)
        | TypeSpec::IntegerKind(_)
        | TypeSpec::Real
        | TypeSpec::RealKind(_)
        | TypeSpec::DoublePrecision
        | TypeSpec::Logical
        | TypeSpec::LogicalKind(_) => Ok(()),
        _ => bail!("the REPL currently supports fixed-size INTEGER, REAL and LOGICAL arrays only"),
    }
}

pub(super) fn restore(program: &mut Program, values: &Values) {
    let mut body = Vec::new();
    let index = "@repl_restore_index".to_string();
    if values
        .values()
        .any(|array| array.iter().any(Option::is_some))
    {
        body.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(None),
            names: vec![index.clone()],
        });
    }
    for stmt in std::mem::take(&mut program.body) {
        let name = match &stmt {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        };
        body.push(stmt);
        if let Some(saved) = name.as_ref().and_then(|n| values.get(n)) {
            for (i, value) in saved.iter().enumerate() {
                if let Some(value) = value {
                    // A literal subscript makes definite-initialization
                    // analysis rescan the growing set of restored elements
                    // after every write. Use one runtime index instead;
                    // the existing per-element flags still check user reads.
                    body.push(Stmt::Assign {
                        name: index.clone(),
                        value: Expr::IntLit((i + 1).to_string()),
                    });
                    body.push(Stmt::AssignIndex {
                        name: name.clone().unwrap(),
                        indices: vec![Expr::Ident(index.clone())],
                        value: value.clone(),
                    });
                }
            }
        }
    }
    program.body = body;
}

// Balance concatenations: a left-deep expression would overflow the worker's
// JSON recursion limit and repeatedly copy the entire accumulated buffer.
fn packed(mut fields: Vec<Expr>) -> Expr {
    if fields.is_empty() {
        return Expr::Str(String::new());
    }
    while fields.len() > 1 {
        let mut next = Vec::with_capacity((fields.len() + 1) / 2);
        let mut items = fields.into_iter();
        while let Some(left) = items.next() {
            next.push(match items.next() {
                Some(right) => Expr::concat(left, right),
                None => left,
            });
        }
        fields = next;
    }
    fields.pop().unwrap()
}

pub(super) struct Capture {
    buffers: Vec<(String, String, TypeSpec)>,
    values: Values,
}

impl Capture {
    pub(super) fn prepare(
        program: &mut Program,
        flags: &mut BTreeMap<String, String>,
        names: &HashSet<String>,
    ) -> Result<Self> {
        let mut capture = Self {
            buffers: Vec::new(),
            values: HashMap::new(),
        };
        let ready = "rfort_repl_snapshot_ready".to_string();
        let logical_text = "rfort_repl_snapshot_logical_text".to_string();
        let mut needs_logical_text = false;
        let declarations: Vec<_> = program
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::ArrayDecl { kind, name, dims } if names.contains(name) => {
                    Some((kind.clone(), name.clone(), dims.clone()))
                }
                _ => None,
            })
            .collect();
        if !declarations.is_empty() {
            program.body.extend([
                Stmt::VarDecl {
                    kind: TypeSpec::Integer(None),
                    names: vec![ready.clone()],
                },
                Stmt::Assign {
                    name: ready.clone(),
                    value: Expr::IntLit("1".into()),
                },
            ]);
        }
        for (kind, name, dims) in declarations {
            let count = match dims.as_slice() {
                [Expr::IntLit(n)] => n.parse::<usize>().ok(),
                _ => None,
            }
            .ok_or_else(|| {
                anyhow::anyhow!("REPL array `{name}` requires fixed constant extents")
            })?;
            if count > 10_000 {
                bail!("REPL array `{name}` exceeds the snapshot limit of 10000 elements");
            }
            let flag = flags
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("missing REPL initialization flag for `{name}`"))?;
            capture.values.insert(name.clone(), vec![None; count]);
            let helper = format!("rfort_repl_snapshot_{}", capture.buffers.len());
            let mut fields = Vec::with_capacity(count);
            for i in 1..=count {
                let index = vec![Expr::IntLit(i.to_string())];
                let mut value = Expr::Index(name.clone(), index.clone());
                if matches!(kind, TypeSpec::Logical) {
                    needs_logical_text = true;
                    value = Expr::Call(logical_text.clone(), vec![value]);
                }
                // This private adapter runs after initialization checks. The
                // interpreter's Rust Vec storage is initialized even for unset
                // Fortran elements. Copy its payload along with the mask; the
                // payload of an unset element is discarded by finish(). This
                // does not mark that element initialized or permit user reads.
                // Interpreter concatenation converts numeric payloads with
                // Rust's round-trip Display representation, avoiding formatted
                // Fortran I/O's precision and field-width limitations.
                fields.push(Expr::concat(
                    Expr::concat(Expr::Str(" ".into()), Expr::Index(flag.clone(), index)),
                    Expr::concat(
                        Expr::concat(Expr::Str(" ".into()), value),
                        Expr::Str("\n".into()),
                    ),
                ));
            }
            program.body.extend([
                Stmt::VarDecl {
                    kind: TypeSpec::Character(Some(0)),
                    names: vec![helper.clone()],
                },
                Stmt::Assign {
                    name: helper.clone(),
                    value: packed(fields),
                },
            ]);
            flags.insert(helper.clone(), ready.clone());
            capture.buffers.push((helper, name, kind));
        }
        if needs_logical_text {
            // Keep logical conversion in a tiny private function. Calls use
            // constant-size scalar frames rather than adding per-element
            // variables to the session's frame.
            program.body.push(Stmt::Function {
                elemental: false,
                pure: true,
                name: logical_text,
                params: vec!["value".into()],
                return_type: Some(TypeSpec::Character(Some(1))),
                body: vec![
                    Stmt::VarDecl {
                        kind: TypeSpec::Logical,
                        names: vec!["value".into()],
                    },
                    Stmt::If {
                        cond: Expr::Ident("value".into()),
                        then_body: vec![Stmt::Return(Some(Expr::Str("T".into())))],
                        else_body: Some(vec![Stmt::Return(Some(Expr::Str("F".into())))]),
                    },
                ],
            });
        }
        Ok(capture)
    }

    pub(super) fn finish(mut self, scalars: &mut HashMap<String, Expr>) -> Result<Values> {
        for (helper, name, kind) in self.buffers {
            let Some(Expr::Str(buffer)) = scalars.remove(&helper) else {
                bail!("missing REPL snapshot buffer for `{name}`");
            };
            let saved = self.values.get_mut(&name).unwrap();
            let mut rows = buffer.lines();
            for value in saved {
                let row = rows
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("incomplete REPL snapshot for `{name}`"))?;
                let mut fields = row.split_whitespace();
                let flag = fields.next();
                let payload = fields.next();
                if fields.next().is_some() || !matches!(flag, Some("0" | "1")) || payload.is_none()
                {
                    bail!("invalid REPL snapshot for `{name}`");
                }
                if flag == Some("1") {
                    let payload = payload.unwrap();
                    *value = Some(match kind {
                        TypeSpec::Integer(_) => {
                            payload.parse::<i64>()?;
                            Expr::IntLit(payload.into())
                        }
                        TypeSpec::Real | TypeSpec::DoublePrecision => {
                            payload.parse::<f64>()?;
                            Expr::RealLit(payload.into())
                        }
                        TypeSpec::Logical => match payload {
                            "T" => Expr::Logical(true),
                            "F" => Expr::Logical(false),
                            _ => bail!("invalid logical REPL snapshot for `{name}`"),
                        },
                        _ => bail!("unsupported REPL snapshot type for `{name}`"),
                    });
                }
            }
            if rows.next().is_some() {
                bail!("oversized REPL snapshot for `{name}`");
            }
        }
        Ok(self.values)
    }
}

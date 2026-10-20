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
    for stmt in std::mem::take(&mut program.body) {
        let name = match &stmt {
            Stmt::ArrayDecl { name, .. } => Some(name.clone()),
            _ => None,
        };
        body.push(stmt);
        if let Some(saved) = name.as_ref().and_then(|n| values.get(n)) {
            for (i, value) in saved.iter().enumerate() {
                if let Some(value) = value {
                    body.push(Stmt::AssignIndex {
                        name: name.clone().unwrap(),
                        indices: vec![Expr::IntLit((i + 1).to_string())],
                        value: value.clone(),
                    });
                }
            }
        }
    }
    program.body = body;
}

pub(super) struct Capture {
    elements: Vec<(String, String, usize)>,
    values: Values,
}

impl Capture {
    pub(super) fn prepare(
        program: &mut Program,
        flags: &mut BTreeMap<String, String>,
        names: &HashSet<String>,
    ) -> Result<Self> {
        let mut capture = Self {
            elements: Vec::new(),
            values: HashMap::new(),
        };
        let declarations = program.body.clone();
        for stmt in declarations {
            let Stmt::ArrayDecl { kind, name, dims } = stmt else {
                continue;
            };
            if !names.contains(&name) {
                continue;
            }
            let count = match dims.as_slice() {
                [Expr::IntLit(n)] => n.parse::<usize>().ok(),
                _ => None,
            }
            .ok_or_else(|| {
                anyhow::anyhow!("REPL array `{name}` requires fixed constant extents")
            })?;
            // Each initialized element uses a scalar export slot. Bound the
            // generated adapter rather than unexpectedly creating huge ASTs.
            if count > 10_000 {
                bail!("REPL array `{name}` exceeds the snapshot limit of 10000 elements");
            }
            let Some(flag) = flags.get(&name).cloned() else {
                bail!("missing REPL initialization flag for `{name}`");
            };
            capture.values.insert(name.clone(), vec![None; count]);
            for i in 0..count {
                let id = capture.elements.len();
                let helper = format!("rfort_repl_snapshot_{id}");
                let ready = format!("rfort_repl_snapshot_ready_{id}");
                let index = vec![Expr::IntLit((i + 1).to_string())];
                program.body.extend([
                    Stmt::VarDecl {
                        kind: kind.clone(),
                        names: vec![helper.clone()],
                    },
                    Stmt::VarDecl {
                        kind: TypeSpec::Integer(None),
                        names: vec![ready.clone()],
                    },
                    Stmt::Assign {
                        name: ready.clone(),
                        value: Expr::IntLit("0".into()),
                    },
                    Stmt::If {
                        cond: Expr::eq(
                            Expr::Index(flag.clone(), index.clone()),
                            Expr::IntLit("1".into()),
                        ),
                        then_body: vec![
                            Stmt::Assign {
                                name: helper.clone(),
                                value: Expr::Index(name.clone(), index),
                            },
                            Stmt::Assign {
                                name: ready.clone(),
                                value: Expr::IntLit("1".into()),
                            },
                        ],
                        else_body: None,
                    },
                ]);
                flags.insert(helper.clone(), ready);
                capture.elements.push((helper, name.clone(), i));
            }
        }
        Ok(capture)
    }

    pub(super) fn finish(mut self, scalars: &mut HashMap<String, Expr>) -> Values {
        for (helper, name, index) in self.elements {
            self.values.get_mut(&name).unwrap()[index] = scalars.remove(&helper);
        }
        self.values
    }
}

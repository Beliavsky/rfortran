// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Runtime FORALL over fixed storage: capture by destination, then commit.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn error(cond: Expr, message: String) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message)],
        }],
        else_body: None,
    }
}
fn rename(e: &mut Expr, names: &HashMap<String, String>) {
    match e {
        Expr::Ident(n) => {
            if let Some(new) = names.get(n) {
                *n = new.clone();
            }
        }
        Expr::Call(_, a) | Expr::Index(_, a) => {
            for e in a {
                rename(e, names);
            }
        }
        Expr::Bin(_, a, b) => {
            rename(a, names);
            rename(b, names);
        }
        Expr::Un(_, a) => rename(a, names),
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                rename(e, names);
            }
        }
        _ => {}
    }
}
fn selection(name: &str, indices: &[Expr]) -> Expr {
    if let [Expr::Section {
        lower,
        upper,
        stride,
        ..
    }] = indices
    {
        Expr::Section {
            name: name.into(),
            lower: lower.clone(),
            upper: upper.clone(),
            stride: stride.clone(),
        }
    } else {
        Expr::Call(name.into(), indices.to_vec())
    }
}
fn section(e: &Expr) -> bool {
    matches!(e, Expr::Section { .. }) || matches!(e,Expr::Call(n,_) if n=="@section_subscript")
}

pub(crate) fn expand(
    controls: &[Expr],
    name: &str,
    indices: &[Expr],
    value: &Expr,
    kind: &TypeSpec,
    dims: &[i64],
    types: &HashMap<String, TypeSpec>,
    serial: &mut usize,
) -> Result<Vec<Stmt>> {
    let count = dims
        .iter()
        .try_fold(1i64, |n, d| n.checked_mul(*d))
        .context("FORALL destination size overflow")?;
    if count < 0 {
        bail!("runtime FORALL requires a statically shaped destination");
    }
    *serial += 1;
    let prefix = format!("@forall_runtime_{serial}");
    let temp = if matches!(kind, TypeSpec::Character(_)) {
        format!("@character_forall_value_{serial}")
    } else {
        format!("{prefix}_value")
    };
    let flags = format!("{prefix}_defined");
    let linear = format!("{prefix}_linear");
    let mut out = vec![
        Stmt::ArrayDecl {
            kind: kind.clone(),
            name: temp.clone(),
            dims: dims.iter().copied().map(int).collect(),
        },
        Stmt::ArrayDecl {
            kind: TypeSpec::Logical,
            name: flags.clone(),
            dims: dims.iter().copied().map(int).collect(),
        },
        Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![linear.clone()],
        },
        assign(&flags, Expr::Logical(false)),
    ];
    let mut names = HashMap::new();
    let mut loops = vec![];
    // Evaluate each control once, before any iteration or destination write.
    for (axis, c) in controls.chunks_exact(4).enumerate() {
        let Expr::Ident(index) = &c[0] else {
            unreachable!()
        };
        out.push(Stmt::DummyIntent {
            name: index.clone(),
            intent: "@forall_index".into(),
        });
        let first = format!("{prefix}_first_{axis}");
        let last = format!("{prefix}_last_{axis}");
        let step = format!("{prefix}_step_{axis}");
        let iterator = format!("{prefix}_iterator_{axis}");
        let current = format!("{prefix}_index_{axis}");
        names.insert(index.clone(), current.clone());
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![first.clone(), last.clone(), step.clone(), iterator.clone()],
        });
        out.push(Stmt::VarDecl {
            kind: types[index].clone(),
            names: vec![current.clone()],
        });
        out.extend([
            assign(&first, c[1].clone()),
            assign(&last, c[2].clone()),
            assign(&step, c[3].clone()),
            error(
                bin(BinOp::Eq, id(&step), int(0)),
                "FORALL stride must not be zero".into(),
            ),
        ]);
        loops.push((iterator, current, first, last, step));
    }
    // Bounds and strides cannot depend on an index declared in this FORALL.
    fn mentions(e: &Expr, names: &HashMap<String, String>) -> bool {
        match e {
            Expr::Ident(n) => names.contains_key(n),
            Expr::Call(_, a) | Expr::Index(_, a) => a.iter().any(|e| mentions(e, names)),
            Expr::Bin(_, a, b) => mentions(a, names) || mentions(b, names),
            Expr::Un(_, a) => mentions(a, names),
            _ => false,
        }
    }
    if controls
        .chunks_exact(4)
        .any(|c| c[1..].iter().any(|e| mentions(e, &names)))
    {
        bail!("FORALL bounds and stride must not reference a FORALL index");
    }
    let mut indices = indices.to_vec();
    let mut value = value.clone();
    for e in &mut indices {
        rename(e, &names);
    }
    rename(&mut value, &names);
    let selected = selection(&flags, &indices);
    let duplicate = if indices.iter().any(section) {
        Expr::Call("any".into(), vec![selected])
    } else {
        selected
    };
    let mut capture = vec![
        error(
            duplicate,
            format!(
                "FORALL iterations define the same destination element of `{name}` more than once"
            ),
        ),
        Stmt::AssignIndex {
            name: temp.clone(),
            indices: indices.clone(),
            value,
        },
        Stmt::AssignIndex {
            name: flags.clone(),
            indices,
            value: Expr::Logical(true),
        },
    ];
    for (iterator, current, first, last, step) in loops.into_iter().rev() {
        capture.insert(0, assign(&current, id(&iterator)));
        // Terminate without overflowing the hidden INTEGER(8) iterator.
        capture.push(Stmt::If {
            cond: bin(BinOp::Gt, id(&step), int(0)),
            then_body: vec![Stmt::If {
                cond: bin(
                    BinOp::Gt,
                    id(&iterator),
                    bin(BinOp::Sub, int(i64::MAX), id(&step)),
                ),
                then_body: vec![Stmt::Exit],
                else_body: None,
            }],
            else_body: Some(vec![Stmt::If {
                cond: bin(
                    BinOp::Lt,
                    id(&iterator),
                    bin(BinOp::Sub, int(i64::MIN), id(&step)),
                ),
                then_body: vec![Stmt::Exit],
                else_body: None,
            }]),
        });
        capture.push(assign(&iterator, bin(BinOp::Add, id(&iterator), id(&step))));
        let cond = bin(
            BinOp::Or,
            bin(
                BinOp::And,
                bin(BinOp::Gt, id(&step), int(0)),
                bin(BinOp::Le, id(&iterator), id(&last)),
            ),
            bin(
                BinOp::And,
                bin(BinOp::Lt, id(&step), int(0)),
                bin(BinOp::Ge, id(&iterator), id(&last)),
            ),
        );
        capture = vec![
            assign(&iterator, id(&first)),
            Stmt::DoWhile {
                cond,
                body: capture,
            },
        ];
    }
    out.extend(capture);
    // Commit only the selected elements. Re-evaluating the original selectors
    // here would be wrong when they read the destination being updated.
    if count > 0 {
        let mut stride = 1;
        let mut coordinates = vec![];
        for extent in dims {
            coordinates.push(bin(
                BinOp::Add,
                Expr::Call(
                    "mod".into(),
                    vec![
                        bin(
                            BinOp::Div,
                            bin(BinOp::Sub, id(&linear), int(1)),
                            int(stride),
                        ),
                        int(*extent),
                    ],
                ),
                int(1),
            ));
            stride *= extent;
        }
        out.push(Stmt::Do {
            var: linear,
            start: int(1),
            end: int(count),
            body: vec![Stmt::If {
                cond: Expr::Call(flags, coordinates.clone()),
                then_body: vec![Stmt::AssignIndex {
                    name: name.into(),
                    indices: coordinates.clone(),
                    value: Expr::Call(temp, coordinates),
                }],
                else_body: None,
            }],
        });
    }
    Ok(out)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Shared AST lowering for unsigned B/O/Z input bit patterns.
use super::*;

pub(super) fn convert(
    field: &str,
    status: &str,
    target: &Expr,
    kind: u8,
    width: usize,
    descriptor: u8,
    blank_zero: bool,
) -> Vec<Stmt> {
    let accumulator = format!("{field}_bits");
    let digit = format!("{field}_digit");
    let position = format!("{field}_column");
    let offset = format!("{field}_offset");
    let character = format!("{field}_character");
    let started = format!("{field}_started");
    let shift = match descriptor {
        b'b' => 1,
        b'o' => 3,
        _ => 4,
    };
    let radix = 1 << shift;
    let call = |name: &str, args| Expr::Call(name.into(), args);
    let between = |low, high| {
        bin(
            BinOp::And,
            bin(BinOp::Ge, var(&character), int(low)),
            bin(BinOp::Le, var(&character), int(high)),
        )
    };
    let advance = vec![
        set(&digit, int(-1)),
        branch(
            between(48, 57),
            vec![set(&digit, bin(BinOp::Sub, var(&character), int(48)))],
        ),
        branch(
            between(65, 70),
            vec![set(&digit, bin(BinOp::Sub, var(&character), int(55)))],
        ),
        branch(
            between(97, 102),
            vec![set(&digit, bin(BinOp::Sub, var(&character), int(87)))],
        ),
        branch(equal(&character, 32), vec![set(&digit, int(0))]),
        Stmt::If {
            cond: bin(
                BinOp::Or,
                bin(
                    BinOp::Or,
                    bin(BinOp::Lt, var(&digit), int(0)),
                    bin(BinOp::Ge, var(&digit), int(radix)),
                ),
                bin(
                    BinOp::Ne,
                    call(
                        "ishft",
                        vec![var(&accumulator), int(shift - kind as i64 * 8)],
                    ),
                    int(0),
                ),
            ),
            then_body: vec![set(status, int(1))],
            else_body: Some(vec![set(
                &accumulator,
                call(
                    "ior",
                    vec![
                        call("ishft", vec![var(&accumulator), int(shift)]),
                        call("int", vec![var(&digit), int(8)]),
                    ],
                ),
            )]),
        },
    ];
    let nonblank = bin(BinOp::Ne, var(&character), int(32));
    let consume = if blank_zero {
        bin(BinOp::Or, nonblank.clone(), var(&started))
    } else {
        nonblank.clone()
    };
    // Storage is represented by signed i64 in both backends. Sign-extend
    // the destination bit pattern explicitly; this pass runs after kind lowering.
    let converted = var(&accumulator);
    let store = match target {
        Expr::Ident(n) => set(n, converted),
        Expr::Index(n, indices) => Stmt::AssignIndex {
            name: n.clone(),
            indices: indices.clone(),
            value: converted,
        },
        _ => unreachable!(),
    };
    vec![Stmt::Block {
        body: vec![
            Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![accumulator.clone()],
            },
            Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: vec![digit, position.clone(), character.clone(), offset.clone()],
            },
            Stmt::VarDecl {
                kind: TypeSpec::Logical,
                names: vec![started.clone()],
            },
            set(&accumulator, int(0)),
            set(&started, Expr::Logical(false)),
            Stmt::Do {
                var: position.clone(),
                start: int(1),
                end: int(width as i64),
                body: vec![
                    set(&offset, bin(BinOp::Sub, var(&position), int(1))),
                    Stmt::CallSub {
                        name: "@internal_read_byte".into(),
                        args: vec![var(&character), var(field), var(&offset), int(width as i64)],
                    },
                    branch(equal(status, 0), vec![branch(consume, advance)]),
                    branch(nonblank, vec![set(&started, Expr::Logical(true))]),
                ],
            },
            branch(
                equal(status, 0),
                if kind < 8 {
                    vec![
                        branch(
                            bin(BinOp::Ge, var(&accumulator), int(1i64 << (kind * 8 - 1))),
                            vec![set(
                                &accumulator,
                                bin(BinOp::Sub, var(&accumulator), int(1i64 << (kind * 8))),
                            )],
                        ),
                        store,
                    ]
                } else {
                    vec![store]
                },
            ),
        ],
    }]
}

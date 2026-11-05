// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Adapt a character file to a statement-scoped integer unit. The normal
//! defined-I/O lowering handles the transfers; cleanup precedes error reporting.
use super::*;
pub(super) fn wrap(
    mut buffer: Expr,
    kind: TypeSpec,
    format: Option<Expr>,
    items: Vec<Expr>,
    writing: bool,
    status: Option<String>,
    message: Option<String>,
    prefix: &str,
) -> Result<Vec<Stmt>> {
    if !matches!(
        buffer,
        Expr::Ident(_) | Expr::Call(_, _) | Expr::Index(_, _) | Expr::Section { .. }
    ) {
        bail!("defined internal I/O requires a CHARACTER variable");
    }
    let unit = format!("{prefix}_unit");
    let ios = format!("{prefix}_status");
    let msg = format!("{prefix}_message");
    let record = format!("{prefix}_record");
    let close = format!("{prefix}_close");
    let mut out = vec![];
    if writing {
        match &mut buffer {
            Expr::Ident(_) => {}
            Expr::Call(_, indices) | Expr::Index(_, indices) => {
                for (i, index) in indices.iter_mut().enumerate() {
                    let n = format!("{prefix}_subscript_{i}");
                    out.push(decl(&n, TypeSpec::Integer(None)));
                    out.push(assign(&n, index.clone()));
                    *index = id(&n);
                }
            }
            _ => bail!(
                "defined internal WRITE requires a scalar CHARACTER variable or array element"
            ),
        }
    }
    let kind = match kind {
        TypeSpec::Character(Some(n)) => TypeSpec::Character(Some(n)),
        _ => TypeSpec::CharacterLength(Box::new(Expr::Call("len".into(), vec![buffer.clone()]))),
    };
    out.extend([
        decl(&unit, TypeSpec::Integer(None)),
        decl(&ios, TypeSpec::Integer(None)),
        decl(&close, TypeSpec::Integer(None)),
        decl(&msg, TypeSpec::Character(Some(256))),
        decl(&record, kind),
        assign(&unit, int(0)),
        assign(&ios, int(0)),
        assign(&close, int(0)),
        assign(&msg, Expr::Str(String::new())),
        assign(
            &record,
            if writing {
                Expr::Str(String::new())
            } else {
                buffer.clone()
            },
        ),
        call(
            "@defined_internal_begin",
            vec![id(&unit), id(&record), int(writing as i64), id(&ios)],
        ),
        Stmt::If {
            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
            then_body: vec![assign(
                &msg,
                Expr::Str("could not open internal file".into()),
            )],
            else_body: None,
        },
    ]);
    let transfer = if writing {
        let mut args = vec![
            id(&unit),
            format.unwrap_or_else(|| Expr::Call("@list_directed".into(), vec![])),
            Expr::Call("@write_option:iostat".into(), vec![id(&ios)]),
            Expr::Call("@write_option:iomsg".into(), vec![id(&msg)]),
        ];
        args.extend(items);
        call("@write", args)
    } else {
        let (name, mut args) = if let Some(format) = format {
            (
                "@formatted_read_status",
                vec![id(&unit), format, Expr::Str("iostat".into()), id(&ios)],
            )
        } else {
            (
                "@internal_read",
                vec![id(&unit), Expr::Str("iostat".into()), id(&ios)],
            )
        };
        args.extend(items);
        Stmt::Block {
            body: vec![
                call(name, args),
                call(crate::initialized::MAY_DEFINE, vec![id(&msg)]),
            ],
        }
    };
    out.push(when_equal(&ios, 0, vec![transfer], None));
    out.push(call(
        "@defined_internal_end",
        vec![id(&unit), id(&record), int(writing as i64), id(&close)],
    ));
    if writing {
        out.push(match buffer {
            Expr::Ident(n) => assign(&n, id(&record)),
            Expr::Call(n, indices) | Expr::Index(n, indices) => Stmt::AssignIndex {
                name: n,
                indices,
                value: id(&record),
            },
            _ => unreachable!(),
        });
    }
    out.push(when_equal(
        &ios,
        0,
        vec![
            assign(&ios, id(&close)),
            Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&close)), Box::new(int(0))),
                then_body: vec![assign(
                    &msg,
                    Expr::Str("could not close internal file".into()),
                )],
                else_body: None,
            },
        ],
        None,
    ));
    if let Some(n) = status {
        out.push(assign(&n, id(&ios)));
    } else {
        out.push(Stmt::If {
            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
            then_body: vec![call("@error_stop", vec![id(&msg)])],
            else_body: None,
        });
    }
    if let Some(n) = message {
        out.push(Stmt::If {
            cond: Expr::Bin(BinOp::Ne, Box::new(id(&ios)), Box::new(int(0))),
            then_body: vec![assign(&n, id(&msg))],
            else_body: None,
        });
    }
    Ok(out)
}

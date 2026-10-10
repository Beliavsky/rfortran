//! Lower list-directed output implied-DOs to ordinary checked control flow.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

type Symbols = HashMap<String, (TypeSpec, Vec<Expr>)>;
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
fn fragment(e: Expr) -> Stmt {
    Stmt::CallSub {
        name: "@print_fragment".into(),
        args: vec![e],
    }
}

pub fn contains_implied_do(e: &Expr) -> bool {
    match e {
        Expr::Call(n, _) if n == "@output_implied_do" => true,
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(contains_implied_do),
        Expr::Bin(_, a, b) => contains_implied_do(a) || contains_implied_do(b),
        Expr::Un(_, a) => contains_implied_do(a),
        _ => false,
    }
}

fn integer(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::IntLit(_) => true,
        Expr::Ident(n) => {
            matches!(symbols.get(n), Some((TypeSpec::Integer(None | Some(4) | Some(8)), dims)) if dims.is_empty())
        }
        Expr::Un(UnOp::Neg, a) => integer(a, symbols),
        Expr::Bin(BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow, a, b) => {
            integer(a, symbols) && integer(b, symbols)
        }
        Expr::Call(n, args) if matches!(n.as_str(), "min" | "max" | "mod" | "abs") => {
            !args.is_empty() && args.iter().all(|e| integer(e, symbols))
        }
        Expr::Call(n, args) if matches!(n.as_str(), "size" | "len" | "len_trim" | "int") => {
            args.len() == 1
        }
        _ => false,
    }
}

fn character(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Str(_) | Expr::Bin(BinOp::Concat, _, _) => true,
        Expr::Ident(n) => matches!(symbols.get(n), Some((TypeSpec::Character(_), _))),
        Expr::Call(n, _) => matches!(
            n.as_str(),
            "trim" | "adjustl" | "adjustr" | "achar" | "char" | "repeat"
        ),
        _ => false,
    }
}

pub fn lower(items: &[Expr], symbols: &Symbols, serial: &mut usize) -> Result<Stmt> {
    *serial += 1;
    let previous = format!("@output_character_{serial}");
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Logical,
            names: vec![previous.clone()],
        },
        assign(&previous, Expr::Logical(false)),
    ];
    emit(
        items,
        symbols,
        serial,
        &previous,
        &mut Vec::new(),
        &mut body,
    )?;
    body.push(Stmt::Print { items: vec![] });
    Ok(Stmt::Block { body })
}

fn emit(
    items: &[Expr],
    symbols: &Symbols,
    serial: &mut usize,
    previous: &str,
    active: &mut Vec<String>,
    body: &mut Vec<Stmt>,
) -> Result<()> {
    for item in items {
        if let Expr::Call(tag, args) = item {
            if tag == "@output_implied_do" {
                let Expr::Ident(var) = &args[0] else {
                    bail!("output implied-DO variable must be a writable scalar INTEGER of kind 4 or 8");
                };
                if !integer(&args[0], symbols) {
                    bail!("output implied-DO variable `{var}` must be a declared scalar INTEGER of kind 4 or 8");
                }
                if active.contains(var) {
                    bail!("nested output implied-DO cannot reuse active variable `{var}`");
                }
                if !args[1..4].iter().all(|e| integer(e, symbols)) {
                    bail!("output implied-DO bounds and step require supported scalar INTEGER expressions");
                }
                if crate::parameters::integer_constant(&args[3]).ok() == Some(0) {
                    bail!("output implied-DO step must not be zero");
                }
                *serial += 1;
                let first = format!("@output_first_{serial}");
                let last = format!("@output_last_{serial}");
                let step = format!("@output_step_{serial}");
                body.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![first.clone(), last.clone(), step.clone()],
                });
                // Evaluate controls exactly once, before assigning the control variable.
                body.extend([
                    assign(&first, args[1].clone()),
                    assign(&last, args[2].clone()),
                    assign(&step, args[3].clone()),
                ]);
                body.push(Stmt::If {
                    cond: bin(BinOp::Eq, id(&step), int(0)),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("output implied-DO step must not be zero".into())],
                    }],
                    else_body: None,
                });
                body.push(assign(var, id(&first)));
                active.push(var.clone());
                let mut inner = Vec::new();
                emit(&args[4..], symbols, serial, previous, active, &mut inner)?;
                active.pop();
                // Avoid wrapping the final increment into an infinite loop at i64 limits.
                let overflow = vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(
                        "output implied-DO control variable overflow".into(),
                    )],
                }];
                inner.push(Stmt::If {
                    cond: bin(BinOp::Gt, id(&step), int(0)),
                    then_body: vec![Stmt::If {
                        cond: bin(
                            BinOp::Gt,
                            id(var),
                            bin(BinOp::Sub, int(i64::MAX), id(&step)),
                        ),
                        then_body: overflow.clone(),
                        else_body: None,
                    }],
                    else_body: Some(vec![Stmt::If {
                        cond: bin(
                            BinOp::Lt,
                            id(var),
                            bin(BinOp::Sub, int(i64::MIN), id(&step)),
                        ),
                        then_body: overflow,
                        else_body: None,
                    }]),
                });
                inner.push(assign(var, bin(BinOp::Add, id(var), id(&step))));
                body.push(Stmt::DoWhile {
                    cond: bin(
                        BinOp::Or,
                        bin(
                            BinOp::And,
                            bin(BinOp::Gt, id(&step), int(0)),
                            bin(BinOp::Le, id(var), id(&last)),
                        ),
                        bin(
                            BinOp::And,
                            bin(BinOp::Lt, id(&step), int(0)),
                            bin(BinOp::Ge, id(var), id(&last)),
                        ),
                    ),
                    body: inner,
                });
                continue;
            }
        }
        if contains_implied_do(item) {
            bail!("output implied-DO must be a direct list-directed output item");
        }
        let arrays = symbols
            .iter()
            .filter(|(_, (_, dims))| !dims.is_empty())
            .map(|(n, (_, dims))| (n.clone(), dims.clone()))
            .collect();
        let mut expanded = Vec::new();
        crate::sections::expand(item.clone(), &arrays, &mut expanded)?;
        // Whole arrays and sections keep their existing element order.
        let mut scalars = Vec::new();
        for e in expanded {
            if let Expr::Ident(n) = &e {
                if let Some((_, dims)) = symbols.get(n).filter(|(_, dims)| !dims.is_empty()) {
                    if dims.len() != 1 {
                        bail!("output implied-DO array item requires a known flattened shape");
                    }
                    let extent = crate::parameters::integer_constant(&dims[0])?;
                    for i in 1..=extent {
                        scalars.push(Expr::Index(n.clone(), vec![int(i)]));
                    }
                    continue;
                }
            }
            scalars.push(e);
        }
        for e in scalars {
            let is_character = character(&e, symbols);
            let space = fragment(Expr::Str(" ".into()));
            body.push(if is_character {
                Stmt::If {
                    cond: Expr::Un(UnOp::Not, Box::new(id(previous))),
                    then_body: vec![space],
                    else_body: None,
                }
            } else {
                space
            });
            body.push(fragment(e));
            body.push(assign(previous, Expr::Logical(is_character)));
        }
    }
    Ok(())
}

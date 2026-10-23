// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Unformatted stream READ and WRITE. Each scalar value is transferred as the
//! bytes of its declared kind by `@stream_put` and `@stream_get`, element by
//! element for arrays, with no record markers. A runtime CHARACTER array,
//! stored as character codes, transfers one byte per character.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

type Symbols = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn var(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign { name: n.into(), value }
}

/// The transfer tag and byte width of a scalar of type `kind`.
pub(crate) fn layout(kind: &TypeSpec) -> Result<(i64, i64)> {
    Ok(match kind {
        TypeSpec::Integer(k @ (None | Some(1 | 2 | 4 | 8))) => (1, i64::from(k.unwrap_or(4))),
        TypeSpec::Real => (2, 4),
        TypeSpec::DoublePrecision => (2, 8),
        TypeSpec::Logical => (3, 4),
        TypeSpec::Character(n) => (4, n.unwrap_or(1) as i64),
        _ => bail!("unformatted I/O currently supports INTEGER, REAL, LOGICAL and CHARACTER values"),
    })
}

/// Stop the program, or set IOSTAT, after a transfer.
fn finish(control: &str, status: &Expr, code: &str, what: &str, body: &mut Vec<Stmt>) -> Result<()> {
    if control.is_empty() {
        body.push(Stmt::If {
            cond: bin(BinOp::Ne, var(code), int(0)),
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(format!("unformatted {what} failed: end of file or error"))],
            }],
            else_body: None,
        });
    } else {
        let Expr::Ident(status) = status else { bail!("IOSTAT requires a variable") };
        body.push(set(status, var(code)));
    }
    Ok(())
}

/// The unit of an unformatted transfer: a file unit, not the formatted
/// standard input or output.
fn file_unit(unit: &Expr, symbols: &Symbols, what: &str) -> Result<()> {
    if matches!(crate::parameters::integer_constant(unit), Ok(5 | 6)) {
        bail!("unformatted {what} requires a file unit; units 5 and 6 are formatted");
    }
    crate::file_io::unit(unit, symbols)
}

/// The status variable and an optional seek to POS.
fn begin(unit: &Expr, position: &Expr, serial: &mut usize, body: &mut Vec<Stmt>) -> String {
    *serial += 1;
    let code = format!("@stream_status_{serial}");
    body.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![code.clone()] });
    body.push(set(&code, int(0)));
    if crate::parameters::integer_constant(position).ok() != Some(0) {
        body.push(Stmt::CallSub { name: "@stream_seek".into(), args: vec![var(&code), unit.clone(), position.clone()] });
    }
    code
}

/// `[unit, control, status, pos, items...]`
pub(crate) fn lower_read(args: &[Expr], symbols: &Symbols, serial: &mut usize) -> Result<Stmt> {
    let [unit, Expr::Str(control), status, position, items @ ..] = args else { bail!("invalid unformatted READ") };
    file_unit(unit, symbols, "READ")?;
    let mut body = vec![];
    let code = begin(unit, position, serial, &mut body);
    let mut transfers = vec![];
    let source = crate::input::Source { value: unit.clone(), next: "@stream_get" };
    crate::input::emit_read_items(items, symbols, serial, &source, &code, &mut transfers)?;
    body.push(Stmt::If { cond: bin(BinOp::Eq, var(&code), int(0)), then_body: transfers, else_body: None });
    finish(control, status, &code, "READ", &mut body)?;
    Ok(Stmt::Block { body })
}

/// `[unit, control, status, pos, items...]`
pub(crate) fn lower_write(args: &[Expr], symbols: &Symbols, serial: &mut usize) -> Result<Stmt> {
    let [unit, Expr::Str(control), status, position, items @ ..] = args else { bail!("invalid unformatted WRITE") };
    file_unit(unit, symbols, "WRITE")?;
    let mut body = vec![];
    let code = begin(unit, position, serial, &mut body);
    let mut local = symbols.clone();
    for item in items {
        write_item(item, unit, &code, &mut local, serial, &mut body)?;
    }
    finish(control, status, &code, "WRITE", &mut body)?;
    Ok(Stmt::Block { body })
}

/// Write one scalar value, unless an earlier transfer failed.
fn put(value: Expr, kind: TypeSpec, unit: &Expr, code: &str, serial: &mut usize, body: &mut Vec<Stmt>) -> Result<()> {
    let (tag, width) = layout(&kind)?;
    *serial += 1;
    let temp = format!("@stream_value_{serial}");
    let store = if tag == 4 {
        Stmt::CallSub { name: "@character_assign".into(), args: vec![var(&temp), value, int(width)] }
    } else {
        set(&temp, value)
    };
    body.push(Stmt::If {
        cond: bin(BinOp::Eq, var(code), int(0)),
        then_body: vec![
            Stmt::VarDecl { kind, names: vec![temp.clone()] },
            store,
            Stmt::CallSub { name: "@stream_put".into(), args: vec![var(code), unit.clone(), var(&temp), int(tag), int(width)] },
        ],
        else_body: None,
    });
    Ok(())
}

fn write_item(item: &Expr, unit: &Expr, code: &str, symbols: &mut Symbols, serial: &mut usize, body: &mut Vec<Stmt>) -> Result<()> {
    match item {
        // A runtime CHARACTER array element: its codes, one byte each.
        Expr::Call(n, a) if n == "@character_heap_element" => {
            let [Expr::Ident(array), index, _, length, stride] = a.as_slice() else { bail!("invalid CHARACTER array WRITE item") };
            *serial += 1;
            let position = format!("@stream_position_{serial}");
            body.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![position.clone()] });
            let offset = bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), int(1)), stride.clone());
            let mut inner = vec![];
            put(Expr::Index(array.clone(), vec![bin(BinOp::Add, offset, bin(BinOp::Add, var(&position), int(1)))]),
                TypeSpec::Integer(Some(1)), unit, code, serial, &mut inner)?;
            body.push(Stmt::Do { var: position, start: int(0), end: bin(BinOp::Sub, length.clone(), int(1)), body: inner });
        }
        Expr::Call(n, a) if (n == "@output_implied_do" || n == "@input_implied_do") && a.len() > 4 => {
            let Expr::Ident(index) = &a[0] else { bail!("WRITE implied-DO requires an INTEGER variable") };
            *serial += 1;
            let (last, step) = (format!("@stream_last_{serial}"), format!("@stream_step_{serial}"));
            body.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![last.clone(), step.clone()] });
            body.extend([set(index, a[1].clone()), set(&last, a[2].clone()), set(&step, a[3].clone())]);
            let mut inner = vec![];
            for e in &a[4..] {
                write_item(e, unit, code, symbols, serial, &mut inner)?;
            }
            inner.push(set(index, bin(BinOp::Add, var(index), var(&step))));
            let more = bin(BinOp::Or,
                bin(BinOp::And, bin(BinOp::Gt, var(&step), int(0)), bin(BinOp::Le, var(index), var(&last))),
                bin(BinOp::And, bin(BinOp::Lt, var(&step), int(0)), bin(BinOp::Ge, var(index), var(&last))));
            body.push(Stmt::DoWhile { cond: more, body: inner });
        }
        _ => {
            if let Ok(kind) = crate::array_calls::scalar_actual_kind(item, symbols) {
                let mut value = item.clone();
                crate::reductions::scalar_value(&mut value, symbols, body, serial)?;
                return put(value, kind, unit, code, serial, body);
            }
            // An array value is evaluated into storage, then written in array
            // element order.
            let mut value = item.clone();
            let start = body.len();
            let (kind, _, length) = crate::reductions::argument(&mut value, symbols, body, serial)?;
            // SHAPE, LBOUND and UBOUND give default INTEGER without KIND,
            // whatever the storage of the evaluated result.
            let kind = match item {
                Expr::Call(n, a) if matches!(n.as_str(), "shape" | "lbound" | "ubound")
                    && !a.iter().any(|a| matches!(crate::named_arguments::keyword(a), Some(("kind", _)))) => TypeSpec::Integer(None),
                _ => kind,
            };
            for s in &body[start..] {
                match s {
                    Stmt::ArrayDecl { kind, name, dims } => { symbols.insert(name.clone(), (kind.clone(), dims.clone())); }
                    Stmt::VarDecl { kind, names } => for n in names { symbols.insert(n.clone(), (kind.clone(), vec![])); },
                    _ => {}
                }
            }
            crate::allocatables::metadata(&body[start..], symbols);
            *serial += 1;
            let index = format!("@stream_index_{serial}");
            body.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
            let element = crate::reductions::argument_element(&value, &var(&index), symbols)?;
            let mut inner = vec![];
            put(element, kind, unit, code, serial, &mut inner)?;
            body.push(Stmt::Do { var: index, start: int(1), end: length, body: inner });
        }
    }
    Ok(())
}

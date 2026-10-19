// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower READ statements to shared, statement-scoped input sessions.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

/// Canonicalize READ controls before the shared I/O lowering passes.
pub(crate) fn read_controls(
    controls: Vec<Expr>,
    items: Vec<Expr>,
) -> std::result::Result<Stmt, &'static str> {
    let mut unit = None;
    let mut format = None;
    let mut option = None;
    let mut named = false;
    let mut message = None;
    let mut seen = std::collections::HashSet::new();
    for (position, value) in controls.into_iter().enumerate() {
        let (key, value) = match value {
            Expr::Call(name, args) if name.starts_with("@read_control:") => {
                named = true;
                (name[14..].to_ascii_lowercase(), args[0].clone())
            }
            value => {
                if named {
                    return Err("positional READ controls must precede named controls");
                }
                let key = match position {
                    0 => "unit",
                    1 => "fmt",
                    _ => return Err("only UNIT and FMT may be positional in READ"),
                };
                (key.into(), value)
            }
        };
        if !seen.insert(key.clone()) {
            return Err("duplicate READ control specifier");
        }
        let star = matches!(&value,Expr::Call(n,_) if n=="@write_star");
        match key.as_str() {
            "unit" => {
                unit = Some(if star { int(5) } else { value });
            }
            "fmt" => format = Some(value),
            // Only defined after an error; an input error that IOSTAT does not
            // report stops the program, so IOMSG is never set.
            "iomsg" => message = Some(value),
            _ => {
                if star {
                    return Err("only READ UNIT and FMT may use '*'");
                }
                if option.is_some() {
                    return Err("READ currently supports one status or branch control");
                }
                option = Some((key, value));
            }
        }
    }
    let unit = unit.ok_or("READ requires UNIT")?;
    let format = format.ok_or("READ currently requires FMT")?;
    let list = matches!(&format,Expr::Call(n,_) if n=="@write_star");
    if !list && items.is_empty() {
        return Err("empty READ currently requires list-directed FMT='*'");
    }
    let empty = items.is_empty();
    let mut args = vec![unit];
    if !list {
        args.push(format);
    }
    let status = option.is_some();
    if list || status {
        let (control, value) = option.unwrap_or_else(|| (String::new(), int(0)));
        args.extend([Expr::Str(control), value]);
    }
    args.extend(items);
    let name = if empty {
        "@empty_read"
    } else if list {
        "@internal_read"
    } else if status {
        "@formatted_read_status"
    } else {
        "@formatted_read"
    };
    let read = Stmt::CallSub {
        name: name.into(),
        args,
    };
    // IOMSG is only defined after an error that IOSTAT reports.
    Ok(match message {
        Some(message) => Stmt::Block { body: vec![read, Stmt::CallSub { name: crate::initialized::MAY_DEFINE.into(), args: vec![message] }] },
        None => read,
    })
}

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn var(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, e: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: e,
    }
}
fn branch(cond: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::If {
        cond,
        then_body: body,
        else_body: None,
    }
}
fn equal(n: &str, value: i64) -> Expr {
    bin(BinOp::Eq, var(n), int(value))
}
fn integer_kind(e: &Expr, symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>) -> Option<u8> {
    let Expr::Ident(name) = e else {
        return None;
    };
    match symbols.get(name) {
        Some((TypeSpec::Integer(kind), dims)) if dims.is_empty() => Some(kind.unwrap_or(4)),
        _ => None,
    }
}

pub(crate) fn lower_list_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Stmt> {
    lower_session_read(args, symbols, serial)
}

fn lower_session_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Stmt> {
    let [unit, Expr::Str(control), status, items @ ..] = args else {
        bail!("invalid external READ");
    };
    let internal = !crate::output::integer(unit, symbols);
    if internal
        && !matches!(unit,Expr::Ident(n) if matches!(symbols.get(n),Some((TypeSpec::Character(_),dims)) if dims.is_empty()))
    {
        bail!("internal READ requires a scalar CHARACTER variable as its unit");
    }
    if control.eq_ignore_ascii_case("iostat") && items.iter().any(|item| item == status) {
        bail!("IOSTAT and an input destination must be distinct variables");
    }
    if !internal && crate::parameters::integer_constant(unit).ok() != Some(5) {
        crate::file_io::unit(unit, symbols)?;
    }
    if !control.is_empty()
        && (!control.eq_ignore_ascii_case("iostat")
            || !matches!(integer_kind(status, symbols), Some(1 | 2 | 4 | 8)))
    {
        bail!("IOSTAT must be a writable scalar INTEGER of kind 1, 2, 4, or 8");
    }
    *serial += 1;
    let handle = format!("@list_read_handle_{serial}");
    let code = format!("@list_read_status_{serial}");
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![handle.clone()],
        },
        Stmt::VarDecl {
            kind: TypeSpec::Integer(None),
            names: vec![code.clone()],
        },
        set(&code, int(0)),
        Stmt::CallSub {
            name: if internal {
                "@list_read_internal_begin"
            } else {
                "@list_read_begin"
            }
            .into(),
            args: vec![var(&handle), var(&code), unit.clone()],
        },
    ];
    emit_read_items(items, symbols, serial, &handle, &code, &mut body)?;
    body.push(Stmt::CallSub {
        name: "@list_read_end".into(),
        args: vec![var(&code), var(&handle)],
    });
    if control.is_empty() {
        body.push(branch(
            bin(BinOp::Ne, var(&code), int(0)),
            vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(
                    if internal {
                        "internal list-directed READ failed: invalid input or end of file"
                    } else {
                        "list-directed READ failed: invalid input or end of file"
                    }
                    .into(),
                )],
            }],
        ));
    } else {
        let Expr::Ident(status) = status else {
            bail!("IOSTAT requires a variable");
        };
        body.push(set(status, var(&code)));
    }
    Ok(Stmt::Block { body })
}
fn emit_read_items(
    items: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
    handle: &str,
    code: &str,
    body: &mut Vec<Stmt>,
) -> Result<()> {
    for item in items {
        if let Expr::Call(n, a) = item {
            if n == "@input_implied_do" || n == "@output_implied_do" {
                let Expr::Ident(index) = &a[0] else {
                    bail!("READ implied-DO requires an INTEGER variable");
                };
                let mut inner = vec![];
                emit_read_items(&a[4..], symbols, serial, handle, code, &mut inner)?;
                inner.push(branch(bin(BinOp::Ne, var(code), int(0)), vec![Stmt::Exit]));
                if !crate::output::integer(&a[0], symbols) {
                    bail!("READ implied-DO requires an INTEGER variable");
                }
                *serial += 1;
                let first = format!("@read_first_{serial}");
                let last = format!("@read_last_{serial}");
                let step = format!("@read_step_{serial}");
                body.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![first.clone(), last.clone(), step.clone()],
                });
                body.extend([
                    set(&first, a[1].clone()),
                    set(&last, a[2].clone()),
                    set(&step, a[3].clone()),
                    branch(
                        equal(&step, 0),
                        vec![Stmt::CallSub {
                            name: "@error_stop".into(),
                            args: vec![Expr::Str("READ implied-DO step must not be zero".into())],
                        }],
                    ),
                    set(index, var(&first)),
                ]);
                let overflow = vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str("READ implied-DO variable overflow".into())],
                }];
                inner.push(Stmt::If {
                    cond: bin(BinOp::Gt, var(&step), int(0)),
                    then_body: vec![branch(
                        bin(
                            BinOp::Gt,
                            var(index),
                            bin(BinOp::Sub, int(i64::MAX), var(&step)),
                        ),
                        overflow.clone(),
                    )],
                    else_body: Some(vec![branch(
                        bin(
                            BinOp::Lt,
                            var(index),
                            bin(BinOp::Sub, int(i64::MIN), var(&step)),
                        ),
                        overflow,
                    )]),
                });
                inner.push(set(index, bin(BinOp::Add, var(index), var(&step))));
                let cond = bin(
                    BinOp::Or,
                    bin(
                        BinOp::And,
                        bin(BinOp::Gt, var(&step), int(0)),
                        bin(BinOp::Le, var(index), var(&last)),
                    ),
                    bin(
                        BinOp::And,
                        bin(BinOp::Lt, var(&step), int(0)),
                        bin(BinOp::Ge, var(index), var(&last)),
                    ),
                );
                body.push(Stmt::DoWhile { cond, body: inner });
                continue;
            }
            if n == "@array_constructor" {
                emit_read_items(a, symbols, serial, handle, code, body)?;
                continue;
            }
        }
        if let Expr::Call(n, a) = item {
            if n == "@input_complex" {
                *serial += 1;
                let re = format!("@read_re_{serial}");
                let im = format!("@read_im_{serial}");
                let width = crate::parameters::integer_constant(&a[2])?;
                let store = |target: &Expr, value: Expr| -> Result<Stmt> {
                    Ok(match target {
                        Expr::Ident(n) => set(n, value),
                        Expr::Index(n, indices) | Expr::Call(n, indices) => Stmt::AssignIndex {
                            name: n.clone(),
                            indices: indices.clone(),
                            value,
                        },
                        _ => bail!("COMPLEX READ requires writable storage"),
                    })
                };
                body.push(branch(
                    equal(code, 0),
                    vec![
                        Stmt::VarDecl {
                            kind: if width == 8 {
                                TypeSpec::DoublePrecision
                            } else {
                                TypeSpec::Real
                            },
                            names: vec![re.clone(), im.clone()],
                        },
                        set(&re, Expr::RealLit("0.0".into())),
                        set(&im, Expr::RealLit("0.0".into())),
                        Stmt::CallSub {
                            name: "@list_read_complex_next".into(),
                            args: vec![var(code), var(&re), var(&im), var(handle), int(width)],
                        },
                        branch(
                            equal(code, 0),
                            vec![store(&a[0], var(&re))?, store(&a[1], var(&im))?],
                        ),
                        branch(
                            bin(
                                BinOp::And,
                                bin(BinOp::Ne, var(code), int(0)),
                                bin(BinOp::Ne, var(code), int(-2)),
                            ),
                            vec![
                                Stmt::CallSub {
                                    name: "@undefine".into(),
                                    args: vec![a[0].clone()],
                                },
                                Stmt::CallSub {
                                    name: "@undefine".into(),
                                    args: vec![a[1].clone()],
                                },
                            ],
                        ),
                        branch(equal(code, -2), vec![set(code, int(0))]),
                    ],
                ));
                continue;
            }
        }
        if let Expr::Call(n, a) = item {
            if n == "@input_character_slot" {
                let mut cases = vec![];
                // Select storage before fetching a value; no uninitialized slot is read.
                *serial += 1;
                let selected = format!("@read_character_index_{serial}");
                body.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![selected.clone()],
                });
                body.push(set(&selected, a[0].clone()));
                body.push(branch(
                    bin(
                        BinOp::Or,
                        bin(BinOp::Lt, var(&selected), int(1)),
                        bin(BinOp::Gt, var(&selected), a[1].clone()),
                    ),
                    vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("CHARACTER READ index out of bounds".into())],
                    }],
                ));
                for (i, target) in a[2..].iter().enumerate() {
                    let mut inner = vec![];
                    emit_read_items(
                        core::slice::from_ref(target),
                        symbols,
                        serial,
                        handle,
                        code,
                        &mut inner,
                    )?;
                    cases.push(crate::ast::CaseBlock {
                        items: vec![crate::ast::CaseItem::Single(int((i + 1) as i64))],
                        body: inner,
                    });
                }
                body.push(Stmt::SelectCase {
                    expr: var(&selected),
                    cases,
                    default: Some(vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("CHARACTER READ index out of bounds".into())],
                    }]),
                });
                continue;
            }
        }
        *serial += 1;
        let index = format!("@list_read_index_{serial}");
        let (target, count, kind) = match item {
            Expr::Ident(n) if matches!(symbols.get(n),Some((TypeSpec::Character(_),d)) if d.is_empty()) => {
                (item.clone(), None, symbols[n].0.clone())
            }
            Expr::Index(n, _) | Expr::Call(n, _) if symbols.contains_key(n) => {
                (item.clone(), None, symbols[n].0.clone())
            }
            _ => crate::reductions::elemental_element(item, &var(&index), symbols)?,
        };
        let (tag, width) = match kind {
            TypeSpec::Integer(k @ (None | Some(1 | 2 | 4 | 8))) => (1, k.unwrap_or(4) as i64),
            TypeSpec::Real => (2, 4),
            TypeSpec::DoublePrecision => (2, 8),
            TypeSpec::Logical => (3, 8),
            TypeSpec::Character(n) => (4, n.unwrap_or(1) as i64),
            _ => bail!("unsupported list-directed READ destination type"),
        };
        let temp = format!("@list_read_value_{serial}");
        let invalidate = Stmt::CallSub {
            name: "@undefine".into(),
            args: vec![target.clone()],
        };
        let store = match target {
            Expr::Ident(n) => {
                if tag == 4 {
                    Stmt::CallSub {
                        name: "@character_assign".into(),
                        args: vec![var(&n), var(&temp), int(width)],
                    }
                } else {
                    set(&n, var(&temp))
                }
            }
            Expr::Index(n, indices) | Expr::Call(n, indices) if symbols.contains_key(&n) => {
                Stmt::AssignIndex {
                    name: n,
                    indices,
                    value: var(&temp),
                }
            }
            _ => bail!("READ item must designate writable storage"),
        };
        let initial = match kind {
            TypeSpec::Real | TypeSpec::DoublePrecision => Expr::RealLit("0.0".into()),
            TypeSpec::Logical => Expr::Logical(false),
            TypeSpec::Character(_) => Expr::Str(String::new()),
            _ => int(0),
        };
        let mut transfer = vec![
            Stmt::VarDecl {
                kind,
                names: vec![temp.clone()],
            },
            set(&temp, initial),
            Stmt::CallSub {
                name: "@list_read_next".into(),
                args: vec![var(code), var(&temp), var(handle), int(tag), int(width)],
            },
            branch(equal(code, 0), vec![store]),
            branch(
                bin(
                    BinOp::And,
                    bin(BinOp::Ne, var(code), int(0)),
                    bin(BinOp::Ne, var(code), int(-2)),
                ),
                vec![invalidate],
            ),
            branch(equal(code, -2), vec![set(code, int(0))]),
        ];
        if let Some(count) = count {
            let length = if let Expr::Call(n, a) = item {
                if n == "@packed_view" {
                    a[1].clone()
                } else {
                    int(count)
                }
            } else {
                int(count)
            };
            transfer.push(branch(bin(BinOp::Ne, var(code), int(0)), vec![Stmt::Exit]));
            body.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: vec![index.clone()],
            });
            body.push(branch(
                equal(code, 0),
                vec![Stmt::Do {
                    var: index,
                    start: int(1),
                    end: length,
                    body: transfer,
                }],
            ));
        } else {
            body.push(branch(equal(code, 0), transfer));
        }
    }
    Ok(())
}

pub(crate) fn lower_empty_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Stmt> {
    let [unit, Expr::Str(control), status] = args else {
        bail!("invalid empty READ control list");
    };
    if crate::parameters::integer_constant(unit).ok() != Some(5) {
        crate::file_io::unit(unit, symbols)?;
    }
    let status_name = if control.is_empty() {
        None
    } else {
        if !control.eq_ignore_ascii_case("iostat") {
            bail!("empty READ currently supports only the IOSTAT control specifier");
        }
        if !matches!(integer_kind(status, symbols), Some(4 | 8)) {
            bail!("IOSTAT must be a writable scalar INTEGER of kind 4 or 8");
        }
        let Expr::Ident(name) = status else {
            unreachable!()
        };
        Some(name)
    };
    *serial += 1;
    let code = format!("@empty_read_status_{serial}");
    let buffer = format!("@empty_read_buffer_{serial}");
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Integer(None),
            names: vec![code.clone()],
        },
        Stmt::VarDecl {
            kind: TypeSpec::Character(Some(0)),
            names: vec![buffer.clone()],
        },
        set(&buffer, Expr::Str(String::new())),
        Stmt::CallSub {
            name: "@read_record_status".into(),
            args: vec![var(&code), var(&buffer), int(0), unit.clone()],
        },
    ];
    if let Some(status) = status_name {
        body.push(set(status, var(&code)));
    } else {
        body.push(branch(
            bin(BinOp::Ne, var(&code), int(0)),
            vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str("end of file or input error in empty READ".into())],
            }],
        ));
    }
    Ok(Stmt::Block { body })
}

/// Whether a field of `width` characters has no decimal point or exponent,
/// so that an F, E, D or G descriptor would imply a decimal point.
fn implied_decimal(field: &str, width: usize) -> Expr {
    let mut none = Expr::Logical(true);
    for i in 0..width {
        let c = Expr::Call("@substring".into(), vec![var(field), int(i as i64), int(1)]);
        for mark in [".", "e", "E", "d", "D"] {
            none = bin(BinOp::And, none, bin(BinOp::Ne, c.clone(), Expr::Str(mark.into())));
        }
    }
    none
}

/// The longest record a formatted READ with edit descriptors reads.
const FIELD_RECORD: usize = 4096;

/// Formatted READ with a constant format of edit descriptors: each record is
/// read whole, then each item takes the field its descriptor selects. A
/// numeric or LOGICAL field is converted by list-directed internal input; a
/// blank numeric field is zero, and an F, E, D or G field without a decimal
/// point or exponent has an implied decimal point. `args` are the unit, the
/// format, the status control and variable, and the items. None when the
/// format or items are outside this subset.
pub(crate) fn lower_field_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Option<Vec<Stmt>>> {
    use crate::format_spec as spec;
    let [unit, format, Expr::Str(control), status, items @ ..] = args else { return Ok(None) };
    let Ok(text) = crate::parameters::character_constant(format) else { return Ok(None) };
    if !control.is_empty() && !control.eq_ignore_ascii_case("iostat") { return Ok(None); }
    // Plain (A) into one CHARACTER variable keeps the record READ.
    let plain = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().eq_ignore_ascii_case("(a)");
    if plain && items.len() == 1 && matches!(&items[0], Expr::Ident(n) if matches!(symbols.get(n), Some((TypeSpec::Character(_), d)) if d.is_empty())) {
        return Ok(None);
    }
    // Items: scalars, array elements, and rank-one arrays of constant size.
    let mut targets: Vec<(Expr, TypeSpec)> = vec![];
    for item in items {
        match item {
            Expr::Ident(n) => {
                let Some((kind, dims)) = symbols.get(n) else { return Ok(None) };
                if dims.is_empty() { targets.push((item.clone(), kind.clone())); continue; }
                let [dim] = dims.as_slice() else { return Ok(None) };
                let (lower, upper) = match dim {
                    Expr::Call(tag, b) if tag == "@declared_bounds" => (crate::parameters::integer_constant(&b[0]), crate::parameters::integer_constant(&b[1])),
                    d => (Ok(1), crate::parameters::integer_constant(d)),
                };
                let (Ok(lower), Ok(upper)) = (lower, upper) else { return Ok(None) };
                for i in lower..=upper { targets.push((Expr::Index(n.clone(), vec![int(i)]), kind.clone())); }
            }
            Expr::Index(n, idx) | Expr::Call(n, idx) if symbols.get(n).is_some_and(|(_, d)| d.len() == idx.len() && !d.is_empty()) => {
                targets.push((Expr::Index(n.clone(), idx.clone()), symbols[n].0.clone()));
            }
            _ => return Ok(None),
        }
    }
    if control.is_empty() && targets.is_empty() { return Ok(None); }
    let mut parser = spec::GroupParser::new(text.as_bytes(), targets.len()).map_err(anyhow::Error::msg)?;
    // Records, each a list of (item, kind, first column, width, decimals).
    let mut records: Vec<Vec<(Expr, TypeSpec, usize, usize, Option<usize>)>> = vec![vec![]];
    let mut column = 1usize;
    let mut next = targets.into_iter();
    while let Some(d) = parser.next().map_err(anyhow::Error::msg)? {
        match d.code {
            spec::AT => bail!("AT edit descriptor is only permitted for output"),
            code if spec::is_data(code) => {
                if matches!(code, b'b' | b'o' | b'z') {
                    bail!("formatted READ does not yet support B, O or Z input");
                }
                for _ in 0..d.repeat {
                    let Some((item, kind)) = next.next() else { break };
                    let character = matches!(kind, TypeSpec::Character(_));
                    if character != (code == b'a') {
                        bail!("formatted READ: the A edit descriptor requires a CHARACTER variable, and the other edit descriptors a numeric or LOGICAL one");
                    }
                    let width = match (code, d.width, &kind) {
                        (b'a', 0, TypeSpec::Character(Some(n))) => *n,
                        (_, 0, _) => bail!("formatted READ requires a field width"),
                        (_, w, _) => w,
                    };
                    let decimals = (matches!(code, b'f' | b'e' | b'd' | b'g' | spec::ES | spec::EN) && d.has_precision).then_some(d.precision);
                    records.last_mut().unwrap().push((item, kind, column, width, decimals));
                    column += width;
                }
            }
            b'x' | spec::TR => column += if d.width > 0 { d.width } else { d.repeat.max(1) },
            spec::TL => column = column.saturating_sub(d.width.max(1)).max(1),
            spec::T => column = d.width.max(1),
            b'/' | spec::REVERSION => {
                for _ in 0..d.repeat.max(1) { records.push(vec![]); }
                column = 1;
            }
            _ => {}
        }
    }
    *serial += 1;
    let buffer = format!("@field_record_{serial}");
    let field = format!("@field_text_{serial}");
    let code = format!("@field_status_{serial}");
    let widest = records.iter().flatten().map(|f| f.3).max().unwrap_or(1);
    let iostat = !control.is_empty();
    let read_record = || Stmt::CallSub { name: "@formatted_read_status".into(),
        args: vec![unit.clone(), Expr::Str("(a)".into()), Expr::Str("iostat".into()), var(&code), var(&buffer)] };
    let substring = |first: usize, last: usize| Expr::Call("@substring".into(), vec![var(&buffer), int(first as i64 - 1), int((last + 1 - first) as i64)]);
    let store = |item: &Expr, value: Expr| match item {
        Expr::Ident(n) => set(n, value),
        Expr::Index(n, idx) => Stmt::AssignIndex { name: n.clone(), indices: idx.clone(), value },
        _ => unreachable!(),
    };
    let convert = |(item, kind, first, width, decimals): &(Expr, TypeSpec, usize, usize, Option<usize>)| -> Vec<Stmt> {
        let last = first + width - 1;
        if let TypeSpec::Character(length) = kind {
            // Aw with w longer than the item takes the rightmost characters.
            let length = length.unwrap_or(1);
            let start = if *width > length { last + 1 - length } else { *first };
            return vec![store(item, substring(start, last))];
        }
        let blank = bin(BinOp::Eq, var(&field), Expr::Str(String::new()));
        let zero = match kind { TypeSpec::Logical => Expr::Logical(false), _ => int(0) };
        let mut read = vec![Stmt::CallSub { name: "@internal_read".into(), args: if iostat {
            vec![var(&field), Expr::Str("iostat".into()), var(&code), item.clone()]
        } else {
            vec![var(&field), Expr::Str(String::new()), int(0), item.clone()]
        } }];
        if let Some(d) = decimals.filter(|d| *d > 0) {
            let scaled = bin(BinOp::Div, item.clone(), Expr::RealLit(format!("{}.0", 10f64.powi(d as i32))));
            read.push(branch(implied_decimal(&field, *width), vec![store(item, scaled)]));
        }
        vec![set(&field, substring(*first, last)), Stmt::If { cond: blank, then_body: vec![store(item, zero)], else_body: Some(read) }]
    };
    // Each later record is read only when the earlier ones succeeded.
    let mut nested: Vec<Stmt> = vec![];
    for record in records.iter().rev() {
        let mut segment = vec![read_record()];
        // A failed transfer ends the input list. Later conversions must not
        // overwrite its status with success (or define subsequent items).
        let mut fields: Vec<Stmt> = record.iter()
            .map(|field| branch(equal(&code, 0), convert(field))).collect();
        fields.extend(nested);
        segment.push(branch(equal(&code, 0), fields));
        nested = segment;
    }
    let mut body = vec![
        Stmt::VarDecl { kind: TypeSpec::Character(Some(FIELD_RECORD)), names: vec![buffer.clone()] },
        Stmt::VarDecl { kind: TypeSpec::Character(Some(widest)), names: vec![field] },
        Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![code.clone()] },
        set(&code, int(0)),
    ];
    body.extend(nested);
    if iostat {
        let Expr::Ident(status) = status else { bail!("IOSTAT must be a writable scalar INTEGER variable") };
        body.push(set(status, var(&code)));
    } else {
        body.push(branch(bin(BinOp::Ne, var(&code), int(0)), vec![Stmt::CallSub {
            name: "@error_stop".into(), args: vec![Expr::Str("end of file or input error in formatted READ".into())] }]));
    }
    Ok(Some(vec![Stmt::Block { body }]))
}

pub(crate) fn lower_record_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Stmt> {
    let [unit, format, Expr::Str(control), status, Expr::Ident(dest)] = args else {
        bail!("formatted READ requires one writable scalar CHARACTER variable");
    };
    if !control.is_empty() && !control.eq_ignore_ascii_case("iostat") {
        bail!("formatted READ currently supports only the IOSTAT control specifier");
    }
    fn character(e: &Expr, symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>) -> bool {
        match e {
            Expr::Str(_) => true,
            Expr::Ident(n) => {
                matches!(symbols.get(n),Some((TypeSpec::Character(_),dims)) if dims.is_empty())
            }
            Expr::Bin(BinOp::Concat, a, b) => character(a, symbols) && character(b, symbols),
            Expr::Call(n, a) if n == "trim" && a.len() == 1 => character(&a[0], symbols),
            Expr::Call(n, a) if n == "@dynamic_substring" && a.len() == 4 => {
                character(&a[0], symbols)
            }
            _ => false,
        }
    }
    if let Ok(text) = crate::parameters::character_constant(format) {
        if !crate::record_input::valid_format(text.as_bytes()) {
            bail!("formatted READ currently supports only the (A) format");
        }
    } else if !character(format, symbols) {
        bail!("READ format must be scalar CHARACTER; numeric format labels are not yet supported");
    }
    if crate::parameters::integer_constant(unit).ok() != Some(5) {
        crate::file_io::unit(unit, symbols)?;
    }
    let Some((TypeSpec::Character(length), dims)) = symbols.get(dest) else {
        bail!("formatted READ requires one writable scalar CHARACTER variable");
    };
    if !dims.is_empty() {
        bail!("formatted READ requires a scalar CHARACTER variable");
    }
    if control.is_empty() && crate::parameters::character_constant(format).is_ok() {
        let mut args = vec![var(dest), int(length.unwrap_or(1) as i64)];
        if crate::parameters::integer_constant(unit).ok() != Some(5) {
            args.push(unit.clone());
        }
        return Ok(Stmt::CallSub {
            name: "@read_record".into(),
            args,
        });
    }
    if !control.is_empty() && !matches!(integer_kind(status, symbols), Some(4 | 8)) {
        bail!("IOSTAT must be a writable scalar INTEGER of kind 4 or 8");
    }
    *serial += 1;
    let code = format!("@record_read_status_{serial}");
    let buffer = format!("@record_read_buffer_{serial}");
    let mut read_args = vec![
        var(&code),
        var(&buffer),
        int(length.unwrap_or(1) as i64),
        unit.clone(),
    ];
    // Constants were validated above; dynamic formats are checked before input.
    if crate::parameters::character_constant(format).is_err() {
        read_args.push(format.clone());
    }
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Integer(None),
            names: vec![code.clone()],
        },
        Stmt::VarDecl {
            kind: TypeSpec::Character(*length),
            names: vec![buffer.clone()],
        },
        set(&buffer, Expr::Str(String::new())),
        Stmt::CallSub {
            name: "@read_record_status".into(),
            args: read_args,
        },
        Stmt::If {
            cond: equal(&code, 0),
            then_body: vec![Stmt::CallSub {
                name: "@character_assign".into(),
                args: vec![var(dest), var(&buffer), int(length.unwrap_or(1) as i64)],
            }],
            else_body: Some(vec![Stmt::CallSub {
                name: "@undefine".into(),
                args: vec![var(dest)],
            }]),
        },
    ];
    if control.is_empty() {
        for (value, message) in [
            (
                crate::record_input::INVALID_FORMAT,
                "unsupported runtime format in formatted READ (only (A) is supported)",
            ),
            (
                crate::record_input::INVALID,
                "formatted READ currently requires non-NUL ASCII input",
            ),
        ] {
            body.push(branch(
                equal(&code, value),
                vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(message.into())],
                }],
            ));
        }
        body.push(branch(
            bin(BinOp::Ne, var(&code), int(0)),
            vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(
                    "end of file or input error in formatted READ".into(),
                )],
            }],
        ));
    } else {
        let Expr::Ident(status) = status else {
            unreachable!()
        };
        // Evaluate the unit and format before writing potentially aliased IOSTAT.
        body.insert(4, set(status, var(&code)));
    }
    Ok(Stmt::Block { body })
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> anyhow::Result<crate::ast::Program> {
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "internal.f90").unwrap();
        crate::reductions::resolve(crate::parameters::resolve(p)?)
    }

    #[test]
    fn validates_list_directed_read() {
        for (extra, statement, valid) in [
            ("", "read(text,*,iostat=ios) n", true),
            ("", "read(text,*) n", true),
            ("", "read(text,*,unknown=ios) n", false),
            ("", "read(text,*,iostat=1) n", false),
            ("", "read(text,*,iostat=n) n", false),
            ("", "read(text,*,iostat=ios) 1", false),
            ("", "read(text,*,iostat=ios) n,n", true),
            ("", "read(5,*,iostat=ios) n", true),
            ("", "read('123',*,iostat=ios) n", false),
            ("real :: x", "read(text,*,iostat=ios) x", true),
            ("real :: x", "read(text,*,iostat=x) n", false),
            ("integer :: a(2)", "read(text,*,iostat=ios) a", true),
            (
                "integer,parameter :: k=3",
                "read(text,*,iostat=ios) k",
                false,
            ),
            ("integer,parameter :: k=3", "read(text,*,iostat=k) n", false),
            (
                "integer(kind=16) :: big",
                "read(text,*,iostat=ios) big",
                false,
            ),
        ] {
            let source=format!("implicit none\ncharacter(len=12) :: text\ninteger :: n,ios\n{extra}\ntext='42'\n{statement}\nend");
            let result = lower(&source);
            assert_eq!(result.is_ok(), valid, "{source}: {result:?}");
        }
    }

    #[test]
    fn internal_input_requires_initialized_source() {
        let source = "character(len=12) :: text\ninteger :: n,ios\nread(text,*,iostat=ios) n\nend";
        let p = lower(source).unwrap();
        let error = crate::initialized::instrument(p).unwrap_err().to_string();
        assert!(error.contains("uninitialized variable `text`"), "{error}");
        for code in ["'123'", "'bad'", "','"] {
            let source=format!("character(len=12) :: text\ninteger :: n,ios\ntext={code}\nread(text,*,iostat=ios) n\nprint *, n\nend");
            // Assignment is conditional; the generated runtime guards distinguish
            // success from malformed input and null values.
            crate::initialized::instrument(lower(&source).unwrap()).unwrap();
        }
    }
}

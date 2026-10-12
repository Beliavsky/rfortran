// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower the supported internal integer READ to ordinary control flow.
//! Native execution and the interpreter share this scanner and its overflow checks.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec, UnOp};
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
    let mut seen = std::collections::HashSet::new();
    let mut stdin_star = false;
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
                stdin_star = star;
                unit = Some(if star { int(5) } else { value });
            }
            "fmt" => format = Some(value),
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
    if list && stdin_star && option.is_none() && !items.is_empty() {
        return Ok(Stmt::Read { args: items });
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
    Ok(Stmt::CallSub {
        name: name.into(),
        args,
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
fn one_of(n: &str, values: &[i64]) -> Expr {
    values
        .iter()
        .map(|&v| equal(n, v))
        .reduce(|a, b| bin(BinOp::Or, a, b))
        .unwrap()
}
fn space(n: &str) -> Expr {
    one_of(n, &[9, 10, 13, 32])
}
fn boundary(n: &str) -> Expr {
    bin(BinOp::Or, space(n), one_of(n, &[0, 44, 47]))
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

pub(crate) fn lower_integer_read(
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
    serial: &mut usize,
) -> Result<Stmt> {
    let [source, Expr::Str(control), status, destination] = args else {
        bail!("internal list-directed READ currently supports one scalar INTEGER destination");
    };
    if !control.is_empty() && !control.eq_ignore_ascii_case("iostat") {
        bail!("internal READ currently supports only the IOSTAT control specifier");
    }
    let length = match source {
        Expr::Ident(name) => match symbols.get(name) {
            Some((TypeSpec::Character(len), dims)) if dims.is_empty() => len.unwrap_or(1),
            _ => bail!("internal READ requires a scalar CHARACTER variable as its unit"),
        },
        _ => bail!("internal READ requires a scalar CHARACTER variable as its unit"),
    };
    if let Expr::Ident(dest)=destination {
        if let Some((TypeSpec::Character(len),dims))=symbols.get(dest) {
            if dims.is_empty() {return lower_character_read(source,control,status,dest,len.unwrap_or(1),symbols,serial);}
        }
    }
    let Some(kind @ (1 | 2 | 4 | 8)) = integer_kind(destination, symbols) else {
        bail!("internal READ destination must be a writable scalar INTEGER of kind 1, 2, 4, or 8");
    };
    let Expr::Ident(dest) = destination else {
        unreachable!()
    };
    let iostat = if control.is_empty() {
        None
    } else {
        if !matches!(integer_kind(status, symbols), Some(4 | 8)) {
            bail!("IOSTAT must be a writable scalar INTEGER of kind 4 or 8");
        }
        let Expr::Ident(name) = status else {
            unreachable!()
        };
        if name == dest {
            bail!("IOSTAT and the input destination must be distinct variables");
        }
        Some(name)
    };
    *serial += 1;
    let names: Vec<String> = [
        "byte", "offset", "value", "sign", "digits", "status", "phase",
    ]
    .iter()
    .map(|n| format!("@internal_read_{serial}_{n}"))
    .collect();
    let (byte, offset, value, sign, digits, stat, phase) = (
        &names[0], &names[1], &names[2], &names[3], &names[4], &names[5], &names[6],
    );
    let fetch = Stmt::CallSub {
        name: "@internal_read_byte".into(),
        args: vec![var(byte), source.clone(), var(offset), int(length as i64)],
    };
    let advance = vec![
        set(offset, bin(BinOp::Add, var(offset), int(1))),
        fetch.clone(),
    ];
    let error = vec![set(stat, int(1)), Stmt::Exit];
    // Accumulate negatively so -2**63 can be represented without overflow.
    let overflow = bin(
        BinOp::Or,
        bin(BinOp::Lt, var(value), int(i64::MIN / 10)),
        bin(
            BinOp::And,
            equal(value, i64::MIN / 10),
            bin(BinOp::Gt, var(byte), int(56)),
        ),
    );
    let mut digit_body = vec![
        branch(overflow, error.clone()),
        set(
            value,
            bin(
                BinOp::Sub,
                bin(BinOp::Mul, var(value), int(10)),
                bin(BinOp::Sub, var(byte), int(48)),
            ),
        ),
        set(digits, bin(BinOp::Add, var(digits), int(1))),
    ];
    digit_body.extend(advance.clone());
    let digit_loop = Stmt::DoWhile {
        cond: bin(
            BinOp::And,
            bin(BinOp::Ge, var(byte), int(48)),
            bin(BinOp::Le, var(byte), int(57)),
        ),
        body: digit_body,
    };
    let mut sign_body = vec![
        set(sign, int(1)),
        branch(equal(byte, 45), vec![set(sign, int(-1))]),
    ];
    sign_body.extend(advance.clone());
    let mut repeat_body = vec![branch(
        bin(
            BinOp::Or,
            bin(BinOp::Ne, var(sign), int(0)),
            one_of(value, &[0, i64::MIN]),
        ),
        error.clone(),
    )];
    repeat_body.extend(advance);
    repeat_body.push(Stmt::Cycle);
    let minimum = match kind {
        1 => i8::MIN as i64,
        2 => i16::MIN as i64,
        4 => i32::MIN as i64,
        _ => i64::MIN,
    };
    let maximum = match kind {
        1 => i8::MAX as i64,
        2 => i16::MAX as i64,
        4 => i32::MAX as i64,
        _ => i64::MAX,
    };
    let out_of_range = bin(
        BinOp::Or,
        bin(
            BinOp::And,
            bin(BinOp::Lt, var(sign), int(0)),
            bin(BinOp::Lt, var(value), int(minimum)),
        ),
        bin(
            BinOp::And,
            bin(BinOp::Ge, var(sign), int(0)),
            bin(BinOp::Lt, var(value), int(-maximum)),
        ),
    );
    let parse = Stmt::Do {
        var: phase.clone(),
        start: int(1),
        end: int(2),
        body: vec![
            set(value, int(0)),
            set(sign, int(0)),
            set(digits, int(0)),
            branch(one_of(byte, &[43, 45]), sign_body),
            digit_loop,
            branch(bin(BinOp::Ne, var(stat), int(0)), vec![Stmt::Exit]),
            branch(
                equal(digits, 0),
                vec![
                    // A null value (comma, slash, or r*) leaves the variable unchanged.
                    branch(
                        bin(
                            BinOp::Or,
                            bin(BinOp::Ne, var(sign), int(0)),
                            Expr::Un(UnOp::Not, Box::new(boundary(byte))),
                        ),
                        vec![set(stat, int(1))],
                    ),
                    Stmt::Exit,
                ],
            ),
            branch(
                bin(BinOp::And, equal(phase, 1), equal(byte, 42)),
                repeat_body,
            ),
            branch(Expr::Un(UnOp::Not, Box::new(boundary(byte))), error.clone()),
            branch(out_of_range, error),
            branch(
                bin(BinOp::Ge, var(sign), int(0)),
                vec![set(value, Expr::Un(UnOp::Neg, Box::new(var(value))))],
            ),
            set(dest, var(value)),
            Stmt::Exit,
        ],
    };
    let mut body = vec![
        Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: names.clone(),
        },
        set(offset, int(0)),
        set(stat, int(0)),
        fetch,
        Stmt::DoWhile {
            cond: space(byte),
            body: vec![
                set(offset, bin(BinOp::Add, var(offset), int(1))),
                Stmt::CallSub {
                    name: "@internal_read_byte".into(),
                    args: vec![var(byte), source.clone(), var(offset), int(length as i64)],
                },
            ],
        },
        Stmt::If {
            cond: equal(byte, 0),
            then_body: vec![set(stat, int(-1))],
            else_body: Some(vec![parse]),
        },
        // Error/EOF does not define an input item. Conservatively invalidate it.
        branch(
            bin(BinOp::Ne, var(stat), int(0)),
            vec![Stmt::CallSub {
                name: "@undefine".into(),
                args: vec![var(dest)],
            }],
        ),
    ];
    if let Some(iostat) = iostat {
        body.push(set(iostat, var(stat)));
    } else {
        body.push(branch(
            bin(BinOp::Ne, var(stat), int(0)),
            vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(
                    "internal integer READ failed: invalid input or end of record".into(),
                )],
            }],
        ));
    }
    Ok(Stmt::Block { body })
}

fn lower_character_read(source:&Expr,control:&str,status:&Expr,dest:&str,length:usize,
                        symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>,serial:&mut usize)->Result<Stmt> {
    if !control.is_empty() && !matches!(integer_kind(status,symbols),Some(4|8)) {bail!("IOSTAT must be a writable scalar INTEGER of kind 4 or 8");}
    *serial+=1;
    let code=format!("@character_read_code_{serial}");
    let buffer=format!("@character_read_buffer_{serial}");
    let mut body=vec![Stmt::VarDecl {kind:TypeSpec::Integer(None),names:vec![code.clone()]},
        Stmt::VarDecl {kind:TypeSpec::Character(Some(length)),names:vec![buffer.clone()]},
        set(&buffer,Expr::Str(String::new())),
        Stmt::CallSub {name:"@read_character_field".into(),args:vec![var(&code),var(&buffer),source.clone(),int(length as i64)]},
        branch(equal(&code,0),vec![Stmt::CallSub {name:"@character_assign".into(),args:vec![var(dest),var(&buffer),int(length as i64)]}])];
    if control.is_empty() {
        body.push(branch(bin(BinOp::And,bin(BinOp::Ne,var(&code),int(0)),bin(BinOp::Ne,var(&code),int(1))),vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str("invalid or exhausted internal CHARACTER input".into())]}]));
    } else {
        let Expr::Ident(status)=status else {unreachable!()};
        body.push(set(status,var(&code)));
        body.push(branch(equal(&code,1),vec![set(status,int(0))]));
    }
    Ok(Stmt::Block {body})
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
        let Expr::Ident(name) = status else { unreachable!() };
        Some(name)
    };
    *serial += 1;
    let code = format!("@empty_read_status_{serial}");
    let buffer = format!("@empty_read_buffer_{serial}");
    let mut body = vec![
        Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![code.clone()] },
        Stmt::VarDecl { kind: TypeSpec::Character(Some(0)), names: vec![buffer.clone()] },
        set(&buffer, Expr::Str(String::new())),
        Stmt::CallSub { name: "@read_record_status".into(),
            args: vec![var(&code), var(&buffer), int(0), unit.clone()] },
    ];
    if let Some(status) = status_name {
        body.push(set(status, var(&code)));
    } else {
        body.push(branch(bin(BinOp::Ne, var(&code), int(0)), vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str("end of file or input error in empty READ".into())],
        }]));
    }
    Ok(Stmt::Block { body })
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
            Expr::Ident(n) => matches!(symbols.get(n),Some((TypeSpec::Character(_),dims)) if dims.is_empty()),
            Expr::Bin(BinOp::Concat,a,b) => character(a,symbols) && character(b,symbols),
            Expr::Call(n,a) if n=="trim" && a.len()==1 => character(&a[0],symbols),
            Expr::Call(n,a) if n=="@dynamic_substring" && a.len()==4 => character(&a[0],symbols),
            _ => false,
        }
    }
    if let Ok(text)=crate::parameters::character_constant(format) {
        if !crate::record_input::valid_format(text.as_bytes()) {
            bail!("formatted READ currently supports only the (A) format");
        }
    } else if !character(format,symbols) {
        bail!("READ format must be scalar CHARACTER; numeric format labels are not yet supported");
    }
    if crate::parameters::integer_constant(unit).ok() != Some(5) {
        crate::file_io::unit(unit, symbols)?;
    }
    let Some((TypeSpec::Character(length), dims)) = symbols.get(dest) else {
        bail!("formatted READ requires one writable scalar CHARACTER variable");
    };
    if !dims.is_empty() { bail!("formatted READ requires a scalar CHARACTER variable"); }
    if control.is_empty() && crate::parameters::character_constant(format).is_ok() {
        let mut args=vec![var(dest),int(length.unwrap_or(1) as i64)];
        if crate::parameters::integer_constant(unit).ok()!=Some(5) {args.push(unit.clone());}
        return Ok(Stmt::CallSub {name:"@read_record".into(),args});
    }
    if !control.is_empty() && !matches!(integer_kind(status, symbols), Some(4 | 8)) {
        bail!("IOSTAT must be a writable scalar INTEGER of kind 4 or 8");
    }
    *serial += 1;
    let code = format!("@record_read_status_{serial}");
    let buffer = format!("@record_read_buffer_{serial}");
    let mut read_args=vec![var(&code),var(&buffer),int(length.unwrap_or(1) as i64),unit.clone()];
    // Constants were validated above; dynamic formats are checked before input.
    if crate::parameters::character_constant(format).is_err() {read_args.push(format.clone());}
    let mut body=vec![
        Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![code.clone()] },
        Stmt::VarDecl { kind: TypeSpec::Character(*length), names: vec![buffer.clone()] },
        set(&buffer, Expr::Str(String::new())),
        Stmt::CallSub { name: "@read_record_status".into(), args: read_args },
        Stmt::If {
            cond: equal(&code, 0), then_body: vec![Stmt::CallSub {
                name: "@character_assign".into(),
                args: vec![var(dest),var(&buffer),int(length.unwrap_or(1) as i64)],
            }],
            else_body: Some(vec![Stmt::CallSub { name: "@undefine".into(), args: vec![var(dest)] }]),
        },
    ];
    if control.is_empty() {
        for (value,message) in [
            (crate::record_input::INVALID_FORMAT,"unsupported runtime format in formatted READ (only (A) is supported)"),
            (crate::record_input::INVALID,"formatted READ currently requires non-NUL ASCII input"),
        ] {
            body.push(branch(equal(&code,value),vec![Stmt::CallSub {
                name:"@error_stop".into(),args:vec![Expr::Str(message.into())],
            }]));
        }
        body.push(branch(bin(BinOp::Ne,var(&code),int(0)),vec![Stmt::CallSub {
            name:"@error_stop".into(),args:vec![Expr::Str("end of file or input error in formatted READ".into())],
        }]));
    } else {
        let Expr::Ident(status)=status else {unreachable!()};
        // Evaluate the unit and format before writing potentially aliased IOSTAT.
        body.insert(4,set(status,var(&code)));
    }
    Ok(Stmt::Block {body})
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> anyhow::Result<crate::ast::Program> {
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "internal.f90").unwrap();
        crate::reductions::resolve(crate::parameters::resolve(p)?)
    }

    #[test]
    fn validates_internal_integer_read() {
        for (extra, statement, valid) in [
            ("", "read(text,*,iostat=ios) n", true),
            ("", "read(text,*) n", true),
            ("", "read(text,*,unknown=ios) n", false),
            ("", "read(text,*,iostat=1) n", false),
            ("", "read(text,*,iostat=n) n", false),
            ("", "read(text,*,iostat=ios) 1", false),
            ("", "read(text,*,iostat=ios) n,n", false),
            ("", "read(5,*,iostat=ios) n", false),
            ("", "read('123',*,iostat=ios) n", false),
            ("real :: x", "read(text,*,iostat=ios) x", false),
            ("real :: x", "read(text,*,iostat=x) n", false),
            ("integer :: a(2)", "read(text,*,iostat=ios) a", false),
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

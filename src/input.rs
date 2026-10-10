//! Lower the supported internal integer READ to ordinary control flow.
//! Native execution and the interpreter share this scanner and its overflow checks.
use crate::ast::{BinOp, Expr, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

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

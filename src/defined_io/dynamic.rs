// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
use super::*;
fn function(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}
fn section(n: &str, end: Expr) -> Expr {
    Expr::Section {
        name: n.into(),
        lower: Some(Box::new(int(1))),
        upper: Some(Box::new(end)),
        stride: None,
    }
}
struct State {
    handle: String,
    value: String,
    text: String,
    ios: String,
    msg: String,
    unit: String,
    code: String,
    count: String,
    values: String,
    index: String,
    cleanup: String,
}
impl State {
    fn new(p: &str) -> Self {
        Self {
            handle: format!("{p}_handle"),
            value: format!("{p}_value"),
            text: format!("{p}_text"),
            ios: format!("{p}_ios"),
            msg: format!("{p}_msg"),
            unit: format!("{p}_unit"),
            code: format!("{p}_code"),
            count: format!("{p}_count"),
            values: format!("{p}_values"),
            index: format!("{p}_index"),
            cleanup: format!("{p}_cleanup"),
        }
    }
    fn operation(&self, op: i64) -> Vec<Stmt> {
        vec![
            call(
                "@defined_format",
                vec![
                    id(&self.handle),
                    int(op),
                    id(&self.value),
                    id(&self.text),
                    id(&self.ios),
                ],
            ),
            Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&self.ios)), Box::new(int(0))),
                then_body: vec![assign(&self.msg, id(&self.text))],
                else_body: None,
            },
        ]
    }
    fn guard(&self, body: Vec<Stmt>) -> Stmt {
        when_equal(&self.ios, 0, body, None)
    }
    fn write(&self, items: Vec<Expr>, advance: bool) -> Stmt {
        let mut args = vec![
            id(&self.unit),
            id(&self.text),
            int(advance as i64),
            id(&self.ios),
            id(&self.msg),
        ];
        args.extend(items);
        call("@write_checked", args)
    }
    fn metadata(&self) -> Vec<Stmt> {
        let mut b = self.operation(5);
        b.push(assign(&self.count, id(&self.value)));
        b.push(Stmt::Block {
            body: {
                let mut loop_body = vec![
                    call(
                        "@do_step",
                        vec![id(&self.index), int(1), id(&self.count), int(1)],
                    ),
                    assign(&self.value, id(&self.index)),
                ];
                loop_body.extend(self.operation(6));
                loop_body.push(Stmt::AssignIndex {
                    name: self.values.clone(),
                    indices: vec![id(&self.index)],
                    value: id(&self.value),
                });
                loop_body
            },
        });
        b.extend(self.operation(4));
        b
    }
    fn setup(&self, unit: Expr, format: Expr, format_capacity: usize) -> Vec<Stmt> {
        let mut b = vec![];
        for n in [
            &self.handle,
            &self.value,
            &self.ios,
            &self.unit,
            &self.code,
            &self.count,
            &self.index,
            &self.cleanup,
        ] {
            b.push(decl(
                n,
                if n == &self.handle || n == &self.value {
                    TypeSpec::Integer(Some(8))
                } else {
                    TypeSpec::Integer(None)
                },
            ));
            b.push(assign(n, int(0)));
        }
        b.extend([
            decl(
                &self.text,
                TypeSpec::Character(Some(format_capacity.max(crate::defined_format::CAP))),
            ),
            decl(&self.msg, TypeSpec::Character(Some(256))),
            Stmt::ArrayDecl {
                name: self.values.clone(),
                kind: TypeSpec::Integer(None),
                dims: vec![int(128)],
            },
            assign(&self.msg, Expr::Str(String::new())),
            assign(&self.unit, unit),
            assign(&self.text, format.clone()),
        ]);
        if matches!(format, Expr::Ident(_)) {
            b.push(Stmt::If {
                cond: Expr::Bin(
                    BinOp::Gt,
                    Box::new(function("len_trim", vec![format.clone()])),
                    Box::new(int(format_capacity.max(crate::defined_format::CAP) as i64)),
                ),
                then_body: vec![
                    assign(&self.ios, int(1)),
                    assign(
                        &self.msg,
                        Expr::Str("runtime defined I/O format exceeds capture capacity".into()),
                    ),
                ],
                else_body: None,
            });
        }
        b.push(self.guard(self.operation(0)));
        b
    }
    fn finish(&self, status: Option<String>, message: Option<String>) -> Vec<Stmt> {
        let mut b = vec![call(
            "@defined_format",
            vec![
                id(&self.handle),
                int(8),
                id(&self.value),
                id(&self.text),
                id(&self.cleanup),
            ],
        )];
        if let Some(n) = status {
            b.push(assign(&n, id(&self.ios)));
        } else {
            b.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&self.ios)), Box::new(int(0))),
                then_body: vec![call("@error_stop", vec![id(&self.msg)])],
                else_body: None,
            });
        }
        if let Some(n) = message {
            b.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&self.ios)), Box::new(int(0))),
                then_body: vec![assign(&n, id(&self.msg))],
                else_body: None,
            });
        }
        b
    }
    fn mismatch(&self) -> Vec<Stmt> {
        vec![
            assign(&self.ios, int(1)),
            assign(
                &self.msg,
                Expr::Str("DT edit descriptor and item type do not match".into()),
            ),
        ]
    }
}
fn capacity(
    format: &Expr,
    types: &crate::operators::ExpressionTypes,
    declarations: &Declarations,
    objects: &Objects,
) -> Result<usize> {
    match types.type_rank(format,declarations,objects)? {
        Some((crate::operators::ExpressionElement::Intrinsic(TypeSpec::Character(n)),0))=>Ok(n.unwrap_or(1)),
        Some((crate::operators::ExpressionElement::Intrinsic(TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_)),0)) if matches!(format,Expr::Ident(_))=>Ok(crate::defined_format::CAP),
        _=>bail!("runtime defined I/O format requires a scalar CHARACTER variable or an expression with known length"),
    }
}
pub(super) fn output(
    unit: Expr,
    format: Expr,
    items: Vec<Expr>,
    prefix: &str,
    types: &crate::operators::ExpressionTypes,
    declarations: &Declarations,
    objects: &Objects,
    bindings: &HashMap<String, String>,
    status: Option<String>,
    message: Option<String>,
) -> Result<Stmt> {
    let s = State::new(prefix);
    let mut out = s.setup(
        unit,
        format.clone(),
        capacity(&format, types, declarations, objects)?,
    );
    out.push(call("@defined_output_record_begin", vec![id(&s.unit)]));
    for (i, item) in items.into_iter().enumerate() {
        let (element, rank) = output_type(types, &item, declarations, objects)?
            .context("cannot determine runtime formatted item type")?;
        let complex = match &element {
            crate::operators::ExpressionElement::Intrinsic(t @ TypeSpec::Complex(_)) => {
                Some(t.clone())
            }
            _ => None,
        };
        let procedure = match element {
            crate::operators::ExpressionElement::Derived(ty) => Some(
                bindings
                    .get(&ty)
                    .context("derived output item requires a defined writer")?
                    .clone(),
            ),
            _ => None,
        };
        let item = output_value(item);
        let (item, mut before, loops) = if rank > 0 {
            array_item(
                item,
                &format!("{prefix}_item_{i}"),
                types,
                declarations,
                objects,
            )?
        } else {
            (item, vec![], vec![])
        };
        // Capture a complex expression once, then transfer its two real components.
        let mut body = vec![];
        let parts = if let Some(kind) = complex {
            let temp = format!("{prefix}_complex_{i}");
            before.push(decl(&temp, kind));
            body.push(assign(&temp, item));
            vec![
                function("real", vec![id(&temp)]),
                function("aimag", vec![id(&temp)]),
            ]
        } else {
            vec![item]
        };
        for item in parts {
            let mut part = vec![assign(&s.msg, Expr::Str(String::new()))];
            part.extend(s.operation(1));
            part.push(assign(&s.code, id(&s.value)));
            let transfer = if let Some(procedure) = procedure.clone() {
                let mut b = s.operation(2);
                b.push(s.guard(vec![s.write(vec![], false)]));
                let mut invoke = s.metadata();
                invoke.extend([
                    crate::imports::reference(procedure.clone()),
                    call("@defined_output_enter", vec![id(&s.unit)]),
                    call(
                        &procedure,
                        vec![
                            item,
                            id(&s.unit),
                            function("trim", vec![id(&s.text)]),
                            section(&s.values, id(&s.count)),
                            id(&s.ios),
                            id(&s.msg),
                        ],
                    ),
                    call("@defined_output_leave", vec![]),
                ]);
                b.push(s.guard(invoke));
                when_equal(
                    &s.code,
                    crate::format_spec::DT as i64,
                    b,
                    Some(s.mismatch()),
                )
            } else {
                let mut b = s.operation(3);
                b.push(s.guard(vec![s.write(vec![item], false)]));
                when_equal(
                    &s.code,
                    crate::format_spec::DT as i64,
                    s.mismatch(),
                    Some(b),
                )
            };
            part.push(s.guard(vec![transfer]));
            body.push(s.guard(part));
        }
        for (index, first, last, step) in loops {
            let mut b = vec![
                call("@do_step", vec![id(&index), first, last, step]),
                Stmt::If {
                    cond: Expr::Bin(BinOp::Ne, Box::new(id(&s.ios)), Box::new(int(0))),
                    then_body: vec![Stmt::Exit],
                    else_body: None,
                },
            ];
            b.extend(body);
            body = vec![Stmt::Block { body: b }];
        }
        before.extend(body);
        out.push(s.guard(vec![Stmt::Block { body: before }]));
    }
    let mut finish = vec![assign(&s.msg, Expr::Str(String::new()))];
    finish.extend(s.operation(7));
    finish.push(s.guard(vec![s.write(vec![], true)]));
    out.push(s.guard(finish));
    out.push(call(
        "@defined_output_record_end",
        vec![id(&s.unit), id(&s.ios), id(&s.msg)],
    ));
    out.extend(s.finish(status, message));
    Ok(Stmt::Block { body: out })
}

pub(super) fn input(
    unit: Expr,
    format: Expr,
    items: Vec<Expr>,
    prefix: &str,
    types: &crate::operators::ExpressionTypes,
    declarations: &Declarations,
    objects: &Objects,
    bindings: &HashMap<String, String>,
    status: Option<String>,
    message: Option<String>,
) -> Result<Stmt> {
    let s = State::new(prefix);
    let session = format!("{prefix}_session");
    let mut out = s.setup(
        unit,
        format.clone(),
        capacity(&format, types, declarations, objects)?,
    );
    out.extend([
        decl(&session, TypeSpec::Integer(Some(8))),
        assign(&session, int(0)),
        call("@defined_input_enter", vec![id(&s.unit)]),
    ]);
    out.push(s.guard(vec![call(
        "@list_read_begin",
        vec![id(&session), id(&s.ios), id(&s.unit)],
    )]));
    let field = |tag: i64, len: Expr| {
        call(
            "@defined_input_field",
            vec![
                id(&s.handle),
                id(&s.unit),
                int(tag),
                len,
                id(&s.text),
                id(&s.ios),
            ],
        )
    };
    for (i, item) in items.into_iter().enumerate() {
        let (element, rank) = types
            .type_rank(&item, declarations, objects)?
            .context("cannot determine runtime formatted input item type")?;
        let complex = match &element {
            crate::operators::ExpressionElement::Intrinsic(TypeSpec::Complex(k)) => Some(k.clone()),
            _ => None,
        };
        let (procedure, tag) = match element {
            crate::operators::ExpressionElement::Derived(ty) => (
                Some(
                    bindings
                        .get(&ty)
                        .context("derived input item requires a defined reader")?
                        .clone(),
                ),
                0,
            ),
            crate::operators::ExpressionElement::Intrinsic(kind) => (
                None,
                match kind {
                    TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) => 1,
                    TypeSpec::Real
                    | TypeSpec::RealKind(_)
                    | TypeSpec::DoublePrecision
                    | TypeSpec::Complex(_) => 2,
                    TypeSpec::Logical | TypeSpec::LogicalKind(_) => 3,
                    TypeSpec::Character(_)
                    | TypeSpec::CharacterLength(_)
                    | TypeSpec::CharacterAssumed => 4,
                    _ => bail!("this intrinsic type is not yet supported in mixed formatted input"),
                },
            ),
        };
        let indexed = matches!(&item,Expr::Call(n,_)|Expr::Index(n,_) if matches!(types.type_rank(&id(n),declarations,objects)?,Some((_,r)) if r>0));
        let (item, mut before, loops) = if rank > 0 || indexed {
            array_item(
                item,
                &format!("{prefix}_item_{i}"),
                types,
                declarations,
                objects,
            )?
        } else if matches!(item, Expr::Ident(_)) {
            (item, vec![], vec![])
        } else {
            bail!("defined input requires writable variables, elements or sections")
        };
        let mut body = vec![];
        let mut complete = None;
        let parts = if let Some(k) = complex {
            let re = format!("{prefix}_real_{i}");
            let im = format!("{prefix}_imag_{i}");
            let kind_arg = if let Some(json) = k.strip_prefix(crate::ast::KIND_EXPRESSION) {
                serde_json::from_str(json)?
            } else if k.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                Expr::IntLit(k.clone())
            } else {
                id(&k)
            };
            let kind = TypeSpec::RealKind(k);
            before.extend([decl(&re, kind.clone()), decl(&im, kind)]);
            complete = Some(store(
                &item,
                function("cmplx", vec![id(&re), id(&im), kind_arg]),
            ));
            vec![id(&re), id(&im)]
        } else {
            vec![item]
        };
        for item in parts {
            let mut part = vec![assign(&s.msg, Expr::Str(String::new()))];
            part.extend(s.operation(1));
            part.push(assign(&s.code, id(&s.value)));
            let transfer = if let Some(procedure) = procedure.clone() {
                let mut b = vec![field(0, int(0))];
                let mut invoke = s.metadata();
                invoke.extend([
                    crate::imports::reference(procedure.clone()),
                    call(
                        &procedure,
                        vec![
                            item,
                            id(&s.unit),
                            function("trim", vec![id(&s.text)]),
                            section(&s.values, id(&s.count)),
                            id(&s.ios),
                            id(&s.msg),
                        ],
                    ),
                ]);
                b.push(s.guard(invoke));
                when_equal(
                    &s.code,
                    crate::format_spec::DT as i64,
                    b,
                    Some(s.mismatch()),
                )
            } else {
                let len = if tag == 4 {
                    function("len", vec![item.clone()])
                } else {
                    int(0)
                };
                let b = vec![
                    field(tag, len),
                    s.guard(if tag == 4 {
                        vec![store(&item, id(&s.text))]
                    } else {
                        vec![call(
                            "@internal_read",
                            vec![id(&s.text), Expr::Str("iostat".into()), id(&s.ios), item],
                        )]
                    }),
                ];
                when_equal(
                    &s.code,
                    crate::format_spec::DT as i64,
                    s.mismatch(),
                    Some(b),
                )
            };
            part.push(s.guard(vec![transfer]));
            body.push(s.guard(part));
        }
        if let Some(complete) = complete {
            body.push(s.guard(vec![complete]));
        }
        for (index, first, last, step) in loops {
            let mut b = vec![
                call("@do_step", vec![id(&index), first, last, step]),
                Stmt::If {
                    cond: Expr::Bin(BinOp::Ne, Box::new(id(&s.ios)), Box::new(int(0))),
                    then_body: vec![Stmt::Exit],
                    else_body: None,
                },
            ];
            b.extend(body);
            body = vec![Stmt::Block { body: b }];
        }
        before.extend(body);
        out.push(s.guard(vec![Stmt::Block { body: before }]));
    }
    let mut finish = vec![assign(&s.msg, Expr::Str(String::new()))];
    finish.extend(s.operation(7));
    finish.push(s.guard(vec![field(0, int(0))]));
    out.push(s.guard(finish));
    out.push(call("@defined_input_leave", vec![id(&s.ios)]));
    out.push(Stmt::If {
        cond: Expr::Bin(
            BinOp::And,
            Box::new(Expr::Bin(BinOp::Ne, Box::new(id(&s.ios)), Box::new(int(0)))),
            Box::new(Expr::Bin(
                BinOp::Eq,
                Box::new(id(&s.msg)),
                Box::new(Expr::Str(String::new())),
            )),
        ),
        then_body: vec![assign(
            &s.msg,
            Expr::Str("defined formatted input failed: invalid field or end of file".into()),
        )],
        else_body: None,
    });
    out.extend(s.finish(status, message));
    Ok(Stmt::Block { body: out })
}

fn store(item: &Expr, value: Expr) -> Stmt {
    match item {
        Expr::Ident(n) => assign(n, value),
        Expr::Call(n, args) | Expr::Index(n, args) => Stmt::AssignIndex {
            name: n.clone(),
            indices: args.clone(),
            value,
        },
        _ => unreachable!("scalar input item"),
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Materialize runtime constructors at their expression evaluation site.
//! Growing storage avoids evaluating items or nested loop controls twice.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: name.into(),
        value,
    }
}
fn error(cond: Expr, text: &str) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(text.into())],
        }],
        else_body: None,
    }
}

struct Lower {
    serial: usize,
}
impl Lower {
    fn fresh(&mut self, role: &str) -> String {
        self.serial += 1;
        format!("@constructor_expr_{role}_{}", self.serial)
    }
    fn kind(items: &[Expr], shapes: &Shapes) -> Result<TypeSpec> {
        let mut result = None;
        for item in items {
            let kind = match item {
                Expr::Ident(n) if shapes.contains_key(n) => shapes[n].0.clone(),
                Expr::Call(n,a) if n=="@conditional" => Self::kind(&a[1..2],shapes)?,
                Expr::Call(n, a) if n == "@array_constructor" => Self::kind(a, shapes)?,
                Expr::Call(n, a) if n == "@output_implied_do" => Self::kind(&a[4..], shapes)?,
                Expr::Call(n, _) if shapes.contains_key(&format!("@constructor_function:{n}")) => {
                    shapes[&format!("@constructor_function:{n}")].0.clone()
                }
                Expr::Call(n, _) if matches!(n.as_str(), "all" | "any") => TypeSpec::Logical,
                Expr::Call(n, _) if matches!(n.as_str(), "kind" | "len" | "len_trim" | "count") => {
                    TypeSpec::Integer(None)
                }
                Expr::Call(n, args) if matches!(n.as_str(), "trim" | "adjustl" | "adjustr") => {
                    Self::kind(&args[..1], shapes)?
                }
                _ => crate::reductions::elemental_element(item, &int(1), shapes)
                    .map(|(_, _, k)| k)
                    .or_else(|_| crate::array_calls::scalar_actual_kind(item, shapes))?,
            };
            let kind = if kind == TypeSpec::Integer(Some(4)) {
                TypeSpec::Integer(None)
            } else {
                kind
            };
            if result.as_ref().is_some_and(|k| k != &kind) {
                bail!("array constructor elements must have the same type and kind");
            }
            result = Some(kind);
        }
        result.context("runtime constructor requires typed elements")
    }
    fn capture(&mut self, value: Expr, kind: TypeSpec, out: &mut Vec<Stmt>) -> String {
        let n = self.fresh("value");
        out.push(Stmt::VarDecl {
            kind,
            names: vec![n.clone()],
        });
        out.push(assign(&n, value));
        n
    }
    fn items(
        &mut self,
        items: &[Expr],
        shapes: &Shapes,
        array: &str,
        cursor: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        for item in items {
            if let Expr::Call(n, args) = item {
                if n == "@array_constructor" {
                    self.items(args, shapes, array, cursor, out)?;
                    continue;
                }
                if n == "@output_implied_do" {
                    let Expr::Ident(index) = &args[0] else {
                        bail!("invalid constructor implied-DO index");
                    };
                    let (kind, dims) = shapes
                        .get(index)
                        .context("constructor implied-DO index must be declared")?;
                    if !dims.is_empty() || !matches!(kind, TypeSpec::Integer(_)) {
                        bail!("constructor implied-DO index must be scalar INTEGER");
                    }
                    if !args[1..4].iter().all(|e| crate::output::integer(e, shapes) || matches!(e,Expr::Call(n,_) if matches!(shapes.get(&format!("@constructor_function:{n}")),Some((TypeSpec::Integer(_),_))))) { bail!("constructor implied-DO bounds and stride must be scalar INTEGER expressions"); }
                    if crate::parameters::integer_constant(&args[3]).ok() == Some(0) {
                        bail!("constructor implied-DO stride must not be zero");
                    }
                    let first = self.capture(args[1].clone(), TypeSpec::Integer(Some(8)), out);
                    let last = self.capture(args[2].clone(), TypeSpec::Integer(Some(8)), out);
                    let step = self.capture(args[3].clone(), TypeSpec::Integer(Some(8)), out);
                    out.push(error(
                        bin(BinOp::Eq, id(&step), int(0)),
                        "constructor implied-DO stride must not be zero",
                    ));
                    let control = self.capture(id(&first), TypeSpec::Integer(Some(8)), out);
                    let local = self.fresh("index");
                    out.push(Stmt::VarDecl {
                        kind: kind.clone(),
                        names: vec![local.clone()],
                    });
                    let mut nested = args[4..].to_vec();
                    for e in &mut nested {
                        crate::constructors::rename(
                            e,
                            &HashMap::from([(index.clone(), local.clone())]),
                        );
                    }
                    let mut scope = shapes.clone();
                    scope.insert(local.clone(), (kind.clone(), vec![]));
                    let mut body = vec![assign(&local, id(&control))];
                    self.items(&nested, &scope, array, cursor, &mut body)?;
                    // Test the distance to the last value before incrementing. In
                    // particular a one-element loop at HUGE never overflows.
                    body.push(Stmt::If {
                        cond: bin(BinOp::Gt, id(&step), int(0)),
                        then_body: vec![Stmt::If {
                            cond: bin(
                                BinOp::Gt,
                                id(&control),
                                bin(BinOp::Sub, int(i64::MAX), id(&step)),
                            ),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        }],
                        else_body: Some(vec![Stmt::If {
                            cond: bin(
                                BinOp::Lt,
                                id(&control),
                                bin(BinOp::Sub, int(i64::MIN), id(&step)),
                            ),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        }]),
                    });
                    body.push(assign(&control, bin(BinOp::Add, id(&control), id(&step))));
                    out.push(Stmt::DoWhile {
                        cond: bin(
                            BinOp::Or,
                            bin(
                                BinOp::And,
                                bin(BinOp::Gt, id(&step), int(0)),
                                bin(BinOp::Le, id(&control), id(&last)),
                            ),
                            bin(
                                BinOp::And,
                                bin(BinOp::Lt, id(&step), int(0)),
                                bin(BinOp::Ge, id(&control), id(&last)),
                            ),
                        ),
                        body: vec![Stmt::Block { body }],
                    });
                    continue;
                }
            }
            if let Expr::Ident(n) = item {
                if let Some((_, dims)) = shapes.get(n).filter(|(_, d)| !d.is_empty()) {
                    // ARRAY constructor items are flattened in column-major order.
                    let mut indices = vec![];
                    let mut controls = vec![];
                    let mut scope = shapes.clone();
                    for axis in 1..=dims.len() {
                        let index = self.fresh("element");
                        out.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![index.clone()],
                        });
                        scope.insert(index.clone(), (TypeSpec::Integer(Some(8)), vec![]));
                        let first = self.capture(
                            Expr::Call("lbound".into(), vec![id(n), int(axis as i64)]),
                            TypeSpec::Integer(Some(8)),
                            out,
                        );
                        let last = self.capture(
                            Expr::Call("ubound".into(), vec![id(n), int(axis as i64)]),
                            TypeSpec::Integer(Some(8)),
                            out,
                        );
                        controls.push((index.clone(), first, last));
                        indices.push(id(&index));
                    }
                    let mut body = vec![];
                    self.items(
                        &[Expr::Index(n.clone(), indices)],
                        &scope,
                        array,
                        cursor,
                        &mut body,
                    )?;
                    for (var, first, last) in controls {
                        body = vec![Stmt::Do {
                            var,
                            start: id(&first),
                            end: id(&last),
                            body,
                        }];
                    }
                    out.extend(body);
                    continue;
                }
            }
            let mut value = item.clone();
            let mut cleanup = vec![];
            self.expr(&mut value, shapes, out, &mut cleanup)?;
            let length = format!("{array}$length");
            out.push(Stmt::If {
                cond: bin(BinOp::Eq, id(cursor), id(&length)),
                then_body: vec![Stmt::CallSub {
                    name: "@heap_resize".into(),
                    args: vec![
                        id(array),
                        id(&length),
                        Expr::Call(
                            "max".into(),
                            vec![int(1), bin(BinOp::Mul, int(2), id(&length))],
                        ),
                    ],
                }],
                else_body: None,
            });
            out.push(assign(cursor, bin(BinOp::Add, id(cursor), int(1))));
            out.push(Stmt::AssignIndex {
                name: array.into(),
                indices: vec![id(cursor)],
                value,
            });
            out.extend(cleanup);
        }
        Ok(())
    }
    fn expr(
        &mut self,
        e: &mut Expr,
        shapes: &Shapes,
        out: &mut Vec<Stmt>,
        cleanup: &mut Vec<Stmt>,
    ) -> Result<()> {
        if crate::conditionals::choice(e) {return Ok(());}
        if matches!(e, Expr::Call(n, _) if n == "@array_constructor") {
            if crate::constructors::delayed(e) {
                crate::constructors::fixed_expression(e, shapes)?;
            }
            if crate::constructors::delayed(e) {
                let Expr::Call(_, items) = e else {
                    unreachable!()
                };
                let kind = Self::kind(items, shapes)?;
                let array = self.fresh("array");
                let cursor = self.capture(int(0), TypeSpec::Integer(Some(8)), out);
                crate::heap::temporary(kind, array.clone(), int(0), out);
                self.items(items, shapes, &array, &cursor, out)?;
                cleanup.push(Stmt::CallSub {
                    name: "@heap_drop".into(),
                    args: vec![id(&array)],
                });
                *e = Expr::Call("@packed_view".into(), vec![id(&array), id(&cursor)]);
                return Ok(());
            }
        }
        match e {
            Expr::Call(n, args) if n == "@output_implied_do" => {
                // I/O implied-DOs belong to the I/O lowering passes. Do not
                // hoist expressions depending on their active index.
                let _ = args;
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    self.expr(arg, shapes, out, cleanup)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, shapes, out, cleanup)?;
                self.expr(b, shapes, out, cleanup)?;
            }
            Expr::Un(_, a) => self.expr(a, shapes, out, cleanup)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expr(e, shapes, out, cleanup)?;
                }
            }
            _ => {}
        }
        if let Expr::Call(n, args) = e {
            if n == "size" {
                if let Some(Expr::Call(tag, values)) = args.first() {
                    if tag == "@packed_view" {
                        if args.len() > 2
                            || args.get(1).is_some_and(|e| {
                                crate::parameters::integer_constant(e).ok() != Some(1)
                            })
                        {
                            bail!("SIZE dimension must be 1 for a rank-one constructor");
                        }
                        *e = Expr::Call("int".into(), vec![values[1].clone()]);
                    }
                }
            }
        }
        Ok(())
    }
    fn scope(&mut self, statements: &mut Vec<Stmt>, inherited: &Shapes) -> Result<()> {
        let mut shapes = inherited.clone();
        for s in statements.iter() {
            match s {
                Stmt::DummyIntent {name,intent} if matches!(intent.as_str(),"@heap"|"@heap_dummy") => {
                    shapes.insert(format!("@constructor_heap:{name}"),(TypeSpec::Logical,vec![]));
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        shapes.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut offset = None;
        for mut s in std::mem::take(statements) {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(p);
            }
            if let Stmt::Print { items } = &s {
                if items.iter().any(crate::output::contains_implied_do)
                    && items.iter().any(crate::constructors::delayed)
                {
                    let mut expanded =
                        vec![crate::output::lower(items, &shapes, &mut self.serial)?];
                    self.scope(&mut expanded, &shapes)?;
                    out.extend(expanded);
                    continue;
                }
            }
            let mut before = vec![];
            let mut after = vec![];
            (|| -> Result<()> {
                match &mut s {
                    Stmt::Assign { name, value }
                        if matches!(&*value,Expr::Call(n,_) if n=="@array_constructor")
                            && !Self::general(value, &shapes)
                            && !shapes.contains_key(&format!("@constructor_heap:{name}")) => {}
                    Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                        self.expr(value, &shapes, &mut before, &mut after)?
                    }
                    Stmt::AssignIndex { indices, value, .. } => {
                        for e in indices {
                            self.expr(e, &shapes, &mut before, &mut after)?;
                        }
                        self.expr(value, &shapes, &mut before, &mut after)?;
                    }
                    Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                        for e in items {
                            self.expr(e, &shapes, &mut before, &mut after)?;
                        }
                    }
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    } => {
                        self.expr(cond, &shapes, &mut before, &mut after)?;
                        self.scope(then_body, &shapes)?;
                        if let Some(b) = else_body {
                            self.scope(b, &shapes)?;
                        }
                    }
                    Stmt::Do {
                        start, end, body, ..
                    } => {
                        self.expr(start, &shapes, &mut before, &mut after)?;
                        self.expr(end, &shapes, &mut before, &mut after)?;
                        self.scope(body, &shapes)?;
                    }
                    Stmt::DoWhile { cond, body } => {
                        self.expr(cond, &shapes, &mut before, &mut after)?;
                        self.scope(body, &shapes)?;
                        if !before.is_empty() {
                            let value = self.capture(cond.clone(), TypeSpec::Logical, &mut before);
                            before.append(&mut after);
                            before.push(Stmt::If {
                                cond: Expr::Un(crate::ast::UnOp::Not, Box::new(id(&value))),
                                then_body: vec![Stmt::Exit],
                                else_body: None,
                            });
                            before.append(body);
                            *body = vec![Stmt::Block {
                                body: std::mem::take(&mut before),
                            }];
                            *cond = Expr::Logical(true);
                        }
                    }
                    Stmt::SelectCase {
                        expr,
                        cases,
                        default,
                    } => {
                        self.expr(expr, &shapes, &mut before, &mut after)?;
                        for case in cases {
                            self.scope(&mut case.body, &shapes)?;
                        }
                        if let Some(b) = default {
                            self.scope(b, &shapes)?;
                        }
                    }
                    Stmt::Function { body, .. }
                    | Stmt::Subroutine { body, .. }
                    | Stmt::Module { body, .. }
                    | Stmt::Block { body } => self.scope(body, &shapes)?,
                    _ => {}
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, offset))?;
            if let Stmt::Assign {
                name,
                value: Expr::Call(tag, a),
            } = &s
            {
                if tag == "@packed_view"
                    && matches!(shapes.get(name),Some((TypeSpec::Character(_),d)) if d.len()==1)
                {
                    let Expr::Ident(source) = &a[0] else {
                        unreachable!()
                    };
                    let index = self.fresh("copy");
                    let lower = match &shapes[name].1[0] {
                        Expr::Call(n, a) if n == "@declared_bounds" => a[0].clone(),
                        _ => int(1),
                    };
                    before.push(error(
                        bin(
                            BinOp::Ne,
                            a[1].clone(),
                            Expr::Call("size".into(), vec![id(name)]),
                        ),
                        "CHARACTER array assignment shapes do not conform",
                    ));
                    before.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![index.clone()],
                    });
                    s = Stmt::Do {
                        var: index.clone(),
                        start: int(1),
                        end: a[1].clone(),
                        body: vec![Stmt::Block {
                            body: vec![Stmt::AssignIndex {
                                name: name.clone(),
                                indices: vec![bin(
                                    BinOp::Add,
                                    lower,
                                    bin(BinOp::Sub, id(&index), int(1)),
                                )],
                                value: Expr::Index(source.clone(), vec![id(&index)]),
                            }],
                        }],
                    };
                }
            }
            if before.is_empty() && after.is_empty() {
                out.push(s);
            } else {
                before.push(s);
                before.append(&mut after);
                out.push(Stmt::Block { body: before });
            }
        }
        *statements = out;
        Ok(())
    }
}
impl Lower {
    // Retain the bounded snapshot optimization for the already-supported
    // numeric assignment form. Other forms use the expression buffer.
    fn general(e: &Expr, shapes: &Shapes) -> bool {
        match e {
            Expr::Ident(n) => shapes
                .get(n)
                .is_some_and(|(k, d)| !d.is_empty() || matches!(k, TypeSpec::Character(_))),
            Expr::Str(_) => true,
            Expr::Call(n, a) if n == "@output_implied_do" => {
                crate::parameters::integer_constant(&a[3]).is_err()
                    || a[4..].iter().any(|e| Self::general(e, shapes))
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                a.iter().any(|e| Self::general(e, shapes))
            }
            _ => false,
        }
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    fn functions(body: &[Stmt], shapes: &mut Shapes) {
        for s in body {
            match s {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    ..
                } => {
                    if let Some(kind) = crate::elemental::result_kind(name, return_type, body) {
                        shapes.insert(format!("@constructor_function:{name}"), (kind, vec![]));
                    }
                    functions(body, shapes);
                }
                Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    functions(body, shapes)
                }
                _ => {}
            }
        }
    }
    let mut shapes = Shapes::new();
    functions(&p.body, &mut shapes);
    Lower { serial: 0 }.scope(&mut p.body, &shapes)?;
    Ok(p)
}

// Materialize only implied-DO constructors whose choices depend on the active
// index. Other choices must first be branched by the conditional lowering pass.
pub(crate) fn conditional_items(s:&mut Stmt,shapes:&Shapes,serial:&mut usize)->Result<(Vec<Stmt>,Vec<Stmt>)> {
    fn walk(e:&mut Expr,shapes:&Shapes,lower:&mut Lower,before:&mut Vec<Stmt>,after:&mut Vec<Stmt>)->Result<()> {
        if crate::conditionals::choice(e) {return Ok(());}
        if matches!(e,Expr::Call(n,_) if n=="@array_constructor") && crate::constructors::delayed(e) {
            return lower.expr(e,shapes,before,after);
        }
        match e {
            Expr::Call(_,a)|Expr::Index(_,a)=>for e in a {walk(e,shapes,lower,before,after)?;},
            Expr::Bin(_,a,b)=>{walk(a,shapes,lower,before,after)?;walk(b,shapes,lower,before,after)?;},
            Expr::Un(_,a)=>walk(a,shapes,lower,before,after)?,_=>{},
        }
        Ok(())
    }
    let mut lower=Lower{serial:*serial};let mut before=vec![];let mut after=vec![];
    for e in crate::procedure_arguments::expressions(s) {walk(e,shapes,&mut lower,&mut before,&mut after)?;}
    *serial=lower.serial;Ok((before,after))
}

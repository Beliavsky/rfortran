// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Runtime CHARACTER arrays use the ordinary heap descriptor, with integer
//! character codes as backing storage. Shape/section lowering has already kept
//! the element dimensions and performed bounds checks before this storage pass.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{Context, Result};
use std::collections::HashMap;

fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
type Arrays = HashMap<String, usize>;

struct Lower {
    serial: usize,
}
impl Lower {
    fn capture(&mut self, kind: TypeSpec, value: Expr, out: &mut Vec<Stmt>) -> String {
        self.serial += 1;
        let n = format!("@character_heap_value_{}", self.serial);
        out.push(Stmt::VarDecl {
            kind,
            names: vec![n.clone()],
        });
        out.push(Stmt::Assign {
            name: n.clone(),
            value,
        });
        n
    }
    fn offset(&mut self, index: Expr, width: usize, out: &mut Vec<Stmt>) -> Expr {
        let n = self.capture(TypeSpec::Integer(Some(8)), index, out);
        bin(
            BinOp::Mul,
            bin(BinOp::Sub, id(&n), int(1)),
            int(width.max(1)),
        )
    }
    fn expression(&mut self, e: &mut Expr, arrays: &Arrays, out: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Index(n, args) | Expr::Call(n, args) if arrays.contains_key(n) => {
                let width = arrays[n];
                let [index] = args.as_mut_slice() else {
                    anyhow::bail!("heap CHARACTER element requires a linear index");
                };
                self.expression(index, arrays, out)?;
                let offset = self.offset(index.clone(), width, out);
                let temp = self.capture(
                    TypeSpec::Character(Some(width)),
                    Expr::Str(String::new()),
                    out,
                );
                let position = self.capture(TypeSpec::Integer(Some(8)), int(0), out);
                out.push(Stmt::Do {
                    var: position.clone(),
                    start: int(0),
                    end: Expr::IntLit((width as i64 - 1).to_string()),
                    body: vec![Stmt::CallSub {
                        name: "@character_code_store".into(),
                        args: vec![
                            id(&temp),
                            id(&position),
                            Expr::Index(
                                n.clone(),
                                vec![bin(
                                    BinOp::Add,
                                    offset,
                                    bin(BinOp::Add, id(&position), int(1)),
                                )],
                            ),
                        ],
                    }],
                });
                *e = id(&temp);
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    self.expression(arg, arrays, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, arrays, out)?;
                self.expression(b, arrays, out)?;
            }
            Expr::Un(_, a) => self.expression(a, arrays, out)?,
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Arrays) -> Result<()> {
        let mut arrays = inherited.clone();
        let mut shapes = HashMap::new();
        crate::allocatables::metadata(body, &mut shapes);
        for stmt in body.iter() {
            match stmt {
                Stmt::ArrayDecl {
                    name,
                    kind: TypeSpec::Character(width),
                    ..
                } if shapes.contains_key(&format!("@heap:{name}")) => {
                    arrays.insert(name.clone(), width.unwrap_or(1));
                }
                Stmt::ArrayDecl { name, .. } => {
                    arrays.remove(name);
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        arrays.remove(n);
                    }
                }
                _ => {}
            }
        }
        let mut result = vec![];
        for mut stmt in std::mem::take(body) {
            let mut prefix = vec![];
            match &mut stmt {
                Stmt::ArrayDecl { name, kind, .. } if arrays.contains_key(name) => {
                    *kind = TypeSpec::Integer(Some(8));
                    prefix.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@character_element_width:{}", arrays[name].max(1)),
                    });
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![format!("@character_heap_bytes${name}")],
                    });
                    prefix.push(Stmt::Assign {
                        name: format!("@character_heap_bytes${name}"),
                        value: int(0),
                    });
                }
                Stmt::DummyIntent { name, intent }
                    if arrays.contains_key(name) && intent.starts_with("@bounded:") =>
                {
                    let (_, flag) = intent
                        .strip_prefix("@bounded:")
                        .unwrap()
                        .split_once(':')
                        .context("invalid CHARACTER descriptor")?;
                    *intent = format!("@bounded:@character_heap_bytes${name}:{flag}");
                }
                Stmt::CallSub { name, args }
                    if name == "@heap_allocate"
                        && matches!(args.first(),Some(Expr::Ident(n)) if arrays.contains_key(n)) =>
                {
                    let Expr::Ident(n) = &args[0] else {
                        unreachable!()
                    };
                    let width = arrays[n].max(1);
                    let length = args[1].clone();
                    // Keep the public descriptor length in elements; the allocator
                    // and initialization mask count the underlying character codes.
                    let bytes = format!("@character_heap_bytes${n}");
                    args[1] = id(&bytes);
                    args.push(int(width));
                    result.extend(prefix);
                    result.push(stmt);
                    let Expr::Ident(length) = length else {
                        anyhow::bail!("CHARACTER allocation requires a length field");
                    };
                    result.push(Stmt::Assign {
                        name: length,
                        value: bin(BinOp::Div, id(&bytes), int(width)),
                    });
                    continue;
                }
                Stmt::CallSub { name, .. }
                    if matches!(
                        name.as_str(),
                        "@heap_bind" | "@heap_free" | "@heap_drop" | "@undefine"
                    ) => {}
                Stmt::CallSub { name, args }
                    if name == "@module_bind"
                        && matches!(args.first(),Some(Expr::Ident(n)) if arrays.contains_key(n)) =>
                {
                    let [Expr::Ident(n), Expr::Str(key), Expr::Str(encoded)] = args.as_mut_slice()
                    else {
                        anyhow::bail!("invalid CHARACTER module binding");
                    };
                    let mut storage: crate::modules::Storage = serde_json::from_str(encoded)?;
                    storage.kind = TypeSpec::Integer(Some(8));
                    storage.values = vec![int(0)];
                    *encoded = serde_json::to_string(&storage)?;
                    let bytes = crate::modules::Storage {
                        kind: TypeSpec::Integer(Some(8)),
                        values: vec![int(0)],
                        initialized: true,
                        rank: 0,
                        deferred: false,
                    };
                    prefix.push(Stmt::CallSub {
                        name: "@module_bind".into(),
                        args: vec![
                            id(&format!("@character_heap_bytes${n}")),
                            Expr::Str(format!("{key}.character_codes_length")),
                            Expr::Str(serde_json::to_string(&bytes)?),
                        ],
                    });
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if arrays.contains_key(name) => {
                    let width = arrays[name];
                    let [index] = indices.as_mut_slice() else {
                        anyhow::bail!("heap CHARACTER assignment requires a linear index");
                    };
                    self.expression(index, &arrays, &mut prefix)?;
                    let offset = self.offset(index.clone(), width, &mut prefix);
                    self.expression(value, &arrays, &mut prefix)?;
                    let temp =
                        self.capture(TypeSpec::Character(Some(width)), value.clone(), &mut prefix);
                    let position = self.capture(TypeSpec::Integer(Some(8)), int(0), &mut prefix);
                    let code = self.capture(TypeSpec::Integer(Some(8)), int(0), &mut prefix);
                    prefix.push(Stmt::Do {
                        var: position.clone(),
                        start: int(0),
                        end: Expr::IntLit((width as i64 - 1).to_string()),
                        body: vec![
                            Stmt::CallSub {
                                name: "@internal_read_byte".into(),
                                args: vec![id(&code), id(&temp), id(&position), int(width)],
                            },
                            Stmt::AssignIndex {
                                name: name.clone(),
                                indices: vec![bin(
                                    BinOp::Add,
                                    offset,
                                    bin(BinOp::Add, id(&position), int(1)),
                                )],
                                value: id(&code),
                            },
                        ],
                    });
                    result.extend(prefix);
                    continue;
                }
                Stmt::Assign { value, .. }
                | Stmt::AssignIndex { value, .. }
                | Stmt::Return(Some(value)) => self.expression(value, &arrays, &mut prefix)?,
                Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expression(e, &arrays, &mut prefix)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &arrays, &mut prefix)?;
                    self.body(then_body, &arrays)?;
                    if let Some(b) = else_body {
                        self.body(b, &arrays)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &arrays, &mut prefix)?;
                    self.expression(end, &arrays, &mut prefix)?;
                    self.body(body, &arrays)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &arrays, &mut prefix)?;
                    self.body(body, &arrays)?;
                    if !prefix.is_empty() {
                        prefix.push(Stmt::If {
                            cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        prefix.append(body);
                        *body = std::mem::take(&mut prefix);
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Block { body } | Stmt::Module { body, .. } => self.body(body, &arrays)?,
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                    self.body(body, &Arrays::new())?
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &arrays, &mut prefix)?;
                    for c in cases {
                        self.body(&mut c.body, &arrays)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &arrays)?;
                    }
                }
                _ => {}
            }
            result.extend(prefix);
            result.push(stmt);
        }
        *body = result;
        Ok(())
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    Lower { serial: 0 }
        .body(&mut p.body, &Arrays::new())
        .context("lowering runtime CHARACTER array storage")?;
    Ok(p)
}

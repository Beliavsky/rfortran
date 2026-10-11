// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve construct names before procedure specialization. Transfers to an
//! enclosing loop unwind intervening loops using ordinary EXIT, then perform
//! EXIT/CYCLE at the target. All backends share this control-flow lowering.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashSet;

pub(crate) fn parsed(stmt: Stmt, name: Option<String>, end: Option<String>) -> Stmt {
    if name.is_none() && end.is_none() {
        return stmt;
    }
    Stmt::Block {
        body: vec![
            Stmt::CallSub {
                name: "@named_loop".into(),
                args: vec![
                    Expr::Str(name.unwrap_or_default()),
                    Expr::Str(end.unwrap_or_default()),
                ],
            },
            stmt,
        ],
    }
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn set(n: &str, value: i64) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: int(value),
    }
}
fn equal(n: &str, value: i64) -> Expr {
    Expr::Bin(BinOp::Eq, Box::new(id(n)), Box::new(int(value)))
}

struct Loop {
    name: Option<String>,
    flag: Option<String>,
}
struct Lower {
    stack: Vec<Loop>,
    names: HashSet<String>,
    flags: Vec<String>,
}
impl Lower {
    fn scope(body: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
        let mut lower = Self {
            stack: vec![],
            names: HashSet::new(),
            flags: vec![],
        };
        lower.body(body, serial)?;
        if !lower.flags.is_empty() {
            let mut prefix = vec![Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: lower.flags.clone(),
            }];
            prefix.extend(lower.flags.iter().map(|n| set(n, 0)));
            prefix.append(body);
            *body = prefix;
        }
        Ok(())
    }

    fn loop_stmt(
        &mut self,
        stmt: &mut Stmt,
        name: Option<String>,
        serial: &mut usize,
    ) -> Result<()> {
        let flag = if let Some(name) = &name {
            if !self.names.insert(name.clone()) {
                bail!("duplicate DO construct name `{name}`");
            }
            *serial += 1;
            let flag = format!("@named_loop_signal_{serial}");
            self.flags.push(flag.clone());
            Some(flag)
        } else {
            None
        };
        self.stack.push(Loop { name, flag });
        match stmt {
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => self.body(body, serial)?,
            Stmt::Block { body } if matches!(body.first(),Some(Stmt::CallSub {name,..}) if name=="@do_step") =>
            {
                let mut inner = body.split_off(1);
                self.body(&mut inner, serial)?;
                body.extend(inner);
            }
            _ => bail!("construct name must identify a DO loop"),
        }
        self.stack.pop();
        Ok(())
    }

    fn dispatch(&self, output: &mut Vec<Stmt>) {
        for (index, outer) in self.stack.iter().enumerate() {
            let Some(flag) = &outer.flag else { continue };
            // At each level, leave the immediately enclosing loop until the
            // named target is reached. Its CYCLE then follows normal increment
            // handling, including explicit positive/negative DO steps.
            let transfer = if index + 1 == self.stack.len() {
                vec![Stmt::If {
                    cond: equal(flag, 1),
                    then_body: vec![set(flag, 0), Stmt::Exit],
                    else_body: Some(vec![set(flag, 0), Stmt::Cycle]),
                }]
            } else {
                vec![Stmt::Exit]
            };
            output.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(flag)), Box::new(int(0))),
                then_body: transfer,
                else_body: None,
            });
        }
    }

    fn body(&mut self, body: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
        let mut output = vec![];
        for mut stmt in std::mem::take(body) {
            let mut loop_name = None;
            if let Stmt::Block { body } = &mut stmt {
                if let Some(Stmt::CallSub { name, args }) = body.first() {
                    if name == "@named_loop" {
                        let [Expr::Str(name), Expr::Str(end)] = args.as_slice() else {
                            unreachable!()
                        };
                        if name.is_empty() {
                            bail!("END DO name `{end}` requires a named DO construct");
                        }
                        if end.is_empty() {
                            bail!("named DO construct `{name}` requires its name on END DO");
                        }
                        if !end.is_empty() && !name.eq_ignore_ascii_case(end) {
                            bail!("END DO name `{end}` does not match DO construct `{name}`");
                        }
                        loop_name = Some(name.to_ascii_lowercase());
                        let loop_stmt = body.pop().expect("named loop missing");
                        stmt = loop_stmt;
                    }
                }
            }
            let is_loop = matches!(&stmt, Stmt::Do { .. } | Stmt::DoWhile { .. })
                || matches!(&stmt,Stmt::Block {body} if matches!(body.first(),Some(Stmt::CallSub {name,..}) if name=="@do_step"));
            if is_loop {
                self.loop_stmt(&mut stmt, loop_name, serial)?;
                output.push(stmt);
                self.dispatch(&mut output);
                continue;
            }
            match &mut stmt {
                Stmt::CallSub { name, args } if name == "@named_exit" || name == "@named_cycle" => {
                    let [Expr::Str(target)] = args.as_slice() else {
                        unreachable!()
                    };
                    let Some(index) = self.stack.iter().rposition(|l| {
                        l.name
                            .as_ref()
                            .is_some_and(|n| n.eq_ignore_ascii_case(target))
                    }) else {
                        bail!(
                            "{} target `{target}` is not an enclosing DO construct",
                            if name == "@named_exit" {
                                "EXIT"
                            } else {
                                "CYCLE"
                            }
                        );
                    };
                    let exit = name == "@named_exit";
                    if index + 1 == self.stack.len() {
                        stmt = if exit { Stmt::Exit } else { Stmt::Cycle };
                    } else {
                        output.push(set(
                            self.stack[index].flag.as_ref().unwrap(),
                            if exit { 1 } else { 2 },
                        ));
                        stmt = Stmt::Exit;
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.body(then_body, serial)?;
                    if let Some(body) = else_body {
                        self.body(body, serial)?;
                    }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases {
                        self.body(&mut case.body, serial)?;
                    }
                    if let Some(body) = default {
                        self.body(body, serial)?;
                    }
                }
                Stmt::Block { body } => self.body(body, serial)?,
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } => Self::scope(body, serial)?,
                _ => {}
            }
            output.push(stmt);
        }
        *body = output;
        Ok(())
    }
}
pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    Lower::scope(&mut program.body, &mut 0)?;
    Ok(program)
}

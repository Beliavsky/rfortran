// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Stage owned record assignments before changing any destination allocation.
use super::*;
use crate::ast::BinOp;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
impl Lower {
    pub(super) fn owned_array_assignment(
        &mut self,
        statement: &Stmt,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let (name, target, mut value) = match statement {
            Stmt::Assign { name, value } => (name.clone(), id(name), value.clone()),
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => (
                name.clone(),
                Expr::Call(name.clone(), indices.clone()),
                value.clone(),
            ),
            _ => unreachable!(),
        };
        self.expression(&mut value, objects, prefix)?;
        let info = objects[&name].clone();
        let destination = self.pack_input(&target, info.clone(), objects, prefix)?;
        let source_info = self
            .pack_object(&value, objects)
            .context("owned record assignment requires a derived value")?;
        if source_info.ty != info.ty {
            bail!("derived array type mismatch");
        }
        let source = if source_info.dims.is_empty() && matches!(&value, Expr::Ident(_)) {
            let Expr::Ident(n) = &value else {
                unreachable!()
            };
            record_pack::Input::scalar(n.clone(), info.ty.clone())
        } else {
            self.pack_input(&value, source_info, objects, prefix)?
        };
        if !source.dims.is_empty() {
            if destination.dims.len() != source.dims.len() {
                bail!("record array assignment rank mismatch");
            }
            for (a, b) in destination.dims.iter().zip(&source.dims) {
                prefix.push(Stmt::If {
                    cond: Expr::Bin(BinOp::Ne, Box::new(a.clone()), Box::new(b.clone())),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("record array assignment shape mismatch".into())],
                    }],
                    else_body: None,
                });
            }
        }
        self.serial += 1;
        let temp = format!("@owned_record_copy_{}", self.serial);
        let index = format!("@owned_record_index_{}", self.serial);
        let length = destination.length();
        let mut generated = vec![
            Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![index.clone()],
            },
            Stmt::DerivedDecl {
                type_name: info.ty,
                names: vec![temp.clone()],
                attributes: vec!["allocatable".into()],
                polymorphic: false,
                dims: deferred(1),
            },
            Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(temp.clone(), vec![length.clone()])],
            },
            Stmt::Do {
                var: index.clone(),
                start: int(1),
                end: length.clone(),
                body: vec![Stmt::AssignIndex {
                    name: temp.clone(),
                    indices: vec![id(&index)],
                    value: source.element(id(&index)),
                }],
            },
            Stmt::Do {
                var: index.clone(),
                start: int(1),
                end: length,
                body: vec![assignment(
                    destination.element(id(&index)),
                    Expr::Call(temp.clone(), vec![id(&index)]),
                )],
            },
            Stmt::CallSub {
                name: "@deallocate".into(),
                args: vec![id(&temp)],
            },
        ];
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        Ok(())
    }
}

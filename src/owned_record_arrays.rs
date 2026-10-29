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
        if matches!(statement, Stmt::Assign { .. })
            && self.allocatable_roots.contains(&name)
            && self
                .pack_object(&value, objects)
                .is_some_and(|o| !o.dims.is_empty())
        {
            return self.allocatable_owned_assignment(&name, value, info, objects, prefix);
        }
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

    /// Stage the RHS before changing any owner descriptor, including shapes
    /// and bounds which may themselves belong to the destination (a=a(...)).
    fn allocatable_owned_assignment(
        &mut self,
        name: &str,
        value: Expr,
        info: Object,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let source_info = self
            .pack_object(&value, objects)
            .context("owned array assignment requires a derived value")?;
        if source_info.ty != info.ty {
            bail!("derived array type mismatch");
        }
        let source = self.pack_input(&value, source_info, objects, prefix)?;
        if source.dims.len() != info.dims.len() {
            bail!("record array assignment rank mismatch");
        }
        let mut dims = vec![];
        let mut bounds = vec![];
        for (axis, extent) in source.dims.iter().enumerate() {
            self.serial += 1;
            let size = format!("@owned_extent_{}", self.serial);
            let lower = format!("@owned_lower_{}", self.serial);
            let mut lo = if matches!(value, Expr::Ident(_)) {
                Expr::Call("lbound".into(), vec![value.clone(), int(axis as i64 + 1)])
            } else {
                int(1)
            };
            self.expression(&mut lo, objects, prefix)?;
            prefix.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![size.clone(), lower.clone()],
            });
            prefix.push(Stmt::Assign {
                name: size.clone(),
                value: extent.clone(),
            });
            prefix.push(Stmt::Assign {
                name: lower.clone(),
                value: lo,
            });
            dims.push(id(&size));
            bounds.push(Expr::Section {
                name: String::new(),
                lower: Some(Box::new(id(&lower))),
                upper: Some(Box::new(Expr::Bin(
                    BinOp::Sub,
                    Box::new(Expr::Bin(
                        BinOp::Add,
                        Box::new(id(&lower)),
                        Box::new(id(&size)),
                    )),
                    Box::new(int(1)),
                ))),
                stride: None,
            });
        }
        let length = dims
            .iter()
            .cloned()
            .reduce(|a, b| Expr::Bin(BinOp::Mul, Box::new(a), Box::new(b)))
            .unwrap();
        self.serial += 1;
        let temp = format!("@owned_allocatable_rhs_{}", self.serial);
        let index = format!("@owned_allocatable_index_{}", self.serial);
        let mut generated = vec![
            Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![index.clone()],
            },
            Stmt::DerivedDecl {
                type_name: info.ty.clone(),
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
        ];
        let allocate = Stmt::CallSub {
            name: "@allocate".into(),
            args: vec![Expr::Call(name.into(), bounds)],
        };
        let mismatch = dims
            .iter()
            .enumerate()
            .map(|(axis, size)| {
                Expr::Bin(
                    BinOp::Ne,
                    Box::new(Expr::Call(
                        "size".into(),
                        vec![id(name), int(axis as i64 + 1)],
                    )),
                    Box::new(size.clone()),
                )
            })
            .reduce(|a, b| Expr::Bin(BinOp::Or, Box::new(a), Box::new(b)))
            .unwrap();
        generated.push(Stmt::If {
            cond: Expr::Call("allocated".into(), vec![id(name)]),
            then_body: vec![Stmt::If {
                cond: mismatch,
                then_body: vec![
                    Stmt::CallSub {
                        name: "@deallocate".into(),
                        args: vec![id(name)],
                    },
                    allocate.clone(),
                ],
                else_body: None,
            }],
            else_body: Some(vec![allocate]),
        });
        // pack_input captures destination bounds here, after allocation in the
        // generated sequence, never while the destination is unallocated.
        let destination = self.pack_input(&id(name), info, objects, &mut generated)?;
        generated.push(Stmt::Do {
            var: index.clone(),
            start: int(1),
            end: length,
            body: vec![assignment(
                destination.element(id(&index)),
                Expr::Call(temp.clone(), vec![id(&index)]),
            )],
        });
        generated.push(Stmt::CallSub {
            name: "@deallocate".into(),
            args: vec![id(&temp)],
        });
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        Ok(())
    }
}

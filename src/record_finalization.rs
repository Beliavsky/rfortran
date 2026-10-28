// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Preserve array RHS values and designators across observable FINAL calls.
use super::*;
use crate::ast::BinOp;
impl Lower {
    fn final_indices(&mut self, indices: &mut [Expr], prefix: &mut Vec<Stmt>) {
        self.capture_indices(indices, prefix);
        for e in indices {
            match e {
                Expr::Ident(n)
                    if self
                        .array_shape(&Expr::Ident(n.clone()))
                        .is_none_or(|d| d.is_empty()) =>
                {
                    self.serial += 1;
                    let temp = format!("@final_index_value_{}", self.serial);
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![temp.clone()],
                    });
                    prefix.push(Stmt::Assign {
                        name: temp.clone(),
                        value: e.clone(),
                    });
                    *e = Expr::Ident(temp);
                }
                Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } => {
                    for bound in [lower, upper, stride].into_iter().flatten() {
                        self.final_indices(std::slice::from_mut(bound.as_mut()), prefix);
                    }
                }
                Expr::Call(n, a) if n == "@section_subscript" => self.final_indices(a, prefix),
                _ => {}
            }
        }
    }
    pub(super) fn final_assignment(
        &mut self,
        args: &[Expr],
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let [dispatch, target, value, Expr::Logical(allocatable)] = args else {
            bail!("invalid finalizable array assignment")
        };
        let mut target = target.clone();
        // Scalar derived components selected across an array retain its axes.
        if let Expr::Call(n, _) | Expr::Ident(n) = &mut target {
            let original = n.clone();
            let mut base = Expr::Ident(n.clone());
            if self.scalar_component_view(&mut base, objects, prefix)? {
                let Expr::Ident(alias) = base else {
                    unreachable!()
                };
                *n = alias;
            }
            if let Expr::Call(alias, indices) = &mut target {
                for index in indices {
                    retarget(index, &original, alias);
                    for (end, _) in original.match_indices('%') {
                        retarget(index, &original[..end], alias);
                    }
                }
            }
        }
        if let Expr::Call(_, indices) = &mut target {
            self.final_indices(indices, prefix);
        }
        let info = self
            .pack_object(&target, objects)
            .context("finalization assignment requires record storage")?;
        let destination = if *allocatable {
            None
        } else {
            Some(self.pack_input(&target, info.clone(), objects, prefix)?)
        };
        let rank = destination
            .as_ref()
            .map_or(info.dims.len(), |d| d.dims.len());
        let f = match dispatch {
            Expr::Str(f) => f.clone(),
            Expr::Call(n, fs) if n == "@final_helpers" => {
                let Some(Expr::Str(f)) = fs.get(rank) else {
                    bail!("invalid array finalization rank")
                };
                f.clone()
            }
            _ => bail!("invalid array finalization dispatch"),
        };
        let mut value = value.clone();
        self.expression(&mut value, objects, prefix)?;
        if matches!(&value,Expr::Call(n,_) if n=="@array_constructor"||n=="@record_constructor") {
            self.constructor_records(&mut value, objects, prefix)?;
        }
        if let Expr::Call(n, indices) = &mut value {
            if objects.contains_key(n) {
                self.final_indices(indices, prefix);
            }
        }
        let source_info = self
            .pack_object(&value, objects)
            .context("finalizable assignment requires a derived value")?;
        let source_dims = if source_info.dims.is_empty() {
            vec![]
        } else {
            self.pack_input(&value, source_info.clone(), objects, prefix)?
                .dims
        };
        let source_rank = source_dims.len();
        let mut allocation_bounds = source_dims.clone();
        if *allocatable && source_rank > 0 && matches!(&value, Expr::Ident(_)) {
            let mut lowers = (1..=source_rank)
                .map(|axis| {
                    Expr::Call(
                        "lbound".into(),
                        vec![value.clone(), Expr::IntLit(axis.to_string())],
                    )
                })
                .collect::<Vec<_>>();
            for lower in &mut lowers {
                self.expression(lower, objects, prefix)?;
            }
            self.capture_indices(&mut lowers, prefix);
            allocation_bounds = lowers
                .into_iter()
                .zip(&source_dims)
                .map(|(lower, extent)| Expr::Section {
                    name: String::new(),
                    lower: Some(Box::new(lower.clone())),
                    upper: Some(Box::new(Expr::Bin(
                        BinOp::Sub,
                        Box::new(Expr::Bin(
                            BinOp::Add,
                            Box::new(lower),
                            Box::new(extent.clone()),
                        )),
                        Box::new(Expr::IntLit("1".into())),
                    ))),
                    stride: None,
                })
                .collect();
        }
        if source_rank != 0 && source_rank != rank {
            bail!("record array assignment rank mismatch");
        }
        self.serial += 1;
        let temp = format!("@final_array_rhs_{}", self.serial);
        let mut generated = vec![Stmt::DerivedDecl {
            type_name: info.ty.clone(),
            names: vec![temp.clone()],
            attributes: if source_rank == 0 {
                vec![]
            } else {
                vec!["allocatable".into()]
            },
            polymorphic: false,
            dims: deferred(source_rank),
        }];
        if source_rank > 0 {
            generated.push(Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(temp.clone(), source_dims.clone())],
            });
        }
        generated.push(Stmt::Assign {
            name: temp.clone(),
            value,
        });
        if !allocatable && source_rank > 0 {
            for (a, b) in destination.as_ref().unwrap().dims.iter().zip(&source_dims) {
                generated.push(Stmt::If {
                    cond: Expr::Bin(BinOp::Ne, Box::new(a.clone()), Box::new(b.clone())),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str("record array assignment shape mismatch".into())],
                    }],
                    else_body: None,
                });
            }
        }
        let finalize = Stmt::CallSub {
            name: f,
            args: vec![target.clone()],
        };
        if *allocatable {
            let Expr::Ident(n) = &target else {
                bail!("allocatable finalization requires a whole array")
            };
            generated.push(Stmt::If {
                cond: Expr::Call("allocated".into(), vec![Expr::Ident(n.clone())]),
                then_body: vec![finalize],
                else_body: None,
            });
            if source_rank > 0 {
                let allocation = Stmt::CallSub {
                    name: "@allocate".into(),
                    args: vec![Expr::Call(n.clone(), allocation_bounds)],
                };
                let mismatch = source_dims
                    .iter()
                    .enumerate()
                    .map(|(axis, extent)| {
                        Expr::Bin(
                            BinOp::Ne,
                            Box::new(Expr::Call(
                                "size".into(),
                                vec![Expr::Ident(n.clone()), Expr::IntLit((axis + 1).to_string())],
                            )),
                            Box::new(extent.clone()),
                        )
                    })
                    .reduce(|a, b| Expr::Bin(BinOp::Or, Box::new(a), Box::new(b)))
                    .unwrap();
                generated.push(Stmt::If {
                    cond: Expr::Call("allocated".into(), vec![Expr::Ident(n.clone())]),
                    then_body: vec![Stmt::If {
                        cond: mismatch,
                        then_body: vec![
                            Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![Expr::Ident(n.clone())],
                            },
                            allocation.clone(),
                        ],
                        else_body: None,
                    }],
                    else_body: Some(vec![allocation]),
                });
            }
        } else {
            generated.push(finalize);
        }
        if !allocatable && rank > 0 && source_rank > 0 {
            // The snapshot has runtime descriptor storage even when the
            // destination is fixed. Copy elements using the captured mapping,
            // rather than asking fixed-section lowering to infer its capacity.
            self.serial += 1;
            let index = format!("@final_copy_index_{}", self.serial);
            let mut ordinal = Expr::Bin(
                BinOp::Sub,
                Box::new(Expr::Ident(index.clone())),
                Box::new(Expr::IntLit("1".into())),
            );
            let mut indices = vec![];
            for extent in &source_dims {
                indices.push(Expr::Bin(
                    BinOp::Add,
                    Box::new(Expr::Call(
                        "mod".into(),
                        vec![ordinal.clone(), extent.clone()],
                    )),
                    Box::new(Expr::IntLit("1".into())),
                ));
                ordinal = Expr::Bin(BinOp::Div, Box::new(ordinal), Box::new(extent.clone()));
            }
            generated.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![index.clone()],
            });
            let destination = destination.as_ref().unwrap();
            generated.push(Stmt::Do {
                var: index.clone(),
                start: Expr::IntLit("1".into()),
                end: destination.length(),
                body: vec![assignment(
                    destination.element(Expr::Ident(index)),
                    Expr::Call(temp.clone(), indices),
                )],
            });
        } else {
            generated.push(match target {
                Expr::Ident(name) => Stmt::Assign {
                    name,
                    value: Expr::Ident(temp.clone()),
                },
                target => assignment(target, Expr::Ident(temp.clone())),
            });
        }
        if source_rank > 0 {
            generated.push(Stmt::CallSub {
                name: "@deallocate".into(),
                args: vec![Expr::Ident(temp)],
            });
        }
        self.body(&mut generated, objects)?;
        prefix.extend(generated);
        Ok(())
    }
}

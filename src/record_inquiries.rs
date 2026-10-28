// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Project record-array inquiries past leading fixed component axes.
use super::*;
use crate::ast::BinOp;

impl Lower {
    pub(super) fn record_inquiry(
        &mut self,
        name: &str,
        args: &[Expr],
        objects: &Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Option<Expr>> {
        let (array, dim, kind) = if matches!(name, "allocated" | "present") {
            let Some(array) = args.first() else {
                return Ok(None);
            };
            (array.clone(), None, None)
        } else {
            crate::inquiries::arguments(name, args)?
        };
        let (owner, indices) = match &array {
            Expr::Ident(n) => (n, None),
            Expr::Call(n, a) | Expr::Index(n, a) => (n, Some(a.clone())),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => (
                name,
                Some(vec![Expr::Section {
                    name: String::new(),
                    lower: lower.clone(),
                    upper: upper.clone(),
                    stride: stride.clone(),
                }]),
            ),
            _ => return Ok(None),
        };
        let Some(object) = objects.get(owner).filter(|o| !o.dims.is_empty()) else {
            return Ok(None);
        };
        let (field, offset) = self.shape_field(&object.ty)?;
        if offset == 0 {
            return Ok(None);
        }
        let field = self
            .fields(&object.ty)?
            .into_iter()
            .find(|f| f.name == field)
            .unwrap();
        let (storage, rank) = if let Some(mut indices) = indices {
            self.capture_indices(&mut indices, prefix);
            let rank = indices.iter().filter(|i| matches!(i, Expr::Section {..})
                || matches!(i, Expr::Call(n, _) if n=="@section_subscript" || n=="@vector_subscript")
                || self.array_shape(i).is_some_and(|d| !d.is_empty())).count();
            (select(owner, &field, vec![], &indices)?, rank)
        } else {
            (Expr::Ident(storage(owner, &field.name)), object.dims.len())
        };
        if matches!(name, "allocated" | "present") {
            return Ok(Some(Expr::Call(name.into(), vec![storage])));
        }
        let int = |n: usize| Expr::IntLit(n.to_string());
        let bin = |op, a, b| Expr::Bin(op, Box::new(a), Box::new(b));
        let no_dim = |e: &Expr| matches!(e, Expr::Call(n, _) if n=="@no_dim");
        let dim = dim.as_ref().filter(|d| !no_dim(d));
        let kind = kind.as_ref();
        let inquiry = |intrinsic: &str, axis: Expr| {
            let mut a = vec![storage.clone(), axis];
            if let Some(kind) = kind {
                a.push(kind.clone());
            }
            Expr::Call(intrinsic.into(), a)
        };
        if let Some(dim) = dim {
            let axis = if let Ok(axis) = crate::parameters::integer_constant(dim) {
                if axis < 1 || axis as usize > rank {
                    bail!("{name}: DIM must be between 1 and {rank}");
                }
                int(axis as usize + offset)
            } else {
                self.serial += 1;
                let temp = format!("@record_inquiry_dim_{}", self.serial);
                prefix.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![temp.clone()],
                });
                prefix.push(Stmt::Assign {
                    name: temp.clone(),
                    value: dim.clone(),
                });
                let dim = Expr::Ident(temp);
                prefix.push(Stmt::If {
                    cond: bin(
                        BinOp::Or,
                        bin(BinOp::Lt, dim.clone(), int(1)),
                        bin(BinOp::Gt, dim.clone(), int(rank)),
                    ),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str(format!(
                            "{name}: DIM must be between 1 and {rank}"
                        ))],
                    }],
                    else_body: None,
                });
                let result = format!("@record_inquiry_value_{}", self.serial);
                prefix.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![result.clone()],
                });
                for axis in 1..=rank {
                    prefix.push(Stmt::If {
                        cond: bin(BinOp::Eq, dim.clone(), int(axis)),
                        then_body: vec![Stmt::Assign {
                            name: result.clone(),
                            value: inquiry(name, int(axis + offset)),
                        }],
                        else_body: None,
                    });
                }
                let mut converted = vec![Expr::Ident(result)];
                if let Some(kind) = kind {
                    converted.push(kind.clone());
                }
                return Ok(Some(Expr::Call("int".into(), converted)));
            };
            return Ok(Some(inquiry(name, axis)));
        }
        let values = (1..=rank)
            .map(|axis| {
                inquiry(
                    if name == "shape" { "size" } else { name },
                    int(axis + offset),
                )
            })
            .collect::<Vec<_>>();
        Ok(Some(if name == "size" {
            values
                .into_iter()
                .reduce(|a, b| bin(BinOp::Mul, a, b))
                .unwrap_or_else(|| int(1))
        } else {
            Expr::Call("@array_constructor".into(), values)
        }))
    }
}

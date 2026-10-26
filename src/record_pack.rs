// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Preserve record identity/rank while sharing a single PACK mask across fields.
use super::*;
use crate::ast::BinOp;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn fail(cond: Expr, message: &str) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message.into())],
        }],
        else_body: None,
    }
}

pub(super) struct Input {
    scalar: bool,
    name: String,
    ty: String,
    // An axis is either a scalar subscript or (first, stride, extent, vector).
    axes: Vec<(Expr, Option<(Expr, Expr, bool)>)>,
    pub(super) dims: Vec<Expr>,
}
impl Input {
    pub(super) fn scalar(name: String, ty: String) -> Self {
        Self {
            name,
            ty,
            scalar: true,
            axes: vec![],
            dims: vec![],
        }
    }
    pub(super) fn length(&self) -> Expr {
        self.dims
            .iter()
            .cloned()
            .reduce(|a, b| bin(BinOp::Mul, a, b))
            .unwrap_or(int(1))
    }
    pub(super) fn element(&self, ordinal: Expr) -> Expr {
        if self.scalar { id(&self.name) } else { Expr::Call(self.name.clone(), self.at(ordinal).0) }
    }
    fn at(&self, ordinal: Expr) -> (Vec<Expr>, Vec<Expr>) {
        let mut offset = bin(BinOp::Sub, ordinal, int(1));
        let (mut indices, mut mask) = (vec![], vec![]);
        for (first, axis) in &self.axes {
            if let Some((step, extent, vector)) = axis {
                let position = bin(
                    BinOp::Add,
                    Expr::Call("mod".into(), vec![offset.clone(), extent.clone()]),
                    int(1),
                );
                mask.push(position.clone());
                indices.push(if *vector {
                    let Expr::Ident(name) = first else {
                        unreachable!()
                    };
                    Expr::Index(name.clone(), vec![position])
                } else {
                    bin(
                        BinOp::Add,
                        first.clone(),
                        bin(BinOp::Mul, bin(BinOp::Sub, position, int(1)), step.clone()),
                    )
                });
                offset = bin(BinOp::Div, offset, extent.clone());
            } else {
                indices.push(first.clone());
            }
        }
        (indices, mask)
    }
}

impl Lower {
    pub(super) fn pack_object(&self, value: &Expr, objects: &Objects) -> Option<Object> {
        match value {
            Expr::Ident(n) => objects.get(n).cloned(),
            Expr::Section { name, .. } => objects.get(name).map(|o| Object {
                ty: o.ty.clone(),
                dims: deferred(1),
            }),
            Expr::Call(n, args) | Expr::Index(n, args) if objects.contains_key(n) => {
                let rank = args
                    .iter()
                    .map(|arg| {
                        if matches!(arg, Expr::Section { .. })
                            || matches!(arg, Expr::Call(tag, _) if tag=="@section_subscript")
                        {
                            1
                        } else {
                            self.array_shape(arg).map_or(0, |dims| dims.len())
                        }
                    })
                    .sum();
                Some(Object {
                    ty: objects[n].ty.clone(),
                    dims: deferred(rank),
                })
            }
            Expr::Call(n, args) if n == "@array_constructor" => args
                .first()
                .and_then(|arg| self.pack_object(arg, objects))
                .map(|o| Object {
                    ty: o.ty,
                    dims: deferred(1),
                }),
            Expr::Call(n, args) if n == "@record_constructor" => match args.first() {
                Some(Expr::Ident(ty)) => Some(Object {
                    ty: ty.clone(),
                    dims: deferred(1),
                }),
                _ => None,
            },
            Expr::Call(n, _) if self.types.contains_key(n) => Some(Object {
                ty: n.clone(),
                dims: vec![],
            }),
            Expr::Call(n, _) if self.procedures.get(n).is_some_and(|p| p.result.is_some()) => {
                self.procedures[n].result.as_ref().map(|(_, o)| o.clone())
            }
            Expr::Call(n, _) => self.scalar_results.get(n).map(|ty| Object {
                ty: ty.clone(),
                dims: vec![],
            }),
            _ => None,
        }
    }

    pub(super) fn pack_input(
        &mut self,
        value: &Expr,
        info: Object,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Input> {
        let (name, mut indices) = match value {
            Expr::Ident(n) => (
                n.clone(),
                vec![
                    Expr::Section {
                        name: String::new(),
                        lower: None,
                        upper: None,
                        stride: None
                    };
                    info.dims.len()
                ],
            ),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => (
                name.clone(),
                vec![Expr::Section {
                    name: String::new(),
                    lower: lower.clone(),
                    upper: upper.clone(),
                    stride: stride.clone(),
                }],
            ),
            Expr::Call(n, args) | Expr::Index(n, args) if objects.contains_key(n) => {
                (n.clone(), args.clone())
            }
            Expr::Call(tag, _) if tag == "@array_constructor" || tag == "@record_constructor" => {
                let mut value = value.clone();
                self.constructor_records(&mut value, objects, prefix)?;
                let info = self
                    .pack_object(&value, objects)
                    .context("invalid record constructor")?;
                return self.pack_input(&value, info, objects, prefix);
            }
            _ => bail!("PACK ARRAY requires supported record-array storage"),
        };
        self.capture_indices(&mut indices, prefix);
        let (field, offset) = self.shape_field(&info.ty)?;
        let source = storage(&name, &field);
        let (mut axes, mut dims) = (vec![], vec![]);
        for (axis, index) in indices.into_iter().enumerate() {
            let section = match &index {
                Expr::Section { .. } => Some(index.clone()),
                Expr::Call(tag, args) if tag == "@section_subscript" => args.first().cloned(),
                _ => None,
            };
            let vector =
                section.is_none() && self.array_shape(&index).is_some_and(|d| !d.is_empty());
            let (first, step, extent) = if let Some(Expr::Section {
                lower,
                upper,
                stride,
                ..
            }) = section
            {
                let bound = |n: &str| {
                    Expr::Call(n.into(), vec![id(&source), int((axis + offset + 1) as i64)])
                };
                let mut bounds = [
                    lower.map(|e| *e).unwrap_or_else(|| bound("lbound")),
                    upper.map(|e| *e).unwrap_or_else(|| bound("ubound")),
                    stride.map(|e| *e).unwrap_or(int(1)),
                ];
                self.capture_indices(&mut bounds, prefix);
                let [first, last, step] = bounds;
                prefix.push(fail(
                    bin(BinOp::Eq, step.clone(), int(0)),
                    "PACK section stride must not be zero",
                ));
                let extent = bin(
                    BinOp::Div,
                    bin(
                        BinOp::Add,
                        bin(BinOp::Sub, last, first.clone()),
                        step.clone(),
                    ),
                    step.clone(),
                );
                (first, step, extent)
            } else if vector {
                (
                    index.clone(),
                    int(1),
                    Expr::Call("size".into(), vec![index]),
                )
            } else {
                axes.push((index, None));
                continue;
            };
            self.serial += 1;
            let extent_name = format!("@record_pack_extent_{}", self.serial);
            prefix.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![extent_name.clone()],
            });
            prefix.push(assign(&extent_name, extent));
            prefix.push(Stmt::If {
                cond: bin(BinOp::Lt, id(&extent_name), int(0)),
                then_body: vec![assign(&extent_name, int(0))],
                else_body: None,
            });
            dims.push(id(&extent_name));
            axes.push((first, Some((step, id(&extent_name), vector))));
        }
        Ok(Input {
            scalar: false,
            name,
            ty: info.ty,
            axes,
            dims,
        })
    }

    pub(super) fn pack_copy(
        &mut self,
        source: &Input,
        fields: &[Field],
        ordinal: Expr,
        target: &str,
        position: Expr,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Vec<Stmt>> {
        let source_indices = source.at(ordinal).0;
        let mut body = vec![];
        for field in fields {
            let mut from = vec![];
            let mut to = vec![];
            let mut component_loop = None;
            if !field.dims.is_empty() {
                self.serial += 1;
                let index = format!("@record_pack_component_{}", self.serial);
                prefix.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![index.clone()],
                });
                let mut ordinal = bin(BinOp::Sub, id(&index), int(1));
                let mut count = 1i64;
                for d in &field.dims {
                    let extent = crate::parameters::integer_constant(d)?.max(0);
                    count = count
                        .checked_mul(extent)
                        .context("PACK component extent overflow")?;
                    let at = bin(
                        BinOp::Add,
                        Expr::Call("mod".into(), vec![ordinal.clone(), int(extent)]),
                        int(1),
                    );
                    from.push(at.clone());
                    to.push(at);
                    ordinal = bin(BinOp::Div, ordinal, int(extent));
                }
                component_loop = Some((index, count));
            }
            from.extend(source_indices.clone());
            to.push(position.clone());
            if matches!(component_loop, Some((_, 0))) {
                continue;
            }
            let from_name = if source.scalar {
                format!("{}%{}", source.name, field.name)
            } else {
                storage(&source.name, &field.name)
            };
            let value = if from.is_empty() {
                id(&from_name)
            } else {
                Expr::Index(from_name, from)
            };
            let set = Stmt::AssignIndex {
                name: storage(target, &field.name),
                indices: to,
                value,
            };
            body.push(if let Some((var, count)) = component_loop {
                Stmt::Do {
                    var,
                    start: int(1),
                    end: int(count),
                    body: vec![set],
                }
            } else {
                set
            });
        }
        Ok(body)
    }

    /// Actual expressions have already been lowered; do not visit them again.
    pub(super) fn pack_records(
        &mut self,
        args: &[Expr],
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Option<String>> {
        let Some(source) = args.first().and_then(|arg| self.pack_object(arg, objects)) else {
            return Ok(None);
        };
        if source.dims.is_empty() {
            bail!("PACK ARRAY must be an array of derived type");
        }
        if !(2..=3).contains(&args.len()) {
            bail!("PACK expects ARRAY, MASK and optional VECTOR");
        }
        let fields = self.fields(&source.ty)?;
        if fields.iter().any(|f| f.heap.is_some()) {
            bail!("PACK of records with owned allocatable components is not yet supported");
        }
        let source = self.pack_input(&args[0], source, objects, prefix)?;
        let vector = args.get(2).filter(|v| {
            !crate::named_arguments::omitted(v)
                && !matches!(v, Expr::Call(tag,_) if tag=="@no_vector")
        });
        let vector = if let Some(vector) = vector {
            let info = self
                .pack_object(vector, objects)
                .context("PACK VECTOR must be an array of the same derived type as ARRAY")?;
            if info.ty != source.ty {
                bail!("PACK ARRAY and VECTOR must have the same derived type");
            }
            if info.dims.len() != 1 {
                bail!("PACK VECTOR must be rank one");
            }
            Some(self.pack_input(vector, info, objects, prefix)?)
        } else {
            None
        };
        self.serial += 1;
        let result = format!("@record_pack_{}", self.serial);
        let mask = format!("{result}_mask");
        let mut mask_value = args[1].clone();
        if matches!(&mask_value,Expr::Call(n,_) if self.scalar_functions.contains(n) && !self.elemental_functions.contains(n))
        {
            let scalar = format!("{result}_scalar_mask");
            prefix.push(Stmt::VarDecl {
                kind: TypeSpec::Logical,
                names: vec![scalar.clone()],
            });
            prefix.push(assign(&scalar, mask_value));
            mask_value = id(&scalar);
        }
        prefix.push(Stmt::ArrayDecl {
            kind: TypeSpec::Logical,
            name: mask.clone(),
            dims: deferred(source.dims.len()),
        });
        prefix.push(Stmt::DummyIntent {
            name: mask.clone(),
            intent: "allocatable".into(),
        });
        prefix.push(Stmt::CallSub {
            name: "@allocate".into(),
            args: vec![Expr::Call(mask.clone(), source.dims.clone())],
        });
        prefix.push(Stmt::Assign {
            name: mask.clone(),
            value: mask_value,
        });
        // Whole allocatable assignment can resize the mask temporary. PACK
        // requires conformance with ARRAY rather than accepting that resize.
        for (axis, extent) in source.dims.iter().enumerate() {
            prefix.push(fail(
                bin(
                    BinOp::Ne,
                    Expr::Call("size".into(), vec![id(&mask), int((axis + 1) as i64)]),
                    extent.clone(),
                ),
                "PACK MASK shape must conform to ARRAY",
            ));
        }
        self.array_shapes
            .insert(mask.clone(), deferred(source.dims.len()));
        let index = format!("{result}_index");
        let count = format!("{result}_count");
        let cursor = format!("{result}_cursor");
        prefix.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![index.clone(), count.clone(), cursor.clone()],
        });
        let total = source
            .dims
            .iter()
            .cloned()
            .reduce(|a, b| bin(BinOp::Mul, a, b))
            .unwrap();
        let selected = Expr::Index(mask, source.at(id(&index)).1);
        prefix.push(assign(&count, int(0)));
        prefix.push(Stmt::Do {
            var: index.clone(),
            start: int(1),
            end: total.clone(),
            body: vec![Stmt::If {
                cond: selected.clone(),
                then_body: vec![assign(&count, bin(BinOp::Add, id(&count), int(1)))],
                else_body: None,
            }],
        });
        let length = vector
            .as_ref()
            .map_or_else(|| id(&count), |v| v.dims[0].clone());
        if vector.is_some() {
            prefix.push(fail(
                bin(BinOp::Gt, id(&count), length.clone()),
                "PACK VECTOR has too few elements for MASK",
            ));
        }
        for field in &fields {
            let name = storage(&result, &field.name);
            prefix.push(Stmt::ArrayDecl {
                kind: field.kind.clone(),
                name: name.clone(),
                dims: deferred(field.dims.len() + 1),
            });
            prefix.push(Stmt::DummyIntent {
                name: name.clone(),
                intent: "allocatable".into(),
            });
            let mut dims = field.dims.clone();
            dims.push(length.clone());
            prefix.push(Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(name.clone(), dims)],
            });
            self.array_shapes
                .insert(name, deferred(field.dims.len() + 1));
        }
        prefix.push(assign(&cursor, int(0)));
        let mut copies = vec![assign(&cursor, bin(BinOp::Add, id(&cursor), int(1)))];
        copies.extend(self.pack_copy(
            &source,
            &fields,
            id(&index),
            &result,
            id(&cursor),
            prefix,
        )?);
        prefix.push(Stmt::Do {
            var: index.clone(),
            start: int(1),
            end: total,
            body: vec![Stmt::If {
                cond: selected,
                then_body: copies,
                else_body: None,
            }],
        });
        if let Some(vector) = vector {
            let copies =
                self.pack_copy(&vector, &fields, id(&index), &result, id(&index), prefix)?;
            prefix.push(Stmt::Do {
                var: index,
                start: bin(BinOp::Add, id(&count), int(1)),
                end: length,
                body: copies,
            });
        }
        objects.insert(
            result.clone(),
            Object {
                ty: source.ty,
                dims: deferred(1),
            },
        );
        Ok(Some(result))
    }
}

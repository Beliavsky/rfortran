// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Materialize CHARACTER MERGE before CHARACTER arrays become scalar slots.
use super::*;

impl Lower {
    pub(super) fn merge_character(
        &mut self,
        e: &mut Expr,
        symbols: &mut Symbols,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(name, args) = e else {
            return Ok(false);
        };
        if name != "merge" || self.returns.contains_key(name) || symbols.contains_key(name) {
            return Ok(false);
        }
        if args.len() != 3 {
            bail!("MERGE expects TSOURCE, FSOURCE and MASK");
        }
        if args.iter().any(|a| runtime_array(a, symbols)) {
            return self
                .merge_runtime(e, symbols, prefix)
                .context("runtime CHARACTER MERGE");
        }
        let first = self.view(&args[0], symbols);
        let second = self.view(&args[1], symbols);
        if !first
            .as_ref()
            .is_ok_and(|v| matches!(v.kind, TypeSpec::Character(_)))
            && !second
                .as_ref()
                .is_ok_and(|v| matches!(v.kind, TypeSpec::Character(_)))
        {
            return Ok(false);
        }
        let views = (|| -> Result<Vec<Value>> {
            Ok(vec![first?, second?, self.view(&args[2], symbols)?])
        })();
        let views = match views {
            Ok(v) => v,
            Err(_) if self.defer_calls => return Ok(false),
            Err(e) => return Err(e),
        };
        if !matches!(views[0].kind, TypeSpec::Character(_))
            || !matches!(views[1].kind, TypeSpec::Character(_))
            || length(&views[0].kind)? != length(&views[1].kind)?
        {
            bail!("MERGE sources must have the same type, kind and CHARACTER length");
        }
        if views[2].kind != TypeSpec::Logical {
            bail!("MERGE MASK must be LOGICAL");
        }
        if views.iter().any(|v| v.active.is_some()) {
            if self.defer_calls {
                return Ok(false);
            }
            bail!("CHARACTER MERGE currently requires fixed array extents");
        }
        let mut count = None;
        let mut shape = None;
        for (arg, view) in args.iter().zip(&views) {
            if let Some(items) = &view.items {
                if count.is_some_and(|n| n != items.len()) {
                    bail!("MERGE argument shapes do not conform");
                }
                count = Some(items.len());
                let dims = crate::multidim::fixed_actual_shape(arg, symbols)?
                    .unwrap_or_else(|| vec![items.len() as i64]);
                if shape.as_ref().is_some_and(|previous| previous != &dims) {
                    bail!("MERGE argument shapes do not conform");
                }
                shape = Some(dims);
            }
        }
        self.serial += 1;
        let result = format!("@character_merge_{}", self.serial);
        let kind = views[0].kind.clone();
        let dims = shape.unwrap_or_default();
        prefix.push(if count.is_some() {
            Stmt::ArrayDecl {
                kind: kind.clone(),
                name: result.clone(),
                dims: dims.iter().copied().map(int).collect(),
            }
        } else {
            Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![result.clone()],
            }
        });
        // A scalar mask expression is a single argument, even when broadcast.
        let scalar_mask = if views[2].items.is_none() {
            let mask = format!("@character_merge_mask_{}", self.serial);
            prefix.push(Stmt::VarDecl {
                kind: TypeSpec::Logical,
                names: vec![mask.clone()],
            });
            prefix.push(Stmt::Assign {
                name: mask.clone(),
                value: args[2].clone(),
            });
            Some(Expr::Ident(mask))
        } else {
            None
        };
        for i in 0..count.unwrap_or(1) {
            let item = |which: usize| {
                views[which]
                    .items
                    .as_ref()
                    .map_or_else(|| args[which].clone(), |v| v[i].clone())
            };
            let assign = |value| {
                if count.is_some() {
                    Stmt::AssignIndex {
                        name: result.clone(),
                        indices: array_indices(i, &dims),
                        value,
                    }
                } else {
                    Stmt::Assign {
                        name: result.clone(),
                        value,
                    }
                }
            };
            prefix.push(Stmt::If {
                cond: scalar_mask.clone().unwrap_or_else(|| item(2)),
                then_body: vec![assign(item(0))],
                else_body: Some(vec![assign(item(1))]),
            });
        }
        symbols.insert(result.clone(), (kind, dims.into_iter().map(int).collect()));
        *e = Expr::Ident(result);
        Ok(true)
    }
}

// Keep descriptor extents out of the bounded, compile-time element expansion.
fn runtime_array(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Ident(n) => crate::allocatables::length(n, symbols).is_some(),
        Expr::Call(n, _) if n == "@packed_view" => true,
        Expr::Section { name, .. } => crate::allocatables::length(name, symbols).is_some(),
        Expr::Bin(_, a, b) => runtime_array(a, symbols) || runtime_array(b, symbols),
        Expr::Un(_, a) => runtime_array(a, symbols),
        _ => false,
    }
}
fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn guard(a: Expr, b: Expr) -> Stmt {
    Stmt::If {
        cond: binary(BinOp::Ne, a, b),
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str("MERGE argument shapes do not conform".into())],
        }],
        else_body: None,
    }
}
struct RuntimeValue {
    kind: TypeSpec,
    element: Expr,
    axes: Vec<Expr>,
}
fn conform(a: &[Expr], b: &[Expr], prefix: &mut Vec<Stmt>) -> Result<()> {
    if a.is_empty() || b.is_empty() {
        return Ok(());
    }
    if a.len() != b.len() {
        bail!("MERGE argument shapes do not conform");
    }
    for (a, b) in a.iter().zip(b) {
        prefix.push(guard(a.clone(), b.clone()));
    }
    Ok(())
}
impl Lower {
    fn runtime_merge_value(
        &self,
        e: &Expr,
        index: &Expr,
        symbols: &Symbols,
        prefix: &mut Vec<Stmt>,
    ) -> Result<RuntimeValue> {
        match e {
            Expr::Ident(n) if symbols.get(n).is_some_and(|v| !v.1.is_empty()) => {
                let (kind, dims) = &symbols[n];
                let axes = crate::allocatables::axes(n, symbols).unwrap_or_else(|| {
                    crate::allocatables::length(n, symbols)
                        .map(|v| vec![v])
                        .unwrap_or(dims.clone())
                });
                if let Some(flag) = crate::allocatables::allocated(n, symbols) {
                    prefix.push(Stmt::If {
                        cond: Expr::Un(crate::ast::UnOp::Not, Box::new(flag)),
                        then_body: vec![Stmt::CallSub {
                            name: "@error_stop".into(),
                            args: vec![Expr::Str(format!("read of unallocated array `{n}`"))],
                        }],
                        else_body: None,
                    });
                }
                let lowers = crate::allocatables::lowers(n, symbols)
                    .unwrap_or_else(|| vec![int(1); axes.len()]);
                let mut ordinal = binary(BinOp::Sub, index.clone(), int(1));
                let mut indices = vec![];
                for (axis, lower) in axes.iter().zip(lowers) {
                    let extent = axis.clone();
                    indices.push(binary(
                        BinOp::Add,
                        lower,
                        Expr::Call("mod".into(), vec![ordinal.clone(), extent.clone()]),
                    ));
                    ordinal = binary(BinOp::Div, ordinal, extent);
                }
                Ok(RuntimeValue {
                    kind: kind.clone(),
                    element: Expr::Index(n.clone(), indices),
                    axes,
                })
            }
            Expr::Call(n, a) if n == "@packed_view" => {
                let mut v = self.runtime_merge_value(&a[0], index, symbols, prefix)?;
                if v.axes.len() <= 1 {
                    v.axes = vec![a[1].clone()];
                }
                Ok(v)
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                let mut v =
                    self.runtime_merge_value(&Expr::Ident(name.clone()), index, symbols, prefix)?;
                if v.axes.len() != 1 {
                    bail!("CHARACTER MERGE section requires rank one");
                }
                let step = stride
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()?
                    .unwrap_or(1);
                if step == 0 {
                    bail!("array section stride must not be zero");
                }
                let first = crate::allocatables::lowers(name, symbols)
                    .and_then(|v| v.first().cloned())
                    .unwrap_or(int(1));
                let last = binary(
                    BinOp::Sub,
                    binary(BinOp::Add, first.clone(), v.axes[0].clone()),
                    int(1),
                );
                let lo = lower.as_deref().cloned().unwrap_or_else(|| {
                    if step < 0 {
                        last.clone()
                    } else {
                        first.clone()
                    }
                });
                let hi = upper
                    .as_deref()
                    .cloned()
                    .unwrap_or(if step < 0 { first } else { last });
                v.axes = vec![Expr::Call(
                    "max".into(),
                    vec![
                        int(0),
                        binary(
                            BinOp::Div,
                            binary(BinOp::Add, binary(BinOp::Sub, hi, lo.clone()), int(step)),
                            int(step),
                        ),
                    ],
                )];
                v.element = Expr::Index(
                    name.clone(),
                    vec![binary(
                        BinOp::Add,
                        lo,
                        binary(
                            BinOp::Mul,
                            binary(BinOp::Sub, index.clone(), int(1)),
                            int(step),
                        ),
                    )],
                );
                Ok(v)
            }
            Expr::Bin(op, a, b) => {
                let a = self.runtime_merge_value(a, index, symbols, prefix)?;
                let b = self.runtime_merge_value(b, index, symbols, prefix)?;
                conform(&a.axes, &b.axes, prefix)?;
                let element = binary(op.clone(), a.element, b.element);
                let kind = crate::array_calls::scalar_actual_kind(&element, symbols)?;
                Ok(RuntimeValue {
                    kind,
                    element,
                    axes: if a.axes.is_empty() { b.axes } else { a.axes },
                })
            }
            Expr::Un(op, a) => {
                let mut v = self.runtime_merge_value(a, index, symbols, prefix)?;
                v.element = Expr::Un(op.clone(), Box::new(v.element));
                Ok(v)
            }
            _ => {
                let value = self.view(e, symbols)?;
                if value.items.is_some() {
                    bail!("runtime CHARACTER MERGE requires whole arrays or rank-one sections");
                }
                Ok(RuntimeValue {
                    kind: value.kind,
                    element: e.clone(),
                    axes: vec![],
                })
            }
        }
    }
    fn merge_runtime(
        &mut self,
        e: &mut Expr,
        symbols: &mut Symbols,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        let Expr::Call(_, args) = e else {
            unreachable!()
        };
        // Numeric MERGE belongs to the ordinary numeric elemental lowering.
        let character = |arg: &Expr| match arg {
            Expr::Ident(n) | Expr::Section { name: n, .. } => symbols
                .get(n)
                .is_some_and(|v| matches!(v.0, TypeSpec::Character(_))),
            Expr::Call(n, a) if n == "@packed_view" => {
                matches!(a.first(),Some(Expr::Ident(n)) if symbols.get(n).is_some_and(|v|matches!(v.0,TypeSpec::Character(_))))
            }
            _ => self
                .view(arg, symbols)
                .is_ok_and(|v| matches!(v.kind, TypeSpec::Character(_))),
        };
        if !character(&args[0]) && !character(&args[1]) {
            return Ok(false);
        }
        self.serial += 1;
        let temp = format!("@character_runtime_merge_{}", self.serial);
        let index = format!("{temp}$index");
        let mut setup = vec![];
        let views = args
            .iter()
            .map(|a| self.runtime_merge_value(a, &Expr::Ident(index.clone()), symbols, &mut setup))
            .collect::<Result<Vec<_>>>()?;
        if !matches!(views[0].kind, TypeSpec::Character(_))
            || !matches!(views[1].kind, TypeSpec::Character(_))
            || length(&views[0].kind)? != length(&views[1].kind)?
        {
            bail!("MERGE sources must have the same type, kind and CHARACTER length");
        }
        if views[2].kind != TypeSpec::Logical {
            bail!("MERGE MASK must be LOGICAL");
        }
        let axes = views
            .iter()
            .find(|v| !v.axes.is_empty())
            .context("runtime MERGE requires an array")?
            .axes
            .clone();
        if axes.len() != 1 {
            bail!("runtime CHARACTER MERGE currently requires rank-one arrays");
        }
        for v in &views {
            conform(&axes, &v.axes, &mut setup)?;
        }
        let mut captured = vec![];
        for (i, axis) in axes.into_iter().enumerate() {
            let name = format!("{temp}$axis{i}");
            setup.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(Some(8)),
                names: vec![name.clone()],
            });
            setup.push(Stmt::Assign {
                name: name.clone(),
                value: axis,
            });
            captured.push(Expr::Ident(name));
        }
        let count = captured
            .iter()
            .cloned()
            .reduce(|a, b| binary(BinOp::Mul, a, b))
            .unwrap();
        let kind = views[0].kind.clone();
        crate::heap::temporary(kind.clone(), temp.clone(), count.clone(), &mut setup);
        // Record the active shape for conformance and allocatable assignment.
        setup.push(Stmt::DummyIntent {
            name: temp.clone(),
            intent: format!(
                "@bounded_shape:{}",
                captured
                    .iter()
                    .map(|e| if let Expr::Ident(n) = e {
                        n.as_str()
                    } else {
                        unreachable!()
                    })
                    .collect::<Vec<_>>()
                    .join(":")
            ),
        });
        let mut elements = vec![];
        for (i, v) in views.into_iter().enumerate() {
            if v.axes.is_empty() {
                let scalar = format!("{temp}$scalar{i}");
                setup.push(Stmt::VarDecl {
                    kind: v.kind,
                    names: vec![scalar.clone()],
                });
                setup.push(Stmt::Assign {
                    name: scalar.clone(),
                    value: v.element,
                });
                elements.push(Expr::Ident(scalar));
            } else {
                elements.push(v.element);
            }
        }
        setup.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![index.clone()],
        });
        let assign = |value| Stmt::AssignIndex {
            name: temp.clone(),
            indices: vec![Expr::Ident(index.clone())],
            value,
        };
        setup.push(Stmt::Do {
            var: index.clone(),
            start: int(1),
            end: count.clone(),
            body: vec![Stmt::If {
                cond: elements[2].clone(),
                then_body: vec![assign(elements[0].clone())],
                else_body: Some(vec![assign(elements[1].clone())]),
            }],
        });
        symbols.insert(temp.clone(), (kind, vec![int(1)]));
        crate::allocatables::metadata(&setup, symbols);
        prefix.extend(setup);
        *e = Expr::Call("@packed_view".into(), vec![Expr::Ident(temp), count]);
        Ok(true)
    }
}

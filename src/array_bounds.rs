// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Normalize fixed explicit bounds before call specialization and array flattening.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone)]
struct Axis {
    lower: i64,
    upper: i64,
    extent: i64,
}
#[derive(Clone)]
struct Array {
    axes: Vec<Axis>,
    explicit: bool,
}
type Arrays = HashMap<String, Array>;
type Symbols = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn fixed(dims: &[Expr]) -> Result<Option<Array>> {
    let explicit = dims
        .iter()
        .any(|d| matches!(d,Expr::Call(n,_) if n == "@declared_bounds"));
    let mut axes = vec![];
    for d in dims {
        let pair = match d {
            Expr::Call(n, args) if n == "@declared_bounds" => Some((
                crate::parameters::integer_constant(&args[0]).context(
                    "explicit array bounds must currently be constant INTEGER expressions",
                )?,
                crate::parameters::integer_constant(&args[1]).context(
                    "explicit array bounds must currently be constant INTEGER expressions",
                )?,
            )),
            _ => crate::parameters::integer_constant(d)
                .ok()
                .map(|upper| (1, upper)),
        };
        let Some((lower, upper)) = pair else {
            if explicit {
                bail!("explicit array bounds must currently be constant INTEGER expressions");
            }
            return Ok(None);
        };
        let extent = i64::try_from((i128::from(upper) - i128::from(lower) + 1).max(0))
            .context("array extent overflows INTEGER(8)")?;
        axes.push(Axis {
            lower,
            upper,
            extent,
        });
    }
    Ok(Some(Array { axes, explicit }))
}

#[derive(Default)]
struct Lowerer {
    guards: Vec<Stmt>,
    modules: HashMap<String, (Arrays, Symbols)>,
}
impl Lowerer {
    fn index(
        &mut self,
        e: &mut Expr,
        name: &str,
        axis: usize,
        bound: &Axis,
        symbols: &Symbols,
    ) -> Result<()> {
        if let Ok(value) = crate::parameters::integer_constant(e) {
            if value < bound.lower || value > bound.upper {
                bail!(
                    "array `{name}` dimension {}: subscript {value} outside {}:{}",
                    axis + 1,
                    bound.lower,
                    bound.upper
                );
            }
            *e = int(i64::try_from(
                i128::from(value) - i128::from(bound.lower) + 1,
            )?);
            return Ok(());
        }
        if !crate::output::integer(e, symbols) {
            bail!("explicit-bound array subscripts must currently be supported scalar INTEGER expressions");
        }
        let n = self.guards.len();
        let function = format!("@lower_bound_index_{n}");
        let param = format!("@lower_bound_value_{n}");
        let value = Expr::Ident(param.clone());
        // Check the original index before subtraction, so an invalid extreme
        // value cannot overflow and alias a valid storage index.
        self.guards.push(Stmt::Function {
            name: function.clone(),
            params: vec![param.clone()],
            pure: false,
            elemental: false,
            return_type: Some(TypeSpec::Integer(Some(8))),
            body: vec![
                Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![param],
                },
                Stmt::If {
                    cond: bin(
                        BinOp::Or,
                        bin(BinOp::Lt, value.clone(), int(bound.lower)),
                        bin(BinOp::Gt, value.clone(), int(bound.upper)),
                    ),
                    then_body: vec![Stmt::CallSub {
                        name: "@error_stop".into(),
                        args: vec![Expr::Str(format!(
                            "array `{name}` dimension {}: subscript outside {}:{}",
                            axis + 1,
                            bound.lower,
                            bound.upper
                        ))],
                    }],
                    else_body: None,
                },
                Stmt::Return(Some(bin(
                    BinOp::Add,
                    bin(BinOp::Sub, value, int(bound.lower)),
                    int(1),
                ))),
            ],
        });
        *e = Expr::Call(function, vec![e.clone()]);
        Ok(())
    }

    fn section(&mut self, e: &mut Expr, bound: &Axis, name: &str, axis: usize) -> Result<()> {
        let Expr::Section {
            lower,
            upper,
            stride,
            ..
        } = e
        else {
            unreachable!()
        };
        let step = stride
            .as_deref()
            .map(crate::parameters::integer_constant)
            .transpose()?
            .unwrap_or(1);
        if step == 0 {
            bail!("array section stride must not be zero");
        }
        let start = lower
            .as_deref()
            .map(crate::parameters::integer_constant)
            .transpose()?
            .unwrap_or(bound.lower);
        let end = upper
            .as_deref()
            .map(crate::parameters::integer_constant)
            .transpose()?
            .unwrap_or(bound.upper);
        let distance = (i128::from(end) - i128::from(start)) * i128::from(step.signum());
        if distance >= 0 {
            let count = distance / i128::from(step).abs() + 1;
            let last = i128::from(start) + (count - 1) * i128::from(step);
            if start < bound.lower
                || start > bound.upper
                || last < i128::from(bound.lower)
                || last > i128::from(bound.upper)
            {
                bail!(
                    "array `{name}` dimension {}: section outside {}:{}",
                    axis + 1,
                    bound.lower,
                    bound.upper
                );
            }
        }
        let translate = |n: i64| -> Result<Box<Expr>> {
            Ok(Box::new(int(i64::try_from(
                i128::from(n) - i128::from(bound.lower) + 1,
            )
            .context("section subscript offset overflows INTEGER(8)")?)))
        };
        *lower = Some(translate(start)?);
        *upper = Some(translate(end)?);
        Ok(())
    }

    fn indices(
        &mut self,
        name: &str,
        args: &mut [Expr],
        a: &Array,
        symbols: &Symbols,
    ) -> Result<()> {
        if args.len() != a.axes.len() {
            bail!(
                "array `{name}` requires {} subscripts, got {}",
                a.axes.len(),
                args.len()
            );
        }
        for (axis, (e, bound)) in args.iter_mut().zip(&a.axes).enumerate() {
            if matches!(e, Expr::Section { .. }) {
                self.section(e, bound, name, axis)?;
            } else if let Expr::Call(tag, items) = e {
                if tag == "@section_subscript" {
                    self.section(&mut items[0], bound, name, axis)?;
                } else {
                    self.index(e, name, axis, bound, symbols)?;
                }
            } else {
                self.index(e, name, axis, bound, symbols)?;
            }
        }
        Ok(())
    }

    fn expr(&mut self, e: &mut Expr, arrays: &Arrays, symbols: &Symbols) -> Result<()> {
        // ASSOCIATED compares descriptor views in the target's declared
        // coordinates, just like pointer association. Do not translate its
        // section bounds into physical one-based array subscripts.
        if matches!(e, Expr::Call(n, _) if n == "associated") { return Ok(()); }

        // Whole-array bounds preserve declarations; expression/section bounds
        // are one-based and are handled after their subscripts are normalized.
        if let Expr::Call(n, args) = e {
            if matches!(n.as_str(), "lbound" | "ubound") {
                if let Some(Expr::Ident(name)) = args.first() {
                    if let Some(a) = arrays.get(name).filter(|a| a.explicit) {
                        let values: Vec<_> = a
                            .axes
                            .iter()
                            .map(|axis| {
                                int(if axis.extent == 0 {
                                    if n == "lbound" {
                                        1
                                    } else {
                                        0
                                    }
                                } else if n == "lbound" {
                                    axis.lower
                                } else {
                                    axis.upper
                                })
                            })
                            .collect();
                        *e = if let Some(dim) = args.get(1) {
                            let dim = crate::parameters::integer_constant(dim).context(
                                "bound inquiry DIM must currently be a constant INTEGER",
                            )?;
                            if dim < 1 || dim as usize > values.len() {
                                bail!("{n} DIM must be between 1 and {}", values.len());
                            }
                            values[dim as usize - 1].clone()
                        } else {
                            Expr::Call("@bounds_vector".into(), values)
                        };
                        return Ok(());
                    }
                }
            }
        }
        match e {
            Expr::Call(name, args) | Expr::Index(name, args) => {
                if let Some(a) = arrays.get(name).filter(|a| a.explicit) {
                    for arg in args.iter_mut() {
                        self.subscript(arg, arrays, symbols)?;
                    }
                    self.indices(name, args, a, symbols)?;
                } else {
                    for arg in args {
                        self.expr(arg, arrays, symbols)?;
                    }
                }
            }
            Expr::Section { name, .. } => {
                let name = name.clone();
                self.subscript(e, arrays, symbols)?;
                if let Some(a) = arrays.get(&name).filter(|a| a.explicit) {
                    if a.axes.len() != 1 {
                        bail!("array `{name}` requires {} subscripts", a.axes.len());
                    }
                    self.section(e, &a.axes[0], &name, 0)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, arrays, symbols)?;
                self.expr(b, arrays, symbols)?;
            }
            Expr::Un(_, a) => self.expr(a, arrays, symbols)?,
            _ => {}
        }
        Ok(())
    }

    fn subscript(&mut self, e: &mut Expr, arrays: &Arrays, symbols: &Symbols) -> Result<()> {
        match e {
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for value in [lower, upper, stride].into_iter().flatten() {
                    self.expr(value, arrays, symbols)?;
                }
            }
            Expr::Call(tag, items) if tag == "@section_subscript" => {
                for value in items {
                    self.subscript(value, arrays, symbols)?;
                }
            }
            _ => self.expr(e, arrays, symbols)?,
        }
        Ok(())
    }

    fn body(
        &mut self,
        body: &mut [Stmt],
        inherited: &Arrays,
        inherited_symbols: &Symbols,
        params: &[String],
    ) -> Result<()> {
        let mut arrays = inherited.clone();
        let mut symbols = inherited_symbols.clone();
        for p in params {
            arrays.remove(p);
            symbols.remove(p);
        }
        for s in body.iter() {
            if let Stmt::Use { module, only, .. } = s {
                if let Some((a, t)) = self.modules.get(module) {
                    for (n, v) in a {
                        if only.as_ref().is_none_or(|names| names.contains(n)) {
                            arrays.insert(n.clone(), v.clone());
                        }
                    }
                    for (n, v) in t {
                        if only.as_ref().is_none_or(|names| names.contains(n)) {
                            symbols.insert(n.clone(), v.clone());
                        }
                    }
                }
            }
        }
        let c_binding=body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,..} if name==crate::c_interop::BIND));
        for s in body.iter_mut() {
            match s {
                // Native C adapters normalize these bounds after association
                // and constant folding; preserve the original dummy bounds.
                Stmt::ArrayDecl { name,kind,dims } if c_binding && params.contains(name) => {
                    arrays.remove(name);
                    symbols.insert(name.clone(),(kind.clone(),dims.clone()));
                }
                Stmt::ArrayDecl { name, kind, dims } => {
                    arrays.remove(name);
                    if let Some(a) =
                        fixed(dims).with_context(|| format!("invalid bounds for array `{name}`"))?
                    {
                        if a.explicit {
                            *dims = a.axes.iter().map(|b| int(b.extent)).collect();
                        }
                        arrays.insert(name.clone(), a);
                    }
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        arrays.remove(n);
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                _ => {}
            }
        }
        for s in body {
            match s {
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    self.expr(value, &arrays, &symbols)?
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.expr(value, &arrays, &symbols)?;
                    if let Some(a) = arrays.get(name).filter(|a| a.explicit) {
                        // Rank-one section assignment contains its named section.
                        for e in indices.iter_mut() {
                            self.subscript(e, &arrays, &symbols)?;
                        }
                        self.indices(name, indices, a, &symbols)?;
                    } else {
                        for e in indices {
                            self.expr(e, &arrays, &symbols)?;
                        }
                    }
                }
                Stmt::CallSub { name, .. } if name == "@pointer_associate" => {},
                Stmt::Print { items }
                | Stmt::Read { args: items }
                | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expr(e, &arrays, &symbols)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expr(cond, &arrays, &symbols)?;
                    self.body(then_body, &arrays, &symbols, &[])?;
                    if let Some(b) = else_body {
                        self.body(b, &arrays, &symbols, &[])?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expr(start, &arrays, &symbols)?;
                    self.expr(end, &arrays, &symbols)?;
                    self.body(body, &arrays, &symbols, &[])?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expr(cond, &arrays, &symbols)?;
                    self.body(body, &arrays, &symbols, &[])?;
                }
                Stmt::Block { body } => self.body(body, &arrays, &symbols, &[])?,
                Stmt::Module { body, .. } => {
                    self.body(body, &Arrays::new(), &Symbols::new(), &[])?
                }
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    self.body(body, &arrays, &symbols, params)?
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expr(expr, &arrays, &symbols)?;
                    for c in cases {
                        for item in &mut c.items {
                            match item {
                                CaseItem::Single(e) => self.expr(e, &arrays, &symbols)?,
                                CaseItem::Range(a, b) => {
                                    self.expr(a, &arrays, &symbols)?;
                                    self.expr(b, &arrays, &symbols)?;
                                }
                            }
                        }
                        self.body(&mut c.body, &arrays, &symbols, &[])?;
                    }
                    if let Some(b) = default {
                        self.body(b, &arrays, &symbols, &[])?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut lower = Lowerer::default();
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let mut arrays = Arrays::new();
            let mut symbols = Symbols::new();
            for s in body {
                if let Stmt::ArrayDecl { name, kind, dims } = s {
                    if let Some(a) = fixed(dims)? {
                        arrays.insert(name.clone(), a);
                    }
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
            }
            lower.modules.insert(name.clone(), (arrays, symbols));
        }
    }
    lower.body(&mut program.body, &Arrays::new(), &Symbols::new(), &[])?;
    program.body.extend(lower.guards);
    Ok(program)
}

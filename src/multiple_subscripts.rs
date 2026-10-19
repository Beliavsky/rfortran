// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Expand F2023 @ subscripts before generic selection and array lowering.
//! Only selector expressions are materialized, preserving writable array views.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use crate::procedure_arguments::{children, expressions, visit_expr};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}
fn marker(n: &str) -> bool {
    matches!(n, "@multiple_subscript" | "@multiple_triplet")
}
fn present(e: &Expr) -> bool {
    let mut found = false;
    visit_expr(&mut e.clone(), &mut |e| {
        if matches!(e,Expr::Call(n,_) if marker(n)) {
            found = true;
        }
    });
    found
}
#[derive(Clone)]
struct Variable {
    kind: Option<TypeSpec>,
    dims: Vec<Expr>,
    derived: Option<String>,
}
#[derive(Clone, Default)]
struct Scope {
    vars: HashMap<String, Variable>,
    constants: HashMap<String, Expr>,
    functions: HashMap<String, (Variable, bool)>,
    types: HashMap<String, HashMap<String, Variable>>,
}
impl Scope {
    fn variable(&self, n: &str) -> Option<Variable> {
        let mut path = n.split('%');
        let mut v = self.vars.get(path.next()?)?.clone();
        for field in path {
            v = self.types.get(v.derived.as_ref()?)?.get(field)?.clone();
        }
        Some(v)
    }
    fn constant(&self, e: &Expr) -> Expr {
        let mut e = e.clone();
        // PARAMETER expressions may reference earlier named constants.
        for _ in 0..self.constants.len() + 1 {
            let old = e.clone();
            visit_expr(&mut e, &mut |e| {
                if let Expr::Ident(n) = e {
                    if let Some(v) = self.constants.get(n) {
                        *e = v.clone();
                    }
                }
            });
            if e == old {
                break;
            }
        }
        e
    }
    fn integer(&self, e: &Expr) -> Option<i64> {
        crate::parameters::integer_constant(&self.constant(e)).ok()
    }
    fn extent(&self, d: &Expr) -> Option<usize> {
        let n = match d {
            Expr::Call(n, a) if n == "@declared_bounds" => {
                (i128::from(self.integer(&a[1])?) - i128::from(self.integer(&a[0])?) + 1).max(0)
            }
            _ => i128::from(self.integer(d)?).max(0),
        };
        usize::try_from(n).ok()
    }
    fn declarations(&mut self, body: &[Stmt]) {
        let parameters: std::collections::HashSet<_> = body
            .iter()
            .filter_map(|s| match s {
                Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } if attributes.iter().any(|a| a == "parameter") => {
                    Some(entities.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        for s in body {
            let names = match s {
                Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
                Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                Stmt::InitializedDecl { entities, .. } => {
                    entities.iter().map(|(n, _)| n.clone()).collect()
                }
                _ => vec![],
            };
            for n in names {
                if !parameters.contains(&n) {
                    self.constants.remove(&n);
                }
            }
        }
        // Shapes precede scalar-looking initialization records for array entities.
        for s in body {
            match s {
                Stmt::ArrayDecl { kind, name, dims } => {
                    self.vars.insert(
                        name.clone(),
                        Variable {
                            kind: Some(kind.clone()),
                            dims: dims.clone(),
                            derived: None,
                        },
                    );
                }
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        self.vars.insert(
                            n.clone(),
                            Variable {
                                kind: Some(kind.clone()),
                                dims: vec![],
                                derived: None,
                            },
                        );
                    }
                }
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } => {
                    for n in names {
                        self.vars.insert(
                            n.clone(),
                            Variable {
                                kind: None,
                                dims: dims.clone(),
                                derived: Some(type_name.clone()),
                            },
                        );
                    }
                }
                Stmt::DerivedType {
                    name, components, ..
                } => {
                    let mut fields = Scope::default();
                    fields.declarations(components);
                    self.types.insert(name.clone(), fields.vars);
                }
                _ => {}
            }
        }
        for s in body {
            if let Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } = s
            {
                for (n, e) in entities {
                    self.vars.entry(n.clone()).or_insert(Variable {
                        kind: Some(kind.clone()),
                        dims: vec![],
                        derived: None,
                    });
                    if attributes.iter().any(|a| a == "parameter") {
                        if let Some(e) = e {
                            self.constants.insert(n.clone(), e.clone());
                        }
                    }
                }
            }
        }
        for s in body {
            if let Stmt::Function {
                name,
                body,
                return_type,
                elemental,
                ..
            } = s
            {
                let mut local = self.clone();
                local.declarations(body);
                let result = body
                    .last()
                    .and_then(|s| match s {
                        Stmt::Return(Some(Expr::Ident(n))) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(name);
                if let Some(mut v) = local.variable(result) {
                    // Fold result extents with the function's own named constants.
                    v.dims = v.dims.iter().map(|e| local.constant(e)).collect();
                    self.functions.insert(name.clone(), (v, *elemental));
                } else if let Some(kind) = return_type {
                    self.functions.insert(
                        name.clone(),
                        (
                            Variable {
                                kind: Some(kind.clone()),
                                dims: vec![],
                                derived: None,
                            },
                            *elemental,
                        ),
                    );
                }
            }
        }
    }
    // rank and size of a rank-one selector; scalar size is irrelevant.
    fn info(
        &self,
        e: &Expr,
        types: &crate::operators::ExpressionTypes,
    ) -> Result<(TypeSpec, usize, Option<usize>)> {
        let from_var = |v: Variable| -> Result<_> {
            Ok((
                v.kind
                    .context("multiple subscripts require INTEGER expressions")?,
                v.dims.len(),
                v.dims.first().and_then(|d| self.extent(d)),
            ))
        };
        match e {
            Expr::Ident(n) => {
                if let Some(v) = self.variable(n) {
                    return from_var(v);
                }
            }
            Expr::Bin(op, a, b) => {
                let (ka, ra, sa) = self.info(a, types)?;
                let (kb, rb, sb) = self.info(b, types)?;
                if ra > 0 && rb > 0 && sa.is_some() && sb.is_some() && sa != sb {
                    bail!("multiple subscript expression arrays must have the same size");
                }
                let k = if !matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) {
                    TypeSpec::Logical
                } else if matches!(ka, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_))
                    && matches!(kb, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_))
                {
                    ka
                } else {
                    TypeSpec::Real
                };
                return Ok((k, ra.max(rb), if ra > 0 { sa } else { sb }));
            }
            Expr::Un(_, a) => return self.info(a, types),
            Expr::Call(n, a) if n == "@conditional" => {
                let mut result =
                    self.info(a.last().context("invalid conditional subscript")?, types)?;
                for e in a.iter().skip(1).step_by(2) {
                    let (_, r, s) = self.info(e, types)?;
                    // Existing conditional validation resolves equivalent kind
                    // spellings (INTEGER and INTEGER(4)) before comparing them.
                    if r != result.1 {
                        bail!("conditional multiple subscripts require matching ranks");
                    }
                    if s != result.2 {
                        result.2 = None;
                    }
                }
                return Ok(result);
            }
            Expr::Call(n, a) if n == "@typed_constructor" => {
                let kind = crate::empty_constructors::kind(e)?
                    .context("invalid typed multiple subscript constructor")?;
                let mut count = Some(0usize);
                for item in &a[1..] {
                    let (_, r, s) = self.info(item, types)?;
                    if r > 1 {
                        bail!("multiple subscript constructor items must be scalar or rank one");
                    }
                    count = count
                        .zip(if r == 0 { Some(1) } else { s })
                        .and_then(|(a, b)| a.checked_add(b));
                }
                return Ok((kind, 1, count));
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if let Some(v) = self.variable(name) {
                    if v.dims.len() != 1 {
                        bail!("multiple subscript expressions must have rank one");
                    }
                    let d = &v.dims[0];
                    let (lb, ub) = match d {
                        Expr::Call(n, a) if n == "@declared_bounds" => {
                            (self.integer(&a[0]), self.integer(&a[1]))
                        }
                        _ => (Some(1), self.integer(d)),
                    };
                    let step = stride
                        .as_deref()
                        .map(|e| self.integer(e))
                        .unwrap_or(Some(1));
                    let size = (|| {
                        let step = step?;
                        if step == 0 {
                            return None;
                        }
                        let first = lower
                            .as_deref()
                            .map(|e| self.integer(e))
                            .unwrap_or(if step > 0 { lb } else { ub })?;
                        let last = upper
                            .as_deref()
                            .map(|e| self.integer(e))
                            .unwrap_or(if step > 0 { ub } else { lb })?;
                        let (last, first, step) =
                            (i128::from(last), i128::from(first), i128::from(step));
                        Some(if (last - first) * step.signum() < 0 {
                            0
                        } else {
                            usize::try_from((last - first) / step + 1).ok()?
                        })
                    })();
                    return Ok((
                        v.kind
                            .context("multiple subscripts require INTEGER expressions")?,
                        1,
                        size,
                    ));
                }
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                let mut count = Some(0usize);
                let mut kind = None;
                for item in a {
                    let (k, r, s) = self.info(item, types)?;
                    if r > 1 {
                        bail!("multiple subscript constructor items must be scalar or rank one");
                    }
                    if !matches!(k, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)) {
                        bail!("multiple subscripts require INTEGER expressions");
                    }
                    kind = Some(k);
                    count = count
                        .zip(if r == 0 { Some(1) } else { s })
                        .and_then(|(a, b)| a.checked_add(b));
                }
                return Ok((kind.unwrap_or(TypeSpec::Integer(None)), 1, count));
            }
            Expr::Call(n, a) if n == "@output_implied_do" && a.len() >= 5 => {
                let count = (|| {
                    let lo = i128::from(self.integer(&a[1])?);
                    let hi = i128::from(self.integer(&a[2])?);
                    let step = i128::from(self.integer(&a[3])?);
                    if step == 0 {
                        return None;
                    }
                    Some(if (hi - lo) * step.signum() < 0 {
                        0
                    } else {
                        usize::try_from((hi - lo) / step + 1).ok()?
                    })
                })();
                let mut local = self.clone();
                if let Expr::Ident(n) = &a[0] {
                    local.vars.insert(
                        n.clone(),
                        Variable {
                            kind: Some(TypeSpec::Integer(None)),
                            dims: vec![],
                            derived: None,
                        },
                    );
                }
                let mut size = Some(0usize);
                let mut kind = TypeSpec::Integer(None);
                for e in &a[4..] {
                    let (k, r, s) = local.info(e, types)?;
                    kind = k;
                    size = size
                        .zip(if r == 0 { Some(1) } else { s })
                        .and_then(|(a, b)| a.checked_add(b));
                }
                return Ok((kind, 1, count.zip(size).and_then(|(a, b)| a.checked_mul(b))));
            }
            Expr::Call(n, a)
                if matches!(n.as_str(), "shape" | "lbound" | "ubound") && a.len() == 1 =>
            {
                let rank = self.array_rank(&a[0], types)?;
                return Ok((TypeSpec::Integer(None), 1, Some(rank)));
            }
            Expr::Call(n, a)
                if matches!(n.as_str(), "int" | "abs" | "mod" | "modulo" | "max" | "min")
                    && !a.is_empty() =>
            {
                let (k, mut r, mut s) = self.info(&a[0], types)?;
                for e in &a[1..] {
                    let (_, next, size) = self.info(e, types)?;
                    if r == 0 && next > 0 {
                        r = next;
                        s = size;
                    } else if r > 0 && next > 0 && s.is_some() && size.is_some() && s != size {
                        bail!("multiple subscript expression arrays must have the same size");
                    }
                }
                return Ok((
                    if n == "int" {
                        TypeSpec::Integer(None)
                    } else {
                        k
                    },
                    r,
                    s,
                ));
            }
            Expr::Call(n, a) => {
                if let Some((v, elemental)) = self.functions.get(n) {
                    let mut info = from_var(v.clone())?;
                    if *elemental && info.1 == 0 {
                        for e in a {
                            let (_, r, s) = self.info(e, types)?;
                            if r > 0 {
                                info.1 = r;
                                info.2 = s;
                                break;
                            }
                        }
                    }
                    return Ok(info);
                }
            }
            _ => {}
        }
        let declarations = self
            .vars
            .iter()
            .filter_map(|(n, v)| {
                v.kind
                    .as_ref()
                    .map(|k| (n.clone(), (k.clone(), v.dims.len())))
            })
            .collect();
        let (k, r) = types
            .kind_rank(e, &declarations)?
            .context("cannot determine the type and rank of a multiple subscript expression")?;
        Ok((k, r, None))
    }
    fn array_rank(&self, e: &Expr, types: &crate::operators::ExpressionTypes) -> Result<usize> {
        if let Expr::Ident(n) = e {
            if let Some(v) = self.variable(n) {
                return Ok(v.dims.len());
            }
        }
        Ok(self.info(e, types)?.1)
    }
}

struct Lower {
    types: crate::operators::ExpressionTypes,
    serial: usize,
}
impl Lower {
    fn fresh(&mut self) -> String {
        self.serial += 1;
        format!("@multiple_{}", self.serial)
    }
    fn guard(&self, e: Expr, count: usize) -> Stmt {
        Stmt::If {
            cond: Expr::Bin(
                BinOp::Ne,
                Box::new(call("size", vec![e])),
                Box::new(int(count as i64)),
            ),
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(
                    "multiple subscript arrays must have the expected size".into(),
                )],
            }],
            else_body: None,
        }
    }
    fn values(
        &mut self,
        e: &Expr,
        count: usize,
        scope: &Scope,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Vec<Expr>> {
        let (kind, rank, size) = scope.info(e, &self.types)?;
        if !matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)) {
            bail!("multiple subscripts require INTEGER expressions");
        }
        if rank > 1 {
            bail!("multiple subscript expressions must have rank one or be scalar triplet bounds");
        }
        // Coordinate storage is internal and always wide. In particular an
        // INTEGER(1) array combined with a default-INTEGER scalar must not
        // narrow the promoted expression back to the array operand's kind.
        let kind = TypeSpec::Integer(Some(8));
        if rank == 0 {
            if scope.integer(e).is_some() || matches!(e, Expr::Ident(_)) {
                return Ok(vec![e.clone(); count]);
            }
            let n = self.fresh();
            prefix.push(Stmt::VarDecl {
                kind,
                names: vec![n.clone()],
            });
            prefix.push(Stmt::Assign {
                name: n.clone(),
                value: e.clone(),
            });
            return Ok(vec![Expr::Ident(n); count]);
        }
        if size.is_some_and(|s| s != count) {
            bail!("multiple subscript arrays must have the same size");
        }
        let constant = scope.constant(e);
        if let Expr::Call(tag, a) = &constant {
            if tag == "@array_constructor"
                && a.len() == count
                && a.iter().all(|e| scope.integer(e).is_some())
            {
                return Ok(a.iter().map(|e| int(scope.integer(e).unwrap())).collect());
            }
        }
        let n = self.fresh();
        let dynamic = size.is_none() && !matches!(e, Expr::Ident(_));
        prefix.push(Stmt::ArrayDecl {
            kind: kind.clone(),
            name: n.clone(),
            dims: vec![if dynamic {
                Expr::Ident("@deferred_extent".into())
            } else {
                int(count as i64)
            }],
        });
        if dynamic {
            prefix.push(Stmt::DummyIntent {
                name: n.clone(),
                intent: "allocatable".into(),
            });
        }
        if size.is_none() && !dynamic {
            prefix.push(self.guard(e.clone(), count));
        }
        prefix.push(Stmt::Assign {
            name: n.clone(),
            value: e.clone(),
        });
        if dynamic {
            prefix.push(self.guard(Expr::Ident(n.clone()), count));
        }
        let mut values = vec![];
        for i in 1..=count {
            let scalar = self.fresh();
            prefix.push(Stmt::VarDecl {
                kind: kind.clone(),
                names: vec![scalar.clone()],
            });
            prefix.push(Stmt::Assign {
                name: scalar.clone(),
                value: call(&n, vec![int(i as i64)]),
            });
            values.push(Expr::Ident(scalar));
        }
        Ok(values)
    }
    fn expand(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        scope: &Scope,
        prefix: &mut Vec<Stmt>,
    ) -> Result<bool> {
        if !args
            .iter()
            .any(|e| matches!(e,Expr::Call(n,_) if marker(n)))
        {
            return Ok(false);
        }
        let parent = name.split('%').next().unwrap_or(name);
        let source = if scope.variable(parent).is_some_and(|v| !v.dims.is_empty()) {
            parent
        } else {
            name
        };
        let rank = scope
            .variable(parent)
            .filter(|v| !v.dims.is_empty())
            .or_else(|| scope.variable(name))
            .map(|v| v.dims.len());
        if rank.is_none() && name != "@component_subscripts" {
            bail!("multiple subscripts require an array designator");
        }
        if rank == Some(0) {
            bail!("multiple subscripts require an array designator");
        }
        let mut sizes = vec![];
        let mut known: usize = 0;
        let mut unknown = 0;
        for e in args.iter() {
            let mut length = Some(1);
            if let Expr::Call(n, a) = e {
                if marker(n) {
                    let expressions: Vec<&Expr> = if n == "@multiple_triplet" {
                        let Expr::Section {
                            lower,
                            upper,
                            stride,
                            ..
                        } = &a[0]
                        else {
                            unreachable!()
                        };
                        [lower, upper, stride]
                            .into_iter()
                            .flatten()
                            .map(|e| e.as_ref())
                            .collect()
                    } else {
                        vec![&a[0]]
                    };
                    let mut arrays = 0;
                    length = None;
                    for e in expressions {
                        let (k, r, s) = scope.info(e, &self.types)?;
                        if !matches!(k, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_)) {
                            bail!("multiple subscripts require INTEGER expressions");
                        }
                        if r > 1 || n == "@multiple_subscript" && r != 1 {
                            bail!("a multiple subscript must be a rank-one INTEGER array");
                        }
                        if r == 1 {
                            arrays += 1;
                            if let Some(s) = s {
                                if length.is_some_and(|a| a != s) {
                                    bail!("multiple subscript arrays must have the same size");
                                }
                                length = Some(s);
                            }
                        }
                    }
                    if arrays == 0 {
                        bail!("a multiple subscript triplet requires at least one rank-one INTEGER array");
                    }
                }
            }
            if let Some(n) = length {
                known = known
                    .checked_add(n)
                    .context("multiple subscript count overflow")?;
            } else {
                unknown += 1;
            }
            sizes.push(length);
        }
        if unknown > 1 {
            bail!("multiple subscripts currently require known sizes when more than one subscript sequence has runtime size");
        }
        let inferred = if unknown == 1 {
            rank.and_then(|r| r.checked_sub(known))
                .context("cannot determine multiple subscript size from the array rank")?
        } else {
            0
        };
        if known + unknown * inferred > 15 {
            bail!("multiple subscripts cannot select more than 15 dimensions");
        }
        if rank.is_some_and(|r| r != known + unknown * inferred) {
            bail!(
                "expanded multiple subscript count does not match array rank {}",
                rank.unwrap()
            );
        }
        let mut expanded = vec![];
        for (e, size) in std::mem::take(args).into_iter().zip(sizes) {
            let count = size.unwrap_or(inferred);
            match e {
                Expr::Call(n, a) if n == "@multiple_subscript" => {
                    expanded.extend(self.values(&a[0], count, scope, prefix)?)
                }
                Expr::Call(n, mut a) if n == "@multiple_triplet" => {
                    let Expr::Section {
                        lower,
                        upper,
                        stride,
                        ..
                    } = a.remove(0)
                    else {
                        unreachable!()
                    };
                    let mut parts = vec![];
                    for e in [lower, upper, stride] {
                        parts.push(if let Some(e) = e {
                            Some(self.values(&e, count, scope, prefix)?)
                        } else {
                            None
                        });
                    }
                    // Multiple-triplet omission denotes the corresponding
                    // array bounds (9.5.3.2), including for a negative stride.
                    for bound in 0..2 {
                        if parts[bound].is_none() {
                            let variable=scope.variable(source).context("multiple triplets on array components currently require explicit bounds")?;
                            let mut values = vec![];
                            for i in 0..count {
                                let axis = expanded.len() + i;
                                let d = variable
                                    .dims
                                    .get(axis)
                                    .context("multiple subscript exceeds array rank")?;
                                let constant = match d {
                                    Expr::Call(n, a) if n == "@declared_bounds" => {
                                        scope.integer(&a[bound])
                                    }
                                    Expr::Ident(n) if n=="@assumed_extent" && bound==1 => bail!("a multiple triplet in the final assumed-size dimension requires upper bounds"),
                                    Expr::Ident(n) if n=="@deferred_extent" || n=="@assumed_extent"=>None,
                                    _ if bound == 0 => Some(1),
                                    _ => scope.integer(d),
                                };
                                values.push(if let Some(n) = constant {
                                    int(n)
                                } else {
                                    let n = self.fresh();
                                    prefix.push(Stmt::VarDecl {
                                        kind: TypeSpec::Integer(Some(8)),
                                        names: vec![n.clone()],
                                    });
                                    prefix.push(Stmt::Assign {
                                        name: n.clone(),
                                        value: call(
                                            if bound == 0 { "lbound" } else { "ubound" },
                                            vec![
                                                Expr::Ident(source.into()),
                                                int((axis + 1) as i64),
                                            ],
                                        ),
                                    });
                                    Expr::Ident(n)
                                });
                            }
                            parts[bound] = Some(values);
                        }
                    }
                    for i in 0..count {
                        expanded.push(Expr::Section {
                            name: String::new(),
                            lower: parts[0].as_ref().map(|v| Box::new(v[i].clone())),
                            upper: parts[1].as_ref().map(|v| Box::new(v[i].clone())),
                            stride: parts[2].as_ref().map(|v| Box::new(v[i].clone())),
                        });
                    }
                }
                e => expanded.push(e),
            }
        }
        *args = expanded;
        Ok(true)
    }
    fn expression(&mut self, e: &mut Expr, scope: &Scope, prefix: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Call(n, a) if n == "@conditional" && a.iter().any(present) => {
                fn branch(lower: &mut Lower, a: &mut [Expr], scope: &Scope) -> Result<Vec<Stmt>> {
                    let mut before = vec![];
                    if a.len() == 1 {
                        lower.expression(&mut a[0], scope, &mut before)?;
                        return Ok(before);
                    }
                    lower.expression(&mut a[0], scope, &mut before)?;
                    let cond = lower.fresh();
                    before.push(Stmt::VarDecl {
                        kind: TypeSpec::Logical,
                        names: vec![cond.clone()],
                    });
                    before.push(Stmt::Assign {
                        name: cond.clone(),
                        value: a[0].clone(),
                    });
                    a[0] = Expr::Ident(cond.clone());
                    let mut yes = vec![];
                    lower.expression(&mut a[1], scope, &mut yes)?;
                    let no = branch(lower, &mut a[2..], scope)?;
                    before.push(Stmt::If {
                        cond: Expr::Ident(cond),
                        then_body: yes,
                        else_body: Some(no),
                    });
                    Ok(before)
                }
                // Declarations must be visible to the later choice lowering;
                // only their assignments stay in the selected branch.
                fn declarations(body: &mut Vec<Stmt>, out: &mut Vec<Stmt>) {
                    let mut kept = vec![];
                    for mut s in std::mem::take(body) {
                        if matches!(
                            s,
                            Stmt::VarDecl { .. }
                                | Stmt::ArrayDecl { .. }
                                | Stmt::DummyIntent { .. }
                        ) {
                            out.push(s);
                            continue;
                        }
                        for b in children(&mut s) {
                            declarations(b, out);
                        }
                        kept.push(s);
                    }
                    *body = kept;
                }
                let mut executable = branch(self, a, scope)?;
                declarations(&mut executable, prefix);
                prefix.extend(executable);
            }
            Expr::Call(n, a) | Expr::Index(n, a) => {
                // A multiple triplet's bounds live inside the marker; visit them,
                // then expand the marker only in its owning subscript list.
                for e in a.iter_mut() {
                    self.expression(e, scope, prefix)?;
                }
                if self.expand(n, a, scope, prefix)? {
                    *e = if n == "@component_subscripts" {
                        Expr::Call(n.clone(), std::mem::take(a))
                    } else {
                        crate::ast::section_or_call(n.clone(), std::mem::take(a))
                    };
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, scope, prefix)?;
                self.expression(b, scope, prefix)?;
            }
            Expr::Un(_, a) => self.expression(a, scope, prefix)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, scope, prefix)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Scope) -> Result<()> {
        let mut scope = inherited.clone();
        scope.declarations(body);
        for s in body.iter() {
            if let Stmt::Module { body, .. } = s {
                scope.declarations(body);
            }
        }
        let mut out = vec![];
        let mut offset = None;
        for mut s in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) {
                offset = Some(p);
            }
            let result = (|| -> Result<()> {
                if let Stmt::Print { items } = &s {
                    if items
                        .iter()
                        .any(|e| present(e) && crate::output::contains_implied_do(e))
                    {
                        let symbols = scope
                            .vars
                            .iter()
                            .filter_map(|(n, v)| {
                                v.kind
                                    .as_ref()
                                    .map(|k| (n.clone(), (k.clone(), v.dims.clone())))
                            })
                            .collect();
                        s = crate::output::lower(items, &symbols, &mut self.serial)?;
                    }
                }
                let has_constructor = expressions(&mut s)
                    .into_iter()
                    .any(|e| present(e) && crate::constructors::delayed(e));
                if has_constructor {
                    let symbols = scope
                        .vars
                        .iter()
                        .filter_map(|(n, v)| {
                            v.kind
                                .as_ref()
                                .map(|k| (n.clone(), (k.clone(), v.dims.clone())))
                        })
                        .collect();
                    let (mut before, after) = crate::constructor_expressions::conditional_items(
                        &mut s,
                        &symbols,
                        &mut self.serial,
                    )?;
                    if !before.is_empty() || !after.is_empty() {
                        before.push(s);
                        before.extend(after);
                        self.body(&mut before, &scope)?;
                        out.extend(before);
                        return Ok(());
                    }
                }
                for child in children(&mut s) {
                    self.body(child, &scope)?;
                }
                let mut prefix = vec![];
                for e in expressions(&mut s) {
                    self.expression(e, &scope, &mut prefix)?;
                }
                if let Stmt::AssignIndex { name, indices, .. } = &mut s {
                    let component = if matches!(indices.last(),Some(Expr::Call(n,_)) if n=="@component_subscripts")
                    {
                        indices.pop()
                    } else {
                        None
                    };
                    if self.expand(name, indices, &scope, &mut prefix)? {
                        let e = crate::ast::section_or_call(name.clone(), std::mem::take(indices));
                        *indices = match e {
                            Expr::Call(_, a) => a,
                            Expr::Section { .. } => vec![e],
                            _ => unreachable!(),
                        };
                    }
                    if let Some(component) = component {
                        indices.push(component);
                    }
                }
                if !prefix.is_empty() {
                    if let Stmt::DoWhile { cond, body } = &mut s {
                        let mut b = prefix;
                        b.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        b.append(body);
                        *body = b;
                        *cond = Expr::Logical(true);
                    } else {
                        out.extend(prefix);
                    }
                }
                out.push(s);
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        *body = out;
        Ok(())
    }
}
pub fn resolve(mut program: Program) -> Result<Program> {
    fn needed(body: &[Stmt]) -> bool {
        body.iter().any(|s| {
            let mut s = s.clone();
            expressions(&mut s).into_iter().any(|e| present(e))
                || children(&mut s).into_iter().any(|b| needed(b))
        })
    }
    if !needed(&program.body) {
        return Ok(program);
    }
    let mut lower = Lower {
        types: crate::operators::ExpressionTypes::new(&program)?,
        serial: 0,
    };
    lower.body(&mut program.body, &Scope::default())?;
    Ok(program)
}

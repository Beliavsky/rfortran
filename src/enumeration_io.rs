// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Validate enumeration edit descriptors before erasing nominal types. Input
//! uses integer temporaries so an invalid ordinal never becomes an enum value.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Layout {
    format: usize,
    first: usize,
    input: bool,
}
pub(super) fn layout(name: &str, args: &[Expr]) -> Option<Layout> {
    let (format, mut first, input) = match name {
        "@formatted_print" | "@formatted_write_noadvance" => (0, 1, false),
        "@write" => (1, 2, false),
        "@formatted_read" => (1, 2, true),
        "@formatted_read_status" => (1, 4, true),
        _ => return None,
    };
    while !input
        && matches!(args.get(first), Some(Expr::Call(n,_)) if n.starts_with("@write_option:"))
    {
        first += 1;
    }
    Some(Layout {
        format,
        first,
        input,
    })
}
fn raw(mut e: &Expr) -> &Expr {
    while let Expr::Call(n, a) = e {
        if n != "@enumeration_reference" {
            break;
        }
        e = &a[2];
    }
    e
}
fn assign(target: Expr, value: Expr) -> Stmt {
    match target {
        Expr::Ident(name) => Stmt::Assign { name, value },
        Expr::Index(name, indices) | Expr::Call(name, indices) => Stmt::AssignIndex {
            name,
            indices,
            value,
        },
        _ => unreachable!(),
    }
}
fn eq(name: &str, value: i64) -> Expr {
    Expr::Bin(
        BinOp::Eq,
        Box::new(Expr::Ident(name.into())),
        Box::new(int(value)),
    )
}
impl Context {
    pub(super) fn io_nested_type(&self, e: &Expr, scope: &Scope, fallback: Ty) -> Result<Ty> {
        if let Expr::Call(n, a) = raw(e) {
            if matches!(n.as_str(), "@output_implied_do" | "@input_implied_do") {
                for item in &a[4..] {
                    let ty = self.expression(&mut item.clone(), scope, false)?;
                    let ty = self.io_nested_type(item, scope, ty)?;
                    if ty.enumeration().is_some() {
                        return Ok(ty.ranked(1));
                    }
                }
            }
        }
        Ok(fallback)
    }
    fn io_array_expression(&self, e: &Expr, scope: &Scope) -> bool {
        match raw(e) {
            Expr::Ident(n) => scope
                .shapes
                .get(n)
                .or_else(|| self.globals.shapes.get(n))
                .is_some_and(|d| !d.is_empty()),
            Expr::Section { .. } => true,
            Expr::Call(n, _)
                if matches!(
                    n.as_str(),
                    "sum"
                        | "product"
                        | "any"
                        | "all"
                        | "count"
                        | "minval"
                        | "maxval"
                        | "size"
                        | "rank"
                        | "len"
                        | "storage_size"
                        | "dot_product"
                        | "norm2"
                ) =>
            {
                false
            }
            Expr::Call(n, _)
                if self
                    .procedures
                    .get(n)
                    .is_some_and(|p| !p.elemental && p.result.rank() == 0) =>
            {
                false
            }
            Expr::Call(n, a) | Expr::Index(n, a) => {
                matches!(n.as_str(), "@array_constructor" | "@record_constructor")
                    || a.iter().any(|e| self.io_array_expression(e, scope))
                    || n == "@section_subscript"
            }
            Expr::Bin(_, a, b) => {
                self.io_array_expression(a, scope) || self.io_array_expression(b, scope)
            }
            Expr::Un(_, a) => self.io_array_expression(a, scope),
            _ => false,
        }
    }
    fn io_format(&self, e: &Expr, scope: &Scope) -> Result<String> {
        let mut value = e.clone();
        for _ in 0..=scope.character_constants.len() + self.globals.character_constants.len() {
            visit_expr(&mut value, &mut |e| {
                if let Expr::Ident(n) = e {
                    if let Some(v) = scope
                        .character_constants
                        .get(n)
                        .or_else(|| self.globals.character_constants.get(n))
                    {
                        *e = v.clone();
                    }
                }
            });
        }
        crate::parameters::character_constant(&value).map_err(|_| {
            anyhow::anyhow!(
                "formatted enumeration I/O currently requires a constant CHARACTER format"
            )
        })
    }
    fn io_items(&self, items: &[Expr], scope: &Scope) -> Result<Vec<(Expr, Option<String>)>> {
        let mut out = vec![];
        for original in items {
            let e = raw(original);
            let ty = self.expression(&mut original.clone(), scope, false)?;
            if let Expr::Call(n, a) = e {
                if n == "@array_constructor" {
                    out.extend(self.io_items(a, scope)?);
                    continue;
                }
                if n == "@record_constructor"
                    && matches!(a.first(),Some(Expr::Ident(ty)) if self.types.contains_key(ty))
                {
                    out.extend(self.io_items(&a[1..], scope)?);
                    continue;
                }
                if matches!(n.as_str(), "@output_implied_do" | "@input_implied_do") {
                    bail!("formatted enumeration I/O currently requires scalar items or fixed whole arrays");
                }
            }
            let shape = if let Expr::Ident(n) = e {
                scope.shapes.get(n).or_else(|| self.globals.shapes.get(n))
            } else {
                None
            };
            if let Some(shape) = shape.filter(|d| !d.is_empty()) {
                let mut indices: Vec<Vec<Expr>> = vec![vec![]];
                for dim in shape {
                    let (first, last) = match dim {
                        Expr::Call(n, a) if n == "@declared_bounds" => {
                            (self.constant(&a[0], scope), self.constant(&a[1], scope))
                        }
                        dim => (Some(1), self.constant(dim, scope)),
                    };
                    let (Some(first), Some(last)) = (first, last) else {
                        bail!("formatted enumeration I/O currently requires fixed array bounds");
                    };
                    let mut expanded = vec![];
                    for i in first..=last {
                        for old in &indices {
                            let mut at = old.clone();
                            at.push(int(i));
                            expanded.push(at);
                        }
                    }
                    indices = expanded;
                }
                let Expr::Ident(name) = e else { unreachable!() };
                out.extend(indices.into_iter().map(|at| {
                    (
                        Expr::Index(name.clone(), at),
                        ty.enumeration().map(str::to_owned),
                    )
                }));
            } else {
                if ty.rank() > 0 || self.io_array_expression(e, scope) {
                    bail!("formatted enumeration I/O currently requires scalar items or fixed whole arrays");
                }
                out.push((e.clone(), ty.enumeration().map(str::to_owned)));
            }
        }
        Ok(out)
    }
    pub(super) fn check_io(
        &self,
        args: &[Expr],
        types: &[Ty],
        scope: &Scope,
        layout: Layout,
    ) -> Result<()> {
        if types[..layout.first]
            .iter()
            .any(|t| t.enumeration().is_some())
        {
            bail!("enumeration values cannot be used as I/O controls");
        }
        // Section 9.3 enums use INTEGER editing. Let the shared output path
        // handle runtime formats, sections, constructors and implied-DO lists.
        if !layout.input && types.iter().all(|t| t.enumeration().is_none_or(|n| self.is_c_enum(n)))
            && (self.io_format(&args[layout.format], scope).is_err()
                || self.io_items(&args[layout.first..], scope).is_err()) {
            return Ok(());
        }
        if matches!(&args[layout.format], Expr::Call(n,_) if n == "@list_directed") {
            bail!("list-directed input/output is not permitted for enumeration values; use INT for output");
        }
        let format = self.io_format(&args[layout.format], scope)?;
        if layout.input && args[layout.first..].iter().any(|e|
            matches!(raw(e), Expr::Call(n,_) if matches!(n.as_str(), "@array_constructor" | "@record_constructor"))) {
            bail!("enumeration READ item must designate writable storage");
        }
        let items = self.io_items(&args[layout.first..], scope)?;
        if layout.input {
            if layout.first == 4 {
                if types.get(3) != Some(&Ty::Integer(0)) {
                    bail!("IOSTAT must be a writable scalar INTEGER variable");
                }
                let name = match raw(&args[3]) {
                    Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => n,
                    _ => bail!("IOSTAT must be a writable scalar INTEGER variable"),
                };
                if scope.parameters.contains(name)
                    || self.globals.parameters.contains(name)
                    || scope
                        .readonly
                        .contains(name.split('%').next().unwrap_or(name))
                {
                    bail!("IOSTAT must be a writable scalar INTEGER variable");
                }
            }
            for (item, _) in &items {
                let name = match item {
                    Expr::Ident(n) | Expr::Index(n, _) => n,
                    Expr::Call(n, _) if self.name(n, scope).rank() > 0 => n,
                    _ => bail!("enumeration READ item must designate writable storage"),
                };
                let root = name.split('%').next().unwrap_or(name);
                if scope.parameters.contains(name) || self.globals.parameters.contains(name) {
                    bail!("cannot READ into named constant `{name}`");
                }
                if scope.readonly.contains(root) {
                    bail!("READ item `{name}` has INTENT(IN)");
                }
            }
        }
        let mut parser = crate::format_spec::GroupParser::new(format.as_bytes(), items.len())
            .map_err(anyhow::Error::msg)?;
        let mut next = items.iter();
        while let Some(d) = parser.next().map_err(anyhow::Error::msg)? {
            if !crate::format_spec::is_data(d.code) {
                continue;
            }
            for _ in 0..d.repeat {
                if let Some((_, Some(owner))) = next.next() {
                    let c_enum = self.is_c_enum(owner);
                    if !matches!(d.code, b'i' | b'b' | b'o' | b'z') && !(c_enum && d.code == b'g') {
                        if c_enum { bail!("enum values require an I, B, O, Z or G edit descriptor"); }
                        bail!("enumeration values require an I, B, O or Z edit descriptor");
                    }
                    if layout.input && !c_enum && d.code != b'i' {
                        bail!("formatted enumeration READ currently supports only the I edit descriptor");
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn lower_io(&mut self, stmt: &Stmt, scope: &Scope) -> Result<Option<Stmt>> {
        let Stmt::CallSub { name, args } = stmt else {
            return Ok(None);
        };
        let Some(layout) = layout(name, args) else {
            return Ok(None);
        };
        let types = args
            .iter()
            .map(|e| {
                let ty = self.expression(&mut e.clone(), scope, false)?;
                self.io_nested_type(e, scope, ty)
            })
            .collect::<Result<Vec<_>>>()?;
        if !types.iter().any(|t| t.enumeration().is_some()) {
            return Ok(None);
        }
        self.check_io(args, &types, scope, layout)?;
        if !layout.input && types.iter().all(|t| t.enumeration().is_none_or(|n| self.is_c_enum(n)))
            && (self.io_format(&args[layout.format], scope).is_err()
                || self.io_items(&args[layout.first..], scope).is_err()) {
            return Ok(None);
        }
        let items = self.io_items(&args[layout.first..], scope)?;
        let format = Expr::Str(self.io_format(&args[layout.format], scope)?);
        if !layout.input {
            let mut values = args[..layout.first].to_vec();
            values[layout.format] = format;
            values.extend(items.into_iter().map(|(e, _)| e));
            for value in &mut values {
                self.erase_expr(value, scope)?;
            }
            return Ok(Some(Stmt::CallSub {
                name: name.clone(),
                args: values,
            }));
        }
        let status = if name == "@formatted_read_status" {
            if !matches!(&args[2], Expr::Str(control) if control.eq_ignore_ascii_case("iostat")) {
                bail!("formatted enumeration READ currently supports only IOSTAT control");
            }
            Some(args[3].clone())
        } else {
            None
        };
        self.serial += 1;
        let code = format!("@enumeration_read_status_{}", self.serial);
        let mut body = vec![Stmt::VarDecl {
            kind: TypeSpec::Integer(None),
            names: vec![code.clone()],
        }];
        let mut read = vec![
            args[0].clone(),
            format,
            Expr::Str("iostat".into()),
            Expr::Ident(code.clone()),
        ];
        let mut transfers = vec![];
        for (mut target, ty) in items {
            let kind = if ty.is_some() {
                TypeSpec::Integer(None)
            } else {
                let name = match &target {
                    Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => n,
                    _ => unreachable!(),
                };
                self.object_kind(name, scope).unwrap_or_else(|| {
                    if matches!(self.name(name, scope), Ty::Integer(_)) {
                        TypeSpec::Integer(None)
                    } else {
                        TypeSpec::Real
                    }
                })
            };
            self.erase_expr(&mut target, scope)?;
            self.serial += 1;
            let temp = format!("@enumeration_read_value_{}", self.serial);
            let value = Expr::Ident(temp.clone());
            let initial = match &kind {
                TypeSpec::Character(_)
                | TypeSpec::CharacterLength(_)
                | TypeSpec::CharacterAssumed => Expr::Str(String::new()),
                TypeSpec::Logical | TypeSpec::LogicalKind(_) => Expr::Logical(false),
                _ => int(0),
            };
            body.push(Stmt::VarDecl {
                kind,
                names: vec![temp.clone()],
            });
            body.push(Stmt::Assign {
                name: temp,
                value: initial,
            });
            read.push(value.clone());
            if let Some(ty) = ty.filter(|ty| !self.is_c_enum(ty)) {
                let max = count(&self.types[&ty]) as i64;
                let invalid = Expr::Bin(
                    BinOp::Or,
                    Box::new(Expr::Bin(
                        BinOp::Lt,
                        Box::new(value.clone()),
                        Box::new(int(1)),
                    )),
                    Box::new(Expr::Bin(
                        BinOp::Gt,
                        Box::new(value.clone()),
                        Box::new(int(max)),
                    )),
                );
                let failure = if status.is_some() {
                    Stmt::Assign {
                        name: code.clone(),
                        value: int(1),
                    }
                } else {
                    Stmt::CallSub { name: "@error_stop".into(), args: vec![Expr::Str(format!("@enumeration_runtime:formatted READ ordinal outside 1..{max} for enumeration `{ty}`"))] }
                };
                transfers.push(Stmt::If {
                    cond: eq(&code, 0),
                    then_body: vec![Stmt::If {
                        cond: invalid,
                        then_body: vec![failure],
                        else_body: Some(vec![assign(target, value)]),
                    }],
                    else_body: None,
                });
            } else {
                // Commit in I/O-list order: a later subscript may depend on an
                // earlier value, including an enumeration converted with INT.
                transfers.push(Stmt::If {
                    cond: eq(&code, 0),
                    then_body: vec![assign(target, value)],
                    else_body: None,
                });
            }
        }
        for value in &mut read[..2] {
            self.erase_expr(value, scope)?;
        }
        body.push(Stmt::CallSub {
            name: "@formatted_read_status".into(),
            args: read,
        });
        body.extend(transfers);
        if let Some(status) = status {
            body.push(assign(raw(&status).clone(), Expr::Ident(code)));
        } else {
            body.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(Expr::Ident(code)), Box::new(int(0))),
                then_body: vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str("@enumeration_runtime:formatted enumeration READ failed".into())],
                }],
                else_body: None,
            });
        }
        Ok(Some(Stmt::Block { body }))
    }
}

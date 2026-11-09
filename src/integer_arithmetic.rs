// SPDX-License-Identifier: MIT
//! Preserve semantic INTEGER widths while the native ABI uses 64-bit cells.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::HashMap;

pub(crate) fn decode(n: &str) -> Option<(&str, u8)> {
    let (op, k) = n.strip_prefix("@integer:")?.split_once(':')?;
    let k = k.parse().ok()?;
    (matches!(k, 1 | 2 | 4 | 8)
        && matches!(
            op,
            "add" | "sub" | "mul" | "div" | "pow" | "neg" | "abs" | "cast" | "real"
        ))
    .then_some((op, k))
}
pub(crate) fn evaluate(op: &str, k: u8, a: i64, b: i64, checked: bool) -> Result<i64> {
    let min = -(1i128 << (k * 8 - 1));
    let max = -min - 1;
    let fit = |x: i128| -> Result<i64> {
        if checked && (x < min || x > max) {
            bail!("INTEGER(kind={k}) overflow");
        }
        Ok(match k {
            1 => x as i8 as i64,
            2 => x as i16 as i64,
            4 => x as i32 as i64,
            _ => x as i64,
        })
    };
    match op {
        "add" => fit(a as i128 + b as i128),
        "sub" => fit(a as i128 - b as i128),
        "mul" => fit(a as i128 * b as i128),
        "neg" => fit(-(a as i128)),
        "abs" => fit((a as i128).abs()),
        "cast" => fit(a as i128),
        "div" => {
            if b == 0 {
                bail!("integer division by zero");
            }
            fit(a as i128 / b as i128)
        }
        "pow" => {
            if b < 0 {
                if a == 0 {
                    bail!("zero raised to a negative integer power");
                }
                return Ok(if a == 1 {
                    1
                } else if a == -1 {
                    if b & 1 == 0 {
                        1
                    } else {
                        -1
                    }
                } else {
                    0
                });
            }
            let mut e = b as u64;
            let mut factor = a;
            let mut value = 1;
            while e != 0 {
                if e & 1 != 0 {
                    value = fit(value as i128 * factor as i128)?;
                }
                e >>= 1;
                if e != 0 {
                    factor = fit(factor as i128 * factor as i128)?;
                }
            }
            Ok(value)
        }
        _ => bail!("unknown integer operation"),
    }
}
fn constant(e: &Expr) -> Option<i64> {
    match e {
        Expr::IntLit(n) => n.parse().ok(),
        Expr::Call(n, a) if n == "@integer_literal" => constant(&a[0]),
        _ => None,
    }
}
type Types = HashMap<String, (TypeSpec, usize)>;
fn kind(e: &Expr, types: &Types) -> Option<u8> {
    if let Expr::Call(n, _) = e {
        if let Some(k) = n
            .strip_prefix("@ptr_load_i")
            .or_else(|| n.strip_prefix("@ptr_load_c_i"))
        {
            return Some(if k.is_empty() { 8 } else { k.parse().ok()? });
        }
    }
    // Lowering has already checked ranks and subscripts. Infer element and
    // procedure-result types without treating a generated subscript function
    // as an array inquiry, which would lose the surrounding arithmetic kind.
    match e {
        Expr::IntLit(_) => return Some(4),
        Expr::Ident(n) | Expr::Index(n, _) => {
            return types.get(n).and_then(|(k, _)| {
                if let TypeSpec::Integer(k) = k {
                    Some(k.unwrap_or(4))
                } else {
                    None
                }
            })
        }
        Expr::Bin(op, a, b)
            if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
            ) =>
        {
            let (a, b) = (kind(a, types)?, kind(b, types)?);
            // INTEGER ** INTEGER has the kind of the operand with the greater
            // range, as other operators do.
            return Some(a.max(b));
        }
        Expr::Un(UnOp::Neg, a) => return kind(a, types),
        Expr::Call(n, _) if types.contains_key(n) => {
            return types.get(n).and_then(|(k, _)| {
                if let TypeSpec::Integer(k) = k {
                    Some(k.unwrap_or(4))
                } else {
                    None
                }
            })
        }
        Expr::Call(n, a) if n == "abs" && a.len() == 1 => return kind(&a[0], types),
        _ => {}
    }
    if let Expr::Call(n, _) = e {
        if let Some((_, k)) = decode(n) {
            return Some(k);
        }
    }
    if let Expr::Call(n, a) = e {
        if n == "int" && !types.contains_key(n) {
            return Some(a.get(1).and_then(constant).unwrap_or(4) as u8);
        }
    }
    match crate::inquiries::model_type(e, &|n| types.get(n).cloned())
        .ok()
        .flatten()?
    {
        TypeSpec::Integer(k) => Some(k.unwrap_or(4)),
        _ => None,
    }
}
fn tag(op: &str, k: u8, a: Expr, b: Expr, checks: bool, line: usize) -> Result<Expr> {
    if matches!(op, "cast" | "real") && constant(&a).is_none() {
        if let Ok(value) = crate::parameters::real_constant(&a) {
            let value = value.trunc();
            let limit = 2.0f64.powi(k as i32 * 8 - 1);
            if !value.is_finite() || value < -limit || value >= limit {
                bail!("INTEGER(kind={k}) overflow in constant conversion");
            }
            return tag(
                "cast",
                k,
                Expr::IntLit((value as i64).to_string()),
                Expr::IntLit("0".into()),
                checks,
                line,
            );
        }
    }
    if let (Some(a), Some(b)) = (constant(&a), constant(&b)) {
        let value = evaluate(op, k, a, b, true)?;
        return Ok(if k == 4 {
            Expr::IntLit(value.to_string())
        } else {
            Expr::Call(
                "@integer_literal".into(),
                vec![Expr::IntLit(value.to_string()), Expr::IntLit(k.to_string())],
            )
        });
    }
    Ok(Expr::Call(
        format!("@integer:{op}:{k}"),
        vec![a, b, Expr::Logical(checks), Expr::IntLit(line.to_string())],
    ))
}
fn expression(e: &mut Expr, types: &Types, checks: bool, line: usize) -> Result<()> {
    // Older array lowering encoded numeric narrowing as IOR(value,0). Keep
    // genuine bit operations, but restore that wider-to-narrower conversion
    // before it can silently discard the value that needs checking.
    if let Expr::Call(n, a) = e {
        if let Some(("ior", target)) = crate::bit_intrinsics::decode(n) {
            if a.len() == 2
                && constant(&a[1]) == Some(0)
                && kind(&a[0], types).is_some_and(|k| k > target)
            {
                *e = Expr::Call(
                    "int".into(),
                    vec![a[0].clone(), Expr::IntLit(target.to_string())],
                );
            }
        }
    }
    let k = kind(e, types);
    if matches!(e,Expr::Call(n,_) if n=="@integer_literal" || decode(n).is_some()) {
        return Ok(());
    }
    match e {
        Expr::Bin(_, a, b) => {
            expression(a, types, checks, line)?;
            expression(b, types, checks, line)?;
        }
        Expr::Un(_, a) => expression(a, types, checks, line)?,
        Expr::Call(_, a) | Expr::Index(_, a) => {
            for a in a {
                expression(a, types, checks, line)?;
            }
        }
        _ => {}
    }
    if let Some(k) = k.filter(|k| matches!(k, 1 | 2 | 4 | 8)) {
        let zero = Expr::IntLit("0".into());
        let result = match e {
            Expr::Bin(op, a, b) => {
                let op = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    BinOp::Pow => "pow",
                    _ => return Ok(()),
                };
                Some(tag(op, k, *a.clone(), *b.clone(), checks, line)?)
            }
            Expr::Un(UnOp::Neg, a) => Some(tag("neg", k, *a.clone(), zero, checks, line)?),
            Expr::Call(n, a) if n == "abs" && !types.contains_key(n) => {
                Some(tag("abs", k, a[0].clone(), zero, checks, line)?)
            }
            Expr::Call(n, a) if n == "int" && !types.contains_key(n) => {
                let op = if kind(&a[0], types).is_some() {
                    "cast"
                } else {
                    "real"
                };
                Some(tag(op, k, a[0].clone(), zero, checks, line)?)
            }
            _ => None,
        };
        if let Some(result) = result {
            *e = result;
        }
    }
    Ok(())
}
pub(crate) fn resolve(
    mut program: Program,
    checks: bool,
    source: &str,
    origins: &[crate::source_diagnostics::SourceOrigin],
) -> Result<Program> {
    fn functions(body: &[Stmt], out: &mut Types) {
        for mut s in body.to_vec() {
            if let Stmt::Function {
                name,
                return_type: Some(k),
                ..
            } = &s
            {
                out.insert(name.clone(), (k.clone(), 0));
            }
            for b in crate::procedure_arguments::children(&mut s) {
                functions(b, out);
            }
        }
    }
    fn walk(
        body: &mut [Stmt],
        parent: &Types,
        checks: bool,
        source: &str,
        origins: &[crate::source_diagnostics::SourceOrigin],
        inherited: Option<usize>,
    ) -> Result<()> {
        let mut types = parent.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        types.insert(n.clone(), (kind.clone(), 0));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    types.insert(name.clone(), (kind.clone(), dims.len()));
                }
                _ => {}
            }
        }
        let mut offset = inherited;
        for s in body {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(*p);
                continue;
            }
            let line = offset.map_or(0, |p| {
                let (text, p) = origins
                    .iter()
                    .find(|o| o.range.contains(&p))
                    .map_or((source, p), |o| (o.source(source), o.position(p)));
                text.as_bytes()[..p.min(text.len())]
                    .iter()
                    .filter(|b| **b == b'\n')
                    .count()
                    + 1
            });
            (|| -> Result<()> {
                // Dimensions and metadata are compiler bookkeeping, not user
                // numeric expressions. Their representation remains unchanged.
                if !matches!(
                    s,
                    Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::DummyIntent { .. }
                ) {
                    for e in crate::procedure_arguments::expressions(s) {
                        expression(e, &types, checks, line)?;
                    }
                }
                if let Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. } = s {
                    if let Some((TypeSpec::Integer(k), _)) = types.get(name) {
                        let k = k.unwrap_or(4);
                        if matches!(k, 1 | 2 | 4 | 8)
                            && (kind(value, &types) != Some(k) || constant(value).is_some())
                        {
                            *value = tag(
                                "cast",
                                k,
                                value.clone(),
                                Expr::IntLit("0".into()),
                                checks,
                                line,
                            )?;
                        }
                    }
                }
                if let Stmt::CallSub { name, args } = s {
                    if let Some(k) = name
                        .strip_prefix("@ptr_store_i")
                        .or_else(|| name.strip_prefix("@ptr_store_c_i"))
                        .and_then(|k| k.parse::<u8>().ok())
                    {
                        if matches!(k, 1 | 2 | 4 | 8) && args.len() == 4 {
                            args[3] = tag(
                                "cast",
                                k,
                                args[3].clone(),
                                Expr::IntLit("0".into()),
                                checks,
                                line,
                            )?;
                        }
                    }
                }
                for b in crate::procedure_arguments::children(s) {
                    walk(b, &types, checks, source, origins, offset)?;
                }
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, offset))?;
        }
        Ok(())
    }
    let mut types = Types::new();
    functions(&program.body, &mut types);
    walk(&mut program.body, &types, checks, source, origins, None)?;
    Ok(program)
}

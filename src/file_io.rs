// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Read-only sequential formatted files. Keep unsupported connection modes explicit.
use crate::ast::{Expr, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

pub(crate) fn unit(expr: &Expr, symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>) -> Result<()> {
    match expr {
        Expr::IntLit(_) => {
            let n = crate::parameters::integer_constant(expr)?;
            if !(0..1024).contains(&n) || matches!(n, 5 | 6) {
                bail!("file units must be in 0..1023, excluding preconnected units 5 and 6");
            }
        }
        Expr::Ident(n) if matches!(symbols.get(n), Some((TypeSpec::Integer(None | Some(4) | Some(8)), dims)) if dims.is_empty()) =>
            {}
        _ => bail!(
            "file unit currently requires a scalar INTEGER variable of kind 4 or 8, or a constant"
        ),
    }
    Ok(())
}

pub(crate) fn lower(
    name: &str,
    args: &[Expr],
    symbols: &HashMap<String, (TypeSpec, Vec<Expr>)>,
) -> Result<Stmt> {
    let operation = name.strip_prefix("@io_").unwrap();
    let mut controls = HashMap::new();
    for (i, arg) in args.iter().enumerate() {
        let (key, value) = match arg {
            Expr::Call(n, values) if n.starts_with("@io_spec:") && values.len() == 1 => (
                n.trim_start_matches("@io_spec:").to_owned(),
                values[0].clone(),
            ),
            _ if i == 0 => ("unit".into(), arg.clone()),
            _ => bail!("{operation}: only UNIT may be positional"),
        };
        if !matches!(
            key.as_str(),
            "unit" | "file" | "status" | "action" | "access" | "form"
        ) {
            bail!("{operation}: unsupported I/O control `{key}`");
        }
        if operation != "open" && key != "unit" {
            bail!("{operation} currently supports only UNIT");
        }
        if controls.insert(key.clone(), value).is_some() {
            bail!("{operation}: duplicate {key} control");
        }
    }
    let unit_expr = controls
        .remove("unit")
        .context("file operation requires UNIT")?;
    unit(&unit_expr, symbols)?;
    let mut args = vec![unit_expr];
    if operation == "open" {
        for (key, expected, required) in [
            ("status", "old", true),
            ("action", "read", true),
            ("access", "sequential", false),
            ("form", "formatted", false),
        ] {
            match controls.remove(key) {
                Some(Expr::Str(s)) if s.trim_end().eq_ignore_ascii_case(expected) => {}
                None if !required => {}
                _ => bail!(
                    "OPEN currently requires {key}='{expected}'{}",
                    if required {
                        " explicitly"
                    } else {
                        " when specified"
                    }
                ),
            }
        }
        let Some(Expr::Str(path)) = controls.remove("file") else {
            bail!("OPEN currently requires a constant FILE name");
        };
        let path = path.trim_end();
        if path.is_empty() || path.contains('\0') {
            bail!("OPEN requires a nonempty FILE name without NUL bytes");
        }
        args.push(Expr::Str(path.into()));
    }
    Ok(Stmt::CallSub {
        name: format!("@file_{operation}"),
        args,
    })
}

#[derive(Default)]
pub(crate) struct Files(HashMap<i64, std::fs::File>);
impl Files {
    fn valid(unit: i64) -> Result<()> {
        if !(0..1024).contains(&unit) || matches!(unit, 5 | 6) {
            bail!("file units must be in 0..1023, excluding preconnected units 5 and 6");
        }
        Ok(())
    }
    pub(crate) fn operation(&mut self, op: &str, unit: i64, path: Option<&str>) -> Result<()> {
        Self::valid(unit)?;
        if op == "@file_open" {
            if self.0.contains_key(&unit) {
                bail!("OPEN on an already connected unit is not yet supported");
            }
            let file = std::fs::File::open(path.unwrap()).context("OPEN failed")?;
            self.0.insert(unit, file);
            return Ok(());
        }
        // CLOSE on an unconnected unit is a successful no-op.
        if op == "@file_close" {
            self.0.remove(&unit);
            return Ok(());
        }
        let file = self
            .0
            .get_mut(&unit)
            .context("file unit is not connected")?;
        if op == "@file_rewind" {
            file.seek(SeekFrom::Start(0))?;
            return Ok(());
        }
        let mut position = file.stream_position()?;
        let mut byte = [0];
        if position > 0 {
            file.seek(SeekFrom::Start(position - 1))?;
            file.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                position -= 1;
            }
        }
        while position > 0 {
            file.seek(SeekFrom::Start(position - 1))?;
            file.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                break;
            }
            position -= 1;
        }
        file.seek(SeekFrom::Start(position))?;
        Ok(())
    }
    pub(crate) fn record(&mut self, unit: i64, length: usize) -> Result<String> {
        Self::valid(unit)?;
        let file = self
            .0
            .get_mut(&unit)
            .context("file unit is not connected")?;
        let mut bytes = Vec::new();
        let mut count = 0usize;
        let mut previous = 0;
        loop {
            let mut byte = [0];
            if file.read(&mut byte)? == 0 {
                if count == 0 {
                    bail!("end of file or input error in formatted READ");
                }
                break;
            }
            if byte[0] == b'\n' {
                if previous == b'\r' && count <= length {
                    bytes.pop();
                }
                break;
            }
            if byte[0] == 0 || byte[0] > 127 {
                bail!("formatted READ currently requires non-NUL ASCII input");
            }
            if count < length {
                bytes.push(byte[0]);
            }
            count = (count + 1).min(length + 1);
            previous = byte[0];
        }
        bytes.resize(length, b' ');
        Ok(String::from_utf8(bytes).unwrap())
    }
}

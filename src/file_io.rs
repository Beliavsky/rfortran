// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Sequential formatted files, including scratch connections.
use crate::ast::{Expr, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{Read, Write, Seek, SeekFrom};

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
            "unit" | "newunit" | "file" | "status" | "action" | "access" | "form" | "name"
        ) {
            bail!("{operation}: unsupported I/O control `{key}`");
        }
        if operation != "open" && key != "unit" && !(operation=="inquire" && key=="name") {
            bail!("{operation} currently supports only UNIT");
        }
        if operation=="open" && key=="name" {bail!("OPEN: unsupported I/O control `name`");}
        if controls.insert(key.clone(), value).is_some() {
            bail!("{operation}: duplicate {key} control");
        }
    }
    if operation=="open" && matches!(controls.get("status"),Some(Expr::Str(s)) if s.trim_end().eq_ignore_ascii_case("scratch")) {
        if controls.contains_key("file") { bail!("scratch OPEN must not specify FILE"); }
        let newunit=controls.remove("newunit");
        if newunit.is_some() && controls.contains_key("unit") {bail!("OPEN must not specify both UNIT and NEWUNIT");}
        let destination=newunit.clone().or_else(||controls.remove("unit")).context("OPEN requires UNIT or NEWUNIT")?;
        unit(&destination,symbols)?;
        if newunit.is_some() && !matches!(destination,Expr::Ident(_)) {bail!("NEWUNIT requires a writable scalar INTEGER variable");}
        for (key,expected) in [("action","readwrite"),("access","sequential"),("form","formatted")] {
            if let Some(v)=controls.remove(key) {
                if !matches!(v,Expr::Str(s) if s.trim_end().eq_ignore_ascii_case(expected)) {bail!("scratch OPEN currently supports {key}='{expected}'");}
            }
        }
        return Ok(Stmt::CallSub {name:if newunit.is_some(){"@file_newunit"}else{"@file_scratch"}.into(),args:vec![destination]});
    }
    if controls.contains_key("newunit") {bail!("NEWUNIT currently requires STATUS='scratch'");}
    let unit_expr = controls
        .remove("unit")
        .context("file operation requires UNIT")?;
    unit(&unit_expr, symbols)?;
    let mut args = vec![unit_expr];
    if operation=="inquire" {
        let dest=controls.remove("name").context("INQUIRE currently requires NAME")?;
        if !matches!(&dest,Expr::Ident(n) if matches!(symbols.get(n),Some((TypeSpec::Character(_),d)) if d.is_empty())) {
            bail!("INQUIRE NAME requires a writable scalar CHARACTER variable");
        }
        let Expr::Ident(n)=&dest else {unreachable!()};
        let TypeSpec::Character(length)=symbols[n].0 else {unreachable!()};
        return Ok(Stmt::CallSub {name:"@file_name".into(),args:vec![dest,args.remove(0),Expr::IntLit(length.unwrap_or(1).to_string())]});
    }
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
pub(crate) struct Files(HashMap<i64, std::fs::File>, HashMap<i64,String>);
impl Files {
    fn valid(unit: i64) -> Result<()> {
        if (!(0..1024).contains(&unit) && !(-1033..=-10).contains(&unit)) || matches!(unit, 5 | 6) {
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
            self.1.insert(unit,path.unwrap().into());
            return Ok(());
        }
        // CLOSE on an unconnected unit is a successful no-op.
        if op == "@file_close" {
            self.0.remove(&unit);
            self.1.remove(&unit);
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
    pub(crate) fn scratch(&mut self, requested: Option<i64>) -> Result<i64> {
        let unit=if let Some(unit)=requested {Self::valid(unit)?;unit} else {
            (-1033..=-10).rev().find(|u|!self.0.contains_key(u)).context("no free NEWUNIT file slots")?
        };
        if self.0.contains_key(&unit) {bail!("OPEN on an already connected unit is not yet supported");}
        // Delete-on-close on Windows; on Unix unlink immediately after opening.
        let mut options=std::fs::OpenOptions::new(); options.read(true).write(true).create_new(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x04000000).access_mode(0xc0010000).share_mode(0x7);
        }
        let mut result=None;
        for serial in 0..1000 {
            let path=std::env::temp_dir().join(format!("rfort-scratch-{}-{unit}-{serial}",std::process::id()));
            match options.open(&path) {
                Ok(file)=>{
                    #[cfg(not(windows))] std::fs::remove_file(&path)?;
                    result=Some(file);break;
                }
                Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,
                Err(e)=>return Err(e.into()),
            }
        }
        self.0.insert(unit,result.context("could not create scratch file")?);
        self.1.remove(&unit);
        Ok(unit)
    }
    pub(crate) fn name(&self, unit:i64)->Result<String> {
        Self::valid(unit)?;
        if !self.0.contains_key(&unit) {bail!("file unit is not connected");}
        Ok(self.1.get(&unit).cloned().unwrap_or_default())
    }
    pub(crate) fn write_record(&mut self, unit:i64, text:&str)->Result<()> {
        Self::valid(unit)?;
        let file=self.0.get_mut(&unit).context("file unit is not connected")?;
        file.write_all(text.as_bytes())?;file.write_all(b"\n")?;
        let position=file.stream_position()?;file.set_len(position)?;
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

    pub(crate) fn record_status(&mut self, unit: i64, buffer: &mut [u8]) -> i64 {
        if unit == 5 {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            return crate::record_input::record(buffer, || {
                let mut byte = [0];
                match input.read(&mut byte) {
                    Ok(0) => Ok(None), Ok(_) => Ok(Some(byte[0])), Err(_) => Err(()),
                }
            });
        }
        if Self::valid(unit).is_err() { return crate::record_input::ERROR; }
        let Some(file) = self.0.get_mut(&unit) else { return crate::record_input::ERROR; };
        crate::record_input::record(buffer, || {
            let mut byte = [0];
            match file.read(&mut byte) {
                Ok(0) => Ok(None), Ok(_) => Ok(Some(byte[0])), Err(_) => Err(()),
            }
        })
    }
}

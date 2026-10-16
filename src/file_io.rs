// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Sequential formatted files, including scratch connections.
use crate::ast::{Expr, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{Read, Write, Seek, SeekFrom};
#[path="file_inquiry.rs"]
mod inquiry;

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
    serial:&mut usize,
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
            "unit" | "newunit" | "file" | "status" | "action" | "access" | "form" | "name" | "opened" | "number" | "iostat"
        ) {
            bail!("{operation}: unsupported I/O control `{key}`");
        }
        if operation=="inquire" && !matches!(key.as_str(),"unit"|"file"|"name"|"opened"|"number"|"iostat") {bail!("INQUIRE: unsupported I/O control `{key}`");}
        if operation != "open" && operation!="inquire" && key != "unit" {
            bail!("{operation} currently supports only UNIT");
        }
        if operation=="open" && matches!(key.as_str(),"name"|"opened"|"number"|"iostat") {bail!("OPEN: unsupported I/O control `{key}`");}
        if controls.insert(key.clone(), value).is_some() {
            bail!("{operation}: duplicate {key} control");
        }
    }
    if operation=="inquire" {return lower_inquiry(controls,symbols,serial);}
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

fn lower_inquiry(mut controls:HashMap<String,Expr>,symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>,serial:&mut usize)->Result<Stmt> {
    let unit=controls.remove("unit");let file=controls.remove("file");
    if unit.is_some()==file.is_some() {bail!("INQUIRE requires exactly one of UNIT or FILE");}
    let by_file=file.is_some();let selector=file.or(unit).unwrap();
    fn character(e:&Expr,symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>)->bool {
        match e {
            Expr::Str(_)=>true,
            Expr::Ident(n)=>matches!(symbols.get(n),Some((TypeSpec::Character(_),d)) if d.is_empty()),
            Expr::Bin(crate::ast::BinOp::Concat,a,b)=>character(a,symbols)&&character(b,symbols),
            Expr::Call(n,a) if matches!(n.as_str(),"trim"|"adjustl"|"adjustr") && a.len()==1=>character(&a[0],symbols),
            _=>false,
        }
    }
    let kind=if by_file && character(&selector,symbols) {TypeSpec::Character(None)} else {
        crate::array_calls::scalar_actual_kind(&selector,symbols).context("INQUIRE selector must be a scalar expression")?
    };
    if if by_file {!matches!(kind,TypeSpec::Character(_))} else {!matches!(kind,TypeSpec::Integer(_))} {
        bail!("INQUIRE {} requires a scalar {} expression",if by_file {"FILE"}else{"UNIT"},if by_file {"CHARACTER"}else{"INTEGER"});
    }
    for (key,value) in &controls {
        let Expr::Ident(n)=value else {bail!("INQUIRE {} requires a writable scalar variable",key.to_ascii_uppercase());};
        let valid=symbols.get(n).is_some_and(|(kind,dims)|dims.is_empty() && match key.as_str() {
            "name"=>matches!(kind,TypeSpec::Character(_)),"opened"=>*kind==TypeSpec::Logical,
            _=>matches!(kind,TypeSpec::Integer(None|Some(1|2|4|8))),
        });
        if !valid {bail!("INQUIRE {} requires a writable scalar {} variable",key.to_ascii_uppercase(),match key.as_str(){"name"=>"CHARACTER","opened"=>"LOGICAL",_=>"INTEGER"});}
    }
    *serial+=1;
    let names:Vec<String>=["status","number","opened","name"].iter().map(|key|format!("@inquire_{key}_{serial}")).collect();
    let id=|i:usize|Expr::Ident(names[i].clone());let int=|n:i64|Expr::IntLit(n.to_string());
    let name=controls.remove("name").unwrap_or_else(||id(3));
    let length=if name==id(3) {0} else {
        let Expr::Ident(n)=&name else {unreachable!()};let TypeSpec::Character(length)=symbols[n].0 else {unreachable!()};length.unwrap_or(1)
    };
    let mut body=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:names[..3].to_vec()}];
    if name==id(3) {body.push(Stmt::VarDecl{kind:TypeSpec::Character(Some(0)),names:vec![names[3].clone()]});}
    body.push(Stmt::CallSub{name:if by_file {"@file_inquire_file"}else{"@file_inquire_unit"}.into(),args:vec![id(0),id(1),id(2),name,selector,int(length as i64)]});
    if let Some(dest)=controls.remove("iostat") {
        let Expr::Ident(name)=dest else {unreachable!()};body.push(Stmt::Assign{name,value:id(0)});
    } else {
        body.push(Stmt::If{cond:Expr::Bin(crate::ast::BinOp::Ne,Box::new(id(0)),Box::new(int(0))),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("INQUIRE failed".into())]}],else_body:None});
    }
    if let Some(Expr::Ident(name))=controls.remove("number") {body.push(Stmt::Assign{name,value:id(1)});}
    if let Some(Expr::Ident(name))=controls.remove("opened") {body.push(Stmt::Assign{name,value:Expr::Bin(crate::ast::BinOp::Ne,Box::new(id(2)),Box::new(int(0)))});}
    Ok(Stmt::Block{body})
}

#[derive(Default)]
pub(crate) struct Files(HashMap<i64, std::fs::File>, HashMap<i64,String>);
impl Files {
    pub(crate) fn inquire(&self,unit:Option<i64>,path:Option<&str>)->(i64,i64,i64,String) {
        if let Some(path)=path {
            if path.as_bytes().contains(&0) {return (1,-1,0,String::new());}
            let found=self.1.iter().find(|(unit,name)|self.0.contains_key(unit) && inquiry::same_name(path.as_bytes(),name.as_bytes()));
            return match found {Some((unit,name))=>(0,*unit,1,name.clone()),None=>(0,-1,0,String::new())};
        }
        let unit=unit.unwrap();
        if inquiry::invalid_unit(unit) {return (1,-1,0,String::new());}
        if matches!(unit,5|6) {return (0,unit,1,if unit==5 {"stdin"}else{"stdout"}.into());}
        if inquiry::index(unit).is_some_and(|index|self.0.contains_key(&inquiry::unit(index))) {(0,unit,1,self.1.get(&unit).cloned().unwrap_or_default())}else{(0,-1,0,String::new())}
    }
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
        self.record_status_mode(unit,buffer,false)
    }
    pub(crate) fn record_status_mode(&mut self, unit:i64,buffer:&mut [u8],checked:bool)->i64 {
        let scan=|buffer:&mut [u8], byte:&mut dyn FnMut()->Result<Option<u8>,()>| {
            if checked {crate::record_input::checked_record(buffer,byte)} else {crate::record_input::record(buffer,byte)}
        };
        if unit == 5 {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            return scan(buffer, &mut || {
                let mut byte = [0];
                match input.read(&mut byte) {
                    Ok(0) => Ok(None), Ok(_) => Ok(Some(byte[0])), Err(_) => Err(()),
                }
            });
        }
        if Self::valid(unit).is_err() { return crate::record_input::ERROR; }
        let Some(file) = self.0.get_mut(&unit) else { return crate::record_input::ERROR; };
        scan(buffer, &mut || {
            let mut byte = [0];
            match file.read(&mut byte) {
                Ok(0) => Ok(None), Ok(_) => Ok(Some(byte[0])), Err(_) => Err(()),
            }
        })
    }
}

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
            "unit" | "newunit" | "file" | "status" | "action" | "access" | "form" | "name" | "opened" | "number" | "exist" | "named" | "iostat" | "position" | "iomsg" | "err"
        ) {
            bail!("{operation}: unsupported I/O control `{key}`");
        }
        if operation=="inquire" && !matches!(key.as_str(),"unit"|"file"|"name"|"opened"|"number"|"exist"|"named"|"action"|"iostat"|"iomsg") {bail!("INQUIRE: unsupported I/O control `{key}`");}
        if operation != "open" && operation!="inquire" && !matches!(key.as_str(), "unit" | "iostat" | "iomsg" | "err") && !(operation=="close" && key=="status") {
            bail!("{} currently supports UNIT, IOSTAT, IOMSG and ERR{}", operation.to_uppercase(), if operation == "close" { " and STATUS" } else { "" });
        }
        if operation=="open" && matches!(key.as_str(),"name"|"opened"|"number"|"exist"|"named"|"err") {bail!("OPEN: unsupported I/O control `{key}`");}
        if controls.insert(key.clone(), value).is_some() {
            bail!("{operation}: duplicate {key} control");
        }
    }
    if operation=="inquire" {return lower_inquiry(controls,symbols,serial);}
    // IOMSG is only defined after an error that IOSTAT reports.
    let message=match controls.remove("iomsg") {
        Some(Expr::Ident(n))=>Some(n),
        Some(_)=>bail!("{} IOMSG requires a scalar CHARACTER variable",operation.to_uppercase()),
        None=>None,
    };
    let may_define=|mut body:Vec<Stmt>|{
        if let Some(n)=&message {body.push(Stmt::CallSub{name:crate::initialized::MAY_DEFINE.into(),args:vec![Expr::Ident(n.clone())]});}
        if body.len()==1 {body.pop().unwrap()} else {Stmt::Block{body}}
    };
    if operation=="open" {
        if let Some(status)=controls.remove("iostat") {
            let Expr::Ident(status)=status else {bail!("OPEN IOSTAT requires a scalar INTEGER variable");};
            let path=controls.get("file").cloned();
            let open=lower_open(controls,symbols,serial,true)?;
            let mut body=open.with_status(&status);
            // Describe a failure as gfortran does, naming the file.
            if let (Some(message),Some(path),Some(Stmt::If{then_body,..}))=(&message,path,body.last_mut()) {
                let text=Expr::Bin(crate::ast::BinOp::Concat,Box::new(Expr::Str("Cannot open file '".into())),
                    Box::new(Expr::Bin(crate::ast::BinOp::Concat,Box::new(Expr::Call("trim".into(),vec![path])),Box::new(Expr::Str("'".into())))));
                then_body.push(Stmt::Assign{name:message.clone(),value:text});
            }
            return Ok(may_define(body));
        }
        return Ok(may_define(vec![lower_open(controls,symbols,serial,false)?.statement()]));
    }
    let unit_expr=controls.remove("unit").context("file operation requires UNIT")?;
    // A failed CLOSE, REWIND, BACKSPACE, ENDFILE or FLUSH stops the program, so
    // when it returns IOSTAT is zero, IOMSG is unchanged and ERR is not taken.
    let iostat=match controls.remove("iostat") {
        Some(Expr::Ident(n))=>Some(n),
        Some(_)=>bail!("{} IOSTAT requires a scalar INTEGER variable",operation.to_uppercase()),
        None=>None,
    };
    controls.remove("err");
    let succeeded=|op:Stmt|match &iostat {
        Some(n)=>Stmt::Block{body:vec![op,Stmt::Assign{name:n.clone(),value:Expr::IntLit("0".into())}]},
        None=>op,
    };
    // FLUSH writes all buffered output, so it accepts any unit.
    if operation=="flush" {
        return Ok(may_define(vec![succeeded(Stmt::CallSub{name:"@file_flush".into(),args:vec![]})]));
    }
    unit(&unit_expr,symbols)?;
    let args=vec![unit_expr];
    let status=match controls.remove("status") {
        None=>"keep".to_owned(),
        Some(Expr::Str(s))=>s.trim_end().to_ascii_lowercase(),
        Some(_)=>bail!("CLOSE STATUS must currently be constant"),
    };
    let name=match status.as_str() {
        "keep"=>format!("@file_{operation}"),
        "delete"=>"@file_close_delete".into(),
        _=>bail!("CLOSE STATUS must be 'keep' or 'delete'"),
    };
    Ok(may_define(vec![succeeded(Stmt::CallSub {
        name,
        args,
    })]))
}

fn lower_open(mut controls:HashMap<String,Expr>,symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>,serial:&mut usize,checked:bool)->Result<Open> {
    if matches!(controls.get("status"),Some(Expr::Str(s)) if s.trim_end().eq_ignore_ascii_case("scratch")) {
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
        return Ok(Open{body:vec![Stmt::CallSub {name:if newunit.is_some(){"@file_newunit"}else{"@file_scratch"}.into(),args:vec![destination]}],result:None,newunit:None});
    }
    {
        let newunit=controls.remove("newunit");
        if newunit.is_some() && controls.contains_key("unit") {bail!("OPEN must not specify both UNIT and NEWUNIT");}
        let destination=newunit.clone().or_else(||controls.remove("unit")).context("OPEN requires UNIT or NEWUNIT")?;
        unit(&destination,symbols)?;
        if newunit.is_some() && !matches!(destination,Expr::Ident(_)) {bail!("NEWUNIT requires an INTEGER variable");}
        // Files are opened in binary mode, so a stream connection for
        // unformatted READ and WRITE needs nothing more.
        for (key,allowed) in [("access",&["sequential","stream"][..]),("form",&["formatted","unformatted"][..]),("position",&["rewind"][..])] {
            if let Some(v)=controls.remove(key) {
                if !matches!(&v,Expr::Str(s) if allowed.iter().any(|a|s.trim_end().eq_ignore_ascii_case(a))) {
                    bail!("OPEN currently supports {key}={}",allowed.iter().map(|a|format!("'{a}'")).collect::<Vec<_>>().join(" or "));
                }
            }
        }
        let status=match controls.remove("status") {Some(Expr::Str(s))=>s.trim_end().to_ascii_lowercase(),None=>"unknown".into(),_=>bail!("OPEN STATUS must currently be constant")};
        let action=match controls.remove("action") {Some(Expr::Str(s))=>s.trim_end().to_ascii_lowercase(),None=>"readwrite".into(),_=>bail!("OPEN ACTION must currently be constant")};
        let mode=match (status.as_str(),action.as_str()) {("old","read")=>0,("replace","write")=>1,("replace","readwrite")=>2,("old","readwrite")=>3,("old","write")=>4,("unknown","readwrite")=>5,("unknown","write")=>6,("unknown","read")=>0,
            // NEW does not yet check that the file is absent.
            ("new","write")=>1,("new","readwrite")=>2,_=>bail!("unsupported OPEN STATUS/ACTION combination")};
        let path=controls.remove("file").context("OPEN requires FILE")?;
        if !matches!(crate::array_calls::scalar_actual_kind(&path,symbols)?,TypeSpec::Character(_)) {bail!("FILE requires scalar CHARACTER");}
        let mut args=vec![destination.clone(),path,Expr::IntLit(mode.to_string()),Expr::IntLit(i64::from(newunit.is_some()).to_string())];
        // With IOSTAT the raw result, a unit or a failure code, goes to a
        // temporary that the caller tests.
        let name=if checked {"@file_open_try"} else {"@file_open_dynamic"};
        if newunit.is_none() || checked {
            *serial+=1;let temp=format!("@open_unit_{serial}");
            args[0]=Expr::Ident(temp.clone());
            if newunit.is_none() {args.push(destination.clone());}
            return Ok(Open{body:vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![temp.clone()]},Stmt::CallSub{name:name.into(),args}],
                result:checked.then_some(temp),newunit:newunit.map(|_|destination)});
        }
        Ok(Open{body:vec![Stmt::CallSub{name:name.into(),args}],result:None,newunit:None})
    }
}

/// A lowered OPEN: its statements, and when IOSTAT is wanted, the temporary
/// holding the raw result and the unit variable NEWUNIT assigns.
struct Open { body: Vec<Stmt>, result: Option<String>, newunit: Option<Expr> }
impl Open {
    fn statement(mut self) -> Stmt {
        if self.body.len() == 1 { self.body.pop().unwrap() } else { Stmt::Block { body: self.body } }
    }
    /// Set IOSTAT from the raw result, a unit or a negative failure code, and
    /// assign NEWUNIT only when the OPEN succeeded.
    fn with_status(mut self, status: &str) -> Vec<Stmt> {
        let Some(result) = self.result.take() else {
            self.body.push(Stmt::Assign { name: status.into(), value: Expr::IntLit("0".into()) });
            return self.body;
        };
        let failed = Expr::Bin(crate::ast::BinOp::Lt, Box::new(Expr::Ident(result.clone())), Box::new(Expr::IntLit("-2000".into())));
        let mut success = vec![Stmt::Assign { name: status.into(), value: Expr::IntLit("0".into()) }];
        if let Some(Expr::Ident(unit)) = self.newunit.take() {
            success.push(Stmt::Assign { name: unit, value: Expr::Ident(result) });
        }
        self.body.push(Stmt::If {
            cond: failed,
            then_body: vec![Stmt::Assign { name: status.into(), value: Expr::IntLit("1".into()) }],
            else_body: Some(success),
        });
        self.body
    }
}

fn lower_inquiry(mut controls:HashMap<String,Expr>,symbols:&HashMap<String,(TypeSpec,Vec<Expr>)>,serial:&mut usize)->Result<Stmt> {
    let unit=controls.remove("unit");let file=controls.remove("file");
    if unit.is_some()==file.is_some() {bail!("INQUIRE requires exactly one of UNIT or FILE");}
    let by_file=file.is_some();let selector=file.or(unit).unwrap();
    let kind=crate::array_calls::scalar_actual_kind(&selector,symbols)
        .context("INQUIRE selector must be a scalar expression")?;
    if if by_file {!matches!(kind,TypeSpec::Character(_))} else {!matches!(kind,TypeSpec::Integer(_))} {
        bail!("INQUIRE {} requires a scalar {} expression",if by_file {"FILE"}else{"UNIT"},if by_file {"CHARACTER"}else{"INTEGER"});
    }
    for (key,value) in &controls {
        let Expr::Ident(n)=value else {bail!("INQUIRE {} requires a writable scalar variable",key.to_ascii_uppercase());};
        let expected=match key.as_str(){"name"|"action"|"iomsg"=>"CHARACTER","opened"|"exist"|"named"=>"LOGICAL",_=>"INTEGER"};
        let valid=symbols.get(n).is_some_and(|(kind,dims)|dims.is_empty() && match expected {
            "CHARACTER"=>matches!(kind,TypeSpec::Character(_)),"LOGICAL"=>*kind==TypeSpec::Logical,
            _=>matches!(kind,TypeSpec::Integer(None|Some(1|2|4|8))),
        });
        if !valid {bail!("INQUIRE {} requires a writable scalar {} variable",key.to_ascii_uppercase(),expected);}
    }
    *serial+=1;
    let keys=["iostat","number","opened","exist","named","name","action"];
    let names:Vec<String>=keys.iter().map(|key|format!("@inquire_{key}_{serial}")).collect();
    let id=|i:usize|Expr::Ident(names[i].clone());let int=|n:i64|Expr::IntLit(n.to_string());
    let name_length=controls.get("name").map(|e|{let Expr::Ident(n)=e else {unreachable!()};let TypeSpec::Character(l)=symbols[n].0 else {unreachable!()};l.unwrap_or(1)}).unwrap_or(0);
    let mut body=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:names[..5].to_vec()},
        Stmt::VarDecl{kind:TypeSpec::Character(Some(name_length)),names:vec![names[5].clone()]},
        Stmt::VarDecl{kind:TypeSpec::Character(Some(9)),names:vec![names[6].clone()]}];
    let mut args:Vec<Expr>=(0..7).map(id).collect();
    args.extend([selector,int(name_length as i64),int(9)]);
    body.push(Stmt::CallSub{name:if by_file {"@file_inquire_file"}else{"@file_inquire_unit"}.into(),args});
    let handled=controls.contains_key("iostat");
    if let Some(Expr::Ident(name))=controls.remove("iostat") {body.push(Stmt::Assign{name,value:id(0)});}
    let mut failure=vec![];
    if let Some(Expr::Ident(name))=controls.remove("iomsg") {
        failure.push(Stmt::Assign{name:name.clone(),value:Expr::Str("INQUIRE failed: filesystem inquiry error".into())});
        for (code,text) in [(1,"INQUIRE failed: invalid unit number"),(2,"INQUIRE failed: invalid file name")] {
            failure.push(Stmt::If{cond:Expr::Bin(crate::ast::BinOp::Eq,Box::new(id(0)),Box::new(int(code))),then_body:vec![Stmt::Assign{name:name.clone(),value:Expr::Str(text.into())}],else_body:None});
        }
        body.push(Stmt::CallSub{name:crate::initialized::MAY_DEFINE.into(),args:vec![Expr::Ident(name)]});
    }
    // IOMSG alone does not suppress error termination.
    if !handled {failure.push(Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("INQUIRE failed".into())]});}
    let mut success=vec![];
    for (i,key) in keys.iter().enumerate().skip(1) {
        if let Some(Expr::Ident(name))=controls.remove(*key) {
            let value=if (2..=4).contains(&i) {Expr::Bin(crate::ast::BinOp::Ne,Box::new(id(i)),Box::new(int(0)))} else {id(i)};
            success.push(Stmt::Assign{name,value});
        }
    }
    body.push(Stmt::If{cond:Expr::Bin(crate::ast::BinOp::Ne,Box::new(id(0)),Box::new(int(0))),then_body:failure,else_body:Some(success)});
    Ok(Stmt::Block{body})
}

#[derive(Default)]
pub(crate) struct Files(HashMap<i64, std::fs::File>, HashMap<i64,String>, HashMap<i64,i64>);
impl Files {
    pub(crate) fn inquire(&self,unit:Option<i64>,path:Option<&str>)->(i64,i64,i64,i64,i64,String,String) {
        if let Some(path)=path {
            let (status,exists)=inquiry::exists(path.as_bytes());
            if status!=0 {return (status,-1,0,0,0,String::new(),String::new());}
            let found=self.1.iter().find(|(unit,name)|self.0.contains_key(unit) && inquiry::same_name(path.as_bytes(),name.as_bytes()));
            let (number,opened,action)=match found {Some((unit,_))=>(*unit,1,*self.2.get(unit).unwrap_or(&1)),None=>(-1,0,0)};
            return (0,number,opened,exists,1,path.trim_end_matches(' ').into(),inquiry::action_text(action).into());
        }
        let unit=unit.unwrap();
        if inquiry::invalid_unit(unit) {return (1,-1,0,0,0,String::new(),String::new());}
        if matches!(unit,0|5|6) && !self.0.contains_key(&unit) {
            return (0,unit,1,1,1,match unit {0=>"stderr",5=>"stdin",_=>"stdout"}.into(),inquiry::action_text(if unit==5 {1}else{2}).into());
        }
        if inquiry::index(unit).is_some_and(|i|self.0.contains_key(&inquiry::unit(i))) {
            let name=self.1.get(&unit).cloned().unwrap_or_default();
            (0,unit,1,1,i64::from(!name.is_empty()),name,inquiry::action_text(*self.2.get(&unit).unwrap_or(&3)).into())
        } else {(0,-1,0,i64::from(unit>=0 && inquiry::index(unit).is_some()),0,String::new(),"UNDEFINED".into())}
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
            self.2.insert(unit,1);
            return Ok(());
        }
        // CLOSE on an unconnected unit is a successful no-op.
        if op == "@file_close" || op == "@file_close_delete" {
            let connected=self.0.remove(&unit).is_some();
            let name=self.1.remove(&unit);
            self.2.remove(&unit);
            if op == "@file_close_delete" && connected {
                if let Some(name)=name.filter(|n|!n.is_empty()) {std::fs::remove_file(name.trim_end()).context("CLOSE could not delete the file")?;}
            }
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
        if op == "@file_endfile" {
            let position = file.stream_position()?;
            file.set_len(position).context("ENDFILE failed")?;
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
        self.2.insert(unit,3);
        Ok(unit)
    }
    pub(crate) fn name(&self, unit:i64)->Result<String> {
        Self::valid(unit)?;
        if !self.0.contains_key(&unit) {bail!("file unit is not connected");}
        Ok(self.1.get(&unit).cloned().unwrap_or_default())
    }
    pub(crate) fn write_record(&mut self, unit:i64, text:&str)->Result<()> {
        Self::valid(unit)?;
        // Unit 0 is preconnected to standard error unless a file is opened on it.
        if unit==0 && !self.0.contains_key(&0) {
            eprintln!("{text}");
            return Ok(());
        }
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

    pub(crate) fn open_dynamic(&mut self,requested:Option<i64>,path:&str,mode:i64)->Result<i64> {
        let path=path.trim_end();if path.is_empty() || path.contains('\0') {bail!("invalid FILE name");}
        let unit=match requested {Some(unit)=>unit,None=>(-1033..=-10).rev().find(|u|!self.0.contains_key(u)).context("no free NEWUNIT file slots")?};
        Self::valid(unit)?;if self.0.contains_key(&unit) {bail!("OPEN on an already connected unit is not yet supported");}
        let mut options=std::fs::OpenOptions::new();
        options.read(matches!(mode,0|2|3|5)).write(mode!=0).create(matches!(mode,1|2|5|6)).truncate(matches!(mode,1|2));
        let file=options.open(path).context("OPEN failed")?;self.0.insert(unit,file);self.1.insert(unit,path.into());self.2.insert(unit,inquiry::action(mode));Ok(unit)
    }
    /// Unformatted stream output: 0, or 1 on an error.
    pub(crate) fn stream_write(&mut self,unit:i64,bytes:&[u8])->i64 {
        match self.0.get_mut(&unit) {Some(file)=>i64::from(file.write_all(bytes).is_err()),None=>1}
    }
    /// Unformatted stream input: 0, -1 at end of file, or 1 on an error.
    pub(crate) fn stream_read(&mut self,unit:i64,buffer:&mut [u8])->i64 {
        let Some(file)=self.0.get_mut(&unit) else {return 1};
        match file.read_exact(buffer) {Ok(())=>0,Err(e) if e.kind()==std::io::ErrorKind::UnexpectedEof=>-1,Err(_)=>1}
    }
    /// Position a stream file at byte POS, counted from 1.
    pub(crate) fn stream_seek(&mut self,unit:i64,position:i64)->i64 {
        let Some(file)=self.0.get_mut(&unit) else {return 1};
        if position<1 {return 1;}
        i64::from(file.seek(SeekFrom::Start((position-1) as u64)).is_err())
    }
    pub(crate) fn input_byte(&mut self,unit:i64)->Result<Option<u8>,()> {
        let mut byte=[0];
        let result=if unit==5 {std::io::stdin().read(&mut byte)} else {
            let file=self.0.get_mut(&unit).ok_or(())?;file.read(&mut byte)
        };
        match result {Ok(0)=>Ok(None),Ok(_)=>Ok(Some(byte[0])),Err(_)=>Err(())}
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

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Emit read-only file operations using the host C library's nonvariadic ABI.
use anyhow::Result;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;

pub(super) fn call(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    name: &str,
    args: &[ir::Value],
    result: ir::Type,
) -> Result<ir::Value> {
    let mut sig = module.make_signature();
    sig.params.extend(
        args.iter()
            .map(|a| AbiParam::new(b.func.dfg.value_type(*a))),
    );
    sig.returns.push(AbiParam::new(result));
    let id = module.declare_function(name, Linkage::Import, &sig)?;
    let function = module.declare_func_in_func(id, b.func);
    let call = b.ins().call(function, args);
    Ok(b.inst_results(call)[0])
}

pub(super) fn require(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    condition: ir::Value,
    message: &str,
) -> Result<()> {
    let success = b.create_block();
    let failure = b.create_block();
    b.ins().brif(condition, success, &[], failure, &[]);
    b.switch_to_block(failure);
    b.seal_block(failure);
    let message = format!("Runtime error: {message}\n");
    let text = super::make_data(module, b, ptr, dc, message.as_bytes())?;
    let stderr = b.ins().iconst(types::I32, 2);
    let length = b.ins().iconst(types::I32, message.len() as i64);
    call(
        module,
        b,
        if cfg!(windows) {
            "_write"
        } else {
            "rfort_write"
        },
        &[stderr, text, length],
        types::I32,
    )?;
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    let id = module.declare_function("exit", Linkage::Import, &sig)?;
    let exit = module.declare_func_in_func(id, b.func);
    let code = b.ins().iconst(types::I32, 1);
    b.ins().call(exit, &[code]);
    b.ins().trap(ir::TrapCode::unwrap_user(1));
    b.switch_to_block(success);
    b.seal_block(success);
    Ok(())
}

fn slot(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    unit: ir::Value,
) -> Result<ir::Value> {
    anyhow::ensure!(
        ptr == types::I64,
        "file I/O currently requires a 64-bit target"
    );
    let (index,range) = unit_index(b,unit);
    let not_stdin = b.ins().icmp_imm(IntCC::NotEqual, unit, 5);
    let not_stdout = b.ins().icmp_imm(IntCC::NotEqual, unit, 6);
    let valid = b.ins().band(range, not_stdin);
    let valid = b.ins().band(valid, not_stdout);
    require(
        module,
        b,
        ptr,
        dc,
        valid,
        "file units must be in 0..1023, excluding preconnected units 5 and 6",
    )?;
    let id = module.declare_data(".rfort.file_units", Linkage::Local, true, false)?;
    let global = module.declare_data_in_func(id, b.func);
    let base = b.ins().global_value(ptr, global);
    let offset = b.ins().imul_imm(index, 8);
    Ok(b.ins().iadd(base, offset))
}
fn unit_index(b: &mut FunctionBuilder, unit:ir::Value)->(ir::Value,ir::Value) {
    let positive=b.ins().icmp_imm(IntCC::UnsignedLessThan,unit,1024);
    let negated=b.ins().ineg(unit);let negative=b.ins().iadd_imm(negated,-10);
    let valid_negative=b.ins().icmp_imm(IntCC::UnsignedLessThan,negative,1024);
    let mapped=b.ins().iadd_imm(negative,1024);
    (b.ins().select(positive,unit,mapped),b.ins().bor(positive,valid_negative))
}
fn table(b: &mut FunctionBuilder,module:&mut ObjectModule,ptr:ir::Type,name:&str)->Result<ir::Value> {
    let id=module.declare_data(name,Linkage::Local,true,false)?;
    let global=module.declare_data_in_func(id,b.func);
    Ok(b.ins().global_value(ptr,global))
}
pub(super) fn inquire(module:&mut ObjectModule,b:&mut FunctionBuilder,ptr:ir::Type,arguments:&[ir::Value])->Result<ir::Value> {
    anyhow::ensure!(ptr==types::I64,"file I/O currently requires a 64-bit target");
    let units=table(b,module,ptr,".rfort.file_units")?;
    let names=table(b,module,ptr,".rfort.file_names")?;
    let actions=table(b,module,ptr,".rfort.file_actions")?;
    let mut args=vec![units,names,actions];args.extend_from_slice(arguments);
    call(module,b,"rfort_file_inquire",&args,types::I64)
}
pub(super) fn newunit(module:&mut ObjectModule,b:&mut FunctionBuilder,ptr:ir::Type,dc:&mut u32)->Result<ir::Value> {
    let slots=table(b,module,ptr,".rfort.file_units")?;
    let unit=call(module,b,"rfort_scratch_newunit",&[slots],types::I64)?;
    let okay=b.ins().icmp_imm(IntCC::NotEqual,unit,0);
    require(module,b,ptr,dc,okay,"scratch OPEN failed or no free NEWUNIT slots")?;
    let (index,_)=unit_index(b,unit);
    let names=table(b,module,ptr,".rfort.file_names")?;
    let offset=b.ins().imul_imm(index,8);let address=b.ins().iadd(names,offset);
    let zero=b.ins().iconst(ptr,0);b.ins().store(MemFlags::new(),zero,address,0);
    let actions=table(b,module,ptr,".rfort.file_actions")?;let address=b.ins().iadd(actions,offset);
    let action=b.ins().iconst(types::I64,3);b.ins().store(MemFlags::new(),action,address,0);
    Ok(unit)
}
/// OPEN a file; unless `checked`, a failure is returned as a negative code.
pub(super) fn open_dynamic(module:&mut ObjectModule,b:&mut FunctionBuilder,ptr:ir::Type,dc:&mut u32,requested:ir::Value,path:ir::Value,length:ir::Value,mode:ir::Value,newunit:ir::Value,append:ir::Value,checked:bool)->Result<ir::Value> {
    let slots=table(b,module,ptr,".rfort.file_units")?;let names=table(b,module,ptr,".rfort.file_names")?;
    let actions=table(b,module,ptr,".rfort.file_actions")?;
    let unit=call(module,b,"rfort_file_open_dynamic",&[slots,names,actions,requested,path,length,mode,newunit,append],types::I64)?;
    if !checked {return Ok(unit);}
    let okay=b.ins().icmp_imm(IntCC::NotEqual,unit,i64::MIN+1);require(module,b,ptr,dc,okay,"OPEN on an already connected unit is not yet supported")?;
    let okay=b.ins().icmp_imm(IntCC::NotEqual,unit,i64::MIN+2);require(module,b,ptr,dc,okay,"file units must be in 0..1023, excluding preconnected units 5 and 6")?;
    let okay=b.ins().icmp_imm(IntCC::NotEqual,unit,i64::MIN);require(module,b,ptr,dc,okay,"OPEN failed")?;Ok(unit)
}
pub(super) fn name(module:&mut ObjectModule,b:&mut FunctionBuilder,ptr:ir::Type,dc:&mut u32,unit:ir::Value)->Result<ir::Value> {
    handle(module,b,ptr,dc,unit)?;
    let (index,_)=unit_index(b,unit);
    let base=table(b,module,ptr,".rfort.file_names")?;
    let offset=b.ins().imul_imm(index,8);let address=b.ins().iadd(base,offset);
    Ok(b.ins().load(ptr,MemFlags::new(),address,0))
}

pub(super) fn handle(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    unit: ir::Value,
) -> Result<ir::Value> {
    let slot = slot(module, b, ptr, dc, unit)?;
    let file = b.ins().load(ptr, MemFlags::new(), slot, 0);
    let connected = b.ins().icmp_imm(IntCC::NotEqual, file, 0);
    require(module, b, ptr, dc, connected, "file unit is not connected")?;
    Ok(file)
}

/// The stream for output to `unit`: unit 0 without a connected file is
/// standard error.
pub(super) fn write_handle(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    unit: ir::Value,
) -> Result<ir::Value> {
    let slot = slot(module, b, ptr, dc, unit)?;
    let file = b.ins().load(ptr, MemFlags::new(), slot, 0);
    let stderr = call(module, b, "rfort_stderr_stream", &[], ptr)?;
    let zero = b.ins().icmp_imm(IntCC::Equal, unit, 0);
    let unconnected = b.ins().icmp_imm(IntCC::Equal, file, 0);
    let error_unit = b.ins().band(zero, unconnected);
    let file = b.ins().select(error_unit, stderr, file);
    let connected = b.ins().icmp_imm(IntCC::NotEqual, file, 0);
    require(module, b, ptr, dc, connected, "file unit is not connected")?;
    Ok(file)
}

// READ with IOSTAT reports connection/range errors without terminating.
// A null handle with status zero denotes preconnected standard input (unit 5).
pub(super) fn read_handle(
    module: &mut ObjectModule, b: &mut FunctionBuilder, ptr: ir::Type,
    unit: ir::Value,
) -> Result<(ir::Value, ir::Value)> {
    anyhow::ensure!(ptr == types::I64, "file I/O currently requires a 64-bit target");
    let done=b.create_block();let file=b.create_block();let lookup=b.create_block();let invalid=b.create_block();
    b.append_block_param(done,ptr);b.append_block_param(done,types::I64);
    let zero=b.ins().iconst(ptr,0);let error=b.ins().iconst(types::I64,1);
    let stdin=b.ins().icmp_imm(IntCC::Equal,unit,5);
    b.ins().brif(stdin,done,&[zero.into(),zero.into()],file,&[]);
    b.switch_to_block(file);b.seal_block(file);
    let (index,range)=unit_index(b,unit);
    let not_stdout=b.ins().icmp_imm(IntCC::NotEqual,unit,6);
    let valid=b.ins().band(range,not_stdout);
    b.ins().brif(valid,lookup,&[],invalid,&[]);
    b.switch_to_block(invalid);b.seal_block(invalid);
    b.ins().jump(done,&[zero.into(),error.into()]);
    b.switch_to_block(lookup);b.seal_block(lookup);
    let id=module.declare_data(".rfort.file_units",Linkage::Local,true,false)?;
    let global=module.declare_data_in_func(id,b.func);
    let base=b.ins().global_value(ptr,global);
    let offset=b.ins().imul_imm(index,8);let address=b.ins().iadd(base,offset);
    let handle=b.ins().load(ptr,MemFlags::new(),address,0);
    let connected=b.ins().icmp_imm(IntCC::NotEqual,handle,0);
    let status=b.ins().select(connected,zero,error);
    b.ins().jump(done,&[handle.into(),status.into()]);
    b.switch_to_block(done);b.seal_block(done);
    Ok((b.block_params(done)[0],b.block_params(done)[1]))
}

fn seek(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    file: ir::Value,
    offset: ir::Value,
) -> Result<()> {
    let start = b.ins().iconst(types::I32, 0);
    let result = call(
        module,
        b,
        if cfg!(windows) { "_fseeki64" } else { "fseeko" },
        &[file, offset, start],
        types::I32,
    )?;
    let okay = b.ins().icmp_imm(IntCC::Equal, result, 0);
    require(module, b, ptr, dc, okay, "file positioning failed")
}

pub(super) fn operation(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    op: &str,
    unit: ir::Value,
    path: Option<&str>,
) -> Result<()> {
    let slot = slot(module, b, ptr, dc, unit)?;
    let file = b.ins().load(ptr, MemFlags::new(), slot, 0);
    if op == "@file_open" || op=="@file_scratch" {
        let closed = b.ins().icmp_imm(IntCC::Equal, file, 0);
        require(
            module,
            b,
            ptr,
            dc,
            closed,
            "OPEN on an already connected unit is not yet supported",
        )?;
        let (file,path)=if op=="@file_scratch" {
            (call(module,b,"tmpfile",&[],ptr)?,b.ins().iconst(ptr,0))
        } else {
            let path = super::make_data(module, b, ptr, dc, path.unwrap().as_bytes())?;
            let mode = super::make_data(module, b, ptr, dc, b"rb")?;
            (call(module, b, "fopen", &[path, mode], ptr)?,path)
        };
        let okay = b.ins().icmp_imm(IntCC::NotEqual, file, 0);
        require(module, b, ptr, dc, okay, "OPEN failed")?;
        b.ins().store(MemFlags::new(), file, slot, 0);
        let (index,_)=unit_index(b,unit);let base=table(b,module,ptr,".rfort.file_names")?;
        let offset=b.ins().imul_imm(index,8);let address=b.ins().iadd(base,offset);
        b.ins().store(MemFlags::new(),path,address,0);
        let actions=table(b,module,ptr,".rfort.file_actions")?;let address=b.ins().iadd(actions,offset);
        let action=b.ins().iconst(types::I64,if op=="@file_scratch" {3}else{1});b.ins().store(MemFlags::new(),action,address,0);
        return Ok(());
    }
    if op == "@file_close" || op == "@file_close_delete" {
        let close = b.create_block();
        let done = b.create_block();
        let connected = b.ins().icmp_imm(IntCC::NotEqual, file, 0);
        b.ins().brif(connected, close, &[], done, &[]);
        b.switch_to_block(close);
        b.seal_block(close);
        let result = call(module, b, "fclose", &[file], types::I32)?;
        let okay = b.ins().icmp_imm(IntCC::Equal, result, 0);
        require(module, b, ptr, dc, okay, "CLOSE failed")?;
        let zero = b.ins().iconst(ptr, 0);
        b.ins().store(MemFlags::new(), zero, slot, 0);
        let (index,_)=unit_index(b,unit);let names=table(b,module,ptr,".rfort.file_names")?;
        let offset=b.ins().imul_imm(index,8);let address=b.ins().iadd(names,offset);let path=b.ins().load(ptr,MemFlags::new(),address,0);
        if op == "@file_close_delete" {
            let result = call(module, b, "rfort_file_delete", &[path], types::I32)?;
            let okay = b.ins().icmp_imm(IntCC::Equal, result, 0);
            require(module, b, ptr, dc, okay, "CLOSE could not delete the file")?;
        }
        call(module,b,"rfort_file_free_name",&[path],types::I32)?;b.ins().store(MemFlags::new(),zero,address,0);
        b.ins().jump(done, &[]);
        b.switch_to_block(done);
        b.seal_block(done);
        return Ok(());
    }
    let connected = b.ins().icmp_imm(IntCC::NotEqual, file, 0);
    require(module, b, ptr, dc, connected, "file unit is not connected")?;
    if op == "@file_rewind" {
        let zero = b.ins().iconst(types::I64, 0);
        return seek(module, b, ptr, dc, file, zero);
    }
    if op == "@file_endfile" {
        let result = call(module, b, "rfort_file_endfile", &[file], types::I32)?;
        let okay = b.ins().icmp_imm(IntCC::Equal, result, 0);
        return require(module, b, ptr, dc, okay, "ENDFILE failed");
    }
    let position = call(
        module,
        b,
        if cfg!(windows) { "_ftelli64" } else { "ftello" },
        &[file],
        types::I64,
    )?;
    let okay = b
        .ins()
        .icmp_imm(IntCC::SignedGreaterThanOrEqual, position, 0);
    require(module, b, ptr, dc, okay, "file positioning failed")?;
    let last = b.create_block();
    let scan = b.create_block();
    let inspect = b.create_block();
    let done = b.create_block();
    b.append_block_param(scan, types::I64);
    b.append_block_param(done, types::I64);
    let zero = b.ins().iconst(types::I64, 0);
    let empty = b.ins().icmp_imm(IntCC::Equal, position, 0);
    b.ins().brif(empty, done, &[zero.into()], last, &[]);
    b.switch_to_block(last);
    b.seal_block(last);
    let previous = b.ins().iadd_imm(position, -1);
    seek(module, b, ptr, dc, file, previous)?;
    let ch = call(module, b, "fgetc", &[file], types::I32)?;
    let okay = b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, ch, 0);
    require(module, b, ptr, dc, okay, "BACKSPACE read failed")?;
    let newline = b.ins().icmp_imm(IntCC::Equal, ch, 10);
    let position = b.ins().select(newline, previous, position);
    b.ins().jump(scan, &[position.into()]);
    b.switch_to_block(scan);
    let position = b.block_params(scan)[0];
    let empty = b.ins().icmp_imm(IntCC::Equal, position, 0);
    b.ins().brif(empty, done, &[position.into()], inspect, &[]);
    b.switch_to_block(inspect);
    b.seal_block(inspect);
    let previous = b.ins().iadd_imm(position, -1);
    seek(module, b, ptr, dc, file, previous)?;
    let ch = call(module, b, "fgetc", &[file], types::I32)?;
    let okay = b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, ch, 0);
    require(module, b, ptr, dc, okay, "BACKSPACE read failed")?;
    let newline = b.ins().icmp_imm(IntCC::Equal, ch, 10);
    b.ins()
        .brif(newline, done, &[position.into()], scan, &[previous.into()]);
    b.seal_block(scan);
    b.switch_to_block(done);
    b.seal_block(done);
    let position = b.block_params(done)[0];
    seek(module, b, ptr, dc, file, position)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Owned component descriptors: [data, initialization mask, length, allocated,
//! extents..., lower bounds...]. All words are 64 bits on every native target.
use super::*;
use cranelift_codegen::ir::{condcodes::IntCC, types, MemFlags};

fn pointer(b: &mut FunctionBuilder, ptr: ir::Type, value: ir::Value) -> ir::Value {
    if ptr == types::I64 {
        value
    } else {
        b.ins().ireduce(ptr, value)
    }
}
fn word(b: &mut FunctionBuilder, value: ir::Value) -> ir::Value {
    if b.func.dfg.value_type(value) == types::I64 {
        value
    } else {
        b.ins().uextend(types::I64, value)
    }
}
fn release(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    value: ir::Value,
) -> Result<()> {
    let mut signature = module.make_signature();
    signature.params.push(ir::AbiParam::new(ptr));
    let id = module.declare_function("free", Linkage::Import, &signature)?;
    let function = module.declare_func_in_func(id, b.func);
    b.ins().call(function, &[value]);
    Ok(())
}
fn free_descriptor(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    handle: ir::Value,
) -> Result<()> {
    let handle = pointer(b, ptr, handle);
    let active = b.ins().icmp_imm(IntCC::NotEqual, handle, 0);
    let dispose = b.create_block();
    let done = b.create_block();
    b.ins().brif(active, dispose, &[], done, &[]);
    b.switch_to_block(dispose);
    b.seal_block(dispose);
    for offset in [0, 8] {
        let data = b.ins().load(types::I64, MemFlags::new(), handle, offset);
        let data = pointer(b, ptr, data);
        release(module, b, ptr, data)?;
    }
    release(module, b, ptr, handle)?;
    b.ins().jump(done, &[]);
    b.switch_to_block(done);
    b.seal_block(done);
    Ok(())
}
fn clone_buffer(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    source: ir::Value,
    length: ir::Value,
) -> Result<ir::Value> {
    let bytes = b.ins().imul_imm(length, 8);
    let bytes = pointer(b, ptr, bytes);
    // malloc(1) makes the empty allocated case independent of malloc(0).
    let empty = b.ins().icmp_imm(IntCC::Equal, bytes, 0);
    let one = b.ins().iconst(ptr, 1);
    let capacity = b.ins().select(empty, one, bytes);
    let data = file_io::call(module, b, "malloc", &[capacity], ptr)?;
    let okay = b.ins().icmp_imm(IntCC::NotEqual, data, 0);
    file_io::require(
        module,
        b,
        ptr,
        dc,
        okay,
        "record component allocation failed",
    )?;
    let copy = b.create_block();
    let done = b.create_block();
    b.ins().brif(empty, done, &[], copy, &[]);
    b.switch_to_block(copy);
    b.seal_block(copy);
    let source = pointer(b, ptr, source);
    file_io::call(module, b, "memcpy", &[data, source, bytes], ptr)?;
    b.ins().jump(done, &[]);
    b.switch_to_block(done);
    b.seal_block(done);
    Ok(data)
}

pub(super) fn emit(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    name: &str,
    args: &[IExpr],
    value: ir::Value,
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
) -> Result<()> {
    let storage = |n: &str| {
        ints.get(n)
            .or_else(|| reals.get(n))
            .or_else(|| bools.get(n))
            .copied()
            .with_context(|| format!("missing record component storage `{n}`"))
    };
    if name == "@heap_record_dispose" {
        return free_descriptor(module, b, ptr, value);
    }
    if name == "@heap_record_drop_array" {
        let IExpr::Ident(array) = &args[0] else {
            bail!("record descriptor array requires storage")
        };
        let address = storage_addr(b, ptr, storage(array)?);
        let loop_head = b.create_block();
        let item = b.create_block();
        let done = b.create_block();
        b.append_block_param(loop_head, types::I64);
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().jump(loop_head, &[zero.into()]);
        b.switch_to_block(loop_head);
        let index = b.block_params(loop_head)[0];
        let more = b.ins().icmp(IntCC::UnsignedLessThan, index, value);
        b.ins().brif(more, item, &[], done, &[]);
        b.switch_to_block(item);
        b.seal_block(item);
        let offset = b.ins().imul_imm(index, 8);
        let offset = pointer(b, ptr, offset);
        let cell = b.ins().iadd(address, offset);
        let handle = b.ins().load(types::I64, MemFlags::new(), cell, 0);
        free_descriptor(module, b, ptr, handle)?;
        let zero = b.ins().iconst(types::I64, 0);
        b.ins().store(MemFlags::new(), zero, cell, 0);
        let next = b.ins().iadd_imm(index, 1);
        b.ins().jump(loop_head, &[next.into()]);
        b.seal_block(loop_head);
        b.switch_to_block(done);
        b.seal_block(done);
        return Ok(());
    }
    let IExpr::Ident(local) = &args[0] else {
        bail!("invalid record component local")
    };
    let IExpr::Ident(mask) = args.last().unwrap() else {
        bail!("missing record component initialization mask")
    };
    let data_cell = heap_cell(b, ptr, storage(local)?)?;
    let mask_cell = heap_cell(b, ptr, storage(mask)?)?;
    let metadata = &args[2..args.len() - 1];
    let mut addresses = vec![];
    for meta in metadata {
        let IExpr::Ident(n) = meta else {
            bail!("record component metadata must designate storage")
        };
        addresses.push((storage_addr(b, ptr, storage(n)?), bools.contains_key(n)));
    }
    if name == "@heap_record_load" {
        for cell in [data_cell, mask_cell] {
            let old = b.ins().load(ptr, MemFlags::new(), cell, 0);
            release(module, b, ptr, old)?;
            let zero = b.ins().iconst(ptr, 0);
            b.ins().store(MemFlags::new(), zero, cell, 0);
        }
        for (i, (address, boolean)) in addresses.iter().enumerate() {
            let default = if i >= 2 + (metadata.len() - 2) / 2 {
                1
            } else {
                0
            };
            let v = b
                .ins()
                .iconst(if *boolean { types::I8 } else { types::I64 }, default);
            b.ins().store(MemFlags::new(), v, *address, 0);
        }
        let handle = pointer(b, ptr, value);
        let active = b.ins().icmp_imm(IntCC::NotEqual, handle, 0);
        let load = b.create_block();
        let done = b.create_block();
        b.ins().brif(active, load, &[], done, &[]);
        b.switch_to_block(load);
        b.seal_block(load);
        for (i, (address, boolean)) in addresses.iter().enumerate() {
            let v = b
                .ins()
                .load(types::I64, MemFlags::new(), handle, ((i + 2) * 8) as i32);
            let v = if *boolean {
                b.ins().ireduce(types::I8, v)
            } else {
                v
            };
            b.ins().store(MemFlags::new(), v, *address, 0);
        }
        // An unallocated component must retain null local data cells. Giving it
        // empty buffers would lose those buffers when ALLOCATE installs storage.
        let allocated = b.ins().load(types::I64, MemFlags::new(), handle, 24);
        let allocated = b.ins().icmp_imm(IntCC::NotEqual, allocated, 0);
        let buffers = b.create_block();
        b.ins().brif(allocated, buffers, &[], done, &[]);
        b.switch_to_block(buffers);
        b.seal_block(buffers);
        let length = b.ins().load(types::I64, MemFlags::new(), handle, 16);
        for (offset, cell) in [(0, data_cell), (8, mask_cell)] {
            let source = b.ins().load(types::I64, MemFlags::new(), handle, offset);
            let data = clone_buffer(module, b, ptr, dc, source, length)?;
            b.ins().store(MemFlags::new(), data, cell, 0);
        }
        b.ins().jump(done, &[]);
        b.switch_to_block(done);
        b.seal_block(done);
    } else {
        let IExpr::Ident(slot) = &args[1] else {
            bail!("record allocation export requires a scalar slot")
        };
        free_descriptor(module, b, ptr, value)?;
        let bytes = b.ins().iconst(ptr, ((metadata.len() + 2) * 8) as i64);
        let handle = file_io::call(module, b, "malloc", &[bytes], ptr)?;
        let okay = b.ins().icmp_imm(IntCC::NotEqual, handle, 0);
        file_io::require(
            module,
            b,
            ptr,
            dc,
            okay,
            "record descriptor allocation failed",
        )?;
        let length = b.ins().load(types::I64, MemFlags::new(), addresses[0].0, 0);
        for (offset, cell) in [(0, data_cell), (8, mask_cell)] {
            let source = b.ins().load(ptr, MemFlags::new(), cell, 0);
            let source = word(b, source);
            let data = clone_buffer(module, b, ptr, dc, source, length)?;
            let data = word(b, data);
            b.ins().store(MemFlags::new(), data, handle, offset);
        }
        for (i, (address, boolean)) in addresses.iter().enumerate() {
            let v = b.ins().load(
                if *boolean { types::I8 } else { types::I64 },
                MemFlags::new(),
                *address,
                0,
            );
            let v = word(b, v);
            b.ins()
                .store(MemFlags::new(), v, handle, ((i + 2) * 8) as i32);
        }
        let handle = word(b, handle);
        let address = storage_addr(b, ptr, storage(slot)?);
        b.ins().store(MemFlags::new(), handle, address, 0);
    }
    Ok(())
}

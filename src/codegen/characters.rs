// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Fixed-length scalar CHARACTER assignment, including overlapping sources.
use super::file_io::call;
use crate::ir::IExpr;
use anyhow::{bail, Context, Result};
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;
use std::collections::HashMap;

type Characters = HashMap<String, (ir::StackSlot, usize)>;
pub(super) struct View {
    pub(super) address: ir::Value,
    pub(super) length: ir::Value,
    capacity: usize,
}

pub(super) fn view(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    chars: &Characters,
    expr: &IExpr,
) -> Result<View> {
    match expr {
        IExpr::Str(text) => Ok(View {
            address: super::make_data(module, b, ptr, dc, text.as_bytes())?,
            length: b.ins().iconst(ptr, text.len() as i64),
            capacity: text.len(),
        }),
        IExpr::Ident(name) => {
            let (slot, length) = chars
                .get(name)
                .context("missing CHARACTER source storage")?;
            Ok(View {
                address: b.ins().stack_addr(ptr, *slot, 0),
                length: b.ins().iconst(ptr, *length as i64),
                capacity: *length,
            })
        }
        IExpr::Bin(crate::ast::BinOp::Concat, left, right) => {
            let left = view(module, b, ptr, dc, chars, left)?;
            let right = view(module, b, ptr, dc, chars, right)?;
            let capacity = left
                .capacity
                .checked_add(right.capacity)
                .filter(|n| *n < i32::MAX as usize)
                .context("CHARACTER concatenation is too large")?;
            let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                capacity.max(1) as u32,
                0,
            ));
            let address = b.ins().stack_addr(ptr, slot, 0);
            call(
                module,
                b,
                "memcpy",
                &[address, left.address, left.length],
                ptr,
            )?;
            let tail = b.ins().iadd(address, left.length);
            call(
                module,
                b,
                "memcpy",
                &[tail, right.address, right.length],
                ptr,
            )?;
            Ok(View {
                address,
                length: b.ins().iadd(left.length, right.length),
                capacity,
            })
        }
        IExpr::Call(name, args) if name.eq_ignore_ascii_case("trim") && args.len() == 1 => {
            let source = view(module, b, ptr, dc, chars, &args[0])?;
            let head = b.create_block();
            let inspect = b.create_block();
            let done = b.create_block();
            b.append_block_param(head, ptr);
            b.append_block_param(done, ptr);
            b.ins().jump(head, &[source.length.into()]);
            b.switch_to_block(head);
            let length = b.block_params(head)[0];
            let empty = b.ins().icmp_imm(IntCC::Equal, length, 0);
            b.ins().brif(empty, done, &[length.into()], inspect, &[]);
            b.switch_to_block(inspect);
            b.seal_block(inspect);
            let previous = b.ins().iadd_imm(length, -1);
            let address = b.ins().iadd(source.address, previous);
            let byte = b.ins().load(types::I8, MemFlags::new(), address, 0);
            let space = b.ins().icmp_imm(IntCC::Equal, byte, 32);
            b.ins()
                .brif(space, head, &[previous.into()], done, &[length.into()]);
            b.seal_block(head);
            b.switch_to_block(done);
            b.seal_block(done);
            Ok(View {
                length: b.block_params(done)[0],
                ..source
            })
        }
        _ => bail!("unsupported CHARACTER assignment expression"),
    }
}

pub(super) fn assign(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    chars: &Characters,
    dest: &str,
    expr: &IExpr,
) -> Result<()> {
    let source = view(module, b, ptr, dc, chars, expr)?;
    let (slot, length) = chars
        .get(dest)
        .context("missing CHARACTER destination storage")?;
    let address = b.ins().stack_addr(ptr, *slot, 0);
    let length_value = b.ins().iconst(ptr, *length as i64);
    let shorter = b
        .ins()
        .icmp(IntCC::UnsignedLessThan, source.length, length_value);
    let count = b.ins().select(shorter, source.length, length_value);
    call(module, b, "memmove", &[address, source.address, count], ptr)?;
    let tail = b.ins().iadd(address, count);
    let padding = b.ins().isub(length_value, count);
    let space = b.ins().iconst(types::I32, 32);
    call(module, b, "memset", &[tail, space, padding], ptr)?;
    let zero = b.ins().iconst(types::I8, 0);
    b.ins()
        .store(MemFlags::new(), zero, address, *length as i32);
    Ok(())
}

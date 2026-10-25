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

// Keep storage lifetime with the character symbol table so expression
// temporaries use the same policy as declarations. Procedure buffers remain
// per invocation; only main-program buffers may use static storage.
pub(super) struct Characters {
    pub(super) main: bool,
    values: HashMap<String, (super::Storage, usize)>,
}
impl Characters {
    pub(super) fn new(main: bool) -> Self { Self { main, values: HashMap::new() } }
}
impl std::ops::Deref for Characters {
    type Target = HashMap<String, (super::Storage, usize)>;
    fn deref(&self) -> &Self::Target { &self.values }
}
impl std::ops::DerefMut for Characters {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.values }
}
type Integers = HashMap<String, super::Storage>;
pub(super) struct View {
    pub(super) address: ir::Value,
    pub(super) length: ir::Value,
    pub(super) capacity: usize,
}

pub(super) fn view(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    chars: &Characters,
    ints: &Integers,
    expr: &IExpr,
) -> Result<View> {
    match expr {
        IExpr::Call(name, args) if name == "repeat" && args.len() == 2 => {
            let IExpr::IntLit(count) = &args[1] else {
                bail!("native REPEAT currently requires a constant INTEGER NCOPIES");
            };
            let count: i64 = count.parse()?;
            if count < 0 { bail!("REPEAT NCOPIES must be nonnegative"); }
            let count = usize::try_from(count).context("REPEAT NCOPIES is too large")?;
            let source = view(module, b, ptr, dc, chars, ints, &args[0])?;
            let capacity = source.capacity.checked_mul(count)
                .filter(|n| *n < i32::MAX as usize)
                .context("REPEAT result is too large")?;
            let storage = super::fixed_storage(b, module, capacity.max(1), chars.main)?;
            let address = super::storage_addr(b, ptr, storage);
            let head = b.create_block();
            let copy = b.create_block();
            let done = b.create_block();
            b.append_block_param(head, ptr);
            let zero = b.ins().iconst(ptr, 0);
            b.ins().jump(head, &[zero.into()]);
            b.switch_to_block(head);
            let index = b.block_params(head)[0];
            let finished = b.ins().icmp_imm(IntCC::UnsignedGreaterThanOrEqual, index, count as i64);
            b.ins().brif(finished, done, &[], copy, &[]);
            b.switch_to_block(copy);
            b.seal_block(copy);
            let offset = b.ins().imul(index, source.length);
            let dest = b.ins().iadd(address, offset);
            call(module, b, "memcpy", &[dest, source.address, source.length], ptr)?;
            let next = b.ins().iadd_imm(index, 1);
            b.ins().jump(head, &[next.into()]);
            b.seal_block(head);
            b.switch_to_block(done);
            b.seal_block(done);
            Ok(View { address, length: b.ins().imul_imm(source.length, count as i64), capacity })
        }
        IExpr::Call(name, args) if name == "@substring" => {
            let [source, IExpr::IntLit(start), IExpr::IntLit(length)] = args.as_slice() else {
                bail!("invalid substring view");
            };
            let source = view(module, b, ptr, dc, chars, ints, source)?;
            let start: usize = start.parse()?;
            let length: usize = length.parse()?;
            if start.checked_add(length).is_none_or(|end| end > source.capacity) {
                bail!("substring view exceeds CHARACTER storage");
            }
            Ok(View {
                address: b.ins().iadd_imm(source.address, start as i64),
                length: b.ins().iconst(ptr, length as i64),
                capacity: length,
            })
        }
        IExpr::Call(name, args) if name == "@dynamic_substring" => {
            let [source, start, length, IExpr::IntLit(capacity)] = args.as_slice() else { bail!("invalid dynamic substring view"); };
            let source = view(module, b, ptr, dc, chars, ints, source)?;
            let capacity: usize = capacity.parse()?;
            if capacity > source.capacity { bail!("dynamic substring exceeds CHARACTER storage capacity"); }
            let mut integer = |e: &IExpr| -> Result<ir::Value> {
                let value = match e {
                    IExpr::IntLit(n) => b.ins().iconst(types::I64, n.parse::<i64>()?),
                    IExpr::Ident(n) => {
                        let slot = ints.get(n).context("missing substring bound storage")?;
                        let address = super::storage_addr(b, ptr, *slot);
                        b.ins().load(types::I64, MemFlags::new(), address, 0)
                    }
                    _ => bail!("dynamic substring bounds must be captured integers"),
                };
                Ok(if ptr == types::I64 { value } else { b.ins().ireduce(ptr, value) })
            };
            let start = integer(start)?;
            let length = integer(length)?;
            Ok(View { address: b.ins().iadd(source.address, start), length, capacity })
        }
        // CHARACTER storage passed by reference: its address and length,
        // and the capacity of the dummy that receives it. A fixed-length
        // dummy takes the leading characters of a longer actual; a strict
        // one, which keeps the actual's length, cannot.
        IExpr::Call(name, args) if name == "@character_at" => {
            let [IExpr::Ident(address), IExpr::Ident(length), IExpr::IntLit(capacity), IExpr::IntLit(strict)] = args.as_slice() else {
                bail!("invalid CHARACTER argument view");
            };
            let mut load = |n: &str| -> Result<ir::Value> {
                let slot = ints.get(n).context("missing CHARACTER argument storage")?;
                let address = super::storage_addr(b, ptr, *slot);
                let value = b.ins().load(types::I64, MemFlags::new(), address, 0);
                Ok(if ptr == types::I64 { value } else { b.ins().ireduce(ptr, value) })
            };
            let address = load(address)?;
            let length = load(length)?;
            let capacity: usize = capacity.parse()?;
            let limit = b.ins().iconst(ptr, capacity as i64);
            let shorter = b.ins().icmp(IntCC::UnsignedLessThanOrEqual, length, limit);
            if strict == "1" {
                super::file_io::require(module, b, ptr, dc, shorter, &format!("CHARACTER actual argument is longer than {capacity} characters"))?;
            }
            let length = b.ins().select(shorter, length, limit);
            Ok(View { address, length, capacity })
        }
        IExpr::Str(text) => Ok(View {
            address: super::make_data(module, b, ptr, dc, text.as_bytes())?,
            length: b.ins().iconst(ptr, text.len() as i64),
            capacity: text.len(),
        }),
        IExpr::Call(name, args) if matches!(name.as_str(), "char" | "achar") && args.len()==1 => {
            let code=match &args[0] {
                IExpr::IntLit(value)=>b.ins().iconst(types::I64,value.parse::<i64>()?),
                IExpr::Ident(code)=>{
                    let slot=ints.get(code).context("missing character code storage")?;
                    let address=super::storage_addr(b,ptr,*slot);
                    b.ins().load(types::I64,MemFlags::new(),address,0)
                },
                _=>bail!("character code must be a captured integer"),
            };
            let byte=b.ins().ireduce(types::I8,code);
            let slot=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,1,0));
            let address=b.ins().stack_addr(ptr,slot,0);
            b.ins().store(MemFlags::new(),byte,address,0);
            Ok(View {address,length:b.ins().iconst(ptr,1),capacity:1})
        }
        IExpr::Ident(name) => {
            let (slot, length) = chars
                .get(name)
                .context("missing CHARACTER source storage")?;
            Ok(View {
                address: super::storage_addr(b, ptr, *slot),
                length: b.ins().iconst(ptr, *length as i64),
                capacity: *length,
            })
        }
        IExpr::Bin(crate::ast::BinOp::Concat, left, right) => {
            let left = view(module, b, ptr, dc, chars, ints, left)?;
            let right = view(module, b, ptr, dc, chars, ints, right)?;
            let capacity = left
                .capacity
                .checked_add(right.capacity)
                .filter(|n| *n < i32::MAX as usize)
                .context("CHARACTER concatenation is too large")?;
            let storage = super::fixed_storage(b, module, capacity.max(1), chars.main)?;
            let address = super::storage_addr(b, ptr, storage);
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
        IExpr::Call(name,args) if matches!(name.as_str(),"adjustl"|"adjustr") && args.len()==1 => {
            let source=view(module,b,ptr,dc,chars,ints,&args[0])?;
            let left=name=="adjustl";
            let head=b.create_block();let inspect=b.create_block();let done=b.create_block();
            b.append_block_param(head,ptr);b.append_block_param(done,ptr);
            let zero=b.ins().iconst(ptr,0);
            b.ins().jump(head,&[if left {zero} else {source.length}.into()]);
            b.switch_to_block(head);
            let offset=b.block_params(head)[0];
            let finished=b.ins().icmp(IntCC::Equal,offset,if left {source.length} else {zero});
            b.ins().brif(finished,done,&[offset.into()],inspect,&[]);
            b.switch_to_block(inspect);b.seal_block(inspect);
            let index=if left {offset} else {b.ins().iadd_imm(offset,-1)};
            let pointer=b.ins().iadd(source.address,index);
            let byte=b.ins().load(types::I8,MemFlags::new(),pointer,0);
            let space=b.ins().icmp_imm(IntCC::Equal,byte,32);
            let next=b.ins().iadd_imm(offset,if left {1} else {-1});
            b.ins().brif(space,head,&[next.into()],done,&[offset.into()]);
            b.seal_block(head);b.switch_to_block(done);b.seal_block(done);
            let offset=b.block_params(done)[0];
            let storage=super::fixed_storage(b,module,source.capacity.max(1),chars.main)?;
            let address=super::storage_addr(b,ptr,storage);
            let (from,to,count,padding,tail)=if left {
                let count=b.ins().isub(source.length,offset);
                let from=b.ins().iadd(source.address,offset);let tail=b.ins().iadd(address,count);
                (from,address,count,offset,tail)
            } else {
                let padding=b.ins().isub(source.length,offset);let to=b.ins().iadd(address,padding);
                (source.address,to,offset,padding,address)
            };
            call(module,b,"memcpy",&[to,from,count],ptr)?;
            let space=b.ins().iconst(types::I32,32);call(module,b,"memset",&[tail,space,padding],ptr)?;
            Ok(View {address,length:source.length,capacity:source.capacity})
        }
        IExpr::Call(name, args) if name.eq_ignore_ascii_case("trim") && args.len() == 1 => {
            let source = view(module, b, ptr, dc, chars, ints, &args[0])?;
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
    ints: &Integers,
    dest: &str,
    expr: &IExpr,
) -> Result<()> {
    let source = view(module, b, ptr, dc, chars, ints, expr)?;
    let (slot, length) = chars
        .get(dest)
        .context("missing CHARACTER destination storage")?;
    let address = super::storage_addr(b, ptr, *slot);
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

pub(super) fn terminated(module: &mut ObjectModule, b: &mut FunctionBuilder, ptr: ir::Type, source: View, main: bool) -> Result<ir::Value> {
    let size = source.capacity.checked_add(1).and_then(|n| u32::try_from(n).ok()).context("CHARACTER output is too large")?;
    let storage = super::fixed_storage(b, module, size as usize, main)?;
    let address = super::storage_addr(b, ptr, storage);
    call(module, b, "memcpy", &[address, source.address, source.length], ptr)?;
    let end = b.ins().iadd(address, source.length);
    let zero = b.ins().iconst(types::I8, 0);
    b.ins().store(MemFlags::new(), zero, end, 0);
    Ok(address)
}

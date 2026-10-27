// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Scalar CHARACTER views, owned runtime buffers and overlapping assignment.
use super::file_io::call;
use crate::ir::IExpr;
use anyhow::{bail, Context, Result};
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_object::ObjectModule;
use cranelift_module::{Linkage, Module};
use cranelift_codegen::cursor::{Cursor, FuncCursor};
use std::cell::RefCell;
use std::collections::HashMap;

// Keep owned buffers with the current invocation. Expression sites can be
// reevaluated in loops without accumulating allocations or sharing recursion state.
pub(super) struct Characters {
    values: HashMap<String, (super::Storage, usize)>,
    temporaries: RefCell<Vec<ir::StackSlot>>,
    heap_lengths: HashMap<String, ir::StackSlot>,
}
impl Characters {
    pub(super) fn new() -> Self { Self { values: HashMap::new(), temporaries: RefCell::new(Vec::new()), heap_lengths: HashMap::new() } }
}
// Each expression site owns a pointer cell in the current invocation. Reusing
// that cell bounds memory in loops; entry initialization and cleanup on every
// return also cover conditional evaluation and recursive invocations.
impl Characters {
    fn buffer(&self, module: &mut ObjectModule, b: &mut FunctionBuilder,
              ptr: ir::Type, dc: &mut u32, length: ir::Value) -> Result<ir::Value> {
        let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, 8, 3));
        self.temporaries.borrow_mut().push(slot);
        let cell = b.ins().stack_addr(ptr, slot, 0);
        let previous = b.ins().load(ptr, MemFlags::new(), cell, 0);
        let zero = b.ins().iconst(ptr, 0);
        let empty = b.ins().icmp(IntCC::Equal, length, zero);
        let one = b.ins().iconst(ptr, 1);
        let size = b.ins().select(empty, one, length);
        let address = call(module, b, "realloc", &[previous, size], ptr)?;
        let valid = b.ins().icmp_imm(IntCC::NotEqual, address, 0);
        super::file_io::require(module, b, ptr, dc, valid, "CHARACTER allocation failed")?;
        b.ins().store(MemFlags::new(), address, cell, 0);
        Ok(address)
    }

    pub(super) fn bind(&mut self, b: &mut FunctionBuilder, ptr: ir::Type, name: &str,
                       ints: &mut Integers) {
        let cell = b.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, 8, 3));
        self.temporaries.borrow_mut().push(cell);
        let length = b.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot, 8, 3));
        let zero = b.ins().iconst(ptr, 0);
        b.ins().stack_store(zero, length, 0);
        self.heap_lengths.insert(name.into(), length);
        self.values.insert(name.into(), (super::Storage::HeapStack(cell), crate::substrings::MAX_EXPRESSION_LENGTH));
        ints.insert(format!("@character_pointer:{name}"), super::Storage::Stack(cell));
    }

    pub(super) fn clear(&self, module: &mut ObjectModule, b: &mut FunctionBuilder,
                        ptr: ir::Type, name: &str) -> Result<()> {
        let (super::Storage::HeapStack(cell), _) = self.values[name] else {bail!("invalid heap CHARACTER storage");};
        let previous = b.ins().stack_load(ptr, cell, 0);
        free(module, b, ptr, previous)?;
        let zero = b.ins().iconst(ptr, 0);
        b.ins().stack_store(zero, cell, 0);
        b.ins().stack_store(zero, self.heap_lengths[name], 0);
        Ok(())
    }

    pub(super) fn finish(&self, module: &mut ObjectModule, function: &mut ir::Function,
                         ptr: ir::Type) -> Result<()> {
        let slots = self.temporaries.borrow();
        if slots.is_empty() { return Ok(()); }
        let mut signature = module.make_signature();
        signature.params.push(ir::AbiParam::new(ptr));
        let id = module.declare_function("free", Linkage::Import, &signature)?;
        let free = module.declare_func_in_func(id, function);
        let entry = function.layout.entry_block().context("missing CHARACTER entry block")?;
        let returns: Vec<_> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
            .filter(|inst| function.dfg.insts[*inst].opcode() == ir::Opcode::Return).collect();
        let mut cursor = FuncCursor::new(function);
        cursor.goto_first_inst(entry);
        let zero = cursor.ins().iconst(types::I64, 0);
        for slot in slots.iter() {
            cursor.ins().stack_store(zero, *slot, 0);
        }
        for ret in returns {
            cursor.goto_inst(ret);
            for slot in slots.iter() {
                let address = cursor.ins().stack_load(ptr, *slot, 0);
                cursor.ins().call(free, &[address]);
            }
        }
        Ok(())
    }
}

impl std::ops::Deref for Characters {
    type Target = HashMap<String, (super::Storage, usize)>;
    fn deref(&self) -> &Self::Target { &self.values }
}
impl std::ops::DerefMut for Characters {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.values }
}
fn free(module: &mut ObjectModule, b: &mut FunctionBuilder, ptr: ir::Type, value: ir::Value) -> Result<()> {
    let mut signature=module.make_signature();
    signature.params.push(ir::AbiParam::new(ptr));
    let id=module.declare_function("free",Linkage::Import,&signature)?;
    let function=module.declare_func_in_func(id,b.func);
    b.ins().call(function,&[value]);
    Ok(())
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
            let source = view(module, b, ptr, dc, chars, ints, &args[0])?;
            let (count, capacity) = if let IExpr::IntLit(count) = &args[1] {
                let count: i64 = count.parse()?;
                if count < 0 { bail!("REPEAT NCOPIES must be nonnegative"); }
                let capacity=source.capacity.saturating_mul(usize::try_from(count)?).min(crate::substrings::MAX_EXPRESSION_LENGTH);
                (b.ins().iconst(ptr,count),capacity)
            } else {
                let IExpr::Ident(count)=&args[1] else {bail!("runtime REPEAT NCOPIES must be captured in INTEGER storage");};
                let slot=ints.get(count).context("missing REPEAT count storage")?;
                let address=super::storage_addr(b,ptr,*slot);
                let count=b.ins().load(types::I64,MemFlags::new(),address,0);
                let count=if ptr==types::I64 {count} else {b.ins().ireduce(ptr,count)};
                let nonnegative=b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual,count,0);
                super::file_io::require(module,b,ptr,dc,nonnegative,"REPEAT NCOPIES must be nonnegative")?;
                (count,crate::substrings::MAX_EXPRESSION_LENGTH)
            };
            // A constant count can still multiply a runtime source length.
            let empty=b.ins().icmp_imm(IntCC::Equal,source.length,0);
            let one=b.ins().iconst(ptr,1);
            let divisor=b.ins().select(empty,one,source.length);
            let limit=b.ins().iconst(ptr,crate::substrings::MAX_EXPRESSION_LENGTH as i64);
            let limit=b.ins().udiv(limit,divisor);
            let bounded=b.ins().icmp(IntCC::UnsignedLessThanOrEqual,count,limit);
            let valid=b.ins().bor(empty,bounded);
            super::file_io::require(module,b,ptr,dc,valid,"REPEAT result is too large")?;
            let zero=b.ins().iconst(ptr,0);
            let count=b.ins().select(empty,zero,count);
            let length = b.ins().imul(source.length, count);
            let address = chars.buffer(module, b, ptr, dc, length)?;
            let head = b.create_block();
            let copy = b.create_block();
            let done = b.create_block();
            b.append_block_param(head, ptr);
            let zero = b.ins().iconst(ptr, 0);
            b.ins().jump(head, &[zero.into()]);
            b.switch_to_block(head);
            let index = b.block_params(head)[0];
            let finished = b.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, count);
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
            Ok(View { address, length, capacity })
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
                length: if let Some(slot)=chars.heap_lengths.get(name) {
                    b.ins().stack_load(ptr, *slot, 0)
                } else {b.ins().iconst(ptr, *length as i64)},
                capacity: *length,
            })
        }
        IExpr::Bin(crate::ast::BinOp::Concat, left, right) => {
            let left = view(module, b, ptr, dc, chars, ints, left)?;
            let right = view(module, b, ptr, dc, chars, ints, right)?;
            let capacity = left.capacity.saturating_add(right.capacity)
                .min(crate::substrings::MAX_EXPRESSION_LENGTH);
            let length = b.ins().iadd(left.length, right.length);
            let valid = b.ins().icmp_imm(IntCC::UnsignedLessThanOrEqual,
                length, crate::substrings::MAX_EXPRESSION_LENGTH as i64);
            super::file_io::require(module, b, ptr, dc, valid, "CHARACTER concatenation is too large")?;
            let address = chars.buffer(module, b, ptr, dc, length)?;
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
                length,
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
            let address=chars.buffer(module,b,ptr,dc,source.length)?;
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
    requested_length: Option<ir::Value>,
) -> Result<()> {
    let source = view(module, b, ptr, dc, chars, ints, expr)?;
    if let Some(length_slot)=chars.heap_lengths.get(dest) {
        let length=requested_length.unwrap_or(source.length);
        let size=b.ins().iadd_imm(length,1);
        let address=call(module,b,"malloc",&[size],ptr)?;
        let valid=b.ins().icmp_imm(IntCC::NotEqual,address,0);
        super::file_io::require(module,b,ptr,dc,valid,"CHARACTER allocation failed")?;
        let shorter=b.ins().icmp(IntCC::UnsignedLessThan,source.length,length);
        let count=b.ins().select(shorter,source.length,length);
        call(module,b,"memcpy",&[address,source.address,count],ptr)?;
        let tail=b.ins().iadd(address,count);
        let padding=b.ins().isub(length,count);
        let space=b.ins().iconst(types::I32,32);
        call(module,b,"memset",&[tail,space,padding],ptr)?;
        let end=b.ins().iadd(address,length);
        let zero=b.ins().iconst(types::I8,0);b.ins().store(MemFlags::new(),zero,end,0);
        let (super::Storage::HeapStack(cell),_)=chars.values[dest] else {bail!("invalid heap CHARACTER assignment");};
        let previous=b.ins().stack_load(ptr,cell,0);
        free(module,b,ptr,previous)?;
        b.ins().stack_store(address,cell,0);
        b.ins().stack_store(length,*length_slot,0);
        return Ok(());
    }
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

pub(super) fn terminated(module: &mut ObjectModule, b: &mut FunctionBuilder, ptr: ir::Type,
                         dc: &mut u32, source: View, chars: &Characters) -> Result<ir::Value> {
    let size = b.ins().iadd_imm(source.length, 1);
    let address = chars.buffer(module, b, ptr, dc, size)?;
    call(module, b, "memcpy", &[address, source.address, source.length], ptr)?;
    let end = b.ins().iadd(address, source.length);
    let zero = b.ins().iconst(types::I8, 0);
    b.ins().store(MemFlags::new(), zero, end, 0);
    Ok(address)
}

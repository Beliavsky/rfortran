// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Convert numeric C storage to/from the private 64-bit rfort ABI.
use super::*;
use crate::c_interop::Signature;
use cranelift_codegen::ir::types;
fn c_type(k: &TypeSpec) -> Result<ir::Type> {
    Ok(match k {
        TypeSpec::Integer(None | Some(4)) => types::I32,
        TypeSpec::Integer(Some(1)) => types::I8,
        TypeSpec::Integer(Some(2)) => types::I16,
        TypeSpec::Integer(Some(8)) => types::I64,
        TypeSpec::Real => types::F32,
        TypeSpec::DoublePrecision => types::F64,
        _ => bail!("unsupported C ABI type"),
    })
}
fn wide(k: &TypeSpec) -> ir::Type {
    if matches!(k, TypeSpec::Real | TypeSpec::DoublePrecision) {
        types::F64
    } else {
        types::I64
    }
}
fn widen(b: &mut FunctionBuilder, v: ir::Value, k: &TypeSpec) -> ir::Value {
    let t = b.func.dfg.value_type(v);
    let dest = wide(k);
    if t == dest {
        v
    } else if dest == types::F64 {
        b.ins().fpromote(dest, v)
    } else {
        b.ins().sextend(dest, v)
    }
}
fn narrow(b: &mut FunctionBuilder, v: ir::Value, k: &TypeSpec) -> Result<ir::Value> {
    let dest = c_type(k)?;
    let t = b.func.dfg.value_type(v);
    Ok(if t == dest {
        v
    } else if dest == types::F32 {
        b.ins().fdemote(dest, v)
    } else {
        b.ins().ireduce(dest, v)
    })
}
fn raw_kind(tag: &str) -> TypeSpec {
    match tag {
        "i1" => TypeSpec::Integer(Some(1)),
        "i2" => TypeSpec::Integer(Some(2)),
        "i4" => TypeSpec::Integer(Some(4)),
        "i8" => TypeSpec::Integer(Some(8)),
        "r4" => TypeSpec::Real,
        "r8" => TypeSpec::DoublePrecision,
        _ => unreachable!("validated C array element type"),
    }
}
pub(super) fn raw_load(
    b: &mut FunctionBuilder,
    base: ir::Value,
    index: ir::Value,
    tag: &str,
) -> CgVal {
    let kind = raw_kind(tag);
    let t = c_type(&kind).unwrap();
    let at = element(b, base, index, 1);
    let v = b.ins().load(t, ir::MemFlags::new(), at, 0);
    let v = widen(b, v, &kind);
    if wide(&kind) == types::F64 {
        CgVal::F64(v)
    } else {
        CgVal::I64(v)
    }
}
pub(super) fn raw_store(
    b: &mut FunctionBuilder,
    base: ir::Value,
    index: ir::Value,
    value: CgVal,
    tag: &str,
) -> Result<()> {
    let kind = raw_kind(tag);
    let at = element(b, base, index, 1);
    let v = if wide(&kind) == types::F64 {
        to_f64(b, value)
    } else {
        to_i64(b, value)
    };
    let v = narrow(b, v, &kind)?;
    b.ins().store(ir::MemFlags::new(), v, at, 0);
    Ok(())
}
fn descriptor_type(k:&TypeSpec)->Result<i64> {
    Ok((if matches!(k,TypeSpec::Real|TypeSpec::DoublePrecision) {2}else{1})
        + (i64::from(c_type(k)?.bytes())<<8))
}
fn signature(m: &ObjectModule, s: &Signature, ptr: ir::Type) -> Result<ir::Signature> {
    let mut sig = m.make_signature();
    for a in &s.args {
        sig.params.push(ir::AbiParam::new(if a.value {
            c_type(&a.kind)?
        } else {
            ptr
        }));
    }
    if let Some(k) = &s.result {
        sig.returns.push(ir::AbiParam::new(c_type(k)?));
    }
    Ok(sig)
}
fn address(b: &mut FunctionBuilder, ptr: ir::Type) -> ir::Value {
    let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        8,
        3,
    ));
    b.ins().stack_addr(ptr, slot, 0)
}
fn as_word(b: &mut FunctionBuilder, v: ir::Value) -> ir::Value {
    if b.func.dfg.value_type(v) == types::I64 {
        v
    } else {
        b.ins().uextend(types::I64, v)
    }
}
fn allocate(
    m: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    count: ir::Value,
    width: i64,
) -> Result<ir::Value> {
    let safe = b.ins().icmp_imm(
        ir::condcodes::IntCC::UnsignedLessThanOrEqual,
        count,
        i64::MAX / width,
    );
    file_io::require(m, b, ptr, dc, safe, "C array size overflow")?;
    let bytes = b.ins().imul_imm(count, width);
    let empty = b.ins().icmp_imm(ir::condcodes::IntCC::Equal, bytes, 0);
    let one = b.ins().iconst(ptr, 1);
    let bytes = b.ins().select(empty, one, bytes);
    let data = file_io::call(m, b, "malloc", &[bytes], ptr)?;
    let okay = b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual, data, 0);
    file_io::require(m, b, ptr, dc, okay, "C array allocation failed")?;
    Ok(data)
}
fn dispose(
    m: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    data: ir::Value,
) -> Result<()> {
    let mut sig = m.make_signature();
    sig.params.push(ir::AbiParam::new(ptr));
    let id = m.declare_function("free", Linkage::Import, &sig)?;
    let f = m.declare_func_in_func(id, b.func);
    b.ins().call(f, &[data]);
    Ok(())
}
fn elements(
    b: &mut FunctionBuilder,
    count: ir::Value,
    mut action: impl FnMut(&mut FunctionBuilder, ir::Value) -> Result<()>,
) -> Result<()> {
    let head = b.create_block();
    let body = b.create_block();
    let done = b.create_block();
    b.append_block_param(head, types::I64);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().jump(head, &[zero.into()]);
    b.switch_to_block(head);
    let i = b.block_params(head)[0];
    let more = b.ins().icmp(ir::condcodes::IntCC::SignedLessThan, i, count);
    b.ins().brif(more, body, &[], done, &[]);
    b.switch_to_block(body);
    b.seal_block(body);
    action(b, i)?;
    let next = b.ins().iadd_imm(i, 1);
    b.ins().jump(head, &[next.into()]);
    b.seal_block(head);
    b.switch_to_block(done);
    b.seal_block(done);
    Ok(())
}
fn element(b: &mut FunctionBuilder, base: ir::Value, i: ir::Value, width: i64) -> ir::Value {
    let offset = b.ins().imul_imm(i, width);
    b.ins().iadd(base, offset)
}
fn copy_array(
    m: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    dc: &mut u32,
    count: ir::Value,
    c: ir::Value,
    local: ir::Value,
    flags: Option<ir::Value>,
    kind: &TypeSpec,
    to_c: bool,
) -> Result<()> {
    let width = i64::from(c_type(kind)?.bytes());
    elements(b, count, |b, i| {
        let c = element(b, c, i, width);
        let local = element(b, local, i, 8);
        let flag = flags.map(|f| element(b, f, i, 8));
        if to_c {
            if let Some(f) = flag {
                let ready = b.ins().load(types::I64, ir::MemFlags::new(), f, 0);
                let ready = b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual, ready, 0);
                file_io::require(
                    m,
                    b,
                    ptr,
                    dc,
                    ready,
                    "read of uninitialized C array argument element",
                )?;
            }
            let v = b.ins().load(wide(kind), ir::MemFlags::new(), local, 0);
            let v = narrow(b, v, kind)?;
            b.ins().store(ir::MemFlags::new(), v, c, 0);
        } else {
            let v = b.ins().load(c_type(kind)?, ir::MemFlags::new(), c, 0);
            let v = widen(b, v, kind);
            b.ins().store(ir::MemFlags::new(), v, local, 0);
            if let Some(f) = flag {
                let one = b.ins().iconst(types::I64, 1);
                b.ins().store(ir::MemFlags::new(), one, f, 0);
            }
        }
        Ok(())
    })
}

fn bound(b: &mut FunctionBuilder, e: &crate::ast::Expr, values: &[ir::Value]) -> Result<ir::Value> {
    use crate::ast::{BinOp, Expr, UnOp};
    Ok(match e {
        Expr::IntLit(n) => b.ins().iconst(types::I64, n.parse::<i64>()?),
        Expr::Ident(n) => values[n.parse::<usize>()?],
        Expr::Un(UnOp::Neg, a) => {
            let a = bound(b, a, values)?;
            b.ins().ineg(a)
        }
        Expr::Bin(op, a, c) => {
            let a = bound(b, a, values)?;
            let c = bound(b, c, values)?;
            match op {
                BinOp::Add => b.ins().iadd(a, c),
                BinOp::Sub => b.ins().isub(a, c),
                BinOp::Mul => b.ins().imul(a, c),
                BinOp::Div => b.ins().sdiv(a, c),
                _ => bail!("unsupported C array bound operator"),
            }
        }
        _ => bail!("unsupported C array bound"),
    })
}
pub(super) fn invoke(
    m: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    args: &[IExpr],
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
    dc: &mut u32,
) -> Result<()> {
    let IExpr::Str(encoded) = &args[0] else {
        bail!("missing C invocation metadata")
    };
    let s: Signature = serde_json::from_str(encoded)?;
    let storage = |e: &IExpr| -> Result<Storage> {
        let IExpr::Ident(n) = e else {
            bail!("C adapter requires scalar temporary")
        };
        ints.get(n)
            .or_else(|| reals.get(n))
            .or_else(|| bools.get(n))
            .copied()
            .context("missing C adapter temporary")
    };
    let mut values = vec![];
    let mut outputs = vec![];
    let mut arrays = vec![];
    let mut descriptors=vec![];
    let mut actuals = args[1..].iter();
    for arg in &s.args {
        let actual = actuals.next().context("missing C argument")?;
        if !arg.dims.is_empty() {
            let mut fields = vec![];
            let descriptor=crate::c_interop::assumed_shape(&arg.dims);
            let extra=if descriptor {3+2*arg.dims.len()}else{3};
            for e in std::iter::once(actual).chain(actuals.by_ref().take(extra)) {
                let address = storage_addr(b, ptr, storage(e)?);
                fields.push(b.ins().load(types::I64, ir::MemFlags::new(), address, 0));
            }
            if descriptor {
                let rank=arg.dims.len();
                let slot=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,(2*rank*8) as u32,3));
                for (i,v) in fields[4..].iter().enumerate() {b.ins().stack_store(*v,slot,(i*8) as i32);}
                let shape=b.ins().stack_addr(ptr,slot,0);
                let rank=b.ins().iconst(types::I64,rank as i64);
                let typ=b.ins().iconst(types::I64,descriptor_type(&arg.kind)?);
                let input=b.ins().iconst(types::I64,i64::from(arg.input));
                let d=file_io::call(m,b,"rfort_c_prepare",&[fields[0],fields[1],rank,typ,fields[3],input,shape],ptr)?;
                values.push(d);descriptors.push((d,arg.output));continue;
            }
            let [data, flags, _label, count] = fields[..] else {
                bail!("invalid C array descriptor")
            };
            let direct = b.create_block();
            let convert = b.create_block();
            let done = b.create_block();
            b.append_block_param(done, ptr);
            let raw = b.ins().icmp_imm(ir::condcodes::IntCC::Equal, count, -1);
            b.ins().brif(raw, direct, &[], convert, &[]);
            b.switch_to_block(direct);
            b.seal_block(direct);
            b.ins().jump(done, &[data.into()]);
            b.switch_to_block(convert);
            b.seal_block(convert);
            let c = allocate(m, b, ptr, dc, count, i64::from(c_type(&arg.kind)?.bytes()))?;
            if arg.input {
                copy_array(m, b, ptr, dc, count, c, data, Some(flags), &arg.kind, true)?;
            }
            b.ins().jump(done, &[c.into()]);
            b.switch_to_block(done);
            b.seal_block(done);
            let c = b.block_params(done)[0];
            values.push(c);
            arrays.push((c, data, flags, count, arg));
            continue;
        }
        let local = storage_addr(b, ptr, storage(actual)?);
        if arg.value {
            let value = b.ins().load(wide(&arg.kind), ir::MemFlags::new(), local, 0);
            values.push(narrow(b, value, &arg.kind)?);
        } else {
            let cell = address(b, ptr);
            if arg.input {
                let value = b.ins().load(wide(&arg.kind), ir::MemFlags::new(), local, 0);
                let value = narrow(b, value, &arg.kind)?;
                b.ins().store(ir::MemFlags::new(), value, cell, 0);
            }
            values.push(cell);
            if arg.output {
                outputs.push((cell, local, arg.kind.clone()));
            }
        }
    }
    let sig = signature(m, &s, ptr)?;
    let id = m.declare_function(&s.symbol, Linkage::Import, &sig)?;
    let function = m.declare_func_in_func(id, b.func);
    let call = b.ins().call(function, &values);
    if let Some(k) = &s.result {
        let value = b.inst_results(call)[0];
        let value = widen(b, value, k);
        let local = storage_addr(b, ptr, storage(args.last().unwrap())?);
        b.ins().store(ir::MemFlags::new(), value, local, 0);
    }
    for (d,output) in descriptors {
        let output=b.ins().iconst(types::I64,i64::from(output));
        file_io::call(m,b,"rfort_c_release",&[d,output],types::I32)?;
    }
    for (c, data, flags, count, arg) in arrays {
        let release = b.create_block();
        let done = b.create_block();
        let owned = b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual, count, -1);
        b.ins().brif(owned, release, &[], done, &[]);
        b.switch_to_block(release);
        b.seal_block(release);
        if arg.output {
            copy_array(m, b, ptr, dc, count, c, data, Some(flags), &arg.kind, false)?;
        }
        dispose(m, b, ptr, c)?;
        b.ins().jump(done, &[]);
        b.switch_to_block(done);
        b.seal_block(done);
    }
    for (cell, local, k) in outputs {
        let value = b.ins().load(c_type(&k)?, ir::MemFlags::new(), cell, 0);
        let value = widen(b, value, &k);
        b.ins().store(ir::MemFlags::new(), value, local, 0);
    }
    Ok(())
}
pub(super) fn exports(
    m: &mut ObjectModule,
    module: &crate::ir::Module,
    ptr: ir::Type,
    ids: &HashMap<String, cranelift_module::FuncId>,
    owners: Option<&HashSet<String>>,
) -> Result<()> {
    let mut dc = 0u32;
    for f in &module.funcs {
        let Some(encoded) = f.body.iter().find_map(|s| match s {
            IStmt::Call(n, a) if n == "@c_export" => match a.first() {
                Some(IExpr::Str(s)) => Some(s),
                _ => None,
            },
            _ => None,
        }) else {
            continue;
        };
        if let Some(module) = f
            .name
            .strip_prefix("rfort$@module$")
            .and_then(|n| n.split('$').next())
        {
            if owners.is_some_and(|owners| !owners.contains(module)) {
                continue;
            }
        }
        let s: Signature = serde_json::from_str(encoded)?;
        let sig = signature(m, &s, ptr)?;
        let id = m.declare_function(&s.symbol, Linkage::Export, &sig)?;
        let mut ctx = m.make_context();
        ctx.func.signature = sig;
        let mut frontend = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut frontend);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);
            let passed = b.block_params(entry).to_vec();
            let mut actuals = vec![];
            let mut outputs = vec![];
            let label = make_data(m, &mut b, ptr, &mut dc, b"C argument\0")?;
            let label = as_word(&mut b, label);
            let one = b.ins().iconst(types::I64, 1);
            let mut bounds = vec![];
            for (arg, value) in s.args.iter().zip(&passed) {
                let v =
                    if matches!(arg.kind, TypeSpec::Integer(_)) && arg.dims.is_empty() && arg.input
                    {
                        let v = if arg.value {
                            *value
                        } else {
                            b.ins()
                                .load(c_type(&arg.kind)?, ir::MemFlags::new(), *value, 0)
                        };
                        widen(&mut b, v, &arg.kind)
                    } else {
                        b.ins().iconst(types::I64, 0)
                    };
                bounds.push(v);
            }
            let mut arrays = vec![];
            for (arg, value) in s.args.iter().zip(passed) {
                if crate::c_interop::assumed_shape(&arg.dims) {
                    let typ=b.ins().iconst(types::I64,descriptor_type(&arg.kind)?);
                    let rank=b.ins().iconst(types::I64,arg.dims.len() as i64);
                    file_io::call(m,&mut b,"rfort_c_validate",&[value,typ,rank],types::I32)?;
                    let base=b.ins().load(ptr,ir::MemFlags::new(),value,0);
                    let zero=b.ins().iconst(types::I64,0);
                    actuals.extend([base,value,label,zero]);continue;
                }
                if crate::c_interop::assumed_size(&arg.dims) {
                    let zero = b.ins().iconst(types::I64, 0);
                    actuals.extend([value, zero, label, zero]);
                    continue;
                }
                if !arg.dims.is_empty() {
                    let mut count = one;
                    for d in &arg.dims {
                        let (lo, hi) = match d {
                            crate::ast::Expr::Call(n, a) if n == "@declared_bounds" => (
                                bound(&mut b, &a[0], &bounds)?,
                                bound(&mut b, &a[1], &bounds)?,
                            ),
                            _ => (one, bound(&mut b, d, &bounds)?),
                        };
                        let empty = b.ins().icmp(ir::condcodes::IntCC::SignedLessThan, hi, lo);
                        let (extent, overflow) = b.ins().ssub_overflow(hi, lo);
                        let (extent, more_overflow) = b.ins().sadd_overflow(extent, one);
                        let overflow = b.ins().bor(overflow, more_overflow);
                        let okay = b.ins().bnot(overflow);
                        let okay = b.ins().bor(empty, okay);
                        file_io::require(m, &mut b, ptr, &mut dc, okay, "C array extent overflow")?;
                        let zero = b.ins().iconst(types::I64, 0);
                        let extent = b.ins().select(empty, zero, extent);
                        let (total, overflow) = b.ins().smul_overflow(count, extent);
                        let okay = b.ins().bnot(overflow);
                        file_io::require(m, &mut b, ptr, &mut dc, okay, "C array size overflow")?;
                        count = total;
                    }
                    let data = allocate(m, &mut b, ptr, &mut dc, count, 8)?;
                    let flags = allocate(m, &mut b, ptr, &mut dc, count, 8)?;
                    let ready = b.ins().iconst(types::I64, i64::from(arg.input));
                    elements(&mut b, count, |b, i| {
                        let f = element(b, flags, i, 8);
                        b.ins().store(ir::MemFlags::new(), ready, f, 0);
                        Ok(())
                    })?;
                    if arg.input {
                        copy_array(
                            m, &mut b, ptr, &mut dc, count, value, data, None, &arg.kind, false,
                        )?;
                    }
                    actuals.extend([data, flags, label, count]);
                    arrays.push((data, flags, value, count, arg));
                    continue;
                }
                let cell = address(&mut b, ptr);
                let flag = address(&mut b, ptr);
                let input = if arg.value {
                    Some(widen(&mut b, value, &arg.kind))
                } else if arg.input {
                    let v = b
                        .ins()
                        .load(c_type(&arg.kind)?, ir::MemFlags::new(), value, 0);
                    Some(widen(&mut b, v, &arg.kind))
                } else {
                    None
                };
                if let Some(v) = input {
                    b.ins().store(ir::MemFlags::new(), v, cell, 0);
                }
                let ready = b
                    .ins()
                    .iconst(types::I64, i64::from(arg.input || arg.value));
                b.ins().store(ir::MemFlags::new(), ready, flag, 0);
                let data = as_word(&mut b, cell);
                let mask = as_word(&mut b, flag);
                actuals.extend([data, mask, label, one]);
                if !arg.value && arg.output {
                    outputs.push((cell, value, arg.kind.clone()));
                }
            }
            let result = if let Some(k) = &s.result {
                let cell = address(&mut b, ptr);
                let flag = address(&mut b, ptr);
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().store(ir::MemFlags::new(), zero, flag, 0);
                let data = as_word(&mut b, cell);
                let mask = as_word(&mut b, flag);
                actuals.splice(0..0, [data, mask, label, one]);
                Some((cell, k))
            } else {
                None
            };
            let target = m.declare_func_in_func(ids[&f.name], b.func);
            b.ins().call(target, &actuals);
            for (data, flags, target, count, arg) in arrays {
                if arg.output {
                    copy_array(
                        m,
                        &mut b,
                        ptr,
                        &mut dc,
                        count,
                        target,
                        data,
                        Some(flags),
                        &arg.kind,
                        true,
                    )?;
                }
                dispose(m, &mut b, ptr, data)?;
                dispose(m, &mut b, ptr, flags)?;
            }
            for (cell, target, k) in outputs {
                let v = b.ins().load(wide(&k), ir::MemFlags::new(), cell, 0);
                let v = narrow(&mut b, v, &k)?;
                b.ins().store(ir::MemFlags::new(), v, target, 0);
            }
            if let Some((cell, k)) = result {
                let v = b.ins().load(wide(k), ir::MemFlags::new(), cell, 0);
                let v = narrow(&mut b, v, k)?;
                b.ins().return_(&[v]);
            } else {
                b.ins().return_(&[]);
            }
            b.finalize();
        }
        m.define_function(id, &mut ctx)?;
    }
    Ok(())
}

pub(super) fn symbols(module: &crate::ir::Module) -> HashSet<String> {
    fn body(stmts: &[IStmt], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                IStmt::Call(name, a) if matches!(name.as_str(), "@c_export" | "@c_invoke") => {
                    if let Some(IExpr::Str(s)) = a.first() {
                        if let Ok(s) = serde_json::from_str::<Signature>(s) {
                            out.insert(s.symbol);
                        }
                    }
                }
                IStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    body(then_body, out);
                    if let Some(b) = else_body {
                        body(b, out);
                    }
                }
                IStmt::Do { body: b, .. }
                | IStmt::DoWhile { body: b, .. }
                | IStmt::Block { body: b } => body(b, out),
                IStmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        body(&c.body, out);
                    }
                    if let Some(b) = default {
                        body(b, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    for f in &module.funcs {
        body(&f.body, &mut out);
    }
    out
}

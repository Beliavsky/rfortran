// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Convert scalar C storage to/from the private 64-bit rfort ABI.
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
pub(super) fn invoke(
    m: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr: ir::Type,
    args: &[IExpr],
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
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
    for (arg, actual) in s.args.iter().zip(&args[1..]) {
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
            for (arg, value) in s.args.iter().zip(passed) {
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

// SPDX-License-Identifier: MIT
//! Bound register-allocation cost for exceptionally large generated procedures.
use anyhow::Result;
use cranelift_codegen::{isa, settings::{self, Configurable}, Context};
use cranelift_module::{FuncId, Module, ModuleReloc};
use cranelift_object::ObjectModule;

const SINGLE_PASS_THRESHOLD: usize = 100_000;

pub(super) fn define(module: &mut ObjectModule, id: FuncId, context: &mut Context) -> Result<()> {
    if context.func.dfg.num_insts() < SINGLE_PASS_THRESHOLD {
        module.define_function(id, context)?;
        return Ok(());
    }
    // Keep optimizations and verification enabled. Only the register allocator
    // changes: backtracking can take minutes on heavily specialized procedures.
    let mut flags = settings::builder();
    for flag in module.isa().flags().iter() {
        flags.set(flag.name, &flag.value_string())?;
    }
    flags.set("regalloc_algorithm", "single_pass")?;
    let mut builder = isa::lookup(module.isa().triple().clone())?;
    for flag in module.isa().isa_flags() {
        builder.set(flag.name, &flag.value_string())?;
    }
    let target = builder.finish(settings::Flags::new(flags))?;
    let _pass = crate::compile_profile::Pass::new("codegen.single_pass");
    context.compile(target.as_ref(), &mut Default::default())
        .map_err(cranelift_module::ModuleError::from)?;
    // This is the same object/relocation path as ObjectModule::define_function,
    // with compilation performed using the per-function allocator above.
    let buffer = &context.compiled_code().unwrap().buffer;
    let relocations = buffer.relocs().iter()
        .map(|reloc| ModuleReloc::from_mach_reloc(reloc, &context.func, id))
        .collect::<Vec<_>>();
    module.define_function_bytes(id, buffer.alignment as u64, buffer.data(), &relocations)?;
    Ok(())
}

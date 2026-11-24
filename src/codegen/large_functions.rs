// SPDX-License-Identifier: MIT
//! Bound register-allocation cost for exceptionally large generated procedures.
use anyhow::Result;
use cranelift_codegen::{isa, settings::{self, Configurable}, Context};
use cranelift_module::{FuncId, Module, ModuleReloc};
use cranelift_object::ObjectModule;

const SINGLE_PASS_THRESHOLD: usize = 100_000;
/// Cranelift's optimizer took over two minutes on a 900,000-instruction
/// procedure, which compiles in seconds without it.
const UNOPTIMIZED_THRESHOLD: usize = 250_000;

pub(super) fn define(module: &mut ObjectModule, id: FuncId, context: &mut Context) -> Result<()> {
    // Cranelift packs instruction numbers into 24 bits; larger functions fail
    // verification with an unhelpful message.
    let instructions = context.func.dfg.num_insts();
    if instructions >= 1 << 24 {
        let name = module.declarations().get_function_decl(id).name.clone().unwrap_or_default();
        anyhow::bail!("procedure `{}` is too large to compile: it needs {instructions} code-generator instructions, \
            and at most 16,777,215 are supported. Calls of procedures with array or CHARACTER dummies are expanded \
            in the caller; dividing the caller into several procedures reduces its size",
            crate::source_diagnostics::user_names(&name));
    }
    if context.func.dfg.num_insts() < SINGLE_PASS_THRESHOLD {
        module.define_function(id, context)?;
        return Ok(());
    }
    // Keep verification enabled. The register allocator changes, since
    // backtracking can take minutes on heavily specialized procedures, and
    // the largest procedures are not optimized.
    let mut flags = settings::builder();
    for flag in module.isa().flags().iter() {
        flags.set(flag.name, &flag.value_string())?;
    }
    flags.set("regalloc_algorithm", "single_pass")?;
    if context.func.dfg.num_insts() >= UNOPTIMIZED_THRESHOLD {
        flags.set("opt_level", "none")?;
    }
    let mut builder = isa::lookup(module.isa().triple().clone())?;
    for flag in module.isa().isa_flags() {
        builder.set(flag.name, &flag.value_string())?;
    }
    let target = builder.finish(settings::Flags::new(flags))?;
    let _pass = crate::compile_profile::Pass::new("codegen.single_pass");
    if let Err(error) = context.compile(target.as_ref(), &mut Default::default()) {
        // Name the first verifier errors: the bare message gives no location.
        if let cranelift_codegen::CodegenError::Verifier(errors) = &error.inner {
            for e in errors.0.iter().take(5) {
                eprintln!("verifier: {} {}: {}", e.location, e.context.as_deref().unwrap_or(""), e.message);
            }
        }
        return Err(cranelift_module::ModuleError::from(error).into());
    }
    // This is the same object/relocation path as ObjectModule::define_function,
    // with compilation performed using the per-function allocator above.
    let buffer = &context.compiled_code().unwrap().buffer;
    let relocations = buffer.relocs().iter()
        .map(|reloc| ModuleReloc::from_mach_reloc(reloc, &context.func, id))
        .collect::<Vec<_>>();
    module.define_function_bytes(id, buffer.alignment as u64, buffer.data(), &relocations)?;
    Ok(())
}

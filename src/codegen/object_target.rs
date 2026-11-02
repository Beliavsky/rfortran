//! Target requirements shared by every native object emission entry point.
use anyhow::Result;
use cranelift_codegen::settings::{self, Configurable};
use target_lexicon::{BinaryFormat, Triple};

pub(super) fn flags_for_target(triple: &Triple, flags: &settings::Flags) -> Result<settings::Flags> {
    if triple.binary_format != BinaryFormat::Macho {
        return Ok(flags.clone());
    }
    // Mach-O needs position-independent addressing. In particular, Cranelift
    // 0.124's object writer supports ARM64 GOT relocations, but not the direct
    // ADRP/ADD relocations generated for colocated data with is_pic=false.
    let mut builder = settings::builder();
    for flag in flags.iter() {
        builder.set(flag.name, &flag.value_string())?;
    }
    builder.set("is_pic", "true")?;
    Ok(settings::Flags::new(builder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::{ir::{self, types, InstBuilder}, isa};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
    use cranelift_module::{DataDescription, Linkage, Module};
    use cranelift_object::{ObjectBuilder, ObjectModule};

    #[test]
    fn target_flags_preserve_optimization_and_other_platform_defaults() {
        let mut builder = settings::builder();
        builder.set("opt_level", "speed").unwrap();
        builder.set("enable_probestack", "true").unwrap();
        builder.set("probestack_strategy", "inline").unwrap();
        let flags = settings::Flags::new(builder);
        for name in ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            let triple: Triple = name.parse().unwrap();
            let actual = flags_for_target(&triple, &flags).unwrap();
            for flag in flags.iter().filter(|flag| flag.name != "is_pic") {
                let value = actual.iter().find(|value| value.name == flag.name).unwrap();
                assert_eq!(flag.value_string(), value.value_string(), "{} on {name}", flag.name);
            }
            assert_eq!(actual.is_pic(), triple.binary_format == BinaryFormat::Macho);
        }
    }

    #[test]
    fn apple_silicon_object_emits_data_and_external_call_relocations() {
        let triple: Triple = "aarch64-apple-darwin".parse().unwrap();
        for optimization in ["none", "speed"] {
            let mut flags = settings::builder();
            flags.set("opt_level", optimization).unwrap();
            let flags = flags_for_target(&triple, &settings::Flags::new(flags)).unwrap();
            let isa = isa::lookup(triple.clone()).unwrap().finish(flags).unwrap();
            let builder = ObjectBuilder::new(isa, "apple-silicon-regression",
                                             cranelift_module::default_libcall_names()).unwrap();
            let mut module = ObjectModule::new(builder);
            let mut description = DataDescription::new();
            description.define(b"Hello\0".to_vec().into_boxed_slice());
            let message = module.declare_data("message", Linkage::Local, false, false).unwrap();
            module.define_data(message, &description).unwrap();
            description.clear();
            description.define(0i64.to_le_bytes().to_vec().into_boxed_slice());
            description.set_align(8);
            let counter = module.declare_data("counter", Linkage::Local, true, false).unwrap();
            module.define_data(counter, &description).unwrap();
            let mut puts_signature = module.make_signature();
            puts_signature.params.push(ir::AbiParam::new(types::I64));
            puts_signature.returns.push(ir::AbiParam::new(types::I32));
            let puts = module.declare_function("puts", Linkage::Import, &puts_signature).unwrap();
            let mut context = module.make_context();
            context.func.signature.returns.push(ir::AbiParam::new(types::I32));
            let main = module.declare_function("main", Linkage::Export, &context.func.signature).unwrap();
            let message_ref = module.declare_data_in_func(message, &mut context.func);
            let counter_ref = module.declare_data_in_func(counter, &mut context.func);
            let puts_ref = module.declare_func_in_func(puts, &mut context.func);
            let mut function_context = FunctionBuilderContext::new();
            let mut function = FunctionBuilder::new(&mut context.func, &mut function_context);
            let entry = function.create_block();
            function.switch_to_block(entry);
            function.seal_all_blocks();
            let address = function.ins().global_value(types::I64, message_ref);
            let cell = function.ins().global_value(types::I64, counter_ref);
            let value = function.ins().load(types::I64, ir::MemFlags::new(), cell, 0);
            let value = function.ins().iadd_imm(value, 1);
            function.ins().store(ir::MemFlags::new(), value, cell, 0);
            let call = function.ins().call(puts_ref, &[address]);
            let result = function.inst_results(call)[0];
            function.ins().return_(&[result]);
            function.finalize();
            // Before enabling PIC, object emission panicked here on
            // Aarch64AdrPrelPgHi21 / Aarch64AddAbsLo12Nc.
            module.define_function(main, &mut context).unwrap();
            let bytes = module.finish().emit().unwrap();
            assert_eq!(&bytes[..4], b"\xcf\xfa\xed\xfe");
        }
    }
}

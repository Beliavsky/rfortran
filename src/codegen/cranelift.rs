#[path = "file_io.rs"]
mod file_io;
#[path = "characters.rs"]
mod characters;
#[path = "module_data.rs"]
mod module_data;
#[path = "record_heap.rs"]
mod record_heap;
#[path = "c_interop.rs"]
mod c_interop;
use anyhow::{bail, Context, Result};
use cranelift_codegen::{isa, settings};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

use super::Backend;
use crate::ir::{IExpr, IStmt, IcCaseItem, Module};
use crate::ast::TypeSpec;

use cranelift_codegen::ir;
use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module as _};
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
static GLOBAL_STR_ID: AtomicU32 = AtomicU32::new(0);

pub struct CraneliftBackend {
    pub triple: Option<Triple>,
    pub flags: settings::Flags,
    module_owners: Option<HashSet<String>>,
}
impl Default for CraneliftBackend {
    fn default() -> Self {
        Self {
            triple: None,
            flags: settings::Flags::new(settings::builder()),
            module_owners: None,
        }
    }
}
impl CraneliftBackend {
    pub fn with_flags(flags: settings::Flags) -> Self {
        Self {
            triple: None,
            flags,
            module_owners: None,
        }
    }
    pub fn with_module_owners(mut self, owners: HashSet<String>) -> Self {
        self.module_owners = Some(owners);
        self
    }
}
enum CgVal {
    I64(ir::Value),
    F64(ir::Value),
    Bool(ir::Value),
}

#[derive(Clone, Copy)]
enum Storage {
    Stack(ir::StackSlot),
    Static(ir::GlobalValue),
    HeapStack(ir::StackSlot),
    HeapStatic(ir::GlobalValue),
}
impl From<ir::StackSlot> for Storage {
    fn from(slot: ir::StackSlot) -> Self { Self::Stack(slot) }
}
fn storage_addr(b: &mut FunctionBuilder, ptr_ty: ir::Type, storage: impl Into<Storage>) -> ir::Value {
    match storage.into() {
        Storage::Stack(slot) => b.ins().stack_addr(ptr_ty, slot, 0),
        Storage::Static(global) => b.ins().global_value(ptr_ty, global),
        Storage::HeapStack(slot)=>{let cell=b.ins().stack_addr(ptr_ty,slot,0);b.ins().load(ptr_ty,ir::MemFlags::new(),cell,0)},
        Storage::HeapStatic(global)=>{let cell=b.ins().global_value(ptr_ty,global);b.ins().load(ptr_ty,ir::MemFlags::new(),cell,0)},
    }
}
fn heap_cell(b:&mut FunctionBuilder,ptr:ir::Type,storage:Storage)->Result<ir::Value> {
    Ok(match storage {Storage::HeapStack(slot)=>b.ins().stack_addr(ptr,slot,0),Storage::HeapStatic(global)=>b.ins().global_value(ptr,global),_=>bail!("array is missing its runtime allocation descriptor")})
}
fn array_storage(b: &mut FunctionBuilder, module: &mut ObjectModule, len: usize, main: bool) -> Result<Storage> {
    let size = len.checked_mul(8).ok_or_else(|| anyhow::anyhow!("array size overflow"))?;
    fixed_storage(b, module, size, main)
}

fn fixed_storage(b: &mut FunctionBuilder, module: &mut ObjectModule, size: usize, main: bool) -> Result<Storage> {
    if main && size > 8192 {
        // Main-program storage has program lifetime, including locals of
        // inlined procedures. Large CHARACTER buffers need the same treatment
        // as numeric arrays, otherwise CSV routines can exhaust the stack.
        let name = format!(".f90c.array.{}", GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed));
        let id = module.declare_data(&name, Linkage::Local, true, false)?;
        let mut data = DataDescription::new();
        data.define_zeroinit(size);
        data.set_align(8);
        module.define_data(id, &data)?;
        Ok(Storage::Static(module.declare_data_in_func(id, b.func)))
    } else {
        let size = u32::try_from(size).map_err(|_| anyhow::anyhow!("array is too large for stack storage"))?;
        Ok(b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot, size, 0)).into())
    }
}

#[derive(Clone, Copy)]
enum RetKind {
    Void,
    Bool,
    I64,
    F64,
}

#[derive(Clone, Copy)]
enum ParamKind {
    I64,
    F64,
}

fn to_f64(b: &mut FunctionBuilder, v: CgVal) -> ir::Value {
    use ir::types;
    match v {
        CgVal::F64(x) => x,
        CgVal::I64(x) => b.ins().fcvt_from_sint(types::F64, x),
        CgVal::Bool(x) => {
            let one = b.ins().iconst(types::I64, 1);
            let zero = b.ins().iconst(types::I64, 0);
            let i = b.ins().select(x, one, zero);
            b.ins().fcvt_from_sint(types::F64, i)
        }
    }
}
/// The value of a constant INTEGER size expression.
fn constant_size(e: &IExpr) -> Option<i128> {
    use crate::ast::{BinOp, UnOp};
    match e {
        IExpr::IntLit(s) => s.parse().ok(),
        IExpr::Un(UnOp::Neg, a) => constant_size(a).map(|v| -v),
        IExpr::Bin(op, a, b) => {
            let (a, b) = (constant_size(a)?, constant_size(b)?);
            match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div if b != 0 => Some(a / b),
                BinOp::Pow if (0..64).contains(&b) => a.checked_pow(b as u32),
                _ => None,
            }
        }
        _ => None,
    }
}
fn to_i64(b: &mut FunctionBuilder, v: CgVal) -> ir::Value {
    use ir::types;
    match v {
        CgVal::I64(x) => x,
        CgVal::F64(x) => b.ins().fcvt_to_sint(types::I64, x),
        CgVal::Bool(x) => {
            let one = b.ins().iconst(types::I64, 1);
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().select(x, one, zero)
        }
    }
}
fn to_bool(b: &mut FunctionBuilder, v: CgVal) -> ir::Value {
    use ir::condcodes::{FloatCC, IntCC};
    use ir::types;
    match v {
        CgVal::Bool(x) => x,
        CgVal::I64(x) => {
            let z = b.ins().iconst(types::I64, 0);
            b.ins().icmp(IntCC::NotEqual, x, z)
        }
        CgVal::F64(x) => {
            let z = b.ins().f64const(0.0);
            b.ins().fcmp(FloatCC::NotEqual, x, z)
        }
    }
}

// xorshift64*: a repeatable default stream, shared by all RANDOM_NUMBER calls.
fn random_value(b: &mut FunctionBuilder, state: ir::Value, double: bool) -> ir::Value {
    let x = b.ins().load(ir::types::I64, ir::MemFlags::new(), state, 0);
    let shifted = b.ins().ushr_imm(x, 12);
    let x = b.ins().bxor(x, shifted);
    let shifted = b.ins().ishl_imm(x, 25);
    let x = b.ins().bxor(x, shifted);
    let shifted = b.ins().ushr_imm(x, 27);
    let x = b.ins().bxor(x, shifted);
    b.ins().store(ir::MemFlags::new(), x, state, 0);
    let bits = b.ins().imul_imm(x, 2685821657736338717);
    let precision = if double { 53 } else { 24 };
    let bits = b.ins().ushr_imm(bits, 64 - precision);
    let value = b.ins().fcvt_from_uint(ir::types::F64, bits);
    let scale = b.ins().f64const(1.0 / ((1u64 << precision) as f64));
    b.ins().fmul(value, scale)
}

// Emit a bounded character record reader. Always consume the entire input record,
// even when the destination is shorter; a following READ starts on the next one.
fn read_record(
    module: &mut ObjectModule, b: &mut FunctionBuilder, ptr_ty: ir::Type,
    dc: &mut u32, destination: ir::Value, length: usize, file: Option<ir::Value>,
) -> Result<()> {
    use ir::{types, AbiParam, MemFlags, condcodes::IntCC};
    let mut sig=module.make_signature();
    if file.is_some() { sig.params.push(AbiParam::new(ptr_ty)); }
    sig.returns.push(AbiParam::new(types::I32));
    let id=module.declare_function(if file.is_some() { "fgetc" } else { "getchar" },Linkage::Import,&sig)?;
    let getchar=module.declare_func_in_func(id,b.func);
    let mut sig=module.make_signature();
    sig.params.extend([AbiParam::new(ptr_ty),AbiParam::new(types::I32),AbiParam::new(ptr_ty)]);
    sig.returns.push(AbiParam::new(ptr_ty));
    let id=module.declare_function("memset",Linkage::Import,&sig)?;
    let memset=module.declare_func_in_func(id,b.func);
    let space=b.ins().iconst(types::I32,32);
    let size=b.ins().iconst(ptr_ty,length as i64);
    b.ins().call(memset,&[destination,space,size]);
    let terminator=b.ins().iadd_imm(destination,length as i64);
    let zero_byte=b.ins().iconst(types::I8,0);
    b.ins().store(MemFlags::new(),zero_byte,terminator,0);
    let read=b.create_block();let eof=b.create_block();let check=b.create_block();
    let store=b.create_block();let advance=b.create_block();let done=b.create_block();
    let invalid=b.create_block();let failure=b.create_block();
    b.append_block_param(read,ptr_ty);
    let zero=b.ins().iconst(ptr_ty,0);
    b.ins().jump(read,&[zero.into()]);
    b.switch_to_block(read);
    let count=b.block_params(read)[0];
    let call=b.ins().call(getchar,&file.into_iter().collect::<Vec<_>>());let ch=b.inst_results(call)[0];
    let is_eof=b.ins().icmp_imm(IntCC::SignedLessThan,ch,0);
    b.ins().brif(is_eof,eof,&[],check,&[]);
    b.switch_to_block(eof);b.seal_block(eof);
    let empty=b.ins().icmp_imm(IntCC::Equal,count,0);
    b.ins().brif(empty,failure,&[],done,&[]);
    b.switch_to_block(check);b.seal_block(check);
    let newline=b.ins().icmp_imm(IntCC::Equal,ch,10);
    let validate=b.create_block();
    b.ins().brif(newline,done,&[],validate,&[]);
    b.switch_to_block(validate);b.seal_block(validate);
    let nul=b.ins().icmp_imm(IntCC::Equal,ch,0);
    let non_ascii=b.ins().icmp_imm(IntCC::UnsignedGreaterThan,ch,127);
    let bad=b.ins().bor(nul,non_ascii);
    let capacity=b.create_block();b.ins().brif(bad,invalid,&[],capacity,&[]);
    b.switch_to_block(capacity);b.seal_block(capacity);
    let fits=b.ins().icmp_imm(IntCC::UnsignedLessThan,count,length as i64);
    b.ins().brif(fits,store,&[],advance,&[]);
    b.switch_to_block(store);b.seal_block(store);
    let address=b.ins().iadd(destination,count);
    let byte=b.ins().ireduce(types::I8,ch);
    b.ins().store(MemFlags::new(),byte,address,0);
    b.ins().jump(advance,&[]);
    b.switch_to_block(advance);b.seal_block(advance);
    // Saturate beyond capacity: still distinguishes an empty file for LEN=0.
    let next=b.ins().iadd_imm(count,1);
    let limit=b.ins().iconst(ptr_ty,length as i64+1);
    let next=b.ins().umin(next,limit);
    b.ins().jump(read,&[next.into()]);b.seal_block(read);
    for (block,message) in [
        (failure,"Runtime error: end of file or input error in formatted READ\n"),
        (invalid,"Runtime error: formatted READ currently requires non-NUL ASCII input\n"),
    ] {
        b.switch_to_block(block);b.seal_block(block);
        let pointer=make_data(module,b,ptr_ty,dc,message.as_bytes())?;
        let mut sig=module.make_signature();
        sig.params.extend([AbiParam::new(types::I32),AbiParam::new(ptr_ty),AbiParam::new(types::I32)]);
        sig.returns.push(AbiParam::new(types::I32));
        let id=module.declare_function(if cfg!(windows) { "_write" } else { "rfort_write" },Linkage::Import,&sig)?;
        let write=module.declare_func_in_func(id,b.func);
        let stderr=b.ins().iconst(types::I32,2);
        let size=b.ins().iconst(types::I32,message.len() as i64);
        b.ins().call(write,&[stderr,pointer,size]);
        let mut sig=module.make_signature();sig.params.push(AbiParam::new(types::I32));
        let id=module.declare_function("exit",Linkage::Import,&sig)?;
        let exit=module.declare_func_in_func(id,b.func);
        let code=b.ins().iconst(types::I32,1);b.ins().call(exit,&[code]);
        b.ins().trap(ir::TrapCode::unwrap_user(1));
    }
    b.switch_to_block(done);b.seal_block(done);
    if let Some(file) = file {
        let error=file_io::call(module,b,"ferror",&[file],types::I32)?;
        let okay=b.ins().icmp_imm(IntCC::Equal,error,0);
        file_io::require(module,b,ptr_ty,dc,okay,"file input failed")?;
        // Binary file streams preserve CR. Strip it only immediately before LF,
        // and only if it was stored rather than truncated past the destination.
        let newline=b.ins().icmp_imm(IntCC::Equal,ch,10);
        let nonempty=b.ins().icmp_imm(IntCC::UnsignedGreaterThan,count,0);
        let fits=b.ins().icmp_imm(IntCC::UnsignedLessThanOrEqual,count,length as i64);
        let strip=b.ins().band(newline,nonempty);let strip=b.ins().band(strip,fits);
        let inspect=b.create_block();let finish=b.create_block();
        b.ins().brif(strip,inspect,&[],finish,&[]);
        b.switch_to_block(inspect);b.seal_block(inspect);
        let offset=b.ins().iadd_imm(count,-1);let address=b.ins().iadd(destination,offset);
        let byte=b.ins().load(types::I8,MemFlags::new(),address,0);
        let cr=b.ins().icmp_imm(IntCC::Equal,byte,13);
        let blank=b.ins().iconst(types::I8,32);let value=b.ins().select(cr,blank,byte);
        b.ins().store(MemFlags::new(),value,address,0);
        b.ins().jump(finish,&[]);b.switch_to_block(finish);b.seal_block(finish);
    }
    Ok(())
}

struct Externs {
    float_model: ir::FuncRef,
    checked_integer: ir::FuncRef,
    cpu_seconds: ir::FuncRef,
    clock_count: ir::FuncRef,
    printf: ir::FuncRef,
    sprintf: ir::FuncRef,
    gcvt: ir::FuncRef,
    character_compare: ir::FuncRef,
    pow: ir::FuncRef,
    scanf: ir::FuncRef,

    // Cached function addresses for reducing movabs instructions
    printf_addr: Option<ir::Value>,
    sprintf_addr: Option<ir::Value>,
    gcvt_addr: Option<ir::Value>,
    scanf_addr: Option<ir::Value>,
    pow_addr: Option<ir::Value>,
    character_compare_addr: Option<ir::Value>,

    // Signatures for call_indirect when caching is enabled
    sig_printf: Option<ir::SigRef>,
    sig_sprintf: Option<ir::SigRef>,
    sig_gcvt: Option<ir::SigRef>,
    sig_character_compare: Option<ir::SigRef>,
    sig_pow: Option<ir::SigRef>,
    sig_scanf: Option<ir::SigRef>,

    fmt_s: ir::Value,
    fmt_f64: ir::Value,
    fmt_i64: ir::Value,
    fmt_i64_dot0: ir::Value,
    p_true: ir::Value,
    p_false: ir::Value,
    p_nl: ir::Value,
}
fn make_data(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr_ty: ir::Type,
    _counter: &mut u32,
    bytes: &[u8],
) -> Result<ir::Value> {
    let mut raw = bytes.to_vec();
    if *raw.last().unwrap_or(&0) != 0 {
        raw.push(0);
    }
    let id_num = GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!(".str.cst.{}", id_num);
    let id = module.declare_data(&name, Linkage::Local, false, false)?;
    let mut dd = DataDescription::new();
    dd.define(raw.into_boxed_slice());
    module.define_data(id, &dd)?;
    let gv = module.declare_data_in_func(id, b.func);
    Ok(b.ins().global_value(ptr_ty, gv))
}

fn declare_externs(
    module: &mut ObjectModule,
    b: &mut FunctionBuilder,
    ptr_ty: ir::Type,
) -> Result<Externs> {
    use ir::{types, AbiParam, Signature};
    let cc = module.isa().default_call_conv();
    let mut sig_printf = Signature::new(cc);
    sig_printf.params.push(AbiParam::new(ptr_ty));
    sig_printf.params.push(AbiParam::new(ptr_ty));
    sig_printf.returns.push(AbiParam::new(types::I32));
    let printf_id = module.declare_function(if cfg!(windows) { "printf" } else { "rfort_printf" }, Linkage::Import, &sig_printf)?;
    let printf_ref = module.declare_func_in_func(printf_id, b.func);
    let mut sig_sprintf = Signature::new(cc);
    sig_sprintf.params.push(AbiParam::new(ptr_ty));
    sig_sprintf.params.push(AbiParam::new(ptr_ty));
    sig_sprintf.params.push(AbiParam::new(types::I64));
    sig_sprintf.returns.push(AbiParam::new(types::I32));
    let sprintf_id = module.declare_function(if cfg!(windows) { "sprintf" } else { "rfort_sprintf" }, Linkage::Import, &sig_sprintf)?;
    let sprintf_ref = module.declare_func_in_func(sprintf_id, b.func);
    let mut sig_gcvt = Signature::new(cc);
    sig_gcvt.params.push(AbiParam::new(types::F64));
    sig_gcvt.params.push(AbiParam::new(types::I32));
    sig_gcvt.params.push(AbiParam::new(ptr_ty));
    sig_gcvt.returns.push(AbiParam::new(ptr_ty));
    let gcvt_id = module.declare_function("rfort_real_text", Linkage::Import, &sig_gcvt)?;
    let gcvt_ref = module.declare_func_in_func(gcvt_id, b.func);
    let mut sig_pow = Signature::new(cc);
    sig_pow.params.push(AbiParam::new(types::F64));
    sig_pow.params.push(AbiParam::new(types::F64));
    sig_pow.returns.push(AbiParam::new(types::F64));
    let pow_id = module.declare_function("pow", Linkage::Import, &sig_pow)?;
    let pow_ref = module.declare_func_in_func(pow_id, b.func);

    let mut sig_scanf = Signature::new(cc);
    sig_scanf.params.push(AbiParam::new(ptr_ty));
    sig_scanf.params.push(AbiParam::new(ptr_ty));
    sig_scanf.returns.push(AbiParam::new(types::I32));
    let scanf_id = module.declare_function(if cfg!(windows) { "scanf" } else { "rfort_scanf" }, Linkage::Import, &sig_scanf)?;
    let scanf_ref = module.declare_func_in_func(scanf_id, b.func);

    let mut sig_character_compare = Signature::new(cc);
    sig_character_compare.params.push(AbiParam::new(ptr_ty));
    sig_character_compare.params.push(AbiParam::new(ptr_ty));
    sig_character_compare.params.push(AbiParam::new(ptr_ty));
    sig_character_compare.params.push(AbiParam::new(ptr_ty));
    sig_character_compare.returns.push(AbiParam::new(types::I32));
    let character_compare_id = module.declare_function("rfort_character_compare", Linkage::Import, &sig_character_compare)?;
    let character_compare_ref = module.declare_func_in_func(character_compare_id, b.func);

    let mut c = 0u32;
    let fmt_s = make_data(module, b, ptr_ty, &mut c, b"%s")?;
    let fmt_f64 = make_data(module, b, ptr_ty, &mut c, b"%lf")?;
    let fmt_i64 = make_data(module, b, ptr_ty, &mut c, b"%lld")?;
    let fmt_i64_dot0 = make_data(module, b, ptr_ty, &mut c, b"%lld.0")?;
    let p_true = make_data(module, b, ptr_ty, &mut c, b"T")?;
    let p_false = make_data(module, b, ptr_ty, &mut c, b"F")?;
    let p_nl = make_data(module, b, ptr_ty, &mut c, b"\n")?;

    let mut sig = module.make_signature();
    sig.returns.push(AbiParam::new(types::F64));
    let id = module.declare_function("rfort_cpu_seconds", Linkage::Import, &sig)?;
    let cpu_seconds = module.declare_func_in_func(id, b.func);
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I64));
    let id = module.declare_function("rfort_clock_count", Linkage::Import, &sig)?;
    let clock_count = module.declare_func_in_func(id, b.func);
    let mut sig=module.make_signature();
    sig.params.extend([AbiParam::new(types::F64),AbiParam::new(types::I64),AbiParam::new(types::I64)]);
    sig.returns.push(AbiParam::new(types::I64));
    let id=module.declare_function("rfort_checked_integer",Linkage::Import,&sig)?;
    let checked_integer=module.declare_func_in_func(id,b.func);
    let mut sig=module.make_signature();
    sig.params.extend([AbiParam::new(types::I64),AbiParam::new(types::I64),AbiParam::new(types::F64),AbiParam::new(types::I64),AbiParam::new(types::F64)]);
    sig.returns.push(AbiParam::new(types::F64));
    let id=module.declare_function("rfort_float_model",Linkage::Import,&sig)?;
    let float_model=module.declare_func_in_func(id,b.func);
    Ok(Externs {
        float_model,
        checked_integer,
        cpu_seconds,
        clock_count,
        printf: printf_ref,
        sprintf: sprintf_ref,
        gcvt: gcvt_ref,
        character_compare: character_compare_ref,
        pow: pow_ref,
        scanf: scanf_ref,
        printf_addr: None,
        sprintf_addr: None,
        gcvt_addr: None,
        scanf_addr: None,
        pow_addr: None,
        character_compare_addr: None,
        sig_printf: None,
        sig_sprintf: None,
        sig_gcvt: None,
        sig_character_compare: None,
        sig_pow: None,
        sig_scanf: None,
        fmt_s,
        fmt_f64,
        fmt_i64,
        fmt_i64_dot0,
        p_true,
        p_false,
        p_nl,
    })
}

impl Externs {
    fn enable_caching(&mut self, b: &mut FunctionBuilder, ptr_ty: ir::Type) {
        use ir::{types, AbiParam, Signature};
        let cc = b.func.signature.call_conv;

        // Compute function addresses once
        self.printf_addr = Some(b.ins().func_addr(ptr_ty, self.printf));
        self.sprintf_addr = Some(b.ins().func_addr(ptr_ty, self.sprintf));
        self.gcvt_addr = Some(b.ins().func_addr(ptr_ty, self.gcvt));
        self.scanf_addr = Some(b.ins().func_addr(ptr_ty, self.scanf));
        self.pow_addr = Some(b.ins().func_addr(ptr_ty, self.pow));
        self.character_compare_addr = Some(b.ins().func_addr(ptr_ty, self.character_compare));

        // Import signatures for call_indirect
        let mut sig_printf = Signature::new(cc);
        sig_printf.params.push(AbiParam::new(ptr_ty));
        sig_printf.params.push(AbiParam::new(ptr_ty));
        sig_printf.returns.push(AbiParam::new(types::I32));
        self.sig_printf = Some(b.func.import_signature(sig_printf));

        let mut sig_sprintf = Signature::new(cc);
        sig_sprintf.params.push(AbiParam::new(ptr_ty));
        sig_sprintf.params.push(AbiParam::new(ptr_ty));
        sig_sprintf.params.push(AbiParam::new(types::I64));
        sig_sprintf.returns.push(AbiParam::new(types::I32));
        self.sig_sprintf = Some(b.func.import_signature(sig_sprintf));

        let mut sig_gcvt = Signature::new(cc);
        sig_gcvt.params.push(AbiParam::new(types::F64));
        sig_gcvt.params.push(AbiParam::new(types::I32));
        sig_gcvt.params.push(AbiParam::new(ptr_ty));
        sig_gcvt.returns.push(AbiParam::new(ptr_ty));
        self.sig_gcvt = Some(b.func.import_signature(sig_gcvt));

        let mut sig_pow = Signature::new(cc);
        sig_pow.params.push(AbiParam::new(types::F64));
        sig_pow.params.push(AbiParam::new(types::F64));
        sig_pow.returns.push(AbiParam::new(types::F64));
        self.sig_pow = Some(b.func.import_signature(sig_pow));

        let mut sig_scanf = Signature::new(cc);
        sig_scanf.params.push(AbiParam::new(ptr_ty));
        sig_scanf.params.push(AbiParam::new(ptr_ty));
        sig_scanf.returns.push(AbiParam::new(types::I32));
        self.sig_scanf = Some(b.func.import_signature(sig_scanf));

        let mut sig_character_compare = Signature::new(cc);
        sig_character_compare.params.push(AbiParam::new(ptr_ty));
        sig_character_compare.params.push(AbiParam::new(ptr_ty));
        sig_character_compare.params.push(AbiParam::new(ptr_ty));
        sig_character_compare.params.push(AbiParam::new(ptr_ty));
        sig_character_compare.returns.push(AbiParam::new(types::I32));
        self.sig_character_compare = Some(b.func.import_signature(sig_character_compare));
    }

    fn call_printf(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        if let (Some(addr), Some(sig)) = (self.printf_addr, self.sig_printf) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.printf, args)
        }
    }

    fn call_sprintf(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        if let (Some(addr), Some(sig)) = (self.sprintf_addr, self.sig_sprintf) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.sprintf, args)
        }
    }

    fn call_gcvt(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        // Fifteen-digit rounding can print HUGE(REAL(8)) just above f64::MAX.
        // Retain enough digits at the upper boundary to round back to a finite
        // value, including when a caller normally uses only six digits.
        let magnitude=b.ins().fabs(args[0]);
        let boundary=b.ins().f64const(f64::MAX * 0.99);
        let large=b.ins().fcmp(ir::condcodes::FloatCC::GreaterThan,magnitude,boundary);
        let full=b.ins().iconst(ir::types::I32,17);
        let digits=b.ins().select(large,full,args[1]);
        let args=&[args[0],digits,args[2]];
        if let (Some(addr), Some(sig)) = (self.gcvt_addr, self.sig_gcvt) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.gcvt, args)
        }
    }

    fn call_scanf(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        if let (Some(addr), Some(sig)) = (self.scanf_addr, self.sig_scanf) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.scanf, args)
        }
    }

    fn call_pow(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        if let (Some(addr), Some(sig)) = (self.pow_addr, self.sig_pow) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.pow, args)
        }
    }

    fn call_character_compare(&self, b: &mut FunctionBuilder, args: &[ir::Value]) -> ir::Inst {
        if let (Some(addr), Some(sig)) = (self.character_compare_addr, self.sig_character_compare) {
            b.ins().call_indirect(sig, addr, args)
        } else {
            b.ins().call(self.character_compare, args)
        }
    }
}

fn int_to_str(
    b: &mut FunctionBuilder,
    ex: &Externs,
    ptr_ty: ir::Type,
    sprintf: ir::FuncRef,
    v: ir::Value,
) -> ir::Value {
    let buf = b.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        32,
        0,
    ));
    let p = b.ins().stack_addr(ptr_ty, buf, 0);
    b.ins().call(sprintf, &[p, ex.fmt_i64, v]);
    p
}

fn integer_power(b: &mut FunctionBuilder, base: ir::Value, exponent: ir::Value) -> ir::Value {
    use ir::condcodes::IntCC;
    let negative = b.create_block();
    let head = b.create_block();
    let body = b.create_block();
    let done = b.create_block();
    let factor = b.append_block_param(head, ir::types::I64);
    let remaining = b.append_block_param(head, ir::types::I64);
    let result = b.append_block_param(head, ir::types::I64);
    let answer = b.append_block_param(done, ir::types::I64);
    let one = b.ins().iconst(ir::types::I64, 1);
    let is_negative = b.ins().icmp_imm(IntCC::SignedLessThan, exponent, 0);
    b.ins().brif(is_negative, negative, &[], head, &[base.into(), exponent.into(), one.into()]);

    b.switch_to_block(negative);
    // Integer reciprocal gives 0 for |base| > 1, +/-1 otherwise; zero traps.
    let reciprocal = b.ins().sdiv(one, base);
    let magnitude = b.ins().ineg(exponent);
    b.ins().jump(head, &[reciprocal.into(), magnitude.into(), one.into()]);

    b.switch_to_block(head);
    let finished = b.ins().icmp_imm(IntCC::Equal, remaining, 0);
    b.ins().brif(finished, done, &[result.into()], body, &[]);
    b.switch_to_block(body);
    let bit = b.ins().band_imm(remaining, 1);
    let odd = b.ins().icmp_imm(IntCC::NotEqual, bit, 0);
    let product = b.ins().imul(result, factor);
    let next_result = b.ins().select(odd, product, result);
    let next_factor = b.ins().imul(factor, factor);
    // Logical shift also handles the magnitude of the most negative exponent.
    let next_exponent = b.ins().ushr_imm(remaining, 1);
    b.ins().jump(head, &[next_factor.into(), next_exponent.into(), next_result.into()]);
    for block in [negative, head, body, done] { b.seal_block(block); }
    b.switch_to_block(done);
    answer
}

fn eval_expr(
    b: &mut FunctionBuilder,
    ex: &Externs,
    ptr_ty: ir::Type,
    e: &IExpr,
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
    func_meta: &HashMap<String, FuncMeta>,
) -> CgVal {
    use ir::{types, MemFlags};
    match e {
        IExpr::IntLit(s) => {
            let v = i128::from_str_radix(s, 10)
                .ok()
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or(0);
            CgVal::I64(b.ins().iconst(types::I64, v))
        }
        IExpr::RealLit(s) => {
            let v = s.parse::<f64>().unwrap_or(0.0);
            CgVal::F64(b.ins().f64const(v))
        }
        IExpr::Logical(bv) => {
            let z = b.ins().iconst(ir::types::I8, 0);
            if *bv {
                let o = b.ins().iconst(ir::types::I8, 1);
                CgVal::Bool(b.ins().icmp(ir::condcodes::IntCC::NotEqual, o, z))
            } else {
                CgVal::Bool(b.ins().icmp(ir::condcodes::IntCC::NotEqual, z, z))
            }
        }
        IExpr::Ident(id) => {
            if let Some(ss) = ints.get(&format!("@character_pointer:{id}")) {
                let a = storage_addr(b, ptr_ty, *ss);
                CgVal::I64(b.ins().load(ir::types::I64, MemFlags::new(), a, 0))
            } else if let Some(ss) = ints.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                CgVal::I64(b.ins().load(ir::types::I64, MemFlags::new(), a, 0))
            } else if let Some(ss) = reals.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                CgVal::F64(b.ins().load(ir::types::F64, MemFlags::new(), a, 0))
            } else if let Some(ss) = bools.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                let v = b.ins().load(ir::types::I8, MemFlags::new(), a, 0);
                let z = b.ins().iconst(ir::types::I8, 0);
                CgVal::Bool(b.ins().icmp(ir::condcodes::IntCC::NotEqual, v, z))
            } else {
                CgVal::I64(b.ins().iconst(ir::types::I64, 0))
            }
        }
        IExpr::Index(name, args) => {
            if args.is_empty() {
                CgVal::I64(b.ins().iconst(ir::types::I64, 0))
            } else {
                let idx = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);

                let one = b.ins().iconst(ir::types::I64, 1);
                let idx_i = to_i64(b, idx);
                let adj = b.ins().isub(idx_i, one);
                let off = b.ins().imul_imm(adj, 8);
                if let Some(ss) = ints.get(name.as_str()) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let addr = b.ins().iadd(a, off);
                    CgVal::I64(b.ins().load(ir::types::I64, MemFlags::new(), addr, 0))
                } else if let Some(ss) = reals.get(name.as_str()) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let addr = b.ins().iadd(a, off);
                    CgVal::F64(b.ins().load(ir::types::F64, MemFlags::new(), addr, 0))
                } else if let Some(ss) = bools.get(name.as_str()) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let addr = b.ins().iadd(a, off);
                    let value = b.ins().load(ir::types::I8, MemFlags::new(), addr, 0);
                    CgVal::Bool(b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual, value, 0))
                } else {
                    CgVal::I64(b.ins().iconst(ir::types::I64, 0))
                }
            }
        }
        IExpr::Un(op, e1) => {
            let v = eval_expr(b, ex, ptr_ty, e1, ints, reals, bools, func_meta);
            match op {
                crate::ast::UnOp::Neg => match v {
                    CgVal::I64(x) => CgVal::I64(b.ins().ineg(x)),
                    CgVal::F64(x) => CgVal::F64(b.ins().fneg(x)),
                    CgVal::Bool(x) => {
                        let one = b.ins().iconst(ir::types::I64, 1);
                        let zero = b.ins().iconst(ir::types::I64, 0);
                        let x64 = b.ins().select(x, one, zero);
                        CgVal::I64(b.ins().ineg(x64))
                    }
                },
                crate::ast::UnOp::Not => {
                    let vb = to_bool(b, v);
                    let z0 = b.ins().iconst(ir::types::I8, 0);
                    let o1 = b.ins().iconst(ir::types::I8, 1);
                    let t = b.ins().icmp(ir::condcodes::IntCC::NotEqual, o1, z0);
                    CgVal::Bool(b.ins().bxor(vb, t))
                }
            }
        }
        IExpr::Bin(op, l, r) => {
            use crate::ast::BinOp;
            match op {
                BinOp::Concat => CgVal::I64(b.ins().iconst(ir::types::I64, 0)),
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                    let lv_raw = eval_expr(b, ex, ptr_ty, l, ints, reals, bools, func_meta);
                    let rv_raw = eval_expr(b, ex, ptr_ty, r, ints, reals, bools, func_meta);
                    match (op, &lv_raw, &rv_raw) {
                        (BinOp::Add, CgVal::I64(a), CgVal::I64(bv)) => {
                            CgVal::I64(b.ins().iadd(*a, *bv))
                        }
                        (BinOp::Sub, CgVal::I64(a), CgVal::I64(bv)) => {
                            CgVal::I64(b.ins().isub(*a, *bv))
                        }
                        (BinOp::Mul, CgVal::I64(a), CgVal::I64(bv)) => {
                            CgVal::I64(b.ins().imul(*a, *bv))
                        }
                        (BinOp::Div, CgVal::I64(a), CgVal::I64(bv)) => {
                            CgVal::I64(b.ins().sdiv(*a, *bv))
                        }
                        (BinOp::Pow, CgVal::I64(a), CgVal::I64(bv)) => {
                            CgVal::I64(integer_power(b, *a, *bv))
                        }
                        _ => {
                            let lv = to_f64(b, lv_raw);
                            let rv = to_f64(b, rv_raw);
                            let res = match op {
                                BinOp::Add => b.ins().fadd(lv, rv),
                                BinOp::Sub => b.ins().fsub(lv, rv),
                                BinOp::Mul => b.ins().fmul(lv, rv),
                                BinOp::Div => b.ins().fdiv(lv, rv),
                                BinOp::Pow => {
                                    let call = ex.call_pow(b, &[lv, rv]);
                                    b.inst_results(call)[0]
                                }
                                _ => unreachable!(),
                            };
                            CgVal::F64(res)
                        }
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    use ir::condcodes::{FloatCC, IntCC};
                    let lv_raw = eval_expr(b, ex, ptr_ty, l, ints, reals, bools, func_meta);
                    let rv_raw = eval_expr(b, ex, ptr_ty, r, ints, reals, bools, func_meta);
                    match (&lv_raw, &rv_raw) {
                        (CgVal::I64(a), CgVal::I64(bv)) => {
                            let cc = match op {
                                BinOp::Eq => IntCC::Equal,
                                BinOp::Ne => IntCC::NotEqual,
                                BinOp::Lt => IntCC::SignedLessThan,
                                BinOp::Le => IntCC::SignedLessThanOrEqual,
                                BinOp::Gt => IntCC::SignedGreaterThan,
                                BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
                                _ => unreachable!(),
                            };
                            CgVal::Bool(b.ins().icmp(cc, *a, *bv))
                        }
                        _ => {
                            let lv = to_f64(b, lv_raw);
                            let rv = to_f64(b, rv_raw);
                            let cc = match op {
                                BinOp::Eq => FloatCC::Equal,
                                BinOp::Ne => FloatCC::NotEqual,
                                BinOp::Lt => FloatCC::LessThan,
                                BinOp::Le => FloatCC::LessThanOrEqual,
                                BinOp::Gt => FloatCC::GreaterThan,
                                BinOp::Ge => FloatCC::GreaterThanOrEqual,
                                _ => unreachable!(),
                            };
                            CgVal::Bool(b.ins().fcmp(cc, lv, rv))
                        }
                    }
                }
                BinOp::And | BinOp::Or | BinOp::Eqv | BinOp::Neqv => {
                    let lv_raw = eval_expr(b, ex, ptr_ty, l, ints, reals, bools, func_meta);
                    let rv_raw = eval_expr(b, ex, ptr_ty, r, ints, reals, bools, func_meta);
                    let lv = to_bool(b, lv_raw);
                    let rv = to_bool(b, rv_raw);
                    match op {
                        BinOp::And => CgVal::Bool(b.ins().band(lv, rv)),
                        BinOp::Or => CgVal::Bool(b.ins().bor(lv, rv)),
                        BinOp::Eqv => {
                            let xor = b.ins().bxor(lv, rv);
                            let z0 = b.ins().iconst(ir::types::I8, 0);
                            let o1 = b.ins().iconst(ir::types::I8, 1);
                            let t = b.ins().icmp(ir::condcodes::IntCC::NotEqual, o1, z0);
                            CgVal::Bool(b.ins().bxor(xor, t))
                        }
                        BinOp::Neqv => CgVal::Bool(b.ins().bxor(lv, rv)),
                        _ => unreachable!(),
                    }
                }
            }
        }
        IExpr::Str(_) => CgVal::I64(b.ins().iconst(ir::types::I64, 0)),
        IExpr::Call(name, args) => {

            let lname = name.to_ascii_lowercase();
            if let Some((operation,kind))=crate::real_intrinsics::decode(&lname) {
                let x=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                let x=to_f64(b,x);
                let mut i=b.ins().iconst(ir::types::I64,0);
                let mut s=b.ins().f64const(0.0);
                if args.len()==2 {
                    let v=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);
                    if operation=="nearest" {s=to_f64(b,v);}else{i=to_i64(b,v);}
                }
                let op=b.ins().iconst(ir::types::I64,crate::real_intrinsics::opcode(operation));
                let kind=b.ins().iconst(ir::types::I64,kind as i64);
                let call=b.ins().call(ex.float_model,&[op,kind,x,i,s]);
                let value=b.inst_results(call)[0];
                return if operation=="exponent" {CgVal::I64(b.ins().fcvt_to_sint(ir::types::I64,value))} else {CgVal::F64(value)};
            }
            if lname == "@cpu_seconds" {
                let call = b.ins().call(ex.cpu_seconds, &[]);
                return CgVal::F64(b.inst_results(call)[0]);
            }
            if lname == "@clock_count" {
                let kind = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                let kind = to_i64(b, kind);
                let call = b.ins().call(ex.clock_count, &[kind]);
                return CgVal::I64(b.inst_results(call)[0]);
            }
            if lname=="@c_associated" {
                let a=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let a=to_i64(b,a);
                let mut yes=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,a,0);
                if let Some(e)=args.get(1) {let other=eval_expr(b,ex,ptr_ty,e,ints,reals,bools,func_meta);let other=to_i64(b,other);let same=b.ins().icmp(ir::condcodes::IntCC::Equal,a,other);yes=b.ins().band(yes,same);}
                return CgVal::I64(b.ins().uextend(types::I64,yes));
            }
            if lname=="@c_pointer_address" {
                let v=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                return CgVal::I64(to_i64(b,v));
            }
            if lname=="@c_desc_extent" || lname=="@c_desc_stride" {
                let base=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let base=to_i64(b,base);
                let axis=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let axis=to_i64(b,axis);
                let word=i64::from(ptr_ty.bytes());
                let offset=b.ins().imul_imm(axis,3*word);
                let offset=b.ins().iadd_imm(offset,2*word+8+if lname=="@c_desc_extent" {word}else{2*word});
                let address=b.ins().iadd(base,offset);
                return CgVal::I64(b.ins().load(types::I64,MemFlags::new(),address,0));
            }
            if let Some(kind) = lname.strip_prefix("@ptr_load_") {
                let base=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                let base=to_i64(b,base);
                let offset=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);
                let offset=to_i64(b,offset);
                if let Some(tag)=kind.strip_prefix("c_") {return c_interop::raw_load(b,base,offset,tag);}
                let bytes=b.ins().imul_imm(offset,8);let address=b.ins().iadd(base,bytes);
                return match kind {
                    "r4"|"r8"=>CgVal::F64(b.ins().load(ir::types::F64,MemFlags::new(),address,0)),
                    "l"=>{let value=b.ins().load(ir::types::I8,MemFlags::new(),address,0);CgVal::Bool(b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,value,0))},
                    _=>CgVal::I64(b.ins().load(ir::types::I64,MemFlags::new(),address,0)),
                };
            }
            if let Some((operation,bytes))=crate::bit_intrinsics::decode(&lname) {
                let values=args.iter().map(|arg| {
                    let value=eval_expr(b,ex,ptr_ty,arg,ints,reals,bools,func_meta);
                    to_i64(b,value)
                }).collect::<Vec<_>>();
                let bits=i64::from(bytes)*8;
                let zero=b.ins().iconst(ir::types::I64,0);
                let one=b.ins().iconst(ir::types::I64,1);
                let mask=b.ins().iconst(ir::types::I64,if bits==64 {-1} else {(1i64<<bits)-1});
                let i=b.ins().band(values[0],mask);
                let result=match operation {
                    "btest"=>{let shifted=b.ins().ushr(i,values[1]);let bit=b.ins().band(shifted,one);return CgVal::Bool(b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,bit,0));},
                    "iand"=>b.ins().band(i,values[1]),
                    "ior"=>b.ins().bor(i,values[1]),
                    "ieor"=>b.ins().bxor(i,values[1]),
                    "not"=>b.ins().bnot(i),
                    "ibset"=>{let bit=b.ins().ishl(one,values[1]);b.ins().bor(i,bit)},
                    "ibclr"=>{let bit=b.ins().ishl(one,values[1]);let clear=b.ins().bnot(bit);b.ins().band(i,clear)},
                    "ibits"=>{
                        let shifted=b.ins().ushr(i,values[1]);
                        let top=b.ins().ishl(one,values[2]);let short=b.ins().isub(top,one);
                        let full=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,values[2],64);
                        let all=b.ins().iconst(ir::types::I64,-1);let mask=b.ins().select(full,all,short);
                        b.ins().band(shifted,mask)
                    },
                    "ishft"=>{
                        let positive=b.ins().icmp_imm(ir::condcodes::IntCC::SignedGreaterThanOrEqual,values[1],0);
                        let negative=b.ins().ineg(values[1]);let magnitude=b.ins().select(positive,values[1],negative);
                        let left=b.ins().ishl(i,magnitude);let right=b.ins().ushr(i,magnitude);
                        let shifted=b.ins().select(positive,left,right);
                        let full=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,magnitude,bits);
                        b.ins().select(full,zero,shifted)
                    },
                    "popcnt"=>b.ins().popcnt(i),
                    "poppar"=>{let count=b.ins().popcnt(i);b.ins().band(count,one)},
                    "leadz"=>{let count=b.ins().clz(i);b.ins().iadd_imm(count,bits-64)},
                    "trailz"=>{
                        let count=b.ins().ctz(i);let width=b.ins().iconst(ir::types::I64,bits);
                        let none=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,i,0);
                        b.ins().select(none,width,count)
                    },
                    "dshiftl"|"dshiftr"=>{
                        // DSHIFTR(I, J, S) is DSHIFTL(I, J, BITS-S); the ends select I or J
                        // because Cranelift shift amounts are taken modulo 64.
                        let j=b.ins().band(values[1],mask);
                        let width=b.ins().iconst(ir::types::I64,bits);
                        let left=if operation=="dshiftl" {values[2]} else {b.ins().isub(width,values[2])};
                        let right=b.ins().isub(width,left);
                        let high=b.ins().ishl(i,left);let low=b.ins().ushr(j,right);
                        let joined=b.ins().bor(high,low);
                        let none=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,left,0);
                        let all=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,left,bits);
                        let joined=b.ins().select(all,j,joined);
                        b.ins().select(none,i,joined)
                    },
                    _=>unreachable!(),
                };
                let result=if bytes<8 {
                    let narrow=b.ins().ireduce(match bytes {1=>ir::types::I8,2=>ir::types::I16,_=>ir::types::I32},result);
                    b.ins().sextend(ir::types::I64,narrow)
                } else {result};
                return CgVal::I64(result);
            }
            match if func_meta.contains_key(name) { "" } else { lname.as_str() } {
                "@round_real32" => {
                    let value=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let value=to_f64(b,value);
                    let value=b.ins().fdemote(ir::types::F32,value);
                    CgVal::F64(b.ins().fpromote(ir::types::F64,value))
                }
                "isnan" => {
                    let value=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let value=to_f64(b,value);
                    CgVal::Bool(b.ins().fcmp(ir::condcodes::FloatCC::Unordered,value,value))
                }
                "max" | "min" => {
                    let mut value = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    for arg in &args[1..] {
                        let next = eval_expr(b, ex, ptr_ty, arg, ints, reals, bools, func_meta);
                        value = match (value, next) {
                            (CgVal::I64(a), CgVal::I64(c)) => {
                                let condition = b.ins().icmp(if lname == "max" { ir::condcodes::IntCC::SignedGreaterThan } else { ir::condcodes::IntCC::SignedLessThan }, a, c);
                                CgVal::I64(b.ins().select(condition, a, c))
                            }
                            (a,c) => {
                                let a = to_f64(b, a); let c = to_f64(b, c);
                                let condition = b.ins().fcmp(if lname == "max" { ir::condcodes::FloatCC::GreaterThan } else { ir::condcodes::FloatCC::LessThan }, a, c);
                                CgVal::F64(b.ins().select(condition, a, c))
                            }
                        };
                    }
                    value
                }
                "abs" => {
                    let v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    match v {
                        CgVal::I64(x) => {
                            let zero = b.ins().iconst(ir::types::I64, 0);
                            let neg = b.ins().ineg(x);
                            let is_neg =
                                b.ins().icmp(ir::condcodes::IntCC::SignedLessThan, x, zero);
                            let sel = b.ins().select(is_neg, neg, x);
                            CgVal::I64(sel)
                        }
                        CgVal::F64(x) => {
                            let neg = b.ins().fneg(x);
                            let zero = b.ins().f64const(0.0);
                            let is_neg = b.ins().fcmp(ir::condcodes::FloatCC::LessThan, x, zero);
                            let sel = b.ins().select(is_neg, neg, x);
                            CgVal::F64(sel)
                        }
                        CgVal::Bool(x) => {
                            let one = b.ins().iconst(ir::types::I64, 1);
                            let zero = b.ins().iconst(ir::types::I64, 0);
                            let sel = b.ins().select(x, one, zero);
                            CgVal::I64(sel)
                        }
                    }
                }
                "floor" | "ceiling" => {
                    let value=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let value=to_f64(b,value);
                    let rounded=if lname=="floor" {b.ins().floor(value)} else {b.ins().ceil(value)};
                    let kind=if let Some(arg)=args.get(1) {let v=eval_expr(b,ex,ptr_ty,arg,ints,reals,bools,func_meta);to_i64(b,v)} else {b.ins().iconst(ir::types::I64,4)};
                    let ceiling=b.ins().iconst(ir::types::I64,i64::from(lname=="ceiling"));
                    let call=b.ins().call(ex.checked_integer,&[rounded,kind,ceiling]);
                    CgVal::I64(b.inst_results(call)[0])
                }
                "nint" if !func_meta.contains_key(name) => {
                    let v=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let x=to_f64(b,v);
                    // Adding 0.5 to ABS(x) can round an already integral large
                    // value upward. Compare the fractional part instead.
                    let integral=b.ins().trunc(x);
                    let fraction=b.ins().fsub(x,integral);
                    let fraction=b.ins().fabs(fraction);
                    let half=b.ins().f64const(0.5);
                    let away=b.ins().fcmp(ir::condcodes::FloatCC::GreaterThanOrEqual,fraction,half);
                    let one=b.ins().f64const(1.0);
                    let unit=b.ins().fcopysign(one,x);
                    let next=b.ins().fadd(integral,unit);
                    let rounded=b.ins().select(away,next,integral);
                    CgVal::I64(b.ins().fcvt_to_sint(ir::types::I64,rounded))
                }
                "int" => {
                    let v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let iv = to_i64(b, v);
                    CgVal::I64(iv)
                }
                "real" | "dble" => {
                    let v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let fv = to_f64(b, v);
                    CgVal::F64(fv)
                }
                "sign" if args.len() == 2 => {
                    let a = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let sign = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    match (a, sign) {
                        (CgVal::I64(a), CgVal::I64(s)) => {
                            let magnitude = b.ins().iabs(a);
                            let negated = b.ins().ineg(magnitude);
                            let negative = b.ins().icmp_imm(ir::condcodes::IntCC::SignedLessThan, s, 0);
                            CgVal::I64(b.ins().select(negative, negated, magnitude))
                        }
                        (a, s) => {
                            // The sign bit of B, so a negative zero gives a negative result.
                            let a = to_f64(b, a);
                            let s = to_f64(b, s);
                            let magnitude = b.ins().fabs(a);
                            CgVal::F64(b.ins().fcopysign(magnitude, s))
                        }
                    }
                }
                "mod" | "modulo" => {
                    let a = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let bval = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    match (a, bval) {
                        (CgVal::I64(ai), CgVal::I64(bi)) => {
                            let r = b.ins().srem(ai, bi);
                            if lname=="modulo" {
                                let nonzero=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,r,0);
                                let signs=b.ins().bxor(r,bi);
                                let opposite=b.ins().icmp_imm(ir::condcodes::IntCC::SignedLessThan,signs,0);
                                let adjust=b.ins().band(nonzero,opposite);
                                let sum=b.ins().iadd(r,bi);
                                CgVal::I64(b.ins().select(adjust,sum,r))
                            } else {CgVal::I64(r)}
                        }
                        (aa, bb) => {
                            let af = to_f64(b, aa);
                            let bf = to_f64(b, bb);

                            let div = b.ins().fdiv(af, bf);
                            let back = if lname=="modulo" { b.ins().floor(div) } else { b.ins().trunc(div) };
                            let prod = b.ins().fmul(bf, back);
                            let res = b.ins().fsub(af, prod);
                            let res=if lname=="modulo" {let zero=b.ins().f64const(0.0);let is_zero=b.ins().fcmp(ir::condcodes::FloatCC::Equal,res,zero);let signed=b.ins().fcopysign(zero,bf);b.ins().select(is_zero,signed,res)} else {res};
                            CgVal::F64(res)
                        }
                    }
                }
                "btest" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let pos_v = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    let a = to_i64(b, a_v);
                    let pos = to_i64(b, pos_v);
                    let one = b.ins().iconst(ir::types::I64, 1);
                    let shifted = b.ins().ushr(a, pos);
                    let bit = b.ins().band(shifted, one);
                    let zero = b.ins().iconst(ir::types::I64, 0);
                    let cmp = b.ins().icmp(ir::condcodes::IntCC::NotEqual, bit, zero);
                    CgVal::Bool(cmp)
                }
                "iand" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let b_v = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    let a = to_i64(b, a_v);
                    let b2 = to_i64(b, b_v);
                    let r = b.ins().band(a, b2);
                    CgVal::I64(r)
                }
                "ior" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let b_v = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    let a = to_i64(b, a_v);
                    let b2 = to_i64(b, b_v);
                    let r = b.ins().bor(a, b2);
                    CgVal::I64(r)
                }
                "ieor" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let b_v = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    let a = to_i64(b, a_v);
                    let b2 = to_i64(b, b_v);
                    let r = b.ins().bxor(a, b2);
                    CgVal::I64(r)
                }
                "not" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let a = to_i64(b, a_v);
                    let r = b.ins().bnot(a);
                    CgVal::I64(r)
                }
                "matmul" => {
                    let a_v = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let b_v = eval_expr(b, ex, ptr_ty, &args[1], ints, reals, bools, func_meta);
                    let a = to_f64(b, a_v);
                    let b2 = to_f64(b, b_v);
                    let r = b.ins().fmul(a, b2);
                    CgVal::F64(r)
                }
                _ => {
                    if let Some(meta) = func_meta.get(name) {
                        let mut argv = Vec::new();
                        for (i, a) in args.iter().enumerate() {
                            let v = eval_expr(b, ex, ptr_ty, a, ints, reals, bools, func_meta);
                            match meta.param_kinds.get(i).copied().unwrap_or(ParamKind::I64) {
                                ParamKind::I64 => argv.push(to_i64(b, v)),
                                ParamKind::F64 => argv.push(to_f64(b, v)),
                            }
                        }
                        let inst = b.ins().call(meta.fref, &argv);
                        let res = b.inst_results(inst);
                        match meta.ret {
                            RetKind::Bool => {let value=res[0];CgVal::Bool(b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,value,0))},
                            RetKind::Void => CgVal::I64(b.ins().iconst(ir::types::I64, 0)),
                            RetKind::I64 => {
                                if !res.is_empty() {
                                    CgVal::I64(res[0])
                                } else {
                                    CgVal::I64(b.ins().iconst(ir::types::I64, 0))
                                }
                            }
                            RetKind::F64 => {
                                if !res.is_empty() {
                                    CgVal::F64(res[0])
                                } else {
                                    CgVal::F64(b.ins().f64const(0.0))
                                }
                            }
                        }
                    } else {
                        UNRESOLVED_CALLS.with(|calls| calls.borrow_mut().push(name.clone()));
                        CgVal::I64(b.ins().iconst(ir::types::I64, 0))
                    }
                }
            }
        }
    }
}

fn is_character_output(e: &IExpr, chars: &characters::Characters) -> bool {
    match e {
        IExpr::Str(_) => true,
        IExpr::Ident(name) => chars.contains_key(name),
        IExpr::Bin(crate::ast::BinOp::Concat, _, _) => true,
        IExpr::Call(name, _) => matches!(name.to_ascii_lowercase().as_str(),
            "trim" | "adjustl" | "adjustr" | "achar" | "char" | "repeat" | "@dynamic_substring" | "@substring"),
        _ => false,
    }
}

/// REAL values are held as doubles; a value stored in a REAL(4) variable is
/// first rounded to single precision, as the variable can hold no more.
fn store_rounded(b: &mut FunctionBuilder, real_kind: &HashMap<String, bool>, name: &str, value: ir::Value) -> ir::Value {
    if real_kind.get(name) == Some(&false) {
        let narrow = b.ins().fdemote(ir::types::F32, value);
        b.ins().fpromote(ir::types::F64, narrow)
    } else {
        value
    }
}

fn double_expression(e: &IExpr, real_kind: &HashMap<String, bool>) -> bool {
    double_value(e, real_kind, &HashMap::new())
}

/// Whether a REAL expression has DOUBLE PRECISION, including user function
/// results and double-precision literals.
fn double_value(e: &IExpr, real_kind: &HashMap<String, bool>, func_meta: &HashMap<String, FuncMeta>) -> bool {
    match e {
        IExpr::Ident(name) | IExpr::Index(name, _) => *real_kind.get(name).unwrap_or(&false),
        IExpr::RealLit(text) => text.contains(['d', 'D']),
        IExpr::Call(name, args) => {
            // A math intrinsic, imported from the C library, has its argument's kind.
            if crate::intrinsics::math_arity(name).is_some() {
                return args.first().is_some_and(|arg| double_value(arg, real_kind, func_meta));
            }
            if let Some((_, kind)) = crate::real_intrinsics::decode(name) { return kind == 8; }
            if let Some(meta) = func_meta.get(name) { return meta.double; }
            if name.eq_ignore_ascii_case("dble") { return true; }
            if name.eq_ignore_ascii_case("real") { return false; }
            // An array element.
            if let Some(double) = real_kind.get(name) { return *double; }
            args.first().is_some_and(|arg| double_value(arg, real_kind, func_meta))
        }
        IExpr::Bin(_, a, b) => double_value(a, real_kind, func_meta) || double_value(b, real_kind, func_meta),
        IExpr::Un(_, a) => double_value(a, real_kind, func_meta),
        _ => false,
    }
}

/// Whether a REAL expression is known to have kind 4. Formatted output rounds
/// such values to single precision, so an unknown kind counts as kind 8.
fn single_value(e: &IExpr, real_kind: &HashMap<String, bool>, func_meta: &HashMap<String, FuncMeta>) -> bool {
    match e {
        IExpr::Ident(name) | IExpr::Index(name, _) => real_kind.get(name) == Some(&false),
        IExpr::RealLit(text) => !text.contains(['d', 'D']),
        IExpr::Call(name, args) => {
            if crate::intrinsics::math_arity(name).is_some() || matches!(name.as_str(), "abs" | "sign" | "mod" | "modulo") {
                return args.first().is_some_and(|arg| single_value(arg, real_kind, func_meta));
            }
            if let Some((_, kind)) = crate::real_intrinsics::decode(name) { return kind == 4; }
            if let Some(meta) = func_meta.get(name) { return !meta.double; }
            if name.eq_ignore_ascii_case("real") { return args.len() == 1; }
            real_kind.get(name) == Some(&false)
        }
        IExpr::Bin(_, a, b) => !double_value(a, real_kind, func_meta) && !double_value(b, real_kind, func_meta)
            && (single_value(a, real_kind, func_meta) || single_value(b, real_kind, func_meta))
            && [a, b].iter().all(|x| single_value(x, real_kind, func_meta) || matches!(***x, IExpr::IntLit(_))),
        IExpr::Un(_, a) => single_value(a, real_kind, func_meta),
        _ => false,
    }
}

// Formatting tests whether a real has an exact INTEGER(8) representation.
// Conversion must not trap for large finite values, infinities or NaNs. Also
// exclude 2**63: converting i64::MAX back to f64 rounds up to that value.
fn print_integer_candidate(b: &mut FunctionBuilder, value: ir::Value) -> (ir::Value, ir::Value) {
    let integer=b.ins().fcvt_to_sint_sat(ir::types::I64,value);
    let back=b.ins().fcvt_from_sint(ir::types::F64,integer);
    let equal=b.ins().fcmp(ir::condcodes::FloatCC::Equal,value,back);
    let upper=b.ins().f64const(9223372036854775808.0);
    let below=b.ins().fcmp(ir::condcodes::FloatCC::LessThan,value,upper);
    let integral=b.ins().band(equal,below);
    // Negative zero is written as `-0.0`, not as the integer 0.
    let bits=b.ins().bitcast(ir::types::I64,ir::MemFlags::new(),value);
    let negative=b.ins().icmp_imm(ir::condcodes::IntCC::SignedLessThan,bits,0);
    let zero=b.ins().f64const(0.0);
    let is_zero=b.ins().fcmp(ir::condcodes::FloatCC::Equal,value,zero);
    let negative_zero=b.ins().band(negative,is_zero);
    let integral=b.ins().band_not(integral,negative_zero);
    (integer,integral)
}

fn print_numeric_value(b: &mut FunctionBuilder, ex: &Externs, ptr_ty: ir::Type, value: CgVal, double: bool) {
    use ir::types;
    match value {
        CgVal::I64(value) => {
            let text = int_to_str(b, ex, ptr_ty, ex.sprintf, value);
            ex.call_printf(b, &[ex.fmt_s, text]);
        }
        CgVal::F64(value) => {
            let (integer,integral) = print_integer_candidate(b,value);
            let int_slot = b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot, 64, 0));
            let int_text = b.ins().stack_addr(ptr_ty, int_slot, 0);
            ex.call_sprintf(b, &[int_text, ex.fmt_i64_dot0, integer]);
            let real_slot = b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot, 64, 0));
            let real_text = b.ins().stack_addr(ptr_ty, real_slot, 0);
            let digits = b.ins().iconst(types::I32, if double { 15 } else { 6 });
            ex.call_gcvt(b, &[value, digits, real_text]);
            let text = b.ins().select(integral, int_text, real_text);
            ex.call_printf(b, &[ex.fmt_s, text]);
        }
        CgVal::Bool(value) => {
            let text = b.ins().select(value, ex.p_true, ex.p_false);
            ex.call_printf(b, &[ex.fmt_s, text]);
        }
    }
}

fn print_expr(
    b: &mut FunctionBuilder,
    ex: &Externs,
    ptr_ty: ir::Type,
    module: &mut ObjectModule,
    _dc: &mut u32,
    ints: &HashMap<String, Storage>,
    reals: &HashMap<String, Storage>,
    bools: &HashMap<String, Storage>,
    chars: &characters::Characters,
    real_kind: &HashMap<String, bool>,
    i128_last: &HashMap<String, String>,
    func_meta: &HashMap<String, FuncMeta>,
    arrays: &HashMap<String, usize>,
    e: &IExpr,
) -> Result<()> {
    use ir::{types, MemFlags};
    match e {
        IExpr::Str(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            let id_num = GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed);
            let name = format!(".str{}.data", id_num);
            let id = module.declare_data(&name, Linkage::Local, false, false)?;
            let mut dd = DataDescription::new();
            dd.define(bytes.into_boxed_slice());
            module.define_data(id, &dd)?;
            let gv = module.declare_data_in_func(id, b.func);
            let p = b.ins().global_value(ptr_ty, gv);
            ex.call_printf(b, &[ex.fmt_s, p]);
        }
        IExpr::IntLit(s) => {
            if let Ok(v128) = i128::from_str_radix(s, 10) {
                if let Ok(v64) = i64::try_from(v128) {
                    let x = b.ins().iconst(types::I64, v64);
                    let p = int_to_str(b, ex, ptr_ty, ex.sprintf, x);
                    ex.call_printf(b, &[ex.fmt_s, p]);
                } else {
                    let mut bytes = s.as_bytes().to_vec();
                    bytes.push(0);
                    let id_num = GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed);
                    let name = format!(".str{}.data", id_num);
                    let id = module.declare_data(&name, Linkage::Local, false, false)?;
                    let mut dd = DataDescription::new();
                    dd.define(bytes.into_boxed_slice());
                    module.define_data(id, &dd)?;
                    let gv = module.declare_data_in_func(id, b.func);
                    let p = b.ins().global_value(ptr_ty, gv);
                    ex.call_printf(b, &[ex.fmt_s, p]);
                }
            } else {
                let x = b.ins().iconst(types::I64, 0);
                let p = int_to_str(b, ex, ptr_ty, ex.sprintf, x);
                ex.call_printf(b, &[ex.fmt_s, p]);
            }
        }
        IExpr::RealLit(s) => {
            let v = s.parse::<f64>().unwrap_or(0.0);
            let fv = b.ins().f64const(v);
            let (trunc,is_int) = print_integer_candidate(b,fv);
            let buf_int = b.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                64,
                0,
            ));
            let p_int = b.ins().stack_addr(ptr_ty, buf_int, 0);
            ex.call_sprintf(b, &[p_int, ex.fmt_i64_dot0, trunc]);
            let buf_f = b.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                64,
                0,
            ));
            let p_f = b.ins().stack_addr(ptr_ty, buf_f, 0);

            // A value exactly representable in single precision, as a folded
            // REAL constant is, prints as REAL; a longer literal as DOUBLE.
            let single = f64::from(v as f32) == v;
            let nd = b.ins().iconst(types::I32, if s.len() > 8 && !single { 15 } else { 6 });
            ex.call_gcvt(b, &[fv, nd, p_f]);
            let p_sel = b.ins().select(is_int, p_int, p_f);
            ex.call_printf(b, &[ex.fmt_s, p_sel]);
        }
        IExpr::Logical(bv) => {
            let p = if *bv { ex.p_true } else { ex.p_false };
            ex.call_printf(b, &[ex.fmt_s, p]);
        }
        IExpr::Ident(id) => {
            if let Some(txt) = i128_last.get(id) {
                let mut bytes = txt.as_bytes().to_vec();
                bytes.push(0);
                let id_num = GLOBAL_STR_ID.fetch_add(1, Ordering::Relaxed);
                let name = format!(".str{}.data", id_num);
                let idd = module.declare_data(&name, Linkage::Local, false, false)?;
                let mut dd = DataDescription::new();
                dd.define(bytes.into_boxed_slice());
                module.define_data(idd, &dd)?;
                let gv = module.declare_data_in_func(idd, b.func);
                let p = b.ins().global_value(ptr_ty, gv);
                ex.call_printf(b, &[ex.fmt_s, p]);
            } else if let Some(len) = arrays.get(id) {
                for i in 0..*len {
                    if i > 0 {
                        let sp = make_data(module, b, ptr_ty, &mut 0u32, b" ")?;
                        ex.call_printf(b, &[ex.fmt_s, sp]);
                    }
                    let element = IExpr::Index(id.clone(), vec![IExpr::IntLit((i + 1).to_string())]);
                    print_expr(b, ex, ptr_ty, module, _dc, ints, reals, bools, chars,
                        real_kind, i128_last, func_meta, arrays, &element)?;
                }
                return Ok(());
            } else if let Some(ss) = ints.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                let v = b.ins().load(types::I64, MemFlags::new(), a, 0);
                let p = int_to_str(b, ex, ptr_ty, ex.sprintf, v);
                ex.call_printf(b, &[ex.fmt_s, p]);
            } else if let Some(ss) = reals.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                let fv = b.ins().load(types::F64, MemFlags::new(), a, 0);
                let (trunc,is_int) = print_integer_candidate(b,fv);
                let buf_int = b.create_sized_stack_slot(ir::StackSlotData::new(
                    ir::StackSlotKind::ExplicitSlot,
                    64,
                    0,
                ));
                let p_int = b.ins().stack_addr(ptr_ty, buf_int, 0);
                ex.call_sprintf(b, &[p_int, ex.fmt_i64_dot0, trunc]);
                let buf_f = b.create_sized_stack_slot(ir::StackSlotData::new(
                    ir::StackSlotKind::ExplicitSlot,
                    64,
                    0,
                ));
                let p_f = b.ins().stack_addr(ptr_ty, buf_f, 0);
                let nd = b.ins().iconst(
                    types::I32,
                    if *real_kind.get(id).unwrap_or(&false) {
                        15
                    } else {
                        6
                    },
                );
                ex.call_gcvt(b, &[fv, nd, p_f]);
                let p_sel = b.ins().select(is_int, p_int, p_f);
                ex.call_printf(b, &[ex.fmt_s, p_sel]);
            } else if let Some(ss) = bools.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                let v = b.ins().load(types::I8, MemFlags::new(), a, 0);
                let z = b.ins().iconst(types::I8, 0);
                let t = b.ins().icmp(ir::condcodes::IntCC::NotEqual, v, z);
                let sel = b.ins().select(t, ex.p_true, ex.p_false);
                ex.call_printf(b, &[ex.fmt_s, sel]);
            } else if let Some((ss, _len)) = chars.get(id) {
                let a = storage_addr(b, ptr_ty, *ss);
                ex.call_printf(b, &[ex.fmt_s, a]);
            } else {
                let z = b.ins().iconst(types::I64, 0);
                let p = int_to_str(b, ex, ptr_ty, ex.sprintf, z);
                ex.call_printf(b, &[ex.fmt_s, p]);
            }
        }
        IExpr::Index(_name, args) => {
            if args.is_empty() {
                ex.call_printf(b, &[ex.fmt_s, ex.p_nl]);
            } else {
                let idxv = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                let one = b.ins().iconst(ir::types::I64, 1);
                let idx_i = to_i64(b, idxv);
                let adj = b.ins().isub(idx_i, one);
                let off = b.ins().imul_imm(adj, 8);
                if let IExpr::Index(name, _) = e {
                    if let Some(ss) = ints.get(name.as_str()) {
                        let a = storage_addr(b, ptr_ty, *ss);
                        let addr = b.ins().iadd(a, off);
                        let v = b.ins().load(ir::types::I64, MemFlags::new(), addr, 0);
                        let p = int_to_str(b, ex, ptr_ty, ex.sprintf, v);
                        ex.call_printf(b, &[ex.fmt_s, p]);
                    } else if let Some(ss) = reals.get(name.as_str()) {
                        let a = storage_addr(b, ptr_ty, *ss);
                        let addr = b.ins().iadd(a, off);
                        let fv = b.ins().load(ir::types::F64, MemFlags::new(), addr, 0);
                        let (trunc,cmp) = print_integer_candidate(b,fv);
                        let buf_int = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            64,
                            0,
                        ));
                        let p_int = b.ins().stack_addr(ptr_ty, buf_int, 0);
                        ex.call_sprintf(b, &[p_int, ex.fmt_i64_dot0, trunc]);
                        let buf_f = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            64,
                            0,
                        ));
                        let p_f = b.ins().stack_addr(ptr_ty, buf_f, 0);
                        let nd = b.ins().iconst(ir::types::I32, if *real_kind.get(name.as_str()).unwrap_or(&false) { 15 } else { 6 });
                        ex.call_gcvt(b, &[fv, nd, p_f]);
                        let p_sel = b.ins().select(cmp, p_int, p_f);
                        ex.call_printf(b, &[ex.fmt_s, p_sel]);
                    } else if bools.contains_key(name) {
                        let value = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
                        let value = to_bool(b, value);
                        let text = b.ins().select(value, ex.p_true, ex.p_false);
                        ex.call_printf(b, &[ex.fmt_s, text]);
                    } else {
                        let z = b.ins().iconst(ir::types::I64, 0);
                        let p = int_to_str(b, ex, ptr_ty, ex.sprintf, z);
                        ex.call_printf(b, &[ex.fmt_s, p]);
                    }
                }
            }
        }
        IExpr::Bin(op, l, r) => {
            if let crate::ast::BinOp::Concat = op {
                // Direct output can stream both operands without a temporary string.
                for part in [l, r] {
                    print_expr(b, ex, ptr_ty, module, _dc, ints, reals, bools, chars,
                        real_kind, i128_last, func_meta, arrays, part)?;
                }
                return Ok(());
            }
            let val = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
            print_numeric_value(b, ex, ptr_ty, val, double_value(e, real_kind, func_meta));
        }
        IExpr::Un(_, _) => {
            let val = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
            print_numeric_value(b, ex, ptr_ty, val, double_value(e, real_kind, func_meta));
        }
        IExpr::Call(name, args) => {
            let lname = name.to_ascii_lowercase();
            if crate::intrinsics::math_arity(name).is_some() || crate::intrinsics::integer_rounding(name) || crate::intrinsics::real_predicate(name)
                || crate::bit_intrinsics::decode(name).is_some() || crate::real_intrinsics::decode(name).is_some()
                || matches!(lname.as_str(), "abs" | "int" | "nint" | "real" | "dble" | "max" | "min" | "mod" | "modulo" | "sign")
            {
                let value = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
                print_numeric_value(b, ex, ptr_ty, value, double_expression(e, real_kind));
                return Ok(());
            }
            if (lname == "trim" || lname == "adjustl" || lname == "adjustr") && args.len() == 1 {
                use ir::condcodes::IntCC;

                let mut cur_expr = &args[0];
                let mut chain: Vec<String> = Vec::new();
                while let IExpr::Call(nm, a) = cur_expr {
                    chain.push(nm.to_ascii_lowercase());
                    if a.is_empty() {
                        break;
                    }
                    cur_expr = &a[0];
                }
                match cur_expr {
                    IExpr::Ident(id) => {
                        if let Some((ss, len)) = chars.get(id) {
                            let a = storage_addr(b, ptr_ty, *ss);
                            if !chain.is_empty() {
                                ex.call_printf(b, &[ex.fmt_s, a]);
                                return Ok(());
                            }

                            let tmp_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                (*len + 1) as u32,
                                0,
                            ));
                            let tmp_ptr = b.ins().stack_addr(ptr_ty, tmp_slot, 0);

                            let zero_i64 = b.ins().iconst(ir::types::I64, 0);
                            let one_i64 = b.ins().iconst(ir::types::I64, 1);
                            let len_i64 = b.ins().iconst(ir::types::I64, *len as i64);
                            let space_i8 = b.ins().iconst(ir::types::I8, 32);
                            let term_i8 = b.ins().iconst(ir::types::I8, 0);
                            let zero_i8 = b.ins().iconst(ir::types::I8, 0);

                            b.ins()
                                .store(ir::MemFlags::new(), term_i8, tmp_ptr, *len as i32);

                            if lname == "trim" {
                                let idx_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let last_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));

                                let idx_addr = b.ins().stack_addr(ptr_ty, idx_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, idx_addr, 0);
                                let neg1 = b.ins().iconst(ir::types::I64, -1);
                                let last_addr = b.ins().stack_addr(ptr_ty, last_slot, 0);
                                b.ins().store(ir::MemFlags::new(), neg1, last_addr, 0);

                                let loop_blk = b.create_block();
                                let body_blk = b.create_block();
                                let after_blk = b.create_block();

                                b.ins().jump(loop_blk, &[]);
                                b.switch_to_block(loop_blk);
                                let cur =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), idx_addr, 0);
                                let done =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, cur, len_i64);
                                b.ins().brif(done, after_blk, &[], body_blk, &[]);
                                b.switch_to_block(body_blk);
                                let p_cur = b.ins().iadd(a, cur);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), p_cur, 0);
                                let is_null = b.ins().icmp(IntCC::Equal, ch, term_i8);

                                let character_blk = b.create_block();
                                b.ins().brif(is_null, after_blk, &[], character_blk, &[]);
                                b.switch_to_block(character_blk);

                                let is_space = b.ins().icmp(IntCC::Equal, ch, space_i8);

                                let neg_cmp = b.ins().icmp(IntCC::Equal, is_space, zero_i8);

                                let then_blk = b.create_block();
                                let cont_blk = b.create_block();
                                b.ins().brif(neg_cmp, then_blk, &[], cont_blk, &[]);
                                b.switch_to_block(then_blk);
                                b.ins().store(ir::MemFlags::new(), cur, last_addr, 0);
                                b.ins().jump(cont_blk, &[]);
                                b.switch_to_block(cont_blk);

                                let nxt = b.ins().iadd(cur, one_i64);
                                b.ins().store(ir::MemFlags::new(), nxt, idx_addr, 0);
                                b.ins().jump(loop_blk, &[]);
                                b.switch_to_block(after_blk);

                                let lastv =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), last_addr, 0);
                                let length = b.ins().iadd(lastv, one_i64);
                                let terminator = b.ins().iadd(tmp_ptr, length);
                                b.ins().store(ir::MemFlags::new(), term_i8, terminator, 0);
                                let is_neg_last =
                                    b.ins().icmp(IntCC::SignedLessThan, lastv, zero_i64);

                                let copy_blk = b.create_block();
                                let done_copy_blk = b.create_block();
                                b.ins().brif(is_neg_last, done_copy_blk, &[], copy_blk, &[]);

                                b.switch_to_block(copy_blk);
                                let i_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let i_addr = b.ins().stack_addr(ptr_ty, i_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, i_addr, 0);
                                let loop2 = b.create_block();
                                let body2 = b.create_block();
                                let after2 = b.create_block();
                                b.ins().jump(loop2, &[]);
                                b.switch_to_block(loop2);
                                let iv =
                                    b.ins().load(ir::types::I64, ir::MemFlags::new(), i_addr, 0);
                                let cmp2 = b.ins().icmp(IntCC::SignedGreaterThan, iv, lastv);
                                b.ins().brif(cmp2, after2, &[], body2, &[]);
                                b.switch_to_block(body2);
                                let p_src = b.ins().iadd(a, iv);
                                let ch2 =
                                    b.ins().load(ir::types::I8, ir::MemFlags::new(), p_src, 0);
                                let p_dst = b.ins().iadd(tmp_ptr, iv);
                                b.ins().store(ir::MemFlags::new(), ch2, p_dst, 0);
                                let ivn = b.ins().iadd(iv, one_i64);
                                b.ins().store(ir::MemFlags::new(), ivn, i_addr, 0);
                                b.ins().jump(loop2, &[]);
                                b.switch_to_block(after2);
                                b.ins().jump(done_copy_blk, &[]);
                                b.switch_to_block(done_copy_blk);

                                for block in [loop_blk, body_blk, after_blk, character_blk, then_blk,
                                    cont_blk, copy_blk, done_copy_blk, loop2, body2, after2] {
                                    b.seal_block(block);
                                }
                                ex.call_printf(b, &[ex.fmt_s, tmp_ptr]);
                                return Ok(());
                            } else if lname == "adjustl" {
                                let idx_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let idx_addr2 = b.ins().stack_addr(ptr_ty, idx_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, idx_addr2, 0);
                                let loop_blk = b.create_block();
                                let body_blk = b.create_block();
                                let after_blk = b.create_block();
                                b.ins().jump(loop_blk, &[]);
                                b.switch_to_block(loop_blk);
                                let cur =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), idx_addr2, 0);
                                let done =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, cur, len_i64);
                                b.ins().brif(done, after_blk, &[], body_blk, &[]);
                                b.switch_to_block(body_blk);
                                let p_cur = b.ins().iadd(a, cur);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), p_cur, 0);
                                let is_null = b.ins().icmp(IntCC::Equal, ch, term_i8);
                                let cont_blk2 = b.create_block();
                                b.ins().brif(is_null, after_blk, &[], cont_blk2, &[]);
                                b.switch_to_block(cont_blk2);
                                let is_space = b.ins().icmp(IntCC::Equal, ch, space_i8);

                                let next_blk = b.create_block();
                                b.ins().brif(is_space, next_blk, &[], after_blk, &[]);
                                b.switch_to_block(next_blk);
                                let nxt = b.ins().iadd(cur, one_i64);
                                b.ins().store(ir::MemFlags::new(), nxt, idx_addr2, 0);
                                b.ins().jump(loop_blk, &[]);

                                b.switch_to_block(after_blk);
                                let start =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), idx_addr2, 0);

                                let cnt_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let cnt_addr = b.ins().stack_addr(ptr_ty, cnt_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, cnt_addr, 0);
                                let loop_c = b.create_block();
                                let body_c = b.create_block();
                                let after_c = b.create_block();
                                b.ins().jump(loop_c, &[]);
                                b.switch_to_block(loop_c);
                                let cur_cnt =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), cnt_addr, 0);
                                let pos = b.ins().iadd(start, cur_cnt);
                                let cmp_end =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, pos, len_i64);
                                b.ins().brif(cmp_end, after_c, &[], body_c, &[]);
                                b.switch_to_block(body_c);
                                let p_src = b.ins().iadd(a, pos);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), p_src, 0);
                                let is_null = b.ins().icmp(IntCC::Equal, ch, term_i8);
                                b.ins().brif(is_null, after_c, &[], after_c, &[]);
                                let p_dst = b.ins().iadd(tmp_ptr, cur_cnt);
                                b.ins().store(ir::MemFlags::new(), ch, p_dst, 0);
                                let nxt = b.ins().iadd(cur_cnt, one_i64);
                                b.ins().store(ir::MemFlags::new(), nxt, cnt_addr, 0);
                                b.ins().jump(loop_c, &[]);
                                b.switch_to_block(after_c);

                                let curc =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), cnt_addr, 0);
                                let p_term = b.ins().iadd(tmp_ptr, curc);
                                b.ins().store(ir::MemFlags::new(), term_i8, p_term, 0);
                                ex.call_printf(b, &[ex.fmt_s, tmp_ptr]);
                                return Ok(());
                            } else {
                                let idx_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let idx_addr3 = b.ins().stack_addr(ptr_ty, idx_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, idx_addr3, 0);

                                let loop_blk = b.create_block();
                                let body_blk = b.create_block();
                                let after_blk = b.create_block();
                                b.ins().jump(loop_blk, &[]);
                                b.switch_to_block(loop_blk);
                                let cur =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), idx_addr3, 0);
                                let done =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, cur, len_i64);
                                b.ins().brif(done, after_blk, &[], body_blk, &[]);
                                b.switch_to_block(body_blk);
                                let p_cur = b.ins().iadd(a, cur);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), p_cur, 0);
                                let is_null = b.ins().icmp(IntCC::Equal, ch, term_i8);
                                let cont_blk3 = b.create_block();
                                b.ins().brif(is_null, after_blk, &[], cont_blk3, &[]);
                                b.switch_to_block(cont_blk3);
                                let is_space = b.ins().icmp(IntCC::Equal, ch, space_i8);
                                let next_blk = b.create_block();
                                b.ins().brif(is_space, next_blk, &[], after_blk, &[]);
                                b.switch_to_block(next_blk);
                                let nxt = b.ins().iadd(cur, one_i64);
                                b.ins().store(ir::MemFlags::new(), nxt, idx_addr3, 0);
                                b.ins().jump(loop_blk, &[]);
                                b.switch_to_block(after_blk);
                                let start =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), idx_addr3, 0);

                                let cnt_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let cnt_addr2 = b.ins().stack_addr(ptr_ty, cnt_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, cnt_addr2, 0);
                                let loop_c = b.create_block();
                                let body_c = b.create_block();
                                let after_c = b.create_block();
                                b.ins().jump(loop_c, &[]);
                                b.switch_to_block(loop_c);
                                let cur_cnt =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), cnt_addr2, 0);
                                let pos = b.ins().iadd(start, cur_cnt);
                                let cmp_end =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, pos, len_i64);
                                b.ins().brif(cmp_end, after_c, &[], body_c, &[]);
                                b.switch_to_block(body_c);
                                let p_src = b.ins().iadd(a, pos);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), p_src, 0);
                                let is_null = b.ins().icmp(IntCC::Equal, ch, term_i8);
                                b.ins().brif(is_null, after_c, &[], after_c, &[]);
                                let p_dst = b.ins().iadd(tmp_ptr, cur_cnt);
                                b.ins().store(ir::MemFlags::new(), ch, p_dst, 0);
                                let nxt = b.ins().iadd(cur_cnt, one_i64);
                                b.ins().store(ir::MemFlags::new(), nxt, cnt_addr2, 0);
                                b.ins().jump(loop_c, &[]);
                                b.switch_to_block(after_c);

                                let content_len =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), cnt_addr2, 0);

                                let shift = b.ins().isub(len_i64, content_len);

                                let i_slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));
                                let i_addr2 = b.ins().stack_addr(ptr_ty, i_slot, 0);
                                b.ins().store(ir::MemFlags::new(), zero_i64, i_addr2, 0);
                                let loop_s = b.create_block();
                                let body_s = b.create_block();
                                let after_s = b.create_block();
                                b.ins().jump(loop_s, &[]);
                                b.switch_to_block(loop_s);
                                let iv =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), i_addr2, 0);
                                let cmp_s =
                                    b.ins().icmp(IntCC::SignedGreaterThanOrEqual, iv, shift);
                                b.ins().brif(cmp_s, after_s, &[], body_s, &[]);
                                b.switch_to_block(body_s);
                                let p_dst = b.ins().iadd(tmp_ptr, iv);
                                b.ins().store(ir::MemFlags::new(), space_i8, p_dst, 0);
                                let ivn = b.ins().iadd(iv, one_i64);
                                b.ins().store(ir::MemFlags::new(), ivn, i_addr2, 0);
                                b.ins().jump(loop_s, &[]);
                                b.switch_to_block(after_s);

                                let copy_idx = b.create_sized_stack_slot(ir::StackSlotData::new(
                                    ir::StackSlotKind::ExplicitSlot,
                                    8,
                                    0,
                                ));

                                let one_i64_pos = b.ins().iconst(ir::types::I64, 1);
                                let cl_minus1 = b.ins().isub(content_len, one_i64_pos);
                                let copy_addr = b.ins().stack_addr(ptr_ty, copy_idx, 0);
                                b.ins().store(ir::MemFlags::new(), cl_minus1, copy_addr, 0);
                                let loop_cp = b.create_block();
                                let body_cp = b.create_block();
                                let after_cp = b.create_block();
                                b.ins().jump(loop_cp, &[]);
                                b.switch_to_block(loop_cp);
                                let ci =
                                    b.ins()
                                        .load(ir::types::I64, ir::MemFlags::new(), copy_addr, 0);
                                let cmp_cp = b.ins().icmp(IntCC::SignedLessThan, ci, zero_i64);
                                b.ins().brif(cmp_cp, after_cp, &[], body_cp, &[]);
                                b.switch_to_block(body_cp);

                                let srcp = b.ins().iadd(tmp_ptr, ci);
                                let ch = b.ins().load(ir::types::I8, ir::MemFlags::new(), srcp, 0);

                                let dest_index = b.ins().iadd(shift, ci);
                                let destp = b.ins().iadd(tmp_ptr, dest_index);
                                b.ins().store(ir::MemFlags::new(), ch, destp, 0);

                                let dec = b.ins().isub(ci, one_i64_pos);
                                b.ins().store(ir::MemFlags::new(), dec, copy_addr, 0);
                                b.ins().jump(loop_cp, &[]);
                                b.switch_to_block(after_cp);

                                let p_term = b.ins().iadd(tmp_ptr, len_i64);
                                b.ins().store(ir::MemFlags::new(), term_i8, p_term, 0);
                                ex.call_printf(b, &[ex.fmt_s, tmp_ptr]);
                                return Ok(());
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(meta) = func_meta.get(name) {
                let mut argv = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    let v = eval_expr(b, ex, ptr_ty, a, ints, reals, bools, func_meta);
                    match meta.param_kinds.get(i).copied().unwrap_or(ParamKind::I64) {
                        ParamKind::I64 => argv.push(to_i64(b, v)),
                        ParamKind::F64 => argv.push(to_f64(b, v)),
                    }
                }
                let inst = b.ins().call(meta.fref, &argv);
                let res = b.inst_results(inst);
                match meta.ret {
                    RetKind::Bool => {let value=res[0];let yes=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,value,0);let text=b.ins().select(yes,ex.p_true,ex.p_false);ex.call_printf(b,&[ex.fmt_s,text]);}
                    RetKind::Void => {}
                    RetKind::I64 => {
                        if !res.is_empty() {
                            let p = int_to_str(b, ex, ptr_ty, ex.sprintf, res[0]);
                            ex.call_printf(b, &[ex.fmt_s, p]);
                        }
                    }
                    RetKind::F64 => {
                        if !res.is_empty() {
                            let x = res[0];
                            let (trunc,is_int) = print_integer_candidate(b,x);
                            let buf_int = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                64,
                                0,
                            ));
                            let p_int = b.ins().stack_addr(ptr_ty, buf_int, 0);
                            ex.call_sprintf(b, &[p_int, ex.fmt_i64_dot0, trunc]);
                            let buf_f = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                64,
                                0,
                            ));
                            let p_f = b.ins().stack_addr(ptr_ty, buf_f, 0);
                            let nd = b.ins().iconst(ir::types::I32, if meta.double { 15 } else { 6 });
                            ex.call_gcvt(b, &[x, nd, p_f]);
                            let p_sel = b.ins().select(is_int, p_int, p_f);
                            ex.call_printf(b, &[ex.fmt_s, p_sel]);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn gen_stmts(
    b: &mut FunctionBuilder,
    ex: &Externs,
    ptr_ty: ir::Type,
    module: &mut ObjectModule,
    dc: &mut u32,
    stmts: &[IStmt],
    ints: &mut HashMap<String, Storage>,
    reals: &mut HashMap<String, Storage>,
    bools: &mut HashMap<String, Storage>,
    chars: &mut characters::Characters,
    real_kind: &mut HashMap<String, bool>,
    i128_vars: &mut HashSet<String>,
    i128_last: &mut HashMap<String, String>,
    func_meta: &HashMap<String, FuncMeta>,
    current_fn: Option<&str>,
    current_loop_after: Option<ir::Block>,
    current_loop_continue: Option<ir::Block>,
    arrays: &mut HashMap<String, usize>,
    tokens: Option<&[crate::lexer::Token]>,
) -> Result<bool> {
    use ir::{types, MemFlags};
    for st in stmts {
        // Fragments share scalar/array rendering with PRINT but do not delimit a record.
        let fragment = matches!(st, IStmt::Call(name, _) if name == "@print_fragment");
        let fragment_print;
        let st = if let IStmt::Call(name, items) = st {
            if name == "@print_fragment" {
                fragment_print = IStmt::Print(items.clone());
                &fragment_print
            } else { st }
        } else { st };
        match st {
            IStmt::Print(items) => {
                let space = make_data(module, b, ptr_ty, dc, b" ")?;
                let mut previous_character = None;
                for e in items.iter() {
                    let character = is_character_output(e, chars);
                    if !fragment && !crate::utils::list_directed_separator(previous_character, character).is_empty() {
                        ex.call_printf(b, &[ex.fmt_s, space]);
                    }
                    if character {
                        let source = characters::view(module, b, ptr_ty, dc, chars, ints, e)?;
                        let text = characters::terminated(module, b, ptr_ty, dc, source, chars)?;
                        ex.call_printf(b, &[ex.fmt_s, text]);
                    } else { print_expr(
                        b, ex, ptr_ty, module, dc, ints, reals, bools, chars, real_kind, i128_last,
                        func_meta, arrays, e,
                    )?; }
                    previous_character = Some(character);
                }
                if !fragment { ex.call_printf(b, &[ex.fmt_s, ex.p_nl]); }
            }
            IStmt::DeclVar(kind, name, dims) => {
                // A constant size expression, such as 10*6, is folded here;
                // only a literal size gives an array its storage below.
                let folded: Vec<IExpr>;
                let dims = if dims.len() == 1 && !matches!(dims[0], IExpr::IntLit(_)) {
                    match constant_size(&dims[0]) {
                        Some(n) => { folded = vec![IExpr::IntLit(n.to_string())]; &folded }
                        None => dims,
                    }
                } else { dims };
                if let crate::ast::TypeSpec::Integer(Some(k)) = kind {
                    INTEGER_KINDS.with(|kinds| kinds.borrow_mut().insert(name.clone(), *k));
                }
                if ints.contains_key(name)
                    || reals.contains_key(name)
                    || bools.contains_key(name)
                    || chars.contains_key(name)
                    || i128_vars.contains(name)
                {
                    continue;
                }
                match kind {
                    crate::ast::TypeSpec::Referenced { .. } | crate::ast::TypeSpec::CharacterLength(_) | crate::ast::TypeSpec::CharacterAssumed | crate::ast::TypeSpec::RealKind(_) | crate::ast::TypeSpec::LogicalKind(_) | crate::ast::TypeSpec::IntegerKind(_) | crate::ast::TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                    crate::ast::TypeSpec::Integer(Some(16)) => {
                        i128_vars.insert(name.clone());
                        i128_last.insert(name.clone(), "0".into());
                    }
                    crate::ast::TypeSpec::Integer(_) => {
                        if dims.len() == 1 {
                            if let IExpr::IntLit(s) = &dims[0] {
                                if let Ok(len128) = i128::from_str_radix(s, 10) {
                                    let len = usize::try_from(len128).unwrap_or(0);
                                    if len > 0 {
                                        let ss = array_storage(b, module, len, current_fn.is_none())?;
                                        ints.insert(name.clone(), ss.into());
                                        arrays.insert(name.clone(), len);
                                        continue;
                                    }
                                }
                            }
                        }
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        ints.insert(name.clone(), ss.into());
                    }
                    crate::ast::TypeSpec::Real => {
                        if dims.len() == 1 {
                            if let IExpr::IntLit(s) = &dims[0] {
                                if let Ok(len128) = i128::from_str_radix(s, 10) {
                                    let len = usize::try_from(len128).unwrap_or(0);
                                    if len > 0 {
                                        let ss = array_storage(b, module, len, current_fn.is_none())?;
                                        reals.insert(name.clone(), ss.into());
                                        real_kind.insert(name.clone(), false);
                                        arrays.insert(name.clone(), len);
                                        continue;
                                    }
                                }
                            }
                        }
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        reals.insert(name.clone(), ss.into());
                        real_kind.insert(name.clone(), false);
                    }
                    crate::ast::TypeSpec::DoublePrecision => {
                        if dims.len() == 1 {
                            if let IExpr::IntLit(s) = &dims[0] {
                                if let Ok(len128) = i128::from_str_radix(s, 10) {
                                    let len = usize::try_from(len128).unwrap_or(0);
                                    if len > 0 {
                                        let ss = array_storage(b, module, len, current_fn.is_none())?;
                                        reals.insert(name.clone(), ss.into());
                                        real_kind.insert(name.clone(), true);
                                        arrays.insert(name.clone(), len);
                                        continue;
                                    }
                                }
                            }
                        }
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        reals.insert(name.clone(), ss.into());
                        real_kind.insert(name.clone(), true);
                    }
                    crate::ast::TypeSpec::Character(len_opt) => {
                        let len = len_opt.unwrap_or(1);
                        let size = len.checked_add(1).context("CHARACTER storage size overflow")?;
                        let storage = fixed_storage(b, module, size, current_fn.is_none())?;
                        chars.insert(name.clone(), (storage, len));
                        let address=storage_addr(b,ptr_ty,storage);
                        let pointer=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,8,0));
                        let value=if ptr_ty==types::I64 {address} else {b.ins().uextend(types::I64,address)};
                        b.ins().stack_store(value,pointer,0);
                        ints.insert(format!("@character_pointer:{name}"),pointer.into());
                    }
                    crate::ast::TypeSpec::Logical => {
                        if let [IExpr::IntLit(length)] = dims.as_slice() {
                            let len: usize = length.parse()?;
                            let ss = array_storage(b, module, len.max(1), current_fn.is_none())?;
                            bools.insert(name.clone(), ss.into());
                            arrays.insert(name.clone(), len);
                            continue;
                        }
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            1,
                            0,
                        ));
                        bools.insert(name.clone(), ss.into());
                    }
                }
            }
            IStmt::AssignStr(name, s) => {
                if let Some((ss, len)) = chars.get(name) {
                    let bytes = s.as_bytes();
                    let a = storage_addr(b, ptr_ty, *ss);
                    for i in 0..*len {
                        let byte = if i < bytes.len() { bytes[i] } else { 32 };
                        let v = b.ins().iconst(ir::types::I8, byte as i64);
                        b.ins().store(MemFlags::new(), v, a, i as i32);
                    }
                    let term = b.ins().iconst(ir::types::I8, 0);
                    b.ins().store(MemFlags::new(), term, a, *len as i32);
                }
            }
            IStmt::AssignIntLit(name, s) => {
                if i128_vars.contains(name) {
                    i128_last.insert(name.clone(), s.clone());
                }
                if let Some(ss) = ints.get(name) {
                    if let Ok(v128) = i128::from_str_radix(s, 10) {
                        let v = i64::try_from(v128).unwrap_or(0);
                        let a = storage_addr(b, ptr_ty, *ss);
                        let iv = b.ins().iconst(types::I64, v);
                        b.ins().store(MemFlags::new(), iv, a, 0);
                    }
                } else if let Some(ss)=reals.get(name) {
                    let value=b.ins().f64const(s.parse::<i128>()? as f64);
                    let address=storage_addr(b,ptr_ty,*ss);
                    b.ins().store(MemFlags::new(),value,address,0);
                }
            }
            IStmt::AssignRealLit(name, s) => {
                if let Some(ss) = reals.get(name) {
                    if let Ok(f) = s.parse::<f64>() {
                        let a = storage_addr(b, ptr_ty, *ss);
                        let f = if real_kind.get(name) == Some(&false) { f64::from(f as f32) } else { f };
                        let fv = b.ins().f64const(f);
                        b.ins().store(MemFlags::new(), fv, a, 0);
                    }
                }
            }
            IStmt::AssignBool(name, vb) => {
                if let Some(ss) = bools.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let v = b.ins().iconst(ir::types::I8, if *vb { 1 } else { 0 });
                    b.ins().store(MemFlags::new(), v, a, 0);
                }
            }
            IStmt::AssignExpr(name, e) => {
                let v = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
                if let Some(ss) = ints.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let iv = to_i64(b, v);
                    b.ins().store(MemFlags::new(), iv, a, 0);
                } else if let Some(ss) = reals.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let fv = to_f64(b, v);
                    let fv = store_rounded(b, real_kind, name, fv);
                    b.ins().store(MemFlags::new(), fv, a, 0);
                } else if let Some(ss) = bools.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let bv = to_bool(b, v);
                    let one = b.ins().iconst(ir::types::I8, 1);
                    let zero = b.ins().iconst(ir::types::I8, 0);
                    let as8 = b.ins().select(bv, one, zero);
                    b.ins().store(MemFlags::new(), as8, a, 0);
                }
            }
            IStmt::AssignIndex(name, inds, val) => {
                if inds.is_empty() {
                    continue;
                }
                let idxv = eval_expr(b, ex, ptr_ty, &inds[0], ints, reals, bools, func_meta);

                let idx_i = to_i64(b, idxv);
                let one = b.ins().iconst(ir::types::I64, 1);
                let _zero = b.ins().iconst(ir::types::I64, 0);
                let adj = b.ins().isub(idx_i, one);

                let off = b.ins().imul_imm(adj, 8);

                if let Some(ss) = ints.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let v = eval_expr(b, ex, ptr_ty, val, ints, reals, bools, func_meta);
                    let iv = to_i64(b, v);
                    let addr = b.ins().iadd(a, off);
                    b.ins().store(MemFlags::new(), iv, addr, 0);
                } else if let Some(ss) = reals.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let v = eval_expr(b, ex, ptr_ty, val, ints, reals, bools, func_meta);
                    let fv = to_f64(b, v);
                    let fv = store_rounded(b, real_kind, name, fv);
                    let addr = b.ins().iadd(a, off);
                    b.ins().store(MemFlags::new(), fv, addr, 0);
                } else if let Some(ss) = bools.get(name) {
                    let a = storage_addr(b, ptr_ty, *ss);
                    let value = eval_expr(b, ex, ptr_ty, val, ints, reals, bools, func_meta);
                    let value = to_bool(b, value);
                    let addr = b.ins().iadd(a, off);
                    b.ins().store(MemFlags::new(), value, addr, 0);
                }
            }
            IStmt::AssignIdent(dst, src) => {
                // LOGICAL dummy arguments use I64 ABI cells, while local
                // LOGICAL variables use byte cells. Convert the value rather
                // than requiring source and destination storage maps to match.
                let value=eval_expr(b,ex,ptr_ty,&IExpr::Ident(src.clone()),ints,reals,bools,func_meta);
                if let Some(slot)=ints.get(dst) {
                    let address=storage_addr(b,ptr_ty,*slot);let value=to_i64(b,value);
                    b.ins().store(MemFlags::new(),value,address,0);
                } else if let Some(slot)=reals.get(dst) {
                    let address=storage_addr(b,ptr_ty,*slot);let value=to_f64(b,value);
                    let value=store_rounded(b,real_kind,dst,value);
                    b.ins().store(MemFlags::new(),value,address,0);
                } else if let Some(slot)=bools.get(dst) {
                    let address=storage_addr(b,ptr_ty,*slot);let value=to_bool(b,value);
                    let one=b.ins().iconst(types::I8,1);let zero=b.ins().iconst(types::I8,0);
                    let value=b.ins().select(value,one,zero);
                    b.ins().store(MemFlags::new(),value,address,0);
                }
            }
            IStmt::Return(expr_opt) => {
                if let Some(e) = expr_opt {
                    let v = eval_expr(b, ex, ptr_ty, e, ints, reals, bools, func_meta);
                    match v {
                        CgVal::I64(x) => {
                            b.ins().return_(&[x]);
                        }
                        CgVal::F64(x) => {
                            b.ins().return_(&[x]);
                        }
                        CgVal::Bool(x) => {
                            let one = b.ins().iconst(ir::types::I64, 1);
                            let zero = b.ins().iconst(ir::types::I64, 0);
                            let as_i = b.ins().select(x, one, zero);
                            b.ins().return_(&[as_i]);
                        }
                    }
                } else {
                    if let Some(fname) = current_fn {
                        use ir::MemFlags;
                        if let Some(ss) = ints.get(fname) {
                            let a = storage_addr(b, ptr_ty, *ss);
                            let rv = b.ins().load(ir::types::I64, MemFlags::new(), a, 0);
                            b.ins().return_(&[rv]);
                        } else if let Some(ss) = reals.get(fname) {
                            let a = storage_addr(b, ptr_ty, *ss);
                            let rv = b.ins().load(ir::types::F64, MemFlags::new(), a, 0);
                            b.ins().return_(&[rv]);
                        } else if let Some(ss) = bools.get(fname) {
                            let a = storage_addr(b, ptr_ty, *ss);
                            let bv = b.ins().load(ir::types::I8, MemFlags::new(), a, 0);
                            let z = b.ins().iconst(ir::types::I8, 0);
                            let t = b.ins().icmp(ir::condcodes::IntCC::NotEqual, bv, z);
                            let one = b.ins().iconst(ir::types::I64, 1);
                            let zero = b.ins().iconst(ir::types::I64, 0);
                            let as_i = b.ins().select(t, one, zero);
                            b.ins().return_(&[as_i]);
                        } else {
                            b.ins().return_(&[]);
                        }
                    } else {
                        b.ins().return_(&[]);
                    }
                }
                return Ok(true);
            }
            IStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let c = eval_expr(b, ex, ptr_ty, cond, ints, reals, bools, func_meta);
                let cb = to_bool(b, c);
                let then_blk = b.create_block();
                let else_blk = b.create_block();
                let cont_blk = b.create_block();
                b.ins().brif(cb, then_blk, &[], else_blk, &[]);
                b.seal_block(then_blk);
                b.seal_block(else_blk);
                b.switch_to_block(then_blk);
                let tr = gen_stmts(
                    b,
                    ex,
                    ptr_ty,
                    module,
                    dc,
                    then_body,
                    ints,
                    reals,
                    bools,
                    chars,
                    real_kind,
                    i128_vars,
                    i128_last,
                    func_meta,
                    current_fn,
                    current_loop_after,
                    current_loop_continue,
                    arrays,
                    tokens,
                )?;
                if !tr { b.ins().jump(cont_blk, &[]); }
                b.switch_to_block(else_blk);
                let mut er = false;
                if let Some(eb) = else_body {
                    er = gen_stmts(
                        b,
                        ex,
                        ptr_ty,
                        module,
                        dc,
                        eb,
                        ints,
                        reals,
                        bools,
                        chars,
                        real_kind,
                        i128_vars,
                        i128_last,
                        func_meta,
                        current_fn,
                        current_loop_after,
                        current_loop_continue,
                        arrays,
                        tokens,
                    )?;
                }
                if !er { b.ins().jump(cont_blk, &[]); }
                if tr && er { return Ok(true); }
                b.seal_block(cont_blk);
                b.switch_to_block(cont_blk);
            }
            IStmt::Block { body } => {
                let r = gen_stmts(
                    b,
                    ex,
                    ptr_ty,
                    module,
                    dc,
                    &body,
                    ints,
                    reals,
                    bools,
                    chars,
                    real_kind,
                    i128_vars,
                    i128_last,
                    func_meta,
                    current_fn,
                    current_loop_after,
                    current_loop_continue,
                    arrays,
                    tokens,
                )?;
                if r {
                    return Ok(true);
                }
            }
            IStmt::Exit => {
                if let Some(after) = current_loop_after {
                    b.ins().jump(after, &[]);

                    let nb = b.create_block();
                    b.switch_to_block(nb);
                    b.seal_block(nb);
                    continue;
                }
                return Err(anyhow::anyhow!("EXIT used outside of a loop"));
            }
            IStmt::Cycle => {
                if let Some(cont) = current_loop_continue {
                    b.ins().jump(cont, &[]);
                    let nb = b.create_block();
                    b.switch_to_block(nb);
                    b.seal_block(nb);
                    continue;
                }
                return Err(anyhow::anyhow!("CYCLE used outside of a loop"));
            }

            IStmt::Do {
                var,
                start,
                end,
                body,
            } => {
                if let Some(ss) = ints.get(var) {
                    let sv = eval_expr(b, ex, ptr_ty, start, ints, reals, bools, func_meta);
                    let ev = eval_expr(b, ex, ptr_ty, end, ints, reals, bools, func_meta);
                    let si = to_i64(b, sv);
                    let ei = to_i64(b, ev);
                    let a = storage_addr(b, ptr_ty, *ss);
                    b.ins().store(MemFlags::new(), si, a, 0);
                    let loop_blk = b.create_block();
                    let body_blk = b.create_block();
                    let continue_blk = b.create_block();
                    let after_blk = b.create_block();

                    b.ins().jump(loop_blk, &[]);

                    b.switch_to_block(loop_blk);
                    let cur = b.ins().load(types::I64, MemFlags::new(), a, 0);
                    let cmp = b
                        .ins()
                        .icmp(ir::condcodes::IntCC::SignedGreaterThan, cur, ei);
                    b.ins().brif(cmp, after_blk, &[], body_blk, &[]);


                    b.switch_to_block(body_blk);
                    b.seal_block(body_blk);
                    let tr = gen_stmts(
                        b,
                        ex,
                        ptr_ty,
                        module,
                        dc,
                        &body,
                        ints,
                        reals,
                        bools,
                        chars,
                        real_kind,
                        i128_vars,
                        i128_last,
                        func_meta,
                        current_fn,
                        Some(after_blk),
                        Some(continue_blk),
                        arrays,
                        tokens,
                    )?;
                    if !tr { b.ins().jump(continue_blk, &[]); }

                    b.switch_to_block(continue_blk);
                    let cur2 = b.ins().load(types::I64, MemFlags::new(), a, 0);
                    let one = b.ins().iconst(types::I64, 1);
                    let next = b.ins().iadd(cur2, one);
                    b.ins().store(MemFlags::new(), next, a, 0);
                    b.ins().jump(loop_blk, &[]);
                    b.seal_block(continue_blk);
                    b.seal_block(loop_blk);

                    b.seal_block(after_blk);
                    b.switch_to_block(after_blk);
                } else {
                    let mut msg = format!("Undefined loop variable `{}` in DO", var);
                    if let Some(toks) = tokens {
                        if let Some(tok) = toks.iter().find(|t| match &t.kind {
                            crate::lexer::TokenKind::Ident(s) => s.eq_ignore_ascii_case(var),
                            _ => false,
                        }) {
                            let span = tok.span.clone();
                            msg = format!("{} @SPAN={:?}..{:?}", msg, span.start, span.end);
                        }
                    }
                    return Err(anyhow::anyhow!("{}", msg));
                }
            }

            IStmt::DoWhile { cond, body } => {
                let loop_blk = b.create_block();
                let body_blk = b.create_block();
                let after_blk = b.create_block();
                b.ins().jump(loop_blk, &[]);
                b.switch_to_block(loop_blk);
                let c = eval_expr(b, ex, ptr_ty, cond, ints, reals, bools, func_meta);
                let cb = to_bool(b, c);
                b.ins().brif(cb, body_blk, &[], after_blk, &[]);

                b.switch_to_block(body_blk);
                b.seal_block(body_blk);
                let er = gen_stmts(
                    b,
                    ex,
                    ptr_ty,
                    module,
                    dc,
                    &body,
                    ints,
                    reals,
                    bools,
                    chars,
                    real_kind,
                    i128_vars,
                    i128_last,
                    func_meta,
                    current_fn,
                    Some(after_blk),
                    Some(loop_blk),
                    arrays,
                    tokens,
                )?;
                if !er { b.ins().jump(loop_blk, &[]); }
                b.seal_block(loop_blk);
                b.seal_block(after_blk);
                    b.switch_to_block(after_blk);
            }
            IStmt::Call(name, args) => {
                if name=="@heap_character_bind" || name=="@heap_character_clear" {
                    let IExpr::Ident(dest)=&args[0] else {bail!("invalid heap CHARACTER binding");};
                    if name=="@heap_character_bind" {chars.bind(b,ptr_ty,dest,ints);}
                    else {chars.clear(module,b,ptr_ty,dest)?;}
                    continue;
                }
                if name == "@character_assign" || name=="@character_element_read" {
                    let [IExpr::Ident(dest), source, length] = args.as_slice() else { anyhow::bail!("invalid CHARACTER assignment"); };
                    let length=if matches!(length,IExpr::IntLit(n) if n==&crate::substrings::MAX_EXPRESSION_LENGTH.to_string()) {None}
                        else {let value=eval_expr(b,ex,ptr_ty,length,ints,reals,bools,func_meta);Some(to_i64(b,value))};
                    characters::assign(module, b, ptr_ty, dc, chars, ints, dest, source, length)?;
                    continue;
                }
                if name == "@character_code_store" {
                    let [IExpr::Ident(dest),index,code]=args.as_slice() else {bail!("invalid CHARACTER code store");};
                    let (slot,length)=chars.get(dest).context("CHARACTER code store requires scalar storage")?;
                    let index=eval_expr(b,ex,ptr_ty,index,ints,reals,bools,func_meta);let index=to_i64(b,index);
                    let inside=b.ins().icmp_imm(ir::condcodes::IntCC::UnsignedLessThan,index,*length as i64);
                    file_io::require(module,b,ptr_ty,dc,inside,"CHARACTER code store outside bounds")?;
                    let code=eval_expr(b,ex,ptr_ty,code,ints,reals,bools,func_meta);let code=to_i64(b,code);let byte=b.ins().ireduce(types::I8,code);
                    let address=storage_addr(b,ptr_ty,*slot);let address=b.ins().iadd(address,index);
                    b.ins().store(MemFlags::new(),byte,address,0);
                    continue;
                }
                if name == "@character_store" {
                    // Copy a CHARACTER dummy back to the actual passed by
                    // reference, up to the shorter length.
                    let [IExpr::Ident(address), IExpr::Ident(length), source] = args.as_slice() else { anyhow::bail!("invalid CHARACTER copy"); };
                    let source = characters::view(module, b, ptr_ty, dc, chars, ints, source)?;
                    let mut load = |n: &str| -> anyhow::Result<ir::Value> {
                        let slot = ints.get(n).ok_or_else(|| anyhow::anyhow!("missing CHARACTER argument storage"))?;
                        let a = storage_addr(b, ptr_ty, *slot);
                        let v = b.ins().load(types::I64, MemFlags::new(), a, 0);
                        Ok(if ptr_ty == types::I64 { v } else { b.ins().ireduce(ptr_ty, v) })
                    };
                    let address = load(address)?;
                    let length = load(length)?;
                    let shorter = b.ins().icmp(ir::condcodes::IntCC::UnsignedLessThan, length, source.length);
                    let count = b.ins().select(shorter, length, source.length);
                    file_io::call(module, b, "memmove", &[address, source.address, count], ptr_ty)?;
                    continue;
                }
                if name == "@character_address" {
                    // The address of a CHARACTER value passed by reference.
                    let [IExpr::Ident(dest), source] = args.as_slice() else { anyhow::bail!("invalid CHARACTER address capture"); };
                    let source = characters::view(module, b, ptr_ty, dc, chars, ints, source)?;
                    let value = if ptr_ty == types::I64 { source.address } else { b.ins().uextend(types::I64, source.address) };
                    let slot = ints.get(dest).ok_or_else(|| anyhow::anyhow!("missing CHARACTER address destination"))?;
                    let address = storage_addr(b, ptr_ty, *slot);
                    b.ins().store(MemFlags::new(), value, address, 0);
                    continue;
                }
                if name == "@character_length" {
                    let [IExpr::Ident(dest), source] = args.as_slice() else { anyhow::bail!("invalid CHARACTER length capture"); };
                    let source = characters::view(module, b, ptr_ty, dc, chars, ints, source)?;
                    let value = if ptr_ty == types::I64 { source.length } else { b.ins().uextend(types::I64, source.length) };
                    let slot = ints.get(dest).ok_or_else(|| anyhow::anyhow!("missing CHARACTER length destination"))?;
                    let address = storage_addr(b, ptr_ty, *slot);
                    b.ins().store(MemFlags::new(), value, address, 0);
                    continue;
                }
                if name == "@internal_read_byte" {
                    let [IExpr::Ident(dest),source,index,IExpr::IntLit(length)]=args.as_slice() else {anyhow::bail!("invalid internal input byte operation");};
                    let pointer=match source {
                        IExpr::Str(text)=>make_data(module,b,ptr_ty,dc,text.as_bytes())?,
                        IExpr::Ident(n)=> {
                            let (slot,_)=chars.get(n).ok_or_else(||anyhow::anyhow!("internal READ requires CHARACTER storage"))?;
                            storage_addr(b,ptr_ty,*slot)
                        }
                        _=>anyhow::bail!("unsupported internal READ source"),
                    };
                    let index=eval_expr(b,ex,ptr_ty,index,ints,reals,bools,func_meta);
                    let index=to_i64(b,index);
                    let length:i64=length.parse()?;
                    let inside=b.ins().icmp_imm(ir::condcodes::IntCC::UnsignedLessThan,index,length);
                    let load=b.create_block();let end=b.create_block();let done=b.create_block();
                    b.append_block_param(done,types::I64);
                    b.ins().brif(inside,load,&[],end,&[]);
                    b.switch_to_block(load);b.seal_block(load);
                    let address=b.ins().iadd(pointer,index);
                    let byte=b.ins().load(types::I8,MemFlags::new(),address,0);
                    let value=b.ins().uextend(types::I64,byte);
                    b.ins().jump(done,&[value.into()]);
                    b.switch_to_block(end);b.seal_block(end);
                    let zero=b.ins().iconst(types::I64,0);b.ins().jump(done,&[zero.into()]);
                    b.switch_to_block(done);b.seal_block(done);
                    let value=b.block_params(done)[0];
                    let slot=ints.get(dest).ok_or_else(||anyhow::anyhow!("internal input byte requires INTEGER storage"))?;
                    let address=storage_addr(b,ptr_ty,*slot);
                    b.ins().store(MemFlags::new(),value,address,0);
                    continue;
                }
                if name=="@file_newunit" {
                    let IExpr::Ident(dest)=&args[0] else {anyhow::bail!("NEWUNIT requires INTEGER storage");};
                    let unit=file_io::newunit(module,b,ptr_ty,dc)?;
                    let slot=ints.get(dest).ok_or_else(||anyhow::anyhow!("NEWUNIT requires INTEGER storage"))?;
                    let address=storage_addr(b,ptr_ty,*slot);b.ins().store(MemFlags::new(),unit,address,0);
                    continue;
                }
                if name=="@read_character_field" {
                    let IExpr::Ident(status)=&args[0] else {anyhow::bail!("internal READ status requires INTEGER storage");};
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1])?;
                    let source=characters::view(module,b,ptr_ty,dc,chars,ints,&args[2])?;
                    let code=file_io::call(module,b,"rfort_read_character_field",&[source.address,source.length,dest.address,dest.length],types::I64)?;
                    let slot=ints.get(status).ok_or_else(||anyhow::anyhow!("internal READ status requires INTEGER storage"))?;
                    let address=storage_addr(b,ptr_ty,*slot);b.ins().store(MemFlags::new(),code,address,0);continue;
                }
                if name=="@file_name" {
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[0])?;
                    let unit=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                    let source=file_io::name(module,b,ptr_ty,dc,unit)?;
                    file_io::call(module,b,"rfort_file_name",&[source,dest.address,dest.length],types::I32)?;
                    continue;
                }
                if name=="@cwd_query" {
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1])?;
                    let code=file_io::call(module,b,"rfort_getcwd",&[dest.address,dest.length],types::I64)?;
                    let IExpr::Ident(status)=&args[0] else {unreachable!()};
                    let status=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,status,0);continue;
                }
                if name=="@argument_query" {
                    let number=eval_expr(b,ex,ptr_ty,&args[3],ints,reals,bools,func_meta);let number=to_i64(b,number);
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[2])?;
                    let present=eval_expr(b,ex,ptr_ty,&args[6],ints,reals,bools,func_meta);let present=to_i64(b,present);
                    let IExpr::Ident(length)=&args[1] else {unreachable!()};let length=storage_addr(b,ptr_ty,ints[length]);
                    let code=file_io::call(module,b,"rfort_argument_query",&[number,dest.address,dest.length,present,length],types::I64)?;
                    let IExpr::Ident(status)=&args[0] else {unreachable!()};let status=storage_addr(b,ptr_ty,ints[status]);
                    b.ins().store(MemFlags::new(),code,status,0);continue;
                }
                if name=="@environment_query" {
                    let source=characters::view(module,b,ptr_ty,dc,chars,ints,&args[3])?;
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[2])?;
                    let trim=eval_expr(b,ex,ptr_ty,&args[4],ints,reals,bools,func_meta);let trim=to_i64(b,trim);
                    let present=eval_expr(b,ex,ptr_ty,&args[6],ints,reals,bools,func_meta);let present=to_i64(b,present);
                    let IExpr::Ident(length)=&args[1] else {unreachable!()};
                    let length=storage_addr(b,ptr_ty,ints[length]);
                    let code=file_io::call(module,b,"rfort_environment_query",&[source.address,source.length,trim,dest.address,dest.length,present,length],types::I64)?;
                    let IExpr::Ident(status)=&args[0] else {unreachable!()};
                    let status=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,status,0);continue;
                }
                if matches!(name.as_str(),"@file_inquire_unit"|"@file_inquire_file") {
                    let mut destinations=vec![];
                    for dest in &args[..5] {
                        let IExpr::Ident(dest)=dest else {anyhow::bail!("INQUIRE requires INTEGER output storage")};
                        let slot=ints.get(dest).ok_or_else(||anyhow::anyhow!("INQUIRE requires INTEGER output storage"))?;
                        destinations.push(storage_addr(b,ptr_ty,*slot));
                    }
                    let dest=characters::view(module,b,ptr_ty,dc,chars,ints,&args[5])?;
                    let (unit,path,length)=if name=="@file_inquire_file" {
                        let source=characters::view(module,b,ptr_ty,dc,chars,ints,&args[7])?;
                        (b.ins().iconst(types::I64,0),source.address,source.length)
                    } else {
                        let value=eval_expr(b,ex,ptr_ty,&args[7],ints,reals,bools,func_meta);let unit=to_i64(b,value);
                        (unit,b.ins().iconst(ptr_ty,0),b.ins().iconst(ptr_ty,0))
                    };
                    let action=characters::view(module,b,ptr_ty,dc,chars,ints,&args[6])?;
                    let code=file_io::inquire(module,b,ptr_ty,&[unit,path,length,destinations[1],destinations[2],destinations[3],destinations[4],dest.address,dest.length,action.address,action.length])?;
                    b.ins().store(MemFlags::new(),code,destinations[0],0);
                    continue;
                }
                if name=="@file_open_dynamic" || name=="@file_open_try" {
                    let IExpr::Ident(dest)=&args[0] else {anyhow::bail!("invalid OPEN output");};
                    let path=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1])?;
                    let mode=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let mode=to_i64(b,mode);
                    let requested=if matches!(&args[3],IExpr::IntLit(n) if n=="1") {b.ins().iconst(types::I64,-1)}else{let v=eval_expr(b,ex,ptr_ty,&args[5],ints,reals,bools,func_meta);to_i64(b,v)};
                    let newunit=eval_expr(b,ex,ptr_ty,&args[3],ints,reals,bools,func_meta);let newunit=to_i64(b,newunit);
                    let append=eval_expr(b,ex,ptr_ty,&args[4],ints,reals,bools,func_meta);let append=to_i64(b,append);
                    let unit=file_io::open_dynamic(module,b,ptr_ty,dc,requested,path.address,path.length,mode,newunit,append,name=="@file_open_dynamic")?;
                    let address=storage_addr(b,ptr_ty,ints[dest]);b.ins().store(MemFlags::new(),unit,address,0);continue;
                }
                if name=="@format_end_file" {
                    let h=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let h=to_i64(b,h);
                    let unit=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                    let file=file_io::write_handle(module,b,ptr_ty,dc,unit)?;
                    let code=file_io::call(module,b,"rfort_format_end_file",&[h,file],types::I32)?;
                    let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,code,0);file_io::require(module,b,ptr_ty,dc,okay,"formatted file WRITE failed")?;continue;
                }
                if name=="@file_write_record" {
                    let unit=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                    let file=file_io::write_handle(module,b,ptr_ty,dc,unit)?;
                    let text=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1])?;
                    let code=file_io::call(module,b,"rfort_file_write_record",&[file,text.address,text.length],types::I32)?;
                    let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,code,0);
                    file_io::require(module,b,ptr_ty,dc,okay,"formatted file WRITE failed")?;continue;
                }
                if name == "@file_flush" {
                    // A null stream flushes every output stream.
                    let null = b.ins().iconst(ptr_ty, 0);
                    file_io::call(module, b, "fflush", &[null], types::I32)?;
                    continue;
                }
                if matches!(name.as_str(), "@file_open" | "@file_close" | "@file_close_delete" | "@file_rewind" | "@file_backspace" | "@file_endfile" | "@file_scratch") {
                    let unit=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let unit=to_i64(b,unit);
                    let path=match args.get(1) {Some(IExpr::Str(path))=>Some(path.as_str()),_=>None};
                    file_io::operation(module,b,ptr_ty,dc,name,unit,path)?;
                    continue;
                }
                if name=="@list_read_internal_begin" {
                    let [IExpr::Ident(dest),IExpr::Ident(status),source]=args.as_slice() else {anyhow::bail!("invalid internal READ session");};
                    let source=characters::view(module,b,ptr_ty,dc,chars,ints,source)?;
                    let handle=file_io::call(module,b,"rfort_list_read_internal_begin",&[source.address,source.length],ptr_ty)?;
                    let failed=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,handle,0);let code=b.ins().uextend(types::I64,failed);
                    let address=storage_addr(b,ptr_ty,ints[dest]);b.ins().store(MemFlags::new(),handle,address,0);
                    let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);continue;
                }
                if name=="@list_read_begin" {
                    let [IExpr::Ident(dest),IExpr::Ident(status),unit]=args.as_slice() else {anyhow::bail!("invalid list READ session");};
                    let unit=eval_expr(b,ex,ptr_ty,unit,ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                    let (file,code)=file_io::read_handle(module,b,ptr_ty,unit)?;
                    // An invalid unit must not accidentally become stdin.
                    let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,code,0);
                    let begin=b.create_block();let done=b.create_block();b.append_block_param(done,ptr_ty);b.append_block_param(done,types::I64);
                    let null=b.ins().iconst(ptr_ty,0);b.ins().brif(okay,begin,&[],done,&[null.into(),code.into()]);
                    b.switch_to_block(begin);b.seal_block(begin);
                    let handle=file_io::call(module,b,"rfort_list_read_begin",&[file],ptr_ty)?;
                    let failed=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,handle,0);let code=b.ins().uextend(types::I64,failed);
                    b.ins().jump(done,&[handle.into(),code.into()]);b.switch_to_block(done);b.seal_block(done);
                    let handle=b.block_params(done)[0];let code=b.block_params(done)[1];
                    let handle=if ptr_ty==types::I64 {handle}else{b.ins().uextend(types::I64,handle)};
                    let address=storage_addr(b,ptr_ty,ints[dest]);b.ins().store(MemFlags::new(),handle,address,0);
                    let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);continue;
                }
                if matches!(name.as_str(),"@nml_begin"|"@nml_internal_begin"|"@nml_next"|"@nml_end"|"@nml_record"|"@nml_layout") {
                    if name=="@nml_begin" || name=="@nml_internal_begin" {
                        let [IExpr::Ident(handle),IExpr::Ident(status),unit,group]=args.as_slice() else {anyhow::bail!("invalid NAMELIST begin");};
                        let group=characters::view(module,b,ptr_ty,dc,chars,ints,group)?;
                        let zero=b.ins().iconst(ptr_ty,0);
                        let (file,source,length,code)=if name=="@nml_internal_begin" {
                            let view=characters::view(module,b,ptr_ty,dc,chars,ints,unit)?;(zero,view.address,view.length,zero)
                        }else{
                            let unit=eval_expr(b,ex,ptr_ty,unit,ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                            let (file,code)=file_io::read_handle(module,b,ptr_ty,unit)?;(file,zero,zero,code)
                        };
                        let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,code,0);
                        let create=b.create_block();let done=b.create_block();b.append_block_param(done,ptr_ty);b.append_block_param(done,types::I64);
                        b.ins().brif(okay,create,&[],done,&[zero.into(),code.into()]);b.switch_to_block(create);b.seal_block(create);
                        let h=file_io::call(module,b,"rfort_nml_begin",&[file,source,length,group.address,group.length],ptr_ty)?;
                        let failed=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,h,0);let failed=b.ins().uextend(types::I64,failed);
                        b.ins().jump(done,&[h.into(),failed.into()]);b.switch_to_block(done);b.seal_block(done);
                        let h=b.block_params(done)[0];let code=b.block_params(done)[1];
                        let address=storage_addr(b,ptr_ty,ints[handle]);b.ins().store(MemFlags::new(),h,address,0);
                        let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);
                    }else if name=="@nml_layout" {
                        let IExpr::Ident(status)=&args[0] else {unreachable!()};let IExpr::Ident(index)=&args[1] else {unreachable!()};
                        let address=storage_addr(b,ptr_ty,ints[index]);
                        let h=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let h=to_i64(b,h);
                        let code=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let code=to_i64(b,code);
                        let mut values=vec![h,address,code];
                        for e in &args[3..] {let value=eval_expr(b,ex,ptr_ty,e,ints,reals,bools,func_meta);values.push(to_i64(b,value));}
                        let code=file_io::call(module,b,"rfort_nml_layout",&values,types::I64)?;
                        let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);
                    }else if name=="@nml_next" {
                        let [IExpr::Ident(status),IExpr::Ident(index),field,token,handle]=args.as_slice() else {anyhow::bail!("invalid NAMELIST next");};
                        let h=eval_expr(b,ex,ptr_ty,handle,ints,reals,bools,func_meta);let h=to_i64(b,h);
                        let index=storage_addr(b,ptr_ty,ints[index]);
                        let field=characters::view(module,b,ptr_ty,dc,chars,ints,field)?;
                        let token=characters::view(module,b,ptr_ty,dc,chars,ints,token)?;
                        let code=file_io::call(module,b,"rfort_nml_next",&[h,index,field.address,field.length,token.address,token.length],types::I64)?;
                        let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);
                    }else if name=="@nml_end" {
                        let h=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let h=to_i64(b,h);
                        file_io::call(module,b,"rfort_nml_end",&[h],types::I64)?;
                    }else{
                        let IExpr::Ident(status)=&args[0] else {anyhow::bail!("invalid NAMELIST output status");};
                        let unit=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                        let view=characters::view(module,b,ptr_ty,dc,chars,ints,&args[2])?;
                        let slots=file_io::table(b,module,ptr_ty,".rfort.file_units")?;
                        let code=file_io::call(module,b,"rfort_nml_record",&[slots,unit,view.address,view.length],types::I64)?;
                        let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);
                    }
                    continue;
                }
                if name=="@list_read_complex_next" {
                    let [IExpr::Ident(status),IExpr::Ident(re),IExpr::Ident(im),handle,width]=args.as_slice() else {anyhow::bail!("invalid COMPLEX READ");};
                    let h=eval_expr(b,ex,ptr_ty,handle,ints,reals,bools,func_meta);let h=to_i64(b,h);
                    let width=eval_expr(b,ex,ptr_ty,width,ints,reals,bools,func_meta);let width=to_i64(b,width);
                    let re=storage_addr(b,ptr_ty,reals[re]);let im=storage_addr(b,ptr_ty,reals[im]);
                    let code=file_io::call(module,b,"rfort_list_read_complex_next",&[h,width,re,im],types::I64)?;
                    let status=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,status,0);continue;
                }
                // Unformatted stream transfers of one scalar, and POS.
                if name=="@stream_put" || name=="@stream_get" || name=="@stream_seek" {
                    let IExpr::Ident(status)=&args[0] else {anyhow::bail!("invalid stream status");};
                    let unit=if name=="@stream_get" {&args[2]} else {&args[1]};
                    let unit=eval_expr(b,ex,ptr_ty,unit,ints,reals,bools,func_meta);let unit=to_i64(b,unit);
                    let file=file_io::handle(module,b,ptr_ty,dc,unit)?;
                    let code=if name=="@stream_seek" {
                        let position=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let position=to_i64(b,position);
                        file_io::call(module,b,"rfort_stream_seek",&[file,position],types::I64)?
                    } else {
                        let value=if name=="@stream_put" {&args[2]} else {&args[1]};
                        let [IExpr::IntLit(tag),IExpr::IntLit(width)]=&args[3..] else {anyhow::bail!("invalid stream transfer");};
                        let tag:i64=tag.parse()?;let width:i64=width.parse()?;
                        let IExpr::Ident(value)=value else {anyhow::bail!("stream transfer requires a variable");};
                        let slot=match tag {4=>chars.get(value).map(|s|s.0),2=>reals.get(value).copied(),3=>bools.get(value).copied(),_=>ints.get(value).copied()}
                            .ok_or_else(||anyhow::anyhow!("missing stream transfer storage"))?;
                        let address=storage_addr(b,ptr_ty,slot);let tag=b.ins().iconst(types::I64,tag);let width=b.ins().iconst(ptr_ty,width);
                        file_io::call(module,b,if name=="@stream_put" {"rfort_stream_put"} else {"rfort_stream_get"},&[file,tag,width,address],types::I64)?
                    };
                    let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);continue;
                }
                if name=="@list_read_next" || name=="@list_read_end" {
                    let h=if name=="@list_read_next" {&args[2]}else{&args[1]};
                    let h=eval_expr(b,ex,ptr_ty,h,ints,reals,bools,func_meta);let h=to_i64(b,h);
                    let h=if ptr_ty==types::I64 {h}else{b.ins().ireduce(ptr_ty,h)};
                    if name=="@list_read_end" {
                        let finish=file_io::call(module,b,"rfort_list_read_end",&[h],types::I64)?;
                        let IExpr::Ident(status)=&args[0] else {anyhow::bail!("invalid READ status");};
                        let address=storage_addr(b,ptr_ty,ints[status]);let previous=b.ins().load(types::I64,MemFlags::new(),address,0);
                        let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,previous,0);let code=b.ins().select(okay,finish,previous);
                        b.ins().store(MemFlags::new(),code,address,0);continue;
                    }
                    let [IExpr::Ident(status),IExpr::Ident(dest),_,IExpr::IntLit(tag),IExpr::IntLit(width)]=args.as_slice() else {anyhow::bail!("invalid list READ transfer");};
                    let tag:i64=tag.parse()?;let width:i64=width.parse()?;
                    let slot=match tag {4=>chars.get(dest).map(|s|s.0),2=>reals.get(dest).copied(),3=>bools.get(dest).copied(),_=>ints.get(dest).copied()}.ok_or_else(||anyhow::anyhow!("missing READ destination"))?;
                    let out=storage_addr(b,ptr_ty,slot);let tag=b.ins().iconst(types::I64,tag);let width=b.ins().iconst(ptr_ty,width);
                    let code=file_io::call(module,b,"rfort_list_read_next",&[h,tag,width,out],types::I64)?;
                    let address=storage_addr(b,ptr_ty,ints[status]);b.ins().store(MemFlags::new(),code,address,0);continue;
                }
                if name == "@read_record_status" || name=="@read_record_checked" {
                    let [IExpr::Ident(status),IExpr::Ident(dest),IExpr::IntLit(length),unit,..]=args.as_slice() else {anyhow::bail!("invalid record READ with IOSTAT");};
                    let (slot,capacity)=chars.get(dest).ok_or_else(||anyhow::anyhow!("record READ requires CHARACTER storage"))?;
                    let length:usize=length.parse()?;
                    anyhow::ensure!(length<=*capacity,"record READ exceeds CHARACTER storage");
                    let pointer=storage_addr(b,ptr_ty,*slot);
                    let (file,code)=if matches!(unit,IExpr::IntLit(n) if n == "5") {
                        (b.ins().iconst(ptr_ty,0),b.ins().iconst(types::I64,0))
                    } else {
                        let value=eval_expr(b,ex,ptr_ty,unit,ints,reals,bools,func_meta);
                        let unit=to_i64(b,value);
                        file_io::read_handle(module,b,ptr_ty,unit)?
                    };
                    let format=args.get(4).map(|format| characters::view(module,b,ptr_ty,dc,chars,ints,format)).transpose()?;
                    let read=b.create_block();let done=b.create_block();b.append_block_param(done,types::I64);
                    let okay=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,code,0);
                    b.ins().brif(okay,read,&[],done,&[code.into()]);
                    b.switch_to_block(read);b.seal_block(read);
                    let size=b.ins().iconst(ptr_ty,length as i64);
                    let code=if let Some(format)=format {
                        file_io::call(module,b,"rfort_read_record_format",&[file,pointer,size,format.address,format.length],types::I64)?
                    } else {file_io::call(module,b,if name=="@read_record_checked" {"rfort_read_record_checked"} else {"rfort_read_record"},&[file,pointer,size],types::I64)?};
                    b.ins().jump(done,&[code.into()]);
                    b.switch_to_block(done);b.seal_block(done);
                    let code=b.block_params(done)[0];
                    let storage=ints.get(status).ok_or_else(||anyhow::anyhow!("IOSTAT requires INTEGER storage"))?;
                    let address=storage_addr(b,ptr_ty,*storage);
                    b.ins().store(MemFlags::new(),code,address,0);
                    continue;
                }
                if name == "@read_record" {
                    let [IExpr::Ident(dest),IExpr::IntLit(length),..]=args.as_slice() else {anyhow::bail!("invalid record READ");};
                    let (slot,capacity)=chars.get(dest).ok_or_else(|| anyhow::anyhow!("record READ requires CHARACTER storage"))?;
                    let length:usize=length.parse()?;
                    anyhow::ensure!(length<=*capacity,"record READ exceeds CHARACTER storage");
                    let pointer=storage_addr(b,ptr_ty,*slot);
                    let file=if let Some(unit)=args.get(2) {
                        let value=eval_expr(b,ex,ptr_ty,unit,ints,reals,bools,func_meta);
                        let unit=to_i64(b,value);
                        Some(file_io::handle(module,b,ptr_ty,dc,unit)?)
                    } else {None};
                    read_record(module,b,ptr_ty,dc,pointer,length,file)?;
                    continue;
                }
                if name=="@internal_format_end" {
                    let [IExpr::Ident(dest),handle,width]=args.as_slice() else {anyhow::bail!("invalid internal WRITE destination");};
                    let view=characters::view(module,b,ptr_ty,dc,chars,ints,&IExpr::Ident(dest.clone()))?;
                    let handle=eval_expr(b,ex,ptr_ty,handle,ints,reals,bools,func_meta);
                    let handle=to_i64(b,handle);
                    let handle=if ptr_ty==types::I64 {handle} else {b.ins().ireduce(ptr_ty,handle)};
                    let width=eval_expr(b,ex,ptr_ty,width,ints,reals,bools,func_meta);let width=to_i64(b,width);
                    let width=if ptr_ty==types::I64 {width} else {b.ins().ireduce(ptr_ty,width)};
                    file_io::call(module,b,"rfort_internal_format_end",&[handle,view.address,width],types::I32)?;
                    continue;
                }
                if name=="@format_begin" {
                    let IExpr::Ident(dest)=&args[0] else {anyhow::bail!("invalid formatted output list");};
                    let format=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1])?;
                    let advance=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);
                    let advance=to_i64(b,advance);let advance=b.ins().ireduce(types::I32,advance);
                    let handle=file_io::call(module,b,"rfort_format_begin",&[format.address,format.length,advance],ptr_ty)?;
                    let handle=if ptr_ty==types::I64 {handle} else {b.ins().uextend(types::I64,handle)};
                    let storage=ints.get(dest).ok_or_else(||anyhow::anyhow!("formatted output list requires INTEGER storage"))?;
                    let address=storage_addr(b,ptr_ty,*storage);b.ins().store(MemFlags::new(),handle,address,0);
                    continue;
                }
                if name=="@format_item" || name=="@format_end" {
                    let handle=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);
                    let handle=to_i64(b,handle);let handle=if ptr_ty==types::I64 {handle} else {b.ins().ireduce(ptr_ty,handle)};
                    if name=="@format_end" {file_io::call(module,b,"rfort_format_end",&[handle],types::I32)?;}
                    else {
                        let (tag,bits,length)=if let Ok(view)=characters::view(module,b,ptr_ty,dc,chars,ints,&args[1]) {
                            let bits=if ptr_ty==types::I64 {view.address} else {b.ins().uextend(types::I64,view.address)};
                            let length=if ptr_ty==types::I64 {view.length} else {b.ins().uextend(types::I64,view.length)};
                            (4,bits,length)
                        } else {
                            let value=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);
                            // The kind travels in the length field of numeric items.
                            let (tag,bits,kind)=match value {
                                CgVal::I64(v)=>(1,v,integer_kind(&args[1])),
                                CgVal::F64(v)=>(2,b.ins().bitcast(types::I64,MemFlags::new(),v),if single_value(&args[1],real_kind,func_meta) {4} else {8}),
                                CgVal::Bool(v)=>(3,b.ins().uextend(types::I64,v),0),
                            };
                            (tag,bits,b.ins().iconst(types::I64,kind))
                        };
                        let tag=b.ins().iconst(types::I64,tag);
                        file_io::call(module,b,"rfort_format_push",&[handle,tag,bits,length],types::I32)?;
                    }
                    continue;
                }
                if name == "@formatted_print" || name == "@formatted_write_noadvance" {
                    let format = characters::view(module,b,ptr_ty,dc,chars,ints,&args[0])?;
                    let count = args.len()-1;
                    let size = count.checked_mul(24).and_then(|n| u32::try_from(n.max(24)).ok())
                        .ok_or_else(|| anyhow::anyhow!("formatted output list is too large"))?;
                    let slot = b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,size,3));
                    let items = b.ins().stack_addr(ptr_ty,slot,0);
                    for (index,e) in args[1..].iter().enumerate() {
                        let (tag,bits,length) = if let Ok(view) = characters::view(module,b,ptr_ty,dc,chars,ints,e) {
                            let bits = if ptr_ty == types::I64 { view.address } else { b.ins().uextend(types::I64,view.address) };
                            let length = if ptr_ty == types::I64 { view.length } else { b.ins().uextend(types::I64,view.length) };
                            (4,bits,length)
                        } else {
                            let value = eval_expr(b,ex,ptr_ty,e,ints,reals,bools,func_meta);
                            let (tag,bits,kind) = match value {
                                CgVal::I64(v) => (1,v,integer_kind(e)),
                                CgVal::F64(v) => (2,b.ins().bitcast(types::I64,MemFlags::new(),v),if single_value(e,real_kind,func_meta) {4} else {8}),
                                CgVal::Bool(v) => (3,b.ins().uextend(types::I64,v),0),
                            };
                            (tag,bits,b.ins().iconst(types::I64,kind))
                        };
                        let tag = b.ins().iconst(types::I64,tag);
                        for (offset,value) in [(0,tag),(8,bits),(16,length)] {
                            b.ins().store(MemFlags::new(),value,items,(index*24+offset) as i32);
                        }
                    }
                    let mut sig = module.make_signature();
                    sig.params.extend((0..4).map(|_| ir::AbiParam::new(ptr_ty)));
                    sig.params.push(ir::AbiParam::new(types::I32));
                    let id = module.declare_function("rfort_formatted_write",Linkage::Import,&sig)?;
                    let function = module.declare_func_in_func(id,b.func);
                    let count = b.ins().iconst(ptr_ty,count as i64);
                    let advance = b.ins().iconst(types::I32,i64::from(name != "@formatted_write_noadvance"));
                    b.ins().call(function,&[format.address,format.length,items,count,advance]);
                    continue;
                }
                if name == "@stop" && args.is_empty() {
                    let mut sig=module.make_signature();sig.params.push(ir::AbiParam::new(types::I32));
                    let id=module.declare_function("exit",Linkage::Import,&sig)?;
                    let function=module.declare_func_in_func(id,b.func);
                    let code=b.ins().iconst(types::I32,0);b.ins().call(function,&[code]);
                    b.ins().trap(ir::TrapCode::unwrap_user(1));return Ok(true);
                }
                if name=="@heap_bounds" {
                    let lower=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let lower=to_i64(b,lower);
                    let upper=eval_expr(b,ex,ptr_ty,&args[3],ints,reals,bools,func_meta);let upper=to_i64(b,upper);
                    let empty=b.ins().icmp(ir::condcodes::IntCC::SignedLessThan,upper,lower);let upper=b.ins().select(empty,lower,upper);
                    let (difference,overflow)=b.ins().ssub_overflow(upper,lower);let okay=b.ins().bnot(overflow);file_io::require(module,b,ptr_ty,dc,okay,"allocation extent overflow")?;
                    let one=b.ins().iconst(types::I64,1);let (extent,overflow)=b.ins().sadd_overflow(difference,one);let okay=b.ins().bnot(overflow);file_io::require(module,b,ptr_ty,dc,okay,"allocation extent overflow")?;
                    let zero=b.ins().iconst(types::I64,0);let extent=b.ins().select(empty,zero,extent);
                    for (target,value) in args[..2].iter().zip([lower,extent]) {let IExpr::Ident(n)=target else{bail!("allocation bounds require descriptor fields")};let address=storage_addr(b,ptr_ty,ints[n]);b.ins().store(MemFlags::new(),value,address,0);}
                    continue;
                }
                if name=="@c_export" {continue;}
                if name=="@c_invoke" {
                    c_interop::invoke(module,b,ptr_ty,args,ints,reals,bools,dc)?;
                    continue;
                }
                if name.starts_with("@heap_record_") {
                    let index=if name=="@heap_record_dispose" || name=="@heap_record_pointer" {0}else{1};
                    let value=eval_expr(b,ex,ptr_ty,&args[index],ints,reals,bools,func_meta);let value=to_i64(b,value);
                    record_heap::emit(module,b,ptr_ty,dc,name,args,value,ints,reals,bools)?;
                    continue;
                }
                if name=="@heap_bind" {
                    let IExpr::Ident(local)=&args[0] else{bail!("invalid heap binding")};
                    let cell=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,8,0));
                    let zero=b.ins().iconst(ptr_ty,0);b.ins().stack_store(zero,cell,0);
                    let storage=Storage::HeapStack(cell);
                    if let Some(slot)=ints.get_mut(local){*slot=storage;}else if let Some(slot)=reals.get_mut(local){*slot=storage;}else if let Some(slot)=bools.get_mut(local){*slot=storage;}else{bail!("unknown heap array `{local}`");}
                    arrays.insert(local.clone(),1);continue;
                }
                if name=="@heap_allocate" || name=="@heap_resize" || name=="@heap_free" || name=="@heap_drop" {
                    let resize=name=="@heap_resize";
                    let storage=|n:&str|ints.get(n).or_else(||reals.get(n)).or_else(||bools.get(n)).copied();
                    let IExpr::Ident(local)=&args[0] else{bail!("invalid heap allocation")};
                    let cell=heap_cell(b,ptr_ty,storage(local).context("missing array storage")?)?;
                    let IExpr::Ident(mask)=&args[if name=="@heap_allocate" || resize{2}else{1}] else{bail!("missing allocation initialization mask")};
                    let mask_cell=heap_cell(b,ptr_ty,storage(mask).context("missing initialization storage")?)?;
                    if name!="@heap_allocate" && !resize {
                        let mut signature=module.make_signature();signature.params.push(ir::AbiParam::new(ptr_ty));
                        let id=module.declare_function("free",Linkage::Import,&signature)?;let f=module.declare_func_in_func(id,b.func);
                        for cell in [cell,mask_cell] {let pointer=b.ins().load(ptr_ty,MemFlags::new(),cell,0);b.ins().call(f,&[pointer]);let zero=b.ins().iconst(ptr_ty,0);b.ins().store(MemFlags::new(),zero,cell,0);}
                    }else{
                        let mut count=b.ins().iconst(types::I64,1);
                        // First collect all extents so a zero-sized shape never overflows.
                        let mut extents=vec![];let mut empty=b.ins().iconst(types::I8,0);
                        for arg in &args[3..] {let value=eval_expr(b,ex,ptr_ty,arg,ints,reals,bools,func_meta);let extent=to_i64(b,value);let zero=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,extent,0);empty=b.ins().bor(empty,zero);extents.push(extent);}
                        for extent in extents {let zero=b.ins().iconst(types::I64,0);let extent=b.ins().select(empty,zero,extent);let (product,overflow)=b.ins().umul_overflow(count,extent);let okay=b.ins().bnot(overflow);file_io::require(module,b,ptr_ty,dc,okay,"allocation size overflow")?;count=product;}
                        let maximum=b.ins().iconst(types::I64,i64::MAX/8);let okay=b.ins().icmp(ir::condcodes::IntCC::UnsignedLessThanOrEqual,count,maximum);file_io::require(module,b,ptr_ty,dc,okay,"allocation size overflow")?;
                        let IExpr::Ident(length)=&args[1] else{bail!("invalid allocation length")};let length_address=storage_addr(b,ptr_ty,ints[length]);
                        let old_count=if resize {b.ins().load(types::I64,MemFlags::new(),length_address,0)} else {b.ins().iconst(types::I64,0)};
                        b.ins().store(MemFlags::new(),count,length_address,0);
                        if resize {
                            let okay=b.ins().icmp(ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,count,old_count);
                            file_io::require(module,b,ptr_ty,dc,okay,"constructor storage size overflow")?;
                            let bytes=b.ins().imul_imm(count,8);
                            let old_bytes=b.ins().imul_imm(old_count,8);
                            let added=b.ins().isub(bytes,old_bytes);
                            let (bytes,old_bytes,added)=if ptr_ty==types::I64 {(bytes,old_bytes,added)} else {
                                let limit=b.ins().iconst(types::I64,u32::MAX as i64);
                                let okay=b.ins().icmp(ir::condcodes::IntCC::UnsignedLessThanOrEqual,bytes,limit);
                                file_io::require(module,b,ptr_ty,dc,okay,"allocation size overflow")?;
                                (b.ins().ireduce(ptr_ty,bytes),b.ins().ireduce(ptr_ty,old_bytes),b.ins().ireduce(ptr_ty,added))
                            };
                            for cell in [cell,mask_cell] {
                                let old=b.ins().load(ptr_ty,MemFlags::new(),cell,0);
                                let data=file_io::call(module,b,"realloc",&[old,bytes],ptr_ty)?;
                                let okay=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,data,0);
                                file_io::require(module,b,ptr_ty,dc,okay,"constructor storage allocation failed")?;
                                b.ins().store(MemFlags::new(),data,cell,0);
                                let tail=b.ins().iadd(data,old_bytes);
                                let zero=b.ins().iconst(types::I32,0);
                                file_io::call(module,b,"memset",&[tail,zero,added],ptr_ty)?;
                            }
                            continue;
                        }
                        let one=b.ins().iconst(types::I64,1);let empty=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,count,0);let count=b.ins().select(empty,one,count);
                        let count=if ptr_ty==types::I64 {count}else{let limit=b.ins().iconst(types::I64,(u32::MAX/8) as i64);let okay=b.ins().icmp(ir::condcodes::IntCC::UnsignedLessThanOrEqual,count,limit);file_io::require(module,b,ptr_ty,dc,okay,"allocation size overflow")?;b.ins().ireduce(ptr_ty,count)};
                        let width=b.ins().iconst(ptr_ty,8);
                        let data=file_io::call(module,b,"calloc",&[count,width],ptr_ty)?;let okay=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,data,0);file_io::require(module,b,ptr_ty,dc,okay,"array allocation failed")?;
                        let mask=file_io::call(module,b,"calloc",&[count,width],ptr_ty)?;let okay=b.ins().icmp_imm(ir::condcodes::IntCC::NotEqual,mask,0);file_io::require(module,b,ptr_ty,dc,okay,"initialization mask allocation failed")?;
                        b.ins().store(MemFlags::new(),data,cell,0);b.ins().store(MemFlags::new(),mask,mask_cell,0);
                    }
                    continue;
                }
                if name=="@module_bind" {
                    let [IExpr::Ident(local),IExpr::Str(key),IExpr::Str(initializer)]=args.as_slice() else {bail!("invalid module storage binding");};
                    let initial:crate::modules::Storage=serde_json::from_str(initializer)?;
                    let symbol=format!(".rfort.module.{key}");
                    let id=if let Some(cranelift_module::FuncOrDataId::Data(id))=module.get_name(&symbol) {id} else {
                        let mut bytes=vec![];
                        let initial_values=if initial.deferred {vec![crate::ast::Expr::IntLit("0".into())]}else{initial.values.clone()};
                        for value in &initial_values {
                            if let TypeSpec::Character(length)=&initial.kind {
                                let mut text=crate::parameters::character_constant(value)?.into_bytes();
                                let length=length.unwrap_or(1);text.resize(length,b' ');text.truncate(length);text.push(0);bytes.extend(text);
                            } else {
                                let bits=if initial.deferred {0} else {match &initial.kind {
                                    TypeSpec::Real|TypeSpec::DoublePrecision=>crate::parameters::real_constant(value)?.to_bits(),
                                    TypeSpec::Logical=>if let crate::ast::Expr::Logical(v)=value {u64::from(*v)}else{bail!("invalid LOGICAL module initializer")},
                                    _=>crate::parameters::integer_constant(value)? as u64,
                                }};
                                if bools.contains_key(local)&&!arrays.contains_key(local) {bytes.push(bits as u8);}
                                else if module.isa().endianness()==ir::Endianness::Little {bytes.extend(bits.to_le_bytes());}else{bytes.extend(bits.to_be_bytes());}
                            }
                        }
                        if bytes.is_empty() {bytes.resize(8,0);}
                        let id=module.declare_data(&symbol,Linkage::Local,true,false)?;
                        let mut data=DataDescription::new();data.define(bytes.into_boxed_slice());data.set_align(8);module.define_data(id,&data)?;id
                    };
                    let global=module.declare_data_in_func(id,b.func);
                    let storage=if initial.deferred {arrays.insert(local.clone(),1);Storage::HeapStatic(global)}else{Storage::Static(global)};
                    if let Some((slot,_))=chars.get_mut(local) {
                        *slot=storage;let address=storage_addr(b,ptr_ty,storage);
                        let value=if ptr_ty==types::I64 {address}else{b.ins().uextend(types::I64,address)};
                        let pointer=storage_addr(b,ptr_ty,ints[&format!("@character_pointer:{local}")]);b.ins().store(MemFlags::new(),value,pointer,0);
                    } else if let Some(slot)=ints.get_mut(local) {*slot=storage;}
                    else if let Some(slot)=reals.get_mut(local) {*slot=storage;}
                    else if let Some(slot)=bools.get_mut(local) {*slot=storage;}
                    else {bail!("module storage requires a declared intrinsic variable");}
                    continue;
                }
                if name == "@saved_bind" {
                    let [IExpr::Ident(local),IExpr::Str(key),value]=args.as_slice() else {bail!("invalid saved-storage binding");};
                    let symbol=format!(".rfort.saved.{key}");
                    let id=if let Some(cranelift_module::FuncOrDataId::Data(id))=module.get_name(&symbol) {id} else {
                        fn bits(value:&IExpr)->Result<u64> {
                            Ok(match value {
                                IExpr::IntLit(v)=>v.parse::<i64>()? as u64,
                                IExpr::RealLit(v)=>v.parse::<f64>()?.to_bits(),
                                IExpr::Logical(v)=>u64::from(*v),
                                IExpr::Call(n,args) if matches!(n.as_str(),"dble"|"@integer_literal")=>bits(&args[0])?,
                                _=>bail!("saved initializer must be a resolved scalar constant"),
                            })
                        }
                        let bytes=if let Some((_,length))=chars.get(local) {
                            let IExpr::Str(text)=value else {bail!("saved CHARACTER initializer must be a resolved string constant");};
                            let mut bytes=text.as_bytes().to_vec();bytes.resize(*length,b' ');bytes.truncate(*length);bytes.push(0);bytes
                        } else {
                            let value=bits(value)?;
                            if bools.contains_key(local) {vec![value as u8;1]}
                            else if module.isa().endianness()==ir::Endianness::Little {value.to_le_bytes().to_vec()} else {value.to_be_bytes().to_vec()}
                        };
                        let id=module.declare_data(&symbol,Linkage::Local,true,false)?;
                        let mut data=DataDescription::new();data.define(bytes.into_boxed_slice());data.set_align(8);
                        module.define_data(id,&data)?;id
                    };
                    let storage=Storage::Static(module.declare_data_in_func(id,b.func));
                    if let Some((slot,_))=chars.get_mut(local) {
                        *slot=storage;
                        // String intrinsics and comparisons also use this pointer cell.
                        let address=storage_addr(b,ptr_ty,storage);
                        let value=if ptr_ty==types::I64 {address} else {b.ins().uextend(types::I64,address)};
                        let pointer=storage_addr(b,ptr_ty,ints[&format!("@character_pointer:{local}")]);
                        b.ins().store(MemFlags::new(),value,pointer,0);
                    }
                    else if let Some(slot)=ints.get_mut(local) {*slot=storage;}
                    else if let Some(slot)=reals.get_mut(local) {*slot=storage;}
                    else if let Some(slot)=bools.get_mut(local) {*slot=storage;}
                    else {bail!("saved storage requires a declared numeric, LOGICAL or CHARACTER scalar");}
                    continue;
                }
                if name == "@error_stop" || name == "@stop" {
                    let keyword=if name=="@stop" {"STOP"} else {"ERROR STOP"};
                    let (message,exit_code)=match args.as_slice() {
                        [message] if is_character_output(message,chars)=>(message.clone(),i32::from(name=="@error_stop")),
                        [IExpr::IntLit(code)]=>{
                            let code=code.parse::<i32>().map_err(|_|anyhow::anyhow!("{keyword} integer code must fit default INTEGER"))?;
                            (IExpr::Str(code.to_string()),code)
                        },
                        _=>anyhow::bail!("{keyword} currently requires a CHARACTER or default INTEGER constant"),
                    };
                    let message=IExpr::Bin(crate::ast::BinOp::Concat,Box::new(IExpr::Bin(crate::ast::BinOp::Concat,Box::new(IExpr::Str(format!("{keyword} "))),Box::new(message))),Box::new(IExpr::Str("\n".into())));
                    let text=characters::view(module,b,ptr_ty,dc,chars,ints,&message)?;let pointer=text.address;
                    let mut sig=module.make_signature();sig.params.extend([ir::AbiParam::new(types::I32),ir::AbiParam::new(ptr_ty),ir::AbiParam::new(types::I32)]);sig.returns.push(ir::AbiParam::new(types::I32));
                    let id=module.declare_function(if cfg!(windows) { "_write" } else { "rfort_write" },Linkage::Import,&sig)?;let function=module.declare_func_in_func(id,b.func);let fd=b.ins().iconst(types::I32,2);let len=b.ins().ireduce(types::I32,text.length);b.ins().call(function,&[fd,pointer,len]);
                    let mut sig=module.make_signature();sig.params.push(ir::AbiParam::new(types::I32));let id=module.declare_function("exit",Linkage::Import,&sig)?;let function=module.declare_func_in_func(id,b.func);let code=b.ins().iconst(types::I32,exit_code as i64);b.ins().call(function,&[code]);b.ins().trap(ir::TrapCode::unwrap_user(1));return Ok(true);
                }
                if name.eq_ignore_ascii_case("random_seed") && !func_meta.contains_key(name) {
                    let id = module.declare_data(".f90c.random_state", Linkage::Local, true, false)?;
                    let global = module.declare_data_in_func(id, b.func);
                    let address = b.ins().global_value(ptr_ty, global);
                    let seed = b.ins().iconst(types::I64, 1);
                    b.ins().store(MemFlags::new(), seed, address, 0);
                    continue;
                }
                if name == "@date_snapshot" {
                    let IExpr::Index(array, _) = &args[0] else {anyhow::bail!("invalid calendar snapshot buffer")};
                    let pointer = storage_addr(b, ptr_ty, ints[array]);
                    file_io::call(module, b, "rfort_date_snapshot", &[pointer], types::I64)?;
                    continue;
                }
                if name=="@random_seed_put" || name=="@random_seed_get" {
                    let id=module.declare_data(".f90c.random_state",Linkage::Local,true,false)?;
                    let global=module.declare_data_in_func(id,b.func);
                    let address=b.ins().global_value(ptr_ty,global);
                    if name=="@random_seed_put" {
                        let low=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let low=to_i64(b,low);
                        let high=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let high=to_i64(b,high);
                        let low=b.ins().band_imm(low,0xffff_ffff);let high=b.ins().ishl_imm(high,32);
                        let value=b.ins().bor(low,high);let zero=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,value,0);
                        let one=b.ins().iconst(types::I64,1);let value=b.ins().select(zero,one,value);
                        b.ins().store(MemFlags::new(),value,address,0);
                    } else {
                        let state=b.ins().load(types::I64,MemFlags::new(),address,0);
                        for (word,arg) in args.iter().enumerate() {
                            let IExpr::Index(array,indices)=arg else {anyhow::bail!("invalid RANDOM_SEED GET target")};
                            let index=eval_expr(b,ex,ptr_ty,&indices[0],ints,reals,bools,func_meta);let index=to_i64(b,index);
                            let index=b.ins().iadd_imm(index,-1);let offset=b.ins().imul_imm(index,8);
                            let base=storage_addr(b,ptr_ty,ints[array]);let dest=b.ins().iadd(base,offset);
                            let bits=b.ins().ushr_imm(state,(word*32) as i64);let bits=b.ins().ireduce(types::I32,bits);let bits=b.ins().sextend(types::I64,bits);
                            b.ins().store(MemFlags::new(),bits,dest,0);
                        }
                    }
                    continue;
                }
                if name == "@ptr_bind" {
                    let [IExpr::Ident(data),IExpr::Ident(flags),IExpr::Ident(label),IExpr::Ident(target),IExpr::Ident(flag)] = args.as_slice() else {anyhow::bail!("invalid pointer storage binding");};
                    let slot=ints.get(target).or_else(||reals.get(target)).or_else(||bools.get(target)).ok_or_else(||anyhow::anyhow!("unknown pointer target `{target}`"))?;
                    let address=storage_addr(b,ptr_ty,*slot);let destination=storage_addr(b,ptr_ty,ints[data]);b.ins().store(MemFlags::new(),address,destination,0);
                    let address=storage_addr(b,ptr_ty,ints[flag]);let destination=storage_addr(b,ptr_ty,ints[flags]);b.ins().store(MemFlags::new(),address,destination,0);
                    let mut text=target.as_bytes().to_vec();text.push(0);let address=make_data(module,b,ptr_ty,dc,&text)?;
                    let destination=storage_addr(b,ptr_ty,ints[label]);b.ins().store(MemFlags::new(),address,destination,0);
                    continue;
                }
                if let Some(tag)=name.strip_prefix("@ptr_store_c_") {
                    let base=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let base=to_i64(b,base);
                    let index=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let index=to_i64(b,index);
                    let value=eval_expr(b,ex,ptr_ty,&args[3],ints,reals,bools,func_meta);
                    c_interop::raw_store(b,base,index,value,tag)?;continue;
                }
                if let Some(kind)=name.strip_prefix("@ptr_store_") {
                    let base=eval_expr(b,ex,ptr_ty,&args[0],ints,reals,bools,func_meta);let base=to_i64(b,base);
                    let flags=eval_expr(b,ex,ptr_ty,&args[1],ints,reals,bools,func_meta);let flags=to_i64(b,flags);
                    let offset=eval_expr(b,ex,ptr_ty,&args[2],ints,reals,bools,func_meta);let offset=to_i64(b,offset);let bytes=b.ins().imul_imm(offset,8);
                    let address=b.ins().iadd(base,bytes);let flag=b.ins().iadd(flags,bytes);
                    let value=eval_expr(b,ex,ptr_ty,&args[3],ints,reals,bools,func_meta);
                    if kind=="clear" {let zero=b.ins().iconst(types::I64,0);b.ins().store(MemFlags::new(),zero,address,0);continue;}
                    let value=match kind {"r4"=>{let v=to_f64(b,value);let narrow=b.ins().fdemote(types::F32,v);b.ins().fpromote(types::F64,narrow)},"r8"=>to_f64(b,value),"l"=>to_bool(b,value),_=>to_i64(b,value)};
                    b.ins().store(MemFlags::new(),value,address,0);let one=b.ins().iconst(types::I64,1);b.ins().store(MemFlags::new(),one,flag,0);
                    continue;
                }
                if name == "@check_initialized" {
                    let value = eval_expr(b, ex, ptr_ty, &args[0], ints, reals, bools, func_meta);
                    let initialized = to_bool(b, value);
                    let success = b.create_block();
                    let failure = b.create_block();
                    b.ins().brif(initialized, success, &[], failure, &[]);
                    b.switch_to_block(failure); b.seal_block(failure);
                    let variable = match &args[1] {IExpr::Str(variable)=>variable.as_str(),IExpr::Call(n,_) if n=="@ptr_name"=>"%s",_=>anyhow::bail!("invalid initialization guard")};
                    let location = match args.get(3) { Some(IExpr::Str(location)) => format!(" at {location}"), _ => String::new() };
                    let (pointer, length) = if let Some(index) = args.get(2).filter(|e| !matches!(e, IExpr::Str(_))) {
                        let index = eval_expr(b, ex, ptr_ty, index, ints, reals, bools, func_meta);
                        let index = to_i64(b, index);
                        let message = format!("Runtime error: read of uninitialized variable `{variable}`: element %lld{}\n\0", location.replace('%', "%%"));
                        let format = make_data(module, b, ptr_ty, dc, message.as_bytes())?;
                        // The variable name is fixed; 32 extra bytes cover any signed 64-bit index.
                        let name_length=if let IExpr::Call(_,a)=&args[1] {if let Some(IExpr::IntLit(n))=a.get(1){n.parse::<usize>()?}else{0}}else{0};
                        let buffer = b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot, (message.len() + name_length + 32) as u32, 0));
                        let pointer = b.ins().stack_addr(ptr_ty, buffer, 0);
                        let call = if let IExpr::Call(_,a)=&args[1] {let name=eval_expr(b,ex,ptr_ty,&a[0],ints,reals,bools,func_meta);let name=to_i64(b,name);let mut sig=module.make_signature();sig.params.extend([ir::AbiParam::new(ptr_ty),ir::AbiParam::new(ptr_ty),ir::AbiParam::new(ptr_ty),ir::AbiParam::new(types::I64)]);sig.returns.push(ir::AbiParam::new(types::I32));let sig=b.import_signature(sig);let address=b.ins().func_addr(ptr_ty,ex.sprintf);b.ins().call_indirect(sig,address,&[pointer,format,name,index])}else{ex.call_sprintf(b, &[pointer, format, index])};
                        (pointer, b.inst_results(call)[0])
                    } else {
                        let message = format!("Runtime error: read of uninitialized variable `{variable}`{location}\n");
                        let pointer = make_data(module, b, ptr_ty, dc, message.as_bytes())?;
                        (pointer, b.ins().iconst(types::I32, message.len() as i64))
                    };
                    let mut sig = module.make_signature();
                    sig.params.extend([ir::AbiParam::new(types::I32), ir::AbiParam::new(ptr_ty), ir::AbiParam::new(types::I32)]);
                    sig.returns.push(ir::AbiParam::new(types::I32));
                    let id = module.declare_function(if cfg!(windows) { "_write" } else { "rfort_write" }, Linkage::Import, &sig)?;
                    let function = module.declare_func_in_func(id, b.func);
                    let stderr = b.ins().iconst(types::I32, 2);
                    b.ins().call(function, &[stderr, pointer, length]);
                    let mut sig = module.make_signature();
                    sig.params.push(ir::AbiParam::new(types::I32));
                    let id = module.declare_function("exit", Linkage::Import, &sig)?;
                    let function = module.declare_func_in_func(id, b.func);
                    let code = b.ins().iconst(types::I32, 1);
                    b.ins().call(function, &[code]);
                    b.ins().trap(ir::TrapCode::unwrap_user(1));
                    b.switch_to_block(success); b.seal_block(success);
                    continue;
                }
                if name.eq_ignore_ascii_case("random_number") && !func_meta.contains_key(name) {
                    let (id, indices) = match &args[0] {
                        IExpr::Ident(id) => (id, None),
                        IExpr::Index(id, indices) => (id, Some(indices)),
                        _ => anyhow::bail!("invalid RANDOM_NUMBER destination"),
                    };
                    let slot = reals.get(id).ok_or_else(|| anyhow::anyhow!("RANDOM_NUMBER requires a real destination"))?;
                    let base = storage_addr(b, ptr_ty, *slot);
                    let state_id = module.declare_data(".f90c.random_state", Linkage::Local, true, false)?;
                    let state_global = module.declare_data_in_func(state_id, b.func);
                    let state = b.ins().global_value(ptr_ty, state_global);
                    let double = *real_kind.get(id).unwrap_or(&false);
                    if let Some(indices) = indices {
                        let index = eval_expr(b, ex, ptr_ty, &indices[0], ints, reals, bools, func_meta);
                        let index = to_i64(b, index);
                        let index = b.ins().iadd_imm(index, -1);
                        let offset = b.ins().imul_imm(index, 8);
                        let address = b.ins().iadd(base, offset);
                        let value = random_value(b, state, double);
                        b.ins().store(MemFlags::new(), value, address, 0);
                    } else if let Some(len) = arrays.get(id) {
                        let loop_block = b.create_block();
                        let after = b.create_block();
                        b.append_block_param(loop_block, ptr_ty);
                        let end = b.ins().iadd_imm(base, (*len as i64) * 8);
                        b.ins().jump(loop_block, &[base.into()]);
                        b.switch_to_block(loop_block);
                        let address = b.block_params(loop_block)[0];
                        let value = random_value(b, state, double);
                        b.ins().store(MemFlags::new(), value, address, 0);
                        let next = b.ins().iadd_imm(address, 8);
                        let more = b.ins().icmp(ir::condcodes::IntCC::UnsignedLessThan, next, end);
                        b.ins().brif(more, loop_block, &[next.into()], after, &[]);
                        b.seal_block(loop_block);
                        b.switch_to_block(after);
                        b.seal_block(after);
                    } else {
                        let value = random_value(b, state, double);
                        b.ins().store(MemFlags::new(), value, base, 0);
                    }
                    continue;
                }
                if let Some(meta) = func_meta.get(name) {
                    let mut argv = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let v = eval_expr(b, ex, ptr_ty, a, ints, reals, bools, func_meta);
                        match meta.param_kinds.get(i).copied().unwrap_or(ParamKind::I64) {
                            ParamKind::I64 => argv.push(to_i64(b, v)),
                            ParamKind::F64 => argv.push(to_f64(b, v)),
                        }
                    }
                    b.ins().call(meta.fref, &argv);
                }
            }
            IStmt::Read(args) => {
                for a in args {
                    if let IExpr::Ident(id) = a {
                        if let Some(ss) = ints.get(id) {
                            let buf = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                16,
                                0,
                            ));
                            let p = b.ins().stack_addr(ptr_ty, buf, 0);
                            ex.call_scanf(b, &[ex.fmt_i64, p]);
                            let val = b.ins().load(ir::types::I64, MemFlags::new(), p, 0);
                            let dst = storage_addr(b, ptr_ty, *ss);
                            b.ins().store(MemFlags::new(), val, dst, 0);
                        } else if let Some(ss) = reals.get(id) {
                            let buf = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                32,
                                0,
                            ));
                            let p = b.ins().stack_addr(ptr_ty, buf, 0);
                            ex.call_scanf(b, &[ex.fmt_f64, p]);
                            let sf = b.ins().load(ir::types::F64, MemFlags::new(), p, 0);
                            let dst = storage_addr(b, ptr_ty, *ss);
                            b.ins().store(MemFlags::new(), sf, dst, 0);
                        } else if let Some(ss) = bools.get(id) {
                            let buf = b.create_sized_stack_slot(ir::StackSlotData::new(
                                ir::StackSlotKind::ExplicitSlot,
                                8,
                                0,
                            ));
                            let p = b.ins().stack_addr(ptr_ty, buf, 0);
                            ex.call_scanf(b, &[ex.fmt_s, p]);
                            let v = b.ins().load(ir::types::I8, MemFlags::new(), p, 0);
                            let dst = storage_addr(b, ptr_ty, *ss);
                            b.ins().store(MemFlags::new(), v, dst, 0);
                        } else if let Some((ss, _len)) = chars.get(id) {
                            let dst = storage_addr(b, ptr_ty, *ss);
                            ex.call_scanf(b, &[ex.fmt_s, dst]);
                        }
                    }
                }
                // Advancing input consumes the remainder of its final record.
                // Otherwise a following (A) READ would see scanf's leftover newline.
                let mut sig=module.make_signature();
                sig.returns.push(ir::AbiParam::new(types::I32));
                let id=module.declare_function("getchar",Linkage::Import,&sig)?;
                let getchar=module.declare_func_in_func(id,b.func);
                let discard=b.create_block();let done=b.create_block();
                b.ins().jump(discard,&[]);b.switch_to_block(discard);
                let call=b.ins().call(getchar,&[]);let ch=b.inst_results(call)[0];
                let eof=b.ins().icmp_imm(ir::condcodes::IntCC::SignedLessThan,ch,0);
                let newline=b.ins().icmp_imm(ir::condcodes::IntCC::Equal,ch,10);
                let end=b.ins().bor(eof,newline);
                b.ins().brif(end,done,&[],discard,&[]);
                b.seal_block(discard);b.switch_to_block(done);b.seal_block(done);
            }
            IStmt::SelectCase {
                selector,
                cases,
                default,
            } => {
                let cont_blk = b.create_block();

                let mut case_blocks: Vec<ir::Block> = Vec::new();
                for _ in cases.iter() {
                    case_blocks.push(b.create_block());
                }
                let default_blk = if default.is_some() {
                    Some(b.create_block())
                } else {
                    None
                };

                let mut cmp_next_blocks: Vec<ir::Block> = Vec::new();
                match selector {
                    IExpr::IntLit(s) => {
                        let value: i64 = s.parse().context("SELECT CASE integer selector is out of range")?;
                        let mut selected = None;
                        for (ci, case) in cases.iter().enumerate() {
                            for item in &case.items {
                                let constant = |e: &IExpr| -> Result<i64> {
                                    if let IExpr::IntLit(s) = e { Ok(s.parse()?) }
                                    else { bail!("integer CASE values must be constant INTEGER expressions") }
                                };
                                let hit = match item {
                                    IcCaseItem::Single(e) => value == constant(e)?,
                                    IcCaseItem::Range(a,b) => value >= constant(a)? && value <= constant(b)?,
                                    IcCaseItem::Multi(items) => {
                                        let mut hit = false;
                                        for item in items { hit |= value == constant(item)?; }
                                        hit
                                    }
                                };
                                if hit { selected = Some(ci); break; }
                            }
                            if selected.is_some() { break; }
                        }
                        if let Some(ci) = selected {
                            let next = b.create_block();
                            cmp_next_blocks.push(next);
                            let yes = b.ins().iconst(ir::types::I8,1);
                            b.ins().brif(yes,case_blocks[ci],&[],next,&[]);
                            b.switch_to_block(next);
                        }
                    }
                    IExpr::Ident(sid) => {
                        if let Some(ss) = ints.get(sid) {
                            let sel_addr = storage_addr(b, ptr_ty, *ss);
                            for (ci, icase) in cases.iter().enumerate() {
                                for item in &icase.items {
                                    match item {
                                        IcCaseItem::Single(e) => {
                                            if let IExpr::IntLit(s) = e {
                                                if let Ok(v128) =
                                                    i128::from_str_radix(s.as_str(), 10)
                                                {
                                                    let v = i64::try_from(v128).unwrap_or(0);
                                                    let lit = b.ins().iconst(ir::types::I64, v);
                                                    let selv = b.ins().load(
                                                        ir::types::I64,
                                                        ir::MemFlags::new(),
                                                        sel_addr,
                                                        0,
                                                    );
                                                    let cmp = b.ins().icmp(
                                                        ir::condcodes::IntCC::Equal,
                                                        selv,
                                                        lit,
                                                    );

                                                    let next = b.create_block();
                                                    cmp_next_blocks.push(next);
                                                    b.ins().brif(
                                                        cmp,
                                                        case_blocks[ci],
                                                        &[],
                                                        next,
                                                        &[],
                                                    );
                                                    b.switch_to_block(next);
                                                }
                                            } else if let IExpr::Ident(id2) = e {
                                                if let Some(ss2) = ints.get(id2) {
                                                    let a2 = storage_addr(b, ptr_ty, *ss2);
                                                    let v2 = b.ins().load(
                                                        ir::types::I64,
                                                        ir::MemFlags::new(),
                                                        a2,
                                                        0,
                                                    );
                                                    let selv = b.ins().load(
                                                        ir::types::I64,
                                                        ir::MemFlags::new(),
                                                        sel_addr,
                                                        0,
                                                    );
                                                    let cmp = b.ins().icmp(
                                                        ir::condcodes::IntCC::Equal,
                                                        selv,
                                                        v2,
                                                    );
                                                    let next = b.create_block();
                                                    cmp_next_blocks.push(next);
                                                    b.ins().brif(
                                                        cmp,
                                                        case_blocks[ci],
                                                        &[],
                                                        next,
                                                        &[],
                                                    );
                                                    b.switch_to_block(next);
                                                }
                                            }
                                        }
                                        IcCaseItem::Range(l, r) => {
                                            if let (IExpr::IntLit(ls), IExpr::IntLit(rs)) = (l, r) {
                                                if let (Ok(lv128), Ok(rv128)) = (
                                                    i128::from_str_radix(ls.as_str(), 10),
                                                    i128::from_str_radix(rs.as_str(), 10),
                                                ) {
                                                    let lv = i64::try_from(lv128).unwrap_or(0);
                                                    let rv = i64::try_from(rv128).unwrap_or(0);
                                                    let lit_l = b.ins().iconst(ir::types::I64, lv);
                                                    let lit_r = b.ins().iconst(ir::types::I64, rv);
                                                    let selv = b.ins().load(
                                                        ir::types::I64,
                                                        ir::MemFlags::new(),
                                                        sel_addr,
                                                        0,
                                                    );
                                                    let ge = b.ins().icmp(ir::condcodes::IntCC::SignedGreaterThanOrEqual, selv, lit_l);
                                                    let le = b.ins().icmp(
                                                        ir::condcodes::IntCC::SignedLessThanOrEqual,
                                                        selv,
                                                        lit_r,
                                                    );
                                                    let both = b.ins().band(ge, le);
                                                    let next = b.create_block();
                                                    cmp_next_blocks.push(next);
                                                    b.ins().brif(
                                                        both,
                                                        case_blocks[ci],
                                                        &[],
                                                        next,
                                                        &[],
                                                    );
                                                    b.switch_to_block(next);
                                                }
                                            }
                                        }
                                        IcCaseItem::Multi(es) => {
                                            for e in es.iter() {
                                                if let IExpr::IntLit(s) = e {
                                                    if let Ok(v128) =
                                                        i128::from_str_radix(s.as_str(), 10)
                                                    {
                                                        let v = i64::try_from(v128).unwrap_or(0);
                                                        let lit = b.ins().iconst(ir::types::I64, v);
                                                        let selv = b.ins().load(
                                                            ir::types::I64,
                                                            ir::MemFlags::new(),
                                                            sel_addr,
                                                            0,
                                                        );
                                                        let cmp = b.ins().icmp(
                                                            ir::condcodes::IntCC::Equal,
                                                            selv,
                                                            lit,
                                                        );
                                                        let next = b.create_block();
                                                        cmp_next_blocks.push(next);
                                                        b.ins().brif(
                                                            cmp,
                                                            case_blocks[ci],
                                                            &[],
                                                            next,
                                                            &[],
                                                        );
                                                        b.switch_to_block(next);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if let Some((ss, _len)) = chars.get(sid) {
                            let _selptr = storage_addr(b, ptr_ty, *ss);
                            for (ci, icase) in cases.iter().enumerate() {
                                for item in &icase.items {
                                    match item {
                                        IcCaseItem::Single(e) => {
                                            if let IExpr::Str(lit) = e {
                                                let bytes = lit.as_bytes();
                                                let litp = make_data(module, b, ptr_ty, dc, bytes)?;
                                                let selp = storage_addr(b, ptr_ty, *ss);
                                                let sel_length=characters::view(module,b,ptr_ty,dc,chars,ints,&IExpr::Ident(sid.clone()))?.length;
                                                let lit_length=b.ins().iconst(ptr_ty,bytes.len() as i64);
                                                let call = ex.call_character_compare(b, &[selp, sel_length, litp, lit_length]);
                                                let res = b.inst_results(call)[0];
                                                let zero = b.ins().iconst(ir::types::I32, 0);
                                                let eq = b.ins().icmp(
                                                    ir::condcodes::IntCC::Equal,
                                                    res,
                                                    zero,
                                                );
                                                let next = b.create_block();
                                                cmp_next_blocks.push(next);
                                                b.ins().brif(eq, case_blocks[ci], &[], next, &[]);
                                                b.switch_to_block(next);
                                            }
                                        }
                                        IcCaseItem::Multi(es) => {
                                            for e in es.iter() {
                                                if let IExpr::Str(lit) = e {
                                                    let bytes = lit.as_bytes();
                                                    let litp =
                                                        make_data(module, b, ptr_ty, dc, bytes)?;
                                                    let selp = storage_addr(b, ptr_ty, *ss);
                                                    let sel_length=characters::view(module,b,ptr_ty,dc,chars,ints,&IExpr::Ident(sid.clone()))?.length;
                                                let lit_length=b.ins().iconst(ptr_ty,bytes.len() as i64);
                                                let call = ex.call_character_compare(b, &[selp, sel_length, litp, lit_length]);
                                                    let res = b.inst_results(call)[0];
                                                    let zero = b.ins().iconst(ir::types::I32, 0);
                                                    let eq = b.ins().icmp(
                                                        ir::condcodes::IntCC::Equal,
                                                        res,
                                                        zero,
                                                    );
                                                    let next = b.create_block();
                                                    cmp_next_blocks.push(next);
                                                    b.ins().brif(
                                                        eq,
                                                        case_blocks[ci],
                                                        &[],
                                                        next,
                                                        &[],
                                                    );
                                                    b.switch_to_block(next);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }

                b.ins().jump(default_blk.unwrap_or(cont_blk), &[]);
                let mut all_terminated = default_blk.is_some();

                for nb in cmp_next_blocks.iter() {
                    b.seal_block(*nb);
                }

                for (ci, icase) in cases.iter().enumerate() {
                    b.switch_to_block(case_blocks[ci]);
                    let tr = gen_stmts(
                        b,
                        ex,
                        ptr_ty,
                        module,
                        dc,
                        &icase.body,
                        ints,
                        reals,
                        bools,
                        chars,
                        real_kind,
                        i128_vars,
                        i128_last,
                        func_meta,
                        current_fn,
                        current_loop_after,
                        current_loop_continue,
                        arrays,
                        tokens,
                    )?;
                    all_terminated &= tr;
                    if !tr { b.ins().jump(cont_blk, &[]); }

                    b.seal_block(case_blocks[ci]);
                }

                if let Some(def_blk) = default_blk {
                    b.switch_to_block(def_blk);
                    let mut tr = false;
                    if let Some(dbody) = default {
                        tr = gen_stmts(
                            b, ex, ptr_ty, module, dc, dbody, ints, reals, bools, chars, real_kind,
                            i128_vars, i128_last, func_meta, current_fn, current_loop_after, current_loop_continue, arrays,
                            tokens,
                        )?;
                    }
                    all_terminated &= tr;
                    if !tr { b.ins().jump(cont_blk, &[]); }
                    b.seal_block(def_blk);
                }

                if all_terminated { return Ok(true); }
                b.seal_block(cont_blk);
                b.switch_to_block(cont_blk);
            }
        }
    }
    Ok(false)
}

struct FuncMeta {
    fref: ir::FuncRef,
    ret: RetKind,
    param_kinds: Vec<ParamKind>,
    /// A REAL result is DOUBLE PRECISION (for list-directed output).
    double: bool,
}

thread_local! {
    /// Calls that expression code generation could not resolve. Code generation
    /// continues with a placeholder value so the error can name every call.
    static UNRESOLVED_CALLS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Declared INTEGER kinds other than the default, for B, O and Z output.
    static INTEGER_KINDS: std::cell::RefCell<HashMap<String, u8>> = std::cell::RefCell::new(HashMap::new());
}

// The kind of an INTEGER output item, 0 when it is the default or unknown.
fn integer_kind(e: &IExpr) -> i64 {
    match e {
        IExpr::Ident(n) | IExpr::Index(n, _) => INTEGER_KINDS.with(|k| k.borrow().get(n).copied()).map_or(0, i64::from),
        IExpr::Call(n, args) if n == "@integer_literal" || n == "int" && args.len() == 2 => match args.get(1) {
            Some(IExpr::IntLit(k)) => k.parse().unwrap_or(0),
            _ => 0,
        },
        IExpr::Un(_, a) => integer_kind(a),
        IExpr::Bin(_, a, b) => integer_kind(a).max(integer_kind(b)),
        _ => 0,
    }
}

impl Backend for CraneliftBackend {
    fn compile(&self, module: &Module, tokens: Option<&[crate::lexer::Token]>) -> Result<Vec<u8>> {
        UNRESOLVED_CALLS.with(|calls| calls.borrow_mut().clear());
        let object = self.compile_module(module, tokens)?;
        let mut calls = UNRESOLVED_CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()));
        calls.sort();
        calls.dedup();
        if !calls.is_empty() {
            let names = calls.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
            anyhow::bail!("unsupported intrinsic or undefined function {names}");
        }
        Ok(object)
    }
}

impl CraneliftBackend {
    fn compile_module(&self, module: &Module, tokens: Option<&[crate::lexer::Token]>) -> Result<Vec<u8>> {
        use crate::ast::TypeSpec;
        use cranelift_module::FuncId;
        use ir::{types, AbiParam};
        let triple = self.triple.clone().unwrap_or_else(Triple::host);
        let isa = isa::lookup(triple.clone())?.finish(self.flags.clone())?;
        let builder = ObjectBuilder::new(
            isa.clone(),
            "f90c",
            cranelift_module::default_libcall_names(),
        )?;
        let mut obj = ObjectModule::new(builder);
        if let Some(owners) = &self.module_owners {
            module_data::declare(&mut obj, module, owners)?;
        }
        let file_units = obj.declare_data(".rfort.file_units", Linkage::Local, true, false)?;
        let mut file_data = DataDescription::new();
        file_data.define_zeroinit(2048 * 8);
        file_data.set_align(8);
        obj.define_data(file_units, &file_data)?;
        let file_names=obj.declare_data(".rfort.file_names",Linkage::Local,true,false)?;
        obj.define_data(file_names,&file_data)?;
        let file_actions=obj.declare_data(".rfort.file_actions",Linkage::Local,true,false)?;
        obj.define_data(file_actions,&file_data)?;
        let random_state = obj.declare_data(".f90c.random_state", Linkage::Local, true, false)?;
        let mut random_data = DataDescription::new();
        random_data.define(1u64.to_le_bytes().to_vec().into_boxed_slice());
        random_data.set_align(8);
        obj.define_data(random_state, &random_data)?;
        let mut func_ids: HashMap<String, FuncId> = HashMap::new();
        let tokens_opt = tokens;
        let mut eff_ret: HashMap<String, crate::ast::TypeSpec> = HashMap::new();
        let mut param_kinds_map: HashMap<String, Vec<ParamKind>> = HashMap::new();
        let mut param_double_flags: HashMap<String, Vec<bool>> = HashMap::new();
        for f in module.funcs.iter() {
            let mut rt = f.return_type.clone();
            if rt.is_none() {
                for st in &f.body {
                    if let IStmt::DeclVar(ts, n, _) = st {
                        if n == &f.name {
                            rt = Some(ts.clone());
                            break;
                        }
                    }
                }
            }
            let rt_final = rt.unwrap_or(crate::ast::TypeSpec::Integer(None));
            eff_ret.insert(f.name.clone(), rt_final.clone());
            let mut pk: Vec<ParamKind> = Vec::new();
            let mut pdf: Vec<bool> = Vec::new();
            for pname in &f.params {
                let mut found = None;
                for st in &f.body {
                    if let IStmt::DeclVar(ts, n, _) = st {
                        if n == pname {
                            found = Some(ts.clone());
                            break;
                        }
                    }
                }
                let ts = found.unwrap_or(crate::ast::TypeSpec::Integer(None));
                match ts {
                    crate::ast::TypeSpec::Real => {
                        pk.push(ParamKind::F64);
                        pdf.push(false);
                    }
                    crate::ast::TypeSpec::DoublePrecision => {
                        pk.push(ParamKind::F64);
                        pdf.push(true);
                    }
                    _ => {
                        pk.push(ParamKind::I64);
                        pdf.push(false);
                    }
                }
            }
            param_kinds_map.insert(f.name.clone(), pk);
            param_double_flags.insert(f.name.clone(), pdf);
        }
        let defined_names: HashSet<String> = module
            .funcs
            .iter()
            .skip(1)
            .map(|f| f.name.clone())
            .collect();
        fn collect_calls_expr(e: &IExpr, out: &mut Vec<(String, usize)>) {
            match e {
                IExpr::Call(n, args) => {
                    out.push((n.clone(), args.len()));
                    for a in args {
                        collect_calls_expr(a, out);
                    }
                }
                IExpr::Bin(_, l, r) => {
                    collect_calls_expr(l, out);
                    collect_calls_expr(r, out);
                }
                IExpr::Un(_, x) => collect_calls_expr(x, out),
                _ => {}
            }
        }
        fn collect_calls_body(sts: &[IStmt], out: &mut Vec<(String, usize)>) {
            for st in sts {
                match st {
                    IStmt::Call(n, args) => {
                        out.push((n.clone(), args.len()));
                        for arg in args { collect_calls_expr(arg, out); }
                    }
                    IStmt::AssignIndex(_, indices, value) => {
                        for index in indices { collect_calls_expr(index, out); }
                        collect_calls_expr(value, out);
                    }
                    IStmt::AssignExpr(_, e) => collect_calls_expr(e, out),
                    IStmt::Return(Some(e)) => collect_calls_expr(e, out),
                    IStmt::Print(items) => {
                        for e in items {
                            collect_calls_expr(e, out)
                        }
                    }
                    IStmt::Read(args) => {
                        for e in args {
                            collect_calls_expr(e, out);
                        }
                    }
                    IStmt::If {
                        cond,
                        then_body,
                        else_body,
                    } => {
                        collect_calls_expr(cond, out);
                        collect_calls_body(then_body, out);
                        if let Some(eb) = else_body {
                            collect_calls_body(eb, out);
                        }
                    }
                    IStmt::Do {
                        start, end, body, ..
                    } => {
                        collect_calls_expr(start, out);
                        collect_calls_expr(end, out);
                        collect_calls_body(body, out);
                    }
                    IStmt::DoWhile { cond, body } => {
                        collect_calls_expr(cond, out);
                        collect_calls_body(body, out);
                    }
                    IStmt::Block { body } => collect_calls_body(body, out),
                    IStmt::SelectCase { selector, cases, default } => {
                        collect_calls_expr(selector, out);
                        for case in cases { collect_calls_body(&case.body, out); }
                        if let Some(body) = default { collect_calls_body(body, out); }
                    }
                    _ => {}
                }
            }
        }
        let mut call_list = Vec::new();
        for f in &module.funcs {
            collect_calls_body(&f.body, &mut call_list);
        }
        let mut import_arity: HashMap<String, usize> = HashMap::new();
        for (n, a) in call_list {
            if !defined_names.contains(&n) {
                let e = import_arity.entry(n).or_insert(0);
                *e = max(*e, a);
            }
        }

        for name in import_arity.keys().cloned().collect::<Vec<_>>() {
            if let Some(kind)=crate::real_intrinsics::tag_kind(&name) {eff_ret.insert(name.clone(),kind);continue;}
            let lname = name.to_ascii_lowercase();
            match lname.as_str() {
                "abs" | "aimag" | "aint" | "anint" | "cmplx" | "conjg" | "dble" | "real"
                | "dim" | "dprod" | "db" | "sqrt" | "exp" | "log" | "log10" | "sin"
                | "cos" | "tan" | "asin" | "acos" | "atan" | "atan2" | "sinh" | "cosh" | "tanh"
                | "scale" | "set_exponent" | "nearest" | "fraction" | "exponent" | "spacing"
                | "rrspacing" | "precision" | "radix" | "range" | "tiny" | "epsilon" | "huge"
                | "max" | "min" | "sum" | "matmul" | "maxval" | "minval"
                | "product" => {
                    eff_ret.insert(name.clone(), crate::ast::TypeSpec::Real);
                    let ar = *import_arity.get(&name).unwrap_or(&1usize);
                    param_kinds_map.insert(name.clone(), vec![ParamKind::F64; ar]);
                    param_double_flags.insert(name.clone(), vec![false; ar]);
                }

                "int" | "nint" | "floor" | "ceiling" | "ichar" | "iachar" | "len" | "len_trim" | "count" | "size"
                | "lbound" | "ubound" | "maxloc" | "minloc" | "index" | "scan" | "verify" => {
                    eff_ret.insert(name.clone(), crate::ast::TypeSpec::Integer(None));
                    let ar = *import_arity.get(&name).unwrap_or(&1usize);

                    param_kinds_map.insert(name.clone(), vec![ParamKind::F64; ar]);
                    param_double_flags.insert(name.clone(), vec![false; ar]);
                }

                "all" | "any" | "allocated" | "lge" | "lgt" | "lle" | "llt" | "isnan" => {
                    eff_ret.insert(name.clone(), crate::ast::TypeSpec::Logical);
                    let ar = *import_arity.get(&name).unwrap_or(&1usize);
                    param_kinds_map.insert(name.clone(), vec![ParamKind::F64; ar]);
                    param_double_flags.insert(name.clone(), vec![false; ar]);
                }

                "btest" | "iand" | "ibclr" | "ibits" | "ibset" | "ieor" | "ior" | "ishft"
                | "ishftc" | "not" | "mod" | "modulo" | "sign" => {
                    eff_ret.insert(name.clone(), crate::ast::TypeSpec::Integer(None));
                    let ar = *import_arity.get(&name).unwrap_or(&1usize);
                    param_kinds_map.insert(name.clone(), vec![ParamKind::I64; ar]);
                    param_double_flags.insert(name.clone(), vec![false; ar]);
                }

                "achar" | "char" | "repeat" | "trim" | "adjustl" | "adjustr" | "reshape"
                | "transpose" | "spread" | "pack" | "unpack" | "merge" => {
                    eff_ret.insert(name.clone(), crate::ast::TypeSpec::Integer(None));
                    let ar = *import_arity.get(&name).unwrap_or(&1usize);
                    param_kinds_map.insert(name.clone(), vec![ParamKind::I64; ar]);
                    param_double_flags.insert(name.clone(), vec![false; ar]);
                }
                _ => {}
            }
        }
        let entry_func_name = module
            .funcs
            .first()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "main".into());
        let is_real_main = entry_func_name == "main" && !module.funcs.is_empty();
        let c_symbols=c_interop::symbols(module);
        for (idx, f) in module.funcs.iter().enumerate() {
            if idx == 0 && is_real_main {
                continue;
            }
            let sig_rt = eff_ret.get(&f.name).unwrap();
            let pk = param_kinds_map.get(&f.name).unwrap();
            let mut ctx = obj.make_context();
            for k in pk {
                match k {
                    ParamKind::I64 => ctx
                        .func
                        .signature
                        .params
                        .push(ir::AbiParam::new(ir::types::I64)),
                    ParamKind::F64 => ctx
                        .func
                        .signature
                        .params
                        .push(ir::AbiParam::new(ir::types::F64)),
                }
            }
            match sig_rt {
                    crate::ast::TypeSpec::Referenced { .. } | crate::ast::TypeSpec::CharacterLength(_) | crate::ast::TypeSpec::CharacterAssumed | crate::ast::TypeSpec::RealKind(_) | crate::ast::TypeSpec::LogicalKind(_) | crate::ast::TypeSpec::IntegerKind(_) | crate::ast::TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                crate::ast::TypeSpec::Integer(_) | crate::ast::TypeSpec::Logical => ctx
                    .func
                    .signature
                    .returns
                    .push(ir::AbiParam::new(ir::types::I64)),
                crate::ast::TypeSpec::Real | crate::ast::TypeSpec::DoublePrecision => ctx
                    .func
                    .signature
                    .returns
                    .push(ir::AbiParam::new(ir::types::F64)),
                crate::ast::TypeSpec::Character(_) => {}
            }
            // Module procedures and generated specializations are private to
            // this object; their definitions are carried in the .rmod file.
            let linkage=if f.name.starts_with('@') || f.name.starts_with("rfort$@module$") || f.name == "__f90c_init" {Linkage::Local} else {Linkage::Export};
            let private_c_adapter=c_symbols.contains(&f.name);
            let symbol=if private_c_adapter {format!(".rfort.c_impl.{}",f.name)}else{f.name.clone()};
            let fid = obj.declare_function(&symbol, if private_c_adapter{Linkage::Local}else{linkage}, &ctx.func.signature)?;
            obj.clear_context(&mut ctx);
            func_ids.insert(f.name.clone(), fid);
        }
        for (name, arity) in &import_arity {
            if name.starts_with('@') || name.eq_ignore_ascii_case("random_seed") { continue; }
            if name.eq_ignore_ascii_case("random_number") { continue; }
            if let Some(arity) = crate::intrinsics::math_arity(name) {
                if !func_ids.contains_key(name) {
                    let mut sig = obj.make_signature();
                    for _ in 0..arity { sig.params.push(AbiParam::new(types::F64)); }
                    sig.returns.push(AbiParam::new(types::F64));
                    let fid = obj.declare_function(&crate::intrinsics::c_symbol(name), Linkage::Import, &sig)?;
                    func_ids.insert(name.clone(), fid);
                    eff_ret.insert(name.clone(), TypeSpec::Real);
                    param_kinds_map.insert(name.clone(), vec![ParamKind::F64; arity]);
                    param_double_flags.insert(name.clone(), vec![true; arity]);
                }
                continue;
            }
            if eff_ret.contains_key(name) {
                continue;
            }
            if func_ids.contains_key(name) {
                continue;
            }
            let mut ctx = obj.make_context();
            for _ in 0..*arity {
                ctx.func
                    .signature
                    .params
                    .push(ir::AbiParam::new(ir::types::I64));
            }
            ctx.func
                .signature
                .returns
                .push(ir::AbiParam::new(ir::types::I64));
            let fid = obj.declare_function(name, Linkage::Import, &ctx.func.signature)?;
            obj.clear_context(&mut ctx);
            func_ids.insert(name.clone(), fid);
            eff_ret.insert(name.clone(), crate::ast::TypeSpec::Integer(None));
            param_kinds_map.insert(name.clone(), vec![ParamKind::I64; *arity]);
            param_double_flags.insert(name.clone(), vec![false; *arity]);
            if !func_ids.contains_key(name) {
                let mut ctx = obj.make_context();
                for _ in 0..*arity {
                    ctx.func
                        .signature
                        .params
                        .push(ir::AbiParam::new(ir::types::I64));
                }
                ctx.func
                    .signature
                    .returns
                    .push(ir::AbiParam::new(ir::types::I64));
                let fid = obj.declare_function(name, Linkage::Import, &ctx.func.signature)?;
                obj.clear_context(&mut ctx);
                func_ids.insert(name.clone(), fid);
                eff_ret.insert(name.clone(), crate::ast::TypeSpec::Integer(None));
                param_kinds_map.insert(name.clone(), vec![ParamKind::I64; *arity]);
                param_double_flags.insert(name.clone(), vec![false; *arity]);
            }
        }
        let mut ret_kinds: HashMap<String, RetKind> = HashMap::new();
        for (name, rt) in &eff_ret {
            ret_kinds.insert(
                name.clone(),
                match rt {
                    crate::ast::TypeSpec::Referenced { .. } | crate::ast::TypeSpec::CharacterLength(_) | crate::ast::TypeSpec::CharacterAssumed | crate::ast::TypeSpec::RealKind(_) | crate::ast::TypeSpec::LogicalKind(_) | crate::ast::TypeSpec::IntegerKind(_) | crate::ast::TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                    crate::ast::TypeSpec::Logical => RetKind::Bool,
                    crate::ast::TypeSpec::Integer(_) => {
                        RetKind::I64
                    }
                    crate::ast::TypeSpec::Real | crate::ast::TypeSpec::DoublePrecision => {
                        RetKind::F64
                    }
                    crate::ast::TypeSpec::Character(_) => RetKind::Void,
                },
            );
        }
        for (idx, f) in module.funcs.iter().enumerate() {
            if idx == 0 && is_real_main {
                continue;
            }
            let rt = eff_ret.get(&f.name).unwrap().clone();
            let pk = param_kinds_map.get(&f.name).unwrap().clone();
            let pdf = param_double_flags.get(&f.name).unwrap().clone();
            let mut ctx_f = obj.make_context();
            for k in &pk {
                match k {
                    ParamKind::I64 => ctx_f.func.signature.params.push(AbiParam::new(types::I64)),
                    ParamKind::F64 => ctx_f.func.signature.params.push(AbiParam::new(types::F64)),
                }
            }
            match rt {
                    crate::ast::TypeSpec::Referenced { .. } | crate::ast::TypeSpec::CharacterLength(_) | crate::ast::TypeSpec::CharacterAssumed | crate::ast::TypeSpec::RealKind(_) | crate::ast::TypeSpec::LogicalKind(_) | crate::ast::TypeSpec::IntegerKind(_) | crate::ast::TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                TypeSpec::Integer(_) | TypeSpec::Logical => {
                    ctx_f.func.signature.returns.push(AbiParam::new(types::I64))
                }
                TypeSpec::Real | TypeSpec::DoublePrecision => {
                    ctx_f.func.signature.returns.push(AbiParam::new(types::F64))
                }
                TypeSpec::Character(_) => {}
            }
            let mut fbctx = FunctionBuilderContext::new();
            {
                let mut b = FunctionBuilder::new(&mut ctx_f.func, &mut fbctx);
                let entry = b.create_block();
                b.append_block_params_for_function_params(entry);
                b.switch_to_block(entry);
                b.seal_block(entry);
                let ptr_ty = isa.pointer_type();
                let mut ex = declare_externs(&mut obj, &mut b, ptr_ty)?;
                ex.enable_caching(&mut b, ptr_ty);

                let mut ints = HashMap::new();
                let mut reals = HashMap::new();
                let mut bools = HashMap::new();
                let mut chars = characters::Characters::new();
                let mut real_kind = HashMap::new();
                let mut i128_vars = HashSet::new();
                let mut i128_last = HashMap::new();
                let mut dc = 0u32;
                let _arrays: HashMap<String, usize> = HashMap::new();
                let mut func_meta: HashMap<String, FuncMeta> = HashMap::new();
                for (name, fid) in &func_ids {
                    let fr = obj.declare_func_in_func(*fid, b.func);
                    let rk = *ret_kinds.get(name).unwrap_or(&RetKind::Void);
                    let pkv = param_kinds_map.get(name).cloned().unwrap_or_default();
                    func_meta.insert(
                        name.clone(),
                        FuncMeta {
                            fref: fr,
                            ret: rk,
                            param_kinds: pkv,
                            double: matches!(eff_ret.get(name), Some(crate::ast::TypeSpec::DoublePrecision)),
                        },
                    );
                }
                for (idx, pname) in f.params.iter().enumerate() {
                    if let Some(length)=f.body.iter().find_map(|s|match s {
                        IStmt::DeclVar(TypeSpec::Character(length),name,_) if name==pname => Some(length.unwrap_or(1)),
                        _=>None,
                    }) {
                        let source=b.block_params(entry)[idx];
                        let source=if ptr_ty==types::I64 {source} else {b.ins().ireduce(ptr_ty,source)};
                        let slot=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,(length+1) as u32,0));
                        let address=b.ins().stack_addr(ptr_ty,slot,0);
                        let count=b.ins().iconst(ptr_ty,length as i64);
                        file_io::call(&mut obj,&mut b,"memcpy",&[address,source,count],ptr_ty)?;
                        let zero=b.ins().iconst(types::I8,0);
                        b.ins().store(ir::MemFlags::new(),zero,address,length as i32);
                        chars.insert(pname.clone(),(slot.into(),length));
                        let pointer=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,8,0));
                        let value=if ptr_ty==types::I64 {address} else {b.ins().uextend(types::I64,address)};
                        b.ins().stack_store(value,pointer,0);
                        ints.insert(format!("@character_pointer:{pname}"),pointer.into());
                        continue;
                    }
                    let slot = b.create_sized_stack_slot(ir::StackSlotData::new(
                        ir::StackSlotKind::ExplicitSlot,
                        8,
                        0,
                    ));
                    let pv = b.block_params(entry)[idx];
                    let addr = b.ins().stack_addr(ptr_ty, slot, 0);
                    match pk[idx] {
                        ParamKind::I64 => {
                            b.ins().store(ir::MemFlags::new(), pv, addr, 0);
                            ints.insert(pname.clone(), slot.into());
                        }
                        ParamKind::F64 => {
                            b.ins().store(ir::MemFlags::new(), pv, addr, 0);
                            reals.insert(pname.clone(), slot.into());
                            real_kind.insert(pname.clone(), pdf[idx]);
                        }
                    }
                }
                enum RetSlot {
                    I(ir::StackSlot),
                    Bool(ir::StackSlot),
                    F64(ir::StackSlot),
                }
                let ret_slot: Option<RetSlot> = match rt {
                    crate::ast::TypeSpec::Referenced { .. } | crate::ast::TypeSpec::CharacterLength(_) | crate::ast::TypeSpec::CharacterAssumed | crate::ast::TypeSpec::RealKind(_) | crate::ast::TypeSpec::LogicalKind(_) | crate::ast::TypeSpec::IntegerKind(_) | crate::ast::TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                    TypeSpec::Integer(_) => {
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        let addr = b.ins().stack_addr(ptr_ty, ss, 0);
                        let z = b.ins().iconst(types::I64, 0);
                        b.ins().store(ir::MemFlags::new(), z, addr, 0);
                        ints.insert(f.name.clone(), ss.into());
                        Some(RetSlot::I(ss))
                    }
                    TypeSpec::Logical => {
                        let ss=b.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,1,0));
                        let addr=b.ins().stack_addr(ptr_ty,ss,0);
                        let zero=b.ins().iconst(types::I8,0);
                        b.ins().store(ir::MemFlags::new(),zero,addr,0);
                        bools.insert(f.name.clone(),ss.into());
                        Some(RetSlot::Bool(ss))
                    }
                    TypeSpec::Real => {
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        let addr = b.ins().stack_addr(ptr_ty, ss, 0);
                        let z = b.ins().f64const(0.0);
                        b.ins().store(ir::MemFlags::new(), z, addr, 0);
                        reals.insert(f.name.clone(), ss.into());
                        real_kind.insert(f.name.clone(), false);
                        Some(RetSlot::F64(ss))
                    }
                    TypeSpec::DoublePrecision => {
                        let ss = b.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            8,
                            0,
                        ));
                        let addr = b.ins().stack_addr(ptr_ty, ss, 0);
                        let z = b.ins().f64const(0.0);
                        b.ins().store(ir::MemFlags::new(), z, addr, 0);
                        reals.insert(f.name.clone(), ss.into());
                        real_kind.insert(f.name.clone(), true);
                        Some(RetSlot::F64(ss))
                    }
                    TypeSpec::Character(_) => None,
                };
                let mut arrays: HashMap<String, usize> = HashMap::new();
                let did_ret = gen_stmts(
                    &mut b,
                    &ex,
                    ptr_ty,
                    &mut obj,
                    &mut dc,
                    &f.body,
                    &mut ints,
                    &mut reals,
                    &mut bools,
                    &mut chars,
                    &mut real_kind,
                    &mut i128_vars,
                    &mut i128_last,
                    &func_meta,
                    Some(&f.name),
                    None,
                    None,
                    &mut arrays,
                    tokens_opt,
                )?;
                if !did_ret {
                    if let Some(rs) = ret_slot {
                        match rs {
                            RetSlot::I(ss) => {
                                let addr = b.ins().stack_addr(ptr_ty, ss, 0);
                                let rv = b.ins().load(types::I64, ir::MemFlags::new(), addr, 0);
                                b.ins().return_(&[rv]);
                            }
                            RetSlot::F64(ss) => {
                                let addr = b.ins().stack_addr(ptr_ty, ss, 0);
                                let rv = b.ins().load(types::F64, ir::MemFlags::new(), addr, 0);
                                b.ins().return_(&[rv]);
                            }
                            RetSlot::Bool(ss) => {
                                let addr=b.ins().stack_addr(ptr_ty,ss,0);
                                let value=b.ins().load(types::I8,ir::MemFlags::new(),addr,0);
                                let value=b.ins().uextend(types::I64,value);
                                b.ins().return_(&[value]);
                            }
                        }
                    } else {
                        b.ins().return_(&[]);
                    }
                }
                b.finalize();
                chars.finish(&mut obj, &mut ctx_f.func, ptr_ty)?;
            }
            let fid = *func_ids.get(&f.name).unwrap();
            obj.define_function(fid, &mut ctx_f)?;
            obj.clear_context(&mut ctx_f);
        }
        let entry_func_name = module
            .funcs
            .first()
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "main".into());
        let is_real_main = entry_func_name == "main" && !module.funcs.is_empty();
        if is_real_main {
            let mut ctx_main = obj.make_context();
            let ptr_ty = isa.pointer_type();
            ctx_main
                .func
                .signature
                .params
                .push(AbiParam::new(types::I32));
            ctx_main.func.signature.params.push(AbiParam::new(ptr_ty));
            ctx_main
                .func
                .signature
                .returns
                .push(AbiParam::new(types::I32));
            let mut fbctx = FunctionBuilderContext::new();
            {
                let mut b = FunctionBuilder::new(&mut ctx_main.func, &mut fbctx);
                let entry = b.create_block();
                b.append_block_params_for_function_params(entry);
                b.switch_to_block(entry);
                b.seal_block(entry);
                let argc=b.block_params(entry)[0];let argv=b.block_params(entry)[1];
                file_io::call(&mut obj,&mut b,"rfort_set_arguments",&[argc,argv],types::I64)?;
                let mut ex = declare_externs(&mut obj, &mut b, ptr_ty)?;
                ex.enable_caching(&mut b, ptr_ty);

                let mut ints = HashMap::new();
                let mut reals = HashMap::new();
                let mut bools = HashMap::new();
                let mut chars = characters::Characters::new();
                let mut real_kind = HashMap::new();
                let mut i128_vars = HashSet::new();
                let mut i128_last = HashMap::new();
                let mut dc = 0u32;
                let mut func_meta: HashMap<String, FuncMeta> = HashMap::new();
                for (name, fid) in &func_ids {
                    let fr = obj.declare_func_in_func(*fid, b.func);
                    let rk = *ret_kinds.get(name).unwrap_or(&RetKind::Void);
                    let pkv = param_kinds_map.get(name).cloned().unwrap_or_default();
                    func_meta.insert(
                        name.clone(),
                        FuncMeta {
                            fref: fr,
                            ret: rk,
                            param_kinds: pkv,
                            double: matches!(eff_ret.get(name), Some(crate::ast::TypeSpec::DoublePrecision)),
                        },
                    );
                }
                let mut arrays: HashMap<String, usize> = HashMap::new();
                let terminated = gen_stmts(
                    &mut b,
                    &ex,
                    ptr_ty,
                    &mut obj,
                    &mut dc,
                    &module.funcs[0].body,
                    &mut ints,
                    &mut reals,
                    &mut bools,
                    &mut chars,
                    &mut real_kind,
                    &mut i128_vars,
                    &mut i128_last,
                    &func_meta,
                    None,
                    None,
                    None,
                    &mut arrays,
                    tokens_opt,
                )?;
                if !terminated {
                    let z = b.ins().iconst(types::I32, 0);
                    b.ins().return_(&[z]);
                }
                b.finalize();
                chars.finish(&mut obj, &mut ctx_main.func, ptr_ty)?;
            }
            let main_id =
                obj.declare_function("main", Linkage::Export, &ctx_main.func.signature)?;
            obj.define_function(main_id, &mut ctx_main)?;
            obj.clear_context(&mut ctx_main);
        }
        c_interop::exports(&mut obj,module,isa.pointer_type(),&func_ids,self.module_owners.as_ref())?;
        let objf = obj.finish();
        Ok(objf.emit()?)
    }
}

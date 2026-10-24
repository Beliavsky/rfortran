use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec, UnOp};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct RecordAllocation {
    integers: Option<Vec<i64>>,
    reals: Option<Vec<f64>>,
    mask: Vec<i64>,
    metadata: Vec<i64>,
}

#[derive(Clone, Debug)]
enum PointerStorage {
    Frame(usize, String),
    Record(usize, bool, String),
}

#[derive(Clone, Debug)]
struct FuncDef {
    params: Vec<String>,
    body: Vec<Stmt>,
    is_function: bool,
}

pub fn interpret(program: &Program) {
    let _ = interpret_inner(program, None);
}

pub(crate) fn interpret_scalars(program: &Program, flags: &std::collections::BTreeMap<String,String>,
    seed: &mut u64) -> anyhow::Result<HashMap<String,Expr>> {
    interpret_inner(program, Some((flags, seed)))
}

fn interpret_inner(program: &Program, session: Option<(&std::collections::BTreeMap<String,String>, &mut u64)>)
    -> anyhow::Result<HashMap<String,Expr>> {
    thread_local! { static RECORD_ALLOCATIONS: std::cell::RefCell<Vec<Option<RecordAllocation>>> = std::cell::RefCell::new(vec![None]); }
    RECORD_ALLOCATIONS.with(|records| *records.borrow_mut()=vec![None]);
    thread_local! { static RANDOM_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) }; }
    RANDOM_STATE.with(|state| state.set(session.as_ref().map_or(1, |(_, seed)| **seed)));
    thread_local! { static FILES: std::cell::RefCell<crate::file_io::Files> = std::cell::RefCell::new(crate::file_io::Files::default()); }
    FILES.with(|files| *files.borrow_mut() = crate::file_io::Files::default());
    thread_local! { static POINTER_STORAGE: std::cell::RefCell<Vec<PointerStorage>> = std::cell::RefCell::new(vec![PointerStorage::Frame(usize::MAX,String::new())]); }
    POINTER_STORAGE.with(|storage|*storage.borrow_mut()=vec![PointerStorage::Frame(usize::MAX,String::new())]);
    thread_local! {
        static OUTPUT_LISTS: std::cell::RefCell<HashMap<i64,(String,bool,Vec<Expr>)>> = std::cell::RefCell::new(HashMap::new());
        static OUTPUT_SERIAL: std::cell::Cell<i64> = const {std::cell::Cell::new(0)};
    }
    thread_local! {static INPUT_LISTS:std::cell::RefCell<HashMap<i64,(i64,crate::list_input::Session,Option<(Vec<u8>,usize)>)>>=std::cell::RefCell::new(HashMap::new());}
    INPUT_LISTS.with(|lists|lists.borrow_mut().clear());
    OUTPUT_LISTS.with(|lists|lists.borrow_mut().clear());
    OUTPUT_SERIAL.with(|serial|serial.set(0));
    thread_local! { static SAVED: std::cell::RefCell<HashMap<String,ValueTuple>> = std::cell::RefCell::new(HashMap::new()); }
    SAVED.with(|values|values.borrow_mut().clear());
    #[derive(Clone,Debug,PartialEq)]
    enum ModuleArray {Integer(Vec<i64>),Real(Vec<f64>)}
    thread_local! {static MODULE_ARRAYS:std::cell::RefCell<HashMap<String,ModuleArray>>=std::cell::RefCell::new(HashMap::new());}
    MODULE_ARRAYS.with(|values|values.borrow_mut().clear());
    fn io_result<T>(result: anyhow::Result<T>) -> T {
        match result { Ok(value)=>value, Err(error)=>{eprintln!("Runtime error: {error:#}");std::process::exit(1);} }
    }
    fn random_value(double: bool) -> f64 {
        RANDOM_STATE.with(|state| {
            let mut x = state.get();
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            state.set(x);
            let precision = if double { 53 } else { 24 };
            (x.wrapping_mul(2685821657736338717) >> (64 - precision)) as f64
                / (1u64 << precision) as f64
        })
    }
    let env_str: HashMap<String, String> = HashMap::new();
    let env_int: HashMap<String, i64> = HashMap::new();
    let env_real32: HashMap<String, f32> = HashMap::new();
    let env_real64: HashMap<String, f64> = HashMap::new();
    let env_log: HashMap<String, bool> = HashMap::new();
    let mut funcs: HashMap<String, FuncDef> = HashMap::new();
    fn collect_functions(body: &[Stmt], funcs: &mut HashMap<String, FuncDef>) {
    for stmt in body {
        match stmt {
            Stmt::Function {
                name, params, body, ..
            } => {
                funcs.insert(
                    name.clone(),
                    FuncDef {
                        params: params.clone(),
                        body: body.clone(),
                        is_function: true,
                    },
                );
            }
            Stmt::Subroutine { name, params, body, .. } => {
                funcs.insert(
                    name.clone(),
                    FuncDef {
                        params: params.clone(),
                        body: body.clone(),
                        is_function: false,
                    },
                );
            }
            Stmt::Module { body, .. } => collect_functions(body, funcs),
            _ => {}
        }
    }
    }
    collect_functions(&program.body, &mut funcs);

    fn parse_i64_lossy(s: &str) -> i64 {
        i128::from_str_radix(s, 10)
            .ok()
            .and_then(|v| i64::try_from(v).ok())
            .unwrap_or(0)
    }
    fn parse_f64_lossy(s: &str) -> f64 {
        s.parse::<f64>().unwrap_or(0.0)
    }

    #[derive(Clone, Debug, Default, PartialEq)]
    struct ValueTuple(
        pub Option<i64>,
        pub Option<f64>,
        pub Option<bool>,
        pub Option<String>,
    );

    impl ValueTuple {
        fn from_int(i: i64) -> Self {
            Self(Some(i), None, None, None)
        }
        fn from_real(f: f64) -> Self {
            Self(None, Some(f), None, None)
        }
        fn from_bool(b: bool) -> Self {
            Self(None, None, Some(b), None)
        }
        fn from_str(s: String) -> Self {
            Self(None, None, None, Some(s))
        }
    }

    fn eval_expr(
        e: &Expr,
        env_str: &HashMap<String, String>,
        env_int: &HashMap<String, i64>,
        env_real32: &HashMap<String, f32>,
        env_real64: &HashMap<String, f64>,
        env_log: &HashMap<String, bool>,
        funcs: &HashMap<String, FuncDef>,
        call_stack: &mut Vec<CallFrame>,
    ) -> ValueTuple {
        match e {
            Expr::Section { .. } => unreachable!("array sections must be resolved before interpretation"),
            Expr::IntLit(s) => ValueTuple::from_int(parse_i64_lossy(s)),
            Expr::RealLit(s) => ValueTuple::from_real(parse_f64_lossy(s)),
            Expr::Logical(b) => ValueTuple::from_bool(*b),
            Expr::Str(s) => ValueTuple::from_str(s.clone()),
            Expr::Ident(id) => {
                if let Some((key,_))=call_stack.last().and_then(|frame|frame.saved.get(id)) {
                    return SAVED.with(|values|values.borrow()[key].clone());
                }
                if let Some(v) = env_int.get(id) {
                    return ValueTuple::from_int(*v);
                } else if let Some(v) = env_real64.get(id) {
                    return ValueTuple::from_real(*v);
                } else if let Some(v) = env_real32.get(id) {
                    return ValueTuple::from_real(*v as f64);
                } else if let Some(v) = env_log.get(id) {
                    return ValueTuple::from_bool(*v);
                } else if let Some(v) = env_str.get(id) {
                    return ValueTuple::from_str(v.clone());
                } else {
                    return ValueTuple::from_int(0);
                }
            }
            Expr::Index(name, inds) => {
                let mut idx = 0usize;
                if let Some(first) = inds.get(0) {
                    let vt = eval_expr(
                        first, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                    );
                    if let Some(i) = vt.0 {
                        idx = (i - 1) as usize;
                    }
                }

                if let Some(fr) = call_stack.last() {
                    if let Some(key)=fr.module_arrays.get(name) {
                        return MODULE_ARRAYS.with(|arrays|match &arrays.borrow()[key] {
                            ModuleArray::Integer(values)=>if fr.logical_arrays.contains(name) {ValueTuple::from_bool(values[idx]!=0)}else{ValueTuple::from_int(values[idx])},
                            ModuleArray::Real(values)=>ValueTuple::from_real(values[idx]),
                        });
                    }
                    if let Some(vec) = fr.env_int_arrays.get(name) {
                        if idx < vec.len() {
                            return if fr.logical_arrays.contains(name) { ValueTuple::from_bool(vec[idx] != 0) } else { ValueTuple::from_int(vec[idx]) };
                        }
                    }
                    if let Some(vec) = fr.env_real_arrays.get(name) {
                        if idx < vec.len() {
                            return ValueTuple::from_real(vec[idx]);
                        }
                    }
                }
                ValueTuple::from_int(0)
            }
            Expr::Un(UnOp::Neg, inner) => {
                let vt = eval_expr(
                    inner, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                );
                if let Some(i) = vt.0 {
                    ValueTuple::from_int(-i)
                } else if let Some(f) = vt.1 {
                    ValueTuple::from_real(-f)
                } else {
                    ValueTuple::from_int(0)
                }
            }
            Expr::Un(UnOp::Not, inner) => {
                let vt = eval_expr(
                    inner, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                );
                ValueTuple::from_bool(!vt.2.unwrap_or(false))
            }
            Expr::Bin(op, l, r) => {
                let lv = eval_expr(
                    l, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                );
                let rv = eval_expr(
                    r, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                );
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow => {
                        let both_int = lv.0.is_some() && rv.0.is_some();
                        if both_int && matches!(op, BinOp::Pow) {
                            let result = crate::utils::checked_integer_power(lv.0.unwrap(), rv.0.unwrap())
                                .unwrap_or_else(|| {
                                    eprintln!("Error: integer exponentiation overflow or zero raised to a negative power");
                                    std::process::exit(1);
                                });
                            return ValueTuple::from_int(result);
                        }
                        if both_int && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
                            let li = lv.0.unwrap();
                            let ri = rv.0.unwrap();
                            let res = match op {
                                BinOp::Add => li + ri,
                                BinOp::Sub => li - ri,
                                BinOp::Mul => li * ri,
                                BinOp::Div => li.checked_div(ri).unwrap_or_else(|| {
                                    eprintln!("Runtime error: integer division by zero or overflow");
                                    std::process::exit(1);
                                }),
                                _ => 0,
                            };
                            ValueTuple::from_int(res)
                        } else {
                            let a = lv.1.or(lv.0.map(|v| v as f64)).unwrap_or(0.0);
                            let b = rv.1.or(rv.0.map(|v| v as f64)).unwrap_or(0.0);
                            let res = match op {
                                BinOp::Add => a + b,
                                BinOp::Sub => a - b,
                                BinOp::Mul => a * b,
                                // IEEE division, as compiled code: x/0 is an infinity or NaN.
                                BinOp::Div => a / b,
                                BinOp::Pow => a.powf(b),
                                _ => 0.0,
                            };
                            ValueTuple::from_real(res)
                        }
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        if let (Some(a),Some(b))=(lv.0,rv.0) {
                            let result=match op {
                                BinOp::Eq=>a==b, BinOp::Ne=>a!=b, BinOp::Lt=>a<b,
                                BinOp::Gt=>a>b, BinOp::Le=>a<=b, BinOp::Ge=>a>=b,
                                _=>unreachable!(),
                            };
                            return ValueTuple::from_bool(result);
                        }
                        let a = lv.1.or(lv.0.map(|v| v as f64)).unwrap_or(0.0);
                        let b = rv.1.or(rv.0.map(|v| v as f64)).unwrap_or(0.0);
                        let res = match op {
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            BinOp::Lt => a < b,
                            BinOp::Gt => a > b,
                            BinOp::Le => a <= b,
                            BinOp::Ge => a >= b,
                            _ => false,
                        };
                        ValueTuple::from_bool(res)
                    }
                    BinOp::And => {
                        ValueTuple::from_bool(lv.2.unwrap_or(false) && rv.2.unwrap_or(false))
                    }
                    BinOp::Or => {
                        ValueTuple::from_bool(lv.2.unwrap_or(false) || rv.2.unwrap_or(false))
                    }
                    BinOp::Eqv => {
                        ValueTuple::from_bool(lv.2.unwrap_or(false) == rv.2.unwrap_or(false))
                    }
                    BinOp::Neqv => {
                        ValueTuple::from_bool(lv.2.unwrap_or(false) != rv.2.unwrap_or(false))
                    }
                    BinOp::Concat => {
                        let ls = lv.3.unwrap_or_else(|| {
                            lv.0.map(|v| v.to_string())
                                .or(lv.1.map(|f| format!("{}", f)))
                                .unwrap_or_default()
                        });
                        let rs = rv.3.unwrap_or_else(|| {
                            rv.0.map(|v| v.to_string())
                                .or(rv.1.map(|f| format!("{}", f)))
                                .unwrap_or_default()
                        });
                        ValueTuple::from_str(format!("{}{}", ls, rs))
                    }
                }
            }
            Expr::Call(name, args) => {

                if name == "@cpu_seconds" {
                    return ValueTuple::from_real(crate::timing::rfort_cpu_seconds());
                }
                if name == "@clock_count" {
                    let kind = eval_expr(&args[0], env_str, env_int, env_real32,
                        env_real64, env_log, funcs, call_stack).0.unwrap();
                    return ValueTuple::from_int(crate::timing::rfort_clock_count(kind));
                }
                if name=="@ptr_name" || name.starts_with("@ptr_load_") {
                    let handle=eval_expr(&args[0],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap();
                    let offset=if name=="@ptr_name" {0}else{eval_expr(&args[1],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap()};
                    let target=POINTER_STORAGE.with(|storage|storage.borrow().get(handle as usize).expect("invalid pointer storage handle").clone());
                    if let PointerStorage::Record(allocation,mask,label)=&target {
                        if name=="@ptr_name" {return ValueTuple::from_str(label.clone());}
                        return RECORD_ALLOCATIONS.with(|records| {
                            let records=records.borrow();
                            let record=io_result(records.get(*allocation).and_then(Option::as_ref).ok_or_else(||anyhow::anyhow!("pointer target allocation no longer exists")));
                            let index=usize::try_from(offset).expect("negative pointer offset");
                            if *mask {return ValueTuple::from_int(record.mask[index]);}
                            if let Some(values)=&record.reals {ValueTuple::from_real(values[index])}
                            else {let value=record.integers.as_ref().unwrap()[index];if name=="@ptr_load_l" {ValueTuple::from_bool(value!=0)} else {ValueTuple::from_int(value)}}
                        });
                    }
                    let PointerStorage::Frame(frame_index,target)=target else {unreachable!()};
                    let fr=&call_stack[frame_index];
                    let target=&target;
                    if name=="@ptr_name" {return ValueTuple::from_str(target.clone());}
                    let index=usize::try_from(offset).expect("negative pointer offset");
                    if let Some(a)=fr.env_int_arrays.get(target){let v=*a.get(index).expect("pointer read outside target");return if name=="@ptr_load_l"{ValueTuple::from_bool(v!=0)}else{ValueTuple::from_int(v)};}
                    if let Some(a)=fr.env_real_arrays.get(target){return ValueTuple::from_real(*a.get(index).expect("pointer read outside target"));}
                    assert_eq!(offset,0,"pointer scalar offset");return fr.result_value(target);
                }
                if name == "@substring" || name == "@dynamic_substring" {
                    let text=eval_expr(&args[0],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).3.unwrap();
                    let start = eval_expr(&args[1],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap() as usize;
                    let length = eval_expr(&args[2],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap() as usize;
                    return ValueTuple::from_str(text[start..start+length].to_string());
                }
                if call_stack.last().is_some_and(|frame| frame.env_real_arrays.contains_key(name) || frame.env_int_arrays.contains_key(name)) {
                    return eval_expr(&Expr::Index(name.clone(), args.clone()), env_str, env_int,
                        env_real32, env_real64, env_log, funcs, call_stack);
                }
                if name == "__f90c_stop" {
                    let mut arg_vals: Vec<ValueTuple> = Vec::new();
                    for a in args {
                        arg_vals.push(eval_expr(
                            a, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                        ));
                    }
                    if let Some(code) = arg_vals.get(0).and_then(|v| v.0) {
                        std::process::exit(code as i32);
                    } else {
                        std::process::exit(0);
                    }
                }
                if let Some(fd) = funcs.get(name) {
                    let mut arg_vals: Vec<ValueTuple> = Vec::new();
                    for a in args {
                        arg_vals.push(eval_expr(
                            a, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                        ));
                    }
                    let mut frame = CallFrame::new(fd, Some(name.clone()));
                    for (i, pn) in fd.params.iter().enumerate() {
                        let av = arg_vals.get(i);
                        if let Some(v) = av {
                            if let Some(iv) = v.0 {
                                frame.env_int.insert(pn.clone(), iv);
                            } else if let Some(fv) = v.1 {
                                frame.env_real64.insert(pn.clone(), fv);
                            } else if let Some(bv) = v.2 {
                                frame.env_log.insert(pn.clone(), bv);
                            } else if let Some(sv) = v.3.clone() {
                                frame.env_str.insert(pn.clone(), sv);
                            } else {
                                frame.env_int.insert(pn.clone(), 0);
                            }
                        } else {
                            frame.env_int.insert(pn.clone(), 0);
                        }
                    }
                    if fd.is_function && !fd.body.iter().any(|s| matches!(s, Stmt::VarDecl { names, .. } if names.contains(name))) {
                        frame.env_int.insert(name.clone(), 0);
                    }
                    call_stack.push(frame);
                    let ret = exec_stmts(&fd.body, funcs, call_stack);
                    let frame = call_stack.pop().unwrap();
                    if let Some(rv) = ret {
                        return rv;
                    }
                    if fd.is_function {
                        return frame.result_value(name);
                    }
                    ValueTuple::from_int(0)
                } else {
                    let mut arg_vals: Vec<ValueTuple> = Vec::new();
                    for a in args {
                        arg_vals.push(eval_expr(
                            a, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                        ));
                    }
                    if matches!(name.as_str(),"floor"|"ceiling") && !args.is_empty() {
                        let kind=crate::inquiries::model_type(&args[0],&|n| {
                            if env_real64.contains_key(n) {Some((crate::ast::TypeSpec::DoublePrecision,0))}
                            else if env_real32.contains_key(n) {Some((crate::ast::TypeSpec::Real,0))}
                            else {None}
                        }).ok().flatten();
                        if kind==Some(crate::ast::TypeSpec::Real) {
                            if let Some(value)=to_real(&arg_vals[0]) {arg_vals[0]=ValueTuple::from_real((value as f32) as f64);}
                        }
                    }
                    if let Some(v) = handle_intrinsic(name, &arg_vals) {
                        return v;
                    }
                    ValueTuple::from_int(0)
                }
            }
        }
    }

    fn to_real(v: &ValueTuple) -> Option<f64> {
        v.1.or(v.0.map(|i| i as f64))
    }
    fn to_int(v: &ValueTuple) -> Option<i64> {
        v.0.or(v.1.map(|f| f as i64))
    }

    fn handle_intrinsic(name: &str, args: &[ValueTuple]) -> Option<ValueTuple> {
        if let Some((op,kind))=crate::real_intrinsics::decode(name) {
            let x=to_real(args.first()?)?;
            let i=if matches!(op,"scale"|"set_exponent") {to_int(args.get(1)?)?} else {0};
            let s=if op=="nearest" {to_real(args.get(1)?)?} else {0.0};
            if op=="nearest" && s==0.0 {eprintln!("Runtime error: NEAREST S must not be zero");std::process::exit(1);}
            let value=crate::float_model::evaluate(crate::real_intrinsics::opcode(op),kind,x,i,s);
            return Some(if op=="exponent" {ValueTuple::from_int(value as i64)} else {ValueTuple::from_real(value)});
        }
        if let Some((operation,bytes))=crate::bit_intrinsics::decode(name) {
            let values=args.iter().map(to_int).collect::<Option<Vec<_>>>()?;
            let value=crate::bit_intrinsics::evaluate(operation,bytes,&values).ok()?;
            return Some(if operation=="btest" {ValueTuple::from_bool(value!=0)} else {ValueTuple::from_int(value)});
        }
        let n = name.to_ascii_lowercase();
        match n.as_str() {
            "@round_real32"=>Some(ValueTuple::from_real((to_real(args.first()?)? as f32) as f64)),
            "isnan"=>Some(ValueTuple::from_bool(to_real(args.first()?)?.is_nan())),
            "abs" => {
                if let Some(i) = args.get(0)?.0 {
                    Some(ValueTuple::from_int(i.abs()))
                } else if let Some(f) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_real(f.abs()))
                } else {
                    Some(ValueTuple::from_int(
                        to_int(args.get(0)?).unwrap_or(0).abs(),
                    ))
                }
            }
            "nint" => Some(ValueTuple::from_int(to_real(args.get(0)?)?.round() as i64)),
            "aint" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.trunc())),
            "anint" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.round())),
            "asinh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.asinh())),
            "acosh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.acosh())),
            "atanh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.atanh())),
            "erf" | "erfc" | "gamma" | "log_gamma" => {
                extern "C" {
                    fn erf(x: f64) -> f64;
                    fn erfc(x: f64) -> f64;
                    fn tgamma(x: f64) -> f64;
                    fn lgamma(x: f64) -> f64;
                }
                let x = to_real(args.get(0)?)?;
                // SAFETY: C99 math library functions of one double.
                let value = unsafe { match n.as_str() { "erf" => erf(x), "erfc" => erfc(x), "gamma" => tgamma(x), _ => lgamma(x) } };
                Some(ValueTuple::from_real(value))
            }
            "bessel_j0" | "bessel_j1" | "bessel_y0" | "bessel_y1" | "rfort_bessel_jn" | "rfort_bessel_yn" => {
                extern "C" {
                    #[cfg_attr(windows, link_name = "_jn")] fn jn(n: i32, x: f64) -> f64;
                    #[cfg_attr(windows, link_name = "_yn")] fn yn(n: i32, x: f64) -> f64;
                }
                let x = to_real(args.get(0)?)?;
                let order = match n.as_str() {
                    "bessel_j0" | "bessel_y0" => 0,
                    "bessel_j1" | "bessel_y1" => 1,
                    _ => to_real(args.get(1)?)? as i32,
                };
                // SAFETY: C library Bessel functions of an int order and a double.
                let value = unsafe { if n.contains("_j") { jn(order, x) } else { yn(order, x) } };
                Some(ValueTuple::from_real(value))
            }
            "hypot" => {
                let x = to_real(args.get(0)?)?;
                let y = to_real(args.get(1)?)?;
                Some(ValueTuple::from_real(x.hypot(y)))
            }
            "floor" | "ceiling" => {
                let x=to_real(args.first()?)?;
                let value=if n=="floor" {x.floor()} else {x.ceil()};
                let kind=args.get(1).and_then(to_int).unwrap_or(4);
                let limit=2.0f64.powi((kind*8-1) as i32);
                if !value.is_finite() || value < -limit || value >= limit {
                    eprintln!("Runtime error: {} result is outside INTEGER(KIND={kind}) range",n.to_ascii_uppercase());
                    std::process::exit(1);
                }
                Some(ValueTuple::from_int(value as i64))
            }
            "sqrt" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.sqrt())),
            "exp" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.exp())),
            "log" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.ln())),
            "log10" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.log10())),
            "sin" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.sin())),
            "cos" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.cos())),
            "tan" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.tan())),
            "asin" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.asin())),
            "acos" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.acos())),
            "atan" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.atan())),
            "atan2" => {
                let y = to_real(args.get(0)?)?;
                let x = to_real(args.get(1)?)?;
                Some(ValueTuple::from_real(y.atan2(x)))
            }
            "sinh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.sinh())),
            "cosh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.cosh())),
            "tanh" => to_real(args.get(0)?).map(|f| ValueTuple::from_real(f.tanh())),
            "dim" => {
                let a = to_real(args.get(0)?)?;
                let b = to_real(args.get(1)?)?;
                Some(ValueTuple::from_real((a - b).max(0.0)))
            }
            "dprod" => {
                if args.len() >= 2 {
                    Some(ValueTuple::from_real(
                        to_real(args.get(0)?)? * to_real(args.get(1)?)?,
                    ))
                } else {
                    None
                }
            }
            "mod" => {
                if let (Some(a),Some(b)) = (args.get(0)?.0,args.get(1)?.0) {
                    if b == 0 {
                        Some(ValueTuple::from_int(0))
                    } else {
                        Some(ValueTuple::from_int(a % b))
                    }
                } else {
                    let a = to_real(args.get(0)?)?;
                    let b = to_real(args.get(1)?)?;
                    if b == 0.0 {
                        Some(ValueTuple::from_real(0.0))
                    } else {
                        Some(ValueTuple::from_real(a % b))
                    }
                }
            }
            "modulo" => {
                if let (Some(a),Some(p))=(args.get(0)?.0,args.get(1)?.0) {
                    let r=a.checked_rem(p).expect("invalid MODULO divisor or integer overflow");
                    Some(ValueTuple::from_int(if r!=0 && (r<0)!=(p<0) {r+p}else{r}))
                } else {
                    let a=to_real(args.get(0)?)?;let p=to_real(args.get(1)?)?;
                    let value=a-(a/p).floor()*p;
                    Some(ValueTuple::from_real(if value==0.0 {0.0f64.copysign(p)} else {value}))
                }
            }
            "int" => Some(ValueTuple::from_int(to_int(args.get(0)?)?)),
            "real" | "dble" => Some(ValueTuple::from_real(to_real(args.get(0)?)?)),
            "sign" => {
                if let (Some(a), Some(b)) = (args.first()?.0, args.get(1)?.0) {
                    if args[0].1.is_none() && args[1].1.is_none() {
                        return Some(ValueTuple::from_int(if b < 0 { -a.abs() } else { a.abs() }));
                    }
                }
                let a = to_real(args.get(0)?)?;
                let b = to_real(args.get(1)?)?;
                Some(ValueTuple::from_real(
                    a.abs() * if b.is_sign_negative() { -1.0 } else { 1.0 },
                ))
            }

            "epsilon" => Some(ValueTuple::from_real(f64::EPSILON)),
            "huge" => Some(ValueTuple::from_real(f64::MAX)),
            "tiny" => Some(ValueTuple::from_real(f64::MIN_POSITIVE)),
            "digits" => Some(ValueTuple::from_int(15)),
            "precision" => Some(ValueTuple::from_int(6)),
            "radix" => Some(ValueTuple::from_int(2)),

            "all" => Some(ValueTuple::from_bool(
                args.get(0).map(|v| v.2.unwrap_or(false)).unwrap_or(false),
            )),
            "any" => Some(ValueTuple::from_bool(
                args.get(0).map(|v| v.2.unwrap_or(false)).unwrap_or(false),
            )),
            "count" => Some(ValueTuple::from_int(
                if args.get(0).map(|v| v.2.unwrap_or(false)).unwrap_or(false) {
                    1
                } else {
                    0
                },
            )),

            "sum" => {
                if let Some(r) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_real(r))
                } else {
                    Some(ValueTuple::from_int(to_int(args.get(0)?)?))
                }
            }
            "product" => {
                if let Some(r) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_real(r))
                } else {
                    Some(ValueTuple::from_int(to_int(args.get(0)?)?))
                }
            }
            "max" | "min" => {
                if args.iter().all(|a| a.0.is_some()) {
                    let mut values = args.iter().map(|a| a.0.unwrap());
                    let first = values.next()?;
                    Some(ValueTuple::from_int(values.fold(first, |a,b| if n == "max" { a.max(b) } else { a.min(b) })))
                } else {
                    let mut values = args.iter().map(|a| to_real(a).unwrap());
                    let first = values.next()?;
                    Some(ValueTuple::from_real(values.fold(first, |a,b| if n == "max" { a.max(b) } else { a.min(b) })))
                }
            }

            "len" => args
                .get(0)
                .and_then(|v| v.3.as_ref().map(|s| ValueTuple::from_int(s.len() as i64))),
            "len_trim" => args.get(0).and_then(|v| {
                v.3.as_ref()
                    .map(|s| ValueTuple::from_int(s.trim_end_matches(' ').len() as i64))
            }),
            "char" | "achar" => args.get(0).and_then(|v| {
                to_int(v).map(|i| ValueTuple::from_str((i as u8 as char).to_string()))
            }),
            "ichar" | "iachar" => args.get(0).and_then(|v| {
                v.3.as_ref()
                    .map(|s| ValueTuple::from_int(s.bytes().next().unwrap_or(0) as i64))
            }),
            "trim" => args.get(0).and_then(|v| {
                v.3.as_ref()
                    .map(|s| ValueTuple::from_str(s.trim_end_matches(' ').to_string()))
            }),
            "adjustl" | "adjustr" => {
                let text=args.first()?.3.as_ref()?;
                let adjusted=if name=="adjustl" {
                    let content=text.trim_start_matches(' ');format!("{}{}",content," ".repeat(text.len()-content.len()))
                } else {
                    let content=text.trim_end_matches(' ');format!("{}{}"," ".repeat(text.len()-content.len()),content)
                };
                Some(ValueTuple::from_str(adjusted))
            }
            "repeat" => {
                if let Some(s) = args.get(0).and_then(|v| v.3.clone()) {
                    let n = to_int(args.get(1)?)?;
                    Some(ValueTuple::from_str(s.repeat(n as usize)))
                } else {
                    None
                }
            }
            "index" => {
                if let (Some(hay), Some(needle)) = (
                    args.get(0).and_then(|v| v.3.clone()),
                    args.get(1).and_then(|v| v.3.clone()),
                ) {
                    let idx = hay.find(&needle).map(|i| (i + 1) as i64).unwrap_or(0);
                    Some(ValueTuple::from_int(idx))
                } else {
                    None
                }
            }

            "btest" => {
                let a = to_int(args.get(0)?)?;
                let pos = to_int(args.get(1)?)?;
                Some(ValueTuple::from_bool(((a >> pos) & 1) != 0))
            }
            "iand" => Some(ValueTuple::from_int(
                to_int(args.get(0)?)? & to_int(args.get(1)?)?,
            )),
            "ior" => Some(ValueTuple::from_int(
                to_int(args.get(0)?)? | to_int(args.get(1)?)?,
            )),
            "ieor" => Some(ValueTuple::from_int(
                to_int(args.get(0)?)? ^ to_int(args.get(1)?)?,
            )),
            "not" => Some(ValueTuple::from_int(!to_int(args.get(0)?)?)),
            "ibset" => {
                let a = to_int(args.get(0)?)?;
                let pos = to_int(args.get(1)?)?;
                Some(ValueTuple::from_int(a | (1 << pos)))
            }
            "ibclr" => {
                let a = to_int(args.get(0)?)?;
                let pos = to_int(args.get(1)?)?;
                Some(ValueTuple::from_int(a & !(1 << pos)))
            }
            "ibits" => {
                let a = to_int(args.get(0)?)?;
                let pos = to_int(args.get(1)?)? as u32;
                let len = to_int(args.get(2).unwrap_or(&ValueTuple::from_int(1)))? as u32;
                let mask = if len >= 64 {
                    u64::MAX
                } else {
                    (1u64 << len) - 1
                };
                Some(ValueTuple::from_int(
                    ((a as i64 as u64 >> pos) & mask) as i64,
                ))
            }
            "ishft" => {
                let a = to_int(args.get(0)?)?;
                let sh = to_int(args.get(1)?)?;
                if sh >= 0 {
                    Some(ValueTuple::from_int(a << sh))
                } else {
                    Some(ValueTuple::from_int((a as u64 >> (-sh) as u32) as i64))
                }
            }

            "matmul" => {
                if args.len() >= 2 {
                    Some(ValueTuple::from_real(
                        to_real(args.get(0)?)? * to_real(args.get(1)?)?,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // The same text as compiled list-directed output: an exactly integral
    // value as `n.0`, otherwise 6 (REAL) or 15 (DOUBLE PRECISION) significant digits.
    fn format_real(f: f64, double: bool) -> String {
        if f.fract() == 0.0 && f.abs() < 9.2e18 { return format!("{:.1}", f); }
        let digits = if f.is_finite() && f.abs() > f64::MAX * 0.99 { 17 } else if double { 15 } else { 6 };
        let mut out = [0u8; 64];
        let n = crate::format_spec::list_real(f, digits, &mut out);
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    fn double_expression(e: &Expr, doubles: &HashMap<String, f64>) -> bool {
        match e {
            Expr::Ident(name) => doubles.contains_key(name),
            Expr::Call(name, args) => {
                if name.eq_ignore_ascii_case("dble") { return true; }
                if name.eq_ignore_ascii_case("real") { return false; }
                args.first().is_some_and(|arg| double_expression(arg, doubles))
            }
            Expr::Bin(_, a, b) => double_expression(a, doubles) || double_expression(b, doubles),
            Expr::Un(_, a) => double_expression(a, doubles),
            _ => false,
        }
    }

    fn eval_logical(
        e: &Expr,
        env_log: &HashMap<String, bool>,
        env_int: &HashMap<String, i64>,
        env_real64: &HashMap<String, f64>,
        env_real32: &HashMap<String, f32>,
        env_str: &HashMap<String, String>,
        funcs: &HashMap<String, FuncDef>,
        call_stack: &mut Vec<CallFrame>,
    ) -> bool {
        match e {
            Expr::Logical(b) => *b,
            Expr::Ident(id) => *env_log.get(id).unwrap_or(&false),
            _ => {
                let vt = eval_expr(
                    e, env_str, env_int, env_real32, env_real64, env_log, funcs, call_stack,
                );
                vt.2.unwrap_or(false)
            }
        }
    }

    #[derive(Clone, Debug)]
    struct CallFrame {
        saved: HashMap<String,(String,TypeSpec)>,
        saved_seen: HashMap<String,ValueTuple>,
        module_arrays:HashMap<String,String>,
        module_seen:HashMap<String,ModuleArray>,
        env_str: HashMap<String, String>,
        env_int: HashMap<String, i64>,
        env_real32: HashMap<String, f32>,
        env_real64: HashMap<String, f64>,
        env_log: HashMap<String, bool>,

        env_int_arrays: HashMap<String, Vec<i64>>,
        env_real_arrays: HashMap<String, Vec<f64>>,
        double_arrays: std::collections::HashSet<String>,
        logical_arrays: std::collections::HashSet<String>,
        func_name: Option<String>,
        is_function: bool,
    }
    impl CallFrame {
        fn apply_saved(&mut self,name:&str,kind:&TypeSpec,value:&ValueTuple) {
            let number=value.1.unwrap_or(value.0.unwrap_or(0) as f64);
            self.env_int.remove(name);self.env_real32.remove(name);self.env_real64.remove(name);self.env_log.remove(name);
            match kind {
                TypeSpec::Integer(_)=>{self.env_int.insert(name.into(),value.0.unwrap_or(number as i64));},
                TypeSpec::Real=>{self.env_real32.insert(name.into(),number as f32);},
                TypeSpec::DoublePrecision=>{self.env_real64.insert(name.into(),number);},
                TypeSpec::Logical=>{self.env_log.insert(name.into(),value.2.unwrap_or(number!=0.0));},
                TypeSpec::Character(length)=>{
                    let length=length.unwrap_or(1);let mut text=value.3.clone().unwrap_or_default();
                    text.truncate(length);text.extend(std::iter::repeat(' ').take(length-text.len()));
                    self.env_str.insert(name.into(),text);
                },_=>unreachable!(),
            }
        }
        fn sync_saved(&mut self) {
            for (name,key) in self.module_arrays.clone() {
                let value=MODULE_ARRAYS.with(|values|values.borrow()[&key].clone());
                match &value {ModuleArray::Integer(v)=>{self.env_int_arrays.insert(name.clone(),v.clone());},ModuleArray::Real(v)=>{self.env_real_arrays.insert(name.clone(),v.clone());}}
                self.module_seen.insert(name,value);
            }
            for (name,(key,kind)) in self.saved.clone() {
                let value=SAVED.with(|values|values.borrow()[&key].clone());
                self.apply_saved(&name,&kind,&value);self.saved_seen.insert(name,value);
            }
        }
        fn commit_saved(&mut self) {
            for (name,key) in self.module_arrays.clone() {
                let value=if let Some(v)=self.env_int_arrays.get(&name) {ModuleArray::Integer(v.clone())}else{ModuleArray::Real(self.env_real_arrays[&name].clone())};
                if self.module_seen.get(&name)!=Some(&value) {
                    MODULE_ARRAYS.with(|values|values.borrow_mut().insert(key,value.clone()));
                    self.module_seen.insert(name,value);
                }
            }
            for (name,(key,_)) in self.saved.clone() {
                let value=self.result_value(&name);
                if self.saved_seen.get(&name)!=Some(&value) {
                    SAVED.with(|values|{values.borrow_mut().insert(key,value.clone());});
                    self.saved_seen.insert(name,value);
                }
            }
        }
        fn result_value(&self, name: &str) -> ValueTuple {
            if let Some(v) = self.env_int.get(name) { ValueTuple::from_int(*v) }
            else if let Some(v) = self.env_real32.get(name) { ValueTuple::from_real(*v as f64) }
            else if let Some(v) = self.env_real64.get(name) { ValueTuple::from_real(*v) }
            else if let Some(v) = self.env_log.get(name) { ValueTuple::from_bool(*v) }
            else if let Some(v) = self.env_str.get(name) { ValueTuple::from_str(v.clone()) }
            else { ValueTuple::from_int(0) }
        }
        fn new(fd: &FuncDef, func_name: Option<String>) -> Self {
            Self {
                saved:HashMap::new(),saved_seen:HashMap::new(),module_arrays:HashMap::new(),module_seen:HashMap::new(),
                env_str: HashMap::new(),
                env_int: HashMap::new(),
                env_real32: HashMap::new(),
                env_real64: HashMap::new(),
                env_log: HashMap::new(),

                env_int_arrays: HashMap::new(),
                env_real_arrays: HashMap::new(),
                double_arrays: std::collections::HashSet::new(),
                logical_arrays: std::collections::HashSet::new(),
                func_name,
                is_function: fd.is_function,
            }
        }
    }
    fn exec_stmts(
        stmts: &[Stmt],
        funcs: &HashMap<String, FuncDef>,
        call_stack: &mut Vec<CallFrame>,
    ) -> Option<ValueTuple> {
        let mut idx: usize = 0;
        while idx < stmts.len() {
            call_stack.last_mut().unwrap().commit_saved();
            call_stack.last_mut().unwrap().sync_saved();
            let stmt = &stmts[idx];
            let fragment = matches!(stmt, Stmt::CallSub { name, .. } if name == "@print_fragment");
            let fragment_print;
            let stmt = if let Stmt::CallSub { name, args } = stmt {
                if name == "@print_fragment" {
                    fragment_print = Stmt::Print { items: args.clone() };
                    &fragment_print
                } else { stmt }
            } else { stmt };
            match stmt {
                Stmt::InitializedDecl { .. } => unreachable!("initialized declarations must be resolved before interpretation"),
                Stmt::VarDecl { kind, names } => {
                    let frame = call_stack.last_mut().unwrap();
                    for n in names {
                        if frame.env_int.contains_key(n)
                            || frame.env_real32.contains_key(n)
                            || frame.env_real64.contains_key(n)
                            || frame.env_log.contains_key(n)
                            || frame.env_str.contains_key(n)
                        {
                            continue;
                        }
                        match kind {
                            TypeSpec::Referenced { .. } | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed | TypeSpec::RealKind(_) | TypeSpec::LogicalKind(_) | TypeSpec::IntegerKind(_) | TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                            TypeSpec::Integer(_) => {
                                frame.env_int.entry(n.clone()).or_insert(0);
                            }
                            TypeSpec::Real => {
                                frame.env_real32.entry(n.clone()).or_insert(0.0);
                            }
                            TypeSpec::DoublePrecision => {
                                frame.env_real64.entry(n.clone()).or_insert(0.0);
                            }
                            TypeSpec::Character(_) => {
                                frame.env_str.entry(n.clone()).or_insert(String::new());
                            }
                            TypeSpec::Logical => {
                                frame.env_log.entry(n.clone()).or_insert(false);
                            }
                        }
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    let fr = call_stack.last_mut().unwrap();

                    if dims.len() >= 1 {
                        if let Expr::IntLit(s) = &dims[0] {
                            if let Ok(v128) = i128::from_str_radix(s.as_str(), 10) {
                                let len = usize::try_from(v128).unwrap_or(0);
                                match kind {
                            TypeSpec::Referenced { .. } | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed | TypeSpec::RealKind(_) | TypeSpec::LogicalKind(_) | TypeSpec::IntegerKind(_) | TypeSpec::Complex(_) => unreachable!("unresolved numeric kind"),
                                    TypeSpec::Integer(_) | TypeSpec::Logical => {
                                        fr.env_int_arrays.insert(name.clone(), vec![0; len]);
                                        if matches!(kind, TypeSpec::Logical) { fr.logical_arrays.insert(name.clone()); }
                                    }
                                    TypeSpec::Real | TypeSpec::DoublePrecision => {
                                        fr.env_real_arrays.insert(name.clone(), vec![0.0; len]);
                                        if matches!(kind, TypeSpec::DoublePrecision) { fr.double_arrays.insert(name.clone()); }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                Stmt::Read { args } => {
                    let fr = call_stack.last_mut().unwrap();
                    use std::io;
                    let mut input = String::new();
                    for arg in args {
                        input.clear();
                        io::stdin().read_line(&mut input).ok();
                        let tok = input.trim_end();
                        if let Expr::Ident(id) = arg {
                            // Numeric input consumes a token, not the unused rest of the record.
                            let tok = if fr.env_str.contains_key(id) { tok } else {
                                tok.split(|c: char| c.is_whitespace() || c == ',')
                                    .find(|s| !s.is_empty()).unwrap_or("")
                            };
                            if fr.env_int.contains_key(id) {
                                let v = parse_i64_lossy(tok);
                                fr.env_int.insert(id.clone(), v);
                            } else if fr.env_real64.contains_key(id) {
                                let v = parse_f64_lossy(tok);
                                fr.env_real64.insert(id.clone(), v);
                            } else if fr.env_real32.contains_key(id) {
                                let v = parse_f64_lossy(tok) as f32;
                                fr.env_real32.insert(id.clone(), v);
                            } else if fr.env_log.contains_key(id) {
                                let v = matches!(
                                    tok.to_ascii_lowercase().as_str(),
                                    ".true." | "true" | "1"
                                );
                                fr.env_log.insert(id.clone(), v);
                            } else if fr.env_str.contains_key(id) {
                                fr.env_str.insert(id.clone(), tok.to_string());
                            } else {
                                let v = parse_i64_lossy(tok);
                                fr.env_int.insert(id.clone(), v);
                            }
                        }
                    }
                }
                Stmt::Assign { name, value } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let vt = eval_expr(value, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                    let frame = call_stack.last_mut().unwrap();
                    if let Some(s) = vt.3.clone() {
                        frame.env_str.insert(name.clone(), s);
                        frame.env_int.remove(name);
                    } else if let Some(i) = vt.0 {
                        if frame.env_real64.contains_key(name) {
                            frame.env_real64.insert(name.clone(),i as f64);
                        } else if frame.env_real32.contains_key(name) {
                            frame.env_real32.insert(name.clone(),i as f32);
                        } else {
                            frame.env_int.insert(name.clone(), i);
                        }
                    } else if let Some(f) = vt.1 {
                        if frame.env_int.contains_key(name) {
                            frame.env_int.insert(name.clone(), f as i64);
                        } else if frame.env_real64.contains_key(name) {
                            frame.env_real64.insert(name.clone(), f);
                        } else if frame.env_real32.contains_key(name) {
                            frame.env_real32.insert(name.clone(), f as f32);
                        } else {
                            frame.env_real64.insert(name.clone(), f);
                        }
                    } else if let Some(b) = vt.2 {
                        frame.env_log.insert(name.clone(), b);
                    }
                    if let Some((key,kind))=frame.saved.get(name).cloned() {
                        frame.apply_saved(name,&kind,&vt);
                        let value=frame.result_value(name);
                        SAVED.with(|values|{values.borrow_mut().insert(key,value.clone());});
                        frame.saved_seen.insert(name.clone(),value);
                    }
                }
                Stmt::Print { items } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let mut parts: Vec<(String, bool)> = Vec::new();
                    for it in items {
                        match it {
                            Expr::Str(s) => parts.push((s.clone(), true)),
                            Expr::IntLit(s) => parts.push((s.clone(), false)),
                            Expr::RealLit(s) => {
                                let f = parse_f64_lossy(s);
                                parts.push((format_real(f, false), false));
                            }
                            Expr::Logical(b) => parts.push((if *b {
                                "T".into()
                            } else {
                                "F".into()
                            }, false)),
                            Expr::Ident(id) => {
                                let frame = call_stack.last().unwrap();
                                if let Some(values) = frame.env_int_arrays.get(id) {
                                    parts.extend(values.iter().map(|v| (
                                        if frame.logical_arrays.contains(id) { if *v != 0 { "T".into() } else { "F".into() } }
                                        else { v.to_string() }, false)));
                                } else if let Some(values) = frame.env_real_arrays.get(id) {
                                    parts.extend(values.iter().map(|v| (format_real(*v, false), false)));
                                } else if let Some(v) = r64.get(id) {
                                    parts.push((format_real(*v, true), false));
                                } else if let Some(v) = r32.get(id) {
                                    parts.push((format_real(*v as f64, false), false));
                                } else if let Some(v) = ints.get(id) {
                                    parts.push((v.to_string(), false));
                                } else if let Some(v) = logs.get(id) {
                                    parts.push((if *v {
                                        "T".into()
                                    } else {
                                        "F".into()
                                    }, false));
                                } else if let Some(v) = strs.get(id) {
                                    parts.push((v.clone(), true));
                                } else {
                                    parts.push(("0".into(), false));
                                }
                            }
                            _ => {
                                let vt = eval_expr(
                                    it, &strs, &ints, &r32, &r64, &logs, funcs, call_stack,
                                );
                                if let Some(s) = vt.3 {
                                    parts.push((s, true));
                                } else if let Some(f) = vt.1 {
                                    parts.push((format_real(f, double_expression(it, &r64)), false));
                                } else if let Some(i) = vt.0 {
                                    parts.push((i.to_string(), false));
                                } else if let Some(b) = vt.2 {
                                    parts.push((if b { "T".into() } else { "F".into() }, false));
                                }
                            }
                        }
                    }
                    let mut record = String::new();
                    let mut previous_character = None;
                    for (text, character) in parts {
                        if !fragment || previous_character.is_some() { record.push_str(crate::utils::list_directed_separator(previous_character, character)); }
                        record.push_str(&text);
                        previous_character = Some(character);
                    }
                    if fragment { print!("{}", record); } else { println!("{}", record); }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let cond_val =
                        eval_logical(cond, &logs, &ints, &r64, &r32, &strs, funcs, call_stack);
                    if cond_val {
                        if let Some(ret) = exec_stmts(then_body, funcs, call_stack) {
                            return Some(ret);
                        }
                    } else if let Some(eb) = else_body {
                        if let Some(ret) = exec_stmts(eb, funcs, call_stack) {
                            return Some(ret);
                        }
                    }
                }
                Stmt::DoWhile { cond, body } => loop {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let cond_val =
                        eval_logical(cond, &logs, &ints, &r64, &r32, &strs, funcs, call_stack);
                    if !cond_val {
                        break;
                    }
                    if let Some(ret) = exec_stmts(body, funcs, call_stack) {
                        if ret.0 == Some(1) { break; }
                        if ret.0 == Some(2) { continue; }
                        return Some(ret);
                    }
                },
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let sv = eval_expr(start, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                    let ev = eval_expr(end, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                    let s = sv.0.or(sv.1.map(|f| f as i64)).unwrap_or(0);
                    let e = ev.0.or(ev.1.map(|f| f as i64)).unwrap_or(0);
                    let mut exited=false;
                    call_stack.last_mut().unwrap().env_int.insert(var.clone(),s);
                    'outer: for i in s..=e {
                        {
                            let frame = call_stack.last_mut().unwrap();
                            frame.env_int.insert(var.clone(), i);
                        }
                        if let Some(ret) = exec_stmts(body, funcs, call_stack) {
                            // Interpret special control returns: 1 = EXIT, 2 = CYCLE
                            if let Some(v) = ret.0 {
                                if v == 1 {
                                    exited=true; break 'outer;
                                } else if v == 2 {
                                    continue 'outer; // CYCLE
                                }
                            }
                            return Some(ret);
                        }
                    }
                    if !exited { call_stack.last_mut().unwrap().env_int.insert(var.clone(), if s>e {s} else {e.checked_add(1).expect("DO index overflow")}); }
                }
                Stmt::Return(expr_opt) => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    if let Some(e) = expr_opt {
                        let vt = eval_expr(e, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                        return Some(vt);
                    } else {
                        let fr = call_stack.last().unwrap();
                        if fr.is_function {
                            if let Some(fname) = &fr.func_name {
                                return Some(fr.result_value(fname));
                            }
                        }
                        return Some(ValueTuple::from_int(0));
                    }
                }
                Stmt::CallSub { name, args } => {
                    if name=="@ptr_bind" {
                        let [Expr::Ident(data),Expr::Ident(flags),Expr::Ident(label),Expr::Ident(target),Expr::Ident(flag)]=args.as_slice() else {unreachable!()};
                        let frame_index=call_stack.len()-1;
                        let handles=POINTER_STORAGE.with(|storage| {
                            let mut storage=storage.borrow_mut();
                            [target,flag].map(|n| {
                                let handle=if let Some(i)=storage.iter().position(|value|matches!(value,PointerStorage::Frame(f,v) if *f==frame_index && v==n)) {i}
                                    else {storage.push(PointerStorage::Frame(frame_index,n.clone()));storage.len()-1};
                                handle as i64
                            })
                        });
                        let fr=call_stack.last_mut().unwrap();
                        fr.env_int.insert(data.clone(),handles[0]);fr.env_int.insert(flags.clone(),handles[1]);fr.env_int.insert(label.clone(),handles[0]);
                        idx+=1;continue;
                    }
                    if let Some(kind)=name.strip_prefix("@ptr_store_") {
                        let fr=call_stack.last().unwrap().clone();
                        let values=args.iter().map(|a|eval_expr(a,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack)).collect::<Vec<_>>();
                        let (target_storage,flag_storage)=POINTER_STORAGE.with(|storage| {
                            let storage=storage.borrow();
                            (storage[values[0].0.unwrap() as usize].clone(),storage[values[1].0.unwrap() as usize].clone())
                        });
                        if let PointerStorage::Record(allocation,_,_)=target_storage {
                            let offset=usize::try_from(values[2].0.unwrap()).expect("negative pointer store offset");
                            let value=&values[3];
                            RECORD_ALLOCATIONS.with(|records| {
                                let mut records=records.borrow_mut();
                                let record=io_result(records.get_mut(allocation).and_then(Option::as_mut).ok_or_else(||anyhow::anyhow!("pointer target allocation no longer exists")));
                                if kind=="clear" {record.mask[offset]=0;return;}
                                if matches!(kind,"r4"|"r8") {let value=value.1.unwrap_or_else(||value.0.unwrap_or(0) as f64);record.reals.as_mut().unwrap()[offset]=if kind=="r4" {value as f32 as f64} else {value};}
                                else {record.integers.as_mut().unwrap()[offset]=if kind=="l" {i64::from(value.2.unwrap_or(false))}else{value.0.unwrap_or_else(||value.1.unwrap_or(0.0) as i64)};}
                                record.mask[offset]=1;
                            });
                            idx+=1;continue;
                        }
                        let PointerStorage::Frame(frame_index,target)=target_storage else {unreachable!()};
                        let PointerStorage::Frame(flag_index,flag)=flag_storage else {unreachable!()};
                        assert_eq!(frame_index,flag_index);
                        let frame=&mut call_stack[frame_index];
                        let offset=usize::try_from(values[2].0.unwrap()).expect("negative pointer store offset");let value=&values[3];
                        if kind=="clear" {if let Some(a)=frame.env_int_arrays.get_mut(&target){a[offset]=0;}else{assert_eq!(offset,0);frame.env_int.insert(target,0);}idx+=1;continue;}
                        if matches!(kind,"r4"|"r8") {let v=value.1.unwrap_or_else(||value.0.unwrap_or(0) as f64);let v=if kind=="r4"{v as f32 as f64}else{v};if let Some(a)=frame.env_real_arrays.get_mut(&target){a[offset]=v;}else if frame.env_real32.contains_key(&target){assert_eq!(offset,0);frame.env_real32.insert(target,v as f32);}else{assert_eq!(offset,0);frame.env_real64.insert(target,v);}}
                        else {let v=if kind=="l"{i64::from(value.2.unwrap_or(false))}else{value.0.unwrap_or_else(||value.1.unwrap_or(0.0) as i64)};if let Some(a)=frame.env_int_arrays.get_mut(&target){a[offset]=v;}else if kind=="l"{assert_eq!(offset,0);frame.env_log.insert(target,v!=0);}else{assert_eq!(offset,0);frame.env_int.insert(target,v);}}
                        if let Some(a)=frame.env_int_arrays.get_mut(&flag){a[offset]=1;}else{assert_eq!(offset,0);frame.env_int.insert(flag,1);}idx+=1;continue;
                    }
                    if name=="@heap_bounds" {
                        let fr=call_stack.last().unwrap().clone();
                        let lower=eval_expr(&args[2],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let upper=eval_expr(&args[3],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let extent=i64::try_from((i128::from(upper)-i128::from(lower)+1).max(0)).unwrap_or_else(|_|{eprintln!("Runtime error: allocation extent overflow");std::process::exit(1)});
                        let fr=call_stack.last_mut().unwrap();for (arg,value) in args[..2].iter().zip([lower,extent]){let Expr::Ident(n)=arg else{unreachable!()};fr.env_int.insert(n.clone(),value);}idx+=1;continue;
                    }
                    if name.starts_with("@heap_record_") {
                        let frame=call_stack.last().unwrap().clone();
                        let position=if name=="@heap_record_dispose" || name=="@heap_record_pointer" {0}else{1};
                        let value=eval_expr(&args[position],&frame.env_str,&frame.env_int,&frame.env_real32,&frame.env_real64,&frame.env_log,funcs,call_stack).0.unwrap_or(0);
                        if name=="@heap_record_pointer" {
                            let Expr::Str(label)=&args[1] else{unreachable!()};
                            let metadata=RECORD_ALLOCATIONS.with(|records|records.borrow().get(value as usize).and_then(Option::as_ref).map(|r|r.metadata.clone()));
                            let metadata=io_result(metadata.filter(|m|m[1]!=0).ok_or_else(||anyhow::anyhow!("pointer association requires an allocated record component")));
                            let handles=POINTER_STORAGE.with(|storage| {
                                let mut storage=storage.borrow_mut();
                                [false,true].map(|mask| {
                                    let handle=if let Some(i)=storage.iter().position(|entry|matches!(entry,PointerStorage::Record(id,m,_) if *id==value as usize && *m==mask)) {i}
                                        else {storage.push(PointerStorage::Record(value as usize,mask,label.clone()));storage.len()-1};
                                    handle as i64
                                })
                            });
                            let rank=(args.len()-7)/3;
                            let mut values=vec![handles[0],handles[1],handles[0],1,0];
                            let mut stride=1;
                            for axis in 0..rank {let extent=metadata[2+axis];values.extend([metadata[2+rank+axis],extent,stride]);stride*=extent;}
                            let frame=call_stack.last_mut().unwrap();
                            for (arg,value) in args[2..].iter().zip(values) {let Expr::Ident(n)=arg else{unreachable!()};frame.env_int.insert(n.clone(),value);}
                        } else if name=="@heap_record_dispose" {
                            RECORD_ALLOCATIONS.with(|records| {if let Some(slot)=records.borrow_mut().get_mut(value as usize) {*slot=None;}});
                        } else if name=="@heap_record_drop_array" {
                            let Expr::Ident(array)=&args[0] else{unreachable!()};
                            let handles=frame.env_int_arrays.get(array).cloned().unwrap_or_default();
                            RECORD_ALLOCATIONS.with(|records| {let mut records=records.borrow_mut();for handle in handles.iter().take(value.max(0) as usize) {if let Some(slot)=records.get_mut(*handle as usize) {*slot=None;}}});
                            if let Some(array)=call_stack.last_mut().unwrap().env_int_arrays.get_mut(array) {array.fill(0);}
                        } else {
                            let Expr::Ident(local)=&args[0] else{unreachable!()};
                            let Expr::Ident(mask)=args.last().unwrap() else{unreachable!()};
                            let metadata=&args[2..args.len()-1];
                            if name=="@heap_record_load" {
                                let record=RECORD_ALLOCATIONS.with(|records|records.borrow().get(value as usize).cloned().flatten());
                                let mut values=vec![0;metadata.len()];
                                for value in values.iter_mut().skip(2+(metadata.len()-2)/2) {*value=1;}
                                if let Some(record)=&record {values=record.metadata.clone();}
                                let frame=call_stack.last_mut().unwrap();
                                for (i,(target,value)) in metadata.iter().zip(values).enumerate() {
                                    let Expr::Ident(target)=target else{unreachable!()};
                                    if i==1 {frame.env_log.insert(target.clone(),value!=0);}else{frame.env_int.insert(target.clone(),value);}
                                }
                                if let Some(array)=frame.env_real_arrays.get_mut(local) {*array=record.as_ref().and_then(|r|r.reals.clone()).unwrap_or_default();}
                                else {frame.env_int_arrays.insert(local.clone(),record.as_ref().and_then(|r|r.integers.clone()).unwrap_or_default());}
                                frame.env_int_arrays.insert(mask.clone(),record.map(|r|r.mask).unwrap_or_default());
                            } else {
                                let Expr::Ident(slot)=&args[1] else{unreachable!()};
                                let values=metadata.iter().enumerate().map(|(i,target)|{let Expr::Ident(n)=target else{unreachable!()};if i==1 {i64::from(*frame.env_log.get(n).unwrap_or(&false))}else{*frame.env_int.get(n).unwrap_or(&0)}}).collect();
                                let record=RecordAllocation {integers:frame.env_int_arrays.get(local).cloned(),reals:frame.env_real_arrays.get(local).cloned(),mask:frame.env_int_arrays.get(mask).cloned().unwrap_or_default(),metadata:values};
                                let handle=RECORD_ALLOCATIONS.with(|records| {let mut records=records.borrow_mut();if value>0 && (value as usize)<records.len() {records[value as usize]=Some(record);value}else if let Some(free)=records.iter().enumerate().skip(1).find_map(|(i,r)|r.is_none().then_some(i)) {records[free]=Some(record);free as i64}else{records.push(Some(record));(records.len()-1) as i64}});
                                call_stack.last_mut().unwrap().env_int.insert(slot.clone(),handle);
                            }
                        }
                        call_stack.last_mut().unwrap().commit_saved();idx+=1;continue;
                    }
                    if name=="@heap_bind" {idx+=1;continue;}
                    if name=="@heap_allocate" || name=="@heap_resize" || name=="@heap_free" || name=="@heap_drop" {
                        let resize=name=="@heap_resize";
                        let Expr::Ident(local)=&args[0] else{unreachable!()};
                        let Expr::Ident(mask)=&args[if name=="@heap_allocate" || resize {2}else{1}] else{unreachable!()};
                        let mut length=0usize;
                        if name=="@heap_allocate" || resize {
                            let fr=call_stack.last().unwrap().clone();
                            let dimensions=args[3..].iter().map(|e|eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap()).collect::<Vec<_>>();
                            if !dimensions.contains(&0) {length=dimensions.iter().try_fold(1usize,|total,&extent|total.checked_mul(usize::try_from(extent).ok()?)).filter(|n|*n<=isize::MAX as usize/8).unwrap_or_else(||{eprintln!("Runtime error: allocation size overflow");std::process::exit(1)});}
                            let Expr::Ident(n)=&args[1] else{unreachable!()};call_stack.last_mut().unwrap().env_int.insert(n.clone(),length as i64);
                        }
                        let fr=call_stack.last_mut().unwrap();
                        if let Some(array)=fr.env_real_arrays.get_mut(local) {
                            if !resize {array.clear();}if name!="@heap_allocate" && !resize {*array=Vec::new();}array.try_reserve_exact(length).unwrap_or_else(|_|{eprintln!("Runtime error: array allocation failed");std::process::exit(1)});array.resize(length,0.0);
                        }else{let array=fr.env_int_arrays.get_mut(local).unwrap();if !resize {array.clear();}if name!="@heap_allocate" && !resize {*array=Vec::new();}array.try_reserve_exact(length).unwrap_or_else(|_|{eprintln!("Runtime error: array allocation failed");std::process::exit(1)});array.resize(length,0);}
                        let flags=fr.env_int_arrays.get_mut(mask).unwrap();if !resize {flags.clear();}if name!="@heap_allocate" && !resize {*flags=Vec::new();}flags.try_reserve_exact(length).unwrap_or_else(|_|{eprintln!("Runtime error: initialization mask allocation failed");std::process::exit(1)});flags.resize(length,0);
                        fr.commit_saved();idx+=1;continue;
                    }
                    if name=="@module_bind" {
                        let [Expr::Ident(local),Expr::Str(key),Expr::Str(initializer)]=args.as_slice() else {unreachable!()};
                        let initial:crate::modules::Storage=serde_json::from_str(initializer).unwrap();
                        let fr=call_stack.last().unwrap().clone();
                        let source=if initial.deferred {vec![]}else{initial.values.clone()};
                        let values=source.iter().map(|e|eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack)).collect::<Vec<_>>();
                        if initial.rank!=0 {
                            let array=if matches!(initial.kind,TypeSpec::Real|TypeSpec::DoublePrecision) {ModuleArray::Real(values.iter().map(|v|v.1.unwrap_or(v.0.unwrap_or(0) as f64)).collect())}
                                else {ModuleArray::Integer(values.iter().map(|v|v.0.unwrap_or(i64::from(v.2.unwrap_or(false)))).collect())};
                            MODULE_ARRAYS.with(|arrays|{arrays.borrow_mut().entry(key.clone()).or_insert(array);});
                            let frame=call_stack.last_mut().unwrap();frame.module_arrays.insert(local.clone(),key.clone());
                            if matches!(initial.kind,TypeSpec::DoublePrecision) {frame.double_arrays.insert(local.clone());}
                            if matches!(initial.kind,TypeSpec::Logical) {frame.logical_arrays.insert(local.clone());}
                            frame.sync_saved();
                        } else {
                            SAVED.with(|values_map|{values_map.borrow_mut().entry(key.clone()).or_insert(values[0].clone());});
                            let frame=call_stack.last_mut().unwrap();frame.saved.insert(local.clone(),(key.clone(),initial.kind));frame.sync_saved();
                        }
                        idx+=1;continue;
                    }
                    if name == "@saved_bind" {
                        let [Expr::Ident(local),Expr::Str(key),initializer]=args.as_slice() else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let initial=eval_expr(initializer,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let kind=if fr.env_str.contains_key(local) {TypeSpec::Character(Some(initial.3.as_ref().unwrap().len()))} else if fr.env_real64.contains_key(local) {TypeSpec::DoublePrecision} else if fr.env_real32.contains_key(local) {TypeSpec::Real} else if fr.env_log.contains_key(local) {TypeSpec::Logical} else {TypeSpec::Integer(None)};
                        SAVED.with(|values|{values.borrow_mut().entry(key.clone()).or_insert(initial);});
                        let frame=call_stack.last_mut().unwrap();frame.saved.insert(local.clone(),(key.clone(),kind));frame.sync_saved();
                        idx+=1;continue;
                    }
                    if name == "@stop" && args.is_empty() { std::process::exit(0); }
                    if name == "@error_stop" || name == "@stop" {
                        let keyword=if name=="@stop" {"STOP"} else {"ERROR STOP"};
                        let (message,code)=match args.as_slice() {
                            [Expr::Str(message)]=>(message.clone(),i32::from(name=="@error_stop")),
                            [Expr::IntLit(value)]=>{let code=value.parse::<i32>().unwrap_or_else(|_|{eprintln!("Runtime error: {keyword} integer code must fit default INTEGER");std::process::exit(1)});(code.to_string(),code)},
                            _=>{eprintln!("Runtime error: {keyword} currently requires a CHARACTER or default INTEGER constant");std::process::exit(1)},
                        };
                        eprintln!("{keyword} {message}");std::process::exit(code);
                    }
                    if name == "@character_assign" || name=="@character_element_read" {
                        let [Expr::Ident(dest), source, Expr::IntLit(length)] = args.as_slice() else { unreachable!() };
                        let fr = call_stack.last().unwrap().clone();
                        let mut text = eval_expr(source, &fr.env_str, &fr.env_int, &fr.env_real32, &fr.env_real64, &fr.env_log, funcs, call_stack).3.unwrap();
                        let length: usize = length.parse().unwrap();
                        text.truncate(length);
                        text.extend(std::iter::repeat(' ').take(length - text.len()));
                        call_stack.last_mut().unwrap().env_str.insert(dest.clone(), text);
                        idx += 1; continue;
                    }
                    if name == "@character_code_store" {
                        let [Expr::Ident(dest),index,code]=args.as_slice() else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let index=eval_expr(index,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap() as usize;
                        let code=eval_expr(code,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap() as u8;
                        let mut text=fr.env_str[dest].as_bytes().to_vec();
                        if index>=text.len() {eprintln!("Runtime error: CHARACTER code store outside bounds");std::process::exit(1);}
                        text[index]=code;
                        call_stack.last_mut().unwrap().env_str.insert(dest.clone(),String::from_utf8(text).expect("CHARACTER storage must contain supported ASCII bytes"));
                        idx+=1;continue;
                    }
                    if name == "@character_length" {
                        let [Expr::Ident(dest), source] = args.as_slice() else { unreachable!() };
                        let fr = call_stack.last().unwrap().clone();
                        let text = eval_expr(source, &fr.env_str, &fr.env_int, &fr.env_real32, &fr.env_real64, &fr.env_log, funcs, call_stack).3.unwrap();
                        call_stack.last_mut().unwrap().env_int.insert(dest.clone(), text.len() as i64);
                        idx += 1; continue;
                    }
                    if name == "@internal_read_byte" {
                        let [Expr::Ident(dest),source,Expr::Ident(offset),Expr::IntLit(length)]=args.as_slice() else {unreachable!()};
                        let frame=call_stack.last_mut().unwrap();
                        let text=match source {Expr::Str(text)=>text,Expr::Ident(name)=>&frame.env_str[name],_=>unreachable!()};
                        let index=frame.env_int[offset] as usize;
                        let length:usize=length.parse().unwrap();
                        let byte=if index<length {text.as_bytes().get(index).copied().unwrap_or(0)} else {0};
                        frame.env_int.insert(dest.clone(),i64::from(byte));
                        idx+=1;continue;
                    }
                    if name=="@file_newunit" {
                        let Expr::Ident(dest)=&args[0] else {unreachable!()};
                        let unit=io_result(FILES.with(|files|files.borrow_mut().scratch(None)));
                        call_stack.last_mut().unwrap().env_int.insert(dest.clone(),unit);
                        idx+=1;continue;
                    }
                    if name=="@read_character_field" {
                        let [Expr::Ident(status),Expr::Ident(dest),source,Expr::IntLit(length)]=args.as_slice() else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let text=eval_expr(source,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        let mut buffer=vec![b' ';length.parse().unwrap()];
                        let code=crate::character_input::field(text.as_bytes(),&mut buffer);
                        let fr=call_stack.last_mut().unwrap();
                        fr.env_int.insert(status.clone(),code);fr.env_str.insert(dest.clone(),String::from_utf8(buffer).unwrap());idx+=1;continue;
                    }
                    if name=="@file_name" {
                        let Expr::Ident(dest)=&args[0] else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let mut value=io_result(FILES.with(|files|files.borrow().name(unit)));
                        let Expr::IntLit(length)=&args[2] else {unreachable!()};
                        let length:usize=length.parse().unwrap();
                        value.truncate(length);value.push_str(&" ".repeat(length-value.len()));
                        call_stack.last_mut().unwrap().env_str.insert(dest.clone(),value);idx+=1;continue;
                    }
                    if name=="@cwd_query" {
                        let fr=call_stack.last().unwrap().clone();
                        let width=eval_expr(&args[2],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap() as usize;
                        let mut value=vec![0u8;width+1];let status=unsafe {crate::environment::rfort_getcwd(value.as_mut_ptr(),width)};
                        let frame=call_stack.last_mut().unwrap();
                        let Expr::Ident(n)=&args[0] else {unreachable!()};frame.env_int.insert(n.clone(),status);
                        let Expr::Ident(n)=&args[1] else {unreachable!()};frame.env_str.insert(n.clone(),String::from_utf8_lossy(&value[..width]).into_owned());idx+=1;continue;
                    }
                    if name=="@argument_query" {
                        let fr=call_stack.last().unwrap().clone();
                        let mut eval=|e:&Expr|eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let number=eval(&args[3]).0.unwrap();let width=eval(&args[5]).0.unwrap() as usize;let present=eval(&args[6]).2.unwrap();
                        let mut output=vec![0u8;width+1];let mut length=0;
                        let status=unsafe {crate::environment::rfort_argument_query(number,output.as_mut_ptr(),width,i64::from(present),&mut length)};
                        let frame=call_stack.last_mut().unwrap();
                        for (arg,val) in args[..2].iter().zip([status,length]) {let Expr::Ident(n)=arg else {unreachable!()};frame.env_int.insert(n.clone(),val);}
                        let Expr::Ident(n)=&args[2] else {unreachable!()};frame.env_str.insert(n.clone(),String::from_utf8_lossy(&output[..width]).into_owned());idx+=1;continue;
                    }
                    if name=="@environment_query" {
                        let fr=call_stack.last().unwrap().clone();
                        let mut eval=|e:&Expr|eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let text=eval(&args[3]).3.unwrap();let trim=eval(&args[4]).2.unwrap();
                        let width=eval(&args[5]).0.unwrap() as usize;let present=eval(&args[6]).2.unwrap();
                        let mut output=vec![0u8;width+1];let mut length=0;
                        let status=unsafe {crate::environment::rfort_environment_query(text.as_ptr(),text.len(),i64::from(trim),output.as_mut_ptr(),width,i64::from(present),&mut length)};
                        let frame=call_stack.last_mut().unwrap();
                        for (arg,val) in args[..2].iter().zip([status,length]) {let Expr::Ident(n)=arg else {unreachable!()};frame.env_int.insert(n.clone(),val);}
                        let Expr::Ident(n)=&args[2] else {unreachable!()};frame.env_str.insert(n.clone(),String::from_utf8_lossy(&output[..width]).into_owned());idx+=1;continue;
                    }
                    if matches!(name.as_str(),"@file_inquire_unit"|"@file_inquire_file") {
                        let fr=call_stack.last().unwrap().clone();
                        let value=eval_expr(&args[7],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let (status,number,opened,exists,named,text,action)=FILES.with(|files|files.borrow().inquire(value.0,value.3.as_deref()));
                        let frame=call_stack.last_mut().unwrap();
                        for (arg,value) in args[..5].iter().zip([status,number,opened,exists,named]) {
                            let Expr::Ident(dest)=arg else {unreachable!()};frame.env_int.insert(dest.clone(),value);
                        }
                        for (i,mut text) in [(5,text),(6,action)] {
                            let Expr::Ident(dest)=&args[i] else {unreachable!()};
                            let Expr::IntLit(length)=&args[i+3] else {unreachable!()};let length:usize=length.parse().unwrap();
                            text.truncate(length);text.push_str(&" ".repeat(length-text.len()));frame.env_str.insert(dest.clone(),text);
                        }
                        idx+=1;continue;
                    }
                    if name=="@file_open_dynamic" || name=="@file_open_try" {
                        let Expr::Ident(dest)=&args[0] else {unreachable!()};let fr=call_stack.last().unwrap().clone();
                        let path=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        let mode=eval_expr(&args[2],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let requested=if matches!(&args[3],Expr::IntLit(n) if n=="1") {None}else{Some(eval_expr(&args[5],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap())};
                        let append=matches!(&args[4],Expr::IntLit(n) if n=="1");
                        let opened=FILES.with(|files|files.borrow_mut().open_dynamic(requested,&path,mode,append));
                        // With IOSTAT a failure is a negative code, not an error.
                        let unit=if name=="@file_open_try" {opened.unwrap_or(i64::MIN)} else {io_result(opened)};
                        call_stack.last_mut().unwrap().env_int.insert(dest.clone(),unit);idx+=1;continue;
                    }
                    if name=="@file_write_record" {
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let text=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        io_result(FILES.with(|files|files.borrow_mut().write_record(unit,&text)));idx+=1;continue;
                    }
                    if name=="@file_scratch" {
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        io_result(FILES.with(|files|files.borrow_mut().scratch(Some(unit))));idx+=1;continue;
                    }
                    if name == "@file_flush" {
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                        idx+=1;continue;
                    }
                    if matches!(name.as_str(), "@file_open" | "@file_close" | "@file_close_delete" | "@file_rewind" | "@file_backspace" | "@file_endfile") {
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let path=match args.get(1) {Some(Expr::Str(path))=>Some(path.as_str()),_=>None};
                        io_result(FILES.with(|files| files.borrow_mut().operation(name,unit,path)));
                        idx+=1;continue;
                    }
                    if name=="@list_read_begin" || name=="@list_read_internal_begin" {
                        let [Expr::Ident(handle),Expr::Ident(status),unit]=args.as_slice() else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let value=eval_expr(unit,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let internal=name=="@list_read_internal_begin";
                        let unit=if internal {0}else{value.0.unwrap()};
                        let source=if internal {Some((value.3.unwrap().into_bytes(),0usize))}else{None};
                        let h=OUTPUT_SERIAL.with(|s|{let n=s.get()+1;s.set(n);n});
                        INPUT_LISTS.with(|lists|lists.borrow_mut().insert(h,(unit,crate::list_input::Session::new(),source)));
                        let fr=call_stack.last_mut().unwrap();fr.env_int.insert(handle.clone(),h);fr.env_int.insert(status.clone(),0);idx+=1;continue;
                    }
                    // Unformatted stream transfers of one scalar value, as the
                    // bytes of its kind.
                    if name=="@stream_put" || name=="@stream_get" || name=="@stream_seek" {
                        let fr=call_stack.last().unwrap().clone();
                        let eval=|e:&Expr,call_stack:&mut Vec<CallFrame>|eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                        let Expr::Ident(status)=&args[0] else {unreachable!()};
                        let code=if name=="@stream_seek" {
                            let unit=eval(&args[1],call_stack).0.unwrap();let position=eval(&args[2],call_stack).0.unwrap();
                            FILES.with(|files|files.borrow_mut().stream_seek(unit,position))
                        } else {
                            let (unit,value)=if name=="@stream_put" {(&args[1],&args[2])} else {(&args[2],&args[1])};
                            let unit=eval(unit,call_stack).0.unwrap();
                            let [Expr::IntLit(tag),Expr::IntLit(width)]=&args[3..] else {unreachable!()};
                            let tag:i64=tag.parse().unwrap();let width:usize=width.parse().unwrap();
                            let Expr::Ident(value)=value else {unreachable!()};
                            if name=="@stream_put" {
                                let bytes=match tag {
                                    1=>fr.env_int[value].to_le_bytes()[..width].to_vec(),
                                    2=>if width==4 {fr.env_real32[value].to_le_bytes().to_vec()} else {fr.env_real64[value].to_le_bytes().to_vec()},
                                    3=>(i32::from(fr.env_log[value])).to_le_bytes().to_vec(),
                                    _=>{let mut text=fr.env_str[value].as_bytes().to_vec();text.resize(width,b' ');text},
                                };
                                FILES.with(|files|files.borrow_mut().stream_write(unit,&bytes))
                            } else {
                                let mut bytes=vec![0;width];
                                let code=FILES.with(|files|files.borrow_mut().stream_read(unit,&mut bytes));
                                if code==0 {
                                    let frame=call_stack.last_mut().unwrap();
                                    match tag {
                                        // Sign-extend from the value's kind.
                                        1=>{let mut wide=[if bytes[width-1]&0x80!=0 {0xff} else {0};8];wide[..width].copy_from_slice(&bytes);frame.env_int.insert(value.clone(),i64::from_le_bytes(wide));}
                                        2=>if width==4 {frame.env_real32.insert(value.clone(),f32::from_le_bytes(bytes.try_into().unwrap()));} else {frame.env_real64.insert(value.clone(),f64::from_le_bytes(bytes.try_into().unwrap()));},
                                        3=>{frame.env_log.insert(value.clone(),bytes.iter().any(|b|*b!=0));}
                                        _=>{frame.env_str.insert(value.clone(),String::from_utf8_lossy(&bytes).into_owned());}
                                    }
                                }
                                code
                            }
                        };
                        call_stack.last_mut().unwrap().env_int.insert(status.clone(),code);
                        idx+=1;continue;
                    }
                    if name=="@list_read_next" || name=="@list_read_complex_next" || name=="@list_read_end" {
                        let fr=call_stack.last().unwrap();let Expr::Ident(h)=&args[if name=="@list_read_next" {2}else if name=="@list_read_complex_next" {3}else{1}] else {unreachable!()};let h=fr.env_int[h];
                        if name=="@list_read_end" {
                            let (unit,mut session,mut source)=INPUT_LISTS.with(|lists|lists.borrow_mut().remove(&h).unwrap());
                            let code=FILES.with(|files|session.finish(||if let Some((text,offset))=&mut source {let c=text.get(*offset).copied();*offset+=usize::from(c.is_some());Ok(c)}else{files.borrow_mut().input_byte(unit)}));
                            let Expr::Ident(status)=&args[0] else {unreachable!()};let fr=call_stack.last_mut().unwrap();if fr.env_int[status]==0 {fr.env_int.insert(status.clone(),code);}idx+=1;continue;
                        }
                        if name=="@list_read_complex_next" {
                            let [Expr::Ident(status),Expr::Ident(re),Expr::Ident(im),_,Expr::IntLit(width)]=args.as_slice() else {unreachable!()};
                            let width:usize=width.parse().unwrap();let mut out=[0;16];
                            let code=INPUT_LISTS.with(|lists|{let mut lists=lists.borrow_mut();let (unit,session,source)=lists.get_mut(&h).unwrap();FILES.with(|files|session.next(5,width,&mut out,||if let Some((text,offset))=source {let c=text.get(*offset).copied();*offset+=usize::from(c.is_some());Ok(c)}else{files.borrow_mut().input_byte(*unit)}))});
                            let fr=call_stack.last_mut().unwrap();fr.env_int.insert(status.clone(),code);
                            if code==0 {for (name,bytes) in [(re,&out[..8]),(im,&out[8..])] {let n=f64::from_ne_bytes(bytes.try_into().unwrap());if width==4 {fr.env_real32.insert(name.clone(),n as f32);}else{fr.env_real64.insert(name.clone(),n);}}}idx+=1;continue;
                        }
                        let [Expr::Ident(status),Expr::Ident(dest),_,Expr::IntLit(tag),Expr::IntLit(width)]=args.as_slice() else {unreachable!()};
                        let tag:i64=tag.parse().unwrap();let width:usize=width.parse().unwrap();let mut out=vec![0;if tag==4 {width}else{8}];
                        let code=INPUT_LISTS.with(|lists|{let mut lists=lists.borrow_mut();let (unit,session,source)=lists.get_mut(&h).unwrap();FILES.with(|files|session.next(tag,width,&mut out,||if let Some((text,offset))=source {let c=text.get(*offset).copied();*offset+=usize::from(c.is_some());Ok(c)}else{files.borrow_mut().input_byte(*unit)}))});
                        let fr=call_stack.last_mut().unwrap();fr.env_int.insert(status.clone(),code);
                        if code==0 {match tag {
                            1=>{fr.env_int.insert(dest.clone(),i64::from_ne_bytes(out.try_into().unwrap()));}
                            2=>{let n=f64::from_ne_bytes(out.try_into().unwrap());if width==4 {fr.env_real32.insert(dest.clone(),n as f32);}else{fr.env_real64.insert(dest.clone(),n);}}
                            3=>{fr.env_log.insert(dest.clone(),i64::from_ne_bytes(out.try_into().unwrap())!=0);}
                            4=>{fr.env_str.insert(dest.clone(),String::from_utf8(out).unwrap());}
                            _=>unreachable!()
                        }}idx+=1;continue;
                    }
                    if name == "@read_record_status" || name=="@read_record_checked" {
                        let [Expr::Ident(status),Expr::Ident(dest),Expr::IntLit(length),unit,..] = args.as_slice() else { unreachable!() };
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(unit,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let mut buffer=vec![b' ';length.parse().unwrap()];
                        let valid=args.get(4).is_none_or(|format| {
                            let text=eval_expr(format,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                            crate::record_input::valid_format(text.as_bytes())
                        });
                        let code=if valid {FILES.with(|files| files.borrow_mut().record_status_mode(unit,&mut buffer,name=="@read_record_checked"))} else {crate::record_input::INVALID_FORMAT};
                        let frame=call_stack.last_mut().unwrap();
                        frame.env_int.insert(status.clone(),code);
                        if code == 0 { frame.env_str.insert(dest.clone(),String::from_utf8(buffer).unwrap()); }
                        idx+=1;continue;
                    }
                    if name == "@read_record" {
                        let Expr::Ident(dest)=&args[0] else {unreachable!()};
                        let Expr::IntLit(length)=&args[1] else {unreachable!()};
                        let length:usize=length.parse().unwrap();
                        if let Some(unit) = args.get(2) {
                            let fr=call_stack.last().unwrap().clone();
                            let unit=eval_expr(unit,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                            let input=io_result(FILES.with(|files| files.borrow_mut().record(unit,length)));
                            call_stack.last_mut().unwrap().env_str.insert(dest.clone(),input);
                            idx+=1;continue;
                        }
                        let mut input=String::new();
                        match std::io::stdin().read_line(&mut input) {
                            Ok(0) | Err(_) => {eprintln!("Runtime error: end of file or input error in formatted READ");std::process::exit(1);}
                            _ => {}
                        }
                        if input.ends_with('\n') {input.pop();if input.ends_with('\r') {input.pop();}}
                        if input.bytes().any(|c| c==0 || c>127) {
                            eprintln!("Runtime error: formatted READ currently requires non-NUL ASCII input");std::process::exit(1);
                        }
                        input.truncate(length);
                        input.push_str(&" ".repeat(length-input.len()));
                        call_stack.last_mut().unwrap().env_str.insert(dest.clone(),input);
                        idx+=1;continue;
                    }
                    if name=="@format_begin" {
                        let Expr::Ident(dest)=&args[0] else {unreachable!()};
                        let fr=call_stack.last().unwrap().clone();
                        let format=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        let advance=eval_expr(&args[2],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap()!=0;
                        let handle=OUTPUT_SERIAL.with(|serial|{let value=serial.get()+1;serial.set(value);value});
                        OUTPUT_LISTS.with(|lists|lists.borrow_mut().insert(handle,(format,advance,vec![])));
                        call_stack.last_mut().unwrap().env_int.insert(dest.clone(),handle);
                        idx+=1;continue;
                    }
                    if name=="@format_item" || name=="@format_end" || name=="@internal_format_end" || name=="@format_end_file" {
                        let fr=call_stack.last().unwrap().clone();
                        let handle=eval_expr(&args[usize::from(name=="@internal_format_end")],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        if name=="@format_item" {
                            let value=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                            let value=if let Some(s)=value.3 {Expr::Str(s)} else if let Some(b)=value.2 {Expr::Logical(b)}
                                // DBLE marks a value that is not exact in single precision
                                // as DOUBLE PRECISION for the output kind.
                                else if let Some(f)=value.1 {let text=Expr::RealLit(format!("{f:.17e}"));if (f as f32) as f64==f {text} else {Expr::Call("dble".into(),vec![text])}} else {Expr::IntLit(value.0.unwrap().to_string())};
                            OUTPUT_LISTS.with(|lists|lists.borrow_mut().get_mut(&handle).unwrap().2.push(value));
                        } else {
                            let (format,advance,items)=OUTPUT_LISTS.with(|lists|lists.borrow_mut().remove(&handle).unwrap());
                            let internal=name=="@internal_format_end";
                            let destination=if internal {Some((args[0].clone(),args[2].clone()))} else {None};
                            let file=if name=="@format_end_file" {Some(args[1].clone())}else{None};
                            let mut args=vec![Expr::Str(format)];args.extend(items);
                            if let Some(unit)=file {args.push(unit);exec_stmts(&[Stmt::CallSub{name:"@file_formatted_record".into(),args}],funcs,call_stack);idx+=1;continue;}
                            if let Some((target,length))=destination {args.extend([target,length]);}
                            exec_stmts(&[Stmt::CallSub {name:if internal {"@internal_formatted_record"} else if advance {"@formatted_print"} else {"@formatted_write_noadvance"}.into(),args}],funcs,call_stack);
                        }
                        idx+=1;continue;
                    }
                    if name == "@formatted_print" || name == "@formatted_write_noadvance" || name=="@internal_formatted_record" || name=="@file_formatted_record" {
                        let internal=name=="@internal_formatted_record";
                        let external=name=="@file_formatted_record";
                        let end=args.len()-if internal {2} else if external {1}else {0};
                        let fr=call_stack.last().unwrap().clone();
                        // A whole array item, as the result of MAXLOC, gives its elements.
                        let expanded:Vec<Expr>;
                        let args=if args[1..end].iter().any(|e|matches!(e,Expr::Ident(n) if fr.env_int_arrays.contains_key(n)||fr.env_real_arrays.contains_key(n))) {
                            let mut items=vec![args[0].clone()];
                            for e in &args[1..end] {
                                let length=match e {Expr::Ident(n)=>fr.env_int_arrays.get(n).map(Vec::len).or_else(||fr.env_real_arrays.get(n).map(Vec::len)),_=>None};
                                match (e,length) {
                                    (Expr::Ident(n),Some(length))=>items.extend((1..=length).map(|i|Expr::Index(n.clone(),vec![Expr::IntLit(i.to_string())]))),
                                    _=>items.push(e.clone()),
                                }
                            }
                            items.extend(args[end..].iter().cloned());
                            expanded=items;&expanded
                        } else {args};
                        let end=args.len()-if internal {2} else if external {1}else {0};
                        let format=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        let fields=crate::formatting::parse(&format,end-1).unwrap_or_else(|e| {
                            eprintln!("Runtime error: invalid output format: {e}"); std::process::exit(1)
                        });
                        let values:Vec<_>=args[1..end].iter().map(|e| {
                            let fr=call_stack.last().unwrap().clone();
                            eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack)
                        }).collect();
                        // The interpreter holds REAL values as f64; recover the kind
                        // from the expression where it names one.
                        fn real_kind(e:&Expr,fr:&CallFrame,value:f64)->u8 {
                            match e {
                                Expr::Ident(n) if fr.env_real32.contains_key(n) => 4,
                                Expr::Ident(n) if fr.env_real64.contains_key(n) => 8,
                                Expr::Index(n,_) | Expr::Call(n,_) if fr.double_arrays.contains(n) => 8,
                                // Temporary arrays may not record a DOUBLE PRECISION kind, so
                                // an element is single only if it is exact in single precision.
                                Expr::Index(n,_) | Expr::Call(n,_) if fr.env_real_arrays.contains_key(n) => if (value as f32) as f64==value {4} else {8},
                                Expr::RealLit(text) => if text.contains(['d','D']) || text.ends_with("_8") {8} else {4},
                                Expr::Call(n,_) if n=="dble" => 8,
                                Expr::Call(n,a) if !a.is_empty() && (crate::intrinsics::math_arity(n).is_some() || matches!(n.as_str(),"abs"|"sign"|"mod"|"modulo"|"max"|"min")) => real_kind(&a[0],fr,value),
                                Expr::Call(n,a) if n=="real" && a.len()==1 => 4,
                                Expr::Bin(_,a,b) => real_kind(a,fr,value).max(real_kind(b,fr,value)),
                                Expr::Un(_,a) => real_kind(a,fr,value),
                                _ => if (value as f32) as f64==value {4} else {8},
                            }
                        }
                        let frame=call_stack.last().unwrap().clone();
                        let items:Vec<(crate::format_spec::Value<'static>,Option<String>)>=args[1..end].iter().zip(&values).map(|(e,v)| {
                            use crate::format_spec::Value;
                            if let Some(s)=&v.3 {(Value::Logical(false),Some(s.clone()))}
                            else if let Some(b)=v.2 {(Value::Logical(b),None)}
                            else if let Some(f)=v.1 {(Value::Real(f,real_kind(e,&frame,f)),None)}
                            else {let i=v.0.unwrap();(Value::Integer(i,if i==i as i32 as i64 {4} else {8}),None)}
                        }).collect();
                        let mut bytes=Vec::new();
                        let rendered=crate::format_spec::render_with(format.as_bytes(),items.len(),&|i,f|{
                            match &items[i] {(_,Some(s))=>f(crate::format_spec::Value::Character(s.as_bytes())),(v,None)=>f(*v)}
                        },&mut |b|bytes.push(b),name!="@formatted_write_noadvance");
                        if let Err(e)=rendered {eprintln!("Runtime error: invalid output format: {e}"); std::process::exit(1)}
                        let mut output=String::from_utf8_lossy(&bytes).into_owned();
                        if internal {
                            let Expr::Ident(destination)=&args[end] else {unreachable!()};
                            let length=eval_expr(&args[end+1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap() as usize;
                            if fields.iter().any(|field|field.literal.as_deref()==Some("\n")) {io_result::<()>(Err(anyhow::anyhow!("internal WRITE requires a single record; multiple records are not yet supported")));}
                            if output.len()>length {io_result::<()>(Err(anyhow::anyhow!("internal WRITE record exceeds CHARACTER destination length")));}
                            output.extend(std::iter::repeat_n(' ',length-output.len()));
                            call_stack.last_mut().unwrap().env_str.insert(destination.clone(),output);
                            idx+=1;continue;
                        }
                        if external {
                            let unit=eval_expr(&args[end],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                            let output=output.strip_suffix('\n').unwrap_or(&output);
                            io_result(FILES.with(|files|files.borrow_mut().write_record(unit,output)));idx+=1;continue;
                        }
                        print!("{output}");
                        if name == "@formatted_write_noadvance" {
                            use std::io::Write;
                            io_result(std::io::stdout().flush().map_err(Into::into));
                        } else {
                            println!();
                        }
                        idx+=1;continue;
                    }
                    if name.eq_ignore_ascii_case("random_seed") && !funcs.contains_key(name) { RANDOM_STATE.with(|s| s.set(1)); idx += 1; continue; }
                    if name == "@date_snapshot" {
                        let values = crate::calendar::snapshot();
                        let Expr::Index(array, _) = &args[0] else {panic!("invalid calendar snapshot buffer")};
                        call_stack.last_mut().unwrap().env_int_arrays.get_mut(array).unwrap()[..8].copy_from_slice(&values);
                        idx += 1;
                        continue;
                    }
                    if name=="@random_seed_put" || name=="@random_seed_get" {
                        let fr=call_stack.last().unwrap();
                        let (strings,ints,r32,r64,logs)=(fr.env_str.clone(),fr.env_int.clone(),fr.env_real32.clone(),fr.env_real64.clone(),fr.env_log.clone());
                        if name=="@random_seed_put" {
                            let low=eval_expr(&args[0],&strings,&ints,&r32,&r64,&logs,funcs,call_stack).0.unwrap() as u32 as u64;
                            let high=eval_expr(&args[1],&strings,&ints,&r32,&r64,&logs,funcs,call_stack).0.unwrap() as u32 as u64;
                            let state=low | (high<<32);RANDOM_STATE.with(|s|s.set(if state==0 {1} else {state}));
                        } else {
                            let state=RANDOM_STATE.with(|s|s.get());
                            for (word,arg) in args.iter().enumerate() {
                                let Expr::Index(array,indices)=arg else {panic!("invalid RANDOM_SEED GET target")};
                                let index=eval_expr(&indices[0],&strings,&ints,&r32,&r64,&logs,funcs,call_stack).0.unwrap();
                                call_stack.last_mut().unwrap().env_int_arrays.get_mut(array).unwrap()[index as usize-1]=(state>>(word*32)) as u32 as i32 as i64;
                            }
                        }
                        idx+=1;continue;
                    }
                    if name == "@check_initialized" {
                        let fr = call_stack.last().unwrap();
                        let (strings, ints, real32, real64, logs) = (fr.env_str.clone(), fr.env_int.clone(), fr.env_real32.clone(), fr.env_real64.clone(), fr.env_log.clone());
                        let value = eval_expr(&args[0], &strings, &ints, &real32, &real64, &logs, funcs, call_stack);
                        if value.0 != Some(1) {
                            let variable=eval_expr(&args[1],&strings,&ints,&real32,&real64,&logs,funcs,call_stack).3.expect("invalid initialization guard name");
                            let location = match args.get(3) { Some(Expr::Str(location)) => format!(" at {location}"), _ => String::new() };
                            if let Some(index) = args.get(2).filter(|e| !matches!(e, Expr::Str(_))) {
                                let index = eval_expr(index, &strings, &ints, &real32, &real64, &logs, funcs, call_stack);
                                eprintln!("Runtime error: read of uninitialized variable `{variable}`: element {}{location}", index.0.unwrap());
                            } else {
                                eprintln!("Runtime error: read of uninitialized variable `{variable}`{location}");
                            }
                            std::process::exit(1);
                        }
                        idx += 1;
                        continue;
                    }
                    if name.eq_ignore_ascii_case("random_number") && !funcs.contains_key(name) {
                        let (id, index) = match &args[0] {
                            Expr::Ident(id) => (id, None),
                            Expr::Call(id, indices) | Expr::Index(id, indices) => {
                                let fr = call_stack.last().unwrap().clone();
                                let value = eval_expr(&indices[0], &fr.env_str, &fr.env_int,
                                    &fr.env_real32, &fr.env_real64, &fr.env_log, funcs, call_stack);
                                (id, Some((value.0.unwrap() - 1) as usize))
                            }
                            _ => unreachable!("validated RANDOM_NUMBER destination"),
                        };
                        let fr = call_stack.last_mut().unwrap();
                        if let Some(values) = fr.env_real_arrays.get_mut(id) {
                            let double = fr.double_arrays.contains(id);
                            if let Some(index) = index {
                                values[index] = random_value(double);
                            } else {
                                for value in values { *value = random_value(double); }
                            }
                        } else if let Some(value) = fr.env_real64.get_mut(id) {
                            *value = random_value(true);
                        } else if let Some(value) = fr.env_real32.get_mut(id) {
                            *value = random_value(false) as f32;
                        }
                        idx += 1;
                        continue;
                    }
                    if let Some(fd) = funcs.get(name) {
                        let (strs, ints, r32, r64, logs) = {
                            let fr = call_stack.last().unwrap();
                            (
                                fr.env_str.clone(),
                                fr.env_int.clone(),
                                fr.env_real32.clone(),
                                fr.env_real64.clone(),
                                fr.env_log.clone(),
                            )
                        };
                        let mut sub_frame = CallFrame::new(fd, None);
                        for (i, pn) in fd.params.iter().enumerate() {
                            if let Some(arg) = args.get(i) {
                                let vt = eval_expr(
                                    arg, &strs, &ints, &r32, &r64, &logs, funcs, call_stack,
                                );
                                if let Some(iv) = vt.0 {
                                    sub_frame.env_int.insert(pn.clone(), iv);
                                } else if let Some(fv) = vt.1 {
                                    sub_frame.env_real64.insert(pn.clone(), fv);
                                } else if let Some(bv) = vt.2 {
                                    sub_frame.env_log.insert(pn.clone(), bv);
                                } else if let Some(sv) = vt.3 {
                                    sub_frame.env_str.insert(pn.clone(), sv);
                                }
                            }
                        }
                        call_stack.push(sub_frame);
                        let _ = exec_stmts(&fd.body, funcs, call_stack);
                        call_stack.pop();
                    }
                    if name.to_ascii_lowercase() == "read" {
                        let fr = call_stack.last_mut().unwrap();
                        use std::io;
                        let mut input = String::new();
                        for arg in args {
                            input.clear();
                            io::stdin().read_line(&mut input).ok();
                            let tok = input.trim_end();
                            match arg {
                                Expr::Ident(id) => {
                                    if fr.env_int.contains_key(id) {
                                        let v = parse_i64_lossy(tok);
                                        fr.env_int.insert(id.clone(), v);
                                    } else if fr.env_real64.contains_key(id) {
                                        let v = parse_f64_lossy(tok);
                                        fr.env_real64.insert(id.clone(), v);
                                    } else if fr.env_real32.contains_key(id) {
                                        let v = parse_f64_lossy(tok) as f32;
                                        fr.env_real32.insert(id.clone(), v);
                                    } else if fr.env_log.contains_key(id) {
                                        let v = matches!(
                                            tok.to_ascii_lowercase().as_str(),
                                            ".true." | "true" | "1"
                                        );
                                        fr.env_log.insert(id.clone(), v);
                                    } else if fr.env_str.contains_key(id) {
                                        fr.env_str.insert(id.clone(), tok.to_string());
                                    } else {
                                        let v = parse_i64_lossy(tok);
                                        fr.env_int.insert(id.clone(), v);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Stmt::Function { .. } | Stmt::Subroutine { .. } => {}
                Stmt::Use { .. } | Stmt::Module { .. } => {}
                Stmt::Implicit(_) => {
                    // Implicit statements are handled during semantic analysis
                }
                Stmt::SourceLocation(_) => {}
                Stmt::DerivedType { .. } | Stmt::DerivedDecl { .. } | Stmt::OperatorInterface { .. } | Stmt::GenericInterface { .. } => unreachable!("derived declarations must be lowered before execution"),
                Stmt::DummyIntent { .. } | Stmt::Accessibility { .. } => {}
                Stmt::Block { body } => {
                    if let Some(ret) = exec_stmts(body, funcs, call_stack) {
                        return Some(ret);
                    }
                }
                Stmt::Exit => {
                    return Some(ValueTuple::from_int(1));
                }
                Stmt::Cycle => {
                    return Some(ValueTuple::from_int(2));
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };

                    let sel = eval_expr(expr, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                    let mut matched = false;
                    for cb in cases {
                        for item in &cb.items {
                            match item {
                                CaseItem::Single(e) => {
                                    let v = eval_expr(
                                        e, &strs, &ints, &r32, &r64, &logs, funcs, call_stack,
                                    );

                                    if let (Some(left),Some(right))=(&sel.3,&v.3) {
                                        if crate::character_input::compare(left.as_bytes(),right.as_bytes())==0 {
                                            matched=true;if let Some(ret)=exec_stmts(&cb.body,funcs,call_stack) {return Some(ret);}break;
                                        }continue;
                                    }
                                    if let (Some(si), Some(ii)) = (sel.0, v.0) {
                                        if si == ii {
                                            matched = true;
                                            if let Some(ret) =
                                                exec_stmts(&cb.body, funcs, call_stack)
                                            {
                                                if let Some(v) = ret.0 {
                                                    if v == 1 || v == 2 {
                                                        return Some(ret);
                                                    }
                                                }
                                                return Some(ret);
                                            }
                                            break;
                                        } else {
                                            let eq = if let (Some(sf), Some(vf)) = (sel.1, v.1) {
                                                (sf - vf).abs() < 1e-12
                                            } else if let (Some(ss), Some(vs)) =
                                                (sel.3.clone(), v.3.clone())
                                            {
                                                ss == vs
                                            } else {
                                                false
                                            };
                                            if eq {
                                                matched = true;
                                                if let Some(ret) =
                                                    exec_stmts(&cb.body, funcs, call_stack)
                                                {
                                                    if let Some(v) = ret.0 {
                                                        if v == 1 || v == 2 {
                                                            return Some(ret);
                                                        }
                                                    }
                                                    return Some(ret);
                                                }
                                                break;
                                            }
                                        }
                                    }
                                }
                                CaseItem::Range(l, r) => {
                                    let lv = eval_expr(
                                        l, &strs, &ints, &r32, &r64, &logs, funcs, call_stack,
                                    );
                                    let rv = eval_expr(
                                        r, &strs, &ints, &r32, &r64, &logs, funcs, call_stack,
                                    );
                                    if let (Some(si), Some(li), Some(ri)) = (sel.0, lv.0, rv.0) {
                                        if si >= li && si <= ri {
                                            matched = true;
                                            if let Some(ret) =
                                                exec_stmts(&cb.body, funcs, call_stack)
                                            {
                                                return Some(ret);
                                            }
                                            break;
                                        }
                                    } else if let (Some(sf), Some(lf), Some(rf)) =
                                        (sel.1, lv.1, rv.1)
                                    {
                                        if sf >= lf && sf <= rf {
                                            matched = true;
                                            if let Some(ret) =
                                                exec_stmts(&cb.body, funcs, call_stack)
                                            {
                                                return Some(ret);
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if matched {
                            break;
                        }
                    }
                    if !matched {
                        if let Some(d) = default {
                            if let Some(ret) = exec_stmts(d, funcs, call_stack) {
                                return Some(ret);
                            }
                        }
                    }
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    let (strs, ints, r32, r64, logs) = {
                        let fr = call_stack.last().unwrap();
                        (
                            fr.env_str.clone(),
                            fr.env_int.clone(),
                            fr.env_real32.clone(),
                            fr.env_real64.clone(),
                            fr.env_log.clone(),
                        )
                    };
                    let vt = eval_expr(value, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);

                    let mut idx0 = 0usize;
                    if let Some(first) = indices.get(0) {
                        let vti =
                            eval_expr(first, &strs, &ints, &r32, &r64, &logs, funcs, call_stack);
                        if let Some(i) = vti.0 {
                            idx0 = (i - 1) as usize;
                        }
                    }
                    let frame = call_stack.last_mut().unwrap();
                    frame.sync_saved();
                    // Store into the declared array, converting the value to its
                    // type as intrinsic assignment does.
                    let real = frame.env_real_arrays.contains_key(name);
                    // A REAL(4) array holds single-precision values.
                    let single = real && !frame.double_arrays.contains(name);
                    let vt = if real {
                        ValueTuple(None, vt.1.or(vt.0.map(|i| i as f64)).map(|f| if single { f64::from(f as f32) } else { f }), None, None)
                    } else if frame.env_int_arrays.contains_key(name) && vt.0.is_none() && vt.2.is_none() {
                        ValueTuple(vt.1.map(|f| f.trunc() as i64), None, None, None)
                    } else {
                        vt
                    };
                    if let Some(i) = vt.0.or(vt.2.map(i64::from)) {
                        let vec = frame
                            .env_int_arrays
                            .entry(name.clone())
                            .or_insert_with(Vec::new);
                        if idx0 >= vec.len() {
                            vec.resize(idx0 + 1, 0);
                        }
                        vec[idx0] = i;
                    } else if let Some(f) = vt.1 {
                        let vec = frame
                            .env_real_arrays
                            .entry(name.clone())
                            .or_insert_with(Vec::new);
                        if idx0 >= vec.len() {
                            vec.resize(idx0 + 1, 0.0);
                        }
                        vec[idx0] = f;
                    }
                }
            }

            call_stack.last_mut().unwrap().commit_saved();
            idx += 1;
        }
        call_stack.last_mut().unwrap().commit_saved();
        None
    }
    let top_frame = CallFrame {
        saved:HashMap::new(),saved_seen:HashMap::new(),module_arrays:HashMap::new(),module_seen:HashMap::new(),
        env_str,
        env_int,
        env_real32,
        env_real64,
        env_log,
        env_int_arrays: HashMap::new(),
        env_real_arrays: HashMap::new(),
        double_arrays: std::collections::HashSet::new(),
        logical_arrays: std::collections::HashSet::new(),
        func_name: None,
        is_function: false,
    };
    let mut call_stack = vec![top_frame];
    let _ = exec_stmts(&program.body, &funcs, &mut call_stack);
    let mut values = HashMap::new();
    if let Some((flags, seed)) = session {
        *seed = RANDOM_STATE.with(|state| state.get());
        let frame = &call_stack[0];
        for (name, flag) in flags {
            if name.starts_with('@') || frame.env_int.get(flag) != Some(&1) { continue; }
            let value = if let Some(v) = frame.env_int.get(name) { Expr::IntLit(v.to_string()) }
                else if let Some(v) = frame.env_real32.get(name) { Expr::RealLit((*v as f64).to_string()) }
                else if let Some(v) = frame.env_real64.get(name) { Expr::RealLit(v.to_string()) }
                else if let Some(v) = frame.env_log.get(name) { Expr::Logical(*v) }
                else if let Some(v) = frame.env_str.get(name) { Expr::Str(v.clone()) }
                else { continue; };
            values.insert(name.clone(), value);
        }
    }
    Ok(values)
}

#[no_mangle]
pub extern "C" fn f90c_read_i64(out: *mut i64) -> i32 {
    use std::io;
    if out.is_null() {
        return -1;
    }
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return -1;
    }
    let tok = input.trim();
    let v = i128::from_str_radix(tok, 10)
        .ok()
        .and_then(|x| i64::try_from(x).ok())
        .unwrap_or(0);
    unsafe {
        *out = v;
    }
    0
}

#[no_mangle]
pub extern "C" fn f90c_read_f64(out: *mut f64) -> i32 {
    use std::io;
    if out.is_null() {
        return -1;
    }
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return -1;
    }
    let tok = input.trim();
    let v = tok.parse::<f64>().unwrap_or(0.0);
    unsafe {
        *out = v;
    }
    0
}

#[no_mangle]
pub extern "C" fn f90c_read_bool(out: *mut u8) -> i32 {
    use std::io;
    if out.is_null() {
        return -1;
    }
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return -1;
    }
    let tok = input.trim().to_ascii_lowercase();
    let v = matches!(tok.as_str(), ".true." | "true" | "1");
    unsafe {
        *out = if v { 1 } else { 0 };
    }
    0
}

#[no_mangle]
pub extern "C" fn f90c_read_str(out: *mut u8, len: usize) -> i32 {
    use std::io;
    if out.is_null() || len == 0 {
        return -1;
    }

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return -1;
    }
    let tok = input.trim_end();
    let bytes = tok.as_bytes();
    let copy_len = std::cmp::min(len.saturating_sub(1), bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, copy_len);
        *out.add(copy_len) = 0;
    }
    0
}

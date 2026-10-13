use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec, UnOp};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct FuncDef {
    params: Vec<String>,
    body: Vec<Stmt>,
    is_function: bool,
}

pub fn interpret(program: &Program) {
    thread_local! { static RANDOM_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) }; }
    RANDOM_STATE.with(|state| state.set(1));
    thread_local! { static FILES: std::cell::RefCell<crate::file_io::Files> = std::cell::RefCell::new(crate::file_io::Files::default()); }
    FILES.with(|files| *files.borrow_mut() = crate::file_io::Files::default());
    thread_local! { static POINTER_STORAGE: std::cell::RefCell<Vec<(usize,String)>> = std::cell::RefCell::new(vec![(usize::MAX,String::new())]); }
    POINTER_STORAGE.with(|storage|*storage.borrow_mut()=vec![(usize::MAX,String::new())]);
    thread_local! {
        static OUTPUT_LISTS: std::cell::RefCell<HashMap<i64,(String,bool,Vec<Expr>)>> = std::cell::RefCell::new(HashMap::new());
        static OUTPUT_SERIAL: std::cell::Cell<i64> = const {std::cell::Cell::new(0)};
    }
    OUTPUT_LISTS.with(|lists|lists.borrow_mut().clear());
    OUTPUT_SERIAL.with(|serial|serial.set(0));
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

    #[derive(Clone, Debug, Default)]
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
                                BinOp::Div => {
                                    if b == 0.0 {
                                        0.0
                                    } else {
                                        a / b
                                    }
                                }
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
                if name=="@ptr_name" || name.starts_with("@ptr_load_") {
                    let handle=eval_expr(&args[0],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap();
                    let offset=if name=="@ptr_name" {0}else{eval_expr(&args[1],env_str,env_int,env_real32,env_real64,env_log,funcs,call_stack).0.unwrap()};
                    let (frame_index,target)=POINTER_STORAGE.with(|storage|storage.borrow().get(handle as usize).expect("invalid pointer storage handle").clone());
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
        if let Some((operation,bytes))=crate::bit_intrinsics::decode(name) {
            let values=args.iter().map(to_int).collect::<Option<Vec<_>>>()?;
            let value=crate::bit_intrinsics::evaluate(operation,bytes,&values).ok()?;
            return Some(if operation=="btest" {ValueTuple::from_bool(value!=0)} else {ValueTuple::from_int(value)});
        }
        let n = name.to_ascii_lowercase();
        match n.as_str() {
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
            "aint" | "anint" | "nint" => {
                if let Some(f) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_int(f.trunc() as i64))
                } else {
                    Some(ValueTuple::from_int(to_int(args.get(0)?)?))
                }
            }
            "ceiling" => {
                if let Some(f) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_int(f.ceil() as i64))
                } else {
                    Some(ValueTuple::from_int(to_int(args.get(0)?)?))
                }
            }
            "floor" => {
                if let Some(f) = to_real(args.get(0)?) {
                    Some(ValueTuple::from_int(f.floor() as i64))
                } else {
                    Some(ValueTuple::from_int(to_int(args.get(0)?)?))
                }
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

            "dot_product" | "matmul" => {
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

    fn format_real(f: f64, double: bool) -> String {
        if f.is_finite() && f.abs() > f64::MAX * 0.99 { return format!("{f:.16e}"); }
        if f != 0.0 && (f.abs() < 1e-4 || f.abs() >= if double { 1e15 } else { 1e6 }) {
            if double { format!("{f:.14e}") } else { format!("{f:.5e}") }
        } else if f.fract() == 0.0 {
            format!("{:.1}", f)
        } else if double {
            format!("{:.15}", f)
        } else {
            format!("{:.6}", f)
        }
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
                            TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed | TypeSpec::RealKind(_) | TypeSpec::IntegerKind(_) => unreachable!("unresolved numeric kind"),
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
                            TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed | TypeSpec::RealKind(_) | TypeSpec::IntegerKind(_) => unreachable!("unresolved numeric kind"),
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
                    if let Some(s) = vt.3 {
                        frame.env_str.insert(name.clone(), s);
                        frame.env_int.remove(name);
                    } else if let Some(i) = vt.0 {
                        frame.env_int.insert(name.clone(), i);
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
                        if !fragment { record.push_str(crate::utils::list_directed_separator(previous_character, character)); }
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
                                let handle=if let Some(i)=storage.iter().position(|(f,v)|*f==frame_index && v==n) {i}
                                    else {storage.push((frame_index,n.clone()));storage.len()-1};
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
                        let ((frame_index,target),(flag_index,flag))=POINTER_STORAGE.with(|storage| {
                            let storage=storage.borrow();
                            (storage[values[0].0.unwrap() as usize].clone(),storage[values[1].0.unwrap() as usize].clone())
                        });
                        assert_eq!(frame_index,flag_index);
                        let frame=&mut call_stack[frame_index];
                        let offset=usize::try_from(values[2].0.unwrap()).expect("negative pointer store offset");let value=&values[3];
                        if matches!(kind,"r4"|"r8") {let v=value.1.unwrap_or_else(||value.0.unwrap_or(0) as f64);let v=if kind=="r4"{v as f32 as f64}else{v};if let Some(a)=frame.env_real_arrays.get_mut(&target){a[offset]=v;}else if frame.env_real32.contains_key(&target){assert_eq!(offset,0);frame.env_real32.insert(target,v as f32);}else{assert_eq!(offset,0);frame.env_real64.insert(target,v);}}
                        else {let v=if kind=="l"{i64::from(value.2.unwrap_or(false))}else{value.0.unwrap_or_else(||value.1.unwrap_or(0.0) as i64)};if let Some(a)=frame.env_int_arrays.get_mut(&target){a[offset]=v;}else if kind=="l"{assert_eq!(offset,0);frame.env_log.insert(target,v!=0);}else{assert_eq!(offset,0);frame.env_int.insert(target,v);}}
                        if let Some(a)=frame.env_int_arrays.get_mut(&flag){a[offset]=1;}else{assert_eq!(offset,0);frame.env_int.insert(flag,1);}idx+=1;continue;
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
                    if matches!(name.as_str(), "@file_open" | "@file_close" | "@file_rewind" | "@file_backspace") {
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let path=match args.get(1) {Some(Expr::Str(path))=>Some(path.as_str()),_=>None};
                        io_result(FILES.with(|files| files.borrow_mut().operation(name,unit,path)));
                        idx+=1;continue;
                    }
                    if name == "@read_record_status" {
                        let [Expr::Ident(status),Expr::Ident(dest),Expr::IntLit(length),unit,..] = args.as_slice() else { unreachable!() };
                        let fr=call_stack.last().unwrap().clone();
                        let unit=eval_expr(unit,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        let mut buffer=vec![b' ';length.parse().unwrap()];
                        let valid=args.get(4).is_none_or(|format| {
                            let text=eval_expr(format,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                            crate::record_input::valid_format(text.as_bytes())
                        });
                        let code=if valid {FILES.with(|files| files.borrow_mut().record_status(unit,&mut buffer))} else {crate::record_input::INVALID_FORMAT};
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
                    if name=="@format_item" || name=="@format_end" {
                        let fr=call_stack.last().unwrap().clone();
                        let handle=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).0.unwrap();
                        if name=="@format_item" {
                            let value=eval_expr(&args[1],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack);
                            let value=if let Some(s)=value.3 {Expr::Str(s)} else if let Some(b)=value.2 {Expr::Logical(b)}
                                else if let Some(f)=value.1 {Expr::RealLit(format!("{f:.17e}"))} else {Expr::IntLit(value.0.unwrap().to_string())};
                            OUTPUT_LISTS.with(|lists|lists.borrow_mut().get_mut(&handle).unwrap().2.push(value));
                        } else {
                            let (format,advance,items)=OUTPUT_LISTS.with(|lists|lists.borrow_mut().remove(&handle).unwrap());
                            let mut args=vec![Expr::Str(format)];args.extend(items);
                            exec_stmts(&[Stmt::CallSub {name:if advance {"@formatted_print"} else {"@formatted_write_noadvance"}.into(),args}],funcs,call_stack);
                        }
                        idx+=1;continue;
                    }
                    if name == "@formatted_print" || name == "@formatted_write_noadvance" {
                        let fr=call_stack.last().unwrap().clone();
                        let format=eval_expr(&args[0],&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack).3.unwrap();
                        let fields=crate::formatting::parse(&format,args.len()-1).unwrap_or_else(|e| {
                            eprintln!("Runtime error: invalid output format: {e}"); std::process::exit(1)
                        });
                        let values:Vec<_>=args[1..].iter().map(|e| {
                            let fr=call_stack.last().unwrap().clone();
                            eval_expr(e,&fr.env_str,&fr.env_int,&fr.env_real32,&fr.env_real64,&fr.env_log,funcs,call_stack)
                        }).collect();
                        for (field,value) in fields.iter().filter(|f| f.literal.is_none()).zip(&values) {
                            let tag=if value.3.is_some() {4} else if value.2.is_some() {3} else if value.1.is_some() {2} else {1};
                            if !crate::format_spec::matches_type(field.code as u8,tag) {
                                eprintln!("Runtime error: invalid output format: descriptor does not match output item type"); std::process::exit(1)
                            }
                        }
                        let mut items=values.into_iter();
                        for field in &fields {
                            if let Some(text)=&field.literal {print!("{text}");continue;}
                            let v=items.next().unwrap();
                            let text=match field.code {'a'=>v.3.unwrap(),  'i'=>{let i=v.0.unwrap();if i==0 && field.precision==0 {String::new()}else{format!("{}{:0width$}",if i<0 {"-"} else {""},i.unsigned_abs(),width=field.precision)}}, 'l'=>if v.2.unwrap_or(v.0==Some(1)) {"T".into()}else{"F".into()},_=>format!("{:.*}",field.precision,v.1.unwrap_or(v.0.unwrap_or(0) as f64))};
                            if field.width==0 {if field.code=='i' && text.is_empty() {print!(" ");}else{print!("{text}");}} else if text.len()>field.width {if field.code=='a' {print!("{}",text.chars().take(field.width).collect::<String>());}else{print!("{}","*".repeat(field.width));}}else{print!("{:>width$}",text,width=field.width);}
                        }
                        if name == "@formatted_write_noadvance" {
                            use std::io::Write;
                            io_result(std::io::stdout().flush().map_err(Into::into));
                        } else {
                            println!();
                        }
                        idx+=1;continue;
                    }
                    if name.eq_ignore_ascii_case("random_seed") && !funcs.contains_key(name) { RANDOM_STATE.with(|s| s.set(1)); idx += 1; continue; }
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
            idx += 1;
        }
        None
    }
    let top_frame = CallFrame {
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

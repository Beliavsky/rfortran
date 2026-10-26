// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve supported named constants before semantic analysis and lowering.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

type Constants = HashMap<String, Expr>;
type Modules = HashMap<String, Constants>;

// Give intrinsic imports a separate identity so an explicitly INTRINSIC USE
// never binds to a user module with the same name. Constants then follow the
// ordinary ONLY, visibility, re-export and host-association paths.
pub(crate) fn resolve_intrinsic_modules(program: &mut Program) -> Result<()> {
    const INTERNAL: &str = "@intrinsic_iso_fortran_env";
    let user_modules: HashSet<String> = program.body.iter().filter_map(|s| {
        if let Stmt::Module { name, .. } = s { Some(name.clone()) } else { None }
    }).collect();
    fn visit(body: &mut [Stmt], modules: &HashSet<String>, needed: &mut HashSet<String>) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        for stmt in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location.set(Some(p)); }
            match stmt {
                Stmt::Use { module, nature, only, .. } => {
                    let intrinsic = match nature.take().map(|n| n.to_ascii_lowercase()).as_deref() {
                        Some("intrinsic") => true,
                        Some("non_intrinsic") => {
                            if !modules.contains(module) { bail!("NON_INTRINSIC module `{module}` must be provided in the same source group"); }
                            false
                        }
                        Some(other) => bail!("unknown USE module nature `{other}`; expected INTRINSIC or NON_INTRINSIC"),
                        None => matches!(module.as_str(), "iso_fortran_env" | "ieee_arithmetic" | "iso_c_binding") && !modules.contains(module),
                    };
                    if intrinsic {
                        if !matches!(module.as_str(), "iso_fortran_env" | "ieee_arithmetic" | "iso_c_binding") { bail!("intrinsic module `{module}` is not yet supported"); }
                        needed.insert(module.clone());
                        *module = format!("@intrinsic_{module}");
                        if let Some(names) = only { for name in names { name.make_ascii_lowercase(); } }

                    }
                }
                Stmt::Module { body, .. } | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } | Stmt::Block { body }
                | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => visit(body, modules, needed)?,
                Stmt::If { then_body, else_body, .. } => {
                    visit(then_body, modules, needed)?;
                    if let Some(body) = else_body { visit(body, modules, needed)?; }
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases { visit(&mut case.body, modules, needed)?; }
                    if let Some(body) = default { visit(body, modules, needed)?; }
                }
                _ => {}
            }
        }
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
    let mut needed = HashSet::new();
    visit(&mut program.body, &user_modules, &mut needed)?;
    if needed.contains("iso_fortran_env") && !user_modules.contains(INTERNAL) {
        let mut body: Vec<Stmt> = [("int8",1), ("int16",2), ("int32",4), ("int64",8),
                    ("real32",4), ("real64",8), ("real128",-1),
                    ("logical8",-2), ("logical16",-2), ("logical32",4), ("logical64",-1), ("real16",-2),
                    ("input_unit",5), ("output_unit",6), ("error_unit",0),
                    ("iostat_end",-1), ("iostat_eor",-2), ("iostat_inquire_internal_unit",-3),
                    ("character_storage_size",8), ("numeric_storage_size",32), ("file_storage_size",8)].into_iter()
            .map(|(name,value)| Stmt::InitializedDecl {
                kind: TypeSpec::Integer(None), attributes: vec!["parameter".into()],
                entities: vec![(name.into(),Some(Expr::IntLit(value.to_string())))],
            }).collect();
        body.extend(intrinsic_module_source()?);
        program.body.insert(0, Stmt::Module { name: INTERNAL.into(), body });
    }
    if needed.contains("iso_c_binding") {
        program.body.insert(0,Stmt::Module{name:"@intrinsic_iso_c_binding".into(),body:crate::c_interop::module_body()});
    }
    if needed.contains("ieee_arithmetic") {
        let body = ieee_module_body()?;
        program.body.insert(0, Stmt::Module { name: "@intrinsic_ieee_arithmetic".into(), body });
    }
    Ok(())
}

pub(crate) fn ieee_module_body() -> Result<Vec<Stmt>> {
    let source = include_str!("ieee_arithmetic.f90");
    let tokens = crate::lexer::lex(source);
    let parsed = crate::parser::parse_with_src(source, &tokens, "<ieee_arithmetic>")
        .map_err(|_| anyhow::anyhow!("internal error: IEEE_ARITHMETIC source does not parse"))?;
    let Some(Stmt::Module { mut body, .. }) = parsed.body.into_iter().next() else { unreachable!() };
    fn lower(body: &mut Vec<Stmt>) {
        body.retain(|s| !matches!(s, Stmt::SourceLocation(_)));
        for s in body {
            if let Stmt::Assign { value, .. } = s {
                crate::procedure_arguments::visit_expr(value, &mut |e| {
                    if let Expr::Call(name, _) = e {
                        if name == "ieee_bits4" { *name = "@model:@ieee_value:4".into(); }
                        if name == "ieee_bits8" { *name = "@model:@ieee_value:8".into(); }
                    }
                });
            }
            for child in crate::procedure_arguments::children(s) { lower(child); }
        }
    }
    lower(&mut body);
    Ok(body)
}

// ISO_FORTRAN_ENV entities that are simplest to state in Fortran.
fn intrinsic_module_source() -> Result<Vec<Stmt>> {
    let source = format!("module iso_fortran_env_entities
integer, parameter :: integer_kinds(4) = [1, 2, 4, 8]
integer, parameter :: real_kinds(2) = [4, 8]
integer, parameter :: logical_kinds(1) = [4]
integer, parameter :: character_kinds(1) = [1]
contains
function compiler_version() result(version)
  character(len={}) :: version
  version = '{}'
end function compiler_version
function compiler_options() result(options)
  character(len=1) :: options
  options = ' '
end function compiler_options
end module iso_fortran_env_entities
", COMPILER_VERSION.len(), COMPILER_VERSION);
    let tokens = crate::lexer::lex(&source);
    let program = crate::parser::parse_with_src(&source, &tokens, "<iso_fortran_env>")
        .map_err(|_| anyhow::anyhow!("internal error: ISO_FORTRAN_ENV source does not parse"))?;
    let Some(Stmt::Module { body, .. }) = program.body.into_iter().next() else { bail!("internal error: ISO_FORTRAN_ENV source is not a module") };
    Ok(body)
}
const COMPILER_VERSION: &str = concat!("rfort version ", env!("CARGO_PKG_VERSION"));

// Keep a literal's kind explicit until scalar lowering can allocate typed storage.
pub(crate) fn integer_literal_kind(expr: &Expr) -> Option<u8> {
    match expr {
        Expr::IntLit(_) => Some(4),
        Expr::Call(tag, args) if tag == "@integer_literal" && args.len() == 2 =>
            if let Expr::IntLit(k) = &args[1] { k.parse().ok() } else { None },
        Expr::Un(UnOp::Neg, a) => integer_literal_kind(a),
        Expr::Bin(op, a, b) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) => {
            let (a,b) = (integer_literal_kind(a)?, integer_literal_kind(b)?);
            Some(a.max(b))
        }
        _ => None,
    }
}
fn typed_integer(value: i64, kind: u8) -> Result<Expr> {
    let max = crate::inquiries::integer_huge(Some(kind))?;
    if value < -max-1 || value > max { bail!("integer literal {value} is out of range for kind {kind}"); }
    let value = Expr::IntLit(value.to_string());
    Ok(if kind == 4 { value } else { Expr::Call("@integer_literal".into(),vec![value,Expr::IntLit(kind.to_string())]) })
}

fn kind_number(name: &str, constants: &Constants) -> Result<i64> {
    if let Some(text) = name.strip_prefix(crate::ast::KIND_EXPRESSION) {
        let mut kind: Expr = serde_json::from_str(text)?;
        rewrite_expr(&mut kind, constants)?;
        return evaluate(&kind, constants).context("a kind type parameter must be a constant INTEGER expression");
    }
    if name.split_once('_').is_some_and(|(number,_)| number.parse::<i64>().is_ok()) {
        let mut literal = Expr::IntLit(name.into());
        rewrite_expr(&mut literal,constants)?;
        return evaluate(&literal,constants);
    }
    name.parse().or_else(|_| evaluate(&Expr::Ident(name.into()), constants))
}
fn resolve_kind(kind: &mut TypeSpec, constants: &Constants) -> Result<()> {
    if let TypeSpec::LogicalKind(name)=kind {
        *kind=match kind_number(name,constants)? {
            4=>TypeSpec::Logical,
            other=>bail!("unsupported LOGICAL kind {other}; supported kind is 4"),
        };
    }
    if crate::substrings::deferred(kind) { return Ok(()); }
    if matches!(kind, TypeSpec::CharacterAssumed) { bail!("assumed CHARACTER length requires a PARAMETER declaration or a supported dummy argument"); }
    if let TypeSpec::CharacterLength(length) = kind {
        if matches!(length.as_ref(),Expr::Call(tag,_) if tag == "@invalid_entity_length") { bail!("per-entity character length requires a CHARACTER declaration"); }
        let mut length = (**length).clone();
        rewrite_expr(&mut length, constants)?;
        fold_length_inquiries(&mut length)?;
        let length = evaluate(&length, constants)
            .context("CHARACTER length must be a supported constant INTEGER expression; runtime-dependent lengths are not yet supported")?;
        *kind = TypeSpec::Character(Some(usize::try_from(length.max(0)).context("CHARACTER length is too large")?));
    }
    if matches!(kind, TypeSpec::Character(Some(n)) if *n > i32::MAX as usize - 1) {
        bail!("CHARACTER length is too large for native storage");
    }
    if let TypeSpec::IntegerKind(name) = kind {
        let value = kind_number(name, constants)?;
        if !matches!(value, 1 | 2 | 4 | 8 | 16) { bail!("unsupported INTEGER kind {value}; supported kinds are 1, 2, 4, 8 and 16"); }
        *kind = TypeSpec::Integer(Some(value as u8));
    }
    if let TypeSpec::Integer(Some(value)) = kind {
        if !matches!(value, 1 | 2 | 4 | 8 | 16) { bail!("unsupported INTEGER kind {value}; supported kinds are 1, 2, 4, 8 and 16"); }
        if *value == 4 { *kind = TypeSpec::Integer(None); }
    }
    if let TypeSpec::Complex(name) = kind {
        let number=kind_number(name,constants)?;
        if !matches!(number,4|8) {bail!("unsupported COMPLEX kind {number}; supported component kinds are 4 and 8");}
        *name=number.to_string();
    }
    if let TypeSpec::RealKind(name) = kind {
        *kind = match kind_number(name, constants)? {
            4 => TypeSpec::Real, 8 => TypeSpec::DoublePrecision,
            other => bail!("unsupported REAL kind {other}; supported kinds are 4 and 8"),
        };
    }
    Ok(())
}

// These inquiries depend only on constants and do not read variable storage.
fn fold_length_inquiries(expr: &mut Expr) -> Result<()> {
    match expr {
        Expr::Call(_, args) | Expr::Index(_, args) => for arg in args { fold_length_inquiries(arg)?; },
        Expr::Bin(_, a, b) => { fold_length_inquiries(a)?; fold_length_inquiries(b)?; }
        Expr::Un(_, a) => fold_length_inquiries(a)?,
        _ => {}
    }
    if let Expr::Call(name, args) = expr {
        let value = match name.as_str() {
            "len" | "len_trim" if args.len() == 1 => {
                let text = character_constant(&args[0])
                    .context("LEN/LEN_TRIM in a CHARACTER length currently requires a character constant")?;
                if !text.is_ascii() { bail!("constant CHARACTER length inquiries currently require ASCII"); }
                if name == "len" { text.len() as i64 } else { text.trim_end_matches(' ').len() as i64 }
            }
            "min" | "max" if args.len() >= 2 => {
                let values = args.iter().map(integer_constant).collect::<Result<Vec<_>>>()?;
                if name == "min" { *values.iter().min().unwrap() } else { *values.iter().max().unwrap() }
            }
            "abs" if args.len() == 1 => integer_constant(&args[0])?.checked_abs().context("integer overflow in CHARACTER length")?,
            "@unary_plus" if args.len() == 1 => integer_constant(&args[0])?,
            _ => return Ok(()),
        };
        *expr = Expr::IntLit(value.to_string());
    }
    Ok(())
}
fn real_literal(value: f64, double: bool) -> Expr {
    let literal = Expr::RealLit(value.to_string());
    if double { Expr::Call("dble".into(), vec![literal]) } else { literal }
}

fn model_type_key(name: &str) -> String { format!("@model_type:{name}") }
fn model_metadata_name(name: &str) -> &str {
    name.strip_prefix("@model_type:").or_else(||name.strip_prefix("@model_procedure:")).or_else(||name.strip_prefix("@parameter_lower:")).or_else(||name.strip_prefix("@character_length:")).unwrap_or(name)
}
fn record_model_type(constants: &mut Constants, name: &str, kind: &TypeSpec, rank: usize) {
    let code = match kind {
        TypeSpec::Integer(bytes) => -i64::from(bytes.unwrap_or(4)),
        TypeSpec::Real => 4,
        TypeSpec::DoublePrecision => 8,
        TypeSpec::Complex(k) => 100+k.parse::<i64>().unwrap_or(4),
        _ => 0,
    };
    constants.insert(model_type_key(name),Expr::Call("@model_type".into(),vec![Expr::IntLit(code.to_string()),Expr::IntLit(rank.to_string())]));
}
fn model_type_lookup(constants: &Constants, name: &str) -> Option<(TypeSpec,usize)> {
    let Expr::Call(_,args)=constants.get(&model_type_key(name))? else {return None};
    let [Expr::IntLit(code),Expr::IntLit(rank)]=args.as_slice() else {return None};
    Some((match code.parse::<i64>().ok()? {
        n if n < 0 => TypeSpec::Integer(Some((-n) as u8)),
        4 => TypeSpec::Real,
        8 => TypeSpec::DoublePrecision,
        104 => TypeSpec::Complex("4".into()),
        108 => TypeSpec::Complex("8".into()),
        _ => TypeSpec::Logical,
    },rank.parse().ok()?))
}
fn conditional_constant<'a>(e:&'a Expr,constants:&Constants)->Result<&'a Expr> {
    if let Expr::Call(n,a)=e {
        if n=="@conditional" {
            for pair in a[..a.len()-1].chunks_exact(2) {
                if logical_constant(&pair[0],constants)? {return conditional_constant(&pair[1],constants);}
            }
            return conditional_constant(a.last().unwrap(),constants);
        }
    }
    Ok(e)
}

pub(crate) fn logical_constant(e: &Expr, constants: &Constants) -> Result<bool> {
    let e=conditional_constant(e,constants)?;
    Ok(match e {
        Expr::Logical(value) => *value,
        Expr::Ident(name) => logical_constant(constants.get(name)
            .context("LOGICAL PARAMETER initializer must be a supported logical constant expression")?, &Constants::new())?,
        Expr::Un(UnOp::Not, value) => !logical_constant(value, constants)?,
        Expr::Bin(op, a, b) if matches!(op, BinOp::And | BinOp::Or | BinOp::Eqv | BinOp::Neqv) => {
            let (a, b) = (logical_constant(a, constants)?, logical_constant(b, constants)?);
            match op { BinOp::And => a && b, BinOp::Or => a || b, BinOp::Eqv => a == b, _ => a != b }
        }
        Expr::Bin(op, a, b) if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) => {
            let ordering = if let (Ok(mut a), Ok(mut b)) = (character_constant(a), character_constant(b)) {
                let size = a.len().max(b.len());
                a.extend(std::iter::repeat_n(' ', size-a.len()));
                b.extend(std::iter::repeat_n(' ', size-b.len()));
                a.cmp(&b)
            } else if let (Ok(a), Ok(b)) = (evaluate(a, constants), evaluate(b, constants)) {
                a.cmp(&b)
            } else {
                real_constant(a)?.partial_cmp(&real_constant(b)?).context("invalid constant comparison")?
            };
            use std::cmp::Ordering::{Less, Equal, Greater};
            match op {
                BinOp::Eq => ordering == Equal, BinOp::Ne => ordering != Equal,
                BinOp::Lt => ordering == Less, BinOp::Le => ordering != Greater,
                BinOp::Gt => ordering == Greater, _ => ordering != Less,
            }
        }
        Expr::Call(name, args) if matches!(name.as_str(), "all" | "any" | "@all_reduction" | "@any_reduction") && matches!(args.len(),1 | 3) => {
            if args.len()==3 && integer_constant(&args[1])?!=1 { bail!("constant logical reduction DIM must be 1 for a rank-one array"); }
            let actual = if let Expr::Ident(n)=&args[0] { constants.get(n).unwrap_or(&args[0]) } else { &args[0] };
            let values = if crate::empty_constructors::kind(actual)?==Some(TypeSpec::Logical) { vec![] }
                else if let Expr::Call(tag, items)=actual {
                    if !matches!(tag.as_str(), "@array_constructor" | "@array_constant") { bail!("constant {name} requires a logical array constructor or PARAMETER array"); }
                    items.iter().map(|e|logical_constant(e,constants)).collect::<Result<Vec<_>>>()?
                } else { bail!("constant {name} requires a logical array constructor or PARAMETER array"); };
            if matches!(name.as_str(),"all" | "@all_reduction") {values.iter().all(|v|*v)} else {values.iter().any(|v|*v)}
        }
        _ => bail!("LOGICAL PARAMETER initializer must be a supported logical constant expression"),
    })
}
pub(crate) fn real_constant(e: &Expr) -> Result<f64> {
    let e=conditional_constant(e,&Constants::new())?;
    fn integral(e: &Expr) -> bool {
        match e { Expr::Call(tag,_) if tag == "@integer_literal" => true, Expr::IntLit(_) => true, Expr::Un(_, a) => integral(a), Expr::Bin(_, a,b) => integral(a) && integral(b), _ => false }
    }
    if integral(e) { return Ok(evaluate(e, &Constants::new())? as f64); }
    let value = match e {
        Expr::RealLit(s) => s.parse()?,
        Expr::IntLit(s) => s.parse::<i64>()? as f64,
        Expr::Un(UnOp::Neg, a) => -real_constant(a)?,
        Expr::Bin(op, a, b) => {
            let (a,b) = (real_constant(a)?, real_constant(b)?);
            match op { BinOp::Add => a+b, BinOp::Sub => a-b, BinOp::Mul => a*b, BinOp::Div => a/b, BinOp::Pow => a.powf(b), _ => bail!("unsupported real constant operator") }
        }
        Expr::Call(name, args) if args.len() == 1 => {
            let value = real_constant(&args[0])?;
            match name.to_ascii_lowercase().as_str() {
                "dble" => value, "real" => (value as f32) as f64,
                "acos" => value.acos(), "asin" => value.asin(), "atan" => value.atan(),
                "cos" => value.cos(), "sin" => value.sin(), "tan" => value.tan(),
                "sqrt" => value.sqrt(), "log" => value.ln(), "exp" => value.exp(),
                "abs" => value.abs(), "ceiling" => value.ceil(), "floor" => value.floor(),
                "int" | "aint" => value.trunc(), "nint" | "anint" => value.round(),
                _ => bail!("unsupported function in real PARAMETER initializer: {name}"),
            }
        }
        // REAL(x, kind) and DBLE-like conversions with a KIND.
        Expr::Call(name, args) if args.len() == 2 && matches!(name.to_ascii_lowercase().as_str(), "real" | "int" | "nint") => {
            let value = real_constant(&args[0])?;
            let kind = integer_constant(&args[1])?;
            match name.to_ascii_lowercase().as_str() {
                "real" if kind == 4 => (value as f32) as f64,
                "real" => value,
                "int" => value.trunc(),
                _ => value.round(),
            }
        }
        Expr::Call(name, args) if args.len() >= 2 && matches!(name.to_ascii_lowercase().as_str(), "max" | "min" | "amax1" | "amin1" | "dmax1" | "dmin1" | "max0" | "min0") => {
            let maximum = name.to_ascii_lowercase().contains("max");
            let mut result = real_constant(&args[0])?;
            for a in &args[1..] {
                let v = real_constant(a)?;
                result = if maximum { result.max(v) } else { result.min(v) };
            }
            result
        }
        _ => bail!("PARAMETER initializer must be a supported constant expression"),
    };
    if !value.is_finite() { bail!("non-finite real PARAMETER initializer"); }
    Ok(value)
}

// Character constants are lowered to literals before either backend sees them.
pub(crate) fn character_constant(expr: &Expr) -> Result<String> {
    let expr=conditional_constant(expr,&Constants::new())?;
    match expr {
        Expr::Str(value) => Ok(value.clone()),
        Expr::Call(name,args) if name=="new_line" && args.len()==1 => {character_constant(&args[0])?;Ok("\n".into())},
        Expr::Bin(BinOp::Concat, left, right) => {
            Ok(character_constant(left)? + &character_constant(right)?)
        }
        Expr::Call(name, args) if matches!(name.as_str(), "char" | "achar") && args.len() == 1 => {
            let code = integer_constant(&args[0])?;
            // Native strings currently use NUL-terminated UTF-8 storage.
            if !(1..=127).contains(&code) {
                bail!("constant {name} currently supports character codes 1 through 127");
            }
            Ok(char::from(code as u8).to_string())
        }
        _ => bail!("CHARACTER PARAMETER initializer must be a supported character constant expression"),
    }
}
fn character_parameter(expr: &Expr, kind: &TypeSpec) -> Result<Expr> {
    let mut value = character_constant(expr)?;
    if !value.is_ascii() || value.contains('\0') {
        bail!("CHARACTER PARAMETER currently requires non-NUL ASCII characters");
    }
    let length = match kind {
        TypeSpec::CharacterAssumed => value.len(),
        TypeSpec::Character(length) => length.unwrap_or(1),
        _ => unreachable!(),
    };
    value.truncate(length);
    value.try_reserve(length.saturating_sub(value.len()))
        .context("CHARACTER PARAMETER length is too large")?;
    while value.len() < length { value.push(' '); }
    Ok(Expr::Str(value))
}

pub fn resolve(program: Program) -> Result<Program> {
    let program = crate::statement_labels::resolve(program)?;
    let program = crate::named_loops::resolve(program)?;
    crate::purity::validate(&program)?;
    resolve_constants(program)
}

// Operator selection needs kinds before calls and derived components have
// been lowered. Purity is checked by resolve() after that lowering.
pub(crate) fn resolve_constants(mut program: Program) -> Result<Program> {
    fn mark_saved(body:&mut [Stmt],procedure:bool,serial:&mut usize) {
        for statement in body {match statement {
            Stmt::Function {name,body,..}=>{
                let result=body.last().and_then(|s|if let Stmt::Return(Some(Expr::Ident(n)))=s {Some(n.clone())}else{None}).unwrap_or_else(||name.clone());
                for stmt in body.iter_mut() {
                    if let Stmt::InitializedDecl {attributes,entities,..}=stmt {
                        if entities.iter().any(|(n,_)|n==&result) && !attributes.iter().any(|a|a=="parameter") {attributes.push("@save_result".into());}
                    }
                }
                mark_saved(body,true,serial);
            },
            Stmt::Subroutine {body,..}=>mark_saved(body,true,serial),
            Stmt::Module {body,..}=>mark_saved(body,false,serial),
            Stmt::Block {body}=>mark_saved(body,procedure,serial),
            Stmt::InitializedDecl {attributes,..} if procedure && !attributes.iter().any(|a|a=="parameter") && !attributes.iter().any(|a|a.starts_with("@save_key:"))=>{
                *serial+=1;attributes.push(format!("@save_key:{serial}"));
            },_=>{},
        }}
    }
    // The key belongs to the declaration, rather than an expanded call site.
    fn reserve(body:&[Stmt],serial:&mut usize) {
        for stmt in body {match stmt {
            Stmt::CallSub{name,args} if name=="@saved_bind"=>{if let Some(Expr::Str(key))=args.get(1) {if let Some(n)=key.split('_').next().and_then(|s|s.parse::<usize>().ok()) {*serial=(*serial).max(n);}}},
            Stmt::Function{body,..}|Stmt::Subroutine{body,..}|Stmt::Module{body,..}|Stmt::Block{body}=>reserve(body,serial),_=>{},
        }}
    }
    let mut serial=0;reserve(&program.body,&mut serial);mark_saved(&mut program.body,false,&mut serial);
    let mut modules = Modules::new();
    for stmt in &mut program.body {
        if let Stmt::Module { name, body } = stmt {
            let default_public = !body.iter().any(|s| matches!(s, Stmt::Accessibility { public: false, names } if names.is_empty()));
            let mut exports = HashMap::new();
            for s in body.iter() {
                if let Stmt::InitializedDecl { attributes, entities, .. } = s {
                    for (n, _) in entities {
                        exports.insert(n.to_ascii_lowercase(), if attributes.iter().any(|a| a.eq_ignore_ascii_case("public")) { true }
                            else if attributes.iter().any(|a| a.eq_ignore_ascii_case("private")) { false } else { default_public });
                    }
                }
            }
            for s in body.iter() {
                if let Stmt::Accessibility { public, names } = s { for n in names { exports.insert(n.to_ascii_lowercase(), *public); } }
            }
            let constants = resolve_scope(body, &Constants::new(), &[], &modules, false, false, false)?;
            modules.insert(name.to_ascii_lowercase(), constants.into_iter().filter(|(n, _)| exports.get(model_metadata_name(n)).copied().unwrap_or(default_public)).collect());
        }
    }
    resolve_scope(&mut program.body, &Constants::new(), &[], &modules, false, true, false)?;
    Ok(program)
}

fn evaluate(expr: &Expr, constants: &Constants) -> Result<i64> {
    let expr=conditional_constant(expr,constants)?;
    let value = match expr {
        Expr::IntLit(s) => s.parse().ok(),
        Expr::Call(tag,args) if tag == "@integer_literal" && args.len() == 2 => return evaluate(&args[0],constants),
        Expr::Ident(name) => {
            return constants
                .get(&name.to_ascii_lowercase())
                .with_context(|| {
                    format!("`{name}` is not a previously defined integer PARAMETER")
                }).and_then(|e| evaluate(e, &Constants::new()));
        }
        // INT of an INTEGER constant converts it to the result kind.
        Expr::Call(name,args) if name=="int" && !args.is_empty()
            && matches!(crate::inquiries::model_type(&args[0],&|_|None),Ok(Some(TypeSpec::Integer(_)))) => {
            let TypeSpec::Integer(Some(bytes))=crate::intrinsics::rounding_kind(name,args)? else {unreachable!()};
            let value=evaluate(&args[0],constants)?;
            let limit=1i128<<(u32::from(bytes)*8-1);
            if i128::from(value) < -limit || i128::from(value) >= limit {bail!("INT result is outside INTEGER range");}
            Some(value)
        }
        // NINT and INT of a REAL constant, as `nint(days_in_year)`, round as
        // FLOOR and CEILING do; INT of an INTEGER is handled below.
        Expr::Call(name,args) if matches!(name.as_str(),"floor"|"ceiling")
            || matches!(name.as_str(),"nint"|"int") && !args.is_empty()
                && matches!(crate::inquiries::model_type(&args[0],&|_|None),Ok(Some(TypeSpec::Real|TypeSpec::DoublePrecision))) => {
            let kind=crate::intrinsics::rounding_kind(name,args)?;
            let TypeSpec::Integer(Some(bytes))=kind else {unreachable!()};
            let input=crate::inquiries::model_type(&args[0],&|_|None)?;
            if input.as_ref().is_some_and(|k|!matches!(k,TypeSpec::Real|TypeSpec::DoublePrecision)) {bail!("{} A must be REAL or DOUBLE PRECISION",name.to_ascii_uppercase());}
            let value=real_constant(&args[0])?;
            let value=if input==Some(TypeSpec::Real) {(value as f32) as f64} else {value};
            // NINT rounds halves away from zero, as `f64::round` does.
            let rounded=match name.as_str() {"floor"=>value.floor(),"ceiling"=>value.ceil(),"nint"=>value.round(),_=>value.trunc()};
            let limit=2.0f64.powi(i32::from(bytes)*8-1);
            if !rounded.is_finite() || rounded < -limit || rounded >= limit {bail!("{} result is outside INTEGER range",name.to_ascii_uppercase());}
            Some(rounded as i64)
        }
        Expr::Call(name,args) if matches!(name.as_str(),"index"|"scan"|"verify") => {
            if !(2..=4).contains(&args.len()) {bail!("{name} requires two CHARACTER arguments and optional BACK and KIND");}
            let text=character_constant(&args[0])?;let pattern=character_constant(&args[1])?;
            let back=args.get(2).filter(|e|!crate::named_arguments::omitted(e)).map(|e|logical_constant(e,constants)).transpose()?.unwrap_or(false);
            let kind=args.get(3).filter(|e|!crate::named_arguments::omitted(e)).map(|e|evaluate(e,constants)).transpose()?.unwrap_or(4);
            if !matches!(kind,1|2|4|8) {bail!("{name} KIND must be a supported INTEGER kind (1, 2, 4 or 8)");}
            let position=if name=="index" {if back {text.rfind(&pattern)} else {text.find(&pattern)}}
                else {
                    let matched=|(i,c):(usize,u8)|if pattern.as_bytes().contains(&c)==(name=="scan") {Some(i)} else {None};
                    if back {text.bytes().enumerate().rev().find_map(matched)} else {text.bytes().enumerate().find_map(matched)}
                };
            Some(position.map_or(0,|i|i as i64+1))
        }
        Expr::Call(name,args) if name=="kind" && args.len()==1 => {
            // Declared variables are recorded as @model_type(code, rank).
            let lookup=|n:&str|constants.get(&model_type_key(&n.to_ascii_lowercase())).or_else(||constants.get(&model_type_key(n))).and_then(|e|match e {
                Expr::Call(tag,a) if tag=="@model_type" => {
                    let code=integer_constant(&a[0]).ok()?;
                    let kind=match code {c if c<0=>TypeSpec::Integer(Some((-c) as u8)),4=>TypeSpec::Real,8=>TypeSpec::DoublePrecision,c if c>100=>TypeSpec::Complex((c-100).to_string()),_=>return None};
                    Some((kind,integer_constant(&a[1]).ok()? as usize))
                }
                _ => None,
            });
            // A literal with a D exponent is DOUBLE PRECISION.
            let kind=if matches!(&args[0],Expr::RealLit(text) if text.contains(['d','D'])) {TypeSpec::DoublePrecision}
                else {crate::inquiries::model_type(&args[0],&lookup)?.context("KIND argument must be a constant or a variable of known type")?};
            Some(match kind {
                TypeSpec::Integer(bytes) => i64::from(bytes.unwrap_or(4)),
                TypeSpec::Real | TypeSpec::Logical => 4,
                TypeSpec::DoublePrecision => 8,
                TypeSpec::Complex(k) => k.parse().unwrap_or(4),
                TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => 1,
                _ => bail!("KIND argument has an unsupported type"),
            })
        }
        // CHARACTER storage is ASCII, which is also the default kind.
        Expr::Call(name,args) if name=="selected_char_kind" && args.len()==1 => {
            Some(if matches!(character_constant(&args[0])?.trim().to_ascii_lowercase().as_str(),"ascii"|"default") {1} else {-1})
        }
        Expr::Call(name,args) if matches!(name.as_str(),"abs"|"iabs") && args.len()==1 => evaluate(&args[0],constants)?.checked_abs(),
        Expr::Call(name,args) if matches!(name.as_str(),"mod"|"modulo"|"sign"|"dim") && args.len()==2 => {
            let (a,b)=(evaluate(&args[0],constants)?,evaluate(&args[1],constants)?);
            match name.as_str() {
                "mod"|"modulo" if b==0 => bail!("{} P must not be zero",name.to_ascii_uppercase()),
                "mod" => a.checked_rem(b),
                "modulo" => a.checked_rem(b).map(|r|if r!=0 && (r<0)!=(b<0) {r+b} else {r}),
                "sign" => a.checked_abs().map(|m|if b<0 {-m} else {m}),
                _ => Some((a-b).max(0)),
            }
        }
        Expr::Call(name,args) if matches!(name.as_str(),"max"|"min") && !args.is_empty() => {
            let values=args.iter().map(|a|evaluate(a,constants)).collect::<Result<Vec<_>>>()?;
            if name=="max" {values.into_iter().max()} else {values.into_iter().min()}
        }
        Expr::Un(UnOp::Neg, e) => evaluate(e, constants)?.checked_neg(),
        Expr::Bin(op, left, right) => {
            let a = evaluate(left, constants)?;
            let b = evaluate(right, constants)?;
            match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div => {
                    if b == 0 {
                        bail!("division by zero in PARAMETER initializer");
                    }
                    a.checked_div(b)
                }
                BinOp::Pow if b < 0 => {
                    if a == 0 {
                        bail!("zero raised to a negative power in PARAMETER initializer");
                    }
                    Some(match a {
                        1 => 1,
                        -1 => {
                            if b % 2 == 0 {
                                1
                            } else {
                                -1
                            }
                        }
                        _ => 0,
                    })
                }
                BinOp::Pow => {
                    let exponent = u32::try_from(b).context("PARAMETER exponent is too large")?;
                    a.checked_pow(exponent)
                }
                _ => bail!("unsupported operator in integer PARAMETER initializer"),
            }
        }
        _ => bail!("PARAMETER initializer must be a supported integer constant expression"),
    };
    value.context("integer overflow in PARAMETER initializer")
}

pub(crate) fn integer_constant(expr: &Expr) -> Result<i64> {
    evaluate(expr, &Constants::new())
}

/// Fold implemented intrinsic expressions before extracting an INTEGER value.
/// Callers must have substituted visible constants and excluded user procedures.
pub(crate) fn folded_integer_constant(expr: &Expr) -> Result<i64> {
    let mut expr = expr.clone();
    rewrite_expr(&mut expr, &Constants::new())?;
    integer_constant(&expr)
}

fn separate_parameters(body: &mut Vec<Stmt>, arguments: &[String]) -> Result<()> {
    if !body.iter().any(|s| matches!(s,Stmt::InitializedDecl { attributes, .. } if attributes.iter().any(|a| a == "@separate_parameter"))) { return Ok(()); }
    let mut declared: HashMap<String,TypeSpec> = HashMap::new();
    let mut converted = HashSet::new();
    let mut output = Vec::new();
    let mut statement_location = None;
    for stmt in std::mem::take(body) {
        if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location = Some(p); }
        (|| -> anyhow::Result<()> {
        match stmt {
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "@separate_parameter") => {
                for (name, value) in entities {
                    if arguments.contains(&name) { bail!("dummy argument `{name}` cannot be a PARAMETER"); }
                    if !converted.insert(name.clone()) { bail!("duplicate PARAMETER definition `{name}`"); }
                    let kind = declared.get(&name).with_context(|| format!("separate PARAMETER `{name}` currently requires a preceding scalar type declaration"))?.clone();
                    output.push(Stmt::InitializedDecl { kind, attributes:vec!["parameter".into()], entities:vec![(name,value)] });
                }
            }
            Stmt::VarDecl { ref kind, ref names } => {
                for name in names {
                    if declared.insert(name.clone(),kind.clone()).is_some() { bail!("duplicate type declaration `{name}`"); }
                }
                output.push(stmt);
            }
            other => output.push(other),
        }
            Ok(())
        })().map_err(|e| crate::source_diagnostics::at(e, statement_location))?;
    }
    for stmt in &mut output {
        if let Stmt::VarDecl { names, .. } = stmt { names.retain(|n| !converted.contains(n)); }
    }
    output.retain(|s| !matches!(s,Stmt::VarDecl { names, .. } if names.is_empty()));
    *body = output;
    Ok(())
}

/// Replace KIND(variable) and KIND(z%re) with the kind number of the declared
/// type, so that KIND= arguments such as CMPLX(x, y, KIND=KIND(z)) are
/// constants for the lowering passes that precede PARAMETER resolution.
/// Named kinds are resolved from the INTEGER PARAMETERs declared in scope.
pub(crate) fn fold_kind_inquiries(program: &mut Program) {
    fn kind_of(kind: &TypeSpec, constants: &Constants) -> Option<i64> {
        match kind {
            TypeSpec::Referenced {..} => None,
            TypeSpec::Integer(bytes) => Some(i64::from(bytes.unwrap_or(4))),
            TypeSpec::Real | TypeSpec::Logical => Some(4),
            TypeSpec::DoublePrecision => Some(8),
            TypeSpec::IntegerKind(k) | TypeSpec::RealKind(k) | TypeSpec::LogicalKind(k) | TypeSpec::Complex(k) => kind_number(k, constants).ok(),
            TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed => Some(1),
        }
    }
    fn fold(e: &mut Expr, types: &HashMap<String, TypeSpec>, constants: &Constants) {
        if let Expr::Call(name, args) = e {
            if name == "kind" && args.len() == 1 {
                if let Expr::Ident(v) = &args[0] {
                    let v = v.to_ascii_lowercase();
                    let (base, component) = match v.rsplit_once('%') {
                        Some((base, part @ ("re" | "im"))) => (base.to_owned(), Some(part)),
                        _ => (v.clone(), None),
                    };
                    let kind = types.get(&base).filter(|t| component.is_none() || matches!(t, TypeSpec::Complex(_)));
                    if let Some(n) = kind.and_then(|t| kind_of(t, constants)) {
                        *e = Expr::IntLit(n.to_string());
                        return;
                    }
                }
            }
        }
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => for a in args { fold(a, types, constants); },
            Expr::Bin(_, a, b) => { fold(a, types, constants); fold(b, types, constants); }
            Expr::Un(_, a) => fold(a, types, constants),
            _ => {}
        }
    }
    fn scope(body: &mut Vec<Stmt>, inherited_types: &HashMap<String, TypeSpec>, inherited: &Constants) {
        let mut types = inherited_types.clone();
        let mut constants = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => for n in names { types.insert(n.to_ascii_lowercase(), kind.clone()); },
                Stmt::ArrayDecl { kind, name, .. } => { types.insert(name.to_ascii_lowercase(), kind.clone()); }
                Stmt::InitializedDecl { kind, attributes, entities } => {
                    let parameter = attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter"));
                    for (n, value) in entities {
                        types.insert(n.to_ascii_lowercase(), kind.clone());
                        if parameter && matches!(kind, TypeSpec::Integer(_)) {
                            if let Some(mut value) = value.clone() {
                                fold(&mut value, &types, &constants);
                                if let Ok(v) = evaluate(&value, &constants) { constants.insert(n.to_ascii_lowercase(), Expr::IntLit(v.to_string())); }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for s in body.iter_mut() {
            match s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => scope(body, &types, &constants),
                Stmt::Module { body, .. } => scope(body, &HashMap::new(), &Constants::new()),
                s => {
                    for e in crate::procedure_arguments::expressions(s) { fold(e, &types, &constants); }
                    for child in crate::procedure_arguments::children(s) {
                        let mut wrapper = std::mem::take(child);
                        scope_statements(&mut wrapper, &types, &constants);
                        *child = wrapper;
                    }
                }
            }
        }
    }
    // Branch and loop bodies share their scope's declarations.
    fn scope_statements(body: &mut Vec<Stmt>, types: &HashMap<String, TypeSpec>, constants: &Constants) {
        for s in body.iter_mut() {
            match s {
                Stmt::Block { body } => scope(body, types, constants),
                s => {
                    for e in crate::procedure_arguments::expressions(s) { fold(e, types, constants); }
                    for child in crate::procedure_arguments::children(s) { scope_statements(child, types, constants); }
                }
            }
        }
    }
    scope(&mut program.body, &HashMap::new(), &Constants::new());
}

fn substitute(e: &mut Expr, index: &str, value: i64) {
    match e {
        Expr::Ident(n) if n == index => *e = Expr::IntLit(value.to_string()),
        Expr::Call(_, args) | Expr::Index(_, args) => for a in args { substitute(a, index, value); },
        Expr::Bin(_, a, b) => { substitute(a, index, value); substitute(b, index, value); }
        Expr::Un(_, a) => substitute(a, index, value),
        _ => {}
    }
}

// The elements of a constant array constructor in array element order, with
// nested constructors and constant-bound implied-DO loops expanded.
fn constructor_elements(e: &Expr, out: &mut Vec<Expr>) -> Result<()> {
    match e {
        Expr::Call(tag, items) if tag == "@array_constructor" => for item in items { constructor_elements(item, out)?; },
        // `r*c` in a DATA value list.
        Expr::Call(tag, args) if tag == crate::data_statements::REPEAT && args.len() == 2 => {
            let count = integer_constant(&args[0])?;
            if count < 0 { bail!("a DATA repeat count must not be negative"); }
            if count > 1 << 24 { bail!("array initializer is too large"); }
            for _ in 0..count { constructor_elements(&args[1], out)?; }
        }
        Expr::Call(tag, args) if tag == "@output_implied_do" && args.len() >= 4 => {
            let Expr::Ident(index) = &args[0] else { bail!("invalid implied-DO index") };
            let (start, end, stride) = (integer_constant(&args[1])?, integer_constant(&args[2])?, integer_constant(&args[3])?);
            if stride == 0 { bail!("implied-DO stride must not be zero"); }
            let mut i = start;
            while if stride > 0 { i <= end } else { i >= end } {
                if out.len() > 1 << 24 { bail!("array initializer is too large"); }
                for item in &args[4..] {
                    let mut item = item.clone();
                    substitute(&mut item, index, i);
                    constructor_elements(&item, out)?;
                }
                i += stride;
            }
        }
        other => out.push(other.clone()),
    }
    Ok(())
}

// The extents of a declared array, from `n` or `lower:upper` bounds.
fn declared_extents(dims: &[Expr], constants: &Constants) -> Result<Vec<i64>> {
    dims.iter().map(|d| {
        let mut d = d.clone();
        rewrite_expr(&mut d, constants)?;
        Ok(match &d {
            Expr::Call(tag, b) if tag == "@declared_bounds" => integer_constant(&b[1])? - integer_constant(&b[0])? + 1,
            d => integer_constant(d)?,
        })
    }).collect()
}

/// Fold an array initializer built from constructors, implied-DO loops and
/// RESHAPE into one flat constructor in array element order. `shapes` gives
/// the extents of declared arrays for SHAPE(array). Other forms are returned
/// unchanged.
fn fold_array_initializer(e: &Expr, shapes: &HashMap<String, Vec<i64>>) -> Result<Expr> {
    let flat = |items: Vec<Expr>| Expr::Call("@array_constructor".into(), items);
    match e {
        Expr::Call(tag, _) if tag == "@array_constructor" => {
            let mut out = vec![];
            constructor_elements(e, &mut out)?;
            Ok(flat(out))
        }
        Expr::Call(name, args) if name == "reshape" && !shapes.contains_key(name) => {
            let mut slots: [Option<&Expr>; 4] = [None; 4];
            for (position, arg) in args.iter().enumerate() {
                match arg {
                    Expr::Call(tag, v) if tag.starts_with("@keyword:") => {
                        let slot = match &tag[9..] { "source" => 0, "shape" => 1, "pad" => 2, "order" => 3, k => bail!("RESHAPE: unknown keyword `{k}`") };
                        slots[slot] = v.first();
                    }
                    _ if position < 4 => slots[position] = Some(arg),
                    _ => bail!("RESHAPE expects SOURCE, SHAPE, PAD and ORDER"),
                }
            }
            let elements = |e: Option<&Expr>| -> Result<Option<Vec<Expr>>> {
                let Some(e) = e.filter(|e| !matches!(e, Expr::Call(tag, _) if tag.starts_with("@no_"))) else { return Ok(None) };
                match fold_array_initializer(e, shapes)? {
                    Expr::Call(tag, items) if tag == "@array_constructor" => Ok(Some(items)),
                    _ => bail!("RESHAPE arguments in an initializer must be constant arrays"),
                }
            };
            let source = elements(slots[0])?.context("RESHAPE requires SOURCE")?;
            let extents: Vec<i64> = match slots[1] {
                Some(Expr::Call(n, a)) if n == "shape" && a.len() == 1 && matches!(&a[0], Expr::Ident(v) if shapes.contains_key(v)) => {
                    let Expr::Ident(v) = &a[0] else { unreachable!() };
                    shapes[v].clone()
                }
                shape => elements(shape)?.context("RESHAPE requires SHAPE")?.iter().map(integer_constant).collect::<Result<_>>()?,
            };
            if extents.iter().any(|n| *n < 0) { bail!("RESHAPE SHAPE must not have negative extents"); }
            let pad = elements(slots[2])?.unwrap_or_default();
            let order: Vec<usize> = match elements(slots[3])? {
                Some(order) => order.iter().map(|e| Ok(integer_constant(e)? as usize)).collect::<Result<_>>()?,
                None => (1..=extents.len()).collect(),
            };
            let mut sorted = order.clone();
            sorted.sort_unstable();
            if sorted != (1..=extents.len()).collect::<Vec<_>>() { bail!("RESHAPE ORDER must be a permutation of 1..{}", extents.len()); }
            let count = extents.iter().try_fold(1usize, |n, e| n.checked_mul(*e as usize)).context("RESHAPE result is too large")?;
            if count > source.len() && pad.is_empty() { bail!("RESHAPE SOURCE has too few elements and no PAD"); }
            // Element k of SOURCE, then PAD repeated, fills the result with the
            // subscripts varying in ORDER; store each at its column-major offset.
            let mut result = vec![Expr::IntLit("0".into()); count];
            let mut subscript = vec![0i64; extents.len()];
            for k in 0..count {
                let value = if k < source.len() { source[k].clone() } else { pad[(k - source.len()) % pad.len()].clone() };
                let mut offset = 0i64;
                for axis in (0..extents.len()).rev() { offset = offset * extents[axis] + subscript[axis]; }
                result[offset as usize] = value;
                for &axis in &order {
                    let axis = axis - 1;
                    subscript[axis] += 1;
                    if subscript[axis] < extents[axis] { break; }
                    subscript[axis] = 0;
                }
            }
            Ok(flat(result))
        }
        other => Ok(other.clone()),
    }
}

// Resolve declaration initializers to constants before choosing automatic or
// persistent storage. Procedure declaration initialization implies SAVE.
pub(crate) fn initial_value(kind: &TypeSpec, value: &Expr) -> Result<Expr> {
    let value=conditional_constant(value,&Constants::new())?;
    if let Some(source)=crate::empty_constructors::kind(value)? {
        let numeric=|kind:&TypeSpec|matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision);
        if !(numeric(kind)&&numeric(&source)
            || matches!((kind,&source),(TypeSpec::Logical,TypeSpec::Logical)|(TypeSpec::Character(_),TypeSpec::Character(_)))) {
            bail!("typed array constructor type is incompatible with declaration");
        }
        return Ok(Expr::Call("@array_constructor".into(),vec![]));
    }
    if let Expr::Call(tag,items) = value {
        if tag == "@array_constructor" {
            return Ok(Expr::Call(tag.clone(),items.iter().map(|e| initial_value(kind,e)).collect::<Result<_>>()?));
        }
    }
    match kind {
        TypeSpec::Complex(k) => crate::complex::constant(value,k),
        TypeSpec::Integer(bytes) => typed_integer(integer_constant(value)
            .context("INTEGER declaration initializer must be a supported integer constant expression")?,bytes.unwrap_or(4)),
        TypeSpec::Real | TypeSpec::DoublePrecision => {
            let value = real_constant(value).context("REAL declaration initializer must be a supported constant expression")?;
            let double = matches!(kind,TypeSpec::DoublePrecision);
            let value = if double { value } else { value as f32 as f64 };
            if !value.is_finite() { bail!("declaration initializer exceeds the REAL kind's range"); }
            Ok(real_literal(value,double))
        }
        TypeSpec::Character(_) => character_parameter(value,kind),
        TypeSpec::Logical => Ok(Expr::Logical(logical_constant(value, &Constants::new())?)),
        _ => bail!("unsupported declaration initialization type"),
    }
}

fn resolve_scope(body: &mut Vec<Stmt>, inherited: &Constants, arguments: &[String], modules: &Modules, assumed_dummies: bool, main_scope: bool, template: bool) -> Result<Constants> {
    separate_parameters(body, arguments)?;
    validate_constructor_indices(body)?;
    let mut constants = inherited.clone();
    for stmt in body.iter() {
        if let Stmt::Use { module, only, .. } = stmt {
            if let Some(imports) = modules.get(&module.to_ascii_lowercase()) {
                constants.extend(imports.iter().filter(|(name, _)| only.as_ref().is_none_or(|names| names.iter().any(|n| { let entity = model_metadata_name(name); n == entity || entity.strip_prefix(n.as_str()).is_some_and(|tail| tail.starts_with('%')) })))
                    .map(|(n, value)| (n.clone(), value.clone())));
            }
        }
    }
    let array_dimensions: HashMap<String,Vec<Expr>> = body.iter().filter_map(|s|if let Stmt::ArrayDecl{name,dims,..}=s {Some((name.clone(),dims.clone()))} else {None}).collect();
    let arrays: HashSet<String> = body.iter().filter_map(|s| if let Stmt::ArrayDecl { name, .. } = s { Some(name.to_ascii_lowercase()) } else { None }).collect();
    let mut array_values = HashMap::new();
    let mut variable_values = HashMap::new();
    let mut saved_keys = HashMap::new();
    let mut saved_undefined = HashSet::new();
    let mut locals: HashSet<String> = arguments.iter().map(|s| s.to_ascii_lowercase()).collect();
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { names, .. } => {
                locals.extend(names.iter().map(|s| s.to_ascii_lowercase()))
            }
            Stmt::ArrayDecl { name, .. } => {
                locals.insert(name.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    for name in &locals {
        constants.remove(name);
        constants.remove(&format!("@character_length:{name}"));
        constants.remove(&model_type_key(name));
    }
    // Local parameter names also hide host constants, even in their initializers.
    for stmt in body.iter() {
        if let Stmt::InitializedDecl { entities, .. } = stmt {
            for (name, _) in entities {
                constants.remove(&name.to_ascii_lowercase());
                constants.remove(&model_type_key(name));
            }
        }
    }
    for stmt in body.iter_mut() {
        if let Stmt::Function {name,..} | Stmt::Subroutine {name,..} | Stmt::GenericInterface {name,..} = stmt {
            constants.insert(format!("@model_procedure:{name}"),Expr::Logical(true));
        }
    }
    let mut declaration_offset=None;
    for stmt in body.iter_mut() {
        if let Stmt::SourceLocation(offset)=stmt {declaration_offset=Some(*offset);}
        (|| -> Result<()> {
        // Keep type information separately from constant values. Numeric model
        // inquiries can appear in PARAMETER initializers and must not read X.
        let (kind,names,rank) = match &*stmt {
            Stmt::VarDecl {kind,names} => (Some(kind),names.clone(),0),
            Stmt::ArrayDecl {kind,name,dims} => (Some(kind),vec![name.clone()],dims.len()),
            _ => (None,vec![],0),
        };
        if let Some(kind) = kind {
            let mut kind=kind.clone();
            if matches!(kind,TypeSpec::IntegerKind(_) | TypeSpec::RealKind(_) | TypeSpec::LogicalKind(_) | TypeSpec::Complex(_)) { resolve_kind(&mut kind,&constants)?; }
            for name in names { record_model_type(&mut constants,&name,&kind,rank); }
        }
        if let Stmt::InitializedDecl {
            kind,
            attributes,
            entities,
        } = stmt
        {
            if !attributes
                .iter()
                .any(|a| a.eq_ignore_ascii_case("parameter"))
            {
                let save_key=attributes.iter().find_map(|a|a.strip_prefix("@save_key:").map(str::to_owned));
                if attributes.iter().any(|a|a=="@save_result") {bail!("a function result cannot have declaration initialization or SAVE storage");}
                if save_key.is_some() && attributes.iter().any(|a|a=="target") {bail!("TARGET persistent locals are not yet supported");}
                if !main_scope && save_key.is_none() && !attributes.iter().any(|a|a=="@module_initializer") { bail!("declaration initialization in module storage or main-program BLOCK scopes is not yet supported"); }
                if attributes.iter().any(|a| !a.eq_ignore_ascii_case("save") && !a.eq_ignore_ascii_case("target") && !a.starts_with("@save_key:") && !matches!(a.as_str(),"@module_initializer"|"public"|"private")) {
                    bail!("unsupported attribute on initialized variable");
                }
                resolve_kind(kind,&constants)?;
                if save_key.is_some() && !matches!(kind,TypeSpec::Integer(None|Some(1|2|4|8))|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical|TypeSpec::Character(_)) {
                    bail!("persistent initialized locals currently require numeric, LOGICAL or fixed-length CHARACTER scalars");
                }
                let mut scalars = vec![];
                for (name,initializer) in entities.iter() {
                    if let Some(id)=&save_key {
                        if arguments.contains(name) {bail!("a dummy argument cannot have a saved initializer");}
                        saved_keys.insert(name.clone(),format!("{id}_{name}"));
                    }
                    if !arrays.contains(name) { record_model_type(&mut constants,name,kind,0); }
                    let key = name.to_ascii_lowercase();
                    if !locals.insert(key.clone()) && !arrays.contains(&key) { bail!("duplicate declaration of initialized variable `{name}`"); }
                    let mut value = if let Some(value) = initializer {
                        value.clone()
                    } else if save_key.is_some() {
                        // Static storage needs bytes, but SAVE alone does not
                        // define a Fortran value. Preserve that distinction for
                        // the initialization checks, including across calls.
                        saved_undefined.insert(name.clone());
                        match kind {
                            TypeSpec::Character(_) => Expr::Str(String::new()),
                            TypeSpec::Logical => Expr::Logical(false),
                            _ => Expr::IntLit("0".into()),
                        }
                    } else {
                        bail!("initialized declaration `{name}` requires a value");
                    };
                    rewrite_expr(&mut value,&constants)?;
                    if arrays.contains(&key) {
                        let shapes=array_dimensions.iter().filter_map(|(n,d)|Some((n.clone(),declared_extents(d,&constants).ok()?))).collect();
                        value=fold_array_initializer(&value,&shapes)?;
                        rewrite_expr(&mut value,&constants)?;
                    }
                    if !arrays.contains(&key) && matches!(&value,Expr::Call(tag,_) if tag == "@array_constructor" || tag == "@typed_constructor") {
                        bail!("scalar declaration initializer cannot be an array");
                    }
                    let mut value = initial_value(kind,&value).with_context(|| format!("invalid declaration initializer for `{name}`"))?;
                    // A folded multidimensional initializer is in array element order.
                    if let (Some(dims),Expr::Call(tag,items))=(array_dimensions.get(&key),&value) {
                        if dims.len()>1 && tag=="@array_constructor" && items.len()>1 {
                            let extents=declared_extents(dims,&constants)?;
                            let shape=Expr::Call("@array_constructor".into(),extents.iter().map(|n|Expr::IntLit(n.to_string())).collect());
                            value=Expr::Call("reshape".into(),vec![value,shape,Expr::Call("@no_pad".into(),vec![]),Expr::Call("@no_order".into(),vec![])]);
                        }
                    }
                    if variable_values.insert(key.clone(),value).is_some() { bail!("duplicate initialization of `{name}`"); }
                    if !arrays.contains(&key) { scalars.push(name.clone()); }
                }
                *stmt = Stmt::VarDecl {kind:kind.clone(),names:scalars};
                return Ok(());
            }
            if !matches!(kind, TypeSpec::CharacterAssumed) { resolve_kind(kind, &constants)?; }
            if !matches!(kind, TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)) | TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::Complex(_) | TypeSpec::Logical | TypeSpec::Character(_) | TypeSpec::CharacterAssumed) {
                bail!("PARAMETER declarations require a supported INTEGER, REAL, COMPLEX, LOGICAL, or CHARACTER type");
            }
            if attributes
                .iter()
                .any(|a| !matches!(a.to_ascii_lowercase().as_str(), "parameter" | "public" | "private"))
            {
                bail!("additional attributes on PARAMETER declarations are not yet supported");
            }
            for (name, initializer) in entities {
                if !arrays.contains(name) { record_model_type(&mut constants,name,kind,0); }
                let key = name.to_ascii_lowercase();
                if !locals.insert(key.clone()) && !arrays.contains(&key) {
                    bail!("duplicate declaration of PARAMETER `{name}`");
                }
                let initializer = initializer
                    .as_ref()
                    .with_context(|| format!("PARAMETER `{name}` requires an initializer"))?;
                let mut initializer = initializer.clone();
                rewrite_expr(&mut initializer, &constants)?;
                if arrays.contains(&key) {
                    if let Expr::Ident(source) = &initializer {
                        if let Some(Expr::Call(tag,items)) = constants.get(source) {
                            if tag == "@array_constant" { initializer=Expr::Call("@array_constructor".into(),items.clone()); }
                        }
                    }
                    let initializer=if crate::empty_constructors::kind(&initializer)?.is_some() {initial_value(kind,&initializer)?} else {initializer};
                    let Expr::Call(tag, items) = initializer else { bail!("array PARAMETER requires an array constructor"); };
                    if tag != "@array_constructor" { bail!("array PARAMETER requires an array constructor"); }
                    let mut dims=array_dimensions[&key].clone();
                    if dims.len()!=1 {bail!("array PARAMETER `{name}` currently requires rank one");}
                    rewrite_expr(&mut dims[0],&constants)?;
                    let lower=match &dims[0] {
                        Expr::Call(tag,bounds) if tag=="@declared_bounds"=>integer_constant(&bounds[0])?,
                        _=>1,
                    };
                    constants.insert(format!("@parameter_lower:{name}"),Expr::IntLit(lower.to_string()));
                    if matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed) {
                        let lengths=items.iter().map(|e|character_constant(e).map(|s|s.len())).collect::<Result<Vec<_>>>()?;
                        if lengths.windows(2).any(|pair|pair[0]!=pair[1]) {bail!("untyped CHARACTER array constructor elements must have equal lengths");}
                    }
                    let element_kind = if matches!(kind,TypeSpec::CharacterAssumed) {
                        let lengths=items.iter().map(|e|character_constant(e).map(|s|s.len())).collect::<Result<Vec<_>>>()?;
                        let length=lengths.first().copied().context("assumed CHARACTER length cannot be inferred from an empty array constructor")?;
                        if lengths.iter().any(|n|*n!=length) {bail!("untyped CHARACTER array constructor elements must have equal lengths");}
                        TypeSpec::Character(Some(length))
                    } else {kind.clone()};
                    if let TypeSpec::Character(length)=&element_kind {constants.insert(format!("@character_length:{name}"),Expr::IntLit(length.unwrap_or(1).to_string()));}
                    let values: Vec<Expr> = items.iter().map(|e| -> Result<Expr> {
                        if matches!(element_kind, TypeSpec::Logical) { Ok(Expr::Logical(logical_constant(e, &constants)?)) }
                        else { initial_value(&element_kind, e) }
                    }).collect::<Result<_>>()?;
                    constants.insert(key.clone(), Expr::Call("@array_constant".into(), values.clone()));
                    array_values.insert(key, values);
                    continue;
                }
                let value = if matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed) {
                    character_parameter(&initializer, kind).with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?
                } else if matches!(kind, TypeSpec::Logical) {
                    Expr::Logical(logical_constant(&initializer, &constants).with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?)
                } else {
                    initial_value(kind, &initializer).with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?
                };
                constants.insert(key, value);
            }
        }
        Ok(())
        })().map_err(|error|crate::source_diagnostics::at(error,declaration_offset))?;
    }
    // Resolve implied extents before ordinary bounds and CHARACTER length lowering.
    for stmt in body.iter_mut() {
        if let Stmt::ArrayDecl {kind,name,dims}=stmt {
            if dims.iter().any(|e|matches!(e,Expr::Ident(tag) if tag=="@assumed_extent")) {
                // An assumed-size dummy left for the by-reference calls of
                // procedures with COMPLEX dummies keeps (*) through COMPLEX
                // lowering.
                if arguments.contains(name) {
                    continue;
                }
                let values=array_values.get(name).with_context(||format!("array `{name}`: (*) currently requires a PARAMETER array constructor initializer"))?;
                if dims.len()!=1 {bail!("implied-shape PARAMETER arrays currently require rank one");}
                dims[0]=Expr::IntLit(values.len().to_string());
            }
            if matches!(kind,TypeSpec::CharacterAssumed) && array_values.contains_key(name) {
                let length=constants.get(&format!("@character_length:{name}")).context("missing CHARACTER array PARAMETER length")?;
                *kind=TypeSpec::Character(Some(usize::try_from(integer_constant(length)?)?));
            }
        }
    }
    body.retain(|s| !matches!(s, Stmt::InitializedDecl { .. }));
    rewrite_body(body, &constants, modules, if assumed_dummies {arguments} else {&[]}, template)?;
    let mut output = Vec::new();
    for stmt in std::mem::take(body) {
        let init = if let Stmt::ArrayDecl { name, dims, .. } = &stmt {
            if let Some(values) = array_values.get(name) {
                let (lower,extent)=match dims.as_slice() {
                    [Expr::Call(tag,bounds)] if tag=="@declared_bounds"=>{
                        let lower=integer_constant(&bounds[0])?;let upper=integer_constant(&bounds[1])?;
                        (lower,(i128::from(upper)-i128::from(lower)+1).max(0))
                    },
                    [dim]=>(1,i128::from(integer_constant(dim)?).max(0)),
                    _=>bail!("array PARAMETER `{name}` currently requires rank one"),
                };
                if extent != values.len() as i128 { bail!("array PARAMETER `{name}` initializer shape mismatch"); }
                values.iter().enumerate().map(|(i,v)| Stmt::AssignIndex { name: name.clone(), indices: vec![Expr::IntLit((lower+i as i64).to_string())], value: v.clone() }).collect()
            } else { vec![] }
        } else { vec![] };
        let variable_init: Vec<Stmt> = match &stmt {
            Stmt::VarDecl {names,..} => names.iter().filter_map(|name| variable_values.remove(&name.to_ascii_lowercase()).map(|value|
                if let Some(key)=saved_keys.get(name) {
                    // The destination declaration already records the INTEGER kind.
                    // Keep its initializer literal here, rather than expanding it to
                    // a temporary variable that cannot initialize object-file data.
                    let value=match value {
                        Expr::Call(tag,args) if tag=="@integer_literal"=>args[0].clone(),
                        other=>other,
                    };
                    let mut args=vec![Expr::Ident(name.clone()),Expr::Str(key.clone()),value];
                    if saved_undefined.contains(name) {args.push(Expr::Logical(false));}
                    Stmt::CallSub {name:"@saved_bind".into(),args}
                }
                else {Stmt::Assign{name:name.clone(),value}})).collect(),
            Stmt::ArrayDecl {name,kind,dims} => match variable_values.remove(&name.to_ascii_lowercase()) {
                Some(value) => match saved_keys.get(name) {
                    Some(key) => vec![saved_array(name,key,kind,dims,value,!saved_undefined.contains(name),&constants)?],
                    None => vec![Stmt::Assign{name:name.clone(),value}],
                },
                None => vec![],
            },
            _ => vec![],
        };
        if !matches!(&stmt,Stmt::VarDecl {names,..} if names.is_empty()) { output.push(stmt); }
        output.extend(init); output.extend(variable_init);
    }
    *body = output;
    crate::empty_constructors::materialize(body)?;
    Ok(constants)
}

/// Bind a SAVE or initialized local array of a procedure to static storage
/// that keeps its values between calls, as module arrays are bound.
fn saved_array(name: &str, key: &str, kind: &TypeSpec, dims: &[Expr], value: Expr, initialized: bool, constants: &Constants) -> Result<Stmt> {
    let count = declared_extents(dims, constants)?.iter().try_fold(1usize, |n, d| n.checked_mul((*d).max(0) as usize))
        .context("saved array extent overflow")?;
    // Values in array element order: a folded constructor, possibly reshaped
    // to the declared rank, or one scalar for every element.
    let items = match value {
        Expr::Call(tag, mut args) if tag == "reshape" => match args.swap_remove(0) {
            Expr::Call(tag, items) if tag == "@array_constructor" => items,
            _ => bail!("unsupported initializer for saved array `{name}`"),
        },
        Expr::Call(tag, items) if tag == "@array_constructor" => items,
        scalar => vec![scalar; count],
    };
    if items.len() != count {
        bail!("saved array `{name}` initializer shape mismatch");
    }
    let values = items.iter().map(|v| initial_value(kind, v)).collect::<Result<Vec<_>>>()?;
    let storage = crate::modules::Storage { kind: kind.clone(), values, initialized, rank: dims.len(), deferred: false };
    Ok(Stmt::CallSub { name: "@module_bind".into(), args: vec![
        Expr::Ident(name.to_owned()), Expr::Str(format!("@saved${key}")), Expr::Str(serde_json::to_string(&storage)?)] })
}

fn variable(name: &str, constants: &Constants) -> Result<()> {
    if constants.contains_key(&name.to_ascii_lowercase()) {
        bail!("cannot modify PARAMETER `{name}`");
    }
    Ok(())
}

// Constructor implied-DO indices have local values and never read/write the
// enclosing variable. Require explicit scalar INTEGER declarations in this subset.
fn validate_constructor_indices(body: &mut Vec<Stmt>) -> Result<()> {
    let mut integers = HashSet::new();
    for s in body.iter() {
        if let Stmt::VarDecl { kind: TypeSpec::Integer(_) | TypeSpec::IntegerKind(_), names } = s {
            integers.extend(names.iter().cloned());
        }
    }
    for s in body.iter() { if let Stmt::ArrayDecl { name, .. } = s { integers.remove(name); } }
    fn expr(e: &Expr, constructor: bool, integers: &HashSet<String>, active: &mut HashSet<String>, used: &mut HashSet<String>) -> Result<()> {
        match e {
            Expr::Call(n, args) if n == "@array_constructor" => for e in args { expr(e, true, integers, active, used)?; },
            Expr::Call(n, args) if constructor && n == "@output_implied_do" => {
                let Some(Expr::Ident(index)) = args.first() else { bail!("invalid constructor implied-DO index"); };
                if !integers.contains(index) { bail!("constructor implied-DO index `{index}` requires a locally declared scalar INTEGER variable"); }
                if !active.insert(index.clone()) { bail!("nested constructor implied-DO reuses index `{index}`"); }
                used.insert(index.clone());
                for e in &args[1..4] { expr(e, false, integers, active, used)?; }
                for e in &args[4..] { expr(e, true, integers, active, used)?; }
                active.remove(index);
            }
            Expr::Call(_, args) | Expr::Index(_, args) => for e in args { expr(e, false, integers, active, used)?; },
            Expr::Bin(_, a, b) => { expr(a, false, integers, active, used)?; expr(b, false, integers, active, used)?; }
            Expr::Un(_, a) => expr(a, false, integers, active, used)?,
            Expr::Section { lower, upper, stride, .. } => for e in [lower, upper, stride].into_iter().flatten() { expr(e, false, integers, active, used)?; },
            _ => {}
        }
        Ok(())
    }
    fn visit(body: &[Stmt], integers: &HashSet<String>, used: &mut HashSet<String>) -> Result<()> {
        for s in body {
            let mut check = |e: &Expr| expr(e, false, integers, &mut HashSet::new(), used);
            match s {
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => check(value)?,
                Stmt::AssignIndex { indices, value, .. } => { for e in indices { check(e)?; } check(value)?; }
                Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => for e in items { check(e)?; },
                Stmt::InitializedDecl { entities, .. } => for (_, e) in entities { if let Some(e) = e { check(e)?; } },
                Stmt::ArrayDecl { dims, .. } => for e in dims { check(e)?; },
                Stmt::If { cond, then_body, else_body } => { check(cond)?; visit(then_body, integers, used)?; if let Some(b) = else_body { visit(b, integers, used)?; } }
                Stmt::Do { start, end, body, .. } => { check(start)?; check(end)?; visit(body, integers, used)?; }
                Stmt::DoWhile { cond, body } => { check(cond)?; visit(body, integers, used)?; }
                Stmt::SelectCase { expr, cases, default } => { check(expr)?; for c in cases { visit(&c.body, integers, used)?; } if let Some(b) = default { visit(b, integers, used)?; } }
                _ => {} // Nested scopes validate their own declarations.
            }
        }
        Ok(())
    }
    let mut used = HashSet::new();
    visit(body, &integers, &mut used)?;
    let mut used: Vec<_> = used.into_iter().collect(); used.sort();
    for name in used { body.insert(0, Stmt::DummyIntent { name, intent: "@constructor_index".into() }); }
    Ok(())
}

fn constructor_items(items: &[Expr], constants: &Constants, output: &mut Vec<Expr>, budget: &mut usize) -> Result<()> {
    for item in items {
        match item {
            Expr::Call(n, args) if n == "@output_implied_do" => {
                let Expr::Ident(index) = &args[0] else { bail!("invalid constructor implied-DO index"); };
                let mut bounds = args[1..4].to_vec();
                for e in &mut bounds { rewrite_expr(e, constants)?; }
                let constant_bounds = bounds.iter().map(integer_constant).collect::<Result<Vec<_>>>();
                let Ok(bounds) = constant_bounds else {
                    let mut local=constants.clone();local.remove(index);
                    let mut values=vec![];
                    constructor_items(&args[4..],&local,&mut values,budget)?;
                    let mut delayed=vec![Expr::Ident(index.clone())];delayed.extend(bounds);delayed.extend(values);
                    output.push(Expr::Call("@output_implied_do".into(),delayed));
                    continue;
                };
                let (mut current, end, step) = (i128::from(bounds[0]), i128::from(bounds[1]), i128::from(bounds[2]));
                if step == 0 { bail!("constructor implied-DO stride must not be zero"); }
                let mut local = constants.clone();
                while if step > 0 { current <= end } else { current >= end } {
                    *budget = budget.checked_sub(1).context("array constructor exceeds the supported compile-time expansion limit")?;
                    local.insert(index.clone(), Expr::IntLit(current.to_string()));
                    constructor_items(&args[4..], &local, output, budget)?;
                    current += step;
                }
            }
            Expr::Call(n, args) if n == "@array_constructor" => constructor_items(args, constants, output, budget)?,
            Expr::Ident(n) if matches!(constants.get(n),Some(Expr::Call(tag,_)) if tag=="@array_constant") => {
                let Some(Expr::Call(_,items))=constants.get(n) else {unreachable!()};
                constructor_items(items,constants,output,budget)?;
            }
            _ => { *budget = budget.checked_sub(1).context("array constructor exceeds the supported compile-time expansion limit")?; let mut item = item.clone(); rewrite_expr(&mut item, constants)?; output.push(item); }
        }
    }
    Ok(())
}

pub(crate) fn expand_constructor(expr:&mut Expr)->Result<()> {
    if let Expr::Call(tag,items)=expr {
        if tag=="@array_constructor" {
            let mut expanded=vec![];
            constructor_items(items,&Constants::new(),&mut expanded,&mut 100_000)?;
            *items=expanded;
        }
    }
    Ok(())
}

// Keep runtime CHARACTER values until their source types and array shapes are
// available. Assignment into fixed-length array storage will apply the
// explicit constructor length, including blank padding and truncation.
fn typed_constructor(kind:&TypeSpec,items:Vec<Expr>)->Result<Expr> {
    match items.iter().map(|e|initial_value(kind,e)).collect::<Result<Vec<_>>>() {
        Ok(items)=>Ok(Expr::Call("@array_constructor".into(),items)),
        Err(_) if matches!(kind,TypeSpec::Character(_))=>{
            let TypeSpec::Character(length)=kind else {unreachable!()};
            let mut args=vec![Expr::IntLit(length.unwrap_or(1).to_string())];
            args.extend(items);
            Ok(Expr::Call("@runtime_character_constructor".into(),args))
        },
        Err(error)=>Err(error).context("typed array constructor currently requires constant elements"),
    }
}

/// Marks a local array, by name, in the scoped constants.
const LOCAL_ARRAY: &str = "@local_array:";

fn rewrite_expr(expr: &mut Expr, constants: &Constants) -> Result<()> {
    if let Expr::Call(name,args)=expr {
        if constants.contains_key(&format!("@model_procedure:{name}")) {
            // A visible user procedure/generic can extend an intrinsic name.
            // Resolve constants in its actuals, but preserve argument keywords
            // and intrinsic-looking calls until overload selection has run.
            for arg in args {rewrite_expr(arg,constants)?;}
            return Ok(());
        }
    }
    if let Expr::Call(name,args)=expr {
        if name=="@conditional" {
            // Preserve the full choice for static checking. Constant evaluators
            // select a consequent lazily; runtime lowering creates real branches.
            for arg in args.iter_mut() {rewrite_expr(arg,constants)?;}
            crate::operators::validate_constant_alternatives(args)?;
            return Ok(());
        }
    }
    if let Expr::Call(name,args)=expr {
        if matches!(name.as_str(),"len"|"len_trim"|"ichar"|"iachar"|"char"|"achar") && args.len()==2
            && !constants.contains_key(&format!("@model_procedure:{name}")) {
            let mut kind=args[1].clone();rewrite_expr(&mut kind,constants)?;
            let bytes=integer_constant(&kind).context("intrinsic KIND must be a constant INTEGER")?;
            let character=matches!(name.as_str(),"char"|"achar");
            if character && bytes!=1 {bail!("{name}: only default CHARACTER KIND=1 is currently supported");}
            if !character && !matches!(bytes,1|2|4|8) {bail!("{name}: INTEGER KIND must be 1, 2, 4 or 8");}
            let mut value=Expr::Call(name.clone(),vec![args[0].clone()]);rewrite_expr(&mut value,constants)?;
            *expr=if character || bytes==4 {value} else if let Ok(n)=integer_constant(&value) {typed_integer(n,bytes as u8)?}
                else {Expr::Call("int".into(),vec![value,Expr::IntLit(bytes.to_string())])};
            return Ok(());
        }
        // INT of a constant INTEGER, such as a BOZ literal, with a constant
        // KIND keeps the low-order bits of that kind, as the bit intrinsics do.
        if name=="int" && args.len()==2 && !constants.contains_key("@model_procedure:int") {
            let mut kind=args[1].clone();rewrite_expr(&mut kind,constants)?;
            let mut value=args[0].clone();rewrite_expr(&mut value,constants)?;
            if let (Ok(bytes),Ok(n))=(evaluate(&kind,constants),evaluate(&value,constants)) {
                if !matches!(&value,Expr::RealLit(_)) {
                    let n=match bytes {1=>n as i8 as i64,2=>n as i16 as i64,4=>n as i32 as i64,8=>n,
                        _=>bail!("INT: INTEGER KIND must be 1, 2, 4 or 8")};
                    *expr=typed_integer(n,bytes as u8)?;
                    return Ok(());
                }
            }
        }
    }
    if let Some(mut kind)=crate::empty_constructors::kind(expr)? {
        resolve_kind(&mut kind,constants)?;
        if crate::substrings::deferred(&kind) {bail!("typed CHARACTER constructor requires an explicit constant length");}
        let Expr::Call(_,args)=expr else {unreachable!()};
        if args.len()==1 {
            *expr=crate::empty_constructors::parsed(kind,vec![]);
        } else {
            let mut items=vec![];constructor_items(&args[1..],constants,&mut items,&mut 4096)?;
            *expr=typed_constructor(&kind,items)?;
        }
        return Ok(());
    }
    if let Expr::Call(name,args)=expr {
        if name=="@typed_character_constructor" {
            let mut length=args[0].clone();rewrite_expr(&mut length,constants)?;
            let length=usize::try_from(integer_constant(&length)?.max(0)).context("CHARACTER constructor length exceeds supported limits")?;
            let mut items=Vec::new();constructor_items(&args[1..],constants,&mut items,&mut 4096)?;
            *expr=typed_constructor(&TypeSpec::Character(Some(length)),items)?;
            return Ok(());
        }
    }
    if let Expr::Call(name, args) = expr {
        if matches!(name.as_str(),"minloc" | "maxloc" | "findloc") {
            let find = name == "findloc";
            let keys: &[&str] = if find { &["array","value","dim","mask","kind","back"] } else { &["array","dim","mask","kind","back"] };
            let mut slots: Vec<Option<Expr>> = vec![None;keys.len()];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (keys.iter().position(|key| *key == &tag[9..]).with_context(|| format!("{name}: unknown keyword `{}`",&tag[9..]))?,&values[0])
                    }
                    _ => { if keywords { bail!("{name}: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= keys.len() { bail!("{name}: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("{name}: duplicate argument"); }
            }
            let offset = usize::from(find);
            let mut kind = slots[3+offset].take().unwrap_or(Expr::IntLit("4".into()));
            rewrite_expr(&mut kind,constants)?;
            let bytes = integer_constant(&kind).context("location KIND must be constant INTEGER")?;
            if !matches!(bytes,4 | 8) { bail!("location intrinsics currently support KIND=4 or KIND=8"); }
            *args = vec![slots[0].take().with_context(|| format!("{name} requires ARRAY"))?,
                if find { slots[1].take().context("FINDLOC requires VALUE")? } else { Expr::Call("@no_value".into(),vec![]) },
                slots[1+offset].take().unwrap_or(Expr::Call("@no_dim".into(),vec![])),
                slots[2+offset].take().unwrap_or(Expr::Logical(true)),kind,slots[4+offset].take().unwrap_or(Expr::Logical(false))];
            *name = format!("@{name}");
        } else if matches!(name.as_str(),"pack" | "unpack") {
            let unpack = name == "unpack";
            let keys = if unpack { ["vector","mask","field"] } else { ["array","mask","vector"] };
            let mut slots: [Option<Expr>;3] = [None,None,None];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (keys.iter().position(|key| *key == &tag[9..]).with_context(|| format!("{name}: unknown keyword `{}`",&tag[9..]))?,&values[0])
                    }
                    _ => { if keywords { bail!("{name}: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= 3 { bail!("{name}: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("{name}: duplicate argument"); }
            }
            *args = vec![slots[0].take().with_context(|| format!("{name} requires {}",keys[0].to_uppercase()))?,slots[1].take().context("PACK/UNPACK requires MASK")?,
                if unpack { slots[2].take().context("UNPACK requires FIELD")? } else { slots[2].take().unwrap_or(Expr::Call("@no_vector".into(),vec![])) }];
        } else if name == "spread" {
            let mut slots: [Option<Expr>; 3] = [None,None,None];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (match &tag[9..] { "source" => 0, "dim" => 1, "ncopies" => 2, key => bail!("SPREAD: unknown keyword `{key}`") }, &values[0])
                    }
                    _ => { if keywords { bail!("SPREAD: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= 3 { bail!("SPREAD: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("SPREAD: duplicate argument"); }
            }
            *args = vec![slots[0].take().context("SPREAD requires SOURCE")?, slots[1].take().context("SPREAD requires DIM")?, slots[2].take().context("SPREAD requires NCOPIES")?];
        } else if name == "transpose" {
            if args.len() != 1 { bail!("TRANSPOSE requires one MATRIX argument"); }
            if let Expr::Call(tag,values) = &args[0] {
                if tag.starts_with("@keyword:") {
                    if tag != "@keyword:matrix" { bail!("TRANSPOSE: unknown keyword `{}`", &tag[9..]); }
                    args[0] = values[0].clone();
                }
            }
        } else if name == "eoshift" {
            let mut slots: [Option<Expr>; 4] = [None,None,None,None];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (match &tag[9..] { "array" => 0, "shift" => 1, "boundary" => 2, "dim" => 3, key => bail!("EOSHIFT: unknown keyword `{key}`") }, &values[0])
                    }
                    _ => { if keywords { bail!("EOSHIFT: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= 4 { bail!("EOSHIFT: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("EOSHIFT: duplicate argument"); }
            }
            *args = vec![slots[0].take().context("EOSHIFT requires ARRAY")?, slots[1].take().context("EOSHIFT requires SHIFT")?, slots[2].take().unwrap_or(Expr::Call("@no_boundary".into(),vec![])), slots[3].take().unwrap_or(Expr::IntLit("1".into()))];
        } else if name == "cshift" {
            let mut slots: [Option<Expr>; 3] = [None,None,None];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (match &tag[9..] { "array" => 0, "shift" => 1, "dim" => 2, key => bail!("CSHIFT: unknown keyword `{key}`") }, &values[0])
                    }
                    _ => { if keywords { bail!("CSHIFT: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= 3 { bail!("CSHIFT: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("CSHIFT: duplicate argument"); }
            }
            *args = vec![slots[0].take().context("CSHIFT requires ARRAY")?, slots[1].take().context("CSHIFT requires SHIFT")?, slots[2].take().unwrap_or(Expr::IntLit("1".into()))];
        } else if name == "reshape" {
            let mut slots: [Option<Expr>; 4] = [None,None,None,None];
            let mut keywords = false;
            for (position,arg) in args.iter().enumerate() {
                let (slot,value) = match arg {
                    Expr::Call(tag,values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        (match &tag[9..] { "source" => 0, "shape" => 1, "pad" => 2, "order" => 3, key => bail!("RESHAPE: unknown keyword `{key}`") }, &values[0])
                    }
                    _ => { if keywords { bail!("RESHAPE: positional argument after keyword argument"); } (position,arg) }
                };
                if slot >= 4 { bail!("RESHAPE: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("RESHAPE: duplicate argument"); }
            }
            *args = vec![slots[0].take().context("RESHAPE requires SOURCE")?, slots[1].take().context("RESHAPE requires SHAPE")?, slots[2].take().unwrap_or(Expr::Call("@no_pad".into(),vec![])), slots[3].take().unwrap_or(Expr::Call("@no_order".into(),vec![]))];
        } else if matches!(name.as_str(), "lbound" | "ubound" | "size" | "shape") {
            let (array, dim, kind) = crate::inquiries::arguments(name, args)?;
            if let Some(mut kind) = kind {
                rewrite_expr(&mut kind, constants)?;
                if integer_constant(&kind)? != 4 { bail!("{name}: only default INTEGER KIND=4 is currently supported"); }
            }
            *args = vec![array];
            args.extend(dim);
        } else if matches!(name.as_str(), "count" | "any" | "all") {
            let count = name == "count";
            let mut slots: [Option<Expr>; 3] = [None, None, None];
            let mut keywords = false;
            for (position, arg) in args.iter().enumerate() {
                let (slot, value) = match arg {
                    Expr::Call(tag, values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        let key = &tag[9..];
                        let slot = match key { "mask" => 0, "dim" => 1, "kind" if count => 2,
                            _ => bail!("{name}: unknown keyword `{key}`") };
                        (slot, &values[0])
                    }
                    _ => {
                        if keywords { bail!("{name}: positional argument after keyword argument"); }
                        (position, arg)
                    }
                };
                if slot >= if count { 3 } else { 2 } { bail!("{name}: too many arguments"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("{name}: duplicate argument"); }
            }
            let mask = slots[0].take().with_context(|| format!("{name} requires MASK"))?;
            if let Some(mut kind) = slots[2].take() {
                rewrite_expr(&mut kind, constants)?;
                match integer_constant(&kind).context("COUNT KIND must be a constant INTEGER")? {
                    4 => {},
                    8 => *name = "@count_i8".into(),
                    _ => bail!("COUNT currently supports KIND=4 or KIND=8"),
                }
            }
            *name = match name.as_str() { "count" => "@count_default", "any" => "@any_reduction", "all" => "@all_reduction", _ => "@count_i8" }.into();
            *args = vec![mask];
            if let Some(dim) = slots[1].take() {
                args.extend([dim, Expr::Logical(true)]);
            }
        } else if matches!(name.as_str(), "sum" | "product" | "minval" | "maxval" | "iany" | "iall" | "iparity" | "parity")
            && !constants.contains_key(&format!("{LOCAL_ARRAY}{name}")) {
            let first = if name == "parity" { "mask" } else { "array" };
            let mut slots: [Option<Expr>; 3] = [None, None, None];
            let mut keywords = false;
            for (position, arg) in args.iter().enumerate() {
                let (slot, value) = match arg {
                    Expr::Call(tag, values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        let key = &tag[9..];
                        let slot = match key { _ if key == first => 0, "dim" => 1, "mask" if name != "parity" => 2,
                            _ => bail!("{name}: unknown keyword `{key}`") };
                        (slot, &values[0])
                    }
                    _ => {
                        if keywords { bail!("{name}: positional argument after keyword argument"); }
                        (position, arg)
                    }
                };
                if slot >= 3 { bail!("{name} expects ARRAY, optional DIM, and optional MASK"); }
                if slots[slot].replace(value.clone()).is_some() { bail!("{name}: duplicate argument"); }
            }
            let array = slots[0].take().with_context(|| format!("{name} requires ARRAY"))?;
            *args = vec![array];
            if slots[1].is_some() || slots[2].is_some() {
                // Preserve omitted DIM distinctly from an invalid explicit zero.
                args.push(slots[1].take().unwrap_or_else(|| Expr::Call("@no_dim".into(), vec![])));
                args.push(slots[2].take().unwrap_or(Expr::Logical(true)));
            }
        }
        // Retain user keyword wrappers until generic overload selection can
        // associate them with the selected specific procedure's dummy names.
    }

    if let Expr::Call(name, items) = expr {
        if name == "@array_constructor" {
            let mut expanded = Vec::new();
            constructor_items(items, constants, &mut expanded, &mut 100_000)?;
            *items = expanded;
            return Ok(());
        }
    }
    match expr {
        Expr::IntLit(text) if text.contains('_') => {
            let (number,suffix) = text.split_once('_').unwrap();
            let kind = kind_number(suffix,constants)?;
            if !matches!(kind,1|2|4|8) { bail!("unsupported integer literal kind {kind}; supported kinds are 1, 2, 4 and 8"); }
            let value = number.parse::<i64>().context("integer literal exceeds the supported 64-bit range")?;
            *expr = typed_integer(value,kind as u8)?;
        }
        Expr::RealLit(text) => {
            let (number, kind) = text.split_once('_').map_or((text.as_str(), None), |(n,k)| (n,Some(k)));
            let double = match kind {
                Some(k) => match kind_number(k, constants)? { 4 => false, 8 => true, n => bail!("unsupported real literal kind {n}") },
                None => number.contains(['d','D']),
            };
            let value: f64 = number.replace(['d','D'], "e").parse()?;
            if !value.is_finite() { bail!("non-finite real literal"); }
            if double { *expr = real_literal(value, true); }
            else if kind.is_some() { *expr = real_literal((value as f32) as f64, false); }
        }
        Expr::Section { name, lower, upper, stride } => {
            for bound in [&mut *lower, &mut *upper, &mut *stride].into_iter().flatten() { rewrite_expr(bound, constants)?; }
            if let Some(value)=constants.get(&name.to_ascii_lowercase()) {
                if let Expr::Str(text)=value {
                    let range=Expr::Section {name:String::new(),lower:lower.clone(),upper:upper.clone(),stride:stride.clone()};
                    if stride.is_some() {bail!("CHARACTER substrings do not allow a stride");}
                    if [lower.as_deref(),upper.as_deref()].into_iter().flatten().all(|e|integer_constant(e).is_ok()) {
                        let (start,length)=crate::substrings::bounds(&range,text.len())?;
                        *expr=Expr::Str(text[start..start+length].into());
                    } else {
                        *expr=Expr::Call("@literal_substring".into(),vec![value.clone(),range]);
                    }
                } else if !matches!(value,Expr::Call(n,_) if n=="@array_constant") {
                    bail!("PARAMETER `{name}` is not an array");
                }
            }
        }
        Expr::Ident(name) => {
            if let Some(value) = constants.get(&name.to_ascii_lowercase()) {
                if !matches!(value, Expr::Call(n, _) if n == "@array_constant") { *expr = value.clone(); }
            }
        }
        Expr::Bin(_, left, right) => {
            rewrite_expr(left, constants)?;
            rewrite_expr(right, constants)?;
        }
        Expr::Un(_, inner) => rewrite_expr(inner, constants)?,
        Expr::Call(name, args) | Expr::Index(name, args) => {
            if let Some(Expr::Call(tag,values))=constants.get(name) {
                if tag=="@array_constant" && args.len()==1 {
                    let mut index=args[0].clone();rewrite_expr(&mut index,constants)?;
                    if let Ok(index)=integer_constant(&index) {
                        let lower=constants.get(&format!("@parameter_lower:{name}")).map(integer_constant).transpose()?.unwrap_or(1);
                        let index=usize::try_from(i128::from(index)-i128::from(lower)).context("PARAMETER array subscript is out of bounds")?;
                        *expr=values.get(index).context("PARAMETER array subscript is out of bounds")?.clone();
                        return Ok(());
                    }
                }
            }
            if constants.get(&name.to_ascii_lowercase()).is_some_and(|v| !matches!(v, Expr::Call(n, _) if n == "@array_constant")) {
                bail!("scalar PARAMETER `{name}` cannot be called or indexed");
            }
            for arg in args {
                rewrite_expr(arg, constants)?;
            }
        }
        _ => {}
    }
    if let Expr::Call(name,args)=expr {
        if args.len()==1 {
            if let Some(kind)=crate::empty_constructors::kind(&args[0])? {
                let folded=match name.as_str() {
                    "size"=>Some(Expr::IntLit("0".into())),
                    "kind"=>Some(Expr::IntLit(match kind {
                        TypeSpec::Integer(bytes)=>bytes.unwrap_or(4),
                        TypeSpec::DoublePrecision=>8,
                        TypeSpec::Character(_)=>1,
                        _=>4,
                    }.to_string())),
                    "len"=>if let TypeSpec::Character(length)=kind {Some(Expr::IntLit(length.unwrap_or(1).to_string()))} else {None},
                    _=>None,
                };
                if let Some(value)=folded {*expr=value;return Ok(());}
            }
        }
    }
    if let Expr::Call(name, args) = expr {
        match name.to_ascii_lowercase().as_str() {
            "trim" | "len_trim" if args.len() == 1 => {
                if let Ok(value) = character_constant(&args[0]) {
                    let value = value.trim_end_matches(' ');
                    *expr = if name.eq_ignore_ascii_case("trim") { Expr::Str(value.into()) }
                        else { Expr::IntLit(value.len().to_string()) };
                }
            }
            "len" if args.len() == 1 => {
                if let Expr::Ident(n) = &args[0] {
                    if let Some(length) = constants.get(&format!("@character_length:{n}")) { *expr = length.clone(); }
                } else if let Ok(text)=character_constant(&args[0]) {
                    *expr=Expr::IntLit(text.len().to_string());
                }
            }
            "selected_real_kind" => {
                if !(1..=3).contains(&args.len()) { bail!("SELECTED_REAL_KIND expects 1 to 3 constant integer arguments"); }
                let values = args.iter().enumerate().map(|(i,e)| if crate::named_arguments::omitted(e) {Ok(if i==2 {2} else {0})} else {evaluate(e, constants)}).collect::<Result<Vec<_>>>()
                    .context("SELECTED_REAL_KIND currently requires constant integer arguments")?;
                let p = values[0];
                let r = values.get(1).copied().unwrap_or(0);
                let radix = values.get(2).copied().unwrap_or(2);
                // Only binary32 and binary64 are implemented. In particular,
                // requesting quadruple precision must not silently select double.
                let kind = if radix != 2 { -5 }
                    else if p <= 6 && r <= 37 { 4 }
                    else if p <= 15 && r <= 307 { 8 }
                    else if p > 15 && r > 307 { -3 }
                    else if p > 15 { -1 }
                    else { -2 };
                *expr = Expr::IntLit(kind.to_string());
            }
            "selected_int_kind" => {
                if args.len() != 1 { bail!("SELECTED_INT_KIND expects one constant integer argument"); }
                let range = evaluate(&args[0], constants)
                    .context("SELECTED_INT_KIND currently requires a constant integer argument")?;
                let kind = if range <= 2 { 1 } else if range <= 4 { 2 }
                    else if range <= 9 { 4 } else if range <= 18 { 8 } else { -1 };
                *expr = Expr::IntLit(kind.to_string());
            }
            "selected_logical_kind" if !constants.contains_key("@model_procedure:selected_logical_kind") && !constants.contains_key(&model_type_key("selected_logical_kind")) => {
                if args.len()!=1 {bail!("SELECTED_LOGICAL_KIND expects one scalar INTEGER argument");}
                if let Ok(bits)=evaluate(&args[0],constants) {
                    *expr=Expr::IntLit(if bits<=32 {"4"} else {"-1"}.into());
                }
            }
            name if crate::real_intrinsics::keys(name).is_some() && !constants.contains_key(&model_type_key(name))
                && !constants.contains_key(&format!("@model_procedure:{name}")) => {
                let arity=crate::real_intrinsics::keys(name).unwrap().len();
                if args.len()!=arity {bail!("{} expects {arity} arguments, got {}",name.to_ascii_uppercase(),args.len());}
                let kinds=args.iter().map(|arg|crate::inquiries::model_type(arg,&|n|model_type_lookup(constants,n))).collect::<Result<Vec<_>>>()?;
                let kinds=kinds.into_iter().collect::<Option<Vec<_>>>();
                if let Some(kinds)=&kinds {crate::real_intrinsics::result_kind(name,kinds)?;}
                // Elemental constant calls also accept constructor and named
                // array constants, with ordinary scalar expansion.
                let arrays=args.iter().map(|arg|match arg {
                    Expr::Call(n,items) if n=="@array_constructor"=>Some(items.clone()),
                    Expr::Ident(n)=>match constants.get(n) {Some(Expr::Call(tag,items)) if tag=="@array_constant"=>Some(items.clone()),_=>None},
                    _=>None,
                }).collect::<Vec<_>>();
                if let Some(count)=arrays.iter().flatten().next().map(Vec::len) {
                    if arrays.iter().flatten().any(|a|a.len()!=count) {bail!("{} array shapes do not conform",name.to_ascii_uppercase());}
                    let mut values=vec![];
                    let mut constant=args.iter().zip(&arrays).all(|(arg,array)|array.is_some() || real_constant(arg).is_ok() || integer_constant(arg).is_ok());
                    for index in 0..count {
                        let args=args.iter().zip(&arrays).map(|(arg,array)|array.as_ref().map_or_else(||arg.clone(),|a|a[index].clone())).collect();
                        let mut item=Expr::Call(name.into(),args);
                        rewrite_expr(&mut item,constants)?;
                        if real_constant(&item).is_err() && integer_constant(&item).is_err() {constant=false;break;}
                        values.push(item);
                    }
                    if constant {*expr=Expr::Call("@array_constructor".into(),values);return Ok(());}
                }
                if name=="nearest" && real_constant(&args[1]).ok()==Some(0.0) {bail!("NEAREST S must not be zero");}
                if let Some(kinds)=kinds {
                    let result=crate::real_intrinsics::result_kind(name,&kinds)?;
                    let kind=if kinds[0]==TypeSpec::DoublePrecision {8}else{4};
                    let x=real_constant(&args[0]).ok();
                    let i=if matches!(name,"scale"|"set_exponent") {integer_constant(&args[1]).ok()} else {Some(0)};
                    let s=if name=="nearest" {real_constant(&args[1]).ok()} else {Some(0.0)};
                    if let (Some(x),Some(i),Some(s))=(x,i,s) {
                        let value=crate::float_model::evaluate(crate::real_intrinsics::opcode(name),kind,x,i,s);
                        if !value.is_finite() {bail!("non-finite {} constant expression",name.to_ascii_uppercase());}
                        *expr=if result==TypeSpec::Integer(None) {Expr::IntLit((value as i64).to_string())}else{
                            let literal=Expr::RealLit(value.to_string());
                            if kind==8 {Expr::Call("dble".into(),vec![literal])}else{literal}
                        };
                    }
                }
            }
            name if crate::bit_intrinsics::keys(name).is_some() && !constants.contains_key(&model_type_key(name))
                && !constants.contains_key(&format!("@model_procedure:{name}")) => {
                let arity=crate::bit_intrinsics::keys(name).unwrap().len();
                if args.len()!=arity {bail!("{} expects {arity} arguments, got {}",name.to_ascii_uppercase(),args.len());}
                let kinds=args.iter().map(|arg|crate::inquiries::model_type(arg,&|n|model_type_lookup(constants,n))).collect::<Result<Vec<_>>>()?;
                if let Some(kinds)=kinds.into_iter().collect::<Option<Vec<_>>>() {
                    crate::bit_intrinsics::result_kind(name,&kinds)?;
                    let TypeSpec::Integer(bytes)=kinds[0] else {unreachable!()};
                    let bytes=bytes.unwrap_or(4);
                    let values=args.iter().map(|arg|integer_constant(arg).ok()).collect::<Vec<_>>();
                    crate::bit_intrinsics::validate_controls(name,bytes,&values)?;
                    if let Some(values)=values.into_iter().collect::<Option<Vec<_>>>() {
                        let value=crate::bit_intrinsics::evaluate(name,bytes,&values)?;
                        *expr=if name=="btest" {Expr::Logical(value!=0)} else {typed_integer(value,bytes)?};
                    }
                }
            }
            name if crate::inquiries::numeric_model(name) && !constants.contains_key(&model_type_key(name))
                && !constants.contains_key(&format!("@model_procedure:{name}")) => {
                let [arg]=args.as_slice() else { bail!("{name} expects one argument"); };
                if let Some(kind)=crate::inquiries::model_type(arg,&|n|model_type_lookup(constants,n))? {
                    *expr=crate::inquiries::model_value(name,&kind)?;
                }
            }
            "kind" | "tiny" | "huge" if args.len() == 1 => {
                let double = matches!(&args[0], Expr::Call(n, _) if n.eq_ignore_ascii_case("dble"));
                let integer_kind = integer_literal_kind(&args[0]);
                let integer = integer_kind.is_some();
                if !double && !integer && !matches!(&args[0], Expr::RealLit(_)) {
                    return Ok(()); // Resolve inquiry on typed variables during array lowering.
                }
                if integer {
                    if name.eq_ignore_ascii_case("huge") {
                        *expr = typed_integer(crate::inquiries::integer_huge(integer_kind)?,integer_kind.unwrap())?;
                        return Ok(());
                    }
                    if name.eq_ignore_ascii_case("tiny") { bail!("TINY requires a REAL argument"); }
                }
                *expr = if name.eq_ignore_ascii_case("kind") { Expr::IntLit(integer_kind.map(|k|k.to_string()).unwrap_or_else(|| if double { "8" } else { "4" }.into())) }
                    else if name.eq_ignore_ascii_case("huge") { real_literal(if double { f64::MAX } else { f32::MAX as f64 }, double) }
                    else { real_literal(if double { f64::MIN_POSITIVE } else { f32::MIN_POSITIVE as f64 }, double) };
            }
            "real" if args.len() == 2 => {
                let kind = evaluate(&args[1], constants)?;
                if !matches!(kind, 4 | 8) { bail!("unsupported REAL conversion kind {kind}"); }
                args.pop();
                if kind == 8 { *name = "dble".into(); }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_optional_character_lengths(body:&[Stmt],optional:&HashSet<String>)->Result<()> {
    fn expression(e:&Expr,optional:&HashSet<String>)->Result<()> {
        match e {
            Expr::Ident(n) if optional.contains(n)=>bail!("CHARACTER length cannot depend on OPTIONAL dummy `{n}`"),
            Expr::Call(_,args)|Expr::Index(_,args)=>for e in args {expression(e,optional)?;},
            Expr::Bin(_,a,b)=>{expression(a,optional)?;expression(b,optional)?;},
            Expr::Un(_,e)=>expression(e,optional)?,_=>{},
        }
        Ok(())
    }
    let mut offset=None;
    for statement in body {
        if let Stmt::SourceLocation(value)=statement {offset=Some(*value);}
        match statement {
            Stmt::VarDecl {kind:TypeSpec::CharacterLength(length),..}|Stmt::ArrayDecl {kind:TypeSpec::CharacterLength(length),..}=>
                expression(length,optional).map_err(|error|crate::source_diagnostics::at(error,offset))?,
            Stmt::Block {body}=>{
                let mut local=optional.clone();
                for declaration in body {match declaration {
                    Stmt::VarDecl {names,..}=>for name in names {local.remove(name);},
                    Stmt::ArrayDecl {name,..}=>{local.remove(name);},_=>{},
                }}
                validate_optional_character_lengths(body,&local)?;
            }
            _=>{},
        }
    }
    Ok(())
}
fn resolve_template_kind(kind: &mut TypeSpec, constants: &Constants, template: bool) -> Result<()> {
    if template {
        if let TypeSpec::CharacterLength(length) = kind {
            rewrite_expr(length, constants)?;
            let mut concrete = kind.clone();
            if resolve_kind(&mut concrete, constants).is_ok() {
                *kind = concrete;
            }
            return Ok(());
        }
    }
    resolve_kind(kind, constants)
}
/// Evaluate a declaration's type in its defining constant environment.
pub(crate) fn concrete_kind_with(mut kind: TypeSpec, constants: &HashMap<String, Expr>) -> Result<TypeSpec> {
    resolve_kind(&mut kind, constants)?;
    Ok(kind)
}
pub(crate) fn constant_integer_with(expr: &Expr, constants: &HashMap<String, Expr>) -> Result<i64> {
    let mut value=expr.clone();
    rewrite_expr(&mut value,constants)?;
    evaluate(&value,constants)
}
/// Best-effort constant evaluation shared by source diagnostics.
pub(crate) fn constant_expression_with(expr: &Expr, constants: &HashMap<String, Expr>) -> Result<Expr> {
    let mut value = expr.clone();
    rewrite_expr(&mut value, constants)?;
    Ok(value)
}
pub(crate) fn concrete_kind(mut kind: TypeSpec) -> Result<TypeSpec> {
    resolve_kind(&mut kind, &Constants::new())?;
    Ok(kind)
}

fn rewrite_body(body: &mut [Stmt], constants: &Constants, modules: &Modules, assumed: &[String], template: bool) -> Result<()> {
    let mut scoped = constants.clone();
    // A local declaration hides host type information as well as its value.
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { names, .. } => for name in names {
                scoped.remove(&format!("@character_length:{name}"));
                scoped.remove(&format!("{LOCAL_ARRAY}{name}"));
            },
            Stmt::ArrayDecl { name, .. } => {
                scoped.remove(&format!("@character_length:{name}"));
                // A local array hides an intrinsic of the same name, as SUM.
                scoped.insert(format!("{LOCAL_ARRAY}{name}"), Expr::Logical(true));
            },
            _ => {},
        }
    }
    let mut source_offset=None;
    for stmt in body {
        if let Stmt::SourceLocation(offset)=stmt {source_offset=Some(*offset);}
        (|| -> Result<()> {
        let constants = &scoped;
        match stmt {
            Stmt::Print { items } => {
                for e in items {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Assign { name, value } => {
                variable(name, constants)?;
                rewrite_expr(value, constants)?;
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                variable(name, constants)?;
                for e in indices {
                    rewrite_expr(e, constants)?;
                }
                rewrite_expr(value, constants)?;
            }
            Stmt::VarDecl { kind: TypeSpec::CharacterAssumed, names } if names.iter().all(|n| assumed.contains(n)) => {},
            Stmt::VarDecl { kind, names } => {
                resolve_template_kind(kind, constants, template)?;
                if let TypeSpec::Character(length) = kind {
                    for name in names { scoped.insert(format!("@character_length:{name}"),Expr::IntLit(length.unwrap_or(1).to_string())); }
                }
            }
            Stmt::DerivedType { components, .. } => {
                // Component initializers are defaults, not PARAMETER declarations.
                // Resolve them in their defining scope before those constants disappear.
                for component in components {
                    match component {
                        Stmt::VarDecl { kind, .. } => resolve_kind(kind, constants)?,
                        Stmt::InitializedDecl { kind, entities, .. } => {
                            resolve_kind(kind, constants)?;
                            for (_, value) in entities { if let Some(e) = value { rewrite_expr(e, constants)?; } }
                        }
                        Stmt::ArrayDecl { kind, dims, .. } => {
                            resolve_kind(kind, constants)?;
                            for e in dims { rewrite_expr(e, constants)?; }
                        }
                        _ => {}
                    }
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                if crate::substrings::deferred(kind) { bail!("deferred CHARACTER arrays are not yet supported"); }
                if !matches!(kind, TypeSpec::CharacterAssumed) || !assumed.contains(name) {
                    resolve_template_kind(kind, constants, template)?;
                }
                for e in dims {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Read { args } => {
                for e in args {
                    match e {
                        Expr::Ident(name) | Expr::Call(name, _) | Expr::Index(name, _) => {
                            variable(name, constants)?
                        }
                        _ => {}
                    }
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::CallSub { name, args } => {
                if constants.contains_key(&name.to_ascii_lowercase()) {
                    bail!("PARAMETER `{name}` cannot be called");
                }
                fn input_variable(e:&Expr,constants:&Constants)->Result<()> {
                    match e {
                        Expr::Call(n,a) if matches!(n.as_str(),"@input_implied_do"|"@output_implied_do")=>{
                            if let Expr::Ident(n)=&a[0] {variable(n,constants)?;}
                            for item in &a[4..] {input_variable(item,constants)?;}
                        }
                        Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)|Expr::Section{name:n,..}=>variable(n,constants)?,
                        _=>{}
                    }Ok(())
                }
                let layout=match name.as_str(){"@internal_read"=>Some((3,1,2)),"@formatted_read_status"=>Some((4,2,3)),"@formatted_read"=>Some((2,usize::MAX,usize::MAX)),"@empty_read"=>Some((3,1,2)),_=>None};
                if let Some((first,control,status))=layout {
                    for e in &args[first..] {input_variable(e,constants)?;}
                    if matches!(args.get(control),Some(Expr::Str(n)) if n.eq_ignore_ascii_case("iostat")) {input_variable(&args[status],constants)?;}
                }
                for e in args {
                    rewrite_expr(e, constants)?;
                }
            }
            Stmt::Return(Some(e)) => rewrite_expr(e, constants)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(then_body, constants, modules, assumed, template)?;
                if let Some(body) = else_body {
                    rewrite_body(body, constants, modules, assumed, template)?;
                }
            }
            Stmt::Do {
                var,
                start,
                end,
                body,
            } => {
                variable(var, constants)?;
                rewrite_expr(start, constants)?;
                rewrite_expr(end, constants)?;
                rewrite_body(body, constants, modules, assumed, template)?;
            }
            Stmt::DoWhile { cond, body } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(body, constants, modules, assumed, template)?;
            }
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                rewrite_expr(expr, constants)?;
                for case in cases {
                    for item in &mut case.items {
                        match item {
                            CaseItem::Single(e) => rewrite_expr(e, constants)?,
                            CaseItem::Range(a, b) => {
                                rewrite_expr(a, constants)?;
                                rewrite_expr(b, constants)?;
                            }
                        }
                    }
                    rewrite_body(&mut case.body, constants, modules, assumed, template)?;
                }
                if let Some(body) = default {
                    rewrite_body(body, constants, modules, assumed, template)?;
                }
            }
            Stmt::Function { name,elemental,params, body, return_type, .. } => {
                source_offset=body.iter().find_map(|s|if let Stmt::SourceLocation(pos)=s {Some(*pos)}else{None});
                let inline=crate::elemental::inline_character(*elemental,name,params,return_type,body);
                let local = resolve_scope(body, constants, params, modules, inline, false, inline)?;
                if let Some(kind) = return_type { resolve_template_kind(kind, &local, inline)?; }
                let result_name = match body.last() { Some(Stmt::Return(Some(Expr::Ident(n)))) => Some(n), _ => None };
                if !inline && (matches!(return_type, Some(TypeSpec::Character(_))) || body.iter().any(|s| matches!(s,
                    Stmt::VarDecl { kind: TypeSpec::Character(_), names } if result_name.is_some_and(|n| names.contains(n))))) {
                    bail!("CHARACTER-valued functions are not yet supported by native function-result storage");
                }
            }
            Stmt::Subroutine { params, body, elemental, .. } => {
                let optional=body.iter().filter_map(|s|if let Stmt::DummyIntent{name,intent}=s {if intent=="optional" {Some(name.clone())}else{None}}else{None}).collect();
                validate_optional_character_lengths(body,&optional)?;
                let specialized=*elemental || body.iter().any(|s|match s {
                    Stmt::ArrayDecl {name,..}=>params.contains(name),
                    Stmt::VarDecl {kind,names}=>matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_)) && names.iter().any(|n|params.contains(n)),
                    _=>false,
                });
                resolve_scope(body, constants, params, modules, true, false, specialized)?;
            }
            Stmt::Block { body } => { resolve_scope(body, constants, &[], modules, false, false, template)?; },
            Stmt::Module { .. } => {},
            Stmt::InitializedDecl { .. } => {
                bail!("PARAMETER declaration must be in a specification part")
            }
            _ => {}
        }
        Ok(())
        })().map_err(|error|crate::source_diagnostics::at(error,source_offset))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_constant_character_lengths() {
        for (expression, length) in [("2*n+1", 7), ("+n", 3), ("-n", 0),
            ("len('ab  ')+len_trim(' z  ')", 6), ("max(1,abs(n-5))+min(n,2)", 4)] {
            let p = resolve_source(&format!("integer, parameter :: n=3\ncharacter(len={expression}) :: text"), "text='a'").unwrap();
            assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl {
                kind: TypeSpec::Character(Some(n)), names
            } if *n == length && names == &["text"])));
        }
    }

    #[test]
    fn parses_runtime_character_length_but_rejects_unsupported_storage() {
        let source = "program main\ninteger :: n\ncharacter(len=n*2) :: text\nend program main";
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "runtime-length.f90").unwrap();
        assert!(resolve(p).unwrap_err().to_string().contains("runtime-dependent lengths"));
        let source = "program main\ncontains\nfunction value() result(text)\ncharacter(len=2+1) :: text\ntext='abc'\nend function value\nend program main";
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "character-result.f90").unwrap();
        assert!(resolve(p).is_ok());
    }

    #[test]
    fn resolves_named_integer_kind_declarations() {
        let source = include_str!("../tests/65_named_integer_kinds.f90");
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "integer-kinds.f90").unwrap();
        let p = crate::derived::resolve(p).unwrap();
        let p = resolve(p).unwrap();
        assert!(!format!("{p:?}").contains("IntegerKind"));
        for kind in [1,2,4,8,16] {
            let p = resolve_source(&format!("integer, parameter :: k={kind}\ninteger(kind=k) :: x\ninteger(k) :: a(2)"), "x=1\na=2").unwrap();
            let expected = TypeSpec::Integer(if kind == 4 { None } else { Some(kind) });
            assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl { kind, names } if *kind == expected && names == &["x"])));
            assert!(p.body.iter().any(|s| matches!(s, Stmt::ArrayDecl { kind, .. } if *kind == expected)));
        }
        // A private selector belongs to the type's defining scope, not its user's.
        // This path has no named generic to trigger early constant resolution.
        let source = "module m\nprivate\npublic :: box\ninteger, parameter :: k=8\ntype :: box\ninteger(k) :: value=17\nend type box\nend module m\nuse m, only: box\ntype(box) :: x\nprint *, x%value\nend";
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "component-kind.f90").unwrap();
        let p = resolve(crate::derived::resolve(p).unwrap()).unwrap();
        assert!(!format!("{p:?}").contains("IntegerKind"));
        assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), .. })));
    }

    #[test]
    fn rejects_invalid_integer_kind_selectors() {
        for (declarations, expected) in [
            ("integer(kind=3) :: x", "unsupported INTEGER kind 3"),
            ("integer(256) :: x", "unsupported INTEGER kind 256"),
            ("integer(kind=0) :: x", "unsupported INTEGER kind 0"),
            ("integer, parameter :: k=-1\ninteger(k) :: x", "unsupported INTEGER kind -1"),
            ("integer(k) :: x", "not a previously defined integer PARAMETER"),
            ("integer :: k\ninteger(kind=k) :: x", "not a previously defined integer PARAMETER"),
        ] {
            let err = resolve_source(declarations, "").unwrap_err().to_string();
            assert!(err.contains(expected), "{declarations}: {err}");
        }
    }

    #[test]
    fn integer_parameters_preserve_declared_kind_and_range() {
        for (kind, value) in [(1,127), (2,32767), (4,2147483647), (8,10000000000i64)] {
            let p = resolve_source(&format!("integer(kind={kind}), parameter :: x={value}_8"), "print *,x").unwrap();
            let items = p.body.iter().find_map(|s|if let Stmt::Print{items}=s {Some(items)}else{None}).unwrap();
            assert_eq!(integer_literal_kind(&items[0]), Some(kind));
            assert_eq!(integer_constant(&items[0]).unwrap(), value);
        }
        assert!(resolve_source("integer(kind=1), parameter :: x=128", "").is_err());
    }

    #[test]
    fn resolves_selected_kinds_and_keyword_real_kind() {
        let source = include_str!("../tests/58_selected_kinds.f90");
        let p = crate::parser::parse_with_src(source, &crate::lexer::lex(source), "kinds.f90").unwrap();
        let p = resolve(p).unwrap();
        let values = p.body.iter().find_map(|s| if let Stmt::Print { items } = s { Some(items) } else { None }).unwrap();
        let values: Vec<_> = values.iter().map(|v| evaluate(v, &Constants::new()).unwrap()).collect();
        assert_eq!(values, [8, 4, 8, 4, 8, -999]);
        let p = resolve_source("integer,parameter :: dp=selected_real_kind(15,307)\nreal(kind=dp)::x\nreal(kind=4)::y", "x=1.0_dp\ny=2.0").unwrap();
        assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl { kind: TypeSpec::DoublePrecision, names } if names == &["x"])));
        assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl { kind: TypeSpec::Real, names } if names == &["y"])));
        // KIND remains usable as an intrinsic and as an ordinary identifier.
        resolve_source("integer :: kind\nreal :: x", "kind=4\nx=real(kind)\nprint *, kind(x)").unwrap();
    }

    #[test]
    fn selected_kind_boundaries_and_invalid_requests() {
        for (call, expected) in [
            ("selected_real_kind(6,37)", 4), ("selected_real_kind(7,37)", 8),
            ("selected_real_kind(6,38)", 8), ("selected_real_kind(15,307)", 8),
            ("selected_real_kind(32)", -1), ("selected_real_kind(6,308)", -2),
            ("selected_real_kind(32,400)", -3), ("selected_real_kind(6,37,10)", -5),
            ("selected_real_kind(6,37,2)", 4), ("selected_real_kind(-1,-1)", 4),
            ("selected_int_kind(0)", 1), ("selected_int_kind(2)", 1),
            ("selected_int_kind(3)", 2), ("selected_int_kind(4)", 2),
            ("selected_int_kind(5)", 4), ("selected_int_kind(9)", 4),
            ("selected_int_kind(10)", 8), ("selected_int_kind(18)", 8),
            ("selected_int_kind(19)", -1),
        ] {
            let p = resolve_source(&format!("integer,parameter :: k={call}"), "print *, k").unwrap();
            let Stmt::Print { items } = p.body.last().unwrap() else { panic!() };
            assert_eq!(evaluate(&items[0], &Constants::new()).unwrap(), expected, "{call}");
        }
        for call in ["selected_real_kind()", "selected_real_kind(1,2,3,4)", "selected_real_kind(6.0)", "selected_int_kind()", "selected_int_kind(1,2)", "selected_int_kind(9.0)", "selected_int_kind(n)"] {
            assert!(resolve_source("integer :: n", &format!("print *, {call}")).is_err(), "{call}");
        }
        assert!(resolve_source("integer,parameter :: qp=selected_real_kind(32)\nreal(kind=qp)::x", "").is_err());
    }

    #[test]
    fn resolves_real_kinds_and_module_constants() {
        let source = include_str!("../tests/44_real_kinds_parameters.f90");
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "kinds.f90").unwrap();
        let p = resolve(p).unwrap();
        assert!(!format!("{p:?}").contains("RealKind"));
        assert!(p.body.iter().any(|s| matches!(s, Stmt::VarDecl { kind: TypeSpec::DoublePrecision, names } if names == &["x"])));
        let p = resolve_source("real, parameter :: x=3/2", "print *, x").unwrap();
        let Stmt::Print { items } = p.body.last().unwrap() else { panic!() };
        assert!(matches!(&items[0], Expr::RealLit(s) if s == "1"));
    }

    #[test]
    fn rejects_unsupported_kinds_and_private_constant_imports() {
        for (decl, statement) in [
            ("real(16) :: x", ""), ("real :: x", "x=1.0_16"),
            ("", "print *, real(1,16)"), ("", "print *, tiny(1)"),
            ("real, parameter :: x=sqrt(-1.0)", ""),
        ] { assert!(resolve_source(decl, statement).is_err(), "{decl}; {statement}"); }
        let source = "module m\nprivate\ninteger,parameter :: dp=8\nend module m\nprogram main\nuse m\nreal(dp) :: x\nend program main";
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "private.f90").unwrap();
        assert!(resolve(p).is_err());
    }

    #[test]
    fn character_parameters_infer_and_adjust_lengths() {
        let p = resolve_source("character(len=*), parameter :: a='ab', b=a//'cd', nl=achar(10)\ncharacter(len=4), parameter :: padded='x', cut=b//'ef'\ncharacter, parameter :: single='xyz'\ncharacter(len=0), parameter :: empty='x'", "print *, a,b,nl,padded,cut,single,empty").unwrap();
        let Stmt::Print { items } = p.body.last().unwrap() else { panic!() };
        let expected = ["ab", "abcd", "\n", "x   ", "abcd", "x", ""];
        for (item, expected) in items.iter().zip(expected) {
            assert!(matches!(item, Expr::Str(value) if value == expected), "{item:?}");
        }
    }

    #[test]
    fn rejects_unsupported_character_parameters() {
        for (declarations, statements, expected) in [
            ("character(len=*), parameter :: a=1", "", "character constant expression"),
            ("character(len=*), parameter :: a=char(0)", "", "codes 1 through 127"),
            ("character(len=*), parameter :: a=achar(128)", "", "codes 1 through 127"),
            ("character(len=*), parameter :: a=char(-1)", "", "codes 1 through 127"),
            ("character(len=*), parameter :: a=char(1,4)", "", "CHARACTER KIND=1"),
            ("character(len=*), parameter :: a='x'", "a='y'", "cannot modify PARAMETER"),
            ("character(len=2), parameter :: a(2)=['ab','cd']", "a(1)='ef'", "cannot modify PARAMETER"),
            ("character(len=*) :: a", "", "requires a PARAMETER"),
        ] {
            let error = format!("{:#}", resolve_source(declarations, statements).unwrap_err());
            assert!(error.contains(expected), "{error}");
        }
    }

    fn resolve_source(declarations: &str, statements: &str) -> Result<Program> {
        let src = format!(
            "program main\nimplicit none\n{declarations}\n{statements}\nend program main\n"
        );
        let tokens = crate::lexer::lex(&src);
        let program = crate::parser::parse_with_src(&src, &tokens, "parameter.f90")
            .map_err(|errors| anyhow::anyhow!("{errors:?}"))?;
        resolve(program)
    }

    #[test]
    fn evaluates_named_integer_constants() {
        let program =
            resolve_source("integer, PARAMETER :: N = 5, m = n**2 + 1", "print *, N, m").unwrap();
        let Stmt::Print { items } = program.body.last().unwrap() else {
            panic!("expected PRINT")
        };
        assert!(matches!(&items[0], Expr::IntLit(s) if s == "5"));
        assert!(matches!(&items[1], Expr::IntLit(s) if s == "26"));
    }

    #[test]
    fn rejects_invalid_parameters_and_modifications() {
        for (decl, stmt, message) in [
            (
                "integer, parameter :: n = 5",
                "n = 6",
                "cannot modify PARAMETER",
            ),
            (
                "integer, parameter :: n = 5",
                "read(*,*) n",
                "cannot modify PARAMETER",
            ),
            ("integer, parameter :: n = 5\ninteger :: i", "read(*,*) (n,i=1,2)", "cannot modify PARAMETER"),
            ("integer, parameter :: n = 5\ninteger :: i", "read(*,*,iostat=n) i", "cannot modify PARAMETER"),
            (
                "integer, parameter :: n = 5",
                "do n = 1, 3\nprint *, n\nend do",
                "cannot modify PARAMETER",
            ),
            ("integer, parameter :: n", "", "requires an initializer"),
            (
                "integer :: i\ninteger, parameter :: n = i",
                "",
                "previously defined",
            ),
            ("integer, parameter :: n = 1/0", "", "division by zero"),
            ("integer, parameter :: n = 2**63", "", "overflow"),
            (
                "integer, parameter :: n = 5\ninteger :: n",
                "",
                "duplicate declaration",
            ),
        ] {
            let error = format!("{:#}", resolve_source(decl, stmt).unwrap_err());
            assert!(error.contains(message), "{error}");
        }
    }
}

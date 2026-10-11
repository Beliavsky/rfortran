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
    fn visit(body: &mut [Stmt], modules: &HashSet<String>, needed: &mut bool) -> Result<()> {
        for stmt in body {
            match stmt {
                Stmt::Use { module, nature, only } => {
                    let intrinsic = match nature.take().map(|n| n.to_ascii_lowercase()).as_deref() {
                        Some("intrinsic") => true,
                        Some("non_intrinsic") => {
                            if !modules.contains(module) { bail!("NON_INTRINSIC module `{module}` must be provided in the same source group"); }
                            false
                        }
                        Some(other) => bail!("unknown USE module nature `{other}`; expected INTRINSIC or NON_INTRINSIC"),
                        None => module.eq_ignore_ascii_case("iso_fortran_env") && !modules.contains(module),
                    };
                    if intrinsic {
                        if !module.eq_ignore_ascii_case("iso_fortran_env") { bail!("intrinsic module `{module}` is not yet supported"); }
                        *module = INTERNAL.into();
                        if let Some(names) = only { for name in names { name.make_ascii_lowercase(); } }
                        *needed = true;
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
    }
    let mut needed = false;
    visit(&mut program.body, &user_modules, &mut needed)?;
    if needed && !user_modules.contains(INTERNAL) {
        let body = [("int8",1), ("int16",2), ("int32",4), ("int64",8),
                    ("real32",4), ("real64",8), ("real128",-1)].into_iter()
            .map(|(name,value)| Stmt::InitializedDecl {
                kind: TypeSpec::Integer(None), attributes: vec!["parameter".into()],
                entities: vec![(name.into(),Some(Expr::IntLit(value.to_string())))],
            }).collect();
        program.body.insert(0, Stmt::Module { name: INTERNAL.into(), body });
    }
    Ok(())
}

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
    if name.split_once('_').is_some_and(|(number,_)| number.parse::<i64>().is_ok()) {
        let mut literal = Expr::IntLit(name.into());
        rewrite_expr(&mut literal,constants)?;
        return evaluate(&literal,constants);
    }
    name.parse().or_else(|_| evaluate(&Expr::Ident(name.into()), constants))
}
fn resolve_kind(kind: &mut TypeSpec, constants: &Constants) -> Result<()> {
    if crate::substrings::deferred(kind) { return Ok(()); }
    if matches!(kind, TypeSpec::CharacterAssumed) { bail!("assumed CHARACTER length is currently supported only for scalar PARAMETER declarations"); }
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
    name.strip_prefix("@model_type:").or_else(||name.strip_prefix("@model_procedure:")).unwrap_or(name)
}
fn record_model_type(constants: &mut Constants, name: &str, kind: &TypeSpec, rank: usize) {
    let code = match kind {
        TypeSpec::Integer(bytes) => -i64::from(bytes.unwrap_or(4)),
        TypeSpec::Real => 4,
        TypeSpec::DoublePrecision => 8,
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
        _ => TypeSpec::Logical,
    },rank.parse().ok()?))
}
fn real_constant(e: &Expr) -> Result<f64> {
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
                "abs" => value.abs(), _ => bail!("unsupported function in real PARAMETER initializer: {name}"),
            }
        }
        _ => bail!("PARAMETER initializer must be a supported constant expression"),
    };
    if !value.is_finite() { bail!("non-finite real PARAMETER initializer"); }
    Ok(value)
}

// Character constants are lowered to literals before either backend sees them.
pub(crate) fn character_constant(expr: &Expr) -> Result<String> {
    match expr {
        Expr::Str(value) => Ok(value.clone()),
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
    crate::purity::validate(&program)?;
    resolve_constants(program)
}

// Operator selection needs kinds before calls and derived components have
// been lowered. Purity is checked by resolve() after that lowering.
pub(crate) fn resolve_constants(mut program: Program) -> Result<Program> {
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
            let constants = resolve_scope(body, &Constants::new(), &[], &modules, false, false)?;
            modules.insert(name.to_ascii_lowercase(), constants.into_iter().filter(|(n, _)| exports.get(model_metadata_name(n)).copied().unwrap_or(default_public)).collect());
        }
    }
    resolve_scope(&mut program.body, &Constants::new(), &[], &modules, false, true)?;
    Ok(program)
}

fn evaluate(expr: &Expr, constants: &Constants) -> Result<i64> {
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

fn separate_parameters(body: &mut Vec<Stmt>, arguments: &[String]) -> Result<()> {
    if !body.iter().any(|s| matches!(s,Stmt::InitializedDecl { attributes, .. } if attributes.iter().any(|a| a == "@separate_parameter"))) { return Ok(()); }
    let mut declared: HashMap<String,TypeSpec> = HashMap::new();
    let mut converted = HashSet::new();
    let mut output = Vec::new();
    for stmt in std::mem::take(body) {
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
    }
    for stmt in &mut output {
        if let Stmt::VarDecl { names, .. } = stmt { names.retain(|n| !converted.contains(n)); }
    }
    output.retain(|s| !matches!(s,Stmt::VarDecl { names, .. } if names.is_empty()));
    *body = output;
    Ok(())
}

// Main-program storage exists for the whole execution. Other scopes need a
// separate persistent-storage implementation before initialized locals are safe.
fn initial_value(kind: &TypeSpec, value: &Expr) -> Result<Expr> {
    if let Expr::Call(tag,items) = value {
        if tag == "@array_constructor" {
            return Ok(Expr::Call(tag.clone(),items.iter().map(|e| initial_value(kind,e)).collect::<Result<_>>()?));
        }
    }
    match kind {
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
        TypeSpec::Logical => if let Expr::Logical(_) = value { Ok(value.clone()) }
            else { bail!("LOGICAL declaration initializer currently requires a logical literal"); },
        _ => bail!("unsupported declaration initialization type"),
    }
}

fn resolve_scope(body: &mut Vec<Stmt>, inherited: &Constants, arguments: &[String], modules: &Modules, assumed_dummies: bool, main_scope: bool) -> Result<Constants> {
    separate_parameters(body, arguments)?;
    validate_constructor_indices(body)?;
    let mut constants = inherited.clone();
    for stmt in body.iter() {
        if let Stmt::Use { module, only, .. } = stmt {
            if let Some(imports) = modules.get(&module.to_ascii_lowercase()) {
                constants.extend(imports.iter().filter(|(name, _)| only.as_ref().is_none_or(|names| names.iter().any(|n| n == model_metadata_name(name))))
                    .map(|(n, value)| (n.clone(), value.clone())));
            }
        }
    }
    let arrays: HashSet<String> = body.iter().filter_map(|s| if let Stmt::ArrayDecl { name, .. } = s { Some(name.to_ascii_lowercase()) } else { None }).collect();
    let mut array_values = HashMap::new();
    let mut variable_values = HashMap::new();
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
        if let Stmt::Function {name,..} | Stmt::Subroutine {name,..} = stmt {
            constants.insert(format!("@model_procedure:{name}"),Expr::Logical(true));
        }
    }
    for stmt in body.iter_mut() {
        // Keep type information separately from constant values. Numeric model
        // inquiries can appear in PARAMETER initializers and must not read X.
        let (kind,names,rank) = match &*stmt {
            Stmt::VarDecl {kind,names} => (Some(kind),names.clone(),0),
            Stmt::ArrayDecl {kind,name,dims} => (Some(kind),vec![name.clone()],dims.len()),
            _ => (None,vec![],0),
        };
        if let Some(kind) = kind {
            let mut kind=kind.clone();
            if matches!(kind,TypeSpec::IntegerKind(_) | TypeSpec::RealKind(_)) { resolve_kind(&mut kind,&constants)?; }
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
                if !main_scope { bail!("declaration initialization without PARAMETER is currently supported only in the main program; persistent SAVE storage in other scopes is not yet implemented"); }
                if attributes.iter().any(|a| !a.eq_ignore_ascii_case("save") && !a.eq_ignore_ascii_case("target")) {
                    bail!("unsupported attribute on initialized main-program variable");
                }
                resolve_kind(kind,&constants)?;
                let mut scalars = vec![];
                for (name,initializer) in entities.iter() {
                    if !arrays.contains(name) { record_model_type(&mut constants,name,kind,0); }
                    let key = name.to_ascii_lowercase();
                    if !locals.insert(key.clone()) && !arrays.contains(&key) { bail!("duplicate declaration of initialized variable `{name}`"); }
                    let mut value = initializer.clone().with_context(|| format!("initialized declaration `{name}` requires a value"))?;
                    rewrite_expr(&mut value,&constants)?;
                    if !arrays.contains(&key) && matches!(&value,Expr::Call(tag,_) if tag == "@array_constructor") {
                        bail!("scalar declaration initializer cannot be an array");
                    }
                    let value = initial_value(kind,&value).with_context(|| format!("invalid declaration initializer for `{name}`"))?;
                    if variable_values.insert(key.clone(),value).is_some() { bail!("duplicate initialization of `{name}`"); }
                    if !arrays.contains(&key) { scalars.push(name.clone()); }
                }
                *stmt = Stmt::VarDecl {kind:kind.clone(),names:scalars};
                continue;
            }
            if !matches!(kind, TypeSpec::CharacterAssumed) { resolve_kind(kind, &constants)?; }
            if matches!(kind, TypeSpec::Integer(Some(k)) if *k != 4) {
                bail!("nondefault-kind INTEGER PARAMETER declarations are not yet supported");
            }
            if !matches!(kind, TypeSpec::Integer(None | Some(4)) | TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::Character(_) | TypeSpec::CharacterAssumed) {
                bail!("only numeric and character PARAMETER declarations are currently supported");
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
                    if matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed) { bail!("CHARACTER array PARAMETER declarations are not yet supported"); }
                    let Expr::Call(tag, items) = initializer else { bail!("array PARAMETER requires an array constructor"); };
                    if tag != "@array_constructor" { bail!("array PARAMETER requires an array constructor"); }
                    let values: Vec<Expr> = items.iter().map(|e| -> Result<Expr> { Ok(if matches!(kind, TypeSpec::Integer(_)) { Expr::IntLit(evaluate(e, &constants)?.to_string()) } else { real_literal(if matches!(kind, TypeSpec::Real) { real_constant(e)? as f32 as f64 } else { real_constant(e)? }, matches!(kind, TypeSpec::DoublePrecision)) }) }).collect::<Result<_>>()?;
                    constants.insert(key.clone(), Expr::Call("@array_constant".into(), values.clone()));
                    array_values.insert(key, values);
                    continue;
                }
                let value = if matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed) {
                    character_parameter(&initializer, kind).with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?
                } else if matches!(kind, TypeSpec::Integer(_)) {
                    Expr::IntLit(evaluate(&initializer, &constants).with_context(|| format!("invalid initializer for PARAMETER `{name}`"))?.to_string())
                } else {
                    let value = real_constant(&initializer)?;
                    real_literal(if matches!(kind, TypeSpec::Real) { (value as f32) as f64 } else { value }, matches!(kind, TypeSpec::DoublePrecision))
                };
                constants.insert(key, value);
            }
        }
    }
    body.retain(|s| !matches!(s, Stmt::InitializedDecl { .. }));
    rewrite_body(body, &constants, modules, if assumed_dummies {arguments} else {&[]})?;
    let mut output = Vec::new();
    for stmt in std::mem::take(body) {
        let init = if let Stmt::ArrayDecl { name, dims, .. } = &stmt {
            if let Some(values) = array_values.get(name) {
                if dims.len() != 1 || integer_constant(&dims[0])? != values.len() as i64 { bail!("array PARAMETER `{name}` initializer shape mismatch"); }
                values.iter().enumerate().map(|(i,v)| Stmt::AssignIndex { name: name.clone(), indices: vec![Expr::IntLit((i+1).to_string())], value: v.clone() }).collect()
            } else { vec![] }
        } else { vec![] };
        let variable_init: Vec<Stmt> = match &stmt {
            Stmt::VarDecl {names,..} => names.iter().filter_map(|name| variable_values.remove(&name.to_ascii_lowercase()).map(|value| Stmt::Assign{name:name.clone(),value})).collect(),
            Stmt::ArrayDecl {name,..} => variable_values.remove(&name.to_ascii_lowercase()).map(|value| vec![Stmt::Assign{name:name.clone(),value}]).unwrap_or_default(),
            _ => vec![],
        };
        if !matches!(&stmt,Stmt::VarDecl {names,..} if names.is_empty()) { output.push(stmt); }
        output.extend(init); output.extend(variable_init);
    }
    *body = output;
    Ok(constants)
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
                let bounds = bounds.iter().map(integer_constant).collect::<Result<Vec<_>>>()
                    .context("constructor implied-DO bounds and stride must currently be constant integers")?;
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
            _ => { *budget = budget.checked_sub(1).context("array constructor exceeds the supported compile-time expansion limit")?; let mut item = item.clone(); rewrite_expr(&mut item, constants)?; output.push(item); }
        }
    }
    Ok(())
}

fn rewrite_expr(expr: &mut Expr, constants: &Constants) -> Result<()> {
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
        } else if matches!(name.as_str(), "sum" | "product" | "minval" | "maxval") {
            let mut slots: [Option<Expr>; 3] = [None, None, None];
            let mut keywords = false;
            for (position, arg) in args.iter().enumerate() {
                let (slot, value) = match arg {
                    Expr::Call(tag, values) if tag.starts_with("@keyword:") => {
                        keywords = true;
                        let key = &tag[9..];
                        let slot = match key { "array" => 0, "dim" => 1, "mask" => 2,
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
            if constants.get(&name.to_ascii_lowercase()).is_some_and(|v|!matches!(v,Expr::Call(n,_) if n=="@array_constant")) { bail!("PARAMETER `{name}` is not an array"); }
            for bound in [lower, upper, stride].into_iter().flatten() { rewrite_expr(bound, constants)?; }
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
            if constants.get(&name.to_ascii_lowercase()).is_some_and(|v| !matches!(v, Expr::Call(n, _) if n == "@array_constant")) {
                bail!("scalar PARAMETER `{name}` cannot be called or indexed");
            }
            for arg in args {
                rewrite_expr(arg, constants)?;
            }
        }
        _ => {}
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
                }
            }
            "selected_real_kind" => {
                if !(1..=3).contains(&args.len()) { bail!("SELECTED_REAL_KIND expects 1 to 3 constant integer arguments"); }
                let values = args.iter().map(|e| evaluate(e, constants)).collect::<Result<Vec<_>>>()
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

fn rewrite_body(body: &mut [Stmt], constants: &Constants, modules: &Modules, assumed: &[String]) -> Result<()> {
    let mut scoped = constants.clone();
    // A local declaration hides host type information as well as its value.
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { names, .. } => for name in names { scoped.remove(&format!("@character_length:{name}")); },
            Stmt::ArrayDecl { name, .. } => { scoped.remove(&format!("@character_length:{name}")); },
            _ => {},
        }
    }
    for stmt in body {
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
                resolve_kind(kind, constants)?;
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
                    resolve_kind(kind, constants)?;
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
                rewrite_body(then_body, constants, modules, assumed)?;
                if let Some(body) = else_body {
                    rewrite_body(body, constants, modules, assumed)?;
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
                rewrite_body(body, constants, modules, assumed)?;
            }
            Stmt::DoWhile { cond, body } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(body, constants, modules, assumed)?;
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
                    rewrite_body(&mut case.body, constants, modules, assumed)?;
                }
                if let Some(body) = default {
                    rewrite_body(body, constants, modules, assumed)?;
                }
            }
            Stmt::Function { params, body, return_type, .. } => {
                let local = resolve_scope(body, constants, params, modules, false, false)?;
                if let Some(kind) = return_type { resolve_kind(kind, &local)?; }
                let result_name = match body.last() { Some(Stmt::Return(Some(Expr::Ident(n)))) => Some(n), _ => None };
                if matches!(return_type, Some(TypeSpec::Character(_))) || body.iter().any(|s| matches!(s,
                    Stmt::VarDecl { kind: TypeSpec::Character(_), names } if result_name.is_some_and(|n| names.contains(n)))) {
                    bail!("CHARACTER-valued functions are not yet supported by native function-result storage");
                }
            }
            Stmt::Subroutine { params, body, .. } => { resolve_scope(body, constants, params, modules, true, false)?; }
            Stmt::Block { body } => { resolve_scope(body, constants, &[], modules, false, false)?; },
            Stmt::Module { .. } => {},
            Stmt::InitializedDecl { .. } => {
                bail!("PARAMETER declaration must be in a specification part")
            }
            _ => {}
        }
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
        assert!(resolve(p).unwrap_err().to_string().contains("CHARACTER-valued functions"));
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
            ("integer(kind=8), parameter :: x=1", "nondefault-kind INTEGER PARAMETER"),
        ] {
            let err = resolve_source(declarations, "").unwrap_err().to_string();
            assert!(err.contains(expected), "{declarations}: {err}");
        }
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
            ("character(len=*), parameter :: a=char(1,1)", "", "character constant expression"),
            ("character(len=*), parameter :: a='x'", "a='y'", "cannot modify PARAMETER"),
            ("character(len=2), parameter :: a(2)=['ab','cd']", "", "CHARACTER array PARAMETER"),
            ("character(len=*) :: a", "", "only for scalar PARAMETER"),
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

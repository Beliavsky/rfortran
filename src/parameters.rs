//! Resolve supported named constants before semantic analysis and lowering.
use crate::ast::{BinOp, CaseItem, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

type Constants = HashMap<String, Expr>;
type Modules = HashMap<String, Constants>;

fn kind_number(name: &str, constants: &Constants) -> Result<i64> {
    name.parse().or_else(|_| evaluate(&Expr::Ident(name.into()), constants))
}
fn resolve_kind(kind: &mut TypeSpec, constants: &Constants) -> Result<()> {
    if matches!(kind, TypeSpec::CharacterAssumed) { bail!("assumed CHARACTER length is currently supported only for scalar PARAMETER declarations"); }
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
fn real_literal(value: f64, double: bool) -> Expr {
    let literal = Expr::RealLit(value.to_string());
    if double { Expr::Call("dble".into(), vec![literal]) } else { literal }
}
fn real_constant(e: &Expr) -> Result<f64> {
    fn integral(e: &Expr) -> bool {
        match e { Expr::IntLit(_) => true, Expr::Un(_, a) => integral(a), Expr::Bin(_, a,b) => integral(a) && integral(b), _ => false }
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
fn character_constant(expr: &Expr) -> Result<String> {
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

pub fn resolve(mut program: Program) -> Result<Program> {
    crate::purity::validate(&program)?;
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
            let constants = resolve_scope(body, &Constants::new(), &[], &modules)?;
            modules.insert(name.to_ascii_lowercase(), constants.into_iter().filter(|(n, _)| exports.get(n).copied().unwrap_or(default_public)).collect());
        }
    }
    resolve_scope(&mut program.body, &Constants::new(), &[], &modules)?;
    Ok(program)
}

fn evaluate(expr: &Expr, constants: &Constants) -> Result<i64> {
    let value = match expr {
        Expr::IntLit(s) => s.parse().ok(),
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

fn resolve_scope(body: &mut Vec<Stmt>, inherited: &Constants, arguments: &[String], modules: &Modules) -> Result<Constants> {
    let mut constants = inherited.clone();
    for stmt in body.iter() {
        if let Stmt::Use { module, only } = stmt {
            if let Some(imports) = modules.get(&module.to_ascii_lowercase()) {
                constants.extend(imports.iter().filter(|(name, _)| only.as_ref().is_none_or(|names| names.contains(name)))
                    .map(|(n, value)| (n.clone(), value.clone())));
            }
        }
    }
    let arrays: HashSet<String> = body.iter().filter_map(|s| if let Stmt::ArrayDecl { name, .. } = s { Some(name.clone()) } else { None }).collect();
    let mut array_values = HashMap::new();
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
    }
    // Local parameter names also hide host constants, even in their initializers.
    for stmt in body.iter() {
        if let Stmt::InitializedDecl { entities, .. } = stmt {
            for (name, _) in entities {
                constants.remove(&name.to_ascii_lowercase());
            }
        }
    }
    for stmt in body.iter_mut() {
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
                bail!("declaration initialization without PARAMETER is not yet supported");
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
    rewrite_body(body, &constants, modules)?;
    let mut output = Vec::new();
    for stmt in std::mem::take(body) {
        let init = if let Stmt::ArrayDecl { name, dims, .. } = &stmt {
            if let Some(values) = array_values.get(name) {
                if dims.len() != 1 || integer_constant(&dims[0])? != values.len() as i64 { bail!("array PARAMETER `{name}` initializer shape mismatch"); }
                values.iter().enumerate().map(|(i,v)| Stmt::AssignIndex { name: name.clone(), indices: vec![Expr::IntLit((i+1).to_string())], value: v.clone() }).collect()
            } else { vec![] }
        } else { vec![] };
        output.push(stmt); output.extend(init);
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

fn rewrite_expr(expr: &mut Expr, constants: &Constants) -> Result<()> {
    match expr {
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
            if constants.contains_key(&name.to_ascii_lowercase()) { bail!("PARAMETER `{name}` is not an array"); }
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
            "kind" | "tiny" | "huge" if args.len() == 1 => {
                let double = matches!(&args[0], Expr::Call(n, _) if n.eq_ignore_ascii_case("dble"));
                if !double && !matches!(&args[0], Expr::RealLit(_) | Expr::IntLit(_)) {
                    return Ok(()); // Resolve inquiry on typed variables during array lowering.
                }
                if !name.eq_ignore_ascii_case("kind") && matches!(&args[0], Expr::IntLit(_)) { bail!("{name} currently requires a REAL argument"); }
                *expr = if name.eq_ignore_ascii_case("kind") { Expr::IntLit(if double { "8" } else { "4" }.into()) }
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

fn rewrite_body(body: &mut [Stmt], constants: &Constants, modules: &Modules) -> Result<()> {
    for stmt in body {
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
            Stmt::VarDecl { kind, .. } => resolve_kind(kind, constants)?,
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
            Stmt::ArrayDecl { kind, dims, .. } => {
                resolve_kind(kind, constants)?;
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
                rewrite_body(then_body, constants, modules)?;
                if let Some(body) = else_body {
                    rewrite_body(body, constants, modules)?;
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
                rewrite_body(body, constants, modules)?;
            }
            Stmt::DoWhile { cond, body } => {
                rewrite_expr(cond, constants)?;
                rewrite_body(body, constants, modules)?;
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
                    rewrite_body(&mut case.body, constants, modules)?;
                }
                if let Some(body) = default {
                    rewrite_body(body, constants, modules)?;
                }
            }
            Stmt::Function { params, body, return_type, .. } => {
                let local = resolve_scope(body, constants, params, modules)?;
                if let Some(kind) = return_type { resolve_kind(kind, &local)?; }
            }
            Stmt::Subroutine { params, body, .. } => { resolve_scope(body, constants, params, modules)?; }
            Stmt::Block { body } => { resolve_scope(body, constants, &[], modules)?; },
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

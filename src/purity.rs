// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Validate the side-effect restrictions of the supported PURE procedure subset.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

struct Procedure { pure: bool, intents: Option<Vec<String>>, optional: Vec<bool> }
fn intents(params: &[String], body: &[Stmt]) -> Vec<String> {
    params.iter().map(|p| body.iter().find_map(|s| match s {
        Stmt::DummyIntent { name, intent } if name == p && intent.starts_with("intent(") => Some(intent.clone()), _ => None,
    }).unwrap_or_default()).collect()
}

pub fn validate(program: &Program) -> Result<()> {
    fn procedures(body: &[Stmt], known: &mut HashMap<String, Procedure>) {
        for s in body {
            match s {
                Stmt::Function { name, pure, .. } => { known.insert(name.clone(), Procedure { pure: *pure, intents: None, optional: vec![] }); }
                Stmt::Subroutine { name, pure, params, body, .. } => { known.insert(name.clone(), Procedure { pure: *pure, intents: Some(intents(params, body)), optional: params.iter().map(|p| body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == p && intent == "optional"))).collect() }); }
                Stmt::Module { body, .. } => procedures(body, known),
                _ => {}
            }
        }
    }
    fn expression(e: &Expr, locals: &HashSet<String>, known: &HashMap<String, Procedure>) -> Result<()> {
        match e {
            Expr::Call(_, _) if crate::named_arguments::omitted(e) => {},
            Expr::Call(name, args) => {
                let intrinsic = matches!(name.as_str(), "allocated" | "shape") || name.starts_with("@keyword:") || crate::intrinsics::math_arity(name).is_some() || crate::inquiries::numeric_model(name) || crate::bit_intrinsics::keys(name).is_some()
                    || matches!(name.to_ascii_lowercase().as_str(), "@integer_literal" | "minloc" | "maxloc" | "findloc" | "@minloc" | "@maxloc" | "@findloc" | "pack" | "unpack" | "spread" | "eoshift" | "transpose" | "cshift" | "reshape" | "mod" | "modulo" | "abs" | "int" | "real" | "dble" | "sum" | "product" | "minval" | "maxval" | "max" | "min" | "kind" | "selected_real_kind" | "selected_int_kind" | "tiny" | "huge" | "size" | "lbound" | "ubound" | "len" | "len_trim" | "ichar" | "iachar" | "char" | "achar" | "index" | "scan" | "verify" | "trim" | "adjustl" | "adjustr" | "repeat" | "present" | "any" | "all" | "count" | "@count_i8" | "@count_default" | "@any_reduction" | "@all_reduction" | "@array_constructor" | "@literal_substring" | "@output_implied_do" | "@section_subscript");
                if !locals.contains(name) && !known.get(name).map(|p| p.pure).unwrap_or(intrinsic) {
                    bail!("PURE procedure cannot call impure or unknown procedure `{name}`");
                }
                for arg in args { expression(arg, locals, known)?; }
            }
            Expr::Index(_, args) => for arg in args { expression(arg, locals, known)?; },
            Expr::Bin(_, a, b) => { expression(a, locals, known)?; expression(b, locals, known)?; }
            Expr::Un(_, a) => expression(a, locals, known)?,
            Expr::Section { lower, upper, stride, .. } => {
                for e in [lower, upper, stride].into_iter().flatten() { expression(e, locals, known)?; }
            }
            _ => {}
        }
        Ok(())
    }
    fn check(body: &[Stmt], inherited: &HashSet<String>, readonly: &HashSet<String>, known: &HashMap<String, Procedure>) -> Result<()> {
        let mut locals = inherited.clone();
        for stmt in body {
            match stmt {
                Stmt::CallSub {name,..} if name=="@pointer_associate" || name=="@nullify" => bail!("pointer association in PURE procedures is not yet supported"),
                Stmt::VarDecl { names, .. } => locals.extend(names.iter().cloned()),
                Stmt::ArrayDecl { name, .. } => { locals.insert(name.clone()); }
                Stmt::InitializedDecl { attributes, entities, .. } => {
                    if !attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) {
                        bail!("PURE procedure cannot have saved or declaration-initialized local variables");
                    }
                    locals.extend(entities.iter().map(|(name, _)| name.clone()));
                }
                _ => {}
            }
        }
        let writable = |name: &str| -> Result<()> {
            if readonly.contains(name) { bail!("PURE procedure cannot modify dummy argument `{name}`"); }
            if !locals.contains(name) { bail!("PURE procedure cannot modify nonlocal variable `{name}`; declare local variables explicitly"); }
            Ok(())
        };
        for stmt in body {
            match stmt {
                Stmt::Print { .. } | Stmt::Read { .. } => bail!("PURE procedure cannot perform external I/O"),
                Stmt::CallSub {name,..} if name == "@empty_read" => bail!("PURE procedure cannot perform external I/O"),
                Stmt::CallSub {name,..} if name=="@stop" => bail!("PURE procedure cannot execute STOP"),
                Stmt::CallSub {name,args} if name == "@internal_read" => {
                    expression(&args[0],&locals,known)?;
                    let has_status=matches!(&args[1],Expr::Str(key) if !key.is_empty());
                    for arg in args.iter().skip(if has_status {2} else {3}) {
                        let Expr::Ident(dest)=arg else {bail!("internal READ currently requires scalar variable destinations");};
                        writable(dest)?;
                    }
                }
                Stmt::CallSub {name,..} if matches!(name.as_str(), "@formatted_read" | "@formatted_read_status" | "@read_record" | "@read_record_status" | "@write") || name.starts_with("@io_") || name.starts_with("@file_") => bail!("PURE procedure cannot perform external I/O"),
                Stmt::CallSub {name,args} if name == "@where" || name == "@elsewhere" => {
                    for e in args { expression(e,&locals,known)?; }
                }
                Stmt::CallSub {name,args} if name == "@do_step" => {
                    if let Expr::Ident(var) = &args[0] { writable(var)?; }
                    for e in &args[1..] { expression(e,&locals,known)?; }
                }
                Stmt::CallSub {name,args} if name == "@forall" => {
                    for control in args.chunks_exact(4) { for e in &control[1..] {expression(e,&locals,known)?;} }
                }
                Stmt::CallSub {name,args} if name == "@allocate" => {
                    // Allocation is permitted for local storage and writable
                    // allocatable dummies, while host and INTENT(IN) objects
                    // remain protected by the ordinary PURE write checks.
                    for arg in args {
                        let Expr::Call(array,bounds)=arg else {bail!("ALLOCATE expects array bounds");};
                        writable(array)?;
                        for bound in bounds {expression(bound,&locals,known)?;}
                    }
                }
                Stmt::CallSub {name,args} if name=="@deallocate" => {
                    for object in crate::allocatables::deallocation_targets(args)? {writable(&object)?;}
                }
                Stmt::CallSub { name, args } => {
                    let Some(Procedure { pure: true, intents: Some(intents), optional }) = known.get(name) else {
                        bail!("PURE procedure cannot call impure or unknown subroutine `{name}`");
                    };
                    if args.len() > intents.len() || optional.iter().skip(args.len()).any(|o| !o) { bail!("wrong argument count for subroutine `{name}`"); }
                    for (i, (arg, intent)) in args.iter().zip(intents).enumerate() {
                        if crate::named_arguments::omitted(arg) {
                            if !optional[i] { bail!("missing required argument of subroutine `{name}`"); }
                            continue;
                        }
                        expression(arg, &locals, known)?;
                        if intent != "intent(in)" {
                            match arg {
                                Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) => writable(n)?,
                                _ => bail!("output argument of PURE subroutine `{name}` must be writable"),
                            }
                        }
                    }
                },
                Stmt::Assign { name, value } => { writable(name)?; expression(value, &locals, known)?; }
                Stmt::AssignIndex { name, indices, value } => {
                    writable(name)?; expression(value, &locals, known)?;
                    for e in indices { expression(e, &locals, known)?; }
                }
                Stmt::Return(Some(e)) => expression(e, &locals, known)?,
                Stmt::If { cond, then_body, else_body } => {
                    expression(cond, &locals, known)?; check(then_body, &locals, readonly, known)?;
                    if let Some(body) = else_body { check(body, &locals, readonly, known)?; }
                }
                Stmt::Do { var, start, end, body } => {
                    writable(var)?; expression(start, &locals, known)?; expression(end, &locals, known)?;
                    check(body, &locals, readonly, known)?;
                }
                Stmt::DoWhile { cond, body } => { expression(cond, &locals, known)?; check(body, &locals, readonly, known)?; }
                Stmt::Block { body } => check(body, &locals, readonly, known)?,
                Stmt::SelectCase { expr, cases, default } => {
                    expression(expr, &locals, known)?;
                    for case in cases { check(&case.body, &locals, readonly, known)?; }
                    if let Some(body) = default { check(body, &locals, readonly, known)?; }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn walk(body: &[Stmt], known: &HashMap<String, Procedure>) -> Result<()> {
        for stmt in body {
            match stmt {
                Stmt::Function { name, pure: true, elemental, return_type, params, body, .. } => {
                    if *elemental { crate::elemental::validate(name,params,return_type,body)?; }
                    let mut locals: HashSet<String> = params.iter().cloned().collect();
                    locals.insert(name.clone());
                    check(body, &locals, &params.iter().cloned().collect(), known)?;
                }
                Stmt::Subroutine { name, pure: true, elemental, params, body, .. } => {
                    if *elemental { crate::elemental::validate_subroutine(name,params,body)?; }
                    let declared = intents(params, body);
                    if declared.iter().any(|i| !matches!(i.as_str(), "intent(in)" | "intent(out)" | "intent(inout)")) {
                        bail!("PURE subroutine dummy arguments require an explicit INTENT");
                    }
                    let readonly = params.iter().zip(&declared).filter(|(_, i)| i.as_str() == "intent(in)").map(|(p, _)| p.clone()).collect();
                    check(body, &params.iter().cloned().collect(), &readonly, known)?;
                }
                Stmt::Module { body, .. } => walk(body, known)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut known = HashMap::new();
    procedures(&program.body, &mut known);
    walk(&program.body, &known)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pure_allocation_protects_inputs_and_host_storage() {
        for (declarations,expected) in [
            ("integer,allocatable,intent(out)::a(:)",None),
            ("integer,allocatable,intent(in)::a(:)",Some("dummy argument")),
        ] {
            let source=format!("pure recursive subroutine f(a)\n{declarations}\nallocate(a(2))\nend subroutine f");
            let p=crate::parser::parse_with_src(&source,&crate::lexer::lex(&source),"pure.f90").unwrap();
            let result=super::validate(&p);
            if let Some(error)=expected {assert!(result.unwrap_err().to_string().contains(error));} else {result.unwrap();}
        }
        let source="module m\ninteger,allocatable::outside(:)\ncontains\npure subroutine f()\nallocate(outside(2))\nend subroutine f\nend module m";
        let p=crate::parser::parse_with_src(source,&crate::lexer::lex(source),"pure.f90").unwrap();
        assert!(super::validate(&p).unwrap_err().to_string().contains("nonlocal variable"));
    }
    #[test]
    fn validates_pure_subroutine_intents_and_calls() {
        for (statement, error) in [
            ("i=3\nj=i", "dummy argument"),
            ("print *, i\nj=i", "external I/O"),
            ("outside=3\nj=i", "nonlocal variable"),
            ("call impure(j)", "impure or unknown subroutine"),
            ("call helper(i)", "dummy argument"),
            ("call helper(outside)", "nonlocal variable"),
        ] {
            let source = format!("pure subroutine f(i,j)\ninteger,intent(in) :: i\ninteger,intent(out) :: j\n{statement}\nend subroutine f\npure subroutine helper(x)\ninteger,intent(out) :: x\nx=7\nend subroutine helper");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "pure_sub.f90").unwrap();
            assert!(super::validate(&p).unwrap_err().to_string().contains(error), "{source}");
        }
        let source = include_str!("../tests/43_pure_subroutine.f90");
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "pure_sub.f90").unwrap();
        super::validate(&p).unwrap();
        let source = "pure subroutine f(i)\ninteger :: i\ni=1\nend subroutine f";
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "pure_sub.f90").unwrap();
        assert!(super::validate(&p).unwrap_err().to_string().contains("explicit INTENT"));
    }

    #[test]
    fn validates_pure_function_side_effects() {
        for (decl, statement, error) in [
            ("", "print *, i\nj=i", "external I/O"),
            ("", "read(*,*) j", "external I/O"),
            ("", "write(*,*) i", "external I/O"),
            ("", "write(*,*)", "external I/O"),
            ("", "stop", "cannot execute STOP"),
            ("", "write(6,'(i3)') i", "external I/O"),
            ("character(len=8) :: text", "read(*,'(a)') text", "external I/O"),
            ("", "i=3\nj=i", "dummy argument"),
            ("character(len=8) :: text", "text='42'\nread(text,*) i", "dummy argument"),
            ("", "outside=3\nj=i", "nonlocal variable"),
            ("real :: x", "call random_number(x)\nj=i", "subroutine"),
            ("", "j=unknown(i)", "unknown procedure"),
            ("integer :: x=1", "j=x+i", "initialized local"),
            ("integer, save :: x", "x=i\nj=x", "saved"),
            ("", "j=impure(i)", "impure"),
        ] {
            let source = format!("pure function f(i) result(j)\ninteger, intent(in) :: i\ninteger :: j\n{decl}\n{statement}\nend function f\nfunction impure(i)\ninteger :: i,impure\nimpure=i\nend function impure");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "pure.f90").unwrap();
            let result = super::validate(&p).unwrap_err().to_string();
            assert!(result.contains(error), "{result}");
        }
        let source = include_str!("../tests/40_pure_function.f90");
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "pure.f90").unwrap();
        super::validate(&p).unwrap();
    }
}

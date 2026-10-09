//! Expand locally defined scalar output-argument subroutines into scoped calls.
//! The existing object ABI passes values; expansion preserves writable argument storage.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone)]
struct Routine { params: Vec<String>, body: Vec<Stmt> }
type Names = HashMap<String, String>;
fn name(n: &mut String, names: &Names) { if let Some(new) = names.get(n) { *n = new.clone(); } }
fn expr(e: &mut Expr, names: &Names) {
    match e {
        Expr::Ident(n) => name(n, names),
        Expr::Index(n, args) | Expr::Call(n, args) => { name(n, names); for a in args { expr(a, names); } }
        Expr::Bin(_, a, b) => { expr(a, names); expr(b, names); }
        Expr::Un(_, a) => expr(a, names),
        Expr::Section { name: n, lower, upper, stride } => {
            name(n, names); for e in [lower, upper, stride].into_iter().flatten() { expr(e, names); }
        }
        _ => {}
    }
}
fn rename(body: &mut Vec<Stmt>, inherited: &Names, params: &[String], serial: &mut usize) -> Result<()> {
    let mut names = inherited.clone();
    for s in body.iter() {
        let declared = match s {
            Stmt::VarDecl { names, .. } => names.clone(),
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            _ => vec![],
        };
        for n in declared {
            if !params.contains(&n) { *serial += 1; names.insert(n, format!("@sub_local_{serial}")); }
        }
    }
    for s in body.iter_mut() {
        match s {
            Stmt::VarDecl { names: declared, .. } => {
                declared.retain(|n| !params.contains(n));
                for n in declared { name(n, &names); }
            }
            Stmt::ArrayDecl { name: n, dims, .. } => { name(n, &names); for e in dims { expr(e, &names); } }
            Stmt::Assign { name: n, value } => { name(n, &names); expr(value, &names); }
            Stmt::AssignIndex { name: n, indices, value } => {
                name(n, &names); for e in indices { expr(e, &names); } expr(value, &names);
            }
            Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => for e in items { expr(e, &names); },
            Stmt::If { cond, then_body, else_body } => {
                expr(cond, &names); rename(then_body, &names, params, serial)?;
                if let Some(body) = else_body { rename(body, &names, params, serial)?; }
            }
            Stmt::Do { var, start, end, body } => {
                name(var, &names); expr(start, &names); expr(end, &names); rename(body, &names, params, serial)?;
            }
            Stmt::DoWhile { cond, body } => { expr(cond, &names); rename(body, &names, params, serial)?; }
            Stmt::Block { body } => rename(body, &names, &[], serial)?,
            Stmt::SelectCase { expr: e, cases, default } => {
                expr(e, &names);
                for case in cases { rename(&mut case.body, &names, params, serial)?; }
                if let Some(body) = default { rename(body, &names, params, serial)?; }
            }
            Stmt::Return(_) => bail!("explicit RETURN in an output-argument subroutine is not yet supported"),
            _ => {}
        }
    }
    body.retain(|s| !matches!(s, Stmt::DummyIntent { .. }) && !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
    Ok(())
}
fn expand(body: &mut Vec<Stmt>, routines: &HashMap<String, Routine>, active: &mut Vec<String>, serial: &mut usize) -> Result<()> {
    for stmt in body {
        if let Stmt::CallSub { name: callee, args } = stmt {
            if let Some(routine) = routines.get(callee) {
                let intents: HashMap<_, _> = routine.body.iter().filter_map(|s| match s {
                    Stmt::DummyIntent { name, intent } => Some((name.clone(), intent.clone())), _ => None,
                }).collect();
                if intents.values().any(|i| i == "intent(out)" || i == "intent(inout)") {
                    if active.contains(callee) { bail!("recursive output-argument subroutine `{callee}` is not yet supported"); }
                    if args.len() != routine.params.len() { bail!("wrong argument count for subroutine `{callee}`"); }
                    let types: HashMap<String, TypeSpec> = routine.body.iter().flat_map(|s| match s {
                        Stmt::VarDecl { names, kind } => names.iter().map(|n| (n.clone(), kind.clone())).collect(),
                        _ => Vec::new(),
                    }).collect();
                    let mut prefix = Vec::new();
                    let mut undefine = Vec::new();
                    let mut names = Names::new();
                    for (param, actual) in routine.params.iter().zip(args.iter()) {
                        let kind = types.get(param).with_context(|| format!("output-argument calls currently require explicitly declared scalar dummy `{param}`"))?;
                        let intent = intents.get(param).map(String::as_str).unwrap_or("");
                        if intent == "intent(in)" {
                            *serial += 1;
                            let temp = format!("@sub_input_{serial}");
                            prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![temp.clone()] });
                            prefix.push(Stmt::Assign { name: temp.clone(), value: actual.clone() });
                            names.insert(param.clone(), temp);
                        } else {
                            let Expr::Ident(actual) = actual else { bail!("writable argument `{param}` of `{callee}` must currently be a scalar variable"); };
                            names.insert(param.clone(), actual.clone());
                            if intent == "intent(out)" {
                                undefine.push(Stmt::CallSub { name: "@undefine".into(), args: vec![Expr::Ident(actual.clone())] });
                            }
                        }
                    }
                    let mut expanded = routine.body.clone();
                    rename(&mut expanded, &names, &routine.params, serial)?;
                    active.push(callee.clone());
                    expand(&mut expanded, routines, active, serial)?;
                    active.pop();
                    prefix.extend(undefine); prefix.extend(expanded);
                    *stmt = Stmt::Block { body: prefix };
                    continue;
                }
            }
        }
        match stmt {
            Stmt::If { then_body, else_body, .. } => {
                expand(then_body, routines, active, serial)?;
                if let Some(body) = else_body { expand(body, routines, active, serial)?; }
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body }
            | Stmt::Module { body, .. } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => expand(body, routines, active, serial)?,
            Stmt::SelectCase { cases, default, .. } => {
                for case in cases { expand(&mut case.body, routines, active, serial)?; }
                if let Some(body) = default { expand(body, routines, active, serial)?; }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn resolve(mut program: Program) -> Result<Program> {
    fn collect(body: &[Stmt], routines: &mut HashMap<String, Routine>) {
        for s in body {
            match s {
                Stmt::Subroutine { name, params, body, .. } => { routines.insert(name.clone(), Routine { params: params.clone(), body: body.clone() }); }
                Stmt::Module { body, .. } => collect(body, routines),
                _ => {}
            }
        }
    }
    let mut routines = HashMap::new(); collect(&program.body, &mut routines);
    expand(&mut program.body, &routines, &mut Vec::new(), &mut 0)?;
    Ok(program)
}

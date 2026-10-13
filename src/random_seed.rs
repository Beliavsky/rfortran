// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Two default INTEGER words serialize the shared 64-bit random state.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
pub fn resolve(mut p: Program) -> Result<Program> {
    fn collect(body: &[Stmt], procedures: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                    procedures.insert(name.clone());
                    collect(body, procedures);
                }
                Stmt::Module { body, .. } | Stmt::Block { body } => collect(body, procedures),
                _ => {}
            }
        }
    }
    fn scope(
        body: &mut Vec<Stmt>,
        inherited: &HashMap<String, (TypeSpec, usize)>,
        inherited_readonly: &HashSet<String>,
        procedures: &HashSet<String>,
        pure: bool,
        types: &crate::operators::ExpressionTypes,
    ) -> Result<()> {
        let mut vars = inherited.clone();
        let mut readonly = inherited_readonly.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        readonly.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    readonly.remove(name);
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (n, _) in entities {
                        readonly.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        vars.insert(n.clone(), (kind.clone(), 0));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    vars.insert(name.clone(), (kind.clone(), dims.len()));
                }
                Stmt::InitializedDecl {
                    kind,
                    entities,
                    attributes,
                } => {
                    for (name, _) in entities {
                        vars.entry(name.clone())
                            .or_insert_with(|| (kind.clone(), 0));
                        if attributes.iter().any(|a| a == "parameter") {
                            readonly.insert(name.clone());
                        }
                    }
                }
                Stmt::DummyIntent { name, intent }
                    if matches!(intent.as_str(), "parameter" | "intent(in)") =>
                {
                    readonly.insert(name.clone());
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(offset) = &s {
                location = Some(*offset);
            }
            (||->Result<()> {match &mut s {
                Stmt::CallSub {name,args} if name=="random_seed" && !procedures.contains(name)=>{
                    if pure {bail!("RANDOM_SEED is not permitted in a PURE procedure");}
                    let signature=crate::named_arguments::intrinsic(name).unwrap();
                    signature.reorder(name,args)?;
                    let present=args.iter().enumerate().filter(|(_,e)|!crate::named_arguments::omitted(e)).collect::<Vec<_>>();
                    if present.len()>1 {bail!("RANDOM_SEED permits at most one of SIZE, PUT and GET");}
                    if let Some((slot,arg))=present.first() {
                        let keyword=["SIZE","PUT","GET"][*slot];
                        if *slot==1 {
                            if !matches!(types.kind_rank(arg,&vars)?,Some((TypeSpec::Integer(None|Some(4)),1))) {
                                bail!("RANDOM_SEED PUT requires a default INTEGER rank-one array expression");
                            }
                            // Defer materialization until array specialization has supplied
                            // bounded storage and runtime extents for allocatable inputs.
                            s=Stmt::CallSub {name:"@random_seed_put_array".into(),args:vec![(*arg).clone()]};
                            return Ok(());
                        }
                        if *slot==0 {
                            if let Expr::Index(variable,indices)|Expr::Call(variable,indices)=arg {
                                let (kind,rank)=vars.get(variable).context("RANDOM_SEED SIZE requires a scalar default INTEGER variable")?;
                                if !matches!(kind,TypeSpec::Integer(None|Some(4))) || *rank==0 || indices.len()!=*rank
                                    || indices.iter().any(|e|matches!(e,Expr::Ident(n) if vars.get(n).is_some_and(|(_,rank)|*rank!=0))) {
                                    bail!("RANDOM_SEED SIZE requires a scalar default INTEGER variable");
                                }
                                if readonly.contains(variable) {bail!("RANDOM_SEED SIZE requires a writable variable");}
                                s=Stmt::AssignIndex {name:variable.clone(),indices:indices.clone(),value:Expr::IntLit("2".into())};
                                return Ok(());
                            }
                        }
                        let Expr::Ident(variable)=arg else {bail!("RANDOM_SEED {keyword} requires a default INTEGER {} variable",if *slot==0 {"scalar"} else {"rank-one array"});};
                        let (kind,rank)=vars.get(variable).context("undeclared RANDOM_SEED argument")?;
                        if !matches!(kind,TypeSpec::Integer(None|Some(4))) || *rank!=if *slot==0 {0} else {1} {bail!("RANDOM_SEED {keyword} requires a default INTEGER {} variable",if *slot==0 {"scalar"} else {"rank-one array"});}
                        if *slot!=1 && readonly.contains(variable) {bail!("RANDOM_SEED {keyword} requires a writable variable");}
                        if *slot==0 {s=Stmt::Assign {name:variable.clone(),value:Expr::IntLit("2".into())};}
                        else {
                            let source=Expr::Ident(variable.clone());
                            let first=Expr::Call("lbound".into(),vec![source.clone(),Expr::IntLit("1".into())]);
                            let second=Expr::Bin(crate::ast::BinOp::Add,Box::new(first.clone()),Box::new(Expr::IntLit("1".into())));
                            let words=vec![Expr::Index(variable.clone(),vec![first]),Expr::Index(variable.clone(),vec![second])];
                            out.push(Stmt::If {cond:Expr::Bin(crate::ast::BinOp::Lt,Box::new(Expr::Call("size".into(),vec![source])),Box::new(Expr::IntLit("2".into()))),then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("RANDOM_SEED {keyword} array must have at least 2 elements"))]}],else_body:None});
                            s=Stmt::CallSub {name:if *slot==1 {"@random_seed_put"} else {"@random_seed_get"}.into(),args:words};
                        }
                    }
                },
                Stmt::Function {body,pure,..}|Stmt::Subroutine {body,pure,..}=>scope(body,&vars,&readonly,procedures,*pure,types)?,
                Stmt::Module {body,..}|Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..}=>scope(body,&vars,&readonly,procedures,pure,types)?,
                Stmt::If {then_body,else_body,..}=>{scope(then_body,&vars,&readonly,procedures,pure,types)?;if let Some(body)=else_body {scope(body,&vars,&readonly,procedures,pure,types)?;}},
                Stmt::SelectCase {cases,default,..}=>{for c in cases {scope(&mut c.body,&vars,&readonly,procedures,pure,types)?;}if let Some(body)=default {scope(body,&vars,&readonly,procedures,pure,types)?;}},_=>{}
            }Ok(())})().map_err(|error|crate::source_diagnostics::at(error,location))?;
            out.push(s);
        }
        *body = out;
        Ok(())
    }
    let mut procedures = HashSet::new();
    collect(&p.body, &mut procedures);
    let types = crate::operators::ExpressionTypes::new(&p)?;
    scope(
        &mut p.body,
        &HashMap::new(),
        &HashSet::new(),
        &procedures,
        false,
        &types,
    )?;
    Ok(p)
}

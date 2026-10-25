// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Deferred CHARACTER component lengths accompanying ordinary array descriptors.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn set(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn length(e: &Expr, lengths: &HashMap<String, String>) -> Expr {
    match e {
        Expr::Ident(n) | Expr::Index(n, _) if lengths.contains_key(n) => id(&lengths[n]),
        Expr::Call(n, a) if n == "@array_constructor" && !a.is_empty() => length(&a[0], lengths),
        Expr::Bin(BinOp::Concat, a, b) => bin(BinOp::Add, length(a, lengths), length(b, lengths)),
        _ => Expr::Call("len".into(), vec![e.clone()]),
    }
}
pub fn resolve(mut p: Program) -> Result<Program> {
    fn scope(
        body: &mut Vec<Stmt>,
        inherited: &HashMap<String, String>,
        inherited_ranks: &HashMap<String, usize>,
        serial: &mut usize,
    ) -> Result<()> {
        let mut lengths = inherited.clone();
        let mut ranks = inherited_ranks.clone();
        for s in body.iter() {
            if let Stmt::ArrayDecl { name, dims, .. } = s {
                ranks.insert(name.clone(), dims.len());
            }
        }
        fn array(
            e: &Expr,
            ranks: &HashMap<String, usize>,
            lengths: &HashMap<String, String>,
        ) -> bool {
            match e {
                Expr::Ident(n) => ranks.contains_key(n) || lengths.contains_key(n),
                Expr::Call(n, _) if n == "@array_constructor" || n == "@typed_constructor" => true,
                Expr::Section { .. } => true,
                Expr::Bin(_, a, b) => array(a, ranks, lengths) || array(b, ranks, lengths),
                Expr::Call(n, a) if matches!(n.as_str(), "adjustl" | "adjustr" | "merge") => {
                    a.iter().any(|e| array(e, ranks, lengths))
                }
                _ => false,
            }
        }
        for s in body.iter() {
            if let Stmt::DummyIntent { name, intent } = s {
                if let Some(n) = intent.strip_prefix("@deferred_array_length:") {
                    lengths.insert(name.clone(), n.into());
                }
            }
        }
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            let mut prefix = vec![];
            let mut suffix = vec![];
            match &mut s {
                Stmt::Assign { name, value }
                    if lengths.contains_key(name) && array(value, &ranks, &lengths) =>
                {
                    let width = length(value, &lengths);
                    *serial += 1;
                    let saved = format!("@deferred_new_length_{serial}");
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![saved.clone()],
                    });
                    prefix.push(set(&saved, width));
                    prefix.push(Stmt::If{cond:bin(BinOp::Gt,id(&saved),int(4096)),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("deferred CHARACTER array element exceeds 4096-character temporary capacity".into())]}],else_body:None});
                    // Evaluate into distinct storage before changing the destination
                    // descriptor, including self-assignment with a new element length.
                    let temp = format!("@deferred_rhs_{serial}");
                    prefix.push(Stmt::ArrayDecl {
                        kind: TypeSpec::Character(Some(4096)),
                        name: temp.clone(),
                        dims: vec![id("@deferred_extent"); *ranks.get(name).unwrap_or(&1)],
                    });
                    prefix.push(Stmt::DummyIntent {
                        name: temp.clone(),
                        intent: "allocatable".into(),
                    });
                    prefix.push(Stmt::DummyIntent {
                        name: temp.clone(),
                        intent: format!("@character_spec_length:{saved}"),
                    });
                    if !matches!(value,Expr::Call(n,_) if n=="@array_constructor" || n=="@typed_constructor")
                    {
                        fn source(
                            e: &Expr,
                            ranks: &HashMap<String, usize>,
                            lengths: &HashMap<String, String>,
                        ) -> Expr {
                            if let Expr::Bin(_, a, b) = e {
                                return source(
                                    if array(a, ranks, lengths) { a } else { b },
                                    ranks,
                                    lengths,
                                );
                            }
                            e.clone()
                        }
                        let source = source(value, &ranks, &lengths);
                        let dims = (1..=*ranks.get(name).unwrap_or(&1))
                            .map(|axis| {
                                Expr::Call("size".into(), vec![source.clone(), int(axis as i64)])
                            })
                            .collect();
                        prefix.push(Stmt::CallSub {
                            name: "@allocate".into(),
                            args: vec![Expr::Call(temp.clone(), dims)],
                        });
                    }
                    prefix.push(set(&temp, value.clone()));
                    *value = id(&temp);
                    suffix.push(Stmt::CallSub {
                        name: "@deallocate".into(),
                        args: vec![id(&temp)],
                    });
                    prefix.push(Stmt::If {
                        cond: bin(
                            BinOp::And,
                            Expr::Call("allocated".into(), vec![id(name)]),
                            bin(BinOp::Ne, id(&lengths[name]), id(&saved)),
                        ),
                        then_body: vec![Stmt::CallSub {
                            name: "@deallocate".into(),
                            args: vec![id(name)],
                        }],
                        else_body: None,
                    });
                    prefix.push(set(&lengths[name], id(&saved)));
                }
                Stmt::CallSub { name, args } if name == crate::substrings::ALLOCATE_CHARACTER => {
                    let width = Expr::Call("max".into(), vec![int(0), args[0].clone()]);
                    let mut objects = vec![];
                    for object in &args[1..] {
                        let (Expr::Call(n, _) | Expr::Index(n, _)) = object else {
                            continue;
                        };
                        if let Some(l) = lengths.get(n) {
                            prefix.push(set(l, width.clone()));
                            prefix.push(Stmt::If{cond:bin(BinOp::Gt,id(l),int(4096)),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("deferred CHARACTER array element exceeds 4096-character temporary capacity".into())]}],else_body:None});
                            objects.push(object.clone());
                        }
                    }
                    if !objects.is_empty() {
                        if objects.len() != args.len() - 1 {
                            bail!(
                                "mixed scalar and array CHARACTER allocation is not yet supported"
                            );
                        }
                        *name = "@allocate".into();
                        *args = objects;
                    }
                }
                _ => {}
            }
            for child in crate::procedure_arguments::children(&mut s) {
                scope(child, &lengths, &ranks, serial)?;
            }
            out.extend(prefix);
            out.push(s);
            out.extend(suffix);
        }
        *body = out;
        Ok(())
    }
    scope(&mut p.body, &HashMap::new(), &HashMap::new(), &mut 0)?;
    Ok(p)
}

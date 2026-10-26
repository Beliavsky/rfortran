// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Scalar C interoperability, with explicit adapters around rfort's private ABI.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
pub const BIND: &str = "@bind_c";
pub const IMPORT: &str = "@c_import";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arg {
    pub kind: TypeSpec,
    pub value: bool,
    pub input: bool,
    pub output: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signature {
    pub symbol: String,
    pub args: Vec<Arg>,
    pub result: Option<TypeSpec>,
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn attr(body: &[Stmt], n: &str, a: &str) -> bool {
    body.iter()
        .any(|s| matches!(s,Stmt::DummyIntent{name,intent} if name==n&&intent==a))
}
fn kinds(body: &[Stmt]) -> HashMap<String, TypeSpec> {
    body.iter()
        .flat_map(|s| match s {
            Stmt::VarDecl { kind, names } => {
                names.iter().map(|n| (n.clone(), kind.clone())).collect()
            }
            Stmt::InitializedDecl { kind, entities, .. } => entities
                .iter()
                .map(|(n, _)| (n.clone(), kind.clone()))
                .collect(),
            _ => vec![],
        })
        .collect()
}
pub fn module_body() -> Vec<Stmt> {
    let long = if cfg!(windows) {
        4
    } else {
        std::mem::size_of::<std::os::raw::c_long>()
    };
    [
        ("c_signed_char", 1),
        ("c_short", 2),
        ("c_int", 4),
        ("c_long", long),
        ("c_long_long", 8),
        ("c_int8_t", 1),
        ("c_int16_t", 2),
        ("c_int32_t", 4),
        ("c_int64_t", 8),
        ("c_size_t", std::mem::size_of::<usize>()),
        ("c_intptr_t", std::mem::size_of::<usize>()),
        ("c_int_least8_t", 1),
        ("c_int_least16_t", 2),
        ("c_int_least32_t", 4),
        ("c_int_least64_t", 8),
        ("c_float", 4),
        ("c_double", 8),
    ]
    .into_iter()
    .map(|(n, k)| Stmt::InitializedDecl {
        kind: TypeSpec::Integer(None),
        attributes: vec!["parameter".into()],
        entities: vec![(n.into(), Some(Expr::IntLit(k.to_string())))],
    })
    .collect()
}
pub fn bind_procedure(mut s: Stmt, binding: Option<String>) -> Stmt {
    if let Some(symbol) = binding {
        match &mut s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => body.insert(
                0,
                Stmt::DummyIntent {
                    name: BIND.into(),
                    intent: if symbol.is_empty() {
                        name.clone()
                    } else {
                        symbol
                    },
                },
            ),
            _ => {}
        }
    }
    s
}
pub fn binding_tokens(tokens: &mut Vec<crate::lexer::Token>, input: &str) {
    use crate::lexer::TokenKind as K;
    let mut i = 0;
    while i < tokens.len() {
        if !matches!(&tokens[i].kind,K::Ident(n) if n=="bind") {
            i += 1;
            continue;
        }
        // BIND is a contextual suffix, and remains a legal procedure/dummy name.
        let suffix = i > 0 && matches!(tokens[i - 1].kind, K::RParen)
            || i >= 2
                && matches!(tokens[i - 2].kind, K::KwSubroutine)
                && matches!(tokens[i - 1].kind, K::Ident(_));
        if !suffix {
            i += 1;
            continue;
        }
        let mut first = i;
        while first > 0
            && crate::lexer::same_statement(
                &input[tokens[first - 1].span.end..tokens[first].span.start],
            )
        {
            first -= 1;
        }
        if !tokens[first..i]
            .iter()
            .any(|t| matches!(t.kind, K::KwFunction | K::KwSubroutine))
        {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < tokens.len() && !matches!(tokens[end].kind, K::RParen) {
            end += 1;
        }
        let a = &tokens[i + 1..end.min(tokens.len())];
        let word = |k: &K, n: &str| matches!(k,K::Ident(s) if s==n);
        let valid = a.len() >= 2 && matches!(a[0].kind, K::LParen) && word(&a[1].kind, "c");
        let symbol = if valid && a.len() == 2 {
            Some(String::new())
        } else if valid
            && a.len() == 6
            && matches!(a[2].kind, K::Comma)
            && word(&a[3].kind, "name")
            && matches!(a[4].kind, K::Eq)
        {
            if let K::Str(s) = &a[5].kind {
                (!s.is_empty() && !s.contains('\0')).then(|| s.clone())
            } else {
                None
            }
        } else {
            None
        };
        let Some(symbol) = symbol.filter(|_| end < tokens.len()) else {
            tokens[i].kind = K::Error("expected BIND(C) or BIND(C, NAME=\"symbol\")".into());
            i += 1;
            continue;
        };
        let span = tokens[i].span.start..tokens[end].span.end;
        tokens.splice(
            i..=end,
            [crate::lexer::Token {
                kind: K::BindSpec(symbol),
                span,
            }],
        );
        let result_clause = tokens.get(i + 1).is_some_and(|next|
            crate::lexer::same_statement(&input[tokens[i].span.end..next.span.start]))
            && matches!(tokens.get(i + 2).map(|t| &t.kind), Some(K::LParen));
        if let Some(next) = tokens.get_mut(i + 1) {
            if result_clause && matches!(&next.kind,K::Ident(n) if n=="result") {
                next.kind = K::KwResult;
            }
        }
        i += 1;
    }
}
pub fn interface_declaration(encoded: &str) -> std::result::Result<Vec<Stmt>, &'static str> {
    let Some(src) = encoded.strip_prefix("c_interface|") else {
        return Ok(crate::ast::procedure_declaration(encoded));
    };
    let (base, src) = src.split_once('|').ok_or("invalid C interface encoding")?;
    let base = base
        .parse::<usize>()
        .map_err(|_| "invalid C interface location")?;
    let source = src
        .split_inclusive('\n')
        .map(|l| {
            if l.trim_start()
                .to_ascii_lowercase()
                .strip_prefix("import")
                .is_some_and(|tail| {
                    tail.is_empty()
                        || tail
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_whitespace() || c == ':')
                })
            {
                l.bytes()
                    .map(|b| {
                        if matches!(b, b'\r' | b'\n') {
                            b as char
                        } else {
                            ' '
                        }
                    })
                    .collect()
            } else {
                l.to_owned()
            }
        })
        .collect::<String>();
    let src = source.as_str();
    let mut p = crate::parser::parse_with_src(src, &crate::lexer::lex(src), "<C interface>")
        .map_err(|_| "invalid BIND(C) interface body")?;
    fn offsets(body: &mut Vec<Stmt>, base: usize) {
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                *n += base;
            }
            for child in crate::procedure_arguments::children(s) {
                offsets(child, base);
            }
        }
    }
    offsets(&mut p.body, base);
    let mut out = vec![];
    for mut s in p.body {
        let function = matches!(s, Stmt::Function { .. });
        match &mut s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                if body
                    .iter()
                    .any(|s| matches!(s,Stmt::DummyIntent{name,..} if name==BIND))
                {
                    body.insert(
                        0,
                        Stmt::DummyIntent {
                            name: IMPORT.into(),
                            intent: "true".into(),
                        },
                    );
                    out.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: if function {
                            "@interface:function"
                        } else {
                            "@interface:subroutine"
                        }
                        .into(),
                    });
                    out.push(s);
                } else {
                    out.extend(crate::ast::procedure_declaration(&format!(
                        "interface|{name}={}",
                        if function { "function" } else { "subroutine" }
                    )));
                }
            }
            Stmt::SourceLocation(_) => {}
            _ => return Err("C interface bodies must contain procedure declarations"),
        }
    }
    Ok(out)
}
/// Build imported procedures as ordinary Fortran adapters. Local temporaries
/// keep output-only C arguments separate from the caller's initialization state.
pub fn prepare(mut p: Program) -> Result<Program> {
    // The implementation below handles both kinds without modifying their ABI.
    fn routines(body: &mut Vec<Stmt>) -> Result<()> {
        for s in body {
            let function = matches!(s, Stmt::Function { .. });
            let location = match &s {
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body
                    .iter()
                    .find_map(crate::source_diagnostics::statement_offset),
                _ => None,
            };
            (|| ->Result<()> {match s {
                Stmt::Function{name,params,body,..}|Stmt::Subroutine{name,params,body,..}=> {
                    if let Some(symbol)=body.iter().find_map(|s|match s {Stmt::DummyIntent{name,intent} if name==BIND=>Some(intent.clone()),_=>None}) {
                        if body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name))) {bail!("BIND(C) array dummies are not yet supported; use scalar numeric arguments");}
                        let ks=kinds(body);
                        let result=if function {Some(body.iter().rev().find_map(|s|match s {Stmt::Return(Some(Expr::Ident(n)))=>Some(n.clone()),_=>None}).unwrap_or_else(||name.clone()))}else{None};
                        let args=params.iter().map(|n|Ok(Arg{kind:ks.get(n).cloned().with_context(||format!("BIND(C) dummy `{n}` requires an explicit numeric type"))?,value:attr(body,n,"value"),input:!attr(body,n,"intent(out)"),output:!attr(body,n,"value")&&!attr(body,n,"intent(in)")})).collect::<Result<Vec<_>>>()?;
                        if params.iter().any(|n|attr(body,n,"value")&&(attr(body,n,"intent(out)")||attr(body,n,"intent(inout)"))) {bail!("VALUE is incompatible with INTENT(OUT) or INTENT(INOUT)");}
                        if params.iter().any(|n|["optional","pointer","allocatable"].iter().any(|a|attr(body,n,a))) {bail!("BIND(C) OPTIONAL, POINTER and ALLOCATABLE dummies are not yet supported");}
                        let result_kind=result.as_ref().map(|n|ks.get(n).cloned().context("BIND(C) function result requires an explicit numeric type")).transpose()?;
                        if args.iter().any(|a|matches!(a.kind,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed|TypeSpec::Logical|TypeSpec::LogicalKind(_)|TypeSpec::Complex(_))) || result_kind.as_ref().is_some_and(|k|matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed|TypeSpec::Logical|TypeSpec::LogicalKind(_)|TypeSpec::Complex(_))) {bail!("BIND(C) currently supports scalar INTEGER and REAL arguments and results; this type is not yet supported");}
                        let signature=Signature{symbol,args,result:result_kind};
                        body.insert(0,Stmt::DummyIntent{name:"@c_definition".into(),intent:serde_json::to_string(&(signature,params.clone(),result))?});
                    }
                    if attr(body,IMPORT,"true") {
                        let original=body.clone();let ks=kinds(&original);
                        let symbol=original.iter().find_map(|s|match s {Stmt::DummyIntent{name,intent} if name==BIND=>Some(intent.clone()),_=>None}).unwrap();
                        let result=if function {Some(original.iter().rev().find_map(|s|match s {Stmt::Return(Some(Expr::Ident(n)))=>Some(n.clone()),_=>None}).unwrap_or_else(||name.clone()))}else{None};
                        let mut args=vec![];let mut values=vec![];let mut before=vec![];let mut after=vec![];
                        for (i,n) in params.iter().enumerate() {
                            let kind=ks.get(n).cloned().with_context(||format!("BIND(C) dummy `{n}` requires an explicit numeric type"))?;
                            let value=attr(&original,n,"value");let input=!attr(&original,n,"intent(out)");let output=!value&&!attr(&original,n,"intent(in)");
                            let temp=format!("@c_argument_{i}");
                            before.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});
                            if input {before.push(Stmt::Assign{name:temp.clone(),value:id(n)});}
                            if output {after.push(Stmt::Assign{name:n.clone(),value:id(&temp)});}
                            values.push(id(&temp));args.push(Arg{kind,value,input,output});
                        }
                        let result_kind=result.as_ref().map(|n|ks.get(n).cloned().context("BIND(C) function result requires an explicit numeric type")).transpose()?;
                        let signature=Signature{symbol,args,result:result_kind};
                        let mut call=vec![Expr::Str(serde_json::to_string(&signature)?)];call.extend(values);
                        if let Some(n)=&result {call.push(id(n));}
                        // Preserve declarations, USE and interface metadata; remove the old terminal RETURN.
                        body.retain(|s|!matches!(s,Stmt::Return(_)));
                        body.extend(before);body.push(Stmt::CallSub{name:"@c_invoke".into(),args:call});body.extend(after);
                        if let Some(n)=result {body.push(Stmt::Return(Some(id(&n))));}
                    }
                    if !attr(body,IMPORT,"true") && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,..} if name==BIND)) {
                        let original=body.clone();let ks=kinds(&original);
                        for n in params.iter().filter(|n|attr(&original,n,"value")&&!attr(&original,n,"intent(in)")) {
                            let copy=format!("@c_value${n}");
                            crate::procedure_arguments::rename(body,n,&copy);
                            body.retain(|s|!matches!(s,Stmt::DummyIntent{name,intent} if name==&copy&&(intent=="value"||intent.starts_with("intent("))));
                            body.insert(0,Stmt::VarDecl{kind:ks[n].clone(),names:vec![n.clone()]});
                            body.insert(1,Stmt::DummyIntent{name:n.clone(),intent:"value".into()});
                            // Insert the copy after declarations, before any executable source statements.
                            let at=body.iter().position(|s|matches!(s,Stmt::Assign{..}|Stmt::AssignIndex{..}|Stmt::CallSub{..}|Stmt::If{..}|Stmt::Do{..}|Stmt::Print{..}|Stmt::Return(_))).unwrap_or(body.len());
                            body.insert(at,Stmt::Assign{name:copy,value:id(n)});
                        }
                    }
                    routines(body)?;
                },_=>for b in crate::procedure_arguments::children(s) {routines(b)?;},
            }Ok(())})().map_err(|e|crate::source_diagnostics::at(e,location))?;
        }
        Ok(())
    }
    routines(&mut p.body)?;
    Ok(p)
}
fn supported(k: &TypeSpec) -> bool {
    matches!(
        k,
        TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)) | TypeSpec::Real | TypeSpec::DoublePrecision
    )
}
/// Resolve adapter metadata after kind and module association, and generate
/// the existing by-reference entry point for each C-exported procedure.
pub fn resolve(mut p: Program) -> Result<Program> {
    let existing: std::collections::HashSet<_> = p
        .body
        .iter()
        .filter_map(|s| match s {
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    fn invocations(body: &mut Vec<Stmt>, ks: &HashMap<String, TypeSpec>) -> Result<()> {
        for s in body {
            if let Stmt::CallSub { name, args } = s {
                if name == "@c_invoke" {
                    let Expr::Str(encoded) = &args[0] else {
                        unreachable!()
                    };
                    let mut sig: Signature = serde_json::from_str(encoded)?;
                    for (arg, actual) in sig.args.iter_mut().zip(&args[1..]) {
                        let Expr::Ident(n) = actual else {
                            bail!("C adapter requires local numeric storage")
                        };
                        arg.kind = ks
                            .get(n)
                            .cloned()
                            .context("missing C adapter argument type")?;
                    }
                    if sig.result.is_some() {
                        let Some(Expr::Ident(n)) = args.last() else {
                            unreachable!()
                        };
                        sig.result = Some(
                            ks.get(n)
                                .cloned()
                                .context("missing C adapter result type")?,
                        );
                    }
                    validate(&sig)?;
                    args[0] = Expr::Str(serde_json::to_string(&sig)?);
                }
            }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                for b in crate::procedure_arguments::children(s) {
                    invocations(b, ks)?;
                }
            }
        }
        Ok(())
    }
    fn walk(
        body: &mut Vec<Stmt>,
        out: &mut Vec<Stmt>,
        existing: &std::collections::HashSet<String>,
        seen: &mut HashMap<String, String>,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let ks = kinds(body);
                    invocations(body, &ks)?;
                    if !attr(body, IMPORT, "true") {
                        if let Some(encoded) = body.iter().find_map(|s| match s {
                            Stmt::DummyIntent { name, intent } if name == "@c_definition" => {
                                Some(intent.clone())
                            }
                            _ => None,
                        }) {
                            let (mut sig, original_params, result): (
                                Signature,
                                Vec<String>,
                                Option<String>,
                            ) = serde_json::from_str(&encoded)?;
                            for (arg, n) in sig.args.iter_mut().zip(&original_params) {
                                arg.kind =
                                    ks.get(n).cloned().context("missing BIND(C) dummy type")?;
                            }
                            if let Some(n) = result {
                                sig.result =
                                    ks.get(&n).or_else(|| ks.get("@external_result")).cloned();
                                if sig.result.is_none() {
                                    bail!("missing BIND(C) function result type");
                                }
                            }
                            validate(&sig)?;
                            let native = if name.starts_with(crate::external_calls::PREFIX) {
                                name.clone()
                            } else {
                                format!("{}{name}", crate::external_calls::PREFIX)
                            };
                            if let Some(previous) = seen.get(&sig.symbol) {
                                if previous != &native {
                                    bail!("duplicate C binding label `{}`", sig.symbol);
                                }
                            } else {
                                seen.insert(sig.symbol.clone(), native.clone());
                                let marker = Stmt::CallSub {
                                    name: "@c_export".into(),
                                    args: vec![Expr::Str(serde_json::to_string(&sig)?)],
                                };
                                if name == &native {
                                    body.insert(0, marker);
                                } else if !existing.contains(&native) {
                                    let mut entry = crate::external_calls::c_entry(
                                        name,
                                        params,
                                        body,
                                        sig.result.as_ref(),
                                    )?;
                                    let Stmt::Subroutine { body, .. } = &mut entry else {
                                        unreachable!()
                                    };
                                    body.insert(0, marker);
                                    out.push(entry);
                                } else {
                                    // The initial external-call pass already made this entry.
                                    out.push(Stmt::DummyIntent {
                                        name: native,
                                        intent: serde_json::to_string(&sig)?,
                                    });
                                }
                            }
                        }
                    }
                    walk(body, out, existing, seen)?;
                }
                _ => {
                    for b in crate::procedure_arguments::children(s) {
                        walk(b, out, existing, seen)?;
                    }
                }
            }
        }
        Ok(())
    }
    let mut added = vec![];
    walk(&mut p.body, &mut added, &existing, &mut HashMap::new())?;
    for s in added {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(Stmt::Subroutine { body, .. }) = p
                .body
                .iter_mut()
                .find(|s| matches!(s,Stmt::Subroutine{name:n,..} if n==&name))
            {
                body.insert(
                    0,
                    Stmt::CallSub {
                        name: "@c_export".into(),
                        args: vec![Expr::Str(intent)],
                    },
                );
            }
        } else {
            p.body.push(s);
        }
    }
    Ok(p)
}
fn validate(s: &Signature) -> Result<()> {
    if s.args.iter().any(|a| !supported(&a.kind))
        || s.result.as_ref().is_some_and(|k| !supported(k))
    {
        bail!("BIND(C) currently supports scalar INTEGER kinds 1, 2, 4, 8 and REAL kinds 4, 8; CHARACTER, LOGICAL, COMPLEX and derived types are not yet supported");
    }
    Ok(())
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Numeric C interoperability, with explicit adapters around rfort's private ABI.
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
    #[serde(default)]
    pub dims: Vec<Expr>,
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
            Stmt::ArrayDecl { kind, name, .. } => vec![(name.clone(), kind.clone())],
            _ => vec![],
        })
        .collect()
}
pub fn assumed_extent(e: &Expr) -> bool {
    matches!(e,Expr::Ident(n) if n=="@assumed_extent")
        || matches!(e,Expr::Call(n,a) if n=="@declared_bounds" && a.get(1).is_some_and(assumed_extent))
}
pub fn assumed_size(dims: &[Expr]) -> bool {
    dims.last().is_some_and(assumed_extent)
}
fn interoperable_dimensions(dims: &[Expr]) -> bool {
    dims.iter().enumerate().all(|(i,d)| match d {
        Expr::Ident(n) if n=="@deferred_extent" => false,
        Expr::Call(n,a) if n=="@declared_bounds" => a.len()==2 && !matches!(&a[1],Expr::Ident(n) if n=="@deferred_extent") && (!assumed_extent(d) || i+1==dims.len()),
        _ => !assumed_extent(d) || i+1==dims.len(),
    })
}
fn dimensions(body: &[Stmt], n: &str) -> Vec<Expr> {
    body.iter().find_map(|s| match s {
        Stmt::ArrayDecl { name, dims, .. } if name == n => Some(dims.clone()),
        _ => None,
    }).unwrap_or_default()
}

fn association_extent(d: &Expr) -> Expr {
    use crate::ast::BinOp;
    if let Expr::Call(tag,a)=d {
        if tag=="@declared_bounds" {
            if assumed_extent(&a[1]) {return a[1].clone();}
            let difference=Expr::Bin(BinOp::Sub,Box::new(a[1].clone()),Box::new(a[0].clone()));
            let extent=Expr::Bin(BinOp::Add,Box::new(difference),Box::new(Expr::IntLit("1".into())));
            return Expr::Call("max".into(),vec![extent,Expr::IntLit("0".into())]);
        }
    }
    d.clone()
}

// Serialize bounds using parameter positions, so module/procedure renaming
// cannot leave stale names in backend metadata.
fn bound(e: &Expr, params: &[String], args: &[Arg]) -> Result<Expr> {
    use crate::ast::{BinOp, UnOp};
    Ok(match e {
        Expr::Ident(n) if n=="@assumed_extent" => e.clone(),
        Expr::IntLit(_) => e.clone(),
        Expr::Ident(n) => {
            let i = params.iter().position(|p| p == n)
                .with_context(|| format!("unsupported BIND(C) array bound `{n}`"))?;
            if !args[i].dims.is_empty() || !args[i].input || !matches!(args[i].kind, TypeSpec::Integer(_)) {
                bail!("BIND(C) array bounds require input scalar INTEGER arguments");
            }
            Expr::Ident(i.to_string())
        }
        Expr::Bin(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div), a, b) =>
            Expr::Bin(op.clone(), Box::new(bound(a, params, args)?), Box::new(bound(b, params, args)?)),
        Expr::Un(UnOp::Neg, a) => Expr::Un(UnOp::Neg, Box::new(bound(a, params, args)?)),
        _ => bail!("unsupported BIND(C) array bound expression"),
    })
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
                        for n in params.iter() {
                            let ds=dimensions(body,n);
                            if !ds.is_empty() && attr(body,n,"value") {bail!("VALUE is not allowed on BIND(C) array dummies");}
                            if !interoperable_dimensions(&ds) {bail!("BIND(C) array dummies require explicit shape or assumed size; assumed-shape arrays are not yet supported");}
                        }
                        let ks=kinds(body);
                        let result=if function {Some(body.iter().rev().find_map(|s|match s {Stmt::Return(Some(Expr::Ident(n)))=>Some(n.clone()),_=>None}).unwrap_or_else(||name.clone()))}else{None};
                        let args=params.iter().map(|n|Ok(Arg{kind:ks.get(n).cloned().with_context(||format!("BIND(C) dummy `{n}` requires an explicit numeric type"))?,value:attr(body,n,"value"),input:!attr(body,n,"intent(out)"),output:!attr(body,n,"value")&&!attr(body,n,"intent(in)"),dims:dimensions(body,n)})).collect::<Result<Vec<_>>>()?;
                        if params.iter().any(|n|attr(body,n,"value")&&(attr(body,n,"intent(out)")||attr(body,n,"intent(inout)"))) {bail!("VALUE is incompatible with INTENT(OUT) or INTENT(INOUT)");}
                        if params.iter().any(|n|["optional","pointer","allocatable"].iter().any(|a|attr(body,n,a))) {bail!("BIND(C) OPTIONAL, POINTER and ALLOCATABLE dummies are not yet supported");}
                        if result.as_ref().is_some_and(|n|!dimensions(body,n).is_empty()) {bail!("BIND(C) function results must be scalar");}
                        let result_kind=result.as_ref().map(|n|ks.get(n).cloned().context("BIND(C) function result requires an explicit numeric type")).transpose()?;
                        if args.iter().any(|a|matches!(a.kind,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed|TypeSpec::Logical|TypeSpec::LogicalKind(_)|TypeSpec::Complex(_))) || result_kind.as_ref().is_some_and(|k|matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed|TypeSpec::Logical|TypeSpec::LogicalKind(_)|TypeSpec::Complex(_))) {bail!("BIND(C) currently supports INTEGER and REAL arguments and scalar results; this type is not yet supported");}
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
                            let dims=dimensions(&original,n);
                            if !dims.is_empty() {
                                values.push(id(n));args.push(Arg{kind,value,input,output,dims});continue;
                            }
                            let temp=format!("@c_argument_{i}");
                            before.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});
                            if input {before.push(Stmt::Assign{name:temp.clone(),value:id(n)});}
                            if output {after.push(Stmt::Assign{name:n.clone(),value:id(&temp)});}
                            values.push(id(&temp));args.push(Arg{kind,value,input,output,dims});
                        }
                        let result_kind=result.as_ref().map(|n|ks.get(n).cloned().context("BIND(C) function result requires an explicit numeric type")).transpose()?;
                        let signature=Signature{symbol,args,result:result_kind};
                        let mut call=vec![Expr::Str(serde_json::to_string(&signature)?)];call.extend(values);
                        if let Some(n)=&result {call.push(id(n));}
                        // Imported C receives a pointer to the first element;
                        // dummy lower bounds have no indexing role in this stub.
                        // Association checks compare extents, not bound syntax.
                        for s in body.iter_mut() {
                            if let Stmt::ArrayDecl{name,dims,..}=s {
                                if params.contains(name) {
                                    for d in dims {*d=association_extent(d);}
                                }
                            }
                        }
                        // Preserve declarations, USE and interface metadata; remove the old terminal RETURN.
                        body.retain(|s|!matches!(s,Stmt::Return(_)));
                        for n in params.iter().filter(|n|attr(&original,n,"value")) {
                            body.push(Stmt::DummyIntent{name:n.clone(),intent:"intent(in)".into()});
                        }
                        body.extend(before);body.push(Stmt::CallSub{name:"@c_invoke".into(),args:call});body.extend(after);
                        if let Some(n)=result {body.push(Stmt::Return(Some(id(&n))));}
                    }
                    if !attr(body,IMPORT,"true") && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,..} if name==BIND)) {
                        let original=body.clone();let ks=kinds(&original);
                        for n in params.iter().filter(|n|attr(&original,n,"value")&&!attr(&original,n,"intent(in)")) {
                            let copy=format!("@c_value${n}");
                            let bounds=body.iter().filter_map(|s| match s {Stmt::ArrayDecl{name,dims,..}=>Some((name.clone(),dims.clone())),_=>None}).collect::<HashMap<_,_>>();
                            crate::procedure_arguments::rename(body,n,&copy);
                            for s in body.iter_mut() {if let Stmt::ArrayDecl{name,dims,..}=s {if let Some(original)=bounds.get(name) {*dims=original.clone();}}}
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
    fn invocations(body: &mut Vec<Stmt>, inherited: &HashMap<String, TypeSpec>) -> Result<()> {
        let mut ks=inherited.clone();ks.extend(kinds(body));
        let mut out=vec![];
        for mut stmt in std::mem::take(body) {
            let s=&mut stmt;
            if let Stmt::CallSub { name, args } = s {
                if name == "@c_invoke" {
                    let Expr::Str(encoded) = &args[0] else {
                        unreachable!()
                    };
                    let mut sig: Signature = serde_json::from_str(encoded)?;
                    let original=args.clone();
                    let mut passed=vec![];
                    for (i,(arg, actual)) in sig.args.iter_mut().zip(&original[1..]).enumerate() {
                        let Expr::Ident(n) = actual else {
                            bail!("C adapter requires local numeric storage")
                        };
                        arg.kind = ks
                            .get(n)
                            .cloned()
                            .context("missing C adapter argument type")?;
                        if arg.dims.is_empty() {passed.push(actual.clone());} else {
                            let fields=(0..4).map(|j|format!("{n}$c_{i}_{j}")).collect::<Vec<_>>();
                            out.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:fields.clone()});
                            let mut bind=fields.iter().map(|n|id(n)).collect::<Vec<_>>();bind.push(actual.clone());
                            out.push(Stmt::CallSub{name:"@c_array_bind".into(),args:bind});
                            passed.extend(fields.iter().map(|n|id(n)));
                        }
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
                    if sig.result.is_some() {passed.push(original.last().unwrap().clone());}
                    *args=vec![Expr::Str(serde_json::to_string(&sig)?)];args.extend(passed);
                }
            }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                for b in crate::procedure_arguments::children(s) {
                    invocations(b, &ks)?;
                }
            }
            out.push(stmt);
        }
        *body=out;
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
                    // Resolve contained C interfaces before cloning this
                    // definition into a native export entry point.
                    walk(body, out, existing, seen)?;
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
                            for (i,n) in original_params.iter().enumerate() {
                                sig.args[i].dims=dimensions(body,n);
                            }
                            let resolved=sig.args.clone();
                            for arg in &mut sig.args {
                                arg.dims=arg.dims.iter().map(|d| match d {
                                    Expr::Call(tag,a) if tag=="@declared_bounds" => Ok(Expr::Call(tag.clone(),vec![bound(&a[0],&original_params,&resolved)?,bound(&a[1],&original_params,&resolved)?])),
                                    _ => bound(d,&original_params,&resolved),
                                }).collect::<Result<_>>()?;
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
    invocations(&mut p.body,&HashMap::new())?;
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
        bail!("BIND(C) currently supports INTEGER kinds 1, 2, 4, 8 and REAL kinds 4, 8; CHARACTER, LOGICAL, COMPLEX and derived types are not yet supported");
    }
    Ok(())
}

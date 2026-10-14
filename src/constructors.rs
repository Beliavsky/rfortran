// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve delayed implied-DOs after procedure specialization. Runtime-bound
//! whole-array assignments build a bounded snapshot before any destination write.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value,
    }
}
fn fail(cond: Expr, message: &str) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message.into())],
        }],
        else_body: None,
    }
}
pub(crate) fn delayed(e: &Expr) -> bool {
    match e {
        Expr::Call(n, _) if n == "@output_implied_do" => true,
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(delayed),
        Expr::Bin(_, a, b) => delayed(a) || delayed(b),
        Expr::Un(_, a) => delayed(a),
        _ => false,
    }
}

// The current array ABI uses bounded backing storage plus an active length.
// Prove a finite constructor capacity without replacing its runtime controls.
type Ranges = HashMap<String, (i64, i64)>;
fn range(e: &Expr, values: &Ranges, shapes: &Shapes) -> Option<(i64,i64)> {
    if let Ok(n)=crate::parameters::integer_constant(e) {return Some((n,n));}
    match e {
        Expr::Ident(n)=>values.get(n).copied(),
        Expr::Call(n,a) if n=="size" && a.len()==1 && !shapes.contains_key("@constructor_procedure:size") => {
            let Expr::Ident(n)=&a[0] else {return None};
            let dims=&shapes.get(n)?.1;
            if dims.is_empty() {return None;}
            let size=dims.iter().try_fold(1i64,|n,d|n.checked_mul(crate::parameters::integer_constant(d).ok()?.max(0)))?;
            Some((size,size))
        },
        Expr::Bin(op,a,b)=>{
            let (alo,ahi)=range(a,values,shapes)?;
            let (blo,bhi)=range(b,values,shapes)?;
            let pairs=[(alo,blo),(alo,bhi),(ahi,blo),(ahi,bhi)];
            let products=pairs.map(|(a,b)|match op {
                BinOp::Add=>a.checked_add(b),BinOp::Sub=>a.checked_sub(b),BinOp::Mul=>a.checked_mul(b),_=>None,
            });
            let products=products.into_iter().collect::<Option<Vec<_>>>()?;
            Some((*products.iter().min()?,*products.iter().max()?))
        },
        _=>None,
    }
}
fn capacity(items: &[Expr], values: &Ranges, shapes: &Shapes) -> Option<i64> {
    items.iter().try_fold(0i64,|sum,item|{
        let count=match item {
            Expr::Call(n,a) if n=="@array_constructor"=>capacity(a,values,shapes)?,
            Expr::Call(n,a) if n=="@output_implied_do" && a.len()>=5=>{
                let Expr::Ident(index)=&a[0] else {return None};
                let (lo_min,lo_max)=range(&a[1],values,shapes)?;
                let (hi_min,hi_max)=range(&a[2],values,shapes)?;
                let step=crate::parameters::integer_constant(&a[3]).ok()?;
                if step==0 {return None;}
                let distance=if step>0 {i128::from(hi_max)-i128::from(lo_min)} else {i128::from(lo_max)-i128::from(hi_min)};
                let iterations=if distance<0 {0} else {i64::try_from(distance/i128::from(step).abs()+1).ok()?};
                let mut local=values.clone();local.insert(index.clone(),(lo_min.min(hi_min),lo_max.max(hi_max)));
                iterations.checked_mul(capacity(&a[4..],&local,shapes)?)?
            },
            _ if array_item(item,shapes)=>return None,
            _=>1,
        };
        sum.checked_add(count)
    })
}
fn item_kind(items: &[Expr], shapes: &Shapes) -> Result<TypeSpec> {
    let mut result=None;
    for item in items {
        let kind=match item {
            Expr::Call(n,a) if n=="@array_constructor"=>item_kind(a,shapes)?,
            Expr::Call(n,a) if n=="@output_implied_do"=>item_kind(&a[4..],shapes)?,
            _=>crate::reductions::elemental_element(item,&int(1),shapes)?.2,
        };
        let kind=if kind==TypeSpec::Integer(Some(4)) {TypeSpec::Integer(None)} else {kind};
        if result.as_ref().is_some_and(|old|old!=&kind) {bail!("array constructor elements must have the same type and kind");}
        result=Some(kind);
    }
    result.context("runtime constructor requires typed elements")
}
pub(crate) fn prepare_arguments(mut p: Program) -> Result<Program> {
    fn procedures(body:&[Stmt], shapes:&mut Shapes) {for s in body {match s {
        Stmt::Function {name,body,..}|Stmt::Subroutine {name,body,..}=>{
            shapes.insert(format!("@constructor_procedure:{name}"),(TypeSpec::Logical,vec![]));procedures(body,shapes);
        },
        Stmt::Module {body,..}|Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..}=>procedures(body,shapes),
        Stmt::If {then_body,else_body,..}=>{procedures(then_body,shapes);if let Some(body)=else_body {procedures(body,shapes);}},_=>{}
    }}}
    fn scope(body: &mut Vec<Stmt>, inherited: &Shapes, serial: &mut usize) -> Result<()> {
        let mut shapes=inherited.clone();
        for s in body.iter() {match s {
            Stmt::ArrayDecl {kind,name,dims}=>{shapes.insert(name.clone(),(kind.clone(),dims.clone()));},
            Stmt::VarDecl {kind,names}=>for n in names {shapes.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
        }}
        // Pointer aliases can invalidate an otherwise known scalar value.
        let aliases=inherited.contains_key("@constructor_aliases") || body.iter().any(|s|matches!(s,Stmt::DummyIntent{intent,..} if matches!(intent.as_str(),"pointer"|"target")));
        if aliases {shapes.insert("@constructor_aliases".into(),(TypeSpec::Logical,vec![]));}
        let mut values=Ranges::new();let mut out=vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::Assign {name,value} if !aliases=>{
                    let known=range(value,&values,&shapes);
                    // Unknown expressions may contain calls with side effects.
                    if let Some(known)=known {values.insert(name.clone(),known);} else {values.clear();}
                },
                Stmt::CallSub {args,..}=>{
                    // A call in another actual could change a control variable.
                    fn calls(e:&Expr,shapes:&Shapes)->bool {match e {
                        Expr::Call(n,a) if matches!(n.as_str(),"@array_constructor"|"@output_implied_do"|"@integer_literal")
                            || (!shapes.contains_key(&format!("@constructor_procedure:{n}"))
                                && (matches!(n.as_str(),"real"|"dble"|"int"|"size") || crate::intrinsics::math_arity(n).is_some()))
                            =>a.iter().any(|e|calls(e,shapes)),
                        Expr::Call(..)=>true,Expr::Bin(_,a,b)=>calls(a,shapes)||calls(b,shapes),Expr::Un(_,a)=>calls(a,shapes),_=>false,
                    }}
                    if args.iter().any(|e|calls(e,&shapes)) {values.clear();}
                    for arg in args {
                        if !matches!(arg,Expr::Call(n,_) if n=="@array_constructor") || !delayed(arg) {continue;}
                        let Expr::Call(_,items)=arg else {unreachable!()};
                        let Some(bound)=capacity(items,&values,&shapes) else {continue;};
                        let kind=item_kind(items,&shapes)?;
                        if !matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical) {continue;}
                        *serial+=1;let temp=format!("@constructor_actual_{serial}");let cursor=format!("@constructor_actual_length_{serial}");
                        out.push(Stmt::ArrayDecl {kind:kind.clone(),name:temp.clone(),dims:vec![int(bound)]});
                        out.push(Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec![cursor.clone()]});
                        out.push(assign(&cursor,int(0)));
                        out.extend(Builder {shapes:&shapes,temp:temp.clone(),cursor:cursor.clone(),capacity:bound,serial}.items(items,&HashMap::new())?);
                        shapes.insert(temp.clone(),(kind,vec![int(bound)]));
                        *arg=Expr::Call("@packed_view".into(),vec![id(&temp),id(&cursor)]);
                    }
                    values.clear();
                },
                Stmt::Module {body,..}|Stmt::Function {body,..}|Stmt::Subroutine {body,..}|Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..}=>{
                    scope(body,&shapes,serial)?;values.clear();
                },
                Stmt::If {then_body,else_body,..}=>{
                    scope(then_body,&shapes,serial)?;if let Some(body)=else_body {scope(body,&shapes,serial)?;}values.clear();
                },
                Stmt::VarDecl {..}|Stmt::ArrayDecl {..}|Stmt::DummyIntent {..}|Stmt::SourceLocation(_)=>{},
                _=>values.clear(),
            }
            out.push(s);
        }
        *body=out;Ok(())
    }
    let mut inherited=Shapes::new();
    procedures(&p.body,&mut inherited);
    scope(&mut p.body,&inherited,&mut 0)?;Ok(p)
}
fn rename(e: &mut Expr, names: &HashMap<String, String>) {
    match e {
        Expr::Ident(n) => {
            if let Some(value) = names.get(n) {
                *n = value.clone();
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                rename(e, names);
            }
        }
        Expr::Bin(_, a, b) => {
            rename(a, names);
            rename(b, names);
        }
        Expr::Un(_, a) => rename(a, names),
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                rename(e, names);
            }
        }
        _ => {}
    }
}
fn expression(e: &mut Expr, shapes: &Shapes) -> Result<()> {
    match e {
        Expr::Call(n, args) if n == "size" => {
            if let Some(value) = crate::allocatables::inquiry(n, args, shapes)? {
                *e = value;
                return Ok(());
            }
            if let Some(Expr::Ident(name)) = args.first() {
                if let Some((_, dims)) = shapes.get(name) {
                    if !dims.is_empty() {
                        let value = match args.as_slice() {
                            [_] => dims
                                .iter()
                                .cloned()
                                .reduce(|a, b| bin(BinOp::Mul, a, b))
                                .unwrap(),
                            [_, dim] => {
                                let dim = crate::parameters::integer_constant(dim)?;
                                if dim < 1 || dim as usize > dims.len() {
                                    bail!("SIZE dimension is out of range");
                                }
                                dims[dim as usize - 1].clone()
                            }
                            _ => bail!("SIZE expects ARRAY and optional DIM"),
                        };
                        *e = value;
                        return Ok(());
                    }
                }
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                expression(e, shapes)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expression(a, shapes)?;
            expression(b, shapes)?;
        }
        Expr::Un(_, a) => expression(a, shapes)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                expression(e, shapes)?;
            }
        }
        _ => {}
    }
    crate::parameters::expand_constructor(e)
}
pub(crate) fn fixed_expression(e:&mut Expr,shapes:&Shapes)->Result<()> {expression(e,shapes)}
fn array_item(e: &Expr, shapes: &Shapes) -> bool {
    match e {
        Expr::Ident(n) => shapes.get(n).is_some_and(|(_, dims)| !dims.is_empty()),
        Expr::Section { name, .. } => shapes.get(name).is_some_and(|(_, dims)| !dims.is_empty()),
        Expr::Call(n, _)
            if matches!(
                n.as_str(),
                "@matrix_view"
                    | "@packed_view"
                    | "reshape"
                    | "pack"
                    | "unpack"
                    | "transpose"
                    | "cshift"
                    | "eoshift"
                    | "spread"
            ) =>
        {
            true
        }
        Expr::Bin(_, a, b) => array_item(a, shapes) || array_item(b, shapes),
        Expr::Un(_, a) => array_item(a, shapes),
        _ => false,
    }
}
struct Builder<'a> {
    shapes: &'a Shapes,
    temp: String,
    cursor: String,
    capacity: i64,
    serial: &'a mut usize,
}
impl Builder<'_> {
    fn items(&mut self, items: &[Expr], names: &HashMap<String, String>) -> Result<Vec<Stmt>> {
        let mut out = vec![];
        for item in items {
            if let Expr::Call(tag, args) = item {
                if tag == "@array_constructor" {
                    out.extend(self.items(args, names)?);
                    continue;
                }
                if tag == "@output_implied_do" {
                    let Some(Expr::Ident(source)) = args.first() else {
                        bail!("invalid constructor implied-DO index");
                    };
                    let (kind, dims) = self
                        .shapes
                        .get(source)
                        .context("constructor implied-DO index must be declared")?;
                    if !dims.is_empty() || !matches!(kind, TypeSpec::Integer(_)) {
                        bail!("constructor implied-DO index must be scalar INTEGER");
                    }
                    let mut bounds = args[1..4].to_vec();
                    for e in &mut bounds {
                        rename(e, names);
                    }
                    if !bounds
                        .iter()
                        .all(|e| crate::output::integer(e, self.shapes))
                    {
                        // Generated outer indices are not in the original scope yet.
                        let mut scope = self.shapes.clone();
                        for (source, name) in names {
                            scope.insert(name.clone(), self.shapes[source].clone());
                        }
                        if !bounds.iter().all(|e| crate::output::integer(e, &scope)) {
                            bail!(
                                "constructor implied-DO bounds must be scalar INTEGER expressions"
                            );
                        }
                    }
                    let step=crate::parameters::integer_constant(&bounds[2]).context("runtime-bound constructor implied-DO stride must currently be a constant INTEGER")?;
                    if step == 0 {
                        bail!("constructor implied-DO stride must not be zero");
                    }
                    *self.serial += 1;
                    let control = format!("@constructor_control_{}", self.serial);
                    let local = format!("@constructor_value_{}", self.serial);
                    let mut nested = names.clone();
                    nested.insert(source.clone(), local.clone());
                    let mut body = vec![assign(&local, id(&control))];
                    body.extend(self.items(&args[4..], &nested)?);
                    // The wide control avoids overflow at the default INTEGER limit;
                    // the separate local preserves the declared index kind in items.
                    out.push(Stmt::Block {
                        body: vec![
                            Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: vec![control.clone()],
                            },
                            Stmt::VarDecl {
                                kind: kind.clone(),
                                names: vec![local],
                            },
                            Stmt::Block {
                                body: std::iter::once(Stmt::CallSub {
                                    name: "@do_step".into(),
                                    args: vec![
                                        id(&control),
                                        bounds[0].clone(),
                                        bounds[1].clone(),
                                        int(step),
                                    ],
                                })
                                .chain(body)
                                .collect(),
                            },
                        ],
                    });
                    continue;
                }
            }
            if array_item(item, self.shapes) {
                bail!("runtime constructor items must currently be scalar");
            }
            let mut value = item.clone();
            rename(&mut value, names);
            out.push(fail(
                bin(BinOp::Ge, id(&self.cursor), int(self.capacity)),
                "constructor shape does not conform: too many elements",
            ));
            out.push(assign(
                &self.cursor,
                bin(BinOp::Add, id(&self.cursor), int(1)),
            ));
            out.push(Stmt::AssignIndex {
                name: self.temp.clone(),
                indices: vec![id(&self.cursor)],
                value,
            });
        }
        Ok(out)
    }
}
fn body(stmts: &mut Vec<Stmt>, inherited: &Shapes, serial: &mut usize) -> Result<()> {
    let mut shapes = inherited.clone();
    for stmt in stmts.iter() {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            _ => {}
        }
    }
    crate::allocatables::metadata(stmts, &mut shapes);
    let mut source_offset = None;
    for stmt in stmts.iter_mut() {
        if let Stmt::SourceLocation(offset) = stmt {
            source_offset = Some(*offset);
        }
        (||->Result<()> {
        match stmt {
            Stmt::Assign { name, value } => {
                expression(value, &shapes)?;
                if matches!(value,Expr::Call(tag,_) if tag=="@array_constructor") && delayed(value)
                {
                    let (kind, dims) = shapes
                        .get(name)
                        .context("runtime constructor destination must be an array")?;
                    if dims.len() != 1
                        || !matches!(
                            kind,
                            TypeSpec::Integer(_)
                                | TypeSpec::Real
                                | TypeSpec::DoublePrecision
                                | TypeSpec::Logical
                        )
                    {
                        bail!("runtime-bound constructors currently require a rank-one numeric or LOGICAL whole-array destination");
                    }
                    let capacity = crate::parameters::integer_constant(&dims[0])
                        .context("runtime constructor destination requires bounded storage")?;
                    if capacity < 0 {
                        bail!("runtime constructor destination extent must be nonnegative");
                    }
                    let extent =
                        crate::allocatables::length(name, &shapes).unwrap_or(int(capacity));
                    *serial += 1;
                    let temp = format!("@runtime_constructor_{serial}");
                    let cursor = format!("@runtime_constructor_length_{serial}");
                    let copy = format!("@runtime_constructor_copy_{serial}");
                    let Expr::Call(_, items) = value else {
                        unreachable!()
                    };
                    let mut b = vec![
                        Stmt::ArrayDecl {
                            kind: kind.clone(),
                            name: temp.clone(),
                            dims: vec![int(capacity)],
                        },
                        Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![cursor.clone(), copy.clone()],
                        },
                        assign(&cursor, int(0)),
                    ];
                    b.extend(
                        Builder {
                            shapes: &shapes,
                            temp: temp.clone(),
                            cursor: cursor.clone(),
                            capacity,
                            serial,
                        }
                        .items(items, &HashMap::new())?,
                    );
                    b.push(fail(
                        bin(BinOp::Ne, id(&cursor), extent),
                        "constructor shape does not conform",
                    ));
                    b.push(Stmt::Do {
                        var: copy.clone(),
                        start: int(1),
                        end: id(&cursor),
                        body: vec![Stmt::AssignIndex {
                            name: name.clone(),
                            indices: vec![id(&copy)],
                            value: Expr::Index(temp, vec![id(&copy)]),
                        }],
                    });
                    *stmt = Stmt::Block { body: b };
                } else if delayed(value) {
                    if matches!(value,Expr::Call(n,_) if n=="@output_implied_do") {bail!("implied-DO assignment requires an array constructor");}
                    bail!("runtime-bound constructor implied-DO currently requires whole-array assignment");
                }
            }
            Stmt::AssignIndex { indices, value, .. } => {
                for e in indices {
                    expression(e, &shapes)?;
                }
                expression(value, &shapes)?;
                if delayed(value) {
                    bail!("runtime-bound constructor implied-DO currently requires whole-array assignment");
                }
            }
            Stmt::Print { items }
            | Stmt::Read { args: items }
            | Stmt::CallSub { args: items, .. } => {
                for e in items {
                    expression(e, &shapes)?;
                    if matches!(e,Expr::Call(n,_) if n=="@array_constructor") && delayed(e) {
                        bail!("runtime-bound constructor implied-DO currently requires whole-array assignment");
                    }
                }
            }
            Stmt::Return(Some(e)) => expression(e, &shapes)?,
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expression(cond, &shapes)?;
                body(then_body, &shapes, serial)?;
                if let Some(b) = else_body {
                    body(b, &shapes, serial)?;
                }
            }
            Stmt::Do {
                start,
                end,
                body: b,
                ..
            } => {
                expression(start, &shapes)?;
                expression(end, &shapes)?;
                body(b, &shapes, serial)?;
            }
            Stmt::DoWhile { cond, body: b } => {
                expression(cond, &shapes)?;
                body(b, &shapes, serial)?;
            }
            Stmt::Block { body: b }
            | Stmt::Function { body: b, .. }
            | Stmt::Subroutine { body: b, .. }
            | Stmt::Module { body: b, .. } => body(b, &shapes, serial)?,
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                expression(expr, &shapes)?;
                for case in cases {
                    body(&mut case.body, &shapes, serial)?;
                }
                if let Some(b) = default {
                    body(b, &shapes, serial)?;
                }
            }
            _ => {}
        }
        Ok(())
        })().map_err(|error|crate::source_diagnostics::at(error,source_offset))?;
    }
    Ok(())
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    body(&mut p.body, &Shapes::new(), &mut 0)?;
    Ok(p)
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Lower rank-one numeric/LOGICAL reductions, assignments and WHERE to loops.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone,Default)]
struct Symbols {
    variables: HashMap<String,(TypeSpec,Vec<Expr>)>,
    elementals: HashMap<String,TypeSpec>,
    scalar_functions: HashMap<String,TypeSpec>,
    character_functions: HashMap<String,Vec<Option<usize>>>,
    procedures: HashSet<String>,
}
impl Symbols { fn new() -> Self { Self::default() } }
impl std::ops::Deref for Symbols {
    type Target=HashMap<String,(TypeSpec,Vec<Expr>)>;
    fn deref(&self) -> &Self::Target { &self.variables }
}
impl std::ops::DerefMut for Symbols {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.variables }
}
fn int(n: i64) -> Expr { Expr::IntLit(n.to_string()) }
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr { Expr::Bin(op, Box::new(a), Box::new(b)) }
fn reduction(name: &str) -> bool { matches!(name.to_ascii_lowercase().as_str(), "sum" | "product" | "minval" | "maxval" | "iany" | "iall" | "iparity" | "parity" | "any" | "all" | "count" | "@count_i8" | "@count_default" | "@any_reduction" | "@all_reduction") }
fn array_element_kind(kind: &TypeSpec) -> bool { matches!(kind, TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::Logical) }
fn numeric_minmax(name:&str,symbols:&Symbols)->bool {
    matches!(name,"min"|"max") && !symbols.contains_key(name) && !symbols.procedures.contains(name)
}
fn same_kind(a: &TypeSpec, b: &TypeSpec) -> bool {
    match (a,b) {
        (TypeSpec::Integer(a),TypeSpec::Integer(b)) => a.unwrap_or(4) == b.unwrap_or(4),
        _ => a == b,
    }
}
fn merge_kind(a: TypeSpec, b: TypeSpec) -> TypeSpec {
    if matches!(a, TypeSpec::DoublePrecision) || matches!(b, TypeSpec::DoublePrecision) { TypeSpec::DoublePrecision }
    else if matches!(a, TypeSpec::Real) || matches!(b, TypeSpec::Real) { TypeSpec::Real }
    else if let (TypeSpec::Integer(a),TypeSpec::Integer(b)) = (&a,&b) { TypeSpec::Integer(Some(a.unwrap_or(4).max(b.unwrap_or(4)))) }
    else { a }
}
fn constant(e: &Expr) -> Result<i64> {
    crate::parameters::integer_constant(e).context("reduction extents and section bounds must be constant integers")
}

// Runtime PACK results retain bounded storage plus a separate logical length.
fn runtime_extent(e: &Expr) -> Option<Expr> {
    match e {
        Expr::Call(n,args) if n == "@packed_view" => Some(args[1].clone()),
        Expr::Bin(_,a,b) => runtime_extent(a).or_else(|| runtime_extent(b)),
        Expr::Un(_,a) => runtime_extent(a),
        _ => None,
    }
}
fn storage_extent(e:&Expr,symbols:&Symbols)->Result<i64> {
    match e {
        Expr::Call(n,a) if n=="@packed_view"=>storage_extent(&a[0],symbols),
        Expr::Bin(_,a,b)=>if element(a,&int(1),symbols)?.1.is_some() {storage_extent(a,symbols)} else {storage_extent(b,symbols)},
        Expr::Un(_,a)=>storage_extent(a,symbols),
        _=>element(e,&int(1),symbols)?.1.context("array argument requires bounded storage"),
    }
}
fn shape_guard(a: Expr,b: Expr) -> Stmt {
    Stmt::If { cond:bin(BinOp::Ne,a,b),then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str("runtime PACK result shape does not conform".into())]}],else_body:None }
}

// Produce a scalar expression for element `index`, its shape, and its type.
fn element(e: &Expr, index: &Expr, symbols: &Symbols) -> Result<(Expr, Option<i64>, TypeSpec)> {
    if let Some((value,count)) = crate::multidim::view_element(e,index) {
        let Expr::Index(name,_) = &value else {unreachable!()};
        let kind=symbols.get(name).context("undefined section source")?.0.clone();
        if !array_element_kind(&kind) { bail!("array sections currently require numeric or LOGICAL arrays"); }
        return Ok((value,Some(count),kind));
    }
    match e {
        Expr::Call(name,args) if crate::real_intrinsics::decode(name).is_some() || crate::real_intrinsics::keys(name).is_some() && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            let mut values=vec![];let mut kinds=vec![];let mut shape=None;
            for arg in args {
                let (value,count,kind)=element(arg,index,symbols)?;
                if let (Some(a),Some(b))=(shape,count) {if a>=0 && b>=0 && a!=b {bail!("floating-point intrinsic array shapes do not conform");}}
                shape=shape.or(count);values.push(value);kinds.push(kind);
            }
            let public=crate::real_intrinsics::decode(name).map_or(name.as_str(),|(n,_)|n);
            let kind=crate::real_intrinsics::result_kind(public,&kinds)?;
            Ok((Expr::Call(name.clone(),values),shape,kind))
        }
        Expr::Call(name,args) if crate::intrinsics::real_predicate(name) && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            if args.len()!=1 {bail!("{} expects 1 argument",name.to_ascii_uppercase());}
            let (value,count,kind)=element(&args[0],index,symbols)?;
            if !matches!(kind,TypeSpec::Real|TypeSpec::DoublePrecision) {bail!("{} requires a REAL or DOUBLE PRECISION argument",name.to_ascii_uppercase());}
            Ok((Expr::Call(name.clone(),vec![value]),count,TypeSpec::Logical))
        }
        Expr::Call(name,args) if crate::bit_intrinsics::keys(name).is_some() && !symbols.contains_key(name) && !symbols.procedures.contains(name)
            || crate::bit_intrinsics::decode(name).is_some() => {
            let mut values=vec![];let mut kinds=vec![];let mut shape=None;
            for arg in args {
                let (value,count,kind)=element(arg,index,symbols)?;
                if let (Some(a),Some(b))=(shape,count) {if a!=b {bail!("bit intrinsic array shapes do not conform");}}
                shape=shape.or(count);values.push(value);kinds.push(kind);
            }
            let public=crate::bit_intrinsics::decode(name).map_or(name.as_str(),|(n,_)|n);
            let kind=crate::bit_intrinsics::result_kind(public,&kinds)?;
            Ok((Expr::Call(name.clone(),values),shape,kind))
        },
        Expr::Call(n,args) if n=="@fixed_packed_view" => {
            let Expr::Ident(array)=&args[0] else{unreachable!()};
            let (kind,dims)=symbols.get(array).context("unknown fixed matrix source")?;
            let dims=dims.iter().map(constant).collect::<Result<Vec<_>>>()?;
            let count=dims.iter().try_fold(1i64,|n,d|n.checked_mul(*d).context("matrix element count overflow"))?;
            if count==0 {return Ok((match kind {TypeSpec::Real|TypeSpec::DoublePrecision=>Expr::RealLit("0.0".into()),TypeSpec::Logical=>Expr::Logical(false),_=>int(0)},Some(0),kind.clone()));}
            let mut ordinal=bin(BinOp::Sub,index.clone(),int(1));let mut indices=vec![];
            for extent in dims {indices.push(bin(BinOp::Add,Expr::Call("mod".into(),vec![ordinal.clone(),int(extent)]),int(1)));ordinal=bin(BinOp::Div,ordinal,int(extent));}
            Ok((Expr::Index(array.clone(),indices),Some(count),kind.clone()))
        },
        Expr::Call(n,args) if n == "@packed_view" => {
            let (value,_,kind) = element(&args[0],index,symbols)?;
            Ok((value,Some(-1),kind))
        }
        Expr::Ident(name) | Expr::Section { name, .. } => {
            let (kind, dims) = symbols.get(name).with_context(|| format!("undefined reduction argument `{name}`"))?;
            if !array_element_kind(kind) { bail!("reductions require numeric or LOGICAL arrays"); }
            if dims.is_empty() && matches!(e, Expr::Ident(_)) { return Ok((e.clone(), None, kind.clone())); }
            if dims.len() != 1 { bail!("reductions currently require one-dimensional arrays"); }
            let extent = constant(&dims[0])?;
            if extent < 0 { bail!("negative array extent is not supported"); }
            let (first, count, step) = if let Expr::Section { lower, upper, stride, .. } = e {
                let first = lower.as_deref().map(constant).transpose()?.unwrap_or(1);
                let last = upper.as_deref().map(constant).transpose()?.unwrap_or(extent);
                let step = stride.as_deref().map(constant).transpose()?.unwrap_or(1);
                if step == 0 { bail!("array section stride must not be zero"); }
                let distance = (i128::from(last) - i128::from(first)) * i128::from(step.signum());
                let count = if distance < 0 { 0 } else { distance / i128::from(step).abs() + 1 };
                let count = i64::try_from(count).context("section size overflow")?;
                let final_index = i128::from(first) + i128::from(count.saturating_sub(1)) * i128::from(step);
                if count > 0 && (first < 1 || first > extent || final_index < 1 || final_index > i128::from(extent)) {
                    bail!("array section of `{name}` selects an index outside 1:{extent}");
                }
                (first, count, step)
            } else { (1, extent, 1) };
            let subscript = bin(BinOp::Add, int(first), bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), int(1)), int(step)));
            Ok((Expr::Index(name.clone(), vec![subscript]), Some(count), kind.clone()))
        }
        Expr::Logical(_) => Ok((e.clone(), None, TypeSpec::Logical)),
        Expr::Call(tag,args) if tag == "@integer_literal" => Ok((e.clone(),None,TypeSpec::Integer(Some(constant(&args[1])? as u8)))),
        Expr::IntLit(_) => Ok((e.clone(), None, TypeSpec::Integer(None))),
        Expr::RealLit(_) => Ok((e.clone(), None, TypeSpec::Real)),
        Expr::Un(op @ (UnOp::Neg | UnOp::Not), a) => {
            let (a, len, kind) = element(a, index, symbols)?;
            Ok((Expr::Un(op.clone(), Box::new(a)), len, kind))
        }
        Expr::Bin(op, a, b) => {
            let (a, alen, akind) = element(a, index, symbols)?;
            let (b, blen, bkind) = element(b, index, symbols)?;
            if let (Some(a), Some(b)) = (alen, blen) { if a >= 0 && b >= 0 && a != b { bail!("array shapes in reduction expression do not conform"); } }
            Ok((bin(op.clone(), a, b), alen.or(blen), if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) { if matches!(op,BinOp::Pow) && matches!(bkind,TypeSpec::Integer(_)) && !matches!(akind,TypeSpec::Integer(_)) { akind } else { merge_kind(akind, bkind) } } else { TypeSpec::Logical }))
        }
        Expr::Index(name, args) | Expr::Call(name, args) if symbols.get(name).is_some_and(|(_, d)| d.len() == 1) => {
            if args.len() != 1 { bail!("array element requires one subscript"); }
            let (arg, len, kind) = element(&args[0], index, symbols)?;
            if len.is_some() || !matches!(kind, TypeSpec::Integer(_)) { bail!("array subscript must be scalar INTEGER"); }
            let kind = symbols[name].0.clone();
            if !array_element_kind(&kind) { bail!("reductions require numeric or LOGICAL arrays"); }
            Ok((Expr::Index(name.clone(), vec![arg]), None, kind))
        }
        Expr::Call(name,args) if symbols.elementals.contains_key(name) => {
            let mut shape=None; let mut values=vec![];
            for arg in args {
                let (value,count,_)=element(arg,index,symbols)?;
                if let (Some(a),Some(b))=(shape,count) { if a!=b {bail!("ELEMENTAL argument shapes do not conform");} }
                shape=shape.or(count); values.push(value);
            }
            Ok((Expr::Call(name.clone(),values),shape,symbols.elementals[name].clone()))
        }
        Expr::Call(name,_) if symbols.scalar_functions.contains_key(name) => {
            Ok((e.clone(),None,symbols.scalar_functions[name].clone()))
        }
        Expr::Call(name,args) if crate::intrinsics::integer_rounding(name) && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            let kind=crate::intrinsics::rounding_kind(name,args)?;
            let (value,count,input)=element(&args[0],index,symbols)?;
            if !matches!(input,TypeSpec::Real|TypeSpec::DoublePrecision) {bail!("{} A must be REAL or DOUBLE PRECISION",name.to_ascii_uppercase());}
            let mut values=vec![value];values.extend(args.iter().skip(1).cloned());
            Ok((Expr::Call(name.clone(),values),count,kind))
        }
        Expr::Call(name,args) if name=="int" && (args.len()==1 || args.len()==2) => {
            let (value,count,_) = element(&args[0],index,symbols)?;
            let kind = if let Some(e)=args.get(1) {TypeSpec::Integer(Some(u8::try_from(constant(e)?)?))} else {TypeSpec::Integer(None)};
            let mut values=vec![value];values.extend(args.iter().skip(1).cloned());
            Ok((Expr::Call(name.clone(),values),count,kind))
        }
        Expr::Call(name, args) if matches!(name.as_str(), "mod" | "modulo" | "sign") => {
            if args.len() != 2 { bail!("{name} requires two arguments"); }
            let (a, alen, kind) = element(&args[0], index, symbols)?;
            let (b, blen, other) = element(&args[1], index, symbols)?;
            if !same_kind(&kind,&other) || matches!(kind, TypeSpec::Logical) { bail!("{name} arguments must have the same numeric type and kind"); }
            if let (Some(a),Some(b)) = (alen,blen) { if a != b { bail!("{name} argument shapes do not conform"); } }
            Ok((Expr::Call(name.clone(),vec![a,b]),alen.or(blen),kind))
        }
        Expr::Call(name,args) if numeric_minmax(name,symbols) => {
            if args.len()<2 {bail!("{} expects at least 2 arguments",name.to_ascii_uppercase());}
            let mut shape=None;let mut kind=None;let mut values=vec![];
            for arg in args {
                let (value,count,input)=element(arg,index,symbols)?;
                if !array_element_kind(&input) || input==TypeSpec::Logical {bail!("{} requires numeric arguments",name.to_ascii_uppercase());}
                if kind.as_ref().is_some_and(|previous|!same_kind(previous,&input)) {bail!("{} arguments must have the same type and kind",name.to_ascii_uppercase());}
                if let (Some(a),Some(b))=(shape,count) {if a>=0 && b>=0 && a!=b {bail!("{} argument shapes do not conform",name.to_ascii_uppercase());}}
                shape=shape.or(count);kind=Some(input);values.push(value);
            }
            Ok((Expr::Call(name.clone(),values),shape,kind.unwrap()))
        }
        Expr::Call(name,args) if name == "abs" && args.len() == 1 => {
            let (value,count,kind) = element(&args[0],index,symbols)?;
            if !array_element_kind(&kind) || kind == TypeSpec::Logical { bail!("ABS requires a numeric argument"); }
            Ok((Expr::Call(name.clone(),vec![value]),count,kind))
        }
        Expr::Call(name, args) if crate::intrinsics::math_arity(name).is_some() || matches!(name.as_str(), "dble" | "real" | "abs") => {
            let mut shape=None; let mut kind=TypeSpec::Real; let mut values=vec![];
            for arg in args { let (value,n,k)=element(arg,index,symbols)?; if let (Some(a),Some(b))=(shape,n) { if a!=b {bail!("array shapes do not conform");} } shape=shape.or(n);kind=merge_kind(kind,k); values.push(value); }
            if name=="dble" {kind=TypeSpec::DoublePrecision;} else if name=="real" {kind=TypeSpec::Real;}
            Ok((Expr::Call(name.clone(),values),shape,kind))
        }
        _ => bail!("unsupported expression in numeric array reduction"),
    }
}
/// Select scalar storage without materializing or reading an OUT actual.
pub(crate) fn elemental_element(e: &Expr, index: &Expr, shapes: &HashMap<String,(TypeSpec,Vec<Expr>)>) -> Result<(Expr,Option<i64>,TypeSpec)> {
    let mut symbols=Symbols::new();
    symbols.variables=shapes.clone();
    element(e,index,&symbols)
}

/// Materialize an array expression before a specialized procedure call, using
/// the same constructor/PACK lowering as ordinary expressions.
pub(crate) fn argument(e: &mut Expr, shapes: &HashMap<String,(TypeSpec,Vec<Expr>)>, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<(TypeSpec,i64,Expr)> {
    argument_with_elementals(e,shapes,prefix,serial,&HashMap::new())
}
pub(crate) fn argument_with_elementals(e: &mut Expr, shapes: &HashMap<String,(TypeSpec,Vec<Expr>)>, prefix: &mut Vec<Stmt>, serial: &mut usize, functions:&HashMap<String,TypeSpec>) -> Result<(TypeSpec,i64,Expr)> {
    let mut symbols = Symbols::new();
    symbols.variables = shapes.clone();
    symbols.elementals=functions.clone();
    if matches!(e,Expr::Section {..}) { *e=Expr::Call("@array_constructor".into(),vec![e.clone()]); }
    lower_expr(e,&mut symbols,prefix,serial)?;
    let backing = if let Expr::Call(n,a)=e { if n=="@packed_view" {&a[0]} else {&*e} } else {&*e};
    let (_,_,kind)=element(backing,&int(1),&symbols)?;
    let count=storage_extent(backing,&symbols)?;
    if count<0 {bail!("array argument requires bounded storage");}
    let length=runtime_extent(e).unwrap_or(int(count));
    if !matches!(e,Expr::Ident(_)) && !matches!(e,Expr::Call(n,_) if n=="@packed_view") {
        *serial+=1;
        let temp=format!("@call_array_{serial}"); let index=format!("@call_array_index_{serial}");
        let value=element(e,&Expr::Ident(index.clone()),&symbols)?.0;
        if runtime_extent(e).is_some() {crate::heap::temporary(kind.clone(),temp.clone(),length.clone(),prefix);}else{prefix.push(Stmt::ArrayDecl {kind:kind.clone(),name:temp.clone(),dims:vec![int(count)]});}
        prefix.push(Stmt::VarDecl {kind:TypeSpec::Integer(None),names:vec![index.clone()]});
        prefix.push(Stmt::Do {var:index.clone(),start:int(1),end:length.clone(),body:vec![Stmt::AssignIndex {name:temp.clone(),indices:vec![Expr::Ident(index)],value}]});
        *e=if runtime_extent(e).is_some() {Expr::Call("@packed_view".into(),vec![Expr::Ident(temp),length.clone()])} else {Expr::Ident(temp)};
    }
    Ok((kind,count,length))
}
pub(crate) fn argument_element(e: &Expr, index: &Expr, shapes: &HashMap<String,(TypeSpec,Vec<Expr>)>) -> Result<Expr> {
    let mut symbols=Symbols::new(); symbols.variables=shapes.clone();
    Ok(element(e,index,&symbols)?.0)
}

fn lower_expr(e: &mut Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    lower_expr_impl(e,symbols,prefix,serial,true)
}

// Integers are held in 64 bits, so a value of a wider INTEGER kind assigned
// to a narrower variable keeps its low-order bits, as two's complement
// conversion does. IOR(VALUE, 0) in the target kind performs that wrap.
fn narrow_integer(name: &str, value: &mut Expr, symbols: &Symbols) {
    let Some((TypeSpec::Integer(target), _)) = symbols.get(name) else { return };
    let target = target.unwrap_or(4);
    if target >= 8 { return; }
    let Ok((_, None, TypeSpec::Integer(have))) = element(value, &int(1), symbols) else { return };
    if have.unwrap_or(4) > target {
        *value = Expr::Call(crate::bit_intrinsics::tag("ior", target), vec![std::mem::replace(value, int(0)), int(0)]);
    }
}

fn lower_assignment_value(e: &mut Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    // Keep assignment-context conversion and scalar procedure calls supported
    // by the existing constructor assignment path.
    if let Expr::Call(tag, items) = e {
        if tag == "@array_constructor" && items.iter().all(|item| !array_valued(item,symbols)) {
            for item in items { lower_expr(item,symbols,prefix,serial)?; }
            return Ok(());
        }
    }
    lower_expr(e,symbols,prefix,serial)
}

// Keep the location lowering outside the recursive expression function so its
// temporaries do not inflate every recursive stack frame on Windows debug builds.
fn lower_location(e: &mut Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    let Expr::Call(name,args) = e else { unreachable!() };
    for i in [0,1,3,5] { lower_expr(&mut args[i],symbols,prefix,serial)?; }
    let find = name == "@findloc";
    let dim = !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_dim");
    let stride = constant(&args[6])?.max(1);
    let axis_len = constant(&args[7])?;
    let Expr::Call(_,source_dims) = &args[8] else { unreachable!() };
    let source_dims = source_dims.iter().map(constant).collect::<Result<Vec<_>>>()?;
    let Expr::Call(_,result_dims) = &args[9] else { unreachable!() };
    let scalar = result_dims.is_empty();
    let output_len = result_dims.iter().try_fold(1i64,|n,d| n.checked_mul(constant(d)?).context("location result extent overflow"))?;
    let result_kind = TypeSpec::Integer(Some(constant(&args[4])? as u8));
    *serial += 1;
    let temp = format!("@location_result_{serial}");
    let index = format!("@location_index_{serial}");
    let outer = format!("@location_outer_{serial}");
    let position = format!("@location_position_{serial}");
    let best = format!("@location_best_{serial}");
    let item = format!("@location_item_{serial}");
    let back = format!("@location_back_{serial}");
    let sought = format!("@location_sought_{serial}");
    let id = |n: &str| Expr::Ident(n.into());
    let assign = |n: &str,value| Stmt::Assign {name:n.into(),value};
    let source_index = if dim {
        let ordinal = bin(BinOp::Sub,id(&outer),int(1));
        bin(BinOp::Add,int(1),bin(BinOp::Add,Expr::Call("modulo".into(),vec![ordinal.clone(),int(stride)]),bin(BinOp::Add,bin(BinOp::Mul,bin(BinOp::Div,ordinal,int(stride)),int(stride*axis_len)),bin(BinOp::Mul,bin(BinOp::Sub,id(&index),int(1)),int(stride)))))
    } else { id(&index) };
    let (value,_,kind) = element(&args[0],&source_index,symbols)?;
    if !find && kind == TypeSpec::Logical { bail!("MINLOC/MAXLOC ARRAY must be numeric"); }
    let (mask,_,mask_kind) = element(&args[3],&source_index,symbols)?;
    if mask_kind != TypeSpec::Logical { bail!("location MASK must be LOGICAL"); }
    let (_,back_len,back_kind) = element(&args[5],&int(1),symbols)?;
    if back_len.is_some() || back_kind != TypeSpec::Logical { bail!("location BACK must be scalar LOGICAL"); }
    prefix.push(Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:if dim {vec![index.clone(),outer.clone(),position.clone()]} else {vec![index.clone(),position.clone()]}});
    prefix.push(Stmt::VarDecl {kind:TypeSpec::Logical,names:vec![back.clone()]});
    prefix.push(assign(&back,args[5].clone()));
    prefix.push(Stmt::VarDecl {kind:kind.clone(),names:if find {vec![item.clone()]} else {vec![item.clone(),best.clone()]}});
    if find {
        let (_,len,value_kind) = element(&args[1],&int(1),symbols)?;
        if len.is_some() || !same_kind(&kind,&value_kind) { bail!("FINDLOC VALUE must be scalar with the same type and kind as ARRAY"); }
        prefix.push(Stmt::VarDecl {kind:kind.clone(),names:vec![sought.clone()]});
        prefix.push(assign(&sought,args[1].clone()));
    }
    if scalar { prefix.push(Stmt::VarDecl {kind:result_kind.clone(),names:vec![temp.clone()]}); }
    else { prefix.push(Stmt::ArrayDecl {kind:result_kind.clone(),name:temp.clone(),dims:vec![int(output_len)]}); }
    let first = bin(BinOp::Eq,id(&position),int(0));
    let update = if find {
        bin(BinOp::And,bin(if kind == TypeSpec::Logical {BinOp::Eqv} else {BinOp::Eq},id(&item),id(&sought)),bin(BinOp::Or,first,id(&back)))
    } else {
        bin(BinOp::Or,first,bin(BinOp::Or,bin(if name == "@minloc" {BinOp::Lt} else {BinOp::Gt},id(&item),id(&best)),bin(BinOp::And,id(&back),bin(BinOp::Eq,id(&item),id(&best)))))
    };
    let mut body = vec![assign(&position,int(0))];
    if !find { body.push(assign(&best,int(0))); }
    body.push(Stmt::Do {var:index.clone(),start:int(1),end:int(axis_len),body:vec![Stmt::If {cond:mask,then_body:vec![assign(&item,value),Stmt::If {cond:update,then_body:if find {vec![assign(&position,id(&index))]} else {vec![assign(&position,id(&index)),assign(&best,id(&item))]},else_body:None}],else_body:None}]});
    if dim {
        body.push(if scalar { assign(&temp,id(&position)) } else { Stmt::AssignIndex {name:temp.clone(),indices:vec![id(&outer)],value:id(&position)} });
        prefix.push(Stmt::Do {var:outer,start:int(1),end:int(output_len),body});
    } else {
        let mut stride = 1;
        for (axis,extent) in source_dims.iter().enumerate() {
            let coordinate = bin(BinOp::Add,int(1),Expr::Call("modulo".into(),vec![bin(BinOp::Div,bin(BinOp::Sub,id(&position),int(1)),int(stride.max(1))),int((*extent).max(1))]));
            let set = |value| Stmt::AssignIndex {name:temp.clone(),indices:vec![int(axis as i64+1)],value};
            body.push(Stmt::If {cond:bin(BinOp::Eq,id(&position),int(0)),then_body:vec![set(int(0))],else_body:Some(vec![set(coordinate)])});
            stride *= extent;
        }
        prefix.extend(body);
    }
    symbols.insert(temp.clone(),(result_kind,if scalar {vec![]} else {vec![int(output_len)]}));
    *e = Expr::Ident(temp);
    Ok(())
}

// Capture both operands at a common padded length before comparing bytes. This
// also handles runtime TRIM lengths without relying on NUL-terminated strcmp.
fn character_capacity(e: &Expr, symbols: &Symbols) -> Result<Option<usize>> {
    Ok(match e {
        Expr::Str(s) => {
            if !s.is_ascii() || s.contains('\0') { bail!("CHARACTER comparison requires non-NUL ASCII text"); }
            Some(s.len())
        }
        Expr::Ident(n) => match symbols.get(n) {
            Some((TypeSpec::Character(n), dims)) if dims.is_empty() => Some(n.unwrap_or(1)),
            _ => None,
        },
        Expr::Bin(BinOp::Concat, a, b) => match (character_capacity(a,symbols)?, character_capacity(b,symbols)?) {
            (Some(a),Some(b)) => Some(a.checked_add(b).context("CHARACTER comparison is too large")?),
            _ => None,
        },
        Expr::Call(n,args) if matches!(n.as_str(),"trim"|"adjustl"|"adjustr") && args.len()==1 => character_capacity(&args[0],symbols)?,
        Expr::Call(n,args) if n == "@dynamic_substring" && args.len() == 4 => Some(constant(&args[3])? as usize),
        _ => None,
    })
}

fn lower_character_comparison(op: BinOp, a: &Expr, b: &Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<Option<Expr>> {
    let lengths = (character_capacity(a,symbols)?,character_capacity(b,symbols)?);
    let length = match lengths {
        (None,None) => return Ok(None),
        (Some(a),Some(b)) => a.max(b),
        _ => bail!("CHARACTER comparison requires two scalar CHARACTER operands"),
    };
    if length >= i32::MAX as usize { bail!("CHARACTER comparison is too large"); }
    *serial += 1;
    let names: Vec<String> = ["left","right","index","a","b","order"].iter()
        .map(|part| format!("@character_compare_{serial}_{part}")).collect();
    let id = |i: usize| Expr::Ident(names[i].clone());
    for (i,name) in names.iter().enumerate() {
        let kind = if i<2 {TypeSpec::Character(Some(length))} else {TypeSpec::Integer(Some(8))};
        prefix.push(Stmt::VarDecl {kind:kind.clone(), names:vec![name.clone()]});
        symbols.insert(name.clone(),(kind,vec![]));
    }
    for (i,source) in [a,b].into_iter().enumerate() {
        prefix.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![id(i),source.clone(),int(length as i64)]});
    }
    prefix.push(Stmt::Assign {name:names[5].clone(),value:int(0)});
    let mut body = Vec::new();
    for i in 0..2 {
        body.push(Stmt::CallSub {name:"@internal_read_byte".into(),args:vec![id(i+3),id(i),id(2),int(length as i64)]});
    }
    body.push(Stmt::If {cond:bin(BinOp::Ne,id(3),id(4)),then_body:vec![
        Stmt::Assign {name:names[5].clone(),value:bin(BinOp::Sub,id(3),id(4))}
    ],else_body:None});
    prefix.push(Stmt::Do {var:names[2].clone(),start:int(0),end:int(length as i64-1),body:vec![
        Stmt::If {cond:bin(BinOp::Eq,id(5),int(0)),then_body:body,else_body:None}
    ]});
    Ok(Some(bin(op,id(5),int(0))))
}

// OUT_OF_RANGE(X, MOLD [, ROUND]) as comparisons with the limits of MOLD's
// type. A REAL X is truncated, or rounded when ROUND is true, before it is
// compared with an INTEGER range; NaN is out of every INTEGER range, while
// infinities and NaN are representable in every REAL kind.
fn out_of_range(e:&Expr,symbols:&Symbols)->Result<Expr> {
    let Expr::Call(_,args)=e else {unreachable!()};
    if !(2..=3).contains(&args.len()) {bail!("OUT_OF_RANGE expects X, MOLD and optional ROUND");}
    let mut kinds=vec![];
    for arg in &args[..2] {
        let (_,shape,kind)=element(arg,&int(1),symbols)?;
        if shape.is_some() {bail!("OUT_OF_RANGE currently requires scalar arguments");}
        kinds.push(kind);
    }
    let round=match args.get(2) {
        None=>false,
        Some(arg) if crate::named_arguments::omitted(arg)=>false,
        Some(Expr::Logical(value))=>*value,
        Some(_)=>bail!("OUT_OF_RANGE ROUND must currently be a LOGICAL constant"),
    };
    let x=args[0].clone();
    Ok(match (&kinds[0],&kinds[1]) {
        (TypeSpec::Integer(have),TypeSpec::Integer(mold)) => {
            let (have,mold)=(have.unwrap_or(4),mold.unwrap_or(4));
            if have<=mold {return Ok(Expr::Logical(false));}
            let high=int(crate::inquiries::integer_huge(Some(mold))?);
            let low=bin(BinOp::Sub,Expr::Un(UnOp::Neg,Box::new(high.clone())),int(1));
            bin(BinOp::Or,bin(BinOp::Gt,x.clone(),high),bin(BinOp::Lt,x,low))
        }
        (TypeSpec::Real|TypeSpec::DoublePrecision,TypeSpec::Integer(mold)) => {
            let limit=2f64.powi(i32::from(mold.unwrap_or(4))*8-1).to_string();
            let limit=if kinds[0]==TypeSpec::DoublePrecision {Expr::Call("dble".into(),vec![Expr::RealLit(limit)])} else {Expr::RealLit(limit)};
            let whole=Expr::Call(if round {"anint"} else {"aint"}.into(),vec![x]);
            let inside=bin(BinOp::And,bin(BinOp::Ge,whole.clone(),Expr::Un(UnOp::Neg,Box::new(limit.clone()))),bin(BinOp::Lt,whole,limit));
            Expr::Un(UnOp::Not,Box::new(inside))
        }
        (TypeSpec::DoublePrecision,TypeSpec::Real) => {
            let magnitude=Expr::Call("abs".into(),vec![x]);
            let huge=|v:f64|Expr::Call("dble".into(),vec![Expr::RealLit(format!("{v:.1}"))]);
            bin(BinOp::And,bin(BinOp::Gt,magnitude.clone(),huge(f32::MAX as f64)),bin(BinOp::Le,magnitude,huge(f64::MAX)))
        }
        (TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision,TypeSpec::Real|TypeSpec::DoublePrecision) => Expr::Logical(false),
        _ => bail!("OUT_OF_RANGE requires INTEGER or REAL X and MOLD"),
    })
}

// Keep bit captures/checks outside the recursive lowering frame on Windows.
fn lower_bit(e:&mut Expr,symbols:&mut Symbols,prefix:&mut Vec<Stmt>,serial:&mut usize,expand_elementals:bool)->Result<()> {
    let Expr::Call(name,args)=e else {unreachable!()};
    let arity=crate::bit_intrinsics::keys(name).unwrap().len();
    if args.len()!=arity {bail!("{} expects {arity} arguments, got {}",name.to_ascii_uppercase(),args.len());}
    let mut kinds=vec![];let mut elemental=false;
    for arg in args.iter_mut() {
        lower_expr_impl(arg,symbols,prefix,serial,expand_elementals)?;
        let (_,shape,kind)=element(arg,&int(1),symbols)?;
        elemental|=shape.is_some();
        kinds.push(kind);
    }
    crate::bit_intrinsics::result_kind(name,&kinds)?;
    let TypeSpec::Integer(bytes)=kinds[0] else {unreachable!()};
    let bytes=bytes.unwrap_or(4);let bits=i64::from(bytes)*8;
    let values=args.iter().map(|arg|crate::parameters::integer_constant(arg).ok()).collect::<Vec<_>>();
    crate::bit_intrinsics::validate_controls(name,bytes,&values)?;
    if elemental {
        // Array arguments are evaluated element by element; only constant
        // control arguments are range checked.
        if values.iter().skip(1).any(Option::is_none) && !crate::bit_intrinsics::counts(name) && !matches!(name.as_str(),"iand"|"ior"|"ieor"|"not") {
            bail!("{} with array arguments currently requires constant control arguments",name.to_ascii_uppercase());
        }
        *name=crate::bit_intrinsics::tag(name,bytes);
        return Ok(());
    }
    // Capture before checks so each argument is evaluated exactly once.
    for (arg,kind) in args.iter_mut().zip(kinds) {
        *serial+=1;let temp=format!("@bit_argument_{serial}");
        prefix.push(Stmt::VarDecl {kind:kind.clone(),names:vec![temp.clone()]});
        prefix.push(Stmt::Assign {name:temp.clone(),value:arg.clone()});
        symbols.insert(temp.clone(),(kind,vec![]));*arg=Expr::Ident(temp);
    }
    if name=="ibits" {
        // Check each bound before subtracting POS, even for INTEGER(8) extremes.
        let cond=bin(BinOp::Or,
            bin(BinOp::Or,bin(BinOp::Lt,args[1].clone(),int(0)),bin(BinOp::Gt,args[1].clone(),int(bits))),
            bin(BinOp::Or,bin(BinOp::Lt,args[2].clone(),int(0)),bin(BinOp::Gt,args[2].clone(),int(bits))));
        prefix.push(Stmt::If {cond,then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(crate::bit_intrinsics::range_message(name,bits))]}],else_body:None});
    }
    let control=match name.as_str() {
        "btest"|"ibset"|"ibclr"=>Some(bin(BinOp::Or,bin(BinOp::Lt,args[1].clone(),int(0)),bin(BinOp::Ge,args[1].clone(),int(bits)))),
        "ishft"=>Some(bin(BinOp::Or,bin(BinOp::Lt,args[1].clone(),int(-bits)),bin(BinOp::Gt,args[1].clone(),int(bits)))),
        "ibits"=>Some(bin(BinOp::Gt,args[2].clone(),bin(BinOp::Sub,int(bits),args[1].clone()))),
        "dshiftl"|"dshiftr"=>Some(bin(BinOp::Or,bin(BinOp::Lt,args[2].clone(),int(0)),bin(BinOp::Gt,args[2].clone(),int(bits)))),
        _=>None,
    };
    if let Some(cond)=control {prefix.push(Stmt::If {cond,then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(crate::bit_intrinsics::range_message(name,bits))]}],else_body:None});}
    *name=crate::bit_intrinsics::tag(name,bytes);
    Ok(())
}

// Keep PACK allocation/materialization outside the recursive expression frame.
fn lower_pack(e:&mut Expr,symbols:&mut Symbols,prefix:&mut Vec<Stmt>,serial:&mut usize)->Result<()> {
    match e {
        Expr::Call(name,args) if name=="pack" => {
            if args.len()!=3 {bail!("PACK requires normalized ARRAY, MASK and VECTOR arguments");}
            for arg in &mut args[..2] {lower_expr(arg,symbols,prefix,serial)?;}
            let count=if matches!(&args[2],Expr::Call(n,_) if n=="@no_vector") {-1} else {
                lower_expr(&mut args[2],symbols,prefix,serial)?;
                let (_,count,_)=element(&args[2],&int(1),symbols)?;
                count.context("PACK VECTOR must be an array")?
            };
            *name="@pack".into();
            args.push(Expr::Call("@packing_shape".into(),vec![int(count)]));
            lower_pack(e,symbols,prefix,serial)?;
        }
        Expr::Call(name,args) if matches!(name.as_str(),"@pack" | "@unpack") => {
            let unpack = name == "@unpack";
            let has_vector = unpack || !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_vector");
            for arg in &mut args[..3] { lower_expr(arg,symbols,prefix,serial)?; }
            let Expr::Call(_,dims) = &args[3] else { unreachable!() };
            let dynamic = !unpack && dims.len()==1 && constant(&dims[0])? == -1;
            let count = if dynamic { storage_extent(&args[0],symbols)? } else { dims.iter().try_fold(1i64,|n,d| n.checked_mul(constant(d)?).context("PACK/UNPACK result extent overflow"))? };
            if count < 0 { bail!("nested runtime PACK results are not yet supported"); }
            *serial += 1;
            let temp = format!("@packing_array_{serial}");
            let index = format!("@packing_index_{serial}");
            let cursor = format!("@packing_cursor_{serial}");
            let id = |n: &str| Expr::Ident(n.into());
            let (mask,mask_len,mask_kind) = element(&args[1],&id(&index),symbols)?;
            if mask_kind != TypeSpec::Logical { bail!("PACK/UNPACK MASK must be LOGICAL"); }
            let (source,source_len,kind) = element(&args[0],&id(if unpack {&cursor} else {&index}),symbols)?;
            let source_len = source_len.context("PACK ARRAY / UNPACK VECTOR must be an array")?;
            if !unpack && mask_len.is_some_and(|n| n>=0 && source_len>=0 && n!=source_len) { bail!("PACK MASK shape must conform to ARRAY"); }
            let source_extent=runtime_extent(&args[0]).unwrap_or(int(source_len));
            if !unpack && mask_len.is_some() {
                let mask_extent=runtime_extent(&args[1]).unwrap_or(int(mask_len.unwrap()));
                if mask_extent!=source_extent {prefix.push(shape_guard(source_extent.clone(),mask_extent));}
            }
            let set = |index,value| Stmt::AssignIndex { name:temp.clone(),indices:vec![index],value };
            let next = Stmt::Assign { name:cursor.clone(),value:bin(BinOp::Add,id(&cursor),int(1)) };
            let guard = |limit| Stmt::If { cond:bin(BinOp::Gt,id(&cursor),limit),then_body:vec![Stmt::CallSub { name:"@error_stop".into(),args:vec![Expr::Str(format!("{} VECTOR has too few elements for MASK",if unpack {"UNPACK"} else {"PACK"}))] }],else_body:None };
            let mut body;
            if unpack {
                let (field,field_len,field_kind) = element(&args[2],&id(&index),symbols)?;
                if !same_kind(&kind,&field_kind) { bail!("UNPACK VECTOR and FIELD must have the same type and kind"); }
                if field_len.is_some_and(|n| n!=count) { bail!("UNPACK FIELD shape must conform to MASK"); }
                body = vec![Stmt::If { cond:mask,then_body:vec![next,guard(int(source_len)),set(id(&index),source)],else_body:Some(vec![set(id(&index),field)]) }];
            } else {
                body = vec![Stmt::If { cond:mask,then_body:vec![next,guard(if dynamic && !has_vector {source_extent.clone()}else{int(count)}),set(id(&cursor),source)],else_body:None }];
            }
            if dynamic && !has_vector {crate::heap::temporary(kind.clone(),temp.clone(),source_extent.clone(),prefix);}else{prefix.push(Stmt::ArrayDecl { kind:kind.clone(),name:temp.clone(),dims:vec![int(count)] });}
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)),names:vec![index.clone(),cursor.clone()] });
            prefix.push(Stmt::Assign { name:cursor.clone(),value:int(0) });
            prefix.push(Stmt::Do { var:index.clone(),start:int(1),end:if unpack {int(count)} else {source_extent},body:std::mem::take(&mut body) });
            if !unpack && has_vector {
                let (vector,_,vector_kind) = element(&args[2],&id(&index),symbols)?;
                if !same_kind(&kind,&vector_kind) { bail!("PACK ARRAY and VECTOR must have the same type and kind"); }
                prefix.push(Stmt::Do { var:index.clone(),start:bin(BinOp::Add,id(&cursor),int(1)),end:int(count),body:vec![set(id(&index),vector)] });
            }
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            symbols.insert(cursor.clone(),(TypeSpec::Integer(Some(8)),vec![]));
            *e = if dynamic { Expr::Call("@packed_view".into(),vec![Expr::Ident(temp),Expr::Ident(cursor)]) } else { Expr::Ident(temp) };
        }
        _=>unreachable!(),
    }
    Ok(())
}

fn lower_expr_impl(e: &mut Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize, expand_elementals: bool) -> Result<()> {

    match e {
        Expr::Call(name,args) if crate::intrinsics::real_predicate(name) && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            if args.len()!=1 {bail!("{} expects 1 argument",name.to_ascii_uppercase());}
            lower_expr_impl(&mut args[0],symbols,prefix,serial,expand_elementals)?;
            let input=element(&args[0],&int(1),symbols).map(|(_,_,kind)|kind)
                .or_else(|_|crate::array_calls::scalar_actual_kind(&args[0],symbols)).ok();
            if input.is_some_and(|kind|!matches!(kind,TypeSpec::Real|TypeSpec::DoublePrecision)) {bail!("{} requires a REAL or DOUBLE PRECISION argument",name.to_ascii_uppercase());}
        }
        Expr::Call(name,args) if crate::intrinsics::integer_rounding(name) && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            crate::intrinsics::rounding_kind(name,args)?;
            lower_expr_impl(&mut args[0],symbols,prefix,serial,expand_elementals)?;
            let input=element(&args[0],&int(1),symbols).map(|(_,_,kind)|kind)
                .or_else(|_|crate::array_calls::scalar_actual_kind(&args[0],symbols)).ok();
            // Native scalar procedure calls are typed by sema, rather than by
            // this array-expression symbol table.
            if input.is_some_and(|k|!matches!(k,TypeSpec::Real|TypeSpec::DoublePrecision)) {bail!("{} A must be REAL or DOUBLE PRECISION",name.to_ascii_uppercase());}
        }
        Expr::Call(name,args) if name=="merge" && !symbols.procedures.contains(name) => {
            if args.len()!=3 {bail!("MERGE expects TSOURCE, FSOURCE and MASK");}
            for arg in args.iter_mut() {lower_expr_impl(arg,symbols,prefix,serial,expand_elementals)?;}
            let kind=|e:&Expr|->Result<(TypeSpec,bool)> {
                element(e,&int(1),symbols).map(|(_,shape,kind)|(kind,shape.is_some()))
                    .or_else(|_|crate::array_calls::scalar_actual_kind(e,&symbols.variables).map(|kind|(kind,false)))
            };
            let (first,array)=kind(&args[0])?;let (second,other_array)=kind(&args[1])?;let (mask,mask_array)=kind(&args[2])?;
            if !same_kind(&first,&second) {bail!("MERGE sources must have the same type, kind and CHARACTER length");}
            if mask!=TypeSpec::Logical {bail!("MERGE MASK must be LOGICAL");}
            if array || other_array || mask_array {
                // Elemental MERGE: select each element into a temporary array.
                if !array_element_kind(&first) {bail!("MERGE of arrays currently requires numeric or LOGICAL sources");}
                let mut extent=None;
                for arg in args.iter() {
                    if let Some(n)=element(arg,&int(1),symbols)?.1 {
                        if n<0 {bail!("MERGE of arrays currently requires arrays of known size");}
                        if extent.is_some_and(|m|m!=n) {bail!("MERGE argument shapes do not conform");}
                        extent=Some(n);
                    }
                }
                let extent=extent.unwrap();
                *serial+=1;
                let temp=format!("@merge_array_{serial}");let index=format!("@merge_index_{serial}");
                let at=|arg:&Expr|element(arg,&Expr::Ident(index.clone()),symbols).map(|(value,_,_)|value);
                let (source,other,choose)=(at(&args[0])?,at(&args[1])?,at(&args[2])?);
                prefix.push(Stmt::ArrayDecl{kind:first.clone(),name:temp.clone(),dims:vec![int(extent)]});
                prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]});
                let set=|value|Stmt::AssignIndex{name:temp.clone(),indices:vec![Expr::Ident(index.clone())],value};
                prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:int(extent),body:vec![
                    Stmt::If{cond:choose,then_body:vec![set(source)],else_body:Some(vec![set(other)])}]});
                symbols.insert(temp.clone(),(first,vec![int(extent)]));
                *e=Expr::Ident(temp);
                return Ok(());
            }
            *serial+=1;let temp=format!("@merge_value_{serial}");
            prefix.push(Stmt::VarDecl{kind:first.clone(),names:vec![temp.clone()]});
            let store=|value:Expr|if let TypeSpec::Character(length)=&first {
                Stmt::CallSub{name:"@character_assign".into(),args:vec![Expr::Ident(temp.clone()),value,int(length.unwrap_or(1) as i64)]}
            }else{Stmt::Assign{name:temp.clone(),value}};
            prefix.push(Stmt::If{cond:args[2].clone(),then_body:vec![store(args[0].clone())],else_body:Some(vec![store(args[1].clone())])});
            symbols.insert(temp.clone(),(first,vec![]));
            *e=Expr::Ident(temp);
        }
        Expr::Call(name,_) if name=="out_of_range" && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            *e=out_of_range(e,symbols)?;
            lower_expr_impl(e,symbols,prefix,serial,expand_elementals)?;
        }
        Expr::Call(name,args) if crate::bit_intrinsics::keys(name).is_some() && !symbols.contains_key(name) && !symbols.procedures.contains(name) => lower_bit(e,symbols,prefix,serial,expand_elementals)?,
        // Call specialization precedes multidimensional normalization. Its
        // rank-one PACK actuals can use the same bounded lowering directly.
        Expr::Call(tag,args) if tag == "@substring" => {
            let length=constant(&args[2])? as usize;
            *serial+=1;
            let temp=format!("@substring_read_{serial}");
            let kind=TypeSpec::Character(Some(length));
            prefix.push(Stmt::VarDecl {kind:kind.clone(),names:vec![temp.clone()]});
            prefix.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![Expr::Ident(temp.clone()),e.clone(),int(length as i64)]});
            symbols.insert(temp.clone(),(kind,vec![]));
            *e=Expr::Ident(temp);
        }
        Expr::Call(tag,args) if tag == "@integer_literal" => {
            let kind = TypeSpec::Integer(Some(constant(&args[1])? as u8));
            *serial += 1;
            let temp = format!("@integer_literal_{serial}");
            prefix.push(Stmt::VarDecl {kind:kind.clone(),names:vec![temp.clone()]});
            prefix.push(Stmt::Assign {name:temp.clone(),value:args[0].clone()});
            symbols.insert(temp.clone(),(kind,vec![]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name,args) if name == "kind" && args.len() == 1 && !matches!(&args[0],Expr::Ident(_)) && !matches!(&args[0],Expr::Call(n,_) if n == "huge") => {
            let argument = match &args[0] {
                Expr::Call(n,inner) if matches!(n.as_str(),"sum" | "product" | "minval" | "maxval") && !inner.is_empty() => &inner[0],
                other => other,
            };
            let (_,_,kind) = element(argument,&int(1),symbols)?;
            *e = int(match kind { TypeSpec::Integer(k) => k.unwrap_or(4) as i64, TypeSpec::DoublePrecision => 8, _ => 4 });
        }
        Expr::Call(name, args) if name == "@array_constructor" => {
            // Constant empty implied-DO constructors lose their element type
            // during expansion. Preserve the existing assignment-context path.
            if args.is_empty() { return Ok(()); }
            let mut items = Vec::new();
            let mut kind = None;
            let mut count = 0i64;
            for arg in args {
                lower_expr_impl(arg, symbols, prefix, serial, expand_elementals)?;
                let (_, len, item_kind) = element(arg, &int(1), symbols)?;
                if kind.as_ref().is_some_and(|k| !same_kind(k,&item_kind)) { bail!("array constructor items must have the same type and kind"); }
                kind = Some(item_kind);
                let len = len.unwrap_or(1);
                if len >= 0 { count = count.checked_add(len).context("array constructor extent overflow")?; }
                items.push((arg.clone(),len));
            }
            let kind = kind.context("empty untyped array constructor is not supported")?;
            if items.iter().any(|(_,len)| *len < 0) {
                // Runtime-sized items, e.g. `[a, x]` with an ALLOCATABLE `a`,
                // fill a heap temporary at a running offset.
                *serial += 1;
                let temp = format!("@constructor_array_{serial}");
                let index = format!("@constructor_index_{serial}");
                let offset = format!("@constructor_offset_{serial}");
                let total = format!("@constructor_length_{serial}");
                let mut lengths = vec![];
                for (arg,len) in &items {
                    lengths.push(if *len >= 0 { int(*len) } else {
                        runtime_extent(arg).context("runtime-sized array constructor items must be whole runtime-sized arrays")?
                    });
                }
                prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)), names:vec![total.clone()] });
                prefix.push(Stmt::Assign { name:total.clone(), value:lengths.iter().cloned().reduce(|a,b| bin(BinOp::Add,a,b)).unwrap() });
                crate::heap::temporary(kind.clone(), temp.clone(), Expr::Ident(total.clone()), prefix);
                prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(None), names:vec![index.clone()] });
                prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)), names:vec![offset.clone()] });
                prefix.push(Stmt::Assign { name:offset.clone(), value:int(0) });
                for ((arg,_),length) in items.into_iter().zip(lengths) {
                    let (value,_,_) = element(&arg,&Expr::Ident(index.clone()),symbols)?;
                    prefix.push(Stmt::Do { var:index.clone(),start:int(1),end:length.clone(),body:vec![Stmt::AssignIndex {
                        name:temp.clone(),indices:vec![bin(BinOp::Add,Expr::Ident(offset.clone()),Expr::Ident(index.clone()))],value,
                    }] });
                    prefix.push(Stmt::Assign { name:offset.clone(), value:bin(BinOp::Add,Expr::Ident(offset.clone()),length) });
                }
                symbols.insert(temp.clone(),(kind,vec![int(1)]));
                *e = Expr::Call("@packed_view".into(),vec![Expr::Ident(temp),Expr::Ident(total)]);
                return Ok(());
            }
            *serial += 1;
            let temp = format!("@constructor_array_{serial}");
            let index = format!("@constructor_index_{serial}");
            prefix.push(Stmt::ArrayDecl { kind:kind.clone(), name:temp.clone(), dims:vec![int(count)] });
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(None), names:vec![index.clone()] });
            let mut offset = 0;
            for (arg,len) in items {
                let (value,_,_) = element(&arg,&Expr::Ident(index.clone()),symbols)?;
                prefix.push(Stmt::Do { var:index.clone(),start:int(1),end:int(len),body:vec![Stmt::AssignIndex {
                    name:temp.clone(),indices:vec![bin(BinOp::Add,int(offset),Expr::Ident(index.clone()))],value,
                }] });
                offset += len;
            }
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name,_) if matches!(name.as_str(),"@minloc" | "@maxloc" | "@findloc") => lower_location(e,symbols,prefix,serial)?,
        Expr::Call(name,_) if matches!(name.as_str(),"pack"|"@pack"|"@unpack")=>lower_pack(e,symbols,prefix,serial)?,
        Expr::Call(name,args) if name == "@spread" => {
            lower_expr(&mut args[0],symbols,prefix,serial)?;
            let stride = constant(&args[1])?.max(1);
            let copies = constant(&args[2])?.max(1);
            let Expr::Call(_,dims) = &args[3] else { unreachable!() };
            let count = dims.iter().try_fold(1i64,|n,d| n.checked_mul(constant(d)?).context("SPREAD result extent overflow"))?;
            *serial += 1;
            let temp = format!("@spread_array_{serial}");
            let index = format!("@spread_index_{serial}");
            let scalar = format!("@spread_scalar_{serial}");
            let id = Expr::Ident(index.clone());
            // Remove the inserted axis from the result's column-major ordinal.
            let ordinal = bin(BinOp::Sub,id.clone(),int(1));
            let source_index = bin(BinOp::Add,int(1),bin(BinOp::Add,Expr::Call("modulo".into(),vec![ordinal.clone(),int(stride)]),bin(BinOp::Mul,bin(BinOp::Div,ordinal,int(stride*copies)),int(stride))));
            let (mut value,len,kind) = element(&args[0],&source_index,symbols)?;
            if len.is_none() {
                prefix.push(Stmt::VarDecl { kind:kind.clone(),names:vec![scalar.clone()] });
                prefix.push(Stmt::Assign { name:scalar.clone(),value });
                value = Expr::Ident(scalar);
            }
            prefix.push(Stmt::ArrayDecl { kind:kind.clone(),name:temp.clone(),dims:vec![int(count)] });
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()] });
            prefix.push(Stmt::Do { var:index,start:int(1),end:int(count),body:vec![Stmt::AssignIndex { name:temp.clone(),indices:vec![id],value }] });
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, args) if name == "@eoshift" => {
            lower_expr(&mut args[0],symbols,prefix,serial)?;
            lower_expr(&mut args[1],symbols,prefix,serial)?;
            let default_boundary = matches!(&args[5],Expr::Call(tag,_) if tag == "@no_boundary");
            if !default_boundary { lower_expr(&mut args[5],symbols,prefix,serial)?; }
            let (_,shift_len,shift_kind) = element(&args[1],&int(1),symbols)?;
            if !matches!(shift_kind,TypeSpec::Integer(_)) { bail!("EOSHIFT SHIFT must be INTEGER"); }
            let stride = constant(&args[2])?.max(1);
            let axis_len = constant(&args[3])?.max(1);
            *serial += 1;
            let temp = format!("@eoshift_array_{serial}");
            let index = format!("@eoshift_index_{serial}");
            let shift = format!("@eoshift_value_{serial}");
            let id = |n: &str| Expr::Ident(n.into());
            let modulo = |a,b| Expr::Call("modulo".into(),vec![a,b]);
            let ordinal = bin(BinOp::Sub,id(&index),int(1));
            let coordinate = modulo(bin(BinOp::Div,ordinal.clone(),int(stride)),int(axis_len));
            let slice = bin(BinOp::Add,int(1),bin(BinOp::Add,modulo(ordinal.clone(),int(stride)),bin(BinOp::Mul,bin(BinOp::Div,ordinal,int(stride*axis_len)),int(stride))));
            let source_index = bin(BinOp::Add,id(&index),bin(BinOp::Mul,id(&shift),int(stride)));
            let (value,count,kind) = element(&args[0],&source_index,symbols)?;
            let count = count.context("EOSHIFT ARRAY must be an array")?;
            let boundary = if default_boundary {
                match kind { TypeSpec::Logical => Expr::Logical(false), TypeSpec::Real => Expr::RealLit("0.0".into()), TypeSpec::DoublePrecision => Expr::RealLit("0.0d0".into()), _ => int(0) }
            } else {
                let (value,_,boundary_kind) = element(&args[5],&slice,symbols)?;
                if !same_kind(&kind,&boundary_kind) { bail!("EOSHIFT BOUNDARY and ARRAY must have the same type and kind"); }
                value
            };
            prefix.push(Stmt::ArrayDecl { kind:kind.clone(),name:temp.clone(),dims:vec![int(count)] });
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)),names:vec![index.clone(),shift.clone()] });
            let mut body = vec![];
            if shift_len.is_some() {
                let (amount,_,_) = element(&args[1],&slice,symbols)?;
                body.push(Stmt::Assign { name:shift.clone(),value:amount });
            } else { prefix.push(Stmt::Assign { name:shift.clone(),value:args[1].clone() }); }
            // Test bounds before adding/multiplying SHIFT, so extreme INTEGER(8)
            // shifts fill with BOUNDARY without overflowing or reading ARRAY.
            let valid = bin(BinOp::And,bin(BinOp::Ge,id(&shift),bin(BinOp::Sub,int(0),coordinate.clone())),bin(BinOp::Le,id(&shift),bin(BinOp::Sub,int(axis_len-1),coordinate)));
            let assign = |value| Stmt::AssignIndex { name:temp.clone(),indices:vec![id(&index)],value };
            body.push(Stmt::If { cond:valid,then_body:vec![assign(value)],else_body:Some(vec![assign(boundary)]) });
            prefix.push(Stmt::Do { var:index,start:int(1),end:int(count),body });
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, args) if name == "@matmul" => {
            // Column-major product: element (row, col) sums A(row, k) * B(k, col).
            lower_expr(&mut args[0], symbols, prefix, serial)?;
            lower_expr(&mut args[1], symbols, prefix, serial)?;
            let (rows, inner, cols) = (constant(&args[2])?, constant(&args[3])?, constant(&args[4])?);
            *serial += 1;
            let temp = format!("@matmul_array_{serial}");
            let [row, col, k] = ["row", "col", "inner"].map(|part| format!("@matmul_{part}_{serial}"));
            let id = |n: &str| Expr::Ident(n.into());
            let less = |n: &str| bin(BinOp::Sub, id(n), int(1));
            let a_index = bin(BinOp::Add, id(&row), bin(BinOp::Mul, less(&k), int(rows)));
            let b_index = bin(BinOp::Add, id(&k), bin(BinOp::Mul, less(&col), int(inner)));
            let (a, _, a_kind) = element(&args[0], &a_index, symbols)?;
            let (b, _, b_kind) = element(&args[1], &b_index, symbols)?;
            if matches!(a_kind, TypeSpec::Logical) || matches!(b_kind, TypeSpec::Logical) {
                bail!("MATMUL of LOGICAL arrays is not yet supported");
            }
            let kind = merge_kind(a_kind, b_kind);
            let target = vec![bin(BinOp::Add, id(&row), bin(BinOp::Mul, less(&col), int(rows)))];
            let item = Expr::Index(temp.clone(), target.clone());
            let zero = match kind { TypeSpec::Real => Expr::RealLit("0.0".into()), TypeSpec::DoublePrecision => Expr::RealLit("0.0d0".into()), _ => int(0) };
            prefix.push(Stmt::ArrayDecl { kind: kind.clone(), name: temp.clone(), dims: vec![int(rows * cols)] });
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![row.clone(), col.clone(), k.clone()] });
            let product = Stmt::AssignIndex { name: temp.clone(), indices: target.clone(), value: bin(BinOp::Add, item, bin(BinOp::Mul, a, b)) };
            let cell = vec![
                Stmt::AssignIndex { name: temp.clone(), indices: target, value: zero },
                Stmt::Do { var: k, start: int(1), end: int(inner), body: vec![product] },
            ];
            let column = vec![Stmt::Do { var: row, start: int(1), end: int(rows), body: cell }];
            prefix.push(Stmt::Do { var: col, start: int(1), end: int(cols), body: column });
            symbols.insert(temp.clone(), (kind, vec![int(rows * cols)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, args) if name == "@cshift" => {
            lower_expr(&mut args[0], symbols, prefix, serial)?;
            lower_expr(&mut args[1], symbols, prefix, serial)?;
            let (_,shift_len,shift_kind) = element(&args[1],&int(1),symbols)?;
            if !matches!(shift_kind,TypeSpec::Integer(_)) { bail!("CSHIFT SHIFT must be INTEGER"); }
            let stride = constant(&args[2])?.max(1);
            let axis_len = constant(&args[3])?.max(1);
            *serial += 1;
            let temp = format!("@shift_array_{serial}");
            let index = format!("@shift_index_{serial}");
            let shift = format!("@shift_value_{serial}");
            let id = |n: &str| Expr::Ident(n.into());
            let modulo = |a,b| Expr::Call("modulo".into(),vec![a,b]);
            // Normalize before adding the axis coordinate, avoiding overflow
            // from large positive or negative runtime shifts.
            let ordinal = bin(BinOp::Sub,id(&index),int(1));
            let coordinate = modulo(bin(BinOp::Div,ordinal.clone(),int(stride)),int(axis_len));
            let shifted = modulo(bin(BinOp::Add,coordinate.clone(),id(&shift)),int(axis_len));
            let source_index = bin(BinOp::Add,id(&index),bin(BinOp::Mul,bin(BinOp::Sub,shifted,coordinate),int(stride)));
            let (value,count,kind) = element(&args[0],&source_index,symbols)?;
            let count = count.context("CSHIFT ARRAY must be an array")?;
            prefix.push(Stmt::ArrayDecl { kind:kind.clone(),name:temp.clone(),dims:vec![int(count)] });
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)),names:vec![index.clone(),shift.clone()] });
            let mut body = vec![];
            if shift_len.is_some() {
                // Remove the shifted axis from the column-major ordinal.
                let slice = bin(BinOp::Add,int(1),bin(BinOp::Add,modulo(ordinal.clone(),int(stride)),bin(BinOp::Mul,bin(BinOp::Div,ordinal,int(stride*axis_len)),int(stride))));
                let (amount,_,_) = element(&args[1],&slice,symbols)?;
                body.push(Stmt::Assign { name:shift,value:modulo(amount,int(axis_len)) });
            } else {
                prefix.push(Stmt::Assign { name:shift,value:modulo(args[1].clone(),int(axis_len)) });
            }
            body.push(Stmt::AssignIndex { name:temp.clone(),indices:vec![id(&index)],value });
            prefix.push(Stmt::Do { var:index.clone(),start:int(1),end:int(count),body });
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, args) if name == "reshape" => {
            let dims = crate::multidim::reshape_shape(args)?;
            let order = crate::multidim::reshape_order(args,dims.len())?;
            let count = dims.iter().product::<i64>();
            lower_expr(&mut args[0], symbols, prefix, serial)?;
            let has_pad = !matches!(&args[2],Expr::Call(tag,_) if tag == "@no_pad");
            if has_pad { lower_expr(&mut args[2], symbols, prefix, serial)?; }
            *serial += 1;
            let temp = format!("@reshape_array_{serial}");
            let index = format!("@reshape_index_{serial}");
            let id = Expr::Ident(index.clone());
            let (value,len,kind) = element(&args[0],&id,symbols)?;
            let len = len.context("RESHAPE SOURCE must be an array")?;
            let pad = if has_pad {
                let (_,plen,pkind) = element(&args[2],&int(1),symbols)?;
                let plen = plen.context("RESHAPE PAD must be an array")?;
                if !same_kind(&kind,&pkind) { bail!("RESHAPE PAD and SOURCE must have the same type and kind"); }
                if count > len && plen == 0 { bail!("RESHAPE needs nonempty PAD when SOURCE has too few elements"); }
                let pad_index = bin(BinOp::Add,int(1),Expr::Call("modulo".into(),vec![bin(BinOp::Sub,id.clone(),int(len+1)),int(plen.max(1))]));
                Some(element(&args[2],&pad_index,symbols)?.0)
            } else { None };
            if count > len && pad.is_none() { bail!("RESHAPE SOURCE has too few elements; provide PAD"); }
            let mut strides = vec![];
            let mut stride = 1;
            for dim in &dims { strides.push(stride); stride *= dim; }
            let mut destination = int(1);
            let mut fill_stride = 1;
            for axis in order {
                let coordinate = Expr::Call("modulo".into(),vec![bin(BinOp::Div,bin(BinOp::Sub,id.clone(),int(1)),int(fill_stride.max(1))),int(dims[axis].max(1))]);
                destination = bin(BinOp::Add,destination,bin(BinOp::Mul,coordinate,int(strides[axis])));
                fill_stride *= dims[axis];
            }
            let assign = |value| Stmt::AssignIndex { name:temp.clone(),indices:vec![destination.clone()],value };
            let body = if count > len {
                vec![Stmt::If { cond:bin(BinOp::Le,id.clone(),int(len)),then_body:vec![assign(value)],else_body:Some(vec![assign(pad.unwrap())]) }]
            } else { vec![assign(value)] };
            prefix.push(Stmt::ArrayDecl { kind:kind.clone(),name:temp.clone(),dims:vec![int(count)] });
            prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()] });
            prefix.push(Stmt::Do { var:index,start:int(1),end:int(count),body });
            symbols.insert(temp.clone(),(kind,vec![int(count)]));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, args) if name == "@bounds_vector" => {
            *serial += 1;
            let temp = format!("@bounds_{serial}");
            let dims = vec![int(args.len() as i64)];
            prefix.push(Stmt::ArrayDecl { name: temp.clone(), kind: TypeSpec::Integer(None), dims: dims.clone() });
            for (i, value) in args.iter().enumerate() {
                prefix.push(Stmt::AssignIndex { name: temp.clone(), indices: vec![int(i as i64 + 1)], value: value.clone() });
            }
            symbols.insert(temp.clone(), (TypeSpec::Integer(None), dims));
            *e = Expr::Ident(temp);
        }
        Expr::Call(name, _) if name == "@output_implied_do" => bail!("implied-DO is currently supported only in list-directed output"),
        Expr::Call(name, args) if reduction(name) => {
            if args.is_empty() || (args.len() > 2 && args.len() != 5) { bail!("{name} requires normalized ARRAY and optional MASK arguments"); }
            lower_expr(&mut args[0], symbols, prefix, serial)?;
            if args.len() >= 2 { lower_expr(&mut args[1], symbols, prefix, serial)?; }
            *serial += 1;
            // These names cannot occur in source identifiers.
            let index_name = format!("@reduction_index_{serial}");
            let acc_name = format!("@reduction_value_{serial}");
            let item_name = format!("@reduction_item_{serial}");
            let outer_name = format!("@reduction_outer_{serial}");
            let output_name = format!("@reduction_array_{serial}");
            let dim_reduction = if args.len() == 5 {
                let stride = constant(&args[2])?;
                let axis_len = constant(&args[3])?;
                let Expr::Call(_, dims) = &args[4] else { unreachable!() };
                let output_len = dims.iter().try_fold(1i64, |n,d| n.checked_mul(constant(d)?).context("reduction result extent overflow"))?;
                Some((stride, axis_len, output_len))
            } else { None };
            let index = if let Some((stride, axis_len, _)) = dim_reduction {
                let ordinal = bin(BinOp::Sub, Expr::Ident(outer_name.clone()), int(1));
                let offset = bin(BinOp::Add,
                    Expr::Call("mod".into(), vec![ordinal.clone(), int(stride.max(1))]),
                    bin(BinOp::Mul, bin(BinOp::Div, ordinal, int(stride.max(1))), int(stride.checked_mul(axis_len).context("reduction stride overflow")?)));
                bin(BinOp::Add, int(1), bin(BinOp::Add, offset, bin(BinOp::Mul, bin(BinOp::Sub, Expr::Ident(index_name.clone()), int(1)), int(stride))))
            } else { Expr::Ident(index_name.clone()) };
            let (value, count, kind) = element(&args[0], &index, symbols)?;
            let count = count.context("reduction argument must be an array")?;
            let dynamic_end = runtime_extent(&args[0]);
            if dynamic_end.is_some() && args.len()!=1 { bail!("runtime PACK reductions with DIM or MASK are not yet supported"); }
            let name = match name.as_str() {
                "@count_default" => "count", "@any_reduction" => "any", "@all_reduction" => "all", _ => name.as_str()
            }.to_ascii_lowercase();
            if matches!(name.as_str(), "any" | "all" | "count" | "@count_i8" | "parity") != matches!(kind, TypeSpec::Logical) { bail!("reduction argument has incorrect numeric/logical type"); }
            if matches!(name.as_str(), "iany" | "iall" | "iparity") && !matches!(kind, TypeSpec::Integer(_)) { bail!("{}: ARRAY must be INTEGER", name.to_ascii_uppercase()); }
            let result_kind = match name.as_str() { "count" => TypeSpec::Integer(None), "@count_i8" => TypeSpec::Integer(Some(8)), _ => kind.clone() };
            let integer = matches!(kind, TypeSpec::Integer(_));
            let integer_max = match kind { TypeSpec::Integer(Some(1)) => i8::MAX as i64, TypeSpec::Integer(Some(2)) => i16::MAX as i64, TypeSpec::Integer(Some(8)) => i64::MAX, _ => i32::MAX as i64 };
            let integer_min = -integer_max - 1;
            let huge = if matches!(kind, TypeSpec::DoublePrecision) { f64::MAX } else { f32::MAX as f64 };
            let identity = match name.as_str() {
                "count" | "@count_i8" => int(0),
                "any" | "parity" => Expr::Logical(false),
                "iany" | "iparity" => int(0),
                "iall" => int(-1),
                "all" => Expr::Logical(true),
                "sum" => if integer { int(0) } else { Expr::RealLit("0.0".into()) },
                "product" => if integer { int(1) } else { Expr::RealLit("1.0".into()) },
                "minval" => if integer { int(integer_max) } else { Expr::RealLit(huge.to_string()) },
                _ => if integer { int(integer_min) } else { Expr::RealLit((-huge).to_string()) },
            };
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index_name.clone()] });
            prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![item_name.clone()] });
            prefix.push(Stmt::VarDecl { kind: result_kind.clone(), names: vec![acc_name.clone()] });
            symbols.insert(acc_name.clone(), (result_kind.clone(), vec![]));
            let loop_start = prefix.len();
            prefix.push(Stmt::Assign { name: acc_name.clone(), value: identity });
            let acc = Expr::Ident(acc_name.clone());
            let item = Expr::Ident(item_name.clone());
            let update = match name.as_str() {
                "count" | "@count_i8" => Stmt::If {
                    cond: item.clone(),
                    then_body: vec![Stmt::Assign { name: acc_name.clone(), value: bin(BinOp::Add, acc.clone(), int(1)) }],
                    else_body: None,
                },
                "any" | "all" => Stmt::Assign { name: acc_name.clone(), value: bin(if name == "any" { BinOp::Or } else { BinOp::And }, acc.clone(), item.clone()) },
                "parity" => Stmt::Assign { name: acc_name.clone(), value: bin(BinOp::Neqv, acc.clone(), item.clone()) },
                "iany" | "iall" | "iparity" => {
                    let TypeSpec::Integer(bytes) = &kind else { unreachable!() };
                    let operation = match name.as_str() { "iany" => "ior", "iall" => "iand", _ => "ieor" };
                    let tag = crate::bit_intrinsics::tag(operation, bytes.unwrap_or(4));
                    Stmt::Assign { name: acc_name.clone(), value: Expr::Call(tag, vec![acc.clone(), item.clone()]) }
                },
                "sum" | "product" => Stmt::Assign { name: acc_name.clone(), value: bin(if name == "sum" { BinOp::Add } else { BinOp::Mul }, acc.clone(), item.clone()) },
                _ => Stmt::If {
                    cond: bin(if name == "minval" { BinOp::Lt } else { BinOp::Gt }, item.clone(), acc.clone()),
                    then_body: vec![Stmt::Assign { name: acc_name.clone(), value: item.clone() }], else_body: None,
                },
            };
            let mut body = vec![Stmt::Assign { name: item_name, value }, update];
            if let Some(mask) = args.get(1) {
                let (condition, mask_count, mask_kind) = element(mask, &index, symbols)?;
                if !matches!(mask_kind, TypeSpec::Logical) { bail!("{name}: MASK must be LOGICAL"); }
                if mask_count.is_some_and(|n| n != count) { bail!("{name}: MASK shape does not conform to ARRAY"); }
                body = vec![Stmt::If { cond: condition, then_body: body, else_body: None }];
            }
            prefix.push(Stmt::Do { var: index_name, start: int(1), end: dynamic_end.unwrap_or_else(|| int(dim_reduction.map_or(count, |(_,n,_)| n))), body });
            if let Some((_, _, output_len)) = dim_reduction {
                let mut body = prefix.split_off(loop_start);
                body.push(Stmt::AssignIndex { name: output_name.clone(), indices: vec![Expr::Ident(outer_name.clone())], value: acc });
                prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![outer_name.clone()] });
                prefix.push(Stmt::ArrayDecl { kind: result_kind.clone(), name: output_name.clone(), dims: vec![int(output_len)] });
                prefix.push(Stmt::Do { var: outer_name, start: int(1), end: int(output_len), body });
                symbols.insert(output_name.clone(), (result_kind, vec![int(output_len)]));
                *e = Expr::Ident(output_name);
            } else { *e = acc; }
        }
        Expr::Call(name, args) if name == "len" => {
            let length = match args.as_slice() {
                [Expr::Str(s)] => s.len(),
                [Expr::Call(n,a)] if n == "@substring" => constant(&a[2])? as usize,
                [Expr::Ident(n)] => match symbols.get(n) {
                    Some((TypeSpec::Character(length), dims)) if dims.is_empty() => length.unwrap_or(1),
                    _ => bail!("LEN currently requires a scalar CHARACTER variable or constant"),
                },
                _ => bail!("LEN currently requires one scalar CHARACTER variable or constant"),
            };
            *e = int(length as i64);
        }
        Expr::Call(name, args) if crate::inquiries::numeric_model(name) && !symbols.contains_key(name) && !symbols.procedures.contains(name) => {
            let [arg] = args.as_slice() else { bail!("{name} expects one argument"); };
            let kind=crate::inquiries::model_type(arg,&|n|symbols.get(n).map(|(k,d)|(k.clone(),d.len())))?
                .with_context(||format!("{name}: unsupported or undefined inquiry argument type"))?;
            *e=crate::inquiries::model_value(name,&kind)?;
        }
        Expr::Call(name, args) if matches!(name.as_str(), "tiny" | "huge" | "kind" | "size") => {
            if name == "kind" && args.len() == 1 {
                if let Expr::Call(inquiry, inner) = &args[0] {
                    if inquiry == "huge" {
                        let [Expr::Ident(n)] = inner.as_slice() else { bail!("HUGE currently requires a typed variable or supported literal"); };
                        let (kind, _) = symbols.get(n).context("undefined inquiry argument")?;
                        match kind {
                            TypeSpec::Integer(bytes) => { crate::inquiries::integer_huge(*bytes)?; },
                            TypeSpec::Real | TypeSpec::DoublePrecision => {},
                            _ => bail!("HUGE requires an INTEGER or REAL argument"),
                        }
                        args[0] = Expr::Ident(n.clone());
                    }
                }
            }
            let [Expr::Ident(n)] = args.as_slice() else { bail!("{name} currently requires a typed variable or supported literal"); };
            let (kind,dims)=symbols.get(n).context("undefined inquiry argument")?;
            if name == "huge" {
                if let TypeSpec::Integer(bytes) = kind {
                    let value = crate::inquiries::integer_huge(*bytes)?;
                    let kind = kind.clone();
                    *serial += 1;
                    let temp = format!("@integer_huge_{serial}");
                    prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![temp.clone()] });
                    prefix.push(Stmt::Assign { name: temp.clone(), value: int(value) });
                    symbols.insert(temp.clone(), (kind, vec![]));
                    *e = Expr::Ident(temp);
                    return Ok(());
                }
            }
            let double=matches!(kind,TypeSpec::DoublePrecision);
            *e=match name.as_str() {
                "size" => { if dims.len()!=1 {bail!("SIZE requires a rank-one array");} dims[0].clone() },
                "kind"=>int(match kind { TypeSpec::Integer(bytes) => i64::from(bytes.unwrap_or(4)), _ => if double {8} else {4} }),
                _=> { if !matches!(kind,TypeSpec::Real|TypeSpec::DoublePrecision) { if name=="huge" {bail!("HUGE requires an INTEGER or REAL argument");} bail!("{name} requires a REAL argument");} let v=if name=="tiny" {if double {f64::MIN_POSITIVE} else {f32::MIN_POSITIVE as f64}} else {if double {f64::MAX} else {f32::MAX as f64}}; let lit=Expr::RealLit(v.to_string()); if double {Expr::Call("dble".into(),vec![lit])} else {lit} }
            };
        }
        Expr::Call(tag, _) if matches!(tag.as_str(),"@matrix_view" | "@packed_view") => {},
        Expr::Call(name, args) | Expr::Index(name, args) => {
            for arg in args.iter_mut() { lower_expr_impl(arg, symbols, prefix, serial, expand_elementals)?; }
            if symbols.get(name).is_some_and(|(_, dims)| !dims.is_empty()) && args.iter().any(|arg|array_valued(arg,symbols)) {
                bail!("this vector subscript expression is not yet supported");
            }
        },
        Expr::Bin(op, a, b) => {
            lower_expr_impl(a, symbols, prefix, serial, expand_elementals)?;
            lower_expr_impl(b, symbols, prefix, serial, expand_elementals)?;
            if matches!(op,BinOp::Eq|BinOp::Ne|BinOp::Lt|BinOp::Le|BinOp::Gt|BinOp::Ge) {
                if let Some(comparison) = lower_character_comparison(op.clone(),a,b,symbols,prefix,serial)? {
                    *e = comparison;
                    return Ok(());
                }
            }
            if runtime_extent(a).is_some() || runtime_extent(b).is_some() {
                let extent = |e: &Expr| -> Result<Option<Expr>> { Ok(runtime_extent(e).or(element(e,&int(1),symbols)?.1.map(int))) };
                if let (Some(a),Some(b)) = (extent(a)?,extent(b)?) { prefix.push(shape_guard(a,b)); }
            }
        }
        Expr::Un(_, a) => lower_expr_impl(a, symbols, prefix, serial, expand_elementals)?,
        _ => {}
    }
    if let Expr::Call(name,args) = e {
        if crate::real_intrinsics::keys(name).is_some() && !symbols.contains_key(name) && !symbols.procedures.contains(name) {
            let arity=crate::real_intrinsics::keys(name).unwrap().len();
            if args.len()!=arity {bail!("{} expects {arity} arguments, got {}",name.to_ascii_uppercase(),args.len());}
            let kinds=args.iter().map(|arg|element(arg,&int(1),symbols).map(|(_,_,kind)|kind)).collect::<Result<Vec<_>>>()?;
            crate::real_intrinsics::result_kind(name,&kinds)?;
            if name=="nearest" && crate::parameters::real_constant(&args[1]).ok()==Some(0.0) {bail!("NEAREST S must not be zero");}
            *name=crate::real_intrinsics::tag(name,if kinds[0]==TypeSpec::DoublePrecision {8}else{4});
        }
        if let Some(lengths) = symbols.character_functions.get(name).cloned() {
            if args.len()!=lengths.len() { bail!("wrong number of arguments to CHARACTER-argument function `{name}`"); }
            for (arg,length) in args.iter_mut().zip(lengths) {
                if let Some(length) = length {
                    fn fixed(e: &Expr) -> bool { match e {
                        Expr::Str(_) | Expr::Ident(_) => true,
                        Expr::Bin(BinOp::Concat,a,b) => fixed(a) && fixed(b),
                        _ => false,
                    }}
                    let actual=character_capacity(arg,symbols)?.context("function argument requires scalar CHARACTER text")?;
                    if !fixed(arg) { bail!("CHARACTER function arguments currently require fixed-length actuals"); }
                    if actual<length { bail!("CHARACTER function actual is shorter than its dummy argument"); }
                    *serial+=1;
                    let temp=format!("@character_argument_{serial}");
                    let kind=TypeSpec::Character(Some(length));
                    prefix.push(Stmt::VarDecl {kind:kind.clone(),names:vec![temp.clone()]});
                    prefix.push(Stmt::CallSub {name:"@character_assign".into(),args:vec![Expr::Ident(temp.clone()),arg.clone(),int(length as i64)]});
                    symbols.insert(temp.clone(),(kind,vec![]));
                    *arg=Expr::Ident(temp);
                }
            }
        }
        if name != "@packed_view" && args.iter().any(|arg| runtime_extent(arg).is_some()) {
            if symbols.elementals.contains_key(name) || crate::intrinsics::math_arity(name).is_some() || crate::intrinsics::integer_rounding(name) || crate::intrinsics::real_predicate(name) || crate::real_intrinsics::decode(name).is_some()
                || matches!(name.as_str(), "real" | "dble" | "int" | "abs" | "mod" | "modulo" | "sign") || numeric_minmax(name,symbols) {
                let length=args.iter().find_map(runtime_extent).unwrap();
                for arg in args.iter() {
                    let other=runtime_extent(arg).or(element(arg,&int(1),symbols)?.1.map(int));
                    if let Some(other)=other {prefix.push(shape_guard(length.clone(),other));}
                }
                *serial+=1;
                let index=format!("@runtime_elemental_index_{serial}");
                let temp=format!("@runtime_elemental_values_{serial}");
                let (value,_,kind)=element(e,&Expr::Ident(index.clone()),symbols)?;
                crate::heap::temporary(kind.clone(),temp.clone(),length.clone(),prefix);
                prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:length.clone(),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![Expr::Ident(index)],value}]});
                symbols.insert(temp.clone(),(kind,vec![int(1)]));
                *e=Expr::Call("@packed_view".into(),vec![Expr::Ident(temp),length]);
                return Ok(());
            }
            bail!("runtime PACK results are not yet supported as arguments to `{name}`");
        }
    }
    if expand_elementals && matches!(e,Expr::Call(name,_) if crate::real_intrinsics::decode(name).is_some() || crate::bit_intrinsics::decode(name).is_some() || symbols.elementals.contains_key(name) || matches!(name.as_str(), "mod" | "modulo" | "sign") || numeric_minmax(name,symbols) || (crate::intrinsics::integer_rounding(name) || crate::intrinsics::real_predicate(name)) && !symbols.contains_key(name) && !symbols.procedures.contains(name)) && array_valued(e,symbols) {
        *serial+=1;
        let index_name=format!("@elemental_index_{serial}");
        let temp=format!("@elemental_result_{serial}");
        let index=Expr::Ident(index_name.clone());
        let (value,count,kind)=element(e,&index,symbols)?;
        let count=count.context("ELEMENTAL array expression has no known shape")?;
        prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index_name.clone()]});
        prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(count)]});
        prefix.push(Stmt::Do{var:index_name,start:int(1),end:int(count),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![index],value}]});
        symbols.insert(temp.clone(),(kind,vec![int(count)]));
        *e=Expr::Ident(temp);
    }
    Ok(())
}

fn array_valued(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Ident(name) => symbols.get(name).is_some_and(|(_, dims)| !dims.is_empty()),
        Expr::Section { .. } => true,
        Expr::Call(name,_) if name == "@spread" => true,
        Expr::Bin(_, a, b) => array_valued(a, symbols) || array_valued(b, symbols),
        Expr::Un(_, a) => array_valued(a, symbols),
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(|arg| array_valued(arg, symbols)),
        _ => false,
    }
}

// WHERE masks are captured before executing a branch. Each assignment then
// snapshots only its selected RHS elements before writing any destination.
fn lower_where(body: &mut Vec<Stmt>, symbols: &mut Symbols, serial: &mut usize) -> Result<()> {
    lower_where_mask(body, symbols, serial, None)
}

fn lower_where_mask(body: &mut Vec<Stmt>, symbols: &mut Symbols, serial: &mut usize, parent: Option<(&str,i64)>) -> Result<()> {
    if !matches!(body.first(), Some(Stmt::CallSub{name,..}) if name == "@where") { return Ok(()); }
    *serial += 1;
    let index = format!("@where_index_{serial}");
    let active = format!("@where_active_{serial}");
    let remaining = format!("@where_remaining_{serial}");
    let id = |n: &str| Expr::Ident(n.into());
    let item = |n: &str| Expr::Index(n.into(), vec![id(&index)]);
    let set = |n: &str, value| Stmt::AssignIndex {name:n.into(),indices:vec![id(&index)],value};
    let mut output = vec![Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]}];
    let mut extent = None;
    let mut first = true;
    for stmt in std::mem::take(body) {
        match stmt {
            Stmt::SourceLocation(_) => output.push(stmt),
            Stmt::CallSub{name, mut args} if name == "@where" || name == "@elsewhere" => {
                let mut prefix = vec![];
                let (value,count,kind) = if let Some(mask) = args.first_mut() {
                    lower_expr_impl(mask,symbols,&mut prefix,serial,false)?;
                    element(mask,&id(&index),symbols)?
                } else { (Expr::Logical(true),extent,TypeSpec::Logical) };
                if kind != TypeSpec::Logical { bail!("WHERE mask must be a LOGICAL array"); }
                let count=count.context("WHERE mask must be an array")?;
                if parent.is_some_and(|(_,n)| n != count) { bail!("nested WHERE mask shapes do not conform"); }
                if let Some(n)=extent { if n != count {bail!("WHERE mask shapes do not conform");} }
                else {
                    extent=Some(count);
                    for name in [&active,&remaining] {
                        output.push(Stmt::ArrayDecl{kind:TypeSpec::Logical,name:name.clone(),dims:vec![int(count)]});
                    }
                }
                output.extend(prefix);
                let mut capture = if first {
                    if let Some((parent,_)) = parent {
                        vec![set(&active,Expr::Logical(false)),Stmt::If{cond:item(parent),then_body:vec![set(&active,value)],else_body:None}]
                    } else { vec![set(&active,value)] }
                } else {
                    vec![set(&active,Expr::Logical(false)),Stmt::If{cond:item(&remaining),then_body:vec![set(&active,value)],else_body:None}]
                };
                let not_active=Expr::Un(UnOp::Not,Box::new(item(&active)));
                capture.push(set(&remaining,if first {
                    if let Some((parent,_)) = parent { bin(BinOp::And,item(parent),not_active) } else { not_active }
                } else {bin(BinOp::And,item(&remaining),not_active)}));
                output.push(Stmt::Do{var:index.clone(),start:int(1),end:int(count),body:capture});
                first=false;
            }
            Stmt::Assign{name,mut value} => {
                let count=extent.unwrap();
                let (kind,dims)=symbols.get(&name).context("WHERE destination must be a declared array")?.clone();
                if dims.len()!=1 || !array_element_kind(&kind) {bail!("WHERE currently requires a whole numeric or LOGICAL array destination");}
                if constant(&dims[0])? != count {bail!("WHERE destination shape does not conform to mask");}
                let mut prefix=vec![];
                lower_expr_impl(&mut value,symbols,&mut prefix,serial,false)?;
                let (value,rhs_count,rhs_kind)=element(&value,&id(&index),symbols)?;
                if rhs_count.is_some_and(|n|n!=count) {bail!("WHERE assignment shapes do not conform");}
                if matches!(kind,TypeSpec::Logical) != matches!(rhs_kind,TypeSpec::Logical) {
                    bail!("WHERE assignment has incompatible numeric/LOGICAL types");
                }
                *serial+=1;
                let temp=format!("@where_value_{serial}");
                output.extend(prefix);
                output.push(Stmt::ArrayDecl{kind,name:temp.clone(),dims:vec![int(count)]});
                output.push(Stmt::Do{var:index.clone(),start:int(1),end:int(count),body:vec![Stmt::If{
                    cond:item(&active),then_body:vec![set(&temp,value)],else_body:None,
                }]});
                output.push(Stmt::Do{var:index.clone(),start:int(1),end:int(count),body:vec![Stmt::If{
                    cond:item(&active),then_body:vec![set(&name,item(&temp))],else_body:None,
                }]});
            }
            Stmt::Block { mut body } if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name == "@where") => {
                lower_where_mask(&mut body,symbols,serial,Some((&active,extent.unwrap())))?;
                output.push(Stmt::Block { body });
            }
            _ => bail!("WHERE currently supports whole-array assignments and nested WHERE; section destinations are not yet supported"),
        }
    }
    *body=output;
    Ok(())
}

// A captured I/O item can become array-valued after vector selection or
// function specialization. Emit every active element into the same session.
// Internal calls whose lowering produces an array.
fn array_call(name: &str) -> bool {
    matches!(name, "@spread" | "@cshift" | "@eoshift" | "@pack" | "@unpack" | "@bounds_vector" | "@array_constructor" | "@matmul")
}
fn formatted_items(handle:&str,items:&[Expr],symbols:&Symbols,serial:&mut usize)->Result<Vec<Stmt>> {
    let mut body=vec![];
    let mut local=symbols.clone();
    for item in items {
        // Other array expressions, such as constructors and CSHIFT results,
        // are materialized in a temporary before their elements are written.
        let mut materialized=None;
        if !element(item,&int(1),&local).is_ok_and(|(_,shape,_)|shape.is_some())
            && (array_valued(item,&local) || matches!(item,Expr::Call(n,_) if array_call(n))) {
            let mut captured=item.clone();let mut prefix=vec![];
            if let Ok((_,count,length))=argument_with_elementals(&mut captured,&local.variables,&mut prefix,serial,&local.elementals) {
                if count>=0 {materialized=Some((captured,prefix,length));}
            }
        }
        if let Some((captured,prefix,length))=materialized {
            let start=body.len();body.extend(prefix);
            for s in &body[start..] {match s {
                Stmt::ArrayDecl{kind,name,dims}=>{local.insert(name.clone(),(kind.clone(),dims.clone()));},
                Stmt::VarDecl{kind,names}=>for n in names {local.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
            }}
            *serial+=1;let index=format!("@formatted_array_index_{serial}");
            let value=argument_element(&captured,&Expr::Ident(index.clone()),&local.variables)?;
            body.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
            body.push(Stmt::Do{var:index,start:int(1),end:length,body:vec![Stmt::CallSub{name:"@format_item".into(),args:vec![Expr::Ident(handle.to_owned()),value]}]});
        } else if element(item,&int(1),&local).is_ok_and(|(_,shape,_)|shape.is_some()) {
            let mut captured=item.clone();let start=body.len();
            let (_,_,length)=argument_with_elementals(&mut captured,&local.variables,&mut body,serial,&local.elementals)?;
            for s in &body[start..] {match s {
                Stmt::ArrayDecl{kind,name,dims}=>{local.insert(name.clone(),(kind.clone(),dims.clone()));},
                Stmt::VarDecl{kind,names}=>for n in names {local.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
            }}
            *serial+=1;let index=format!("@formatted_array_index_{serial}");
            let value=argument_element(&captured,&Expr::Ident(index.clone()),&local.variables)?;
            body.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
            body.push(Stmt::Do{var:index,start:int(1),end:length,body:vec![Stmt::CallSub{name:"@format_item".into(),args:vec![Expr::Ident(handle.to_owned()),value]}]});
        } else {body.push(Stmt::CallSub{name:"@format_item".into(),args:vec![Expr::Ident(handle.to_owned()),item.clone()]});}
    }
    Ok(body)
}

fn formatted_capture(format:Expr,items:&[Expr],advance:Expr,symbols:&Symbols,serial:&mut usize)->Result<Vec<Stmt>> {
    *serial+=1;
    let handle=format!("@formatted_array_list_{serial}");
    let mut body=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![handle.clone()]},
        Stmt::CallSub{name:"@format_begin".into(),args:vec![Expr::Ident(handle.clone()),format,advance]}];
    body.extend(formatted_items(&handle,items,symbols,serial)?);
    body.push(Stmt::CallSub{name:"@format_end".into(),args:vec![Expr::Ident(handle)]});
    lower_body(&mut body,symbols,serial)?;
    Ok(body)
}

pub(crate) fn advance_control(value:&Expr,variables:&HashMap<String,(TypeSpec,Vec<Expr>)>,prefix:&mut Vec<Stmt>,serial:&mut usize)->Result<Expr> {
    if let Ok(value)=crate::parameters::character_constant(value) {
        return match value.trim_end().to_ascii_lowercase().as_str() {
            "yes"=>Ok(int(1)),"no"=>Ok(int(0)),_=>bail!("ADVANCE must be 'yes' or 'no'"),
        };
    }
    let mut symbols=Symbols::new();symbols.variables=variables.clone();
    let width=character_capacity(value,&symbols)?.context("ADVANCE must be scalar CHARACTER with a known storage length")?;
    let start=prefix.len();
    *serial+=1;let text=format!("@advance_text_{serial}");let flag=format!("@advance_flag_{serial}");
    prefix.extend([Stmt::VarDecl{kind:TypeSpec::Character(Some(width)),names:vec![text.clone()]},
        Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![flag.clone()]},
        Stmt::Assign{name:text.clone(),value:value.clone()}]);
    let cases=|words:&[&str]|words.iter().map(|word|bin(BinOp::Eq,Expr::Call("trim".into(),vec![Expr::Ident(text.clone())]),Expr::Str((*word).into())))
        .reduce(|a,b|bin(BinOp::Or,a,b)).unwrap();
    let yes=cases(&["yes","yeS","yEs","yES","Yes","YeS","YEs","YES"]);
    let no=cases(&["no","nO","No","NO"]);
    prefix.push(Stmt::If{cond:Expr::Un(UnOp::Not,Box::new(bin(BinOp::Or,yes.clone(),no))),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("ADVANCE must be 'yes' or 'no'".into())]}],else_body:None});
    prefix.push(Stmt::If{cond:yes,then_body:vec![Stmt::Assign{name:flag.clone(),value:int(1)}],else_body:Some(vec![Stmt::Assign{name:flag.clone(),value:int(0)}])});
    let mut control=prefix.split_off(start);
    lower_body(&mut control,&symbols,serial)?;
    prefix.extend(control);
    Ok(Expr::Ident(flag))
}

fn lower_body(body: &mut Vec<Stmt>, inherited: &Symbols, serial: &mut usize) -> Result<()> {
    let mut symbols = inherited.clone();
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { kind, names } => for name in names { symbols.insert(name.clone(), (kind.clone(), vec![])); },
            Stmt::ArrayDecl { kind, name, dims } => { symbols.insert(name.clone(), (kind.clone(), dims.clone())); }
            _ => {}
        }
    }
    let mut output = Vec::new();
    let mut source_offset=None;
    (||->Result<()> {
    for mut stmt in std::mem::take(body) {
        if let Stmt::SourceLocation(offset)=&stmt {source_offset=Some(*offset);}
        let mut prefix = Vec::new();
        if let Stmt::Assign { name, value } = &stmt {
            if let Some((TypeSpec::Character(length), dims)) = symbols.get(name) {
                if dims.is_empty() {
                    fn check(e: &Expr, symbols: &Symbols) -> Result<()> {
                        match e {
                            Expr::Str(s) if s.is_ascii() && !s.contains('\0') => Ok(()),
                            Expr::Ident(n) if matches!(symbols.get(n), Some((TypeSpec::Character(_), d)) if d.is_empty()) => Ok(()),
                            Expr::Bin(BinOp::Concat, a, b) => { check(a, symbols)?; check(b, symbols) },
                            Expr::Call(n, a) if matches!(n.as_str(),"trim"|"adjustl"|"adjustr") && a.len() == 1 => check(&a[0], symbols),
                            Expr::Call(n, a) if n == "repeat" && a.len() == 2 => {
                                check(&a[0], symbols)?;
                                let (_, rank, kind) = element(&a[1], &int(1), symbols)?;
                                if rank.is_some() || !matches!(kind, TypeSpec::Integer(_)) {
                                    bail!("REPEAT NCOPIES must be a scalar INTEGER");
                                }
                                Ok(())
                            },
                            Expr::Call(n, a) if n == "@substring" && a.len() == 3 => check(&a[0], symbols),
                            Expr::Call(n, a) if n == "@dynamic_substring" && a.len() == 4 => check(&a[0], symbols),
                            _ => bail!("CHARACTER assignment requires a non-NUL ASCII literal, scalar CHARACTER variable, concatenation, or a supported CHARACTER intrinsic"),
                        }
                    }
                    check(value, &symbols)?;
                    stmt = Stmt::CallSub { name: "@character_assign".into(), args: vec![Expr::Ident(name.clone()), value.clone(), int(length.unwrap_or(1) as i64)] };
                }
            }
        }
        if let Stmt::CallSub {name,args}=&stmt {
            if name=="@random_seed_put_array" {
                let mut actual=args[0].clone();
                let (kind,_,length)=argument_with_elementals(&mut actual,&symbols.variables,&mut prefix,serial,&symbols.elementals)?;
                if !matches!(kind,TypeSpec::Integer(None|Some(4))) {
                    bail!("RANDOM_SEED PUT requires a default INTEGER rank-one array expression");
                }
                // Generated arrays must be visible when selecting their elements.
                let mut captured=symbols.variables.clone();
                for declaration in &prefix {
                    if let Stmt::ArrayDecl {kind,name,dims}=declaration {
                        captured.insert(name.clone(),(kind.clone(),dims.clone()));
                    }
                }
                prefix.push(Stmt::If {cond:bin(BinOp::Lt,length,int(2)),then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str("RANDOM_SEED PUT array must have at least 2 elements".into())]}],else_body:None});
                let words=[1,2].into_iter().map(|i|argument_element(&actual,&int(i),&captured)).collect::<Result<Vec<_>>>()?;
                prefix.push(Stmt::CallSub {name:"@random_seed_put".into(),args:words});
                output.push(Stmt::Block {body:prefix});
                continue;
            }
        }
        if let Stmt::CallSub {name,args}=&stmt {
            if name=="@format_item" && args.len()==2 && element(&args[1],&int(1),&symbols).is_ok_and(|(_,shape,_)|shape.is_some()) {
                let Expr::Ident(handle)=&args[0] else {bail!("invalid formatted output session");};
                let mut expanded=formatted_items(handle,&args[1..],&symbols,serial)?;
                lower_body(&mut expanded,&symbols,serial)?;
                output.push(Stmt::Block{body:expanded});
                continue;
            }
            if name=="@format_begin" {
                if character_capacity(&args[1],&symbols)?.is_none() {
                    bail!("WRITE/PRINT format must be scalar CHARACTER");
                }
                if let Ok(format)=crate::parameters::character_constant(&args[1]) {
                    crate::format_spec::GroupParser::new(format.as_bytes(),0).map_err(anyhow::Error::msg)?;
                }
            }
            if name=="@write" {
                if crate::parameters::integer_constant(&args[0]).ok()!=Some(6) {
                    crate::file_io::unit(&args[0], &symbols.variables)?;
                    let simple=crate::parameters::character_constant(&args[1]).is_ok_and(|f|f.chars().filter(|c|!c.is_whitespace()).collect::<String>().eq_ignore_ascii_case("(a)"))
                        && !args[2..].iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@write_option:")));
                    if simple {
                    let items=if args.len()==2 {vec![Expr::Str(String::new())]} else {args[2..].to_vec()};
                    let mut body=Vec::new();
                    for item in items {
                        if character_capacity(&item,&symbols)?.is_none() {bail!("(A) file WRITE requires CHARACTER items");}
                        body.push(Stmt::CallSub {name:"@file_write_record".into(),args:vec![args[0].clone(),item]});
                    }
                    if crate::parameters::integer_constant(&args[0]).is_err() {
                        // The file (A) subset writes one record per item. Preserve
                        // those records on stdout without requiring format reversion.
                        let mut then_body=body.iter().map(|stmt| {
                            let Stmt::CallSub{args:record,..}=stmt else {unreachable!()};
                            Stmt::CallSub{name:"@write".into(),args:vec![int(6),args[1].clone(),record[1].clone()]}
                        }).collect();
                        lower_body(&mut then_body,&symbols,serial)?;
                        output.push(Stmt::If{cond:bin(BinOp::Eq,args[0].clone(),int(6)),then_body,else_body:Some(body)});
                    } else {output.push(Stmt::Block {body});}
                    continue;
                    }
                    let options=args[2..].iter().any(|e|matches!(e,Expr::Call(n,_) if n.starts_with("@write_option:")));
                    if matches!(&args[1],Expr::Call(n,_) if n=="@list_directed") && !options {
                        // List-directed file output: a blank, then each item as G0
                        // separated by blanks, which list-directed READ accepts.
                        let format=Expr::Str("(*(1x,g0))".into());
                        let mut file_body=formatted_capture(format,&args[2..],int(1),&symbols,serial)?;
                        if let Some(Stmt::CallSub{name, args:end})=file_body.last_mut() {if name=="@format_end" {*name="@format_end_file".into();end.push(args[0].clone());}}
                        if crate::parameters::integer_constant(&args[0]).is_ok() {output.push(Stmt::Block{body:file_body});}
                        else {
                            let mut stdout=vec![Stmt::Print{items:args[2..].to_vec()}];
                            lower_body(&mut stdout,&symbols,serial)?;
                            output.push(Stmt::If{cond:bin(BinOp::Eq,args[0].clone(),int(6)),then_body:stdout,else_body:Some(vec![Stmt::Block{body:file_body}])});
                        }
                        continue;
                    }
                    if matches!(&args[1],Expr::Call(n,_) if n=="@list_directed") || options {
                        if crate::parameters::integer_constant(&args[0]).is_ok() {bail!("external file WRITE currently requires an explicit format without control options");}
                        prefix.push(crate::output::require_stdout(args[0].clone()));
                    } else {
                    let mut file_body=formatted_capture(args[1].clone(),&args[2..],int(1),&symbols,serial)?;
                    if let Some(Stmt::CallSub{name, args:end})=file_body.last_mut() {if name=="@format_end" {*name="@format_end_file".into();end.push(args[0].clone());}}
                    if crate::parameters::integer_constant(&args[0]).is_ok() {output.push(Stmt::Block{body:file_body});}
                    else {
                        let stdout=formatted_capture(args[1].clone(),&args[2..],int(1),&symbols,serial)?;
                        output.push(Stmt::If{cond:bin(BinOp::Eq,args[0].clone(),int(6)),then_body:vec![Stmt::Block{body:stdout}],else_body:Some(vec![Stmt::Block{body:file_body}])});
                    }
                        continue;
                    }

                }
                let mut first_item = 2;
                let mut advance = None;
                let mut dynamic_advance = None;
                while let Some(Expr::Call(option, values)) = args.get(first_item) {
                    let Some(option) = option.strip_prefix("@write_option:") else { break; };
                    if option != "advance" { bail!("unsupported WRITE control `{option}`"); }
                    if advance.is_some() || dynamic_advance.is_some() { bail!("duplicate ADVANCE in WRITE"); }
                    let value=advance_control(&values[0],&symbols.variables,&mut prefix,serial)?;
                    if let Ok(value)=crate::parameters::integer_constant(&value) {advance=Some(value!=0);} else {dynamic_advance=Some(value);}
                    first_item += 1;
                }
                stmt=match &args[1] {
                    Expr::Call(tag, _) if tag=="@list_directed" => {
                        if advance.is_some() || dynamic_advance.is_some() { bail!("ADVANCE requires an explicit format; list-directed WRITE is not allowed"); }
                        Stmt::Print{items:args[first_item..].to_vec()}
                    }
                    _ => {
                        if let Some(advance)=dynamic_advance {
                            let body=formatted_capture(args[1].clone(),&args[first_item..],advance,&symbols,serial)?;
                            output.extend(prefix);output.push(Stmt::Block{body});continue;
                        }
                        let mut values = vec![args[1].clone()]; values.extend_from_slice(&args[first_item..]);
                        Stmt::CallSub{name:if advance == Some(false) {"@formatted_write_noadvance"} else {"@formatted_print"}.into(),args:values}
                    }
                };
            }
        }
        if let Stmt::CallSub { name, args } = &stmt {
            if name.starts_with("@io_") {
                stmt = crate::file_io::lower(name, args, &symbols.variables,serial)?;
            }
        }
        if let Stmt::Print { items } = &stmt {
            if items.iter().any(crate::output::contains_implied_do) {
                stmt = crate::output::lower(items, &symbols.variables, serial)?;
            }
        }
        if let Stmt::CallSub {name,args}=&stmt {
            if matches!(name.as_str(), "@formatted_print" | "@formatted_write_noadvance") && args.iter().any(crate::output::contains_implied_do) {
                bail!("implied-DO is currently supported only in list-directed output");
            }
            if name=="@empty_read" {
                output.push(crate::input::lower_empty_read(args,&symbols.variables,serial)?);
                continue;
            }
            if name=="@internal_read" {
                output.push(crate::input::lower_list_read(args,&symbols.variables,serial)?);
                continue;
            }
            if name=="@formatted_read" {
                let mut controlled=args[..2].to_vec();
                controlled.extend([Expr::Str(String::new()),int(0)]);
                controlled.extend_from_slice(&args[2..]);
                output.push(crate::input::lower_record_read(&controlled,&symbols.variables,serial)?);
                continue;
            }
            if name=="@formatted_read_status" {
                output.push(crate::input::lower_record_read(args,&symbols.variables,serial)?);
                continue;
            }
        }
        if let Stmt::CallSub {name,args}=&mut stmt {
            if name=="@allocate" {bail!("ALLOCATE is currently supported only in specialized local array subroutines");}
            if matches!(name.as_str(), "@formatted_print" | "@formatted_write_noadvance") {
                fn array_value(e: &Expr, symbols: &Symbols) -> bool {
                    match e {
                        Expr::Ident(n) => symbols.get(n).is_some_and(|(_, dims)| !dims.is_empty()),
                        Expr::Section { .. } => true,
                        Expr::Call(n, _) if matches!(n.as_str(),"@matrix_view"|"@packed_view"|"@array_constructor") => true,
                        Expr::Bin(_, a, b) => array_value(a, symbols) || array_value(b, symbols),
                        Expr::Un(_, a) => array_value(a, symbols),
                        // Whether a call is array-valued is decided per item
                        // when the list is written.
                        Expr::Call(n, args) if !n.starts_with('@') && args.iter().any(|a| array_value(a, symbols)) => true,
                        Expr::Call(n, _) if array_call(n) => true,
                        _ => false,
                    }
                }
                // A constant format can consume a fixed whole numeric/LOGICAL
                // array in element order. Runtime-format array lists remain separate.
                if crate::parameters::character_constant(&args[0]).is_ok() {
                    let mut expanded=vec![args[0].clone()];
                    for e in &args[1..] {
                        if let Expr::Ident(n)=e {
                            if let Some((kind,dims))=symbols.get(n) {
                                if dims.len()==1 && array_element_kind(kind) {
                                    for i in 1..=constant(&dims[0])? {expanded.push(Expr::Index(n.clone(),vec![int(i)]));}
                                    continue;
                                }
                            }
                        }
                        expanded.push(e.clone());
                    }
                    *args=expanded;
                }
                if args[1..].iter().any(|e| array_value(e, &symbols)) {
                    let body=formatted_capture(args[0].clone(),&args[1..],int(i64::from(name=="@formatted_print")),&symbols,serial)?;
                    output.extend(prefix);output.push(Stmt::Block{body});continue;
                }
                if let Ok(fmt) = crate::parameters::character_constant(&args[0]) {
                    crate::formatting::parse(&fmt,args.len()-1)?;
                    args[0] = Expr::Str(fmt);
                } else {
                    fn character(e: &Expr, symbols: &Symbols) -> bool {
                        match e {
                            Expr::Str(_) => true,
                            Expr::Ident(n) => matches!(symbols.get(n), Some((TypeSpec::Character(_), dims)) if dims.is_empty()),
                            Expr::Bin(BinOp::Concat, a, b) => character(a, symbols) && character(b, symbols),
                            Expr::Call(n, args) if n == "trim" && args.len() == 1 => character(&args[0], symbols),
                            Expr::Call(n, args) if n == "@dynamic_substring" && args.len() == 4 => character(&args[0], symbols),
                            _ => false,
                        }
                    }
                    if !character(&args[0], &symbols) {
                        bail!("WRITE/PRINT format must be scalar CHARACTER; numeric format labels are not yet supported");
                    }
                }
            }
            if name=="@error_stop" && !matches!(args.as_slice(),[Expr::Str(_)]) {
                let [code]=args.as_slice() else {bail!("ERROR STOP requires one stop code");};
                let code=crate::parameters::integer_constant(code).context("ERROR STOP currently requires a CHARACTER or default INTEGER constant")?;
                let code=i32::try_from(code).context("ERROR STOP integer code must fit default INTEGER")?;
                *args=vec![int(code as i64)];
            }
        }
        if let Stmt::Read {args} = &stmt {
            fn has_elemental(e: &Expr, symbols: &Symbols) -> bool {
                match e {
                    Expr::Call(n,args) | Expr::Index(n,args) => symbols.elementals.contains_key(n) || args.iter().any(|e|has_elemental(e,symbols)),
                    Expr::Bin(_,a,b) => has_elemental(a,symbols) || has_elemental(b,symbols),
                    Expr::Un(_,a) => has_elemental(a,symbols),
                    _ => false,
                }
            }
            if args.iter().any(|e|has_elemental(e,&symbols)) {bail!("ELEMENTAL function result is not a writable READ destination");}
        }
        match &mut stmt {
            Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => for e in items { lower_expr(e, &mut symbols, &mut prefix, serial)?; },
            Stmt::Assign { name, value } => {
                lower_assignment_value(value, &mut symbols, &mut prefix, serial)?;
                narrow_integer(name, value, &symbols);
            }
            Stmt::Return(Some(value)) => lower_expr(value, &mut symbols, &mut prefix, serial)?,
            Stmt::AssignIndex { name, value, indices } => {
                lower_assignment_value(value, &mut symbols, &mut prefix, serial)?;
                narrow_integer(name, value, &symbols);
                for e in indices { lower_expr(e, &mut symbols, &mut prefix, serial)?; }
            }
            Stmt::If { cond, then_body, else_body } => {
                lower_expr(cond, &mut symbols, &mut prefix, serial)?;
                lower_body(then_body, &symbols, serial)?;
                if let Some(body) = else_body { lower_body(body, &symbols, serial)?; }
            }
            Stmt::Do { start, end, body, .. } => {
                lower_expr(start, &mut symbols, &mut prefix, serial)?;
                lower_expr(end, &mut symbols, &mut prefix, serial)?;
                lower_body(body, &symbols, serial)?;
            }
            Stmt::DoWhile { cond, body } => {
                lower_expr(cond, &mut symbols, &mut prefix, serial)?;
                lower_body(body, &symbols, serial)?;
                if !prefix.is_empty() {
                    // Evaluate captures on every iteration, including after CYCLE.
                    let (declarations,mut evaluate): (Vec<_>,Vec<_>) = std::mem::take(&mut prefix).into_iter()
                        .partition(|s| matches!(s,Stmt::VarDecl{..}|Stmt::ArrayDecl{..}));
                    prefix=declarations;
                    evaluate.push(Stmt::If {cond:Expr::Un(UnOp::Not,Box::new(cond.clone())),then_body:vec![Stmt::Exit],else_body:None});
                    evaluate.append(body);
                    *body=evaluate;
                    *cond=Expr::Logical(true);
                }
            }
            Stmt::Block { body } => {
                lower_where(body, &mut symbols, serial)?;
                crate::do_steps::lower(body, &symbols.variables, serial)?;
                lower_body(body, &symbols, serial)?;
            }
            Stmt::Module { body, .. } => lower_body(body, &symbols, serial)?,
            Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                let mut local = symbols.clone();
                for param in params { local.remove(param); }
                lower_body(body, &local, serial)?;
            }
            Stmt::SelectCase { expr, cases, default } => {
                lower_expr(expr, &mut symbols, &mut prefix, serial)?;
                if let Ok((_, None, TypeSpec::Integer(bytes))) = element(expr,&int(1),&symbols) {
                    let max = crate::inquiries::integer_huge(bytes)?;
                    let mut intervals: Vec<(i64,i64)> = Vec::new();
                    for case in cases.iter_mut() {
                        for item in &mut case.items {
                            let bounds = match item {
                                crate::ast::CaseItem::Single(e) => vec![(e,0)],
                                crate::ast::CaseItem::Range(a,b) => vec![(a,-max-1),(b,max)],
                            };
                            for (bound,limit) in bounds {
                                let value = if matches!(bound,Expr::Call(tag,_) if tag == "@case_unbounded") { limit }
                                    else { crate::parameters::integer_constant(bound).context("integer CASE values must be constant INTEGER expressions")? };
                                *bound = int(value);
                            }
                            let (lo,hi) = match item {
                                crate::ast::CaseItem::Single(e) => { let n=constant(e)?; (n,n) },
                                crate::ast::CaseItem::Range(a,b) => (constant(a)?,constant(b)?),
                            };
                            if lo <= hi {
                                if intervals.iter().any(|(a,b)| lo <= *b && *a <= hi) { bail!("overlapping integer CASE selectors"); }
                                intervals.push((lo,hi));
                            }
                        }
                    }
                    if !matches!(expr,Expr::Ident(_)) {
                        *serial += 1;
                        let temp = format!("@case_selector_{serial}");
                        prefix.push(Stmt::VarDecl { kind:TypeSpec::Integer(bytes),names:vec![temp.clone()] });
                        prefix.push(Stmt::Assign { name:temp.clone(),value:expr.clone() });
                        *expr = Expr::Ident(temp);
                    }
                } else if cases.iter().any(|case| case.items.iter().any(|item| matches!(item,crate::ast::CaseItem::Range(..)))) {
                    bail!("CASE ranges currently require a supported scalar INTEGER selector");
                }
                for case in cases { lower_body(&mut case.body, &symbols, serial)?; }
                if let Some(body) = default { lower_body(body, &symbols, serial)?; }
            }
            _ => {}
        }
        if let Stmt::AssignIndex { name, indices, value } = &stmt {
            if let [section] = indices.as_slice() {
                if matches!(section,Expr::Section{..}) || crate::multidim::view_element(section,&int(1)).is_some() {
                    let (_, count, kind) = element(section, &int(1), &symbols)?;
                    let count=count.unwrap();
                    let constructor=match value {Expr::Call(tag,items) if tag == "@array_constructor" => Some(items),_ => None};
                    if constructor.is_some_and(|items|items.iter().any(|e|array_valued(e,&symbols) || matches!(e,Expr::Call(tag,_) if tag == "@array_constructor"))) {
                        bail!("section assignment constructor items must currently be scalar");
                    }
                    let array_rhs=constructor.is_some() || array_valued(value,&symbols);
                    if array_rhs {
                        let rhs_count=if let Some(items)=constructor {Some(items.len() as i64)} else {element(value,&int(1),&symbols)?.1};
                        if rhs_count!=Some(count) {bail!("array section assignment shapes do not conform");}
                    }
                    let mut writes=Vec::new();
                    let scalar = if !array_rhs {
                        *serial+=1;
                        let scalar=format!("@section_assign_value_{serial}");
                        prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![scalar.clone()]});
                        prefix.push(Stmt::Assign{name:scalar.clone(),value:value.clone()});
                        Some(scalar)
                    } else {None};
                    // Capture all RHS elements before any destination write, including
                    // overlapping/reversed sections. Constant destinations retain exact
                    // definite-initialization information.
                    for i in 1..=count {
                        let temp=if let Some(scalar)=&scalar {scalar.clone()} else {
                            *serial+=1;
                            let temp=format!("@section_assign_value_{serial}");
                            let item=if let Some(items)=constructor {items[(i-1) as usize].clone()} else {element(value,&int(i),&symbols)?.0};
                            prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});
                            prefix.push(Stmt::Assign{name:temp.clone(),value:item});
                            temp
                        };
                        let (target,_,_)=element(section,&int(i),&symbols)?;
                        let Expr::Index(_,indices)=target else {unreachable!()};
                        writes.push(Stmt::AssignIndex{name:name.clone(),indices,value:Expr::Ident(temp)});
                    }
                    prefix.extend(writes);
                    stmt=Stmt::Block{body:std::mem::take(&mut prefix)};
                }
            }
        }
        if let Stmt::Print { items } = &mut stmt {
            fn flatten(e:Expr,out:&mut Vec<Expr>) {
                if let Expr::Call(n,args)=e {
                    if n=="@array_constructor" {for e in args {flatten(e,out);}return;}
                    out.push(Expr::Call(n,args));
                } else {out.push(e);}
            }
            let original=std::mem::take(items);
            for e in original {flatten(e,items);}
            // A zero-size array contributes no output items (but PRINT still
            // terminates the record), regardless of its allocated storage.
            items.retain(|e| !matches!(e,Expr::Ident(n) if symbols.get(n).is_some_and(|(_,dims)| dims.len()==1 && constant(&dims[0]).ok()==Some(0))));
            // An elemental expression of a fixed-size rank-1 array, such as
            // X*2 or SQRT(X), prints each of its elements.
            let fixed_expression = |e: &Expr, symbols: &Symbols| -> Option<i64> {
                // Internal section views already print as whole arrays.
                if !matches!(e, Expr::Bin(..) | Expr::Un(..)) && !matches!(e, Expr::Call(n, _) if !n.starts_with('@'))
                    || !array_valued(e, symbols) || runtime_extent(e).is_some() {
                    return None;
                }
                element(e, &int(1), symbols).ok().and_then(|(_, count, _)| count).filter(|n| *n >= 0)
            };
            if items.iter().any(|e| runtime_extent(e).is_some() || fixed_expression(e, &symbols).is_some()) {
                let mut declarations = vec![];
                for item in items.iter_mut() {
                    if let Some(length) = runtime_extent(item).or_else(|| fixed_expression(item, &symbols).map(int)) {
                        *serial += 1;
                        let index = format!("@packed_output_{serial}");
                        let kind = TypeSpec::Integer(Some(8));
                        declarations.push(Stmt::VarDecl {kind:kind.clone(),names:vec![index.clone()]});
                        symbols.insert(index.clone(),(kind,vec![]));
                        let value = element(item,&Expr::Ident(index.clone()),&symbols)?.0;
                        *item = Expr::Call("@output_implied_do".into(),vec![Expr::Ident(index),int(1),length,int(1),value]);
                    }
                }
                let mut printed = crate::output::lower(items,&symbols.variables,serial)?;
                let Stmt::Block {body} = &mut printed else { unreachable!() };
                body.splice(0..0,declarations);
                if prefix.iter().any(|s|matches!(s,Stmt::VarDecl {names,..} if names.iter().any(|n|n.starts_with("@bit_argument_")))) {
                    prefix.push(printed);
                    printed=Stmt::Block {body:std::mem::take(&mut prefix)};
                }
                output.extend(prefix); output.push(printed); continue;
            }
        }
        if let Stmt::Assign { name, value } = &stmt {
            if let Some((kind, dims)) = symbols.get(name).filter(|(_, dims)| !dims.is_empty()) {
                if dims.len() != 1 || !array_element_kind(kind) {
                    bail!("whole-array assignment currently requires a one-dimensional numeric or LOGICAL array");
                }
                if let Expr::Call(tag,values) = value { if tag == "@array_constructor" {
                    if values.iter().any(|e| array_valued(e, &symbols)) {
                        bail!("array constructor items must currently be scalar");
                    }
                    if constant(&dims[0])? != values.len() as i64 {bail!("array constructor shape does not conform");}
                    let mut writes=vec![];
                    for (i,v) in values.iter().enumerate() {
                        *serial+=1;let temp=format!("@constructor_{serial}");
                        prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});prefix.push(Stmt::Assign{name:temp.clone(),value:v.clone()});
                        writes.push(Stmt::AssignIndex{name:name.clone(),indices:vec![int(i as i64+1)],value:Expr::Ident(temp)});
                    }
                    prefix.extend(writes);output.push(Stmt::Block{body:prefix});continue;
                } }
                if array_valued(value, &symbols) {
                    *serial+=1;let index=format!("@array_index_{serial}");let temp=format!("@array_temp_{serial}");let index_expr=Expr::Ident(index.clone());
                    let (scalar,count,_)=element(value,&index_expr,&symbols)?;let extent=constant(&dims[0])?;
                    if let Some(length) = runtime_extent(value) { prefix.push(shape_guard(length,int(extent))); }
                    else if count!=Some(extent) {bail!("array assignment shapes do not conform");}
                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]});
                    prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(extent)]});
                    prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:int(extent),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![index_expr.clone()],value:scalar}]});
                    prefix.push(Stmt::Do{var:index,start:int(1),end:int(extent),body:vec![Stmt::AssignIndex{name:name.clone(),indices:vec![index_expr.clone()],value:Expr::Index(temp,vec![index_expr])}]});
                    output.push(Stmt::Block{body:prefix});continue;
                }
                let extent = constant(&dims[0])?;
                if extent < 0 { bail!("negative array extent is not supported"); }
                *serial += 1;
                let index = format!("@array_assign_index_{serial}");
                let scalar = format!("@array_assign_value_{serial}");
                prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
                prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![scalar.clone()] });
                // Capture before writing any elements, including when the RHS reads this array.
                prefix.push(Stmt::Assign { name: scalar.clone(), value: value.clone() });
                stmt = Stmt::Do {
                    var: index.clone(), start: int(1), end: int(extent),
                    body: vec![Stmt::AssignIndex {
                        name: name.clone(), indices: vec![Expr::Ident(index)], value: Expr::Ident(scalar),
                    }],
                };
                // Give generated temporaries a declaration scope even inside a loop or IF.
                prefix.push(stmt);
                stmt = Stmt::Block { body: std::mem::take(&mut prefix) };
            }
        }
        // Captured bit arguments need a declaration scope inside loops and
        // branches, as well as when captures precede a DO WHILE condition.
        if prefix.iter().any(|s|matches!(s,Stmt::VarDecl {names,..} if names.iter().any(|n|n.starts_with("@bit_argument_")))) {
            prefix.push(stmt);
            stmt=Stmt::Block {body:std::mem::take(&mut prefix)};
        }
        output.extend(prefix);
        output.push(stmt);
    }
    Ok(())
    })().map_err(|error|crate::source_diagnostics::at(error,source_offset))?;
    *body = output;
    Ok(())
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut symbols=Symbols::new();
    fn procedures(body:&[Stmt],names:&mut HashSet<String>) {
        for stmt in body {
            match stmt {
                Stmt::Function {name,body,..}|Stmt::Subroutine {name,body,..} => {names.insert(name.clone());procedures(body,names);},
                Stmt::Module {body,..}|Stmt::Block {body} => procedures(body,names),
                _ => {},
            }
        }
    }
    procedures(&program.body,&mut symbols.procedures);
    symbols.elementals=crate::elemental::functions(&program);
    fn scalar_functions(body:&[Stmt],out:&mut HashMap<String,TypeSpec>) {
        for stmt in body {match stmt {
            Stmt::Function{name,return_type,body,elemental:false,..}=>{
                let result=match body.last() {Some(Stmt::Return(Some(Expr::Ident(n))))=>n.as_str(),_=>name.as_str()};
                if !body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==result)) {
                    if let Some(kind)=crate::elemental::result_kind(name,return_type,body) {out.insert(name.clone(),kind);}
                }
                scalar_functions(body,out);
            }
            Stmt::Module{body,..}|Stmt::Subroutine{body,..}|Stmt::Function{body,..}|Stmt::Block{body}=>scalar_functions(body,out),_=>{}
        }}
    }
    scalar_functions(&program.body,&mut symbols.scalar_functions);
    collect_character_functions(&program.body,&mut symbols.character_functions)?;
    // Call arguments may have been materialized in an earlier pass. Continue
    // its numbering so constructor and PACK temporaries cannot shadow them.
    fn maximum(body: &[Stmt]) -> usize {
        body.iter().map(|s| match s {
            Stmt::ArrayDecl {name,..} => suffix(name),
            Stmt::VarDecl {names,..} => names.iter().map(|n|suffix(n)).max().unwrap_or(0),
            Stmt::Module {body,..} | Stmt::Function {body,..} | Stmt::Subroutine {body,..} | Stmt::Block {body} | Stmt::Do {body,..} | Stmt::DoWhile {body,..} => maximum(body),
            Stmt::If {then_body,else_body,..} => maximum(then_body).max(else_body.as_ref().map(|b|maximum(b)).unwrap_or(0)),
            Stmt::SelectCase {cases,default,..} => cases.iter().map(|c|maximum(&c.body)).max().unwrap_or(0).max(default.as_ref().map(|b|maximum(b)).unwrap_or(0)),
            _=>0,
        }).max().unwrap_or(0)
    }
    fn suffix(n: &str)->usize {if n.starts_with('@') {n.rsplit('_').next().and_then(|s|s.parse().ok()).unwrap_or(0)} else {0}}
    let mut serial=maximum(&program.body);
    lower_body(&mut program.body, &symbols, &mut serial)?;
    crate::substrings::hoist(&mut program.body);
    Ok(program)
}

// CHARACTER inputs use local value copies. Reject writable dummies until a
// caller-storage ABI is available, including writes through subroutine calls.
fn collect_character_functions(body: &[Stmt], signatures: &mut HashMap<String,Vec<Option<usize>>>) -> Result<()> {
    fn mentions(e: &Expr, names: &[String]) -> bool { match e {
        Expr::Ident(n) => names.contains(n),
        Expr::Bin(_,a,b) => mentions(a,names)||mentions(b,names),
        Expr::Un(_,a) => mentions(a,names),
        Expr::Call(n,args)|Expr::Index(n,args) => names.contains(n)||args.iter().any(|e|mentions(e,names)),
        Expr::Section{name,..} => names.contains(name),
        _ => false,
    }}
    fn readonly(body: &[Stmt], names: &[String]) -> Result<()> {
        for s in body { match s {
            Stmt::Assign{name,..}|Stmt::AssignIndex{name,..} if names.contains(name) => bail!("CHARACTER function dummies currently must be read-only"),
            Stmt::DummyIntent{name,intent} if names.contains(name) && matches!(intent.as_str(),"intent(out)"|"intent(inout)") => bail!("CHARACTER function dummies currently must be read-only"),
            Stmt::Read{args}|Stmt::CallSub{args,..} if args.iter().any(|e|mentions(e,names)) => bail!("writable or subroutine-forwarded CHARACTER function dummies are not yet supported"),
            Stmt::If{then_body,else_body,..} => {readonly(then_body,names)?;if let Some(b)=else_body {readonly(b,names)?;}}
            Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body} => readonly(body,names)?,
            Stmt::SelectCase{cases,default,..} => {for c in cases {readonly(&c.body,names)?;}if let Some(b)=default {readonly(b,names)?;}}
            _ => {}
        }}
        Ok(())
    }
    for s in body { match s {
        Stmt::Function{name,params,body,..} => {
            let lengths:Vec<_>=params.iter().map(|p|body.iter().find_map(|s|match s {
                Stmt::VarDecl{kind:TypeSpec::Character(length),names} if names.contains(p) => Some(length.unwrap_or(1)),
                _=>None,
            })).collect();
            if lengths.iter().any(Option::is_some) {
                let names=params.iter().zip(&lengths).filter(|(_,n)|n.is_some()).map(|(p,_)|p.clone()).collect::<Vec<_>>();
                readonly(body,&names)?;
                signatures.insert(name.clone(),lengths);
            }
        }
        Stmt::Module{body,..} => collect_character_functions(body,signatures)?,
        _=>{}
    }}
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_checks_initialized_output_items() {
        for statement in ["write(*,*) n", "write(6,'(i3)') n"] {
            let source=format!("integer :: n\n{statement}\nend");
            let tokens=crate::lexer::lex(&source);
            let p=crate::parser::parse_with_src(&source,&tokens,"write.f90").unwrap();
            let p=super::resolve(crate::parameters::resolve(p).unwrap()).unwrap();
            let error=crate::initialized::instrument(p).unwrap_err().to_string();
            assert!(error.contains("uninitialized variable `n`"),"{error}");
        }
    }

    #[test]
    fn validates_write_units_and_formats() {
        for (declarations,statement,valid) in [
            ("", "write(*,*) 42", true),
            ("", "write(6,*)", true),
            ("integer,parameter :: stdout=6", "write(stdout,*) 42", true),
            ("", "write(*,'(i3)') 42", true),
            ("", "write(*,*)\nx=42", true),
            ("", "write(7,*) 42", true),
            ("integer :: unit", "unit=6\nwrite(unit,*) 42", true),
            // Internal WRITE is lowered by an earlier pass, not run here.
            ("character(len=8) :: text", "write(text,*) 42", false),
            ("", "write(*,'*') 42", false),
            ("", "write(*,'(q3)') 42", false),
        ] {
            let source=format!("integer :: x\n{declarations}\n{statement}\nend");
            let tokens=crate::lexer::lex(&source);
            let p=crate::parser::parse_with_src(&source,&tokens,"write.f90").unwrap();
            let result=crate::parameters::resolve(p).and_then(super::resolve);
            assert_eq!(result.is_ok(),valid,"{source}: {result:?}");
        }
    }

    #[test]
    fn validates_record_read_destinations_and_formats() {
        for (declaration, statement, valid) in [
            ("character(len=8) :: text", "read(*,'(a)') text", true),
            ("character :: text", "read(5,'( A )') text", true),
            ("character(len=0) :: text", "read(*,'(a)') text", true),
            ("integer :: text", "read(*,'(a)') text", false),
            ("character(len=8) :: text(2)", "read(*,'(a)') text", false),
            ("character(len=8) :: text", "read(*,'(a)') trim(text)", false),
            ("character(len=8) :: text", "read(*,'(a)') 'literal'", false),
            ("character(len=8) :: text", "read(*,'(a)') text,text", false),
            ("character(len=8) :: text", "read(*,'(a4)') text", false),
            ("character(len=8) :: text", "read(10,'(a)') text", true),
            ("character(len=8) :: text\ninteger :: iu", "read(iu,'(a)') text", false),
            ("character(len=*),parameter :: text='constant'", "read(*,'(a)') text", false),
        ] {
            let source=format!("implicit none\n{declaration}\n{statement}\nprint *, text\nend");
            let tokens=crate::lexer::lex(&source);
            let p=crate::parser::parse_with_src(&source,&tokens,"read.f90").unwrap();
            let result=crate::parameters::resolve(p).and_then(super::resolve)
                .and_then(crate::initialized::instrument);
            assert_eq!(result.is_ok(),valid,"{source}: {result:?}");
        }
    }

    #[test]
    fn section_assignment_tracks_initialized_elements() {
        for (assignment, output, valid) in [
            ("v(1:2)=10", "v", false),
            ("v(1:2)=10", "v(3)", false),
            ("v(1:2)=10", "v(1:2)", true),
            ("v(:)=10", "v", true),
            ("v(3:1:-1)=10", "v", true),
            ("v(::2)=10", "v(2)", false),
            ("v(2:1)=10", "v(1)", false),
        ] {
            let source = format!("implicit none\ninteger :: v(3)\n{assignment}\nprint *, {output}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            let p = crate::sections::resolve(super::resolve(crate::parameters::resolve(p).unwrap()).unwrap()).unwrap();
            assert_eq!(crate::initialized::instrument(p).is_ok(), valid, "{source}");
        }
        for assignment in ["v(::0)=10", "v(0:2)=10", "v(2:4)=10", "v(:)=v(1:2)", "v(1:1)=[v]"] {
            let source = format!("integer :: v(3)\n{assignment}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            assert!(super::resolve(crate::parameters::resolve(p).unwrap()).is_err(), "{source}");
        }
    }

    #[test]
    fn lowers_supported_array_assignments() {
        for assignment in ["a = b", "a = b + 1", "a = b(:)", "a = sqrt(b)"] {
            let source = format!("implicit none\nreal :: a(3), b(3)\n{assignment}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            super::resolve(crate::parameters::resolve(p).unwrap()).unwrap();
        }
    }

    #[test]
    fn rejects_invalid_reductions() {
        for (decls, expression, message) in [
            ("real :: x", "sum(x)", "must be an array"),
            ("logical :: x(3)", "any(x,.true.)", "normalized ARRAY"),
            ("real :: x(3), y(2)", "sum(x+y)", "do not conform"),
            ("real :: x(3)", "product(x(::0))", "stride must not be zero"),
            ("real :: x(3)", "minval(x(0:2))", "outside 1:3"),
            ("logical :: x(3)", "maxval(x)", "incorrect numeric/logical type"),
        ] {
            let source = format!("program main\nimplicit none\n{decls}\nprint *, {expression}\nend program main");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "reductions.f90").unwrap();
            let error = super::resolve(crate::parameters::resolve(p).unwrap()).unwrap_err();
            assert!(format!("{error:#}").contains(message), "{error:#}");
        }
    }
}

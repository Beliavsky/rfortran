// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Evaluate bounded CHARACTER specification expressions at procedure entry.
use crate::ast::{BinOp,Expr,Stmt,TypeSpec,UnOp};
use anyhow::{bail,Context,Result};
use std::collections::HashMap;
fn int(n:i64)->Expr {Expr::IntLit(n.to_string())}
fn bin(op:BinOp,a:Expr,b:Expr)->Expr {Expr::Bin(op,Box::new(a),Box::new(b))}
type Types=HashMap<String,TypeSpec>;
type Sizes=HashMap<String,Vec<i64>>;

fn width(e:&Expr,types:&Types)->Result<i64> {
    let kind=crate::inquiries::model_type(e,&|n|types.get(n).map(|k|(k.clone(),0)))?;
    match kind {Some(TypeSpec::Character(n))=>Ok(n.unwrap_or(1) as i64),_=>bail!("CHARACTER specification requires a known argument element length")}
}
fn range(e:&Expr,types:&Types,sizes:&Sizes)->Result<(Expr,i64,i64)> {
    if let Ok(n)=crate::parameters::integer_constant(e) {return Ok((int(n),n,n));}
    match e {
        Expr::Call(name,args) if name=="@character_spec_len" && args.len()==2=>Ok((args[1].clone(),0,width(&args[0],types)?)),
        Expr::Call(name,args) if name=="sum" && args.len()==1=>{
            if let Expr::Call(n,a)=&args[0] {if n=="len_trim" && a.len()==1 {
                if let Expr::Ident(array)=&a[0] {
                    let dims=sizes.get(array).context("SUM(LEN_TRIM(...)) requires a bounded array argument")?;
                    let count=dims.iter().try_fold(1i64,|n,d|n.checked_mul((*d).max(0)).context("CHARACTER specification array size overflow"))?;
                    if count>1024 {bail!("CHARACTER specification expansion supports at most 1024 elements");}
                    let width=width(&a[0],types)?;
                    let value=Expr::Call("@character_spec_sum".into(),vec![Expr::Ident(array.clone()),Expr::Call("size".into(),vec![Expr::Ident(array.clone())])]);
                    return Ok((value,0,count.checked_mul(width).context("CHARACTER specification length overflow")?));
                }
            }}
            bail!("unsupported array reduction in a bounded CHARACTER specification")
        }
        Expr::Call(name,args) if name=="size" && args.len()==1=>{
            let [Expr::Ident(array)]=args.as_slice() else {bail!("CHARACTER specification SIZE requires an array argument");};
            let dims=sizes.get(array).context("CHARACTER specification SIZE needs bounded array storage")?;
            let count=dims.iter().try_fold(1i64,|n,d|n.checked_mul((*d).max(0)).context("CHARACTER specification size overflow"))?;
            Ok((e.clone(),0,count))
        }
        Expr::Call(name,args) if matches!(name.as_str(),"len"|"len_trim") && args.len()==1=>{
            if let Ok(text)=crate::parameters::character_constant(&args[0]) {
                let n=if name=="len" {text.len()} else {text.trim_end_matches(' ').len()} as i64;
                return Ok((int(n),n,n));
            }
            let width=width(&args[0],types)?;
            if name=="len" {Ok((int(width),width,width))} else {Ok((e.clone(),0,width))}
        }
        Expr::Bin(op,a,b) if matches!(op,BinOp::Add|BinOp::Sub|BinOp::Mul|BinOp::Div)=>{
            let (a,al,ah)=range(a,types,sizes)?;let (b,bl,bh)=range(b,types,sizes)?;
            let (al,ah,bl,bh)=(al as i128,ah as i128,bl as i128,bh as i128);
            let candidates=match op {
                BinOp::Add=>vec![al+bl,ah+bh],BinOp::Sub=>vec![al-bh,ah-bl],
                BinOp::Mul=>vec![al*bl,al*bh,ah*bl,ah*bh],
                _=>{if bl<=0 && bh>=0 {bail!("CHARACTER specification divisor can be zero");}vec![al/bl,al/bh,ah/bl,ah/bh]}
            };
            let low=i64::try_from(*candidates.iter().min().unwrap()).context("CHARACTER specification integer overflow")?;
            let high=i64::try_from(*candidates.iter().max().unwrap()).context("CHARACTER specification integer overflow")?;
            Ok((bin(op.clone(),a,b),low,high))
        }
        Expr::Un(UnOp::Neg,a)=>{let (a,l,h)=range(a,types,sizes)?;Ok((Expr::Un(UnOp::Neg,Box::new(a)),h.checked_neg().context("CHARACTER length overflow")?,l.checked_neg().context("CHARACTER length overflow")?))}
        Expr::Call(name,args) if matches!(name.as_str(),"min"|"max") && args.len()>=2=>{
            let values=args.iter().map(|e|range(e,types,sizes)).collect::<Result<Vec<_>>>()?;
            let low=if name=="min" {values.iter().map(|v|v.1).min()} else {values.iter().map(|v|v.1).max()}.unwrap();
            let high=if name=="min" {values.iter().map(|v|v.2).min()} else {values.iter().map(|v|v.2).max()}.unwrap();
            Ok((Expr::Call(name.clone(),values.into_iter().map(|v|v.0).collect()),low,high))
        }
        _=>bail!("CHARACTER specification does not have a supported finite storage bound"),
    }
}
// Preserve a scalar actual's logical length while using its physical width as
// the conservative storage bound for a nested CHARACTER call.
pub(crate) fn dynamic_lengths(e:&mut Expr,lengths:&HashMap<String,Expr>) {
    if let Expr::Call(name,args)=e {
        if name=="len" && args.len()==1 {
            if let Expr::Ident(n)=&args[0] {
                if let Some(length)=lengths.get(n) {*e=Expr::Call("@character_spec_len".into(),vec![args[0].clone(),length.clone()]);return;}
            }
        }
    }
    match e {
        Expr::Call(_,args)|Expr::Index(_,args)=>for e in args {dynamic_lengths(e,lengths);},
        Expr::Bin(_,a,b)=>{dynamic_lengths(a,lengths);dynamic_lengths(b,lengths);},
        Expr::Un(_,a)=>dynamic_lengths(a,lengths),_=>{},
    }
}
pub(crate) fn bounded(e:&Expr,types:&Types,sizes:&Sizes)->Result<(Expr,usize)> {
    let (value,_,upper)=range(e,types,sizes)?;
    let capacity=usize::try_from(upper.max(0)).context("CHARACTER length is too large")?;
    if capacity>=i32::MAX as usize {bail!("CHARACTER specification storage is too large");}
    Ok((Expr::Call("max".into(),vec![int(0),value]),capacity))
}

// SUM(LEN_TRIM(array)) reads only the active elements, once at entry.
pub(crate) fn capture(e:&mut Expr,out:&mut Vec<Stmt>,serial:&mut usize)->Result<()> {
    match e {
        Expr::Call(n,args) if n=="@character_spec_sum"=>{
            let Expr::Ident(array)=&args[0] else {bail!("invalid CHARACTER specification array");};
            *serial+=1;let total=format!("@character_spec_sum_{serial}");let index=format!("@character_spec_index_{serial}");
            out.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![total.clone(),index.clone()]});
            out.push(Stmt::Assign{name:total.clone(),value:int(0)});
            out.push(Stmt::Do{var:index.clone(),start:int(1),end:args[1].clone(),body:vec![Stmt::Assign{name:total.clone(),value:bin(BinOp::Add,Expr::Ident(total.clone()),Expr::Call("len_trim".into(),vec![Expr::Index(array.clone(),vec![Expr::Ident(index)])]))}]});
            *e=Expr::Ident(total);
        }
        Expr::Call(_,args)|Expr::Index(_,args)=>for e in args {capture(e,out,serial)?;},
        Expr::Bin(_,a,b)=>{capture(a,out,serial)?;capture(b,out,serial)?;},
        Expr::Un(_,a)=>capture(a,out,serial)?,_=>{},
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_include_negative_intermediates_and_empty_sums() {
        let types=HashMap::from([("text".into(),TypeSpec::Character(Some(10)))]);
        let trim=Expr::Call("len_trim".into(),vec![Expr::Ident("text".into())]);
        let difference=bin(BinOp::Sub,trim.clone(),int(5));
        assert_eq!(bounded(&bin(BinOp::Mul,difference,int(-3)),&types,&Sizes::new()).unwrap().1,15);
        assert_eq!(bounded(&Expr::Un(UnOp::Neg,Box::new(trim.clone())),&types,&Sizes::new()).unwrap().1,0);
        let divisor=Expr::Call("max".into(),vec![int(1),trim]);
        assert_eq!(bounded(&bin(BinOp::Div,int(10),divisor),&types,&Sizes::new()).unwrap().1,10);
        let sum=Expr::Call("sum".into(),vec![Expr::Call("len_trim".into(),vec![Expr::Ident("text".into())])]);
        assert_eq!(bounded(&sum,&types,&HashMap::from([("text".into(),vec![0])])).unwrap().1,0);
    }
}

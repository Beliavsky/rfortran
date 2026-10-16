// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Argument ordering and fixed-shape array inquiry results.
use crate::ast::{BinOp, Expr, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};

pub fn numeric_model(name: &str) -> bool {
    matches!(name, "epsilon" | "digits" | "radix" | "precision" | "range" | "minexponent" | "maxexponent")
}

/// These inquiries describe a type, and do not evaluate the argument's value.
pub fn model_value(name: &str, kind: &TypeSpec) -> Result<Expr> {
    let int = |n: i64| Expr::IntLit(n.to_string());
    if let TypeSpec::Integer(bytes) = kind {
        if !matches!(name, "digits" | "radix" | "range") {
            bail!("{} requires a REAL argument", name.to_ascii_uppercase());
        }
        let bytes = bytes.unwrap_or(4);
        if !matches!(bytes,1|2|4|8) { bail!("{} currently supports INTEGER kinds 1, 2, 4 and 8, not {bytes}",name.to_ascii_uppercase()); }
        return Ok(int(match name {
            "digits" => i64::from(bytes) * 8 - 1,
            "radix" => 2,
            _ => match bytes { 1 => 2, 2 => 4, 4 => 9, 8 => 18, _ => unreachable!() },
        }));
    }
    if let TypeSpec::Complex(k) = kind {
        if !matches!(name,"precision"|"range") { bail!("{} does not accept COMPLEX",name.to_ascii_uppercase()); }
        return model_value(name,&if k=="8" {TypeSpec::DoublePrecision}else{TypeSpec::Real});
    }
    let double = match kind {
        TypeSpec::Real => false,
        TypeSpec::DoublePrecision => true,
        _ => bail!("{} requires {} argument", name.to_ascii_uppercase(),
            if matches!(name,"digits" | "radix" | "range") { "an INTEGER or REAL" } else { "a REAL" }),
    };
    Ok(match name {
        "epsilon" => {
            let value = Expr::RealLit(if double { f64::EPSILON } else { f32::EPSILON as f64 }.to_string());
            if double { Expr::Call("dble".into(), vec![value]) } else { value }
        }
        "digits" => int(if double { 53 } else { 24 }),
        "radix" => int(2),
        "precision" => int(if double { 15 } else { 6 }),
        "range" => int(if double { 307 } else { 37 }),
        "minexponent" => int(if double { -1021 } else { -125 }),
        "maxexponent" => int(if double { 1024 } else { 128 }),
        _ => bail!("unknown numeric model inquiry `{name}`"),
    })
}

/// Infer supported inquiry argument types without generating reads or calls.
/// Array shape and subscript values do not affect these scalar results.
pub fn model_type(e: &Expr, lookup: &impl Fn(&str) -> Option<(TypeSpec,usize)>) -> Result<Option<TypeSpec>> {
    let subscript = |e: &Expr| -> Result<()> {
        let kind=model_type(e,lookup)?.context("undefined or unsupported inquiry subscript type")?;
        if !matches!(kind,TypeSpec::Integer(_)) { bail!("inquiry array subscript must be INTEGER"); }
        Ok(())
    };
    let kind = match e {
        Expr::Call(name,_) if name=="@typed_constructor" => crate::empty_constructors::kind(e)?,
        Expr::Call(tag,_) if crate::bit_intrinsics::tag_kind(tag).is_some() => crate::bit_intrinsics::tag_kind(tag),
        Expr::Call(name,args) if crate::bit_intrinsics::keys(name).is_some() && lookup(name).is_none() => {
            let kinds=args.iter().map(|arg|model_type(arg,lookup)).collect::<Result<Vec<_>>>()?;
            if let Some(kinds)=kinds.into_iter().collect::<Option<Vec<_>>>() {
                Some(crate::bit_intrinsics::result_kind(name,&kinds)?)
            } else {None}
        },
        Expr::Ident(n) => lookup(n).map(|(kind,_)|kind),
        Expr::Section {name,lower,upper,stride} => {
            for bound in [lower,upper,stride].into_iter().flatten() {subscript(bound)?;}
            let Some((kind,rank))=lookup(name) else {return Ok(None)};
            if rank!=1 {bail!("numeric inquiry section currently requires a rank-one array");}
            Some(kind)
        },
        Expr::Index(n,a) | Expr::Call(n,a) if lookup(n).is_some() => {
            let (kind,rank)=lookup(n).unwrap();
            if rank==0 || rank!=a.len() {bail!("numeric inquiry array argument `{n}` has the wrong number of subscripts");}
            for arg in a {subscript(arg)?;}
            Some(kind)
        },
        Expr::Call(n,a) if n=="cmplx" || n=="@complex_literal" => {
            let k=if n=="cmplx" {a.get(2).filter(|e|!crate::named_arguments::omitted(e)).map(crate::parameters::integer_constant).transpose()?.unwrap_or(4)} else {
                let mut k=4;for e in a {if model_type(e,lookup)?==Some(TypeSpec::DoublePrecision){k=8;}}k
            };
            Some(TypeSpec::Complex(k.to_string()))
        },
        Expr::IntLit(_) => Some(TypeSpec::Integer(None)),
        Expr::RealLit(_) => Some(TypeSpec::Real),
        Expr::Logical(_) => Some(TypeSpec::Logical),
        Expr::Str(_) => Some(TypeSpec::Character(None)),
        Expr::Un(UnOp::Not,_) => Some(TypeSpec::Logical),
        Expr::Un(_,a) => model_type(a,lookup)?,
        Expr::Bin(op,a,b) => {
            if !matches!(op,BinOp::Add|BinOp::Sub|BinOp::Mul|BinOp::Div|BinOp::Pow) { Some(TypeSpec::Logical) }
            else {
                match (model_type(a,lookup)?,model_type(b,lookup)?) {
                    (Some(a),Some(b)) => {
                        if !matches!(a,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Complex(_))
                            || !matches!(b,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Complex(_)) {
                            bail!("numeric inquiry argument has nonnumeric arithmetic operands");
                        }
                        Some(if matches!(op,BinOp::Pow) && matches!(b,TypeSpec::Integer(_)) { a }
                            else if matches!(&a,TypeSpec::Complex(_)) || matches!(&b,TypeSpec::Complex(_)) {
                                let component=|t:&TypeSpec|match t {TypeSpec::Complex(k)=>k.parse::<u8>().unwrap_or(4),TypeSpec::DoublePrecision=>8,_=>4};
                                TypeSpec::Complex(component(&a).max(component(&b)).to_string())
                            }
                            else if a==TypeSpec::DoublePrecision || b==TypeSpec::DoublePrecision {TypeSpec::DoublePrecision}
                            else if a==TypeSpec::Real || b==TypeSpec::Real {TypeSpec::Real}
                            else if let (TypeSpec::Integer(a),TypeSpec::Integer(b))=(a,b) {TypeSpec::Integer(Some(a.unwrap_or(4).max(b.unwrap_or(4))))}
                            else { unreachable!() })
                    },
                    _ => None,
                }
            }
        },
        Expr::Call(n,a) if matches!(n.as_str(),"index"|"scan"|"verify") && (2..=4).contains(&a.len()) => {
            for arg in &a[..2] {
                if model_type(arg,lookup)?.is_some_and(|k|!matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed)) {bail!("{n} requires CHARACTER arguments");}
            }
            let kind=a.get(3).filter(|e|!crate::named_arguments::omitted(e)).map(crate::parameters::integer_constant).transpose()?.unwrap_or(4);
            if !matches!(kind,1|2|4|8) {bail!("{n} KIND must be a supported INTEGER kind (1, 2, 4 or 8)");}
            Some(TypeSpec::Integer(Some(kind as u8)))
        },
        Expr::Call(n,a) if crate::intrinsics::integer_rounding(n) && lookup(n).is_none() => {
            let kind=crate::intrinsics::rounding_kind(n,a)?;
            if model_type(&a[0],lookup)?.is_some_and(|k|!matches!(k,TypeSpec::Real|TypeSpec::DoublePrecision)) {
                bail!("{} A must be REAL or DOUBLE PRECISION",n.to_ascii_uppercase());
            }
            Some(kind)
        },
        Expr::Call(n,a) if n=="@integer_literal" && a.len()==2 => Some(TypeSpec::Integer(Some(crate::parameters::integer_constant(&a[1])? as u8))),
        Expr::Call(n,a) if matches!(n.as_str(),"dble"|"real"|"int"|"abs"|"aimag"|"conjg"|"@unary_plus") && a.len()==1 => {
            let input=model_type(&a[0],lookup)?;
            if input.as_ref().is_some_and(|k|!matches!(k,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Complex(_))) {
                bail!("{} requires a numeric argument",n.to_ascii_uppercase());
            }
            input.map(|k| match n.as_str() {"dble"=>TypeSpec::DoublePrecision,"real"|"aimag"|"abs" if matches!(&k,TypeSpec::Complex(_)) => if k==TypeSpec::Complex("8".into()){TypeSpec::DoublePrecision}else{TypeSpec::Real},"real"=>TypeSpec::Real,"int"=>TypeSpec::Integer(None),_=>k})
        },
        _ => None,
    };
    Ok(kind)
}

/// Largest representable integer for the implemented scalar execution kinds.
pub fn integer_huge(kind: Option<u8>) -> Result<i64> {
    Ok(match kind.unwrap_or(4) {
        1 => i8::MAX as i64,
        2 => i16::MAX as i64,
        4 => i32::MAX as i64,
        8 => i64::MAX,
        n => bail!("HUGE currently supports INTEGER kinds 1, 2, 4 and 8, not {n}"),
    })
}

pub fn arguments(name: &str, args: &[Expr]) -> Result<(Expr, Option<Expr>, Option<Expr>)> {
    let shape = name == "shape";
    let mut slots: [Option<Expr>; 3] = [None, None, None];
    let mut keyword_seen = false;
    for (position, arg) in args.iter().enumerate() {
        let (slot, value) = if let Expr::Call(tag, values) = arg {
            if let Some(key) = tag.strip_prefix("@keyword:") {
                keyword_seen = true;
                let slot = match key {
                    "array" if !shape => 0,
                    "source" if shape => 0,
                    "dim" if !shape => 1,
                    "kind" => if shape { 1 } else { 2 },
                    _ => bail!("{name}: unknown keyword `{key}`"),
                };
                (slot, &values[0])
            } else {
                if keyword_seen {
                    bail!("{name}: positional argument after keyword argument");
                }
                (position, arg)
            }
        } else {
            if keyword_seen {
                bail!("{name}: positional argument after keyword argument");
            }
            (position, arg)
        };
        if shape && slot >= 2 {
            bail!("SHAPE expects SOURCE and optional KIND");
        }
        if slot >= 3 {
            bail!("{name} expects ARRAY, optional DIM, and optional KIND");
        }
        if slots[slot].replace(value.clone()).is_some() {
            bail!("{name}: duplicate argument");
        }
    }
    Ok((
        slots[0]
            .take()
            .with_context(|| format!("{name} requires {}", if shape { "SOURCE" } else { "ARRAY" }))?,
        if shape { None } else { slots[1].take() },
        if shape { slots[1].take() } else { slots[2].take() },
    ))
}

pub fn bounds(name: &str, dims: &[i64], dim: Option<&Expr>) -> Result<Expr> {
    if dims.is_empty() {
        bail!("{name} requires an array");
    }
    let bound =
        |extent: i64| Expr::IntLit(if name == "lbound" { 1 } else { extent.max(0) }.to_string());
    if let Some(dim) = dim {
        let axis = crate::parameters::integer_constant(dim)
            .with_context(|| format!("{name} DIM must currently be a constant INTEGER"))?;
        if axis < 1 || axis as usize > dims.len() {
            bail!("{name} DIM must be between 1 and {}", dims.len());
        }
        Ok(bound(dims[axis as usize - 1]))
    } else {
        Ok(Expr::Call(
            "@bounds_vector".into(),
            dims.iter().map(|n| bound(*n)).collect(),
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn int(n: i64) -> Expr {
        Expr::IntLit(n.to_string())
    }
    #[test]
    fn scalar_and_vector_bounds_include_empty_extents() {
        assert_eq!(
            bounds("lbound", &[2, 0], None).unwrap(),
            Expr::Call("@bounds_vector".into(), vec![int(1), int(1)])
        );
        assert_eq!(
            bounds("ubound", &[2, 0], None).unwrap(),
            Expr::Call("@bounds_vector".into(), vec![int(2), int(0)])
        );
        assert_eq!(bounds("ubound", &[2, 0], Some(&int(2))).unwrap(), int(0));
        assert!(bounds("lbound", &[2, 0], Some(&int(3))).is_err());
    }
    #[test]
    fn keywords_reorder_without_accepting_duplicates() {
        let array = Expr::Ident("a".into());
        let kw = |name: &str, value| Expr::Call(format!("@keyword:{name}"), vec![value]);
        assert_eq!(
            arguments(
                "ubound",
                &[
                    kw("kind", int(4)),
                    kw("dim", int(2)),
                    kw("array", array.clone())
                ]
            )
            .unwrap(),
            (array.clone(), Some(int(2)), Some(int(4)))
        );
        assert!(arguments("lbound", &[array.clone(), int(1), kw("dim", int(2))]).is_err());
        assert!(arguments("lbound", &[kw("array", array), int(1)]).is_err());
    }
}

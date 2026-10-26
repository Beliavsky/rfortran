// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Descriptor-backed vector selections. Fixed selections share the same
//! Cartesian indexing rule in vector_subscripts; these loops retain active sizes.
use crate::allocatables::{self, Shapes};
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

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
fn constant(e: &Expr) -> Option<i64> {
    crate::parameters::integer_constant(e).ok()
}
fn product(dims: &[Expr]) -> Expr {
    dims.iter()
        .cloned()
        .fold(int(1), |a, b| bin(BinOp::Mul, a, b))
}
fn error(cond: Expr, message: String) -> Stmt {
    Stmt::If {
        cond,
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message)],
        }],
        else_body: None,
    }
}
fn declarations(body: &[Stmt], symbols: &mut Shapes) {
    for s in body {
        match s {
            Stmt::ArrayDecl { kind, name, dims } => {
                for key in [
                    "@heap",
                    "@pointer",
                    "@borrowed_pointer",
                    "@axes",
                    "@lowers",
                    "@extent",
                    "@allocated",
                    "@axis_limits",
                    "@readonly",
                ] {
                    symbols.remove(&format!("{key}:{name}"));
                }
                symbols.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    for key in [
                        "@heap",
                        "@axes",
                        "@lowers",
                        "@extent",
                        "@allocated",
                        "@axis_limits",
                        "@readonly",
                    ] {
                        symbols.remove(&format!("{key}:{n}"));
                    }
                    symbols.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            _ => {}
        }
    }
    allocatables::metadata(body, symbols);
    for stmt in body {
        if let Stmt::DummyIntent { name, intent } = stmt {
            if intent == "pointer" {
                symbols.insert(format!("@pointer:{name}"), (TypeSpec::Logical, vec![]));
            }
            if intent.starts_with("@borrowed_descriptor:") {
                symbols.insert(format!("@borrowed_pointer:{name}"),(TypeSpec::Logical,vec![]));
            }
            if matches!(intent.as_str(),"parameter"|"intent(in)"|"@heap_readonly") {
                symbols.insert(format!("@readonly:{name}"),(TypeSpec::Logical,vec![]));
            }
        }
    }
}
fn axes(n: &str, s: &Shapes) -> Result<Vec<(Expr, Expr)>> {
    let (_, decl) = s.get(n).context("undeclared vector selection source")?;
    if s.contains_key(&format!("@pointer:{n}")) {
        return Ok((1..=decl.len())
            .map(|axis| {
                (
                    Expr::Call("lbound".into(), vec![id(n), int(axis as i64)]),
                    Expr::Call("size".into(), vec![id(n), int(axis as i64)]),
                )
            })
            .collect());
    }
    if let Some(d) = allocatables::axes(n, s) {
        let lowers = allocatables::lowers(n, s).unwrap_or_else(|| vec![int(1); d.len()]);
        return Ok(lowers.into_iter().zip(d).collect());
    }
    if let Some(length) = allocatables::length(n, s) {
        return Ok(vec![(int(1), length)]);
    }
    decl.iter()
        .map(|e| {
            if let Expr::Call(tag, a) = e {
                if tag == "@declared_bounds" {
                    let extent = if let (Some(lo), Some(hi)) = (constant(&a[0]), constant(&a[1])) {
                        int((hi - lo + 1).max(0))
                    } else {
                        Expr::Call(
                            "max".into(),
                            vec![
                                int(0),
                                bin(
                                    BinOp::Add,
                                    bin(BinOp::Sub, a[1].clone(), a[0].clone()),
                                    int(1),
                                ),
                            ],
                        )
                    };
                    return Ok((a[0].clone(), extent));
                }
            }
            if matches!(e,Expr::Ident(n) if n=="@deferred_extent") {
                bail!("vector selection requires a resolved array descriptor");
            }
            Ok((int(1), e.clone()))
        })
        .collect()
}
fn triplet(e: &Expr, axis: &(Expr, Expr)) -> Result<(Expr, Expr, Expr)> {
    let Expr::Section {
        lower,
        upper,
        stride,
        ..
    } = e
    else {
        bail!("invalid array triplet");
    };
    let step = stride.as_deref().cloned().unwrap_or(int(1));
    let Some(step_value) = constant(&step) else {
        let last=bin(BinOp::Sub,bin(BinOp::Add,axis.0.clone(),axis.1.clone()),int(1));
        // Omitted bounds are the declared bounds, whatever the stride's sign.
        let first=lower.as_deref().cloned().unwrap_or_else(||axis.0.clone());
        let end=upper.as_deref().cloned().unwrap_or(last);
        let count=Expr::Call("@runtime_section_extent".into(),vec![first.clone(),end,step.clone()]);
        return Ok((first,count,step));
    };
    if step_value == 0 {
        bail!("array section stride must not be zero");
    }
    let last = bin(
        BinOp::Sub,
        bin(BinOp::Add, axis.0.clone(), axis.1.clone()),
        int(1),
    );
    let first = lower.as_deref().cloned().unwrap_or(axis.0.clone());
    let end = upper.as_deref().cloned().unwrap_or(last);
    let count = if let (Some(first), Some(end)) = (constant(&first), constant(&end)) {
        int(i64::try_from(
            ((i128::from(end) - i128::from(first) + i128::from(step_value))
                / i128::from(step_value))
            .max(0),
        )?)
    } else {
        Expr::Call(
            "max".into(),
            vec![
                int(0),
                bin(
                    BinOp::Div,
                    bin(
                        BinOp::Add,
                        bin(BinOp::Sub, end, first.clone()),
                        step.clone(),
                    ),
                    step.clone(),
                ),
            ],
        )
    };
    Ok((first, count, step))
}
fn shape(e: &Expr, s: &Shapes) -> Result<Option<(TypeSpec, Vec<Expr>)>> {
    match e {
        Expr::Call(n,a) if n=="@packed_view" && a.len()==2=>{
            // Packed views retain a multidimensional descriptor when supplied
            // by argument association; their flat length is not their rank.
            Ok(shape(&a[0],s)?.map(|(kind,dims)|(kind,if dims.len()>1 {dims}else{vec![a[1].clone()]})))
        },
        Expr::Ident(n) => s
            .get(n)
            .map(|(kind, d)| {
                Ok((
                    kind.clone(),
                    if d.is_empty() {
                        vec![]
                    } else {
                        axes(n, s)?.into_iter().map(|(_, d)| d).collect()
                    },
                ))
            })
            .transpose(),
        Expr::IntLit(_) => Ok(Some((TypeSpec::Integer(None), vec![]))),
        Expr::RealLit(n) => Ok(Some((
            if n.to_ascii_lowercase().contains('d') {
                TypeSpec::DoublePrecision
            } else {
                TypeSpec::Real
            },
            vec![],
        ))),
        Expr::Logical(_) => Ok(Some((TypeSpec::Logical, vec![]))),
        Expr::Str(n) => Ok(Some((TypeSpec::Character(Some(n.len())), vec![]))),
        Expr::Section { name, .. } => {
            if matches!(s.get(name),Some((TypeSpec::Character(_),d)) if d.is_empty()) {return Ok(Some((s[name].0.clone(),vec![])));}
            let a = axes(name, s)?;
            if a.len() != 1 {
                bail!("vector expression must be rank one");
            }
            Ok(Some((s[name].0.clone(), vec![triplet(e, &a[0])?.1])))
        }
        Expr::Bin(op, a, b) => {
            let (Some((ak, ad)), Some((bk, bd))) = (shape(a, s)?, shape(b, s)?) else {
                return Ok(None);
            };
            let kind = if matches!(op, BinOp::Concat) {
                super::concatenation_kind(&ak, &bk)?
            } else if matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
            ) {
                super::merge(ak, bk)
            } else {
                TypeSpec::Logical
            };
            Ok(Some((kind, if ad.is_empty() { bd } else { ad })))
        }
        Expr::Un(_, a) => shape(a, s),
        Expr::Call(n, a) if n == "@typed_constructor" && a.len() == 1 => Ok(Some((
            crate::empty_constructors::kind(e)?.context("invalid typed constructor")?,
            vec![int(0)],
        ))),
        Expr::Call(n, a) if n == "@integer_literal" => Ok(Some((
            TypeSpec::Integer(Some(constant(&a[1]).unwrap_or(4) as u8)),
            vec![],
        ))),
        Expr::Call(n, a) if n == "@vector_subscript" => shape(&a[0], s),
        Expr::Call(n,a) if n=="unpack" && a.len()==3 && !s.contains_key(n) => {
            let (Some((kind,_)),Some((_,dims)))=(shape(&a[0],s)?,shape(&a[1],s)?) else {return Ok(None);};
            Ok(Some((kind,dims)))
        }
        Expr::Call(n,a) if matches!(n.as_str(),"cshift"|"eoshift") && !s.contains_key(n) => shape(&a[0],s),
        Expr::Call(n,a) if n == "spread" && a.len()==3 && !s.contains_key(n) => {
            let Some((kind,mut dims))=shape(&a[0],s)? else {return Ok(None);};
            let dim=constant(&a[1]).context("SPREAD DIM must currently be a constant INTEGER")?;
            if dim<1 || dim as usize>dims.len()+1 {bail!("SPREAD DIM must be between 1 and {}",dims.len()+1);}
            dims.insert(dim as usize-1,Expr::Call("max".into(),vec![a[2].clone(),int(0)]));
            Ok(Some((kind,dims)))
        }
        Expr::Call(n, a) if n == "transpose" && a.len() == 1 && !s.contains_key(n) => {
            let Some((kind, mut dims)) = shape(&a[0], s)? else { return Ok(None); };
            if dims.len() != 2 { bail!("TRANSPOSE MATRIX must be a rank-two array"); }
            dims.reverse();
            Ok(Some((kind, dims)))
        }
        Expr::Call(n, a) if n == "reshape" && !s.contains_key(n) => {
            let Some(Expr::Call(tag,dims))=a.get(1) else {bail!("RESHAPE SHAPE must currently be an array constructor");};
            if tag!="@array_constructor" || dims.is_empty() || dims.len()>15 {bail!("RESHAPE SHAPE must contain 1 through 15 extents");}
            crate::multidim::reshape_order(a, dims.len())?;
            let Some((kind, _)) = shape(&a[0], s)? else { return Ok(None); };
            Ok(Some((kind, dims.clone())))
        }
        Expr::Call(n, a) if n == "matmul" && a.len() == 2 && !s.contains_key(n) => {
            let (Some((ak, ad)), Some((bk, bd))) = (shape(&a[0], s)?, shape(&a[1], s)?) else { return Ok(None); };
            let dims = match (ad.as_slice(), bd.as_slice()) {
                ([rows, _], [_, cols]) => vec![rows.clone(), cols.clone()],
                ([rows, _], [_]) => vec![rows.clone()],
                ([_], [_, cols]) => vec![cols.clone()],
                _ => bail!("MATMUL requires rank-one or rank-two arrays, with at least one rank-two operand"),
            };
            Ok(Some((super::merge(ak, bk), dims)))
        }
        Expr::Call(n, a) if n == "@array_constructor" => {
            let mut dims = vec![];
            let mut kind = TypeSpec::Integer(None);
            for (i, e) in a.iter().enumerate() {
                let Some((k, d)) = shape(e, s)? else {
                    return Ok(None);
                };
                if d.len() > 1 {
                    bail!("vector constructors require scalar or rank-one items");
                }
                if i == 0 {
                    kind = k;
                } else if !crate::pointers::same_kind(&kind, &k) {
                    bail!("vector constructor items must have the same type and kind");
                }
                dims.push(d.first().cloned().unwrap_or(int(1)));
            }
            Ok(Some((
                kind,
                vec![dims.into_iter().fold(int(0), |a, b| bin(BinOp::Add, a, b))],
            )))
        }
        Expr::Call(n, a) | Expr::Index(n, a) if s.get(n).is_some_and(|(_, d)| !d.is_empty()) => {
            let source = axes(n, s)?;
            if a.len() != source.len() {
                bail!("array `{n}` requires {} subscripts", source.len());
            }
            let mut dims = vec![];
            for (index, axis) in a.iter().zip(source) {
                if let Expr::Call(tag, items) = index {
                    if tag == "@section_subscript" {
                        dims.push(triplet(&items[0], &axis)?.1);
                        continue;
                    }
                }
                if let Some((kind, shape)) = shape(index, s)? {
                    if !matches!(kind, TypeSpec::Integer(_)) || shape.len() > 1 {
                        bail!(
                            "vector subscript must be a rank-one INTEGER array or scalar INTEGER"
                        );
                    }
                    dims.extend(shape);
                }
            }
            Ok(Some((s[n].0.clone(), dims)))
        }
        Expr::Call(n, a)
            if crate::intrinsics::math_arity(n).is_some() || crate::intrinsics::integer_rounding(n) || crate::intrinsics::real_predicate(n) || matches!(
                n.as_str(),
                "abs" | "int" | "mod" | "modulo" | "sign" | "real" | "dble" | "min" | "max"
            ) =>
        {
            let Some((mut kind, mut d)) = shape(&a[0], s)? else {
                return Ok(None);
            };
            for arg in &a[1..] {
                let Some((_,dims))=shape(arg,s)? else {return Ok(None)};
                if d.is_empty() {d=dims;}
            }
            if crate::intrinsics::integer_rounding(n) {kind=crate::intrinsics::rounding_kind(n,a)?;}
            else if n == "int" { kind = TypeSpec::Integer(a.get(1).map(|e|constant(e).and_then(|k|u8::try_from(k).ok()).context("INT KIND must be a constant integer")).transpose()?); }
            else if crate::intrinsics::real_predicate(n) {kind=TypeSpec::Logical;}
            else if n == "dble" { kind = TypeSpec::DoublePrecision; }
            else if n == "real" { kind = if a.get(1).and_then(constant)==Some(8) {TypeSpec::DoublePrecision}else{TypeSpec::Real}; }
            Ok(Some((kind, d)))
        }
        Expr::Call(n,a) if s.contains_key(&format!("@elemental_function:{n}"))=>{
            let mut dims=vec![];
            for arg in a {let Some((_,shape))=shape(arg,s)? else {return Ok(None)};if !shape.is_empty() {dims=shape;break;}}
            Ok(Some((s[&format!("@elemental_function:{n}")].0.clone(),dims)))
        },
        Expr::Call(n, _) if s.contains_key(&format!("@array_function:{n}")) => Ok(None),
        Expr::Call(n, _) => Ok(s
            .get(&format!("@function:{n}"))
            .map(|(k, _)| (k.clone(), vec![]))),
        _ => Ok(None),
    }
}
fn runtime_character(e:&Expr,s:&Shapes)->bool {
    match e {
        Expr::Ident(n)|Expr::Section{name:n,..}=>allocatables::length(n,s).is_some(),
        Expr::Call(n,a)|Expr::Index(n,a)=>n=="@packed_view" || allocatables::length(n,s).is_some() || a.iter().any(|e|runtime_character(e,s)),
        Expr::Bin(_,a,b)=>runtime_character(a,s)||runtime_character(b,s),Expr::Un(_,a)=>runtime_character(a,s),_=>false,
    }
}
fn vector(e: &Expr, s: &Shapes) -> bool {
    shape(e, s)
        .ok()
        .flatten()
        .is_some_and(|(_, d)| !d.is_empty())
}

fn selection(n: &str, a: &[Expr], s: &Shapes) -> bool {
    // Borrowed recursive descriptors retain their runtime strides. Ordinary
    // triplets can be lowered directly by the pointer pass without a value view.
    if s.contains_key(&format!("@borrowed_pointer:{n}")) && !vector_selection(n,a,s) {return false;}
    if matches!(a,[Expr::Section{name,..}] if name==n) {
        return false;
    }
    s.get(n).is_some_and(|(_, d)| !d.is_empty())
        && a.iter()
            .any(|e| matches!(e,Expr::Call(tag,_) if tag=="@section_subscript") || vector(e, s))
}
fn vector_selection(n: &str, a: &[Expr], s: &Shapes) -> bool {
    s.get(n).is_some_and(|(_, d)| !d.is_empty()) && a.iter()
        .any(|e| !matches!(e,Expr::Call(tag,_) if tag=="@section_subscript") && vector(e,s))
}
fn contains_vector(e: &Expr, s: &Shapes) -> bool {
    match e {
        Expr::Call(n,a) | Expr::Index(n,a) => vector_selection(n,a,s) || a.iter().any(|e|contains_vector(e,s)),
        Expr::Bin(_,a,b) => contains_vector(a,s) || contains_vector(b,s),
        Expr::Un(_,a) => contains_vector(a,s),
        _ => false,
    }
}
fn dynamic(n: &str, a: &[Expr], s: &Shapes) -> bool {
    if !selection(n, a, s) {
        return false;
    }
    allocatables::length(n,s).is_some() || axes(n,s).is_ok_and(|d|d.iter().any(|(l,n)|constant(l).is_none()||constant(n).is_none()))
        // A scalar selector such as the column in A(:,column) changes the
        // selected storage even when the section's shape is entirely fixed.
        || a.iter().any(|e|!matches!(e,Expr::Call(tag,_) if tag=="@section_subscript")
            && shape(e,s).ok().flatten().is_some_and(|(_,d)|d.is_empty()) && constant(e).is_none())
        || a.iter().any(|e|shape(e,s).ok().flatten().is_some_and(|(_,d)|d.iter().any(|n|constant(n).is_none() || constant(n).is_some_and(|n|n>1024)))
            || matches!(e,Expr::Call(tag,items) if tag=="@section_subscript" && matches!(&items[0],Expr::Section{lower,upper,stride,..} if lower.as_deref().is_some_and(|e|constant(e).is_none())||upper.as_deref().is_some_and(|e|constant(e).is_none())||stride.as_deref().is_some_and(|e|constant(e).is_none()))))
}
fn contains(e: &Expr, s: &Shapes) -> bool {
    match e {
        Expr::Section{name,..} if !s.contains_key(&format!("@pointer:{name}")) && s.get(name).is_some_and(|(_,d)|d.len()==1) =>
            dynamic(name,&[Expr::Call("@section_subscript".into(),vec![e.clone()])],s),
        Expr::Call(n, a) | Expr::Index(n, a) => {
            dynamic(n, a, s) || a.iter().any(|e| contains(e, s))
        }
        Expr::Bin(_, a, b) => contains(a, s) || contains(b, s),
        Expr::Un(_, a) => contains(a, s),
        _ => false,
    }
}
fn capacity(e: &Expr, s: &Shapes) -> Option<Vec<i64>> {
    match e {
        // Heap backing declarations have extent one; that is not a capacity bound.
        Expr::Ident(n) => {
            if s.contains_key(&format!("@heap:{n}")) {
                None
            } else {
                s.get(n)
                    .and_then(|(_, d)| d.iter().map(super::fixed_extent).collect())
            }
        }
        Expr::Section { name, .. } => capacity(&id(name), s),
        Expr::Call(n, a) if n == "@vector_subscript" => capacity(&a[0], s),
        Expr::Call(n, a) if n == "@array_constructor" => Some(vec![a
            .iter()
            .map(|e| {
                if vector(e, s) {
                    capacity(e, s)?.first().copied()
                } else {
                    Some(1)
                }
            })
            .collect::<Option<Vec<_>>>()?
            .iter()
            .sum()]),
        Expr::Bin(_, a, b) => capacity(a, s).or_else(|| capacity(b, s)),
        Expr::Un(_, a) => capacity(a, s),
        _ => None,
    }
}
fn selection_capacity(n: &str, args: &[Expr], s: &Shapes) -> Option<Vec<i64>> {
    let source = capacity(&id(n), s)?;
    let mut result = vec![];
    for (i, e) in args.iter().enumerate() {
        if matches!(e,Expr::Call(tag,_) if tag=="@section_subscript") {
            result.push(*source.get(i)?);
        } else if vector(e, s) {
            result.push(*capacity(e, s)?.first()?);
        }
    }
    Some(result)
}
#[derive(Clone)]
enum Axis {
    Scalar(Expr),
    Vector(String),
    Triplet(Expr, Expr),
}
#[derive(Default)]
struct Lower {
    serial: usize,
    procedures: std::sync::Arc<HashMap<String, Vec<String>>>,
    force: bool,
    shallow: bool,
}
impl Lower {
    fn scalar(&mut self, role: &str, value: Expr, out: &mut Vec<Stmt>, s: &mut Shapes) -> Expr {
        if let Some(n) = constant(&value) {
            return int(n);
        }
        self.serial += 1;
        let n = format!("@vector_runtime_{role}_{}", self.serial);
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![n.clone()],
        });
        out.push(assign(&n, value));
        s.insert(n.clone(), (TypeSpec::Integer(Some(8)), vec![]));
        id(&n)
    }
    fn buffer(
        &mut self,
        kind: TypeSpec,
        dims: &[Expr],
        out: &mut Vec<Stmt>,
        s: &mut Shapes,
    ) -> Result<String> {
        self.buffer_capacity(kind, dims, None, out, s)
    }
    fn buffer_capacity(
        &mut self,
        kind: TypeSpec,
        dims: &[Expr],
        capacity: Option<Vec<i64>>,
        out: &mut Vec<Stmt>,
        s: &mut Shapes,
    ) -> Result<String> {
        self.serial += 1;
        let n = format!("@vector_runtime_values_{}", self.serial);
        let character=matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_));
        let fixed_count=dims.iter().try_fold(1i64,|n,d|n.checked_mul(constant(d)?));
        if dims.iter().all(|d| constant(d).is_some()) && !(character && fixed_count.is_none_or(|n|n>1024)) {
            out.push(Stmt::ArrayDecl {
                kind: kind.clone(),
                name: n.clone(),
                dims: dims.to_vec(),
            });
            s.insert(n.clone(), (kind, dims.to_vec()));
        } else {
            if matches!(
                kind,
                TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_)
            ) && capacity.as_ref().is_some_and(|d|d.iter().try_fold(1i64,|n,d|n.checked_mul(*d)).is_some_and(|n|n<=1024)) {
                let capacity = capacity.context(
                    "runtime-sized CHARACTER vector selections require bounded result storage",
                )?;
                let declaration = capacity.iter().map(|n| int(*n)).collect::<Vec<_>>();
                out.push(Stmt::ArrayDecl {
                    kind: kind.clone(),
                    name: n.clone(),
                    dims: declaration.clone(),
                });
                let length = self.scalar("length", product(dims), out, s);
                let mut fields = vec![];
                for dim in dims {
                    let value = self.scalar("extent", dim.clone(), out, s);
                    if let Expr::Ident(field) = value {
                        fields.push(field);
                    } else {
                        self.serial += 1;
                        let field = format!("@vector_runtime_extent_{}", self.serial);
                        out.push(Stmt::VarDecl {
                            kind: TypeSpec::Integer(Some(8)),
                            names: vec![field.clone()],
                        });
                        out.push(assign(&field, value));
                        fields.push(field);
                    }
                }
                let Expr::Ident(length) = length else {
                    unreachable!()
                };
                let flag = format!("{n}$allocated");
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Logical,
                    names: vec![flag.clone()],
                });
                out.push(assign(&flag, Expr::Logical(true)));
                out.push(Stmt::DummyIntent {
                    name: n.clone(),
                    intent: format!("@bounded:{length}:{flag}"),
                });
                out.push(Stmt::DummyIntent {
                    name: n.clone(),
                    intent: format!("@bounded_shape:{}", fields.join(":")),
                });
                out.push(Stmt::DummyIntent {
                    name: n.clone(),
                    intent: format!(
                        "@bounded_limits:{}",
                        capacity
                            .iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<_>>()
                            .join(":")
                    ),
                });
                s.insert(n.clone(), (kind, declaration));
                declarations(out, s);
                return Ok(n);
            }
            heap_buffer(kind, n.clone(), dims, out, s)?;
        }
        Ok(n)
    }
    fn loop_vars(&mut self, dims: &[Expr], out: &mut Vec<Stmt>, s: &mut Shapes) -> Vec<String> {
        dims.iter()
            .map(|_| {
                self.serial += 1;
                let n = format!("@vector_runtime_coordinate_{}", self.serial);
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![n.clone()],
                });
                s.insert(n.clone(), (TypeSpec::Integer(Some(8)), vec![]));
                n
            })
            .collect()
    }
    fn nest(&self, vars: &[String], dims: &[Expr], mut body: Vec<Stmt>) -> Vec<Stmt> {
        for (var, end) in vars.iter().zip(dims) {
            body = vec![Stmt::Do {
                var: var.clone(),
                start: int(1),
                end: end.clone(),
                body,
            }];
        }
        body
    }
    fn element(&mut self, e: &Expr, index: Expr, s: &Shapes, out: &mut Vec<Stmt>) -> Result<Expr> {
        if !vector(e, s) {
            return Ok(e.clone());
        }
        Ok(match e {
            Expr::Ident(n) => {
                let a = axes(n, s)?;
                if a.len() != 1 {
                    bail!("vector subscript must be a rank-one INTEGER array");
                }
                Expr::Index(
                    n.clone(),
                    vec![bin(
                        BinOp::Add,
                        a[0].0.clone(),
                        bin(BinOp::Sub, index, int(1)),
                    )],
                )
            }
            Expr::Section { name, .. } => {
                let (first, _, step) = triplet(e, &axes(name, s)?[0])?;
                Expr::Index(
                    name.clone(),
                    vec![bin(
                        BinOp::Add,
                        first,
                        bin(BinOp::Mul, bin(BinOp::Sub, index, int(1)), step),
                    )],
                )
            }
            Expr::Bin(op, a, b) => bin(
                op.clone(),
                self.element(a, index.clone(), s, out)?,
                self.element(b, index, s, out)?,
            ),
            Expr::Un(op, a) => Expr::Un(op.clone(), Box::new(self.element(a, index, s, out)?)),
            Expr::Call(n, a) if matches!(n.as_str(), "abs" | "int" | "mod" | "modulo" | "sign") => {
                Expr::Call(
                    n.clone(),
                    a.iter()
                        .map(|e| self.element(e, index.clone(), s, out))
                        .collect::<Result<_>>()?,
                )
            }
            Expr::Call(n, args) | Expr::Index(n, args)
                if s.get(n).is_some_and(|(_, d)| !d.is_empty()) =>
            {
                let source = axes(n, s)?;
                let mut selected = vec![];
                for (arg, axis) in args.iter().zip(source) {
                    if let Expr::Call(tag, items) = arg {
                        if tag == "@section_subscript" {
                            let (first, _, step) = triplet(&items[0], &axis)?;
                            selected.push(bin(
                                BinOp::Add,
                                first,
                                bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), int(1)), step),
                            ));
                            continue;
                        }
                    }
                    selected.push(arg.clone());
                }
                Expr::Index(n.clone(), selected)
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                self.serial += 1;
                let result = format!("@vector_runtime_constructor_{}", self.serial);
                out.push(Stmt::VarDecl {
                    kind: shape(e,s)?.context("unknown constructor element type")?.0,
                    names: vec![result.clone()],
                });
                let mut offset = int(0);
                for arg in a {
                    let dims = shape(arg, s)?.context("unknown vector constructor item")?.1;
                    let count = dims.first().cloned().unwrap_or(int(1));
                    let mut body = vec![];
                    let value = self.element(
                        arg,
                        bin(BinOp::Sub, index.clone(), offset.clone()),
                        s,
                        &mut body,
                    )?;
                    body.push(assign(&result, value));
                    let end = bin(BinOp::Add, offset.clone(), count);
                    out.push(Stmt::If {
                        cond: bin(
                            BinOp::And,
                            bin(BinOp::Gt, index.clone(), offset),
                            bin(BinOp::Le, index.clone(), end.clone()),
                        ),
                        then_body: body,
                        else_body: None,
                    });
                    offset = end;
                }
                id(&result)
            }
            Expr::Call(n, a) if n == "@typed_constructor" && a.len() == 1 => int(0),
            // Argument association retains a packed array's active length in
            // a view. Shape/capture uses that length; element access still uses
            // the underlying storage and its lower bound.
            Expr::Call(n, a) if n == "@packed_view" && a.len() == 2 => self.element(&a[0], index, s, out)?,
            Expr::Call(n, a) if n == "@vector_subscript" => self.element(&a[0], index, s, out)?,
            _ => bail!("this vector subscript expression is not yet supported"),
        })
    }
    fn freeze(&mut self, e: &mut Expr, s: &mut Shapes, out: &mut Vec<Stmt>) -> Result<()> {
        if let Expr::Section {
            lower,
            upper,
            stride,
            ..
        } = e
        {
            for bound in [lower, upper, stride].into_iter().flatten() {
                self.freeze(bound, s, out)?;
            }
            return Ok(());
        }
        if matches!(e,Expr::Call(tag,_) if tag=="@section_subscript" || tag=="@vector_subscript") {
            if let Expr::Call(_, a) = e {
                for e in a {
                    self.freeze(e, s, out)?;
                }
            }
            return Ok(());
        }
        if let Some((kind, dims)) = shape(e, s)? {
            if dims.is_empty() {
                if !matches!(
                    e,
                    Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) | Expr::Str(_)
                ) && constant(e).is_none()
                {
                    self.serial += 1;
                    let n = format!("@vector_runtime_operand_{}", self.serial);
                    out.push(Stmt::VarDecl {
                        kind: kind.clone(),
                        names: vec![n.clone()],
                    });
                    out.push(assign(&n, e.clone()));
                    s.insert(n.clone(), (kind, vec![]));
                    *e = id(&n);
                }
                return Ok(());
            }
        }
        match e {
            Expr::Bin(_, a, b) => {
                self.freeze(a, s, out)?;
                self.freeze(b, s, out)?;
            }
            Expr::Un(_, a) => self.freeze(a, s, out)?,
            Expr::Call(n, a) | Expr::Index(n, a)
                if s.get(n).is_some_and(|(_, d)| !d.is_empty()) =>
            {
                for e in a {
                    self.freeze(e, s, out)?;
                }
            }
            Expr::Call(n, a)
                if matches!(
                    n.as_str(),
                    "@array_constructor" | "abs" | "int" | "mod" | "modulo" | "sign"
                ) =>
            {
                for e in a {
                    self.freeze(e, s, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn indices(
        &mut self,
        n: &str,
        args: &mut [Expr],
        s: &mut Shapes,
        out: &mut Vec<Stmt>,
        writable: bool,
    ) -> Result<(Vec<Axis>, Vec<Expr>)> {
        let source = axes(n, s)?;
        if source.len() != args.len() {
            bail!("array `{n}` requires {} subscripts", source.len());
        }
        if let Some(flag) = allocatables::allocated(n, s) {
            out.push(error(
                Expr::Un(crate::ast::UnOp::Not, Box::new(flag)),
                format!("vector selection of unallocated array `{n}`"),
            ));
        }
        let mut result = vec![];
        let mut dims = vec![];
        for (arg, (lower, extent)) in args.iter_mut().zip(source) {
            self.freeze(arg, s, out)?;
            if let Expr::Call(tag, a) = arg {
                if tag == "@vector_subscript" {
                    *arg = a[0].clone();
                }
            }
            let source_axis = (lower.clone(), extent.clone());
            let upper = bin(BinOp::Sub, bin(BinOp::Add, lower.clone(), extent), int(1));
            if let Expr::Call(tag, a) = arg {
                if tag == "@section_subscript" {
                    let (first, count, step) = triplet(&a[0], &source_axis)?;
                    let first = self.scalar("first", first, out, s);
                    let count = self.scalar("count", count, out, s);
                    dims.push(count);
                    result.push(Axis::Triplet(first, step));
                    continue;
                }
            }
            let previous = self.force;
            self.force = true;
            self.expression(arg, s, out)?;
            self.force = previous;
            let (kind, d) = shape(arg, s)?.context("unknown vector subscript type")?;
            if !matches!(kind, TypeSpec::Integer(_)) || d.len() > 1 {
                bail!("vector subscript must be a rank-one INTEGER array or scalar INTEGER");
            }
            if d.is_empty() {
                let value = self.scalar("scalar_index", arg.clone(), out, s);
                result.push(Axis::Scalar(value));
                continue;
            }
            let count = self.scalar("count", d[0].clone(), out, s);
            let captured = self.buffer(
                TypeSpec::Integer(Some(8)),
                std::slice::from_ref(&count),
                out,
                s,
            )?;
            let vars = self.loop_vars(std::slice::from_ref(&count), out, s);
            let i = id(&vars[0]);
            let mut body = vec![];
            let value = self.element(arg, i.clone(), s, &mut body)?;
            body.push(Stmt::AssignIndex {
                name: captured.clone(),
                indices: vec![i.clone()],
                value,
            });
            let read = Expr::Index(captured.clone(), vec![i]);
            body.push(error(
                bin(
                    BinOp::Or,
                    bin(BinOp::Lt, read.clone(), lower),
                    bin(BinOp::Gt, read, upper),
                ),
                format!("vector subscript of `{n}` is outside bounds"),
            ));
            out.extend(self.nest(&vars, std::slice::from_ref(&count), body));
            if writable {
                let other = self.loop_vars(std::slice::from_ref(&count), out, s);
                let j = id(&other[0]);
                out.push(Stmt::Do{var:vars[0].clone(),start:int(1),end:count.clone(),body:vec![Stmt::Do{var:other[0].clone(),start:bin(BinOp::Add,id(&vars[0]),int(1)),end:count.clone(),body:vec![error(bin(BinOp::Eq,Expr::Index(captured.clone(),vec![id(&vars[0])]),Expr::Index(captured.clone(),vec![j])),format!("vector-subscript assignment to `{n}` selects an element more than once"))]}]});
            }
            dims.push(count);
            result.push(Axis::Vector(captured));
        }
        Ok((result, dims))
    }
    fn selected(&self, axes: &[Axis], vars: &[String]) -> Vec<Expr> {
        let mut coordinates = vars.iter();
        axes.iter()
            .map(|a| match a {
                Axis::Scalar(e) => e.clone(),
                Axis::Vector(n) => Expr::Index(n.clone(), vec![id(coordinates.next().unwrap())]),
                Axis::Triplet(first, step) => bin(
                    BinOp::Add,
                    first.clone(),
                    bin(
                        BinOp::Mul,
                        bin(BinOp::Sub, id(coordinates.next().unwrap()), int(1)),
                        step.clone(),
                    ),
                ),
            })
            .collect()
    }
    fn character_element(e:&Expr, vars:&[String], s:&Shapes)->Result<Expr> {
        if let Expr::Ident(n)=e {
            if s.get(n).is_some_and(|(_,d)|!d.is_empty()) {
                let indices=axes(n,s)?.into_iter().zip(vars).map(|((lower,_),v)|bin(BinOp::Add,lower,bin(BinOp::Sub,id(v),int(1)))).collect();
                return Ok(Expr::Index(n.clone(),indices));
            }
        }
        Ok(e.clone())
    }
    // Select an element in Fortran array-element order, retaining lower bounds.
    fn reshape_element(&mut self,e:&Expr,index:Expr,s:&Shapes,out:&mut Vec<Stmt>)->Result<Expr> {
        if let Expr::Ident(n)=e {
            let axes=axes(n,s)?;
            if axes.is_empty() {return Ok(e.clone());}
            let mut offset=bin(BinOp::Sub,index,int(1));
            let mut indices=vec![];
            for (lower,extent) in axes {
                let divisor=Expr::Call("max".into(),vec![extent.clone(),int(1)]);
                indices.push(bin(BinOp::Add,lower,Expr::Call("modulo".into(),vec![offset.clone(),divisor.clone()])));
                offset=bin(BinOp::Div,offset,divisor);
            }
            return Ok(Expr::Index(n.clone(),indices));
        }
        match e {
            Expr::Bin(op,a,b) => return Ok(bin(op.clone(),self.reshape_element(a,index.clone(),s,out)?,self.reshape_element(b,index,s,out)?)),
            Expr::Un(op,a) => return Ok(Expr::Un(op.clone(),Box::new(self.reshape_element(a,index,s,out)?))),
            Expr::Call(n,a) if n == "@packed_view" && a.len()==2 => return self.reshape_element(&a[0],index,s,out),
            _ => {}
        }
        self.element(e,index,s,out)
    }
    // Capture scalar controls once, including when an empty result has no iterations.
    fn capture_scalar(&mut self,e:Expr,kind:TypeSpec,s:&mut Shapes,out:&mut Vec<Stmt>)->Expr {
        self.serial+=1;
        let n=format!("@vector_runtime_control_{}",self.serial);
        out.push(Stmt::VarDecl{kind:kind.clone(),names:vec![n.clone()]});
        if matches!(kind,TypeSpec::Character(_)) {self.preserve_character_length(&e,&n,s,out);}
        out.push(assign(&n,e));
        s.insert(n.clone(),(kind,vec![]));
        id(&n)
    }
    fn character_length(e:&Expr,s:&Shapes)->Result<Expr> {
        let name=match e {Expr::Ident(n)|Expr::Index(n,_)|Expr::Section{name:n,..}=>Some(n),_=>None};
        if let Some((_,v))=name.and_then(|n|s.get(&format!("@character_spec_length:{n}"))) {
            if let Some(length)=v.first() {return Ok(length.clone());}
        }
        if let Expr::Call(tag,a)=e {if tag=="@packed_view" {return Self::character_length(&a[0],s);}}
        match shape(e,s)? {Some((TypeSpec::Character(n),_))=>Ok(int(n.unwrap_or(1) as i64)),_=>bail!("CHARACTER expression requires a known element length")}
    }
    fn conform_character_lengths(a:&Expr,b:&Expr,s:&Shapes,out:&mut Vec<Stmt>,label:&str)->Result<()> {
        let (a,b)=(Self::character_length(a,s)?,Self::character_length(b,s)?);
        if let (Some(a),Some(b))=(constant(&a),constant(&b)) {
            if a!=b {bail!("{label} CHARACTER lengths must match");}
        }else{out.push(error(bin(BinOp::Ne,a,b),format!("{label} CHARACTER lengths must match")));}
        Ok(())
    }
    fn preserve_character_length(&mut self,source:&Expr,target:&str,s:&mut Shapes,out:&mut Vec<Stmt>) {
        let name=match source {Expr::Ident(n)|Expr::Index(n,_)|Expr::Section{name:n,..}=>Some(n),Expr::Call(tag,a) if tag=="@packed_view"=>match a.first(){Some(Expr::Ident(n))=>Some(n),_=>None},_=>None};
        if let Some((_,values))=name.and_then(|n|s.get(&format!("@character_spec_length:{n}"))).cloned() {
            if let Some(value)=values.first() {
                let length=self.capture_scalar(value.clone(),TypeSpec::Integer(Some(8)),s,out);
                if let Expr::Ident(n)=length {
                    out.push(Stmt::DummyIntent{name:target.into(),intent:format!("@character_spec_length:{n}")});
                    s.insert(format!("@character_spec_length:{target}"),(TypeSpec::Integer(Some(8)),vec![id(&n)]));
                }
            }
        }
    }
    fn unpack(&mut self,e:&mut Expr,s:&mut Shapes,out:&mut Vec<Stmt>)->Result<()> {
        let Expr::Call(name,args)=e else {return Ok(());};
        if name!="unpack" || s.contains_key(name) || args.len()!=3 {return Ok(());}
        let (Some((kind,vd)),Some((mk,md)),Some((fk,fd)))=(shape(&args[0],s)?,shape(&args[1],s)?,shape(&args[2],s)?) else {return Ok(());};
        if !matches!(kind,TypeSpec::Character(_)) && vd.iter().chain(&md).chain(&fd).all(|d|constant(d).is_some()) {return Ok(());}
        if vd.len()!=1 {bail!("UNPACK VECTOR must be a rank-one array");}
        if md.is_empty() || mk!=TypeSpec::Logical {bail!("UNPACK MASK must be a LOGICAL array");}
        if matches!((&kind,&fk),(TypeSpec::Character(_),TypeSpec::Character(_))) {
            Self::conform_character_lengths(&args[0],&args[2],s,out,"UNPACK VECTOR and FIELD")?;
        }else if !crate::pointers::same_kind(&kind,&fk) {bail!("UNPACK VECTOR and FIELD must have the same type and kind");}
        let length=self.scalar("unpack_length",vd[0].clone(),out,s);
        let mut dims=vec![];
        for d in md {dims.push(self.scalar("unpack_extent",d,out,s));}
        if fd.is_empty() {args[2]=self.capture_scalar(args[2].clone(),fk,s,out);}else{
            if fd.len()!=dims.len() {bail!("UNPACK FIELD shape must match MASK");}
            for (a,b) in fd.iter().zip(&dims) {out.push(error(bin(BinOp::Ne,a.clone(),b.clone()),"UNPACK FIELD shape must match MASK".into()));}
        }
        let start=out.len();
        let result=self.buffer(kind,&dims,out,s)?;
        let mut allocation=out.split_off(start);
        self.preserve_character_length(&args[0],&result,s,out);
        out.append(&mut allocation);
        let vars=self.loop_vars(&dims,out,s);
        let cursor=self.capture_scalar(int(0),TypeSpec::Integer(Some(8)),s,out);
        let Expr::Ident(cursor_name)=&cursor else {unreachable!()};
        let mut index=int(1);let mut stride=int(1);
        for (v,d) in vars.iter().zip(&dims) {
            index=bin(BinOp::Add,index,bin(BinOp::Mul,bin(BinOp::Sub,id(v),int(1)),stride.clone()));
            stride=bin(BinOp::Mul,stride,d.clone());
        }
        let mut body=vec![];
        let mask=self.reshape_element(&args[1],index.clone(),s,&mut body)?;
        let mut selected=vec![assign(cursor_name,bin(BinOp::Add,cursor.clone(),int(1))),error(bin(BinOp::Gt,cursor.clone(),length),"UNPACK VECTOR has too few elements for MASK".into())];
        let value=self.reshape_element(&args[0],cursor,s,&mut selected)?;
        let store=|value|Stmt::AssignIndex{name:result.clone(),indices:vars.iter().map(|n|id(n)).collect(),value};
        selected.push(store(value));
        let mut rejected=vec![];
        let value=self.reshape_element(&args[2],index,s,&mut rejected)?;
        rejected.push(store(value));
        body.push(Stmt::If{cond:mask,then_body:selected,else_body:Some(rejected)});
        out.extend(self.nest(&vars,&dims,body));
        *e=id(&result);
        Ok(())
    }
    fn shift(&mut self,e:&mut Expr,s:&mut Shapes,out:&mut Vec<Stmt>)->Result<()> {
        let Expr::Call(name,args)=e else {return Ok(());};
        if !matches!(name.as_str(),"cshift"|"eoshift") || s.contains_key(name) {return Ok(());}
        let endoff=name=="eoshift";
        let label=name.to_ascii_uppercase();
        let Some((kind,source_dims))=shape(&args[0],s)? else {return Ok(());};
        let mut runtime_control=false;
        for arg in &args[1..if endoff {3}else{2}] {
            if let Some((_,d))=shape(arg,s)? {runtime_control |= d.iter().any(|d|constant(d).is_none());}
        }
        if !matches!(kind,TypeSpec::Character(_)) && !runtime_control && source_dims.iter().all(|d|constant(d).is_some()) {return Ok(());}
        let dim=constant(&args[if endoff {3}else{2}]).with_context(||format!("{label} DIM must currently be a constant INTEGER"))?;
        if dim<1 || dim as usize>source_dims.len() {bail!("{label} DIM must be between 1 and {}",source_dims.len());}
        let axis=dim as usize-1;
        let mut dims=vec![];
        for d in source_dims {dims.push(self.scalar("shift_extent",d,out,s));}
        let mut expected=dims.clone();expected.remove(axis);
        let mut controls=vec![1];
        let default_boundary=endoff && matches!(&args[2],Expr::Call(n,_) if n=="@no_boundary");
        if endoff && !default_boundary {controls.push(2);}
        for i in controls {
            let control=if i==1 {"SHIFT"}else{"BOUNDARY"};
            let Some((ck,cd))=shape(&args[i],s)? else {return Ok(());};
            if i==1 && !matches!(ck,TypeSpec::Integer(_)) {bail!("{label} SHIFT must be INTEGER");}
            if i==2 {
                if matches!((&kind,&ck),(TypeSpec::Character(_),TypeSpec::Character(_))) {
                    Self::conform_character_lengths(&args[0],&args[i],s,out,"EOSHIFT ARRAY and BOUNDARY")?;
                }else if !crate::pointers::same_kind(&kind,&ck) {bail!("EOSHIFT BOUNDARY and ARRAY must have the same type and kind");}
            }
            if cd.is_empty() {args[i]=self.capture_scalar(args[i].clone(),ck,s,out);} else {
                if cd.len()!=expected.len() {bail!("{label} {control} shape must match ARRAY with DIM removed");}
                for (actual,wanted) in cd.iter().zip(&expected) {
                    out.push(error(bin(BinOp::Ne,actual.clone(),wanted.clone()),format!("{label} {control} shape must match ARRAY with DIM removed")));
                }
            }
        }
        let start=out.len();
        let result=self.buffer(kind.clone(),&dims,out,s)?;
        let mut allocation=out.split_off(start);
        self.preserve_character_length(&args[0],&result,s,out);
        out.append(&mut allocation);
        let vars=self.loop_vars(&dims,out,s);
        let mut slice=int(1);let mut stride=int(1);
        for (i,d) in dims.iter().enumerate() {
            if i==axis {continue;}
            slice=bin(BinOp::Add,slice,bin(BinOp::Mul,bin(BinOp::Sub,id(&vars[i]),int(1)),stride.clone()));
            stride=bin(BinOp::Mul,stride,d.clone());
        }
        let mut body=vec![];
        let amount=self.reshape_element(&args[1],slice.clone(),s,&mut body)?;
        let amount=self.scalar("shift_amount",amount,&mut body,s);
        let coordinate=id(&vars[axis]);
        let extent=dims[axis].clone();
        let shifted=if endoff {bin(BinOp::Add,coordinate.clone(),amount.clone())} else {
            let divisor=Expr::Call("max".into(),vec![extent.clone(),int(1)]);
            let reduced=Expr::Call("modulo".into(),vec![amount.clone(),divisor.clone()]);
            bin(BinOp::Add,int(1),Expr::Call("modulo".into(),vec![bin(BinOp::Add,bin(BinOp::Sub,coordinate.clone(),int(1)),reduced),divisor]))
        };
        let mut index=int(1);let mut stride=int(1);
        for (i,d) in dims.iter().enumerate() {
            let c=if i==axis {shifted.clone()}else{id(&vars[i])};
            index=bin(BinOp::Add,index,bin(BinOp::Mul,bin(BinOp::Sub,c,int(1)),stride.clone()));
            stride=bin(BinOp::Mul,stride,d.clone());
        }
        let mut selected=vec![];
        let value=self.reshape_element(&args[0],index,s,&mut selected)?;
        let store=|value|Stmt::AssignIndex{name:result.clone(),indices:vars.iter().map(|n|id(n)).collect(),value};
        selected.push(store(value));
        if endoff {
            let mut vacant=vec![];
            let boundary=if default_boundary {match kind {
                TypeSpec::Logical=>Expr::Logical(false),TypeSpec::Real=>Expr::RealLit("0.0".into()),TypeSpec::DoublePrecision=>Expr::RealLit("0.0d0".into()),TypeSpec::Character(_)=>Expr::Str(String::new()),_=>int(0)
            }}else{self.reshape_element(&args[2],slice,s,&mut vacant)?};
            vacant.push(store(boundary));
            // Check before adding SHIFT: extreme INTEGER(8) values must not overflow.
            let valid=bin(BinOp::And,bin(BinOp::Ge,amount.clone(),bin(BinOp::Sub,int(1),coordinate.clone())),bin(BinOp::Le,amount,bin(BinOp::Sub,extent,coordinate)));
            body.push(Stmt::If{cond:valid,then_body:selected,else_body:Some(vacant)});
        }else{body.extend(selected);}
        out.extend(self.nest(&vars,&dims,body));
        *e=id(&result);
        Ok(())
    }
    fn spread(&mut self,e:&mut Expr,s:&mut Shapes,out:&mut Vec<Stmt>)->Result<()> {
        let Expr::Call(name,args)=e else {return Ok(());};
        if name!="spread" || s.contains_key(name) || args.len()!=3 {return Ok(());}
        let Some((kind,source_dims))=shape(&args[0],s)? else {return Ok(());};
        if (!source_dims.is_empty() || s.contains_key("@where_scope")) && constant(&args[2]).is_some() && source_dims.iter().all(|d|constant(d).is_some()) {return Ok(());}
        let dim=constant(&args[1]).context("SPREAD DIM must currently be a constant INTEGER")?;
        if dim<1 || dim as usize>source_dims.len()+1 {bail!("SPREAD DIM must be between 1 and {}",source_dims.len()+1);}
        if source_dims.len()>=15 {bail!("SPREAD result rank must not exceed 15");}
        if !crate::output::integer(&args[2],s) || shape(&args[2],s)?.is_some_and(|(_,d)|!d.is_empty()) {bail!("SPREAD NCOPIES must be scalar INTEGER");}
        if source_dims.is_empty() {
            self.serial+=1;
            let scalar=format!("@vector_runtime_spread_source_{}",self.serial);
            out.push(Stmt::VarDecl{kind:kind.clone(),names:vec![scalar.clone()]});
            out.push(assign(&scalar,args[0].clone()));
            s.insert(scalar.clone(),(kind.clone(),vec![]));
            args[0]=id(&scalar);
        }
        let mut dims=vec![];
        for d in source_dims {dims.push(self.scalar("spread_extent",d,out,s));}
        let copies=if let Some(n)=constant(&args[2]) {int(n.max(0))} else {self.scalar("spread_copies",Expr::Call("max".into(),vec![args[2].clone(),int(0)]),out,s)};
        let axis=dim as usize-1;
        dims.insert(axis,copies);
        let result=self.buffer(kind,&dims,out,s)?;
        let vars=self.loop_vars(&dims,out,s);
        let mut index=int(1);
        let mut stride=int(1);
        for (i,d) in dims.iter().enumerate() {
            if i==axis {continue;}
            index=bin(BinOp::Add,index,bin(BinOp::Mul,bin(BinOp::Sub,id(&vars[i]),int(1)),stride.clone()));
            stride=bin(BinOp::Mul,stride,d.clone());
        }
        let mut body=vec![];
        let value=self.reshape_element(&args[0],index,s,&mut body)?;
        body.push(Stmt::AssignIndex{name:result.clone(),indices:vars.iter().map(|n|id(n)).collect(),value});
        out.extend(self.nest(&vars,&dims,body));
        *e=id(&result);
        Ok(())
    }
    fn transpose(&mut self,e:&mut Expr,s:&mut Shapes,out:&mut Vec<Stmt>)->Result<()> {
        let Expr::Call(name,args)=e else {return Ok(());};
        if name!="transpose" || s.contains_key(name) || args.len()!=1 {return Ok(());}
        let Some((kind,source_dims))=shape(&args[0],s)? else {return Ok(());};
        if source_dims.len()!=2 {bail!("TRANSPOSE MATRIX must be a rank-two array");}
        if source_dims.iter().all(|d|constant(d).is_some()) {return Ok(());}
        let rows=self.scalar("transpose_rows",source_dims[0].clone(),out,s);
        let cols=self.scalar("transpose_cols",source_dims[1].clone(),out,s);
        let dims=vec![cols,rows.clone()];
        let result=self.buffer(kind,&dims,out,s)?;
        let vars=self.loop_vars(&dims,out,s);
        let index=bin(BinOp::Add,id(&vars[1]),bin(BinOp::Mul,bin(BinOp::Sub,id(&vars[0]),int(1)),rows));
        let mut body=vec![];
        let value=self.reshape_element(&args[0],index,s,&mut body)?;
        body.push(Stmt::AssignIndex{name:result.clone(),indices:vars.iter().map(|n|id(n)).collect(),value});
        out.extend(self.nest(&vars,&dims,body));
        *e=id(&result);
        Ok(())
    }
    fn reshape(&mut self,e:&mut Expr,s:&mut Shapes,out:&mut Vec<Stmt>)->Result<()> {
        let Expr::Call(name,_)=e else {return Ok(());};
        if name!="reshape" || s.contains_key(name) {return Ok(());}
        let Some((kind,dimensions))=shape(e,s)? else {return Ok(());};
        let Expr::Call(_,args)=e else {unreachable!()};
        let Some((_,source_dims))=shape(&args[0],s)? else {return Ok(());};
        if source_dims.is_empty() {bail!("RESHAPE SOURCE must be an array");}
        if dimensions.iter().chain(&source_dims).all(|d|constant(d).is_some()) {return Ok(());}
        let order=crate::multidim::reshape_order(args,dimensions.len())?;
        let mut dims=vec![];
        for d in dimensions {
            if !crate::output::integer(&d,s) {bail!("RESHAPE SHAPE extents must be scalar INTEGER");}
            let extent=self.scalar("reshape_extent",d,out,s);
            out.push(error(bin(BinOp::Lt,extent.clone(),int(0)),"RESHAPE SHAPE extents must be nonnegative".into()));
            dims.push(extent);
        }
        let count=self.scalar("reshape_source_count",product(&source_dims),out,s);
        let total=self.scalar("reshape_count",product(&dims),out,s);
        let pad=if matches!(&args[2],Expr::Call(n,_) if n=="@no_pad") {None} else {
            let Some((pk,pd))=shape(&args[2],s)? else {return Ok(());};
            if pd.is_empty() {bail!("RESHAPE PAD must be an array");}
            if !crate::pointers::same_kind(&kind,&pk) {bail!("RESHAPE SOURCE and PAD must have the same type and kind");}
            Some(self.scalar("reshape_pad_count",product(&pd),out,s))
        };
        let short=bin(BinOp::Lt,count.clone(),total);
        out.push(error(if let Some(n)=&pad {bin(BinOp::And,short,bin(BinOp::Eq,n.clone(),int(0)))}else{short},
            "RESHAPE requires enough SOURCE elements or a nonempty PAD".into()));
        let result=self.buffer(kind,&dims,out,s)?;
        let vars=self.loop_vars(&dims,out,s);
        let mut ordinal=int(0);
        let mut stride=int(1);
        for axis in order {
            ordinal=bin(BinOp::Add,ordinal,bin(BinOp::Mul,bin(BinOp::Sub,id(&vars[axis]),int(1)),stride.clone()));
            stride=bin(BinOp::Mul,stride,dims[axis].clone());
        }
        let index=bin(BinOp::Add,ordinal,int(1));
        let mut source_body=vec![];
        let value=self.reshape_element(&args[0],index.clone(),s,&mut source_body)?;
        let store=|value|Stmt::AssignIndex{name:result.clone(),indices:vars.iter().map(|n|id(n)).collect(),value};
        source_body.push(store(value));
        let body=if let Some(pad_count)=pad {
            let mut padding=vec![];
            let pi=bin(BinOp::Add,int(1),Expr::Call("modulo".into(),vec![bin(BinOp::Sub,bin(BinOp::Sub,index.clone(),count.clone()),int(1)),Expr::Call("max".into(),vec![pad_count,int(1)])]));
            let value=self.reshape_element(&args[2],pi,s,&mut padding)?;
            padding.push(store(value));
            vec![Stmt::If{cond:bin(BinOp::Le,index,count),then_body:source_body,else_body:Some(padding)}]
        }else{source_body};
        out.extend(self.nest(&vars,&dims,body));
        *e=id(&result);
        Ok(())
    }
    fn expression(&mut self, e: &mut Expr, s: &mut Shapes, out: &mut Vec<Stmt>) -> Result<()> {
        // ASSOCIATED inspects target identity, not a copied section value.
        if matches!(e,Expr::Call(n,_) if n=="associated") {return Ok(());}
        // Rank-one triplets use a dedicated AST node; use the same captured
        // selection machinery as multidimensional triplets when they vary.
        if let Expr::Section{name,..}=&*e {
            if !s.contains_key(&format!("@pointer:{name}")) && s.get(name).is_some_and(|(_,d)|d.len()==1) {
                let args=vec![Expr::Call("@section_subscript".into(),vec![e.clone()])];
                if dynamic(name,&args,s) {*e=Expr::Call(name.clone(),args);}
            }
        }
        if let Expr::Call(n, a) = e {
            if matches!(
                n.as_str(),
                "size" | "shape" | "lbound" | "ubound" | "kind" | "len"
            ) && a.first().is_some_and(|e| contains(e, s))
            {
                let (kind, d) = shape(&a[0], s)?.context("unknown vector inquiry shape")?;
                let dim = a
                    .get(1)
                    .map(|e| constant(e).context("array inquiry DIM must be constant"))
                    .transpose()?;
                if dim.is_some_and(|i| i < 1 || i as usize > d.len()) {
                    bail!("invalid array inquiry DIM");
                }
                *e = match n.as_str() {
                    "size" => Expr::Call(
                        "int".into(),
                        vec![dim
                            .map(|i| d[i as usize - 1].clone())
                            .unwrap_or_else(|| product(&d))],
                    ),
                    "len" => match &kind {
                        TypeSpec::Character(n) => int(n.unwrap_or(1) as i64),
                        TypeSpec::CharacterLength(e) => *e.clone(),
                        _ => bail!("LEN requires a CHARACTER argument"),
                    },
                    "kind" => int(match kind {
                        TypeSpec::Integer(k) => i64::from(k.unwrap_or(4)),
                        TypeSpec::DoublePrecision => 8,
                        TypeSpec::Character(_) => 1,
                        _ => 4,
                    }),
                    _ => {
                        let values = d
                            .iter()
                            .map(|nsize| {
                                if n == "lbound" {
                                    int(1)
                                } else {
                                    Expr::Call("int".into(), vec![nsize.clone()])
                                }
                            })
                            .collect::<Vec<_>>();
                        if let Some(i) = dim {
                            values[i as usize - 1].clone()
                        } else {
                            Expr::Call("@array_constructor".into(), values)
                        }
                    }
                };
                return Ok(());
            }
        }
        match e {
            Expr::Call(n, a) | Expr::Index(n, a)
                if dynamic(n, a, s) || self.force && selection(n, a, s) =>
            {
                let n = n.clone();
                let kind = s[&n].0.clone();
                let capacity = selection_capacity(&n, a, s);
                let (selected, dims) = self.indices(&n, a, s, out, false)?;
                let start=out.len();
                let result = self.buffer_capacity(kind, &dims, capacity, out, s)?;
                let mut allocation=out.split_off(start);
                self.preserve_character_length(&id(&n),&result,s,out);
                out.append(&mut allocation);
                let vars = self.loop_vars(&dims, out, s);
                out.extend(self.nest(
                    &vars,
                    &dims,
                    vec![Stmt::AssignIndex {
                        name: result.clone(),
                        indices: vars.iter().map(|n| id(n)).collect(),
                        value: Expr::Index(n, self.selected(&selected, &vars)),
                    }],
                ));
                *e = id(&result);
            }
            Expr::Call(_, a) | Expr::Index(_, a) => {
                for e in a {
                    self.expression(e, s, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, s, out)?;
                self.expression(b, s, out)?;
            }
            Expr::Un(_, a) => self.expression(a, s, out)?,
            _ => {}
        }
        self.unpack(e,s,out)?;
        self.shift(e,s,out)?;
        self.spread(e,s,out)?;
        self.transpose(e,s,out)?;
        self.reshape(e,s,out)?;
        // CHARACTER array operations must be scalarized while their element
        // types are still visible, before the final storage passes flatten them.
        if let Expr::Bin(op,a,b)=e {
            if matches!(op,BinOp::Concat|BinOp::Eq|BinOp::Ne|BinOp::Lt|BinOp::Le|BinOp::Gt|BinOp::Ge)
                && (runtime_character(a,s)||runtime_character(b,s)) {
                if let (Some((ak,ad)),Some((bk,bd)))=(shape(a,s)?,shape(b,s)?) {
                    if matches!(ak,TypeSpec::Character(_)) && matches!(bk,TypeSpec::Character(_)) && (!ad.is_empty()||!bd.is_empty())
                        && (runtime_character(a,s)||runtime_character(b,s)) {
                        if !ad.is_empty() && !bd.is_empty() {
                            if ad.len()!=bd.len() {bail!("CHARACTER array expression shapes do not conform");}
                            for (a,b) in ad.iter().zip(&bd) {out.push(error(bin(BinOp::Ne,a.clone(),b.clone()),"CHARACTER array expression shapes do not conform".into()));}
                        }
                        let dims=if ad.is_empty(){bd}else{ad};
                        let result_capacity=capacity(a,s).or_else(||capacity(b,s));
                        let kind=if matches!(op,BinOp::Concat){super::concatenation_kind(&ak,&bk)?}else{TypeSpec::Logical};
                        // Constructors and other non-storage array operands need
                        // storage before selecting one scalar value per coordinate.
                        for operand in [&mut **a,&mut **b] {
                            let (kind,shape)=shape(operand,s)?.context("unknown CHARACTER array operand")?;
                            if !shape.is_empty() && !matches!(operand,Expr::Ident(n) if s.get(n).is_some_and(|(_,d)|!d.is_empty())) {
                                let bound=capacity(operand,s);
                                let values=self.buffer_capacity(kind,&shape,bound,out,s)?;
                                out.push(assign(&values,operand.clone()));
                                *operand=id(&values);
                            }
                        }
                        self.freeze(a,s,out)?;self.freeze(b,s,out)?;
                        let temp=self.buffer_capacity(kind,&dims,result_capacity,out,s)?;
                        let vars=self.loop_vars(&dims,out,s);
                        let value=bin(op.clone(),Self::character_element(a,&vars,s)?,Self::character_element(b,&vars,s)?);
                        out.extend(self.nest(&vars,&dims,vec![Stmt::AssignIndex{name:temp.clone(),indices:vars.iter().map(|v|id(v)).collect(),value}]));
                        *e=id(&temp);
                    }
                }
            }
        }
        Ok(())
    }
    // Scalarize elemental expressions before array-expression materialization:
    // excluded WHERE elements must never be evaluated.
    fn where_element(&mut self, e:&Expr, index:Expr, s:&Shapes, out:&mut Vec<Stmt>)->Result<Expr> {
        match e {
            Expr::Bin(op,a,b)=>Ok(bin(op.clone(),self.where_element(a,index.clone(),s,out)?,self.where_element(b,index,s,out)?)),
            Expr::Un(op,a)=>Ok(Expr::Un(op.clone(),Box::new(self.where_element(a,index,s,out)?))),
            Expr::Call(n,a) if s.contains_key(&format!("@elemental_function:{n}"))
                || crate::intrinsics::math_arity(n).is_some() || crate::intrinsics::real_predicate(n)
                || matches!(n.as_str(),"abs"|"int"|"real"|"dble"|"mod"|"modulo"|"sign"|"min"|"max") => {
                Ok(Expr::Call(n.clone(),a.iter().map(|a|self.where_element(a,index.clone(),s,out)).collect::<Result<_>>()?))
            }
            _=>self.reshape_element(e,index,s,out),
        }
    }
    fn where_body(&mut self, body:&[Stmt], s:&mut Shapes, dims:&[Expr], parent:Option<&str>)->Result<Vec<Stmt>> {
        let mut out=vec![];
        let length=self.scalar("where_length",product(dims),&mut out,s);
        let index=self.loop_vars(&[length.clone()],&mut out,s).remove(0);
        let active=self.buffer(TypeSpec::Logical,&[length.clone()],&mut out,s)?;
        let remaining=self.buffer(TypeSpec::Logical,&[length.clone()],&mut out,s)?;
        let item=|n:&str|Expr::Index(n.into(),vec![id(&index)]);
        let set=|n:&str,value|Stmt::AssignIndex{name:n.into(),indices:vec![id(&index)],value};
        let loop_body=|body|Stmt::Do{var:index.clone(),start:int(1),end:length.clone(),body};
        let conditional=|cond,then_body|Stmt::If{cond,then_body,else_body:None};
        let mut first=true;
        for stmt in body {
            match stmt {
                Stmt::SourceLocation(_)=>out.push(stmt.clone()),
                Stmt::CallSub{name,args} if name=="@where" || name=="@elsewhere"=>{
                    let mut evaluate=vec![];
                    let value=if let Some(mask)=args.first() {
                        let (kind,md)=shape(mask,s)?.context("unknown WHERE mask shape")?;
                        if kind!=TypeSpec::Logical || md.is_empty() {bail!("WHERE mask must be a LOGICAL array");}
                        Self::where_conform(&md,dims,&mut out,"WHERE mask shapes do not conform")?;
                        self.where_element(mask,id(&index),s,&mut evaluate)?
                    }else{Expr::Logical(true)};
                    evaluate.push(set(&active,value));
                    let eligible=if first {parent.map(item)}else{Some(item(&remaining))};
                    let mut capture=if let Some(cond)=eligible.clone() {
                        vec![set(&active,Expr::Logical(false)),conditional(cond,evaluate)]
                    }else{evaluate};
                    let not_active=Expr::Un(crate::ast::UnOp::Not,Box::new(item(&active)));
                    capture.push(set(&remaining,eligible.map_or(not_active.clone(),|e|bin(BinOp::And,e,not_active))));
                    out.push(loop_body(capture));
                    first=false;
                }
                Stmt::Assign{name,value}=>{
                    let (kind,td)=shape(&id(name),s)?.context("WHERE destination must be a declared array")?;
                    if td.is_empty() {bail!("WHERE destination must be an array");}
                    if matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)|TypeSpec::CharacterAssumed) {
                        bail!("WHERE currently requires a whole numeric or LOGICAL array destination");
                    }
                    if s.contains_key(&format!("@readonly:{name}")) {bail!("cannot assign to INTENT(IN) or PARAMETER array `{name}`");}
                    Self::where_conform(&td,dims,&mut out,"WHERE destination shape does not conform to mask")?;
                    let (rhs_kind,rd)=shape(value,s)?.context("unknown WHERE assignment shape")?;
                    if !rd.is_empty() {Self::where_conform(&rd,dims,&mut out,"WHERE assignment shapes do not conform")?;}
                    if matches!(kind,TypeSpec::Logical)!=matches!(rhs_kind,TypeSpec::Logical) {bail!("WHERE assignment has incompatible numeric/LOGICAL types");}
                    let temp=self.buffer(kind,&[length.clone()],&mut out,s)?;
                    let mut evaluate=vec![];
                    let scalar=self.where_element(value,id(&index),s,&mut evaluate)?;
                    evaluate.push(set(&temp,scalar));
                    out.push(loop_body(vec![conditional(item(&active),evaluate)]));
                    let Expr::Index(target,indices)=self.reshape_element(&id(name),id(&index),s,&mut vec![])? else {bail!("WHERE destination must be an array");};
                    out.push(loop_body(vec![conditional(item(&active),vec![Stmt::AssignIndex{name:target,indices,value:item(&temp)}])]));
                }
                Stmt::Block{body}=>out.extend(self.where_body(body,s,dims,Some(&active))?),
                _=>bail!("WHERE currently supports whole-array assignments and nested WHERE; section destinations are not yet supported"),
            }
        }
        Ok(out)
    }
    fn where_conform(actual:&[Expr],expected:&[Expr],out:&mut Vec<Stmt>,message:&str)->Result<()> {
        if actual.len()!=expected.len() {bail!("{message}");}
        for (a,b) in actual.iter().zip(expected) {
            if let (Some(a),Some(b))=(constant(a),constant(b)) {if a!=b {bail!("{message}");}}
            else {out.push(error(bin(BinOp::Ne,a.clone(),b.clone()),message.into()));}
        }
        Ok(())
    }
    fn child(&mut self, body: &mut Vec<Stmt>, inherited: &Shapes, params: &[String]) -> Result<()> {
        if self.shallow { Ok(()) } else { self.body(body, inherited, params) }
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Shapes, params: &[String]) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut s = inherited.clone();
        for p in params {
            s.remove(p);
        }
        declarations(body, &mut s);
        // FORALL must capture all iterations before any writes. Leave its one
        // assignment intact until the FORALL pass expands that ordering.
        if body.iter().any(|stmt|matches!(stmt,Stmt::CallSub{name,..} if name=="@forall")) {
            return Ok(());
        }
        if params.iter().any(|p| {
            s.get(p)
                .is_some_and(|(_, d)| !d.is_empty() && axes(p, &s).is_err())
        }) {
            return Ok(());
        }
        if let Some(Stmt::CallSub{name,args})=body.iter().find(|stmt|!matches!(stmt,Stmt::SourceLocation(_))) {
            if name=="@where" {
                if let Some((_,dims))=args.first().map(|e|shape(e,&s)).transpose()?.flatten() {
                    if dims.iter().any(|d|constant(d).is_none()) {
                        fn ready(body:&[Stmt],s:&Shapes)->Result<bool> {
                            for stmt in body {
                                match stmt {
                                    Stmt::Assign{value,..} if shape(value,s)?.is_none()=>return Ok(false),
                                    Stmt::CallSub{name,args} if name=="@where" || name=="@elsewhere"=>{
                                        if let Some(e)=args.first() {if shape(e,s)?.is_none() {return Ok(false);}}
                                    }
                                    Stmt::Block{body} if !ready(body,s)?=>return Ok(false),
                                    _=>{}
                                }
                            }
                            Ok(true)
                        }
                        // Specialization may precede availability of function
                        // signatures. Preserve the construct for the next pass.
                        if !ready(body,&s)? {return Ok(());}
                        *body=self.where_body(body,&mut s,&dims,None)?;
                        return Ok(());
                    }
                }
            }
        }
        if body.iter().any(
            |stmt| matches!(stmt,Stmt::CallSub{name,..} if name=="@where" || name=="@elsewhere"),
        ) {
            // Fixed transformations inside WHERE are materialized by the WHERE
            // pass, which controls which assignments are subject to its mask.
            s.insert("@where_scope".into(),(TypeSpec::Logical,vec![]));
            for stmt in body.iter() {
                if match stmt {
                    Stmt::Assign { value, .. } => contains(value, &s),
                    Stmt::AssignIndex {
                        name,
                        indices,
                        value,
                    } => dynamic(name, indices, &s) || contains(value, &s),
                    _ => false,
                } {
                    bail!("vector selections in WHERE assignments are not yet supported");
                }
            }
        }
        let mut result = vec![];
        for mut stmt in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location.set(Some(p)); }
            let mut prefix = vec![];
            let mut suffix = vec![];
            if let Stmt::AssignIndex{name,indices,..}=&mut stmt {
                if !s.contains_key(&format!("@pointer:{name}")) && s.get(name).is_some_and(|(_,d)|d.len()==1) && indices.len()==1
                    && matches!(&indices[0],Expr::Section{..}) {
                    let wrapped=Expr::Call("@section_subscript".into(),vec![indices[0].clone()]);
                    if dynamic(name,std::slice::from_ref(&wrapped),&s) {indices[0]=wrapped;}
                }
            }
            match &mut stmt {
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if dynamic(name, indices, &s) => {
                    if s.contains_key(&format!("@readonly:{name}")) {bail!("cannot assign to a section of PARAMETER or INTENT(IN) array `{name}`");}
                    // Array-valued calls are materialized by the function pass
                    // between specialization rounds. Preserve the assignment
                    // until its RHS type/shape is available instead of guessing.
                    if shape(value,&s)?.is_none() {
                        // The result can be unknown while its section operands
                        // already have descriptors. Capture those operands so
                        // the reduction/function pass can resolve the result.
                        self.expression(value,&mut s,&mut prefix)?;
                        result.extend(prefix);
                        result.push(stmt);
                        continue;
                    }
                    let capacity = selection_capacity(name, indices, &s);
                    let (selected, dims) =
                        self.indices(name, indices, &mut s, &mut prefix, true)?;
                    self.expression(value, &mut s, &mut prefix)?;
                    let (kind, rhs) =
                        shape(value, &s)?.context("unknown vector assignment RHS type")?;
                    if !rhs.is_empty() && rhs.len() != dims.len() {
                        bail!("vector-subscript assignment shapes do not conform");
                    }
                    for (a, b) in rhs.iter().zip(&dims) {
                        prefix.push(error(
                            bin(BinOp::Ne, a.clone(), b.clone()),
                            "vector-subscript assignment shapes do not conform".into(),
                        ));
                    }
                    let vars = self.loop_vars(&dims, &mut prefix, &mut s);
                    let value = if rhs.is_empty() {
                        self.serial += 1;
                        let temp = format!("@vector_runtime_scalar_{}", self.serial);
                        prefix.push(Stmt::VarDecl {
                            kind,
                            names: vec![temp.clone()],
                        });
                        prefix.push(assign(&temp, value.clone()));
                        id(&temp)
                    } else {
                        let temp =
                            self.buffer_capacity(kind, &dims, capacity, &mut prefix, &mut s)?;
                        prefix.push(assign(&temp, value.clone()));
                        Expr::Index(temp, vars.iter().map(|n| id(n)).collect())
                    };
                    prefix.extend(self.nest(
                        &vars,
                        &dims,
                        vec![Stmt::AssignIndex {
                            name: name.clone(),
                            indices: self.selected(&selected, &vars),
                            value,
                        }],
                    ));
                    result.extend(prefix);
                    continue;
                }
                Stmt::Assign { value, .. } | Stmt::AssignIndex { value, .. } => {
                    self.expression(value, &mut s, &mut prefix)?
                }
                Stmt::Print { items } => {
                    for e in items {
                        self.expression(e, &mut s, &mut prefix)?;
                    }
                }
                // Association must retain the original TARGET and its aliasing.
                Stmt::CallSub { name, .. } if matches!(name.as_str(),"@pointer_associate"|"@c_desc_bind"|"@c_array_bind"|"@c_f_pointer") => {},
                Stmt::CallSub { name, args } => {
                    for (i, e) in args.iter().enumerate() {
                        if contains_vector(e, &s) {
                            super::validate_actual(name, i, &self.procedures)?;
                        }
                    }
                    if name=="@internal_read" {
                        for item in &mut args[3..] {
                            if let Expr::Call(n,a)|Expr::Index(n,a)=item {
                                if selection(n,a,&s) && !vector_selection(n,a,&s) {
                                    if s.contains_key(&format!("@readonly:{n}")) {bail!("cannot READ into PARAMETER or INTENT(IN) array `{n}`");}
                                    let (selected,dims)=self.indices(n,a,&mut s,&mut prefix,true)?;
                                    let vars=self.loop_vars(&dims,&mut prefix,&mut s);
                                    let mut value=Expr::Index(n.clone(),self.selected(&selected,&vars));
                                    for (var,dim) in vars.iter().zip(&dims) {
                                        value=Expr::Call("@input_implied_do".into(),vec![id(var),int(1),dim.clone(),int(1),value]);
                                    }
                                    *item=value;
                                }
                            }
                        }
                        result.extend(prefix);result.push(stmt);continue;
                    }
                    if crate::input::transfer_start(name)
                        .is_some_and(|start|args.iter().skip(start).any(|e|contains(e,&s)))
                    {
                        bail!("READ into vector-subscript selections is not yet supported");
                    }
                    for (i,e) in args.iter_mut().enumerate() {
                        let intent=self.procedures.get(name).and_then(|v|v.get(i)).cloned()
                            .or_else(||(name=="random_number" && i==0).then(||"intent(out)".into()));
                        if let Some(intent)=intent.as_deref().and_then(|i|i.strip_prefix("@c_descriptor:")) {
                            let root=match &*e {Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)=>Some(n),Expr::Section{name,..}=>Some(name),_=>None};
                            if let Some(n)=root.filter(|n|s.get(*n).is_some_and(|(_,d)|!d.is_empty())) {
                                if !contains_vector(e,&s) {
                                    if intent!="intent(in)" && s.contains_key(&format!("@readonly:{n}")) {bail!("cannot pass a section of PARAMETER or INTENT(IN) array `{n}` as a writable C argument");}
                                    continue;
                                }
                            }
                        }
                        if intent.as_deref().is_some_and(|i|i!="intent(in)") {
                            // Rank-one triplets use Section rather than Call
                            // in the AST. Retain writable storage and copy back
                            // through the same path as higher-rank sections.
                            if matches!(e,Expr::Section{name,..} if s.get(name).is_some_and(|(_,d)|d.len()==1)) {
                                let Expr::Section{name,..}=&*e else {unreachable!()};
                                *e=Expr::Call(name.clone(),vec![Expr::Call("@section_subscript".into(),vec![e.clone()])]);
                            }
                            if let Expr::Call(n,a) | Expr::Index(n,a) = e {
                                if selection(n,a,&s) && !vector_selection(n,a,&s) {
                                    if s.contains_key(&format!("@readonly:{n}")) {bail!("cannot pass a section of PARAMETER or INTENT(IN) array `{n}` as a writable argument");}
                                    let kind=s[n].0.clone();
                                    let capacity=selection_capacity(n,a,&s);
                                    let (selected,dims)=self.indices(n,a,&mut s,&mut prefix,true)?;
                                    let temp=self.buffer_capacity(kind,&dims,capacity,&mut prefix,&mut s)?;
                                    let vars=self.loop_vars(&dims,&mut prefix,&mut s);
                                    let source=Expr::Index(n.clone(),self.selected(&selected,&vars));
                                    let target=Expr::Index(temp.clone(),vars.iter().map(|v|id(v)).collect());
                                    if intent.as_deref()!=Some("intent(out)") {
                                        prefix.extend(self.nest(&vars,&dims,vec![Stmt::AssignIndex{name:temp.clone(),indices:vars.iter().map(|v|id(v)).collect(),value:source}]));
                                    }
                                    suffix.extend(self.nest(&vars,&dims,vec![Stmt::AssignIndex{name:n.clone(),indices:self.selected(&selected,&vars),value:target}]));
                                    *e=id(&temp);
                                    continue;
                                }
                            }
                        }
                        self.expression(e, &mut s, &mut prefix)?;
                    }
                }
                Stmt::Read { args } => {
                    if args.iter().any(|e| contains(e, &s)) {
                        bail!("READ into vector-subscript selections is not yet supported");
                    }
                }
                Stmt::Return(Some(e)) => self.expression(e, &mut s, &mut prefix)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &mut s, &mut prefix)?;
                    self.child(then_body, &s, &[])?;
                    if let Some(b) = else_body {
                        self.child(b, &s, &[])?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &mut s, &mut prefix)?;
                    self.expression(end, &mut s, &mut prefix)?;
                    self.child(body, &s, &[])?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &mut s, &mut prefix)?;
                    self.child(body, &s, &[])?;
                    if !prefix.is_empty() {
                        let (decls, mut evaluate): (Vec<_>, Vec<_>) =
                            prefix.into_iter().partition(|s| {
                                matches!(
                                    s,
                                    Stmt::VarDecl { .. }
                                        | Stmt::ArrayDecl { .. }
                                        | Stmt::DummyIntent { .. }
                                )
                            });
                        prefix = decls;
                        evaluate.push(Stmt::If {
                            cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        evaluate.append(body);
                        *body = evaluate;
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Block { body } | Stmt::Module { body, .. } => self.child(body, &s, &[])?,
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    self.child(body, &s, params)?
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &mut s, &mut prefix)?;
                    for c in cases {
                        self.child(&mut c.body, &s, &[])?;
                    }
                    if let Some(b) = default {
                        self.child(b, &s, &[])?;
                    }
                }
                _ => {}
            }
            declarations(&prefix, &mut s);
            result.extend(prefix);
            result.push(stmt);
            result.extend(suffix);
        }
        *body = result;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
pub(super) fn specialize(body:&mut Vec<Stmt>, shapes:&Shapes, procedures:std::sync::Arc<HashMap<String,Vec<String>>>, serial:&mut usize)->Result<()> {
    let inherited_serial=shapes.keys().filter(|n|n.contains("@vector_")).filter_map(|n|n.rsplit('_').next()?.parse::<usize>().ok()).max().unwrap_or(0);
    // array_calls::expand visits child scopes itself. Revisiting the entire
    // subtree at each ancestor makes specialization quadratic in nesting depth.
    let mut lower=Lower{serial:(*serial).max(inherited_serial).max(super::serial_start(body)),procedures,force:false,shallow:true};
    lower.body(body,shapes,&[])?;
    *serial=lower.serial;
    Ok(())
}
pub(super) fn specialize_expression(e:&mut Expr, shapes:&mut Shapes, prefix:&mut Vec<Stmt>, serial:&mut usize)->Result<()> {
    let inherited_serial=shapes.keys().filter(|n|n.contains("@vector_")).filter_map(|n|n.rsplit('_').next()?.parse::<usize>().ok()).max().unwrap_or(0);
    let mut lower=Lower{serial:(*serial).max(inherited_serial),procedures:std::sync::Arc::default(),force:false,shallow:false};
    lower.expression(e,shapes,prefix)?;
    *serial=lower.serial;
    Ok(())
}

/// Query the full shape before a compound actual is flattened by reductions.
pub(super) fn argument_expression_shape(e:&Expr, shapes:&Shapes, functions:&HashMap<String,TypeSpec>)->Result<Option<(TypeSpec,Vec<Expr>)>> {
    let mut symbols=shapes.clone();
    for (name,kind) in functions {symbols.insert(format!("@elemental_function:{name}"),(kind.clone(),vec![]));}
    shape(e,&symbols)
}

/// Reuse Cartesian section indexing and owned expression buffers for an
/// elemental function with writable actuals. Inputs are bound before outputs
/// are defined, and each scalar invocation writes its own result/status.
pub(super) fn elemental_function(
    e: &mut Expr,
    s: &mut Shapes,
    out: &mut Vec<Stmt>,
    serial: &mut usize,
    routine: (&[String], &[Stmt], &str, &TypeSpec),
) -> Result<bool> {
    let (params, body, result, kind) = routine;
    let Expr::Call(_, args) = e else {
        return Ok(false);
    };
    let info = args
        .iter()
        .map(|a| shape(a, s)?.context("unknown elemental argument shape"))
        .collect::<Result<Vec<_>>>()?;
    let Some((_, dims)) = info.iter().find(|(_, d)| !d.is_empty()) else {
        return Ok(false);
    };
    let inherited_serial = s
        .keys()
        .filter(|n| n.contains("@vector_"))
        .filter_map(|n| n.rsplit('_').next()?.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    let mut lower = Lower {
        serial: (*serial).max(inherited_serial),
        procedures: std::sync::Arc::default(),
        force: false,
        shallow: false,
    };
    let dims = dims
        .iter()
        .map(|d| lower.scalar("elemental_extent", d.clone(), out, s))
        .collect::<Vec<_>>();
    for (_, actual) in &info {
        if actual.is_empty() {
            continue;
        }
        if actual.len() != dims.len() {
            bail!("nonconforming elemental array argument shapes");
        }
        for (a, b) in actual.iter().zip(&dims) {
            if let (Some(a), Some(b)) = (constant(a), constant(b)) {
                if a != b {
                    bail!("nonconforming elemental array argument shapes");
                }
            } else {
                out.push(error(bin(BinOp::Ne,a.clone(),b.clone()),"@enumeration_runtime:nonconforming elemental array argument shapes in NEXT/PREVIOUS".into()));
            }
        }
    }
    let vars = lower.loop_vars(&dims, out, s);
    let mut names = HashMap::new();
    let mut invocation = vec![];
    let mut stores = vec![];
    for ((param, arg), (actual_kind, actual_dims)) in params.iter().zip(args.iter_mut()).zip(&info)
    {
        let writable=body.iter().any(|stmt|matches!(stmt,Stmt::DummyIntent{name,intent} if name==param && matches!(intent.as_str(),"intent(out)"|"intent(inout)")));
        let expected = body
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::VarDecl { kind, names } if names.contains(param) => Some(kind),
                _ => None,
            })
            .context("elemental scalar dummy requires a declared type")?;
        if !crate::pointers::same_kind(expected, actual_kind) {
            bail!("type mismatch for elemental function dummy `{param}`");
        }
        if writable && actual_dims.is_empty() {
            bail!("ELEMENTAL writable dummy `{param}` requires an array actual when any argument is an array");
        }
        let value = if actual_dims.is_empty() {
            lower.freeze(arg, s, out)?;
            arg.clone()
        } else {
            // An OUT selection retains its original storage; it must never
            // be materialized by reading its currently undefined elements.
            if writable {
                let n = match &*arg {
                    Expr::Ident(n)
                    | Expr::Section { name: n, .. }
                    | Expr::Call(n, _)
                    | Expr::Index(n, _) => n,
                    _ => bail!(
                        "writable elemental function dummy `{param}` requires a variable actual"
                    ),
                };
                if s.contains_key(&format!("@readonly:{n}"))
                    || s.contains_key(&format!("@function_readonly:{n}"))
                {
                    bail!("cannot pass PARAMETER or INTENT(IN) variable as writable argument `{param}`");
                }
            }
            let (array, selected) = match &mut *arg {
                Expr::Call(tag, values) if tag == "@packed_view" && values.len() == 2 => {
                    let Expr::Ident(n) = &values[0] else {
                        bail!("writable array view requires named storage")
                    };
                    let selected = axes(n, s)?
                        .into_iter()
                        .map(|(lo, _)| {
                            Axis::Triplet(lower.scalar("elemental_lower", lo, out, s), int(1))
                        })
                        .collect();
                    (n.clone(), selected)
                }
                Expr::Ident(n) => {
                    let selected = axes(n, s)?
                        .into_iter()
                        .map(|(lo, _)| {
                            Axis::Triplet(lower.scalar("elemental_lower", lo, out, s), int(1))
                        })
                        .collect();
                    (n.clone(), selected)
                }
                Expr::Section { name, .. } => {
                    let n = name.clone();
                    let mut subscripts =
                        vec![Expr::Call("@section_subscript".into(), vec![arg.clone()])];
                    let (selected, _) = lower.indices(&n, &mut subscripts, s, out, writable)?;
                    (n, selected)
                }
                Expr::Call(n, a) | Expr::Index(n, a)
                    if s.get(n).is_some_and(|(_, d)| !d.is_empty()) =>
                {
                    let n = n.clone();
                    let (selected, _) = lower.indices(&n, a, s, out, writable)?;
                    (n, selected)
                }
                _ if !writable => {
                    let n = lower.buffer(actual_kind.clone(), actual_dims, out, s)?;
                    out.push(assign(&n, arg.clone()));
                    (
                        n,
                        actual_dims
                            .iter()
                            .map(|_| Axis::Triplet(int(1), int(1)))
                            .collect(),
                    )
                }
                _ => {
                    bail!("writable elemental function dummy `{param}` requires an array variable")
                }
            };
            if let Some(flag) = allocatables::allocated(&array, s) {
                out.push(error(
                    Expr::Un(crate::ast::UnOp::Not, Box::new(flag)),
                    format!("NEXT/PREVIOUS argument `{param}` is not allocated"),
                ));
            }
            Expr::Index(array, lower.selected(&selected, &vars))
        };
        lower.serial += 1;
        let temp = format!("@vector_elemental_argument_{}", lower.serial);
        invocation.push(Stmt::VarDecl {
            kind: expected.clone(),
            names: vec![temp.clone()],
        });
        if writable {
            let Expr::Index(array, indices) = value else {
                bail!("elemental output must identify an array element")
            };
            stores.push(Stmt::AssignIndex {
                name: array,
                indices,
                value: id(&temp),
            });
        } else {
            invocation.push(assign(&temp, value));
        }
        names.insert(param.clone(), temp);
    }
    let output = lower.buffer(kind.clone(), &dims, out, s)?;
    lower.serial += 1;
    let scalar_result = format!("@vector_elemental_result_{}", lower.serial);
    invocation.push(Stmt::VarDecl {
        kind: kind.clone(),
        names: vec![scalar_result.clone()],
    });
    names.insert(result.into(), scalar_result.clone());
    let mut implementation = body.to_vec();
    if matches!(implementation.last(),Some(Stmt::Return(Some(Expr::Ident(n)))) if n==result) {
        implementation.pop();
    }
    let mut bound = params.to_vec();
    bound.push(result.into());
    crate::subroutines::rename_with_prefix(
        &mut implementation,
        &names,
        &bound,
        &mut lower.serial,
        "@vector_elemental_local",
    )?;
    implementation.retain(|stmt| {
        !matches!(stmt, Stmt::DummyIntent { .. } | Stmt::Implicit(_))
            && !matches!(stmt,Stmt::VarDecl{names,..} if names.is_empty())
    });
    invocation.extend(implementation);
    invocation.extend(stores);
    invocation.push(Stmt::AssignIndex {
        name: output.clone(),
        indices: vars.iter().map(|n| id(n)).collect(),
        value: id(&scalar_result),
    });
    out.extend(lower.nest(&vars, &dims, vec![Stmt::Block { body: invocation }]));
    *e = id(&output);
    *serial = lower.serial;
    Ok(true)
}

fn heap_buffer(
    kind: TypeSpec,
    n: String,
    dims: &[Expr],
    out: &mut Vec<Stmt>,
    s: &mut Shapes,
) -> Result<()> {
    crate::heap::unallocated_result(kind, n.clone(), dims.len(), out, s)?;
    out.push(Stmt::CallSub {
        name: "@allocate".into(),
        args: vec![Expr::Call(n, dims.to_vec())],
    });
    declarations(out, s);
    Ok(())
}

// Specification/inquiry expressions and ordinary selections use the same
// helpers, including when only the section's shape (not its values) is needed.
fn section_helpers(body: &mut Vec<Stmt>) {
    fn expression(e:&Expr)->bool {
        match e {
            Expr::Call(n,a)|Expr::Index(n,a)=>n=="@runtime_section_first" || n=="@runtime_section_extent" || a.iter().any(expression),
            Expr::Bin(_,a,b)=>expression(a)||expression(b),Expr::Un(_,a)=>expression(a),
            Expr::Section{lower,upper,stride,..}=>[lower,upper,stride].into_iter().flatten().any(|e|expression(e)),_=>false,
        }
    }
    fn needed(body:&[Stmt])->bool {
        body.iter().any(|s|match s {
            Stmt::Assign{value,..}|Stmt::Return(Some(value))=>expression(value),
            Stmt::AssignIndex{indices,value,..}=>indices.iter().any(expression)||expression(value),
            Stmt::Print{items}|Stmt::CallSub{args:items,..}|Stmt::Read{args:items}=>items.iter().any(expression),
            Stmt::ArrayDecl{dims,..}=>dims.iter().any(expression),
            Stmt::If{cond,then_body,else_body}=>expression(cond)||needed(then_body)||else_body.as_ref().is_some_and(|b|needed(b)),
            Stmt::Do{start,end,body,..}=>expression(start)||expression(end)||needed(body),
            Stmt::DoWhile{cond,body}=>expression(cond)||needed(body),
            Stmt::Block{body}|Stmt::Module{body,..}|Stmt::Function{body,..}|Stmt::Subroutine{body,..}=>needed(body),
            Stmt::SelectCase{expr,cases,default}=>expression(expr)||cases.iter().any(|c|needed(&c.body))||default.as_ref().is_some_and(|b|needed(b)),_=>false,
        })
    }
    if !needed(body) || body.iter().any(|s|matches!(s,Stmt::Function{name,..} if name=="@runtime_section_extent")) {return;}
    let params:Vec<String>=vec!["@section_first_arg".into(),"@section_last_arg".into(),"@section_step_arg".into()];
    let first=id(&params[0]);let last=id(&params[1]);let step=id(&params[2]);
    for name in ["@runtime_section_first","@runtime_section_extent"] {
        let mut statements=vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:params.clone()}];
        statements.extend(params.iter().map(|n|Stmt::DummyIntent{name:n.clone(),intent:"intent(in)".into()}));
        statements.push(error(bin(BinOp::Eq,step.clone(),int(0)),"array section stride must not be zero".into()));
        if name=="@runtime_section_first" {
            statements.push(Stmt::If{cond:bin(BinOp::Gt,step.clone(),int(0)),then_body:vec![Stmt::Return(Some(first.clone()))],else_body:None});
            statements.push(Stmt::Return(Some(last.clone())));
        } else {
            let empty=bin(BinOp::Or,bin(BinOp::And,bin(BinOp::Gt,step.clone(),int(0)),bin(BinOp::Lt,last.clone(),first.clone())),bin(BinOp::And,bin(BinOp::Lt,step.clone(),int(0)),bin(BinOp::Gt,last.clone(),first.clone())));
            statements.push(Stmt::If{cond:empty,then_body:vec![Stmt::Return(Some(int(0)))],else_body:None});
            statements.push(Stmt::Return(Some(bin(BinOp::Add,bin(BinOp::Div,bin(BinOp::Sub,last.clone(),first.clone()),step.clone()),int(1)))));
        }
        body.push(Stmt::Function{name:name.into(),params:params.clone(),return_type:Some(TypeSpec::Integer(Some(8))),pure:true,elemental:false,body:statements});
    }
}
pub(super) fn resolve(
    mut p: Program,
    procedures: &HashMap<String, Vec<String>>,
) -> Result<Program> {
    fn signatures(body: &[Stmt], symbols: &mut Shapes) {
        for stmt in body {
            match stmt {
                Stmt::Function {
                    name,
                    return_type,
                    body,
                    elemental,
                    ..
                } => {
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                        _ => name,
                    };
                    let kind = return_type.clone().or_else(|| {
                        body.iter().find_map(|s| {
                            if let Stmt::VarDecl { kind, names } = s {
                                names.contains(result).then(|| kind.clone())
                            } else {
                                None
                            }
                        })
                    });
                    if body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==result)) {
                        symbols.insert(format!("@array_function:{name}"),(TypeSpec::Logical,vec![]));
                    }
                    if let Some(kind) = kind {
                        if *elemental {symbols.insert(format!("@elemental_function:{name}"),(kind.clone(),vec![]));}
                        symbols.insert(format!("@function:{name}"), (kind, vec![]));
                    }
                    signatures(body, symbols);
                }
                Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => {
                    signatures(body, symbols)
                }
                _ => {}
            }
        }
    }
    let mut symbols = Shapes::new();
    signatures(&p.body, &mut symbols);
    Lower {
        serial: super::serial_start(&p.body),
        procedures: std::sync::Arc::new(procedures.clone()),
        force: false,
        shallow: false,
    }
    .body(&mut p.body, &symbols, &[])?;
    section_helpers(&mut p.body);
    Ok(p)
}

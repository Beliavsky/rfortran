// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Bounded scalar storage for fixed-size rank-one CHARACTER arrays.
use crate::ast::{BinOp, CaseBlock, CaseItem, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone)]
struct Array {
    length: usize,
    elements: Vec<String>,
    extent: Option<Expr>,
}
type Arrays = HashMap<String, Array>;
pub fn original_element(name: &str) -> Option<(&str, i64)> {
    let (serial, rest) = name.strip_prefix("@character_array_")?.split_once('_')?;
    serial.parse::<usize>().ok()?;
    let (name, index) = rest.rsplit_once('_')?;
    Some((name, index.parse().ok()?))
}
#[derive(Clone)]
enum Value {
    Scalar(Expr),
    Array(Vec<Expr>),
}
impl Value {
    fn expr(self) -> Expr {
        match self {
            Self::Scalar(e) => e,
            Self::Array(v) => Expr::Call("@array_constructor".into(), v),
        }
    }
}
fn integer(e: &Expr) -> Result<i64> {
    crate::parameters::integer_constant(e)
        .context("CHARACTER array indices and bounds must currently be constant integers")
}
fn select(name: &str, section: Option<&Expr>, arrays: &Arrays) -> Result<Vec<String>> {
    let a = &arrays[name];
    let Some(section) = section else {
        return Ok(a.elements.clone());
    };
    let n = a.elements.len() as i64;
    if let Expr::Section {
        lower,
        upper,
        stride,
        ..
    } = section
    {
        let step = stride.as_deref().map(integer).transpose()?.unwrap_or(1);
        if step == 0 {
            bail!("CHARACTER array section stride must not be zero");
        }
        let start = lower.as_deref().map(integer).transpose()?.unwrap_or(1);
        let end = upper.as_deref().map(integer).transpose()?.unwrap_or(n);
        let mut result = Vec::new();
        let mut index = i128::from(start);
        while if step > 0 {
            index <= i128::from(end)
        } else {
            index >= i128::from(end)
        } {
            if index < 1 || index > i128::from(n) {
                bail!("CHARACTER array `{name}` section index {index} is outside 1:{n}");
            }
            result.push(a.elements[index as usize - 1].clone());
            index += i128::from(step);
        }
        Ok(result)
    } else {
        let i = integer(section)?;
        if i < 1 || i > n {
            bail!("CHARACTER array `{name}` index {i} is outside 1:{n}");
        }
        Ok(vec![a.elements[i as usize - 1].clone()])
    }
}
fn expression(e: Expr, arrays: &Arrays) -> Result<Value> {
    Ok(match e {
        Expr::Ident(n) if arrays.contains_key(&n) => Value::Array(
            select(&n, None, arrays)?
                .into_iter()
                .map(Expr::Ident)
                .collect(),
        ),
        e @ Expr::Section { .. } => {
            let Expr::Section { name, .. } = &e else {
                unreachable!()
            };
            if arrays.contains_key(name) {
                Value::Array(
                    select(name, Some(&e), arrays)?
                        .into_iter()
                        .map(Expr::Ident)
                        .collect(),
                )
            } else {
                Value::Scalar(e)
            }
        }
        Expr::Call(n, args) | Expr::Index(n, args) if arrays.contains_key(&n) => {
            let [index] = args.as_slice() else {
                bail!("CHARACTER arrays currently require one subscript");
            };
            Value::Scalar(Expr::Ident(select(&n, Some(index), arrays)?[0].clone()))
        }
        Expr::Call(n, args)
            if matches!(n.as_str(), "size" | "len")
                && args.len() == 1
                && matches!(&args[0],Expr::Ident(a) if arrays.contains_key(a)) =>
        {
            let Expr::Ident(a) = &args[0] else {
                unreachable!()
            };
            Value::Scalar(Expr::IntLit(
                if n == "len" {
                    arrays[a].length
                } else {
                    arrays[a].elements.len()
                }
                .to_string(),
            ))
        }
        Expr::Call(n, args) if n == "@array_substring" => {
            let [base, range] = args.as_slice() else {
                unreachable!()
            };
            let slice = |e: Expr| -> Result<Expr> {
                let Expr::Ident(name) = e else {
                    bail!("array substring requires CHARACTER elements");
                };
                let Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } = range
                else {
                    bail!("array substring requires a range");
                };
                Ok(Expr::Section {
                    name,
                    lower: lower.clone(),
                    upper: upper.clone(),
                    stride: stride.clone(),
                })
            };
            match expression(base.clone(), arrays)? {
                Value::Scalar(e) => Value::Scalar(slice(e)?),
                Value::Array(v) => Value::Array(v.into_iter().map(slice).collect::<Result<_>>()?),
            }
        }
        Expr::Call(n, args)
            if n == "@array_constructor"
                && !args.is_empty()
                && args.iter().all(|e| matches!(e, Expr::Str(_))) =>
        {
            let lengths: Vec<_> = args
                .iter()
                .filter_map(|e| {
                    if let Expr::Str(s) = e {
                        Some(s.len())
                    } else {
                        None
                    }
                })
                .collect();
            if lengths
                .first()
                .is_some_and(|first| lengths.iter().any(|n| n != first))
            {
                bail!("CHARACTER array constructor elements must have equal lengths");
            }
            Value::Array(args)
        }
        Expr::Call(n, args) => {
            fn logical(e: &Expr) -> bool {
                match e {
                    Expr::Logical(_) => true,
                    Expr::Bin(
                        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge,
                        _,
                        _,
                    ) => true,
                    Expr::Bin(BinOp::And | BinOp::Or | BinOp::Eqv | BinOp::Neqv, a, b) => {
                        logical(a) && logical(b)
                    }
                    Expr::Un(crate::ast::UnOp::Not, e) => logical(e),
                    _ => false,
                }
            }
            let logical_mask = args.first().is_some_and(logical);
            let values = args
                .into_iter()
                .map(|e| expression(e, arrays))
                .collect::<Result<Vec<_>>>()?;
            if matches!(
                n.as_str(),
                "all" | "any" | "@all_reduction" | "@any_reduction"
            ) && values.len() == 1
            {
                if let Value::Array(items) = &values[0] {
                    if !logical_mask {
                        bail!("ALL/ANY requires a LOGICAL array mask");
                    }
                    let all = matches!(n.as_str(), "all" | "@all_reduction");
                    let mut level = items.clone();
                    while level.len() > 1 {
                        level = level
                            .chunks(2)
                            .map(|pair| {
                                if pair.len() == 1 {
                                    pair[0].clone()
                                } else {
                                    Expr::Bin(
                                        if all { BinOp::And } else { BinOp::Or },
                                        Box::new(pair[0].clone()),
                                        Box::new(pair[1].clone()),
                                    )
                                }
                            })
                            .collect();
                    }
                    Value::Scalar(level.pop().unwrap_or(Expr::Logical(all)))
                } else {
                    Value::Scalar(Expr::Call(n, values.into_iter().map(Value::expr).collect()))
                }
            } else {
                if let Some(count)=values.iter().find_map(|v|if let Value::Array(v)=v {Some(v.len())} else {None}) {
                    if !matches!(n.as_str(),"index"|"scan"|"verify"|"len_trim"|"ichar"|"iachar"|"adjustl"|"adjustr") {
                        bail!("CHARACTER array arguments to `{n}` are not yet supported");
                    }
                    if values.iter().any(|v|matches!(v,Value::Array(v) if v.len()!=count)) {bail!("nonconforming CHARACTER intrinsic arguments to `{n}`");}
                    Value::Array((0..count).map(|i|Expr::Call(n.clone(),values.iter().map(|v|match v {
                        Value::Array(v)=>v[i].clone(),Value::Scalar(e)=>e.clone(),
                    }).collect())).collect())
                } else {
                    Value::Scalar(Expr::Call(n, values.into_iter().map(Value::expr).collect()))
                }
            }
        }
        Expr::Bin(op, a, b) => {
            let a = expression(*a, arrays)?;
            let b = expression(*b, arrays)?;
            let bin = |a, b| Expr::Bin(op.clone(), Box::new(a), Box::new(b));
            match (a, b) {
                (Value::Scalar(a), Value::Scalar(b)) => Value::Scalar(bin(a, b)),
                (Value::Array(a), Value::Scalar(b)) => {
                    Value::Array(a.into_iter().map(|a| bin(a, b.clone())).collect())
                }
                (Value::Scalar(a), Value::Array(b)) => {
                    Value::Array(b.into_iter().map(|b| bin(a.clone(), b)).collect())
                }
                (Value::Array(a), Value::Array(b)) => {
                    if a.len() != b.len() {
                        bail!("nonconforming CHARACTER array expression");
                    }
                    Value::Array(a.into_iter().zip(b).map(|(a, b)| bin(a, b)).collect())
                }
            }
        }
        Expr::Un(op, e) => match expression(*e, arrays)? {
            Value::Scalar(e) => Value::Scalar(Expr::Un(op, Box::new(e))),
            Value::Array(v) => Value::Array(
                v.into_iter()
                    .map(|e| Expr::Un(op.clone(), Box::new(e)))
                    .collect(),
            ),
        },
        other => Value::Scalar(other),
    })
}
fn assign(
    targets: Vec<String>,
    length: usize,
    range: Option<Expr>,
    value: Expr,
    arrays: &Arrays,
    serial: &mut usize,
    output: &mut Vec<Stmt>,
) -> Result<()> {
    let length = if let Some(range) = &range {
        crate::substrings::bounds(range, length)?.1
    } else {
        length
    };
    let values = match expression(value, arrays)? {
        Value::Array(v) => {
            if v.len() != targets.len() {
                bail!("nonconforming CHARACTER array assignment");
            }
            v
        }
        Value::Scalar(e) => {
            if targets.is_empty() {
                Vec::new()
            } else {
                *serial += 1;
                let temp = format!("@character_array_fill_{serial}");
                output.push(Stmt::VarDecl {
                    kind: TypeSpec::Character(Some(length)),
                    names: vec![temp.clone()],
                });
                output.push(Stmt::Assign {
                    name: temp.clone(),
                    value: e,
                });
                vec![Expr::Ident(temp); targets.len()]
            }
        }
    };
    let mut writes = Vec::new();
    for (target, value) in targets.into_iter().zip(values) {
        *serial += 1;
        let temp = format!("@character_array_value_{serial}");
        output.push(Stmt::VarDecl {
            kind: TypeSpec::Character(Some(length)),
            names: vec![temp.clone()],
        });
        output.push(Stmt::Assign {
            name: temp.clone(),
            value,
        });
        writes.push(if let Some(range) = &range {
            Stmt::AssignIndex {
                name: target,
                indices: vec![range.clone()],
                value: Expr::Ident(temp),
            }
        } else {
            Stmt::Assign {
                name: target,
                value: Expr::Ident(temp),
            }
        });
    }
    output.extend(writes);
    Ok(())
}
fn capture_index(name: &str, index: Expr, arrays: &Arrays, serial: &mut usize,
                 output: &mut Vec<Stmt>) -> Expr {
    *serial += 1;
    let temp = format!("@character_index_{serial}");
    output.push(Stmt::VarDecl {kind:TypeSpec::Integer(Some(8)),names:vec![temp.clone()]});
    output.push(Stmt::Assign {name:temp.clone(),value:index});
    let index=Expr::Ident(temp);
    if let Some(extent)=&arrays[name].extent {
        output.push(Stmt::If {
            cond:Expr::Bin(BinOp::Or,
                Box::new(Expr::Bin(BinOp::Lt,Box::new(index.clone()),Box::new(Expr::IntLit("1".into())))),
                Box::new(Expr::Bin(BinOp::Gt,Box::new(index.clone()),Box::new(extent.clone())))),
            then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("CHARACTER array `{name}` index outside allocated extent"))]}],else_body:None});
    }
    index
}
fn dispatch(name: &str, index: Expr, arrays: &Arrays, action: impl Fn(String)->Stmt) -> Stmt {
    Stmt::SelectCase {expr:index,cases:arrays[name].elements.iter().enumerate().map(|(i,n)|CaseBlock {
        items:vec![CaseItem::Single(Expr::IntLit((i+1).to_string()))],body:vec![action(n.clone())]
    }).collect(),default:Some(vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("CHARACTER array `{name}` index out of bounds"))]}])}
}
// Dispatch at runtime to exactly one scalar slot, preserving initialization checks.
fn dynamic(mut e: Expr, arrays: &Arrays, serial: &mut usize, output: &mut Vec<Stmt>) -> Result<Expr> {
    match &mut e {
        Expr::Call(n,a) | Expr::Index(n,a) if arrays.contains_key(n) && a.len()==1 => {
            let index=dynamic(a[0].clone(),arrays,serial,output)?;
            if crate::parameters::integer_constant(&index).is_ok() && arrays[n].extent.is_none() { return Ok(e); }
            let index=capture_index(n,index,arrays,serial,output);
            *serial+=1;
            let temp=format!("@character_element_{serial}");
            output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(arrays[n].length)),names:vec![temp.clone()]});
            output.push(dispatch(n,index,arrays,|slot|Stmt::CallSub {name:"@character_element_read".into(),args:vec![Expr::Ident(temp.clone()),Expr::Ident(slot),Expr::IntLit(arrays[n].length.to_string())]}));
            return Ok(Expr::Ident(temp));
        }
        Expr::Call(_,a) | Expr::Index(_,a) => for x in a { *x=dynamic(x.clone(),arrays,serial,output)?; },
        Expr::Bin(_,a,b) => { **a=dynamic((**a).clone(),arrays,serial,output)?; **b=dynamic((**b).clone(),arrays,serial,output)?; }
        Expr::Un(_,a) => **a=dynamic((**a).clone(),arrays,serial,output)?,
        _=>{}
    }
    Ok(e)
}
fn expanded(e: Expr, arrays: &Arrays, serial: &mut usize, output: &mut Vec<Stmt>) -> Result<Value> {
    expression(dynamic(e,arrays,serial,output)?,arrays)
}
fn body(
    stmts: &mut Vec<Stmt>,
    inherited: &Arrays,
    params: &[String],
    module: bool,
    serial: &mut usize,
) -> Result<()> {
    let mut arrays = inherited.clone();
    for p in params {
        arrays.remove(p);
    }
    let mut shapes=HashMap::new();
    crate::allocatables::metadata(stmts,&mut shapes);
    for s in stmts.iter() {
        match s {
            Stmt::VarDecl { names, .. } => {
                for n in names {
                    arrays.remove(n);
                }
            }
            Stmt::ArrayDecl {
                kind: TypeSpec::Character(length),
                name,
                dims,
            } => {
                if module || params.contains(name) {
                    bail!("CHARACTER arrays currently require local storage, not module or dummy storage");
                }
                let [extent] = dims.as_slice() else {
                    bail!("CHARACTER arrays currently require rank one");
                };
                let extent = integer(extent)?;
                if !(0..=1024).contains(&extent) {
                    bail!("CHARACTER arrays currently support at most 1024 elements");
                }
                *serial += 1;
                arrays.insert(
                    name.clone(),
                    Array {
                        length: length.unwrap_or(1),
                        extent: crate::allocatables::length(name,&shapes),
                        elements: (1..=extent)
                            .map(|i| format!("@character_array_{serial}_{name}_{i}"))
                            .collect(),
                    },
                );
            }
            Stmt::ArrayDecl { name, .. } => {
                arrays.remove(name);
            }
            _ => {}
        }
    }
    let mut output = Vec::new();
    for mut s in std::mem::take(stmts) {
        match &mut s {
            Stmt::CallSub {name,args} if name=="@undefine" && args.len()==1 && matches!(&args[0],Expr::Ident(n) if arrays.contains_key(n)) => {
                let Expr::Ident(n)=&args[0] else {unreachable!()};
                output.extend(arrays[n].elements.iter().map(|n|Stmt::CallSub {name:"@undefine".into(),args:vec![Expr::Ident(n.clone())]}));
                continue;
            }
            Stmt::ArrayDecl {
                kind: TypeSpec::Character(_),
                name,
                ..
            } => {
                let a = &arrays[name];
                if !a.elements.is_empty() {
                    output.push(Stmt::VarDecl {
                        kind: TypeSpec::Character(Some(a.length)),
                        names: a.elements.clone(),
                    });
                }
                continue;
            }
            Stmt::Assign { name, value } if arrays.contains_key(name) => {
                let value=dynamic(value.clone(),&arrays,serial,&mut output)?;
                assign(
                    select(name, None, &arrays)?,
                    arrays[name].length,
                    None,
                    value,
                    &arrays,
                    serial,
                    &mut output,
                )?;
                continue;
            }
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } if arrays.contains_key(name) => {
                let [index] = indices.as_slice() else {
                    bail!("CHARACTER arrays currently require one subscript");
                };
                if !matches!(index,Expr::Section {..}) && (crate::parameters::integer_constant(index).is_err() || arrays[name].extent.is_some()) {
                    let index=dynamic(index.clone(),&arrays,serial,&mut output)?;
                    let index=capture_index(name,index,&arrays,serial,&mut output);
                    let value=expanded(value.clone(),&arrays,serial,&mut output)?.expr();
                    *serial+=1;
                    let temp=format!("@character_store_{serial}");
                    output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(arrays[name].length)),names:vec![temp.clone()]});
                    output.push(Stmt::Assign {name:temp.clone(),value});
                    output.push(dispatch(name,index,&arrays,|slot|Stmt::Assign {name:slot,value:Expr::Ident(temp.clone())}));
                    continue;
                }
                let value=dynamic(value.clone(),&arrays,serial,&mut output)?;
                assign(
                    select(name, Some(index), &arrays)?,
                    arrays[name].length,
                    None,
                    value,
                    &arrays,
                    serial,
                    &mut output,
                )?;
                continue;
            }
            Stmt::CallSub { name, args } if name == "@array_substring_assign" => {
                let [base, range, value] = args.as_slice() else {
                    unreachable!()
                };
                let (name, index) = match base {
                    Expr::Section { name, .. } => (name, Some(base)),
                    Expr::Call(name, args) | Expr::Index(name, args) if args.len() == 1 => {
                        (name, Some(&args[0]))
                    }
                    _ => bail!("array substring assignment requires a CHARACTER array"),
                };
                let a = arrays
                    .get(name)
                    .context("array substring assignment requires a CHARACTER array")?;
                assign(
                    select(name, index, &arrays)?,
                    a.length,
                    Some(range.clone()),
                    value.clone(),
                    &arrays,
                    serial,
                    &mut output,
                )?;
                continue;
            }
            Stmt::Print { items } => {
                let mut values = Vec::new();
                for e in std::mem::take(items) {
                    match expanded(e, &arrays, serial, &mut output)? {
                        Value::Scalar(e) => values.push(e),
                        Value::Array(v) => values.extend(v),
                    }
                }
                *items = values;
            }
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                *value = expanded(value.clone(), &arrays, serial, &mut output)?.expr()
            }
            Stmt::AssignIndex { indices, value, .. } => {
                for e in indices {
                    *e = expanded(e.clone(), &arrays, serial, &mut output)?.expr();
                }
                *value = expanded(value.clone(), &arrays, serial, &mut output)?.expr();
            }
            Stmt::CallSub { name, args } if matches!(name.as_str(),"@formatted_print"|"@formatted_write_noadvance"|"@write") => {
                let mut first=if name=="@write" {2} else {1};
                if name=="@write" {while matches!(args.get(first),Some(Expr::Call(n,_)) if n.starts_with("@write_option:")) {first+=1;}}
                let mut values=vec![];
                for (i,e) in std::mem::take(args).into_iter().enumerate() {
                    match expanded(e,&arrays,serial,&mut output)? {
                        Value::Scalar(e)=>values.push(e),
                        Value::Array(items) if i>=first=>values.extend(items),
                        Value::Array(_)=>bail!("formatted I/O controls require scalar values"),
                    }
                }
                *args=values;
            }
            Stmt::CallSub { name, args } => {
                if name=="@internal_read" && matches!(args.get(3),Some(Expr::Call(n,_)|Expr::Index(n,_)) if arrays.contains_key(n)) {
                    bail!("internal READ currently requires a scalar CHARACTER destination, not an array element");
                }
                let dest=match name.as_str() {"@formatted_read_status"=>Some((4,Some(3))),"@formatted_read"=>Some((2,None)),_=>None};
                let mut suffix=Vec::new();
                if let Some((position,status))=dest {
                    if let Some(Expr::Call(n,a) | Expr::Index(n,a))=args.get(position).cloned() {
                        if arrays.contains_key(&n) && a.len()==1 {
                            let index=dynamic(a[0].clone(),&arrays,serial,&mut output)?;
                            let index=capture_index(&n,index,&arrays,serial,&mut output);
                            *serial+=1;
                            let temp=format!("@character_read_{serial}");
                            output.push(Stmt::VarDecl {kind:TypeSpec::Character(Some(arrays[&n].length)),names:vec![temp.clone()]});
                            args[position]=Expr::Ident(temp.clone());
                            let store=dispatch(&n,index.clone(),&arrays,|slot|Stmt::Assign {name:slot,value:Expr::Ident(temp.clone())});
                            suffix.push(if let Some(status)=status {
                                let code=&args[status];
                                if !matches!(code,Expr::Ident(_)) {store} else {Stmt::If {cond:Expr::Bin(BinOp::Eq,Box::new(code.clone()),Box::new(Expr::IntLit("0".into()))),then_body:vec![store],else_body:Some(vec![dispatch(&n,index,&arrays,|slot|Stmt::CallSub {name:"@undefine".into(),args:vec![Expr::Ident(slot)]})])}}
                            } else {store});
                        }
                    }
                }
                for e in args {
                    *e=match expanded(e.clone(),&arrays,serial,&mut output)? {Value::Scalar(e)=>e,Value::Array(_)=>bail!("CHARACTER array I/O destinations and procedure arguments are not yet supported")};
                }
                output.push(s);output.extend(suffix);continue;
            }
            Stmt::Read { args } => {
                for e in args { *e=expanded(e.clone(),&arrays,serial,&mut output)?.expr(); }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                *cond = expanded(cond.clone(), &arrays, serial, &mut output)?.expr();
                body(then_body, &arrays, &[], false, serial)?;
                if let Some(b) = else_body {
                    body(b, &arrays, &[], false, serial)?;
                }
            }
            Stmt::Do {
                start,
                end,
                body: b,
                ..
            } => {
                *start = expanded(start.clone(), &arrays, serial, &mut output)?.expr();
                *end = expanded(end.clone(), &arrays, serial, &mut output)?.expr();
                body(b, &arrays, &[], false, serial)?;
            }
            Stmt::DoWhile { cond, body: b } => {
                let mut prefix=Vec::new();
                *cond = expanded(cond.clone(), &arrays, serial, &mut prefix)?.expr();
                body(b, &arrays, &[], false, serial)?;
                if !prefix.is_empty() {
                    prefix.push(Stmt::If {cond:Expr::Un(crate::ast::UnOp::Not,Box::new(cond.clone())),then_body:vec![Stmt::Exit],else_body:None});
                    prefix.append(b);*b=prefix;*cond=Expr::Logical(true);
                }
            }
            Stmt::Function {
                params, body: b, ..
            }
            | Stmt::Subroutine {
                params, body: b, ..
            } => body(b, &arrays, params, false, serial)?,
            Stmt::Module { body: b, .. } => body(b, &Arrays::new(), &[], true, serial)?,
            Stmt::Block { body: b } => body(b, &arrays, &[], false, serial)?,
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                *expr = expanded(expr.clone(), &arrays, serial, &mut output)?.expr();
                for c in cases {
                    body(&mut c.body, &arrays, &[], false, serial)?;
                }
                if let Some(b) = default {
                    body(b, &arrays, &[], false, serial)?;
                }
            }
            _ => {}
        }
        output.push(s);
    }
    *stmts = output;
    Ok(())
}
pub fn resolve(mut p: Program) -> Result<Program> {
    body(&mut p.body, &Arrays::new(), &[], false, &mut 0)?;
    Ok(p)
}

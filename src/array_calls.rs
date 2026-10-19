// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize assumed-shape subroutine calls using fixed or runtime descriptors.
//! These private descriptors do not define a separate-object procedure ABI.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
type Shapes = HashMap<String, (TypeSpec, Vec<Expr>)>;
#[derive(Clone)]
struct Routine {
    elemental: bool,
    params: Vec<String>,
    body: Vec<Stmt>,
}

fn array_actual(e: &Expr, shapes: &Shapes) -> bool {
    match e {
        Expr::Ident(name) => shapes.get(name).is_some_and(|(_,dims)|!dims.is_empty()),
        Expr::Section {..} => true,
        Expr::Un(_,a) => array_actual(a,shapes),
        Expr::Bin(_,a,b) => array_actual(a,shapes)||array_actual(b,shapes),
        Expr::Call(name,args) if !shapes.contains_key(name) => {
            matches!(name.as_str(),"@array_constructor"|"@matrix_view"|"@packed_view"|"reshape"|"pack")
                || args.iter().any(|a|array_actual(a,shapes))
        }
        _ => false,
    }
}
fn runtime_actual_axes(e:&Expr,shapes:&Shapes)->Option<Vec<Expr>> {
    match e {
        Expr::Ident(n)=>crate::allocatables::axes(n,shapes),
        Expr::Bin(_,a,b)=>runtime_actual_axes(a,shapes).or_else(||runtime_actual_axes(b,shapes)),
        Expr::Un(_,a)=>runtime_actual_axes(a,shapes),
        Expr::Call(n,args) if n=="@packed_view"=>runtime_actual_axes(&args[0],shapes),
        _=>None,
    }
}
// Expand array calls before ordinary scalar specialization. Keeping this in a
// separate function also limits the stack frame of the recursive `expand` pass.
fn elemental_array_call(name: &str, args: &[Expr], routine: &Routine, shapes: &Shapes, readonly: &HashSet<String>, serial: &mut usize, functions: &HashMap<String,TypeSpec>) -> Result<Option<Stmt>> {
    if !args.iter().any(|a|array_actual(a,shapes)) {return Ok(None);}
    if args.len()>routine.params.len() {bail!("too many arguments to `{name}`");}
    *serial+=1;
    let index=format!("@elemental_index_{serial}");
    let position=Expr::Ident(index.clone());
    let mut prefix=vec![Stmt::VarDecl {kind:TypeSpec::Integer(None),names:vec![index.clone()]}];
    let mut local=shapes.clone();
    let mut extent=None;
    let mut scalars=vec![];
    let mut before=vec![];
    let mut after=vec![];
    for (param,actual) in routine.params.iter().zip(args) {
        if crate::named_arguments::omitted(actual) {scalars.push(actual.clone());continue;}
        let intent=routine.body.iter().find_map(|s|match s {
            Stmt::DummyIntent {name,intent} if name==param && intent.starts_with("intent(")=>Some(intent.as_str()),_=>None,
        }).context("ELEMENTAL dummy requires explicit INTENT")?;
        let array=array_actual(actual,&local);
        if intent!="intent(in)" && !array {bail!("ELEMENTAL writable dummy `{param}` requires an array actual when any argument is an array");}
        if intent!="intent(in)" && matches!(actual,Expr::Ident(n)|Expr::Section {name:n,..} if readonly.contains(n)) {
            bail!("cannot pass PARAMETER or INTENT(IN) variable as writable argument `{param}`");
        }
        let (kind,value)=if array {
            if intent!="intent(in)" && !matches!(actual,Expr::Ident(_)|Expr::Section {..}) {
                bail!("ELEMENTAL writable dummy `{param}` requires a whole array or section variable");
            }
            let (value,count,kind)=if intent=="intent(in)" {
                let mut captured=actual.clone();
                let (kind,capacity,length)=crate::reductions::argument_with_elementals(&mut captured,&local,&mut prefix,serial,functions)?;
                let count=crate::parameters::integer_constant(&length).context("ELEMENTAL array calls currently require constant rank-one extents")?;
                if count!=capacity {bail!("ELEMENTAL array calls currently require constant rank-one extents");}
                for s in &prefix {match s {
                    Stmt::ArrayDecl {kind,name,dims}=>{local.insert(name.clone(),(kind.clone(),dims.clone()));},
                    Stmt::VarDecl {kind,names}=>for n in names {local.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
                }}
                (crate::reductions::elemental_element(&captured,&position,&local)?.0,count,kind)
            } else {
                // An OUT section must select storage without reading its unset values.
                let (value,count,kind)=crate::reductions::elemental_element(actual,&position,&local)?;
                (value,count.context("ELEMENTAL writable argument must be an array")?,kind)
            };
            if extent.is_some_and(|n|n!=count) {bail!("ELEMENTAL array actual shapes do not conform");}
            extent=Some(count);
            (kind,value)
        } else {(scalar_actual_kind(actual,&local)?,actual.clone())};
        *serial+=1;
        let temp=format!("@elemental_actual_{serial}");
        let declaration=Stmt::VarDecl {kind,names:vec![temp.clone()]};
        if array {
            before.push(declaration);
            if intent!="intent(out)" {before.push(Stmt::Assign {name:temp.clone(),value:value.clone()});}
            if intent!="intent(in)" {
                let Expr::Index(array,indices)=value else {bail!("ELEMENTAL writable argument must identify array elements");};
                after.push(Stmt::AssignIndex {name:array,indices,value:Expr::Ident(temp.clone())});
            }
        } else {
            // Evaluate scalar input expressions once, even for a zero-size call.
            prefix.push(declaration);
            prefix.push(Stmt::Assign {name:temp.clone(),value});
        }
        scalars.push(Expr::Ident(temp));
    }
    before.push(Stmt::CallSub {name:name.into(),args:scalars});
    before.extend(after);
    prefix.push(Stmt::Do {var:index,start:Expr::IntLit("1".into()),end:Expr::IntLit(extent.unwrap().to_string()),body:vec![Stmt::Block {body:before}]});
    Ok(Some(Stmt::Block {body:prefix}))
}

pub(crate) fn scalar_actual_kind(e: &Expr, shapes: &Shapes) -> Result<TypeSpec> {
    use crate::ast::BinOp;
    match e {
        Expr::Call(n,args) if crate::real_intrinsics::keys(n).is_some() && !shapes.contains_key(n) || crate::real_intrinsics::decode(n).is_some() => {
            let public=crate::real_intrinsics::decode(n).map_or(n.as_str(),|(n,_)|n);
            let kinds=args.iter().map(|arg|scalar_actual_kind(arg,shapes)).collect::<Result<Vec<_>>>()?;
            crate::real_intrinsics::result_kind(public,&kinds)
        }
        Expr::Call(n,args) if crate::intrinsics::real_predicate(n) && !shapes.contains_key(n) => {
            if args.first().is_some_and(|a|crate::reductions::elemental_element(a,&Expr::IntLit("1".into()),shapes).is_ok_and(|(_,shape,_)|shape.is_some())) {
                bail!("scalar actual argument must not be an array expression");
            }
            Ok(TypeSpec::Logical)
        },
        Expr::Call(n,args) if crate::intrinsics::integer_rounding(n) && !shapes.contains_key(n) => {
            if args.first().is_some_and(|a|crate::reductions::elemental_element(a,&Expr::IntLit("1".into()),shapes).is_ok_and(|(_,shape,_)|shape.is_some())) {
                bail!("scalar actual argument must not be an array expression");
            }
            crate::intrinsics::rounding_kind(n,args)
        },
        Expr::Ident(n) => {
            let (kind, dims) = shapes.get(n).context("scalar actual argument must be declared")?;
            if !dims.is_empty() { bail!("subroutine array actual arguments are not yet supported; pass scalar arguments"); }
            Ok(kind.clone())
        }
        Expr::IntLit(_) => Ok(TypeSpec::Integer(None)),
        Expr::Call(n,args) if n=="@integer_literal" && args.len()==2 =>
            Ok(TypeSpec::Integer(Some(u8::try_from(crate::parameters::integer_constant(&args[1])?)?))),
        Expr::RealLit(_) => Ok(TypeSpec::Real),
        Expr::Logical(_) => Ok(TypeSpec::Logical),
        Expr::Str(s) => Ok(TypeSpec::Character(Some(s.len()))),
        Expr::Un(_, a) => scalar_actual_kind(a, shapes),
        Expr::Bin(op, a, b) => {
            let a = scalar_actual_kind(a, shapes)?;
            let b = scalar_actual_kind(b, shapes)?;
            if matches!(op, BinOp::Concat) {
                if let (TypeSpec::Character(a), TypeSpec::Character(b)) = (a, b) {
                    return Ok(TypeSpec::Character(Some(a.unwrap_or(1) + b.unwrap_or(1))));
                }
                bail!("CHARACTER concatenation requires CHARACTER operands");
            }
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) {
                Ok(if matches!(a, TypeSpec::DoublePrecision) || matches!(b, TypeSpec::DoublePrecision) {TypeSpec::DoublePrecision}
                   else if matches!(a, TypeSpec::Real) || matches!(b, TypeSpec::Real) {TypeSpec::Real} else {a})
            } else { Ok(TypeSpec::Logical) }
        }
        Expr::Index(n, args) | Expr::Call(n, args) if shapes.get(n).is_some_and(|(_,dims)| !dims.is_empty()) => {
            let (kind,dims)=&shapes[n];
            if args.len()!=dims.len() {bail!("array element actual `{n}` requires {} subscripts",dims.len());}
            Ok(kind.clone())
        }
        Expr::Call(n, args) if matches!(n.as_str(), "dble" | "real" | "int") && args.len() == 1 => {
            scalar_actual_kind(&args[0], shapes)?;
            Ok(match n.as_str() { "dble" => TypeSpec::DoublePrecision, "real" => TypeSpec::Real, _ => TypeSpec::Integer(None) })
        }
        Expr::Call(n,args) if matches!(n.as_str(),"mod"|"modulo"|"sign") && args.len()==2 => {
            let a=scalar_actual_kind(&args[0],shapes)?;let b=scalar_actual_kind(&args[1],shapes)?;
            Ok(match (&a,&b) {
                (TypeSpec::Integer(x),TypeSpec::Integer(y))=>TypeSpec::Integer(Some(x.unwrap_or(4).max(y.unwrap_or(4)))),
                _=>a,
            })
        }
        Expr::Call(n,args) if n=="@allocated_extent"=>scalar_actual_kind(&args[1],shapes),
        Expr::Call(n,_) if matches!(n.as_str(),"@heap_bound"|"@heap_lower_bound"|"@heap_upper_bound")=>Ok(TypeSpec::Integer(Some(8))),
        Expr::Call(n,args) if n=="size" || matches!(n.as_str(),"lbound"|"ubound") && args.len()==2=>Ok(TypeSpec::Integer(None)),
        Expr::Section { .. } => bail!("subroutine array actual arguments are not yet supported; pass scalar arguments"),
        _ => bail!("unsupported scalar actual expression in specialized subroutine call"),
    }
}

fn rewrite(
    e: &mut Expr,
    shapes: &Shapes,
    constants: &HashMap<String, Expr>,
    absent: &HashSet<String>,
) -> Result<()> {
    if matches!(e,Expr::Ident(n) if shapes.contains_key(&format!("@pointer:{n}")) || shapes.contains_key(&format!("@target:{n}"))) { return Ok(()); }
    if let Expr::Call(n,args)=e {
        if matches!(n.as_str(),"size"|"shape"|"lbound"|"ubound"|"associated") && matches!(args.first(),Some(Expr::Ident(p)) if shapes.contains_key(&format!("@pointer:{p}"))) { return Ok(()); }
    }
    if let Expr::Call(n,args)=e {
        if matches!(n.as_str(),"allocated"|"size"|"lbound"|"ubound"|"shape"|"kind"|"len") {
            for dim in args.iter_mut().skip(1) {rewrite(dim,shapes,constants,absent)?;}
            if let Some(value)=crate::allocatables::inquiry(n,args,shapes)? {*e=value;return Ok(());}
        }
    }
    match e {
        Expr::Ident(n) => {
            if absent.contains(n) { bail!("reference to absent OPTIONAL dummy `{n}`"); }
            if let Some(v) = constants.get(n) {
                *e = v.clone();
            } else if crate::allocatables::length(n,shapes).is_some() {
                *e=crate::allocatables::view(n,shapes);
            }
        }
        Expr::Call(n,_) if n=="@packed_view" => {},
        Expr::Call(n,args) if n=="@matrix_view" => {
            if matches!(args.first(),Some(Expr::Ident(array)) if crate::allocatables::length(array,shapes).is_some_and(|e|crate::parameters::integer_constant(&e).is_err())) {
                bail!("sections of runtime-sized array dummies are not yet supported");
            }
            for e in args.iter_mut().skip(1) {rewrite(e,shapes,constants,absent)?;}
        },
        Expr::Call(n,args) if n=="allocated" => {
            if let [Expr::Ident(array)]=args.as_slice() {if let Some(flag)=crate::allocatables::allocated(array,shapes) {*e=flag;}}
        }
        Expr::Call(n, args) if n == "present" => {
            let [Expr::Ident(param)] = args.as_slice() else {
                bail!("PRESENT requires an optional dummy argument");
            };
            if !shapes.contains_key(&format!("@optional:{param}")) { bail!("PRESENT requires an OPTIONAL dummy argument"); }
            *e = Expr::Logical(!absent.contains(param));
        }
        Expr::Call(n, args) if n == "len" && args.len()==1 => {
            let operand=if let Expr::Call(tag,values)=&args[0] {
                if tag=="@packed_view" {&values[0]} else {&args[0]}
            } else {&args[0]};
            if let Expr::Ident(name)=operand {
                if absent.contains(name) {bail!("reference to absent OPTIONAL dummy `{name}`");}
                if let Some((TypeSpec::Character(length),_))=shapes.get(name) {
                    *e=Expr::IntLit(length.unwrap_or(1).to_string());
                }
            }
        }
        Expr::Call(n, args) if n == "size" => {
            if let Some(Expr::Call(tag,a))=args.first() {if tag=="@packed_view" {
                if args.len()>2 || args.get(1).is_some_and(|d|crate::parameters::integer_constant(d).ok()!=Some(1)) {bail!("SIZE dimension must be 1 for a rank-one array");}
                *e=a[1].clone();return Ok(());
            }}
            let Some(Expr::Ident(array)) = args.first() else {
                bail!("SIZE currently requires a whole array");
            };
            let (_, dims) = shapes
                .get(array)
                .context("SIZE argument must be an array")?;
            if dims.is_empty() {
                bail!("SIZE argument must be an array");
            }
            if let Some(length)=crate::allocatables::length(array,shapes) {
                if args.len()>2 || args.get(1).is_some_and(|d|crate::parameters::integer_constant(d).ok()!=Some(1)) {bail!("SIZE dimension must be 1 for a rank-one array");}
                *e=if let Some(flag)=crate::allocatables::allocated(array,shapes) {Expr::Call("@allocated_extent".into(),vec![flag,length,Expr::Str(array.clone())])} else {length};return Ok(());
            }
            *e = match args.as_slice() {
                [_] => dims
                    .iter()
                    .cloned()
                    .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Mul, Box::new(a), Box::new(b)))
                    .unwrap(),
                [_, dim] => {
                    let mut dim = dim.clone();
                    rewrite(&mut dim, shapes, constants, absent)?;
                    let i = crate::parameters::integer_constant(&dim)?;
                    if i < 1 || i as usize > dims.len() {
                        bail!("SIZE dimension must be between 1 and {}", dims.len());
                    }
                    dims[i as usize - 1].clone()
                }
                _ => bail!("SIZE expects an array and optional dimension"),
            };
        }
        Expr::Call(n, args) if n == "shape" => {
            if let [Expr::Ident(array)] = args.as_slice() {
                if absent.contains(array) { bail!("reference to absent OPTIONAL dummy `{array}`"); }
                if let Some((_, dims)) = shapes.get(array) {
                    if dims.is_empty() { bail!("SHAPE currently requires an array"); }
                    if dims.iter().all(|d| crate::parameters::integer_constant(d).is_ok()) {
                        *e = Expr::Call("@bounds_vector".into(), dims.clone());
                    }
                }
            }
        }
        Expr::Call(n, args) if matches!(n.as_str(), "lbound" | "ubound") => {
            for dim in args.iter_mut().skip(1) { rewrite(dim, shapes, constants, absent)?; }
            if let Some(Expr::Ident(array)) = args.first() {
                if let Some(length)=crate::allocatables::length(array,shapes) {
                    if args.len()>2 || args.get(1).is_some_and(|d|crate::parameters::integer_constant(d).ok()!=Some(1)) {bail!("array bound dimension must be 1 for a rank-one array");}
                    let mut value=if n=="ubound" {length} else {Expr::IntLit("1".into())};
                    if let Some(flag)=crate::allocatables::allocated(array,shapes) {value=Expr::Call("@allocated_extent".into(),vec![flag,value,Expr::Str(array.clone())]);}
                    *e=if args.len()==1 {Expr::Call("@array_constructor".into(),vec![value])} else {value};
                    return Ok(());
                }
                if let Some((_, dims)) = shapes.get(array) {
                    if let Ok(dims) = dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>() {
                        *e = crate::inquiries::bounds(n, &dims, args.get(1))?;
                    }
                }
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args {
                rewrite(a, shapes, constants, absent)?;
            }
        }
        Expr::Bin(_, a, b) => {
            rewrite(a, shapes, constants, absent)?;
            rewrite(b, shapes, constants, absent)?;
        }
        Expr::Un(_, a) => rewrite(a, shapes, constants, absent)?,
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                rewrite(e, shapes, constants, absent)?;
            }
        }

        _ => {}
    }
    if let Ok(n) = crate::parameters::integer_constant(e) {
        *e = Expr::IntLit(n.to_string());
    } else if let Ok(value)=crate::parameters::logical_constant(e,&HashMap::new()) {
        *e=Expr::Logical(value);
    }
    Ok(())
}

// Substitute proven constants only in section bounds; keep ordinary variable
// references so later initialization and unused-variable checks retain them.
fn rewrite_value(
    e: &mut Expr,
    shapes: &Shapes,
    constants: &HashMap<String, Expr>,
    absent: &HashSet<String>,
) -> Result<()> {
    match e {
        Expr::Call(n,_) if matches!(n.as_str(),"size" | "lbound" | "ubound" | "shape" | "present" | "allocated" | "@packed_view") => return rewrite(e,shapes,constants,absent),
        // Literal substring bounds can be evaluated at runtime; retaining the
        // original variables also preserves initialization and usage diagnostics.
        Expr::Call(n, _) if n == "@literal_substring" => return rewrite(e, shapes, &HashMap::new(), absent),
        Expr::Section { .. } => return rewrite(e, shapes, constants, absent),
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for e in args {
                rewrite_value(e, shapes, constants, absent)?;
            }
        }
        Expr::Bin(_, a, b) => {
            rewrite_value(a, shapes, constants, absent)?;
            rewrite_value(b, shapes, constants, absent)?;
        }
        Expr::Un(_, a) => rewrite_value(a, shapes, constants, absent)?,
        _ => {}
    }
    rewrite(e, shapes, &HashMap::new(), absent)
}

// Local arrays that are explicitly deallocated need runtime allocation state.
// Infer bounded backing storage from ALLOCATE shapes known at this call site.
fn prepare_deallocated_locals(
    body: &mut Vec<Stmt>,
    shapes: &mut Shapes,
    constants: &HashMap<String, Expr>,
    absent: &HashSet<String>,
    stable: &HashSet<String>,
) -> Result<()> {
    let candidates: HashSet<_>=body.iter().filter_map(|s|match s {
        Stmt::ArrayDecl {name,dims,..} if dims.len()==1 && matches!(&dims[0],Expr::Ident(n) if n=="@deferred_extent")
            && crate::allocatables::allocated(name,shapes).is_none()
            && body.iter().any(|s|matches!(s,Stmt::DummyIntent {name:n,intent} if n==name && intent=="allocatable"))=>Some(name.clone()),
        _=>None,
    }).collect();
    fn targets(body: &[Stmt], candidates: &HashSet<String>, found: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == "@deallocate" => {
                    for e in args {
                        if let Expr::Ident(n) = e {
                            if candidates.contains(n) {
                                found.insert(n.clone());
                            }
                        }
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    targets(then_body, candidates, found);
                    if let Some(b) = else_body {
                        targets(b, candidates, found);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                    targets(body, candidates, found)
                }
                Stmt::Block { body } => {
                    let mut visible = candidates.clone();
                    for s in body {
                        match s {
                            Stmt::ArrayDecl { name, .. } => {
                                visible.remove(name);
                            }
                            Stmt::VarDecl { names, .. } => {
                                for n in names {
                                    visible.remove(n);
                                }
                            }
                            _ => {}
                        }
                    }
                    targets(body, &visible, found);
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        targets(&c.body, candidates, found);
                    }
                    if let Some(b) = default {
                        targets(b, candidates, found);
                    }
                }
                _ => {}
            }
        }
    }
    let mut wanted = HashSet::new();
    targets(body, &candidates, &mut wanted);
    if wanted.is_empty() {
        return Ok(());
    }
    let mut local = shapes.clone();
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, kind, dims } => {
                local
                    .entry(name.clone())
                    .or_insert((kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    local.entry(n.clone()).or_insert((kind.clone(), vec![]));
                }
            }
            _ => {}
        }
    }
    fn infer(
        body: &[Stmt],
        wanted: &HashSet<String>,
        shapes: &mut Shapes,
        constants: &mut HashMap<String, Expr>,
        absent: &HashSet<String>,
        stable: &HashSet<String>,
        bounds: &mut HashMap<String, i64>,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::Assign { name, value } if stable.contains(name) => {
                    let mut value = value.clone();
                    rewrite(&mut value, shapes, constants, absent)?;
                    if crate::parameters::integer_constant(&value).is_ok() {
                        constants.insert(name.clone(), value);
                    }
                }
                Stmt::CallSub { name, args } if name == "@allocate" => {
                    for a in args {
                        if let Expr::Call(n, dims) = a {
                            if wanted.contains(n) {
                                let [dim] = dims.as_slice() else {
                                    bail!("ALLOCATE rank mismatch for `{n}`");
                                };
                                let mut dim = dim.clone();
                                rewrite(&mut dim, shapes, constants, absent)?;
                                let size=crate::parameters::integer_constant(&dim).context("ALLOCATE currently requires a shape known at the call site")?.max(0);
                                bounds
                                    .entry(n.clone())
                                    .and_modify(|v| *v = (*v).max(size))
                                    .or_insert(size);
                                if let Some((_, d)) = shapes.get_mut(n) {
                                    *d = vec![Expr::IntLit(size.to_string())];
                                }
                            }
                        }
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    infer(
                        then_body,
                        wanted,
                        &mut shapes.clone(),
                        &mut constants.clone(),
                        absent,
                        stable,
                        bounds,
                    )?;
                    if let Some(b) = else_body {
                        infer(
                            b,
                            wanted,
                            &mut shapes.clone(),
                            &mut constants.clone(),
                            absent,
                            stable,
                            bounds,
                        )?;
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => infer(
                    body,
                    wanted,
                    &mut shapes.clone(),
                    &mut constants.clone(),
                    absent,
                    stable,
                    bounds,
                )?,
                Stmt::Block { body } => {
                    let mut visible = wanted.clone();
                    for s in body {
                        match s {
                            Stmt::ArrayDecl { name, .. } => {
                                visible.remove(name);
                            }
                            Stmt::VarDecl { names, .. } => {
                                for n in names {
                                    visible.remove(n);
                                }
                            }
                            _ => {}
                        }
                    }
                    infer(
                        body,
                        &visible,
                        &mut shapes.clone(),
                        &mut constants.clone(),
                        absent,
                        stable,
                        bounds,
                    )?;
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        infer(
                            &c.body,
                            wanted,
                            &mut shapes.clone(),
                            &mut constants.clone(),
                            absent,
                            stable,
                            bounds,
                        )?;
                    }
                    if let Some(b) = default {
                        infer(
                            b,
                            wanted,
                            &mut shapes.clone(),
                            &mut constants.clone(),
                            absent,
                            stable,
                            bounds,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let mut bounds = HashMap::new();
    infer(
        body,
        &wanted,
        &mut local,
        &mut constants.clone(),
        absent,
        stable,
        &mut bounds,
    )?;
    let mut declarations = vec![];
    for s in body.iter_mut() {
        if let Stmt::ArrayDecl { name, dims, kind } = s {
            if wanted.contains(name) {
                *dims = vec![Expr::IntLit(
                    bounds.get(name).copied().unwrap_or(0).to_string(),
                )];
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
                let mut suffix = 0;
                let (length, flag) = loop {
                    let length = format!("@dealloc_length_{name}_{suffix}");
                    let flag = format!("@dealloc_flag_{name}_{suffix}");
                    if !shapes.contains_key(&length) && !shapes.contains_key(&flag) {
                        break (length, flag);
                    }
                    suffix += 1;
                };
                declarations.extend([
                    Stmt::VarDecl {
                        kind: TypeSpec::Integer(None),
                        names: vec![length.clone()],
                    },
                    Stmt::VarDecl {
                        kind: TypeSpec::Logical,
                        names: vec![flag.clone()],
                    },
                    Stmt::Assign {
                        name: length.clone(),
                        value: Expr::IntLit("0".into()),
                    },
                    Stmt::Assign {
                        name: flag.clone(),
                        value: Expr::Logical(false),
                    },
                    Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@bounded:{length}:{flag}"),
                    },
                ]);
            }
        }
    }
    body.retain(|s|!matches!(s,Stmt::DummyIntent {name,intent} if wanted.contains(name) && intent=="allocatable"));
    declarations.append(body);
    *body = declarations;
    crate::allocatables::metadata(body, shapes);
    Ok(())
}
fn specialize_character_kind(kind:&mut TypeSpec,shapes:&Shapes,constants:&HashMap<String,Expr>,absent:&HashSet<String>)->Result<()> {
    if let TypeSpec::CharacterLength(length)=kind {
        rewrite(length,shapes,constants,absent)?;
        *kind=crate::parameters::concrete_kind(kind.clone())?;
    }
    Ok(())
}
fn specialize(
    body: &mut Vec<Stmt>,
    shapes: &mut Shapes,
    absent: &HashSet<String>,
    constants: &mut HashMap<String, Expr>,
    stable: &HashSet<String>,
) -> Result<()> {
    let statement_location = std::cell::Cell::new(None);
    let result = (|| -> Result<()> {
    crate::allocatables::prepare_matrices(body, shapes, constants)?;
    crate::modules::bind_scope(body)?;
    prepare_deallocated_locals(body, shapes, constants, absent, stable)?;
    if let Some(Stmt::CallSub { name, args }) = body.first() {
        if name == "@forall" || name == "@do_step" {
            for control in args.chunks_exact(4) {
                if let Expr::Ident(index) = &control[0] { constants.remove(index); }
            }
        }
    }
    let allocatable: HashSet<_> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if intent == "allocatable" => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut allocated = HashSet::new();
    let deferred: HashMap<_, _> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::ArrayDecl { name, kind, dims }
                if dims
                    .iter()
                    .any(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent")) && !shapes.contains_key(&format!("@pointer:{name}")) =>
            {
                Some((name.clone(), (kind.clone(), dims.clone())))
            }
            _ => None,
        })
        .collect();
    body.retain(|s| !matches!(s, Stmt::ArrayDecl { name, .. } if deferred.contains_key(name)));
    let mut output = Vec::new();
    for mut s in std::mem::take(body) {
        if let Some(p) = crate::source_diagnostics::statement_offset(&s) { statement_location.set(Some(p)); }
        let pending: HashSet<_> = deferred
            .keys()
            .filter(|n| !allocated.contains(*n))
            .cloned()
            .collect();
        check_allocated(&s, &pending)?;
        match &mut s {
            Stmt::ArrayDecl { name, kind, dims } => {
                specialize_character_kind(kind,shapes,constants,absent)?;
                for d in dims.iter_mut() {
                    rewrite(d, shapes, constants, absent)?;
                }
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { names, kind } => {
                specialize_character_kind(kind,shapes,constants,absent)?;
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            Stmt::CallSub { name, args } if name == "@allocate" => {
                for a in args {
                    let Expr::Call(n, dims) = a else {
                        bail!("ALLOCATE expects array bounds");
                    };
                    if crate::allocatables::allocated(n,shapes).is_some() {
                        for d in dims {rewrite_value(d,shapes,constants,absent)?;}
                        output.push(Stmt::CallSub {name:"@allocate".into(),args:vec![a.clone()]});
                        continue;
                    }
                    if !allocatable.contains(n) {
                        bail!("ALLOCATE requires ALLOCATABLE array `{n}`");
                    }
                    if !allocated.insert(n.clone()) {
                        bail!("repeated ALLOCATE of `{n}` without DEALLOCATE is not supported");
                    }
                    let (kind, declared_dims) = deferred
                        .get(n)
                        .context(
                            "ALLOCATE requires a local deferred-shape array declared in this scope",
                        )?
                        .clone();
                    for d in dims.iter_mut() {
                        rewrite(d, shapes, constants, absent)?;
                    }
                    if dims.len() != declared_dims.len() {
                        bail!("ALLOCATE rank mismatch for `{n}`");
                    }
                    if !declared_dims
                        .iter()
                        .all(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent"))
                    {
                        bail!("ALLOCATABLE array `{n}` requires all dimensions to be deferred");
                    }
                    if dims
                        .iter()
                        .any(|d| crate::parameters::integer_constant(d).is_err())
                    {
                        bail!("ALLOCATE currently requires a shape known at the call site");
                    }
                    // Nonpositive extents describe an empty allocated array.
                    for d in dims.iter_mut() {
                        *d=Expr::IntLit(crate::parameters::integer_constant(d)?.max(0).to_string());
                    }
                    shapes.insert(n.clone(), (kind.clone(), dims.clone()));
                    output.push(Stmt::ArrayDecl {
                        name: n.clone(),
                        kind,
                        dims: dims.clone(),
                    });
                }
                continue;
            }
            Stmt::CallSub {name,args} if name=="@deallocate"=>{
                crate::allocatables::deallocation_targets(args)?;
                output.push(s);continue;
            }
            Stmt::Assign { name, value } => {
                if absent.contains(name) { bail!("assignment to absent OPTIONAL dummy `{name}`"); }
                let mut folded = value.clone();
                rewrite(&mut folded, shapes, constants, absent)?;
                if stable.contains(name)
                    && shapes.get(name).is_some_and(|(_, dims)| dims.is_empty())
                    && crate::parameters::integer_constant(&folded).is_ok()
                {
                    constants.insert(name.clone(), folded);
                }
                rewrite_value(value, shapes, constants, absent)?;
            }
            Stmt::AssignIndex { value, indices, .. } => {
                rewrite_value(value, shapes, constants, absent)?;
                for e in indices {
                    rewrite_value(e, shapes, constants, absent)?;
                }
            }
            Stmt::CallSub { name, args } if name == "@forall" || name == "@do_step" => {
                for control in args.chunks_exact_mut(4) {
                    for arg in &mut control[1..] { rewrite(arg, shapes, constants, absent)?; }
                }
            }
            Stmt::CallSub { args, .. } => {
                for e in args {
                    // Association needs the storage identity, not a value view.
                    if matches!(e,Expr::Ident(n) if shapes.get(n).is_some_and(|(_,d)|!d.is_empty())) {continue;}
                    rewrite_value(e, shapes, constants, absent)?;
                }
            }
            Stmt::Print { items: args } => {
                for e in args {
                    rewrite_value(e, shapes, constants, absent)?;
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                rewrite(cond, shapes, constants, absent)?;
                if let Expr::Logical(value) = cond {
                    let mut chosen = if *value {
                        std::mem::take(then_body)
                    } else {
                        else_body.take().unwrap_or_default()
                    };
                    specialize(&mut chosen, shapes, absent, constants, stable)?;
                    let returned=matches!(chosen.iter().rev().find(|s|!matches!(s,Stmt::SourceLocation(_))),Some(Stmt::Return(_)));
                    output.extend(chosen);
                    if returned {break;}
                    continue;
                }
                specialize(
                    then_body,
                    &mut shapes.clone(),
                    absent,
                    &mut constants.clone(),
                    stable,
                )?;
                if let Some(b) = else_body {
                    specialize(
                        b,
                        &mut shapes.clone(),
                        absent,
                        &mut constants.clone(),
                        stable,
                    )?;
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                rewrite(start, shapes, constants, absent)?;
                rewrite(end, shapes, constants, absent)?;
                specialize(
                    body,
                    &mut shapes.clone(),
                    absent,
                    &mut constants.clone(),
                    stable,
                )?;
            }
            Stmt::Block { body } => {
                let mut local = shapes.clone();
                let mut values = constants.clone();
                // A BLOCK declaration shadows both the outer storage and its
                // allocation metadata, even when the source name is identical.
                for declaration in body.iter() {
                    let names = match declaration {
                        Stmt::ArrayDecl { name, .. } => vec![name.clone()],
                        Stmt::VarDecl { names, .. } => names.clone(),
                        _ => vec![],
                    };
                    for name in names {
                        local.remove(&name);
                        local.remove(&format!("@allocated:{name}"));
                        local.remove(&format!("@extent:{name}"));
                        local.remove(&format!("@axes:{name}"));
                        local.remove(&format!("@axis_limits:{name}"));
                        local.remove(&format!("@optional:{name}"));
                        values.remove(&name);
                    }
                }
                let forall=matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@forall");
                specialize(body, &mut local, absent, &mut values, stable)?;
                if forall {
                    for statement in body {
                        if let Stmt::AssignIndex {indices,..}=statement {
                            for e in indices {rewrite(e,&local,&values,absent)?;}
                        }
                    }
                }
            }
            _ => {}
        }
        output.push(s);
        if matches!(output.last(),Some(Stmt::Return(_))) {break;}
    }
    *body = output;
    crate::heap::automatic(body,shapes)?;
    Ok(())
    })();
    result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
}
// Resolve a CHARACTER function's fixed declarations and stable local constants
// while its original dummy names still identify the associated array shapes.
pub(crate) fn specialize_character_function(body:&mut Vec<Stmt>,shapes:&mut Shapes,constants:&HashMap<String,Expr>)->Result<()> {
    let stable=stable(body);
    specialize(body,shapes,&HashSet::new(),&mut constants.clone(),&stable)
}
pub(crate) fn check_allocated(s: &Stmt, pending: &HashSet<String>) -> Result<()> {
    fn name(n: &str, pending: &HashSet<String>) -> Result<()> {
        if pending.contains(n) {
            bail!("array `{n}` is used before ALLOCATE");
        }
        Ok(())
    }
    fn expr(e: &Expr, pending: &HashSet<String>) -> Result<()> {
        match e {
            Expr::Call(n,_) if matches!(n.as_str(),"allocated"|"len"|"kind"|"present")=>{},
            Expr::Ident(n) => name(n, pending)?,
            Expr::Call(n, args) | Expr::Index(n, args) => {
                name(n, pending)?;
                for a in args {
                    expr(a, pending)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expr(a, pending)?;
                expr(b, pending)?;
            }
            Expr::Un(_, a) => expr(a, pending)?,
            Expr::Section {
                name: n,
                lower,
                upper,
                stride,
            } => {
                name(n, pending)?;
                for a in [lower, upper, stride].into_iter().flatten() {
                    expr(a, pending)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    match s {
        Stmt::Assign { name: n, value } => {
            name(n, pending)?;
            expr(value, pending)?;
        }
        Stmt::AssignIndex {
            name: n,
            indices,
            value,
        } => {
            name(n, pending)?;
            for e in indices {
                expr(e, pending)?;
            }
            expr(value, pending)?;
        }
        Stmt::CallSub { name: n, .. } if n == "@allocate" || n=="@deallocate" => {}
        Stmt::CallSub { args, .. } | Stmt::Read { args } | Stmt::Print { items: args } => {
            for e in args {
                expr(e, pending)?;
            }
        }
        Stmt::Return(Some(e)) => expr(e, pending)?,
        Stmt::ArrayDecl { dims, .. } => {
            for e in dims {
                expr(e, pending)?;
            }
        }
        Stmt::SelectCase {
            expr: e,
            cases,
            default,
        } => {
            expr(e, pending)?;
            for c in cases {
                for s in &c.body {
                    check_allocated(s, pending)?;
                }
            }
            if let Some(b) = default {
                for s in b {
                    check_allocated(s, pending)?;
                }
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr(cond, pending)?;
            for s in then_body {
                check_allocated(s, pending)?;
            }
            if let Some(b) = else_body {
                for s in b {
                    check_allocated(s, pending)?;
                }
            }
        }
        Stmt::Do {
            start, end, body, ..
        } => {
            expr(start, pending)?;
            expr(end, pending)?;
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        Stmt::DoWhile { cond, body } => {
            expr(cond, pending)?;
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        Stmt::Block { body } => {
            for s in body {
                check_allocated(s, pending)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn stable(body: &[Stmt]) -> HashSet<String> {
    fn scan(body: &[Stmt], counts: &mut HashMap<String, usize>, nested: bool) {
        for s in body {
            match s {
                Stmt::Assign { name, .. } => {
                    *counts.entry(name.clone()).or_default() += if nested { 2 } else { 1 }
                }
                Stmt::Do { var, body, .. } => {
                    counts.insert(var.clone(), 2);
                    scan(body, counts, true);
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    scan(then_body, counts, true);
                    if let Some(b) = else_body {
                        scan(b, counts, true);
                    }
                }
                Stmt::DoWhile { body, .. } | Stmt::Block { body } => scan(body, counts, true),
                Stmt::CallSub { name, .. } if name == "@forall" => {}
                Stmt::CallSub { args, .. } | Stmt::Read { args } => {
                    for a in args {
                        if let Expr::Ident(n) = a {
                            counts.insert(n.clone(), 2);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut counts = HashMap::new();
    scan(body, &mut counts, false);
    // ALLOCATE/DEALLOCATE update these values in a later lowering pass.
    // Their declaration-time initializers do not make them constants.
    for statement in body {
        if let Stmt::DummyIntent {intent,..}=statement {
            if let Some(rest)=intent.strip_prefix("@bounded_shape:").or_else(||intent.strip_prefix("@bounded_lower:")) {for axis in rest.split(':') {counts.remove(axis);}}
            if let Some(rest)=intent.strip_prefix("@bounded:") {
                if let Some((length,flag))=rest.split_once(':') {
                    counts.remove(length);
                    counts.remove(flag);
                }
            }
        }
    }
    counts
        .into_iter()
        .filter_map(|(n, c)| (c == 1).then_some(n))
        .collect()
}
fn validate_input_writes(body: &[Stmt], inputs: &HashSet<String>) -> Result<()> {
    for s in body {
        match s {
            Stmt::Assign { name, .. }
            | Stmt::AssignIndex { name, .. }
            | Stmt::Do { var: name, .. }
                if inputs.contains(name) =>
            {
                bail!("cannot modify INTENT(IN) dummy `{name}`")
            }
            Stmt::CallSub {name,args} if name=="@allocate" && args.iter().any(|e|matches!(e,Expr::Call(n,_) if inputs.contains(n))) => bail!("cannot allocate an INTENT(IN) dummy"),
            Stmt::CallSub {name,args} if name=="@deallocate" && args.iter().any(|e|matches!(e,Expr::Ident(n) if inputs.contains(n))) => bail!("cannot deallocate an INTENT(IN) dummy"),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                validate_input_writes(then_body, inputs)?;
                if let Some(b) = else_body {
                    validate_input_writes(b, inputs)?;
                }
            }
            Stmt::Block {body}=>{
                let mut local=inputs.clone();
                for declaration in body {match declaration {
                    Stmt::VarDecl {names,..}=>for name in names {local.remove(name);},
                    Stmt::ArrayDecl {name,..}=>{local.remove(name);},_=>{},
                }}
                validate_input_writes(body,&local)?;
            }
            Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                validate_input_writes(body, inputs)?
            }
            _ => {}
        }
    }
    Ok(())
}
// CHARACTER constructors need CHARACTER storage before array-dummy specialization;
// the numeric reduction materializer cannot provide that storage.
pub(crate) fn character_constructor(actual:&mut Expr, shapes:&Shapes, prefix:&mut Vec<Stmt>, serial:&mut usize)->Result<Option<(TypeSpec,i64,Expr)>> {
    if matches!(actual,Expr::Call(n,_) if matches!(n.as_str(),"reshape"|"transpose")) {
        let (kind,items)=crate::character_elementals::Lower::fixed_items(actual,shapes)?;
        *actual=if items.is_empty() {crate::empty_constructors::parsed(kind,items)} else {Expr::Call("@array_constructor".into(),items)};
    }
    if let Expr::Call(tag,args)=actual {
        if tag=="@packed_view" && args.len()==2 {
            if let Expr::Ident(n)=&args[0] {
                if let Some((kind,dims))=shapes.get(n) {
                    if matches!(kind,TypeSpec::Character(_)) && dims.len()==1 {
                        return Ok(Some((kind.clone(),crate::parameters::integer_constant(&dims[0])?,args[1].clone())));
                    }
                }
            }
        }
        if tag=="pack" && args.len()==3 {
            let mut local=shapes.clone();
            let mut source=args[0].clone();
            let (kind,capacity,extent)=if let Expr::Ident(n)=&source {
                let (kind,dims)=local.get(n).context("undeclared PACK ARRAY")?;
                if dims.len()!=1 || !matches!(kind,TypeSpec::Character(_)) {return Ok(None);}
                (kind.clone(),crate::parameters::integer_constant(&dims[0])?,crate::allocatables::length(n,&local).unwrap_or(dims[0].clone()))
            } else {character_constructor(&mut source,&local,prefix,serial)?.context("CHARACTER PACK ARRAY requires bounded rank-one storage")?};
            for s in prefix.iter() {if let Stmt::ArrayDecl{kind,name,dims}=s {local.insert(name.clone(),(kind.clone(),dims.clone()));}}
            let mut mask=args[1].clone();
            let (mask_value,mask_count,mask_kind)=crate::reductions::elemental_element(&mask,&Expr::IntLit("1".into()),&local)?;
            if mask_kind!=TypeSpec::Logical {bail!("PACK MASK must be LOGICAL");}
            let mask_extent=if mask_count.is_some() {
                let (_,_,length)=crate::reductions::argument(&mut mask,&local,prefix,serial)?;
                Some(length)
            } else {mask=mask_value;None};
            for s in prefix.iter() {match s {
                Stmt::ArrayDecl{kind,name,dims}=>{local.insert(name.clone(),(kind.clone(),dims.clone()));},
                Stmt::VarDecl{kind,names}=>for n in names {local.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
            }}
            let mut vector=args[2].clone();
            let has_vector=!matches!(&vector,Expr::Call(n,_) if n=="@no_vector");
            let vector_info=if has_vector {
                let info=if let Expr::Ident(n)=&vector {
                    let (k,d)=local.get(n).context("undeclared PACK VECTOR")?;
                    if d.len()!=1 {bail!("PACK VECTOR must be rank one");}
                    (k.clone(),crate::parameters::integer_constant(&d[0])?,crate::allocatables::length(n,&local).unwrap_or(d[0].clone()))
                } else {character_constructor(&mut vector,&local,prefix,serial)?.context("CHARACTER PACK VECTOR requires bounded storage")?};
                if info.0!=kind {bail!("PACK VECTOR type and CHARACTER length must match ARRAY");}
                Some(info)
            } else {None};
            *serial+=1;
            let temp=format!("@call_character_pack_{serial}");
            let index=format!("@call_character_pack_index_{serial}");
            let cursor=format!("@call_character_pack_length_{serial}");
            let id=|n:&str|Expr::Ident(n.into());
            let int=|n:i64|Expr::IntLit(n.to_string());
            let bin=|op,a,b|Expr::Bin(op,Box::new(a),Box::new(b));
            let capacity=vector_info.as_ref().map_or(capacity,|v|v.1);
            prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(capacity)]});
            prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone(),cursor.clone()]});
            prefix.push(Stmt::Assign{name:cursor.clone(),value:int(0)});
            if mask_count.is_none() {
                let scalar=format!("@call_character_pack_mask_{serial}");
                prefix.push(Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![scalar.clone()]});
                prefix.push(Stmt::Assign{name:scalar.clone(),value:mask});
                mask=id(&scalar);
            }
            if let Some(length)=mask_extent {prefix.push(Stmt::If{cond:bin(crate::ast::BinOp::Ne,extent.clone(),length),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("PACK MASK shape must conform to ARRAY".into())]}],else_body:None});}
            let condition=if mask_count.is_some() {crate::reductions::argument_element(&mask,&id(&index),&local)?} else {mask};
            let Expr::Ident(source)=source else {bail!("CHARACTER PACK ARRAY must resolve to array storage");};
            let mut selected=vec![Stmt::Assign{name:cursor.clone(),value:bin(crate::ast::BinOp::Add,id(&cursor),int(1))}];
            if let Some((_,_,length))=&vector_info {selected.push(Stmt::If{cond:bin(crate::ast::BinOp::Gt,id(&cursor),length.clone()),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("PACK VECTOR has too few elements for MASK".into())]}],else_body:None});}
            selected.push(Stmt::AssignIndex{name:temp.clone(),indices:vec![id(&cursor)],value:Expr::Index(source,vec![id(&index)])});
            prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:extent,body:vec![Stmt::If{cond:condition,then_body:selected,else_body:None}]});
            let length=if let Some((_,_,length))=vector_info {
                let Expr::Ident(vector)=vector else {bail!("CHARACTER PACK VECTOR must resolve to array storage");};
                prefix.push(Stmt::Do{var:index.clone(),start:bin(crate::ast::BinOp::Add,id(&cursor),int(1)),end:length.clone(),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![id(&index)],value:Expr::Index(vector,vec![id(&index)])}]});
                length
            } else {id(&cursor)};
            *actual=Expr::Call("@packed_view".into(),vec![id(&temp),length.clone()]);
            return Ok(Some((kind,capacity,length)));
        }
    }
    let empty=crate::empty_constructors::kind(actual)?;
    let mut section_kind=None;
    let values=match actual {
        Expr::Section{name,lower,upper,stride}=>{
            let (kind,dims)=shapes.get(name).context("undeclared CHARACTER section actual")?;
            if !matches!(kind,TypeSpec::Character(_)) || dims.len()!=1 {return Ok(None);}
            let extent=crate::parameters::integer_constant(&dims[0])?;
            let step=stride.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(1);
            if step==0 {bail!("CHARACTER section stride must not be zero");}
            let first=lower.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(if step>0 {1} else {extent});
            let last=upper.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(if step>0 {extent} else {1});
            let distance=(i128::from(last)-i128::from(first))*i128::from(step.signum());
            let count=if distance<0 {0} else {distance/i128::from(step).abs()+1};
            if count>1024 {bail!("CHARACTER section actuals support at most 1024 elements");}
            if count>0 && (first<1 || first>extent || last<1 || last>extent) {bail!("CHARACTER section actual is outside the array bounds");}
            section_kind=Some(kind.clone());
            (0..count).map(|i|Expr::Index(name.clone(),vec![Expr::IntLit((i128::from(first)+i*i128::from(step)).to_string())])).collect()
        },
        Expr::Call(tag,items) if tag=="@array_constructor"=>items.clone(),
        _ if empty.as_ref().is_some_and(|k|matches!(k,TypeSpec::Character(_)))=>vec![],
        _=>return Ok(None),
    };
    let kind=if let Some(value)=values.first() {scalar_actual_kind(value,shapes)?} else {empty.or(section_kind).context("empty CHARACTER constructor requires an explicit type")?};
    if !matches!(kind,TypeSpec::Character(_)) {bail!("CHARACTER array actual requires CHARACTER constructor elements");}
    for value in &values {
        if matches!(value,Expr::Ident(n) if shapes.get(n).is_some_and(|(_,dims)|!dims.is_empty())) {bail!("CHARACTER array actual constructor elements must currently be scalar");}
        if scalar_actual_kind(value,shapes)?!=kind {bail!("CHARACTER constructor elements must have equal lengths");}
    }
    if values.len()>1024 {bail!("CHARACTER array actual constructors support at most 1024 elements");}
    *serial+=1;let temp=format!("@call_character_array_{serial}");let count=values.len() as i64;
    prefix.push(Stmt::ArrayDecl {kind:kind.clone(),name:temp.clone(),dims:vec![Expr::IntLit(count.to_string())]});
    for (i,value) in values.into_iter().enumerate() {prefix.push(Stmt::AssignIndex {name:temp.clone(),indices:vec![Expr::IntLit((i+1).to_string())],value});}
    *actual=Expr::Ident(temp);
    Ok(Some((kind,count,Expr::IntLit(count.to_string()))))
}

fn expand(
    body: &mut Vec<Stmt>,
    inherited: &Shapes,
    routines: &HashMap<String, Routine>,
    functions: &HashMap<String,TypeSpec>,
    active: &mut Vec<String>,
    serial: &mut usize,
) -> Result<()> {
    let statement_location = std::cell::Cell::new(None);
    let result = (|| -> Result<()> {
    let mut inherited=if body.iter().any(|s|matches!(s,Stmt::DummyIntent{intent,..} if intent=="@native_recursive")) {Shapes::new()} else {inherited.clone()};
    for s in body.iter() {
        if let Stmt::DummyIntent{name,intent}=s {
            if matches!(intent.as_str(),"pointer"|"target") {inherited.insert(format!("@{intent}:{name}"),(TypeSpec::Logical,vec![]));}
            if intent.starts_with("@borrowed_descriptor:") {inherited.insert(format!("@borrowed_pointer:{name}"),(TypeSpec::Logical,vec![]));}
        }
    }
    if body
        .iter()
        .any(|s| matches!(s, Stmt::DummyIntent { intent, .. } if intent == "allocatable"))
    {
        let stable = stable(body);
        specialize(
            body,
            &mut inherited.clone(),
            &HashSet::new(),
            &mut HashMap::new(),
            &stable,
        )?;
    }
    let readonly: HashSet<_> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::DummyIntent { name, intent } if matches!(intent.as_str(),"parameter"|"intent(in)"|"@heap_readonly") => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut shapes = inherited.clone();
    for s in body.iter() {
        match s {
            Stmt::ArrayDecl { name, kind, dims } => {
                shapes.insert(name.clone(), (kind.clone(), dims.clone()));
            }
            Stmt::VarDecl { names, kind } => {
                for n in names {
                    shapes.insert(n.clone(), (kind.clone(), vec![]));
                }
            }
            _ => {}
        }
    }
    crate::allocatables::metadata(body,&mut shapes);
    let procedures=routines.iter().map(|(n,r)| (n.clone(),r.params.iter().map(|p|r.body.iter().find_map(|s|match s {Stmt::DummyIntent{name,intent} if name==p && intent.starts_with("intent(")=>Some(intent.clone()),_=>None}).unwrap_or_default()).collect())).collect();
    crate::vector_subscripts::specialize(body,&shapes,procedures,serial)?;
    for s in body.iter() {match s {
        Stmt::ArrayDecl{kind,name,dims}=>{shapes.insert(name.clone(),(kind.clone(),dims.clone()));},
        Stmt::VarDecl{kind,names}=>for n in names {shapes.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
    }}
    crate::allocatables::metadata(body,&mut shapes);
    for s in body.iter() {
        if let Stmt::DummyIntent {name,intent}=s {
            if matches!(intent.as_str(),"pointer"|"target") {shapes.insert(format!("@{intent}:{name}"),(TypeSpec::Logical,vec![]));}
        }
    }
    for s in body.iter_mut() {
        if let Some(p) = crate::source_diagnostics::statement_offset(&s) { statement_location.set(Some(p)); }
        if let Stmt::CallSub { name, args } = s {
            if let Some(r) = routines.get(name) {

                if r.elemental { crate::elemental::validate_subroutine(name, &r.params, &r.body)?; }
                if active.contains(name) {
                    bail!("recursive specialized subroutine calls are not yet supported");
                }
                if r.elemental {
                    if let Some(mut expanded)=elemental_array_call(name,args,r,&shapes,&readonly,serial,functions)? {
                        let Stmt::Block {body}=&mut expanded else {unreachable!()};
                        expand(body,&shapes,routines,functions,active,serial)?;
                        *s=expanded;
                        continue;
                    }
                }
                if args.len() > r.params.len() {
                    bail!("too many arguments to `{name}`");
                }
                let optional: HashSet<_> = r
                    .body
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::DummyIntent { name, intent } if intent == "optional" => {
                            Some(name.clone())
                        }
                        _ => None,
                    })
                    .collect();
                let intents: HashMap<_, _> = r
                    .body
                    .iter()
                    .filter_map(|s| match s {
                        Stmt::DummyIntent { name, intent } if intent.starts_with("intent(") => {
                            Some((name.clone(), intent.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                validate_input_writes(
                    &r.body,
                    &intents
                        .iter()
                        .filter_map(|(n, i)| (i == "intent(in)").then_some(n.clone()))
                        .collect(),
                )?;
                let mut local = shapes.clone();
                for p in &r.params {for key in ["@pointer","@target","@heap","@extent","@allocated","@axes","@lowers","@axis_limits"] {local.remove(&format!("{key}:{p}"));}}
                for d in &r.body {
                    match d {
                        Stmt::VarDecl { kind, names } => {
                            for n in names {
                                local.insert(n.clone(), (kind.clone(), vec![]));
                            }
                        }
                        Stmt::ArrayDecl { kind, name, dims } => {
                            local.insert(name.clone(), (kind.clone(), dims.clone()));
                        }
                        Stmt::DummyIntent {name,intent} if matches!(intent.as_str(),"pointer"|"target") => {local.insert(format!("@{intent}:{name}"),(TypeSpec::Logical,vec![]));}
                        _ => {}
                    }
                }
                for p in &optional {
                    if !r.params.contains(p) { bail!("OPTIONAL attribute requires a dummy argument `{p}`"); }
                    local.insert(format!("@optional:{p}"),(TypeSpec::Logical,vec![]));
                }
                let mut names = HashMap::new();
                let mut borrowed_widths=HashSet::new();
                let mut absent = HashSet::new();
                let mut prefix = vec![];
                let mut undefine = vec![];
                let mut shape_checks = Vec::new();
                for (i, p) in r.params.iter().enumerate() {
                    local.remove(&format!("@extent:{p}"));
                    local.remove(&format!("@allocated:{p}"));
                    let Some(actual) = args.get(i) else {
                        if !optional.contains(p) {
                            bail!("missing required argument `{p}` of `{name}`");
                        }
                        absent.insert(p.clone());
                        continue;
                    };
                    let (mut kind, dims) = local
                        .get(p)
                        .context("dummy argument must be declared")?
                        .clone();
                    let assumed_character=dims.is_empty() && matches!(kind,TypeSpec::CharacterAssumed);
                    if r.elemental || (dims.is_empty() && matches!(kind,TypeSpec::Character(_) | TypeSpec::CharacterAssumed)) {
                        let actual_kind = scalar_actual_kind(actual, &shapes)?;
                        if let TypeSpec::Character(length) = actual_kind {
                            match kind {
                                TypeSpec::CharacterAssumed => kind = TypeSpec::Character(length),
                                TypeSpec::Character(expected) if expected.unwrap_or(1) == length.unwrap_or(1) => {},
                                _ => bail!("type or CHARACTER length mismatch for dummy `{p}`"),
                            }
                        } else if !matches!((&kind,&actual_kind),(TypeSpec::Integer(a),TypeSpec::Integer(b)) if a.unwrap_or(4)==b.unwrap_or(4)) && kind != actual_kind {
                            bail!("type mismatch for dummy `{p}`");
                        }
                    }
                    local.insert(p.clone(),(kind.clone(),dims.clone()));
                    if assumed_character || !dims.is_empty() && matches!(kind,TypeSpec::CharacterAssumed) {
                        if let Expr::Ident(actual)=actual {
                            if let Some(value)=shapes.get(&format!("@character_spec_length:{actual}")) {
                                local.insert(format!("@character_spec_length:{p}"),value.clone());
                            }
                        }
                    }
                    let intent = intents.get(p).map(String::as_str).unwrap_or("");
                    if local.contains_key(&format!("@pointer:{p}")) {
                        let Expr::Ident(a)=actual else {bail!("POINTER dummy `{p}` requires a POINTER variable");};
                        if !shapes.contains_key(&format!("@pointer:{a}")) {bail!("POINTER dummy `{p}` requires a POINTER actual");}
                        let (actual_kind,actual_dims)=shapes.get(a).context("undeclared POINTER actual")?;
                        if !crate::pointers::same_kind(actual_kind,&kind) || actual_dims.len()!=dims.len() {bail!("type or rank mismatch for POINTER dummy `{p}`");}
                        if intent=="intent(in)" {bail!("INTENT(IN) POINTER dummy arguments are not yet supported");}
                        names.insert(p.clone(),a.clone());
                        if intent=="intent(out)" {undefine.push(Stmt::CallSub{name:"@nullify".into(),args:vec![actual.clone()]});}
                        continue;
                    }
                    if matches!(actual,Expr::Ident(a) if shapes.contains_key(&format!("@pointer:{a}"))) {bail!("POINTER actuals currently require POINTER dummy arguments");}
                    let pointer_target=local.contains_key(&format!("@target:{p}")) && r.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if intent=="pointer" && r.params.contains(name)));
                    if pointer_target && !matches!(actual,Expr::Ident(a) if shapes.contains_key(&format!("@target:{a}"))) {bail!("escaping POINTER association requires a TARGET actual for `{p}`");}
                    if intent != "intent(in)"
                        && matches!(actual,Expr::Ident(n) if readonly.contains(n))
                    {
                        bail!("cannot pass PARAMETER or INTENT(IN) variable as writable argument `{p}`");
                    }
                    if !dims.is_empty() {
                        let allocatable=r.body.iter().any(|s|matches!(s,Stmt::DummyIntent {name,intent} if name==p && intent=="allocatable"));
                        if allocatable {
                            if let Expr::Ident(a)=actual {
                                if let (Some((_,formal)),Some((_,actual_width)))=(local.get(&format!("@character_spec_length:{p}")),shapes.get(&format!("@character_spec_length:{a}"))) {
                                    if let (Some(Expr::Ident(formal)),Some(Expr::Ident(actual_width)))=(formal.first(),actual_width.first()) {
                                        names.insert(formal.clone(),format!("{}{actual_width}",crate::heap::BORROWED_FIELD));
                                        borrowed_widths.insert(formal.clone());
                                    }
                                }
                            }
                        }
                        if allocatable && !matches!(actual,Expr::Ident(_)) {bail!("ALLOCATABLE dummy `{p}` requires an ALLOCATABLE variable");}
                        let mut captured=actual.clone();
                        // This is an optional rank hint, not expression validation.
                        // Runtime sections and generated selectors are checked by
                        // the materializer below and need not have a fixed shape.
                        let expression_shape=crate::multidim::fixed_actual_shape(actual,&shapes).ok().flatten();
                        let (actual_kind,actual_dims,length) = if let Expr::Ident(a)=actual {
                            let (k,d)=shapes.get(a).context("undeclared array argument")?;
                            (k.clone(),d.clone(),crate::allocatables::length(a,&shapes))
                        } else {
                            if intent!="intent(in)" {bail!("writable array dummy `{p}` requires a whole-array actual argument");}
                            if crate::constructors::delayed(&captured) {
                                bail!("runtime-bound array constructor arguments are not yet supported for `{p}`");
                            }
                            let start=prefix.len();
                            let (k,capacity,length)=if matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterAssumed) {
                                character_constructor(&mut captured,&shapes,&mut prefix,serial)?.context("CHARACTER array dummy currently requires a whole array or scalar-element constructor")?
                            } else {
                                // Expose active runtime extents before materializing
                                // expressions; heap backing dimensions are placeholders.
                                fn views(e: &mut Expr, shapes: &Shapes) -> Result<()> {
                                    if let Expr::Call(n,args)=e {
                                        if matches!(n.as_str(),"size"|"shape"|"lbound"|"ubound"|"allocated"|"kind"|"len") {
                                            if let Some(value)=crate::allocatables::inquiry(n,args,shapes)? {*e=value;return Ok(());}
                                        }
                                    }
                                    match e {
                                        Expr::Ident(n) if crate::allocatables::length(n,shapes).is_some()=>*e=crate::allocatables::view(n,shapes),
                                        Expr::Call(n,_) if n=="@packed_view"=>{},
                                        Expr::Call(_,args)|Expr::Index(_,args)=>for arg in args {views(arg,shapes)?;},
                                        Expr::Bin(_,a,b)=>{views(a,shapes)?;views(b,shapes)?;},
                                        Expr::Un(_,a)=>views(a,shapes)?,_=>{}
                                    }
                                    Ok(())
                                }
                                views(&mut captured,&shapes)?;
                                crate::reductions::argument_with_elementals(&mut captured,&shapes,&mut prefix,serial,functions)?
                            };
                            for s in &prefix[start..] {match s {
                                Stmt::ArrayDecl {kind,name,dims}=>{shapes.insert(name.clone(),(kind.clone(),dims.clone()));},
                                Stmt::VarDecl {kind,names}=>for n in names {shapes.insert(n.clone(),(kind.clone(),vec![]));},_=>{}
                            }}
                            crate::allocatables::metadata(&prefix[start..],&mut shapes);
                            if let Expr::Call(tag,values)=&captured {
                                if tag=="@packed_view" && matches!(k,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical) && crate::parameters::integer_constant(&length).is_err()
                                    && matches!(&values[0],Expr::Ident(n) if !shapes.contains_key(&format!("@heap:{n}"))) {
                                    // A bounded expression buffer has capacity,
                                    // not the active argument shape. Give the
                                    // actual its own descriptor before binding
                                    // a callee which may forward it again.
                                    *serial+=1;let temp=format!("@array_call_view_{serial}");let index=format!("{temp}_index");
                                    let element=crate::reductions::argument_element(&captured,&Expr::Ident(index.clone()),&shapes)?;
                                    crate::heap::unallocated_result(k.clone(),temp.clone(),1,&mut prefix,&mut shapes)?;
                                    prefix.push(Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(temp.clone(),vec![length.clone()])]});
                                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                                    prefix.push(Stmt::Do{var:index.clone(),start:Expr::IntLit("1".into()),end:length.clone(),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![Expr::Ident(index)],value:element}]});
                                    captured=Expr::Ident(temp.clone());
                                    (k,shapes[&temp].1.clone(),crate::allocatables::length(&temp,&shapes))
                                } else {
                                    let length=if crate::parameters::integer_constant(&length).ok()==Some(capacity) {None} else {Some(length)};
                                    (k,vec![Expr::IntLit(capacity.to_string())],length)
                                }
                            } else {
                            // Statically sized expressions need no runtime view descriptor.
                            let length=if crate::parameters::integer_constant(&length).ok()==Some(capacity) {None} else {Some(length)};
                            (k,vec![Expr::IntLit(capacity.to_string())],length)
                            }
                        };
                        let a=match &captured {Expr::Ident(a)=>a.clone(),Expr::Call(n,v) if n=="@packed_view"=>if let Expr::Ident(a)=&v[0] {a.clone()} else {bail!("array view requires materialized storage");},_=>bail!("array dummy `{p}` requires a materialized array argument")};
                        if matches!(kind,TypeSpec::CharacterAssumed) {
                            if let Some(value)=shapes.get(&format!("@character_spec_length:{a}")) {local.insert(format!("@character_spec_length:{p}"),value.clone());}
                        }
                        let mut runtime_axes=runtime_actual_axes(actual,&shapes).or_else(||crate::allocatables::axes(&a,&shapes));
                        if runtime_axes.is_none() && actual_dims.len()==1 {
                            if let Some(shape)=expression_shape.filter(|s|s.len()>1) {
                                let mut axes=vec![];
                                for extent in shape {
                                    *serial+=1;let field=format!("@array_call_shape_{serial}");
                                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![field.clone()]});
                                    prefix.push(Stmt::Assign{name:field.clone(),value:Expr::IntLit(extent.to_string())});
                                    axes.push(Expr::Ident(field));
                                }
                                runtime_axes=Some(axes);
                            }
                        }
                        if !allocatable && matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical) && runtime_axes.is_none() && actual_dims.len()==1 {
                            if let Some(length)=&length {
                                *serial+=1;let extent=format!("@array_call_active_extent_{serial}");
                                prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![extent.clone()]});
                                prefix.push(Stmt::Assign{name:extent.clone(),value:length.clone()});
                                runtime_axes=Some(vec![Expr::Ident(extent)]);
                            }
                        }
                        if allocatable && runtime_axes.is_some() && !shapes.contains_key(&format!("@heap:{a}")) {bail!("numeric ALLOCATABLE dummy `{p}` requires a heap descriptor");}
                        if matches!(kind, TypeSpec::CharacterAssumed) && matches!(actual_kind, TypeSpec::Character(_)) {
                            kind = actual_kind.clone();
                        }
                        if !crate::pointers::same_kind(&actual_kind,&kind) || runtime_axes.as_ref().map_or(actual_dims.len(),Vec::len) != dims.len() {
                            bail!("type or rank mismatch for `{p}`");
                        }
                        if !dims
                            .iter()
                            .all(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent"))
                        {
                            if runtime_axes.is_none() && dims.iter().zip(&actual_dims).any(|(a, b)| {
                                crate::parameters::integer_constant(a)
                                    .ok()
                                    .zip(crate::parameters::integer_constant(b).ok())
                                    .is_some_and(|(a, b)| a != b)
                            }) {
                                bail!("explicit-shape dummy `{p}` currently requires an exactly matching extent");
                            }
                            let actual_axes=runtime_axes.as_ref().unwrap_or(&actual_dims);
                            for (formal,actual) in dims.iter().zip(actual_axes) {
                                shape_checks.push((p.clone(),formal.clone(),actual.clone()));
                            }
                        }
                        if let Some(axes)=runtime_axes {
                            if !allocatable {if let Some(flag)=crate::allocatables::allocated(&a,&shapes) {prefix.push(Stmt::If{cond:Expr::Un(crate::ast::UnOp::Not,Box::new(flag)),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str(format!("unallocated actual for `{p}`"))]}],else_body:None});}}
                            local.insert(format!("@axes:{p}"),(TypeSpec::Integer(Some(8)),axes));
                        }
                        local.insert(p.clone(), (kind, actual_dims.clone()));
                        if let Some(length)=length {local.insert(format!("@extent:{p}"),(TypeSpec::Integer(None),vec![length]));}
                        if allocatable {
                            let flag=crate::allocatables::allocated(&a,&shapes).context("ALLOCATABLE dummy requires an ALLOCATABLE actual")?;
                            local.insert(format!("@allocated:{p}"),(TypeSpec::Logical,vec![flag.clone()]));
                            if shapes.contains_key(&format!("@heap:{a}")) {
                                local.insert(format!("@heap:{p}"),(TypeSpec::Logical,vec![]));
                                if let Some(lower)=crate::allocatables::lowers(&a,&shapes){local.insert(format!("@lowers:{p}"),(TypeSpec::Integer(Some(8)),lower));}
                                crate::heap::borrow_fields(p,&mut local);
                            }
                            if intent=="intent(out)" {
                                if shapes.contains_key(&format!("@heap:{a}")) {
                                    undefine.push(Stmt::If{cond:flag,then_body:vec![Stmt::CallSub{name:"@deallocate".into(),args:vec![Expr::Ident(a.clone())]}],else_body:None});
                                }else{
                                    if let Expr::Ident(flag)=flag {undefine.push(Stmt::Assign {name:flag,value:Expr::Logical(false)});}
                                    if let Some(Expr::Ident(len))=crate::allocatables::length(&a,&shapes) {undefine.push(Stmt::Assign {name:len,value:Expr::IntLit("0".into())});}
                                }
                            }
                        }
                        names.insert(p.clone(), a.clone());
                        if !allocatable {
                            if let Some(flag)=crate::allocatables::allocated(&a,&shapes) {
                                prefix.push(Stmt::If {cond:Expr::Un(crate::ast::UnOp::Not,Box::new(flag)),then_body:vec![Stmt::CallSub {name:"@error_stop".into(),args:vec![Expr::Str(format!("unallocated array `{a}` passed as an input argument"))]}],else_body:None});
                            }
                        }
                        if intent == "intent(out)" {
                            undefine.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![actual.clone()],
                            });
                        }
                    } else if pointer_target {
                        let Expr::Ident(a)=actual else {bail!("pointer TARGET requires a variable");};
                        if shapes.get(a).is_none_or(|(k,d)| *k!=kind || !d.is_empty()) {bail!("type or rank mismatch for TARGET dummy `{p}`");}
                        names.insert(p.clone(),a.clone());
                    } else if intent == "intent(in)" {
                        *serial += 1;
                        let temp = format!("@array_call_input_{serial}");
                        prefix.push(Stmt::VarDecl {
                            kind,
                            names: vec![temp.clone()],
                        });
                        prefix.push(Stmt::Assign {
                            name: temp.clone(),
                            value: actual.clone(),
                        });
                        names.insert(p.clone(), temp);
                    } else {
                        let Expr::Ident(a) = actual else {
                            bail!("writable dummy `{p}` requires a variable");
                        };
                        names.insert(p.clone(), a.clone());
                        if intent == "intent(out)" {
                            undefine.push(Stmt::CallSub {
                                name: "@undefine".into(),
                                args: vec![actual.clone()],
                            });
                        }
                    }
                }
                // Bounds are in the dummy scope. Capture all scalar actuals
                // before evaluating them, including dummies listed after an array.
                for (param,mut formal,actual) in shape_checks {
                    crate::subroutines::expr(&mut formal,&names);
                    prefix.push(Stmt::If{cond:Expr::Bin(crate::ast::BinOp::Ne,Box::new(formal),Box::new(actual)),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str(format!("explicit-shape dummy `{param}` requires a matching actual shape"))]}],else_body:None});
                }
                prefix.extend(undefine);
                let mut cloned = r.body.clone();
                for p in &r.params {
                    if let Some((_,values))=local.get(&format!("@character_spec_length:{p}")) {
                        if let Some(Expr::Ident(length))=values.first() {cloned.push(Stmt::DummyIntent{name:p.clone(),intent:format!("@character_spec_length:{length}")});}
                    }
                    if intents.get(p).is_some_and(|i|i=="intent(in)") && local.get(p).is_some_and(|(_,d)|!d.is_empty()) {
                        cloned.push(Stmt::DummyIntent{name:p.clone(),intent:"@heap_readonly".into()});
                    }
                    if local.contains_key(&format!("@heap:{p}")) {
                        let field=|e:Expr|->Result<String>{if let Expr::Ident(n)=e{Ok(n)}else{bail!("ALLOCATABLE association requires writable descriptor fields")}};
                        let length=field(crate::allocatables::length(p,&local).context("missing dummy length")?)?;
                        let flag=field(crate::allocatables::allocated(p,&local).context("missing dummy status")?)?;
                        let axes=crate::allocatables::axes(p,&local).context("missing dummy axes")?;
                        let lower=crate::allocatables::lowers(p,&local).context("missing dummy lower bounds")?;
                        let count=axes.len();let axes=axes.into_iter().map(field).collect::<Result<Vec<_>>>()?;let lower=lower.into_iter().map(field).collect::<Result<Vec<_>>>()?;
                        if intents.get(p).is_some_and(|i|i=="intent(in)"){cloned.push(Stmt::DummyIntent{name:p.clone(),intent:"@heap_readonly".into()});}
                        cloned.extend([
                            Stmt::DummyIntent{name:p.clone(),intent:"@heap".into()},
                            Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded:{length}:{flag}")},
                            Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded_shape:{}",axes.join(":"))},
                            Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded_lower:{}",lower.join(":"))},
                            Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded_limits:{}",vec!["0";count].join(":"))},
                        ]);
                    }else if let Some(axes)=crate::allocatables::axes(p,&local) {
                        let axes=axes.iter().map(|e|if let Expr::Ident(n)=e {Ok(n.clone())}else{bail!("runtime dummy axes require descriptor fields")}).collect::<Result<Vec<_>>>()?;
                        cloned.push(Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded_shape:{}",axes.join(":"))});
                        let mut lowers=vec![];for _ in &axes {*serial+=1;let n=format!("@dummy_lower_{serial}");cloned.splice(0..0,vec![Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![n.clone()]},Stmt::Assign{name:n.clone(),value:Expr::IntLit("1".into())}]);lowers.push(n);}
                        cloned.push(Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded_lower:{}",lowers.join(":"))});
                        if matches!(local.get(p),Some((TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical,_))) {
                        if let (Some(Expr::Ident(length)),Some(actual))=(crate::allocatables::length(p,&local),names.get(p)) {
                            if let Some(Expr::Ident(flag))=crate::allocatables::allocated(actual,&shapes) {
                                let borrowed=crate::heap::BORROWED_FIELD;
                                cloned.push(Stmt::DummyIntent{name:p.clone(),intent:format!("@bounded:{borrowed}{length}:{borrowed}{flag}")});
                            }
                        }
                        }
                    }

                    if local.contains_key(&format!("@target:{p}")) && intents.get(p).is_some_and(|i|i=="intent(in)") {
                        cloned.push(Stmt::DummyIntent{name:p.clone(),intent:"@pointer_readonly_target".into()});
                    }
                }
                cloned.retain(|s|!matches!(s,Stmt::DummyIntent{name,intent} if intent=="target" && r.params.contains(name)));
                for declaration in &mut cloned {
                    if let Stmt::VarDecl {names,..} = declaration { names.retain(|n| !r.params.contains(n) && !borrowed_widths.contains(n)); }
                }
                cloned.retain(|s| !matches!(s,Stmt::ArrayDecl{name,..} if r.params.contains(name))
                    && !matches!(s,Stmt::VarDecl {names,..} if names.is_empty()));
                let stable = stable(&cloned);
                specialize(
                    &mut cloned,
                    &mut local,
                    &absent,
                    &mut HashMap::new(),
                    &stable,
                )?;
                crate::subroutines::rename(&mut cloned, &names, &r.params, serial)?;
                let mut call_shapes = shapes.clone();
                for statement in &prefix {
                    if let Stmt::VarDecl { kind, names } = statement {
                        for name in names { call_shapes.insert(name.clone(), (kind.clone(), vec![])); }
                    }
                }
                active.push(name.clone());
                expand(&mut cloned, &call_shapes, routines, functions, active, serial)?;
                active.pop();
                crate::subroutines::inline_returns(&mut cloned,serial)?;
                // Dummy bounds belong to the callee scope. In particular they
                // must not rebase array temporaries while their actual values
                // are still being materialized in the association prefix.
                prefix.push(Stmt::Block { body: cloned });
                *s = Stmt::Block { body: prefix };
                continue;
            }
        }
        match s {
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => expand(body, &shapes, routines, functions, active, serial)?,
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                expand(then_body, &shapes, routines, functions, active, serial)?;
                if let Some(b) = else_body {
                    expand(b, &shapes, routines, functions, active, serial)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
    })();
    result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
}
pub fn resolve(mut p: Program) -> Result<Program> {
    p=crate::input_items::prepare(p)?;
    p=crate::modules::capture(p)?;
    p=crate::heap::automatic_locals(p)?;
    p=crate::heap::prepare(p)?;
    p=crate::modules::prepare_globals(p)?;
    p=crate::internal_procedures::resolve(p)?;
    p=crate::optional::resolve(p)?;
    p=crate::recursive_arrays::resolve(p)?;
    let (prepared,mut function_lower)=crate::character_elementals::prepare(p)?;
    p=prepared;
    p=crate::output::prepare_formatted(p)?;
    p=crate::constructor_expressions::resolve(p)?;
    p = crate::pointers::prepare(p)?;
    p = crate::constructors::prepare_arguments(p)?;
    p = crate::allocatables::prepare(p)?;
    p = crate::modules::bind_descriptors(p)?;
    p = crate::vector_subscripts::resolve(p)?;
    p = crate::array_bounds::resolve(p)?;
    crate::elemental::validate_program(&p)?;
    fn collect(body: &mut Vec<Stmt>, routines: &mut HashMap<String, Routine>) {
        body.retain_mut(|s| match s {
            Stmt::Subroutine {
                name, params, body, elemental, ..
            } if *elemental || body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if intent=="pointer" && params.contains(name))) || body
                .iter()
                .any(|s| matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name))
                    || matches!(s,Stmt::VarDecl {kind:TypeSpec::Character(_) | TypeSpec::CharacterAssumed,names} if names.iter().any(|n| params.contains(n)))) =>
            {
                routines.insert(
                    name.clone(),
                    Routine {
                        elemental: *elemental,
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                false
            }
            Stmt::Module { body, .. } => {
                collect(body, routines);
                true
            }
            _ => true,
        });
    }
    let functions=crate::elemental::functions(&p);
    let mut routines = HashMap::new();
    collect(&mut p.body, &mut routines);
    let mut call_serial=0;
    expand(&mut p.body, &Shapes::new(), &routines, &functions, &mut vec![], &mut call_serial)?;
    // Expanding a descriptor-backed function can expose further function calls
    // in its helper body. Alternate the two passes until those calls are bound.
    for pass in 0..128 {
        p=function_lower.finish(p)?;
        p=crate::constructors::prepare_arguments(p)?;
        expand(&mut p.body, &Shapes::new(), &routines, &functions, &mut vec![], &mut call_serial)?;
        if !function_lower.contains_calls(&p.body) {break;}
        if pass==127 {bail!("array/function call specialization did not converge after 128 passes");}
    }
    // Writable assumed-length CHARACTER dummies have concrete storage only
    // after call-site specialization. Capture their internal output lists now.
    p = crate::output::prepare_formatted(p)?;
    p = crate::pointers::resolve(p)?;
    p = crate::modules::bind_descriptors(p)?;
    p = crate::modules::finish_bounds(p)?;
    p = crate::vector_subscripts::finish(p)?;
    p = crate::allocatables::finish(p)?;
    p = crate::output::prepare_character_views(p)?;
    p = crate::constructors::resolve(p)?;
    p = crate::multidim::prepare_character_storage(p)?;
    let p = crate::heap_characters::resolve(p)?;
    let p = crate::character_arrays::resolve(p)?;
    crate::multidim::resolve(crate::function_arrays::resolve(crate::substrings::resolve(p)?)?)
}

#[cfg(test)]
mod tests {
    fn lower(source: &str) -> anyhow::Result<crate::ast::Program> {
        let tokens = crate::lexer::lex(source);
        let p = crate::parser::parse_with_src(source, &tokens, "array_calls.f90").unwrap();
        crate::reductions::resolve(super::resolve(crate::parameters::resolve(p)?)?)
    }
    #[test]
    fn rejects_unsupported_or_invalid_array_calls() {
        for (actual,decl,dummy,body,error) in [
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:),c(:)","b=0.0d0","missing required"),
            ("a","integer :: a(2)","real(8),intent(out)::b(:)","b=0.0d0","type or rank"),
            ("a","real(8) :: a(2)","real(8),intent(in)::b(:)","b=0.0d0","INTENT(IN)"),
            ("a","real(8),parameter::a(2)=[1.0d0,2.0d0]","real(8),intent(out)::b(:)","b=0.0d0","PARAMETER"),
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:)","real(8)::scratch(:)\nallocate(scratch(2))\nb=0.0d0","ALLOCATABLE"),
            ("a","real(8) :: a(2)","real(8),intent(out)::b(:)","real(8),allocatable::scratch(:)\nallocate(scratch(2))\nallocate(scratch(2))\nb=0.0d0","repeated ALLOCATE"),
        ] {
            let params=if dummy.contains(",c(:)") {"b,c"} else {"b"};
            let source=format!("program main\n{decl}\ncall f({actual})\ncontains\nsubroutine f({params})\n{dummy}\n{body}\nend subroutine f\nend program main");
            let err=lower(&source).unwrap_err();assert!(format!("{err:#}").contains(error),"{err:#}");
        }
    }
    #[test]
    fn mixture_and_array_features_lower_and_check_initialization() {
        for source in [
            include_str!("../tests/47_normal_mixture.f90"),
            include_str!("../tests/48_array_calls_expressions.f90"),
        ] {
            let p = lower(source).unwrap();
            crate::initialized::instrument(p).unwrap();
        }
    }
}

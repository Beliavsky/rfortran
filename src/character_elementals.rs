// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize array/CHARACTER functions at argument association, independently
//! of PURE. Bounded array storage retains ranks and column-major element order.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
type Symbols = HashMap<String, (TypeSpec, Vec<Expr>)>;
#[derive(Clone)]
struct Routine {
    params: Vec<String>,
    elemental: bool,
    body: Vec<Stmt>,
    result: String,
    kind: TypeSpec,
    descriptor_call: Option<String>,
}
#[derive(Clone)]
struct Value {
    active:Option<Expr>,
    kind: TypeSpec,
    items: Option<Vec<Expr>>,
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn array_count(dims:&[i64])->Result<i64> {
    if dims.is_empty() || dims.len()>15 {bail!("function array rank must be between 1 and 15");}
    dims.iter().try_fold(1i64,|n,d| {
        n.checked_mul((*d).max(0)).context("function array element count overflow")
    })
}
fn array_indices(mut ordinal:usize,dims:&[i64])->Vec<Expr> {
    dims.iter().map(|d| {let i=ordinal%(*d as usize);ordinal/=*d as usize;int(i as i64+1)}).collect()
}
fn length(kind: &TypeSpec) -> Result<usize> {
    if let TypeSpec::Character(n) = kind {
        Ok(n.unwrap_or(1))
    } else {
        bail!("CHARACTER actual argument requires a fixed length")
    }
}
fn same(a: &TypeSpec, b: &TypeSpec) -> bool {
    matches!((a,b),(TypeSpec::Integer(a),TypeSpec::Integer(b)) if a.unwrap_or(4)==b.unwrap_or(4))
        || a == b
}
#[derive(Default)]
pub(crate) struct Lower {
    routines: HashMap<String, Routine>,
    returns: HashMap<String, (bool, TypeSpec)>,
    serial: usize,
    active: Vec<String>,
    pure_functions: HashMap<String, bool>,
    defer_calls: bool,
    allocation_helpers: Vec<Stmt>,
}
impl Lower {
    pub(crate) fn fixed_items(e:&Expr,symbols:&Symbols)->Result<(TypeSpec,Vec<Expr>)> {
        let value=Self::default().view(e,symbols)?;
        if value.active.is_some() {bail!("fixed array expression requires a constant extent");}
        Ok((value.kind,value.items.context("array expression required")?))
    }
    pub(crate) fn finish(&mut self, mut p: Program) -> Result<Program> {
        self.defer_calls=false;
        self.body(&mut p.body,&Symbols::new())?;
        Ok(p)
    }
    fn view(&self, e: &Expr, symbols: &Symbols) -> Result<Value> {
        if let Some(kind)=crate::empty_constructors::kind(e)? {return Ok(Value { active:None,kind,items:Some(vec![])});}
        if let Some(items)=crate::multidim::fixed_section_elements(e,symbols)? {
            let (Expr::Call(name,_)|Expr::Index(name,_))=e else {unreachable!()};
            return Ok(Value { active:None,kind:symbols[name].0.clone(),items:Some(items)});
        }
        let scalar = |kind| Ok(Value { active:None, kind, items: None });
        match e {
            Expr::Call(n, _) if matches!(n.as_str(), "shape" | "lbound" | "ubound")
                && !symbols.contains_key(n) && !self.returns.contains_key(n) => {
                let value = crate::multidim::fixed_inquiry(e, symbols)?;
                if let Expr::Call(tag, items) = value {
                    if tag == "@bounds_vector" {
                        return Ok(Value { active:None, kind: TypeSpec::Integer(None), items: Some(items) });
                    }
                }
                scalar(TypeSpec::Integer(None))
            }
            Expr::Str(s) => scalar(TypeSpec::Character(Some(s.len()))),
            Expr::Call(n,args) if n=="@packed_view" && args.len()==2=>{
                let mut view=self.view(&args[0],symbols)?;
                if view.items.as_ref().is_some_and(|items|!items.is_empty()) {view.active=Some(args[1].clone());}
                Ok(view)
            }
            Expr::Ident(n) => {
                let (kind, dims) = symbols
                    .get(n)
                    .with_context(|| format!("undeclared elemental actual `{n}`"))?;
                if dims.is_empty() {
                    return scalar(kind.clone());
                }
                if dims.iter().any(|d|matches!(d, Expr::Ident(n) if n == "@deferred_extent")) {
                    bail!("cannot specialize assumed-shape array argument `{n}`: its extent is not yet known at this function call");
                }
                let dims=dims.iter().map(|d|crate::parameters::integer_constant(d).map(|n|n.max(0))).collect::<Result<Vec<_>>>()?;
                let count = array_count(&dims).with_context(|| {
                    format!("cannot specialize array argument `{n}`: its extent must be known at this function call")
                })?;
                if !(0..=1024).contains(&count) {
                    bail!("CHARACTER elemental expansion supports at most 1024 elements");
                }
                let active=crate::allocatables::length(n,symbols).filter(|e|crate::parameters::integer_constant(e).is_err());
                if active.is_some() && symbols.contains_key(&format!("@heap:{n}")) {bail!("content-dependent CHARACTER results currently require a bounded input array");}
                Ok(Value { active:if count==0 {None}else{active},
                    kind: kind.clone(),
                    items: Some(
                        (0..count as usize)
                            .map(|i| Expr::Index(n.clone(), array_indices(i,&dims)))
                            .collect(),
                    ),
                })
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if let Some((kind, dims)) = symbols.get(name) {
                    if dims.len()>1 {bail!("array section `{name}` requires {} subscripts",dims.len());}
                    if dims.is_empty() && matches!(kind, TypeSpec::Character(_)) {
                        if stride.is_some() {
                            bail!("CHARACTER substring cannot have a stride");
                        }
                        let first = lower
                            .as_deref()
                            .map(crate::parameters::integer_constant)
                            .transpose()?
                            .unwrap_or(1);
                        let last = upper
                            .as_deref()
                            .map(crate::parameters::integer_constant)
                            .transpose()?
                            .unwrap_or(length(kind)? as i64);
                        return scalar(TypeSpec::Character(Some(usize::try_from(
                            (last - first + 1).max(0),
                        )?)));
                    }
                }
                let base = self.view(&Expr::Ident(name.clone()), symbols)?;
                let items = base
                    .items
                    .context("elemental array section requires an array")?;
                let step = stride.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(1);
                if step==0 {bail!("array section stride must not be zero");}
                let first = lower.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(if step>0 {1} else {items.len() as i64});
                let last = upper.as_deref().map(crate::parameters::integer_constant).transpose()?.unwrap_or(if step>0 {items.len() as i64} else {1});
                let mut output = vec![];
                let mut i = first;
                while if step > 0 { i <= last } else { i >= last } {
                    if output.len() >= 1024 {
                        bail!("CHARACTER elemental expansion supports at most 1024 elements");
                    }
                    output.push(
                        items
                            .get(
                                usize::try_from(i - 1)
                                    .context("invalid elemental section index")?,
                            )
                            .context("elemental section index is outside the array")?
                            .clone(),
                    );
                    i = i
                        .checked_add(step)
                        .context("elemental section index overflow")?;
                }
                Ok(Value { active:None,
                    kind: base.kind,
                    items: Some(output),
                })
            }
            Expr::Call(n, args) if n == "@array_constructor" => {
                let mut kind = None;
                let mut items = vec![];
                for item in args {
                    let view = self.view(item, symbols)?;
                    if kind.as_ref().is_some_and(|k| !same(k, &view.kind)) {
                        bail!("array constructor elements must have matching type and CHARACTER length");
                    }
                    kind = Some(view.kind);
                    items.extend(view.items.unwrap_or_else(|| vec![item.clone()]));
                }
                if items.len() > 1024 {
                    bail!("CHARACTER elemental expansion supports at most 1024 elements");
                }
                Ok(Value { active:None,
                    kind: kind.context("empty untyped elemental constructor")?,
                    items: Some(items),
                })
            }
            Expr::Call(n,args) if n=="reshape" => {
                let dims=crate::multidim::reshape_shape(args)?;
                let count=array_count(&dims)?;
                if count>1024 {bail!("function array expression specialization supports at most 1024 elements");}
                let order=crate::multidim::reshape_order(args,dims.len())?;
                let source=self.view(&args[0],symbols)?;
                let items=source.items.context("RESHAPE SOURCE must be an array")?;
                let pad=if matches!(&args[2],Expr::Call(n,_) if n=="@no_pad") {vec![]} else {
                    let pad=self.view(&args[2],symbols)?;
                    if !same(&source.kind,&pad.kind) {bail!("RESHAPE PAD type must match SOURCE");}
                    pad.items.context("RESHAPE PAD must be an array")?
                };
                if count as usize>items.len() && pad.is_empty() {bail!("RESHAPE SOURCE has too few elements without PAD");}
                let values=(0..count as usize).map(|i| {
                    let coords=array_indices(i,&dims);
                    let mut ordinal=0usize;let mut stride=1usize;
                    for axis in &order {
                        ordinal+=(crate::parameters::integer_constant(&coords[*axis]).unwrap() as usize-1)*stride;
                        stride*=dims[*axis] as usize;
                    }
                    if ordinal<items.len() {items[ordinal].clone()} else {pad[(ordinal-items.len())%pad.len()].clone()}
                }).collect();
                Ok(Value{active:None,kind:source.kind,items:Some(values)})
            }
            Expr::Call(n,args) if n=="transpose" && args.len()==1 => {
                let shape=crate::multidim::fixed_actual_shape(&args[0],symbols)?.context("TRANSPOSE requires a rank-two array")?;
                if shape.len()!=2 {bail!("TRANSPOSE requires a rank-two array");}
                self.view(&Expr::Call("reshape".into(),vec![args[0].clone(),Expr::Call("@array_constructor".into(),vec![int(shape[1]),int(shape[0])]),Expr::Call("@no_pad".into(),vec![]),Expr::Call("@array_constructor".into(),vec![int(2),int(1)])]),symbols)
            }
            Expr::Call(n, _) | Expr::Index(n, _) if symbols.contains_key(n) => {
                scalar(symbols[n].0.clone())
            }
            Expr::Bin(op, a, b) => {
                let aview = self.view(a, symbols)?;
                let bview = self.view(b, symbols)?;
                let count = match (&aview.items, &bview.items) {
                    (Some(a), Some(b)) if a.len() != b.len() => {
                        bail!("nonconforming elemental array argument shapes")
                    }
                    (Some(a), _) => Some(a.len()),
                    (_, Some(b)) => Some(b.len()),
                    _ => None,
                };
                let kind = if matches!(op, BinOp::Concat) {
                    TypeSpec::Character(Some(
                        length(&aview.kind)?
                            .checked_add(length(&bview.kind)?)
                            .context("CHARACTER length overflow")?,
                    ))
                } else {
                    let mut scalar_symbols = Symbols::new();
                    scalar_symbols.insert("@a".into(), (aview.kind.clone(), vec![]));
                    scalar_symbols.insert("@b".into(), (bview.kind.clone(), vec![]));
                    crate::array_calls::scalar_actual_kind(
                        &Expr::Bin(
                            op.clone(),
                            Box::new(Expr::Ident("@a".into())),
                            Box::new(Expr::Ident("@b".into())),
                        ),
                        &scalar_symbols,
                    )?
                };
                let items = count.map(|count| {
                    (0..count)
                        .map(|i| {
                            Expr::Bin(
                                op.clone(),
                                Box::new(
                                    aview
                                        .items
                                        .as_ref()
                                        .map_or_else(|| (**a).clone(), |v| v[i].clone()),
                                ),
                                Box::new(
                                    bview
                                        .items
                                        .as_ref()
                                        .map_or_else(|| (**b).clone(), |v| v[i].clone()),
                                ),
                            )
                        })
                        .collect()
                });
                Ok(Value { active:None, kind, items })
            }
            Expr::Call(n, args) if n == "@array_substring" => {
                let [base, Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                }] = args.as_slice()
                else {
                    bail!("invalid CHARACTER substring actual");
                };
                if stride.is_some() {
                    bail!("CHARACTER substring cannot have a stride");
                }
                let view = self.view(base, symbols)?;
                if view.items.is_some() {
                    bail!("CHARACTER elemental substring actual currently requires one element");
                }
                let first = lower
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()?
                    .unwrap_or(1);
                let last = upper
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()?
                    .unwrap_or(length(&view.kind)? as i64);
                scalar(TypeSpec::Character(Some(usize::try_from(
                    (last - first + 1).max(0),
                )?)))
            }
            Expr::Call(n, args) if self.returns.contains_key(n) => {
                let (elemental, kind) = &self.returns[n];
                if !elemental {
                    return scalar(kind.clone());
                }
                let views = args
                    .iter()
                    .map(|arg| self.view(arg, symbols))
                    .collect::<Result<Vec<_>>>()?;
                let mut count = None;
                for view in &views {
                    if let Some(items) = &view.items {
                        if count.is_some_and(|c| c != items.len()) {
                            bail!("nonconforming elemental array argument shapes");
                        }
                        count = Some(items.len());
                    }
                }
                Ok(Value { active:None,
                    kind: kind.clone(),
                    items: count.map(|count| {
                        (0..count)
                            .map(|i| {
                                Expr::Call(
                                    n.clone(),
                                    args.iter()
                                        .zip(&views)
                                        .map(|(arg, v)| {
                                            v.items.as_ref().map_or_else(
                                                || arg.clone(),
                                                |items| items[i].clone(),
                                            )
                                        })
                                        .collect(),
                                )
                            })
                            .collect()
                    }),
                })
            }
            // TRIM has a runtime length unless its argument is constant. Fixed
            // buffer capacity must not be mistaken for an assumed-length actual.
            Expr::Call(n, _) if n == "trim" => {
                let value=crate::parameters::character_constant(e).context("CHARACTER elemental actual length must be constant; runtime TRIM actuals are not yet supported")?;
                scalar(TypeSpec::Character(Some(value.len())))
            }
            _ => {
                if let Ok((_, Some(count), kind)) =
                    crate::reductions::elemental_element(e, &int(1), symbols)
                {
                    if !(0..=1024).contains(&count) {
                        bail!("CHARACTER elemental expansion supports at most 1024 elements");
                    }
                    let items = (1..=count)
                        .map(|i| {
                            crate::reductions::elemental_element(e, &int(i), symbols).map(|v| v.0)
                        })
                        .collect::<Result<_>>()?;
                    return Ok(Value { active:None,
                        kind,
                        items: Some(items),
                    });
                }
                if let Some(kind) = crate::inquiries::model_type(e, &|n| {
                    symbols.get(n).map(|(k, d)| (k.clone(), d.len()))
                })? {
                    return scalar(kind);
                }
                if matches!(e,Expr::Call(n,_) if matches!(n.as_str(),"len"|"len_trim"|"size"|"kind"|"index"|"scan"|"verify"))
                {
                    return scalar(TypeSpec::Integer(None));
                }
                scalar(crate::array_calls::scalar_actual_kind(e, symbols)?)
            }
        }
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        symbols: &mut Symbols,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        if matches!(e,Expr::Call(n,_) if n=="@array_constructor") && crate::constructors::delayed(e) {
            if self.contains_calls(&[Stmt::Print {items:vec![e.clone()]}]) {
                bail!("CHARACTER function calls in delayed constructor implied-DOs are not yet supported");
            }
            return Ok(());
        }
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    self.expression(arg, symbols, prefix)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, symbols, prefix)?;
                self.expression(b, symbols, prefix)?;
            }
            Expr::Un(_, a) => self.expression(a, symbols, prefix)?,
            _ => {}
        }
        if let Expr::Call(name,args)=e {
            if name=="@runtime_character_constructor" {
                let length=usize::try_from(crate::parameters::integer_constant(&args[0])?)
                    .context("CHARACTER constructor requires a nonnegative constant length")?;
                let kind=TypeSpec::Character(Some(length));
                let mut items=vec![];
                for arg in &args[1..] {
                    let value=self.view(arg,symbols)?;
                    if !matches!(value.kind,TypeSpec::Character(_)) {bail!("typed CHARACTER constructor requires CHARACTER elements");}
                    if value.active.is_some() {bail!("typed CHARACTER constructor currently requires a fixed array extent");}
                    items.extend(value.items.unwrap_or_else(||vec![arg.clone()]));
                }
                self.serial+=1;
                let temp=format!("@typed_character_array_{}",self.serial);
                let dims=vec![int(items.len() as i64)];
                prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:dims.clone()});
                for (index,value) in items.into_iter().enumerate() {
                    prefix.push(Stmt::AssignIndex{name:temp.clone(),indices:vec![int(index as i64+1)],value});
                }
                symbols.insert(temp.clone(),(kind,dims));
                *e=Expr::Ident(temp);
                return Ok(());
            }
        }
        let Expr::Call(name, args) = e else {
            return Ok(());
        };
        let Some(routine) = self.routines.get(name).cloned() else {
            return Ok(());
        };
        if self.active.contains(name) {
            bail!("recursive CHARACTER function `{name}` is not yet supported");
        }
        if args.len() != routine.params.len() {
            bail!("wrong argument count for CHARACTER function `{name}`");
        }
        let array_result=routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==&routine.result));
        fn runtime(e:&Expr,symbols:&Symbols)->bool {
            match e {
                Expr::Ident(n)=>symbols.get(n).is_some_and(|(_,dims)|dims.iter().any(|d|crate::parameters::integer_constant(d).is_err()))
                    || symbols.contains_key(&format!("@axes:{n}"))
                    || crate::allocatables::length(n,symbols).is_some_and(|e|crate::parameters::integer_constant(&e).is_err()),
                Expr::Call(n,a) if n=="@packed_view" && a.len()==2=>runtime(&a[0],symbols)
                    || crate::parameters::integer_constant(&a[1]).is_err(),
                Expr::Call(_,args)|Expr::Index(_,args)=>args.iter().any(|e|runtime(e,symbols)),
                Expr::Section{name,lower,upper,stride}=>runtime(&Expr::Ident(name.clone()),symbols)
                    || [lower,upper,stride].into_iter().flatten().any(|e|runtime(e,symbols)),
                Expr::Bin(_,a,b)=>runtime(a,symbols)||runtime(b,symbols),Expr::Un(_,e)=>runtime(e,symbols),_=>false,
            }
        }
        fn pending(e:&Expr,symbols:&Symbols)->bool {
            match e {
                Expr::Ident(n)=>symbols.get(n).is_some_and(|(_,d)|d.iter().any(|d|matches!(d,Expr::Ident(n) if n=="@deferred_extent"))) && !symbols.contains_key(&format!("@axes:{n}")),
                Expr::Section{name,..}=>pending(&Expr::Ident(name.clone()),symbols),
                Expr::Call(_,a)|Expr::Index(_,a)=>a.iter().any(|e|pending(e,symbols)),Expr::Bin(_,a,b)=>pending(a,symbols)||pending(b,symbols),Expr::Un(_,e)=>pending(e,symbols),_=>false,
            }
        }
        if array_result && self.defer_calls && args.iter().any(|e|pending(e,symbols)){return Ok(());}
        if name.starts_with("@enumeration_step_") {
            if self.defer_calls && args.iter().any(|e|pending(e,symbols)) {return Ok(());}
            for (n,(elemental,kind)) in &self.returns {
                symbols.insert(format!("@function:{n}"),(kind.clone(),vec![]));
                if *elemental {symbols.insert(format!("@elemental_function:{n}"),(kind.clone(),vec![]));}
            }
            let mut call=Expr::Call(name.clone(),args.clone());
            if crate::vector_subscripts::elemental_function(&mut call,symbols,prefix,&mut self.serial,
                (&routine.params,&routine.body,&routine.result,&routine.kind))? {*e=call;return Ok(());}
        }
        if !args.iter().any(|e|pending(e,symbols)) {
            for (param,actual) in routine.params.iter().zip(args.iter_mut()) {
                if routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && intent=="intent(in)"))
                    && !routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && matches!(intent.as_str(),"target"|"pointer"|"allocatable"))) {
                    crate::vector_subscripts::specialize_expression(actual,symbols,prefix,&mut self.serial)?;
                }
            }
        }
        fn references(e:&Expr,name:&str)->bool {match e {
            Expr::Ident(n)=>n==name,Expr::Call(_,a)|Expr::Index(_,a)=>a.iter().any(|e|references(e,name)),
            Expr::Bin(_,a,b)=>references(a,name)||references(b,name),Expr::Un(_,e)=>references(e,name),_=>false,
        }}
        let runtime_result=array_result && (args.iter().any(|e|runtime(e,symbols))
            || routine.params.iter().zip(args.iter()).any(|(p,e)|routine.body.iter().any(|s|matches!(s,Stmt::VarDecl{kind:TypeSpec::Integer(_),names} if names.contains(p))) && routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,dims,..} if name==&routine.result && dims.iter().any(|d|references(d,p)))) && crate::parameters::integer_constant(e).is_err()));
        let allocatable_result=array_result && routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&routine.result && intent=="allocatable"));
        let descriptor_kind=if !array_result && matches!(routine.kind,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)) {
            let mut types=HashMap::new();
            for (param,actual) in routine.params.iter().zip(args.iter()) {
                let kind=self.view(actual,symbols).ok().map(|v|v.kind).or_else(|| {
                    // Runtime array views retain a known element type even
                    // when their active extent cannot be enumerated here.
                    if matches!(actual,Expr::Ident(_)|Expr::Section{..}) || matches!(actual,Expr::Call(tag,_) if tag=="@packed_view") {
                        crate::inquiries::model_type(actual,&|n|symbols.get(n).map(|(k,d)|(k.clone(),d.len()))).ok().flatten()
                    } else {None}
                });
                if let Some(kind)=kind {types.insert(param.clone(),kind);} else {types.clear();break;}
            }
            Self::specialize_kind(&routine.kind,&types,&HashMap::new(),&HashMap::new()).ok()
        } else {Some(routine.kind.clone())};
        if let Some(helper) = routine.descriptor_call.as_ref().filter(|_|descriptor_kind.is_some() && (!array_result||runtime_result||allocatable_result)) {
            self.serial += 1;
            let result = format!("@array_function_result_{}", self.serial);
            let result_rank=routine.body.iter().find_map(|s|match s {
                Stmt::ArrayDecl{name,dims,..} if name==&routine.result=>Some(dims.len()),_=>None,
            });
            if let Some(rank)=result_rank {
                crate::heap::unallocated_result(routine.kind.clone(),result.clone(),rank,prefix,symbols)?;
            } else {
                let kind=descriptor_kind.clone().unwrap();
                prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![result.clone()] });
                symbols.insert(result.clone(), (kind, vec![]));
            }
            let mut actuals = args.clone();
            for (param,actual) in routine.params.iter().zip(&mut actuals) {
                if routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && matches!(intent.as_str(),"intent(out)"|"intent(inout)"))) {
                    let Expr::Ident(name)=actual else {bail!("writable function dummy `{param}` requires a whole variable actual");};
                    if symbols.contains_key(&format!("@function_readonly:{name}")) {bail!("cannot pass INTENT(IN) variable as writable function argument `{param}`");}
                }
                if !matches!(actual,Expr::Ident(_)) {
                    // Uncalled procedure templates can contain unresolved
                    // bounds/views. Association handles those later; this
                    // optional capture only consumes known matrix shapes.
                    if let Some(shape)=crate::multidim::fixed_actual_shape(actual,symbols).ok().flatten() {
                        if shape.len()>1 {
                            let view=self.view(actual,symbols)?;
                            let items=view.items.context("function array expression must have bounded storage")?;
                            if array_count(&shape)? as usize!=items.len() {bail!("function argument shape does not match its selected elements");}
                            self.serial+=1;
                            let temp=format!("@array_function_expression_{}",self.serial);
                            prefix.push(Stmt::ArrayDecl{kind:view.kind,name:temp.clone(),dims:shape.iter().copied().map(int).collect()});
                            for (i,value) in items.into_iter().enumerate() {prefix.push(Stmt::AssignIndex{name:temp.clone(),indices:array_indices(i,&shape),value});}
                            *actual=Expr::Ident(temp);
                            continue;
                        }
                    }
                }
                if let Some(items)=crate::multidim::fixed_section_elements(actual,symbols)? {
                    let shape=crate::multidim::fixed_actual_shape(actual,symbols)?.context("function section must have an array shape")?;
                    let (Expr::Call(source,_)|Expr::Index(source,_))=actual else {unreachable!()};
                    let kind=symbols[source].0.clone();
                    self.serial+=1;let temp=format!("@array_function_section_{}",self.serial);
                    prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:shape.iter().copied().map(int).collect()});
                    for (i,value) in items.into_iter().enumerate() {prefix.push(Stmt::AssignIndex{name:temp.clone(),indices:array_indices(i,&shape),value});}
                    *actual=Expr::Ident(temp);
                }
            }
            actuals.push(Expr::Ident(result.clone()));
            prefix.push(Stmt::CallSub { name: helper.clone(), args: actuals });
            *e = Expr::Ident(result);
            return Ok(());
        }
        fn unbound(e: &Expr, symbols: &Symbols) -> bool {
            match e {
                Expr::Ident(n) => symbols.get(n).is_some_and(|(kind,dims)|
                    matches!(kind,TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_))
                    || dims.iter().any(|d|crate::parameters::integer_constant(d).is_err())),
                Expr::Call(_,args)|Expr::Index(_,args)=>args.iter().any(|e|unbound(e,symbols)),
                Expr::Bin(_,a,b)=>unbound(a,symbols)||unbound(b,symbols),
                Expr::Un(_,a)=>unbound(a,symbols),
                Expr::Section{name,lower,upper,stride}=>unbound(&Expr::Ident(name.clone()),symbols)
                    || [lower,upper,stride].into_iter().flatten().any(|e|unbound(e,symbols)),
                _=>false,
            }
        }
        if self.defer_calls && args.iter().any(|e|unbound(e,symbols)) {
            // Preserve the original call in the procedure template. Its actual
            // array extents and CHARACTER lengths are supplied by association.
            return Ok(());
        }
        if self.defer_calls && routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==&routine.result))
            && routine.params.iter().zip(args.iter()).any(|(param,arg)|
                routine.body.iter().any(|s|matches!(s,Stmt::VarDecl{kind:TypeSpec::Integer(_),names} if names.contains(param)))
                && crate::parameters::integer_constant(arg).is_err()) {
            return Ok(());
        }
        for arg in args.iter_mut() {crate::constructors::fixed_expression(arg,symbols)?;}
        let views = args
            .iter()
            .map(|arg| self.view(arg, symbols))
            .collect::<Result<Vec<_>>>()?;
        let shapes=args.iter().zip(&views).map(|(arg,view)| {
            if let Some(items)=&view.items {
                if view.active.is_some() {Ok(vec![items.len() as i64])} else {Ok(crate::multidim::fixed_actual_shape(arg,symbols)?.unwrap_or_else(||vec![items.len() as i64]))}
            } else {Ok(vec![])}
        }).collect::<Result<Vec<_>>>()?;
        for (view,shape) in views.iter().zip(&shapes) {
            if let Some(items)=&view.items {
                if array_count(shape)? as usize!=items.len() {bail!("function argument shape does not match its selected elements");}
            }
        }
        if routine.elemental && views.iter().any(|v|v.active.is_some()) {bail!("runtime-sized ELEMENTAL CHARACTER calls are not yet supported");}
        let bound_types = self.types(&routine, &views)?;
        let writable=routine.params.iter().filter(|param|routine.body.iter().any(|s|
            matches!(s,Stmt::DummyIntent{name,intent} if name==*param && matches!(intent.as_str(),"intent(out)"|"intent(inout)"))))
            .cloned().collect::<std::collections::HashSet<_>>();
        if routine.elemental && !writable.is_empty() {
            let array_call=views.iter().any(|v|v.items.is_some());
            for ((param,arg),view) in routine.params.iter().zip(args.iter()).zip(&views) {
                if !writable.contains(param) {continue;}
                if !same(&bound_types[param],&view.kind) {bail!("type mismatch for writable elemental function dummy `{param}`");}
                if array_call && view.items.is_none() {bail!("ELEMENTAL writable dummy `{param}` requires an array actual when any argument is an array");}
                let n=match arg {
                    Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)|Expr::Section{name:n,..}=>n,
                    _=>bail!("writable elemental function dummy `{param}` requires a variable actual"),
                };
                if symbols.contains_key(&format!("@function_readonly:{n}")) {bail!("cannot pass INTENT(IN) variable as writable function argument `{param}`");}
            }
        }
        if !routine.elemental {
            for (param,view) in routine.params.iter().zip(&views) {
                let array=routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==param));
                if array && view.items.is_none() {bail!("function array dummy `{param}` requires an array actual");}
                if !array && view.items.is_some() {bail!("non-elemental CHARACTER function `{name}` currently requires scalar actuals for `{param}`");}
                if array && routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param&&matches!(intent.as_str(),"pointer"|"allocatable"))) {bail!("POINTER/ALLOCATABLE CHARACTER function array dummies are not yet supported");}
            }
            for param in &routine.params {
                if !writable.contains(param) && !routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param&&intent=="intent(in)")) {
                    bail!("CHARACTER function `{name}` currently requires INTENT(IN) scalar dummy `{param}`");
                }
            }
            for ((param,arg),view) in routine.params.iter().zip(args.iter()).zip(&views) {
                if writable.contains(param) {
                    if !same(&bound_types[param],&view.kind) {bail!("type or CHARACTER length mismatch for writable function dummy `{param}`");}
                    let Expr::Ident(actual)=arg else {bail!("writable function dummy `{param}` requires a whole variable actual");};
                    if symbols.contains_key(&format!("@function_readonly:{actual}")) {bail!("cannot pass INTENT(IN) variable as writable function argument `{param}`");}
                }
            }
        }
        let mut count = None;
        let mut elemental_shape=None;
        for (view,shape) in views.iter().zip(&shapes) {
            if !routine.elemental {break;}
            if let Some(items) = &view.items {
                if elemental_shape.as_ref().is_some_and(|d|d!=shape) {bail!("nonconforming elemental array argument shapes");}
                elemental_shape=Some(shape.clone());
                if count.is_some_and(|c| c != items.len()) {
                    bail!("nonconforming elemental array argument shapes");
                }
                count = Some(items.len());
            }
        }
        let mut actuals = args.clone();
        // Evaluate scalar actual expressions once for the whole elemental call.
        for ((arg, view),param) in actuals.iter_mut().zip(&views).zip(&routine.params) {
            if writable.contains(param) {continue;}
            if view.items.is_none() {
                self.serial += 1;
                let temp = format!("@character_elemental_actual_{}", self.serial);
                prefix.push(Stmt::VarDecl {
                    kind: view.kind.clone(),
                    names: vec![temp.clone()],
                });
                prefix.push(Stmt::Assign {
                    name: temp.clone(),
                    value: arg.clone(),
                });
                symbols.insert(temp.clone(), (view.kind.clone(), vec![]));
                if let Expr::Ident(actual)=arg {
                    if let Some((_,values))=symbols.get(&format!("@character_spec_length:{actual}")) {
                        if let Some(Expr::Ident(length))=values.first() {
                            prefix.push(Stmt::DummyIntent{name:temp.clone(),intent:format!("@character_spec_length:{length}")});
                        }
                    }
                }
                crate::allocatables::metadata(prefix,symbols);
                *arg = Expr::Ident(temp);
            }
        }
        self.active.push(name.clone());
        let mut results = vec![];
        // Check declarations and result length even when the result has zero size.
        let mut constants=HashMap::new();
        for (param,arg) in routine.params.iter().zip(args.iter()) {
            if let Ok(mut value)=crate::parameters::character_constant(arg) {
                let size=length(&bound_types[param])?;
                value.truncate(size);
                value.extend(std::iter::repeat_n(' ',size-value.len()));
                constants.insert(param.clone(),Expr::Str(value));
            } else if let Ok(value)=crate::parameters::integer_constant(arg) {
                constants.insert(param.clone(),int(value));
            }
        }
        let mut specialized=routine.body.clone();
        let logical_lengths:HashMap<_,_>=routine.params.iter().zip(&actuals).filter_map(|(param,arg)| {
            if !routine.body.iter().any(|s|matches!(s,Stmt::VarDecl{kind:TypeSpec::CharacterAssumed,names} if names.contains(param))) {return None;}
            let Expr::Ident(actual)=arg else {return None;};
            symbols.get(&format!("@character_spec_length:{actual}")).and_then(|(_,v)|v.first()).map(|length|(param.clone(),length.clone()))
        }).collect();
        for stmt in &mut specialized {
            if let Stmt::VarDecl{kind:TypeSpec::CharacterLength(e),..}|Stmt::ArrayDecl{kind:TypeSpec::CharacterLength(e),..}=stmt {
                crate::character_spec::dynamic_lengths(e,&logical_lengths);
            }
        }
        let runtime_lengths=self.specialize_types(&mut specialized,&routine,&views,&shapes,&constants)?;
        for s in &mut specialized {
            if let Stmt::ArrayDecl{dims,..}=s {
                for d in dims {if crate::parameters::integer_constant(d).is_ok_and(|n|n<0) {*d=int(0);}}
            }
        }
        // ALLOCATABLE results carry an active length independently of the
        // conservative storage bound inferred from their allocation helpers.
        let alloc_result=specialized.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&routine.result && intent=="allocatable"))
            && specialized.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==&routine.result));
        if alloc_result {
            let helper_count=self.allocation_helpers.len();
            specialized.extend(self.allocation_helpers.clone());
            let mut prepared=crate::allocatables::prepare_function_result(Program{name:String::new(),body:specialized,contains:vec![]},&routine.result,&constants)?;
            prepared.body.truncate(prepared.body.len()-helper_count);
            specialized=prepared.body;
        }
        let mut result_dims=specialized.iter().find_map(|s|if let Stmt::ArrayDecl{name,dims,..}=s {if name==&routine.result {Some(dims.clone())} else {None}} else {None});
        if let Some(dims)=&mut result_dims {
            if routine.elemental {bail!("ELEMENTAL function requires a scalar result");}
            let extents=dims.iter().map(crate::parameters::integer_constant).collect::<Result<Vec<_>>>().context("specialized function result extents must resolve to call-site constants")?;
            let count=array_count(&extents)?;
            if !(0..=1024).contains(&count) {bail!("specialized function array results support at most 1024 elements");}
            // Later storage/output passes require literal extents, rather than
            // expressions such as 2*2 whose value is now statically known.
            *dims=extents.into_iter().map(int).collect();
        }
        let result_kind=if crate::substrings::deferred(&routine.kind) {self.call_kind(&routine,&views,&shapes,&constants)?} else {
            specialized.iter().find_map(|s|match s {
                Stmt::VarDecl{kind,names} if names.contains(&routine.result)=>Some(kind.clone()),
                Stmt::ArrayDecl{kind,name,..} if name==&routine.result=>Some(kind.clone()),_=>None,
            }).map(Ok).unwrap_or_else(||self.call_kind(&routine,&views,&shapes,&constants))?
        };
        for i in 0..count.unwrap_or(1) {
            let args = actuals
                .iter()
                .zip(&views)
                .map(|(arg, v)| {
                    if !routine.elemental {arg.clone()} else {v.items
                        .as_ref()
                        .map_or_else(|| arg.clone(), |items| items[i].clone())}
                })
                .collect::<Vec<_>>();
            self.serial += 1;
            let result = format!("@character_elemental_result_{}", self.serial);
            prefix.push(if let Some(dims)=&result_dims {
                Stmt::ArrayDecl {kind:result_kind.clone(),name:result.clone(),dims:dims.clone()}
            } else {Stmt::VarDecl {kind:result_kind.clone(),names:vec![result.clone()]}});
            if crate::substrings::deferred(&result_kind) {
                prefix.push(Stmt::DummyIntent{name:result.clone(),intent:"allocatable".into()});
            }
            symbols.insert(result.clone(), (result_kind.clone(), result_dims.clone().unwrap_or_default()));
            let mut names = HashMap::new();
            names.insert(routine.result.clone(), result.clone());
            let mut expanded = vec![];
            let mut copybacks = vec![];
            for (param, arg) in routine.params.iter().zip(args) {
                if writable.contains(param) {
                    let actual=match arg {
                        Expr::Ident(actual)=>actual,
                        Expr::Index(array,indices)|Expr::Call(array,indices) if routine.elemental=>{
                            // Bind an OUT element through a fresh local, then
                            // copy back without reading its previous value.
                            self.serial+=1;
                            let temp=format!("@elemental_writable_{}",self.serial);
                            expanded.push(Stmt::VarDecl{kind:bound_types[param].clone(),names:vec![temp.clone()]});
                            let mut saved=vec![];
                            for index in indices {
                                self.serial+=1;
                                let name=format!("@elemental_writable_index_{}",self.serial);
                                expanded.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![name.clone()]});
                                expanded.push(Stmt::Assign{name:name.clone(),value:index});
                                saved.push(Expr::Ident(name));
                            }
                            copybacks.push(Stmt::AssignIndex{name:array,indices:saved,value:Expr::Ident(temp.clone())});
                            temp
                        },
                        _=>bail!("writable function dummy `{param}` requires a variable actual"),
                    };
                    if routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param && intent=="intent(out)")) {
                        expanded.push(Stmt::CallSub{name:"@undefine".into(),args:vec![Expr::Ident(actual.clone())]});
                    }
                    names.insert(param.clone(),actual);
                    continue;
                }
                self.serial += 1;
                let temp = format!("@character_elemental_dummy_{}", self.serial);
                let view=&views[routine.params.iter().position(|p|p==param).unwrap()];
                if !routine.elemental && view.items.is_some() {
                    let items=view.items.as_ref().unwrap();
                    let shape=&shapes[routine.params.iter().position(|p|p==param).unwrap()];
                    expanded.push(Stmt::ArrayDecl {kind:bound_types[param].clone(),name:temp.clone(),dims:shape.iter().copied().map(int).collect()});
                    let active=if let Some(value)=&view.active {
                        self.serial+=1;let length=format!("@character_argument_length_{}",self.serial);let flag=format!("@character_argument_allocated_{}",self.serial);
                        expanded.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![length.clone()]});
                        expanded.push(Stmt::Assign{name:length.clone(),value:value.clone()});
                        expanded.push(Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![flag.clone()]});
                        expanded.push(Stmt::Assign{name:flag.clone(),value:Expr::Logical(true)});
                        expanded.push(Stmt::DummyIntent{name:temp.clone(),intent:format!("@bounded:{length}:{flag}")});
                        expanded.push(Stmt::DummyIntent{name:temp.clone(),intent:format!("@bounded_shape:{length}")});
                        Some(length)
                    } else {None};
                    for (i,value) in items.iter().enumerate() {
                        let store=Stmt::AssignIndex {name:temp.clone(),indices:array_indices(i,shape),value:value.clone()};
                        expanded.push(if let Some(length)=&active {Stmt::If{cond:Expr::Bin(BinOp::Ge,Box::new(Expr::Ident(length.clone())),Box::new(int(i as i64+1))),then_body:vec![store],else_body:None}}else{store});
                    }
                } else {
                    expanded.push(Stmt::VarDecl {kind:bound_types[param].clone(),names:vec![temp.clone()]});
                    expanded.push(Stmt::Assign {name:temp.clone(),value:arg});
                    if let Some(Expr::Ident(length))=logical_lengths.get(param) {
                        expanded.push(Stmt::DummyIntent{name:temp.clone(),intent:format!("@character_spec_length:{length}")});
                    }
                }
                names.insert(param.clone(), temp);
            }
            let mut body = specialized.clone();
            for (param,length) in &logical_lengths {if let Expr::Ident(length)=length {
                body.push(Stmt::DummyIntent{name:param.clone(),intent:format!("@character_spec_length:{length}")});
            }}
            for (variable,expression,capacity) in &runtime_lengths {
                self.serial+=1;
                let length=format!("@character_spec_length_{}",self.serial);
                prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![length.clone()]});
                let mut expression=expression.clone();crate::subroutines::expr(&mut expression,&names);
                crate::character_spec::capture(&mut expression,&mut expanded,&mut self.serial)?;
                expanded.push(Stmt::Assign{name:length.clone(),value:expression});
                expanded.push(Stmt::If{cond:Expr::Bin(BinOp::Gt,Box::new(Expr::Ident(length.clone())),Box::new(int(*capacity as i64))),then_body:vec![Stmt::CallSub{name:"@error_stop".into(),args:vec![Expr::Str("CHARACTER specification exceeded its storage bound".into())]}],else_body:None});
                let marker=Stmt::DummyIntent{name:variable.clone(),intent:format!("@character_spec_length:{length}")};
                if variable==&routine.result {
                    prefix.push(Stmt::DummyIntent{name:result.clone(),intent:format!("@character_spec_length:{length}")});
                    crate::allocatables::metadata(prefix,symbols);
                }
                body.push(marker);
            }
            if !routine.elemental {
                let mut shapes=HashMap::new();
                for stmt in &body {match stmt {
                    Stmt::VarDecl {kind,names}=>for n in names {shapes.insert(n.clone(),(kind.clone(),vec![]));},
                    Stmt::ArrayDecl {kind,name,dims}=>{shapes.insert(name.clone(),(kind.clone(),dims.clone()));},_=>{}
                }}
                crate::allocatables::metadata(&expanded,&mut shapes);
                crate::allocatables::metadata(&body,&mut shapes);
                for (param,bound) in &names {
                    if let Some((_,values))=shapes.get(&format!("@extent:{bound}")).cloned() {
                        shapes.insert(format!("@extent:{param}"),(TypeSpec::Integer(Some(8)),values.clone()));
                        shapes.insert(format!("@axes:{param}"),(TypeSpec::Integer(Some(8)),values));
                    }
                }
                crate::array_calls::specialize_character_function(&mut body,&mut shapes,&constants)?;
            }
            if matches!(body.last(),Some(Stmt::Return(Some(Expr::Ident(n)))) if n==&routine.result)
            {
                body.pop();
            }
            fn returns(body: &mut [Stmt]) {
                for s in body {
                    match s {
                        Stmt::Return(value) => *value = None,
                        Stmt::If {
                            then_body,
                            else_body,
                            ..
                        } => {
                            returns(then_body);
                            if let Some(body) = else_body {
                                returns(body);
                            }
                        }
                        Stmt::Do { body, .. }
                        | Stmt::DoWhile { body, .. }
                        | Stmt::Block { body } => returns(body),
                        Stmt::SelectCase { cases, default, .. } => {
                            for c in cases {
                                returns(&mut c.body);
                            }
                            if let Some(body) = default {
                                returns(body);
                            }
                        }
                        _ => {}
                    }
                }
            }
            returns(&mut body);
            let mut bound = routine.params.clone();
            bound.push(routine.result.clone());
            // Function and subroutine expansion have independent counters.
            // Keep their locals distinct when an expanded function calls a helper.
            crate::subroutines::rename_with_prefix(&mut body, &names, &bound, &mut self.serial, "@character_function_local")?;
            if alloc_result {
                // The caller consumes the result after the inlined function's
                // BLOCK ends, so its allocation state belongs to caller scope.
                let mut state=std::collections::HashSet::new();
                for s in &body {
                    if let Stmt::DummyIntent{name,intent}=s {
                        if name==&result {
                            if let Some(rest)=intent.strip_prefix("@bounded:") {state.extend(rest.split(':').map(str::to_owned));}
                        }
                    }
                }
                body.retain(|s| {
                    let hoist=match s {
                        Stmt::DummyIntent{name,intent}=>name==&result && intent.starts_with("@bounded:"),
                        Stmt::VarDecl{names,..}=>names.iter().all(|n|state.contains(n)),
                        Stmt::Assign{name,..}=>state.contains(name),
                        _=>false,
                    };
                    if hoist {prefix.push(s.clone());}
                    !hoist
                });
                crate::allocatables::metadata(prefix,symbols);
            }
            body.retain(|s| {
                !matches!(s, Stmt::DummyIntent { intent,.. } if !intent.starts_with("@bounded") && !intent.starts_with("@character_spec_length:"))
                    && !matches!(s,Stmt::Implicit(_))
                    && !matches!(s,Stmt::VarDecl{names,..} if names.is_empty())
                    && !matches!(s,Stmt::ArrayDecl{name,..} if bound.contains(name))
            });
            crate::subroutines::inline_returns_with_prefix(&mut body, &mut self.serial, "@character_function_returned")?;
            expanded.extend(body);
            expanded.extend(copybacks);
            self.body(&mut expanded, symbols)?;
            prefix.push(Stmt::Block { body: expanded });
            results.push(Expr::Ident(result));
        }
        self.active.pop();
        *e = if count.is_some() && elemental_shape.as_ref().is_some_and(|d|d.len()>1) {
            let dims=elemental_shape.unwrap();
            self.serial+=1;
            let temp=format!("@elemental_multirank_result_{}",self.serial);
            prefix.push(Stmt::ArrayDecl{kind:result_kind.clone(),name:temp.clone(),dims:dims.iter().copied().map(int).collect()});
            for (i,value) in results.into_iter().enumerate() {
                prefix.push(Stmt::AssignIndex{name:temp.clone(),indices:array_indices(i,&dims),value});
            }
            symbols.insert(temp.clone(),(result_kind,dims.into_iter().map(int).collect()));
            Expr::Ident(temp)
        } else if count.is_some() {
            if results.is_empty() {
                self.serial += 1;
                let temp = format!("@character_elemental_empty_{}", self.serial);
                prefix.push(Stmt::ArrayDecl {
                    kind: result_kind.clone(),
                    name: temp.clone(),
                    dims: vec![int(0)],
                });
                symbols.insert(temp.clone(), (result_kind, vec![int(0)]));
                Expr::Ident(temp)
            } else if matches!(result_kind, TypeSpec::Character(_)) {
                self.serial += 1;
                let temp = format!("@character_elemental_array_{}", self.serial);
                prefix.push(Stmt::ArrayDecl {
                    kind: result_kind.clone(),
                    name: temp.clone(),
                    dims: vec![int(results.len() as i64)],
                });
                for (i, value) in results.into_iter().enumerate() {
                    prefix.push(Stmt::AssignIndex {
                        name: temp.clone(),
                        indices: vec![int(i as i64 + 1)],
                        value,
                    });
                }
                symbols.insert(
                    temp.clone(),
                    (result_kind, vec![int(count.unwrap() as i64)]),
                );
                Expr::Ident(temp)
            } else {
                Expr::Call("@array_constructor".into(), results)
            }
        } else {
            results.remove(0)
        };
        Ok(())
    }
    fn types(&self, routine: &Routine, views: &[Value]) -> Result<HashMap<String, TypeSpec>> {
        let mut types = HashMap::new();
        for (param, actual) in routine.params.iter().zip(views) {
            let declared = routine
                .body
                .iter()
                .find_map(|s| match s {
                    Stmt::VarDecl { kind, names } if names.contains(param) => Some(kind),
                    Stmt::ArrayDecl {kind,name,..} if name==param=>Some(kind),
                    _ => None,
                })
                .context("CHARACTER function dummy must be a declared scalar")?;
            let kind = match declared {
                TypeSpec::CharacterAssumed => {
                    length(&actual.kind)?;
                    actual.kind.clone()
                }
                TypeSpec::Character(_) => {
                    if routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==param)) && length(&actual.kind)?!=length(declared)? {
                        bail!("fixed-length CHARACTER array dummy `{param}` currently requires a matching element length");
                    }
                    if length(&actual.kind)? < length(declared)? {
                        bail!("actual CHARACTER length is shorter than elemental dummy `{param}`");
                    }
                    declared.clone()
                }
                _ => {
                    if !same(declared, &actual.kind) {
                        bail!("type or CHARACTER length mismatch for elemental dummy `{param}`");
                    }
                    declared.clone()
                }
            };
            types.insert(param.clone(), kind);
        }
        Ok(types)
    }
    fn fold_lengths(e: &mut Expr, types: &HashMap<String, TypeSpec>) {
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    Self::fold_lengths(arg, types);
                }
            }
            Expr::Bin(_, a, b) => {
                Self::fold_lengths(a, types);
                Self::fold_lengths(b, types);
            }
            Expr::Un(_, a) => Self::fold_lengths(a, types),
            _ => {}
        }
        if let Expr::Call(name, args) = e {
            if name == "len" {
                if let [Expr::Ident(n)] = args.as_slice() {
                    if let Some(TypeSpec::Character(length)) = types.get(n) {
                        *e = int(length.unwrap_or(1) as i64);
                    }
                }
            }
        }
    }
    fn fold_sizes(e:&mut Expr,sizes:&HashMap<String,Vec<i64>>) {
        match e {
            Expr::Call(n,args) if n=="size"=>{
                if let Some(Expr::Ident(name))=args.first() {
                    if let Some(dims)=sizes.get(name) {
                        if args.len()==1 {*e=int(dims.iter().product());return;}
                        if args.len()==2 {
                            if let Ok(axis)=crate::parameters::integer_constant(&args[1]) {
                                if axis>0 && (axis as usize)<=dims.len() {*e=int(dims[axis as usize-1]);return;}
                            }
                        }
                    }
                }
                for e in args {Self::fold_sizes(e,sizes);}
            },
            Expr::Call(_,args)|Expr::Index(_,args)=>for e in args {Self::fold_sizes(e,sizes);},
            Expr::Bin(_,a,b)=>{Self::fold_sizes(a,sizes);Self::fold_sizes(b,sizes);},Expr::Un(_,e)=>Self::fold_sizes(e,sizes),_=>{}
        }
    }
    fn length_constants(e:&mut Expr, constants:&HashMap<String,Expr>) {
        match e {
            Expr::Ident(n)=>if let Some(value)=constants.get(n) {*e=value.clone();},
            Expr::Call(_,args)|Expr::Index(_,args)=>for e in args {Self::length_constants(e,constants);},
            Expr::Bin(_,a,b)=>{Self::length_constants(a,constants);Self::length_constants(b,constants);},
            Expr::Un(_,e)=>Self::length_constants(e,constants),_=>{}
        }
    }
    fn specialize_kind(kind: &TypeSpec, types: &HashMap<String, TypeSpec>, constants: &HashMap<String,Expr>, sizes: &HashMap<String,Vec<i64>>) -> Result<TypeSpec> {
        let mut kind = kind.clone();
        if let TypeSpec::CharacterLength(length) = &mut kind {
            Self::fold_lengths(length, types);
            Self::length_constants(length,constants);
            Self::fold_sizes(length,sizes);
        }
        let kind=crate::parameters::concrete_kind(kind).context("CHARACTER function result/local length must resolve to a compile-time constant for these actuals")?;
        if crate::substrings::deferred(&kind) || matches!(kind, TypeSpec::CharacterAssumed) {
            bail!("CHARACTER function result requires an explicit fixed length or supported ALLOCATABLE storage");
        }
        Ok(kind)
    }
    fn call_kind(&self, routine: &Routine, views: &[Value], shapes: &[Vec<i64>], constants:&HashMap<String,Expr>) -> Result<TypeSpec> {
        if !routine.elemental && crate::substrings::deferred(&routine.kind) {
            if !routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&routine.result&&intent=="allocatable")) {
                bail!("deferred CHARACTER function result `{}` requires ALLOCATABLE",routine.result);
            }
            return Ok(routine.kind.clone());
        }
        let sizes:HashMap<_,_>=routine.params.iter().zip(shapes).filter(|(_,d)|!d.is_empty()).map(|(n,d)|(n.clone(),d.clone())).collect();
        Self::specialize_kind(&routine.kind, &self.types(routine, views)?,constants,&sizes)
    }
    fn specialize_types(
        &self,
        body: &mut [Stmt],
        routine: &Routine,
        views: &[Value],
        shapes: &[Vec<i64>],
        constants:&HashMap<String,Expr>,
    ) -> Result<Vec<(String,Expr,usize)>> {
        let mut runtime=vec![];
        let mut types = self.types(routine, views)?;
        let capacities:HashMap<_,_>=routine.params.iter().zip(shapes).filter(|(_,d)|!d.is_empty()).map(|(n,d)|(n.clone(),d.clone())).collect();
        let sizes:HashMap<_,_>=routine.params.iter().zip(shapes).zip(views).filter(|((_,d),v)|!d.is_empty() && v.active.is_none()).map(|((n,d),_)|(n.clone(),d.clone())).collect();
        for s in body {
            if let Stmt::ArrayDecl {kind,name,dims}=s {
                if routine.params.contains(name) {
                    let shape=capacities.get(name).context("function array dummy requires an array actual")?;
                    if dims.len()!=shape.len() {bail!("rank mismatch for function dummy `{name}`: expected {}, got {}",dims.len(),shape.len());}
                    for (dim,extent) in dims.iter().zip(shape) {
                        if matches!(dim,Expr::Ident(n) if n=="@deferred_extent") {continue;}
                        let mut dim=dim.clone();Self::fold_sizes(&mut dim,&sizes);Self::length_constants(&mut dim,constants);
                        if crate::parameters::integer_constant(&dim)?!=*extent {bail!("explicit-shape function dummy `{name}` requires matching extents");}
                    }
                    *kind=types[name].clone();*dims=shape.iter().copied().map(int).collect();
                } else {
                    *kind=Self::specialize_kind(kind,&types,constants,&sizes)?;
                    for dim in dims {Self::fold_sizes(dim,&sizes);Self::length_constants(dim,constants);}
                    types.insert(name.clone(),kind.clone());
                }
            }
            if let Stmt::VarDecl { kind, names } = s {
                if names.iter().all(|n| routine.params.contains(n)) {
                    continue;
                }
                if names.contains(&routine.result) && !routine.elemental && crate::substrings::deferred(kind) {continue;}
                *kind = match Self::specialize_kind(kind, &types,constants,&sizes) {
                    Ok(kind)=>kind,
                    Err(error)=>{
                        let TypeSpec::CharacterLength(expression)=kind else {return Err(error);};
                        let mut expression=(**expression).clone();
                        Self::fold_lengths(&mut expression,&types);Self::length_constants(&mut expression,constants);Self::fold_sizes(&mut expression,&sizes);
                        let (expression,capacity)=crate::character_spec::bounded(&expression,&types,&capacities).context(error)?;
                        for name in names.iter() {runtime.push((name.clone(),expression.clone(),capacity));}
                        TypeSpec::Character(Some(capacity))
                    }
                };
                for n in names {
                    types.insert(n.clone(), kind.clone());
                }
            }
        }
        Ok(runtime)
    }
    pub(crate) fn contains_calls(&self, body: &[Stmt]) -> bool {
        if body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@where")) {
            // Masked calls without STAT must not evaluate excluded elements.
            let mut copy=body.to_vec();
            for stmt in &mut copy {
                for e in crate::procedure_arguments::expressions(stmt) {
                    crate::procedure_arguments::visit_expr(e,&mut |e| {
                        if let Expr::Call(n,_)=e {
                            if n.starts_with("@enumeration_step_") && self.routines.get(n).is_some_and(|r|r.params.len()==1) {
                                *n="@masked_enumeration_step".into();
                            }
                        }
                    });
                }
            }
            return self.contains_calls_raw(&copy);
        }
        self.contains_calls_raw(body)
    }
    fn contains_calls_raw(&self, body: &[Stmt]) -> bool {
        fn expr(e: &Expr, routines: &HashMap<String, Routine>) -> bool {
            match e {
                Expr::Call(n, args) => {
                    routines.contains_key(n) || args.iter().any(|a| expr(a, routines))
                }
                Expr::Index(_, args) => args.iter().any(|a| expr(a, routines)),
                Expr::Bin(_, a, b) => expr(a, routines) || expr(b, routines),
                Expr::Un(_, a) => expr(a, routines),
                _ => false,
            }
        }
        body.iter().any(|s| match s {
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => expr(value, &self.routines),
            Stmt::AssignIndex { indices,value,.. }=>expr(value,&self.routines)||indices.iter().any(|e|expr(e,&self.routines)),
            Stmt::Print { items } | Stmt::Read {args:items} | Stmt::CallSub { args: items, .. } => {
                items.iter().any(|e| expr(e, &self.routines))
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                expr(cond, &self.routines)
                    || self.contains_calls(then_body)
                    || else_body.as_ref().is_some_and(|b| self.contains_calls(b))
            }
            Stmt::Do{start,end,body,..}=>expr(start,&self.routines)||expr(end,&self.routines)||self.contains_calls(body),
            Stmt::DoWhile{cond,body}=>expr(cond,&self.routines)||self.contains_calls(body),
            Stmt::Block { body } | Stmt::Module{body,..} | Stmt::Function{body,..} | Stmt::Subroutine{body,..} => {
                self.contains_calls(body)
            }
            Stmt::SelectCase{expr:e,cases,default}=>expr(e,&self.routines)||cases.iter().any(|c|self.contains_calls(&c.body))||default.as_ref().is_some_and(|b|self.contains_calls(b)),
            _ => false,
        })
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Symbols) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut symbols = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        symbols.remove(&format!("@function_readonly:{n}"));
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    symbols.remove(&format!("@function_readonly:{name}"));
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                _ => {}
            }
        }
        crate::allocatables::metadata(body,&mut symbols);
        for s in body.iter() {
            if let Stmt::DummyIntent{name,intent}=s {
                if intent=="intent(in)" {symbols.insert(format!("@function_readonly:{name}"),(TypeSpec::Logical,vec![]));}
            }
        }
        let mut allocation_facts=HashMap::new();
        let mut out = vec![];
        let mut offset = None;
        for mut stmt in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&stmt) { statement_location.set(Some(p)); }
            // A direct constant allocation supplies the active shape before
            // CHARACTER calls are materialized for allocation-bound analysis.
            if let Stmt::CallSub{name,args}=&stmt {
                if name=="@allocate" {
                    for arg in args {if let Expr::Call(array,dims)=arg {
                        if let Some((kind,old))=symbols.get(array).cloned() {
                            if !symbols.contains_key(&format!("@axes:{array}"))
                                && old.iter().all(|d|matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
                                && dims.iter().all(|d|crate::parameters::integer_constant(d).is_ok()) {
                                allocation_facts.insert(array.clone(),old);
                                symbols.insert(array.clone(),(kind,dims.iter().map(|d|int(crate::parameters::integer_constant(d).unwrap().max(0))).collect()));
                            }
                        }
                    }}
                }
            }
            if let Stmt::SourceLocation(pos) = &stmt { offset = Some(*pos); }
            let mut prefix = vec![];
            (|| -> Result<()> {
                if let Stmt::Block { body } = &stmt {
                    if matches!(body.first(),Some(Stmt::CallSub{name,..}) if name=="@where") && self.contains_calls(body) {
                        bail!("specialized function calls inside WHERE are not yet supported");
                    }
                }
                if let Stmt::Block {body}=&mut stmt {
                    let marker=body.iter().position(|s|matches!(s,Stmt::CallSub{name,..} if name=="@forall"));
                    if marker.is_some() {
                        if !crate::multidim::specialize_forall(body,&symbols,&self.pure_functions,&self.returns,&mut self.serial)? {
                            // Argument specialization will supply the missing shape.
                            return Ok(());
                        }
                    }
                }
                match &mut stmt {
                    Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                        self.expression(value, &mut symbols, &mut prefix)?
                    }
                    Stmt::AssignIndex { indices, value, .. } => {
                        for e in indices {
                            self.expression(e, &mut symbols, &mut prefix)?;
                        }
                        self.expression(value, &mut symbols, &mut prefix)?;
                    }
                    Stmt::Print { items }
                    | Stmt::CallSub { args: items, .. }
                    | Stmt::Read { args: items } => {
                        for e in items {
                            self.expression(e, &mut symbols, &mut prefix)?;
                        }
                    }
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    } => {
                        self.expression(cond, &mut symbols, &mut prefix)?;
                        self.body(then_body, &symbols)?;
                        if let Some(body) = else_body {
                            self.body(body, &symbols)?;
                        }
                    }
                    Stmt::Do {
                        start, end, body, ..
                    } => {
                        self.expression(start, &mut symbols, &mut prefix)?;
                        self.expression(end, &mut symbols, &mut prefix)?;
                        self.body(body, &symbols)?;
                    }
                    Stmt::DoWhile { cond, body } => {
                        self.expression(cond, &mut symbols, &mut prefix)?;
                        self.body(body, &symbols)?;
                        if !prefix.is_empty() {
                            let owns_heap=prefix.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@heap_bind"));
                            // Result descriptors belong to one evaluation of
                            // the condition, and must be released on every test.
                            if !owns_heap {
                                prefix.retain(|s| {
                                    if matches!(s,Stmt::VarDecl{..}|Stmt::ArrayDecl{..}) {out.push(s.clone());false} else {true}
                                });
                            }
                            let condition = cond.clone();
                            *cond = Expr::Logical(true);
                            prefix.push(Stmt::If {
                                cond: Expr::not(condition),
                                then_body: vec![Stmt::Exit],
                                else_body: None,
                            });
                            prefix.append(body);
                            *body = if owns_heap {vec![Stmt::Block{body:std::mem::take(&mut prefix)}]} else {std::mem::take(&mut prefix)};
                        }
                    }
                    Stmt::Block {body} if body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@where"))=>{},
                    Stmt::Module { body, .. }
                    | Stmt::Function { body, .. }
                    | Stmt::Subroutine { body, .. }
                    | Stmt::Block { body } => self.body(body, &symbols)?,
                    Stmt::SelectCase {
                        expr,
                        cases,
                        default,
                    } => {
                        self.expression(expr, &mut symbols, &mut prefix)?;
                        for c in cases {
                            self.body(&mut c.body, &symbols)?;
                        }
                        if let Some(body) = default {
                            self.body(body, &symbols)?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            })().map_err(|error| crate::source_diagnostics::at(error, offset))?;
            fn mutations(s:&Stmt,names:&mut Vec<String>) {
                match s {
                    Stmt::CallSub{name,args} if name=="@allocate"=>for e in args {if let Expr::Call(n,_) =e {names.push(n.clone());}},
                    Stmt::CallSub{name,args} if name=="@deallocate" || !name.starts_with('@')=>for e in args {if let Expr::Ident(n)=e {names.push(n.clone());}},
                    Stmt::If{then_body,else_body,..}=>for s in then_body.iter().chain(else_body.iter().flatten()){mutations(s,names);},
                    Stmt::Do{body,..}|Stmt::DoWhile{body,..}|Stmt::Block{body}=>for s in body {mutations(s,names);},_=>{},
                }
            }
            let mut changed=vec![];
            // Preserve facts established by this statement's direct allocation.
            if !matches!(&stmt,Stmt::CallSub{name,..} if name=="@allocate") {mutations(&stmt,&mut changed);}
            for n in changed {if let Some(dims)=allocation_facts.remove(&n) {if let Some(value)=symbols.get_mut(&n){value.1=dims;}}}
            if prefix.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@heap_bind")) {
                prefix.push(stmt);
                out.push(Stmt::Block{body:prefix});
            }else{out.extend(prefix);out.push(stmt);}
        }
        *body = out;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
// Expansion may not accidentally bind a defining-scope variable to a same-named
// caller local. Constants have already been substituted by parameter resolution.
fn validate_locals(
    name: &str,
    params: &[String],
    result: &str,
    body: &[Stmt],
    host: &std::collections::HashSet<String>,
) -> Result<()> {
    let mut locals: std::collections::HashSet<String> = params.iter().cloned().collect();
    locals.insert(result.into());
    fn declarations(body: &[Stmt], locals: &mut std::collections::HashSet<String>) {
        for stmt in body {
            match stmt {
                Stmt::VarDecl { names, .. } => locals.extend(names.iter().cloned()),
                Stmt::ArrayDecl { name, .. } => {
                    locals.insert(name.clone());
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    declarations(then_body, locals);
                    if let Some(body) = else_body {
                        declarations(body, locals);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    declarations(body, locals)
                }
                _ => {}
            }
        }
    }
    declarations(body, &mut locals);
    let forbidden = host
        .difference(&locals)
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let variable = |n: &str| -> Result<()> {
        if !locals.contains(n) {
            bail!("CHARACTER function `{name}` requires explicitly declared locals; host variable `{n}` is not yet supported");
        }
        Ok(())
    };
    fn expression(
        e: &Expr,
        variable: &impl Fn(&str) -> Result<()>,
        forbidden: &std::collections::HashSet<String>,
    ) -> Result<()> {
        match e {
            Expr::Ident(n) => variable(n)?,
            Expr::Call(name, args) => {
                if forbidden.contains(name) {
                    variable(name)?;
                }
                for arg in args {
                    expression(arg, variable, forbidden)?;
                }
            }
            Expr::Index(n, args) => {
                variable(n)?;
                for arg in args {
                    expression(arg, variable, forbidden)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if !name.is_empty() && !name.starts_with('@') {
                    variable(name)?;
                }
                for bound in [lower, upper, stride].into_iter().flatten() {
                    expression(bound, variable, forbidden)?;
                }
            }
            Expr::Bin(_, a, b) => {
                expression(a, variable, forbidden)?;
                expression(b, variable, forbidden)?;
            }
            Expr::Un(_, e) => expression(e, variable, forbidden)?,
            _ => {}
        }
        Ok(())
    }
    let writable=|n:&str|->Result<()> {
        variable(n)?;
        if params.iter().any(|param|param==n) && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n&&intent=="intent(in)")) {
            bail!("CHARACTER function `{name}` cannot modify INTENT(IN) dummy `{n}`");
        }
        Ok(())
    };
    fn statements(
        body: &[Stmt],
        variable: &impl Fn(&str) -> Result<()>,
        writable: &impl Fn(&str) -> Result<()>,
        forbidden: &std::collections::HashSet<String>,
    ) -> Result<()> {
        for stmt in body {
            match stmt {
                Stmt::Assign { name, value } => {
                    writable(name)?;
                    expression(value, variable, forbidden)?;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    writable(name)?;
                    for index in indices {
                        expression(index, variable, forbidden)?;
                    }
                    expression(value, variable, forbidden)?;
                }
                Stmt::Print { items }
                | Stmt::CallSub { args: items, .. } => {
                    for item in items {
                        expression(item, variable, forbidden)?;
                    }
                }
                Stmt::Read {args}=>for e in args {
                    if let Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)=e {writable(n)?;}
                    expression(e,variable,forbidden)?;
                },
                Stmt::Return(Some(e)) => expression(e, variable, forbidden)?,
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    expression(cond, variable, forbidden)?;
                    statements(then_body, variable, writable, forbidden)?;
                    if let Some(body) = else_body {
                        statements(body, variable, writable, forbidden)?;
                    }
                }
                Stmt::Do {
                    var,
                    start,
                    end,
                    body,
                } => {
                    writable(var)?;
                    expression(start, variable, forbidden)?;
                    expression(end, variable, forbidden)?;
                    statements(body, variable, writable, forbidden)?;
                }
                Stmt::DoWhile { cond, body } => {
                    expression(cond, variable, forbidden)?;
                    statements(body, variable, writable, forbidden)?;
                }
                Stmt::Block { body } => statements(body, variable, writable, forbidden)?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    expression(expr, variable, forbidden)?;
                    for c in cases {
                        statements(&c.body, variable, writable, forbidden)?;
                    }
                    if let Some(body) = default {
                        statements(body, variable, writable, forbidden)?;
                    }
                }
                Stmt::Function { .. } | Stmt::Subroutine { .. } => bail!(
                    "internal procedures in CHARACTER functions are not yet supported"
                ),
                _ => {}
            }
        }
        Ok(())
    }
    statements(body, &variable, &writable, &forbidden)
}

pub(crate) fn inline_array(_pure: bool, params: &[String], result: &str, body: &[Stmt]) -> bool {
    let array_result=body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==result));
    array_result || body.iter().any(|s|matches!(s,Stmt::ArrayDecl{kind,name,dims}
        if params.contains(name) && !dims.is_empty()
            && matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical)
            ))
}

pub(crate) fn prepare(mut p: Program) -> Result<(Program, Lower)> {
    crate::elemental::validate_program(&p)?;
    let mut lower = Lower {
        routines: HashMap::new(),
        returns: HashMap::new(),
        serial: 0,
        active: vec![],
        pure_functions: HashMap::new(),
        defer_calls: true,
        allocation_helpers: vec![],
    };
    fn pure_functions(body: &[Stmt], known: &mut HashMap<String,bool>) {
        for s in body {match s {
            Stmt::Function{name,pure,body,..}=>{known.insert(name.clone(),*pure);pure_functions(body,known);},
            Stmt::Module{body,..}|Stmt::Subroutine{body,..}=>pure_functions(body,known),_=>{}
        }}
    }
    pure_functions(&p.body,&mut lower.pure_functions);
    fn allocation_helpers(body:&[Stmt],helpers:&mut Vec<Stmt>) {
        for s in body {match s {
            Stmt::Subroutine{params,body,..} if params.iter().any(|p|
                body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p && intent=="allocatable"))
                && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==p && intent=="intent(out)")))=>helpers.push(s.clone()),
            Stmt::Module{body,..}=>allocation_helpers(body,helpers),_=>{}
        }}
    }
    allocation_helpers(&p.body,&mut lower.allocation_helpers);
    fn collect(body: &mut Vec<Stmt>, lower: &mut Lower) -> Result<()> {
        let mut host = std::collections::HashSet::new();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { names, .. } => host.extend(names.iter().cloned()),
                Stmt::ArrayDecl { name, .. } => {
                    host.insert(name.clone());
                }
                _ => {}
            }
        }
        let mut out = vec![];
        for mut stmt in std::mem::take(body) {
            if let Stmt::Function {
                name,
                params,
                return_type,
                body,
                elemental,
                pure,
                ..
            } = &stmt
            {
                if let Some(kind) = crate::elemental::result_kind(name, return_type, body) {
                    lower
                        .returns
                        .insert(name.clone(), (*elemental, kind.clone()));
                    let result = match body.last() {
                        Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                        _ => name.clone(),
                    };
                    if inline_array(*pure,params,&result,body) || crate::elemental::inline_character(
                        *elemental,
                        name,
                        params,
                        return_type,
                        body,
                    ) {
                        validate_locals(name, params, &result, body, &host)?;
                        if lower.routines.contains_key(name) {
                            bail!("same-named CHARACTER functions from different scopes are not yet supported: `{name}`");
                        }
                        let numeric = |kind: &TypeSpec| matches!(kind, TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::Logical);
                        // Reuse subroutine argument association for scalar-valued
                        // numeric and fixed-length CHARACTER functions. Calls can be preserved in procedure
                        // templates until their actual array descriptors are bound.
                        let input = |kind: &TypeSpec| numeric(kind) || matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed);
                        let character_array_input=body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if params.contains(name)))
                            && !body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==&result));
                        let descriptor = !*elemental && (numeric(&kind) || character_array_input && matches!(kind,TypeSpec::Character(_)|TypeSpec::CharacterLength(_)))
                            && params.iter().all(|param| {
                                body.iter().any(|s| matches!(s, Stmt::VarDecl {kind,names} if names.contains(param) && input(kind))
                                    || matches!(s, Stmt::ArrayDecl {kind,name,..} if name == param && input(kind)))
                                && body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == param && matches!(intent.as_str(),"intent(in)"|"intent(out)"|"intent(inout)")))
                                && !body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == param && matches!(intent.as_str(), "optional" | "allocatable" | "pointer")))
                            });
                        let descriptor_call = descriptor.then(|| format!("@array_function${name}"));
                        if let Some(helper) = &descriptor_call {
                            let mut helper_body = body.clone();
                            if matches!(helper_body.last(), Some(Stmt::Return(Some(Expr::Ident(n)))) if n == &result) { helper_body.pop(); }
                            fn returns(body: &mut [Stmt]) {
                                for s in body { match s {
                                    Stmt::Return(value) => *value = None,
                                    Stmt::If {then_body,else_body,..} => { returns(then_body); if let Some(body)=else_body {returns(body);} },
                                    Stmt::Do {body,..} | Stmt::DoWhile {body,..} | Stmt::Block {body} => returns(body),
                                    Stmt::SelectCase {cases,default,..} => {for c in cases {returns(&mut c.body);} if let Some(body)=default {returns(body);} },
                                    _ => {}
                                }}
                            }
                            returns(&mut helper_body);
                            if let Some(dims)=helper_body.iter().find_map(|s|match s {
                                Stmt::ArrayDecl{name,dims,..} if name==&result=>Some(dims.clone()),_=>None,
                            }) {
                                let allocatable=helper_body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&result&&intent=="allocatable"));
                                for s in &mut helper_body {
                                    if let Stmt::ArrayDecl{name,dims,..}=s {if name==&result {*dims=vec![Expr::Ident("@deferred_extent".into());dims.len()];}}
                                }
                                helper_body.push(Stmt::DummyIntent{name:result.clone(),intent:"@heap_dummy".into()});
                                if allocatable {
                                    fn result_assignments(body:&mut [Stmt],result:&str) {
                                        for s in body {match s {
                                            Stmt::Assign{name,value} if name==result=>{
                                                *s=Stmt::CallSub{name:"@alloc_result_assign".into(),args:vec![Expr::Ident(name.clone()),value.clone()]};
                                            },
                                            Stmt::If{then_body,else_body,..}=>{result_assignments(then_body,result);if let Some(b)=else_body {result_assignments(b,result);}},
                                            Stmt::Block{body}|Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>result_assignments(body,result),
                                            Stmt::SelectCase{cases,default,..}=>{for c in cases {result_assignments(&mut c.body,result);}if let Some(b)=default {result_assignments(b,result);}},
                                            _=>{},
                                        }}
                                    }
                                    result_assignments(&mut helper_body,&result);
                                }
                                if !allocatable {
                                    helper_body.push(Stmt::DummyIntent{name:result.clone(),intent:"allocatable".into()});
                                    let position=helper_body.iter().position(|s|!matches!(s,Stmt::SourceLocation(_)|Stmt::VarDecl{..}|Stmt::ArrayDecl{..}|Stmt::InitializedDecl{..}|Stmt::DummyIntent{..}|Stmt::Implicit(_)|Stmt::Use{..})).unwrap_or(helper_body.len());
                                    helper_body.insert(position,Stmt::CallSub{name:"@allocate".into(),args:vec![Expr::Call(result.clone(),dims)]});
                                }
                            } else if !helper_body.iter().any(|s| matches!(s, Stmt::VarDecl {names,..} if names.contains(&result))) {
                                helper_body.insert(0, Stmt::VarDecl {kind:kind.clone(),names:vec![result.clone()]});
                            }
                            helper_body.push(Stmt::DummyIntent {name:result.clone(),intent:"intent(out)".into()});
                            let mut helper_params = params.clone(); helper_params.push(result.clone());
                            out.push(Stmt::Subroutine {name:helper.clone(),params:helper_params,body:helper_body,pure:*pure,elemental:false});
                        }
                        lower.routines.insert(
                            name.clone(),
                            Routine {
                                params: params.clone(),
                                elemental: *elemental,
                                body: body.clone(),
                                result,
                                kind,
                                descriptor_call,
                            },
                        );
                        if name.starts_with("@enumeration_step_") && params.len()==1 {
                            out.push(stmt.clone());
                        }
                        continue;
                    }
                }
            }
            if let Stmt::Module { body, .. } = &mut stmt {
                collect(body, lower)?;
            }
            out.push(stmt);
        }
        *body = out;
        Ok(())
    }
    collect(&mut p.body, &mut lower)?;
    lower.body(&mut p.body, &Symbols::new())?;
    Ok((p,lower))
}

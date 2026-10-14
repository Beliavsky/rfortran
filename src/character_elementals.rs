// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Specialize functions with CHARACTER inputs/results or read-only rank-one
//! array inputs. Results use caller-owned locals, preserving the scalar ABI.
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
    kind: TypeSpec,
    items: Option<Vec<Expr>>,
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
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
    pub(crate) fn finish(&mut self, mut p: Program) -> Result<Program> {
        self.defer_calls=false;
        self.body(&mut p.body,&Symbols::new())?;
        Ok(p)
    }
    fn view(&self, e: &Expr, symbols: &Symbols) -> Result<Value> {
        if let Some(kind)=crate::empty_constructors::kind(e)? {return Ok(Value {kind,items:Some(vec![])});}
        if let Some(items)=crate::multidim::fixed_section_elements(e,symbols)? {
            let (Expr::Call(name,_)|Expr::Index(name,_))=e else {unreachable!()};
            return Ok(Value {kind:symbols[name].0.clone(),items:Some(items)});
        }
        let scalar = |kind| Ok(Value { kind, items: None });
        match e {
            Expr::Call(n, _) if matches!(n.as_str(), "shape" | "lbound" | "ubound")
                && !symbols.contains_key(n) && !self.returns.contains_key(n) => {
                let value = crate::multidim::fixed_inquiry(e, symbols)?;
                if let Expr::Call(tag, items) = value {
                    if tag == "@bounds_vector" {
                        return Ok(Value { kind: TypeSpec::Integer(None), items: Some(items) });
                    }
                }
                scalar(TypeSpec::Integer(None))
            }
            Expr::Str(s) => scalar(TypeSpec::Character(Some(s.len()))),
            Expr::Ident(n) => {
                let (kind, dims) = symbols
                    .get(n)
                    .with_context(|| format!("undeclared elemental actual `{n}`"))?;
                if dims.is_empty() {
                    return scalar(kind.clone());
                }
                if dims.len() != 1 {
                    bail!("elemental calls involving CHARACTER currently require rank-one array actuals");
                }
                if matches!(&dims[0], Expr::Ident(n) if n == "@deferred_extent") {
                    bail!("cannot specialize assumed-shape array argument `{n}`: its extent is not yet known at this function call");
                }
                let count = crate::parameters::integer_constant(&dims[0]).with_context(|| {
                    format!("cannot specialize array argument `{n}`: its extent must be known at this function call")
                })?;
                if !(0..=1024).contains(&count) {
                    bail!("CHARACTER elemental expansion supports at most 1024 elements");
                }
                Ok(Value {
                    kind: kind.clone(),
                    items: Some(
                        (1..=count)
                            .map(|i| Expr::Call(n.clone(), vec![int(i)]))
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
                Ok(Value {
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
                Ok(Value {
                    kind: kind.context("empty untyped elemental constructor")?,
                    items: Some(items),
                })
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
                Ok(Value { kind, items })
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
                Ok(Value {
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
                    return Ok(Value {
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
        if let Some(helper) = &routine.descriptor_call {
            self.serial += 1;
            let result = format!("@array_function_result_{}", self.serial);
            prefix.push(Stmt::VarDecl { kind: routine.kind.clone(), names: vec![result.clone()] });
            let mut actuals = args.clone();
            for actual in &mut actuals {
                if let Some(items)=crate::multidim::fixed_section_elements(actual,symbols)? {
                    let (Expr::Call(source,_)|Expr::Index(source,_))=actual else {unreachable!()};
                    let kind=symbols[source].0.clone();
                    self.serial+=1;let temp=format!("@array_function_section_{}",self.serial);
                    prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(items.len() as i64)]});
                    for (i,value) in items.into_iter().enumerate() {prefix.push(Stmt::AssignIndex{name:temp.clone(),indices:vec![int(i as i64+1)],value});}
                    *actual=Expr::Ident(temp);
                }
            }
            actuals.push(Expr::Ident(result.clone()));
            prefix.push(Stmt::CallSub { name: helper.clone(), args: actuals });
            symbols.insert(result.clone(), (routine.kind.clone(), vec![]));
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
        let views = args
            .iter()
            .map(|arg| self.view(arg, symbols))
            .collect::<Result<Vec<_>>>()?;
        if !routine.elemental {
            for (param,view) in routine.params.iter().zip(&views) {
                let array=routine.body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==param));
                if array && view.items.is_none() {bail!("CHARACTER function array dummy `{param}` requires a rank-one array actual");}
                if !array && view.items.is_some() {bail!("non-elemental CHARACTER function `{name}` currently requires scalar actuals for `{param}`");}
                if array && routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param&&matches!(intent.as_str(),"pointer"|"allocatable"))) {bail!("POINTER/ALLOCATABLE CHARACTER function array dummies are not yet supported");}
            }
            for param in &routine.params {
                if !routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==param&&intent=="intent(in)")) {
                    bail!("CHARACTER function `{name}` currently requires INTENT(IN) scalar dummy `{param}`");
                }
            }
        }
        let mut count = None;
        for view in &views {
            if !routine.elemental {break;}
            if let Some(items) = &view.items {
                if count.is_some_and(|c| c != items.len()) {
                    bail!("nonconforming elemental array argument shapes");
                }
                count = Some(items.len());
            }
        }
        let mut actuals = args.clone();
        // Evaluate scalar actual expressions once for the whole elemental call.
        for (arg, view) in actuals.iter_mut().zip(&views) {
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
                *arg = Expr::Ident(temp);
            }
        }
        self.active.push(name.clone());
        let mut results = vec![];
        // Check declarations and result length even when the result has zero size.
        let bound_types = self.types(&routine, &views)?;
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
        self.specialize_types(&mut specialized,&routine,&views,&constants)?;
        // ALLOCATABLE results carry an active length independently of the
        // conservative storage bound inferred from their allocation helpers.
        let alloc_result=specialized.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&routine.result && intent=="allocatable"))
            && specialized.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==&routine.result));
        if alloc_result {
            let helper_count=self.allocation_helpers.len();
            specialized.extend(self.allocation_helpers.clone());
            let mut prepared=crate::allocatables::prepare(Program{name:String::new(),body:specialized})?;
            prepared.body.truncate(prepared.body.len()-helper_count);
            specialized=prepared.body;
        }
        let result_dims=specialized.iter().find_map(|s|if let Stmt::ArrayDecl{name,dims,..}=s {if name==&routine.result {Some(dims.clone())} else {None}} else {None});
        if let Some(dims)=&result_dims {
            if routine.elemental {bail!("ELEMENTAL function requires a scalar result");}
            if dims.len()!=1 {bail!("specialized function array results currently require rank one");}
            let count=crate::parameters::integer_constant(&dims[0]).context("specialized function result extent must resolve to a call-site constant")?;
            if !(0..=1024).contains(&count) {bail!("specialized function array results support at most 1024 elements");}
        }
        let result_kind = self.call_kind(&routine, &views, &constants)?;
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
            for (param, arg) in routine.params.iter().zip(args) {
                self.serial += 1;
                let temp = format!("@character_elemental_dummy_{}", self.serial);
                let view=&views[routine.params.iter().position(|p|p==param).unwrap()];
                if !routine.elemental && view.items.is_some() {
                    let items=view.items.as_ref().unwrap();
                    expanded.push(Stmt::ArrayDecl {kind:bound_types[param].clone(),name:temp.clone(),dims:vec![int(items.len() as i64)]});
                    for (i,value) in items.iter().enumerate() {expanded.push(Stmt::AssignIndex {name:temp.clone(),indices:vec![int(i as i64+1)],value:value.clone()});}
                } else {
                    expanded.push(Stmt::VarDecl {kind:bound_types[param].clone(),names:vec![temp.clone()]});
                    expanded.push(Stmt::Assign {name:temp.clone(),value:arg});
                }
                names.insert(param.clone(), temp);
            }
            let mut body = specialized.clone();
            if !routine.elemental {
                let mut shapes=HashMap::new();
                for stmt in &body {match stmt {
                    Stmt::VarDecl {kind,names}=>for n in names {shapes.insert(n.clone(),(kind.clone(),vec![]));},
                    Stmt::ArrayDecl {kind,name,dims}=>{shapes.insert(name.clone(),(kind.clone(),dims.clone()));},_=>{}
                }}
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
                !matches!(s, Stmt::DummyIntent { intent,.. } if !intent.starts_with("@bounded"))
                    && !matches!(s,Stmt::Implicit(_))
                    && !matches!(s,Stmt::VarDecl{names,..} if names.is_empty())
                    && !matches!(s,Stmt::ArrayDecl{name,..} if bound.contains(name))
            });
            crate::subroutines::inline_returns_with_prefix(&mut body, &mut self.serial, "@character_function_returned")?;
            expanded.extend(body);
            self.body(&mut expanded, symbols)?;
            prefix.push(Stmt::Block { body: expanded });
            results.push(Expr::Ident(result));
        }
        self.active.pop();
        *e = if count.is_some() {
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
    fn fold_sizes(e:&mut Expr,sizes:&HashMap<String,i64>) {
        match e {
            Expr::Call(n,args) if n=="size"=>{
                if let Some(Expr::Ident(name))=args.first() {
                    if let Some(count)=sizes.get(name) {
                        if args.len()==1 || args.len()==2&&crate::parameters::integer_constant(&args[1]).ok()==Some(1) {*e=int(*count);return;}
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
    fn specialize_kind(kind: &TypeSpec, types: &HashMap<String, TypeSpec>, constants: &HashMap<String,Expr>) -> Result<TypeSpec> {
        let mut kind = kind.clone();
        if let TypeSpec::CharacterLength(length) = &mut kind {
            Self::fold_lengths(length, types);
            Self::length_constants(length,constants);
        }
        let kind=crate::parameters::concrete_kind(kind).context("CHARACTER function result/local length must resolve to a compile-time constant for these actuals")?;
        if crate::substrings::deferred(&kind) || matches!(kind, TypeSpec::CharacterAssumed) {
            bail!("CHARACTER function result requires an explicit fixed length or supported ALLOCATABLE storage");
        }
        Ok(kind)
    }
    fn call_kind(&self, routine: &Routine, views: &[Value], constants:&HashMap<String,Expr>) -> Result<TypeSpec> {
        if !routine.elemental && crate::substrings::deferred(&routine.kind) {
            if !routine.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==&routine.result&&intent=="allocatable")) {
                bail!("deferred CHARACTER function result `{}` requires ALLOCATABLE",routine.result);
            }
            return Ok(routine.kind.clone());
        }
        Self::specialize_kind(&routine.kind, &self.types(routine, views)?,constants)
    }
    fn specialize_types(
        &self,
        body: &mut [Stmt],
        routine: &Routine,
        views: &[Value],
        constants:&HashMap<String,Expr>,
    ) -> Result<()> {
        let mut types = self.types(routine, views)?;
        let sizes:HashMap<_,_>=routine.params.iter().zip(views).filter_map(|(n,v)|v.items.as_ref().map(|items|(n.clone(),items.len() as i64))).collect();
        for s in body {
            if let Stmt::ArrayDecl {kind,name,dims}=s {
                if routine.params.contains(name) {
                    let count=*sizes.get(name).context("array dummy requires a rank-one array actual")?;
                    if dims.len()!=1 {bail!("CHARACTER function array dummies currently require rank one");}
                    if !matches!(&dims[0],Expr::Ident(n) if n=="@deferred_extent") {
                        let mut dim=dims[0].clone();Self::fold_sizes(&mut dim,&sizes);Self::length_constants(&mut dim,constants);
                        if crate::parameters::integer_constant(&dim)?!=count {bail!("explicit-shape CHARACTER function dummy `{name}` requires a matching extent");}
                    }
                    *kind=types[name].clone();*dims=vec![int(count)];
                } else {
                    *kind=Self::specialize_kind(kind,&types,constants)?;
                    for dim in dims {Self::fold_sizes(dim,&sizes);Self::length_constants(dim,constants);}
                    types.insert(name.clone(),kind.clone());
                }
            }
            if let Stmt::VarDecl { kind, names } = s {
                if names.iter().all(|n| routine.params.contains(n)) {
                    continue;
                }
                if names.contains(&routine.result) && !routine.elemental && crate::substrings::deferred(kind) {continue;}
                *kind = Self::specialize_kind(kind, &types,constants)?;
                for n in names {
                    types.insert(n.clone(), kind.clone());
                }
            }
        }
        Ok(())
    }
    fn contains_calls(&self, body: &[Stmt]) -> bool {
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
            Stmt::Assign { value, .. }
            | Stmt::AssignIndex { value, .. }
            | Stmt::Return(Some(value)) => expr(value, &self.routines),
            Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
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
            Stmt::Block { body } | Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } => {
                self.contains_calls(body)
            }
            _ => false,
        })
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Symbols) -> Result<()> {
        let mut symbols = inherited.clone();
        for s in body.iter() {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        symbols.insert(n.clone(), (kind.clone(), vec![]));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    symbols.insert(name.clone(), (kind.clone(), dims.clone()));
                }
                _ => {}
            }
        }
        crate::allocatables::metadata(body,&mut symbols);
        let mut out = vec![];
        let mut offset = None;
        for mut stmt in std::mem::take(body) {
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
                    fn character(e: &Expr, symbols: &Symbols) -> bool {
                        let named=|n:&String| symbols.get(n).is_some_and(|(k,_)|matches!(k,TypeSpec::Character(_)|TypeSpec::CharacterAssumed|TypeSpec::CharacterLength(_)));
                        match e {
                            Expr::Ident(n)=>named(n),Expr::Str(_)=>true,
                            Expr::Call(n,args)|Expr::Index(n,args)=>named(n)||args.iter().any(|e|character(e,symbols)),
                            Expr::Bin(_,a,b)=>character(a,symbols)||character(b,symbols),Expr::Un(_,a)=>character(a,symbols),
                            Expr::Section{name,..}=>named(name),_=>false
                        }
                    }
                    let characters=body.iter().any(|s|matches!(s,Stmt::AssignIndex{name,value,..}
                        if character(&Expr::Ident(name.clone()),&symbols)||character(value,&symbols)));
                    if marker.is_some() && (self.contains_calls(body)||characters||marker!=Some(0)) {
                        if !crate::multidim::specialize_forall(body,&symbols,&self.pure_functions,&mut self.serial)? {
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
                            // Declarations live in the enclosing scope; executable
                            // association and calls must repeat at every loop test.
                            prefix.retain(|s| {
                                if matches!(s,Stmt::VarDecl{..}|Stmt::ArrayDecl{..}) {out.push(s.clone());false} else {true}
                            });
                            let condition = cond.clone();
                            *cond = Expr::Logical(true);
                            prefix.push(Stmt::If {
                                cond: Expr::not(condition),
                                then_body: vec![Stmt::Exit],
                                else_body: None,
                            });
                            prefix.append(body);
                            *body = std::mem::take(&mut prefix);
                        }
                    }
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
            out.extend(prefix);
            out.push(stmt);
        }
        *body = out;
        Ok(())
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

pub(crate) fn inline_array(pure: bool, params: &[String], result: &str, body: &[Stmt]) -> bool {
    let array_result=body.iter().any(|s|matches!(s,Stmt::ArrayDecl{name,..} if name==result));
    pure && body.iter().any(|s|matches!(s,Stmt::ArrayDecl{kind,name,dims}
        if params.contains(name) && !dims.is_empty()
            && matches!(kind,TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision|TypeSpec::Logical)
            && (array_result || dims.iter().any(|d|matches!(d,Expr::Ident(n) if n=="@deferred_extent")))))
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
                        // numeric functions. Calls can be preserved in procedure
                        // templates until their actual array descriptors are bound.
                        let input = |kind: &TypeSpec| numeric(kind) || matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed);
                        let descriptor = *pure && !*elemental && numeric(&kind)
                            && !body.iter().any(|s| matches!(s, Stmt::ArrayDecl {name,..} if name == &result))
                            && params.iter().all(|param| {
                                body.iter().any(|s| matches!(s, Stmt::VarDecl {kind,names} if names.contains(param) && input(kind))
                                    || matches!(s, Stmt::ArrayDecl {kind,name,..} if name == param && input(kind)))
                                && body.iter().any(|s| matches!(s, Stmt::DummyIntent {name,intent} if name == param && intent == "intent(in)"))
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
                            if !helper_body.iter().any(|s| matches!(s, Stmt::VarDecl {names,..} if names.contains(&result))) {
                                helper_body.insert(0, Stmt::VarDecl {kind:kind.clone(),names:vec![result.clone()]});
                            }
                            helper_body.push(Stmt::DummyIntent {name:result.clone(),intent:"intent(out)".into()});
                            let mut helper_params = params.clone(); helper_params.push(result.clone());
                            out.push(Stmt::Subroutine {name:helper.clone(),params:helper_params,body:helper_body,pure:true,elemental:false});
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

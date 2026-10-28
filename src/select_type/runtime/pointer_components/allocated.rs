// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Concrete scalar allocatable targets use a one-element record descriptor.
//! Epochs distinguish successive allocations of the same declared target.
use super::*;

const BACKING: &str = "@allocated_target_storage:";
const EPOCH: &str = "@allocated_target_epoch:";

#[derive(Clone)]
pub(in crate::select_type::runtime) struct Info {
    pub backing: Option<String>,
    pub epoch: String,
}
pub(in crate::select_type::runtime) fn info(ds: &[Stmt]) -> Option<Info> {
    let attrs: Vec<_> = ds
        .iter()
        .filter_map(|s| match s {
            Stmt::DerivedDecl { attributes, .. } => Some(attributes),
            _ => None,
        })
        .flatten()
        .collect();
    Some(Info {
        epoch: attrs
            .iter()
            .find_map(|a| a.strip_prefix(EPOCH))
            .map(str::to_owned)?,
        backing: attrs
            .iter()
            .find_map(|a| a.strip_prefix(BACKING))
            .map(str::to_owned),
    })
}
pub(super) fn epoch_attribute(epoch: &str) -> String {
    format!("{EPOCH}{epoch}")
}
pub(in crate::select_type::runtime) fn element(n: &str, ds: &Declarations) -> Expr {
    ds.get(n)
        .and_then(|d| info(d))
        .and_then(|i| i.backing)
        .map_or_else(|| id(n), |b| Expr::Call(b, vec![int(1)]))
}
pub(in crate::select_type::runtime) fn valid(n: &str, epoch: &str, ds: &Declarations) -> Expr {
    ds.get(n)
        .and_then(|d| info(d))
        .map_or(Expr::Logical(true), |i| {
            Expr::Bin(
                BinOp::And,
                Box::new(Expr::Bin(
                    BinOp::Gt,
                    Box::new(id(&i.epoch)),
                    Box::new(int(0)),
                )),
                Box::new(Expr::Bin(
                    BinOp::Eq,
                    Box::new(id(epoch)),
                    Box::new(id(&i.epoch)),
                )),
            )
        })
}

pub(super) fn prepare(program: &mut Program) -> Result<()> {
    fn scope(body: &mut Vec<Stmt>, params: &[String], module: bool) -> Result<()> {
        let vars = super::super::super::objects(body, &Objects::new());
        let mut out = vec![];
        let mut entry = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    attributes,
                    polymorphic: false,
                    dims,
                } if dims.is_empty()
                    && names.iter().any(|n| {
                        vars.get(n).is_some_and(|v| {
                            v.attrs.iter().any(|a| a == "allocatable")
                                && v.attrs.iter().any(|a| a == "target")
                        })
                    }) =>
                {
                    for n in names.iter() {
                        if !vars.get(n).is_some_and(|v| {
                            v.attrs.iter().any(|a| a == "allocatable")
                                && v.attrs.iter().any(|a| a == "target")
                        }) {
                            out.push(Stmt::DerivedDecl {
                                type_name: type_name.clone(),
                                names: vec![n.clone()],
                                attributes: attributes.clone(),
                                polymorphic: false,
                                dims: vec![],
                            });
                            continue;
                        }
                        if params.contains(n) {
                            bail!("concrete scalar ALLOCATABLE TARGET dummy descriptors are not yet supported");
                        }
                        let backing = format!("{n}$@allocated_storage");
                        let epoch = format!("{n}$@allocated_epoch");
                        let mut attrs = attributes.clone();
                        attrs.push(format!("{BACKING}{backing}"));
                        attrs.push(epoch_attribute(&epoch));
                        out.push(Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![n.clone()],
                            attributes: attrs,
                            polymorphic: false,
                            dims: vec![],
                        });
                        out.push(Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![backing],
                            attributes: vec![
                                "allocatable".into(),
                                "target".into(),
                                "@scalar_heap".into(),
                                "@referenced_type".into(),
                            ],
                            polymorphic: false,
                            dims: vec![id("@deferred_extent")],
                        });
                        if module {
                            out.push(Stmt::InitializedDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                attributes: vec!["@module_initializer".into()],
                                entities: vec![(epoch, Some(int(0)))],
                            });
                        } else {
                            out.push(Stmt::VarDecl {
                                kind: TypeSpec::Integer(Some(8)),
                                names: vec![epoch.clone()],
                            });
                            entry.push(set(&epoch, 0));
                        }
                    }
                    continue;
                }
                Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                    scope(body, params, false)?
                }
                Stmt::Module { body, .. } => scope(body, &[], true)?,
                _ => {}
            }
            out.push(s);
        }
        // Place entry initialization after specifications, ahead of execution.
        let at = out
            .iter()
            .position(|s| {
                !matches!(
                    s,
                    Stmt::SourceLocation(_)
                        | Stmt::Implicit(_)
                        | Stmt::Use { .. }
                        | Stmt::VarDecl { .. }
                        | Stmt::ArrayDecl { .. }
                        | Stmt::DerivedDecl { .. }
                        | Stmt::InitializedDecl { .. }
                        | Stmt::DummyIntent { .. }
                        | Stmt::Accessibility { .. }
                        | Stmt::DerivedType { .. }
                        | Stmt::Function { .. }
                        | Stmt::Subroutine { .. }
                        | Stmt::Module { .. }
                        | Stmt::GenericInterface { .. }
                        | Stmt::OperatorInterface { .. }
                )
            })
            .unwrap_or(out.len());
        out.splice(at..at, entry);
        *body = out;
        Ok(())
    }
    scope(&mut program.body, &[], false)
}

/// Erase virtual scalar declarations only after pointer and interface lowering
/// have used their original TKR. Ordinary storage lowering sees record arrays.
pub(in crate::select_type::runtime) fn finish(
    program: &mut Program,
    defs: &Definitions,
) -> Result<()> {
    // Preserve source signatures for model expressions, including function
    // results. Interface provenance may follow the parser's final RETURN.
    fn model_options(body: &mut [Stmt], targets: &mut bool, models: &mut bool) {
        for s in body {
            *targets |= matches!(s,Stmt::DerivedDecl{attributes,..} if attributes.iter().any(|a|a.starts_with(BACKING)));
            *models |= matches!(s,Stmt::CallSub{name,args} if matches!(name.as_str(),"@allocate"|"@allocate_derived_type") && args.iter().any(|e|matches!(e,Expr::Call(n,_) if n=="@keyword:source"||n=="@keyword:mold")));
            for b in children(s) {
                model_options(b, targets, models);
            }
        }
    }
    let (mut has_targets, mut has_models) = (false, false);
    model_options(&mut program.body, &mut has_targets, &mut has_models);
    let types = if has_targets && has_models {
        let mut snapshot = program.clone();
        crate::imports::clear_declarations(&mut snapshot.body);
        Some(crate::operators::OwnerInterfaces::new(
            &snapshot,
            HashSet::new(),
            defs.parents.clone(),
        )?)
    } else {
        None
    };
    fn expression(e: &mut Expr, targets: &HashMap<String, Info>) {
        visit_expr(e, &mut |e| match e {
            Expr::Ident(n) => {
                let (root, field) = n
                    .split_once('%')
                    .map_or((n.as_str(), None), |(a, b)| (a, Some(b)));
                if let Some(b) = targets.get(root).and_then(|i| i.backing.as_ref()) {
                    *e = if let Some(f) = field {
                        Expr::Call(
                            format!("@record_component:{f}"),
                            vec![Expr::Call(b.clone(), vec![int(1)])],
                        )
                    } else {
                        Expr::Call(b.clone(), vec![int(1)])
                    };
                }
            }
            Expr::Call(n, args) if n.contains('%') => {
                let (root, field) = n.split_once('%').unwrap();
                if let Some(b) = targets.get(root).and_then(|i| i.backing.as_ref()) {
                    let mut a = vec![Expr::Call(b.clone(), vec![int(1)])];
                    if !args.is_empty() {
                        a.push(Expr::Call("@component_subscripts".into(), args.clone()));
                    }
                    *e = Expr::Call(format!("@record_component:{field}"), a);
                }
            }
            _ => {}
        });
    }
    fn scope(
        body: &mut Vec<Stmt>,
        parent: &HashMap<String, Info>,
        parent_decls: &Declarations,
        types: Option<&crate::operators::OwnerInterfaces>,
        origin: &str,
        serial: &mut usize,
    ) -> Result<()> {
        let declared = if types.is_some() {
            declarations(body, parent_decls)
        } else {
            Declarations::new()
        };
        let model_scope = types.map(|t| t.scope(body, origin));
        let mut derived = HashMap::new();
        if types.is_some() {
            for (n, ds) in &declared {
                for s in ds {
                    match s {
                        Stmt::DerivedDecl {
                            type_name, dims, ..
                        } => {
                            derived.insert(n.clone(), (type_name.clone(), dims.len()));
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut targets = parent.clone();
        for (n, ds) in declarations(body, &Declarations::new()) {
            if let Some(i) = info(&ds) {
                targets.insert(n, i);
            } else {
                targets.remove(&n);
            }
        }
        let mut out = vec![];
        let mut temporaries = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = &s {
                location = Some(*p);
            }
            match &mut s {
                Stmt::Function { name, body, .. }
                | Stmt::Subroutine { name, body, .. }
                | Stmt::Module { name, body, .. } => {
                    scope(body, &targets, &declared, types, name, serial)?;
                    out.push(s);
                    continue;
                }
                Stmt::DerivedType { .. } => {
                    out.push(s);
                    continue;
                }
                Stmt::DerivedDecl {
                    names, attributes, ..
                } => {
                    attributes.retain(|a| !a.starts_with(EPOCH) && !a.starts_with(BACKING));
                    for n in names
                        .iter()
                        .filter(|n| targets.get(*n).is_some_and(|i| i.backing.is_some()))
                    {
                        out.push(crate::imports::declaration(n.clone()));
                    }
                    names.retain(|n| !targets.get(n).is_some_and(|i| i.backing.is_some()));
                    if names.is_empty() {
                        continue;
                    }
                }
                Stmt::DummyIntent { name, .. }
                    if targets.get(name).is_some_and(|i| i.backing.is_some()) =>
                {
                    if let Some(b) = &targets[name].backing {
                        *name = b.clone();
                    }
                }
                _ => {}
            }
            let mut suffix = vec![];
            let mut allocation_status = None;
            if let Stmt::Assign { name, .. } = &s {
                if let Some(i) = targets.get(name).filter(|i| i.backing.is_some()) {
                    let backing = i.backing.as_ref().unwrap();
                    out.push(Stmt::If {
                        cond: Expr::not(Expr::Call("allocated".into(), vec![id(backing)])),
                        then_body: vec![
                            Stmt::CallSub {
                                name: "@allocate".into(),
                                args: vec![Expr::Call(backing.clone(), vec![int(1)])],
                            },
                            Stmt::Assign {
                                name: i.epoch.clone(),
                                value: Expr::Bin(
                                    BinOp::Add,
                                    Box::new(Expr::Call("abs".into(), vec![id(&i.epoch)])),
                                    Box::new(int(1)),
                                ),
                            },
                        ],
                        else_body: None,
                    });
                }
            }
            if let Stmt::CallSub { name, args } = &mut s {
                if matches!(
                    name.as_str(),
                    "@allocate" | "@deallocate" | "@allocate_derived_type"
                ) {
                    let allocating = name != "@deallocate";
                    let selected: Vec<_> = args
                        .iter()
                        .filter_map(|a| {
                            if let Expr::Ident(n) = a {
                                targets
                                    .get(n)
                                    .filter(|i| i.backing.is_some())
                                    .map(|i| (n.clone(), i.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let mut source_value=None;
                    if allocating && !selected.is_empty() {
                        let models:Vec<_>=args.iter().filter_map(|e| match e {
                            Expr::Call(n,a) if (n=="@keyword:source"||n=="@keyword:mold") && a.len()==1=>Some((n.clone(),a[0].clone())),_=>None,
                        }).collect();
                        if !models.is_empty() {
                            let validation=(||->Result<()> {
                                if models.len()!=1 {bail!("ALLOCATE permits only one SOURCE or MOLD specifier");}
                                if name=="@allocate_derived_type" {bail!("ALLOCATE type-spec cannot be combined with SOURCE or MOLD");}
                                if args.iter().filter(|e| !matches!(e,Expr::Call(n,_) if n.starts_with("@keyword:"))).count()!=selected.len() {bail!("mixed SOURCE/MOLD allocation lists with scalar ALLOCATABLE TARGET records are not yet supported");}
                                let types=types.context("missing allocation expression types")?;
                                let model_scope=model_scope.as_ref().context("missing allocation model scope")?;
                                if types.dynamic(&models[0].1,model_scope) {bail!("polymorphic SOURCE/MOLD expressions for concrete scalar ALLOCATABLE TARGET records are not yet supported");}
                                let Some((kind,0))=types.derived(&models[0].1,model_scope)? else {bail!("scalar ALLOCATABLE TARGET allocation requires a scalar derived-type SOURCE or MOLD expression");};
                                for (n,_) in &selected {
                                    if derived.get(n).is_none_or(|(expected,_)|expected!=&kind) {bail!("SOURCE/MOLD type must match the declared type of nonpolymorphic allocation target `{n}`");}
                                }
                                if models[0].0=="@keyword:source" {
                                    *serial+=1;
                                    let temp=format!("@allocated_source{serial}");
                                    temporaries.push(Stmt::DerivedDecl{type_name:kind,names:vec![temp.clone()],attributes:vec!["@referenced_type".into()],polymorphic:false,dims:vec![]});
                                    let mut value=models[0].1.clone();expression(&mut value,&targets);
                                    out.push(Stmt::CallSub{name:crate::operators::INTRINSIC_ASSIGNMENT.into(),args:vec![id(&temp),value]});
                                    source_value=Some(id(&temp));
                                }
                                Ok(())
                            })();
                            validation.map_err(|e|crate::source_diagnostics::at(e,location))?;
                            args.retain(|e|!matches!(e,Expr::Call(n,_) if n=="@keyword:source"||n=="@keyword:mold"));
                        }
                    }
                    if !selected.is_empty() {
                        let stat = args.iter().find_map(|e| match e { Expr::Call(n,a) if n=="@keyword:stat" && a.len()==1 => Some(a[0].clone()), _=>None });
                        let errmsg = args.iter().find_map(|e| match e { Expr::Call(n,a) if n=="@keyword:errmsg" && a.len()==1 => Some(a[0].clone()), _=>None });
                        if let Some(stat) = stat {
                            if args.iter().filter(|e| !matches!(e,Expr::Call(n,_) if n.starts_with("@keyword:")) && !matches!(e,Expr::Str(_))).count()!=selected.len() {
                                bail!("mixed scalar ALLOCATABLE TARGET allocation lists with STAT are not yet supported");
                            }
                            let failed = selected.iter().map(|(_,i)| {
                                let a=Expr::Call("allocated".into(),vec![id(i.backing.as_ref().unwrap())]);
                                if allocating {a} else {Expr::not(a)}
                            }).reduce(|a,b|Expr::Bin(BinOp::Or,Box::new(a),Box::new(b))).unwrap();
                            allocation_status=Some((failed,stat,errmsg));
                            args.retain(|e| !matches!(e,Expr::Call(n,_) if n=="@keyword:stat" || n=="@keyword:errmsg"));
                        }
                    }
                    for (n, i) in selected {
                        for a in args.iter_mut() {
                            if *a == id(&n) {
                                *a = if allocating {
                                    Expr::Call(i.backing.clone().unwrap(), vec![int(1)])
                                } else {
                                    id(i.backing.as_ref().unwrap())
                                };
                            }
                        }
                        let status =
                            Expr::Call("allocated".into(), vec![id(i.backing.as_ref().unwrap())]);
                        suffix.push(Stmt::If {
                            cond: if allocating {
                                Expr::Bin(
                                    BinOp::And,
                                    Box::new(status),
                                    Box::new(Expr::Bin(
                                        BinOp::Le,
                                        Box::new(id(&i.epoch)),
                                        Box::new(int(0)),
                                    )),
                                )
                            } else {
                                Expr::not(status)
                            },
                            then_body: vec![Stmt::Assign {
                                name: i.epoch.clone(),
                                value: if allocating {
                                    Expr::Bin(
                                        BinOp::Add,
                                        Box::new(Expr::Call("abs".into(), vec![id(&i.epoch)])),
                                        Box::new(int(1)),
                                    )
                                } else {
                                    Expr::Un(
                                        crate::ast::UnOp::Neg,
                                        Box::new(Expr::Call("abs".into(), vec![id(&i.epoch)])),
                                    )
                                },
                            }],
                            else_body: None,
                        });
                        if let Some(value)=&source_value {
                            suffix.push(Stmt::AssignIndex{name:i.backing.clone().unwrap(),indices:vec![int(1)],value:value.clone()});
                        }
                    }
                } else if matches!(name.as_str(),"move_alloc"|"@move_alloc") && args.iter().any(|a| {let mut found=false;visit_expr(&mut a.clone(),&mut |e| {if matches!(e,Expr::Ident(n) if targets.get(n).is_some_and(|i|i.backing.is_some())) {found=true;}});found}) {
                    return Err(crate::source_diagnostics::at(anyhow::anyhow!("MOVE_ALLOC of scalar ALLOCATABLE TARGET records requires a movable allocation identity and is not yet supported"),location));
                } else if let Some((root, method)) = name.split_once('%') {
                    if let Some(b) = targets.get(root).and_then(|i| i.backing.as_ref()) {
                        args.insert(0, Expr::Call(b.clone(), vec![int(1)]));
                        *name = format!("@element_call:{method}");
                    }
                }
            }
            // ALLOCATED is an inquiry on the descriptor, not its scalar value.
            for e in expressions(&mut s) {
                visit_expr(e, &mut |e| {
                    if let Expr::Call(n, a) = e {
                        if n == "allocated" && a.len() == 1 {
                            let actual = match &a[0] {
                                Expr::Call(n, v) if n == "@keyword:scalar" && v.len() == 1 => &v[0],
                                e => e,
                            };
                            if let Expr::Ident(v) = actual {
                                if let Some(b) = targets.get(v).and_then(|i| i.backing.as_ref()) {
                                    a[0] = id(b);
                                }
                            }
                        }
                    }
                });
                expression(e, &targets);
            }
            if let Stmt::Assign { name, value } = &s {
                let (root, field) = name
                    .split_once('%')
                    .map_or((name.as_str(), None), |(a, b)| (a, Some(b)));
                if let Some(b) = targets.get(root).and_then(|i| i.backing.as_ref()) {
                    s = Stmt::AssignIndex {
                        name: field.map_or_else(|| b.clone(), |f| format!("{b}%{f}")),
                        indices: vec![int(1)],
                        value: value.clone(),
                    };
                }
            }
            if let Stmt::AssignIndex { name, indices, .. } = &mut s {
                if let Some((root, field)) = name.split_once('%') {
                    if let Some(b) = targets.get(root).and_then(|i| i.backing.as_ref()) {
                        *name = format!("{b}%{field}");
                        let component = std::mem::take(indices);
                        *indices = vec![
                            int(1),
                            Expr::Call("@component_subscripts".into(), component),
                        ];
                    }
                }
            }
            for child in children(&mut s) {
                scope(child, &targets, &declared, types, origin, serial)?;
            }
            if let Some((failed, stat, errmsg)) = allocation_status {
                let mut success = vec![s];
                success.extend(suffix);
                success.push(super::assign(&stat, int(0))?);
                let mut failure = vec![super::assign(&stat, int(1))?];
                if let Some(errmsg) = errmsg {
                    failure.push(super::assign(
                        &errmsg,
                        Expr::Str("invalid scalar target allocation status".into()),
                    )?);
                }
                out.push(Stmt::If {
                    cond: failed,
                    then_body: failure,
                    else_body: Some(success),
                });
            } else {
                out.push(s);
                out.extend(suffix);
            }
        }
        temporaries.extend(out);
        *body = temporaries;
        Ok(())
    }
    let mut globals = HashMap::new();
    let mut global_decls = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            if types.is_some() {
                global_decls.extend(declarations(body, &Declarations::new()));
            }
            for (n, d) in declarations(body, &Declarations::new()) {
                if let Some(i) = info(&d) {
                    globals.insert(n, i);
                }
            }
        }
    }
    scope(
        &mut program.body,
        &globals,
        &global_decls,
        types.as_ref(),
        "",
        &mut 0,
    )
}

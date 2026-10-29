// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Indexed owners keep allocation identities in their elements. Borrowed
//! scalar-owner calls reuse the scalar component descriptor implementation.
use super::*;

const ARRAY: &str = "@allocated_array_component:";
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(in crate::select_type::runtime) struct Target {
    pub owner: String,
    pub owner_type: String,
    pub field: String,
    pub epoch: String,
    pub rank: usize,
    pub readonly: bool,
}
pub(super) fn target(ds: &[Stmt]) -> Option<Target> {
    ds.iter().find_map(|s| {
        if let Stmt::DerivedDecl { attributes, .. } = s {
            attributes.iter().find_map(|a| {
                a.strip_prefix(ARRAY)
                    .and_then(|s| serde_json::from_str(s).ok())
            })
        } else {
            None
        }
    })
}
pub(super) fn association(e: &Expr, declared: &Declarations) -> Option<(String, Expr)> {
    let Expr::Call(tag, args) = e else {
        return None;
    };
    let field = tag.strip_prefix("@record_component:")?;
    let Expr::Call(owner, indices) = args.first()? else {
        return None;
    };
    declared.iter().find_map(|(alias, ds)| {
        let t = target(ds)?;
        (t.owner == *owner && t.field == field).then(|| {
            (
                alias.clone(),
                Expr::Call(format!("{}%{}", owner, t.epoch), indices.clone()),
            )
        })
    })
}
impl Target {
    pub fn attribute(&self) -> String {
        format!("{ARRAY}{}", serde_json::to_string(self).unwrap())
    }
    pub fn position(&self, epoch: Expr) -> Expr {
        Expr::Call(
            format!("@component_allocation_position_{}", self.rank),
            vec![id(&format!("{}%{}", self.owner, self.epoch)), epoch],
        )
    }
    pub fn element(&self, epoch: Expr) -> Expr {
        let mut offset = Expr::Bin(BinOp::Sub, Box::new(self.position(epoch)), Box::new(int(1)));
        let mut indices = vec![];
        for axis in 1..=self.rank {
            let size = Expr::Call("size".into(), vec![id(&self.owner), int(axis)]);
            let lower = Expr::Call("lbound".into(), vec![id(&self.owner), int(axis)]);
            indices.push(Expr::Bin(
                BinOp::Add,
                Box::new(lower),
                Box::new(Expr::Call("mod".into(), vec![offset.clone(), size.clone()])),
            ));
            offset = Expr::Bin(BinOp::Div, Box::new(offset), Box::new(size));
        }
        Expr::Call(
            format!("@record_component:{}", self.field),
            vec![
                Expr::Call(self.owner.clone(), indices),
                Expr::Call("@component_subscripts".into(), vec![int(1)]),
            ],
        )
    }
}
fn position_function(rank: usize) -> Stmt {
    let name = format!("@component_allocation_position_{rank}");
    let mut body = vec![
        Stmt::ArrayDecl {
            kind: TypeSpec::Integer(Some(8)),
            name: "epochs".into(),
            dims: vec![id("@deferred_extent"); rank],
        },
        Stmt::DummyIntent {
            name: "epochs".into(),
            intent: "intent(in)".into(),
        },
        Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec!["epoch".into(), "position".into(), "count".into()],
        },
        Stmt::DummyIntent {
            name: "epoch".into(),
            intent: "intent(in)".into(),
        },
        set("position", 0),
        set("count", 0),
    ];
    let indices = (0..rank)
        .map(|a| format!("@component_axis{a}"))
        .collect::<Vec<_>>();
    body.push(Stmt::VarDecl {
        kind: TypeSpec::Integer(Some(8)),
        names: indices.clone(),
    });
    body.push(Stmt::If {
        cond: Expr::Bin(BinOp::Le, Box::new(id("epoch")), Box::new(int(0))),
        then_body: vec![Stmt::Return(Some(id("position")))],
        else_body: None,
    });
    let mut scan = vec![
        Stmt::Assign {
            name: "count".into(),
            value: Expr::Bin(BinOp::Add, Box::new(id("count")), Box::new(int(1))),
        },
        Stmt::If {
            cond: Expr::Bin(
                BinOp::Eq,
                Box::new(Expr::Call(
                    "epochs".into(),
                    indices.iter().map(|n| id(n)).collect(),
                )),
                Box::new(id("epoch")),
            ),
            then_body: vec![
                Stmt::Assign {
                    name: "position".into(),
                    value: id("count"),
                },
                Stmt::Return(Some(id("position"))),
            ],
            else_body: None,
        },
    ];
    for (axis, var) in indices.into_iter().enumerate() {
        scan = vec![Stmt::Do {
            var,
            start: int(1),
            end: Expr::Call("size".into(), vec![id("epochs"), int(axis + 1)]),
            body: scan,
        }];
    }
    body.extend(scan);
    body.push(Stmt::Return(Some(id("position"))));
    Stmt::Function {
        elemental: false,
        pure: true,
        name,
        params: vec!["epochs".into(), "epoch".into()],
        return_type: Some(TypeSpec::Integer(Some(8))),
        body,
    }
}

pub(super) fn prepare(
    program: &mut Program,
    paths: &HashMap<String, Vec<(String, String, String)>>,
) -> Result<()> {
    super::dummies::validate_component_inputs(program)?;
    fn readonly(ds: &[Stmt]) -> bool {
        ds.iter().any(|s| match s {
            Stmt::DerivedDecl { attributes, .. } => attributes.iter().any(|a| a == "intent(in)"),
            Stmt::DummyIntent { intent, .. } => intent == "intent(in)",
            _ => false,
        })
    }
    fn collect(
        kind: &str,
        prefix: &str,
        paths: &HashMap<String, Vec<(String, String, String)>>,
        seen: &mut HashSet<String>,
        out: &mut Vec<(String, String, String)>,
    ) {
        if !seen.insert(kind.into()) {
            return;
        }
        for (f, ty, epoch) in paths.get(kind).into_iter().flatten() {
            let field = format!("{prefix}{f}");
            if epoch.is_empty() {
                collect(ty, &format!("{field}%"), paths, seen, out);
            } else {
                out.push((field, ty.clone(), format!("{prefix}{epoch}")));
            }
        }
        seen.remove(kind);
    }
    fn scope(
        body: &mut Vec<Stmt>,
        parent: &HashMap<(String, String), (Target, String)>,
        declared_parent: &Declarations,
        paths: &HashMap<String, Vec<(String, String, String)>>,
        generated: &mut Vec<Stmt>,
        ranks: &mut HashSet<usize>,
        serial: &mut usize,
        pure: bool,
    ) -> Result<()> {
        let declared = declarations(body, declared_parent);
        let mut arrays = parent.clone();
        for owner in super::super::super::super::objects(body, &Objects::new()).keys() {
            arrays.retain(|(o, _), _| o != owner);
        }
        let mut declarations_out = vec![];
        for (owner, ds) in &declared {
            for d in ds {
                if let Stmt::DerivedDecl {
                    type_name,
                    attributes,
                    dims,
                    polymorphic: false,
                    ..
                } = d
                {
                    if dims.is_empty()
                        || !(attributes.iter().any(|a| a == "target")
                            || ds.iter().any(
                                |s| matches!(s,Stmt::DummyIntent{intent,..} if intent=="target"),
                            ))
                        || attributes
                            .iter()
                            .any(|a| a == "allocatable" || a == "pointer")
                    {
                        continue;
                    }
                    let mut fields = vec![];
                    collect(type_name, "", paths, &mut HashSet::new(), &mut fields);
                    for (field, kind, epoch) in fields {
                        let t = Target {
                            owner: owner.clone(),
                            owner_type: type_name.clone(),
                            field: field.clone(),
                            epoch,
                            rank: dims.len(),
                            readonly: readonly(ds),
                        };
                        let alias = if owner.starts_with("@module$") {
                            format!("{owner}$@array_component${}", field.replace('%', "$"))
                        } else {
                            format!("@array_component${owner}${}", field.replace('%', "$"))
                        };
                        if !arrays.contains_key(&(owner.clone(), field.clone())) {
                            declarations_out.push(Stmt::DerivedDecl {
                                type_name: kind,
                                names: vec![alias.clone()],
                                attributes: vec![
                                    "allocatable".into(),
                                    "target".into(),
                                    "@referenced_type".into(),
                                    backing_attribute(&format!("{alias}$storage")),
                                    epoch_attribute(&format!("{alias}$epoch")),
                                    t.attribute(),
                                ],
                                polymorphic: false,
                                dims: vec![],
                            });
                        }
                        ranks.insert(dims.len());
                        arrays.insert((owner.clone(), field), (t, alias));
                    }
                }
            }
        }
        // Wrap only executable leaf statements. Index expressions are actual
        // arguments, so the record-element call machinery evaluates them once.
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = s {
                location = Some(p);
                out.push(s);
                continue;
            }
            if matches!(s, Stmt::DerivedType { .. }) {
                out.push(s);
                continue;
            }
            let nested_pure = match &s {
                Stmt::Function { pure, .. } | Stmt::Subroutine { pure, .. } => *pure,
                Stmt::Module { .. } => false,
                _ => pure,
            };
            for b in children(&mut s) {
                scope(
                    b,
                    &arrays,
                    &declared,
                    paths,
                    generated,
                    ranks,
                    serial,
                    nested_pure,
                )?;
            }
            if matches!(
                s,
                Stmt::Module { .. } | Stmt::Subroutine { .. } | Stmt::Function { .. }
            ) {
                out.push(s);
                continue;
            }
            let mut elements: Vec<(Expr, String, String)> = vec![];
            let mut record = |element: &Expr, field: &str| {
                if let Expr::Call(owner, indices) | Expr::Index(owner, indices) = element {
                    if arrays.keys().any(|(o, f)| {
                        o == owner && (field == f || field.starts_with(&format!("{f}%")))
                    }) && !elements.iter().any(|(e, _, _)| e == element)
                    {
                        let kind = arrays
                            .iter()
                            .find(|((o, _), _)| o == owner)
                            .unwrap()
                            .1
                             .0
                            .owner_type
                            .clone();
                        elements.push((
                            element.clone(),
                            format!("@component_owner{}", elements.len()),
                            kind,
                        ));
                        let _ = indices;
                    }
                }
            };
            if matches!(
                s,
                Stmt::CallSub { .. }
                    | Stmt::Assign { .. }
                    | Stmt::AssignIndex { .. }
                    | Stmt::Read { .. }
                    | Stmt::Print { .. }
            ) && !matches!(&s,Stmt::CallSub{name,..} if name=="@pointer_associate")
                && !declared.iter().any(|(n,ds)|uses(&s,n) && ds.iter().any(|d|matches!(d,Stmt::DerivedDecl{polymorphic:true,attributes,..} if attributes.iter().any(|a|a=="pointer"))))
            {
                for e in expressions(&mut s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(tag, args) = e {
                            if let Some(field) = tag.strip_prefix("@record_component:") {
                                if let Some(element) = args.first() {
                                    record(element, field);
                                }
                            }
                        }
                    });
                }
                if let Stmt::AssignIndex { name, indices, .. } = &s {
                    if let Some((owner, field)) = name.split_once('%') {
                        let count = indices.len()
                            - usize::from(
                                matches!(indices.last(),Some(Expr::Call(n,_)) if n=="@component_subscripts"),
                            );
                        record(&Expr::Call(owner.into(), indices[..count].to_vec()), field);
                    }
                }
            }
            if elements.is_empty() {
                out.push(s);
                continue;
            }
            *serial += 1;
            let name = format!("@indexed_component_call{serial}");
            let mut params = vec![];
            let mut args = vec![];
            let mut helper = vec![];
            for (element, param, kind) in &elements {
                params.push(param.clone());
                args.push(element.clone());
                let input = matches!(element,Expr::Call(owner,_)|Expr::Index(owner,_) if declared.get(owner).is_some_and(|ds|readonly(ds)));
                helper.push(Stmt::DerivedDecl {
                    type_name: kind.clone(),
                    names: vec![param.clone()],
                    attributes: vec![
                        "target".into(),
                        if input { "intent(in)" } else { "intent(inout)" }.into(),
                        "@referenced_type".into(),
                    ],
                    polymorphic: false,
                    dims: vec![],
                });
            }
            for e in expressions(&mut s) {
                visit_expr(e, &mut |e| {
                    if let Expr::Call(tag, actuals) = e {
                        if let Some(field) = tag.strip_prefix("@record_component:") {
                            if let Some((_, param, _)) = elements
                                .iter()
                                .find(|(element, _, _)| actuals.first() == Some(element))
                            {
                                let path = format!("{param}%{field}");
                                *e = if let Some(Expr::Call(n, subs)) = actuals.get(1) {
                                    if n == "@component_subscripts" {
                                        Expr::Call(path, subs.clone())
                                    } else {
                                        id(&path)
                                    }
                                } else {
                                    id(&path)
                                };
                            }
                        }
                    }
                });
            }
            if let Stmt::AssignIndex { name, indices, .. } = &mut s {
                if let Some((owner, field)) = name.split_once('%') {
                    let count = indices.len()
                        - usize::from(
                            matches!(indices.last(),Some(Expr::Call(n,_)) if n=="@component_subscripts"),
                        );
                    if let Some((_, param, _)) = elements
                        .iter()
                        .find(|(e, _, _)| e == &Expr::Call(owner.into(), indices[..count].to_vec()))
                    {
                        *name = format!("{param}%{field}");
                        *indices = if let Some(Expr::Call(_, sub)) = indices.get(count) {
                            sub.clone()
                        } else {
                            vec![]
                        };
                    }
                }
            }
            if let Stmt::AssignIndex {
                name,
                indices,
                value,
            } = &s
            {
                if indices.is_empty() {
                    s = Stmt::Assign {
                        name: name.clone(),
                        value: value.clone(),
                    };
                }
            }
            let mut captures = declared
                .keys()
                .filter(|n| uses(&s, n))
                .cloned()
                .collect::<HashSet<_>>();
            loop {
                let previous = captures.len();
                for n in captures.clone() {
                    if let Some(i) = declared.get(&n).and_then(|d| info(d)) {
                        if let Some(a) = i.array {
                            captures.insert(a.owner);
                        } else {
                            captures.insert(i.epoch);
                            if let Some(b) = i.backing {
                                captures.insert(b);
                            }
                        }
                    }
                }
                for n in declared.keys() {
                    if captures.iter().any(|c| {
                        declared
                            .get(c)
                            .is_some_and(|ds| ds.iter().any(|d| uses(d, n)))
                    }) {
                        captures.insert(n.clone());
                    }
                }
                if captures.len() == previous {
                    break;
                }
            }
            for (n, ds) in &declared {
                if !captures.contains(n) {
                    continue;
                }
                if n.contains('%') {
                    continue;
                }
                let virtual_view = info(ds).is_some();
                let constant=ds.iter().any(|d|matches!(d,Stmt::InitializedDecl{attributes,..} if attributes.iter().any(|a|a=="parameter")));
                if !constant && !virtual_view {
                    params.push(n.clone());
                    args.push(id(n));
                }
                for mut d in ds.clone() {
                    if let Stmt::DerivedDecl { attributes, .. } = &mut d {
                        attributes.retain(|a| !a.starts_with("intent("));
                        attributes.push("@referenced_type".into());
                    }
                    if !matches!(d, Stmt::DummyIntent { .. }) {
                        helper.push(d);
                    }
                }
                if !constant && !virtual_view {
                    helper.push(Stmt::DummyIntent {
                        name: n.clone(),
                        intent: if readonly(ds) {
                            "intent(in)"
                        } else {
                            "intent(inout)"
                        }
                        .into(),
                    });
                }
            }
            if let Some(p) = location {
                helper.push(Stmt::SourceLocation(p));
            }
            helper.push(s);
            generated.push(Stmt::Subroutine {
                elemental: false,
                pure,
                name: name.clone(),
                params,
                body: helper,
            });
            out.push(Stmt::CallSub { name, args });
        }
        declarations_out.extend(out);
        *body = declarations_out;
        Ok(())
    }
    let mut generated = vec![];
    let mut ranks = HashSet::new();
    let mut globals = Declarations::new();
    for s in &program.body {
        if let Stmt::Module { body, .. } = s {
            globals.extend(declarations(body, &Declarations::new()));
        }
    }
    scope(
        &mut program.body,
        &HashMap::new(),
        &globals,
        paths,
        &mut generated,
        &mut ranks,
        &mut 0,
        false,
    )?;
    let mut static_views = vec![];
    program.body.retain(|s| {
        if target(std::slice::from_ref(s)).is_some_and(|t| t.owner.starts_with("@module$")) {
            static_views.push(s.clone());
            false
        } else {
            true
        }
    });
    for s in &mut program.body {
        if let Stmt::Module { body, .. } = s {
            let local = declarations(body, &Declarations::new());
            for view in &static_views {
                if target(std::slice::from_ref(view)).is_some_and(|t| local.contains_key(&t.owner))
                {
                    if !body.iter().any(|s| {
                        target(std::slice::from_ref(s))
                            .zip(target(std::slice::from_ref(view)))
                            .is_some_and(|(a, b)| a.owner == b.owner && a.field == b.field)
                    }) {
                        body.push(view.clone());
                    }
                }
            }
        }
    }
    let mut ranks = ranks.into_iter().collect::<Vec<_>>();
    ranks.sort();
    for rank in ranks {
        generated.push(position_function(rank));
    }
    crate::finalization::generated_component_moves(program, &mut generated)?;
    program
        .contains
        .extend(generated.iter().filter_map(|s| match s {
            Stmt::Subroutine { name, .. } | Stmt::Function { name, .. } => Some(name.clone()),
            _ => None,
        }));
    program.body.extend(generated);
    Ok(())
}

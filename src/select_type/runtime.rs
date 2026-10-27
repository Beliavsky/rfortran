// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Runtime type tags and owned, one-element concrete allocation descriptors.
//! Runtime branches call ordinary CLASS specializations; concrete storage is
//! allocated only for the selected type, and is released through record-array
//! allocation lowering (including owned components).
use super::*;
use crate::ast::{BinOp, CaseBlock, TypeSpec};
use std::collections::BTreeMap;
mod pointers;
mod components;
mod pointer_components;

#[derive(Clone)]
struct Object {
    display: String,
    kind: String,
    polymorphic: bool,
    pointer: bool,
    member: bool,
    readonly: bool,
    dummy: bool,
    output: bool,
    tag: String,
    variants: Vec<(String, String)>,
}
type Runtime = BTreeMap<String, Object>;
type Declarations = BTreeMap<String, Vec<Stmt>>;
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn set(n: &str, v: usize) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: int(v),
    }
}
fn failure(message: String) -> Vec<Stmt> {
    vec![Stmt::CallSub {
        name: "@error_stop".into(),
        args: vec![Expr::Str(message)],
    }]
}
fn cases(object: &Object, bodies: Vec<Vec<Stmt>>, default: Vec<Stmt>) -> Stmt {
    Stmt::SelectCase {
        expr: id(&object.tag),
        cases: bodies
            .into_iter()
            .enumerate()
            .map(|(i, body)| CaseBlock {
                items: vec![CaseItem::Single(int(i + 1))],
                body,
            })
            .collect(),
        default: Some(default),
    }
}
fn declarations(body: &[Stmt], parent: &Declarations) -> Declarations {
    let mut out = parent.clone();
    for s in body {
        let entries: Vec<(String, Stmt)> = match s {
            Stmt::VarDecl { kind, names } => names
                .iter()
                .map(|n| {
                    (
                        n.clone(),
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![n.clone()],
                        },
                    )
                })
                .collect(),
            Stmt::ArrayDecl { name, .. } => vec![(name.clone(), s.clone())],
            Stmt::DerivedDecl {
                type_name,
                names,
                attributes,
                polymorphic,
                dims,
            } => names
                .iter()
                .map(|n| {
                    (
                        n.clone(),
                        Stmt::DerivedDecl {
                            type_name: type_name.clone(),
                            names: vec![n.clone()],
                            attributes: attributes.clone(),
                            polymorphic: *polymorphic,
                            dims: dims.clone(),
                        },
                    )
                })
                .collect(),
            Stmt::InitializedDecl {
                kind,
                attributes,
                entities,
            } => entities
                .iter()
                .map(|(n, v)| {
                    let d = if attributes.iter().any(|a| a == "parameter") {
                        Stmt::InitializedDecl {
                            kind: kind.clone(),
                            attributes: attributes.clone(),
                            entities: vec![(n.clone(), v.clone())],
                        }
                    } else {
                        Stmt::VarDecl {
                            kind: kind.clone(),
                            names: vec![n.clone()],
                        }
                    };
                    (n.clone(), d)
                })
                .collect(),
            _ => vec![],
        };
        for (n, d) in entries {
            out.insert(n, vec![d]);
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if let Some(d) = out.get_mut(name) {
                if !intent.starts_with('@') && !matches!(intent.as_str(), "save" | "parameter") {
                    d.push(s.clone());
                }
            }
        }
    }
    out
}
fn uses(s: &Stmt, n: &str) -> bool {
    let matches = |v: &str| v == n || v.strip_prefix(n).is_some_and(|r| r.starts_with('%'));
    let mut s = s.clone();
    if matches!(&s,Stmt::Assign{name,..}|Stmt::AssignIndex{name,..}|Stmt::CallSub{name,..}|Stmt::Do{var:name,..} if matches(name))
    {
        return true;
    }
    let mut found = false;
    for e in expressions(&mut s) {
        visit_expr(e, &mut |e| {
            if matches!(e,Expr::Ident(v)|Expr::Call(v,_)|Expr::Index(v,_)|Expr::Section{name:v,..} if matches(v))
            {
                found = true;
            }
        });
    }
    found
        || children(&mut s)
            .into_iter()
            .any(|b| b.iter().any(|s| uses(s, n)))
}
#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    dummies: HashMap<usize, (String, String)>,
}
fn procedures(body: &[Stmt], out: &mut HashMap<String, Procedure>) {
    for s in body {
        match s {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => {
                let vars = super::objects(body, &Objects::new());
                let dummies = params
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| {
                        let v = vars.get(n)?;
                        (v.polymorphic && v.attrs.iter().any(|a| a == "allocatable")).then(|| {
                            (
                                i,
                                (
                                    v.kind.clone(),
                                    v.attrs
                                        .iter()
                                        .find(|a| a.starts_with("intent("))
                                        .cloned()
                                        .unwrap_or_else(|| "intent(inout)".into()),
                                ),
                            )
                        })
                    })
                    .collect::<HashMap<_, _>>();
                if !dummies.is_empty() {
                    out.insert(
                        name.clone(),
                        Procedure {
                            signature: crate::named_arguments::Signature::new(params, body),
                            dummies,
                        },
                    );
                }
                procedures(body, out);
            }
            Stmt::Module { body, .. } | Stmt::Block { body } => procedures(body, out),
            _ => {}
        }
    }
}
struct Lower<'a> {
    defs: &'a Definitions,
    procedures: HashMap<String, Procedure>,
    generated: Vec<Stmt>,
    serial: usize,
}
impl Lower<'_> {
    fn scope(
        &mut self,
        body: &mut Vec<Stmt>,
        parent: &Declarations,
        params: &mut Vec<String>,
        pure: bool,
    ) -> Result<()> {
        let declared = declarations(body, parent);
        let vars = self.defs.component_objects(super::objects(body, &Objects::new()));
        let mut runtime = Runtime::new();
        for (n, v) in &vars {
            if !v.polymorphic || !v.attrs.iter().any(|a| a == "allocatable" || a == "pointer") {
                continue;
            }
            let pointer = v.attrs.iter().any(|a| a == "pointer");
            if v.rank != 0 {
                if pointer {bail!("runtime polymorphic POINTER arrays are not yet supported");}
                bail!("runtime polymorphic ALLOCATABLE arrays are not yet supported");
            }
            if pointer && v.kind == "*" {
                bail!("unlimited polymorphic POINTER descriptors are not yet supported");
            }
            if pointer && params.contains(n) {
                bail!("polymorphic POINTER dummy descriptors are not yet supported; use a nonpointer CLASS dummy to borrow the target");
            }
            if v.attrs.iter().any(|a| a == "save") {
                bail!("SAVED runtime polymorphic objects are not yet supported");
            }
            if params.contains(n) && v.attrs.iter().any(|a| a == "optional") {
                bail!("OPTIONAL polymorphic ALLOCATABLE dummy arguments are not yet supported");
            }
            let mut kinds: Vec<_> = self
                .defs
                .fields
                .keys()
                .filter(|k| {
                    !self.defs.abstracts.contains(*k) && self.defs.distance(k, &v.kind).is_some()
                })
                .cloned()
                .collect();
            kinds.sort();
            self.serial += 1;
            let member = n.contains('%');
            let stem = if member {components::stem(n)} else {format!("@polymorphic{}", self.serial)};
            runtime.insert(
                n.clone(),
                Object {
                    display: body.iter().find_map(|s|match s {
                        Stmt::CallSub{name,args} if name=="@pointer_component_targets" && args.first()==Some(&id(n))=>
                            args.last().and_then(|e|if let Expr::Str(label)=e {Some(label.clone())}else{None}),
                        _=>None,
                    }).unwrap_or_else(||n.clone()),
                    kind: v.kind.clone(),
                    polymorphic: true,
                    pointer,
                    member,
                    readonly: v.attrs.iter().any(|a| a == "intent(in)"),
                    dummy: params.contains(n),
                    output: params.contains(n) && v.attrs.iter().any(|a| a == "intent(out)"),
                    tag: format!("{stem}$tag"),
                    variants: if pointer {
                        pointers::targets(n, &declared, &v.kind, self.defs, body)
                    } else {
                        kinds.into_iter().enumerate()
                            .map(|(i,k)|(k,format!("{stem}$storage{i}"))).collect()
                    },
                },
            );
        }
        // Procedures have independent allocation lifetimes. Module objects need
        // persistent descriptors and are intentionally diagnosed for now.
        for s in body.iter_mut() {
            match s {
                Stmt::Function {
                    body, params, pure, ..
                }
                | Stmt::Subroutine {
                    body, params, pure, ..
                } => self.scope(body, &declared, params, *pure)?,
                Stmt::Module { body, .. } => {
                    if super::objects(body, &Objects::new())
                        .values()
                        .any(|v| v.polymorphic && v.attrs.iter().any(|a| a == "allocatable" || a == "pointer"))
                    {
                        bail!("module polymorphic ALLOCATABLE or POINTER objects are not yet supported");
                    }
                    self.scope(body, &declared, &mut vec![], false)?;
                }
                _ => {}
            }
        }
        if runtime.is_empty() {
            self.body(body, &runtime, &declared, pure)?;
            return Ok(());
        }
        let mut cleaned = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::DerivedDecl { names, .. } => {
                    names.retain(|n| !runtime.contains_key(n));
                    if names.is_empty() {
                        continue;
                    }
                }
                Stmt::DummyIntent { name, .. } if runtime.contains_key(name) => continue,
                _ => {}
            }
            cleaned.push(s);
        }
        let mut storage = vec![];
        for object in runtime.values() {
            if object.member { continue; }
            storage.push(Stmt::VarDecl {
                kind: TypeSpec::Integer(None),
                names: vec![object.tag.clone()],
            });
            if object.dummy {
                storage.push(Stmt::DummyIntent {
                    name: object.tag.clone(),
                    intent: if object.readonly {
                        "intent(in)"
                    } else {
                        "intent(inout)"
                    }
                    .into(),
                });
            }
            for (kind, name) in &object.variants {
                if object.pointer { continue; }
                let mut attrs = vec!["allocatable".into(), "@scalar_heap".into(), "@referenced_type".into()];
                if object.dummy {
                    attrs.push(
                        if object.readonly {
                            "intent(in)"
                        } else {
                            "intent(inout)"
                        }
                        .into(),
                    );
                }
                storage.push(Stmt::DerivedDecl {
                    type_name: kind.clone(),
                    names: vec![name.clone()],
                    attributes: attrs,
                    polymorphic: false,
                    dims: vec![id("@deferred_extent")],
                });
            }
        }
        let declared = declarations(&storage, &declared);
        // Keep every specification statement ahead of generated executable
        // initialization. Automatic-array lowering inserts entry allocations
        // at this boundary and needs the storage declarations first.
        let at = cleaned
            .iter()
            .position(|s| {
                !matches!(
                    s,
                    Stmt::GenericInterface { .. }
                        | Stmt::OperatorInterface { .. }
                        | Stmt::DerivedType { .. }
                        | Stmt::DerivedDecl { .. }
                        | Stmt::Accessibility { .. }
                        | Stmt::DummyIntent { .. }
                        | Stmt::VarDecl { .. }
                        | Stmt::InitializedDecl { .. }
                        | Stmt::ArrayDecl { .. }
                        | Stmt::Use { .. }
                        | Stmt::Implicit(_)
                        | Stmt::SourceLocation(_)
                        | Stmt::Module { .. }
                        | Stmt::Function { .. }
                        | Stmt::Subroutine { .. }
                ) && !matches!(s,Stmt::CallSub{name,..} if name=="@extends")
            })
            .unwrap_or(cleaned.len());
        let mut entry = vec![];
        for object in runtime.values() {
            if object.member { continue; }
            if !object.dummy {
                entry.push(set(&object.tag, 0));
            } else if object.output {
                entry.push(cases(
                    object,
                    object
                        .variants
                        .iter()
                        .map(|(_, n)| {
                            vec![Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![id(n)],
                            }]
                        })
                        .collect(),
                    vec![],
                ));
                entry.push(set(&object.tag, 0));
            }
        }
        cleaned.splice(at..at, entry);
        *params = std::mem::take(params)
            .into_iter()
            .flat_map(|n| {
                if let Some(o) = runtime.get(&n) {
                    std::iter::once(o.tag.clone())
                        .chain(o.variants.iter().map(|(_, s)| s.clone()))
                        .collect::<Vec<_>>()
                } else {
                    vec![n]
                }
            })
            .collect();
        self.body(&mut cleaned, &runtime, &declared, pure)?;
        storage.extend(cleaned);
        // Record-array lowering releases automatic allocation descriptors on
        // procedure exit, just as it does for ordinary local allocatables.
        *body = storage;
        Ok(())
    }
    fn arguments(&self, name: &str, args: &mut Vec<Expr>, vars: &Runtime) -> Result<()> {
        let Some(procedure) = self.procedures.get(name) else {
            return Ok(());
        };
        // Calls have the source interface until this point. Expand one scalar
        // polymorphic descriptor into its tag and all concrete borrowed arrays.
        procedure.signature.reorder(name, args)?;
        let mut expanded = vec![];
        for (i, arg) in std::mem::take(args).into_iter().enumerate() {
            if let Some((kind, intent)) = procedure.dummies.get(&i) {
                let Expr::Ident(actual) = arg else {
                    bail!("ALLOCATABLE polymorphic dummy requires an allocatable scalar variable");
                };
                let object = vars.get(&actual).context(
                    "ALLOCATABLE polymorphic dummy requires a polymorphic ALLOCATABLE actual",
                )?;
                if !object.polymorphic {
                    bail!("ALLOCATABLE polymorphic dummy requires an ALLOCATABLE actual, not a SELECT TYPE associate");
                }
                if &object.kind != kind {
                    bail!(
                        "ALLOCATABLE polymorphic actual and dummy must have the same declared type"
                    );
                }
                let mut layout = self
                    .defs
                    .fields
                    .keys()
                    .filter(|k| {
                        !self.defs.abstracts.contains(*k) && self.defs.distance(k, kind).is_some()
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                layout.sort();
                if !layout.iter().eq(object.variants.iter().map(|(k, _)| k)) {
                    bail!("narrowed SELECT TYPE descriptors are not yet supported as polymorphic ALLOCATABLE actuals");
                }
                if object.readonly && intent != "intent(in)" {
                    bail!("cannot pass INTENT(IN) polymorphic object `{actual}` to a writable ALLOCATABLE dummy");
                }
                expanded.push(id(&object.tag));
                expanded.extend(object.variants.iter().map(|(_, s)| id(s)));
            } else {
                expanded.push(arg);
            }
        }
        *args = expanded;
        Ok(())
    }
    fn calls(&self, s: &mut Stmt, vars: &Runtime) -> Result<()> {
        if let Stmt::CallSub { name, args } = s {
            if self.procedures.contains_key(name) {
                self.arguments(name, args, vars)?;
                *name = format!("@polymorphic_descriptor_call:{name}");
            }
        }
        for e in expressions(s) {
            let mut error = None;
            visit_expr(e, &mut |e| {
                if error.is_none() {
                    if let Expr::Call(name, args) = e {
                        if self.procedures.contains_key(name) {
                            if let Err(e) = self.arguments(name, args, vars) {
                                error = Some(e);
                            } else {
                                *name = format!("@polymorphic_descriptor_call:{name}");
                            }
                        }
                    }
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
        }
        Ok(())
    }
    fn transfer(
        &self,
        to: &str,
        object: &Object,
        kind: &str,
        value: Option<Expr>,
        assignment: bool,
    ) -> Result<Vec<Stmt>> {
        if self.defs.abstracts.contains(kind) {
            bail!("cannot allocate ABSTRACT type `{kind}`");
        }
        let Some((index, (_, storage))) = object
            .variants
            .iter()
            .enumerate()
            .find(|(_, (k, _))| k == kind)
        else {
            bail!(
                "allocation or assignment type `{kind}` must extend declared type `{}`",
                object.kind
            );
        };
        let allocate = vec![
            Stmt::CallSub {
                name: "@allocate".into(),
                args: vec![Expr::Call(storage.clone(), vec![int(1)])],
            },
            set(&object.tag, index + 1),
        ];
        let mut out = vec![];
        if assignment {
            let release = cases(
                object,
                object
                    .variants
                    .iter()
                    .map(|(_, s)| {
                        vec![Stmt::CallSub {
                            name: "@deallocate".into(),
                            args: vec![id(s)],
                        }]
                    })
                    .collect(),
                vec![],
            );
            let mut replace = vec![release];
            replace.extend(allocate);
            out.push(Stmt::If {
                cond: Expr::Bin(
                    BinOp::Ne,
                    Box::new(id(&object.tag)),
                    Box::new(int(index + 1)),
                ),
                then_body: replace,
                else_body: None,
            });
        } else {
            out.push(Stmt::If {
                cond: Expr::Bin(BinOp::Ne, Box::new(id(&object.tag)), Box::new(int(0))),
                then_body: failure(format!(
                    "allocation of already allocated polymorphic object `{to}`"
                )),
                else_body: None,
            });
            out.extend(allocate);
        }
        if let Some(value) = value {
            out.push(Stmt::AssignIndex {
                name: storage.clone(),
                indices: vec![int(1)],
                value,
            });
        }
        Ok(out)
    }
    fn source_kind(&self, value: &Expr, decls: &Declarations) -> Result<String> {
        match value {
            Expr::Ident(n) => {
                if let Some(d) = decls.get(n) {
                    for s in d {
                        if let Stmt::DerivedDecl {
                            type_name,
                            polymorphic: false,
                            dims,
                            ..
                        } = s
                        {
                            if dims.is_empty() {
                                return Ok(type_name.clone());
                            }
                        }
                    }
                }
            }
            Expr::Call(n, _) if self.defs.fields.contains_key(n) => return Ok(n.clone()),
            _ => {}
        }
        bail!("polymorphic allocation and assignment currently require a scalar derived variable or constructor")
    }
    fn from_source(
        &self,
        to: &str,
        object: &Object,
        source: &Expr,
        copy: bool,
        assignment: bool,
        vars: &Runtime,
        decls: &Declarations,
    ) -> Result<Vec<Stmt>> {
        if let Expr::Ident(n) = source {
            if let Some(from) = vars.get(n) {
                if assignment && from.tag == object.tag {
                    return Ok(vec![Stmt::If {
                        cond: Expr::Bin(BinOp::Eq, Box::new(id(&from.tag)), Box::new(int(0))),
                        then_body: failure(format!(
                            "assignment from unallocated polymorphic object `{n}`"
                        )),
                        else_body: None,
                    }]);
                }
                let mut branches = vec![];
                for (kind, storage) in &from.variants {
                    let value = copy.then(|| Expr::Call(storage.clone(), vec![int(1)]));
                    branches.push(if self.defs.distance(kind,&object.kind).is_some() {
                        self.transfer(to,object,kind,value,assignment)?
                    } else {failure(format!("dynamic type `{kind}` is incompatible with polymorphic destination `{to}`"))});
                }
                let default = if !copy && !self.defs.abstracts.contains(&from.kind) {
                    self.transfer(to, object, &from.kind, None, assignment)?
                } else {
                    failure(format!("unallocated polymorphic SOURCE or MOLD `{n}` has no usable dynamic value or concrete type"))
                };
                return Ok(vec![cases(from, branches, default)]);
            }
        }
        // An ordinary CLASS dummy receives its concrete type during the
        // following call-site specialization pass. Preserve an exact TYPE IS
        // choice until then instead of treating its declared parent as dynamic.
        if let Expr::Ident(n) = source {
            if let Some(base)=decls.get(n).and_then(|ds|ds.iter().find_map(|s|match s {
                Stmt::DerivedDecl {type_name,polymorphic:true,dims,..} if dims.is_empty()=>Some(type_name),
                _=>None,
            })) {
                let alias="@polymorphic_source_view";
                let mut blocks=vec![];
                for (kind,_) in &object.variants {
                    if self.defs.distance(kind,base).is_some() {
                        blocks.push(CaseOrDefault::Case(CaseBlock {
                            items:vec![CaseItem::Single(Expr::Call("@type_is".into(),vec![id(kind)]))],
                            body:self.transfer(to,object,kind,copy.then(||id(alias)),assignment)?,
                        }));
                    }
                }
                blocks.push(CaseOrDefault::Default(failure(format!("unsupported or incompatible dynamic type in polymorphic SOURCE/MOLD `{n}`"))));
                return Ok(vec![super::construct((alias.into(),n.clone()),blocks).map_err(anyhow::Error::msg)?]);
            }
        }
        let kind = self.source_kind(source, decls)?;
        self.transfer(to, object, &kind, copy.then(|| source.clone()), assignment)
    }
    fn evaluate_source(
        &mut self,
        source: &Expr,
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<(Vec<Stmt>, Expr, Declarations)> {
        if matches!(source, Expr::Ident(_)) {
            return Ok((vec![], source.clone(), decls.clone()));
        }
        let kind = self.source_kind(source, decls)?;
        self.serial += 1;
        let temp = format!("@polymorphic_source{}", self.serial);
        let declaration = Stmt::DerivedDecl {
            type_name: kind,
            names: vec![temp.clone()],
            attributes: vec![],
            polymorphic: false,
            dims: vec![],
        };
        let mut local = decls.clone();
        local.insert(temp.clone(), vec![declaration.clone()]);
        let mut evaluation = vec![Stmt::Assign {
            name: temp.clone(),
            value: source.clone(),
        }];
        self.body(&mut evaluation, vars, &local, pure)?;
        let mut prefix = vec![declaration];
        prefix.extend(evaluation);
        Ok((prefix, id(&temp), local))
    }
    fn allocation(
        &mut self,
        name: &str,
        args: &[Expr],
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<Option<Vec<Stmt>>> {
        let (kind, args) = if name == "@allocate_derived_type" {
            let Some(Expr::Ident(kind)) = args.first() else {
                bail!("invalid derived ALLOCATE type specification");
            };
            (Some(kind.as_str()), &args[1..])
        } else {
            (None, args)
        };
        if !args
            .iter()
            .any(|a| matches!(a,Expr::Ident(n) if vars.contains_key(n)))
        {
            return Ok(None);
        }
        let mut objects = vec![];
        let mut source = None;
        let mut mold = None;
        for a in args {
            if let Expr::Call(k, v) = a {
                if let Some(k) = k.strip_prefix("@keyword:") {
                    if name == "@deallocate" {
                        bail!("polymorphic DEALLOCATE options are not yet supported");
                    }
                    let [value] = v.as_slice() else {
                        bail!("invalid ALLOCATE option");
                    };
                    match k {
                        "source" => {
                            if source.replace(value).is_some() {
                                bail!("duplicate SOURCE= option");
                            }
                        }
                        "mold" => {
                            if mold.replace(value).is_some() {
                                bail!("duplicate MOLD= option");
                            }
                        }
                        _ => bail!("polymorphic ALLOCATE option `{k}=` is not yet supported"),
                    }
                    continue;
                }
            }
            objects.push(a);
        }
        if source.is_some() && mold.is_some() {
            bail!("ALLOCATE cannot specify both SOURCE= and MOLD=");
        }
        if kind.is_some() && (source.is_some() || mold.is_some()) {
            bail!("ALLOCATE cannot combine a type specification with SOURCE= or MOLD=");
        }
        let (mut out, evaluated, local) = if let Some(value) = source {
            let (prefix, value, local) = self.evaluate_source(value, vars, decls, pure)?;
            (prefix, Some(value), local)
        } else {
            (vec![], None, decls.clone())
        };
        for a in objects {
            let Expr::Ident(n) = a else {
                bail!("polymorphic allocation currently requires scalar variables");
            };
            let Some(object) = vars.get(n) else {
                bail!(
                    "mixed polymorphic and nonpolymorphic allocation lists are not yet supported"
                );
            };
            if object.readonly {
                bail!("cannot change allocation status of INTENT(IN) polymorphic object `{n}`");
            }
            if name == "@deallocate" {
                out.push(cases(
                    object,
                    object
                        .variants
                        .iter()
                        .map(|(_, s)| {
                            vec![Stmt::CallSub {
                                name: "@deallocate".into(),
                                args: vec![id(s)],
                            }]
                        })
                        .collect(),
                    failure(format!(
                        "deallocation of unallocated polymorphic object `{n}`"
                    )),
                ));
                out.push(set(&object.tag, 0));
            } else if let Some(value) = evaluated.as_ref().or(mold) {
                out.extend(self.from_source(
                    n,
                    object,
                    value,
                    source.is_some(),
                    false,
                    vars,
                    &local,
                )?);
            } else {
                out.extend(self.transfer(n, object, kind.unwrap_or(&object.kind), None, false)?);
            }
        }
        Ok(Some(out))
    }
    fn body(
        &mut self,
        body: &mut Vec<Stmt>,
        vars: &Runtime,
        decls: &Declarations,
        pure: bool,
    ) -> Result<()> {
        let mut out = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = s {
                location = Some(p);
                out.push(s);
                continue;
            }
            if matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                out.push(s);
                continue;
            }
            let lowered=(||->Result<Vec<Stmt>> {
                if let Some(lowered)=self.pointer_statement(&s,vars,decls)? { return Ok(lowered); }
                self.pointer_inquiries(&mut s,vars,decls)?;
                self.calls(&mut s,vars)?;
                if let Stmt::CallSub{name,args}=&s {
                    if matches!(name.as_str(),"@allocate"|"@allocate_derived_type"|"@deallocate") {
                        if let Some(out)=self.allocation(name,args,vars,decls,pure)? {return Ok(out);}
                    }
                }
                if let Stmt::Assign{name,value}=&s {
                    if let Some(object)=vars.get(name) {
                        if object.pointer {bail!("whole-object assignment through a polymorphic POINTER is not yet supported");}
                        if object.readonly {bail!("cannot define INTENT(IN) polymorphic object `{name}`");}
                        if !object.polymorphic {bail!("whole-object assignment of a SELECT TYPE associate requires ordinary derived assignment");}
                        let (mut prefix,value,local)=self.evaluate_source(value,vars,decls,pure)?;
                        prefix.extend(self.from_source(name,object,&value,true,true,vars,&local)?);
                        return Ok(prefix);
                    }
                }
                // Inquiries do not evaluate the stored value.
                for e in expressions(&mut s) {visit_expr(e,&mut |e| {
                    if let Expr::Call(n,a)=e {
                        if n=="allocated"&&a.len()==1 {
                            let actual=match &a[0] {Expr::Call(k,v) if k=="@keyword:array"&&v.len()==1=>&v[0],e=>e};
                            if let Expr::Ident(v)=actual {if let Some(o)=vars.get(v).filter(|o|!o.pointer) {*e=Expr::Bin(BinOp::Ne,Box::new(id(&o.tag)),Box::new(int(0)));}}
                        }
                    }
                });}
                let selected=matches!(&s,Stmt::SelectCase{expr,..} if selection(expr).is_some_and(|(n,_)|vars.contains_key(n)));
                if selected {
                    let Stmt::SelectCase{expr,cases:guards,default}=&s else {unreachable!()};
                    let (selector,alias)=selection(expr).unwrap();
                    let object=&vars[selector];
                    // An associate name retains the selected target even if
                    // the original pointer is reassociated inside the guard.
                    let mut prefix=vec![];
                    let mut local_decls=decls.clone();
                    let snapshot=if object.pointer {
                        self.serial+=1;
                        let tag=format!("@polymorphic_selection{}",self.serial);
                        let declaration=Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![tag.clone()]};
                        local_decls.insert(tag.clone(),vec![declaration.clone()]);
                        prefix.push(declaration);
                        prefix.push(Stmt::Assign{name:tag.clone(),value:id(&object.tag)});
                        Some(tag)
                    } else {None};
                    let mut branches=vec![];
                    for (kind,_) in &object.variants {
                        let mut best=None;
                        let mut distance=usize::MAX;
                        for c in guards {
                            let (class,guard_kind)=guard(&c.items[0])?;
                            let d=if class {self.defs.distance(kind,&guard_kind).map(|d|d+1)} else {(kind==&guard_kind).then_some(0)};
                            if let Some(d)=d {if d<distance {distance=d;best=Some((class,guard_kind,c.body.clone()));}}
                        }
                        let mut local=vars.clone();
                        let mut view=object.clone();
                        if let Some(tag)=&snapshot {view.tag=tag.clone();}
                        let mut branch=if let Some((class,guard_kind,body))=best {
                            view.kind=guard_kind.into(); view.polymorphic=class; body
                        } else {default.clone().unwrap_or_default()};
                        local.insert(alias.into(),view);
                        self.body(&mut branch,&local,&local_decls,pure)?;
                        branches.push(branch);
                    }
                    prefix.push(cases(object,branches,failure(if object.pointer {format!("SELECT TYPE on unassociated polymorphic pointer `{selector}`")} else {format!("SELECT TYPE on unallocated polymorphic object `{selector}`")})));
                    return Ok(prefix);
                }
                // Evaluate runtime-dependent conditions at their original
                // control point, never move EXIT/CYCLE into a helper procedure.
                if let Stmt::If{cond,then_body,else_body}=&mut s {
                    let probe=Stmt::Assign{name:String::new(),value:cond.clone()};
                    if vars.keys().any(|n|uses(&probe,n)) {
                        self.serial+=1;
                        let temp=format!("@polymorphic_condition{}",self.serial);
                        let declaration=Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![temp.clone()]};
                        let mut local=decls.clone(); local.insert(temp.clone(),vec![declaration.clone()]);
                        let mut evaluation=vec![Stmt::Assign{name:temp.clone(),value:cond.clone()}];
                        self.body(&mut evaluation,vars,&local,pure)?;
                        *cond=id(&temp);
                        self.body(then_body,vars,decls,pure)?;
                        if let Some(b)=else_body {self.body(b,vars,decls,pure)?;}
                        let mut lowered=vec![declaration]; lowered.extend(evaluation); lowered.push(s);
                        return Ok(lowered);
                    }
                }
                if let Stmt::DoWhile{cond,body}=&mut s {
                    let probe=Stmt::Assign{name:String::new(),value:cond.clone()};
                    if vars.keys().any(|n|uses(&probe,n)) {
                        self.serial+=1;
                        let temp=format!("@polymorphic_condition{}",self.serial);
                        let declaration=Stmt::VarDecl{kind:TypeSpec::Logical,names:vec![temp.clone()]};
                        let mut local=decls.clone(); local.insert(temp.clone(),vec![declaration.clone()]);
                        let mut loop_body=vec![Stmt::Assign{name:temp.clone(),value:cond.clone()},
                            Stmt::If{cond:Expr::Un(crate::ast::UnOp::Not,Box::new(id(&temp))),then_body:vec![Stmt::Exit],else_body:None}];
                        loop_body.append(body);
                        self.body(&mut loop_body,vars,&local,pure)?;
                        return Ok(vec![declaration,Stmt::DoWhile{cond:Expr::Logical(true),body:loop_body}]);
                    }
                }
                for b in children(&mut s) {
                    let local=declarations(b,decls);
                    self.body(b,vars,&local,pure)?;
                }
                let used:Vec<_>=vars.iter().filter(|(n,_)|uses(&s,n)).collect();
                if used.is_empty() {return Ok(vec![s]);}
                if matches!(s,Stmt::Do{..}) {bail!("polymorphic values in counted DO bounds are not yet supported");}
                self.dispatch(s,&used,0,&mut vec![],decls,pure,location)
            })().map_err(|e|crate::source_diagnostics::at(e,location))?;
            out.extend(lowered);
        }
        *body = out;
        Ok(())
    }
    fn dispatch(
        &mut self,
        mut s: Stmt,
        used: &[(&String, &Object)],
        index: usize,
        chosen: &mut Vec<(String, String, String)>,
        decls: &Declarations,
        pure: bool,
        location: Option<usize>,
    ) -> Result<Vec<Stmt>> {
        if index < used.len() {
            let (n, o) = used[index];
            let mut bodies = vec![];
            for (kind, storage) in &o.variants {
                if self.defs.distance(kind, &o.kind).is_none() {
                    bodies.push(failure("invalid polymorphic associate type".into()));
                    continue;
                }
                chosen.push((n.clone(), kind.clone(), storage.clone()));
                bodies.push(self.dispatch(
                    s.clone(),
                    used,
                    index + 1,
                    chosen,
                    decls,
                    pure,
                    location,
                )?);
                chosen.pop();
            }
            return Ok(vec![cases(
                o,
                bodies,
                failure(if o.pointer {format!("use of unassociated polymorphic pointer `{}`",o.display)} else {format!("use of unallocated polymorphic object `{n}`")}),
            )]);
        }
        self.serial += 1;
        let name = format!("@runtime_polymorphic_call{}", self.serial);
        let mut params = vec![];
        let mut args = vec![];
        let mut body = vec![];
        for (n, _, storage) in chosen.iter() {
            let object = used.iter().find(|(v, _)| *v == n).unwrap().1;
            let parameter=if n.contains('%') {
                let parameter=format!("@polymorphic_argument{}_{}",self.serial,params.len());
                super::rename(std::slice::from_mut(&mut s),n,&parameter);
                parameter
            } else {n.clone()};
            params.push(parameter.clone());
            args.push(if object.pointer {id(storage)} else {Expr::Call(storage.clone(), vec![int(1)])});
            body.push(Stmt::DerivedDecl {
                type_name: object.kind.clone(),
                names: vec![parameter],
                attributes: vec![if object.readonly {
                    "intent(in)"
                } else {
                    "intent(inout)"
                }
                .into(), "@referenced_type".into()],
                polymorphic: object.polymorphic,
                dims: vec![],
            });
        }
        let mut captures: HashSet<String> = decls.keys().filter(|n| uses(&s, n)).cloned().collect();
        loop {
            let mut added = false;
            for n in decls.keys() {
                if captures.contains(n) {
                    continue;
                }
                if captures.iter().any(|c| decls[c].iter().any(|s| uses(s, n))) {
                    captures.insert(n.clone());
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        for (n, d) in decls {
            let parameter=d.iter().any(|s|matches!(s,Stmt::InitializedDecl{attributes,..} if attributes.iter().any(|a|a=="parameter")));
            if used.iter().any(|(v, _)| *v == n) || (!parameter && !captures.contains(n)) {
                continue;
            }
            if !parameter {
                params.push(n.clone());
                args.push(id(n));
            }
            for mut declaration in d.clone() {
                // Captures refer to the enclosing activation's current state.
                // INTENT(OUT) already took effect on entry to that activation;
                // repeating it here would undefine an assigned array (or
                // deallocate an owned component) before evaluating this statement.
                match &mut declaration {
                    Stmt::DummyIntent {intent,..} if intent=="intent(out)" => *intent="intent(inout)".into(),
                    Stmt::DerivedDecl {attributes,..} => {
                        for attribute in attributes.iter_mut() {
                            if attribute=="intent(out)" {*attribute="intent(inout)".into();}
                        }
                        attributes.push("@referenced_type".into());
                    }
                    _ => {}
                }
                body.push(declaration);
            }
            if pure
                && !parameter
                && !d.iter().any(|s| match s {
                    Stmt::DummyIntent { intent, .. } => intent.starts_with("intent("),
                    Stmt::DerivedDecl { attributes, .. } => {
                        attributes.iter().any(|a| a.starts_with("intent("))
                    }
                    _ => false,
                })
            {
                body.push(Stmt::DummyIntent {
                    name: n.clone(),
                    intent: "intent(inout)".into(),
                });
            }
        }
        if let Some(p) = location {
            body.push(Stmt::SourceLocation(p));
        }
        body.push(s);
        self.generated.push(Stmt::Subroutine {
            elemental: false,
            pure,
            name: name.clone(),
            params,
            body,
        });
        Ok(vec![Stmt::CallSub { name, args }])
    }
}

pub(super) fn resolve(mut program: Program, defs: &mut Definitions) -> Result<Program> {
    pointer_components::prepare(&mut program, defs)?;
    fn needed(body: &[Stmt]) -> bool {
        for s in body {
            if let Stmt::DummyIntent {name,intent} = s {
                if matches!(intent.as_str(),"pointer"|"allocatable") && body.iter().any(|s|matches!(s,Stmt::DerivedDecl {polymorphic:true,names,..} if names.contains(name))) {
                    return true;
                }
            }
            if matches!(s,Stmt::DerivedDecl{polymorphic:true,attributes,..} if attributes.iter().any(|a|a=="allocatable" || a=="pointer"))
            {
                return true;
            }
            let nested = match s {
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => needed(body),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => needed(then_body) || else_body.as_ref().is_some_and(|b| needed(b)),
                Stmt::SelectCase { cases, default, .. } => {
                    cases.iter().any(|c| needed(&c.body))
                        || default.as_ref().is_some_and(|b| needed(b))
                }
                _ => false,
            };
            if nested {
                return true;
            }
        }
        false
    }
    let has_components=defs.has_polymorphic_components;
    if !needed(&program.body) && !has_components {
        return Ok(program);
    }
    defs.validate(&program.body, &Objects::new())?;
    if has_components {components::prepare(&mut program.body,defs)?;}
    let mut signatures = HashMap::new();
    procedures(&program.body, &mut signatures);
    let mut lower = Lower {
        defs,
        procedures: signatures,
        generated: vec![],
        serial: 0,
    };
    lower.scope(&mut program.body, &Declarations::new(), &mut vec![], false)?;
    program.body.extend(lower.generated);
    fn restore(body: &mut [Stmt]) {
        for s in body {
            if let Stmt::CallSub { name, .. } = s {
                if let Some(n) = name.strip_prefix("@polymorphic_descriptor_call:") {
                    *name = n.into();
                }
            }
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if let Expr::Call(name, _) = e {
                        if let Some(n) = name.strip_prefix("@polymorphic_descriptor_call:") {
                            *name = n.into();
                        }
                    }
                });
            }
            for b in children(s) {
                restore(b);
            }
        }
    }
    restore(&mut program.body);
    Ok(program)
}

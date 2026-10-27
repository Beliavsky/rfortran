// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Local record components store pointer identities in the record's own tag
//! field. Short-lived scalar pointer views reuse ordinary runtime dispatch.
use super::*;

#[derive(Clone)]
struct Member {
    kind: String,
    tag: String,
    rank: usize,
    readonly: bool,
}
type Members = BTreeMap<String, Member>;

fn field(name: &str) -> String {
    format!("@pointer_component_{name}")
}

pub(super) fn assign(target: &Expr, value: Expr) -> Result<Stmt> {
    match target {
        Expr::Ident(name) => Ok(Stmt::Assign {
            name: name.clone(),
            value,
        }),
        Expr::Call(marker, args) if marker.starts_with("@record_component:") && args.len() == 1 => {
            let Expr::Call(owner, indices) = &args[0] else {
                bail!("pointer component owner must be a scalar array element");
            };
            Ok(Stmt::AssignIndex {
                name: format!(
                    "{owner}%{}",
                    marker.trim_start_matches("@record_component:")
                ),
                indices: indices.clone(),
                value,
            })
        }
        _ => bail!("unsupported pointer component storage"),
    }
}

fn members(
    defs: &Definitions,
    kind: &str,
    path: &str,
    rank: usize,
    readonly: bool,
    out: &mut Members,
    seen: &mut HashSet<String>,
) -> Result<()> {
    if !seen.insert(kind.into()) {
        return Ok(());
    }
    if let Some(parent) = defs.parents.get(kind) {
        members(defs, parent, path, rank, readonly, out, seen)?;
    }
    for (name, component) in defs.members.get(kind).into_iter().flatten() {
        let key = format!("{path}%{name}");
        if component.polymorphic && component.attrs.iter().any(|a| a == "pointer") {
            if component.rank != 0 {
                bail!("polymorphic POINTER component arrays are not yet supported");
            }
            if component.kind == "*" {
                bail!("unlimited polymorphic POINTER components are not yet supported");
            }
            out.insert(
                key,
                Member {
                    kind: component.kind.clone(),
                    tag: format!("{path}%{}", field(name)),
                    rank,
                    readonly,
                },
            );
        } else if component.rank == 0
            && !component
                .attrs
                .iter()
                .any(|a| a == "allocatable" || a == "pointer")
        {
            members(defs, &component.kind, &key, rank, readonly, out, seen)?;
        }
    }
    seen.remove(kind);
    Ok(())
}

struct Scope<'a> {
    members: Members,
    constructors: HashSet<String>,
    targets: Vec<String>,
    serial: &'a mut usize,
    declarations: Vec<Stmt>,
}
impl Scope<'_> {
    fn owner(&self, e: &Expr) -> bool {
        let name = match e {
            Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) => n,
            _ => return false,
        };
        self.members.keys().any(|key| {
            key.strip_prefix(name)
                .is_some_and(|tail| tail.starts_with('%'))
        })
    }
    /// Return a pointer view and the component suffix beyond it. Import the
    /// identity once for this reference, capturing owner subscripts once too.
    fn reference(
        &mut self,
        e: &Expr,
        prefix: &mut Vec<Stmt>,
        writes: &mut Vec<Stmt>,
        write: bool,
    ) -> Result<Option<(String, String, Option<Vec<Expr>>)>> {
        let (path, base, arguments) = match e {
            Expr::Ident(n) => (n.clone(), None, None),
            Expr::Call(n, args) | Expr::Index(n, args) if n.starts_with("@record_component:") => {
                let Some(Expr::Call(owner, indices)) = args.first() else {
                    return Ok(None);
                };
                let arguments = match args.get(1) {
                    Some(Expr::Call(n, args)) if n == "@component_subscripts" => Some(args.clone()),
                    Some(_) => bail!("unsupported pointer component subscripts"),
                    None => None,
                };
                (
                    format!("{owner}%{}", n.trim_start_matches("@record_component:")),
                    Some((owner.clone(), indices.clone())),
                    arguments,
                )
            }
            Expr::Call(n, args) | Expr::Index(n, args) => (n.clone(), None, Some(args.clone())),
            _ => return Ok(None),
        };
        let Some((key, member)) = self
            .members
            .iter()
            .filter(|(key, _)| {
                path == **key
                    || path
                        .strip_prefix(key.as_str())
                        .is_some_and(|s| s.starts_with('%'))
            })
            .max_by_key(|(key, _)| key.len())
            .map(|(k, v)| (k.clone(), v.clone()))
        else {
            return Ok(None);
        };
        if write && member.readonly {
            bail!("cannot change association of an INTENT(IN) pointer component `{key}`");
        }
        let suffix = path[key.len()..].to_owned();
        if suffix.is_empty() && arguments.is_some() {
            bail!("scalar polymorphic pointer component cannot have subscripts");
        }
        let tag = if let Some((owner, indices)) = base {
            if indices.len() != member.rank {
                bail!(
                    "pointer component owner requires {} scalar subscripts",
                    member.rank
                );
            }
            let mut captured = vec![];
            for mut index in indices {
                if matches!(index, Expr::Section { .. })
                    || matches!(&index,Expr::Call(n,_) if n=="@section_subscript")
                {
                    bail!("pointer components currently require a scalar owner element");
                }
                self.expression(&mut index, prefix, writes, false)?;
                *self.serial += 1;
                let temp = format!("@pointer_component_index{}", self.serial);
                self.declarations.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(None),
                    names: vec![temp.clone()],
                });
                prefix.push(Stmt::Assign {
                    name: temp.clone(),
                    value: index,
                });
                captured.push(id(&temp));
            }
            Expr::Call(
                format!(
                    "@record_component:{}",
                    member
                        .tag
                        .strip_prefix(&format!("{owner}%"))
                        .context("invalid pointer component owner")?
                ),
                vec![Expr::Call(owner, captured)],
            )
        } else {
            if member.rank != 0 {
                bail!("pointer components of record arrays require a scalar owner element");
            }
            id(&member.tag)
        };
        *self.serial += 1;
        let proxy = format!("@pointer_component_view{}", self.serial);
        self.declarations.push(Stmt::DerivedDecl {
            type_name: member.kind,
            names: vec![proxy.clone()],
            attributes: vec!["pointer".into(), "@referenced_type".into()],
            polymorphic: true,
            dims: vec![],
        });
        let mut candidates = vec![id(&proxy)];
        candidates.extend(self.targets.iter().map(|n| id(n)));
        candidates.push(Expr::Str(key));
        self.declarations.push(Stmt::CallSub {
            name: "@pointer_component_targets".into(),
            args: candidates,
        });
        prefix.push(Stmt::CallSub {
            name: "@pointer_component_load".into(),
            args: vec![id(&proxy), tag.clone()],
        });
        if write {
            writes.push(Stmt::CallSub {
                name: "@pointer_component_store".into(),
                args: vec![id(&proxy), tag],
            });
        }
        Ok(Some((proxy, suffix, arguments)))
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        prefix: &mut Vec<Stmt>,
        writes: &mut Vec<Stmt>,
        write: bool,
    ) -> Result<()> {
        if let Expr::Call(name, args) = e {
            if self.constructors.contains(name) && !args.is_empty() {
                bail!("explicit constructor values for polymorphic POINTER components are not yet supported");
            }
            if !name.starts_with('@')
                && crate::intrinsics::purity(name).is_none()
                && args.iter().any(|arg| self.owner(arg))
            {
                bail!("passing owners with polymorphic POINTER components requires a component descriptor interface, which is not yet supported");
            }
        }
        // Handle the outer component before descending into its owner, so
        // indexed designators retain their owner/component distinction.
        if let Some((proxy, suffix, args)) = self.reference(e, prefix, writes, write)? {
            *e = match args {
                Some(mut args) => {
                    for argument in &mut args {
                        self.expression(argument, prefix, writes, false)?;
                    }
                    Expr::Call(format!("{proxy}{suffix}"), args)
                }
                None => id(&format!("{proxy}{suffix}")),
            };
            return Ok(());
        }
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for e in args {
                    self.expression(e, prefix, writes, false)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, prefix, writes, false)?;
                self.expression(b, prefix, writes, false)?;
            }
            Expr::Un(_, e) => self.expression(e, prefix, writes, false)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, prefix, writes, false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>) -> Result<()> {
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
                Stmt::DerivedType { .. }
                    | Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
            ) {
                out.push(s);
                continue;
            }
            let mut prefix = vec![];
            let mut writes = vec![];
            (||->Result<()> {
                if let Stmt::Block{body}=&s {
                    if body.iter().any(|s|matches!(s,Stmt::DerivedDecl{type_name,attributes,..}
                        if self.constructors.contains(type_name)||attributes.iter().any(|a|a=="target"||a=="pointer"))) {
                        bail!("BLOCK-local pointer component owners and targets are not yet supported");
                    }
                }
                for child in children(&mut s) {self.body(child)?;}
                if let Stmt::CallSub{name,args}=&s {
                    if !name.starts_with('@') && args.iter().any(|a|self.owner(a)) {
                        bail!("passing owners with polymorphic POINTER components requires a component descriptor interface, which is not yet supported");
                    }
                }
                if let Stmt::Return(Some(value))=&s {if self.owner(value) {
                    bail!("function results with polymorphic POINTER components are not yet supported");
                }}
                match &mut s {
                    Stmt::CallSub{name,args} if name=="@pointer_associate" => {
                        if let Some(pointer)=args.first_mut() {self.expression(pointer,&mut prefix,&mut writes,true)?;}
                        for e in args.iter_mut().skip(1) {self.expression(e,&mut prefix,&mut writes,false)?;}
                    }
                    Stmt::CallSub{name,args} if name=="@nullify" => for e in args {self.expression(e,&mut prefix,&mut writes,true)?;},
                    Stmt::CallSub{name,args} if name.starts_with("@element_call:") && !args.is_empty() => {
                        let field=name.trim_start_matches("@element_call:");
                        let mut designator=Expr::Call(format!("@record_component:{field}"),vec![args[0].clone(),Expr::Call("@component_subscripts".into(),args[1..].to_vec())]);
                        self.expression(&mut designator,&mut prefix,&mut writes,false)?;
                        if let Expr::Call(target,actuals)=designator {if !target.starts_with("@record_component:") {*name=target;*args=actuals;}}
                    }
                    Stmt::Assign{name,..} => {
                        if let Some((proxy,suffix,_))=self.reference(&id(name),&mut prefix,&mut writes,false)? {
                            if suffix.is_empty() {bail!("whole-object assignment through polymorphic POINTER components is not yet supported");}
                            *name=format!("{proxy}{suffix}");
                        }
                        for e in expressions(&mut s) {self.expression(e,&mut prefix,&mut writes,false)?;}
                    }
                    Stmt::AssignIndex{name,indices,value} => {
                        let root=name.split('%').next().unwrap();
                        let path=name.strip_prefix(&format!("{root}%")).map(str::to_owned);
                        if let Some(path)=path {
                            let subs=indices.last().and_then(|e|if let Expr::Call(n,a)=e {if n=="@component_subscripts" {Some(a.clone())}else{None}}else{None});
                            let count=indices.len()-usize::from(subs.is_some());
                            let mut args=vec![Expr::Call(root.into(),indices[..count].to_vec())];
                            if let Some(subs)=subs {args.push(Expr::Call("@component_subscripts".into(),subs));}
                            let designator=Expr::Call(format!("@record_component:{path}"),args);
                            if let Some((proxy,suffix,sub))=self.reference(&designator,&mut prefix,&mut writes,false)? {
                                if suffix.is_empty() {bail!("whole-object assignment through polymorphic POINTER components is not yet supported");}
                                let mut rhs=value.clone();self.expression(&mut rhs,&mut prefix,&mut writes,false)?;
                                s=if let Some(indices)=sub {Stmt::AssignIndex{name:format!("{proxy}{suffix}"),indices,value:rhs}}
                                    else {Stmt::Assign{name:format!("{proxy}{suffix}"),value:rhs}};
                            } else {for e in expressions(&mut s) {self.expression(e,&mut prefix,&mut writes,false)?;}}
                        } else {for e in expressions(&mut s) {self.expression(e,&mut prefix,&mut writes,false)?;}}
                    }
                    _=>for e in expressions(&mut s) {self.expression(e,&mut prefix,&mut writes,false)?;},
                }
                if let Stmt::CallSub{name,args}=&s {
                    if name.starts_with("@element_call:") && args.iter().any(|arg|self.owner(arg)) {
                        bail!("type-bound calls on pointer component owners require a component descriptor interface, which is not yet supported");
                    }
                }
                Ok(())
            })().map_err(|e|crate::source_diagnostics::at(e,location))?;
            if let Stmt::DoWhile { cond, body } = &mut s {
                if !prefix.is_empty() {
                    let test = std::mem::replace(cond, Expr::Logical(true));
                    prefix.push(Stmt::If {
                        cond: Expr::not(test),
                        then_body: vec![Stmt::Exit],
                        else_body: None,
                    });
                    prefix.append(body);
                    *body = std::mem::take(&mut prefix);
                }
            }
            out.extend(prefix);
            out.push(s);
            out.extend(writes);
        }
        *body = out;
        Ok(())
    }
}

fn scope(
    body: &mut Vec<Stmt>,
    defs: &Definitions,
    parent: &Declarations,
    params: &[String],
    module: bool,
    serial: &mut usize,
) -> Result<()> {
    let declared = declarations(body, parent);
    let mut fields = Members::new();
    for (name, object) in super::super::objects(body, &Objects::new()) {
        let mut found = Members::new();
        members(
            defs,
            &object.kind,
            &name,
            object.rank,
            object.attrs.iter().any(|a| a == "intent(in)"),
            &mut found,
            &mut HashSet::new(),
        )?;
        if !found.is_empty() && (module || params.contains(&name)) {
            bail!("module objects and dummy arguments with polymorphic POINTER components are not yet supported");
        }
        fields.extend(found);
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function {
                name, params, body, ..
            } => {
                if body.iter().any(|s|matches!(s,Stmt::DerivedDecl{names,type_name,..} if names.contains(name) && defs.members.get(type_name).is_some_and(|m|m.values().any(|v|v.polymorphic&&v.attrs.iter().any(|a|a=="pointer"))))) {
                bail!("function results with polymorphic POINTER components are not yet supported");
            }
                scope(body, defs, &declared, params, false, serial)?;
            }
            Stmt::Subroutine { params, body, .. } => {
                scope(body, defs, &declared, params, false, serial)?
            }
            Stmt::Module { body, .. } => scope(body, defs, &declared, &[], true, serial)?,
            _ => {}
        }
    }
    if fields.is_empty() {
        return Ok(());
    }
    fn targets(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            if let Stmt::CallSub { name, args } = s {
                if name == "@pointer_associate" {
                    if let Some(Expr::Ident(n)) = args.get(1) {
                        out.insert(n.clone());
                    }
                }
            }
            if !matches!(
                s,
                Stmt::Function { .. }
                    | Stmt::Subroutine { .. }
                    | Stmt::Module { .. }
                    | Stmt::DerivedType { .. }
            ) {
                let mut s = s.clone();
                for child in children(&mut s) {
                    targets(child, out);
                }
            }
        }
    }
    let mut candidates = HashSet::new();
    targets(body, &mut candidates);
    let mut candidates: Vec<_> = candidates.into_iter().collect();
    candidates.sort();
    let constructors = defs
        .fields
        .keys()
        .filter(|kind| {
            let mut found = Members::new();
            members(defs, kind, "", 0, false, &mut found, &mut HashSet::new()).is_ok()
                && !found.is_empty()
        })
        .cloned()
        .collect();
    let mut lower = Scope {
        members: fields,
        constructors,
        targets: candidates,
        serial,
        declarations: vec![],
    };
    lower.body(body)?;
    lower.declarations.append(body);
    *body = lower.declarations;
    Ok(())
}

fn schemas(body: &mut [Stmt], defs: &mut Definitions) -> Result<()> {
    for s in body {
        match s {
            Stmt::DerivedType {
                name, components, ..
            } => {
                let replaced: HashSet<_> = defs
                    .members
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|(_, m)| m.polymorphic && m.attrs.iter().any(|a| a == "pointer"))
                    .map(|(n, _)| n.clone())
                    .collect();
                if replaced.is_empty() {
                    continue;
                }
                let mut out = vec![];
                for s in std::mem::take(components) {
                    match s {
                        Stmt::DerivedDecl { names, type_name, dims, .. }
                            if names.iter().any(|n| replaced.contains(n)) =>
                        {
                            if !dims.is_empty() {bail!("polymorphic POINTER component arrays are not yet supported");}
                            if type_name=="*" {bail!("unlimited polymorphic POINTER components are not yet supported");}
                            for n in names {
                                let tag = field(&n);
                                out.push(Stmt::InitializedDecl {
                                    kind: TypeSpec::Integer(None),
                                    attributes: vec![],
                                    entities: vec![(tag.clone(), Some(int(0)))],
                                });
                                let fields = defs.fields.get_mut(name).unwrap();
                                fields.remove(&n);
                                fields.insert(tag, None);
                            }
                        }
                        Stmt::CallSub { name, args }
                            if name == "@pointer_associate"
                                && matches!(args.first(),Some(Expr::Ident(n)) if replaced.contains(n)) =>
                        {
                            if !matches!(args.get(1),Some(Expr::Call(n,a)) if n=="null"&&a.is_empty())
                            {
                                bail!("polymorphic POINTER component default initialization requires NULL()");
                            }
                        }
                        s => out.push(s),
                    }
                }
                *components = out;
            }
            s => {
                for child in children(s) {
                    schemas(child, defs)?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn prepare(program: &mut Program, defs: &mut Definitions) -> Result<()> {
    if !defs.members.values().any(|m| {
        m.values()
            .any(|v| v.polymorphic && v.attrs.iter().any(|a| a == "pointer"))
    }) {
        return Ok(());
    }
    scope(
        &mut program.body,
        defs,
        &Declarations::new(),
        &[],
        false,
        &mut 0,
    )?;
    schemas(&mut program.body, defs)
}

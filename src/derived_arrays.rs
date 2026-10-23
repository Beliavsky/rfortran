// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Arrays of records share their shape across separate primitive component arrays.
//! Array results use a single hidden subroutine call, preserving side effects.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[path = "elemental_records.rs"]
mod elemental_records;
#[path = "elemental_record_results.rs"]
mod elemental_record_results;
#[path = "record_components.rs"]
mod record_components;
#[path = "record_pack.rs"]
mod record_pack;
#[path = "record_constructors.rs"]
mod record_constructors;
#[path = "derived_pointers.rs"]
mod derived_pointers;

#[derive(Clone)]
struct Field {
    name: String,
    kind: TypeSpec,
    default: Option<Expr>,
    /// Constant extents of an array component; empty for a scalar. Its
    /// storage puts these axes ahead of the axes of the array of records.
    dims: Vec<Expr>,
    /// A per-element owned numeric allocation, stored as a descriptor handle.
    heap: Option<(TypeSpec, Vec<Expr>)>,
}
fn whole_axes(f: &Field) -> Vec<Expr> {
    f.dims.iter().map(|_| Expr::Section { name: String::new(), lower: None, upper: None, stride: None }).collect()
}
/// Rename the owner of section subscripts already bound to `name`.
fn retarget(e: &mut Expr, name: &str, target: &str) {
    match e {
        Expr::Section { name: n, .. } if n == name => *n = target.to_owned(),
        Expr::Call(_, args) => {
            for e in args {
                retarget(e, name, target);
            }
        }
        _ => {}
    }
}
/// Split trailing component subscripts, as in `a(i)%x(j)`, from the
/// subscripts of the array of records.
fn component_subscripts(indices: &mut Vec<Expr>) -> Vec<Expr> {
    match indices.last() {
        Some(Expr::Call(n, _)) if n == "@component_subscripts" => {
            let Some(Expr::Call(_, args)) = indices.pop() else { unreachable!() };
            args
        }
        _ => vec![],
    }
}
/// Element or section `object(indices)%field(component)` of record storage.
fn select(object: &str, f: &Field, component: Vec<Expr>, indices: &[Expr]) -> Result<Expr> {
    let target = storage(object, &f.name);
    if f.dims.is_empty() && !component.is_empty() {
        // A substring of a CHARACTER component.
        let [range @ Expr::Section { .. }] = component.as_slice() else {
            bail!("component `{}` is not an array", f.name);
        };
        let mut indices = indices.to_vec();
        for e in &mut indices {
            retarget(e, object, &target);
        }
        return Ok(Expr::Call("@array_substring".into(), vec![crate::ast::section_or_call(target, indices), range.clone()]));
    }
    if !component.is_empty() && component.len() != f.dims.len() {
        bail!("component `{}` requires {} subscripts", f.name, f.dims.len());
    }
    let mut args = if component.is_empty() { whole_axes(f) } else { component };
    for e in indices {
        let mut e = e.clone();
        retarget(&mut e, object, &target);
        // An already bound triplet keeps its owner; unbind it so the combined
        // subscript list binds every section to the storage array.
        if let Expr::Call(n, a) = &e {
            if n == "@section_subscript" && a.len() == 1 {
                if let Expr::Section { lower, upper, stride, .. } = &a[0] {
                    e = Expr::Section { name: String::new(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() };
                }
            }
        }
        if let Expr::Section { name, .. } = &mut e {
            if name == object || name == &target {
                name.clear();
            }
        }
        args.push(e);
    }
    Ok(crate::ast::section_or_call(target, args))
}
/// Assign to an element or section that `select` gives.
fn assignment(target: Expr, value: Expr) -> Stmt {
    match target {
        Expr::Call(name, mut args) if name == "@array_substring" => {
            args.push(value);
            Stmt::CallSub { name: "@array_substring_assign".into(), args }
        }
        Expr::Call(name, indices) => Stmt::AssignIndex { name, indices, value },
        Expr::Section { ref name, .. } => Stmt::AssignIndex { name: name.clone(), indices: vec![target], value },
        _ => unreachable!("record storage selections are calls or sections"),
    }
}
/// Storage shape of a field of an array of records with shape `dims`.
fn storage_dims(f: &Field, dims: &[Expr]) -> Vec<Expr> {
    if !dims.is_empty() && dims.iter().all(|d| matches!(d, Expr::Ident(n) if n == "@deferred_extent")) {
        return deferred(f.dims.len() + dims.len());
    }
    f.dims.iter().chain(dims).cloned().collect()
}
#[derive(Clone)]
struct Object {
    ty: String,
    dims: Vec<Expr>,
}
#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    arrays: HashMap<String, Object>,
    result: Option<(String, Object)>,
}
type Objects = HashMap<String, Object>;
// Generated ELEMENTAL calls have evaluated their actuals to determine shape.
// The marker is consumed by `body` before the call leaves this lowering pass.
const PREPARED_CALL: &str = "@record_prepared:";
struct Lower {
    types: HashMap<String, Vec<Field>>,
    procedures: HashMap<String, Procedure>,
    serial: usize,
    array_shapes: HashMap<String, Vec<Expr>>,
    /// Components of each type that are arrays of a derived type, and scalar
    /// derived components, which may contain such arrays in turn.
    array_components: HashMap<String, Vec<(String, Object)>>,
    scalar_components: HashMap<String, Vec<(String, String)>>,
    /// Dummy arguments of the procedure being lowered.
    dummies: Vec<String>,
    /// The components of each type in order, with the type of each derived
    /// component, whose fields appear in `types` as `component%field`.
    layouts: HashMap<String, Vec<(String, Option<String>)>>,
    /// Procedures with a scalar derived dummy, which an array element is
    /// passed to through a temporary object.
    scalar_procedures: std::collections::HashSet<String>,
    /// Ordinary scalar derived-valued functions, materialized once before
    /// splitting their results into array component storage.
    scalar_results: HashMap<String, String>,
    /// Known scalar results, including intrinsic-type mask functions.
    scalar_functions: std::collections::HashSet<String>,
    target_roots: std::collections::HashSet<String>,
    elemental_records: HashMap<String, elemental_records::Procedure>,
    elemental_results: HashMap<String, elemental_record_results::ResultProcedure>,
    elemental_functions: std::collections::HashSet<String>,
    /// Type-bound procedure names of each type.
    bindings: HashMap<String, std::collections::HashSet<String>>,
}
/// Storage of one field of an array of records. An array component `c` of a
/// scalar object `g` is stored as the ordinary array components `c$field`,
/// which scalar derived lowering then names `g%c$field`.
/// A field of a nested derived component, `p%x`, is stored as `p$x`.
fn storage(object: &str, field: &str) -> String {
    let field = field.replace('%', "$");
    if let Some(prefix) = object.strip_prefix("@record_view:") {
        return format!("{prefix}${field}");
    }
    if object.contains('%') {
        format!("{object}${field}")
    } else {
        format!("@record${object}${field}")
    }
}
fn deferred(rank: usize) -> Vec<Expr> {
    vec![Expr::Ident("@deferred_extent".into()); rank]
}
impl Lower {
    /// Replace each component that is an array of a derived type with one
    /// array component per field of that type, keeping its shape and its
    /// ALLOCATABLE or POINTER attribute.
    fn expand_array_components(&mut self, body: &mut [Stmt]) -> Result<()> {
        for s in body.iter_mut() {
            match s {
                Stmt::DerivedType { name, components, .. } => {
                    let mut out = vec![];
                    let mut arrays = vec![];
                    let mut scalars = vec![];
                    for c in std::mem::take(components) {
                        match c {
                            Stmt::DerivedDecl { type_name, names, attributes, dims, .. } if !dims.is_empty() => {
                                let fields = self.fields(&type_name)
                                    .with_context(|| format!("array component of type `{type_name}` in type `{name}`"))?;
                                if let Some(a) = attributes.iter().find(|a| !matches!(a.as_str(), "allocatable" | "pointer")) {
                                    bail!("attribute `{a}` on a derived array component is not yet supported");
                                }
                                for n in names {
                                    for f in &fields {
                                        let field = format!("{n}${}", f.name.replace('%', "$"));
                                        // Default initialization of fixed storage; allocated
                                        // storage takes the defaults when it is allocated.
                                        if let (Some(value), true) = (&f.default, attributes.is_empty()) {
                                            out.push(Stmt::InitializedDecl {
                                                kind: f.kind.clone(),
                                                attributes: vec![],
                                                entities: vec![(field.clone(), Some(value.clone()))],
                                            });
                                        }
                                        out.push(Stmt::ArrayDecl { kind: f.kind.clone(), name: field.clone(), dims: dims.clone() });
                                        if f.heap.is_some() {out.push(Stmt::DummyIntent {name:field.clone(),intent:"@record_slots".into()});}
                                        for a in &attributes {
                                            out.push(Stmt::DummyIntent { name: field.clone(), intent: a.clone() });
                                        }
                                    }
                                    arrays.push((n, Object { ty: type_name.clone(), dims: dims.clone() }));
                                }
                            }
                            Stmt::DerivedDecl { ref type_name, ref names, .. } => {
                                scalars.extend(names.iter().map(|n| (n.clone(), type_name.clone())));
                                out.push(c);
                            }
                            c => out.push(c),
                        }
                    }
                    *components = out;
                    if !arrays.is_empty() {
                        self.array_components.insert(name.clone(), arrays);
                    }
                    if !scalars.is_empty() {
                        self.scalar_components.insert(name.clone(), scalars);
                    }
                    self.collect(std::slice::from_ref(s))?;
                }
                Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } => self.expand_array_components(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    /// Register the array components of a scalar object, such as `g%nodes`,
    /// as derived arrays, including those of its derived components.
    fn register_components(&self, object: &str, ty: &str, objects: &mut Objects) {
        for (c, o) in self.array_components.get(ty).into_iter().flatten() {
            objects.insert(format!("{object}%{c}"), o.clone());
        }
        for (c, sub) in self.scalar_components.get(ty).into_iter().flatten() {
            self.register_components(&format!("{object}%{c}"), sub, objects);
        }
    }
    /// Default initialization of field `f` of every record of `object`. A
    /// scalar default fills the storage; an array default is copied into each
    /// record's component.
    fn default_fill(&mut self, object: &str, f: &Field, value: &Expr, rank: usize) -> Result<Vec<Stmt>> {
        let target = storage(object, &f.name);
        if f.dims.is_empty() || !matches!(value, Expr::Call(n, _) if n == "@array_constructor") {
            return Ok(vec![Stmt::Assign { name: target, value: value.clone() }]);
        }
        let mut out = vec![];
        let mut indices = vec![];
        for _ in 0..rank {
            self.serial += 1;
            let index = format!("@record_default_{}", self.serial);
            out.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![index.clone()] });
            indices.push(Expr::Ident(index));
        }
        let mut body = vec![assignment(select(object, f, vec![], &indices)?, value.clone())];
        for (axis, index) in indices.iter().enumerate().rev() {
            let Expr::Ident(var) = index else { unreachable!() };
            let bound = |name: &str| Expr::Call(name.into(), vec![Expr::Ident(target.clone()), Expr::IntLit((axis + 1 + f.dims.len()).to_string())]);
            body = vec![Stmt::Do { var: var.clone(), start: bound("lbound"), end: bound("ubound"), body }];
        }
        out.extend(body);
        Ok(out)
    }
    /// An element `a(i)` of an array of records, with scalar subscripts.
    fn element<'e>(&self, e: &'e Expr, objects: &Objects) -> Option<(&'e str, &'e [Expr])> {
        let (Expr::Call(n, idx) | Expr::Index(n, idx)) = e else { return None };
        let o = objects.get(n).filter(|o| !o.dims.is_empty())?;
        (idx.len() == o.dims.len() && idx.iter().all(|i| !matches!(i, Expr::Section { .. })
            && !matches!(i, Expr::Call(t, _) if t == "@section_subscript" || t == "@vector_subscript")
            && self.array_shape(i).is_none()))
            .then_some((n.as_str(), idx.as_slice()))
    }
    /// Copy an element of an array of records into a new scalar object, and,
    /// when `copy_back` is given, copy the object back after the statement.
    fn element_temp(&mut self, e: &Expr, objects: &mut Objects, prefix: &mut Vec<Stmt>,
        copy_back: Option<&mut Vec<Stmt>>) -> Result<String> {
        self.element_storage(e,objects,prefix,copy_back,true)
    }
    fn element_storage(&mut self, e: &Expr, objects: &mut Objects, prefix: &mut Vec<Stmt>,
        copy_back: Option<&mut Vec<Stmt>>, copy_in:bool) -> Result<String> {
        let (object, idx) = self.element(e, objects).context("a type-bound call requires a scalar object or an array element")?;
        let (object, mut idx) = (object.to_owned(), idx.to_vec());
        self.capture_indices(&mut idx, prefix);
        let ty = objects[&object].ty.clone();
        self.serial += 1;
        let temp = format!("@record_element_{}", self.serial);
        prefix.push(Stmt::DerivedDecl { type_name: ty.clone(), names: vec![temp.clone()], attributes: vec![], polymorphic: false, dims: vec![] });
        objects.insert(temp.clone(), Object { ty: ty.clone(), dims: vec![] });
        self.register_components(&temp, &ty, objects);
        for f in self.fields(&ty)? {
            let local = format!("{temp}%{}", f.name);
            if f.heap.is_some() {
                prefix.push(Stmt::DummyIntent {name:local.clone(),intent:"@record_heap_local".into()});
                if copy_in {prefix.push(Stmt::CallSub {name:"@heap_record_load".into(),args:vec![Expr::Ident(local),select(&object,&f,vec![],&idx)?]});}
            } else if copy_in {
                prefix.push(Stmt::Assign { name: local, value: select(&object, &f, vec![], &idx)? });
            }
        }
        if let Some(back) = copy_back {
            for f in self.fields(&ty)? {
                if f.heap.is_some() {
                    self.serial+=1;let handle=format!("@record_slot_{}",self.serial);
                    back.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![handle.clone()]});
                    back.push(Stmt::Assign{name:handle.clone(),value:select(&object,&f,vec![],&idx)?});
                    back.push(Stmt::CallSub {name:"@heap_record_store".into(),args:vec![Expr::Ident(format!("{temp}%{}",f.name)),Expr::Ident(handle.clone())]});
                    back.push(assignment(select(&object,&f,vec![],&idx)?,Expr::Ident(handle)));
                } else {
                    back.push(assignment(select(&object, &f, vec![], &idx)?, Expr::Ident(format!("{temp}%{}", f.name))));
                }
            }
        }
        Ok(temp)
    }
    fn collect(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::DerivedType {
                    name, components, ..
                } => {
                    let mut fields = vec![];
                    let mut layout = vec![];
                    for c in components {
                        match c {
                            Stmt::VarDecl { kind, names } => {
                                for name in names {
                                    fields.push(Field {
                                        name: name.clone(),
                                        kind: kind.clone(),
                                        default: None,
                                        dims: vec![],
                                        heap: None,
                                    });
                                    layout.push((name.clone(), None));
                                }
                            }
                            // A scalar derived component contributes its fields.
                            Stmt::DerivedDecl { type_name, names, attributes, dims, .. }
                                if dims.is_empty() && attributes.is_empty()
                                    && self.types.get(type_name).is_some_and(|f| !f.is_empty()) =>
                            {
                                for n in names {
                                    for f in self.types[type_name].clone() {
                                        fields.push(Field { name: format!("{n}%{}", f.name), ..f });
                                    }
                                    layout.push((n.clone(), Some(type_name.clone())));
                                }
                            }
                            Stmt::InitializedDecl { kind, entities, .. } => {
                                for (name, value) in entities {
                                    fields.push(Field {
                                        name: name.clone(),
                                        kind: kind.clone(),
                                        default: value.clone(),
                                        dims: vec![],
                                        heap: None,
                                    });
                                    layout.push((name.clone(), None));
                                }
                            }
                            Stmt::ArrayDecl { kind, name, dims }
                                if components.iter().any(|c| matches!(c, Stmt::DummyIntent {name:n,intent} if n==name && intent=="allocatable"))
                                    && matches!(kind,TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision | TypeSpec::Logical) =>
                            {
                                fields.push(Field {name:name.clone(),kind:TypeSpec::Integer(Some(8)),default:Some(Expr::IntLit("0".into())),dims:vec![],heap:Some((kind.clone(),dims.clone()))});
                                layout.push((name.clone(),None));
                            }
                            Stmt::DummyIntent {name,intent} if intent=="allocatable" && components.iter().any(|c|matches!(c,Stmt::ArrayDecl {name:n,kind,..} if n==name && matches!(kind,TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Real | TypeSpec::RealKind(_) | TypeSpec::DoublePrecision | TypeSpec::Logical))) => {}
                            Stmt::DummyIntent {intent,..} if intent=="@record_slots" => { fields.clear(); break; }
                            // A fixed-size array component; an initialized one
                            // follows its InitializedDecl.
                            Stmt::ArrayDecl { kind, name, dims }
                                if dims.iter().all(|d| crate::parameters::integer_constant(d).is_ok()) =>
                            {
                                match fields.iter_mut().find(|f| &f.name == name && f.dims.is_empty() && f.default.is_some()) {
                                    Some(f) => f.dims = dims.clone(),
                                    None => {
                                        fields.push(Field { name: name.clone(), kind: kind.clone(), default: None, dims: dims.clone(), heap: None });
                                        layout.push((name.clone(), None));
                                    }
                                }
                            }
                            Stmt::SourceLocation(_) => {}
                            Stmt::DummyIntent { name, .. } if name == crate::extensions::EXTENDS => {}
                            _ => {
                                fields.clear();
                                break;
                            }
                        }
                    }
                    self.types.insert(name.clone(), fields);
                    self.layouts.insert(name.clone(), layout);
                    if let Stmt::DerivedType { bindings, .. } = s {
                        self.bindings.insert(name.clone(), bindings.iter().map(|(b, _)| b.clone()).filter(|b| !b.starts_with('@')).collect());
                    }
                }
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    if matches!(s, Stmt::Function { elemental: true, .. }) {
                        self.elemental_functions.insert(name.clone());
                    }
                    if matches!(s, Stmt::Function { elemental: true, .. } | Stmt::Subroutine { elemental: true, .. }) {
                        self.collect_elemental(name, params, body);
                    }
                    let arrays: Objects = body
                        .iter()
                        .flat_map(|s| match s {
                            Stmt::DerivedDecl {
                                type_name,
                                names,
                                dims,
                                ..
                            } if !dims.is_empty() => names
                                .iter()
                                .map(|n| {
                                    (
                                        n.clone(),
                                        Object {
                                            ty: type_name.clone(),
                                            dims: dims.clone(),
                                        },
                                    )
                                })
                                .collect::<Vec<_>>(),
                            _ => vec![],
                        })
                        .collect();
                    let result = if matches!(s, Stmt::Function { .. }) {
                        let n = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                            _ => name,
                        };
                        if !body.iter().any(|s| match s {
                            Stmt::ArrayDecl{name,dims,..} => name==n && !dims.is_empty(),
                            Stmt::DerivedDecl{names,dims,..} => names.contains(n) && !dims.is_empty(),
                            _ => false,
                        }) { self.scalar_functions.insert(name.clone()); }
                        if let Some(ty) = body.iter().find_map(|s| match s {
                            Stmt::DerivedDecl { type_name, names, dims, .. }
                                if dims.is_empty() && names.contains(n) => Some(type_name.clone()),
                            _ => None,
                        }) {
                            self.scalar_results.insert(name.clone(), ty);
                        }
                        arrays.get(n).cloned().map(|o| (n.clone(), o))
                    } else {
                        None
                    };
                    if body.iter().any(|s| matches!(s, Stmt::DerivedDecl { names, dims, .. }
                        if dims.is_empty() && names.iter().any(|n| params.contains(n))))
                    {
                        self.scalar_procedures.insert(name.clone());
                    }
                    if result.is_some() || params.iter().any(|n| arrays.contains_key(n)) {
                        self.procedures.insert(
                            name.clone(),
                            Procedure {
                                signature: crate::named_arguments::Signature::new(params, body),
                                arrays,
                                result,
                            },
                        );
                    }
                    self.collect(body)?;
                }
                Stmt::Module { body, .. }
                | Stmt::Block { body }
                | Stmt::Do { body, .. }
                | Stmt::DoWhile { body, .. } => self.collect(body)?,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.collect(then_body)?;
                    if let Some(b) = else_body {
                        self.collect(b)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn fields(&self, ty: &str) -> Result<Vec<Field>> {
        let fields = self
            .types
            .get(ty)
            .with_context(|| format!("unknown derived array type `{ty}`"))?;
        if fields.is_empty()
            || fields.iter().any(|f| {
                !matches!(
                    f.kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::IntegerKind(_)
                        | TypeSpec::Real
                        | TypeSpec::RealKind(_)
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                        | TypeSpec::LogicalKind(_)
                        | TypeSpec::Character(_)
                        | TypeSpec::CharacterLength(_)
                        | TypeSpec::CharacterAssumed
                )
            })
        {
            bail!("arrays of type `{ty}` are not yet supported: the components of an array of a derived type must be scalars, fixed-size arrays or scalars of a derived type, not ALLOCATABLE or POINTER");
        }
        Ok(fields.clone())
    }
    fn component(&self, n: &mut String, objects: &Objects) -> Result<()> {
        self.component_field(n, objects).map(|_| ())
    }
    /// Rename `object%field` of an array of records to its storage, returning
    /// the object and field.
    fn component_field(&self, n: &mut String, objects: &Objects) -> Result<Option<(String, Field)>> {
        // The array of records is the first prefix that names one, as `a` in
        // `a%p%x` or `g%nodes` in `g%nodes%w`.
        let split = n.match_indices('%').map(|(i, _)| (&n[..i], &n[i + 1..]))
            .find(|(object, _)| objects.get(*object).is_some_and(|o| !o.dims.is_empty()));
        if let Some((object, field)) = split {
            if let Some(o) = objects.get(object).filter(|o| !o.dims.is_empty()) {
                let f = self.fields(&o.ty)?.into_iter().find(|f| f.name == field)
                    .with_context(|| format!("unknown component `{field}` of `{}`", o.ty))?;
                if f.heap.is_some() {bail!("allocatable component `{field}` requires a scalar parent element");}
                let object = object.to_owned();
                *n = storage(&object, field);
                return Ok(Some((object, f)));
            }
        }
        Ok(None)
    }
    /// The first field and the number of its component axes, for inquiries
    /// about the shape of the array of records.
    fn shape_field(&self, ty: &str) -> Result<(String, usize)> {
        let fields = self.fields(ty)?;
        let f = fields.iter().find(|f| f.dims.is_empty()).unwrap_or(&fields[0]);
        Ok((f.name.clone(), f.dims.len()))
    }
    fn array_shape(&self, e: &Expr) -> Option<Vec<Expr>> {
        match e {
            Expr::Ident(n) => self.array_shapes.get(n).cloned(),
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                let source = self.array_shapes.get(name)?;
                if source.len() != 1 {
                    return None;
                }
                let bounds = match &source[0] {
                    Expr::Call(tag, a) if tag == "@declared_bounds" => (
                        crate::parameters::integer_constant(&a[0]).ok()?,
                        crate::parameters::integer_constant(&a[1]).ok()?,
                    ),
                    d => (1, crate::parameters::integer_constant(d).ok()?),
                };
                let step = stride
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(1);
                if step == 0 {
                    return None;
                }
                let first = lower
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(if step > 0 { bounds.0 } else { bounds.1 });
                let end = upper
                    .as_deref()
                    .map(crate::parameters::integer_constant)
                    .transpose()
                    .ok()?
                    .unwrap_or(if step > 0 { bounds.1 } else { bounds.0 });
                Some(vec![Expr::IntLit(
                    (((i128::from(end) - i128::from(first) + i128::from(step)) / i128::from(step))
                        .max(0))
                    .to_string(),
                )])
            }
            Expr::Call(n, a) if n == "@typed_constructor" && a.len() == 1 => {
                Some(vec![Expr::IntLit("0".into())])
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                Some(vec![Expr::IntLit(a.len().to_string())])
            }
            Expr::Call(n, a) if n == "@vector_subscript" => self.array_shape(&a[0]),
            Expr::Bin(_, a, b) => self.array_shape(a).or_else(|| self.array_shape(b)),
            Expr::Un(_, a) => self.array_shape(a),
            _ => None,
        }
    }
    fn capture_indices(&mut self, args: &mut [Expr], prefix: &mut Vec<Stmt>) {
        for arg in args {
            if let Some(dims) = self.array_shape(arg).filter(|d| !d.is_empty()) {
                // Captured vectors have lower bound one; retain known capacity
                // even when the original index vector has explicit bounds.
                let dims = dims
                    .into_iter()
                    .map(|d| {
                        crate::vector_subscripts::fixed_extent(&d)
                            .map_or(d, |n| Expr::IntLit(n.to_string()))
                    })
                    .collect::<Vec<_>>();
                self.serial += 1;
                let n = format!("@record_vector_index_{}", self.serial);
                prefix.push(Stmt::ArrayDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    name: n.clone(),
                    dims: if dims
                        .iter()
                        .all(|e| crate::parameters::integer_constant(e).is_ok())
                    {
                        dims.clone()
                    } else {
                        deferred(1)
                    },
                });
                if dims
                    .iter()
                    .any(|e| crate::parameters::integer_constant(e).is_err())
                {
                    prefix.push(Stmt::DummyIntent {
                        name: n.clone(),
                        intent: "allocatable".into(),
                    });
                    prefix.push(Stmt::CallSub {
                        name: "@allocate".into(),
                        args: vec![Expr::Call(
                            n.clone(),
                            vec![Expr::Call("size".into(), vec![arg.clone()])],
                        )],
                    });
                }
                prefix.push(Stmt::Assign {
                    name: n.clone(),
                    value: match arg {
                        Expr::Call(tag, a) if tag == "@vector_subscript" => a[0].clone(),
                        _ => arg.clone(),
                    },
                });
                self.array_shapes.insert(n.clone(), dims);
                *arg = Expr::Ident(n);
                continue;
            }
            match arg {
                Expr::Section {
                    lower,
                    upper,
                    stride,
                    ..
                } => {
                    for e in [lower, upper, stride].into_iter().flatten() {
                        self.capture_indices(std::slice::from_mut(e.as_mut()), prefix);
                    }
                }
                Expr::Call(tag, a) if tag == "@section_subscript" => {
                    self.capture_indices(a, prefix)
                }
                _ => {
                    if crate::parameters::integer_constant(arg).is_ok()
                        || matches!(arg, Expr::Ident(_))
                    {
                        continue;
                    }
                    self.serial += 1;
                    let n = format!("@record_index_{}", self.serial);
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![n.clone()],
                    });
                    prefix.push(Stmt::Assign {
                        name: n.clone(),
                        value: arg.clone(),
                    });
                    *arg = Expr::Ident(n);
                }
            }
        }
    }
    fn parts(
        &mut self,
        mut e: Expr,
        ty: &str,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<(Vec<Expr>, bool)> {
        self.expression(&mut e, objects, prefix)?;
        self.prepared_parts(e, ty, objects, prefix)
    }
    /// Split a value whose expressions have already been lowered. Revisiting
    /// calls would interpret the flattened ABI arguments as source arguments.
    fn prepared_parts(
        &mut self,
        e: Expr,
        ty: &str,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<(Vec<Expr>, bool)> {
        let fields = self.fields(ty)?;
        if fields.iter().any(|f|f.heap.is_some()) && matches!(&e, Expr::Call(n,_) if n==ty || n=="@array_constructor" || n=="@record_constructor") {
            bail!("array constructors of records with allocatable components are not yet supported");
        }
        match e {
            Expr::Call(n, args) if n.starts_with("@complex_array$") && crate::complex::type_kind(ty).is_some() => {
                if args.len() != 2 { bail!("invalid internal COMPLEX array components"); }
                Ok((args, true))
            }
            Expr::Ident(n) if objects.contains_key(&n) => {
                let o = &objects[&n];
                if o.ty != ty {
                    bail!(
                        "derived array type mismatch: expected `{ty}`, got `{}`",
                        o.ty
                    );
                }
                Ok((
                    fields
                        .iter()
                        .map(|f| {
                            Expr::Ident(if o.dims.is_empty() {
                                format!("{n}%{}", f.name)
                            } else {
                                storage(&n, &f.name)
                            })
                        })
                        .collect(),
                    !o.dims.is_empty(),
                ))
            }
            Expr::Call(n, mut args) if objects.contains_key(&n) => {
                let o = &objects[&n];
                if o.ty != ty {
                    bail!("derived array type mismatch");
                }
                let array = args.iter().any(|e| {
                    self.array_shape(e).is_some()
                        || matches!(e, Expr::Section { .. })
                        || matches!(e,Expr::Call(tag,_) if tag=="@section_subscript")
                });
                self.capture_indices(&mut args, prefix);
                Ok((
                    fields
                        .iter()
                        .map(|f| if f.dims.is_empty() { Ok(Expr::Call(storage(&n, &f.name), args.clone())) } else { select(&n, f, vec![], &args) })
                        .collect::<Result<_>>()?,
                    array,
                ))
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } if objects.contains_key(&name) => {
                if objects[&name].ty != ty {
                    bail!("derived array type mismatch");
                }
                Ok((
                    fields
                        .iter()
                        .map(|f| if f.dims.is_empty() {
                            Ok(Expr::Section {
                                name: storage(&name, &f.name),
                                lower: lower.clone(),
                                upper: upper.clone(),
                                stride: stride.clone(),
                            })
                        } else {
                            select(&name, f, vec![], &[Expr::Section { name: String::new(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() }])
                        })
                        .collect::<Result<_>>()?,
                    true,
                ))
            }
            Expr::Call(n, mut args) if n == ty && fields.iter().any(|f| f.name.contains('%')) => {
                let layout = self.layouts[ty].clone();
                let defaulted = |c: &str| fields.iter().filter(|f| f.name == c || f.name.starts_with(&format!("{c}%")))
                    .all(|f| f.default.is_some());
                let signature = crate::named_arguments::Signature {
                    params: layout.iter().map(|(c, _)| c.clone()).collect(),
                    optional: layout.iter().filter(|(c, _)| defaulted(c)).map(|(c, _)| c.clone()).collect(),
                };
                signature.reorder(ty, &mut args)?;
                args.resize_with(layout.len(), || Expr::Call(crate::named_arguments::OMITTED.into(), vec![]));
                let mut values = vec![];
                for (arg, (c, sub)) in args.into_iter().zip(&layout) {
                    let omitted = crate::named_arguments::omitted(&arg);
                    match sub {
                        Some(sub) if !omitted => {
                            let (parts, array) = self.prepared_parts(arg, sub, objects, prefix)?;
                            if array {
                                bail!("component `{c}` of a `{ty}` constructor requires a scalar value");
                            }
                            values.extend(parts);
                        }
                        _ => {
                            for f in fields.iter().filter(|f| &f.name == c || f.name.starts_with(&format!("{c}%"))) {
                                values.push(if omitted {
                                    f.default.clone().with_context(|| format!("missing derived constructor component `{c}`"))?
                                } else {
                                    arg.clone()
                                });
                            }
                        }
                    }
                }
                Ok((values, false))
            }
            Expr::Call(n, mut args) if n == ty => {
                let signature = crate::named_arguments::Signature {
                    params: fields.iter().map(|f| f.name.clone()).collect(),
                    optional: fields
                        .iter()
                        .filter(|f| f.default.is_some())
                        .map(|f| f.name.clone())
                        .collect(),
                };
                signature.reorder(ty, &mut args)?;
                args.resize_with(fields.len(), || {
                    Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
                });
                for (arg, f) in args.iter_mut().zip(&fields) {
                    if crate::named_arguments::omitted(arg) {
                        *arg = f
                            .default
                            .clone()
                            .context("missing derived constructor component")?;
                    }
                }
                Ok((args, false))
            }
            Expr::Call(n, args) if n == "@record_constructor" => {
                let Some(Expr::Ident(name)) = args.first() else {
                    bail!("invalid derived constructor type");
                };
                if name != ty {
                    bail!("derived array constructor type mismatch");
                }
                if args.len() == 1 {
                    return Ok((
                        fields
                            .iter()
                            .map(|f| crate::empty_constructors::parsed(f.kind.clone(), vec![]))
                            .collect(),
                        true,
                    ));
                }
                self.prepared_parts(
                    Expr::Call("@array_constructor".into(), args[1..].to_vec()),
                    ty,
                    objects,
                    prefix,
                )
            }
            Expr::Call(n, args) if n == "@array_constructor" => {
                if fields.iter().any(|f| !f.dims.is_empty()) {
                    bail!("array constructors of type `{ty}` with array components are not yet supported");
                }
                let mut values = vec![vec![]; fields.len()];
                for e in args {
                    // An array-valued item contributes all of its records in
                    // array element order, just as for intrinsic constructors.
                    let (parts, _) = self.prepared_parts(e, ty, objects, prefix)?;
                    for (out, e) in values.iter_mut().zip(parts) {
                        out.push(e);
                    }
                }
                Ok((
                    values
                        .into_iter()
                        .map(|v| Expr::Call("@array_constructor".into(), v))
                        .collect(),
                    true,
                ))
            }
            Expr::Call(n, args) if self.scalar_results.contains_key(&n)
                || n=="merge" && args.len()==3 && self.array_shape(&args[2]).is_none()
                    && !args[..2].iter().any(|arg| match arg {
                        Expr::Ident(n) | Expr::Section {name:n,..} | Expr::Call(n,_) | Expr::Index(n,_) =>
                            objects.get(n).is_some_and(|o|!o.dims.is_empty()),
                        _ => false,
                    }) => {
                if self.scalar_results.get(&n).is_some_and(|result| result != ty) {
                    bail!("derived result type mismatch: expected `{ty}`, got `{}`", self.scalar_results[&n]);
                }
                self.serial += 1;
                let temp = format!("@scalar_record_result_{}", self.serial);
                prefix.push(Stmt::DerivedDecl {
                    type_name: ty.into(), names: vec![temp.clone()],
                    attributes: vec![], polymorphic: false, dims: vec![],
                });
                prefix.push(Stmt::Assign { name: temp.clone(), value: Expr::Call(n, args) });
                objects.insert(temp.clone(), Object { ty: ty.into(), dims: vec![] });
                self.register_components(&temp, ty, objects);
                Ok((fields.iter().map(|f| Expr::Ident(format!("{temp}%{}", f.name))).collect(), false))
            }
            _ => bail!("unsupported derived array value of type `{ty}`"),
        }
    }
    /// Flatten actuals after expression evaluation and ELEMENTAL adaptation.
    fn arguments(
        &mut self,
        name: &str,
        args: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<Option<(String, Object)>> {
        let Some(p) = self.procedures.get(name).cloned() else { return Ok(None) };
        p.signature.reorder(name, args)?;
        args.resize_with(p.signature.params.len(), || {
            Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
        });
        let mut flat = vec![];
        for (e, param) in std::mem::take(args).into_iter().zip(&p.signature.params) {
            if let Some(o) = p.arrays.get(param) {
                if crate::named_arguments::omitted(&e) {
                    flat.extend(std::iter::repeat_n(e, self.fields(&o.ty)?.len()));
                    continue;
                }
                // An element passes the rest of its array from it (sequence
                // association), which only by-reference calls support, and a
                // scalar variable passes its parts.
                let simple = |values: &[Expr]| values.iter().all(|v| matches!(v, Expr::Call(..) | Expr::Index(..) | Expr::Ident(_)));
                let values = match self.prepared_parts(e.clone(), &o.ty, objects, prefix) {
                    Ok((values, array)) if array || simple(&values) => values,
                    _ if self.fields(&o.ty)?.iter().all(|f| f.dims.is_empty()) => {
                        // An expression passes its value in a scalar temporary.
                        self.serial += 1;
                        let temp = format!("@record_actual_{}", self.serial);
                        prefix.push(Stmt::DerivedDecl { type_name: o.ty.clone(), names: vec![temp.clone()], attributes: vec![], polymorphic: false, dims: vec![] });
                        objects.insert(temp.clone(), Object { ty: o.ty.clone(), dims: vec![] });
                        self.register_components(&temp, &o.ty, objects);
                        prefix.push(Stmt::Assign { name: temp.clone(), value: e });
                        self.fields(&o.ty)?.iter().map(|f| Expr::Ident(format!("{temp}%{}", f.name))).collect()
                    }
                    _ => bail!("derived array dummy `{param}` requires an array actual"),
                };
                flat.extend(values);
            } else {
                flat.push(e);
            }
        }
        *args = flat;
        Ok(p.result)
    }
    /// A scalar derived component selected across a record array is a view
    /// of the existing component arrays, with the parent's shape/subscripts.
    /// The alias has no storage and must never acquire ownership of it.
    fn scalar_component_view(&mut self, e: &mut Expr, objects: &mut Objects,
        prefix: &mut Vec<Stmt>) -> Result<bool> {
        let (mut parent, field) = match e {
            Expr::Ident(n) => match n.rsplit_once('%') {
                Some((parent, field)) => (Expr::Ident(parent.into()), field.to_owned()),
                None => return Ok(false),
            },
            Expr::Call(tag, args) if tag.starts_with("@record_component:") && args.len() == 1 =>
                (args[0].clone(), tag[18..].to_owned()),
            _ => return Ok(false),
        };
        let mut evaluation = vec![];
        self.expression(&mut parent, objects, &mut evaluation)?;
        let owner = match &parent {
            Expr::Ident(n) | Expr::Section {name:n,..} | Expr::Call(n,_) | Expr::Index(n,_) => n,
            _ => return Ok(false),
        };
        let Some(object) = objects.get(owner).filter(|o| !o.dims.is_empty()).cloned() else { return Ok(false) };
        let mut ty = object.ty.clone();
        for component in field.split('%') {
            let Some(child) = self.scalar_components.get(&ty).into_iter().flatten()
                .find(|(name,_)| name == component).map(|(_,ty)| ty.clone()) else { return Ok(false) };
            ty = child;
        }
        let alias = format!("@record_view:{}", storage(owner, &field));
        objects.insert(alias.clone(), Object {ty, dims:object.dims});
        match &mut parent {
            Expr::Ident(n) | Expr::Section {name:n,..} | Expr::Call(n,_) | Expr::Index(n,_) => *n = alias,
            _ => unreachable!(),
        }
        prefix.extend(evaluation);
        *e = parent;
        Ok(true)
    }
    fn expression(
        &mut self,
        e: &mut Expr,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        if self.pointer_inquiry(e, objects, prefix)? { return Ok(()); }
        if self.scalar_component_view(e, objects, prefix)? { return Ok(()); }
        self.heap_component_actual(e,objects,prefix,None)?;
        if self.materialize_elemental_result(e, objects, prefix)? { return Ok(()); }
        if self.constructor_records(e, objects, prefix)? { return Ok(()); }
        match e {
            Expr::Ident(n) => self.component(n, objects)?,
            Expr::Call(n, args) if n.starts_with("@record_component:") => {
                let field = n[18..].to_owned();
                let component = component_subscripts(args);
                let [value] = args.as_mut_slice() else {
                    bail!("invalid record component selection");
                };
                self.expression(value, objects, prefix)?;
                let owner=match value {Expr::Ident(n)|Expr::Section{name:n,..}|Expr::Call(n,_)|Expr::Index(n,_)=>Some(n.as_str()),_=>None};
                if owner.is_some_and(|owner|self.heap_component(owner,&field,objects)) && self.element(value,objects).is_none() {
                    bail!("allocatable component `{field}` requires a scalar parent element");
                }
                let mut component = component;
                for e in &mut component {
                    self.expression(e, objects, prefix)?;
                }
                // A type-bound function of an array element, `a(i)%f(...)`.
                let binding = self.element(value, objects).map(|(n, _)| objects[n].ty.clone())
                    .is_some_and(|ty| self.bindings.get(&ty).is_some_and(|b| b.contains(&field)));
                if binding {
                    let temp = self.element_temp(value, objects, prefix, None)?;
                    *e = Expr::Call(format!("{temp}%{field}"), component);
                    return Ok(());
                }
                if let Expr::Section { name, lower, upper, stride } = value {
                    if let Some(f) = objects.get(name.as_str()).map(|o| self.fields(&o.ty)).transpose()?
                        .and_then(|fields| fields.into_iter().find(|f| f.name == field && (!f.dims.is_empty() || !component.is_empty())))
                    {
                        let section = Expr::Section { name: String::new(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() };
                        *e = select(name, &f, component, &[section])?;
                        return Ok(());
                    }
                }
                if let Expr::Call(object, indices) | Expr::Index(object, indices) = value {
                    if let Some(f) = objects.get(object.as_str()).map(|o| self.fields(&o.ty)).transpose()?
                        .and_then(|fields| fields.into_iter().find(|f| f.name == field && (!f.dims.is_empty() || !component.is_empty())))
                    {
                        *e = select(object, &f, component, indices)?;
                        return Ok(());
                    }
                }
                // A section selects the same section of the component's storage.
                if let Expr::Section { name, lower, upper, stride } = value {
                    let o = objects
                        .get(name)
                        .context("component selection requires a derived array")?;
                    if !self.fields(&o.ty)?.iter().any(|f| f.name == field) {
                        bail!("unknown component `{field}`");
                    }
                    *e = Expr::Section { name: storage(name, &field), lower: lower.clone(), upper: upper.clone(), stride: stride.clone() };
                    return Ok(());
                }
                let (object, indices) = match value {
                    Expr::Call(n, a) | Expr::Index(n, a) => (n, a),
                    _ => bail!("record element component requires an array element"),
                };
                let o = objects
                    .get(object)
                    .context("component selection requires a derived array")?;
                if !self.fields(&o.ty)?.iter().any(|f| f.name == field) {
                    bail!("unknown component `{field}`");
                }
                *e = Expr::Call(storage(object, &field), indices.clone());
            }
            Expr::Call(n, args)
                if matches!(
                    n.as_str(),
                    "size" | "shape" | "lbound" | "ubound" | "allocated" | "present"
                ) =>
            {
                for e in args.iter_mut() {
                    self.expression(e, objects, prefix)?;
                }
                if let Some(array) = args.first_mut() {
                    let owner=match array {Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)|Expr::Section{name:n,..}=>Some(n),_=>None};
                    if let Some(owner)=owner {
                        if let Some(o) = objects.get(owner).filter(|o| !o.dims.is_empty()) {
                            let (field, axes) = self.shape_field(&o.ty)?;
                            if axes > 0 {
                                bail!("shape inquiries of arrays of type `{}` require a scalar component", o.ty);
                            }
                            *owner = storage(owner, &field);
                        }
                    }
                }
            }
            Expr::Call(n, args) => {
                self.component(n, objects)?;
                if objects.contains_key(n) {
                    for e in args {
                        self.expression(e, objects, prefix)?;
                    }
                    return Ok(());
                }
                let intrinsic_pack = n=="pack" && !self.procedures.contains_key(n)
                    && !self.scalar_results.contains_key(n) && !self.scalar_procedures.contains(n);
                if intrinsic_pack {
                    crate::named_arguments::Signature {
                        params: vec!["array".into(),"mask".into(),"vector".into()],
                        optional: ["vector".into()].into_iter().collect(),
                    }.reorder(n, args)?;
                }
                // Normalize each actual once, before inspecting its resulting
                // shape or splitting record storage into ABI arguments.
                for arg in args.iter_mut() {
                    self.expression(arg, objects, prefix)?;
                }
                if intrinsic_pack {
                    if let Some(result) = self.pack_records(args, objects, prefix)? {
                        *e = Expr::Ident(result);
                        return Ok(());
                    }
                }
                self.elemental_actuals(n, args, objects, prefix)?;
                // MERGE has scalar record actuals when its sources are array
                // elements. Preserve their values before scalar record lowering
                // removes the original arrays in favor of component storage.
                if n=="merge" && args.len()==3 {
                    for actual in &mut args[..2] {
                        if self.element(actual, objects).is_some() {
                            *actual = Expr::Ident(self.element_temp(actual, objects, prefix, None)?);
                        }
                    }
                }
                // An element passed to a function's scalar derived dummy.
                if self.scalar_procedures.contains(n.as_str()) {
                    for a in args.iter_mut() {
                        if self.element(a, objects).is_some() {
                            *a = Expr::Ident(self.element_temp(a, objects, prefix, None)?);
                        }
                    }
                }
                if let Some((_, o)) = self.arguments(n, args, objects, prefix)? {
                    self.serial += 1;
                    let temp = format!("@record_result_{}", self.serial);
                    for f in self.fields(&o.ty)? {
                        let field = storage(&temp, &f.name);
                        prefix.push(Stmt::ArrayDecl {
                            kind: f.kind.clone(),
                            name: field.clone(),
                            dims: deferred(f.dims.len() + o.dims.len()),
                        });
                        prefix.push(Stmt::DummyIntent {
                            name: field.clone(),
                            intent: "allocatable".into(),
                        });
                        args.push(Expr::Ident(field));
                    }
                    prefix.push(Stmt::CallSub {
                        name: format!("@record_result${n}"),
                        args: args.clone(),
                    });
                    objects.insert(
                        temp.clone(),
                        Object {
                            ty: o.ty,
                            dims: deferred(o.dims.len()),
                        },
                    );
                    *e = Expr::Ident(temp);
                }
            }
            Expr::Index(n, args) => {
                self.component(n, objects)?;
                for e in args {
                    self.expression(e, objects, prefix)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                self.component(name, objects)?;
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, objects, prefix)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, objects, prefix)?;
                self.expression(b, objects, prefix)?;
            }
            Expr::Un(_, e) => self.expression(e, objects, prefix)?,
            _ => {}
        }
        Ok(())
    }
    /// Transfer each primitive component of one record, expanding fixed array
    /// components before proceeding to the next record.
    fn output_fields(&mut self, array: &str, ty: &str, indices: &[Expr],
        prefix: &mut Vec<Stmt>) -> Result<Vec<Expr>> {
        let fields = self.fields(ty)?;
        if fields.iter().any(|f| f.heap.is_some()) {
            bail!("output of a record with allocatable components requires defined I/O");
        }
        let mut items = vec![];
        for f in &fields {
            if f.dims.is_empty() {
                items.push(Expr::Index(storage(array, &f.name), indices.to_vec()));
                continue;
            }
            // An array component: its elements in array element order.
            self.serial += 1;
            let inner = format!("@record_output_{}", self.serial);
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![inner.clone()] });
            let mut ordinal = Expr::Bin(crate::ast::BinOp::Sub, Box::new(Expr::Ident(inner.clone())), Box::new(Expr::IntLit("1".into())));
            let mut subscripts = vec![];
            let mut total = 1i64;
            for d in &f.dims {
                let extent = crate::parameters::integer_constant(d)?.max(0);
                total *= extent;
                let extent = Expr::IntLit(extent.to_string());
                subscripts.push(Expr::Bin(crate::ast::BinOp::Add,
                    Box::new(Expr::Call("mod".into(), vec![ordinal.clone(), extent.clone()])),
                    Box::new(Expr::IntLit("1".into()))));
                ordinal = Expr::Bin(crate::ast::BinOp::Div, Box::new(ordinal), Box::new(extent));
            }
            subscripts.extend(indices.iter().cloned());
            items.push(Expr::Call("@output_implied_do".into(), vec![
                Expr::Ident(inner), Expr::IntLit("1".into()), Expr::IntLit(total.to_string()), Expr::IntLit("1".into()),
                Expr::Index(storage(array, &f.name), subscripts),
            ]));
        }
        Ok(items)
    }
    fn output(
        &mut self,
        items: &mut Vec<Expr>,
        objects: &mut Objects,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let mut expanded = vec![];
        for mut item in std::mem::take(items) {
            self.expression(&mut item, objects, prefix)?;
            if let Expr::Ident(array) = &item {
                if let Some(o) = objects.get(array).filter(|o| !o.dims.is_empty()).cloned() {
                    let (field, offset) = self.shape_field(&o.ty)?;
                    let first = storage(array, &field);
                    self.serial += 1;
                    let index = format!("@record_output_{}", self.serial);
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![index.clone()],
                    });
                    let mut ordinal = Expr::Bin(
                        crate::ast::BinOp::Sub,
                        Box::new(Expr::Ident(index.clone())),
                        Box::new(Expr::IntLit("1".into())),
                    );
                    let mut indices = vec![];
                    for axis in 1..=o.dims.len() {
                        let extent = Expr::Call(
                            "size".into(),
                            vec![Expr::Ident(first.clone()), Expr::IntLit((axis + offset).to_string())],
                        );
                        indices.push(Expr::Bin(
                            crate::ast::BinOp::Add,
                            Box::new(Expr::Call(
                                "mod".into(),
                                vec![ordinal.clone(), extent.clone()],
                            )),
                            Box::new(Expr::IntLit("1".into())),
                        ));
                        ordinal =
                            Expr::Bin(crate::ast::BinOp::Div, Box::new(ordinal), Box::new(extent));
                    }
                    let count = (1..=o.dims.len())
                        .map(|axis| Expr::Call("size".into(), vec![Expr::Ident(first.clone()), Expr::IntLit((axis + offset).to_string())]))
                        .reduce(|a, b| Expr::Bin(crate::ast::BinOp::Mul, Box::new(a), Box::new(b)))
                        .unwrap_or(Expr::IntLit("1".into()));
                    let mut args = vec![
                        Expr::Ident(index),
                        Expr::IntLit("1".into()),
                        count,
                        Expr::IntLit("1".into()),
                    ];
                    args.extend(self.output_fields(array, &o.ty, &indices, prefix)?);
                    expanded.push(Expr::Call("@output_implied_do".into(), args));
                    continue;
                }
            }
            if let Expr::Call(array, _) | Expr::Index(array, _) | Expr::Section { name: array, .. } = &item {
                if let Some(o) = objects.get(array).filter(|o| !o.dims.is_empty()).cloned() {
                    let mut indices = match &item {
                        Expr::Call(_, args) | Expr::Index(_, args) => args.clone(),
                        Expr::Section { lower, upper, stride, .. } => vec![Expr::Section {
                            name: String::new(), lower: lower.clone(), upper: upper.clone(), stride: stride.clone(),
                        }],
                        _ => unreachable!(),
                    };
                    if indices.len() != o.dims.len() { bail!("derived array output requires {} subscripts", o.dims.len()); }
                    // Capture bounds and vectors once, before iterating records
                    // or components; a subscript may contain an impure call.
                    self.capture_indices(&mut indices, prefix);
                    let (field, offset) = self.shape_field(&o.ty)?;
                    let source = storage(array, &field);
                    let mut loops = vec![];
                    for (axis, index) in indices.iter_mut().enumerate() {
                        let section = match &*index {
                            Expr::Section { .. } => Some(index.clone()),
                            Expr::Call(tag, args) if tag=="@section_subscript" => args.first().cloned(),
                            _ => None,
                        };
                        let vector = self.array_shape(index).is_some_and(|dims| !dims.is_empty());
                        let vector = vector && section.is_none();
                        if section.is_none() && !vector { continue; }
                        self.serial += 1;
                        let name = format!("@record_output_axis_{}", self.serial);
                        prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![name.clone()] });
                        let (first, last, step) = if let Some(Expr::Section { lower, upper, stride, .. }) = section {
                            let step = stride.map(|s| *s).unwrap_or(Expr::IntLit("1".into()));
                            let bound = |n: &str| Expr::Call(n.into(), vec![Expr::Ident(source.clone()), Expr::IntLit((axis+offset+1).to_string())]);
                            // Omitted bounds are the declared bounds regardless
                            // of the sign of the stride.
                            (lower.map(|s| *s).unwrap_or_else(|| bound("lbound")),
                             upper.map(|s| *s).unwrap_or_else(|| bound("ubound")), step)
                        } else {
                            (Expr::IntLit("1".into()), Expr::Call("size".into(), vec![index.clone()]), Expr::IntLit("1".into()))
                        };
                        *index = if vector {
                            let Expr::Ident(vector) = index else { bail!("derived output vector requires captured storage"); };
                            Expr::Index(vector.clone(), vec![Expr::Ident(name.clone())])
                        } else { Expr::Ident(name.clone()) };
                        let mut controls = [first, last, step];
                        self.capture_indices(&mut controls, prefix);
                        let [first, last, step] = controls;
                        loops.push((name, first, last, step));
                    }
                    let mut values = self.output_fields(array, &o.ty, &indices, prefix)?;
                    // Axis one varies fastest, so wrap its loop first.
                    for (name, first, last, step) in loops {
                        let mut args = vec![Expr::Ident(name), first, last, step];
                        args.extend(values);
                        values = vec![Expr::Call("@output_implied_do".into(), args)];
                    }
                    expanded.extend(values);
                    continue;
                }
            }
            expanded.push(item);
        }
        *items = expanded;
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Objects) -> Result<()> {
        let mut objects = inherited.clone();
        let outer_shapes = self.array_shapes.clone();
        let outer_targets = self.target_roots.clone();
        for statement in body.iter() {
            if let Stmt::DerivedDecl {names,attributes,..}=statement {
                for name in names {
                    if attributes.iter().any(|a|a=="target") {self.target_roots.insert(name.clone());}
                    else {self.target_roots.remove(name);}
                }
            }
        }
        for statement in body.iter() {
            if let Stmt::DummyIntent {name,intent}=statement {
                if intent=="target" {self.target_roots.insert(name.clone());}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::ArrayDecl { name, dims, .. } => {
                    self.array_shapes.insert(name.clone(), dims.clone());
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        self.array_shapes.remove(n);
                    }
                }
                _ => {}
            }
        }
        for s in body.iter() {
            match s {
                Stmt::DerivedDecl {
                    type_name,
                    names,
                    dims,
                    ..
                } => {
                    for n in names {
                        objects.insert(
                            n.clone(),
                            Object {
                                ty: type_name.clone(),
                                dims: dims.clone(),
                            },
                        );
                        if dims.is_empty() {
                            self.register_components(n, type_name, &mut objects);
                        }
                    }
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        objects.remove(n);
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    objects.remove(name);
                }
                _ => {}
            }
        }
        let mut out = vec![];
        let mut offset = None;
        for mut s in std::mem::take(body) {
            if let Stmt::SourceLocation(p) = s {
                offset = Some(p);
                out.push(Stmt::SourceLocation(p));
                continue;
            }
            let mut prefix = vec![];
            let mut suffix = vec![];
            if self.pointer_statement(&s, &mut objects, &mut prefix)
                .map_err(|e| crate::source_diagnostics::at(e, offset))? {
                out.extend(prefix);
                continue;
            }
            self.heap_component_assignment(&mut s,&mut objects,&mut prefix,&mut suffix).map_err(|e|crate::source_diagnostics::at(e,offset))?;
            // Reuse component-array views on the left of assignment too. The
            // alias is only a name for existing storage, not a new variable.
            let original = match &s {
                Stmt::Assign {name,..} | Stmt::AssignIndex {name,..} => Some(name.clone()),
                _ => None,
            };
            if let Some(original) = original {
                let mut view=Expr::Ident(original.clone());
                if self.scalar_component_view(&mut view,&mut objects,&mut prefix)
                    .map_err(|e|crate::source_diagnostics::at(e,offset))? {
                    let Expr::Ident(alias)=view else {unreachable!()};
                    match &mut s {
                        Stmt::Assign {name,..} => *name=alias,
                        Stmt::AssignIndex {name,indices,..} => {
                            for index in indices {
                                // Parser-bound sections can name the complete
                                // component path or an earlier part reference.
                                retarget(index,&original,&alias);
                                for (end,_) in original.match_indices('%') {
                                    retarget(index,&original[..end],&alias);
                                }
                            }
                            *name=alias;
                        }
                        _ => unreachable!(),
                    }
                }
            }

            (|| -> Result<()> {
                match &mut s {
                    Stmt::ArrayDecl{dims,..} => {
                        for dim in dims {self.expression(dim,&mut objects,&mut prefix)?;}
                    }
                    Stmt::DerivedDecl {
                        type_name,
                        names,
                        dims,
                        attributes,
                        polymorphic,
                    } if !dims.is_empty() => {
                        if *polymorphic {
                            bail!("polymorphic derived arrays are not yet supported");
                        }
                        if attributes
                            .iter()
                            .any(|a| matches!(a.as_str(), "save" | "parameter"))
                        {
                            bail!(
                                "SAVED and PARAMETER derived arrays are not yet supported"
                            );
                        }
                        if attributes.iter().any(|a| a == "pointer") {
                            self.pointer_fields(type_name)?;
                        }
                        if attributes.iter().any(|a|a=="intent(out)") && self.fields(type_name)?.iter().any(|f|f.heap.is_some()) {
                            bail!("INTENT(OUT) arrays of records with allocatable components require entry finalization, which is not yet supported");
                        }
                        for e in dims.iter_mut() {
                            self.expression(e, &mut objects, &mut prefix)?;
                        }
                        // Default initialization of fixed storage; ALLOCATE
                        // applies the defaults to allocated storage.
                        let dummies = self.dummies.clone();
                        let initialize = |n: &String| !dummies.contains(n) && !attributes.iter()
                            .any(|a| matches!(a.as_str(), "allocatable" | "pointer") || a.starts_with("intent("));
                        let mut defaults = vec![];
                        for n in names {
                            let initialize = initialize(n);
                            for f in self.fields(type_name)? {
                                let field = storage(n, &f.name);
                                prefix.push(Stmt::ArrayDecl {
                                    kind: f.kind.clone(),
                                    name: field.clone(),
                                    dims: storage_dims(&f, dims),
                                });
                                if f.heap.is_some() && !dummies.contains(n) && !attributes.iter().any(|a|a.starts_with("intent(")) {prefix.push(Stmt::DummyIntent {name:field.clone(),intent:"@record_slots".into()});}
                                if let (true, Some(value)) = (initialize, &f.default) {
                                    defaults.extend(self.default_fill(n, &f, value, dims.len())?);
                                }
                                for intent in attributes.iter().filter(|a| {
                                    matches!(
                                        a.as_str(),
                                        "allocatable" | "optional" | "pointer" | "target"
                                    ) || a.starts_with("intent(")
                                }) {
                                    prefix.push(Stmt::DummyIntent {
                                        name: field.clone(),
                                        intent: intent.clone(),
                                    });
                                }
                            }
                        }
                        prefix.extend(defaults);
                        return Ok(());
                    }
                    Stmt::Assign { name, value } | Stmt::AssignIndex { name, value, .. }
                        if objects.get(name).is_some_and(|o| !o.dims.is_empty()) =>
                    {
                        let name = name.clone();
                        let o = objects[&name].clone();
                        if self.fields(&o.ty)?.iter().any(|f|f.heap.is_some()) {
                            bail!("whole-array assignment of records with allocatable components is not yet supported; assign scalar elements instead");
                        }
                        let (values, array) =
                            self.parts(value.clone(), &o.ty, &mut objects, &mut prefix)?;
                        let mut indices = match &s {
                            Stmt::AssignIndex { indices, .. } => Some(indices.clone()),
                            _ => None,
                        };
                        if let Some(indices) = &mut indices {
                            self.capture_indices(indices, &mut prefix);
                        }
                        let fields = self.fields(&o.ty)?;
                        let mut captured = vec![];
                        for (f, e) in fields.iter().zip(values) {
                            if array {
                                if let Expr::Call(tag, args) = &e {
                                    if tag == "@array_constructor" {
                                        self.serial += 1;
                                        let temp = format!("@record_array_value_{}", self.serial);
                                        prefix.push(Stmt::ArrayDecl {
                                            kind: f.kind.clone(),
                                            name: temp.clone(),
                                            dims: vec![Expr::IntLit(args.len().to_string())],
                                        });
                                        if !args.is_empty() {
                                            prefix.push(Stmt::Assign {
                                                name: temp.clone(),
                                                value: e,
                                            });
                                        }
                                        captured.push(Expr::Ident(temp));
                                        continue;
                                    }
                                }
                                captured.push(e);
                            } else {
                                self.serial += 1;
                                let temp = format!("@record_value_{}", self.serial);
                                if !f.dims.is_empty() && indices.is_none() {
                                    bail!("assigning one value of type `{}` to every element of `{name}` is not yet supported", o.ty);
                                }
                                prefix.push(if f.dims.is_empty() {
                                    Stmt::VarDecl { kind: f.kind.clone(), names: vec![temp.clone()] }
                                } else {
                                    Stmt::ArrayDecl { kind: f.kind.clone(), name: temp.clone(), dims: f.dims.clone() }
                                });
                                prefix.push(Stmt::Assign {
                                    name: temp.clone(),
                                    value: e,
                                });
                                captured.push(Expr::Ident(temp));
                            }
                        }
                        for (f, value) in fields.into_iter().zip(captured) {
                            let target = storage(&name, &f.name);
                            if let Some(indices) = &indices {
                                // Bind triplets to the primitive storage and
                                // include leading axes of array components.
                                prefix.push(assignment(select(&name, &f, vec![], indices)?, value));
                            } else {
                                prefix.push(Stmt::Assign {name:target,value});
                            }
                        }
                        return Ok(());
                    }
                    Stmt::CallSub { name, args }
                        if name == "@allocate" || name == "@deallocate" =>
                    {
                        for actual in args.iter_mut() {self.heap_component_actual(actual,&mut objects,&mut prefix,Some(&mut suffix))?;}
                        let mut flat = vec![];
                        let mut defaults = vec![];
                        for mut e in std::mem::take(args) {
                            let (n, dims) = match &e {
                                Expr::Call(n, d) => (n, Some(d)),
                                Expr::Ident(n) => (n, None),
                                _ => {
                                    self.expression(&mut e, &mut objects, &mut prefix)?;
                                    flat.push(e);
                                    continue;
                                }
                            };
                            if let Some(o) = objects.get(n).filter(|o| !o.dims.is_empty()).cloned()
                            {
                                let mut bounds = dims.cloned();
                                if let Some(d) = &mut bounds {
                                    for e in d.iter_mut() {
                                        self.expression(e, &mut objects, &mut prefix)?;
                                    }
                                    self.capture_indices(d, &mut prefix);
                                }
                                for f in self.fields(&o.ty)? {
                                    let field = storage(n, &f.name);
                                    if name=="@deallocate" && f.heap.is_some() {
                                        prefix.push(Stmt::CallSub{name:"@heap_record_drop_array".into(),args:vec![Expr::Ident(field.clone()),Expr::Call("size".into(),vec![Expr::Ident(field.clone())])]});
                                    }
                                    flat.push(if let Some(d) = &bounds {
                                        Expr::Call(field.clone(), f.dims.iter().cloned().chain(d.iter().cloned()).collect())
                                    } else {
                                        Expr::Ident(field.clone())
                                    });
                                    if name == "@allocate" {
                                        if let Some(value) = &f.default {
                                            defaults.extend(self.default_fill(n, &f, value, o.dims.len())?);
                                        }
                                    }
                                }
                            } else {
                                if let Expr::Call(_,bounds)=&mut e {
                                    for bound in bounds {self.expression(bound,&mut objects,&mut prefix)?;}
                                }
                                flat.push(e);
                            }
                        }
                        *args = flat;
                        prefix.push(s.clone());
                        prefix.extend(defaults);
                        return Ok(());
                    }
                    Stmt::CallSub { name, args }
                        if matches!(name.as_str(), "@write" | "@formatted_print" | "@unformatted_read" | "@unformatted_write") =>
                    {
                        // Unformatted items follow the unit, control, status and position.
                        let first = match name.as_str() { "@write" => 2, "@formatted_print" => 1, _ => 4 };
                        let mut values = args.split_off(first.min(args.len()));
                        self.output(&mut values, &mut objects, &mut prefix)?;
                        args.extend(values);
                    }
                    Stmt::CallSub { name, args } => {
                        let prepared = name.strip_prefix(PREPARED_CALL).map(str::to_owned);
                        if let Some(original) = &prepared { *name = original.clone(); }
                        if !name.starts_with("@heap_record_") {
                            for actual in args.iter_mut() {self.heap_component_actual(actual,&mut objects,&mut prefix,Some(&mut suffix))?;}
                        }
                        if prepared.is_none() {
                            for actual in args.iter_mut() {
                                self.expression(actual, &mut objects, &mut prefix)?;
                            }
                        }
                        self.elemental_actuals(name, args, &mut objects, &mut prefix)?;
                        // A type-bound call on an array element, `call a(i)%m()`,
                        // and an element passed to a scalar derived dummy use a
                        // temporary object copied back after the call.
                        if let Some(method) = name.strip_prefix("@element_call:").map(str::to_owned) {
                            let element = args.remove(0);
                            let temp = self.element_temp(&element, &mut objects, &mut prefix, Some(&mut suffix))?;
                            *name = format!("{temp}%{method}");
                        } else if self.scalar_procedures.contains(name.as_str()) {
                            for a in args.iter_mut() {
                                if self.element(a, &objects).is_some() {
                                    let temp = self.element_temp(a, &mut objects, &mut prefix, Some(&mut suffix))?;
                                    *a = Expr::Ident(temp);
                                }
                            }
                        }
                        if self
                            .arguments(name, args, &mut objects, &mut prefix)?
                            .is_some()
                        {
                            bail!("derived array function cannot be invoked with CALL");
                        }
                    }
                    Stmt::Print { items } => self.output(items, &mut objects, &mut prefix)?,
                    Stmt::Read { args: items } => {
                        for e in items {
                            self.heap_component_actual(e,&mut objects,&mut prefix,Some(&mut suffix))?;
                            self.expression(e, &mut objects, &mut prefix)?;
                        }
                    }
                    Stmt::AssignIndex { name, value, indices } => {
                        let mut component = component_subscripts(indices);
                        for e in indices.iter_mut().chain(component.iter_mut()) {
                            self.expression(e, &mut objects, &mut prefix)?;
                        }
                        self.expression(value, &mut objects, &mut prefix)?;
                        let original = name.clone();
                        match self.component_field(name, &objects)? {
                            // An array component, or a part of one: `a(i)%x(j) = v`.
                            Some((object, f)) if !f.dims.is_empty() || !component.is_empty() => {
                                let target = select(&object, &f, component, indices)?;
                                s = assignment(target, value.clone());
                            }
                            _ if !component.is_empty() => bail!("`{original}` is not a component of an array of derived type"),
                            _ => {}
                        }
                    }
                    Stmt::Assign { name, value } => {
                        if let Some((owner,_))=self.element(value,&objects) {
                            // The scalar record pass cannot refer to flattened
                            // array elements; give it a scalar record copy.
                            if objects.get(name).is_some_and(|o|o.dims.is_empty() && o.ty==objects[owner].ty)
                                || self.types.get(&objects[owner].ty).is_some_and(|fields|fields.iter().any(|f|f.heap.is_some())) {
                                *value=Expr::Ident(self.element_temp(value,&mut objects,&mut prefix,None)?);
                            }
                        }
                        self.component(name, &objects)?;
                        self.expression(value, &mut objects, &mut prefix)?;
                    }
                    Stmt::Return(Some(e)) => self.expression(e, &mut objects, &mut prefix)?,
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                    } => {
                        self.expression(cond, &mut objects, &mut prefix)?;
                        self.body(then_body, &objects)?;
                        if let Some(b) = else_body {
                            self.body(b, &objects)?;
                        }
                    }
                    Stmt::Do {
                        start, end, body, ..
                    } => {
                        self.expression(start, &mut objects, &mut prefix)?;
                        self.expression(end, &mut objects, &mut prefix)?;
                        self.body(body, &objects)?;
                    }
                    Stmt::DoWhile { cond, body } => {
                        self.expression(cond, &mut objects, &mut prefix)?;
                        self.body(body, &objects)?;
                    }
                    Stmt::Block { body } | Stmt::Module { body, .. } => {
                        self.body(body, &objects)?
                    }
                    Stmt::Function {
                        name,
                        params,
                        body,
                        pure,
                        ..
                    }
                    | Stmt::Subroutine {
                        name,
                        params,
                        body,
                        pure,
                        ..
                    } => {
                        let old_name = name.clone();
                        let outer_dummies = std::mem::replace(&mut self.dummies, params.clone());
                        if let Some(p) = self.procedures.get(name).cloned() {
                            let mut flat = vec![];
                            for n in params.iter() {
                                if let Some(o) = p.arrays.get(n) {
                                    flat.extend(
                                        self.fields(&o.ty)?.iter().map(|f| storage(n, &f.name)),
                                    );
                                } else {
                                    flat.push(n.clone());
                                }
                            }
                            if let Some((result, o)) = &p.result {
                                for statement in body.iter_mut() {
                                    if let Stmt::DerivedDecl {
                                        names,
                                        dims,
                                        attributes,
                                        ..
                                    } = statement
                                    {
                                        if names.contains(result) {
                                            *dims = deferred(o.dims.len());
                                            if !attributes.iter().any(|a| a == "allocatable") {
                                                attributes.push("allocatable".into());
                                            }
                                            attributes.push("intent(out)".into());
                                        }
                                    }
                                }
                                let allocatable = p.arrays[result]
                                    .dims
                                    .iter()
                                    .all(|d| matches!(d,Expr::Ident(n) if n=="@deferred_extent"));
                                if !allocatable {
                                    let pos = body
                                        .iter()
                                        .position(|s| {
                                            !matches!(
                                                s,
                                                Stmt::SourceLocation(_)
                                                    | Stmt::DerivedDecl { .. }
                                                    | Stmt::VarDecl { .. }
                                                    | Stmt::ArrayDecl { .. }
                                                    | Stmt::InitializedDecl { .. }
                                                    | Stmt::DummyIntent { .. }
                                                    | Stmt::Implicit(_)
                                                    | Stmt::Use { .. }
                                            )
                                        })
                                        .unwrap_or(body.len());
                                    body.insert(
                                        pos,
                                        Stmt::CallSub {
                                            name: "@allocate".into(),
                                            args: vec![Expr::Call(result.clone(), o.dims.clone())],
                                        },
                                    );
                                }
                                fn returns(body: &mut [Stmt], result: &str) {
                                    for s in body {
                                        match s {
                                            Stmt::Return(Some(Expr::Ident(n))) if n == result => {
                                                *s = Stmt::Return(None)
                                            }
                                            Stmt::If {
                                                then_body,
                                                else_body,
                                                ..
                                            } => {
                                                returns(then_body, result);
                                                if let Some(b) = else_body {
                                                    returns(b, result);
                                                }
                                            }
                                            Stmt::Do { body, .. }
                                            | Stmt::DoWhile { body, .. }
                                            | Stmt::Block { body } => returns(body, result),
                                            _ => {}
                                        }
                                    }
                                }
                                returns(body, result);
                                flat.extend(
                                    self.fields(&o.ty)?.iter().map(|f| storage(result, &f.name)),
                                );
                                self.body(body, &objects)?;
                                s = Stmt::Subroutine {
                                    name: format!("@record_result${old_name}"),
                                    params: flat,
                                    body: std::mem::take(body),
                                    pure: *pure,
                                    elemental: false,
                                };
                            } else {
                                *params = flat;
                                self.body(body, &objects)?;
                            }
                        } else {
                            self.body(body, &objects)?;
                        }
                        self.dummies = outer_dummies;
                    }
                    _ => {}
                }
                prefix.push(s.clone());
                Ok(())
            })()
            .map_err(|e| crate::source_diagnostics::at(e, offset))?;
            out.extend(prefix);
            out.extend(suffix);
        }
        *body = out;
        self.array_shapes = outer_shapes;
        self.target_roots = outer_targets;
        Ok(())
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    let mut lower = Lower {
        types: HashMap::new(),
        procedures: HashMap::new(),
        serial: 0,
        array_shapes: HashMap::new(),
        array_components: HashMap::new(),
        scalar_components: HashMap::new(),
        dummies: vec![],
        layouts: HashMap::new(),
        scalar_procedures: Default::default(),
        scalar_results: HashMap::new(),
        scalar_functions: Default::default(),
        target_roots: Default::default(),
        elemental_records: HashMap::new(),
        elemental_results: HashMap::new(),
        elemental_functions: Default::default(),
        bindings: HashMap::new(),
    };
    lower.prepare_elemental_results(&mut p.body);
    lower.expand_array_components(&mut p.body)?;
    lower.collect(&p.body)?;
    // Module objects have unique names after module association, so every
    // scope that may use them can inherit them.
    let mut globals = Objects::new();
    for s in &p.body {
        if let Stmt::Module { body, .. } = s {
            for s in body {
                if let Stmt::DerivedDecl { type_name, names, dims, attributes, .. } = s {
                    for n in names {
                        if attributes.iter().any(|a|a=="target") {lower.target_roots.insert(n.clone());}
                        globals.insert(n.clone(), Object { ty: type_name.clone(), dims: dims.clone() });
                        if dims.is_empty() {
                            lower.register_components(n, type_name, &mut globals);
                        }
                    }
                }
            }
        }
    }
    lower.body(&mut p.body, &globals)?;
    Ok(p)
}

// SPDX-License-Identifier: MIT
//! A proof-only view of simple records as their ordinary component arrays.
//! Actual storage and initialization masks remain owned by derived_arrays.
use super::*;

#[derive(Clone, PartialEq)]
struct Field { name: String, kind: TypeSpec, dims: Vec<Expr> }
type Fields = Vec<Field>;
const COMPONENT: &str = "@parallel_component:";
pub(super) fn module_metadata(name: &str) -> bool {
    name.strip_prefix(COMPONENT).is_some_and(|n|n.starts_with("@module$"))
}
thread_local! {
    static TYPES: std::cell::RefCell<HashMap<String, Option<Fields>>> = Default::default();
}

type Definitions = HashMap<String, Vec<(Vec<Stmt>, Vec<(String, String)>)>>;

fn fields(components: &[Stmt], bindings: &[(String, String)], definitions: &Definitions,
          resolved: &mut HashMap<String, Option<Fields>>, active: &mut HashSet<String>) -> Option<Fields> {
    if !bindings.is_empty() { return None; }
    let mut out: Fields = vec![];
    for s in components {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names { out.push(Field { name:n.clone(), kind:kind.clone(), dims:vec![] }); }
            }
            Stmt::InitializedDecl { kind, entities, attributes } if attributes.is_empty() => {
                for (n, _) in entities { out.push(Field { name:n.clone(), kind:kind.clone(), dims:vec![] }); }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                if let Some(field) = out.iter_mut().find(|f| &f.name == name) { field.dims = dims.clone(); }
                else { out.push(Field { name:name.clone(), kind:kind.clone(), dims:dims.clone() }); }
            }
            Stmt::DerivedDecl { type_name, names, dims, attributes, polymorphic: false }
                if dims.is_empty() && attributes.is_empty() => {
                let nested = resolve(type_name, definitions, resolved, active)?;
                for name in names {
                    for field in &nested {
                        out.push(Field { name:format!("{name}%{}",field.name), ..field.clone() });
                    }
                }
            }
            Stmt::SourceLocation(_) | Stmt::Accessibility { .. } => {}
            // Includes owned/pointer components, inheritance and type
            // parameters. Do not approve a safe-looking field of an unsafe type.
            _ => return None,
        }
    }
    (!out.is_empty() && out.iter().all(|f| calls::scalar(&f.kind))).then_some(out)
}

// Resolve the whole graph before admitting an owner. Unsafe or recursive
// descendants invalidate every ancestor, including otherwise safe siblings.
fn resolve(name: &str, definitions: &Definitions, resolved: &mut HashMap<String, Option<Fields>>,
           active: &mut HashSet<String>) -> Option<Fields> {
    if let Some(result) = resolved.get(name) { return result.clone(); }
    if !active.insert(name.to_owned()) { return None; }
    let result = (|| {
        let candidates = definitions.get(name)?;
        let mut result = None;
        for (components, bindings) in candidates {
            let candidate = fields(components, bindings, definitions, resolved, active)?;
            if result.as_ref().is_some_and(|old|old != &candidate) { return None; }
            result = Some(candidate);
        }
        result
    })();
    active.remove(name);
    resolved.insert(name.to_owned(), result.clone());
    result
}

pub(super) fn prepare(program: &Program) {
    fn collect(body: &[Stmt], types: &mut Definitions) {
        for s in body {
            if let Stmt::DerivedType { name, components, bindings } = s {
                // Module association supplies qualified identities. Ambiguous
                // local spellings must resolve to the same complete layout.
                types.entry(name.clone()).or_default().push((components.clone(),bindings.clone()));
            } else {
                let mut copy = s.clone();
                for b in children(&mut copy) { collect(b, types); }
            }
        }
    }
    let mut definitions = HashMap::new();
    collect(&program.body, &mut definitions);
    let mut types = HashMap::new();
    for name in definitions.keys() { resolve(name, &definitions, &mut types, &mut HashSet::new()); }
    TYPES.with(|t| *t.borrow_mut() = types);
}

pub(super) fn declarations(body: &[Stmt], scope: &mut Scope) {
    if !parallel::enabled() { return; }
    // A local declaration shadows both the owner and its inherited field view.
    let shadowed: HashSet<&str> = body.iter().flat_map(|s| {
        match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.iter().map(String::as_str).collect(),
            Stmt::ArrayDecl { name, .. } => vec![name.as_str()],
            Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.as_str()).collect(),
            _ => Vec::<&str>::new(),
        }
    }).collect();
    scope.retain(|key, _| key.strip_prefix("@parallel_dummy:").or_else(||key.strip_prefix(COMPONENT)).unwrap_or(key)
        .split_once('%').is_none_or(|(owner, _)| !shadowed.contains(owner)));
    TYPES.with(|t| {
        let types = t.borrow();
        for s in body {
            let Stmt::DerivedDecl { type_name, names, dims, polymorphic: false, .. } = s else { continue };
            if dims.is_empty() { continue; }
            let Some(Some(fields)) = types.get(type_name) else { continue };
            for n in names {
                let Some(owner) = scope.get(n).cloned() else { continue };
                let constants = scope.iter().filter_map(|(n,o)| o.constant.clone().map(|v|(n.clone(),v))).collect();
                let fixed = fields.iter().map(|f| {
                    let dims = f.dims.iter().map(|d| -> Option<Expr> {
                        let integer = |e| crate::parameters::constant_integer_with(e,&constants).ok().map(int);
                        match d {
                            Expr::Call(tag,a) if tag=="@declared_bounds" && a.len()==2 =>
                                Some(Expr::Call(tag.clone(),vec![integer(&a[0])?,integer(&a[1])?])),
                            _ => integer(d),
                        }
                    }).collect::<Option<Vec<_>>>()?;
                    Some((f,dims))
                }).collect::<Option<Vec<_>>>();
                let Some(fixed) = fixed else { continue };
                for (field, component_dims) in fixed {
                    let key = format!("{n}%{}",field.name);
                    let dims = component_dims.iter().chain(&owner.dims).cloned().collect();
                    scope.insert(key.clone(), Object { kind: field.kind.clone(), dims, derived: false, ..owner.clone() });
                    scope.insert(format!("{COMPONENT}{key}"),Object { dims:component_dims, ..owner.clone() });
                    if let Some(marker) = scope.get(&format!("@parallel_dummy:{n}")).cloned() {
                        scope.insert(format!("@parallel_dummy:{key}"), marker);
                    }
                }
            }
        }
    });
}

fn expression(e: &mut Expr, scope: &Scope) {
    visit_expr(e, &mut |e| {
        let Expr::Call(tag, args) = e else { return };
        let Some(field) = tag.strip_prefix("@record_component:") else { return };
        let Some(Expr::Call(owner, indices) | Expr::Index(owner, indices)) = args.first() else { return };
        let name = format!("{owner}%{field}");
        if scope.get(&name).is_some_and(|o| !o.derived && !o.dims.is_empty()) {
            let mut indices = indices.clone();
            if args.len()==2 {
                let Expr::Call(tag, component) = &args[1] else { return };
                if tag!="@component_subscripts" { return; }
                indices.push(Expr::Call(tag.clone(),component.clone()));
            } else if args.len()!=1 { return; }
            if let Some(indices) = selection(&name, &indices, scope) { *e = Expr::Call(name, indices); }
        }
    });
}

pub(super) fn proof(body: &mut [Stmt], scope: &Scope) {
    for s in body {
        if let Stmt::AssignIndex { name, indices, .. } = s {
            if let Some(selected) = selection(name, indices, scope) { *indices = selected; }
        }
        for e in expressions(s) { expression(e, scope); }
        for b in children(s) { proof(b, scope); }
    }
}

pub(super) fn mask(mask: &mut Expr, scope: &Scope) { expression(mask, scope); }

pub(super) fn component_rank(name: &str, scope: &Scope) -> usize {
    scope.get(&format!("{COMPONENT}{name}")).map_or(0,|o|o.dims.len())
}

// Match derived_arrays::select: component axes precede the record-owner axes.
fn selection(name: &str, indices: &[Expr], scope: &Scope) -> Option<Vec<Expr>> {
    let metadata = scope.get(&format!("{COMPONENT}{name}"))?;
    let mut owner = indices.to_vec();
    let mut component = match owner.last() {
        Some(Expr::Call(tag,_)) if tag=="@component_subscripts" => {
            let Expr::Call(_,a) = owner.pop()? else { unreachable!() }; a
        }
        _ => metadata.dims.iter().map(|_|Expr::Section {name:String::new(),lower:None,upper:None,stride:None}).collect(),
    };
    if component.len()!=metadata.dims.len() || owner.len()!=metadata_owner_rank(name,scope)? {return None;}
    for e in &mut component {
        match e {
            Expr::Section{name,..} => name.clear(),
            Expr::Call(tag,a) if tag=="@section_subscript" => {
                if let [Expr::Section{name,..}] = a.as_mut_slice() {name.clear();}
            }
            _ => {}
        }
    }
    component.extend(owner);
    Some(component)
}

fn metadata_owner_rank(name: &str, scope: &Scope) -> Option<usize> {
    scope.get(name)?.dims.len().checked_sub(component_rank(name,scope))
}

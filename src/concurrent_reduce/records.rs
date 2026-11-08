// SPDX-License-Identifier: MIT
//! Primitive-field metadata for record ownership proofs and scalar locality.
//! Shared storage stays in derived_arrays; private fields use intrinsic locality.
use super::*;

#[derive(Clone, PartialEq)]
struct Field { name: String, kind: TypeSpec, dims: Vec<Expr>, default: Option<Expr> }
type Fields = Vec<Field>;
const DEFAULT: &str = "@parallel_default:";
const COMPONENT: &str = "@parallel_component:";
const IDENTITY: &str = "@parallel_record_type:";
pub(super) fn module_metadata(name: &str) -> bool {
    name.strip_prefix(COMPONENT).or_else(||name.strip_prefix(DEFAULT)).or_else(||name.strip_prefix(IDENTITY)).is_some_and(|n|n.starts_with("@module$"))
}
thread_local! {
    static TYPES: std::cell::RefCell<HashMap<String, Option<Fields>>> = Default::default();
    static DEFINED_ASSIGNMENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(super) fn defined_assignment() -> bool { DEFINED_ASSIGNMENT.with(|v|v.get()) }

type Definitions = HashMap<String, Vec<(Vec<Stmt>, Vec<(String, String)>)>>;

fn fields(components: &[Stmt], bindings: &[(String, String)], definitions: &Definitions,
          resolved: &mut HashMap<String, Option<Fields>>, active: &mut HashSet<String>) -> Option<Fields> {
    if !bindings.is_empty() { return None; }
    let mut out: Fields = vec![];
    for s in components {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names { out.push(Field { name:n.clone(), kind:kind.clone(), dims:vec![], default:None }); }
            }
            Stmt::InitializedDecl { kind, entities, attributes } if attributes.is_empty() => {
                for (n, value) in entities { out.push(Field { name:n.clone(), kind:kind.clone(), dims:vec![], default:value.clone() }); }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                if let Some(field) = out.iter_mut().find(|f| &f.name == name) { field.dims = dims.clone(); }
                else { out.push(Field { name:name.clone(), kind:kind.clone(), dims:dims.clone(), default:None }); }
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

pub(super) fn active() -> bool { TYPES.with(|t|!t.borrow().is_empty()) }

pub(super) fn has_locality(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::CallSub {name,args} if matches!(name.as_str(),"@do_concurrent"|"@concurrent_reduce") =>
            args.iter().any(|e|matches!(e,Expr::Call(tag,_) if matches!(tag.as_str(),"@locality:local"|"@locality:local_init"))),
        Stmt::Module {body,..}|Stmt::Function {body,..}|Stmt::Subroutine {body,..}|Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..} => has_locality(body),
        Stmt::If {then_body,else_body,..} => has_locality(then_body)||else_body.as_ref().is_some_and(|b|has_locality(b)),
        Stmt::SelectCase {cases,default,..} => cases.iter().any(|c|has_locality(&c.body))||default.as_ref().is_some_and(|b|has_locality(b)),
        _=>false,
    })
}

pub(super) fn prepare(program: &Program) {
    fn assignment(body: &[Stmt]) -> bool {
        body.iter().any(|s| {
            if matches!(s,Stmt::OperatorInterface {operator,..} if operator=="=") {return true;}
            let mut s=s.clone();children(&mut s).into_iter().any(|b|assignment(b))
        })
    }
    DEFINED_ASSIGNMENT.with(|v|v.set(false));
    if !parallel::enabled() && !has_locality(&program.body) {
        TYPES.with(|t|t.borrow_mut().clear()); return;
    }
    DEFINED_ASSIGNMENT.with(|v|v.set(assignment(&program.body)));
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
    if !active() { return; }
    // A local declaration shadows both the owner and its inherited field view.
    let shadowed: HashSet<&str> = body.iter().flat_map(|s| {
        match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.iter().map(String::as_str).collect(),
            Stmt::ArrayDecl { name, .. } => vec![name.as_str()],
            Stmt::InitializedDecl { entities, .. } => entities.iter().map(|(n, _)| n.as_str()).collect(),
            _ => Vec::<&str>::new(),
        }
    }).collect();
    scope.retain(|key, _| !key.strip_prefix(IDENTITY).is_some_and(|n|shadowed.contains(n)) && key.strip_prefix("@parallel_dummy:").or_else(||key.strip_prefix(COMPONENT)).or_else(||key.strip_prefix(DEFAULT)).unwrap_or(key)
        .split_once('%').is_none_or(|(owner, _)| !shadowed.contains(owner)));
    TYPES.with(|t| {
        let types = t.borrow();
        for s in body {
            let Stmt::DerivedDecl { type_name, names, polymorphic: false, .. } = s else { continue };
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
                    let mut kind=f.kind.clone();
                    crate::parameters::resolve_kind(&mut kind,&constants).ok()?;
                    let default=f.default.as_ref().map(|e|
                        crate::parameters::constant_expression_with(e,&constants)).transpose().ok()?;
                    Some((f,dims,kind,default))
                }).collect::<Option<Vec<_>>>();
                let Some(fixed) = fixed else { continue };
                scope.insert(format!("{IDENTITY}{n}"),Object {constant:Some(id(type_name)),..owner.clone()});
                for (field, component_dims, kind, default) in fixed {
                    let key = format!("{n}%{}",field.name);
                    let dims = component_dims.iter().chain(&owner.dims).cloned().collect();
                    if let Some(value)=default {
                        scope.insert(format!("{DEFAULT}{key}"),Object {constant:Some(value),..owner.clone()});
                    }
                    scope.insert(key.clone(), Object { kind, dims, derived: false, ..owner.clone() });
                    scope.insert(format!("{COMPONENT}{key}"),Object { dims:component_dims, ..owner.clone() });
                    if let Some(marker) = scope.get(&format!("@parallel_dummy:{n}")).cloned() {
                        scope.insert(format!("@parallel_dummy:{key}"), marker);
                    }
                }
            }
        }
    });
}

pub(super) fn identity<'a>(name: &str, scope: &'a Scope) -> Option<&'a Expr> {
    scope.get(&format!("{IDENTITY}{name}"))?.constant.as_ref()
}

// Only complete, bounded layouts populated above can be privatized.
pub(super) fn private_fields(name: &str, scope: &Scope) -> Option<Vec<(String,Object,Option<Expr>)>> {
    let prefix=format!("{COMPONENT}{name}%");
    let mut fields=scope.keys().filter_map(|key| {
        let field=key.strip_prefix(&prefix)?;
        let key=format!("{name}%{field}");
        let object=scope.get(&key)?.clone();
        let default=scope.get(&format!("{DEFAULT}{key}")).and_then(|o|o.constant.clone());
        Some((key,object,default))
    }).collect::<Vec<_>>();
    fields.sort_by(|a,b|a.0.cmp(&b.0));
    (!fields.is_empty()).then_some(fields)
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
        // The proof audits exactly the same reads and writes as an assignment;
        // native initialization lowering additionally transports definition bits.
        if let Stmt::CallSub {name,args}=s {
            if name==crate::initialized::TRANSPORT && args.len()==2 {
                let value=args[1].clone();
                *s=match &args[0] {
                    Expr::Ident(name)=>Stmt::Assign{name:name.clone(),value},
                    Expr::Call(tag,a) if tag.starts_with("@record_component:")=>{
                        let Some(Expr::Call(owner,indices))=a.first() else {continue};
                        let mut indices=indices.clone();
                        indices.extend(a.iter().skip(1).cloned());
                        Stmt::AssignIndex{name:format!("{owner}%{}",tag.strip_prefix("@record_component:").unwrap()),indices,value}
                    },
                    Expr::Call(name,indices)|Expr::Index(name,indices)=>Stmt::AssignIndex{name:name.clone(),indices:indices.clone(),value},
                    _=>continue,
                };
            }
        }
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

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Local scalar polymorphic pointers borrow concrete TARGET storage. Tags
//! select target identities (not only types), so reassociation never copies or
//! owns the target, and two pointers may independently alias the same object.
use super::*;

pub(super) fn targets(
    pointer: &str,
    decls: &Declarations,
    parent: &str,
    defs: &Definitions,
    body: &[Stmt],
) -> Vec<(String, String)> {
    fn associations(body: &[Stmt], names: &mut HashMap<String, HashSet<String>>) {
        for s in body {
            match s {
                Stmt::CallSub { name, args } if name == "@pointer_associate" => {
                    if let [Expr::Ident(p), Expr::Ident(n)] = args.as_slice() {
                        names.entry(p.clone()).or_default().insert(n.clone());
                    }
                }
                Stmt::CallSub {name,args} if name=="@pointer_component_targets" => {
                    if let Some(Expr::Ident(p))=args.first() {
                        for target in &args[1..] {if let Expr::Ident(n)=target {
                            names.entry(p.clone()).or_default().insert(n.clone());
                        }}
                    }
                }
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    associations(then_body, names);
                    if let Some(b) = else_body {
                        associations(b, names);
                    }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => {
                    associations(body, names)
                }
                Stmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        associations(&c.body, names);
                    }
                    if let Some(b) = default {
                        associations(b, names);
                    }
                }
                _ => {}
            }
        }
    }
    let mut edges = HashMap::new();
    associations(body, &mut edges);
    // Follow pointer-to-pointer assignments transitively; a target that only
    // another unrelated pointer can select must not generate a value read.
    let mut referenced = HashSet::new();
    let mut pending = vec![pointer.to_owned()];
    while let Some(n) = pending.pop() {
        if referenced.insert(n.clone()) {
            pending.extend(edges.get(&n).into_iter().flatten().cloned());
        }
    }
    let mut targets: Vec<_> = decls
        .iter()
        .filter(|(n, _)| referenced.contains(*n))
        .filter_map(|(n, ds)| {
            let has = |attribute: &str| {
                ds.iter().any(|s| match s {
                    Stmt::DerivedDecl { attributes, .. } => {
                        attributes.iter().any(|a| a == attribute)
                    }
                    Stmt::DummyIntent { intent, .. } => intent == attribute,
                    _ => false,
                })
            };
            ds.iter().find_map(|s| match s {
                Stmt::DerivedDecl {
                    type_name,
                    polymorphic: false,
                    dims,
                    ..
                } if dims.is_empty()
                    && has("target")
                    && !has("pointer")
                    && !has("allocatable")
                    && defs.distance(type_name, parent).is_some() =>
                {
                    Some((type_name.clone(), n.clone()))
                }
                _ => None,
            })
        })
        .collect();
    // Static storage occupies the same prefix in every borrowed environment.
    // Do not let the spelling of a generated local TARGET change its identity
    // when that local is renamed to a hidden dummy at a procedure boundary.
    targets.sort_by(|(_, a), (_, b)| {
        (!a.starts_with("@module$"), a).cmp(&(!b.starts_with("@module$"), b))
    });
    targets
}
fn null(e: &Expr) -> bool {
    matches!(e,Expr::Call(n,a) if n=="null" && a.is_empty())
}
fn eq(n: &str, k: usize) -> Expr {
    Expr::Bin(BinOp::Eq, Box::new(id(n)), Box::new(int(k)))
}
impl Lower<'_> {
    pub(super) fn pointer_statement(
        &self,
        s: &Stmt,
        vars: &Runtime,
        decls: &Declarations,
    ) -> Result<Option<Vec<Stmt>>> {
        let Stmt::CallSub { name, args } = s else {
            return Ok(None);
        };
        if name=="@pointer_component_targets" {return Ok(Some(vec![]));}
        if matches!(name.as_str(),"@pointer_component_load"|"@pointer_component_store") {
            let [Expr::Ident(p),tag]=args.as_slice() else {bail!("invalid pointer component descriptor transfer");};
            let object=vars.get(p).filter(|o|o.pointer).context("missing pointer component view")?;
            return Ok(Some(vec![if name=="@pointer_component_load" {
                Stmt::Assign{name:object.tag.clone(),value:tag.clone()}
            } else {pointer_components::assign(tag,id(&object.tag))?}]));
        }
        if name == "@pointer_associate" {
            let [Expr::Ident(p), target] = args.as_slice() else {
                return Ok(None);
            };
            let Some(object) = vars.get(p).filter(|o| o.pointer) else {
                return Ok(None);
            };
            if null(target) {
                return Ok(Some(vec![set(&object.tag, 0)]));
            }
            let Expr::Ident(target) = target else {
                bail!("polymorphic POINTER association currently requires a scalar TARGET variable or local polymorphic pointer");
            };
            if let Some(other) = vars.get(target).filter(|o| o.pointer) {
                if self.defs.distance(&other.kind, &object.kind).is_none() {
                    bail!("polymorphic pointer target type must extend its declared type");
                }
                let bodies = other
                    .variants
                    .iter()
                    .map(|(_, n)| {
                        let tag = object
                            .variants
                            .iter()
                            .position(|(_, v)| v == n)
                            .map_or(0, |i| i + 1);
                        vec![set(&object.tag, tag)]
                    })
                    .collect();
                return Ok(Some(vec![cases(other, bodies, vec![set(&object.tag, 0)])]));
            }
            let Some(index) = object.variants.iter().position(|(_, n)| n == target) else {
                if !decls.contains_key(target) {
                    bail!("unknown polymorphic pointer target `{target}`");
                }
                bail!("polymorphic POINTER target `{target}` must be a compatible concrete scalar TARGET variable; allocated and component targets are not yet supported");
            };
            return Ok(Some(vec![set(&object.tag, index + 1)]));
        }
        if name == "@nullify" {
            let mut out = vec![];
            let mut rest = vec![];
            for arg in args {
                if let Expr::Ident(n) = arg {
                    if let Some(o) = vars.get(n).filter(|o| o.pointer) {
                        out.push(set(&o.tag, 0));
                        continue;
                    }
                }
                rest.push(arg.clone());
            }
            if out.is_empty() {
                return Ok(None);
            }
            if !rest.is_empty() {
                out.push(Stmt::CallSub {
                    name: name.clone(),
                    args: rest,
                });
            }
            return Ok(Some(out));
        }
        if matches!(
            name.as_str(),
            "@allocate" | "@allocate_derived_type" | "@deallocate"
        ) && vars.iter().any(|(n, o)| o.pointer && uses(s, n))
        {
            bail!("allocation and deallocation through polymorphic POINTER descriptors are not yet supported");
        }
        Ok(None)
    }
    pub(super) fn pointer_inquiries(
        &self,
        s: &mut Stmt,
        vars: &Runtime,
        decls: &Declarations,
    ) -> Result<()> {
        for e in expressions(s) {
            let mut error = None;
            visit_expr(e, &mut |e| {
                if let Err(err) = self.pointer_inquiry(e, vars, decls) {
                    error = Some(err);
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
        }
        Ok(())
    }
    fn pointer_inquiry(&self, e: &mut Expr, vars: &Runtime, decls: &Declarations) -> Result<()> {
        let Expr::Call(n, args) = e else {
            return Ok(());
        };
        if n != "associated" {
            return Ok(());
        }
        let mut args = args.clone();
        crate::named_arguments::Signature {
            params: vec!["pointer".into(), "target".into()],
            optional: HashSet::from(["target".into()]),
        }
        .reorder(n, &mut args)?;
        let Some(Expr::Ident(p)) = args.first() else {
            return Ok(());
        };
        let Some(object) = vars.get(p).filter(|o| o.pointer) else {
            return Ok(());
        };
        let target = args.get(1).filter(|a| !crate::named_arguments::omitted(a));
        let value = if let Some(target) = target {
            let Expr::Ident(target) = target else {
                bail!("ASSOCIATED with a polymorphic pointer currently requires a scalar target variable");
            };
            if let Some(other) = vars.get(target).filter(|o| o.pointer) {
                let mut checks = vec![];
                for (i, (_, name)) in object.variants.iter().enumerate() {
                    if let Some(j) = other.variants.iter().position(|(_, n)| n == name) {
                        checks.push(Expr::Bin(
                            BinOp::And,
                            Box::new(eq(&object.tag, i + 1)),
                            Box::new(eq(&other.tag, j + 1)),
                        ));
                    }
                }
                checks
                    .into_iter()
                    .reduce(|a, b| Expr::Bin(BinOp::Or, Box::new(a), Box::new(b)))
                    .unwrap_or(Expr::Logical(false))
            } else {
                if !decls.contains_key(target) {
                    bail!("unknown ASSOCIATED target `{target}`");
                }
                object
                    .variants
                    .iter()
                    .position(|(_, n)| n == target)
                    .map_or(Expr::Logical(false), |i| eq(&object.tag, i + 1))
            }
        } else {
            Expr::Bin(BinOp::Ne, Box::new(id(&object.tag)), Box::new(int(0)))
        };
        *e = value;
        Ok(())
    }
}

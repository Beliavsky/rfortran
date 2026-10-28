// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Translate the language's parent-component constructor slot to flat storage.
use super::*;
use crate::ast::Expr;
use crate::procedure_arguments::{children, expressions, visit_expr};

fn names(components: &[Stmt]) -> Vec<String> {
    components
        .iter()
        .flat_map(|s| match s {
            Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } => names.clone(),
            Stmt::InitializedDecl { entities, .. } => {
                entities.iter().map(|(n, _)| n.clone()).collect()
            }
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            _ => vec![],
        })
        .collect()
}
fn absent() -> Expr {
    Expr::Call(crate::named_arguments::OMITTED.into(), vec![])
}
const VIEW: &str = "@constructor_parent_view:";

fn ancestors(name: &str, parents: &HashMap<String, String>) -> Vec<String> {
    let mut result = vec![];
    let mut next = parents.get(name);
    while let Some(parent) = next {
        result.push(parent.clone());
        next = parents.get(parent);
    }
    result
}

// Preserve the declared type of a bare parent-component designator while
// extension storage lowering erases its final selector (x%base becomes x).
pub(super) fn preserve_parent_views(body: &mut [Stmt], parents: &HashMap<String, String>) {
    for s in body {
        for e in expressions(s) {
            visit_expr(e, &mut |e| {
                let Expr::Call(n, args) = e else { return };
                let ancestors = ancestors(n, parents);
                for (i, arg) in args.iter_mut().enumerate() {
                    let value = match arg {
                        Expr::Call(n, a)
                            if a.len() == 1
                                && ancestors.iter().any(|p| {
                                    n == &format!("@keyword:{}", p.rsplit('$').next().unwrap())
                                }) =>
                        {
                            &mut a[0]
                        }
                        arg if i == 0 && crate::named_arguments::keyword(arg).is_none() => arg,
                        _ => continue,
                    };
                    let parent=ancestors.iter().find(|p|matches!(value,Expr::Ident(n) if n.rsplit_once('%').is_some_and(|(_,key)|key==p.rsplit('$').next().unwrap())));
                    if let Some(parent) = parent {
                        *value = Expr::Call(format!("{VIEW}{parent}"), vec![value.clone()]);
                    }
                }
            });
        }
        for b in children(s) {
            preserve_parent_views(b, parents);
        }
        if let Stmt::DerivedType { components, .. } = s {
            preserve_parent_views(components, parents);
        }
    }
}

struct Lower {
    types: HashMap<String, Definition>,
    parents: HashMap<String, String>,
    interfaces: crate::operators::OwnerInterfaces,
    serial: usize,
}
impl Lower {
    fn constructor(
        &mut self,
        e: &mut Expr,
        scope: &crate::operators::OwnerScope,
        declarations: &mut Vec<Stmt>,
        prefix: &mut Vec<Stmt>,
    ) -> Result<()> {
        let Expr::Call(ty, args) = e else {
            return Ok(());
        };
        if !self.parents.contains_key(ty) {
            return Ok(());
        }
        let ancestors = ancestors(ty, &self.parents);
        let fields = names(&self.types[ty].0);
        let explicit_parent = ancestors
            .iter()
            .find(|p| {
                args.iter().any(|a| {
                    crate::named_arguments::keyword(a)
                        .is_some_and(|(k, _)| k == p.rsplit('$').next().unwrap())
                })
            })
            .cloned();
        let positional_parent = if let Some(a) = args
            .first()
            .filter(|a| crate::named_arguments::keyword(a).is_none())
        {
            let kind = if let Expr::Call(n, a) = a {
                n.strip_prefix(VIEW)
                    .filter(|_| a.len() == 1)
                    .map(|p| (p.to_owned(), 0))
            } else {
                None
            };
            let kind = match kind {
                Some(k) => Some(k),
                None => self.interfaces.derived(a, scope)?,
            };
            kind.filter(|(p, r)| *r == 0 && ancestors.contains(p))
                .map(|(p, _)| p)
        } else {
            None
        };
        let Some(parent) = explicit_parent.or(positional_parent) else {
            // Fortran also permits construction using inherited fields in
            // declaration order, without an explicit parent object.
            crate::named_arguments::Signature {
                params: fields.clone(),
                optional: fields.iter().cloned().collect(),
            }
            .reorder(ty, args)?;
            return Ok(());
        };
        let inherited = names(&self.types[&parent].0);
        let own = &fields[inherited.len()..];
        let parent_key = parent.rsplit('$').next().unwrap();
        let view =
            |v: &Expr| matches!(v,Expr::Call(n,a) if n==&format!("{VIEW}{parent}") && a.len()==1);
        // Positional construction has one parent slot. Keywords may instead
        // specify inherited components individually, as the standard permits.
        let mut params = vec![parent_key.to_owned()];
        params.extend_from_slice(own);
        params.extend(inherited.clone());
        let signature = crate::named_arguments::Signature {
            optional: params.iter().cloned().collect(),
            params,
        };
        // Do not interpret excess positional arguments as inherited keywords.
        if args
            .iter()
            .take_while(|a| crate::named_arguments::keyword(a).is_none())
            .count()
            > own.len() + 1
        {
            bail!("too many positional components in constructor `{ty}`");
        }
        let slots = signature.associate(ty, args)?;
        let get = |slot: Option<usize>| {
            slot.map(|i| {
                crate::named_arguments::keyword(&args[i])
                    .map_or_else(|| args[i].clone(), |(_, v)| v.clone())
            })
        };
        let parent_value = get(slots[0]);
        let individual = slots[own.len() + 1..].iter().any(Option::is_some);
        if parent_value.is_some() && individual {
            bail!("constructor `{ty}` specifies both parent `{parent_key}` and an inherited component");
        }
        let mut flat = if let Some(mut value) = parent_value {
            let kind = if view(&value) {
                let Expr::Call(_, a) = value else {
                    unreachable!()
                };
                value = a.into_iter().next().unwrap();
                let actual = self.interfaces.derived(&value, scope)?;
                let mut ancestor = actual.as_ref().map(|(t, _)| t.as_str());
                while let Some(n) = ancestor {
                    if n == parent {
                        break;
                    }
                    ancestor = self.parents.get(n).map(String::as_str);
                }
                if ancestor.is_some() && actual.is_some_and(|(_, r)| r == 0) {
                    Some((parent.clone(), 0))
                } else {
                    None
                }
            } else {
                self.interfaces.derived(&value, scope)?
            };
            if kind.as_ref() != Some(&(parent.clone(), 0)) {
                bail!("parent component `{parent_key}` of constructor `{ty}` requires a scalar of type `{parent}`");
            }
            match value {
                Expr::Call(n, mut a) if n == parent => {
                    crate::named_arguments::Signature {
                        params: inherited.clone(),
                        optional: inherited.iter().cloned().collect(),
                    }
                    .reorder(&parent, &mut a)?;
                    a.resize_with(inherited.len(), absent);
                    a
                }
                Expr::Ident(n) => inherited
                    .iter()
                    .map(|f| Expr::Ident(format!("{n}%{f}")))
                    .collect(),
                value => {
                    self.serial += 1;
                    let temp = format!("@constructor_parent{}", self.serial);
                    declarations.push(Stmt::DerivedDecl {
                        type_name: parent,
                        names: vec![temp.clone()],
                        attributes: vec![],
                        polymorphic: false,
                        dims: vec![],
                    });
                    prefix.push(Stmt::CallSub {
                        name: crate::operators::INTRINSIC_ASSIGNMENT.into(),
                        args: vec![Expr::Ident(temp.clone()), value],
                    });
                    inherited
                        .iter()
                        .map(|f| Expr::Ident(format!("{temp}%{f}")))
                        .collect()
                }
            }
        } else {
            slots[own.len() + 1..]
                .iter()
                .map(|s| get(*s).unwrap_or_else(absent))
                .collect()
        };
        flat.extend(
            slots[1..own.len() + 1]
                .iter()
                .map(|s| get(*s).unwrap_or_else(absent)),
        );
        *args = flat;
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, origin: &str) -> Result<()> {
        let scope = self.interfaces.scope(body, origin);
        let mut out = vec![];
        let mut declarations = vec![];
        let mut location = None;
        for mut s in std::mem::take(body) {
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) {
                location = Some(p);
            }
            let mut prefix = vec![];
            let loop_condition = matches!(&s, Stmt::DoWhile { .. });
            let mut constructor_condition = false;
            for e in expressions(&mut s) {
                let mut result = Ok(());
                visit_expr(e, &mut |e| {
                    if loop_condition && matches!(e,Expr::Call(n,_) if self.parents.contains_key(n))
                    {
                        constructor_condition = true;
                    }
                    if result.is_ok() {
                        result = self.constructor(e, &scope, &mut declarations, &mut prefix);
                    }
                });
                result.map_err(|e| crate::source_diagnostics::at(e, location))?;
            }
            match &mut s {
                Stmt::Module { name, body }
                | Stmt::Function { name, body, .. }
                | Stmt::Subroutine { name, body, .. } => self.body(body, name)?,
                Stmt::DerivedType { components, .. } => self.body(components, origin)?,
                _ => {
                    for b in children(&mut s) {
                        self.body(b, origin)?;
                    }
                }
            }
            // A condition's parent expression must run on every loop test,
            // including the test after CYCLE, rather than once before the DO.
            if let Stmt::DoWhile { cond, body } = &mut s {
                if !prefix.is_empty() || constructor_condition {
                    prefix.push(Stmt::If {
                        cond: Expr::not(cond.clone()),
                        then_body: vec![Stmt::Exit],
                        else_body: None,
                    });
                    prefix.append(body);
                    *body = std::mem::take(&mut prefix);
                    *cond = Expr::Logical(true);
                }
            }
            out.extend(prefix);
            out.push(s);
        }
        declarations.extend(out);
        *body = declarations;
        Ok(())
    }
}

pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut types = HashMap::new();
    let mut unused = HashMap::new();
    collect(&mut program.body, &mut types, &mut unused)?;
    let parents: HashMap<_, _> = types
        .iter()
        .filter_map(|(n, (c, _))| {
            c.iter().find_map(|s| match s {
                Stmt::DummyIntent { name, intent } if name == EXTENDS => {
                    Some((n.clone(), intent.clone()))
                }
                _ => None,
            })
        })
        .collect();
    if parents.is_empty() {
        return Ok(program);
    }
    fn needed(body: &mut [Stmt], parents: &HashMap<String, String>) -> bool {
        for s in body {
            let mut found = false;
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if matches!(e,Expr::Call(n,_) if parents.contains_key(n)) {
                        found = true;
                    }
                });
            }
            if found || children(s).into_iter().any(|b| needed(b, parents)) {
                return true;
            }
            if let Stmt::DerivedType { components, .. } = s {
                if needed(components, parents) {
                    return true;
                }
            }
        }
        false
    }
    if !needed(&mut program.body, &parents) {
        return Ok(program);
    }
    let interfaces =
        crate::operators::OwnerInterfaces::new(&program, HashSet::new(), parents.clone())?;
    let mut lower = Lower {
        types,
        parents,
        interfaces,
        serial: 0,
    };
    lower.body(&mut program.body, "")?;
    Ok(program)
}

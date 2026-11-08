// SPDX-License-Identifier: MIT
//! Intrinsic assignment of bounded records involving a scalar locality entity.
use super::*;

fn owner(e: &Expr, scope: &Scope) -> Option<(String, Vec<Expr>)> {
    // Repeated selections must not re-evaluate calls or vector subscripts.
    fn scalar(e: &Expr, scope: &Scope) -> bool {
        match e {
            Expr::Ident(n) => scope.get(n).is_none_or(|o| o.dims.is_empty() && !o.derived),
            Expr::IntLit(_) => true,
            Expr::Bin(_, a, b) => scalar(a, scope) && scalar(b, scope),
            Expr::Un(_, a) => scalar(a, scope),
            _ => false,
        }
    }
    let (name, indices) = match e {
        Expr::Ident(n) => (n, vec![]),
        Expr::Call(n, a) | Expr::Index(n, a) => (n, a.clone()),
        _ => return None,
    };
    let o = scope.get(name)?;
    if !o.derived || o.dims.len() != indices.len() || !indices.iter().all(|e| scalar(e, scope)) {
        return None;
    }
    Some((name.clone(), indices))
}

fn selected(owner: &str, field: &str, parent: &[Expr], component: &[Expr]) -> Expr {
    if parent.is_empty() {
        let name = format!("{owner}%{field}");
        if component.is_empty() {
            id(&name)
        } else {
            Expr::Call(name, component.to_vec())
        }
    } else {
        let mut args = vec![Expr::Call(owner.into(), parent.to_vec())];
        if !component.is_empty() {
            args.push(Expr::Call(
                "@component_subscripts".into(),
                component.to_vec(),
            ));
        }
        Expr::Call(format!("@record_component:{field}"), args)
    }
}

pub(super) fn expand(body: &mut Vec<Stmt>, private: &HashSet<String>, scope: &Scope) -> Result<()> {
    let mut out = vec![];
    for mut s in std::mem::take(body) {
        let assignment = match &s {
            Stmt::Assign { name, value } => Some((id(name), value.clone())),
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => Some((Expr::Call(name.clone(), indices.clone()), value.clone())),
            _ => None,
        };
        if let Some((target, source)) = assignment {
            if let (Some((to, ti)), Some((from, si))) =
                (owner(&target, scope), owner(&source, scope))
            {
                if private.contains(&to) || private.contains(&from) {
                    if records::defined_assignment() {
                        bail!("whole-record locality assignment in a source group containing defined assignment is not yet supported");
                    }
                    if records::identity(&to, scope).is_none()
                        || records::identity(&to, scope) != records::identity(&from, scope)
                    {
                        bail!("whole-record locality assignment requires the same supported declared type");
                    }
                    // Re-evaluating an index which the copy changes would violate
                    // RHS-before-LHS semantics. Leave this case explicitly bounded.
                    let mut unstable = false;
                    for index in &si {
                        visit_expr(&mut index.clone(), &mut |e| {
                            if matches!(e,Expr::Ident(n) if n==&to || n.starts_with(&format!("{to}%")))
                            {
                                unstable = true;
                            }
                        });
                    }
                    if unstable {
                        bail!("whole-record locality assignment with a destination-dependent source subscript is not yet supported");
                    }
                    let fields = records::private_fields(&to, scope)
                        .context("whole-record locality assignment requires a bounded layout")?;
                    let mut count = 0usize;
                    for (path, object, _) in fields {
                        let field = path.strip_prefix(&format!("{to}%")).unwrap();
                        let rank = records::component_rank(&path, scope);
                        let dims = &scope[&path].dims[..rank];
                        let mut tuples = vec![vec![]];
                        for d in dims {
                            let (lo, hi) = match d {
                                Expr::Call(tag, a) if tag == "@declared_bounds" && a.len() == 2 => {
                                    (&a[0], &a[1])
                                }
                                _ => (&int(1), d),
                            };
                            let integer =
                                |e| crate::parameters::constant_integer_with(e, &HashMap::new());
                            let lo = integer(lo)?;
                            let hi = integer(hi)?;
                            let extent = (i128::from(hi) - i128::from(lo) + 1).max(0);
                            if extent * tuples.len() as i128 > 4096 {
                                bail!("whole-record locality assignment exceeds 4096 components");
                            }
                            tuples = tuples
                                .into_iter()
                                .flat_map(|t| {
                                    (lo..=hi).map(move |i| {
                                        let mut t = t.clone();
                                        t.push(int(i));
                                        t
                                    })
                                })
                                .collect();
                        }
                        count += tuples.len()
                            * if matches!(object.kind, TypeSpec::Complex(_)) {
                                2
                            } else {
                                1
                            };
                        if count > 4096 {
                            bail!("whole-record locality assignment exceeds 4096 components");
                        }
                        for component in tuples {
                            let destination = selected(&to, field, &ti, &component);
                            out.push(Stmt::CallSub {
                                name: crate::initialized::TRANSPORT.into(),
                                args: vec![destination, selected(&from, field, &si, &component)],
                            });
                        }
                    }
                    continue;
                }
            }
        }
        for b in children(&mut s) {
            expand(b, private, scope)?;
        }
        out.push(s);
    }
    *body = out;
    Ok(())
}

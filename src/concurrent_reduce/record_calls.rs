// SPDX-License-Identifier: MIT
//! Field-based helpers for bounded private record actuals. Public ABIs stay intact.
use super::*;

#[derive(Clone)]
struct Record {
    identity: Expr,
    fields: Vec<(String, Object, Option<Expr>)>,
    intent: String,
}
#[derive(Clone)]
struct Plan {
    helper: String,
    signature: crate::named_arguments::Signature,
    records: HashMap<String, Record>,
    outputs: HashSet<String>,
}

fn argument(e: &Expr) -> &Expr {
    crate::named_arguments::keyword(e).map_or(e, |(_, e)| e)
}

fn associate(
    name: &mut String,
    args: &mut Vec<Expr>,
    plans: &HashMap<String, Plan>,
    private: &HashSet<String>,
    scope: &Scope,
    used: &mut HashSet<String>,
) -> Result<()> {
    let Some(plan) = plans.get(name) else {
        return Ok(());
    };
    if !args
        .iter()
        .any(|e| matches!(argument(e),Expr::Ident(n) if private.contains(n)))
    {
        return Ok(());
    }
    let slots = plan.signature.associate(name, args)?;
    let mut actuals = vec![];
    for (p, slot) in plan.signature.params.iter().zip(slots) {
        let value = slot
            .map(|i| argument(&args[i]).clone())
            .context("optional arguments in a private-record call are not yet supported")?;
        if let Some(record) = plan.records.get(p) {
            let Expr::Ident(n) = &value else {
                bail!("private-record calls require scalar record variables");
            };
            if !private.contains(n) || scope.get(n).is_none_or(|o| !o.dims.is_empty()) {
                bail!("record arguments to a private-record helper must be LOCAL or LOCAL_INIT scalar records");
            }
            if records::identity(n, scope) != Some(&record.identity) {
                bail!("private-record argument `{p}` requires the same declared type as its dummy");
            }
        }
        actuals.push(value);
    }
    // Reject overlap before field expansion, including a record alongside one
    // of its components. Do not let flattening make unsafe associations disappear.
    for (i, p) in plan.signature.params.iter().enumerate() {
        if !plan.outputs.contains(p) {
            continue;
        }
        let root = calls::designator(&actuals[i])
            .context("private-record output requires variable storage")?;
        for (j, other) in actuals.iter().enumerate() {
            if i == j {
                continue;
            }
            let mut aliases = false;
            visit_expr(&mut other.clone(), &mut |e| {
                if matches!(e,Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)|Expr::Section{name:n,..}
                    if n==root || n.starts_with(&format!("{root}%")) || root.starts_with(&format!("{n}%")))
                {
                    aliases = true;
                }
            });
            if aliases {
                bail!("private-record actual arguments may alias writable storage `{root}`");
            }
        }
    }
    let mut flattened = vec![];
    for (p, value) in plan.signature.params.iter().zip(actuals) {
        if let Some(record) = plan.records.get(p) {
            let Expr::Ident(n) = value else {
                unreachable!()
            };
            for (field, _, _) in &record.fields {
                flattened.push(id(&format!(
                    "{n}%{}",
                    field.strip_prefix(&format!("{p}%")).unwrap()
                )));
            }
        } else {
            flattened.push(value);
        }
    }
    *name = plan.helper.clone();
    *args = flattened;
    used.insert(name.clone());
    Ok(())
}

fn rewrite(
    body: &mut [Stmt],
    plans: &HashMap<String, Plan>,
    private: &HashSet<String>,
    scope: &Scope,
    used: &mut HashSet<String>,
) -> Result<()> {
    let mut offset = None;
    for s in body {
        if let Stmt::SourceLocation(p) = s {
            offset = Some(*p);
            continue;
        }
        let result = (|| -> Result<()> {
            if let Stmt::CallSub { name, args } = s {
                associate(name, args, plans, private, scope, used)?;
            }
            for e in expressions(s) {
                let mut error = None;
                visit_expr(e, &mut |e| {
                    if error.is_none() {
                        if let Expr::Call(name, args) = e {
                            error = associate(name, args, plans, private, scope, used).err();
                        }
                    }
                });
                if let Some(e) = error {
                    return Err(e);
                }
            }
            for b in children(s) {
                rewrite(b, plans, private, scope, used)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, offset))?;
    }
    Ok(())
}

pub(super) fn prepare(program: &mut Program) -> Result<()> {
    if !records::active() || !records::has_locality(&program.body) {
        return Ok(());
    }
    fn collect(
        body: &[Stmt],
        parent: &Scope,
        definitions: &mut HashMap<String, Vec<(Stmt, Scope)>>,
    ) {
        let scope = declarations(body, parent);
        for s in body {
            if let Stmt::Subroutine { name, body, .. } | Stmt::Function { name, body, .. } = s {
                definitions
                    .entry(name.clone())
                    .or_default()
                    .push((s.clone(), declarations(body, &scope)));
                collect(body, &scope, definitions);
            } else if let Stmt::Module { body, .. } = s {
                collect(body, &scope, definitions);
            }
        }
    }
    let mut definitions = HashMap::new();
    collect(&program.body, &module_objects(program), &mut definitions);
    let mut plans = HashMap::new();
    let mut names = definitions.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let defs = &definitions[&name];
        if defs.len() != 1 {
            continue;
        }
        let (s, scope) = &defs[0];
        let (params, body, pure) = match s {
            Stmt::Subroutine {
                params, body, pure, ..
            }
            | Stmt::Function {
                params, body, pure, ..
            } => (params, body, *pure),
            _ => unreachable!(),
        };
        if !pure {
            continue;
        }
        let mut record_params = HashMap::new();
        let mut valid = true;
        for p in params {
            let Some(o) = scope.get(p) else {
                valid = false;
                break;
            };
            if !o.derived {
                continue;
            }
            let intent = o
                .attrs
                .iter()
                .find(|a| matches!(a.as_str(), "intent(in)" | "intent(out)" | "intent(inout)"));
            let fields = records::private_fields(p, scope);
            let identity = records::identity(p, scope);
            if !o.dims.is_empty()
                || intent.is_none()
                || fields.is_none()
                || identity.is_none()
                || o.attrs
                    .iter()
                    .any(|a| !matches!(a.as_str(), "intent(in)" | "intent(out)" | "intent(inout)"))
            {
                valid = false;
                break;
            }
            record_params.insert(
                p.clone(),
                Record {
                    identity: identity.unwrap().clone(),
                    fields: fields.unwrap(),
                    intent: intent.unwrap().clone(),
                },
            );
        }
        if valid && !record_params.is_empty() {
            let helper = format!("@parallel_record_call_{}", plans.len());
            let outputs = params
                .iter()
                .filter(|p| {
                    scope.get(*p).is_some_and(|o| {
                        o.attrs
                            .iter()
                            .any(|a| matches!(a.as_str(), "intent(out)" | "intent(inout)"))
                    })
                })
                .cloned()
                .collect();
            plans.insert(
                name,
                Plan {
                    helper,
                    signature: crate::named_arguments::Signature::new(params, body),
                    records: record_params,
                    outputs,
                },
            );
        }
    }
    if plans.is_empty() {
        return Ok(());
    }
    let mut helpers = HashMap::new();
    for (original, plan) in &plans {
        let (mut helper, scope) = definitions[original][0].clone();
        let function = matches!(&helper, Stmt::Function { .. });
        let (name, params, body) = match &mut helper {
            Stmt::Subroutine {
                name, params, body, ..
            }
            | Stmt::Function {
                name, params, body, ..
            } => (name, params, body),
            _ => unreachable!(),
        };
        let private = plan.records.keys().cloned().collect();
        let candidate = (|| -> Result<()> {
            if body
                .iter()
                .any(|s| matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }))
            {
                bail!("private-record helpers with internal procedures are not yet supported");
            }
            rewrite(body, &plans, &private, &scope, &mut HashSet::new())?;
            record_assignment::expand(body, &private, &scope)?;
            let mut prefix = vec![];
            prefix.push(Stmt::DummyIntent {
                name: "@procedure".into(),
                intent: if function {
                    parallel::WORKSPACE
                } else {
                    parallel::SUBROUTINE
                }
                .into(),
            });
            let mut flattened = vec![];
            for p in params.iter() {
                let Some(record) = plan.records.get(p) else {
                    flattened.push(p.clone());
                    continue;
                };
                for (i, (field, o, default)) in record.fields.iter().enumerate() {
                    let leaf = format!("{p}$parallel_record_field{i}");
                    prefix.push(if o.dims.is_empty() {
                        Stmt::VarDecl {
                            kind: o.kind.clone(),
                            names: vec![leaf.clone()],
                        }
                    } else {
                        Stmt::ArrayDecl {
                            kind: o.kind.clone(),
                            name: leaf.clone(),
                            dims: o.dims.clone(),
                        }
                    });
                    prefix.push(Stmt::DummyIntent {
                        name: leaf.clone(),
                        intent: record.intent.clone(),
                    });
                    if record.intent == "intent(in)" {
                        prefix.push(Stmt::DummyIntent {
                            name: leaf.clone(),
                            intent: crate::optional::BORROW_INPUT.into(),
                        });
                    }
                    if record.intent == "intent(out)" {
                        if let Some(value) = default {
                            prefix.push(set(&leaf, value.clone()));
                        }
                    }
                    crate::block_scopes::rename(body, field, &leaf);
                    flattened.push(leaf);
                }
            }
            body.retain_mut(|s| match s {
                Stmt::DerivedDecl { names, .. } => {
                    names.retain(|n| !private.contains(n));
                    !names.is_empty()
                }
                Stmt::DummyIntent { name, .. } => !private.contains(name),
                _ => true,
            });
            // A remaining whole-record reference must not access the public ABI.
            for p in &private {
                if record_locality::references(body, p) {
                    bail!("unsupported whole-record use in private-record procedure");
                }
            }
            prefix.append(body);
            *body = prefix;
            *params = flattened;
            let local = declarations(body, &Scope::new());
            for n in captures::constant_inputs(body, &scope)
                .into_iter()
                .filter(|n| !local.contains_key(n))
            {
                let o = &scope[&n];
                body.insert(
                    0,
                    Stmt::InitializedDecl {
                        kind: o.kind.clone(),
                        attributes: vec!["parameter".into()],
                        entities: vec![(n, o.constant.clone())],
                    },
                );
            }
            if function
                && matches!(body.last(),Some(Stmt::Return(Some(Expr::Ident(n)))) if n==original)
            {
                crate::procedure_arguments::rename(body, original, &plan.helper);
            }
            *name = plan.helper.clone();
            Ok(())
        })();
        if candidate.is_ok() {
            helpers.insert(plan.helper.clone(), helper);
        }
    }
    plans.retain(|_, p| helpers.contains_key(&p.helper));
    // Rewrite only constructs with private records, leaving ordinary calls and
    // original procedures untouched. Capture preparation and the existing PURE
    // audits run afterwards; unproved helpers keep ordinary serial execution.
    fn loops(
        body: &mut [Stmt],
        parent: &Scope,
        plans: &HashMap<String, Plan>,
        used: &mut HashSet<String>,
    ) -> Result<()> {
        let scope = declarations(body, parent);
        for s in body {
            if let Stmt::Block { body } = s {
                if let Some(Stmt::CallSub { name, args }) = body.first() {
                    if matches!(name.as_str(), "@do_concurrent" | "@concurrent_reduce") {
                        let private = args
                            .iter()
                            .filter_map(|e| match e {
                                Expr::Call(tag, a)
                                    if matches!(
                                        tag.as_str(),
                                        "@locality:local" | "@locality:local_init"
                                    ) =>
                                {
                                    Some(a)
                                }
                                _ => None,
                            })
                            .flatten()
                            .filter_map(|e| match e {
                                Expr::Ident(n) if scope.get(n).is_some_and(|o| o.derived) => {
                                    Some(n.clone())
                                }
                                _ => None,
                            })
                            .collect();
                        rewrite(&mut body[1..], plans, &private, &scope, used)?;
                    }
                }
            }
            for b in children(s) {
                loops(b, &scope, plans, used)?;
            }
        }
        Ok(())
    }
    let objects = module_objects(program);
    let mut used = HashSet::new();
    loops(&mut program.body, &objects, &plans, &mut used)?;
    let mut pending = used.iter().cloned().collect::<Vec<_>>();
    while let Some(n) = pending.pop() {
        let Some(mut helper) = helpers.remove(&n) else {
            continue;
        };
        fn targets(s: &mut Stmt, out: &mut Vec<String>) {
            if let Stmt::CallSub { name, .. } = s {
                out.push(name.clone());
            }
            for e in expressions(s) {
                visit_expr(e, &mut |e| {
                    if let Expr::Call(n, _) = e {
                        out.push(n.clone())
                    }
                });
            }
            for b in children(s) {
                for s in b {
                    targets(s, out);
                }
            }
        }
        let mut next = vec![];
        targets(&mut helper, &mut next);
        for n in next {
            if used.insert(n.clone()) {
                pending.push(n);
            }
        }
        program.body.push(helper);
    }
    Ok(())
}

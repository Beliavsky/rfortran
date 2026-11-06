// SPDX-License-Identifier: MIT
//! Source-side contract for calls outlined into native concurrent workers.
use super::*;
thread_local! {static FUNCTIONS: std::cell::RefCell<HashMap<String, std::result::Result<crate::named_arguments::Signature,String>>> = std::cell::RefCell::new(HashMap::new());}
thread_local! {static WORKSPACES: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());}

pub(super) fn scalar(kind: &TypeSpec) -> bool {
    matches!(
        kind,
        TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
            | TypeSpec::IntegerKind(_)
            | TypeSpec::Real
            | TypeSpec::RealKind(_)
            | TypeSpec::DoublePrecision
            | TypeSpec::Logical
            | TypeSpec::LogicalKind(_)
    )
}
#[derive(Clone)]
struct Function {
    result: String,
    params: Vec<String>,
    body: Vec<Stmt>,
    scope: Scope,
    locals: HashSet<String>,
    pure: bool,
}
pub(super) fn prepare(program: &Program) {
    fn collect(body: &[Stmt], parent: &Scope, out: &mut HashMap<String, Vec<Function>>) {
        let scope = declarations(body, parent);
        for stmt in body {
            match stmt {
                Stmt::Function {
                    name,
                    params,
                    body,
                    pure,
                    ..
                } => {
                    let constants = scope
                        .iter()
                        .filter(|(_, o)| o.attrs.contains("parameter"))
                        .map(|(n, o)| (n.clone(), o.clone()))
                        .collect();
                    out.entry(name.clone()).or_default().push(Function {
                        result: match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n.clone(),
                            _ => name.clone(),
                        },
                        params: params.clone(),
                        body: body.clone(),
                        scope: declarations(body, &constants),
                        locals: declarations(body, &Scope::new()).into_keys().collect(),
                        pure: *pure,
                    });
                    collect(body, &scope, out);
                }
                Stmt::Module { body, .. } | Stmt::Subroutine { body, .. } => {
                    collect(body, &scope, out)
                }
                _ => {}
            }
        }
    }
    let mut functions = HashMap::new();
    collect(&program.body, &module_constants(program), &mut functions);
    let mut approved = HashMap::new();
    // A fixed point accepts call chains and leaves recursive cycles unproved.
    loop {
        let mut changed = false;
        for (name, definitions) in &functions {
            if approved.contains_key(name) || definitions.len() != 1 {
                continue;
            }
            let f = &definitions[0];
            if validate(name, f, &approved).is_ok() {
                approved.insert(
                    name.clone(),
                    crate::named_arguments::Signature::new(&f.params, &f.body),
                );
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // A scalar wrapper around a workspace function needs specialization too:
    // otherwise its native body would acquire arrays after the early proof.
    let mut workspaces = functions
        .iter()
        .filter(|(n, defs)| {
            approved.contains_key(*n)
                && defs[0]
                    .body
                    .iter()
                    .any(|s| matches!(s, Stmt::ArrayDecl { .. }))
        })
        .map(|(n, _)| n.clone())
        .collect::<HashSet<_>>();
    if !workspaces.is_empty() {
        let mut callers: HashMap<String, Vec<String>> = HashMap::new();
        for (n, defs) in functions.iter().filter(|(n, _)| approved.contains_key(*n)) {
            fn callees(body: &[Stmt], targets: &mut HashSet<String>) {
                for mut s in body.to_vec() {
                    for e in expressions(&mut s) {
                        visit_expr(e, &mut |e| {
                            if let Expr::Call(n, _) = e {
                                targets.insert(n.clone());
                            }
                        });
                    }
                    for body in children(&mut s) {
                        callees(body, targets);
                    }
                }
            }
            let mut targets = HashSet::new();
            callees(&defs[0].body, &mut targets);
            for target in targets {
                if approved.contains_key(&target) {
                    callers.entry(target).or_default().push(n.clone());
                }
            }
        }
        let mut pending = workspaces.iter().cloned().collect::<Vec<_>>();
        while let Some(callee) = pending.pop() {
            for caller in callers.get(&callee).into_iter().flatten() {
                if workspaces.insert(caller.clone()) {
                    pending.push(caller.clone());
                }
            }
        }
    }
    WORKSPACES.with(|w| *w.borrow_mut() = workspaces);
    let results = functions
        .into_iter()
        .map(|(name, defs)| {
            let value = if let Some(s) = approved.get(&name) {
                Ok(s.clone())
            } else if defs.len() != 1 {
                Err("ambiguous procedure name".into())
            } else {
                Err(validate(&name, &defs[0], &approved)
                    .unwrap_err()
                    .to_string())
            };
            (name, value)
        })
        .collect();
    FUNCTIONS.with(|f| *f.borrow_mut() = results);
}
fn validate(
    name: &str,
    f: &Function,
    approved: &HashMap<String, crate::named_arguments::Signature>,
) -> Result<()> {
    if !f.pure {
        bail!("function is not PURE or SIMPLE");
    }
    if parallel::intrinsic_name(name) {
        bail!("shadowed intrinsic requires serial execution");
    }
    let result = f.result.as_str();
    let object = f
        .scope
        .get(result)
        .context("result requires an explicit declaration")?;
    if object.derived || !scalar(&object.kind) {
        bail!("requires a numeric or LOGICAL result");
    }
    for (n, o) in f.scope.iter().filter(|(n, _)| f.locals.contains(*n)) {
        let array = !o.dims.is_empty() && (!f.params.contains(n) || o.attrs.contains("intent(in)"));
        if o.derived || (!o.dims.is_empty() && !array) || !scalar(&o.kind) {
            bail!("non-scalar or unsupported storage `{n}`");
        }
        if o.attrs
            .iter()
            .any(|a| !matches!(a.as_str(), "parameter" | "in" | "intent(in)" | "value")
                && !(n == result && !o.dims.is_empty() && a == "allocatable"))
        {
            bail!("unsupported or saved storage `{n}`");
        }
    }
    for p in &f.params {
        let o = f
            .scope
            .get(p)
            .context("dummy argument requires an explicit scalar declaration")?;
        if !o
            .attrs
            .iter()
            .any(|a| matches!(a.as_str(), "in" | "intent(in)" | "value"))
        {
            bail!("dummy `{p}` requires INTENT(IN) or VALUE");
        }
    }
    fn expr(
        e: &Expr,
        f: &Function,
        approved: &HashMap<String, crate::named_arguments::Signature>,
    ) -> Result<()> {
        match e {
            Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) => {}
            Expr::Ident(n)
                if f.scope
                    .get(n)
                    .is_some_and(|o| !o.derived && scalar(&o.kind)) => {}
            Expr::Index(n, args) | Expr::Call(n, args)
                if f.scope.get(n).is_some_and(|o| !o.dims.is_empty()) =>
            {
                if args.len() != f.scope[n].dims.len() {
                    bail!("array subscripts must match the dummy rank");
                }
                for a in args {
                    expr(a, f, approved)?;
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if !name.is_empty() {
                    expr(&Expr::Ident(name.clone()), f, approved)?;
                }
                for e in [lower, upper, stride].into_iter().flatten() {
                    expr(e, f, approved)?;
                }
            }
            Expr::Call(n, args)
                if n == "@array_constructor" || matches!(n.as_str(), "@section_subscript" | "@vector_subscript")
                    && matches!(args.as_slice(), [Expr::Section { .. }]) =>
            {
                for arg in args { expr(arg, f, approved)?; }
            }
            Expr::Bin(_, a, b) => {
                expr(a, f, approved)?;
                expr(b, f, approved)?;
            }
            Expr::Un(_, a) => expr(a, f, approved)?,
            Expr::Call(n, args) => {
                if let Some(signature) = approved.get(n) {
                    signature.associate(n, args)?;
                } else if !(parallel::intrinsic(n) || array_intrinsic(n)
                    || n == "allocated" && parallel::unshadowed(n)
                        && matches!(args.as_slice(), [arg] if matches!(
                            crate::named_arguments::keyword(arg).map(|(_,v)|v).unwrap_or(arg),
                            Expr::Ident(r) if r==&f.result))) || f.scope.contains_key(n)
                {
                    bail!("callee `{n}` is not a proven PURE function");
                }
                if n == "allocated" && parallel::unshadowed(n) {
                    crate::named_arguments::intrinsic(n).unwrap().associate(n,args)?;
                }
                for a in args {
                    expr(
                        crate::named_arguments::keyword(a)
                            .map(|(_, v)| v)
                            .unwrap_or(a),
                        f,
                        approved,
                    )?;
                }
            }
            Expr::Ident(n) => bail!("nonlocal reference `{n}`"),
            _ => bail!("unsupported expression in PURE function"),
        }
        Ok(())
    }
    fn statements(
        body: &[Stmt],
        f: &Function,
        approved: &HashMap<String, crate::named_arguments::Signature>,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::SourceLocation(_)
                | Stmt::Use { .. }
                | Stmt::Implicit(_)
                | Stmt::VarDecl { .. }
                | Stmt::Return(_)
                | Stmt::If { .. }
                | Stmt::DoWhile { .. }
                | Stmt::SelectCase { .. }
                | Stmt::Block { .. }
                | Stmt::Exit
                | Stmt::Cycle => {}
                Stmt::ArrayDecl { dims, .. } => {
                    fn dimension(
                        e: &Expr,
                        f: &Function,
                        approved: &HashMap<String, crate::named_arguments::Signature>,
                    ) -> Result<()> {
                        match e {
                            Expr::Ident(n) if n == "@deferred_extent" => Ok(()),
                            Expr::Call(n, a) if n == "@declared_bounds" => {
                                for e in a {
                                    dimension(e, f, approved)?;
                                }
                                Ok(())
                            }
                            _ => expr(e, f, approved),
                        }
                    }
                    for d in dims {
                        dimension(d, f, approved)?;
                    }
                    continue;
                }
                Stmt::DummyIntent { name, intent }
                    if matches!(intent.as_str(), "@simple" | "@module_host")
                        || intent == parallel::WORKSPACE
                        || name == &f.result && intent == "allocatable"
                        || matches!(
                            intent.as_str(),
                            "parameter" | "in" | "intent(in)" | "value"
                        ) => {}
                Stmt::InitializedDecl { attributes, .. }
                    if attributes.iter().any(|a| a == "parameter") => {}
                Stmt::CallSub {name,args} if name == "@allocate" || name == "@deallocate" => {
                    // Only the result is owned by this call. Other allocation
                    // targets and control specifiers need a separate proof.
                    let owned=f.scope.get(&f.result).is_some_and(|o|
                        !o.dims.is_empty() && o.attrs.contains("allocatable"));
                    if !owned || args.len()!=1 || !match (&args[0],name.as_str()) {
                        (Expr::Call(n,_),"@allocate") | (Expr::Ident(n),"@deallocate") => n==&f.result,
                        (Expr::Section{name:n,..},"@allocate") => n==&f.result && f.scope[n].dims.len()==1,
                        _ => false,
                    } {bail!("allocation requires a private ALLOCATABLE function result");}
                }
                Stmt::Assign { name, .. }
                | Stmt::AssignIndex { name, .. }
                | Stmt::Do { var: name, .. } => {
                    let o = f
                        .scope
                        .get(name)
                        .context("assignment or loop index uses nonlocal storage")?;
                    if (f.params.contains(name) && !o.attrs.contains("value"))
                        || o.attrs.contains("parameter")
                    {
                        bail!("assignment to dummy argument or named constant `{name}`");
                    }
                }
                _ => bail!("unsupported statement or saved storage in PURE function"),
            }
            let mut s = s.clone();
            for e in expressions(&mut s) {
                expr(e, f, approved)?;
            }
            for b in children(&mut s) {
                statements(b, f, approved)?;
            }
        }
        Ok(())
    }
    statements(&f.body, f, approved)
}
pub(super) fn array_intrinsic(n: &str) -> bool {
    parallel::unshadowed(n)
        && matches!(
            n,
            "sum"
                | "product"
                | "minval"
                | "maxval"
                | "count"
                | "any"
                | "all"
                | "dot_product"
                | "size"
                | "lbound"
                | "ubound"
        )
}
pub(super) fn check(name: &str, args: &[Expr]) -> Result<bool> {
    FUNCTIONS.with(|f| {
        let functions = f.borrow();
        let Some(result) = functions.get(name) else {
            return Ok(false);
        };
        let signature = result
            .as_ref()
            .map_err(|e| anyhow::anyhow!("PURE call `{name}` uses serial execution: {e}"))?;
        signature.associate(name, args)?;
        Ok(true)
    })
}
pub(super) fn workspace(name: &str) -> bool {
    WORKSPACES.with(|w| w.borrow().contains(name))
}

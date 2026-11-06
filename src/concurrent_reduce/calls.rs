// SPDX-License-Identifier: MIT
//! Source-side contract for calls outlined into native concurrent workers.
use super::*;
thread_local! {static FUNCTIONS: std::cell::RefCell<HashMap<String, std::result::Result<crate::named_arguments::Signature,String>>> = std::cell::RefCell::new(HashMap::new());}

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
    let result = f
        .body
        .last()
        .and_then(|s| {
            if let Stmt::Return(Some(Expr::Ident(n))) = s {
                Some(n.as_str())
            } else {
                None
            }
        })
        .unwrap_or(name);
    let object = f
        .scope
        .get(result)
        .context("scalar result requires an explicit declaration")?;
    if !object.dims.is_empty() || object.derived || !scalar(&object.kind) {
        bail!("requires a numeric or LOGICAL scalar result");
    }
    for (n, o) in f.scope.iter().filter(|(n, _)| f.locals.contains(*n)) {
        if o.derived || !o.dims.is_empty() || !scalar(&o.kind) {
            bail!("non-scalar or unsupported storage `{n}`");
        }
        if o.attrs
            .iter()
            .any(|a| !matches!(a.as_str(), "parameter" | "in" | "intent(in)" | "value"))
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
                    .is_some_and(|o| !o.derived && o.dims.is_empty() && scalar(&o.kind)) => {}
            Expr::Bin(_, a, b) => {
                expr(a, f, approved)?;
                expr(b, f, approved)?;
            }
            Expr::Un(_, a) => expr(a, f, approved)?,
            Expr::Call(n, args) => {
                if let Some(signature) = approved.get(n) {
                    signature.associate(n, args)?;
                } else if !parallel::intrinsic(n) || f.scope.contains_key(n) {
                    bail!("callee `{n}` is not a proven scalar PURE function");
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
                Stmt::DummyIntent { intent, .. }
                    if matches!(intent.as_str(), "@simple" | "@module_host")
                        || matches!(
                            intent.as_str(),
                            "parameter" | "in" | "intent(in)" | "value"
                        ) => {}
                Stmt::InitializedDecl { attributes, .. }
                    if attributes.iter().any(|a| a == "parameter") => {}
                Stmt::Assign { name, .. } | Stmt::Do { var: name, .. } => {
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
pub(super) fn check(name: &str, args: &[Expr], scope: &Scope) -> Result<bool> {
    FUNCTIONS.with(|f|{
        let functions=f.borrow();
        let Some(result)=functions.get(name) else {return Ok(false);};
        let signature=result.as_ref().map_err(|e|anyhow::anyhow!("PURE call `{name}` uses serial execution: {e}"))?;
        signature.associate(name,args)?;
        for a in args {
            let mut a=crate::named_arguments::keyword(a).map(|(_,v)|v).unwrap_or(a).clone();
            let mut array=false;
            visit_expr(&mut a,&mut |e|{
                array|=matches!(e,Expr::Section{..}) || matches!(e,Expr::Ident(n) if scope.get(n).is_some_and(|o|!o.dims.is_empty()));
            });
            if array {bail!("PURE call `{name}` currently requires scalar actual arguments");}
        }
        Ok(true)
    })
}

// SPDX-License-Identifier: MIT
//! Recheck native scalar callees after all source specialization and lowering.
use super::*;
thread_local! {static FUNCTIONS: std::cell::RefCell<HashMap<String,std::result::Result<(),String>>> = std::cell::RefCell::new(HashMap::new());}
fn scalar(kind: &TypeSpec) -> bool {
    matches!(
        kind,
        TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
            | TypeSpec::Real
            | TypeSpec::DoublePrecision
            | TypeSpec::Logical
    )
}
fn intrinsic(n: &str) -> bool {
    crate::intrinsics::parallel_scalar(n)
        // REAL-kind normalization is an inline demote/promote operation.
        || n == "@round_real32"
        // Strict decoding admits only implemented bit operations and kinds.
        || crate::bit_intrinsics::decode(n).is_some()
        // Do not inherit other/future model opcodes (such as IEEE_VALUE)
        // merely because they share the runtime helper.
        || crate::real_intrinsics::decode(n)
            .is_some_and(|(operation, _)| crate::intrinsics::parallel_scalar(operation))
}
pub(super) fn prepare(module: &Module) {
    fn workers(body: &[IStmt]) -> bool {
        body.iter().any(|s| match s {
            IStmt::Call(n, _) => n == "@parallel_iteration",
            IStmt::Block { body } | IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } => {
                workers(body)
            }
            IStmt::If {
                then_body,
                else_body,
                ..
            } => workers(then_body) || else_body.as_ref().is_some_and(|b| workers(b)),
            IStmt::SelectCase { cases, default, .. } => {
                cases.iter().any(|c| workers(&c.body))
                    || default.as_ref().is_some_and(|b| workers(b))
            }
            _ => false,
        })
    }
    if !module.funcs.iter().any(|f| workers(&f.body)) {
        FUNCTIONS.with(|f| f.borrow_mut().clear());
        return;
    }
    let names = module
        .funcs
        .iter()
        .map(|f| f.name.clone())
        .collect::<HashSet<_>>();
    let mut approved = HashSet::new();
    loop {
        let mut changed = false;
        for f in &module.funcs {
            if !approved.contains(&f.name) && validate(f, &names, &approved).is_ok() {
                approved.insert(f.name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let results = module
        .funcs
        .iter()
        .map(|f| {
            let result = if approved.contains(&f.name) {
                Ok(())
            } else {
                Err(validate(f, &names, &approved).unwrap_err().to_string())
            };
            (f.name.clone(), result)
        })
        .collect();
    FUNCTIONS.with(|f| *f.borrow_mut() = results);
}
fn validate(
    f: &crate::ir::Function,
    names: &HashSet<String>,
    approved: &HashSet<String>,
) -> Result<()> {
    if !f.return_type.as_ref().is_some_and(scalar) {
        bail!("requires a native numeric or LOGICAL scalar result");
    }
    let mut locals = f.params.iter().cloned().collect::<HashSet<_>>();
    locals.insert(f.name.clone());
    fn declarations(body: &[IStmt], locals: &mut HashSet<String>) -> Result<()> {
        for s in body {
            match s {
                IStmt::DeclVar(kind, n, d) => {
                    if !scalar(kind) || !d.is_empty() {
                        bail!("native non-scalar storage `{n}`");
                    }
                    locals.insert(n.clone());
                }
                IStmt::Block { body } | IStmt::Do { body, .. } | IStmt::DoWhile { body, .. } => {
                    declarations(body, locals)?
                }
                IStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    declarations(then_body, locals)?;
                    if let Some(b) = else_body {
                        declarations(b, locals)?;
                    }
                }
                IStmt::SelectCase { cases, default, .. } => {
                    for c in cases {
                        declarations(&c.body, locals)?;
                    }
                    if let Some(b) = default {
                        declarations(b, locals)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    declarations(&f.body, &mut locals)?;
    fn expr(
        e: &IExpr,
        locals: &HashSet<String>,
        names: &HashSet<String>,
        approved: &HashSet<String>,
    ) -> Result<()> {
        match e {
            IExpr::IntLit(_) | IExpr::RealLit(_) | IExpr::Logical(_) => {}
            IExpr::Ident(n) if locals.contains(n) => {}
            IExpr::Ident(n) => bail!("native nonlocal storage `{n}`"),
            IExpr::Bin(_, a, b) => {
                expr(a, locals, names, approved)?;
                expr(b, locals, names, approved)?;
            }
            IExpr::Un(_, a) => expr(a, locals, names, approved)?,
            IExpr::Call(n, args) => {
                if names.contains(n) {
                    if !approved.contains(n) {
                        bail!("native callee `{n}` has not been proved thread-safe");
                    }
                } else if !intrinsic(n) {
                    bail!("unproven native runtime helper `{n}`");
                }
                for a in args {
                    expr(a, locals, names, approved)?;
                }
            }
            _ => bail!("native array or unsupported scalar expression"),
        }
        Ok(())
    }
    fn body(
        stmts: &[IStmt],
        locals: &HashSet<String>,
        names: &HashSet<String>,
        approved: &HashSet<String>,
    ) -> Result<()> {
        let check = |e: &IExpr| expr(e, locals, names, approved);
        let target = |n: &str| {
            if locals.contains(n) {
                Ok(())
            } else {
                bail!("native write to nonlocal storage `{n}`")
            }
        };
        for s in stmts {
            match s {
                IStmt::DeclVar(..) | IStmt::Exit | IStmt::Cycle => {}
                // Dead-store elimination can remove an unused initialization
                // flag's declaration while retaining a branch-local literal
                // store. Codegen ignores such stores; they allocate no storage.
                IStmt::AssignIntLit(n, _)
                    if !locals.contains(n)
                        && n.strip_prefix("@initialized_").is_some_and(|s| {
                            !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                        }) => {}
                IStmt::AssignIntLit(n, _)
                | IStmt::AssignRealLit(n, _)
                | IStmt::AssignBool(n, _) => target(n)?,
                IStmt::AssignIdent(n, v) => {
                    target(n)?;
                    check(&IExpr::Ident(v.clone()))?;
                }
                IStmt::AssignExpr(n, e) => {
                    target(n)?;
                    check(e)?;
                }
                IStmt::Return(e) => {
                    if let Some(e) = e {
                        check(e)?;
                    }
                }
                IStmt::Block { body: b } => body(b, locals, names, approved)?,
                IStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    check(cond)?;
                    body(then_body, locals, names, approved)?;
                    if let Some(b) = else_body {
                        body(b, locals, names, approved)?;
                    }
                }
                IStmt::Do {
                    var,
                    start,
                    end,
                    body: b,
                } => {
                    target(var)?;
                    check(start)?;
                    check(end)?;
                    body(b, locals, names, approved)?;
                }
                IStmt::DoWhile { cond, body: b } => {
                    check(cond)?;
                    body(b, locals, names, approved)?;
                }
                IStmt::SelectCase {
                    selector,
                    cases,
                    default,
                } => {
                    check(selector)?;
                    for c in cases {
                        for item in &c.items {
                            match item {
                                crate::ir::IcCaseItem::Single(e) => check(e)?,
                                crate::ir::IcCaseItem::Range(a, b) => {
                                    check(a)?;
                                    check(b)?;
                                }
                                crate::ir::IcCaseItem::Multi(a) => {
                                    for e in a {
                                        check(e)?;
                                    }
                                }
                            }
                        }
                        body(&c.body, locals, names, approved)?;
                    }
                    if let Some(b) = default {
                        body(b, locals, names, approved)?;
                    }
                }
                // Generated bounds and initialization guards terminate on failure;
                // their arguments use only local values and immutable text.
                IStmt::Call(n, args)
                    if matches!(n.as_str(), "@check_initialized" | "@error_stop") =>
                {
                    for a in args {
                        if !matches!(a, IExpr::Str(_)) {
                            check(a)?;
                        }
                    }
                }
                _ => bail!("native I/O, allocation, saved storage or unsupported statement"),
            }
        }
        Ok(())
    }
    body(&f.body, &locals, names, approved)
}
pub(super) fn check(body: &[IStmt]) -> Result<()> {
    fn expr(e: &IExpr, functions: &HashMap<String, std::result::Result<(), String>>) -> Result<()> {
        match e {
            IExpr::Call(n, args) => {
                if let Some(result) = functions.get(n) {
                    result.as_ref().map_err(|reason| {
                        anyhow::anyhow!("native call `{n}` requires serial execution: {reason}")
                    })?;
                } else if (!n.starts_with('@') && !intrinsic(n))
                    || n.starts_with("@internal_")
                    || n.starts_with("@module$")
                {
                    bail!("unproven native call `{n}`");
                }
                for a in args {
                    expr(a, functions)?;
                }
            }
            IExpr::Bin(_, a, b) => {
                expr(a, functions)?;
                expr(b, functions)?;
            }
            IExpr::Un(_, a) => expr(a, functions)?,
            IExpr::Index(_, a) => {
                for e in a {
                    expr(e, functions)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn walk(
        body: &[IStmt],
        functions: &HashMap<String, std::result::Result<(), String>>,
    ) -> Result<()> {
        for s in body {
            match s {
                IStmt::AssignExpr(_, e) | IStmt::Return(Some(e)) => expr(e, functions)?,
                IStmt::AssignIndex(_, args, e) => {
                    for a in args {
                        expr(a, functions)?;
                    }
                    expr(e, functions)?;
                }
                IStmt::Block { body } => walk(body, functions)?,
                IStmt::Do {
                    start, end, body, ..
                } => {
                    expr(start, functions)?;
                    expr(end, functions)?;
                    walk(body, functions)?;
                }
                IStmt::DoWhile { cond, body } => {
                    expr(cond, functions)?;
                    walk(body, functions)?;
                }
                IStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    expr(cond, functions)?;
                    walk(then_body, functions)?;
                    if let Some(b) = else_body {
                        walk(b, functions)?;
                    }
                }
                IStmt::SelectCase {
                    selector,
                    cases,
                    default,
                } => {
                    expr(selector, functions)?;
                    for c in cases {
                        for item in &c.items {
                            match item {
                                crate::ir::IcCaseItem::Single(e) => expr(e, functions)?,
                                crate::ir::IcCaseItem::Range(a, b) => {
                                    expr(a, functions)?;
                                    expr(b, functions)?;
                                }
                                crate::ir::IcCaseItem::Multi(a) => {
                                    for e in a {
                                        expr(e, functions)?;
                                    }
                                }
                            }
                        }
                        walk(&c.body, functions)?;
                    }
                    if let Some(b) = default {
                        walk(b, functions)?;
                    }
                }
                IStmt::DeclVar(_, _, dims) => {
                    for e in dims {
                        expr(e, functions)?;
                    }
                }
                IStmt::Call(n, args) if n != "@parallel_iteration" => {
                    expr(&IExpr::Call(n.clone(), args.clone()), functions)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    FUNCTIONS.with(|f| walk(body, &f.borrow()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ident(n: &str) -> IExpr {
        IExpr::Ident(n.into())
    }
    fn call(n: &str) -> IExpr {
        IExpr::Call(n.into(), vec![ident("x")])
    }
    #[test]
    fn native_pure_call_contract_rechecks_callees_and_storage() {
        let function = |name: &str, body| crate::ir::Function {
            name: name.into(),
            params: vec!["x".into()],
            return_type: Some(TypeSpec::Integer(None)),
            body,
        };
        let declaration = IStmt::DeclVar(TypeSpec::Integer(None), "x".into(), vec![]);
        let leaf = function(
            "leaf",
            vec![declaration.clone(), IStmt::Return(Some(ident("x")))],
        );
        let outer = function(
            "outer",
            vec![declaration.clone(), IStmt::Return(Some(call("leaf")))],
        );
        let entry = crate::ir::Function {
            name: "main".into(),
            params: vec![],
            return_type: None,
            body: vec![IStmt::Call("@parallel_iteration".into(), vec![])],
        };
        let mut module = Module {
            funcs: vec![entry, outer, leaf],
            uses_modules: vec![],
        };
        let worker = vec![IStmt::AssignExpr("y".into(), call("outer"))];
        let marker = std::mem::take(&mut module.funcs[0].body);
        module.funcs[0].body = vec![IStmt::SelectCase {
            selector: IExpr::IntLit("1".into()),
            cases: vec![],
            default: Some(marker),
        }];
        prepare(&module);
        assert!(check(&worker).is_ok());
        module.funcs[2].body = vec![
            declaration.clone(),
            IStmt::Return(Some(IExpr::Call("@bit:iand:4".into(), vec![ident("x"), IExpr::IntLit("1".into())]))),
        ];
        prepare(&module);
        assert!(check(&worker).is_ok());
        module.funcs[2].body = vec![
            declaration.clone(),
            IStmt::Return(Some(IExpr::Call("@model:exponent:8".into(), vec![ident("x")]))),
        ];
        prepare(&module);
        assert!(check(&worker).is_ok());
        for bad in [
            IStmt::Return(Some(call("@model:unknown:4"))),
            IStmt::Return(Some(call("@model:scale:16"))),
            IStmt::Return(Some(call("@model:@ieee_value:4"))),
            // Internal bit tags must identify an implemented operation/kind.
            IStmt::Return(Some(call("@bit:unknown:4"))),
            IStmt::Return(Some(call("@bit:iand:16"))),
            IStmt::Call("@module_bind".into(), vec![]),
            IStmt::Call("@saved_bind".into(), vec![]),
            IStmt::DeclVar(
                TypeSpec::Integer(None),
                "array".into(),
                vec![IExpr::IntLit("2".into())],
            ),
            IStmt::Return(Some(ident("host_variable"))),
            IStmt::Return(Some(call("unknown_external"))),
            // Language purity does not prove C runtime helpers thread-safe.
            IStmt::Return(Some(call("log_gamma"))),
            IStmt::Print(vec![ident("x")]),
        ] {
            module.funcs[2].body = vec![declaration.clone(), bad, IStmt::Return(Some(ident("x")))];
            prepare(&module);
            assert!(check(&worker).is_err(), "{:?}", module.funcs[2].body);
        }
        module.funcs[2].body = vec![
            declaration,
            IStmt::AssignIntLit("@initialized_42".into(), "1".into()),
            IStmt::Return(Some(ident("x"))),
        ];
        prepare(&module);
        assert!(check(&worker).is_ok());
        module.funcs[2]
            .body
            .push(IStmt::Return(Some(ident("@initialized_42"))));
        prepare(&module);
        assert!(check(&worker).is_err());
        assert!(check(&[IStmt::AssignExpr(
            "y".into(),
            call("unavailable_pure_external")
        )])
        .is_err());
    }
}

// SPDX-License-Identifier: MIT
//! Conservative source-level proof for the first native parallel subset.
use super::*;
thread_local! {static SHADOWED: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());}
pub(super) fn procedures(program: &Program) {
    if !OPTIONS.with(|o| o.get().0) {
        return;
    }
    fn collect(body: &[Stmt], names: &mut HashSet<String>) {
        for s in body {
            if let Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. }
            | Stmt::GenericInterface { name, .. } = s
            {
                names.insert(name.clone());
            }
            if let Stmt::DummyIntent { name, intent } = s {
                if intent != "intrinsic" {
                    names.insert(name.clone());
                }
            }
            if let Stmt::CallSub { name, args } = s {
                if name == "@external" {
                    for e in args {
                        if let Expr::Ident(n) = e {
                            names.insert(n.clone());
                        }
                    }
                }
            }
            let mut s = s.clone();
            for b in children(&mut s) {
                collect(b, names);
            }
        }
    }
    let mut names = HashSet::new();
    collect(&program.body, &mut names);
    SHADOWED.with(|s| *s.borrow_mut() = names);
}
thread_local! {static OPTIONS: std::cell::Cell<(bool,bool)> = const {std::cell::Cell::new((false,false))};}
pub(crate) fn configure(enabled: bool, report: bool) {
    OPTIONS.with(|o| o.set((enabled, report)));
}
pub(crate) fn report(message: &str) {
    if OPTIONS.with(|o| o.get().1) {
        eprintln!("DO CONCURRENT: {message}");
    }
}
pub(super) fn eligible(
    controls: &[Expr],
    mask: &Expr,
    body: &[Stmt],
    clauses: &[Expr],
    scope: &Scope,
    name: Option<&str>,
) -> bool {
    if !OPTIONS.with(|o| o.get().0) {
        return false;
    }
    let result = check(controls, mask, body, clauses, scope, name);
    match result {
        Ok(()) => true,
        Err(e) => {
            report(&format!("serial fallback: {e}"));
            false
        }
    }
}
fn check(
    controls: &[Expr],
    mask: &Expr,
    body: &[Stmt],
    clauses: &[Expr],
    scope: &Scope,
    name: Option<&str>,
) -> Result<()> {
    if scope.contains_key("@parallel_procedure") {
        bail!("procedure captures require further alias analysis");
    }
    if name.is_some() {
        bail!("named constructs are not yet supported");
    }
    if controls.len() != 4 || !matches!(&controls[3],Expr::IntLit(n) if n=="1") {
        bail!("requires one index and unit stride");
    }
    let Expr::Ident(index) = &controls[0] else {
        bail!("missing index")
    };
    let mut locals = HashSet::from([index.clone()]);
    let mut reductions = false;
    for clause in clauses {
        if let Expr::Call(tag, args) = clause {
            if tag.starts_with("@reduce:") {
                for a in args {
                    if let Expr::Ident(n) = a {
                        let o = scope
                            .get(n)
                            .context("reduction requires an explicit declaration")?;
                        if !o.dims.is_empty() {
                            bail!("parallel array REDUCE is not yet supported");
                        }
                        locals.insert(n.clone());
                        reductions = true;
                    }
                }
            }
            if matches!(tag.as_str(), "@locality:local" | "@locality:local_init") {
                for a in args {
                    if let Expr::Ident(n) = a {
                        locals.insert(n.clone());
                    }
                }
            }
        }
    }
    for n in &locals {
        if let Some(o) = scope.get(n) {
            if !o.dims.is_empty() {
                bail!("parallel local arrays are not yet supported");
            }
        }
    }
    fn writes(
        body: &[Stmt],
        locals: &HashSet<String>,
        written: &mut HashSet<String>,
        index: &str,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::SourceLocation(_)=>{},
                Stmt::Assign{name,..} if locals.contains(name)=>{},
                Stmt::AssignIndex{name,indices,..} if indices.len()==1 && matches!(&indices[0],Expr::Ident(n) if n==index)=>{written.insert(name.clone());},
                Stmt::If{then_body,else_body,..}=>{writes(then_body,locals,written,index)?;if let Some(b)=else_body {writes(b,locals,written,index)?;}},
                _=>bail!("body requires scalar local assignments, independent array-element writes, and IF statements"),
            }
        }
        Ok(())
    }
    let mut written = HashSet::new();
    writes(body, &locals, &mut written, index)?;
    if written.is_empty() && !reductions {
        bail!("no independent array output");
    }
    fn expr(e: &Expr, scope: &Scope, written: &HashSet<String>, index: &str) -> Result<()> {
        fn object(n: &str, scope: &Scope) -> Result<()> {
            let o = scope
                .get(n)
                .with_context(|| format!("capture `{n}` requires an explicit declaration"))?;
            if o.derived
                || !matches!(
                    o.kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::IntegerKind(_)
                        | TypeSpec::Real
                        | TypeSpec::RealKind(_)
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                )
            {
                bail!("capture `{n}` requires an ordinary numeric or LOGICAL type");
            }
            if matches!(&o.kind, TypeSpec::Integer(Some(k)) if *k > 8)
                || matches!(&o.kind, TypeSpec::IntegerKind(k) if !matches!(k.as_str(), "1" | "2" | "4" | "8"))
            {
                bail!("capture `{n}` requires a supported native scalar kind");
            }
            if o.attrs.iter().any(|a| {
                !matches!(a.as_str(), "parameter" | "save")
                    && !(a == "allocatable" && o.dims.len() == 1)
            }) {
                bail!("capture `{n}` has unsupported attributes");
            }
            if o.dims.len() > 1 {
                bail!("capture `{n}` requires a rank-one array");
            }
            fn constant(e: &Expr, scope: &Scope) -> bool {
                match e {
                    Expr::IntLit(_) => true,
                    Expr::Ident(n) => scope.get(n).is_some_and(|o| o.attrs.contains("parameter")),
                    Expr::Bin(_, a, b) => constant(a, scope) && constant(b, scope),
                    Expr::Un(_, a) => constant(a, scope),
                    _ => false,
                }
            }
            for d in &o.dims {
                if o.attrs.contains("allocatable")
                    && matches!(d, Expr::Ident(n) if n == "@deferred_extent")
                {
                    continue;
                }
                if !constant(d, scope) {
                    bail!("array `{n}` requires constant extents and default lower bounds");
                }
            }
            Ok(())
        }
        match e {
            Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) => Ok(()),
            Expr::Ident(n) => {
                object(n, scope)?;
                if !scope[n].dims.is_empty() {
                    bail!("whole-array expressions require serial execution");
                }
                Ok(())
            }
            Expr::Index(n, args) | Expr::Call(n, args)
                if scope.get(n).is_some_and(|o| !o.dims.is_empty()) =>
            {
                object(n, scope)?;
                if args.len() != 1 {
                    bail!("requires a scalar rank-one subscript");
                }
                if written.contains(n) && !matches!(&args[0],Expr::Ident(v) if v==index) {
                    bail!("read of written array `{n}` is not confined to the current iteration");
                }
                for a in args {
                    expr(a, scope, written, index)?;
                }
                Ok(())
            }
            Expr::Bin(_, a, b) => {
                expr(a, scope, written, index)?;
                expr(b, scope, written, index)
            }
            Expr::Un(_, a) => expr(a, scope, written, index),
            // Functions with no mutable runtime state, implemented directly by codegen.
            Expr::Call(n, args)
                if !scope.contains_key(n)
                    && !SHADOWED.with(|s| s.borrow().contains(n))
                    && matches!(
                        n.as_str(),
                        "sqrt"
                            | "abs"
                            | "int"
                            | "real"
                            | "dble"
                            | "sin"
                            | "cos"
                            | "exp"
                            | "log"
                            | "min"
                            | "max"
                    ) =>
            {
                for a in args {
                    expr(a, scope, written, index)?;
                }
                Ok(())
            }
            _ => bail!("expression or procedure call is outside the parallel subset"),
        }
    }
    for n in &written {
        expr(
            &Expr::Index(n.clone(), vec![Expr::Ident(index.clone())]),
            scope,
            &written,
            index,
        )?;
    }
    expr(mask, scope, &written, index)?;
    fn visit(body: &[Stmt], scope: &Scope, written: &HashSet<String>, index: &str) -> Result<()> {
        for s in body {
            let mut s = s.clone();
            for e in expressions(&mut s) {
                expr(e, scope, written, index)?;
            }
            for b in children(&mut s) {
                visit(b, scope, written, index)?;
            }
        }
        Ok(())
    }
    visit(body, scope, &written, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn procedure_declarations_are_not_thread_safe_intrinsics() {
        configure(true, false);

        let scope = Scope::from([
            (
                "i".into(),
                Object {
                    kind: TypeSpec::Integer(None),
                    dims: vec![],
                    attrs: HashSet::new(),
                    derived: false,
                },
            ),
            (
                "a".into(),
                Object {
                    kind: TypeSpec::Real,
                    dims: vec![int(9000)],
                    attrs: HashSet::new(),
                    derived: false,
                },
            ),
        ]);
        let body = vec![Stmt::AssignIndex {
            name: "a".into(),
            indices: vec![id("i")],
            value: Expr::Call("sin".into(), vec![Expr::RealLit("1.0".into())]),
        }];
        for declaration in [
            Stmt::DummyIntent {
                name: "sin".into(),
                intent: "external".into(),
            },
            Stmt::DummyIntent {
                name: "sin".into(),
                intent: "procedure(iface)".into(),
            },
            Stmt::GenericInterface {
                name: "sin".into(),
                procedures: vec!["my_sin".into()],
                end_name: None,
            },
        ] {
            procedures(&Program {
                name: "p".into(),
                contains: vec![],
                body: vec![declaration],
            });
            let error = check(
                &[id("i"), int(1), int(9000), int(1)],
                &Expr::Logical(true),
                &body,
                &[],
                &scope,
                None,
            )
            .unwrap_err();
            assert!(error.to_string().contains("outside the parallel subset"));
        }
        configure(false, false);
    }
}

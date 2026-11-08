// SPDX-License-Identifier: MIT
//! Give parallel PURE callees explicit read-only inputs without changing their
//! public ABI. Qualified module identities also cover promoted host variables.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone)]
struct Copy {
    name: String,
    inputs: Vec<String>,
}

pub(super) fn constant_inputs(body: &[Stmt], objects: &Scope) -> BTreeSet<String> {
    fn kind(kind: &TypeSpec, names: &mut BTreeSet<String>) {
        if let TypeSpec::IntegerKind(n)
        | TypeSpec::RealKind(n)
        | TypeSpec::LogicalKind(n)
        | TypeSpec::Complex(n) = kind
        {
            names.insert(n.clone());
        }
    }
    fn expr(e: &mut Expr, names: &mut BTreeSet<String>) {
        visit_expr(e, &mut |e| match e {
            Expr::Ident(n) | Expr::Index(n, _) => {
                names.insert(n.clone());
            }
            Expr::RealLit(s) | Expr::IntLit(s) => {
                if let Some((_, n)) = s.split_once('_') {
                    names.insert(n.into());
                }
            }
            _ => {}
        });
    }
    fn scan(body: &[Stmt], names: &mut BTreeSet<String>) {
        for s in body {
            match s {
                Stmt::VarDecl { kind: k, .. }
                | Stmt::ArrayDecl { kind: k, .. }
                | Stmt::InitializedDecl { kind: k, .. } => kind(k, names),
                _ => {}
            }
            let mut s = s.clone();
            for e in expressions(&mut s) {
                expr(e, names);
            }
            for b in children(&mut s) {
                scan(b, names);
            }
        }
    }
    let mut names = BTreeSet::new();
    scan(body, &mut names);
    let mut pending = names.iter().cloned().collect::<Vec<_>>();
    while let Some(n) = pending.pop() {
        if let Some(o) = objects.get(&n).filter(|o| o.attrs.contains("parameter")) {
            let mut extra = BTreeSet::new();
            kind(&o.kind, &mut extra);
            if let Some(mut value) = o.constant.clone() {
                expr(&mut value, &mut extra);
            }
            for n in extra {
                if names.insert(n.clone()) {
                    pending.push(n);
                }
            }
        }
    }
    names.retain(|n| {
        objects.get(n).is_some_and(|o| {
            o.attrs.contains("parameter") && o.dims.is_empty() && o.constant.is_some()
        })
    });
    names
}

pub(super) fn prepare(program: &mut Program) {
    let objects = module_objects(program);
    // Procedure names are already module-qualified. Ambiguous local names and
    // borrowed (unpromoted) host environments keep the existing serial path.
    fn collect(body: &[Stmt], out: &mut HashMap<String, Vec<Stmt>>) {
        for s in body {
            if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
                out.entry(name.clone()).or_default().push(s.clone());
            }
            let mut s = s.clone();
            for b in children(&mut s) {
                collect(b, out);
            }
        }
    }
    let mut definitions = HashMap::new();
    collect(&program.body, &mut definitions);
    let mut inputs: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, defs) in &definitions {
        if defs.len() != 1 {
            continue;
        }
        let (body, pure, elemental) = match &defs[0] {
            Stmt::Function {
                body,
                pure,
                elemental,
                ..
            }
            | Stmt::Subroutine {
                body,
                pure,
                elemental,
                ..
            } => (body, *pure, *elemental),
            _ => unreachable!(),
        };
        if !pure
            || elemental
            || body.iter().any(|s| {
                matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. })
                    || matches!(s,Stmt::DummyIntent{name,..} if name==crate::c_interop::BIND)
            })
        {
            continue;
        }
        let locals = declarations(body, &Scope::new());
        fn scan(
            body: &[Stmt],
            locals: &Scope,
            objects: &Scope,
            read: &mut BTreeSet<String>,
            calls: &mut HashSet<String>,
        ) {
            for s in body {
                if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                    continue;
                }
                if let Stmt::CallSub { name, .. } = s {
                    calls.insert(name.clone());
                }
                let mut s = s.clone();
                for e in expressions(&mut s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(n, _) = e {
                            calls.insert(n.clone());
                        }
                        if let Expr::Ident(n)
                        | Expr::Index(n, _)
                        | Expr::Call(n, _)
                        | Expr::Section { name: n, .. } = e
                        {
                            if !locals.contains_key(n)
                                && objects
                                    .get(n)
                                    .is_some_and(|o| !o.attrs.contains("parameter"))
                            {
                                read.insert(n.clone());
                            }
                        }
                    });
                }
                for b in children(&mut s) {
                    scan(b, locals, objects, read, calls);
                }
            }
        }
        let mut read = BTreeSet::new();
        let mut calls = HashSet::new();
        scan(body, &locals, &objects, &mut read, &mut calls);
        inputs.insert(name.clone(), read);
        edges.insert(name.clone(), calls);
    }
    loop {
        let previous = inputs.clone();
        let mut changed = false;
        for (name, read) in &mut inputs {
            for callee in &edges[name] {
                if let Some(extra) = previous.get(callee) {
                    for n in extra {
                        changed |= read.insert(n.clone());
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut copies = HashMap::new();
    let mut names = inputs.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let read = &inputs[&name];
        if read.is_empty()
            || read.iter().any(|n| {
                let o = &objects[n];
                o.derived
                    || matches!(o.kind, TypeSpec::Complex(_))
                    || !calls::scalar(&o.kind)
                    || o.attrs.iter().any(|a| {
                        !matches!(
                            a.as_str(),
                            "save" | "@module_initializer" | "public" | "private"
                        ) && !(a == "allocatable" && !o.dims.is_empty())
                    })
            })
        {
            continue;
        }
        copies.insert(
            name,
            Copy {
                name: format!("@parallel_capture_{}", copies.len() + 1),
                inputs: read.iter().cloned().collect(),
            },
        );
    }
    // Generate only helpers reachable from concurrent constructs. Unused PURE
    // procedures must not enlarge or otherwise affect an ordinary source unit.
    fn roots(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            if let Stmt::Block { body } = s {
                if matches!(body.first(),Some(Stmt::CallSub{name,..}) if matches!(name.as_str(),"@do_concurrent"|"@concurrent_reduce"))
                {
                    crate::procedure_arguments::invoked(body, out);
                    continue;
                }
            }
            let mut s = s.clone();
            for b in children(&mut s) {
                roots(b, out);
            }
        }
    }
    let mut reachable = HashSet::new();
    roots(&program.body, &mut reachable);
    let mut pending = reachable.iter().cloned().collect::<Vec<_>>();
    while let Some(n) = pending.pop() {
        for callee in edges.get(&n).into_iter().flatten() {
            if reachable.insert(callee.clone()) {
                pending.push(callee.clone());
            }
        }
    }
    copies.retain(|n, _| reachable.contains(n));
    if copies.is_empty() {
        return;
    }
    fn rewrite(body: &mut [Stmt], copies: &HashMap<String, Copy>, only_loops: bool) {
        for s in body {
            // DO CONCURRENT is represented by the @do_concurrent marker block.
            let active = !only_loops
                || matches!(s,Stmt::Block{body} if matches!(body.first(),Some(Stmt::CallSub{name,..}) if matches!(name.as_str(),"@do_concurrent"|"@concurrent_reduce")));
            fn call(n: &mut String, args: &mut Vec<Expr>, copies: &HashMap<String, Copy>) {
                if let Some(copy) = copies.get(n) {
                    *n = copy.name.clone();
                    args.extend(copy.inputs.iter().enumerate().map(|(i, n)| {
                        Expr::Call(format!("@keyword:@parallel_read_{i}"), vec![id(n)])
                    }));
                }
            }
            if active {
                if let Stmt::CallSub { name, args } = s {
                    call(name, args, copies);
                }
                for e in expressions(s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(n, args) = e {
                            call(n, args, copies);
                        }
                    });
                }
            }
            for b in children(s) {
                rewrite(b, copies, !active);
            }
        }
    }
    let mut helpers = Vec::new();
    let mut names = copies.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for original in names {
        let copy = &copies[&original];
        let mut helper = definitions[&original][0].clone();
        let function = matches!(&helper, Stmt::Function { .. });
        let (name, params, body) = match &mut helper {
            Stmt::Function {
                name, params, body, ..
            }
            | Stmt::Subroutine {
                name, params, body, ..
            } => (name, params, body),
            _ => unreachable!(),
        };
        *name = copy.name.clone();
        // Resolve calls before renaming captured objects, so transitive inputs
        // refer to this helper's dummy rather than to shared global storage.
        rewrite(body, &copies, false);
        let result = match body.last() {
            Some(Stmt::Return(Some(Expr::Ident(n)))) => n.as_str(),
            _ => original.as_str(),
        };
        if function && result == original {
            crate::procedure_arguments::rename(body, &original, &copy.name);
        }
        for (i, n) in copy.inputs.iter().enumerate() {
            let dummy = format!("@parallel_read_{i}");
            body.retain(|s| !matches!(s,Stmt::DummyIntent{name,intent} if name==n && intent=="@module_host"));
            crate::procedure_arguments::rename(body, n, &dummy);
            let object = &objects[n];
            let declaration = if object.dims.is_empty() {
                Stmt::VarDecl {
                    kind: object.kind.clone(),
                    names: vec![dummy.clone()],
                }
            } else {
                Stmt::ArrayDecl {
                    kind: object.kind.clone(),
                    name: dummy.clone(),
                    dims: object.dims.clone(),
                }
            };
            body.insert(
                0,
                Stmt::DummyIntent {
                    name: dummy.clone(),
                    intent: "intent(in)".into(),
                },
            );
            body.insert(
                0,
                Stmt::DummyIntent {
                    name: dummy.clone(),
                    intent: parallel::READONLY_CAPTURE.into(),
                },
            );
            if object.attrs.contains("allocatable") {
                body.insert(
                    0,
                    Stmt::DummyIntent {
                        name: dummy.clone(),
                        intent: "allocatable".into(),
                    },
                );
            }
            body.insert(0, declaration);
            params.push(dummy);
        }
        // Copy constants used by kinds and bounds into the helper's scope.
        // The ordinary parameter pass folds them before native code generation.
        for n in constant_inputs(body, &objects) {
            let o = &objects[&n];
            body.retain(|s| !matches!(s,Stmt::DummyIntent{name,intent} if name==&n && intent=="@module_host"));
            body.insert(
                0,
                Stmt::InitializedDecl {
                    kind: o.kind.clone(),
                    attributes: vec!["parameter".into()],
                    entities: vec![(n, o.constant.clone())],
                },
            );
        }
        helpers.push(helper);
    }
    // Do not alter even a serial call until the complete transformed call
    // graph is proved safe. In particular, recursive/unavailable callees must
    // retain the original interface and lazy access to their globals.
    let helper_names = copies
        .values()
        .map(|c| c.name.clone())
        .collect::<HashSet<_>>();
    program.body.extend(helpers);
    calls::prepare(program);
    copies.retain(|_, c| calls::workspace(&c.name));
    let approved = copies
        .values()
        .map(|c| c.name.clone())
        .collect::<HashSet<_>>();
    program.body.retain(|s| !matches!(s,Stmt::Function{name,..}|Stmt::Subroutine{name,..} if helper_names.contains(name) && !approved.contains(name)));
    rewrite(&mut program.body, &copies, true);
    // These are implementation inputs of an already access-checked procedure,
    // including private data and entities excluded by the caller's USE ONLY.
    // Grant visibility only for generated arguments, in their owning unit;
    // placing directives inside the concurrent body would change its proof.
    fn grant(body: &mut Vec<Stmt>) {
        let mut names = BTreeSet::new();
        fn scan(body: &mut [Stmt], names: &mut BTreeSet<String>) {
            for s in body {
                if let Stmt::Module { body, .. }
                | Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. } = s
                {
                    grant(body);
                    continue;
                }
                for e in expressions(s) {
                    visit_expr(e, &mut |e| {
                        if let Expr::Call(tag, args) = e {
                            if tag.starts_with("@keyword:@parallel_read_") {
                                if let [Expr::Ident(n)] = args.as_slice() {
                                    if n.starts_with("@module$") {
                                        names.insert(n.clone());
                                    }
                                }
                            }
                        }
                    });
                }
                for b in children(s) {
                    scan(b, names);
                }
            }
        }
        scan(body, &mut names);
        body.extend(names.into_iter().map(crate::imports::reference));
    }
    grant(&mut program.body);
}

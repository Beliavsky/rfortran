// SPDX-License-Identifier: MIT
//! Source-side contract for calls outlined into native concurrent workers.
use super::*;
thread_local! {static PROCEDURES: std::cell::RefCell<HashMap<String, std::result::Result<crate::named_arguments::Signature,String>>> = std::cell::RefCell::new(HashMap::new());}
thread_local! {static SUBROUTINES: std::cell::RefCell<HashMap<String,Vec<(bool,usize)>>> = std::cell::RefCell::new(HashMap::new());}
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
            | TypeSpec::Complex(_)
    )
}
#[derive(Clone)]
struct Procedure {
    result: String,
    subroutine: bool,
    params: Vec<String>,
    body: Vec<Stmt>,
    scope: Scope,
    locals: HashSet<String>,
    pure: bool,
}
fn owned_array(f: &Procedure, name: &str) -> bool {
    f.locals.contains(name) && !f.params.iter().any(|p| p==name)
        && f.scope.get(name).is_some_and(|o| !o.dims.is_empty()
            && !o.derived && scalar(&o.kind) && o.attrs.contains("allocatable")
            && !o.attrs.iter().any(|a|matches!(a.as_str(),"pointer"|"target"|"save")))
}
pub(super) fn prepare(program: &Program) {
    fn collect(body: &[Stmt], parent: &Scope, out: &mut HashMap<String, Vec<Procedure>>) {
        let scope = declarations(body, parent);
        for stmt in body {
            match stmt {
                Stmt::Function { name, params, body, pure, .. }
                | Stmt::Subroutine { name, params, body, pure, .. } => {
                    let subroutine=matches!(stmt,Stmt::Subroutine{..});
                    let constants = scope
                        .iter()
                        .filter(|(_, o)| o.attrs.contains("parameter"))
                        .map(|(n, o)| (n.clone(), o.clone()))
                        .collect();
                    out.entry(name.clone()).or_default().push(Procedure {
                        subroutine,
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
                Stmt::Module { body, .. } => {
                    collect(body, &scope, out)
                }
                _ => {}
            }
        }
    }
    let mut functions = HashMap::new();
    collect(&program.body, &module_constants(program), &mut functions);
    SUBROUTINES.with(|s| *s.borrow_mut()=functions.iter().filter_map(|(n,defs)| {
        let f=&defs[0];f.subroutine.then(||(n.clone(),f.params.iter().map(|p|f.scope.get(p).map_or((false,0),|o|(writable(o),o.dims.len()))).collect()))
    }).collect());
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
                    if let Stmt::CallSub{name,..}=&s {targets.insert(name.clone());}
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
    PROCEDURES.with(|f| *f.borrow_mut() = results);
}
fn validate(
    name: &str,
    f: &Procedure,
    approved: &HashMap<String, crate::named_arguments::Signature>,
) -> Result<()> {
    if !f.pure {
        bail!("procedure is not PURE or SIMPLE");
    }
    if parallel::intrinsic_name(name) {
        bail!("shadowed intrinsic requires serial execution");
    }
    if !f.subroutine {
        let result = f.result.as_str();
        let object = f
            .scope
            .get(result)
            .context("result requires an explicit declaration")?;
        if object.derived || !scalar(&object.kind) {
            bail!("requires a numeric or LOGICAL result");
        }
    }
    for (n, o) in f.scope.iter().filter(|(n, _)| f.locals.contains(*n)) {
        let array = !o.dims.is_empty() && (!f.params.contains(n) || o.attrs.contains("intent(in)") || f.subroutine && writable(o));
        if o.derived || (!o.dims.is_empty() && !array) || !scalar(&o.kind) {
            bail!("non-scalar or unsupported storage `{n}`");
        }
        if o.attrs
            .iter()
            .any(|a| !matches!(a.as_str(), "parameter" | "in" | "intent(in)" | "value")
                && !(owned_array(f,n) && a == "allocatable")
                && !(f.subroutine && f.params.contains(n) && matches!(a.as_str(),"intent(out)"|"intent(inout)")))
        {
            bail!("unsupported or saved storage `{n}`");
        }
    }
    for p in &f.params {
        let o = f
            .scope
            .get(p)
            .context("dummy argument requires an explicit scalar declaration")?;
        if f.subroutine && !o.attrs.iter().any(|a|matches!(a.as_str(),"intent(in)"|"intent(out)"|"intent(inout)")) {
            bail!("subroutine dummy `{p}` requires explicit INTENT");
        }
        if !(f.subroutine && writable(o)) && !o
            .attrs
            .iter()
            .any(|a| matches!(a.as_str(), "in" | "intent(in)" | "value"))
        {
            bail!("dummy `{p}` requires INTENT(IN) or VALUE");
        }
    }
    fn expr(
        e: &Expr,
        f: &Procedure,
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
                if let Some(signature) = approved.get(n).filter(|_|!is_subroutine(n)) {
                    signature.associate(n, args)?;
                } else if !(parallel::intrinsic(n) || array_intrinsic(n)
                    || n == "allocated" && parallel::unshadowed(n)
                        && matches!(args.as_slice(), [arg] if matches!(
                            crate::named_arguments::keyword(arg).map(|(_,v)|v).unwrap_or(arg),
                            Expr::Ident(r) if owned_array(f,r)))) || f.scope.contains_key(n)
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
            _ => bail!("unsupported expression in PURE procedure"),
        }
        Ok(())
    }
    fn statements(
        body: &[Stmt],
        f: &Procedure,
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
                        f: &Procedure,
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
                        || owned_array(f,name) && intent == "allocatable"
                        || f.subroutine && f.params.contains(name) && matches!(intent.as_str(),"intent(out)"|"intent(inout)")
                        || matches!(
                            intent.as_str(),
                            "parameter" | "in" | "intent(in)" | "value"
                        ) => {}
                Stmt::InitializedDecl { attributes, .. }
                    if attributes.iter().any(|a| a == "parameter") => {}
                Stmt::CallSub{name,args} if is_subroutine(name) => {
                    let signature=approved.get(name).with_context(||format!("callee `{name}` is not a proven PURE subroutine"))?;
                    for (actual,output) in associate_subroutine(name,args,signature,&f.scope)? {
                        if output {
                            let n=designator(&actual).context("subroutine output requires writable local storage")?;
                            let o=f.scope.get(n).context("subroutine output uses nonlocal storage")?;
                            if !f.locals.contains(n) || o.attrs.contains("parameter") || f.params.iter().any(|p|p==n) && !writable(o) {
                                bail!("subroutine output uses read-only or nonlocal storage `{n}`");
                            }
                        }
                        expr(&actual,f,approved)?;
                    }
                    continue;
                }
                Stmt::CallSub {name,args} if name == "@allocate" || name == "@deallocate" => {
                    let mut objects=vec![];
                    let mut controls=HashSet::new();
                    for arg in args {
                        if let Some((key,value))=crate::named_arguments::keyword(arg) {
                            if !controls.insert(key) {bail!("duplicate allocation control `{key}`");}
                            match key {
                                "stat" => {
                                    let Expr::Ident(n)=value else {bail!("STAT requires a local INTEGER variable");};
                                    let local=f.locals.contains(n) && !f.params.contains(n)
                                        && f.scope.get(n).is_some_and(|o|o.dims.is_empty()
                                            && matches!(o.kind,TypeSpec::Integer(_)|TypeSpec::IntegerKind(_))
                                            && o.attrs.is_empty());
                                    if !local {bail!("STAT requires a local INTEGER variable");}
                                }
                                "source" | "mold" if name=="@allocate" => expr(value,f,approved)?,
                                _ => bail!("unsupported parallel allocation control `{key}`"),
                            }
                        } else {objects.push(arg);}
                    }
                    if controls.contains("source") && controls.contains("mold") {bail!("SOURCE and MOLD are mutually exclusive");}
                    // Bounds/data are read; STAT writes only call-local scalar storage.
                    if objects.len()!=1 || !match (objects[0],name.as_str()) {
                        (Expr::Call(n,_),"@allocate") | (Expr::Ident(n),"@deallocate") => owned_array(f,n),
                        (Expr::Ident(n),"@allocate") => owned_array(f,n) && (controls.contains("source") || controls.contains("mold")),
                        (Expr::Section{name:n,..},"@allocate") => owned_array(f,n) && f.scope[n].dims.len()==1,
                        _ => false,
                    } {bail!("allocation requires one private ALLOCATABLE array");}
                    for target in objects {expr(target,f,approved)?;}
                    continue;
                }
                Stmt::Assign { name, .. }
                | Stmt::AssignIndex { name, .. }
                | Stmt::Do { var: name, .. } => {
                    let o = f
                        .scope
                        .get(name)
                        .context("assignment or loop index uses nonlocal storage")?;
                    if (f.params.contains(name) && !o.attrs.contains("value") && !(f.subroutine && writable(o)))
                        || o.attrs.contains("parameter")
                    {
                        bail!("assignment to dummy argument or named constant `{name}`");
                    }
                }
                _ => bail!("unsupported statement or saved storage in PURE procedure"),
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
                | "iall" | "iany" | "iparity" | "parity"
                | "norm2" | "minloc" | "maxloc" | "findloc"
                | "size"
                | "lbound"
                | "ubound"
        )
}
pub(super) fn check(name: &str, args: &[Expr]) -> Result<bool> {
    if is_subroutine(name) {return Ok(false);}
    PROCEDURES.with(|f| {
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

fn writable(o:&Object)->bool {
    o.attrs.iter().any(|a|matches!(a.as_str(),"intent(out)"|"intent(inout)"))
}
fn is_subroutine(name:&str)->bool {SUBROUTINES.with(|s|s.borrow().contains_key(name))}
pub(super) fn designator(e:&Expr)->Option<&str> {
    match e {Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)|Expr::Section{name:n,..}=>Some(n),_=>None}
}
fn argument_rank(e:&Expr,scope:&Scope)->Option<usize> {
    match e {
        Expr::Ident(n)=>scope.get(n).map(|o|o.dims.len()),
        Expr::IntLit(_)|Expr::RealLit(_)|Expr::Logical(_)=>Some(0),
        Expr::Section{..}=>Some(1),
        Expr::Call(n,_) if n=="@array_constructor"=>Some(1),
        Expr::Call(n,_) if matches!(n.as_str(),"@section_subscript"|"@vector_subscript")=>Some(1),
        Expr::Index(n,a)|Expr::Call(n,a) if scope.get(n).is_some_and(|o|!o.dims.is_empty())=> {
            a.iter().try_fold(0,|rank,index|Some(rank+usize::from(argument_rank(index,scope)?>0)))
        }
        Expr::Bin(_,a,b)=>Some(argument_rank(a,scope)?.max(argument_rank(b,scope)?)),
        Expr::Un(_,a)=>argument_rank(a,scope),
        _=>None,
    }
}
fn associate_subroutine(name:&str,args:&[Expr],signature:&crate::named_arguments::Signature,scope:&Scope)->Result<Vec<(Expr,bool)>> {
    let actuals=signature.associate(name,args)?.into_iter().map(|i| {
        let i=i.context("parallel subroutines require every argument")?;
        Ok(crate::named_arguments::keyword(&args[i]).map(|(_,e)|e).unwrap_or(&args[i]).clone())
    }).collect::<Result<Vec<_>>>()?;
    let outputs=SUBROUTINES.with(|s|s.borrow()[name].clone());
    let associated=actuals.into_iter().zip(outputs).map(|(actual,(output,formal_rank))| {
        // Sequence association can expose more than the scalar element named
        // at the call site. It needs a region proof beyond the tuple check.
        if argument_rank(&actual,scope).is_some_and(|rank|rank!=formal_rank) {
            bail!("parallel subroutine arguments require matching ranks; sequence association uses serial execution");
        }
        Ok((actual,output))
    }).collect::<Result<Vec<_>>>()?;
    // A writable actual cannot alias another argument, even a read-only view.
    // This deliberately rejects disjoint views of the same base array too.
    for (i,(actual,output)) in associated.iter().enumerate() {
        if !output {continue;}
        let n=designator(actual).context("subroutine output requires a variable or array section")?;
        for (j,(other,_)) in associated.iter().enumerate() {
            if i==j {continue;}
            let mut other=other.clone();let mut aliases=false;
            visit_expr(&mut other,&mut |e| {aliases |= matches!(e,Expr::Ident(m)|Expr::Index(m,_)|Expr::Call(m,_)|Expr::Section{name:m,..} if m==n);});
            if aliases {bail!("subroutine actual arguments may alias writable storage `{n}`");}
        }
    }
    Ok(associated)
}
pub(super) fn subroutine(name:&str,args:&[Expr],scope:&Scope)->Result<Vec<(Expr,bool)>> {
    if !is_subroutine(name) {bail!("subroutine `{name}` is not a proven PURE or SIMPLE procedure");}
    PROCEDURES.with(|f| {
        let functions=f.borrow();
        let signature=functions[name].as_ref().map_err(|e|anyhow::anyhow!("PURE subroutine `{name}` uses serial execution: {e}"))?;
        associate_subroutine(name,args,signature,scope)
    })
}
pub(super) fn inline_subroutine(name:&str)->bool {
    is_subroutine(name) && PROCEDURES.with(|f|f.borrow().get(name).is_some_and(|r|r.is_ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn subroutine(name:&str,statements:Vec<Stmt>)->Stmt {
        let mut body=vec![
            Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec!["x".into(),"y".into()]},
            Stmt::DummyIntent{name:"x".into(),intent:"intent(in)".into()},
            Stmt::DummyIntent{name:"y".into(),intent:"intent(out)".into()},
        ];body.extend(statements);
        Stmt::Subroutine{name:name.into(),params:vec!["x".into(),"y".into()],body,pure:true,elemental:false}
    }
    fn call(name:&str)->Stmt {Stmt::CallSub{name:name.into(),args:vec![id("x"),id("y")]}}
    fn prepare(body:Vec<Stmt>) {super::prepare(&Program{name:"p".into(),contains:vec![],body});}
    #[test]
    fn parallel_subroutine_rank_contract_rejects_sequence_association() {
        prepare(vec![Stmt::Subroutine{name:"array_writer".into(),params:vec!["a".into()],pure:true,elemental:false,
            body:vec![Stmt::ArrayDecl{kind:TypeSpec::Integer(None),name:"a".into(),dims:vec![int(2)]},
                Stmt::DummyIntent{name:"a".into(),intent:"intent(out)".into()},
                Stmt::AssignIndex{name:"a".into(),indices:vec![int(1)],value:int(1)}]}]);
        let scope=Scope::from([("data".into(),Object{kind:TypeSpec::Integer(None),dims:vec![int(20)],attrs:HashSet::new(),derived:false,constant:None})]);
        assert!(super::subroutine("array_writer",&[id("data")],&scope).is_ok());
        let element=Expr::Index("data".into(),vec![int(1)]);
        assert!(super::subroutine("array_writer",&[element],&scope).unwrap_err().to_string().contains("sequence association"));
    }
    #[test]
    fn parallel_subroutine_contract_proves_graph_and_rejects_aliases_and_effects() {
        let leaf=subroutine("leaf",vec![set("y",id("x"))]);
        prepare(vec![subroutine("outer",vec![call("leaf")]),leaf.clone()]);
        let actuals=super::subroutine("outer",&[id("a"),id("b")],&Scope::new()).unwrap();
        assert_eq!(actuals,vec![(id("a"),false),(id("b"),true)]);
        assert!(!super::check("outer",&[id("a"),id("b")]).unwrap());
        assert!(super::subroutine("outer",&[id("a"),id("a")],&Scope::new()).unwrap_err().to_string().contains("may alias"));
        for bad in [
            subroutine("bad",vec![set("y",id("host_variable"))]),
            subroutine("bad",vec![set("x",int(1))]),
            subroutine("bad",vec![call("unknown_external")]),
            subroutine("bad",vec![call("bad")]),
        ] {
            prepare(vec![bad]);
            assert!(super::subroutine("bad",&[id("a"),id("b")],&Scope::new()).is_err());
        }
        let mut missing=leaf.clone();
        if let Stmt::Subroutine{body,..}=&mut missing {body.retain(|s|!matches!(s,Stmt::DummyIntent{name,..} if name=="x"));}
        prepare(vec![missing]);
        assert!(super::subroutine("leaf",&[id("a"),id("b")],&Scope::new()).unwrap_err().to_string().contains("explicit INTENT"));
    }
}

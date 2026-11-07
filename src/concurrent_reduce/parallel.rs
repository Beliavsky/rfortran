// SPDX-License-Identifier: MIT
//! Conservative source-level proof for the first native parallel subset.
use super::*;
pub(crate) const SUBROUTINE: &str = "@parallel_subroutine";
pub(crate) const WORKSPACE: &str = "@parallel_function_workspace";
pub(crate) const READONLY_CAPTURE: &str = "@parallel_readonly_capture";
pub(super) fn mark_workspaces(program: &mut Program) {
    fn mark(body: &mut [Stmt]) {
        for s in body {
            if let Stmt::Subroutine{name,body,..}=s {
                if calls::inline_subroutine(name) && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{intent,..} if intent==SUBROUTINE)) {
                    body.insert(0,Stmt::DummyIntent{name:"@procedure".into(),intent:SUBROUTINE.into()});
                }
            }
            if let Stmt::Function { name, body, .. } = s {
                if calls::workspace(name)
                    && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{intent,..} if intent==WORKSPACE))
                {
                    // Carry the proof through internal-procedure renaming.
                    // Unproved/recursive functions keep their ordinary lowering.
                    body.insert(0, Stmt::DummyIntent {
                        name: "@procedure".into(), intent: WORKSPACE.into()
                    });
                }
            }
            for body in children(s) { mark(body); }
        }
    }
    if enabled() { mark(&mut program.body); }
}
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
    calls::prepare(program);
}
thread_local! {static OPTIONS: std::cell::Cell<(bool,bool)> = const {std::cell::Cell::new((false,false))};}
pub(super) fn enabled() -> bool {
    OPTIONS.with(|o| o.get().0)
}
pub(crate) fn configure(enabled: bool, report: bool) {
    OPTIONS.with(|o| o.set((enabled, report)));
}
pub(crate) fn report(message: &str) {
    if OPTIONS.with(|o| o.get().1) {
        eprintln!("DO CONCURRENT: {message}");
    }
}
pub(super) fn intrinsic_name(n: &str) -> bool {
    crate::intrinsics::parallel_scalar(n)
}
pub(super) fn intrinsic(n: &str) -> bool {
    intrinsic_name(n) && unshadowed(n)
}
pub(super) fn unshadowed(n: &str) -> bool {
    !SHADOWED.with(|s| s.borrow().contains(n))
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
    if name.is_some() {
        bail!("named constructs are not yet supported");
    }
    if controls.is_empty() || controls.len() % 4 != 0 {
        bail!("missing concurrent controls");
    }
    let indices: HashSet<String> = controls
        .chunks(4)
        .map(|c| match &c[0] {
            Expr::Ident(n) => Ok(n.clone()),
            _ => bail!("missing index"),
        })
        .collect::<Result<_>>()?;
    let mut locals = indices.clone();
    let mut reductions = false;
    let mut reduction_arrays = HashSet::new();
    let mut reduction_elements = 0usize;
    for clause in clauses {
        if let Expr::Call(tag, args) = clause {
            if tag.starts_with("@reduce:") {
                for a in args {
                    if let Expr::Ident(n) = a {
                        let o = scope
                            .get(n)
                            .context("reduction requires an explicit declaration")?;
                        if matches!(o.kind,TypeSpec::Complex(_))
                            && !matches!(tag.as_str(), "@reduce:+" | "@reduce:*") {
                            bail!("COMPLEX REDUCE supports only addition and multiplication");
                        }
                        if !o.dims.is_empty() {
                            let constants = scope
                                .iter()
                                .filter_map(|(n, o)| {
                                    o.constant
                                        .as_ref()
                                        .filter(|_| o.attrs.contains("parameter"))
                                        .map(|e| (n.clone(), e.clone()))
                                })
                                .collect::<HashMap<_, _>>();
                            let mut length = 1usize;
                            let mut dynamic = o.attrs.contains("allocatable");
                            for d in &o.dims {
                                let (lo, hi) = if let Expr::Call(tag, a) = d {
                                    if tag == "@declared_bounds" && a.len() == 2 {
                                        (&a[0], &a[1])
                                    } else {
                                        dynamic = true;
                                        break;
                                    }
                                } else {
                                    (&Expr::IntLit("1".into()), d)
                                };
                                let (Ok(lo), Ok(hi)) = (
                                    crate::parameters::evaluate(lo, &constants),
                                    crate::parameters::evaluate(hi, &constants),
                                ) else {
                                    dynamic = true;
                                    break;
                                };
                                let (lo, hi) = (lo as i128, hi as i128);
                                let extent = usize::try_from((hi - lo + 1).max(0))
                                    .context("parallel array REDUCE size overflow")?;
                                length = length
                                    .checked_mul(extent)
                                    .context("parallel array REDUCE size overflow")?;
                            }
                            if !dynamic && length == 0 {
                                bail!("zero-sized parallel array REDUCE uses serial execution");
                            }
                            reduction_elements = reduction_elements
                                .checked_add(if dynamic { 0 } else {
                                    length.checked_mul(if matches!(o.kind, TypeSpec::Complex(_)) { 2 } else { 1 })
                                        .context("parallel array REDUCE size overflow")?
                                })
                                .context("parallel array REDUCE size overflow")?;
                            if reduction_elements > 4096 {
                                bail!("parallel array REDUCE exceeds private-storage budget (4096 elements across arrays)");
                            }
                            reduction_arrays.insert(n.clone());
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
    let constants=scope.iter().filter_map(|(n,o)|o.constant.as_ref()
        .filter(|_|o.attrs.contains("parameter")).map(|e|(n.clone(),e.clone()))).collect();
    let mut local_elements=0usize;
    for n in &locals {
        if let Some(o) = scope.get(n) {
            if !o.dims.is_empty() && !reduction_arrays.contains(n) {
                let mut count=if matches!(o.kind,TypeSpec::Complex(_)) {2usize}else{1};
                for d in &o.dims {
                    let (lo,hi)=if let Expr::Call(tag,a)=d {
                        if tag=="@declared_bounds" && a.len()==2 {(&a[0],&a[1])}
                        else {bail!("parallel LOCAL arrays require constant explicit bounds");}
                    } else {(&Expr::IntLit("1".into()),d)};
                    let (Ok(lo),Ok(hi))=(crate::parameters::evaluate(lo,&constants),crate::parameters::evaluate(hi,&constants))
                        else {count=0;break;}; // Runtime extents are budgeted before dispatch.
                    let extent=usize::try_from((hi as i128-lo as i128+1).max(0))
                        .context("parallel LOCAL array size overflow")?;
                    if extent==0 {bail!("zero-sized parallel LOCAL arrays use serial execution");}
                    count=count.checked_mul(extent).context("parallel LOCAL array size overflow")?;
                }
                local_elements=local_elements.checked_add(count).context("parallel LOCAL array size overflow")?;
                if local_elements>4096 {
                    bail!("parallel LOCAL arrays exceed private-storage budget (4096 components across arrays)");
                }
            }
        }
    }
    // A subscript is either invariant, or one index plus/minus an invariant
    // offset. Every index must occur exactly once across the destination tuple.
    // Exclude LOCAL, LOCAL_INIT and REDUCE objects from invariant expressions.
    fn invariant(e: &Expr, locals: &HashSet<String>, scope: &Scope) -> bool {
        match e {
            Expr::IntLit(_) => true,
            Expr::Ident(n) => {
                !locals.contains(n)
                    && scope.get(n).is_some_and(|o| {
                        o.dims.is_empty()
                            && matches!(o.kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_))
                    })
            }
            Expr::Bin(BinOp::Add | BinOp::Sub, a, b) => {
                invariant(a, locals, scope) && invariant(b, locals, scope)
            }
            Expr::Un(crate::ast::UnOp::Neg, a) => invariant(a, locals, scope),
            // A fixed section is disjoint across iterations when another
            // destination axis carries each concurrent index exactly once.
            Expr::Section { lower, upper, stride, .. } =>
                [lower, upper, stride].into_iter().flatten().all(|e| invariant(e, locals, scope)),
            Expr::Call(n, a) if n=="@section_subscript" && matches!(a.as_slice(),[Expr::Section{..}]) =>
                invariant(&a[0], locals, scope),
            _ => false,
        }
    }
    fn axis(
        e: &Expr,
        indices: &HashSet<String>,
        locals: &HashSet<String>,
        scope: &Scope,
    ) -> Option<String> {
        match e {
            Expr::Ident(n) if indices.contains(n) => Some(n.clone()),
            Expr::Bin(BinOp::Add | BinOp::Sub, a, b) if invariant(b, locals, scope) => {
                axis(a, indices, locals, scope)
            }
            Expr::Bin(BinOp::Add, a, b) if invariant(a, locals, scope) => {
                axis(b, indices, locals, scope)
            }
            _ => None,
        }
    }
    fn writes(
        body: &[Stmt],
        locals: &HashSet<String>,
        written: &mut HashMap<String, Vec<Expr>>,
        indices_set: &HashSet<String>,
        scope: &Scope,
    ) -> Result<()> {
        for s in body {
            match s {
                Stmt::SourceLocation(_)=>{},
                Stmt::Assign{name,..} if locals.contains(name)=>{},
                Stmt::AssignIndex{name,..} if locals.contains(name)=>{},
                Stmt::AssignIndex{name,indices,..} => {
                    let mut seen=HashSet::new();
                    for e in indices {
                        if invariant(e,locals,scope) {continue;}
                        let Some(n)=axis(e,indices_set,locals,scope) else {
                            bail!("independent array-element writes require invariant subscripts or index offsets");
                        };
                        if !seen.insert(n) {bail!("array writes must use every concurrent index exactly once");}
                    }
                    if seen!=*indices_set {bail!("array writes must use every concurrent index exactly once");}
                    if written.get(name).is_some_and(|old| old!=indices) {
                        bail!("array writes must use the same index tuple");
                    }
                    written.insert(name.clone(),indices.clone());
                },
                Stmt::CallSub{name,args} => {
                    if scope.contains_key(&format!("@parallel_dummy:{name}")) {
                        bail!("procedure dummy call requires serial execution");
                    }
                    for (actual,output) in calls::subroutine(name,args,scope)? {
                        if !output {continue;}
                        let assignment=match actual {
                            Expr::Ident(name) if locals.contains(&name) && !indices_set.contains(&name)=>Stmt::Assign{name,value:Expr::IntLit("0".into())},
                            Expr::Index(name,indices)|Expr::Call(name,indices) if scope.get(&name).is_some_and(|o|!o.dims.is_empty())=>Stmt::AssignIndex{name,indices,value:Expr::IntLit("0".into())},
                            _=>bail!("parallel subroutine output requires LOCAL storage or a disjoint array element/section"),
                        };
                        writes(&[assignment],locals,written,indices_set,scope)?;
                    }
                },
                Stmt::If{then_body,else_body,..}=>{writes(then_body,locals,written,indices_set,scope)?;if let Some(b)=else_body {writes(b,locals,written,indices_set,scope)?;}},
                _=>bail!("body requires scalar local assignments, independent array-element writes, and IF statements"),
            }
        }
        Ok(())
    }
    let mut written = HashMap::new();
    writes(body, &locals, &mut written, &indices, scope)?;
    // Shared storage keeps its ordinary Fortran identity. Only read-only
    // captures are admitted, including hidden inputs made explicit by captures.
    let nonlocal = |n: &str| scope.get(n).is_some_and(|o|
        !o.attrs.contains("parameter") && (n.starts_with("@module$") || o.attrs.contains("@parallel_host")));
    if written.keys().chain(locals.iter()).any(|n| nonlocal(n)) {
        bail!("writes to host or module storage require serial execution");
    }
    let writable_dummy = written.keys().chain(locals.iter()).any(|n| scope.contains_key(&format!("@parallel_dummy:{n}")));
    if writable_dummy {
        fn reads_host(body: &[Stmt], nonlocal: &impl Fn(&str)->bool) -> bool {
            body.iter().any(|s| {
                let mut s=s.clone(); let mut found=false;
                for e in expressions(&mut s) { visit_expr(e,&mut |e| {
                    if let Expr::Ident(n)|Expr::Index(n,_)|Expr::Call(n,_)|Expr::Section{name:n,..}=e {
                        found |= nonlocal(n);
                    }
                }); }
                found || children(&mut s).into_iter().any(|b| reads_host(b,nonlocal))
            })
        }
        let headers=vec![Stmt::Print{items:std::iter::once(mask.clone()).chain(controls.iter().cloned()).chain(clauses.iter().cloned()).collect()}];
        if reads_host(body,&nonlocal) || reads_host(&headers,&nonlocal) {
            bail!("writable dummy may alias read-only host or module storage");
        }
    }
    if written.is_empty() && !reductions {
        bail!("no independent array output");
    }
    let mut local_scope=scope.clone();
    for n in &locals {
        if !reduction_arrays.contains(n) {
            if let Some(o)=local_scope.get_mut(n).filter(|o|!o.dims.is_empty()) {
                o.attrs.insert("@parallel_local_array".into());
            }
        }
    }
    let scope=&local_scope;
    fn expr(e: &Expr, scope: &Scope, written: &HashMap<String, Vec<Expr>>) -> Result<()> {
        fn object(n: &str, scope: &Scope) -> Result<()> {
            let o = scope
                .get(n)
                .with_context(|| format!("capture `{n}` requires an explicit declaration"))?;
            let procedure = scope.contains_key("@parallel_procedure");
            let dummy = scope.contains_key(&format!("@parallel_dummy:{n}"));
            if dummy
                && !o.attrs.iter().any(|a| {
                    matches!(
                        a.as_str(),
                        "in" | "out" | "inout" | "intent(in)" | "intent(out)" | "intent(inout)"
                    )
                })
            {
                bail!("procedure captures require explicit INTENT on dummy `{n}`");
            }
            if procedure && o.attrs.contains("save") && !o.attrs.contains("parameter") && !n.starts_with("@module$") {
                bail!("procedure captures of SAVE storage are not yet supported");
            }
            let constants = scope
                .iter()
                .filter_map(|(n, o)| {
                    o.constant
                        .as_ref()
                        .filter(|_| o.attrs.contains("parameter"))
                        .map(|e| (n.clone(), e.clone()))
                })
                .collect();
            let mut kind = o.kind.clone();
            if o.derived
                || !matches!(
                    kind,
                    TypeSpec::Integer(_)
                        | TypeSpec::IntegerKind(_)
                        | TypeSpec::Real
                        | TypeSpec::RealKind(_)
                        | TypeSpec::DoublePrecision
                        | TypeSpec::Logical
                        | TypeSpec::LogicalKind(_)
                        | TypeSpec::Complex(_)
                )
            {
                bail!("capture `{n}` requires an ordinary numeric or LOGICAL type");
            }
            crate::parameters::resolve_kind(&mut kind, &constants).with_context(|| {
                format!("capture `{n}` requires a supported native scalar kind")
            })?;
            if matches!(kind,TypeSpec::Integer(Some(k)) if k>8) {
                bail!("capture `{n}` requires a supported native scalar kind");
            }
            if matches!(&kind,TypeSpec::Complex(k) if !matches!(k.as_str(),"4"|"8")) {
                bail!("capture `{n}` requires COMPLEX kind 4 or 8");
            }
            if o.attrs.iter().any(|a| {
                !matches!(a.as_str(), "parameter" | "save")
                    && !(n.starts_with("@module$") && matches!(a.as_str(),"@module_initializer"|"@module_host"|"@procedure_reference"|"public"|"private"))
                    && !(a == "allocatable" && !o.dims.is_empty())
                    && a != "@parallel_host"
                    && a != "@parallel_local_array"
                    && !(dummy
                        && matches!(
                            a.as_str(),
                            "in" | "out"
                                | "inout"
                                | "intent(in)"
                                | "intent(out)"
                                | "intent(inout)"
                                | "contiguous"
                                | "@parallel_readonly_capture"
                        ))
            }) {
                bail!("capture `{n}` has unsupported attributes");
            }
            fn constant(e: &Expr, scope: &Scope) -> bool {
                match e {
                    Expr::IntLit(_) => true,
                    Expr::Ident(n) => scope.get(n).is_some_and(|o| o.attrs.contains("parameter")),
                    Expr::Bin(_, a, b) => constant(a, scope) && constant(b, scope),
                    Expr::Un(_, a) => constant(a, scope),
                    Expr::Call(n, a) if n == "@declared_bounds" => {
                        a.iter().all(|e| constant(e, scope))
                    }
                    _ => false,
                }
            }
            for d in &o.dims {
                if o.attrs.contains("@parallel_local_array") {continue;}
                if (o.attrs.contains("allocatable") || dummy)
                    && matches!(d, Expr::Ident(n) if matches!(n.as_str(), "@deferred_extent" | "@assumed_extent"))
                {
                    continue;
                }
                if !dummy && !constant(d, scope) {
                    bail!("array `{n}` requires constant bounds or supported runtime dimensions");
                }
            }
            Ok(())
        }
        match e {
            Expr::IntLit(_) | Expr::RealLit(_) | Expr::Logical(_) => Ok(()),
            Expr::Ident(n) => {
                object(n, scope)?;
                if !scope[n].dims.is_empty() && written.contains_key(n) {
                    bail!("whole-array expressions require serial execution");
                }
                Ok(())
            }
            Expr::Index(n, args) | Expr::Call(n, args)
                if scope.get(n).is_some_and(|o| !o.dims.is_empty()) =>
            {
                object(n, scope)?;
                if args.len() != scope[n].dims.len() {
                    bail!("requires scalar subscripts matching the array rank");
                }
                if written.get(n).is_some_and(|tuple| tuple != args) {
                    bail!("read of written array `{n}` is not confined to the current iteration");
                }
                for a in args {
                    expr(a, scope, written)?;
                }
                Ok(())
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
            } => {
                if !name.is_empty() {
                    object(name, scope)?;
                    if written.contains_key(name) {
                        bail!("sections of written arrays require serial execution");
                    }
                }
                for e in [lower, upper, stride].into_iter().flatten() {
                    expr(e, scope, written)?;
                }
                Ok(())
            }
            Expr::Call(n, args)
                if n == "@section_subscript" && matches!(args.as_slice(),[Expr::Section{..}]) =>
            {
                let Expr::Section {lower, upper, stride, ..}=&args[0] else {unreachable!()};
                // The parent array expression already validates the storage
                // and destination tuple; the section name is parser metadata.
                for e in [lower, upper, stride].into_iter().flatten() {expr(e, scope, written)?;}
                Ok(())
            }
            Expr::Call(n, args)
                if n == "@array_constructor"
                    || matches!(n.as_str(), "@vector_subscript" | "@section_subscript")
                        && matches!(args.as_slice(), [Expr::Section { .. }]) =>
            {
                for e in args {
                    expr(e, scope, written)?;
                }
                Ok(())
            }
            Expr::Bin(_, a, b) => {
                expr(a, scope, written)?;
                expr(b, scope, written)
            }
            Expr::Un(_, a) => expr(a, scope, written),
            Expr::Call(n, args) if !scope.contains_key(n)
                && !scope.contains_key(&format!("@parallel_dummy:{n}")) && calls::check(n, args)? => {
                for arg in args {
                    if crate::named_arguments::omitted(arg) {continue;}
                    expr(
                        crate::named_arguments::keyword(arg)
                            .map(|(_, v)| v)
                            .unwrap_or(arg),
                        scope,
                        written,
                    )?;
                }
                Ok(())
            }
            // Functions with no mutable runtime state, implemented directly by codegen.
            Expr::Call(n, args)
                if !scope.contains_key(n)
                    && !SHADOWED.with(|s| s.borrow().contains(n))
                    && (calls::array_intrinsic(n) || intrinsic_name(n)) =>
            {
                for a in args {
                    expr(
                        crate::named_arguments::keyword(a).map(|(_, v)| v).unwrap_or(a),
                        scope,
                        written,
                    )?;
                }
                Ok(())
            }
            _ => bail!("expression or procedure call is outside the parallel subset"),
        }
    }
    for (n, tuple) in &written {
        expr(&Expr::Index(n.clone(), tuple.clone()), scope, &written)?;
    }
    expr(mask, scope, &written)?;
    fn visit(body: &[Stmt], scope: &Scope, written: &HashMap<String, Vec<Expr>>) -> Result<()> {
        for s in body {
            if let Stmt::CallSub{name,args}=s {
                for (actual,_) in calls::subroutine(name,args,scope)? {expr(&actual,scope,written)?;}
                continue;
            }
            let mut s = s.clone();
            for e in expressions(&mut s) {
                expr(e, scope, written)?;
            }
            for b in children(&mut s) {
                visit(b, scope, written)?;
            }
        }
        Ok(())
    }
    visit(body, scope, &written)
}

/// Flatten a Cartesian iteration space. The first written array determines the
/// fastest-varying axis, preserving column-major locality independently of the
/// order of the controls. The pool still sees a single inclusive integer range.
pub(super) fn flatten(
    controls: Vec<Expr>,
    mask: Expr,
    body: Vec<Stmt>,
    prefix: &mut Vec<Stmt>,
    serial: &mut usize,
) -> Stmt {
    fn temporary(prefix: &mut Vec<Stmt>, serial: &mut usize) -> String {
        *serial += 1;
        let n = format!("@parallel_extent_{serial}");
        prefix.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![n.clone()],
        });
        n
    }
    fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }
    fn guard(cond: Expr) -> Stmt {
        Stmt::If {
            cond,
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(
                    "parallel iteration space exceeds signed 64-bit range".into(),
                )],
            }],
            else_body: None,
        }
    }
    fn first_write(body: &[Stmt]) -> Option<Vec<Expr>> {
        for s in body {
            match s {
                Stmt::AssignIndex { indices, .. } => return Some(indices.clone()),
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if let Some(v) = first_write(then_body)
                        .or_else(|| else_body.as_ref().and_then(|b| first_write(b)))
                    {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let total = temporary(prefix, serial);
    let linear = temporary(prefix, serial);
    let remaining = temporary(prefix, serial);
    let max = Expr::Call(
        "@integer_literal".into(),
        vec![Expr::IntLit(i64::MAX.to_string()), int(8)],
    );
    let mut dimensions = vec![];
    let mut nonempty = Expr::Logical(true);
    for c in controls.chunks(4) {
        let low = temporary(prefix, serial);
        let high = temporary(prefix, serial);
        let extent = temporary(prefix, serial);
        let start = temporary(prefix, serial);
        let step = temporary(prefix, serial);
        let magnitude = temporary(prefix, serial);
        prefix.push(set(&start, c[1].clone()));
        prefix.push(set(&step, c[3].clone()));
        prefix.push(Stmt::If {
            cond: binary(BinOp::Eq, id(&step), int(0)),
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str("DO CONCURRENT stride must not be zero".into())],
            }],
            else_body: None,
        });
        prefix.push(guard(binary(
            BinOp::Lt,
            id(&step),
            Expr::sub(int(0), max.clone()),
        )));
        prefix.push(set(&magnitude, id(&step)));
        prefix.push(set(&low, c[1].clone()));
        prefix.push(set(&high, c[2].clone()));
        prefix.push(Stmt::If {
            cond: binary(BinOp::Lt, id(&step), int(0)),
            then_body: vec![
                set(&low, c[2].clone()),
                set(&high, c[1].clone()),
                set(&magnitude, Expr::sub(int(0), id(&step))),
            ],
            else_body: None,
        });
        prefix.push(set(&extent, int(1)));
        nonempty = binary(BinOp::And, nonempty, binary(BinOp::Ge, id(&high), id(&low)));
        dimensions.push((c[0].clone(), low, high, extent, start, step, magnitude));
    }
    prefix.push(set(&total, int(0)));
    let mut calculate = vec![set(&total, int(1))];
    for (_, low, high, extent, _, _, magnitude) in &dimensions {
        // Test emptiness for *all* axes before doing any extent arithmetic.
        // A nonempty extent and its product must fit the callback ABI's i64.
        calculate.push(Stmt::If {
            cond: binary(BinOp::Le, id(low), int(0)),
            then_body: vec![guard(binary(
                BinOp::Gt,
                id(high),
                Expr::sub(Expr::add(max.clone(), id(low)), int(1)),
            ))],
            else_body: None,
        });
        calculate.push(set(
            extent,
            Expr::add(
                Expr::div(Expr::sub(id(high), id(low)), id(magnitude)),
                int(1),
            ),
        ));
        calculate.push(guard(binary(
            BinOp::Gt,
            id(&total),
            Expr::div(max.clone(), id(extent)),
        )));
        calculate.push(set(&total, Expr::mul(id(&total), id(extent))));
    }
    prefix.push(Stmt::If {
        cond: nonempty,
        then_body: calculate,
        else_body: None,
    });
    fn indices_in(e: &Expr, controls: &[Expr], order: &mut Vec<Expr>) {
        if controls.chunks(4).any(|c| c[0] == *e) {
            order.push(e.clone());
            return;
        }
        if let Expr::Bin(_, a, b) = e {
            indices_in(a, controls, order);
            indices_in(b, controls, order);
        }
    }
    let order = if let Some(tuple) = first_write(&body) {
        let mut order = vec![];
        for e in tuple {
            indices_in(&e, &controls, &mut order);
        }
        if order.len() == dimensions.len()
            && order
                .iter()
                .collect::<Vec<_>>()
                .iter()
                .enumerate()
                .all(|(i, e)| !order[..i].contains(e))
        {
            order
        } else {
            dimensions.iter().rev().map(|d| d.0.clone()).collect()
        }
    } else {
        dimensions.iter().rev().map(|d| d.0.clone()).collect()
    };
    let mut iterations = vec![set(&remaining, id(&linear))];
    for index in order {
        let (_, _, _, extent, start, step, _) = dimensions
            .iter()
            .find(|d| d.0 == index)
            .expect("proved concurrent index");
        let Expr::Ident(n) = index else {
            unreachable!()
        };
        iterations.push(set(
            &n,
            Expr::add(
                id(start),
                Expr::mul(
                    id(step),
                    Expr::sub(
                        id(&remaining),
                        Expr::mul(Expr::div(id(&remaining), id(extent)), id(extent)),
                    ),
                ),
            ),
        ));
        iterations.push(set(&remaining, Expr::div(id(&remaining), id(extent))));
    }
    iterations.push(Stmt::If {
        cond: mask,
        then_body: body,
        else_body: None,
    });
    Stmt::Do {
        var: linear,
        start: int(0),
        end: Expr::sub(id(&total), int(1)),
        body: iterations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_procedure_dummy_cannot_inherit_a_same_named_definition() {
        configure(true,false);
        procedures(&Program{name:"p".into(),contains:vec![],body:vec![Stmt::Subroutine{
            name:"step".into(),params:vec!["x".into(),"y".into()],pure:true,elemental:false,
            body:vec![Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec!["x".into(),"y".into()]},
                Stmt::DummyIntent{name:"x".into(),intent:"intent(in)".into()},
                Stmt::DummyIntent{name:"y".into(),intent:"intent(out)".into()},set("y",id("x"))],
        },Stmt::Function{name:"calc".into(),params:vec!["x".into()],pure:true,elemental:false,
            return_type:Some(TypeSpec::Integer(None)),body:vec![
                Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec!["x".into(),"calc".into()]},
                Stmt::DummyIntent{name:"x".into(),intent:"intent(in)".into()},
                set("calc",id("x")),Stmt::Return(Some(id("calc")))],
        }]});
        let scalar=Object{kind:TypeSpec::Integer(None),dims:vec![],attrs:HashSet::new(),derived:false,constant:None};
        let mut array=scalar.clone();array.dims=vec![int(9000)];
        let mut scope=Scope::from([("i".into(),scalar.clone()),("a".into(),array)]);
        let body=vec![Stmt::CallSub{name:"step".into(),args:vec![id("i"),Expr::Call("a".into(),vec![id("i")])]}];
        let controls=[id("i"),int(1),int(9000),int(1)];
        assert!(check(&controls,&Expr::Logical(true),&body,&[],&scope,None).is_ok());
        scope.insert("@parallel_dummy:step".into(),scalar);
        assert!(check(&controls,&Expr::Logical(true),&body,&[],&scope,None).unwrap_err().to_string().contains("procedure dummy"));
        let body=vec![Stmt::AssignIndex{name:"a".into(),indices:vec![id("i")],value:Expr::Call("calc".into(),vec![id("i")])}];
        assert!(check(&controls,&Expr::Logical(true),&body,&[],&scope,None).is_ok());
        scope.insert("@parallel_dummy:calc".into(),scope["i"].clone());
        assert!(check(&controls,&Expr::Logical(true),&body,&[],&scope,None).is_err());
        configure(false,false);
    }
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
                    constant: None,
                },
            ),
            (
                "a".into(),
                Object {
                    kind: TypeSpec::Real,
                    dims: vec![int(9000)],
                    attrs: HashSet::new(),
                    derived: false,
                    constant: None,
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

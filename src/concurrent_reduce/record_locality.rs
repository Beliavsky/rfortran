// SPDX-License-Identifier: MIT
//! Scalar record locality delegates storage and checks to primitive locality.
use super::*;

fn references(body: &[Stmt], name: &str) -> bool {
    let mut found=false;
    for s in body {
        let mut s=s.clone();
        let designator=|n:&str|n==name || n.strip_prefix(name).is_some_and(|s|s.starts_with('%'));
        if let Stmt::Assign {name,..}|Stmt::AssignIndex {name,..}= &s {found|=designator(name);}
        for e in expressions(&mut s) {
            // Hidden inputs belong to the callee's host, not to the construct
            // entity that happens to shadow that host name.
            visit_expr(e,&mut |e| {
                if matches!(e,Expr::Call(tag,_) if tag.starts_with("@keyword:@parallel_read_")) {*e=int(0);}
            });
            visit_expr(e,&mut |e| {
                if let Expr::Ident(n)|Expr::Call(n,_)|Expr::Index(n,_)|Expr::Section {name:n,..}=e {found|=designator(n);}
            });
        }
        for b in children(&mut s) {found|=references(b,name);}
    }
    found
}

fn nested_concurrent(body: &[Stmt]) -> bool {
    body.iter().any(|s| {
        if matches!(s,Stmt::CallSub {name,..} if matches!(name.as_str(),"@do_concurrent"|"@concurrent_reduce")) {return true;}
        let mut s=s.clone(); children(&mut s).into_iter().any(|b|nested_concurrent(b))
    })
}

fn host_inputs(body: &mut [Stmt], saved: &mut Vec<Expr>, restore: bool) {
    for s in body {
        for e in expressions(s) {
            visit_expr(e,&mut |e| {
                if restore {
                    if let Expr::Call(tag,args)=e {
                        if tag=="@locality_host_input" {
                            let Expr::IntLit(slot)=&args[0] else {unreachable!()};
                            *e=saved[slot.parse::<usize>().unwrap()].clone();
                        }
                    }
                } else if matches!(e,Expr::Call(tag,_) if tag.starts_with("@keyword:@parallel_read_")) {
                    let slot=saved.len();saved.push(e.clone());
                    *e=Expr::Call("@locality_host_input".into(),vec![int(slot as i64)]);
                }
            });
        }
        for b in children(s) {host_inputs(b,saved,restore);}
    }
}

pub(super) fn expand(controls: &[Expr], mask: &mut Expr, body: &mut Vec<Stmt>, clauses: &mut [Expr],
                     scope: &mut Scope, serial: &mut usize) -> Result<Vec<Stmt>> {
    if !clauses.iter().any(|e|matches!(e,Expr::Call(tag,names)
        if matches!(tag.as_str(),"@locality:local"|"@locality:local_init")
        && names.iter().any(|e|matches!(e,Expr::Ident(n) if scope.get(n).is_some_and(|o|o.derived))))) {return Ok(vec![]);}
    let mut hidden=vec![];
    let mut header=vec![Stmt::Print {items:vec![mask.clone()]}];
    host_inputs(body,&mut hidden,false);
    host_inputs(&mut header,&mut hidden,false);
    let Stmt::Print {items}=header.remove(0) else {unreachable!()};
    *mask=items.into_iter().next().unwrap();
    let mut prefix=vec![];
    let mut defaults=vec![];
    for clause in clauses {
        let Expr::Call(tag,names)=clause else {continue};
        if !matches!(tag.as_str(),"@locality:local"|"@locality:local_init") {continue;}
        let mut expanded=vec![];
        for name in std::mem::take(names) {
            let Expr::Ident(n)=&name else {unreachable!()};
            let object=&scope[n];
            if !object.derived {expanded.push(name);continue;}
            if !object.dims.is_empty() {bail!("derived LOCAL/LOCAL_INIT `{n}` currently requires a scalar record");}
            for attr in ["intent(in)","in","optional","allocatable","pointer","target","asynchronous","volatile","protected"] {
                if object.attrs.contains(attr) {bail!("LOCAL/LOCAL_INIT variable `{n}` with {attr} is not supported");}
            }
            let fields=records::private_fields(n,scope)
                .with_context(||format!("LOCAL/LOCAL_INIT of derived-type variable `{n}` requires a bounded numeric/LOGICAL layout without owned components or bindings"))?;
            if nested_concurrent(body) {bail!("nested DO CONCURRENT with derived LOCAL/LOCAL_INIT `{n}` is not yet supported");}
            if tag=="@locality:local" && references(&[Stmt::Print {items:std::iter::once(mask.clone()).chain(controls.iter().cloned()).collect()}],n) {
                bail!("LOCAL variable `{n}` cannot appear in the concurrent header");
            }
            for (field,mut object,default) in fields {
                *serial+=1;
                let base=if n.starts_with("@module$") {n.rsplit('$').next().unwrap_or(n)} else {n};
                let leaf=format!("{base}$block_record{serial}");
                object.attrs.clear();
                prefix.push(if object.dims.is_empty() {Stmt::VarDecl {kind:object.kind.clone(),names:vec![leaf.clone()]}}
                    else {Stmt::ArrayDecl {kind:object.kind.clone(),name:leaf.clone(),dims:object.dims.clone()}});
                if tag=="@locality:local_init" {prefix.push(set(&leaf,id(&field)));}
                else if let Some(value)=default {defaults.push(set(&leaf,value));}
                crate::block_scopes::rename(body,&field,&leaf);
                let mut header=vec![Stmt::Print {items:vec![mask.clone()]}];
                crate::block_scopes::rename(&mut header,&field,&leaf);
                let Stmt::Print {items}=header.remove(0) else {unreachable!()};
                *mask=items.into_iter().next().unwrap();
                scope.insert(format!("@parallel_record_leaf:{leaf}"),object.clone());
                scope.insert(leaf.clone(),object);
                expanded.push(id(&leaf));
            }
            if references(body,n) || references(&[Stmt::Print {items:vec![mask.clone()]}],n) {
                bail!("whole-object or unsupported component use of derived LOCAL/LOCAL_INIT `{n}` is not yet supported");
            }
        }
        *names=expanded;
    }
    host_inputs(body,&mut hidden,true);
    let mut header=vec![Stmt::Print {items:vec![mask.clone()]}];
    host_inputs(&mut header,&mut hidden,true);
    let Stmt::Print {items}=header.remove(0) else {unreachable!()};
    *mask=items.into_iter().next().unwrap();
    if !defaults.is_empty() {defaults.append(body);*body=defaults;}
    Ok(prefix)
}

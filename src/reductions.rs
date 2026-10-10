//! Lower rank-one numeric reductions and scalar array assignments to loops.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone,Default)]
struct Symbols {
    variables: HashMap<String,(TypeSpec,Vec<Expr>)>,
    elementals: HashMap<String,TypeSpec>,
}
impl Symbols { fn new() -> Self { Self::default() } }
impl std::ops::Deref for Symbols {
    type Target=HashMap<String,(TypeSpec,Vec<Expr>)>;
    fn deref(&self) -> &Self::Target { &self.variables }
}
impl std::ops::DerefMut for Symbols {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.variables }
}
fn int(n: i64) -> Expr { Expr::IntLit(n.to_string()) }
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr { Expr::Bin(op, Box::new(a), Box::new(b)) }
fn reduction(name: &str) -> bool { matches!(name.to_ascii_lowercase().as_str(), "sum" | "product" | "minval" | "maxval" | "any" | "all") }
fn numeric(kind: &TypeSpec) -> bool { matches!(kind, TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision) }
fn merge_kind(a: TypeSpec, b: TypeSpec) -> TypeSpec {
    if matches!(a, TypeSpec::DoublePrecision) || matches!(b, TypeSpec::DoublePrecision) { TypeSpec::DoublePrecision }
    else if matches!(a, TypeSpec::Real) || matches!(b, TypeSpec::Real) { TypeSpec::Real }
    else { a }
}
fn constant(e: &Expr) -> Result<i64> {
    crate::parameters::integer_constant(e).context("reduction extents and section bounds must be constant integers")
}

// Produce a scalar expression for element `index`, its shape, and its type.
fn element(e: &Expr, index: &Expr, symbols: &Symbols) -> Result<(Expr, Option<i64>, TypeSpec)> {
    if let Some((value,count)) = crate::multidim::view_element(e,index) {
        let Expr::Index(name,_) = &value else {unreachable!()};
        let kind=symbols.get(name).context("undefined section source")?.0.clone();
        if !numeric(&kind) { bail!("array sections currently require numeric arrays"); }
        return Ok((value,Some(count),kind));
    }
    match e {
        Expr::Ident(name) | Expr::Section { name, .. } => {
            let (kind, dims) = symbols.get(name).with_context(|| format!("undefined reduction argument `{name}`"))?;
            if !numeric(kind) { bail!("reductions require numeric arrays"); }
            if dims.is_empty() && matches!(e, Expr::Ident(_)) { return Ok((e.clone(), None, kind.clone())); }
            if dims.len() != 1 { bail!("reductions currently require one-dimensional arrays"); }
            let extent = constant(&dims[0])?;
            if extent < 0 { bail!("negative array extent is not supported"); }
            let (first, count, step) = if let Expr::Section { lower, upper, stride, .. } = e {
                let first = lower.as_deref().map(constant).transpose()?.unwrap_or(1);
                let last = upper.as_deref().map(constant).transpose()?.unwrap_or(extent);
                let step = stride.as_deref().map(constant).transpose()?.unwrap_or(1);
                if step == 0 { bail!("array section stride must not be zero"); }
                let distance = (i128::from(last) - i128::from(first)) * i128::from(step.signum());
                let count = if distance < 0 { 0 } else { distance / i128::from(step).abs() + 1 };
                let count = i64::try_from(count).context("section size overflow")?;
                let final_index = i128::from(first) + i128::from(count.saturating_sub(1)) * i128::from(step);
                if count > 0 && (first < 1 || first > extent || final_index < 1 || final_index > i128::from(extent)) {
                    bail!("array section of `{name}` selects an index outside 1:{extent}");
                }
                (first, count, step)
            } else { (1, extent, 1) };
            let subscript = bin(BinOp::Add, int(first), bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), int(1)), int(step)));
            Ok((Expr::Index(name.clone(), vec![subscript]), Some(count), kind.clone()))
        }
        Expr::Logical(_) => Ok((e.clone(), None, TypeSpec::Logical)),
        Expr::IntLit(_) => Ok((e.clone(), None, TypeSpec::Integer(None))),
        Expr::RealLit(_) => Ok((e.clone(), None, TypeSpec::Real)),
        Expr::Un(UnOp::Neg, a) => {
            let (a, len, kind) = element(a, index, symbols)?;
            Ok((Expr::Un(UnOp::Neg, Box::new(a)), len, kind))
        }
        Expr::Bin(op, a, b) => {
            let (a, alen, akind) = element(a, index, symbols)?;
            let (b, blen, bkind) = element(b, index, symbols)?;
            if let (Some(a), Some(b)) = (alen, blen) { if a != b { bail!("array shapes in reduction expression do not conform"); } }
            Ok((bin(op.clone(), a, b), alen.or(blen), if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow) { merge_kind(akind, bkind) } else { TypeSpec::Logical }))
        }
        Expr::Index(name, args) | Expr::Call(name, args) if symbols.get(name).is_some_and(|(_, d)| d.len() == 1) => {
            if args.len() != 1 { bail!("array element requires one subscript"); }
            let (arg, len, kind) = element(&args[0], index, symbols)?;
            if len.is_some() || !matches!(kind, TypeSpec::Integer(_)) { bail!("array subscript must be scalar INTEGER"); }
            let kind = symbols[name].0.clone();
            if !numeric(&kind) { bail!("reductions require numeric arrays"); }
            Ok((Expr::Index(name.clone(), vec![arg]), None, kind))
        }
        Expr::Call(name,args) if symbols.elementals.contains_key(name) => {
            let mut shape=None; let mut values=vec![];
            for arg in args {
                let (value,count,_)=element(arg,index,symbols)?;
                if let (Some(a),Some(b))=(shape,count) { if a!=b {bail!("ELEMENTAL argument shapes do not conform");} }
                shape=shape.or(count); values.push(value);
            }
            Ok((Expr::Call(name.clone(),values),shape,symbols.elementals[name].clone()))
        }
        Expr::Call(name, args) if crate::intrinsics::math_arity(name).is_some() || matches!(name.as_str(), "dble" | "real" | "abs") => {
            let mut shape=None; let mut kind=TypeSpec::Real; let mut values=vec![];
            for arg in args { let (value,n,k)=element(arg,index,symbols)?; if let (Some(a),Some(b))=(shape,n) { if a!=b {bail!("array shapes do not conform");} } shape=shape.or(n);kind=merge_kind(kind,k); values.push(value); }
            if name=="dble" {kind=TypeSpec::DoublePrecision;} else if name=="real" {kind=TypeSpec::Real;}
            Ok((Expr::Call(name.clone(),values),shape,kind))
        }
        _ => bail!("unsupported expression in numeric array reduction"),
    }
}

fn lower_expr(e: &mut Expr, symbols: &mut Symbols, prefix: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
    match e {
        Expr::Call(name, args) if reduction(name) => {
            if args.len() != 1 { bail!("{name} currently expects one array argument; DIM and MASK are not supported"); }
            lower_expr(&mut args[0], symbols, prefix, serial)?;
            *serial += 1;
            // These names cannot occur in source identifiers.
            let index_name = format!("@reduction_index_{serial}");
            let acc_name = format!("@reduction_value_{serial}");
            let item_name = format!("@reduction_item_{serial}");
            let index = Expr::Ident(index_name.clone());
            let (value, count, kind) = element(&args[0], &index, symbols)?;
            let count = count.context("reduction argument must be an array")?;
            let name = name.to_ascii_lowercase();
            if matches!(name.as_str(), "any" | "all") != matches!(kind, TypeSpec::Logical) { bail!("reduction argument has incorrect numeric/logical type"); }
            let integer = matches!(kind, TypeSpec::Integer(_));
            let huge = if matches!(kind, TypeSpec::DoublePrecision) { f64::MAX } else { f32::MAX as f64 };
            let identity = match name.as_str() {
                "any" => Expr::Logical(false),
                "all" => Expr::Logical(true),
                "sum" => if integer { int(0) } else { Expr::RealLit("0.0".into()) },
                "product" => if integer { int(1) } else { Expr::RealLit("1.0".into()) },
                "minval" => if integer { int(i64::MAX) } else { Expr::RealLit(huge.to_string()) },
                _ => if integer { int(-i64::MAX) } else { Expr::RealLit((-huge).to_string()) },
            };
            prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index_name.clone()] });
            prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![acc_name.clone(), item_name.clone()] });
            symbols.insert(acc_name.clone(), (kind, vec![]));
            prefix.push(Stmt::Assign { name: acc_name.clone(), value: identity });
            let acc = Expr::Ident(acc_name.clone());
            let item = Expr::Ident(item_name.clone());
            let update = match name.as_str() {
                "any" | "all" => Stmt::Assign { name: acc_name.clone(), value: bin(if name == "any" { BinOp::Or } else { BinOp::And }, acc.clone(), item.clone()) },
                "sum" | "product" => Stmt::Assign { name: acc_name.clone(), value: bin(if name == "sum" { BinOp::Add } else { BinOp::Mul }, acc.clone(), item.clone()) },
                _ => Stmt::If {
                    cond: bin(if name == "minval" { BinOp::Lt } else { BinOp::Gt }, item.clone(), acc.clone()),
                    then_body: vec![Stmt::Assign { name: acc_name.clone(), value: item.clone() }], else_body: None,
                },
            };
            prefix.push(Stmt::Do { var: index_name, start: int(1), end: int(count), body: vec![Stmt::Assign { name: item_name, value }, update] });
            *e = acc;
        }
        Expr::Call(name, args) if matches!(name.as_str(), "tiny" | "huge" | "kind" | "size") => {
            let [Expr::Ident(n)] = args.as_slice() else { bail!("{name} requires a typed variable"); };
            let (kind,dims)=symbols.get(n).context("undefined inquiry argument")?;
            let double=matches!(kind,TypeSpec::DoublePrecision);
            *e=match name.as_str() {
                "size" => { if dims.len()!=1 {bail!("SIZE requires a rank-one array");} dims[0].clone() },
                "kind"=>int(if double {8} else {4}),
                _=> { if !matches!(kind,TypeSpec::Real|TypeSpec::DoublePrecision) {bail!("{name} requires a REAL argument");} let v=if name=="tiny" {if double {f64::MIN_POSITIVE} else {f32::MIN_POSITIVE as f64}} else {if double {f64::MAX} else {f32::MAX as f64}}; let lit=Expr::RealLit(v.to_string()); if double {Expr::Call("dble".into(),vec![lit])} else {lit} }
            };
        }
        Expr::Call(tag, _) if tag == "@matrix_view" => {},
        Expr::Call(_, args) | Expr::Index(_, args) => for arg in args { lower_expr(arg, symbols, prefix, serial)?; },
        Expr::Bin(_, a, b) => { lower_expr(a, symbols, prefix, serial)?; lower_expr(b, symbols, prefix, serial)?; }
        Expr::Un(_, a) => lower_expr(a, symbols, prefix, serial)?,
        _ => {}
    }
    if matches!(e,Expr::Call(name,_) if symbols.elementals.contains_key(name)) && array_valued(e,symbols) {
        *serial+=1;
        let index_name=format!("@elemental_index_{serial}");
        let temp=format!("@elemental_result_{serial}");
        let index=Expr::Ident(index_name.clone());
        let (value,count,kind)=element(e,&index,symbols)?;
        let count=count.context("ELEMENTAL array expression has no known shape")?;
        prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index_name.clone()]});
        prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(count)]});
        prefix.push(Stmt::Do{var:index_name,start:int(1),end:int(count),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![index],value}]});
        symbols.insert(temp.clone(),(kind,vec![int(count)]));
        *e=Expr::Ident(temp);
    }
    Ok(())
}

fn array_valued(e: &Expr, symbols: &Symbols) -> bool {
    match e {
        Expr::Ident(name) => symbols.get(name).is_some_and(|(_, dims)| !dims.is_empty()),
        Expr::Section { .. } => true,
        Expr::Bin(_, a, b) => array_valued(a, symbols) || array_valued(b, symbols),
        Expr::Un(_, a) => array_valued(a, symbols),
        Expr::Call(_, args) | Expr::Index(_, args) => args.iter().any(|arg| array_valued(arg, symbols)),
        _ => false,
    }
}

fn lower_body(body: &mut Vec<Stmt>, inherited: &Symbols, serial: &mut usize) -> Result<()> {
    let mut symbols = inherited.clone();
    for stmt in body.iter() {
        match stmt {
            Stmt::VarDecl { kind, names } => for name in names { symbols.insert(name.clone(), (kind.clone(), vec![])); },
            Stmt::ArrayDecl { kind, name, dims } => { symbols.insert(name.clone(), (kind.clone(), dims.clone())); }
            _ => {}
        }
    }
    let mut output = Vec::new();
    for mut stmt in std::mem::take(body) {
        let mut prefix = Vec::new();
        if let Stmt::CallSub {name,args}=&stmt {
            if name=="@allocate" {bail!("ALLOCATE is currently supported only in specialized local array subroutines");}
            if name=="@formatted_print" { let Some(Expr::Str(fmt))=args.first() else {bail!("PRINT format must be constant");};crate::formatting::parse(fmt,args.len()-1)?; }
            if name=="@error_stop" && !matches!(args.as_slice(),[Expr::Str(_)]) {bail!("ERROR STOP currently requires a character constant");}
        }
        if let Stmt::Read {args} = &stmt {
            fn has_elemental(e: &Expr, symbols: &Symbols) -> bool {
                match e {
                    Expr::Call(n,args) | Expr::Index(n,args) => symbols.elementals.contains_key(n) || args.iter().any(|e|has_elemental(e,symbols)),
                    Expr::Bin(_,a,b) => has_elemental(a,symbols) || has_elemental(b,symbols),
                    Expr::Un(_,a) => has_elemental(a,symbols),
                    _ => false,
                }
            }
            if args.iter().any(|e|has_elemental(e,&symbols)) {bail!("ELEMENTAL function result is not a writable READ destination");}
        }
        match &mut stmt {
            Stmt::Print { items } | Stmt::Read { args: items } | Stmt::CallSub { args: items, .. } => for e in items { lower_expr(e, &mut symbols, &mut prefix, serial)?; },
            Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => lower_expr(value, &mut symbols, &mut prefix, serial)?,
            Stmt::AssignIndex { value, indices, .. } => {
                lower_expr(value, &mut symbols, &mut prefix, serial)?;
                for e in indices { lower_expr(e, &mut symbols, &mut prefix, serial)?; }
            }
            Stmt::If { cond, then_body, else_body } => {
                lower_expr(cond, &mut symbols, &mut prefix, serial)?;
                lower_body(then_body, &symbols, serial)?;
                if let Some(body) = else_body { lower_body(body, &symbols, serial)?; }
            }
            Stmt::Do { start, end, body, .. } => {
                lower_expr(start, &mut symbols, &mut prefix, serial)?;
                lower_expr(end, &mut symbols, &mut prefix, serial)?;
                lower_body(body, &symbols, serial)?;
            }
            Stmt::DoWhile { cond, body } => {
                lower_expr(cond, &mut symbols, &mut prefix, serial)?;
                if !prefix.is_empty() { bail!("reductions in DO WHILE conditions are not yet supported"); }
                lower_body(body, &symbols, serial)?;
            }
            Stmt::Block { body } | Stmt::Module { body, .. } => lower_body(body, &symbols, serial)?,
            Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } => {
                let mut local = symbols.clone();
                for param in params { local.remove(param); }
                lower_body(body, &local, serial)?;
            }
            Stmt::SelectCase { expr, cases, default } => {
                lower_expr(expr, &mut symbols, &mut prefix, serial)?;
                for case in cases { lower_body(&mut case.body, &symbols, serial)?; }
                if let Some(body) = default { lower_body(body, &symbols, serial)?; }
            }
            _ => {}
        }
        if let Stmt::AssignIndex { name, indices, value } = &stmt {
            if let [section] = indices.as_slice() {
                if matches!(section,Expr::Section{..}) || crate::multidim::view_element(section,&int(1)).is_some() {
                    let (_, count, kind) = element(section, &int(1), &symbols)?;
                    let count=count.unwrap();
                    let constructor=match value {Expr::Call(tag,items) if tag == "@array_constructor" => Some(items),_ => None};
                    if constructor.is_some_and(|items|items.iter().any(|e|array_valued(e,&symbols) || matches!(e,Expr::Call(tag,_) if tag == "@array_constructor"))) {
                        bail!("section assignment constructor items must currently be scalar");
                    }
                    let array_rhs=constructor.is_some() || array_valued(value,&symbols);
                    if array_rhs {
                        let rhs_count=if let Some(items)=constructor {Some(items.len() as i64)} else {element(value,&int(1),&symbols)?.1};
                        if rhs_count!=Some(count) {bail!("array section assignment shapes do not conform");}
                    }
                    let mut writes=Vec::new();
                    let scalar = if !array_rhs {
                        *serial+=1;
                        let scalar=format!("@section_assign_value_{serial}");
                        prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![scalar.clone()]});
                        prefix.push(Stmt::Assign{name:scalar.clone(),value:value.clone()});
                        Some(scalar)
                    } else {None};
                    // Capture all RHS elements before any destination write, including
                    // overlapping/reversed sections. Constant destinations retain exact
                    // definite-initialization information.
                    for i in 1..=count {
                        let temp=if let Some(scalar)=&scalar {scalar.clone()} else {
                            *serial+=1;
                            let temp=format!("@section_assign_value_{serial}");
                            let item=if let Some(items)=constructor {items[(i-1) as usize].clone()} else {element(value,&int(i),&symbols)?.0};
                            prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});
                            prefix.push(Stmt::Assign{name:temp.clone(),value:item});
                            temp
                        };
                        let (target,_,_)=element(section,&int(i),&symbols)?;
                        let Expr::Index(_,indices)=target else {unreachable!()};
                        writes.push(Stmt::AssignIndex{name:name.clone(),indices,value:Expr::Ident(temp)});
                    }
                    prefix.extend(writes);
                    stmt=Stmt::Block{body:std::mem::take(&mut prefix)};
                }
            }
        }
        if let Stmt::Assign { name, value } = &stmt {
            if let Some((kind, dims)) = symbols.get(name).filter(|(_, dims)| !dims.is_empty()) {
                if dims.len() != 1 || !numeric(kind) {
                    bail!("whole-array assignment currently requires a one-dimensional numeric array");
                }
                if let Expr::Call(tag,values) = value { if tag == "@array_constructor" {
                    if constant(&dims[0])? != values.len() as i64 {bail!("array constructor shape does not conform");}
                    let mut writes=vec![];
                    for (i,v) in values.iter().enumerate() {
                        *serial+=1;let temp=format!("@constructor_{serial}");
                        prefix.push(Stmt::VarDecl{kind:kind.clone(),names:vec![temp.clone()]});prefix.push(Stmt::Assign{name:temp.clone(),value:v.clone()});
                        writes.push(Stmt::AssignIndex{name:name.clone(),indices:vec![int(i as i64+1)],value:Expr::Ident(temp)});
                    }
                    prefix.extend(writes);output.push(Stmt::Block{body:prefix});continue;
                } }
                if array_valued(value, &symbols) {
                    *serial+=1;let index=format!("@array_index_{serial}");let temp=format!("@array_temp_{serial}");let index_expr=Expr::Ident(index.clone());
                    let (scalar,count,_)=element(value,&index_expr,&symbols)?;let extent=constant(&dims[0])?;
                    if count!=Some(extent) {bail!("array assignment shapes do not conform");}
                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(None),names:vec![index.clone()]});
                    prefix.push(Stmt::ArrayDecl{kind:kind.clone(),name:temp.clone(),dims:vec![int(extent)]});
                    prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:int(extent),body:vec![Stmt::AssignIndex{name:temp.clone(),indices:vec![index_expr.clone()],value:scalar}]});
                    prefix.push(Stmt::Do{var:index,start:int(1),end:int(extent),body:vec![Stmt::AssignIndex{name:name.clone(),indices:vec![index_expr.clone()],value:Expr::Index(temp,vec![index_expr])}]});
                    output.push(Stmt::Block{body:prefix});continue;
                }
                let extent = constant(&dims[0])?;
                if extent < 0 { bail!("negative array extent is not supported"); }
                *serial += 1;
                let index = format!("@array_assign_index_{serial}");
                let scalar = format!("@array_assign_value_{serial}");
                prefix.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
                prefix.push(Stmt::VarDecl { kind: kind.clone(), names: vec![scalar.clone()] });
                // Capture before writing any elements, including when the RHS reads this array.
                prefix.push(Stmt::Assign { name: scalar.clone(), value: value.clone() });
                stmt = Stmt::Do {
                    var: index.clone(), start: int(1), end: int(extent),
                    body: vec![Stmt::AssignIndex {
                        name: name.clone(), indices: vec![Expr::Ident(index)], value: Expr::Ident(scalar),
                    }],
                };
                // Give generated temporaries a declaration scope even inside a loop or IF.
                prefix.push(stmt);
                stmt = Stmt::Block { body: std::mem::take(&mut prefix) };
            }
        }
        output.extend(prefix);
        output.push(stmt);
    }
    *body = output;
    Ok(())
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut symbols=Symbols::new();
    symbols.elementals=crate::elemental::functions(&program);
    lower_body(&mut program.body, &symbols, &mut 0)?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    #[test]
    fn section_assignment_tracks_initialized_elements() {
        for (assignment, output, valid) in [
            ("v(1:2)=10", "v", false),
            ("v(1:2)=10", "v(3)", false),
            ("v(1:2)=10", "v(1:2)", true),
            ("v(:)=10", "v", true),
            ("v(3:1:-1)=10", "v", true),
            ("v(::2)=10", "v(2)", false),
            ("v(2:1)=10", "v(1)", false),
        ] {
            let source = format!("implicit none\ninteger :: v(3)\n{assignment}\nprint *, {output}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            let p = crate::sections::resolve(super::resolve(crate::parameters::resolve(p).unwrap()).unwrap()).unwrap();
            assert_eq!(crate::initialized::instrument(p).is_ok(), valid, "{source}");
        }
        for assignment in ["v(::0)=10", "v(0:2)=10", "v(2:4)=10", "v(:)=v(1:2)", "v(1:1)=[v]"] {
            let source = format!("integer :: v(3)\n{assignment}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            assert!(super::resolve(crate::parameters::resolve(p).unwrap()).is_err(), "{source}");
        }
    }

    #[test]
    fn lowers_supported_array_assignments() {
        for assignment in ["a = b", "a = b + 1", "a = b(:)", "a = sqrt(b)"] {
            let source = format!("implicit none\nreal :: a(3), b(3)\n{assignment}\nend");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "assignment.f90").unwrap();
            super::resolve(crate::parameters::resolve(p).unwrap()).unwrap();
        }
    }

    #[test]
    fn rejects_invalid_reductions() {
        for (decls, expression, message) in [
            ("real :: x", "sum(x)", "must be an array"),
            ("real :: x(3)", "sum(x,1)", "one array argument"),
            ("real :: x(3), y(2)", "sum(x+y)", "do not conform"),
            ("real :: x(3)", "product(x(::0))", "stride must not be zero"),
            ("real :: x(3)", "minval(x(0:2))", "outside 1:3"),
            ("logical :: x(3)", "maxval(x)", "numeric arrays"),
        ] {
            let source = format!("program main\nimplicit none\n{decls}\nprint *, {expression}\nend program main");
            let tokens = crate::lexer::lex(&source);
            let p = crate::parser::parse_with_src(&source, &tokens, "reductions.f90").unwrap();
            let error = super::resolve(crate::parameters::resolve(p).unwrap()).unwrap_err();
            assert!(format!("{error:#}").contains(message), "{error:#}");
        }
    }
}

// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Runtime CHARACTER arrays use the ordinary heap descriptor, with integer
//! character codes as backing storage. Shape/section lowering has already kept
//! the element dimensions and performed bounds checks before this storage pass.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec};
use anyhow::{Context, Result};
use std::collections::HashMap;

fn int(n: usize) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
type Arrays = HashMap<String, usize>;

/// The storage and extent of a whole runtime CHARACTER array view.
fn view(e: &Expr, arrays: &Arrays) -> Option<(String, Expr)> {
    match e {
        Expr::Call(tag, a) if tag == "@packed_view" && a.len() == 2 => match &a[0] {
            Expr::Ident(n) if arrays.contains_key(n) => Some((n.clone(), a[1].clone())),
            _ => None,
        },
        _ => None,
    }
}

struct Lower {
    serial: usize,
    widths: HashMap<String,Expr>,
}
impl Lower {
    fn capture(&mut self, kind: TypeSpec, value: Expr, out: &mut Vec<Stmt>) -> String {
        self.serial += 1;
        let n = format!("@character_heap_value_{}", self.serial);
        out.push(Stmt::VarDecl {
            kind,
            names: vec![n.clone()],
        });
        out.push(Stmt::Assign {
            name: n.clone(),
            value,
        });
        n
    }
    fn offset(&mut self, index: Expr, width: Expr, out: &mut Vec<Stmt>) -> Expr {
        let n = self.capture(TypeSpec::Integer(Some(8)), index, out);
        bin(
            BinOp::Mul,
            bin(BinOp::Sub, id(&n), int(1)),
            width,
        )
    }
    fn logical_width(&self,n:&str,capacity:usize)->Expr {self.widths.get(n).cloned().unwrap_or(int(capacity))}
    fn stride(&self,n:&str,capacity:usize)->Expr {Expr::Call("max".into(),vec![self.logical_width(n,capacity),int(1)])}
    fn value(&self,n:&str,temp:&str,capacity:usize)->Expr {
        if self.widths.contains_key(n) {Expr::Call("@dynamic_substring".into(),vec![id(temp),int(0),self.logical_width(n,capacity),int(capacity)])} else {id(temp)}
    }
    fn expression(&mut self, e: &mut Expr, arrays: &Arrays, out: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Index(n, args) | Expr::Call(n, args) if arrays.contains_key(n) => {
                let width = arrays[n];
                let [index] = args.as_mut_slice() else {
                    anyhow::bail!("heap CHARACTER element requires a linear index");
                };
                self.expression(index, arrays, out)?;
                let offset = self.offset(index.clone(), self.stride(n,width), out);
                let temp = self.capture(
                    TypeSpec::Character(Some(width)),
                    Expr::Str(String::new()),
                    out,
                );
                let position = self.capture(TypeSpec::Integer(Some(8)), int(0), out);
                out.push(Stmt::Do {
                    var: position.clone(),
                    start: int(0),
                    end: bin(BinOp::Sub,self.logical_width(n,width),int(1)),
                    body: vec![Stmt::CallSub {
                        name: "@character_code_store".into(),
                        args: vec![
                            id(&temp),
                            id(&position),
                            Expr::Index(
                                n.clone(),
                                vec![bin(
                                    BinOp::Add,
                                    offset,
                                    bin(BinOp::Add, id(&position), int(1)),
                                )],
                            ),
                        ],
                    }],
                });
                *e = self.value(n,&temp,width);
            }
            // An inquiry of a whole runtime CHARACTER array, as LEN_TRIM(s),
            // gives a runtime INTEGER array of the inquiries of its elements.
            Expr::Call(n, args)
                if matches!(n.as_str(), "len_trim" | "index" | "scan" | "verify" | "ichar" | "iachar")
                    && args.iter().any(|a| view(a, arrays).is_some()) =>
            {
                let length = args.iter().find_map(|a| view(a, arrays)).unwrap().1;
                self.serial += 1;
                let index = format!("@character_heap_index_{}", self.serial);
                let values = format!("@character_heap_inquiry_{}", self.serial);
                let mut reads = vec![];
                for arg in args.iter_mut() {
                    let Some((array, extent)) = view(arg, arrays) else {
                        self.expression(arg, arrays, out)?;
                        continue;
                    };
                    out.push(Stmt::If {
                        cond: bin(BinOp::Ne, extent, length.clone()),
                        then_body: vec![Stmt::CallSub {
                            name: "@error_stop".into(),
                            args: vec![Expr::Str(format!("nonconforming CHARACTER array arguments to `{n}`"))],
                        }],
                        else_body: None,
                    });
                    let width = arrays[&array];
                    let element = self.capture(TypeSpec::Character(Some(width)), Expr::Str(String::new()), out);
                    let position = self.capture(TypeSpec::Integer(Some(8)), int(0), out);
                    let offset = bin(BinOp::Mul, bin(BinOp::Sub, id(&index), int(1)), self.stride(&array,width));
                    reads.push(Stmt::Do {
                        var: position.clone(),
                        start: int(0),
                        end: bin(BinOp::Sub,self.logical_width(&array,width),int(1)),
                        body: vec![Stmt::CallSub {
                            name: "@character_code_store".into(),
                            args: vec![
                                id(&element),
                                id(&position),
                                Expr::Index(array.clone(), vec![bin(BinOp::Add, offset, bin(BinOp::Add, id(&position), int(1)))]),
                            ],
                        }],
                    });
                    *arg = self.value(&array,&element,width);
                }
                crate::heap::temporary(TypeSpec::Integer(None), values.clone(), length.clone(), out);
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![index.clone()],
                });
                reads.push(Stmt::AssignIndex {
                    name: values.clone(),
                    indices: vec![id(&index)],
                    value: e.clone(),
                });
                out.push(Stmt::Do {
                    var: index,
                    start: int(1),
                    end: length.clone(),
                    body: reads,
                });
                *e = Expr::Call("@packed_view".into(), vec![id(&values), length]);
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for arg in args {
                    self.expression(arg, arrays, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, arrays, out)?;
                self.expression(b, arrays, out)?;
            }
            Expr::Un(_, a) => self.expression(a, arrays, out)?,
            _ => {}
        }
        Ok(())
    }
    /// A READ item that is a runtime CHARACTER array, or an element of one,
    /// becomes `@character_heap_element(array, index, capacity, length,
    /// stride)` elements, which list-directed input reads as CHARACTER values
    /// and stores as character codes. A whole array becomes an implied DO.
    fn read_item(&mut self, item: &mut Expr, arrays: &Arrays, out: &mut Vec<Stmt>) -> Result<()> {
        let element = |s: &Self, n: &str, index: Expr| Expr::Call("@character_heap_element".into(), vec![
            id(n), index, int(arrays[n]), s.logical_width(n, arrays[n]), s.stride(n, arrays[n])]);
        match item {
            Expr::Call(tag, a) if tag == "@packed_view" && a.len() == 2 && matches!(&a[0], Expr::Ident(n) if arrays.contains_key(n)) => {
                let Expr::Ident(n) = a[0].clone() else { unreachable!() };
                let extent = a[1].clone();
                self.serial += 1;
                let index = format!("@character_heap_read_{}", self.serial);
                out.push(Stmt::VarDecl { kind: TypeSpec::Integer(None), names: vec![index.clone()] });
                *item = Expr::Call("@input_implied_do".into(), vec![id(&index), int(1), extent, int(1), element(self, &n, id(&index))]);
            }
            Expr::Index(n, a) | Expr::Call(n, a) if arrays.contains_key(n.as_str()) => {
                let [index] = a.as_mut_slice() else { anyhow::bail!("heap CHARACTER element requires a linear index") };
                self.expression(index, arrays, out)?;
                *item = element(self, &n.clone(), index.clone());
            }
            Expr::Call(tag, a) if (tag == "@input_implied_do" || tag == "@output_implied_do") && a.len() > 4 => {
                for e in &mut a[4..] { self.read_item(e, arrays, out)?; }
            }
            e => self.expression(e, arrays, out)?,
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &Arrays) -> Result<()> {
        let saved_widths=self.widths.clone();
        let mut arrays = inherited.clone();
        let mut shapes = HashMap::new();
        crate::allocatables::metadata(body, &mut shapes);
        for stmt in body.iter() {
            match stmt {
                Stmt::ArrayDecl {
                    name,
                    kind: TypeSpec::Character(width),
                    ..
                } if shapes.contains_key(&format!("@heap:{name}")) => {
                    arrays.insert(name.clone(), width.unwrap_or(1));
                    if let Some((_,values))=shapes.get(&format!("@character_spec_length:{name}")) {
                        if let Some(value)=values.first(){self.widths.insert(name.clone(),value.clone());}
                    }
                }
                Stmt::ArrayDecl { name, .. } => {
                    arrays.remove(name);
                }
                Stmt::VarDecl { names, .. } => {
                    for n in names {
                        arrays.remove(n);
                    }
                }
                _ => {}
            }
        }
        let mut result = vec![];
        for mut stmt in std::mem::take(body) {
            let mut prefix = vec![];
            match &mut stmt {
                Stmt::CallSub{name,args} if name==crate::initialized::TRANSPORT => {
                    let element=|e:&Expr|match e {
                        Expr::Index(n,a)|Expr::Call(n,a) if arrays.contains_key(n) && a.len()==1=>Some((n.clone(),a[0].clone())),
                        _=>None,
                    };
                    let target=args.first().and_then(element);
                    let source=args.get(1).and_then(element);
                    if let (Some((to,ti)),Some((from,si)))=(target.clone(),source.clone()) {
                        let width=self.stride(&to,arrays[&to]);
                        let to_offset=self.offset(ti,width.clone(),&mut prefix);
                        let from_offset=self.offset(si,self.stride(&from,arrays[&from]),&mut prefix);
                        self.serial+=1;
                        let index=format!("@character_transport_{}",self.serial);
                        prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                        prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:width,body:vec![Stmt::CallSub{name:name.clone(),args:vec![
                            Expr::Index(to,vec![bin(BinOp::Add,to_offset,id(&index))]),
                            Expr::Index(from,vec![bin(BinOp::Add,from_offset,id(&index))])]}]});
                        result.extend(prefix);continue;
                    }
                    if let Some((to,ti))=target {
                        let capacity=arrays[&to];
                        let width=self.logical_width(&to,capacity);
                        let offset=self.offset(ti,self.stride(&to,capacity),&mut prefix);
                        self.serial+=1;
                        let temp=format!("@character_transport_scalar_{}",self.serial);
                        prefix.push(Stmt::VarDecl{kind:TypeSpec::Character(Some(capacity)),names:vec![temp.clone()]});
                        prefix.push(Stmt::CallSub{name:name.clone(),args:vec![id(&temp),args[1].clone()]});
                        prefix.push(Stmt::CallSub{name:crate::initialized::TRANSPORT_TO_CODES.into(),args:vec![id(&to),offset,width,id(&temp),int(capacity)]});
                        result.extend(prefix);continue;
                    }
                    if let Some((from,si))=source {
                        let capacity=arrays[&from];
                        let width=self.logical_width(&from,capacity);
                        let offset=self.offset(si,self.stride(&from,capacity),&mut prefix);
                        self.serial+=1;
                        let temp=format!("@character_transport_scalar_{}",self.serial);
                        prefix.push(Stmt::VarDecl{kind:TypeSpec::Character(Some(capacity)),names:vec![temp.clone()]});
                        prefix.push(Stmt::CallSub{name:crate::initialized::TRANSPORT_FROM_CODES.into(),args:vec![id(&temp),offset,width,id(&from),int(capacity)]});
                        prefix.push(Stmt::CallSub{name:name.clone(),args:vec![args[0].clone(),id(&temp)]});
                        result.extend(prefix);continue;
                    }
                    result.push(stmt);continue;
                }
                Stmt::CallSub{name,args} if name=="@scalar_character_from_codes" => {
                    fn storage(e:&Expr)->Option<String> {match e {
                        Expr::Ident(n)=>Some(n.clone()),
                        Expr::Call(tag,args) if tag=="@packed_view"=>args.first().and_then(storage),
                        _=>None,
                    }}
                    let [target,source,length]=args.as_slice() else {anyhow::bail!("invalid scalar CHARACTER result buffer");};
                    let target=storage(target).with_context(||format!("invalid CHARACTER result target: {target:?}"))?;
                    let source=storage(source).with_context(||format!("invalid CHARACTER result codes: {source:?}"))?;
                    anyhow::ensure!(arrays.contains_key(&target),"CHARACTER result requires owned scalar storage");
                    self.serial+=1;let index=format!("@character_result_decode_{}",self.serial);
                    prefix.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                    prefix.push(Stmt::Do{var:index.clone(),start:int(1),end:Expr::Call("max".into(),vec![length.clone(),int(1)]),body:vec![Stmt::If {
                        cond:bin(BinOp::Le,id(&index),length.clone()),
                        then_body:vec![Stmt::AssignIndex{name:target.clone(),indices:vec![id(&index)],value:Expr::Index(source.clone(),vec![id(&index)])}],
                        else_body:Some(vec![Stmt::AssignIndex{name:target.clone(),indices:vec![id(&index)],value:int(32)}])
                    }]});
                    result.extend(prefix);continue;
                }
                Stmt::ArrayDecl { name, kind, .. } if arrays.contains_key(name) => {
                    *kind = TypeSpec::Integer(Some(8));
                    prefix.push(Stmt::DummyIntent {
                        name: name.clone(),
                        intent: format!("@character_element_width:{}", arrays[name].max(1)),
                    });
                    prefix.push(Stmt::VarDecl {
                        kind: TypeSpec::Integer(Some(8)),
                        names: vec![format!("@character_heap_bytes${name}")],
                    });
                    prefix.push(Stmt::Assign {
                        name: format!("@character_heap_bytes${name}"),
                        value: int(0),
                    });
                }
                Stmt::DummyIntent { name, intent }
                    if arrays.contains_key(name) && intent.starts_with("@bounded:") =>
                {
                    let (_, flag) = intent
                        .strip_prefix("@bounded:")
                        .unwrap()
                        .split_once(':')
                        .context("invalid CHARACTER descriptor")?;
                    *intent = format!("@bounded:@character_heap_bytes${name}:{flag}");
                }
                // Record descriptors own the underlying character-code buffers.
                // Their length counts codes, while public shape metadata still
                // counts Fortran character elements.
                Stmt::CallSub { name, args }
                    if matches!(name.as_str(), "@heap_record_load" | "@heap_record_store" | "@heap_record_borrow" | "@heap_record_unborrow")
                        && matches!(args.first(),Some(Expr::Ident(n)) if arrays.contains_key(n)) =>
                {
                    let Expr::Ident(n) = &args[0] else { unreachable!() };
                    let bytes = id(&format!("@character_heap_bytes${n}"));
                    let width = self.stride(n, arrays[n]);
                    let length = std::mem::replace(&mut args[2], bytes.clone());
                    // Keep both lengths in the owned descriptor: code-buffer
                    // bytes for storage and character elements for inquiries.
                    // A borrowed view must see both change through an alias.
                    args.push(length.clone());
                    let load = name == "@heap_record_load" || name == "@heap_record_borrow";
                    if !load && name != "@heap_record_unborrow" {
                        let Expr::Ident(bytes_name) = &bytes else { unreachable!() };
                        prefix.push(Stmt::Assign {
                            name: bytes_name.clone(),
                            value: bin(BinOp::Mul, length.clone(), width.clone()),
                        });
                    }
                    result.extend(prefix);
                    result.push(stmt);
                    continue;
                }
                Stmt::CallSub { name, args }
                    if matches!(name.as_str(), "@heap_allocate" | "@heap_resize")
                        && matches!(args.first(),Some(Expr::Ident(n)) if arrays.contains_key(n)) =>
                {
                    let Expr::Ident(n) = &args[0] else {
                        unreachable!()
                    };
                    let width = self.stride(n,arrays[n]);
                    let length = args[1].clone();
                    // Keep the public descriptor length in elements; the allocator
                    // and initialization mask count the underlying character codes.
                    let bytes = format!("@character_heap_bytes${n}");
                    args[1] = id(&bytes);
                    args.push(width.clone());
                    result.extend(prefix);
                    result.push(stmt);
                    let Expr::Ident(length) = length else {
                        anyhow::bail!("CHARACTER allocation requires a length field");
                    };
                    result.push(Stmt::Assign {
                        name: length,
                        value: bin(BinOp::Div, id(&bytes), width),
                    });
                    continue;
                }
                Stmt::CallSub {name,args} if name=="@heap_transfer_buffers" => {
                    if let [Expr::Ident(to),Expr::Ident(from)]=args.as_slice() {
                        if arrays.contains_key(to) {
                            result.push(Stmt::Assign{name:format!("@character_heap_bytes${to}"),value:id(&format!("@character_heap_bytes${from}"))});
                            result.push(Stmt::Assign{name:format!("@character_heap_bytes${from}"),value:int(0)});
                        }
                    }
                }
                Stmt::CallSub { name, .. }
                    if matches!(
                        name.as_str(),
                        "@heap_bind" | "@heap_free" | "@heap_drop" | "@undefine"
                    ) => {}
                Stmt::CallSub { name, args }
                    if name == "@module_bind"
                        && matches!(args.first(),Some(Expr::Ident(n)) if arrays.contains_key(n)) =>
                {
                    let [Expr::Ident(n), Expr::Str(key), Expr::Str(encoded)] = args.as_mut_slice()
                    else {
                        anyhow::bail!("invalid CHARACTER module binding");
                    };
                    let mut storage: crate::modules::Storage = serde_json::from_str(encoded)?;
                    storage.kind = TypeSpec::Integer(Some(8));
                    storage.values = vec![int(0)];
                    *encoded = serde_json::to_string(&storage)?;
                    let bytes = crate::modules::Storage {
                        kind: TypeSpec::Integer(Some(8)),
                        values: vec![int(0)],
                        initialized: true,
                        rank: 0,
                        deferred: false,
                    };
                    prefix.push(Stmt::CallSub {
                        name: "@module_bind".into(),
                        args: vec![
                            id(&format!("@character_heap_bytes${n}")),
                            Expr::Str(format!("{key}.character_codes_length")),
                            Expr::Str(serde_json::to_string(&bytes)?),
                        ],
                    });
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if arrays.contains_key(name) => {
                    let width = arrays[name];
                    let [index] = indices.as_mut_slice() else {
                        anyhow::bail!("heap CHARACTER assignment requires a linear index");
                    };
                    self.expression(index, &arrays, &mut prefix)?;
                    let offset = self.offset(index.clone(), self.stride(name,width), &mut prefix);
                    self.expression(value, &arrays, &mut prefix)?;
                    let temp =
                        self.capture(TypeSpec::Character(Some(width)), value.clone(), &mut prefix);
                    let position = self.capture(TypeSpec::Integer(Some(8)), int(0), &mut prefix);
                    let code = self.capture(TypeSpec::Integer(Some(8)), int(0), &mut prefix);
                    prefix.push(Stmt::Do {
                        var: position.clone(),
                        start: int(0),
                        end: bin(BinOp::Sub,self.logical_width(name,width),int(1)),
                        body: vec![
                            Stmt::CallSub {
                                name: "@internal_read_byte".into(),
                                args: vec![id(&code), id(&temp), id(&position), int(width)],
                            },
                            Stmt::AssignIndex {
                                name: name.clone(),
                                indices: vec![bin(
                                    BinOp::Add,
                                    offset,
                                    bin(BinOp::Add, id(&position), int(1)),
                                )],
                                value: id(&code),
                            },
                        ],
                    });
                    result.extend(prefix);
                    continue;
                }
                Stmt::Assign { value, .. }
                | Stmt::AssignIndex { value, .. }
                | Stmt::Return(Some(value)) => self.expression(value, &arrays, &mut prefix)?,
                // List-directed READ: unit, control, status, then the items.
                // Unformatted READ and WRITE also have a position.
                Stmt::CallSub { name, args } if matches!(name.as_str(), "@internal_read" | "@unformatted_read" | "@unformatted_write") => {
                    let first = if name == "@internal_read" { 3 } else { 4 };
                    let head = first.min(args.len());
                    for e in &mut args[..head] {
                        self.expression(e, &arrays, &mut prefix)?;
                    }
                    for item in args.iter_mut().skip(first) {
                        self.read_item(item, &arrays, &mut prefix)?;
                    }
                }
                Stmt::Print { items } | Stmt::CallSub { args: items, .. } => {
                    for e in items {
                        self.expression(e, &arrays, &mut prefix)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &arrays, &mut prefix)?;
                    self.body(then_body, &arrays)?;
                    if let Some(b) = else_body {
                        self.body(b, &arrays)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &arrays, &mut prefix)?;
                    self.expression(end, &arrays, &mut prefix)?;
                    self.body(body, &arrays)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &arrays, &mut prefix)?;
                    self.body(body, &arrays)?;
                    if !prefix.is_empty() {
                        prefix.push(Stmt::If {
                            cond: Expr::Un(crate::ast::UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        prefix.append(body);
                        *body = std::mem::take(&mut prefix);
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::Block { body } | Stmt::Module { body, .. } => self.body(body, &arrays)?,
                Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => {
                    self.body(body, &Arrays::new())?
                }
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &arrays, &mut prefix)?;
                    for c in cases {
                        self.body(&mut c.body, &arrays)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &arrays)?;
                    }
                }
                _ => {}
            }
            result.extend(prefix);
            result.push(stmt);
        }
        *body = result;
        self.widths=saved_widths;
        Ok(())
    }
}
pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    Lower { serial: 0, widths: HashMap::new() }
        .body(&mut p.body, &Arrays::new())
        .context("lowering runtime CHARACTER array storage")?;
    Ok(p)
}

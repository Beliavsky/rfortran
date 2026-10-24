// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Numeric pointer descriptors. The storage address, initialization address,
//! origin and each dimension's bounds/stride are captured at association time.
//! Native code uses addresses; the interpreter uses checked storage handles.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

pub const MAX_RANK: usize = 15;
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn add(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Add, a, b)
}
fn sub(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Sub, a, b)
}
fn mul(a: Expr, b: Expr) -> Expr {
    bin(BinOp::Mul, a, b)
}
fn call(n: &str, a: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), a)
}
fn assign(n: &str, e: Expr) -> Stmt {
    Stmt::Assign {
        name: n.into(),
        value: e,
    }
}
fn require(cond: Expr, message: String) -> Stmt {
    Stmt::If {
        cond: Expr::Un(UnOp::Not, Box::new(cond)),
        then_body: vec![Stmt::CallSub {
            name: "@error_stop".into(),
            args: vec![Expr::Str(message)],
        }],
        else_body: None,
    }
}
fn product(xs: impl Iterator<Item = Expr>) -> Expr {
    // Keep generated expression depth logarithmic for high-rank descriptors.
    let mut xs: Vec<_> = xs.collect();
    if xs.is_empty() {
        return int(1);
    }
    while xs.len() > 1 {
        xs = xs
            .chunks(2)
            .map(|pair| {
                if pair.len() == 2 {
                    mul(pair[0].clone(), pair[1].clone())
                } else {
                    pair[0].clone()
                }
            })
            .collect();
    }
    xs.pop().unwrap()
}
pub(crate) fn same_kind(a: &TypeSpec, b: &TypeSpec) -> bool {
    a == b
        || matches!((a,b),(TypeSpec::Integer(a),TypeSpec::Integer(b)) if a.unwrap_or(4)==b.unwrap_or(4))
}
fn numeric(k: &TypeSpec) -> bool {
    matches!(
        k,
        TypeSpec::Integer(None | Some(1 | 2 | 4 | 8))
            | TypeSpec::Real
            | TypeSpec::DoublePrecision
            | TypeSpec::Logical
    )
}
fn promote(a: TypeSpec, b: TypeSpec) -> TypeSpec {
    if matches!(a, TypeSpec::DoublePrecision) || matches!(b, TypeSpec::DoublePrecision) {
        TypeSpec::DoublePrecision
    } else if matches!(a, TypeSpec::Real) || matches!(b, TypeSpec::Real) {
        TypeSpec::Real
    } else if let (TypeSpec::Integer(a), TypeSpec::Integer(b)) = (&a, &b) {
        TypeSpec::Integer(Some(a.unwrap_or(4).max(b.unwrap_or(4))))
    } else {
        a
    }
}
fn suffix(k: &TypeSpec) -> &'static str {
    match k {
        TypeSpec::Real => "r4",
        TypeSpec::DoublePrecision => "r8",
        TypeSpec::Logical => "l",
        _ => "i",
    }
}
fn children(s: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match s {
        Stmt::Block { body }
        | Stmt::Do { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Function { body, .. }
        | Stmt::Subroutine { body, .. }
        | Stmt::Module { body, .. } => vec![body],
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            let mut v = vec![then_body];
            v.extend(else_body.iter_mut());
            v
        }
        Stmt::SelectCase { cases, default, .. } => {
            let mut v: Vec<_> = cases.iter_mut().map(|c| &mut c.body).collect();
            v.extend(default.iter_mut());
            v
        }
        _ => vec![],
    }
}

pub fn prepare(mut p: Program) -> Result<Program> {
    fn has_pointer(body: &mut Vec<Stmt>) -> bool {
        body.iter_mut().any(|s| {
            matches!(s,Stmt::DummyIntent{intent,..} if intent=="pointer")
                || children(s).into_iter().any(has_pointer)
        })
    }
    fn walk(body: &mut Vec<Stmt>, serial: &mut usize) -> Result<()> {
        let mut extra = vec![];
        for s in body.iter_mut() {
            if let Stmt::ArrayDecl { name, dims, .. } = s {
                let lower = dims
                    .iter()
                    .map(|d| match d {
                        Expr::Call(n, a) if n == "@declared_bounds" => {
                            crate::parameters::integer_constant(&a[0])
                        }
                        _ => Ok(1),
                    })
                    .collect::<Result<Vec<_>>>()?;
                extra.push(Stmt::DummyIntent {
                    name: name.clone(),
                    intent: format!(
                        "@pointer_bounds:{}",
                        lower
                            .iter()
                            .map(i64::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                });
            }
            if let Stmt::Block { body } = s {
                crate::subroutines::rename_allocation_block(body, serial)?;
            }
            for b in children(s) {
                walk(b, serial)?;
            }
        }
        body.extend(extra);
        Ok(())
    }
    if has_pointer(&mut p.body) {
        walk(&mut p.body, &mut 0)?;
    }
    Ok(p)
}

#[derive(Clone)]
struct Variable {
    kind: TypeSpec,
    dims: Vec<i64>,
    lower: Vec<i64>,
    level: usize,
    target: bool,
    allocatable: bool,
    /// The variable holding the current extent of a rank-one ALLOCATABLE,
    /// whose storage has its largest extent.
    length: Option<String>,
    /// Logical dimensions of descriptor-backed storage, before flattening.
    active_shape: Option<Vec<String>>,
    heap: bool,
}
#[derive(Clone)]
struct Axis {
    lower: Expr,
    extent: Expr,
    stride: Expr,
}
#[derive(Clone)]
struct Descriptor {
    data: String,
    flags: String,
    name: String,
    state: String,
    origin: String,
    axes: Vec<Axis>,
    /// For borrowed storage whose last extent is assumed, the number of
    /// elements from `data`, which every element offset must stay below.
    limit: Option<String>,
}
impl Descriptor {
    fn fields(&self) -> Vec<String> {
        let mut n = vec![
            self.data.clone(),
            self.flags.clone(),
            self.name.clone(),
            self.state.clone(),
            self.origin.clone(),
        ];
        for a in &self.axes {
            for e in [&a.lower, &a.extent, &a.stride] {
                let Expr::Ident(s) = e else { unreachable!() };
                n.push(s.clone());
            }
        }
        n
    }
}
#[derive(Clone)]
struct Pointer {
    kind: TypeSpec,
    level: usize,
    desc: Descriptor,
    targets: HashSet<String>,
    record_targets: HashSet<String>,
    borrowed: bool,
}
#[derive(Clone)]
struct View {
    data: Expr,
    flags: Expr,
    name: Expr,
    origin: Expr,
    axes: Vec<Axis>,
    kind: TypeSpec,
    capacity: i64,
    limit: Option<String>,
}
impl View {
    fn count(&self) -> Expr {
        product(self.axes.iter().map(|a| a.extent.clone()))
    }
}
struct ArrayValue {
    shape: Vec<Expr>,
    kind: TypeSpec,
    capacity: i64,
    value: Expr,
    reads: Vec<Stmt>,
}
struct Lower {
    vars: HashMap<String, Variable>,
    pointers: HashMap<String, Pointer>,
    functions: HashSet<String>,
    serial: usize,
    capacity: i64,
    name_capacity: usize,
    /// Scalar temporaries free for reuse, by type. A statement's
    /// temporaries are live until the statement, nested bodies included,
    /// is lowered, so a unit declares only as many as one statement needs.
    free: HashMap<String, Vec<String>>,
    live: Vec<(String, String)>,
    /// The declarations of the temporaries, for the start of the unit.
    temporaries: Vec<Stmt>,
}
impl Lower {
    fn release(&mut self, base: usize) {
        for (kind, name) in self.live.drain(base..) {
            self.free.entry(kind).or_default().push(name);
        }
    }
    fn fresh(&mut self) -> String {
        self.serial += 1;
        let name = format!("@pointer_{}", self.serial);
        self.name_capacity = self.name_capacity.max(name.len());
        name
    }
    fn temp(&mut self, kind: TypeSpec, value: Expr, out: &mut Vec<Stmt>) -> Expr {
        let key = format!("{kind:?}");
        let n = match self.free.get_mut(&key).and_then(Vec::pop) {
            Some(n) => n,
            None => {
                let n = self.fresh();
                self.temporaries.push(Stmt::VarDecl { kind, names: vec![n.clone()] });
                n
            }
        };
        self.live.push((key, n.clone()));
        out.push(assign(&n, value));
        id(&n)
    }
    fn integer(&mut self, e: Expr, out: &mut Vec<Stmt>) -> Expr {
        self.temp(TypeSpec::Integer(Some(8)), e, out)
    }
    fn descriptor(&mut self, rank: usize) -> Descriptor {
        Descriptor {
            data: self.fresh(),
            flags: self.fresh(),
            name: self.fresh(),
            state: self.fresh(),
            origin: self.fresh(),
            axes: (0..rank)
                .map(|_| Axis {
                    lower: id(&self.fresh()),
                    extent: id(&self.fresh()),
                    stride: id(&self.fresh()),
                })
                .collect(),
            limit: None,
        }
    }
    fn initialize(&self, d: &Descriptor, out: &mut Vec<Stmt>) {
        let fields = d.fields();
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: fields.clone(),
        });
        for n in fields {
            out.push(assign(&n, int(0)));
        }
    }
    fn declarations(&mut self, body: &mut Vec<Stmt>, level: usize) -> Result<()> {
        let attrs: Vec<_> = body
            .iter()
            .filter_map(|s| {
                if let Stmt::DummyIntent { name, intent } = s {
                    Some((name.clone(), intent.clone()))
                } else {
                    None
                }
            })
            .collect();
        for s in body.iter() {
            let (kind, names, dims) = match s {
                Stmt::VarDecl { kind, names } => (kind, names.clone(), vec![]),
                Stmt::ArrayDecl { kind, name, dims } => (kind, vec![name.clone()], dims.clone()),
                _ => continue,
            };
            for name in names {
                let has = |a: &str| attrs.iter().any(|(n, i)| n == &name && i == a);
                if has("pointer") {
                    if has("allocatable") || has("target") {
                        bail!("POINTER cannot also have ALLOCATABLE or TARGET attribute");
                    }
                    if !numeric(kind) {
                        bail!("POINTER association currently supports numeric and LOGICAL types through 64 bits");
                    }
                    if dims.len() > MAX_RANK
                        || dims
                            .iter()
                            .any(|d| !matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
                    {
                        bail!("POINTER arrays require deferred shape with rank at most {MAX_RANK}");
                    }
                    let fields = attrs.iter().find_map(|(n,i)| (n==&name).then(||i.strip_prefix("@borrowed_descriptor:")).flatten());
                    let borrowed = fields.is_some();
                    let desc = if let Some(fields) = fields {
                        let fields = fields.split(',').collect::<Vec<_>>();
                        if fields.len()!=5+3*dims.len() {bail!("invalid borrowed array descriptor");}
                        self.name_capacity=self.name_capacity.max(256);
                        let limit = attrs.iter().find_map(|(n,i)| (n==&name).then(||i.strip_prefix("@borrowed_limit:")).flatten()).map(String::from);
                        Descriptor {data:fields[0].into(),flags:fields[1].into(),name:fields[2].into(),state:fields[3].into(),origin:fields[4].into(),
                            axes:fields[5..].chunks_exact(3).map(|a|Axis{lower:id(a[0]),extent:id(a[1]),stride:id(a[2])}).collect(),limit}
                    } else {self.descriptor(dims.len())};
                    self.pointers.insert(
                        name.clone(),
                        Pointer {
                            kind: kind.clone(),
                            level,
                            desc,
                            targets: HashSet::new(),
                            record_targets: HashSet::new(),
                            borrowed,
                        },
                    );
                } else {
                    let extents = dims
                        .iter()
                        .map(crate::parameters::integer_constant)
                        .collect::<Result<Vec<_>>>();
                    if let Ok(dims) = extents {
                        if dims.len() > MAX_RANK {
                            bail!("array rank exceeds {MAX_RANK}");
                        }
                        let lower = attrs
                            .iter()
                            .find_map(|(n, i)| {
                                if n == &name {
                                    i.strip_prefix("@pointer_bounds:")
                                } else {
                                    None
                                }
                            })
                            .map(|s| {
                                s.split(',')
                                    .filter(|s| !s.is_empty())
                                    .map(str::parse)
                                    .collect::<std::result::Result<Vec<i64>, _>>()
                            })
                            .transpose()?
                            .unwrap_or_else(|| vec![1; dims.len()]);
                        let count = dims.iter().try_fold(1i64, |n, d| {
                            n.checked_mul((*d).max(0)).context("array size overflow")
                        })?;
                        self.capacity = self.capacity.max(count);
                        self.name_capacity = self.name_capacity.max(name.len());
                        self.vars.insert(
                            name.clone(),
                            Variable {
                                kind: kind.clone(),
                                dims,
                                lower,
                                level,
                                target: has("target"),
                                allocatable: has("allocatable")
                                    || attrs
                                        .iter()
                                        .any(|(n, i)| n == &name && i.starts_with("@bounded:")),
                                length: attrs.iter().find_map(|(n, i)| {
                                    (n == &name).then(|| i.strip_prefix("@bounded:")).flatten()
                                        .and_then(|rest| rest.split_once(':')).map(|(length, _)| length.to_string())
                                }),
                                active_shape: attrs.iter().find_map(|(n,i)| (n==&name).then(||i.strip_prefix("@bounded_shape:")).flatten()).map(|s|s.split(':').map(str::to_owned).collect()),
                                heap: has("@heap"),
                            },
                        );
                    }
                }
            }
        }
        for s in body {
            if matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                continue;
            }
            let next = level + usize::from(matches!(s, Stmt::Block { .. }));
            for b in children(s) {
                self.declarations(b, next)?;
            }
        }
        Ok(())
    }
    fn targets(&mut self, body: &mut Vec<Stmt>) -> Result<()> {
        fn collect(body: &mut Vec<Stmt>, edges: &mut Vec<(String, String)>, owned: &mut Vec<(String,String,usize)>) {
            for s in body {
                if let Stmt::CallSub { name, args } = s {
                    if name=="@record_pointer_associate" {
                        if let [Expr::Ident(p),handle,Expr::Str(label)]=args.as_slice() {
                            if let Some(target)=root(handle) {owned.push((p.clone(),target.into(),label.len()));}
                        }
                    }
                    if name == "@pointer_associate" {
                        if let [Expr::Ident(p), e] = args.as_slice() {
                            if let Some(n) = root(e) {
                                if n != "null" {
                                    edges.push((p.clone(), n.into()));
                                }
                            }
                        }
                    }
                }
                if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                    for b in children(s) {
                        collect(b, edges, owned);
                    }
                }
            }
        }
        let mut edges = vec![];
        let mut owned=vec![];
        collect(body, &mut edges, &mut owned);
        for (pointer,target,label_len) in owned {
            self.name_capacity=self.name_capacity.max(label_len);
            self.pointers.get_mut(&pointer).context("owned target requires a pointer")?.record_targets.insert(target);
        }
        for _ in 0..=self.pointers.len() {
            for (p, t) in &edges {
                let targets = if let Some(q) = self.pointers.get(t) {
                    q.targets.clone()
                } else {
                    HashSet::from([t.clone()])
                };
                let record_targets=self.pointers.get(t).map(|q|q.record_targets.clone()).unwrap_or_default();
                let ptr = self
                    .pointers
                    .get_mut(p)
                    .context("pointer association requires a POINTER variable")?;
                ptr.targets.extend(targets);
                ptr.record_targets.extend(record_targets);
            }
        }
        for (p, t) in edges {
            let ptr = &self.pointers[&p];
            for n in if self.pointers.contains_key(&t) {
                self.pointers[&t].targets.clone()
            } else {
                HashSet::from([t])
            } {
                let v = self
                    .vars
                    .get(&n)
                    .context("undeclared or dynamically sized pointer association target")?;
                if !v.target {
                    bail!("pointer association target `{n}` requires TARGET attribute");
                }
                if v.allocatable {
                    bail!("pointer association to ALLOCATABLE storage is not yet supported");
                }
                if v.level > ptr.level {
                    bail!("pointer `{p}` cannot outlive local TARGET `{n}`");
                }
                if !same_kind(&ptr.kind, &v.kind) {
                    bail!("type or kind mismatch in pointer association for `{p}`");
                }
            }
        }
        for (name,pointer) in &self.pointers {
            for target in &pointer.record_targets {
                let level=self.vars.get(target).map(|v|v.level).or_else(||self.pointers.get(target).filter(|p|!p.borrowed).map(|p|p.level));
                if level.is_some_and(|level|level>pointer.level) {bail!("pointer `{name}` cannot outlive local owned TARGET `{target}`");}
            }
        }
        Ok(())
    }
    fn guard(&self, p: &str) -> Stmt {
        require(
            bin(BinOp::Ne, id(&self.pointers[p].desc.state), int(0)),
            format!("access through unassociated pointer `{p}`"),
        )
    }
    fn pointer_view(&self, p: &str) -> View {
        let ptr = &self.pointers[p];
        let d = &ptr.desc;
        View {
            data: id(&d.data),
            flags: id(&d.flags),
            name: id(&d.name),
            origin: id(&d.origin),
            axes: d.axes.clone(),
            limit: d.limit.clone(),
            kind: ptr.kind.clone(),
            capacity: ptr
                .targets
                .iter()
                .filter_map(|n| self.vars.get(n))
                .map(|v| v.dims.iter().product::<i64>())
                .max()
                .unwrap_or(0),
        }
    }
    fn storage_view(
        &mut self,
        n: &str,
        original_bounds: bool,
        out: &mut Vec<Stmt>,
    ) -> Result<View> {
        let v = self
            .vars
            .get(n)
            .context("pointer view requires fixed target storage")?
            .clone();
        if !numeric(&v.kind) {
            bail!("pointer array expressions require numeric or LOGICAL storage");
        }
        let d = self.descriptor(0);
        self.initialize(&d, out);
        out.push(Stmt::CallSub {
            name: "@ptr_bind".into(),
            args: vec![id(&d.data), id(&d.flags), id(&d.name), id(n)],
        });
        let capacity=if v.heap {0}else{v.dims.iter().try_fold(1i64,|n,d|n.checked_mul((*d).max(0)).context("array stride overflow"))?};
        let dims=v.active_shape.as_ref().map(|dims|dims.iter().map(|n|id(n)).collect::<Vec<_>>())
            .unwrap_or_else(||v.dims.iter().map(|d|int(*d)).collect());
        let mut stride = int(1);
        let mut axes = vec![];
        for (i, extent) in dims.iter().enumerate() {
            let extent=match &v.length {Some(length) if dims.len()==1=>id(length),_=>extent.clone()};
            axes.push(Axis {
                lower: int(if original_bounds { v.lower.get(i).copied().unwrap_or(1) } else { 1 }),
                extent: extent.clone(),
                stride: stride.clone(),
            });
            stride = mul(stride,extent);
        }
        Ok(View {
            data: id(&d.data),
            flags: id(&d.flags),
            name: id(&d.name),
            origin: int(0),
            axes,
            kind: v.kind,
            capacity,
            limit: None,
        })
    }
    fn section(
        &mut self,
        mut v: View,
        subs: &[Expr],
        label: &str,
        out: &mut Vec<Stmt>,
    ) -> Result<View> {
        if subs.len() != v.axes.len() {
            bail!("pointer `{label}` requires {} subscripts", v.axes.len());
        }
        let mut axes = vec![];
        let mut origin = v.origin.clone();
        for (dim, (s, a)) in subs.iter().zip(&v.axes).enumerate() {
            let s = if let Expr::Call(n, args) = s {
                if n == "@section_subscript" {
                    &args[0]
                } else {
                    s
                }
            } else {
                s
            };
            if let Expr::Section {
                lower,
                upper,
                stride,
                ..
            } = s
            {
                let mut lo = lower.as_deref().cloned().unwrap_or(a.lower.clone());
                let mut hi = upper
                    .as_deref()
                    .cloned()
                    .unwrap_or(add(a.lower.clone(), sub(a.extent.clone(), int(1))));
                let mut step = stride.as_deref().cloned().unwrap_or(int(1));
                self.expr(&mut lo, out)?;
                self.expr(&mut hi, out)?;
                self.expr(&mut step, out)?;
                let lo = self.integer(lo, out);
                let hi = self.integer(hi, out);
                let step = self.integer(step, out);
                out.push(require(
                    bin(BinOp::Ne, step.clone(), int(0)),
                    "pointer section stride must not be zero".into(),
                ));
                let len = self.fresh();
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![len.clone()],
                });
                out.push(assign(&len, int(0)));
                let valid = bin(
                    BinOp::Or,
                    bin(
                        BinOp::And,
                        bin(BinOp::Gt, step.clone(), int(0)),
                        bin(BinOp::Le, lo.clone(), hi.clone()),
                    ),
                    bin(
                        BinOp::And,
                        bin(BinOp::Lt, step.clone(), int(0)),
                        bin(BinOp::Ge, lo.clone(), hi.clone()),
                    ),
                );
                out.push(Stmt::If {
                    cond: valid,
                    then_body: vec![assign(
                        &len,
                        add(bin(BinOp::Div, sub(hi, lo.clone()), step.clone()), int(1)),
                    )],
                    else_body: None,
                });
                let last = add(lo.clone(), mul(sub(id(&len), int(1)), step.clone()));
                let upper = add(a.lower.clone(), sub(a.extent.clone(), int(1)));
                let bounds = |x: Expr| {
                    bin(
                        BinOp::And,
                        bin(BinOp::Ge, x.clone(), a.lower.clone()),
                        bin(BinOp::Le, x, upper.clone()),
                    )
                };
                out.push(Stmt::If {
                    cond: bin(BinOp::Gt, id(&len), int(0)),
                    then_body: vec![require(
                        bin(BinOp::And, bounds(lo.clone()), bounds(last)),
                        format!(
                            "pointer `{label}` section outside bounds in dimension {}",
                            dim + 1
                        ),
                    )],
                    else_body: None,
                });
                origin = self.integer(
                    add(origin, mul(sub(lo, a.lower.clone()), a.stride.clone())),
                    out,
                );
                axes.push(Axis {
                    lower: int(1),
                    extent: id(&len),
                    stride: mul(a.stride.clone(), step),
                });
            } else {
                if matches!(s,Expr::Call(n,_) if n=="@vector_subscript") {
                    bail!("vector subscripts are not permitted in pointer association");
                }
                let mut e = s.clone();
                self.expr(&mut e, out)?;
                let e = self.integer(e, out);
                let upper = add(a.lower.clone(), sub(a.extent.clone(), int(1)));
                out.push(require(
                    bin(
                        BinOp::And,
                        bin(BinOp::Ge, e.clone(), a.lower.clone()),
                        bin(BinOp::Le, e.clone(), upper),
                    ),
                    format!(
                        "pointer `{label}` subscript outside bounds in dimension {}",
                        dim + 1
                    ),
                ));
                origin = self.integer(
                    add(origin, mul(sub(e, a.lower.clone()), a.stride.clone())),
                    out,
                );
            }
        }
        v.origin = origin;
        v.axes = axes;
        Ok(v)
    }
    fn view(&mut self, e: &Expr, association: bool, out: &mut Vec<Stmt>) -> Result<Option<View>> {
        let Some(n) = root(e) else { return Ok(None) };
        let mut v = if self.pointers.contains_key(n) {
            out.push(self.guard(n));
            self.pointer_view(n)
        } else if self
            .vars
            .get(n)
            .is_some_and(|v| !v.dims.is_empty() || association)
        {
            self.storage_view(n, association, out)?
        } else {
            return Ok(None);
        };
        let subs = match e {
            Expr::Ident(_) => None,
            Expr::Section { .. } => Some(vec![e.clone()]),
            Expr::Call(_, a) | Expr::Index(_, a) => Some(a.clone()),
            _ => None,
        };
        if let Some(s) = subs {
            v = self.section(v, &s, n, out)?;
        }
        Ok(Some(v))
    }
    fn offset(&mut self, v: &View, index: Expr, out: &mut Vec<Stmt>) -> Expr {
        let offset = self.integer(v.origin.clone(), out);
        let ordinal = self.integer(sub(index, int(1)), out);
        let Expr::Ident(offset_name) = &offset else {
            unreachable!()
        };
        let Expr::Ident(ordinal_name) = &ordinal else {
            unreachable!()
        };
        for a in &v.axes {
            let extent = call("max", vec![a.extent.clone(), int(1)]);
            let digit = call("mod", vec![ordinal.clone(), extent.clone()]);
            out.push(assign(
                offset_name,
                add(offset.clone(), mul(digit, a.stride.clone())),
            ));
            out.push(assign(
                ordinal_name,
                bin(BinOp::Div, ordinal.clone(), extent),
            ));
        }
        offset
    }
    /// Stop an access past the elements that borrowed storage has.
    fn check_limit(v: &View, offset: &Expr, out: &mut Vec<Stmt>) {
        if let Some(limit) = &v.limit {
            out.push(require(
                bin(BinOp::Lt, offset.clone(), id(limit)),
                "array element is past the end of its actual argument".into(),
            ));
        }
    }
    fn read(&mut self, v: &View, offset: Expr, out: &mut Vec<Stmt>) -> Expr {
        let offset = self.integer(offset, out);
        Self::check_limit(v, &offset, out);
        out.push(Stmt::CallSub {
            name: "@check_initialized".into(),
            args: vec![
                call("@ptr_load_i", vec![v.flags.clone(), offset.clone()]),
                call(
                    "@ptr_name",
                    vec![v.name.clone(), int(self.name_capacity as i64)],
                ),
                add(offset.clone(), int(1)),
            ],
        });
        self.temp(
            v.kind.clone(),
            call(
                &format!("@ptr_load_{}", suffix(&v.kind)),
                vec![v.data.clone(), offset],
            ),
            out,
        )
    }
    fn store(&self, v: &View, offset: Expr, value: Expr, out: &mut Vec<Stmt>) {
        Self::check_limit(v, &offset, out);
        out.push(Stmt::CallSub {
            name: format!("@ptr_store_{}", suffix(&v.kind)),
            args: vec![v.data.clone(), v.flags.clone(), offset, value],
        });
    }
    fn associated(&mut self, e: &mut Expr, args: &[Expr], out: &mut Vec<Stmt>) -> Result<()> {
        let mut actuals = args.to_vec();
        crate::named_arguments::Signature {
            params: vec!["pointer".into(), "target".into()],
            optional: HashSet::from(["target".into()]),
        }
        .reorder("associated", &mut actuals)?;
        let Some(Expr::Ident(p)) = actuals.first() else {
            bail!("ASSOCIATED requires a POINTER variable");
        };
        let Some(ptr) = self.pointers.get(p).cloned() else {
            bail!("ASSOCIATED requires a POINTER variable");
        };
        let associated = bin(BinOp::Ne, id(&ptr.desc.state), int(0));
        let target = actuals.get(1).filter(|t| !crate::named_arguments::omitted(t));
        let Some(target) = target else {
            *e = associated;
            return Ok(());
        };
        // Associated with the same elements in the same order: equal storage,
        // origin, extents and strides (lower bounds may differ). A zero-sized
        // target is never associated.
        let (data, origin, axes): (Expr, Expr, Vec<(Expr, Expr)>) = match target {
            Expr::Ident(q) if self.pointers.contains_key(q) => {
                let q = self.pointers[q].desc.clone();
                let both = bin(BinOp::And, associated.clone(), bin(BinOp::Ne, id(&q.state), int(0)));
                let mut cond = both;
                for (a, b) in ptr.desc.axes.iter().zip(&q.axes) {
                    cond = bin(BinOp::And, cond, bin(BinOp::Eq, a.extent.clone(), b.extent.clone()));
                    cond = bin(BinOp::And, cond, bin(BinOp::Eq, a.stride.clone(), b.stride.clone()));
                }
                cond = bin(BinOp::And, cond, bin(BinOp::Eq, id(&ptr.desc.data), id(&q.data)));
                cond = bin(BinOp::And, cond, bin(BinOp::Eq, id(&ptr.desc.origin), id(&q.origin)));
                *e = cond;
                return Ok(());
            }
            _ => {
                let v = self
                    .view(target, true, out)?
                    .context("ASSOCIATED TARGET must be a TARGET variable, array section or POINTER")?;
                if v.axes.len() != ptr.desc.axes.len() {
                    *e = Expr::Logical(false);
                    return Ok(());
                }
                let data = self.integer(v.data, out);
                let origin = self.integer(v.origin, out);
                let axes = v.axes.into_iter().map(|a| (self.integer(a.extent, out), self.integer(a.stride, out))).collect();
                (data, origin, axes)
            }
        };
        let mut cond = bin(BinOp::And, associated, bin(BinOp::Eq, id(&ptr.desc.data), data));
        cond = bin(BinOp::And, cond, bin(BinOp::Eq, id(&ptr.desc.origin), origin));
        let mut nonempty = None;
        for (axis, (extent, stride)) in ptr.desc.axes.iter().zip(axes) {
            cond = bin(BinOp::And, cond, bin(BinOp::Eq, axis.extent.clone(), extent.clone()));
            cond = bin(BinOp::And, cond, bin(BinOp::Eq, axis.stride.clone(), stride));
            let positive = bin(BinOp::Gt, extent, int(0));
            nonempty = Some(match nonempty { None => positive, Some(c) => bin(BinOp::And, c, positive) });
        }
        if let Some(nonempty) = nonempty {
            cond = bin(BinOp::And, cond, nonempty);
        }
        *e = cond;
        Ok(())
    }
    fn inquiry(&mut self, n: &str, args: &[Expr], out: &mut Vec<Stmt>) -> Result<Expr> {
        let signature = crate::named_arguments::Signature {
            params: if n == "shape" {
                vec!["array".into()]
            } else {
                vec!["array".into(), "dim".into()]
            },
            optional: HashSet::from(["dim".into()]),
        };
        let mut actuals = args.to_vec();
        signature.reorder(n, &mut actuals)?;
        let args = actuals.as_slice();
        let v = self
            .view(&args[0], false, out)?
            .context("pointer inquiry requires an array")?;
        if v.axes.is_empty() {
            bail!("pointer array inquiry requires an array");
        }
        let mut vals = vec![];
        for a in &v.axes {
            if matches!(n, "lbound" | "ubound") {
                let result = self.integer(int(if n == "lbound" { 1 } else { 0 }), out);
                let Expr::Ident(dest) = &result else {
                    unreachable!()
                };
                let bound = if n == "lbound" {
                    a.lower.clone()
                } else {
                    add(a.lower.clone(), sub(a.extent.clone(), int(1)))
                };
                out.push(Stmt::If {
                    cond: bin(BinOp::Gt, a.extent.clone(), int(0)),
                    then_body: vec![assign(dest, bound)],
                    else_body: None,
                });
                vals.push(result);
            } else {
                vals.push(a.extent.clone());
            }
        }
        if args.len() == 1 {
            return Ok(if n == "size" {
                v.count()
            } else {
                call("@array_constructor", vals)
            });
        }
        if args.len() != 2 || n == "shape" {
            bail!("unsupported pointer array inquiry arguments");
        }
        let mut dim = args[1].clone();
        if let Expr::Call(tag, a) = &dim {
            if tag == "@named_arg" && matches!(&a[0],Expr::Str(s) if s=="dim") {
                dim = a[1].clone();
            }
        }
        self.expr(&mut dim, out)?;
        let dim = self.integer(dim, out);
        out.push(require(
            bin(
                BinOp::And,
                bin(BinOp::Ge, dim.clone(), int(1)),
                bin(BinOp::Le, dim.clone(), int(vals.len() as i64)),
            ),
            "pointer array inquiry DIM outside rank".into(),
        ));
        let result = self.integer(int(0), out);
        let Expr::Ident(dest) = &result else {
            unreachable!()
        };
        for (i, e) in vals.into_iter().enumerate() {
            out.push(Stmt::If {
                cond: bin(BinOp::Eq, dim.clone(), int(i as i64 + 1)),
                then_body: vec![assign(dest, e)],
                else_body: None,
            });
        }
        Ok(result)
    }
    fn has_pointer(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(n) | Expr::Section { name: n, .. } => self.pointers.contains_key(n),
            Expr::Call(n, a) | Expr::Index(n, a) => {
                self.pointers.contains_key(n) || a.iter().any(|e| self.has_pointer(e))
            }
            Expr::Bin(_, a, b) => self.has_pointer(a) || self.has_pointer(b),
            Expr::Un(_, a) => self.has_pointer(a),
            _ => false,
        }
    }
    fn kind(&self, e: &Expr) -> Result<TypeSpec> {
        if let Some(n) = root(e) {
            if let Some(p) = self.pointers.get(n) {
                return Ok(p.kind.clone());
            }
            if let Some(v) = self.vars.get(n) {
                return Ok(v.kind.clone());
            }
        }
        Ok(match e {
            Expr::IntLit(_) => TypeSpec::Integer(None),
            Expr::RealLit(_) => TypeSpec::Real,
            Expr::Logical(_) => TypeSpec::Logical,
            Expr::Un(UnOp::Not, _) => TypeSpec::Logical,
            Expr::Un(_, a) => self.kind(a)?,
            Expr::Bin(op, a, b) => {
                if matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow
                ) {
                    promote(self.kind(a)?, self.kind(b)?)
                } else {
                    TypeSpec::Logical
                }
            }
            Expr::Call(n, _) if n == "dble" => TypeSpec::DoublePrecision,
            Expr::Call(n, _) if n == "real" => TypeSpec::Real,
            Expr::Call(n, _)
                if matches!(n.as_str(), "int" | "size" | "count" | "lbound" | "ubound") =>
            {
                TypeSpec::Integer(None)
            }
            Expr::Call(n, a) if n == "@array_constructor" => {
                let mut result = None;
                for a in a {
                    let k = self.kind(a)?;
                    result = Some(result.map_or(k.clone(), |r| promote(r, k)));
                }
                result.context("empty constructor requires an explicit type")?
            }
            Expr::Call(n, a) if n == "reshape" || n == "abs" => self.kind(&a[0])?,
            _ => bail!("unsupported type in pointer array expression"),
        })
    }
    fn array(&mut self, e: &Expr, out: &mut Vec<Stmt>) -> Result<Option<ArrayValue>> {
        if let Expr::Call(n, args) = e {
            if matches!(n.as_str(), "@array_constructor" | "reshape") && !self.has_pointer(e) {
                let (kind, dims) = if n == "reshape" {
                    (self.kind(&args[0])?, crate::multidim::reshape_shape(args)?)
                } else {
                    (self.kind(e)?, vec![args.len() as i64])
                };
                let temp = self.fresh();
                out.push(Stmt::ArrayDecl {
                    kind: kind.clone(),
                    name: temp.clone(),
                    dims: dims.iter().copied().map(int).collect(),
                });
                out.push(assign(&temp, e.clone()));
                self.vars.insert(
                    temp.clone(),
                    Variable {
                        kind,
                        dims: dims.clone(),
                        lower: vec![1; dims.len()],
                        level: 0,
                        target: false,
                        allocatable: false,
                        length: None,
                        active_shape: None,
                        heap: false,
                    },
                );
                return self.array(&id(&temp), out);
            }
        }
        if let Some(v) = self.view(e, false, out)? {
            if v.axes.is_empty() {
                return Ok(None);
            }
            let mut reads = vec![];
            let offset = self.offset(&v, id("@pointer_element"), &mut reads);
            let value = self.read(&v, offset, &mut reads);
            return Ok(Some(ArrayValue {
                shape: v.axes.iter().map(|a| a.extent.clone()).collect(),
                kind: v.kind,
                capacity: v.capacity,
                value,
                reads,
            }));
        }
        if let Expr::Bin(op, a, b) = e {
            let left = self.array(a, out)?;
            let right = self.array(b, out)?;
            if left.is_none() && right.is_none() {
                return Ok(None);
            }
            let (shape, kind, capacity) = left
                .as_ref()
                .or(right.as_ref())
                .map(|a| (a.shape.clone(), a.kind.clone(), a.capacity))
                .unwrap();
            let kind = promote(
                self.kind(a).unwrap_or_else(|_| kind.clone()),
                self.kind(b).unwrap_or_else(|_| kind.clone()),
            );
            let mut reads = vec![];
            let av = if let Some(a) = left {
                reads.extend(a.reads);
                a.value
            } else {
                let mut e = *a.clone();
                self.expr(&mut e, out)?;
                e
            };
            let bv = if let Some(b) = right {
                self.conform(&shape, &b.shape, out)?;
                reads.extend(b.reads);
                b.value
            } else {
                let mut e = *b.clone();
                self.expr(&mut e, out)?;
                e
            };
            let kind = if matches!(
                op,
                BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::Eqv
                    | BinOp::Neqv
            ) {
                TypeSpec::Logical
            } else {
                kind
            };
            return Ok(Some(ArrayValue {
                shape,
                kind,
                capacity,
                value: bin(op.clone(), av, bv),
                reads,
            }));
        }
        if let Expr::Un(op, a) = e {
            if let Some(mut a) = self.array(a, out)? {
                a.value = Expr::Un(op.clone(), Box::new(a.value));
                return Ok(Some(a));
            }
        }
        // An elemental intrinsic applies to each element of its array
        // arguments, with its scalar arguments evaluated once.
        if let Expr::Call(n, args) = e {
            let conversion = match n.as_str() {
                "dble" | "dfloat" if args.len() == 1 => Some(TypeSpec::DoublePrecision),
                "real" | "sngl" | "float" if args.len() == 1 => Some(TypeSpec::Real),
                "int" | "nint" | "idint" | "idnint" | "ifix" if args.len() == 1 => Some(TypeSpec::Integer(None)),
                _ => None,
            };
            let elemental = conversion.is_some()
                || matches!(
                    n.as_str(),
                    "abs" | "dabs" | "iabs" | "sqrt" | "dsqrt" | "exp" | "dexp" | "log" | "dlog" | "log10" | "dlog10"
                        | "sin" | "dsin" | "cos" | "dcos" | "tan" | "dtan" | "asin" | "dasin" | "acos" | "dacos"
                        | "atan" | "datan" | "sinh" | "dsinh" | "cosh" | "dcosh" | "tanh" | "dtanh" | "aint" | "anint"
                        | "max" | "min" | "dmax1" | "dmin1" | "max0" | "min0" | "mod" | "dmod" | "sign" | "dsign"
                        | "isign" | "dim" | "ddim"
                );
            if elemental && !self.functions.contains(n) && !self.vars.contains_key(n) && !self.pointers.contains_key(n) {
                let mut parts = vec![];
                for a in args {
                    parts.push(self.array(a, out)?);
                }
                if let Some(first) = parts.iter().flatten().next() {
                    let (shape, capacity) = (first.shape.clone(), first.capacity);
                    let mut kind = first.kind.clone();
                    let mut reads = vec![];
                    let mut values = vec![];
                    for (a, part) in args.iter().zip(parts) {
                        match part {
                            Some(p) => {
                                self.conform(&shape, &p.shape, out)?;
                                kind = promote(kind, p.kind.clone());
                                reads.extend(p.reads);
                                values.push(p.value);
                            }
                            None => {
                                let mut e = a.clone();
                                self.expr(&mut e, out)?;
                                if let Ok(k) = self.kind(a) {
                                    kind = promote(kind, k);
                                }
                                values.push(e);
                            }
                        }
                    }
                    return Ok(Some(ArrayValue {
                        shape,
                        kind: conversion.unwrap_or(kind),
                        capacity,
                        value: call(n, values),
                        reads,
                    }));
                }
            }
        }
        Ok(None)
    }
    fn conform(&self, a: &[Expr], b: &[Expr], out: &mut Vec<Stmt>) -> Result<()> {
        if a.len() != b.len() {
            bail!("pointer array expression rank mismatch");
        }
        for (x, y) in a.iter().zip(b) {
            out.push(require(
                bin(BinOp::Eq, x.clone(), y.clone()),
                "pointer array shapes do not conform".into(),
            ));
        }
        Ok(())
    }
    fn loop_body(&mut self, mut a: ArrayValue, index: &str) -> (Vec<Stmt>, Expr) {
        replace(&mut a.value, "@pointer_element", &id(index));
        for s in &mut a.reads {
            replace_stmt(s, "@pointer_element", &id(index));
        }
        (a.reads, a.value)
    }
    /// MAXLOC or MINLOC along the only dimension of a rank-one array: the
    /// first position of the largest or smallest value, or 0 if it is empty.
    fn location(&mut self, n: &str, args: &[Expr], out: &mut Vec<Stmt>) -> Result<Expr> {
        // The normalized arguments: ARRAY, VALUE, DIM, MASK, KIND, BACK.
        let n = n.trim_start_matches('@');
        let dim = args.get(2).map(crate::parameters::integer_constant);
        if args.len() != 6 || !matches!(dim, Some(Ok(1))) || args[3] != Expr::Logical(true) || args[5] != Expr::Logical(false) {
            bail!("POINTER `{n}` currently requires a rank-one array and DIM=1, without MASK or BACK");
        }
        let a = self.array(&args[0], out)?.context("pointer location requires an array")?;
        if a.shape.len() != 1 {
            bail!("POINTER `{n}` currently requires a rank-one array and DIM=1");
        }
        let count = a.shape[0].clone();
        let kind = a.kind.clone();
        let best = self.temp(kind.clone(), int(0), out);
        let position = self.temp(TypeSpec::Integer(None), int(0), out);
        let (Expr::Ident(best_name), Expr::Ident(position_name)) = (&best, &position) else { unreachable!() };
        let (best_name, position_name) = (best_name.clone(), position_name.clone());
        let element = self.fresh();
        out.push(Stmt::VarDecl { kind, names: vec![element.clone()] });
        let index = self.fresh();
        out.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![index.clone()] });
        let (mut body, value) = self.loop_body(a, &index);
        body.push(assign(&element, value));
        let better = bin(if n == "maxloc" { BinOp::Gt } else { BinOp::Lt }, id(&element), best.clone());
        body.push(Stmt::If {
            cond: bin(BinOp::Or, bin(BinOp::Eq, position.clone(), int(0)), better),
            then_body: vec![assign(&best_name, id(&element)), assign(&position_name, id(&index))],
            else_body: None,
        });
        out.push(Stmt::Do { var: index, start: int(1), end: count, body });
        Ok(position)
    }
    fn reduction(&mut self, n: &str, args: &[Expr], out: &mut Vec<Stmt>) -> Result<Expr> {
        let count_i8 = n == "@count_i8";
        let n = match n {
            "@count_default" | "@count_i8" => "count",
            "@any_reduction" => "any",
            "@all_reduction" => "all",
            _ => n,
        };
        if args.len() != 1 {
            bail!("pointer reductions currently support the ARRAY argument without DIM or MASK");
        }
        let a = self
            .array(&args[0], out)?
            .context("pointer reduction requires an array")?;
        if matches!(n, "any" | "all" | "count") != matches!(a.kind, TypeSpec::Logical) {
            bail!("pointer reduction argument has incorrect numeric/logical type");
        }
        let count = product(a.shape.iter().cloned());
        let kind = if matches!(n, "any" | "all") {
            TypeSpec::Logical
        } else if n == "count" {
            TypeSpec::Integer(if count_i8 { Some(8) } else { None })
        } else {
            a.kind.clone()
        };
        let zero = match &kind {
            TypeSpec::Real => Expr::RealLit("0.0".into()),
            TypeSpec::DoublePrecision => call("dble", vec![Expr::RealLit("0.0".into())]),
            TypeSpec::Logical => Expr::Logical(n == "all"),
            _ => int(0),
        };
        let initial = match n {
            "product" => match kind {
                TypeSpec::Real | TypeSpec::DoublePrecision => {
                    call("dble", vec![Expr::RealLit("1.0".into())])
                }
                _ => int(1),
            },
            "minval" | "maxval" => {
                let huge = match kind {
                    TypeSpec::Integer(bytes) => int(crate::inquiries::integer_huge(bytes)?),
                    TypeSpec::Real => Expr::RealLit(format!("{:.17e}", f64::from(f32::MAX))),
                    _ => call("dble", vec![Expr::RealLit(f64::MAX.to_string())]),
                };
                if n == "maxval" {
                    Expr::Un(UnOp::Neg, Box::new(huge))
                } else {
                    huge
                }
            }
            _ => zero,
        };
        let acc = self.temp(kind, initial, out);
        let Expr::Ident(dest) = &acc else {
            unreachable!()
        };
        let index = self.fresh();
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![index.clone()],
        });
        let (mut body, value) = self.loop_body(a, &index);
        let next = match n {
            "sum" => add(acc.clone(), value),
            "product" => mul(acc.clone(), value),
            "minval" => call("min", vec![acc.clone(), value]),
            "maxval" => call("max", vec![acc.clone(), value]),
            "any" => bin(BinOp::Or, acc.clone(), value),
            "all" => bin(BinOp::And, acc.clone(), value),
            "count" => {
                body.push(Stmt::If {
                    cond: value,
                    then_body: vec![assign(dest, add(acc.clone(), int(1)))],
                    else_body: None,
                });
                acc.clone()
            }
            _ => unreachable!(),
        };
        body.push(assign(dest, next));
        out.push(Stmt::Do {
            var: index,
            start: int(1),
            end: count,
            body,
        });
        Ok(acc)
    }
    fn expr(&mut self, e: &mut Expr, out: &mut Vec<Stmt>) -> Result<()> {
        if let Expr::Call(n, args) = e {
            if !self.functions.contains(n) {
                if n == "associated" {
                    let args = args.clone();
                    return self.associated(e, &args, out);
                }
                if args.iter().any(|e| self.has_pointer(e)) {
                    if matches!(n.as_str(), "size" | "shape" | "lbound" | "ubound") {
                        *e = self.inquiry(n, args, out)?;
                        return Ok(());
                    }
                    if matches!(
                        n.as_str(),
                        "sum"
                            | "product"
                            | "minval"
                            | "maxval"
                            | "any"
                            | "all"
                            | "count"
                            | "@count_default"
                            | "@count_i8"
                            | "@any_reduction"
                            | "@all_reduction"
                    ) {
                        *e = self.reduction(n, args, out)?;
                        return Ok(());
                    }
                    if matches!(n.as_str(), "@maxloc" | "@minloc") {
                        *e = self.location(n, args, out)?;
                        return Ok(());
                    }
                }
            }
        }
        if root(e).is_some_and(|n| self.pointers.contains_key(n)) {
            let v = self.view(e, false, out)?.unwrap();
            if !v.axes.is_empty() {
                bail!("whole POINTER array in this expression is not yet supported");
            }
            *e = self.read(&v, v.origin.clone(), out);
            return Ok(());
        }
        match e {
            Expr::Call(_, a) | Expr::Index(_, a) => {
                for e in a {
                    self.expr(e, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, out)?;
                self.expr(b, out)?;
            }
            Expr::Un(_, a) => self.expr(a, out)?,
            _ => {}
        }
        Ok(())
    }
    fn writable(&self, p: &str, readonly: &HashSet<String>) -> Result<()> {
        if let Some(ptr) = self.pointers.get(p) {
            if let Some(n) = ptr.targets.iter().chain(&ptr.record_targets).find(|n| readonly.contains(*n)) {
                bail!("cannot modify INTENT(IN) TARGET `{n}` through POINTER `{p}`");
            }
        }
        Ok(())
    }
    fn assignment(
        &mut self,
        dest: &Expr,
        value: &Expr,
        readonly: &HashSet<String>,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let n = root(dest).context("invalid pointer assignment destination")?;
        self.writable(n, readonly)?;
        let v = self
            .view(dest, false, out)?
            .context("pointer assignment requires an array or POINTER")?;
        if v.axes.is_empty() {
            let mut value = value.clone();
            self.expr(&mut value, out)?;
            let value = self.temp(v.kind.clone(), value, out);
            self.store(&v, v.origin.clone(), value, out);
            return Ok(());
        }
        let index = self.fresh();
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![index.clone()],
        });
        let count = v.count();
        let rhs = self.array(value, out)?;
        let read_value = if let Some(a) = rhs {
            self.conform(
                &v.axes.iter().map(|a| a.extent.clone()).collect::<Vec<_>>(),
                &a.shape,
                out,
            )?;
            let temp = self.fresh();
            if v.capacity == 0 || a.capacity == 0 {
                crate::heap::temporary(v.kind.clone(), temp.clone(), count.clone(), out);
            } else {
                out.push(Stmt::ArrayDecl {
                    kind: v.kind.clone(),
                    name: temp.clone(),
                    dims: vec![int(v.capacity.max(a.capacity))],
                });
            }
            // The descriptor count can be zero; initialization is checked only
            // for elements actually written/read by these runtime loops.

            let (mut body, value) = self.loop_body(a, &index);
            body.push(Stmt::AssignIndex {
                name: temp.clone(),
                indices: vec![id(&index)],
                value,
            });
            out.push(Stmt::Do {
                var: index.clone(),
                start: int(1),
                end: count.clone(),
                body,
            });
            Expr::Index(temp, vec![id(&index)])
        } else {
            let mut e = value.clone();
            self.expr(&mut e, out)?;
            self.temp(v.kind.clone(), e, out)
        };
        let mut body = vec![];
        let offset = self.offset(&v, id(&index), &mut body);
        self.store(&v, offset, read_value, &mut body);
        out.push(Stmt::Do {
            var: index,
            start: int(1),
            end: count,
            body,
        });
        Ok(())
    }
    fn print(&mut self, items: &[Expr], out: &mut Vec<Stmt>) -> Result<()> {
        let mut previous_character = false;
        for (i, e) in items.iter().enumerate() {
            if let Some(a) = self.array(e, out)? {
                let count = product(a.shape.iter().cloned());
                let index = self.fresh();
                out.push(Stmt::VarDecl {
                    kind: TypeSpec::Integer(Some(8)),
                    names: vec![index.clone()],
                });
                let (mut body, value) = self.loop_body(a, &index);
                body.push(Stmt::CallSub {
                    name: "@print_fragment".into(),
                    args: vec![Expr::Str(" ".into()), value],
                });
                out.push(Stmt::Do {
                    var: index,
                    start: int(1),
                    end: count,
                    body,
                });
                previous_character = false;
            } else {
                let mut e = e.clone();
                self.expr(&mut e, out)?;
                if let Expr::Call(n, args) = &e {
                    if n == "@array_constructor" {
                        for item in args {
                            out.push(Stmt::CallSub {
                                name: "@print_fragment".into(),
                                args: vec![Expr::Str(" ".into()), item.clone()],
                            });
                        }
                        previous_character = false;
                        continue;
                    }
                }
                let character = matches!(e, Expr::Str(_))
                    || matches!(&e,Expr::Ident(n) if self.vars.get(n).is_some_and(|v|matches!(v.kind,TypeSpec::Character(_))));
                let mut args = vec![];
                if i == 0 || !character || !previous_character {
                    args.push(Expr::Str(" ".into()));
                }
                args.push(e);
                out.push(Stmt::CallSub {
                    name: "@print_fragment".into(),
                    args,
                });
                previous_character = character;
            }
        }
        out.push(Stmt::CallSub {
            name: "@print_fragment".into(),
            args: vec![Expr::Str("\n".into())],
        });
        Ok(())
    }
    fn associate(
        &mut self,
        p: &str,
        e: &Expr,
        readonly: &HashSet<String>,
        out: &mut Vec<Stmt>,
    ) -> Result<()> {
        let ptr = self
            .pointers
            .get(p)
            .context("pointer association requires POINTER")?
            .clone();
        if readonly.contains(p) {
            bail!("cannot change association of INTENT(IN) POINTER `{p}`");
        }
        if matches!(e,Expr::Call(n,a) if n=="null"&&a.is_empty()) {
            out.push(assign(&ptr.desc.state, int(0)));
            return Ok(());
        }
        if let Expr::Ident(q) = e {
            if let Some(q) = self.pointers.get(q) {
                if ptr.desc.axes.len() != q.desc.axes.len() {
                    bail!("rank mismatch in pointer association for `{p}`");
                }
                if !same_kind(&ptr.kind, &q.kind) {
                    bail!("type or kind mismatch in pointer association for `{p}`");
                }
                let source = q.desc.fields();
                for (dest, source) in ptr.desc.fields().into_iter().zip(source) {
                    out.push(assign(&dest, id(&source)));
                }
                return Ok(());
            }
        }
        let v = self
            .view(e, true, out)?
            .context("pointer association requires a TARGET variable or array section")?;
        if ptr.desc.axes.len() != v.axes.len() {
            bail!("rank mismatch in pointer association for `{p}`");
        }
        if !same_kind(&ptr.kind, &v.kind) {
            bail!("type or kind mismatch in pointer association for `{p}`");
        }
        // Snapshot all source metadata before changing the destination (p=>p(...)).
        let mut values = vec![v.data, v.flags, v.name, int(1), v.origin];
        for a in v.axes {
            values.extend([a.lower, a.extent, a.stride]);
        }
        let values = values
            .into_iter()
            .map(|e| self.integer(e, out))
            .collect::<Vec<_>>();
        for (dest, value) in ptr.desc.fields().into_iter().zip(values) {
            out.push(assign(&dest, value));
        }
        Ok(())
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &HashSet<String>) -> Result<()> {
        let statement_location = std::cell::Cell::new(None);
        let result = (|| -> Result<()> {
        let mut readonly = inherited.clone();
        for s in body.iter() {
            if let Stmt::DummyIntent { name, intent } = s {
                if intent == "@pointer_readonly_target" || intent == "intent(in)" {
                    readonly.insert(name.clone());
                }
            }
        }
        let mut out = vec![];
        let base = self.live.len();
        for mut s in std::mem::take(body) {
            self.release(base);
            if let Some(p) = crate::source_diagnostics::statement_offset(&s) { statement_location.set(Some(p)); }
            match &mut s {
                Stmt::DummyIntent { intent, .. }
                    if matches!(
                        intent.as_str(),
                        "pointer" | "target" | "@pointer_readonly_target"
                    ) || intent.starts_with("@pointer_bounds:") || intent.starts_with("@borrowed_descriptor:")
                        || intent.starts_with("@borrowed_limit:") =>
                {
                    continue
                }
                Stmt::VarDecl { names, .. } => {
                    for p in names.iter().filter_map(|n| self.pointers.get(n)) {
                        if !p.borrowed {self.initialize(&p.desc, &mut out);}
                    }
                    names.retain(|n| !self.pointers.contains_key(n));
                    if names.is_empty() {
                        continue;
                    }
                }
                Stmt::ArrayDecl { name, .. } if self.pointers.contains_key(name) => {
                    if !self.pointers[name].borrowed {self.initialize(&self.pointers[name].desc, &mut out);}
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@borrowed_out" => {
                    let v=self.view(&args[0],false,&mut out)?.context("missing borrowed output descriptor")?;
                    let index=self.fresh();
                    out.push(Stmt::VarDecl{kind:TypeSpec::Integer(Some(8)),names:vec![index.clone()]});
                    let mut body=vec![];
                    let offset=self.offset(&v,id(&index),&mut body);
                    body.push(Stmt::CallSub{name:"@ptr_store_clear".into(),args:vec![v.flags.clone(),v.flags.clone(),offset,int(0)]});
                    out.push(Stmt::Do{var:index,start:int(1),end:v.count(),body});
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@record_pointer_associate" => {
                    let [Expr::Ident(p),handle,label] = args.as_slice() else {bail!("invalid owned pointer association")};
                    if readonly.contains(p) {bail!("cannot change association of INTENT(IN) POINTER `{p}`");}
                    let pointer=self.pointers.get(p).context("owned association requires POINTER")?.clone();
                    let mut handle=handle.clone();
                    self.expr(&mut handle,&mut out)?;
                    let mut args=vec![handle,label.clone()];
                    args.extend(pointer.desc.fields().into_iter().map(Expr::Ident));
                    out.push(Stmt::CallSub{name:"@heap_record_pointer".into(),args});
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@pointer_associate" => {
                    let [Expr::Ident(p), e] = args.as_slice() else {
                        bail!("invalid pointer association")
                    };
                    self.associate(p, e, &readonly, &mut out)?;
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@nullify" => {
                    if args.is_empty() {
                        bail!("NULLIFY requires POINTER variables");
                    }
                    for e in args {
                        let Expr::Ident(p) = e else {
                            bail!("NULLIFY requires POINTER variables")
                        };
                        let ptr = self
                            .pointers
                            .get(p)
                            .context("NULLIFY requires POINTER variables")?;
                        if readonly.contains(p) {
                            bail!("cannot NULLIFY INTENT(IN) POINTER `{p}`");
                        }
                        out.push(assign(&ptr.desc.state, int(0)));
                    }
                    continue;
                }
                Stmt::Assign { name, value }
                    if self.pointers.contains_key(name)
                        || self.has_pointer(value)
                            && self.vars.get(name).is_some_and(|v| !v.dims.is_empty()) =>
                {
                    self.assignment(&id(name), value, &readonly, &mut out)?;
                    continue;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if self.pointers.contains_key(name) => {
                    self.assignment(
                        &Expr::Index(name.clone(), indices.clone()),
                        value,
                        &readonly,
                        &mut out,
                    )?;
                    continue;
                }
                Stmt::Print { items } if items.iter().any(|e| self.has_pointer(e)) => {
                    self.print(items, &mut out)?;
                    continue;
                }
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    self.expr(value, &mut out)?
                }
                Stmt::AssignIndex { indices, value, .. } => {
                    for e in indices {
                        self.expr(e, &mut out)?;
                    }
                    self.expr(value, &mut out)?;
                }
                Stmt::Print { items } => {
                    for e in items {
                        self.expr(e, &mut out)?;
                    }
                }
                Stmt::Read { args } if args.iter().any(|e| self.has_pointer(e)) => {
                    bail!("READ into POINTER storage is not yet supported")
                }
                // Allocating a local, such as an automatic array, whose
                // bounds read a POINTER only evaluates them.
                Stmt::CallSub { name, args } if name == "@allocate"
                    && args.iter().all(|e| matches!(e, Expr::Call(n, _) if !self.pointers.contains_key(n))) => {
                    for e in args.iter_mut() {
                        if let Expr::Call(_, bounds) = e {
                            for b in bounds {
                                self.expr(b, &mut out)?;
                            }
                        }
                    }
                }
                Stmt::CallSub { name, args } => {
                    if args.iter().any(|e| self.has_pointer(e))
                        && (name.contains("read")
                            || !name.starts_with('@')
                            || matches!(name.as_str(), "@allocate" | "@deallocate" | "@undefine"))
                    {
                        bail!("POINTER arguments to `{name}` are not yet supported");
                    }
                    for e in args {
                        self.expr(e, &mut out)?;
                    }
                }
                Stmt::If { cond, .. } => self.expr(cond, &mut out)?,
                Stmt::Do {
                    var, start, end, ..
                } => {
                    if self.pointers.contains_key(var) {
                        bail!("POINTER loop variables are not yet supported");
                    }
                    self.expr(start, &mut out)?;
                    self.expr(end, &mut out)?;
                }
                Stmt::DoWhile { cond, body } => {
                    let mut prefix = vec![];
                    self.expr(cond, &mut prefix)?;
                    if !prefix.is_empty() {
                        prefix.push(Stmt::If {
                            cond: Expr::Un(UnOp::Not, Box::new(cond.clone())),
                            then_body: vec![Stmt::Exit],
                            else_body: None,
                        });
                        prefix.append(body);
                        *body = prefix;
                        *cond = Expr::Logical(true);
                    }
                }
                Stmt::SelectCase { expr, .. } => self.expr(expr, &mut out)?,
                // An explicit-step DO or FORALL is a BLOCK that starts with
                // its control. Read the bounds before it, as for DO, so that
                // the control stays first.
                Stmt::Block { body: inner }
                    if matches!(inner.first(), Some(Stmt::CallSub { name, .. }) if name == "@do_step" || name == "@forall") =>
                {
                    if let Some(Stmt::CallSub { args, .. }) = inner.first_mut() {
                        for control in args.chunks_exact_mut(4) {
                            for e in &mut control[1..] {
                                self.expr(e, &mut out)?;
                            }
                        }
                    }
                    let mut rest = inner.split_off(1);
                    self.body(&mut rest, &readonly)?;
                    inner.append(&mut rest);
                    out.push(s);
                    continue;
                }
                Stmt::Function { .. } | Stmt::Subroutine { .. } => {
                    out.push(s);
                    continue;
                }
                _ => {}
            }
            for b in children(&mut s) {
                self.body(b, &readonly)?;
            }
            out.push(s);
        }
        self.release(base);
        *body = out;
        Ok(())
            })();
        result.map_err(|e| crate::source_diagnostics::at(e, statement_location.get()))
    }
}
fn root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) | Expr::Section { name: n, .. } | Expr::Call(n, _) | Expr::Index(n, _) => {
            Some(n)
        }
        _ => None,
    }
}
fn replace(e: &mut Expr, n: &str, value: &Expr) {
    match e {
        Expr::Ident(s) if s == n => *e = value.clone(),
        Expr::Call(_, a) | Expr::Index(_, a) => {
            for e in a {
                replace(e, n, value)
            }
        }
        Expr::Bin(_, a, b) => {
            replace(a, n, value);
            replace(b, n, value)
        }
        Expr::Un(_, a) => replace(a, n, value),
        _ => {}
    }
}
fn replace_stmt(s: &mut Stmt, n: &str, value: &Expr) {
    match s {
        Stmt::Assign { value: e, .. } => replace(e, n, value),
        Stmt::CallSub { args, .. } => {
            for e in args {
                replace(e, n, value)
            }
        }
        _ => {}
    }
}

pub fn resolve(mut p: Program) -> Result<Program> {
    fn known(body: &mut Vec<Stmt>, names: &mut HashSet<String>) {
        for s in body {
            if let Stmt::Function { name, .. } = s {
                names.insert(name.clone());
            }
            for b in children(s) {
                known(b, names);
            }
        }
    }
    fn unit(body: &mut Vec<Stmt>, functions: &HashSet<String>) -> Result<()> {
        let mut lower = Lower {
            vars: HashMap::new(),
            pointers: HashMap::new(),
            functions: functions.clone(),
            serial: 0,
            capacity: 1,
            name_capacity: 1,
            free: HashMap::new(),
            live: vec![],
            temporaries: vec![],
        };
        lower.declarations(body, 0)?;
        if !lower.pointers.is_empty() {
            lower.targets(body)?;
            lower.body(body, &HashSet::new())?;
            body.splice(0..0, std::mem::take(&mut lower.temporaries));
        }
        fn procedures(body: &mut Vec<Stmt>, functions: &HashSet<String>) -> Result<()> {
            for s in body {
                if let Stmt::Function { name, body, .. } = s {
                    let pointers: HashSet<_> = body
                        .iter()
                        .filter_map(|s| {
                            if let Stmt::DummyIntent { name, intent } = s {
                                if intent == "pointer" {
                                    Some(name.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect();
                    if pointers.contains(name)||body.iter().any(|s|matches!(s,Stmt::Return(Some(Expr::Ident(n))) if pointers.contains(n))){bail!("POINTER function results are not yet supported");}
                }
                if let Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } =
                    s
                {
                    if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if intent=="pointer"&&params.contains(name))){bail!("POINTER dummy arguments require local subroutine specialization; POINTER functions are not yet supported");}
                    unit(body, functions)?;
                } else {
                    for b in children(s) {
                        procedures(b, functions)?;
                    }
                }
            }
            Ok(())
        }
        procedures(body, functions)
    }
    let mut functions = HashSet::new();
    known(&mut p.body, &mut functions);
    unit(&mut p.body, &functions)?;
    crate::substrings::hoist(&mut p.body);
    Ok(p)
}

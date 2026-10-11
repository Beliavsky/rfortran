// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Fixed-size aliases. Association stores a selector and captured subscripts;
//! every subsequent access reads or writes the selected target's actual storage.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
pub(crate) fn same_kind(a: &TypeSpec, b: &TypeSpec) -> bool {
    a == b
        || matches!((a,b),(TypeSpec::Integer(a),TypeSpec::Integer(b)) if a.unwrap_or(4)==b.unwrap_or(4))
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn error(message: String) -> Stmt {
    Stmt::CallSub {
        name: "@error_stop".into(),
        args: vec![Expr::Str(message)],
    }
}
fn require(cond: Expr, message: String) -> Stmt {
    Stmt::If {
        cond: Expr::Un(UnOp::Not, Box::new(cond)),
        then_body: vec![error(message)],
        else_body: None,
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
    let enabled = has_pointer(&mut p.body);
    fn walk(body: &mut Vec<Stmt>, serial: &mut usize, enabled: bool) -> Result<()> {
        let mut extra = vec![];
        for s in body.iter_mut() {
            if let Stmt::ArrayDecl { name, dims, .. } = s {
                if dims.iter().any(|d|matches!(d,Expr::Call(n,a) if n=="@declared_bounds" && crate::parameters::integer_constant(&a[0]).ok()!=Some(1))) {
                    extra.push(Stmt::DummyIntent{name:name.clone(),intent:"@pointer_nonunit".into()});
                }
            }
            if let Stmt::Block { body } = s {
                if enabled {
                    crate::subroutines::rename_allocation_block(body, serial)?;
                }
            }
            for b in children(s) {
                walk(b, serial, enabled)?;
            }
        }
        body.extend(extra);
        Ok(())
    }
    walk(&mut p.body, &mut 0, enabled)?;
    Ok(p)
}
#[derive(Clone)]
struct Variable {
    kind: TypeSpec,
    dims: Vec<Expr>,
    level: usize,
    target: bool,
    nonunit: bool,
    allocatable: bool,
}
#[derive(Clone)]
struct Binding {
    target: String,
    indices: Vec<Expr>,
    captures: Vec<(String, Expr, i64)>,
    count: usize,
}
struct Pointer {
    kind: TypeSpec,
    rank: usize,
    level: usize,
    tag: String,
    bindings: Vec<Binding>,
}
struct Lower {
    vars: HashMap<String, Variable>,
    pointers: HashMap<String, Pointer>,
    functions: HashSet<String>,
    serial: usize,
}
impl Lower {
    fn fresh(&mut self) -> String {
        self.serial += 1;
        format!("@pointer_{}", self.serial)
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
                    if !matches!(
                        kind,
                        TypeSpec::Integer(_)
                            | TypeSpec::Real
                            | TypeSpec::DoublePrecision
                            | TypeSpec::Logical
                    ) {
                        bail!("POINTER association currently supports numeric and LOGICAL types");
                    }
                    if dims.len() > 1
                        || dims
                            .iter()
                            .any(|d| !matches!(d,Expr::Ident(n) if n=="@deferred_extent"))
                    {
                        bail!("POINTER arrays currently require deferred-shape rank one");
                    }
                    let tag = self.fresh();
                    self.pointers.insert(
                        name.clone(),
                        Pointer {
                            kind: kind.clone(),
                            rank: dims.len(),
                            level,
                            tag,
                            bindings: vec![],
                        },
                    );
                }
                self.vars.insert(
                    name.clone(),
                    Variable {
                        kind: kind.clone(),
                        dims: dims.clone(),
                        level,
                        target: has("target"),
                        nonunit: has("@pointer_nonunit"),
                        allocatable: has("allocatable")
                            || attrs
                                .iter()
                                .any(|(n, i)| n == &name && i.starts_with("@bounded:")),
                    },
                );
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
    fn section(e: &Expr, extent: i64) -> Result<Vec<i64>> {
        let Expr::Section {
            lower,
            upper,
            stride,
            ..
        } = e
        else {
            bail!("expected array section");
        };
        let constant = |e: &Option<Box<Expr>>, default: i64| {
            e.as_deref()
                .map(crate::parameters::integer_constant)
                .transpose()
                .map(|e| e.unwrap_or(default))
        };
        let step = constant(stride, 1)?;
        if step == 0 {
            bail!("pointer section stride must not be zero");
        }
        // Fortran uses the declared bounds even with a negative stride.
        let first = constant(lower, 1)?;
        let last = constant(upper, extent)?;
        let mut result = vec![];
        let mut index = i128::from(first);
        while if step > 0 {
            index <= i128::from(last)
        } else {
            index >= i128::from(last)
        } {
            if index < 1 || index > i128::from(extent) {
                bail!("pointer section subscript outside 1:{extent}");
            }
            if result.len() >= 4096 {
                bail!("fixed POINTER views currently support at most 4096 elements");
            }
            result.push(index as i64);
            index += i128::from(step);
        }
        Ok(result)
    }
    fn binding(&mut self, p: &str, target: &Expr) -> Result<Binding> {
        let ptr = self
            .pointers
            .get(p)
            .context("pointer association requires a POINTER variable")?;
        let rank = ptr.rank;
        let kind = ptr.kind.clone();
        let level = ptr.level;
        let (name, subs) = match target {
            Expr::Ident(n) => (n.clone(), None),
            Expr::Section { name, .. } => (name.clone(), Some(vec![target.clone()])),
            Expr::Call(n, a) | Expr::Index(n, a) => (n.clone(), Some(a.clone())),
            _ => bail!("pointer association requires a TARGET variable or array section"),
        };
        if self.pointers.contains_key(&name) {
            bail!("pointer-to-pointer association is not yet supported");
        }
        let var = self
            .vars
            .get(&name)
            .context("undeclared pointer association target")?
            .clone();
        if !var.target {
            bail!("pointer association target `{name}` requires TARGET attribute");
        }
        if var.allocatable || var.nonunit {
            bail!("pointer association currently requires fixed one-based TARGET storage");
        }
        if var.level > level {
            bail!("pointer `{p}` cannot outlive local TARGET `{name}`");
        }
        if !same_kind(&kind, &var.kind) {
            bail!("type or kind mismatch in pointer association for `{p}`");
        }
        let extents = var
            .dims
            .iter()
            .map(crate::parameters::integer_constant)
            .collect::<Result<Vec<_>>>()
            .context("pointer association requires fixed TARGET extents")?;
        let mut indices = vec![];
        let mut captures = vec![];
        let mut count = 1;
        let mut result_rank = 0;
        if let Some(subs) = subs {
            if subs.len() != extents.len() {
                bail!("pointer target has incorrect number of subscripts");
            }
            for (mut sub, extent) in subs.into_iter().zip(extents) {
                if let Expr::Call(n, a) = &sub {
                    if n == "@section_subscript" {
                        sub = a[0].clone();
                    }
                }
                if matches!(sub, Expr::Section { .. }) {
                    result_rank += 1;
                    if result_rank > 1 {
                        bail!("pointer association currently supports scalar or rank-one views");
                    }
                    let values = Self::section(&sub, extent)?;
                    count = values.len();
                    let first = values.first().copied().unwrap_or(1);
                    let step = if values.len() > 1 {
                        values[1] - values[0]
                    } else {
                        1
                    };
                    indices.push(bin(
                        BinOp::Add,
                        int(first),
                        bin(
                            BinOp::Mul,
                            bin(BinOp::Sub, id("@pointer_index"), int(1)),
                            int(step),
                        ),
                    ));
                } else {
                    let capture = self.fresh();
                    indices.push(id(&capture));
                    captures.push((capture, sub, extent));
                }
            }
        } else {
            result_rank = var.dims.len();
            if result_rank > 1 {
                bail!("pointer association currently supports scalar or rank-one views");
            }
            if let Some(&extent) = extents.first() {
                count = usize::try_from(extent).context("invalid TARGET extent")?;
                indices.push(id("@pointer_index"));
            }
        }
        if result_rank != rank {
            bail!("rank mismatch in pointer association for `{p}`");
        }
        if count > 4096 {
            bail!("fixed POINTER views currently support at most 4096 elements");
        }
        if self.pointers[p]
            .bindings
            .first()
            .is_some_and(|b| b.count != count)
        {
            bail!("pointer `{p}` associations currently require the same fixed extent");
        }
        Ok(Binding {
            target: name,
            indices,
            captures,
            count,
        })
    }
    fn associations(&mut self, body: &mut Vec<Stmt>) -> Result<()> {
        for s in body {
            if let Stmt::CallSub { name, args } = s {
                if name == "@pointer_associate" {
                    let [Expr::Ident(p), target] = args.as_slice() else {
                        bail!("invalid pointer association");
                    };
                    if !self.pointers.contains_key(p) {
                        bail!("pointer association requires a POINTER variable `{p}`");
                    }
                    if matches!(target,Expr::Call(n,a) if n.eq_ignore_ascii_case("null") && a.is_empty())
                    {
                        args.push(int(0));
                    } else {
                        let p = p.clone();
                        let b = self.binding(&p, target)?;
                        let ptr = self.pointers.get_mut(&p).unwrap();
                        ptr.bindings.push(b);
                        args.push(int(ptr.bindings.len() as i64));
                    }
                }
            }
            if !matches!(s, Stmt::Function { .. } | Stmt::Subroutine { .. }) {
                for b in children(s) {
                    self.associations(b)?;
                }
            }
        }
        Ok(())
    }
    fn guard(&self, p: &str) -> Stmt {
        let ptr = &self.pointers[p];
        require(
            bin(
                BinOp::And,
                bin(BinOp::Ge, id(&ptr.tag), int(1)),
                bin(BinOp::Le, id(&ptr.tag), int(ptr.bindings.len() as i64)),
            ),
            format!("access through unassociated pointer `{p}`"),
        )
    }
    fn element(b: &Binding, index: &Expr) -> Expr {
        fn substitute(e: &mut Expr, i: &Expr) {
            match e {
                Expr::Ident(n) if n == "@pointer_index" => *e = i.clone(),
                Expr::Bin(_, a, b) => {
                    substitute(a, i);
                    substitute(b, i);
                }
                _ => {}
            }
        }
        if b.indices.is_empty() {
            return id(&b.target);
        }
        let mut indices = b.indices.clone();
        for e in &mut indices {
            substitute(e, index);
        }
        Expr::Index(b.target.clone(), indices)
    }
    fn index(&mut self, p: &str, index: Expr, out: &mut Vec<Stmt>) -> Expr {
        let temp = self.fresh();
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: vec![temp.clone()],
        });
        out.push(Stmt::Assign {
            name: temp.clone(),
            value: index,
        });
        let count = self.pointers[p].bindings.first().map_or(0, |b| b.count) as i64;
        out.push(require(
            bin(
                BinOp::And,
                bin(BinOp::Ge, id(&temp), int(1)),
                bin(BinOp::Le, id(&temp), int(count)),
            ),
            format!("pointer `{p}` subscript outside 1:{count}"),
        ));
        id(&temp)
    }
    fn read(&mut self, p: &str, index: Expr, out: &mut Vec<Stmt>) -> Expr {
        out.push(self.guard(p));
        let temp = self.fresh();
        let ptr = &self.pointers[p];
        out.push(Stmt::VarDecl {
            kind: ptr.kind.clone(),
            names: vec![temp.clone()],
        });
        let zero = match ptr.kind {
            TypeSpec::Real => Expr::RealLit("0.0".into()),
            TypeSpec::DoublePrecision => {
                Expr::Call("dble".into(), vec![Expr::RealLit("0.0".into())])
            }
            TypeSpec::Logical => Expr::Logical(false),
            _ => int(0),
        };
        out.push(Stmt::Assign {
            name: temp.clone(),
            value: zero,
        });
        for (i, b) in ptr.bindings.iter().enumerate() {
            out.push(Stmt::If {
                cond: bin(BinOp::Eq, id(&ptr.tag), int(i as i64 + 1)),
                then_body: vec![Stmt::Assign {
                    name: temp.clone(),
                    value: Self::element(b, &index),
                }],
                else_body: None,
            });
        }
        id(&temp)
    }
    fn write(
        &self,
        p: &str,
        index: Expr,
        value: Expr,
        out: &mut Vec<Stmt>,
        readonly: &HashSet<String>,
    ) -> Result<()> {
        let ptr = &self.pointers[p];
        for (i, b) in ptr.bindings.iter().enumerate() {
            if readonly.contains(&b.target) {
                bail!(
                    "cannot modify INTENT(IN) TARGET `{}` through POINTER `{p}`",
                    b.target
                );
            }
            let assign = match Self::element(b, &index) {
                Expr::Ident(name) => Stmt::Assign {
                    name,
                    value: value.clone(),
                },
                Expr::Index(name, indices) => Stmt::AssignIndex {
                    name,
                    indices,
                    value: value.clone(),
                },
                _ => unreachable!(),
            };
            out.push(Stmt::If {
                cond: bin(BinOp::Eq, id(&ptr.tag), int(i as i64 + 1)),
                then_body: vec![assign],
                else_body: None,
            });
        }
        Ok(())
    }
    fn expr(&mut self, e: &mut Expr, out: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Ident(p) if self.pointers.contains_key(p) => {
                let p = p.clone();
                *e = if self.pointers[&p].rank == 0 {
                    self.read(&p, int(1), out)
                } else {
                    out.push(self.guard(&p));
                    let count = self.pointers[&p].bindings.first().map_or(0, |b| b.count);
                    if count == 0 {
                        let temp = self.fresh();
                        out.push(Stmt::ArrayDecl {
                            kind: self.pointers[&p].kind.clone(),
                            name: temp.clone(),
                            dims: vec![int(0)],
                        });
                        id(&temp)
                    } else {
                        Expr::Call(
                            "@array_constructor".into(),
                            (1..=count)
                                .map(|i| self.read(&p, int(i as i64), out))
                                .collect(),
                        )
                    }
                };
            }
            Expr::Call(n, args)
                if !self.functions.contains(n)
                    && matches!(
                        n.to_ascii_lowercase().as_str(),
                        "associated" | "size" | "shape" | "lbound" | "ubound"
                    )
                    && matches!(args.first(),Some(Expr::Ident(p)) if self.pointers.contains_key(p)) =>
            {
                let Expr::Ident(p) = &args[0] else {
                    unreachable!()
                };
                let ptr = &self.pointers[p];
                if n.eq_ignore_ascii_case("associated") {
                    if args.len() != 1 {
                        bail!("ASSOCIATED currently supports only its POINTER argument");
                    }
                    *e = bin(BinOp::Ne, id(&ptr.tag), int(0));
                } else {
                    if ptr.rank != 1 {
                        bail!("pointer array inquiry requires a rank-one POINTER");
                    }
                    if args.len() > 2
                        || args
                            .get(1)
                            .is_some_and(|d| crate::parameters::integer_constant(d).ok() != Some(1))
                    {
                        bail!("pointer array inquiry DIM must be 1");
                    }
                    out.push(self.guard(p));
                    let count = ptr.bindings.first().map_or(0, |b| b.count) as i64;
                    let value = int(if n == "lbound" { 1 } else { count });
                    *e = if n == "shape" || n != "size" && args.len() == 1 {
                        Expr::Call("@array_constructor".into(), vec![value])
                    } else {
                        value
                    };
                }
            }
            Expr::Call(n, args) | Expr::Index(n, args) if self.pointers.contains_key(n) => {
                let p = n.clone();
                if self.pointers[&p].rank != 1 || args.len() != 1 {
                    bail!("pointer element requires one subscript");
                }
                self.expr(&mut args[0], out)?;
                out.push(self.guard(&p));
                let index = self.index(&p, args[0].clone(), out);
                *e = self.read(&p, index, out);
            }
            Expr::Section { name, .. } if self.pointers.contains_key(name) => {
                let p = name.clone();
                out.push(self.guard(&p));
                let count = self.pointers[&p].bindings.first().map_or(0, |b| b.count);
                let values = Self::section(e, count as i64)?;
                *e = if values.is_empty() {
                    let temp = self.fresh();
                    out.push(Stmt::ArrayDecl {
                        kind: self.pointers[&p].kind.clone(),
                        name: temp.clone(),
                        dims: vec![int(0)],
                    });
                    id(&temp)
                } else {
                    Expr::Call(
                        "@array_constructor".into(),
                        values
                            .into_iter()
                            .map(|i| self.read(&p, int(i), out))
                            .collect(),
                    )
                };
            }
            Expr::Call(n, _)
                if n.eq_ignore_ascii_case("associated") && !self.functions.contains(n) =>
            {
                bail!("ASSOCIATED requires a POINTER variable")
            }
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for a in args {
                    self.expr(a, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expr(a, out)?;
                self.expr(b, out)?;
            }
            Expr::Un(_, a) => self.expr(a, out)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expr(a, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn uses_pointer(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(n) | Expr::Section { name: n, .. } => self.pointers.contains_key(n),
            Expr::Call(n, a) | Expr::Index(n, a) => {
                self.pointers.contains_key(n) || a.iter().any(|e| self.uses_pointer(e))
            }
            Expr::Bin(_, a, b) => self.uses_pointer(a) || self.uses_pointer(b),
            Expr::Un(_, a) => self.uses_pointer(a),
            _ => false,
        }
    }
    fn initialize(&self, p: &str, out: &mut Vec<Stmt>) {
        let ptr = &self.pointers[p];
        let mut names = vec![ptr.tag.clone()];
        names.extend(
            ptr.bindings
                .iter()
                .flat_map(|b| b.captures.iter().map(|(n, _, _)| n.clone())),
        );
        out.push(Stmt::VarDecl {
            kind: TypeSpec::Integer(Some(8)),
            names: names.clone(),
        });
        for name in names.iter().skip(1) {
            out.push(Stmt::Assign {
                name: name.clone(),
                value: int(0),
            });
        }
        out.push(Stmt::Assign {
            name: ptr.tag.clone(),
            value: int(0),
        });
    }
    fn body(&mut self, body: &mut Vec<Stmt>, inherited: &HashSet<String>) -> Result<()> {
        let mut readonly = inherited.clone();
        for s in body.iter() {
            if let Stmt::DummyIntent { name, intent } = s {
                if intent == "@pointer_readonly_target" || intent == "intent(in)" {
                    readonly.insert(name.clone());
                }
            }
        }
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            match &mut s {
                Stmt::DummyIntent { intent, .. }
                    if matches!(
                        intent.as_str(),
                        "pointer" | "target" | "@pointer_nonunit" | "@pointer_readonly_target"
                    ) =>
                {
                    continue
                }
                Stmt::VarDecl { names, .. } => {
                    for p in names.iter().filter(|n| self.pointers.contains_key(*n)) {
                        self.initialize(p, &mut out);
                    }
                    names.retain(|n| !self.pointers.contains_key(n));
                    if names.is_empty() {
                        continue;
                    }
                }
                Stmt::ArrayDecl { name, .. } if self.pointers.contains_key(name) => {
                    self.initialize(name, &mut out);
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@pointer_associate" => {
                    let Expr::Ident(p) = &args[0] else {
                        unreachable!()
                    };
                    let p = p.clone();
                    if readonly.contains(&p) {
                        bail!("cannot change association of INTENT(IN) POINTER `{p}`");
                    }
                    let code = crate::parameters::integer_constant(&args[2])?;
                    if code != 0 {
                        let b = self.pointers[&p].bindings[code as usize - 1].clone();
                        for (capture, mut value, extent) in b.captures {
                            self.expr(&mut value, &mut out)?;
                            out.push(Stmt::Assign {
                                name: capture.clone(),
                                value,
                            });
                            out.push(require(
                                bin(
                                    BinOp::And,
                                    bin(BinOp::Ge, id(&capture), int(1)),
                                    bin(BinOp::Le, id(&capture), int(extent)),
                                ),
                                format!("pointer association target subscript outside 1:{extent}"),
                            ));
                        }
                    }
                    out.push(Stmt::Assign {
                        name: self.pointers[&p].tag.clone(),
                        value: int(code),
                    });
                    continue;
                }
                Stmt::CallSub { name, args } if name == "@nullify" => {
                    if args.is_empty() {
                        bail!("NULLIFY requires at least one POINTER variable");
                    }
                    for e in args {
                        let Expr::Ident(p) = e else {
                            bail!("NULLIFY requires POINTER variables");
                        };
                        let ptr = self
                            .pointers
                            .get(p)
                            .context("NULLIFY requires a POINTER variable")?;
                        if readonly.contains(p) {
                            bail!("cannot NULLIFY INTENT(IN) POINTER `{p}`");
                        }
                        out.push(Stmt::Assign {
                            name: ptr.tag.clone(),
                            value: int(0),
                        });
                    }
                    continue;
                }
                Stmt::Assign { name, value } if self.pointers.contains_key(name) => {
                    let p = name.clone();
                    out.push(self.guard(&p));
                    self.expr(value, &mut out)?;
                    let temp = self.fresh();
                    let ptr = &self.pointers[&p];
                    let rank = ptr.rank;
                    let count = ptr.bindings.first().map_or(0, |b| b.count);
                    if rank == 0 {
                        out.push(Stmt::VarDecl {
                            kind: ptr.kind.clone(),
                            names: vec![temp.clone()],
                        });
                    } else {
                        out.push(Stmt::ArrayDecl {
                            kind: ptr.kind.clone(),
                            name: temp.clone(),
                            dims: vec![int(count as i64)],
                        });
                    }
                    out.push(Stmt::Assign {
                        name: temp.clone(),
                        value: value.clone(),
                    });
                    for i in 1..=count {
                        self.write(
                            &p,
                            int(i as i64),
                            if rank == 0 {
                                id(&temp)
                            } else {
                                Expr::Index(temp.clone(), vec![int(i as i64)])
                            },
                            &mut out,
                            &readonly,
                        )?;
                    }
                    continue;
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } if self.pointers.contains_key(name) => {
                    let p = name.clone();
                    if indices.len() != 1
                        || self.pointers[&p].rank != 1
                        || matches!(&indices[0], Expr::Section { .. })
                        || matches!(&indices[0],Expr::Call(n,_) if n=="@vector_subscript" || n=="@section_subscript")
                    {
                        bail!("pointer section assignment is not yet supported");
                    }
                    out.push(self.guard(&p));
                    self.expr(&mut indices[0], &mut out)?;
                    let index = self.index(&p, indices[0].clone(), &mut out);
                    self.expr(value, &mut out)?;
                    let temp = self.fresh();
                    out.push(Stmt::VarDecl {
                        kind: self.pointers[&p].kind.clone(),
                        names: vec![temp.clone()],
                    });
                    out.push(Stmt::Assign {
                        name: temp.clone(),
                        value: value.clone(),
                    });
                    self.write(&p, index, id(&temp), &mut out, &readonly)?;
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
                Stmt::CallSub { name, args } => {
                    if (name.contains("read")
                        || !name.starts_with('@')
                        || matches!(name.as_str(), "@allocate" | "@deallocate" | "@undefine"))
                        && args.iter().any(|e| self.uses_pointer(e))
                    {
                        bail!("POINTER arguments to `{name}` are not yet supported");
                    }
                    for e in args {
                        self.expr(e, &mut out)?;
                    }
                }
                Stmt::Read { args } => {
                    if args.iter().any(|e| self.uses_pointer(e)) {
                        bail!("READ into POINTER storage is not yet supported");
                    }
                }
                Stmt::If { cond, .. } => self.expr(cond, &mut out)?,
                Stmt::Do {
                    start, end, var, ..
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
        *body = out;
        Ok(())
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
    let mut functions = HashSet::new();
    known(&mut p.body, &mut functions);
    fn unit(body: &mut Vec<Stmt>, functions: &HashSet<String>) -> Result<()> {
        let mut lower = Lower {
            vars: HashMap::new(),
            pointers: HashMap::new(),
            functions: functions.clone(),
            serial: 0,
        };
        lower.declarations(body, 0)?;
        lower.associations(body)?;
        lower.body(body, &HashSet::new())?;
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
                    if pointers.contains(name) || body.iter().any(|s|matches!(s,Stmt::Return(Some(Expr::Ident(n))) if pointers.contains(n))) {bail!("POINTER function results are not yet supported");}
                }
                if let Stmt::Function { body, params, .. } | Stmt::Subroutine { body, params, .. } =
                    s
                {
                    if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if intent=="pointer" && params.contains(name))) {bail!("POINTER dummy arguments require local subroutine specialization; POINTER functions are not yet supported");}
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
    unit(&mut p.body, &functions)?;
    crate::substrings::hoist(&mut p.body);
    Ok(p)
}

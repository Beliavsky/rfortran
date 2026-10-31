// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! COMMON blocks as modules. Each block becomes a module whose variables are
//! the block's objects by position, and each scoping unit with the block USEs
//! that module, renaming the variables to its own names for them.
//!
//! A block whose units declare different layouts, or that a unit extends or
//! overlays by EQUIVALENCE, instead has one storage array in its module. Each
//! unit's objects of the block, and the objects equivalenced to them, are
//! views of that array from their element offsets (see crate::equivalence).
//! The objects of such a block must have elements of one size.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

/// The statement `COMMON /b1/ x, y(3) /b2/ z`: each block name, as a
/// string (empty for blank COMMON), then its objects.
pub(crate) const STATEMENT: &str = "@common";

#[derive(Clone)]
struct Object {
    kind: TypeSpec,
    dims: Vec<Expr>,
}

struct Layout {
    unit: String,
    objects: Vec<Object>,
}

type Rules = Option<HashMap<char, TypeSpec>>;

#[derive(Default)]
struct Context {
    layouts: HashMap<String, Layout>,
    order: Vec<String>,
    /// The analysis pass records which blocks need storage, without rewriting.
    analysis: bool,
    mismatched: HashSet<String>,
    equivalenced: HashSet<String>,
    /// Blocks with a storage array, and the type of its elements.
    storage: HashMap<String, TypeSpec>,
    /// The number of elements of each storage array.
    extents: HashMap<String, i64>,
    /// The types of every unit's objects of each block, from the analysis.
    kinds: HashMap<String, Vec<TypeSpec>>,
}

fn storage_name(block: &str) -> String {
    format!("{}_storage", if block.is_empty() { "blank" } else { block })
}

/// The EQUIVALENCE sets of a unit, by statement.
fn equivalence_sets(body: &[Stmt]) -> Vec<Vec<Expr>> {
    body.iter().filter_map(|s| match s {
        Stmt::CallSub { name, args } if name == crate::equivalence::STATEMENT => Some(args.clone()),
        _ => None,
    }).collect()
}

fn mentions(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n) | Expr::Section { name: n, .. } => n == name,
        Expr::Call(n, args) | Expr::Index(n, args) => n == name || args.iter().any(|a| mentions(a, name)),
        _ => false,
    }
}

/// Whether a module holds a COMMON block.
pub(crate) fn is_block_module(name: &str) -> bool {
    name.starts_with("rfort_common_") || name == "rfort_blank_common"
}

fn module_name(block: &str) -> String {
    if block.is_empty() { "rfort_blank_common".into() } else { format!("rfort_common_{block}") }
}

fn variable_name(block: &str, position: usize) -> String {
    format!("{}_{position}", if block.is_empty() { "blank" } else { block })
}

fn display(block: &str) -> String {
    if block.is_empty() { "blank COMMON".into() } else { format!("COMMON block /{block}/") }
}

/// The implicit typing rules of a unit, starting from its host's.
fn rules(body: &[Stmt], host: &Rules) -> Rules {
    let mut rules = host.clone()?;
    for s in body {
        let Stmt::Implicit(implicit) = s else { continue };
        let mut set = |kind: &TypeSpec, ranges: &[crate::ast::LetterRange]| {
            for r in ranges {
                for c in r.start.to_ascii_lowercase()..=r.end.unwrap_or(r.start).to_ascii_lowercase() {
                    rules.insert(c, kind.clone());
                }
            }
        };
        match implicit {
            Implicit::None => return None,
            Implicit::Rule { type_spec, letter_ranges } => set(type_spec, letter_ranges),
            Implicit::Rules { rules: list } => for rule in list { set(&rule.type_spec, &rule.letter_ranges) },
        }
    }
    Some(rules)
}

/// Replace named constants of the unit in an array bound, so that the
/// block's module, which cannot see them, gets constant bounds.
fn substitute(e: &Expr, constants: &HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Ident(n) => constants.get(n).map_or_else(|| e.clone(), |v| substitute(v, constants)),
        Expr::Bin(op, a, b) => Expr::Bin(op.clone(), Box::new(substitute(a, constants)), Box::new(substitute(b, constants))),
        Expr::Un(op, a) => Expr::Un(op.clone(), Box::new(substitute(a, constants))),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(|a| substitute(a, constants)).collect()),
        _ => e.clone(),
    }
}

/// A bound as a constant where it can be evaluated.
fn bound(e: &Expr, constants: &HashMap<String, Expr>, block: &str, name: &str) -> Result<Expr> {
    let e = substitute(e, constants);
    let constant = |e: &Expr| -> Result<Expr> {
        match crate::parameters::integer_constant(e) {
            Ok(v) => Ok(Expr::IntLit(v.to_string())),
            Err(_) => bail!("array `{name}` in {} requires constant bounds", display(block)),
        }
    };
    match &e {
        Expr::Call(n, args) if n == "@declared_bounds" && args.len() == 2 =>
            Ok(Expr::Call(n.clone(), vec![constant(&args[0])?, constant(&args[1])?])),
        _ => constant(&e),
    }
}

fn unit(body: &mut Vec<Stmt>, label: &str, host: &Rules, internal: Option<&[String]>, ctx: &mut Context) -> Result<()> {
    let unit_rules = rules(body, host);
    let constants: HashMap<String, Expr> = body.iter().flat_map(|s| match s {
        Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter")) =>
            entities.iter().filter_map(|(n, v)| v.clone().map(|v| (n.clone(), v))).collect::<Vec<_>>(),
        _ => vec![],
    }).collect();
    let mut statements = vec![];
    body.retain(|s| match s {
        Stmt::CallSub { name, args } if name == STATEMENT => { statements.push(args.clone()); false }
        _ => true,
    });
    let mut uses = vec![];
    // The objects of each block, in order, across all COMMON statements of the unit.
    let mut members: Vec<(String, Vec<Expr>)> = vec![];
    for args in statements {
        let mut block = None;
        for a in args {
            match a {
                Expr::Str(name) => {
                    let name = name.to_ascii_lowercase();
                    if !members.iter().any(|(b, _)| *b == name) { members.push((name.clone(), vec![])); }
                    block = Some(name);
                }
                object => {
                    let block = block.as_ref().expect("COMMON objects follow a block name");
                    members.iter_mut().find(|(b, _)| b == block).unwrap().1.push(object);
                }
            }
        }
    }
    for (block, list) in members {
        let mut objects = vec![];
        let mut renames = vec![];
        for (position, object) in list.iter().enumerate() {
            let (name, common_dims) = match object {
                Expr::Ident(n) => (n.clone(), vec![]),
                Expr::Call(n, dims) => (n.clone(), dims.clone()),
                _ => bail!("invalid object in {}", display(&block)),
            };
            if renames.iter().any(|(n, _): &(String, String)| *n == name) {
                bail!("`{name}` appears more than once in {}", display(&block));
            }
            let mut kind = None;
            let mut dims = common_dims.clone();
            let mut found = false;
            for s in body.iter_mut() {
                match s {
                    Stmt::ArrayDecl { kind: k, name: n, dims: d } if *n == name => {
                        if !common_dims.is_empty() { bail!("array `{name}` has bounds in both a declaration and {}", display(&block)); }
                        kind = Some(k.clone());
                        dims = d.clone();
                        found = true;
                    }
                    Stmt::VarDecl { kind: k, names } if names.contains(&name) => {
                        names.retain(|n| *n != name);
                        kind = Some(k.clone());
                    }
                    Stmt::InitializedDecl { entities, .. } if entities.iter().any(|(n, _)| *n == name) => {
                        bail!("`{name}` in {} cannot be initialized in its declaration; BLOCK DATA is not yet supported", display(&block));
                    }
                    Stmt::DerivedDecl { names, .. } if names.contains(&name) => {
                        bail!("derived-type object `{name}` in {} is not yet supported", display(&block));
                    }
                    _ => {}
                }
            }
            if found { body.retain(|s| !matches!(s, Stmt::ArrayDecl { name: n, .. } if *n == name)); }
            body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
            let kind = match kind {
                Some(kind) => kind,
                None => {
                    let letter = name.chars().next().unwrap_or('a').to_ascii_lowercase();
                    match unit_rules.as_ref().and_then(|r| r.get(&letter)) {
                        Some(kind) => kind.clone(),
                        None => bail!("`{name}` in {} has no type under IMPLICIT NONE", display(&block)),
                    }
                }
            };
            if !matches!(kind, TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::RealKind(_)
                | TypeSpec::Logical | TypeSpec::LogicalKind(_) | TypeSpec::Complex(_) | TypeSpec::Character(Some(_))) {
                bail!("`{name}` in {}: only INTEGER, REAL, DOUBLE PRECISION, COMPLEX, LOGICAL and fixed-length CHARACTER objects are supported", display(&block));
            }
            let dims = dims.iter().map(|d| bound(d, &constants, &block, &name)).collect::<Result<Vec<_>>>()?;
            renames.push((name, variable_name(&block, position + 1)));
            objects.push(Object { kind, dims });
        }
        if ctx.analysis && renames.iter().any(|(n, _)| equivalence_sets(body).iter().flatten().any(|e| mentions(e, n))) {
            ctx.equivalenced.insert(block.clone());
        }
        if ctx.analysis {
            ctx.kinds.entry(block.clone()).or_default().extend(objects.iter().map(|o| o.kind.clone()));
        }
        if let Some(kind) = ctx.storage.get(&block).cloned() {
            if !ctx.layouts.contains_key(&block) {
                ctx.order.push(block.clone());
                ctx.layouts.insert(block.clone(), Layout { unit: label.into(), objects: vec![] });
            }
            uses.extend(storage_views(body, &block, &kind, &renames, &objects, &unit_rules, ctx)?);
            continue;
        }
        match ctx.layouts.get(&block) {
            None => {
                ctx.order.push(block.clone());
                ctx.layouts.insert(block.clone(), Layout { unit: label.into(), objects });
            }
            Some(first) => {
                let same = first.objects.len() == objects.len() && first.objects.iter().zip(&objects).all(|(a, b)|
                    format!("{:?}", a.kind) == format!("{:?}", b.kind) && format!("{:?}", a.dims) == format!("{:?}", b.dims));
                if !same {
                    if !ctx.analysis {
                        bail!("{} has different layouts in {} and {}; COMMON objects must match in number, type, kind and shape", display(&block), first.unit, label);
                    }
                    ctx.mismatched.insert(block.clone());
                }
            }
        }
        uses.push(Stmt::Use {
            module: module_name(&block),
            nature: None,
            only: Some(renames.iter().map(|(_, remote)| remote.clone()).collect()),
            renames,
        });
    }
    if !uses.is_empty() {
        let at = body.iter().position(|s| !matches!(s, Stmt::Use { .. } | Stmt::SourceLocation(_))).unwrap_or(body.len());
        body.splice(at..at, uses);
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                // A procedure of the main program inherits its implicit rules;
                // an external procedure has the default rules.
                let host = if internal.is_none_or(|c| c.contains(name)) { unit_rules.clone() } else { default_rules() };
                let label = format!("`{name}`");
                unit(body, &label, &host, None, ctx)?;
            }
            Stmt::Module { name, body } => {
                let label = format!("module `{name}`");
                unit(body, &label, &default_rules(), None, ctx)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// The statements that make a unit's objects of `block`, and the objects
/// equivalenced to them, views of the block's storage array: a USE of the
/// array, the objects' declarations and their `@storage_view` markers. The
/// EQUIVALENCE sets placed here are removed from the unit.
fn storage_views(body: &mut Vec<Stmt>, block: &str, kind: &TypeSpec, renames: &[(String, String)], objects: &[Object], unit_rules: &Rules, ctx: &mut Context) -> Result<Vec<Stmt>> {
    let bytes = crate::equivalence::element_bytes(kind).unwrap();
    let storage = storage_name(block);
    let mut placed: Vec<(String, i64, i64)> = vec![];
    let mut out = vec![Stmt::Use { module: module_name(block), nature: None, only: Some(vec![storage.clone()]), renames: vec![(storage.clone(), storage.clone())] }];
    let mut offset = 0;
    for ((name, _), o) in renames.iter().zip(objects) {
        let shape = crate::equivalence::shape_of(&o.kind, &o.dims, body, name)?;
        if shape.bytes % bytes != 0 {
            bail!("{} has objects whose elements have different sizes, such as `{name}`; storage association between them is not yet supported", display(block));
        }
        let size = shape.size() * (shape.bytes / bytes);
        placed.push((name.clone(), offset, size));
        offset += size;
        out.push(if o.dims.is_empty() { Stmt::VarDecl { kind: o.kind.clone(), names: vec![name.clone()] } }
            else { Stmt::ArrayDecl { kind: o.kind.clone(), name: name.clone(), dims: o.dims.clone() } });
    }
    // Place the objects of the EQUIVALENCE sets that reach the block.
    let declared = |name: &str| -> Option<Result<crate::equivalence::Shape>> {
        body.iter().find_map(|s| match s {
            Stmt::ArrayDecl { kind, name: n, dims } if n == name => Some(crate::equivalence::shape_of(kind, dims, body, n)),
            Stmt::VarDecl { kind, names } if names.iter().any(|n| n == name) => Some(crate::equivalence::shape_of(kind, &[], body, name)),
            Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| n == name) => Some(crate::equivalence::shape_of(kind, &[], body, name)),
            _ => None,
        })
    };
    let mut sets: Vec<(Expr, bool)> = equivalence_sets(body).into_iter().flatten().map(|s| (s, false)).collect();
    loop {
        let mut progress = false;
        for (set, used) in sets.iter_mut().filter(|(_, used)| !*used) {
            let Expr::Call(_, items) = set else { continue };
            let mut shapes = vec![];
            for item in items.iter() {
                let name = crate::equivalence::object_name(item)?;
                let shape = match objects.iter().zip(renames).find(|(_, (n, _))| *n == name) {
                    Some((o, _)) => crate::equivalence::shape_of(&o.kind, &o.dims, body, &name)?,
                    None => match declared(&name) {
                        Some(shape) => shape?,
                        // An undeclared object has its implicit type.
                        None => match name.chars().next().and_then(|c| unit_rules.as_ref()?.get(&c.to_ascii_lowercase()).cloned()) {
                            Some(kind) => crate::equivalence::shape_of(&kind, &[], body, &name)?,
                            None => bail!("EQUIVALENCE object `{name}` has no type under IMPLICIT NONE"),
                        },
                    },
                };
                // The object's offset in storage elements.
                let at = crate::equivalence::object_offset(item, &shape)?;
                if at % bytes != 0 || shape.bytes % bytes != 0 {
                    bail!("EQUIVALENCE of `{name}` with {} does not align with its elements; this is not yet supported", display(block));
                }
                shapes.push((name, at / bytes, shape));
            }
            // The storage element the set's objects share.
            let Some(point) = shapes.iter().find_map(|(n, at, _)| placed.iter().find(|(p, _, _)| p == n).map(|(_, start, _)| start + at)) else { continue };
            for (name, at, shape) in shapes {
                let start = point - at;
                match placed.iter().find(|(p, _, _)| *p == name) {
                    Some((_, old, _)) if *old != start => bail!("EQUIVALENCE gives `{name}` two different positions in {}", display(block)),
                    Some(_) => {}
                    None => {
                        if start < 0 { bail!("EQUIVALENCE of `{name}` extends {} before its first object", display(block)); }
                        if objects.iter().zip(renames).all(|(_, (n, _))| *n != name) {
                            if let Some(other) = ctx.storage.keys().find(|b| *b != block && body.iter().any(|s| matches!(s, Stmt::CallSub { name: v, args } if v == crate::equivalence::VIEW && matches!(args.first(), Some(Expr::Ident(n)) if *n == name) && matches!(args.get(1), Some(Expr::Ident(st)) if *st == storage_name(b))))) {
                                bail!("EQUIVALENCE associates {} and {}; this is not yet supported", display(block), display(other));
                            }
                        }
                        placed.push((name, start, shape.size() * (shape.bytes / bytes)));
                    }
                }
            }
            *used = true;
            progress = true;
        }
        if !progress { break; }
    }
    // Remove the placed sets; the rest are ordinary EQUIVALENCE.
    let remaining: Vec<Expr> = sets.into_iter().filter(|(_, used)| !used).map(|(s, _)| s).collect();
    body.retain(|s| !matches!(s, Stmt::CallSub { name, .. } if name == crate::equivalence::STATEMENT));
    if !remaining.is_empty() { body.push(Stmt::CallSub { name: crate::equivalence::STATEMENT.into(), args: remaining }); }
    let end = placed.iter().map(|(_, start, size)| start + size).max().unwrap_or(0);
    let extent = ctx.extents.entry(block.into()).or_insert(0);
    *extent = (*extent).max(end);
    for (name, start, _) in placed {
        out.push(Stmt::CallSub { name: crate::equivalence::VIEW.into(), args: vec![Expr::Ident(name), Expr::Ident(storage.clone()), Expr::IntLit(start.to_string())] });
    }
    Ok(out)
}

fn default_rules() -> Rules {
    Some(('a'..='z').map(|c| (c, if ('i'..='n').contains(&c) { TypeSpec::Integer(None) } else { TypeSpec::Real })).collect())
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let contains = program.contains.clone();
    // Find the blocks that need storage: different layouts, or EQUIVALENCE.
    let mut analysis = Context { analysis: true, ..Default::default() };
    unit(&mut program.body.clone(), "the main program", &default_rules(), Some(&contains), &mut analysis)?;
    let mut ctx = Context::default();
    for block in analysis.mismatched.union(&analysis.equivalenced) {
        let first = &analysis.layouts[block].objects;
        let Some(object) = first.first() else { continue };
        // One type, or for objects of different sizes, elements of their
        // greatest common size: CHARACTER for CHARACTER objects, otherwise
        // INTEGER. EQUIVALENCE may still bring other sizes (see storage_views).
        let kinds = &analysis.kinds[block];
        fn gcd(a: i64, b: i64) -> i64 { if b == 0 { a } else { gcd(b, a % b) } }
        let sizes: Vec<i64> = kinds.iter().filter_map(crate::equivalence::element_bytes).collect();
        let unit = sizes.iter().copied().reduce(gcd).unwrap_or(1);
        let kind = if sizes.iter().all(|s| *s == unit) && kinds.iter().all(|k| format!("{k:?}") == format!("{:?}", object.kind)) {
            object.kind.clone()
        } else if kinds.iter().all(|k| matches!(k, TypeSpec::Character(_))) {
            TypeSpec::Character(Some(unit as usize))
        } else if matches!(unit, 1 | 2 | 4 | 8) && !kinds.iter().any(|k| matches!(k, TypeSpec::Character(_))) {
            TypeSpec::Integer(Some(unit as u8))
        } else {
            object.kind.clone()
        };
        ctx.storage.insert(block.clone(), kind);
    }
    unit(&mut program.body, "the main program", &default_rules(), Some(&contains), &mut ctx)?;
    let (layouts, order) = (&ctx.layouts, &ctx.order);
    let modules: Vec<Stmt> = order.iter().map(|block| {
        if let Some(kind) = ctx.storage.get(block) {
            let body = vec![Stmt::ArrayDecl { kind: kind.clone(), name: storage_name(block), dims: vec![Expr::IntLit(ctx.extents[block].to_string())] }];
            return Stmt::Module { name: module_name(block), body };
        }
        let layout = &layouts[block];
        let body = layout.objects.iter().enumerate().map(|(i, o)| {
            let name = variable_name(block, i + 1);
            if o.dims.is_empty() { Stmt::VarDecl { kind: o.kind.clone(), names: vec![name] } }
            else { Stmt::ArrayDecl { kind: o.kind.clone(), name, dims: o.dims.clone() } }
        }).collect();
        Stmt::Module { name: module_name(block), body }
    }).collect();
    program.body.splice(0..0, modules);
    Ok(program)
}

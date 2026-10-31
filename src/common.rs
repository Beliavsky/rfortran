// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! COMMON blocks as modules. Each block becomes a module whose variables are
//! the block's objects by position, and each scoping unit with the block USEs
//! that module, renaming the variables to its own names for them. The units
//! that declare a block must agree on the type, kind and shape of each object;
//! storage association between different layouts is not yet supported.
use crate::ast::{Expr, Implicit, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;

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

fn unit(body: &mut Vec<Stmt>, label: &str, host: &Rules, internal: Option<&[String]>,
        layouts: &mut HashMap<String, Layout>, order: &mut Vec<String>) -> Result<()> {
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
        match layouts.get(&block) {
            None => {
                order.push(block.clone());
                layouts.insert(block.clone(), Layout { unit: label.into(), objects });
            }
            Some(first) => {
                let same = first.objects.len() == objects.len() && first.objects.iter().zip(&objects).all(|(a, b)|
                    format!("{:?}", a.kind) == format!("{:?}", b.kind) && format!("{:?}", a.dims) == format!("{:?}", b.dims));
                if !same {
                    bail!("{} has different layouts in {} and {}; COMMON objects must match in number, type, kind and shape (storage association between different layouts is not yet supported)",
                        display(&block), first.unit, label);
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
                unit(body, &label, &host, None, layouts, order)?;
            }
            Stmt::Module { name, body } => {
                let label = format!("module `{name}`");
                unit(body, &label, &default_rules(), None, layouts, order)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn default_rules() -> Rules {
    Some(('a'..='z').map(|c| (c, if ('i'..='n').contains(&c) { TypeSpec::Integer(None) } else { TypeSpec::Real })).collect())
}

pub fn resolve(mut program: Program) -> Result<Program> {
    let mut layouts = HashMap::new();
    let mut order = vec![];
    let contains = program.contains.clone();
    unit(&mut program.body, "the main program", &default_rules(), Some(&contains), &mut layouts, &mut order)?;
    let modules: Vec<Stmt> = order.iter().map(|block| {
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

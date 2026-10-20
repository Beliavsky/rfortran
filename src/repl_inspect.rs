// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky

//! Inspect committed snapshots and constant declarations, never user execution.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

pub(super) struct View {
    pub shapes: HashMap<String, Vec<i64>>,
    constants: HashMap<String, Expr>,
    arrays: super::arrays::Values,
}
fn constant(value: &Expr) -> Result<Expr> {
    if let Ok(n) = crate::parameters::integer_constant(value) {
        return Ok(Expr::IntLit(n.to_string()));
    }
    if let Ok(n) = crate::parameters::real_constant(value) {
        return Ok(Expr::RealLit(n.to_string()));
    }
    if let Ok(s) = crate::parameters::character_constant(value) {
        return Ok(Expr::Str(s));
    }
    Ok(Expr::Logical(crate::parameters::logical_constant(
        value,
        &HashMap::new(),
    )?))
}
impl View {
    pub(super) fn new(session: &super::Session) -> Result<Self> {
        let prepared = super::dynamic::prepare(
            Program {
                name: "<repl-inspect>".into(),
                body: session.declarations.clone(),
                contains: vec![],
            },
            &session.allocations,
            &session.names,
            false,
        )?;
        let mut body = prepared.program.body;
        let array_names: HashSet<_> = body
            .iter()
            .filter_map(|s| match s {
                Stmt::ArrayDecl { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        let mut scalar_parameters = vec![];
        let mut array_parameters = HashSet::new();
        for stmt in &body {
            if let Stmt::InitializedDecl {
                attributes,
                entities,
                ..
            } = stmt
            {
                if attributes.iter().any(|a| a == "parameter") {
                    for (name, _) in entities {
                        if array_names.contains(name) {
                            array_parameters.insert(name.clone());
                        } else {
                            scalar_parameters.push(name.clone());
                        }
                    }
                }
            }
        }
        body.push(Stmt::Print {
            items: scalar_parameters.iter().cloned().map(Expr::Ident).collect(),
        });
        let program = crate::parameters::resolve(Program {
            name: "<repl-inspect>".into(),
            body,
            contains: vec![],
        })?;
        let mut view = Self {
            shapes: HashMap::new(),
            constants: HashMap::new(),
            arrays: session.arrays.clone(),
        };
        let mut lowers = HashMap::new();
        for stmt in &program.body {
            match stmt {
                Stmt::ArrayDecl { name, dims, .. } => {
                    let mut lower = vec![];
                    let mut shape = vec![];
                    for dim in dims {
                        if let Expr::Call(tag, bounds) = dim {
                            if tag == "@declared_bounds" {
                                let first = crate::parameters::integer_constant(&bounds[0])?;
                                let last = crate::parameters::integer_constant(&bounds[1])?;
                                lower.push(first);
                                shape.push((last - first + 1).max(0));
                                continue;
                            }
                        }
                        lower.push(1);
                        shape.push(crate::parameters::integer_constant(dim)?.max(0));
                    }
                    if array_parameters.contains(name) {
                        view.arrays.insert(
                            name.clone(),
                            vec![None; shape.iter().product::<i64>() as usize],
                        );
                    }
                    lowers.insert(name.clone(), lower);
                    view.shapes.insert(name.clone(), shape);
                }
                Stmt::Print { items } => {
                    for (name, value) in scalar_parameters.iter().zip(items) {
                        view.constants.insert(name.clone(), constant(value)?);
                    }
                }
                _ => {}
            }
        }
        for stmt in &program.body {
            if let Stmt::AssignIndex {
                name,
                indices,
                value,
            } = stmt
            {
                if array_parameters.contains(name) && indices.len() == 1 {
                    let offset =
                        crate::parameters::integer_constant(&indices[0])? - lowers[name][0];
                    let values = view.arrays.get_mut(name).unwrap();
                    if let Some(slot) = usize::try_from(offset).ok().and_then(|i| values.get_mut(i))
                    {
                        *slot = Some(constant(value)?);
                    }
                }
            }
        }
        Ok(view)
    }
    pub(super) fn value(&self, name: &str, session: &super::Session) -> String {
        if session
            .allocations
            .get(name)
            .is_some_and(|a| a.extent.is_none())
        {
            return "<unallocated>".into();
        }
        if let Some(values) = self.arrays.get(name) {
            let indices: Vec<_> = if values.len() <= 6 {
                (0..values.len()).map(Some).collect()
            } else {
                (0..3)
                    .map(Some)
                    .chain([None])
                    .chain((values.len() - 3..values.len()).map(Some))
                    .collect()
            };
            let values = indices
                .into_iter()
                .map(|i| match i {
                    None => "...".into(),
                    Some(i) => values[i].as_ref().map_or("<unset>".into(), text),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{values}]")
        } else {
            session
                .values
                .get(name)
                .or_else(|| self.constants.get(name))
                .map_or("<unset>".into(), text)
        }
    }
    pub(super) fn shape(&self, name: &str, session: &super::Session, size: bool) -> Result<String> {
        let shape = self
            .shapes
            .get(name)
            .with_context(|| format!("`{name}` is not an array"))?;
        if session
            .allocations
            .get(name)
            .is_some_and(|a| a.extent.is_none())
        {
            return Ok("<unallocated>".into());
        }
        if size {
            Ok(shape.iter().product::<i64>().to_string())
        } else {
            Ok(format!(
                "[{}]",
                shape
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }
}
fn text(value: &Expr) -> String {
    match value {
        Expr::IntLit(n) | Expr::RealLit(n) => n.clone(),
        Expr::Logical(b) => if *b { "T" } else { "F" }.into(),
        Expr::Str(s) => format!("'{}'", s.replace('\'', "''")),
        _ => "<unset>".into(),
    }
}

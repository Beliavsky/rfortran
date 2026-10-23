// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! STORAGE_SIZE(A [, KIND]): the size in bits of an element of A, a constant
//! resolved before derived types are lowered. A derived type is laid out as
//! gfortran lays it out, as a C structure: each component at the next
//! multiple of its alignment, and the whole padded to its largest alignment.
//! Types with ALLOCATABLE or POINTER components are left unresolved.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::Result;
use std::collections::HashMap;

/// A declared entity: an intrinsic type or the name of a derived type.
#[derive(Clone)]
enum Declared {
    Intrinsic(TypeSpec),
    Derived(String),
}
type Scope = HashMap<String, Declared>;

struct Layout {
    types: HashMap<String, Vec<Stmt>>,
    constants: HashMap<String, Expr>,
    globals: Scope,
}

fn align(offset: u64, alignment: u64) -> u64 {
    offset.div_ceil(alignment.max(1)) * alignment.max(1)
}

impl Layout {
    /// The size in bytes and the alignment of a value of an intrinsic type.
    fn intrinsic(&self, kind: &TypeSpec) -> Option<(u64, u64)> {
        let kind = crate::parameters::concrete_kind_with(kind.clone(), &self.constants).ok()?;
        Some(match kind {
            TypeSpec::Integer(k) => {
                let k = u64::from(k.unwrap_or(4));
                (k, k)
            }
            TypeSpec::Real => (4, 4),
            TypeSpec::DoublePrecision => (8, 8),
            TypeSpec::Logical => (4, 4),
            TypeSpec::Character(n) => (n.unwrap_or(1) as u64, 1),
            TypeSpec::Complex(k) => {
                let k: u64 = k.parse().ok()?;
                (2 * k, k)
            }
            _ => return None,
        })
    }
    fn count(&self, dims: &[Expr]) -> Option<u64> {
        dims.iter().try_fold(1u64, |n, d| {
            let extent = crate::parameters::constant_integer_with(d, &self.constants).ok()?;
            n.checked_mul(u64::try_from(extent.max(0)).ok()?)
        })
    }
    /// The size in bytes and the alignment of a value of a derived type.
    fn derived(&self, name: &str) -> Option<(u64, u64)> {
        let components = self.types.get(&name.to_ascii_lowercase())?;
        let mut fields: Vec<(u64, u64, u64)> = vec![];
        let mut indirect = std::collections::HashSet::new();
        for c in components {
            match c {
                Stmt::DummyIntent { name, intent } if matches!(intent.as_str(), "allocatable" | "pointer") => {
                    indirect.insert(name.clone());
                }
                Stmt::DerivedDecl { attributes, .. } if attributes.iter().any(|a| matches!(a.as_str(), "allocatable" | "pointer")) => return None,
                _ => {}
            }
        }
        for c in components {
            match c {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        if indirect.contains(n) { return None; }
                        let (size, alignment) = self.intrinsic(kind)?;
                        fields.push((size, alignment, 1));
                    }
                }
                Stmt::InitializedDecl { kind, entities, .. } => {
                    for (n, _) in entities {
                        if indirect.contains(n) { return None; }
                        let (size, alignment) = self.intrinsic(kind)?;
                        fields.push((size, alignment, 1));
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    if indirect.contains(name) { return None; }
                    let (size, alignment) = self.intrinsic(kind)?;
                    fields.push((size, alignment, self.count(dims)?));
                }
                Stmt::DerivedDecl { type_name, names, dims, .. } => {
                    let (size, alignment) = self.derived(type_name)?;
                    for n in names {
                        if indirect.contains(n) { return None; }
                        fields.push((size, alignment, self.count(dims)?));
                    }
                }
                _ => {}
            }
        }
        let mut offset = 0;
        let mut largest = 1;
        for (size, alignment, count) in fields {
            offset = align(offset, alignment) + size.checked_mul(count)?;
            largest = largest.max(alignment);
        }
        Some((align(offset, largest), largest))
    }
    fn bits(&self, declared: &Declared) -> Option<u64> {
        let (size, _) = match declared {
            Declared::Intrinsic(kind) => self.intrinsic(kind)?,
            Declared::Derived(name) => self.derived(name)?,
        };
        size.checked_mul(8)
    }
}

fn declarations(body: &[Stmt], scope: &mut Scope) {
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names { scope.insert(n.to_ascii_lowercase(), Declared::Intrinsic(kind.clone())); }
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities { scope.insert(n.to_ascii_lowercase(), Declared::Intrinsic(kind.clone())); }
            }
            Stmt::ArrayDecl { kind, name, .. } => {
                scope.insert(name.to_ascii_lowercase(), Declared::Intrinsic(kind.clone()));
            }
            Stmt::DerivedDecl { type_name, names, .. } => {
                for n in names { scope.insert(n.to_ascii_lowercase(), Declared::Derived(type_name.clone())); }
            }
            _ => {}
        }
    }
}

fn collect(body: &[Stmt], types: &mut HashMap<String, Vec<Stmt>>, constants: &mut HashMap<String, Expr>, globals: &mut Scope) {
    for s in body {
        match s {
            Stmt::DerivedType { name, components, .. } => {
                types.insert(name.to_ascii_lowercase(), components.clone());
            }
            Stmt::InitializedDecl { attributes, entities, .. } if attributes.iter().any(|a| a == "parameter") => {
                for (n, value) in entities {
                    if let Some(value) = value { constants.insert(n.to_ascii_lowercase(), value.clone()); }
                }
            }
            Stmt::Module { body, .. } => {
                declarations(body, globals);
                collect(body, types, constants, globals);
            }
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Block { body } => collect(body, types, constants, globals),
            _ => {}
        }
    }
}

impl Layout {
    fn rewrite(&self, body: &mut [Stmt], inherited: &Scope) {
        let mut scope = inherited.clone();
        declarations(body, &mut scope);
        for s in body.iter_mut() {
            for e in crate::procedure_arguments::expressions(s) {
                crate::procedure_arguments::visit_expr(e, &mut |e| self.replace(e, &scope));
            }
            for child in crate::procedure_arguments::children(s) {
                self.rewrite(child, &scope);
            }
        }
    }
    fn replace(&self, e: &mut Expr, scope: &Scope) {
        let Expr::Call(name, args) = e else { return };
        if !name.eq_ignore_ascii_case("storage_size") || args.is_empty() || args.len() > 2 {
            return;
        }
        let value = |a: &Expr| crate::named_arguments::keyword(a).map_or(a.clone(), |(_, v)| v.clone());
        let source = args.iter().find(|a| matches!(crate::named_arguments::keyword(a), Some(("a", _)))).map(value)
            .unwrap_or_else(|| value(&args[0]));
        let kind = args.iter().find(|a| matches!(crate::named_arguments::keyword(a), Some(("kind", _)))).map(value)
            .or_else(|| args.get(1).filter(|a| crate::named_arguments::keyword(a).is_none()).cloned());
        let base = match &source {
            Expr::Ident(n) | Expr::Index(n, _) | Expr::Call(n, _) | Expr::Section { name: n, .. } => n.to_ascii_lowercase(),
            _ => return,
        };
        // A component designator, as `x%r`, names a component, not a variable.
        if base.contains('%') { return; }
        let Some(declared) = scope.get(&base).or_else(|| self.globals.get(&base)) else { return };
        let Some(bits) = self.bits(declared) else { return };
        let bits = Expr::IntLit(bits.to_string());
        *e = match kind {
            Some(kind) => Expr::Call("int".into(), vec![bits, kind]),
            None => bits,
        };
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    if !serde_json::to_string(&program.body).is_ok_and(|t| t.to_ascii_lowercase().contains("\"storage_size\"")) {
        return Ok(program);
    }
    let (mut types, mut constants, mut globals) = (HashMap::new(), HashMap::new(), Scope::new());
    collect(&program.body, &mut types, &mut constants, &mut globals);
    let layout = Layout { types, constants, globals };
    layout.rewrite(&mut program.body, &Scope::new());
    Ok(program)
}

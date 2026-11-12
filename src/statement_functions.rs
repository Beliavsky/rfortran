// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Statement functions, such as `cabs1(z) = abs(dble(z)) + abs(dimag(z))`.
//! The parser reads a definition as an assignment to an element of a scalar.
//! Each reference is replaced by the defining expression with the actual
//! arguments in place of the dummies, converted to the function's type, so
//! later passes see an ordinary expression of any type, COMPLEX included.
//! CHARACTER has fixed lengths: an actual of a CHARACTER dummy, and the
//! value of a CHARACTER function, are assigned to temporaries of the declared
//! lengths before the statement, which pads or truncates them.
use crate::ast::{Expr, Program, Stmt, TypeSpec};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

struct Definition {
    dummies: Vec<String>,
    value: Expr,
    /// The declared CHARACTER type of the function, and of each dummy.
    result: Option<TypeSpec>,
    dummy_kinds: Vec<Option<TypeSpec>>,
    /// The declared type of each dummy.
    dummy_types: Vec<Option<TypeSpec>>,
}

/// A CHARACTER type with a declared length, which a value is converted to.
fn character_length(kind: Option<&TypeSpec>) -> Option<TypeSpec> {
    matches!(kind, Some(TypeSpec::Character(_) | TypeSpec::CharacterLength(_))).then(|| kind.unwrap().clone())
}

/// Statements computed before the statement that references a CHARACTER
/// statement function, and a counter for their temporaries.
struct Hoisted {
    prefix: Vec<Stmt>,
    serial: usize,
    /// Whether actuals that reference functions are evaluated once, before
    /// the statement, which a DO WHILE condition does not allow.
    values: bool,
}

/// Whether `e` may reference a function, which may have side effects.
fn references_function(e: &Expr) -> bool {
    match e {
        Expr::Call(..) | Expr::Index(..) => true,
        Expr::Bin(_, a, b) => references_function(a) || references_function(b),
        Expr::Un(_, a) => references_function(a),
        _ => false,
    }
}

impl Hoisted {
    fn temporary(&mut self, kind: TypeSpec, value: Expr) -> Expr {
        self.serial += 1;
        let name = format!("@statement_function_value_{}", self.serial);
        self.prefix.push(Stmt::VarDecl { kind, names: vec![name.clone()] });
        self.prefix.push(Stmt::Assign { name: name.clone(), value });
        Expr::Ident(name)
    }
}

/// Whether `s` belongs to the specification part, where statement functions
/// are defined.
fn specification(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::SourceLocation(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. }
            | Stmt::DummyIntent { .. } | Stmt::Implicit(_) | Stmt::Use { .. } | Stmt::DerivedDecl { .. }
            | Stmt::DerivedType { .. } | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. }
            | Stmt::OperatorInterface { .. }
    ) || matches!(s, Stmt::CallSub { name, .. } if name == "@data" || name == "@format_statement" || name == crate::equivalence::STATEMENT)
}

/// The dummies of the statement functions that a specification part seems
/// to define, for passes before this one: those dummies are only names, not
/// variables, whatever attributes the declarations of the names give.
pub(crate) fn definition_dummies(body: &[Stmt]) -> HashSet<String> {
    let scalars: HashSet<&str> = body.iter().flat_map(|s| match s {
        Stmt::VarDecl { names, .. } => names.iter().map(String::as_str).collect(),
        _ => vec![],
    }).collect();
    let arrays = declared_arrays(body);
    let mut out = HashSet::new();
    for s in body {
        match s {
            Stmt::AssignIndex { name, indices, .. } if scalars.contains(name.as_str()) && !arrays.contains(name)
                && !indices.is_empty() && indices.iter().all(|i| matches!(i, Expr::Ident(_))) => {
                out.extend(indices.iter().filter_map(|i| match i {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                }));
            }
            s if specification(s) => {}
            _ => break,
        }
    }
    out
}

fn substitute(e: &mut Expr, actuals: &HashMap<String, Expr>) {
    match e {
        Expr::Ident(n) => {
            if let Some(a) = actuals.get(n.as_str()) {
                *e = a.clone();
            }
        }
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args {
                substitute(a, actuals);
            }
        }
        Expr::Bin(_, a, b) => {
            substitute(a, actuals);
            substitute(b, actuals);
        }
        Expr::Un(_, a) => substitute(a, actuals),
        // A substring of a CHARACTER dummy, whose actual is a temporary.
        Expr::Section { name, lower, upper, stride } => {
            if let Some(Expr::Ident(actual)) = actuals.get(name.as_str()) {
                *name = actual.clone();
            }
            for x in [lower, upper, stride].into_iter().flatten() {
                substitute(x, actuals);
            }
        }
        _ => {}
    }
}

/// Replace references to statement functions in `e`, innermost first.
fn expand(e: &mut Expr, definitions: &HashMap<String, Definition>, hoisted: &mut Hoisted) -> Result<()> {
    match e {
        Expr::Call(_, args) | Expr::Index(_, args) => {
            for a in args.iter_mut() {
                expand(a, definitions, hoisted)?;
            }
        }
        Expr::Bin(_, a, b) => {
            expand(a, definitions, hoisted)?;
            expand(b, definitions, hoisted)?;
        }
        Expr::Un(_, a) => expand(a, definitions, hoisted)?,
        Expr::Section { lower, upper, stride, .. } => {
            for x in [lower, upper, stride].into_iter().flatten() {
                expand(x, definitions, hoisted)?;
            }
        }
        _ => {}
    }
    if let Expr::Call(name, args) | Expr::Index(name, args) = e {
        if let Some(d) = definitions.get(name.as_str()) {
            if args.len() != d.dummies.len() {
                bail!("statement function `{name}` has {} arguments, not {}", d.dummies.len(), args.len());
            }
            let mut actuals = HashMap::new();
            for (((dummy, kind), declared), actual) in d.dummies.iter().zip(&d.dummy_kinds).zip(&d.dummy_types).zip(args.iter()) {
                // An actual is evaluated once, however often the definition
                // references its dummy.
                let actual = match (kind, declared) {
                    (Some(kind), _) => hoisted.temporary(kind.clone(), actual.clone()),
                    (None, Some(declared)) if hoisted.values && references_function(actual) => {
                        hoisted.temporary(declared.clone(), actual.clone())
                    }
                    _ => actual.clone(),
                };
                actuals.insert(dummy.clone(), actual);
            }
            let mut value = d.value.clone();
            substitute(&mut value, &actuals);
            // The statement functions that the definition references.
            expand(&mut value, definitions, hoisted)?;
            *e = match &d.result {
                Some(kind) => hoisted.temporary(kind.clone(), value),
                None => value,
            };
        }
    }
    Ok(())
}

fn statements(body: &mut Vec<Stmt>, definitions: &HashMap<String, Definition>, serial: &mut usize) -> Result<()> {
    for mut s in std::mem::take(body) {
        if !matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. }) {
            let mut hoisted = Hoisted { prefix: vec![], serial: *serial, values: !matches!(s, Stmt::DoWhile { .. }) };
            for e in crate::procedure_arguments::expressions(&mut s) {
                expand(e, definitions, &mut hoisted)?;
            }
            *serial = hoisted.serial;
            if !hoisted.prefix.is_empty() && matches!(s, Stmt::DoWhile { .. }) {
                bail!("a CHARACTER statement function in a DO WHILE condition is not yet supported");
            }
            body.append(&mut hoisted.prefix);
            for b in crate::procedure_arguments::children(&mut s) {
                statements(b, definitions, serial)?;
            }
        }
        body.push(s);
    }
    Ok(())
}

/// The value of a statement function, converted to its declared type.
fn converted(value: Expr, kind: Option<&TypeSpec>) -> Expr {
    let conversion = match kind {
        Some(TypeSpec::DoublePrecision) => "dble",
        Some(TypeSpec::Real) => "real",
        Some(TypeSpec::RealKind(k)) if k == "8" => "dble",
        Some(TypeSpec::RealKind(k)) if k == "4" => "real",
        Some(TypeSpec::Integer(None)) => "int",
        _ => return value,
    };
    Expr::Call(conversion.into(), vec![value])
}

/// The arrays a scope declares.
fn declared_arrays(body: &[Stmt]) -> HashSet<String> {
    body.iter()
        .flat_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => vec![name.clone()],
            Stmt::DerivedDecl { names, dims, .. } if !dims.is_empty() => names.clone(),
            _ => vec![],
        })
        .collect()
}

/// The arrays of the program's modules, with those of the modules they use.
type Modules = HashMap<String, HashSet<String>>;

fn unit(body: &mut Vec<Stmt>, host: &HashSet<String>, modules: &mut Modules) -> Result<()> {
    // Modules, which have no host, come first, for the units that use them.
    for s in body.iter_mut() {
        if let Stmt::Module { name, body } = s {
            unit(body, &HashSet::new(), modules)?;
            let mut public = declared_arrays(body);
            for s in body.iter() {
                if let Stmt::Use { module, only: None, renames, .. } = s {
                    if renames.is_empty() {
                        public.extend(modules.get(module).cloned().unwrap_or_default());
                    }
                }
            }
            modules.insert(name.clone(), public);
        }
    }
    // An array of the host or of a used module is not a statement function.
    let mut arrays = host.clone();
    let mut unknown_module = false;
    for s in body.iter() {
        if let Stmt::Use { module, only, renames, .. } = s {
            match modules.get(module) {
                Some(public) => {
                    for n in public {
                        if only.as_ref().is_none_or(|o| o.contains(n)) {
                            arrays.insert(n.clone());
                        }
                    }
                    for (local, remote) in renames {
                        if public.contains(remote) {
                            arrays.insert(local.clone());
                        }
                    }
                }
                None => unknown_module = true,
            }
        }
    }
    let local = declared_arrays(body);
    // A local scalar declaration hides a host array.
    for s in body.iter() {
        if let Stmt::VarDecl { names, .. } = s {
            for n in names {
                arrays.remove(n);
            }
        }
    }
    arrays.extend(local);
    for s in body.iter_mut() {
        if let Stmt::Subroutine { body, .. } | Stmt::Function { body, .. } = s {
            unit(body, &arrays, modules)?;
        }
    }
    let kinds: HashMap<String, TypeSpec> = body
        .iter()
        .flat_map(|s| match s {
            Stmt::VarDecl { kind, names } => names.iter().map(|n| (n.clone(), kind.clone())).collect(),
            _ => vec![],
        })
        .collect();
    let mut definitions = HashMap::new();
    let mut kept = vec![];
    let mut specifying = true;
    for s in std::mem::take(body) {
        // With a module this compilation does not define, only a name
        // declared here is known not to be one of its arrays.
        let definition = specifying
            && matches!(&s, Stmt::AssignIndex { name, indices, .. } if !arrays.contains(name) && !name.contains('%')
                && (!unknown_module || kinds.contains_key(name))
                && !indices.is_empty() && indices.iter().all(|i| matches!(i, Expr::Ident(_))));
        if definition {
            let Stmt::AssignIndex { name, indices, value } = s else { unreachable!() };
            // A definition may use the statement functions before it, which
            // are expanded where it is referenced.
            let dummies: Vec<String> = indices.into_iter().map(|i| match i {
                Expr::Ident(n) => n,
                _ => unreachable!(),
            }).collect();
            let dummy_kinds = dummies.iter().map(|d| character_length(kinds.get(d))).collect();
            let dummy_types = dummies.iter().map(|d| kinds.get(d).cloned()).collect();
            let result = character_length(kinds.get(&name));
            let value = converted(value, kinds.get(&name));
            definitions.insert(name, Definition { dummies, value, result, dummy_kinds, dummy_types });
            continue;
        }
        specifying &= specification(&s) || matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. });
        kept.push(s);
    }
    *body = kept;
    if !definitions.is_empty() {
        // Their names and dummies are no longer variables.
        for s in body.iter_mut() {
            if let Stmt::VarDecl { names, .. } = s {
                names.retain(|n| !definitions.contains_key(n));
            }
        }
        body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
        // A dummy is only a name: POINTER and TARGET do not apply to it.
        let dummies: HashSet<&String> = definitions.values().flat_map(|d| &d.dummies).collect();
        body.retain(|s| !matches!(s, Stmt::DummyIntent { name, intent } if dummies.contains(name) && (intent == "pointer" || intent == "target")));
        statements(body, &definitions, &mut 0)?;
    }
    Ok(())
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    unit(&mut p.body, &HashSet::new(), &mut Modules::new())?;
    Ok(p)
}

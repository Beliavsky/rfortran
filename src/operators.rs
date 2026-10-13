// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Resolve module-defined operators and named generics before derived values are flattened.
//! Selection uses the original operand types, without numeric argument conversion.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Derived(String),
    Scalar(TypeSpec),
    Array(Box<Ty>, usize),
    Unknown,
}
type Vars = HashMap<String, Ty>;
type Bindings = HashMap<String, Vec<String>>;

fn defined_operator(op: &str) -> bool {
    op.starts_with('.') && !matches!(op, ".and." | ".or." | ".not." | ".eqv." | ".neqv.")
}

fn known_type(ty: &Ty) -> bool {
    match ty {
        Ty::Unknown => false,
        Ty::Array(element, _) => known_type(element),
        _ => true,
    }
}

fn scalar(kind: &TypeSpec) -> Ty {
    Ty::Scalar(match kind {
        TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)),
        TypeSpec::RealKind(k) if k == "4" => TypeSpec::Real,
        TypeSpec::RealKind(k) if k == "8" => TypeSpec::DoublePrecision,
        TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_) => TypeSpec::Character(None),
        _ => kind.clone(),
    })
}
// An intrinsic operator may be extended for operand types for which the
// language has no intrinsic operation (for example CHARACTER + CHARACTER).
fn intrinsic_operator(op: &str, args: &[Ty]) -> bool {
    fn element(ty:&Ty)->&Ty {match ty {Ty::Array(ty,_)=>element(ty),ty=>ty}}
    let numeric=|ty:&Ty|matches!(element(ty),Ty::Scalar(TypeSpec::Integer(_)|TypeSpec::Real|TypeSpec::DoublePrecision));
    let character=|ty:&Ty|matches!(element(ty),Ty::Scalar(TypeSpec::Character(_)));
    let logical=|ty:&Ty|matches!(element(ty),Ty::Scalar(TypeSpec::Logical));
    match op {
        "+"|"-"|"*"|"/"|"**"=>args.iter().all(numeric),
        "//"=>args.iter().all(character),
        "=="|"/="|"<"|"<="|">"|">="=>args.iter().all(numeric)||args.iter().all(character),
        ".and."|".or."|".not."|".eqv."|".neqv."=>args.iter().all(logical),
        _=>false,
    }
}
fn binary_type(op: &BinOp, left: Ty, right: Ty) -> Ty {
    let (left, lr) = match left {
        Ty::Array(t, r) => (*t, r),
        t => (t, 0),
    };
    let (right, rr) = match right {
        Ty::Array(t, r) => (*t, r),
        t => (t, 0),
    };
    if lr != 0 && rr != 0 && lr != rr {
        return Ty::Unknown;
    }
    let result = if left == Ty::Unknown || right == Ty::Unknown {
        Ty::Unknown
    } else if matches!(
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
        scalar(&TypeSpec::Logical)
    } else if left == scalar(&TypeSpec::DoublePrecision)
        || right == scalar(&TypeSpec::DoublePrecision)
    {
        scalar(&TypeSpec::DoublePrecision)
    } else if left == scalar(&TypeSpec::Real) || right == scalar(&TypeSpec::Real) {
        scalar(&TypeSpec::Real)
    } else if let (Ty::Scalar(TypeSpec::Integer(Some(a))), Ty::Scalar(TypeSpec::Integer(Some(b)))) =
        (&left, &right)
    {
        scalar(&TypeSpec::Integer(Some((*a).max(*b))))
    } else if left == right {
        left
    } else {
        Ty::Unknown
    };
    if lr.max(rr) == 0 {
        result
    } else {
        Ty::Array(Box::new(result), lr.max(rr))
    }
}
fn declarations(body: &[Stmt], inherited: &Vars) -> Vars {
    let mut vars = inherited.clone();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    vars.insert(n.clone(), scalar(kind));
                }
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    vars.insert(n.clone(), scalar(kind));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                vars.insert(name.clone(), Ty::Array(Box::new(scalar(kind)), dims.len()));
            }
            Stmt::DerivedDecl {
                type_name, names, ..
            } => {
                for n in names {
                    vars.insert(n.clone(), Ty::Derived(type_name.clone()));
                }
            }
            _ => {}
        }
    }
    vars
}

#[derive(Clone)]
struct Procedure {
    signature: crate::named_arguments::Signature,
    args: Vec<Ty>,
    result: Ty,
    inputs: bool,
    subroutine: bool,
    array_specialization: bool,
}
#[derive(Default)]
struct Resolver {
    procedures: HashMap<String, Procedure>,
    duplicate_procedures: HashSet<String>,
    module_procedures: HashSet<String>,
    components: HashMap<String, Vars>,
    methods: HashMap<String, HashMap<String, String>>,
    exports: HashMap<String, Bindings>,
}

fn merge(target: &mut Bindings, source: &Bindings) {
    for (op, names) in source {
        let out = target.entry(op.clone()).or_default();
        for n in names {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
    }
}
fn export_name(key: &str) -> String {
    key.strip_prefix("generic:")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("operator({key})"))
}
fn bin_name(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Pow => "**",
        BinOp::Concat => "//",
        BinOp::Eq => "==",
        BinOp::Ne => "/=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => ".and.",
        BinOp::Or => ".or.",
        BinOp::Eqv => ".eqv.",
        BinOp::Neqv => ".neqv.",
    }
}

impl Resolver {
    fn collect(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            match s {
                Stmt::DerivedType {
                    name,
                    components,
                    bindings,
                } => {
                    self.components
                        .insert(name.clone(), declarations(components, &Vars::new()));
                    self.methods
                        .insert(name.clone(), bindings.iter().cloned().collect());
                }
                Stmt::Function {
                    name,
                    params,
                    return_type,
                    body,
                    pure,
                    elemental,
                    ..
                } => {
                    let vars = declarations(body, &Vars::new());
                    let result_name=match body.last() {Some(Stmt::Return(Some(Expr::Ident(n))))=>n,_=>name};
                    let array_specialization=crate::character_elementals::inline_array(*pure,params,result_name,body)
                        || crate::elemental::inline_character(*elemental,name,params,return_type,body);
                    let result=vars.get(result_name).cloned()
                        .or_else(||return_type.as_ref().map(scalar)).unwrap_or(Ty::Unknown);
                    let mut inputs = HashSet::new();
                    for s in body {
                        match s {
                            Stmt::DummyIntent { name, intent } if intent == "intent(in)" => {
                                inputs.insert(name.clone());
                            }
                            Stmt::DerivedDecl {
                                names, attributes, ..
                            } if attributes.iter().any(|a| a == "intent(in)") => {
                                // Derived declarations retain their attributes; scalar declarations
                                // also carry separate DummyIntent statements from the parser.
                                inputs.extend(names.iter().cloned());
                            }
                            _ => {}
                        }
                    }
                    if self
                        .procedures
                        .insert(
                            name.clone(),
                            Procedure {
                                signature: crate::named_arguments::Signature::new(params, body),
                                args: params
                                    .iter()
                                    .map(|p| vars.get(p).cloned().unwrap_or(Ty::Unknown))
                                    .collect(),
                                result,
                                inputs: params.iter().all(|p| inputs.contains(p))
                                    && !body.iter().any(|s| match s {
                                        Stmt::DummyIntent { intent, .. } => intent == "optional",
                                        Stmt::DerivedDecl { attributes, .. } => attributes.iter().any(|a| a == "optional"),
                                        _ => false,
                                    }),
                                subroutine: false,
                                array_specialization,
                            },
                        )
                        .is_some()
                    {
                        self.duplicate_procedures.insert(name.clone());
                    }
                    self.collect(body)?;
                }
                Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    let vars = declarations(body, &Vars::new());
                    if self
                        .procedures
                        .insert(
                            name.clone(),
                            Procedure {
                                signature: crate::named_arguments::Signature::new(params, body),
                                args: params
                                    .iter()
                                    .map(|n| vars.get(n).cloned().unwrap_or(Ty::Unknown))
                                    .collect(),
                                result: Ty::Unknown,
                                inputs: false,
                                subroutine: true,
                                array_specialization:false,
                            },
                        )
                        .is_some()
                    {
                        self.duplicate_procedures.insert(name.clone());
                    }
                    self.collect(body)?;
                }
                Stmt::Module { body, .. } => {
                    for s in body {
                        if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = s {
                            self.module_procedures.insert(name.clone());
                        }
                    }
                    self.collect(body)?;
                }
                Stmt::Block { body } => self.collect(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn local_bindings(&self, body: &[Stmt]) -> Result<Bindings> {
        let mut bindings = Bindings::new();
        for s in body {
            if let Stmt::GenericInterface {
                name: generic,
                procedures,
                end_name,
            } = s
            {
                if end_name.as_ref().is_some_and(|n| n != generic) {
                    bail!("END INTERFACE name does not match `{generic}`");
                }
                for name in procedures {
                    if !self.module_procedures.contains(name) {
                        bail!("generic `{generic}` requires a defined module procedure `{name}`");
                    }
                    if self.duplicate_procedures.contains(name) {
                        bail!("generic procedure `{name}` must have a unique name within this compilation");
                    }
                    let Some(p) = self.procedures.get(name) else {
                        bail!("generic `{generic}` requires a defined module procedure `{name}`");
                    };
                    if p.args.iter().any(|a| matches!(a, Ty::Unknown))
                        || (!p.subroutine && p.result == Ty::Unknown)
                    {
                        bail!("generic `{generic}` requires explicitly typed arguments and function results");
                    }
                    let names = bindings
                        .entry(format!("generic:{generic}"))
                        .or_insert_with(Vec::new);
                    for n in names.iter() {
                        let other = &self.procedures[n];
                        if other.subroutine != p.subroutine {
                            bail!("generic `{generic}` cannot mix functions and subroutines");
                        }
                        if other.args == p.args {
                            bail!("ambiguous generic `{generic}`: duplicate argument types for `{name}`");
                        }
                    }
                    names.push(name.clone());
                }
            }
            if let Stmt::OperatorInterface {
                operator,
                procedures,
                end_operator,
            } = s
            {
                if end_operator.as_ref().is_some_and(|n| n != operator) {
                    bail!("END INTERFACE operator does not match operator({operator})");
                }
                for name in procedures {
                    if self.duplicate_procedures.contains(name) {
                        bail!("operator procedure `{name}` must have a unique name within this compilation");
                    }
                    let Some(p) = self.procedures.get(name) else {
                        bail!("operator({operator}) requires a defined function `{name}`");
                    };
                    if p.subroutine {
                        bail!("operator({operator}) requires a defined function `{name}`");
                    }
                    let arity_ok = match operator.as_str() {
                        "+" | "-" => matches!(p.args.len(), 1 | 2),
                        op if defined_operator(op) => matches!(p.args.len(), 1 | 2),
                        ".not." => p.args.len() == 1,
                        _ => p.args.len() == 2,
                    };
                    if !arity_ok || !p.inputs || p.subroutine {
                        bail!("operator({operator}) function `{name}` must have the appropriate number of nonoptional INTENT(IN) operands");
                    }
                    if !known_type(&p.result) || p.args.iter().any(|a| !known_type(a)) {
                        bail!("operator({operator}) function `{name}` requires explicitly typed arguments and result");
                    }
                    if !defined_operator(operator) && (matches!(p.result, Ty::Array(..))
                        || p.args
                            .iter()
                            .any(|a| matches!(a, Ty::Array(..)))) {
                        bail!("operator({operator}) function `{name}` requires explicitly typed scalar arguments and result");
                    }
                    if !defined_operator(operator) && intrinsic_operator(operator,&p.args) {
                        bail!("operator({operator}) overload `{name}` cannot replace an intrinsic operation for these operand types");
                    }
                    let names = bindings.entry(operator.clone()).or_insert_with(Vec::new);
                    if names.iter().any(|n| self.procedures[n].args == p.args) {
                        bail!(
                            "ambiguous operator({operator}): duplicate operand types for `{name}`"
                        );
                    }
                    names.push(name.clone());
                }
            }
        }
        Ok(bindings)
    }
    fn visible(&self, body: &[Stmt], inherited: &Bindings) -> Result<Bindings> {
        let mut visible = inherited.clone();
        merge(&mut visible, &self.local_bindings(body)?);
        for s in body {
            if let Stmt::Use { module, only, .. } = s {
                if let Some(exported) = self.exports.get(module) {
                    let selected = exported
                        .iter()
                        .filter(|(op, _)| {
                            only.as_ref()
                                .is_none_or(|names| names.contains(&export_name(op)))
                        })
                        .map(|(op, names)| (op.clone(), names.clone()))
                        .collect();
                    merge(&mut visible, &selected);
                }
            }
        }
        for (key, names) in &visible {
            if let Some(generic) = key.strip_prefix("generic:") {
                for (i, a) in names.iter().enumerate() {
                    for b in &names[i + 1..] {
                        let a = &self.procedures[a];
                        let b = &self.procedures[b];
                        if a.subroutine != b.subroutine {
                            bail!("generic `{generic}` cannot mix functions and subroutines");
                        }
                        if a.args == b.args {
                            bail!("ambiguous generic `{generic}`: duplicate argument types in associated procedures");
                        }
                    }
                }
            }
        }
        Ok(visible)
    }
    fn select(&self, op: &str, args: &[Ty], visible: &Bindings) -> Result<Option<String>> {
        if !defined_operator(op) && (intrinsic_operator(op,args)
            || (!visible.contains_key(op) && !args.iter().any(|a|matches!(a,Ty::Derived(_))))
            || args.iter().any(|a|!known_type(a))) {
            return Ok(None);
        }
        let matches: Vec<_> = visible
            .get(op)
            .into_iter()
            .flatten()
            .filter(|name| self.procedures[*name].args == args)
            .collect();
        match matches.as_slice() {
            [name] => Ok(Some((*name).clone())),
            [] => bail!("no matching operator({op}) for operand types {args:?}"),
            _ => bail!("ambiguous operator({op}) for operand types {args:?}"),
        }
    }
    fn select_generic(
        &self,
        name: &str,
        actuals: &mut Vec<Expr>,
        args: &[Ty],
        subroutine: bool,
        visible: &Bindings,
    ) -> Result<Option<String>> {
        let Some(candidates) = visible.get(&format!("generic:{name}")) else {
            return Ok(None);
        };
        let mut matches = vec![];
        let mut association_errors = vec![];
        for n in candidates {
            let p = &self.procedures[n];
            if p.subroutine != subroutine { continue; }
            match p.signature.associate(name, actuals) {
                Ok(slots) if p.args.iter().zip(&slots).all(|(ty, slot)| slot.is_none_or(|i| ty == &args[i])) => matches.push(n),
                Ok(_) => {},
                Err(error) => association_errors.push(error),
            }
        }
        match matches.as_slice() {
            [target] => {
                self.procedures[*target].signature.reorder(name, actuals)?;
                Ok(Some((*target).clone()))
            },
            [] if crate::named_arguments::has_keywords(actuals)
                && association_errors.len() == candidates.len()
                && !association_errors.is_empty()
                && association_errors.iter().all(|e| e.to_string() == association_errors[0].to_string())
                => Err(association_errors.remove(0)),
            [] => bail!(
                "no matching specific procedure for generic `{name}` with argument types {args:?}"
            ),
            _ => bail!("ambiguous generic `{name}` with argument types {args:?}"),
        }
    }
    fn name_type(&self, name: &str, vars: &Vars) -> Ty {
        if let Some((object, component)) = name.split_once('%') {
            if let Some(Ty::Derived(ty)) = vars.get(object) {
                return self
                    .components
                    .get(ty)
                    .and_then(|c| c.get(component))
                    .cloned()
                    .unwrap_or(Ty::Unknown);
            }
        }
        vars.get(name).cloned().unwrap_or(Ty::Unknown)
    }
    fn expression(&self, e: &mut Expr, vars: &Vars, visible: &Bindings) -> Result<Ty> {
        if let Some(kind)=crate::empty_constructors::kind(e)? {
            return Ok(Ty::Array(Box::new(scalar(&kind)),1));
        }
        if let Expr::Call(name, args) = e {
            if name.starts_with("@keyword:") && args.len() == 1 {
                return self.expression(&mut args[0], vars, visible);
            }
            if let Some(op) = name.strip_prefix("@defined_operator:") {
                let op = op.to_owned();
                let types = args.iter_mut().map(|a| self.expression(a, vars, visible)).collect::<Result<Vec<_>>>()?;
                let target = self.select(&op, &types, visible)?.expect("defined operators always resolve or fail");
                if matches!(self.procedures[&target].result,Ty::Array(..)) && !self.procedures[&target].array_specialization {
                    bail!("operator({op}) resolves to `{target}`, but its array result requires a supported function specialization");
                }
                // Preserve the array actuals: ordinary function lowering selects
                // the supported ABI or specializes read-only array arguments.
                *name = target.clone();
                return Ok(self.procedures[&target].result.clone());
            }
            if name == "@unary_plus" {
                let ty = self.expression(&mut args[0], vars, visible)?;
                if let Some(target) = self.select("+", &[ty.clone()], visible)? {
                    *name = target.clone();
                    return Ok(self.procedures[&target].result.clone());
                }
                *e = args.remove(0);
                return Ok(ty);
            }
        }
        Ok(match e {
            Expr::Ident(n) => self.name_type(n, vars),
            Expr::IntLit(_) => scalar(&TypeSpec::Integer(None)),
            Expr::RealLit(n) => scalar(&if n.contains(['d', 'D']) || n.ends_with("_8") {
                TypeSpec::DoublePrecision
            } else {
                TypeSpec::Real
            }),
            Expr::Logical(_) => scalar(&TypeSpec::Logical),
            Expr::Str(s) => scalar(&TypeSpec::Character(Some(s.len()))),
            Expr::Call(n, args) | Expr::Index(n, args) => {
                let mut arg_types = vec![];
                for a in args.iter_mut() {
                    arg_types.push(self.expression(a, vars, visible)?);
                }
                // A local array can hide a host-associated generic of the same name.
                if let Ty::Array(element, _) = self.name_type(n, vars) {
                    return Ok(*element);
                }
                if let Some(target) = self.select_generic(n, args, &arg_types, false, visible)? {
                    *n = target;
                    return Ok(self.procedures[n].result.clone());
                }
                if self.components.contains_key(n) {
                    Ty::Derived(n.clone())
                } else if let Some(p) = self.procedures.get(n) {
                    p.result.clone()
                } else if let Some((object, binding)) = n.split_once('%') {
                    if let Some(Ty::Derived(ty)) = vars.get(object) {
                        self.methods
                            .get(ty)
                            .and_then(|m| m.get(binding))
                            .and_then(|n| self.procedures.get(n))
                            .map(|p| p.result.clone())
                            .unwrap_or(Ty::Unknown)
                    } else {
                        Ty::Unknown
                    }
                } else {
                    match self.name_type(n, vars) {
                        Ty::Array(t, _) => *t,
                        _ => match n.as_str() {
                            "@array_constructor" if !arg_types.is_empty() && arg_types.iter().all(|t| t == &arg_types[0]) => {
                                let element = match &arg_types[0] { Ty::Array(t, _) => (**t).clone(), t => t.clone() };
                                Ty::Array(Box::new(element), 1)
                            }
                            "@integer_literal" if args.len()==2 => scalar(&TypeSpec::Integer(Some(crate::parameters::integer_constant(&args[1])? as u8))),
                            "size" | "len" => scalar(&TypeSpec::Integer(None)),
                            "int" | "nint" | "floor" | "ceiling" | "real" | "dble" => {
                                let kind = match args.get(1) {
                                    None => Some(4),
                                    Some(Expr::IntLit(k)) => k.parse::<u8>().ok(),
                                    _ => None,
                                };
                                let t = match (n.as_str(), kind) {
                                    ("dble", _) | ("real", Some(8)) => {
                                        scalar(&TypeSpec::DoublePrecision)
                                    }
                                    ("real", Some(4)) => scalar(&TypeSpec::Real),
                                    ("real", _) | (_, None) => Ty::Unknown,
                                    (_, Some(k)) => scalar(&TypeSpec::Integer(Some(k))),
                                };
                                match arg_types.first() {
                                    Some(Ty::Array(_, r)) => Ty::Array(Box::new(t), *r),
                                    _ => t,
                                }
                            }
                            "abs" | "mod" | "modulo" | "min" | "max" => {
                                arg_types.first().cloned().unwrap_or(Ty::Unknown)
                            }
                            "sum" | "product" | "minval" | "maxval" if args.len() == 1 => {
                                match arg_types.first() {
                                    Some(Ty::Array(t, _)) => (**t).clone(),
                                    _ => Ty::Unknown,
                                }
                            }
                            "dot_product" if args.len() == 2 => match arg_types.first() {
                                Some(Ty::Array(t, _)) => (**t).clone(),
                                _ => Ty::Unknown,
                            },
                            _ if crate::intrinsics::math_arity(n).is_some() => {
                                arg_types.first().cloned().unwrap_or(Ty::Unknown)
                            }
                            _ => Ty::Unknown,
                        },
                    }
                }
            }
            Expr::Bin(op, a, b) => {
                let left = self.expression(a, vars, visible)?;
                let right = self.expression(b, vars, visible)?;
                if let Some(name) =
                    self.select(bin_name(op), &[left.clone(), right.clone()], visible)?
                {
                    let ty = self.procedures[&name].result.clone();
                    *e = Expr::Call(name, vec![(**a).clone(), (**b).clone()]);
                    ty
                } else {
                    binary_type(op, left, right)
                }
            }
            Expr::Un(op, a) => {
                let ty = self.expression(a, vars, visible)?;
                let name = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => ".not.",
                };
                if let Some(name) = self.select(name, &[ty.clone()], visible)? {
                    let result = self.procedures[&name].result.clone();
                    *e = Expr::Call(name, vec![(**a).clone()]);
                    result
                } else {
                    ty
                }
            }
            Expr::Section {
                name,
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expression(a, vars, visible)?;
                }
                self.name_type(name, vars)
            }
        })
    }
    fn body(&self, body: &mut Vec<Stmt>, vars: &Vars, visible: &Bindings) -> Result<()> {
        let vars = declarations(body, vars);
        let visible = self.visible(body, visible)?;
        for s in body.iter_mut() {
            match s {
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    self.expression(value, &vars, &visible)?;
                }
                Stmt::AssignIndex { indices, value, .. } => {
                    for i in indices {
                        self.expression(i, &vars, &visible)?;
                    }
                    self.expression(value, &vars, &visible)?;
                }
                Stmt::CallSub { name, args } => {
                    let types = args
                        .iter_mut()
                        .map(|e| self.expression(e, &vars, &visible))
                        .collect::<Result<Vec<_>>>()?;
                    if let Some(target) = self.select_generic(name, args, &types, true, &visible)? {
                        *name = target;
                    }
                }
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        self.expression(e, &vars, &visible)?;
                    }
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (_, e) in entities {
                        if let Some(e) = e {
                            self.expression(e, &vars, &visible)?;
                        }
                    }
                }
                Stmt::ArrayDecl { dims, .. } => {
                    for e in dims {
                        self.expression(e, &vars, &visible)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &vars, &visible)?;
                    self.body(then_body, &vars, &visible)?;
                    if let Some(b) = else_body {
                        self.body(b, &vars, &visible)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &vars, &visible)?;
                    self.expression(end, &vars, &visible)?;
                    self.body(body, &vars, &visible)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &vars, &visible)?;
                    self.body(body, &vars, &visible)?;
                }
                Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => self.body(body, &vars, &visible)?,
                Stmt::Module { body, .. } => self.body(body, &Vars::new(), &Bindings::new())?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &vars, &visible)?;
                    for c in cases {
                        self.body(&mut c.body, &vars, &visible)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &vars, &visible)?;
                    }
                }
                _ => {}
            }
        }
        body.retain(|s| {
            !matches!(
                s,
                Stmt::OperatorInterface { .. } | Stmt::GenericInterface { .. }
            )
        });
        Ok(())
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
    fn needs_constants(body: &[Stmt]) -> bool {
        body.iter().any(|s| match s {
            Stmt::GenericInterface { .. } | Stmt::OperatorInterface { .. } => true,
            Stmt::VarDecl {
                kind: TypeSpec::IntegerKind(_) | TypeSpec::CharacterLength(_),
                ..
            }
            | Stmt::ArrayDecl {
                kind: TypeSpec::IntegerKind(_) | TypeSpec::CharacterLength(_),
                ..
            }
            | Stmt::InitializedDecl {
                kind: TypeSpec::IntegerKind(_) | TypeSpec::CharacterLength(_),
                ..
            } => true,
            Stmt::DerivedType { components, .. } => needs_constants(components),
            Stmt::Module { body, .. }
            | Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => needs_constants(body),
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                needs_constants(then_body) || else_body.as_ref().is_some_and(|b| needs_constants(b))
            }
            Stmt::SelectCase { cases, default, .. } => {
                cases.iter().any(|c| needs_constants(&c.body))
                    || default.as_ref().is_some_and(|b| needs_constants(b))
            }
            _ => false,
        })
    }
    // Overload selection must see resolved kind parameters and literal suffixes.
    if needs_constants(&program.body) {
        program = crate::parameters::resolve_constants(program)?;
    }
    let mut resolver = Resolver::default();
    resolver.collect(&program.body)?;
    // Source-order modules can re-export operators brought in by USE.
    for s in &program.body {
        if let Stmt::Module { name, body } = s {
            let mut exports = resolver.visible(body, &Bindings::new())?;
            let mut default_public = true;
            let mut access = HashMap::new();
            for s in body {
                if let Stmt::Accessibility { public, names } = s {
                    if names.is_empty() {
                        default_public = *public;
                    }
                    for n in names {
                        access.insert(n.clone(), *public);
                    }
                }
            }
            exports.retain(|op, _| *access.get(&export_name(op)).unwrap_or(&default_public));
            resolver.exports.insert(name.clone(), exports);
        }
    }
    resolver.body(&mut program.body, &Vars::new(), &Bindings::new())?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    #[test]
    fn defined_operator_diagnostics_and_visibility() {
        let original = include_str!("../tests/76_defined_operators.f90");
        lower(original).unwrap();
        for (source, message) in [
            (original.replace("x=2 .PLUS. 3*4", "x=2 .plus. 3.0"), "no matching operator(.plus.)"),
            (original.replace("x=2 .PLUS. 3*4", "x=.plus. 3"), "no matching operator(.plus.)"),
            (original.replace("x=2 .PLUS. 3*4", "x=2 .missing. 3"), "no matching operator(.missing.)"),
            (original.replace("module procedure add_int, add_real", "module procedure add_int, add_int"), "ambiguous operator(.plus.)"),
            (original.replace("integer, intent(in) :: a,b", "integer, intent(out) :: a,b"), "INTENT(IN) operands"),
            (original.replace("integer, intent(in) :: a,b", "integer, intent(in), optional :: a,b"), "nonoptional"),
            (original.replace("end interface operator(.plus.)", "end interface operator(.wrong.)"), "does not match"),
            (original.replace("use public_ops, only: operator(.plus.),", "use public_ops, only:"), "no matching operator(.plus.)"),
            (original.replace("public :: operator(.plus.),", "public ::"), "not a public entity"),
        ] {
            let error = lower(&source).unwrap_err().to_string();
            assert!(error.contains(message), "expected {message}: {error}");
        }
    }

    #[test]
    fn defined_array_operators_preserve_operands_and_check_rank() {
        let source = "module m\ninterface operator(.in.)\nmodule procedure member\nend interface\ncontains\nlogical function member(x,a) result(found)\ninteger,intent(in)::x,a(3)\nfound=any(a==x)\nend function member\nend module m\nuse m\ninteger::a(3)\na=[1,3,5]\nprint *,3 .in. a\nend";
        for operand in ["a", "[1,3,5]", "a(:)", "[integer::]"] {
            let program=lower(&source.replace("3 .in. a",&format!("3 .in. {operand}"))).unwrap();
            let items=program.body.iter().find_map(|s|if let crate::ast::Stmt::Print{items}=s {Some(items)} else {None}).unwrap();
            let crate::ast::Expr::Call(name,args)=&items[0] else {panic!("operator must lower to a function call")};
            assert_eq!(name,"member");assert_eq!(args.len(),2);
            assert!(!matches!(&args[1],crate::ast::Expr::IntLit(_)|crate::ast::Expr::Index(..)),"array operand must remain an array: {args:?}");
        }
        let error = lower(&source.replace("3 .in. a", "3 .in. a(1)")).unwrap_err().to_string();
        assert!(error.contains("no matching operator(.in.)"), "{error}");
    }

    #[test]
    fn generic_dispatch_preserves_rank_and_local_array_shadowing() {
        let source = "module m\ninterface g\nmodule procedure one, two\nend interface\ncontains\nsubroutine one(x)\nreal :: x(:)\nend subroutine one\nsubroutine two(x)\nreal :: x(2,2)\nend subroutine two\ninteger function local()\ninteger :: g(2)\ng=3\nlocal=g(1)\nend function local\nend module m\nuse m, only: g\nreal :: a(2),b(2,2)\ncall g(a+1.0)\ncall g(b)\nend";
        let p = lower(source).unwrap();
        let calls: Vec<_> = p
            .body
            .iter()
            .filter_map(|s| {
                if let crate::ast::Stmt::CallSub { name, .. } = s {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(calls, ["one", "two"]);
    }

    #[test]
    fn resolves_named_generic_interfaces() {
        for source in [
            include_str!("../tests/63_generic_interfaces.f90"),
            include_str!("../tests/64_generic_derived.f90"),
        ] {
            lower(source).unwrap();
        }
        let source = include_str!("../tests/61_use_only_operator.f90")
            .replace("end interface", "end interface operator(+)");
        lower(&source).unwrap();
        let invalid = source.replace("end interface operator(+)", "end interface operator(-)");
        assert!(lower(&invalid)
            .unwrap_err()
            .to_string()
            .contains("does not match"));
    }

    #[test]
    fn rejects_invalid_generic_interfaces_and_calls() {
        const BASE: &str = "module m\nprivate\npublic :: choose\ninterface choose\nmodule procedure f\nend interface choose\ncontains\ninteger function f(x)\ninteger :: x\nf=x\nend function f\nend module m\n";
        for (source, expected) in [
            (
                format!("{BASE}use m, only: choose\nprint *, choose(1.0)\nend"),
                "no matching specific",
            ),
            (
                format!("{BASE}use m, only: choose\nprint *, choose(1,2)\nend"),
                "no matching specific",
            ),
            (
                format!("{BASE}use m, only: choose\ncall choose(1)\nend"),
                "no matching specific",
            ),
            (
                format!("{BASE}use m, only: choose\nprint *, f(1)\nend"),
                "not accessible",
            ),
            (
                BASE.replace("end interface choose", "end interface wrong"),
                "does not match",
            ),
            (
                BASE.replace("module procedure f", "module procedure missing"),
                "requires a defined module procedure",
            ),
            (
                BASE.replace("module procedure f", "module procedure f,f"),
                "ambiguous generic",
            ),
            (
                BASE.replace("module procedure f", "module procedure f,s")
                    .replace(
                        "end module m",
                        "subroutine s(x)\ninteger :: x\nend subroutine s\nend module m",
                    ),
                "cannot mix functions and subroutines",
            ),
        ] {
            let err = lower(&source).unwrap_err().to_string();
            assert!(err.contains(expected), "{expected}: {err}");
        }
    }

    #[test]
    fn generic_imports_merge_and_report_ambiguity() {
        let modules = "module a\ninterface g\nmodule procedure f\nend interface\ncontains\ninteger function f(x)\ninteger :: x\nf=x\nend function f\nend module a\nmodule b\ninterface g\nmodule procedure h\nend interface\ncontains\ninteger function h(x)\nreal :: x\nh=int(x)\nend function h\nend module b\n";
        let source = format!("{modules}use a, only: g\nuse b, only: g\nprint *, g(1),g(2.0)\nend");
        lower(&source).unwrap();
        let ambiguous = source.replace("real :: x\nh=int(x)", "integer :: x\nh=x");
        assert!(lower(&ambiguous)
            .unwrap_err()
            .to_string()
            .contains("ambiguous generic"));
        let unused = ambiguous.replace("print *, g(1),g(2.0)\n", "");
        assert!(lower(&unused)
            .unwrap_err()
            .to_string()
            .contains("ambiguous generic"));
        let mixed = source.replace(
            "integer function h(x)\nreal :: x\nh=int(x)\nend function h",
            "subroutine h(x)\nreal :: x\nend subroutine h",
        );
        assert!(lower(&mixed)
            .unwrap_err()
            .to_string()
            .contains("cannot mix functions and subroutines"));
    }

    fn lower(src: &str) -> anyhow::Result<crate::ast::Program> {
        let p =
            crate::parser::parse_with_src(src, &crate::lexer::lex(src), "operators.f90").unwrap();
        crate::derived::resolve(p)
    }
    #[test]
    fn resolves_date_operators_and_nested_expressions() {
        for src in [
            include_str!("../tests/56_derived_operators.f90"),
            include_str!("../tests/57_derived_operator_edges.f90"),
        ] {
            let p = crate::parameters::resolve(lower(src).unwrap()).unwrap();
            let diagnostics = crate::sema::analyze_with_src(
                &p,
                src,
                &crate::lexer::lex(src),
                "operators.f90",
                &crate::sema::SemaSettings::default(),
                &std::collections::HashSet::new(),
                false,
            );
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            crate::initialized::instrument(p).unwrap();
        }
    }
    #[test]
    fn rejects_missing_inaccessible_and_ambiguous_operators() {
        let original = include_str!("../tests/56_derived_operators.f90");
        for (source, message) in [
            (
                original.replace("c = a + 5", "c = a + 5.0"),
                "no matching operator(+)",
            ),
            (
                original.replace("use year_month_mod", ""),
                "no matching operator(+)",
            ),
            (
                original.replace("public :: operator(+), operator(-)", ""),
                "no matching operator(+)",
            ),
            (
                original.replace(
                    "module procedure add_months_left",
                    "module procedure add_months_left, add_months_right",
                ),
                "ambiguous operator(+)",
            ),
            (
                original.replace(
                    "module procedure add_months_left",
                    "module procedure print_year_month",
                ),
                "requires a defined function",
            ),
            (
                original.replace("if (.not. (c == b))", "if (c /= b)"),
                "no matching operator(/=)",
            ),
            (
                original.replace("integer, intent(in) :: n", "integer, intent(out) :: n"),
                "INTENT(IN) operands",
            ),
        ] {
            let error = lower(&source).unwrap_err().to_string();
            assert!(error.contains(message), "expected {message}: {error}");
        }
    }
}

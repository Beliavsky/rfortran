// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Associate keyword actual arguments before positional procedure lowering.
use crate::ast::{Expr, Program, Stmt};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

pub const OMITTED: &str = "@absent_argument";
pub fn omitted(e: &Expr) -> bool {
    matches!(e, Expr::Call(n, a) if n == OMITTED && a.is_empty())
}
pub fn keyword(e: &Expr) -> Option<(&str, &Expr)> {
    match e {
        Expr::Call(n, values) if values.len() == 1 => {
            n.strip_prefix("@keyword:").map(|key| (key, &values[0]))
        }
        _ => None,
    }
}
pub fn has_keywords(args: &[Expr]) -> bool {
    args.iter().any(|e| keyword(e).is_some())
}

#[derive(Clone)]
pub struct Signature {
    pub params: Vec<String>,
    pub optional: HashSet<String>,
}
impl Signature {
    pub fn new(params: &[String], body: &[Stmt]) -> Self {
        let mut optional = HashSet::new();
        for s in body {
            match s {
                Stmt::DummyIntent { name, intent } if intent == "optional" => {
                    optional.insert(name.clone());
                }
                Stmt::DerivedDecl {
                    names, attributes, ..
                } if attributes.iter().any(|a| a == "optional") => {
                    optional.extend(names.iter().cloned());
                }
                _ => {}
            }
        }
        Self {
            params: params.to_vec(),
            optional,
        }
    }
    // Slots refer to the original actual list, including keyword wrappers.
    // Generic selection uses these slots to match types as well.
    pub fn associate(&self, name: &str, args: &[Expr]) -> Result<Vec<Option<usize>>> {
        let mut slots = vec![None; self.params.len()];
        let mut keywords = false;
        for (position, arg) in args.iter().enumerate() {
            let (slot, value) = if let Some((key, value)) = keyword(arg) {
                keywords = true;
                let Some(slot) = self.params.iter().position(|p| p.eq_ignore_ascii_case(key))
                else {
                    bail!("unknown keyword `{key}` in call to `{name}`");
                };
                (slot, value)
            } else {
                if keywords {
                    bail!("positional argument after keyword argument in call to `{name}`");
                }
                if position >= slots.len() {
                    bail!("too many arguments to `{name}`");
                }
                (position, arg)
            };
            if slots[slot].replace(position).is_some() {
                bail!(
                    "duplicate argument `{}` in call to `{name}`",
                    self.params[slot]
                );
            }
            if omitted(value) {
                if !self.optional.contains(&self.params[slot]) {
                    bail!(
                        "missing required argument `{}` of `{name}`",
                        self.params[slot]
                    );
                }
                slots[slot] = None;
            }
        }
        for (param, slot) in self.params.iter().zip(&slots) {
            if slot.is_none() && !self.optional.contains(param) {
                bail!("missing required argument `{param}` of `{name}`");
            }
        }
        Ok(slots)
    }
    pub fn reorder(&self, name: &str, args: &mut Vec<Expr>) -> Result<()> {
        let slots = self.associate(name, args)?;
        let count = slots.iter().rposition(Option::is_some).map_or(0, |i| i + 1);
        *args = slots[..count]
            .iter()
            .map(|slot| match slot {
                Some(i) => keyword(&args[*i]).map_or(&args[*i], |(_, v)| v).clone(),
                None => Expr::Call(OMITTED.into(), vec![]),
            })
            .collect();
        Ok(())
    }
}

pub(crate) fn intrinsic(name: &str) -> Option<Signature> {
    let keys: &[&str] = match name {
        name if crate::real_intrinsics::keys(name).is_some() => crate::real_intrinsics::keys(name).unwrap(),
        name if crate::bit_intrinsics::keys(name).is_some() => crate::bit_intrinsics::keys(name).unwrap(),
        name if crate::inquiries::numeric_model(name) => &["x"],
        name if crate::intrinsics::real_predicate(name) => &["x"],
        "atan2" => &["y", "x"],
        "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
        | "sinh" | "cosh" | "tanh" => &["x"],
        "abs" | "dble" => &["a"],
        "cmplx"=> &["x","y","kind"],
        "aimag"|"conjg"=> &["z"],
        "kind" => &["x"],
        "mod" | "modulo" => &["a", "p"],
        "new_line" => &["a"],
        "adjustl"|"adjustr"|"trim" => &["string"],
        "repeat" => &["string", "ncopies"],
        "index" => &["string", "substring", "back", "kind"],
        "scan" | "verify" => &["string", "set", "back", "kind"],
        "random_number" => &["harvest"],
        "random_seed" => &["size", "put", "get"],
        "cpu_time" => &["time"],
        "get_command_argument" => &["number","value","length","status"],
        "getcwd" => &["c","status"],
        "get_environment_variable" => &["name","value","length","status","trim_name"],
        "date_and_time" => &["date", "time", "zone", "values"],
        "system_clock" => &["count", "count_rate", "count_max"],
        "real" | "int" | "nint" | "aint" | "anint" | "floor" | "ceiling" => &["a", "kind"],
        "char" | "achar" => &["i", "kind"],
        "ichar" | "iachar" => &["c", "kind"],
        "len" | "len_trim" => &["string", "kind"],
        "sign" | "dim" => &["a", "b"],
        "dprod" => &["x", "y"],
        "dot_product" => &["vector_a", "vector_b"],
        "matmul" => &["matrix_a", "matrix_b"],
        "transpose" => &["matrix"],
        "merge" => &["tsource", "fsource", "mask"],
        "lge" | "lgt" | "lle" | "llt" => &["string_a", "string_b"],
        "tiny" | "huge" => &["x"],
        "allocated" => &["array"],
        "present" => &["a"],
        "selected_int_kind" => &["r"],
        "selected_real_kind" => &["p", "r", "radix"],
        "max" | "min" => &["a1", "a2"],
        "ishftc" => &["i", "shift", "size"],
        _ => return None,
    };
    Some(Signature {
        params: keys.iter().map(|s| (*s).into()).collect(),
        optional: if name=="cmplx" {HashSet::from(["y".into(),"kind".into()])}else if matches!(name,"index"|"scan"|"verify") {
            HashSet::from(["back".into(),"kind".into()])
        } else if name=="get_command_argument" {HashSet::from(["value".into(),"length".into(),"status".into()])
        } else if name=="getcwd" {HashSet::from(["status".into()])
        } else if name=="get_environment_variable" {
            HashSet::from(["value".into(),"length".into(),"status".into(),"trim_name".into()])
        } else if name=="date_and_time" {
            HashSet::from(["date".into(),"time".into(),"zone".into(),"values".into()])
        } else if name=="system_clock" {
            HashSet::from(["count".into(),"count_rate".into(),"count_max".into()])
        } else if name=="random_seed" {
            HashSet::from(["size".into(),"put".into(),"get".into()])
        } else if name=="selected_real_kind" {
            HashSet::from(["p".into(),"r".into(),"radix".into()])
        } else if name=="ishftc" {HashSet::from(["size".into()])
        } else if matches!(name, "real" | "int" | "nint" | "aint" | "anint" | "floor" | "ceiling" | "char" | "achar" | "ichar" | "iachar" | "len" | "len_trim") {
            HashSet::from(["kind".into()])
        } else {
            HashSet::new()
        },
    })
}
fn intrinsic_keywords(name: &str) -> bool {
    // These routines already associate optional keywords during intrinsic lowering.
    matches!(
        name,
        "size"
            | "lbound"
            | "ubound"
            | "shape"
            | "sum"
            | "product"
            | "minval"
            | "maxval"
            | "count"
            | "any"
            | "all"
            | "minloc"
            | "maxloc"
            | "findloc"
            | "real"
            | "int"
            | "reshape"
            | "cshift"
            | "eoshift"
            | "spread"
            | "transpose"
            | "pack"
            | "unpack"
    )
}
struct Resolver {
    signatures: HashMap<String, Signature>,
    generics: HashSet<String>,
    duplicates: HashSet<String>,
    final_pass: bool,
}
impl Resolver {
    fn collect(&mut self, body: &[Stmt]) {
        for s in body {
            match s {
                Stmt::GenericInterface {name,..} => { self.generics.insert(name.clone()); },
                Stmt::Function {
                    name, params, body, ..
                }
                | Stmt::Subroutine {
                    name, params, body, ..
                } => {
                    if self
                        .signatures
                        .insert(name.clone(), Signature::new(params, body))
                        .is_some()
                    {
                        self.duplicates.insert(name.clone());
                    }
                    self.collect(body);
                }
                Stmt::Module { body, .. } | Stmt::Block { body } => self.collect(body),
                _ => {}
            }
        }
    }
    fn call(&self, name: &str, args: &mut Vec<Expr>, arrays: &HashSet<String>) -> Result<()> {
        if has_keywords(args) && (!name.starts_with('@') || self.signatures.contains_key(name) || name.starts_with("@module$")) {
            if arrays.contains(name) {
                bail!("keyword arguments cannot be used in array subscripts of `{name}`");
            }
            if self.duplicates.contains(name) {
                bail!("keyword call to `{name}` requires a unique procedure name within this compilation");
            }
            if let Some(mut signature) = self
                .signatures
                .get(name)
                .cloned()
                // Generic selection must associate each candidate's own keywords
                // before falling back to an intrinsic signature.
                .or_else(|| if !self.final_pass && self.generics.contains(name) {None} else {intrinsic(name)})
            {
                if matches!(name,"min"|"max") && !self.signatures.contains_key(name) {
                    let mut count=args.len().max(2);
                    for arg in args.iter() {
                        if let Some((key,_))=keyword(arg) {
                            if let Some(n)=key.strip_prefix('a').and_then(|s|s.parse::<usize>().ok()).filter(|n|*n>=1 && *n<=1024) {count=count.max(n);}
                        }
                    }
                    signature.params=(1..=count).map(|n|format!("a{n}")).collect();
                    signature.optional=signature.params.iter().skip(2).cloned().collect();
                }
                signature.reorder(name, args)?;
                if matches!(name,"min"|"max") && !self.signatures.contains_key(name) {
                    args.retain(|e|!omitted(e));
                }
            } else if self.final_pass && !name.contains('%') && !intrinsic_keywords(name) {
                bail!("keyword arguments to `{name}` require a known explicit procedure interface");
            }
        }
        for e in args {
            self.expression(e, arrays)?;
        }
        Ok(())
    }
    fn expression(&self, e: &mut Expr, arrays: &HashSet<String>) -> Result<()> {
        match e {
            Expr::Call(n, args) | Expr::Index(n, args) => self.call(n, args, arrays)?,
            Expr::Bin(_, a, b) => {
                self.expression(a, arrays)?;
                self.expression(b, arrays)?;
            }
            Expr::Un(_, e) => self.expression(e, arrays)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, arrays)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&self, body: &mut [Stmt], inherited: &HashSet<String>) -> Result<()> {
        let mut arrays = inherited.clone();
        for s in body.iter() {
            if let Stmt::VarDecl { names, .. } = s {
                for n in names {
                    arrays.remove(n);
                }
            }
        }
        for s in body.iter() {
            if let Stmt::ArrayDecl { name, .. } = s {
                arrays.insert(name.clone());
            }
        }
        for s in body {
            match s {
                Stmt::CallSub { name, args } => self.call(name, args, &arrays)?,
                Stmt::Assign { value, .. } | Stmt::Return(Some(value)) => {
                    self.expression(value, &arrays)?
                }
                Stmt::AssignIndex {
                    name,
                    indices,
                    value,
                } => {
                    self.call(name, indices, &arrays)?;
                    self.expression(value, &arrays)?;
                }
                Stmt::Print { items } | Stmt::Read { args: items } => {
                    for e in items {
                        self.expression(e, &arrays)?;
                    }
                }
                Stmt::InitializedDecl { entities, .. } => {
                    for (_, e) in entities {
                        if let Some(e) = e {
                            self.expression(e, &arrays)?;
                        }
                    }
                }
                Stmt::ArrayDecl { dims, .. } => {
                    for e in dims {
                        self.expression(e, &arrays)?;
                    }
                }
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.expression(cond, &arrays)?;
                    self.body(then_body, &arrays)?;
                    if let Some(b) = else_body {
                        self.body(b, &arrays)?;
                    }
                }
                Stmt::Do {
                    start, end, body, ..
                } => {
                    self.expression(start, &arrays)?;
                    self.expression(end, &arrays)?;
                    self.body(body, &arrays)?;
                }
                Stmt::DoWhile { cond, body } => {
                    self.expression(cond, &arrays)?;
                    self.body(body, &arrays)?;
                }
                Stmt::Function { params, body, .. } | Stmt::Subroutine { params, body, .. } => {
                    let mut inherited = arrays.clone();
                    for n in params {
                        inherited.remove(n);
                    }
                    self.body(body, &inherited)?;
                }
                Stmt::Block { body } => self.body(body, &arrays)?,
                Stmt::Module { body, .. } => self.body(body, &HashSet::new())?,
                Stmt::DerivedType { components, .. } => self.body(components, &HashSet::new())?,
                Stmt::SelectCase {
                    expr,
                    cases,
                    default,
                } => {
                    self.expression(expr, &arrays)?;
                    for c in cases {
                        self.body(&mut c.body, &arrays)?;
                    }
                    if let Some(b) = default {
                        self.body(b, &arrays)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
pub fn resolve(mut program: Program, final_pass: bool) -> Result<Program> {
    let mut resolver = Resolver {
        signatures: HashMap::new(),
        generics: HashSet::new(),
        duplicates: HashSet::new(),
        final_pass,
    };
    resolver.collect(&program.body);
    resolver.body(&mut program.body, &HashSet::new())?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn implemented_intrinsics_have_keyword_association() {
        for name in crate::intrinsics::REAL_MATH.iter().map(|(n,_)|*n)
            .chain(crate::intrinsics::REAL_PREDICATES.iter().copied())
            .chain(crate::bit_intrinsics::NAMES.iter().copied())
            .chain(["random_number","random_seed","cpu_time","system_clock","date_and_time","get_environment_variable","getcwd","get_command_argument","abs","dble","kind","mod","modulo","new_line",
                "adjustl","adjustr","trim","repeat","index","scan","verify","real","int","nint","aint","anint",
                "floor","ceiling","char","achar","ichar","iachar","len","len_trim","sign","dim","dprod",
                "dot_product","matmul","merge","lge","lgt","lle","llt","tiny","huge","allocated","present",
                "selected_int_kind","selected_real_kind","max","min","transpose","epsilon","digits","radix",
                "precision","range","minexponent","maxexponent","size","lbound","ubound","shape","sum","product",
                "minval","maxval","count","any","all","minloc","maxloc","findloc","reshape","cshift","eoshift","spread","pack","unpack"]) {
            assert!(intrinsic(name).is_some() || intrinsic_keywords(name),"missing keyword interface for {name}");
            assert!(crate::intrinsics::purity(name).is_some(), "missing intrinsic purity classification for {name}");
            if let Some(signature)=intrinsic(name) {
                let mut args=signature.params.iter().enumerate().rev().map(|(i,p)|Expr::Call(format!("@keyword:{p}"),vec![Expr::IntLit(i.to_string())])).collect();
                signature.reorder(name,&mut args).unwrap();
                for (i,arg) in args.iter().enumerate() {assert_eq!(arg,&Expr::IntLit(i.to_string()),"{name}");}
            }
        }
    }
    fn actual(key: &str) -> Expr {
        Expr::Call(format!("@keyword:{key}"), vec![Expr::IntLit("1".into())])
    }
    fn signature() -> Signature {
        Signature {
            params: vec!["a".into(), "b".into(), "c".into()],
            optional: HashSet::from(["b".into()]),
        }
    }
    #[test]
    fn associates_keywords_and_optional_holes() {
        let mut args = vec![actual("C"), actual("a")];
        signature().reorder("f", &mut args).unwrap();
        assert_eq!(args.len(), 3);
        assert!(omitted(&args[1]));
        assert!(!has_keywords(&args));
        let args = vec![Expr::IntLit("2".into()), actual("c"), actual("b")];
        assert_eq!(
            signature().associate("f", &args).unwrap(),
            vec![Some(0), Some(2), Some(1)]
        );
    }
    #[test]
    fn rejects_invalid_associations() {
        for (args, message) in [
            (vec![actual("z")], "unknown keyword"),
            (vec![actual("a"), actual("a")], "duplicate argument"),
            (
                vec![Expr::IntLit("2".into()), actual("a")],
                "duplicate argument",
            ),
            (
                vec![actual("a"), Expr::IntLit("2".into())],
                "positional argument after",
            ),
            (vec![actual("a")], "missing required argument `c`"),
        ] {
            assert!(signature()
                .associate("f", &args)
                .unwrap_err()
                .to_string()
                .contains(message));
        }
    }
}

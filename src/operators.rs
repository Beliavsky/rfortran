//! Resolve module-defined intrinsic operators before derived values are flattened.
//! Selection uses the original operand types, without numeric argument conversion.
use crate::ast::{BinOp, Expr, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
enum Ty {
    Derived(String),
    Scalar(TypeSpec),
    Array(Box<Ty>),
    Unknown,
}
type Vars = HashMap<String, Ty>;
type Bindings = HashMap<String, Vec<String>>;

fn scalar(kind: &TypeSpec) -> Ty {
    Ty::Scalar(match kind {
        TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)),
        TypeSpec::RealKind(k) if k == "4" => TypeSpec::Real,
        TypeSpec::RealKind(k) if k == "8" => TypeSpec::DoublePrecision,
        TypeSpec::Character(_) => TypeSpec::Character(None),
        _ => kind.clone(),
    })
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
            Stmt::ArrayDecl { kind, name, .. } => {
                vars.insert(name.clone(), Ty::Array(Box::new(scalar(kind))));
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
    args: Vec<Ty>,
    result: Ty,
    inputs: bool,
}
#[derive(Default)]
struct Resolver {
    procedures: HashMap<String, Procedure>,
    duplicate_procedures: HashSet<String>,
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
                    ..
                } => {
                    let vars = declarations(body, &Vars::new());
                    let result = return_type.as_ref().map(scalar).unwrap_or_else(|| {
                        let result_name = match body.last() {
                            Some(Stmt::Return(Some(Expr::Ident(n)))) => n,
                            _ => name,
                        };
                        vars.get(result_name).cloned().unwrap_or(Ty::Unknown)
                    });
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
                                args: params
                                    .iter()
                                    .map(|p| vars.get(p).cloned().unwrap_or(Ty::Unknown))
                                    .collect(),
                                result,
                                inputs: params.iter().all(|p| inputs.contains(p)),
                            },
                        )
                        .is_some()
                    {
                        self.duplicate_procedures.insert(name.clone());
                    }
                    self.collect(body)?;
                }
                Stmt::Module { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Block { body } => self.collect(body)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn local_bindings(&self, body: &[Stmt]) -> Result<Bindings> {
        let mut bindings = Bindings::new();
        for s in body {
            if let Stmt::OperatorInterface {
                operator,
                procedures,
            } = s
            {
                for name in procedures {
                    if self.duplicate_procedures.contains(name) {
                        bail!("operator procedure `{name}` must have a unique name within this compilation");
                    }
                    let Some(p) = self.procedures.get(name) else {
                        bail!("operator({operator}) requires a defined function `{name}`");
                    };
                    let arity_ok = match operator.as_str() {
                        "+" | "-" => matches!(p.args.len(), 1 | 2),
                        ".not." => p.args.len() == 1,
                        _ => p.args.len() == 2,
                    };
                    if !arity_ok || !p.inputs {
                        bail!("operator({operator}) function `{name}` must have the appropriate number of INTENT(IN) operands");
                    }
                    if matches!(p.result, Ty::Unknown | Ty::Array(_))
                        || p.args
                            .iter()
                            .any(|a| matches!(a, Ty::Unknown | Ty::Array(_)))
                    {
                        bail!("operator({operator}) function `{name}` requires explicitly typed scalar arguments and result");
                    }
                    if !p.args.iter().any(|a| matches!(a, Ty::Derived(_))) {
                        bail!("operator({operator}) overload `{name}` currently requires a derived-type operand");
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
            if let Stmt::Use { module, only } = s {
                if let Some(exported) = self.exports.get(module) {
                    let selected = exported.iter().filter(|(op, _)| only.as_ref().is_none_or(|names| names.contains(&format!("operator({op})"))))
                        .map(|(op, names)| (op.clone(), names.clone())).collect();
                    merge(&mut visible, &selected);
                }
            }
        }
        Ok(visible)
    }
    fn select(&self, op: &str, args: &[Ty], visible: &Bindings) -> Result<Option<String>> {
        if !args.iter().any(|a| matches!(a, Ty::Derived(_))) {
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
        if let Expr::Call(name, args) = e {
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
                for a in args {
                    arg_types.push(self.expression(a, vars, visible)?);
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
                        Ty::Array(t) => *t,
                        _ => match n.as_str() {
                            "int" | "nint" | "floor" | "ceiling" | "size" | "len" => {
                                scalar(&TypeSpec::Integer(None))
                            }
                            "real" => scalar(&TypeSpec::Real),
                            "dble" => scalar(&TypeSpec::DoublePrecision),
                            "abs" | "mod" | "modulo" | "min" | "max" => {
                                arg_types.first().cloned().unwrap_or(Ty::Unknown)
                            }
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
                } else {
                    left
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
                lower,
                upper,
                stride,
                ..
            } => {
                for a in [lower, upper, stride].into_iter().flatten() {
                    self.expression(a, vars, visible)?;
                }
                Ty::Unknown
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
                Stmt::Print { items }
                | Stmt::CallSub { args: items, .. }
                | Stmt::Read { args: items } => {
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
        body.retain(|s| !matches!(s, Stmt::OperatorInterface { .. }));
        Ok(())
    }
}

pub fn resolve(mut program: Program) -> Result<Program> {
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
            exports.retain(|op, _| {
                *access
                    .get(&format!("operator({op})"))
                    .unwrap_or(&default_public)
            });
            resolver.exports.insert(name.clone(), exports);
        }
    }
    resolver.body(&mut program.body, &Vars::new(), &Bindings::new())?;
    Ok(program)
}

#[cfg(test)]
mod tests {
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

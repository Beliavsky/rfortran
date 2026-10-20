// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! By-reference calls of external subroutines, as in FORTRAN 77.
//!
//! Call-site specialization needs the callee's source. A separately compiled
//! subroutine instead gets an exported entry point, `rfort$name`, that takes
//! each actual argument by reference: the address of its storage, the address
//! of its initialization flags, its name for diagnostics, and the number of
//! elements from that address to the end of the actual array. A caller needs
//! no interface, so it can pass a scalar, a whole array or an array element
//! (sequence association) alike, and the callee applies its own declared
//! bounds, including assumed size `(*)`. Dummies become borrowed pointer
//! descriptors, so element accesses keep their bounds and initialization
//! checks and writes reach the caller's storage.
//!
//! A call to a subroutine defined in the same compilation keeps call-site
//! specialization unless only the entry point supports it: an assumed-size
//! dummy, or an array element passed to an array dummy.
use crate::ast::{BinOp, Expr, Implicit, Program, Stmt, TypeSpec, UnOp};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

/// The symbol prefix of by-reference entry points.
pub(crate) const PREFIX: &str = "rfort$";
/// The result dummy of a function's entry point.
const RESULT: &str = "@external_result";
/// Fields that each actual argument passes, in order.
const FIELDS: [&str; 4] = ["data", "flags", "name", "count"];

fn id(n: &str) -> Expr {
    Expr::Ident(n.into())
}
fn int(n: i64) -> Expr {
    Expr::IntLit(n.to_string())
}
fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}
fn assign(n: &str, e: Expr) -> Stmt {
    Stmt::Assign { name: n.into(), value: e }
}
fn integers(names: Vec<String>) -> Stmt {
    Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names }
}
fn call(n: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(n.into(), args)
}

/// Intrinsic subroutines, which are never external.
const INTRINSIC_SUBROUTINES: &[&str] = &[
    "random_number", "random_seed", "cpu_time", "system_clock", "date_and_time", "get_environment_variable",
    "getcwd", "get_command_argument", "get_command", "execute_command_line", "move_alloc", "mvbits",
    "tokenize", "split", "abort", "exit", "flush", "sleep",
];

fn character(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_))
}

/// Characters an assumed-length CHARACTER dummy can receive.
const CHARACTER_CAPACITY: usize = 1024;

fn numeric(kind: &TypeSpec) -> bool {
    matches!(kind, TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) | TypeSpec::Real | TypeSpec::RealKind(_)
        | TypeSpec::DoublePrecision | TypeSpec::Logical | TypeSpec::LogicalKind(_))
}

/// A dummy that a by-reference entry point receives.
#[derive(Clone, Debug)]
struct Borrowed {
    data: String,
    flags: String,
    name: String,
    count: String,
    /// Lower bound, extent and stride of each dimension.
    axes: Vec<[String; 3]>,
}

/// What a procedure body declares, for binding the actual arguments it passes.
#[derive(Clone, Default)]
struct Scope {
    kinds: HashMap<String, TypeSpec>,
    ranks: HashMap<String, usize>,
    constants: HashSet<String>,
    borrowed: HashMap<String, Borrowed>,
    /// Dummy procedures and other names that are not data.
    procedures: HashSet<String>,
    dummies: HashSet<String>,
    /// Names declared EXTERNAL.
    externals: HashSet<String>,
    /// Statement functions, defined like `f(x) = ...` for a scalar `f`.
    statement_functions: HashSet<String>,
    /// Whether a USE statement may supply procedures that are not declared.
    uses: bool,
    /// Names used as variables: assigned, read, or referenced without
    /// parentheses. Such a name followed by parentheses is not a function.
    variables: HashSet<String>,
    /// Whether the procedure is PURE or ELEMENTAL, which cannot make the
    /// impure by-reference calls.
    pure: bool,
    implicit: HashMap<char, TypeSpec>,
}

impl Scope {
    fn new(host: Option<&Scope>) -> Self {
        let mut scope = host.cloned().unwrap_or_default();
        if host.is_none() {
            for c in 'a'..='z' {
                let kind = if ('i'..='n').contains(&c) { TypeSpec::Integer(None) } else { TypeSpec::Real };
                scope.implicit.insert(c, kind);
            }
        }
        scope
    }
    fn declare(&mut self, body: &[Stmt], params: &[String]) {
        self.dummies.extend(params.iter().cloned());
        self.uses |= body.iter().any(|s| matches!(s, Stmt::Use { .. }));
        fn variables(body: &[Stmt], out: &mut HashSet<String>) {
            for s in body {
                match s {
                    Stmt::Assign { name, .. } | Stmt::Do { var: name, .. } => {
                        out.insert(name.clone());
                    }
                    Stmt::Read { args } => {
                        for a in args {
                            if let Expr::Ident(n) = a {
                                out.insert(n.clone());
                            }
                        }
                    }
                    _ => {}
                }
                for e in crate::procedure_arguments::expressions(&mut s.clone()) {
                    crate::procedure_arguments::visit_expr(e, &mut |e| {
                        if let Expr::Ident(n) = e {
                            out.insert(n.clone());
                        }
                    });
                }
                if !matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. }) {
                    for b in crate::procedure_arguments::children(&mut s.clone()) {
                        variables(b, out);
                    }
                }
            }
        }
        variables(body, &mut self.variables);
        let arrays: HashSet<&String> = body.iter().filter_map(|s| match s {
            Stmt::ArrayDecl { name, .. } => Some(name),
            _ => None,
        }).collect();
        for s in body {
            if let Stmt::AssignIndex { name, .. } = s {
                if !arrays.contains(name) {
                    self.statement_functions.insert(name.clone());
                }
            }
        }
        for s in body {
            match s {
                Stmt::VarDecl { kind, names } => {
                    for n in names {
                        self.kinds.insert(n.clone(), kind.clone());
                        self.ranks.remove(n);
                    }
                }
                Stmt::ArrayDecl { kind, name, dims } => {
                    self.kinds.insert(name.clone(), kind.clone());
                    self.ranks.insert(name.clone(), dims.len());
                }
                Stmt::InitializedDecl { kind, attributes, entities } => {
                    // A PARAMETER statement has no type of its own; the name
                    // keeps its declared or implicit one.
                    let separate = attributes.iter().any(|a| a == "@separate_parameter");
                    for (n, _) in entities {
                        if !separate {
                            self.kinds.insert(n.clone(), kind.clone());
                        }
                        if attributes.iter().any(|a| a == "parameter") {
                            self.constants.insert(n.clone());
                        }
                    }
                }
                Stmt::DummyIntent { name, intent } if intent == "external" => {
                    self.externals.insert(name.clone());
                    if params.contains(name) {
                        self.procedures.insert(name.clone());
                    }
                }
                Stmt::Implicit(Implicit::None) => self.implicit.clear(),
                Stmt::Implicit(Implicit::Rule { type_spec, letter_ranges }) => {
                    for r in letter_ranges {
                        for c in r.start..=r.end.unwrap_or(r.start) {
                            self.implicit.insert(c, type_spec.clone());
                        }
                    }
                }
                Stmt::Implicit(Implicit::Rules { rules }) => {
                    for rule in rules {
                        for r in &rule.letter_ranges {
                            for c in r.start..=r.end.unwrap_or(r.start) {
                                self.implicit.insert(c, rule.type_spec.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn kind(&self, name: &str) -> Option<TypeSpec> {
        self.kinds.get(name).cloned().or_else(|| name.chars().next().and_then(|c| self.implicit.get(&c).cloned()))
    }
    fn is_array(&self, name: &str) -> bool {
        self.ranks.contains_key(name) || self.borrowed.get(name).is_some_and(|b| !b.axes.is_empty())
    }
}

/// The interface of a subroutine defined in this compilation.
#[derive(Clone)]
struct Definition {
    /// The rank of each dummy, in order.
    ranks: Vec<usize>,
    /// Whether an array dummy has explicit bounds known only at run time,
    /// which call-site specialization requires the actual to match.
    runtime_shape: bool,
    /// For each dummy, whether call-site specialization requires a variable
    /// actual: one without INTENT of a procedure with array or CHARACTER
    /// dummies.
    variable: Vec<bool>,
    /// Whether it has a by-reference entry point.
    native: bool,
    /// Whether only the entry point remains: the procedure has an
    /// assumed-size dummy, or, in a compilation without a main program, an
    /// array or CHARACTER dummy, which call-site specialization handles only
    /// for the calls it sees.
    native_only: bool,
    /// The result type of a function.
    function: Option<TypeSpec>,
}

/// The type of an actual argument expression, for its temporary.
fn infer(scope: &Scope, e: &Expr) -> Result<TypeSpec> {
    fn order(kind: &TypeSpec) -> u8 {
        match kind {
            TypeSpec::Integer(_) | TypeSpec::IntegerKind(_) => 1,
            TypeSpec::Real => 2,
            TypeSpec::RealKind(k) if k == "4" => 2,
            TypeSpec::DoublePrecision | TypeSpec::RealKind(_) => 3,
            _ => 0,
        }
    }
    fn wider(a: TypeSpec, b: TypeSpec) -> TypeSpec {
        match (&a, &b) {
            (TypeSpec::Integer(x), TypeSpec::Integer(y)) => TypeSpec::Integer(match (x, y) {
                (Some(x), Some(y)) => Some(*x.max(y)),
                (Some(x), None) | (None, Some(x)) => Some((*x).max(4)),
                (None, None) => None,
            }),
            _ if order(&b) > order(&a) => b,
            _ => a,
        }
    }
    Ok(match e {
        Expr::IntLit(text) => match text.split_once('_') {
            Some((_, kind)) => TypeSpec::IntegerKind(kind.into()),
            None => TypeSpec::Integer(None),
        },
        Expr::RealLit(text) => match text.split_once('_') {
            Some((_, kind)) => TypeSpec::RealKind(kind.into()),
            None if text.contains(['d', 'D']) => TypeSpec::DoublePrecision,
            None => TypeSpec::Real,
        },
        Expr::Logical(_) => TypeSpec::Logical,
        Expr::Str(_) => bail!("CHARACTER actual arguments to external subroutines are not yet supported"),
        Expr::Ident(n) => scope.kind(n).with_context(|| format!("`{n}` has no type"))?,
        Expr::Index(n, _) => scope.kind(n).with_context(|| format!("`{n}` has no type"))?,
        Expr::Call(n, args) => {
            if scope.is_array(n) {
                return scope.kind(n).with_context(|| format!("`{n}` has no type"));
            }
            let argument = |i: usize| -> Result<TypeSpec> {
                infer(scope, args.get(i).with_context(|| format!("missing argument of `{n}`"))?)
            };
            match n.as_str() {
                "dble" | "dfloat" | "dsqrt" | "dabs" | "dexp" | "dlog" | "dlog10" | "dsin" | "dcos" | "dtan" | "dasin"
                | "dacos" | "datan" | "datan2" | "dsinh" | "dcosh" | "dtanh" | "dsign" | "dmax1" | "dmin1" | "dmod"
                | "dprod" | "dint" | "dnint" | "ddim" => TypeSpec::DoublePrecision,
                "real" | "float" | "sngl" | "amax1" | "amin1" | "amax0" | "amin0" | "alog" | "alog10" | "amod" => {
                    TypeSpec::Real
                }
                "int" | "nint" | "ifix" | "idint" | "idnint" | "len" | "len_trim" | "index" | "ichar" | "iachar"
                | "size" | "lbound" | "ubound" | "max0" | "min0" | "max1" | "min1" | "iabs" | "isign" | "idim" | "kind"
                | "scan" | "verify" | "ceiling" | "floor" | "exponent" | "digits" | "range" | "precision"
                | "maxexponent" | "minexponent" | "radix" | "bit_size" | "iand" | "ior" | "ieor" | "ishft" | "not" => {
                    TypeSpec::Integer(None)
                }
                "lge" | "lgt" | "lle" | "llt" | "btest" | "isnan" | "present" | "allocated" | "associated" | "any"
                | "all" | "logical" => TypeSpec::Logical,
                "abs" | "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh"
                | "cosh" | "tanh" | "aint" | "anint" | "epsilon" | "huge" | "tiny" | "spacing" | "fraction" | "nearest"
                | "rrspacing" | "scale" | "set_exponent" | "sum" | "product" | "maxval" | "minval" => argument(0)?,
                "max" | "min" | "mod" | "modulo" | "sign" | "dim" | "atan2" => {
                    let mut kind = argument(0)?;
                    for i in 1..args.len() {
                        kind = wider(kind, argument(i)?);
                    }
                    kind
                }
                _ => scope.kind(n).with_context(|| format!("`{n}` has no type"))?,
            }
        }
        Expr::Bin(op, a, b) => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => wider(infer(scope, a)?, infer(scope, b)?),
            BinOp::Pow => {
                let base = infer(scope, a)?;
                let exponent = infer(scope, b)?;
                if order(&exponent) > 1 { wider(base, exponent) } else { base }
            }
            BinOp::Concat => bail!("CHARACTER actual arguments to external subroutines are not yet supported"),
            _ => TypeSpec::Logical,
        },
        Expr::Un(UnOp::Neg, a) => infer(scope, a)?,
        Expr::Un(UnOp::Not, _) => TypeSpec::Logical,
        Expr::Section { .. } => bail!("array section actual arguments to external subroutines are not yet supported"),
    })
}

/// Whether an actual argument is given by keyword, which needs an interface.
fn keyword(e: &Expr) -> bool {
    matches!(e, Expr::Call(tag, _) if tag.starts_with("@keyword:"))
}

/// Whether an actual argument is, or is an element of, a dummy of an entry
/// point, which only another entry point can receive.
fn borrowed(e: &Expr, scope: &Scope) -> bool {
    matches!(e, Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) if scope.borrowed.contains_key(n))
}

/// Whether an actual argument is a scalar CHARACTER value.
fn character_value(scope: &Scope, e: &Expr) -> Result<bool> {
    let kind = |n: &str| scope.kind(n).is_some_and(|k| character(&k));
    Ok(match e {
        Expr::Str(_) | Expr::Bin(BinOp::Concat, ..) => true,
        Expr::Ident(n) | Expr::Section { name: n, .. } | Expr::Call(n, _) | Expr::Index(n, _) if kind(n) => {
            if scope.is_array(n) {
                bail!("CHARACTER array actual arguments to external procedures are not yet supported");
            }
            true
        }
        Expr::Call(n, _) => matches!(n.as_str(), "trim" | "adjustl" | "adjustr" | "repeat" | "char" | "achar"),
        _ => false,
    })
}

/// Intrinsics whose result is a scalar for a single array argument.
const SCALAR_INTRINSICS: &[&str] = &[
    "size", "sum", "product", "maxval", "minval", "count", "any", "all", "norm2", "len", "len_trim", "kind",
    "rank", "epsilon", "huge", "tiny", "digits", "radix", "precision", "range", "minexponent", "maxexponent",
];

/// Whether an expression refers to a whole array, which only a variable
/// actual can pass.
fn array_valued(scope: &Scope, e: &Expr) -> bool {
    match e {
        Expr::Ident(n) => scope.is_array(n),
        Expr::Section { .. } => true,
        // Inquiry and reduction intrinsics of a whole array are scalars.
        Expr::Call(n, args) if !scope.is_array(n) && SCALAR_INTRINSICS.contains(&n.as_str()) && args.len() == 1 => false,
        Expr::Call(n, args) | Expr::Index(n, args) => {
            args.iter().any(|a| matches!(a, Expr::Section { .. }) || (!scope.is_array(n) && array_valued(scope, a)))
        }
        Expr::Bin(_, a, b) => array_valued(scope, a) || array_valued(scope, b),
        Expr::Un(_, a) => array_valued(scope, a),
        _ => false,
    }
}

struct Lower {
    definitions: HashMap<String, Definition>,
    /// Whether the compilation has no main program: a library of FORTRAN 77
    /// procedures, whose typed function names need no EXTERNAL.
    library: bool,
    /// Procedures defined in this compilation that are not external.
    others: HashSet<String>,
    serial: usize,
}

impl Lower {
    fn fresh(&mut self, stem: &str) -> String {
        self.serial += 1;
        format!("@external_{stem}_{}", self.serial)
    }

    /// The fields that pass `actual`, and the statements that compute them.
    fn bind(&mut self, scope: &Scope, actual: &Expr, callee: &str, out: &mut Vec<Stmt>) -> Result<[Expr; 4]> {
        if character_value(scope, actual)? {
            let (address, length) = (self.fresh("address"), self.fresh("length"));
            out.push(integers(vec![address.clone(), length.clone()]));
            out.push(Stmt::CallSub { name: "@character_address".into(), args: vec![id(&address), actual.clone()] });
            out.push(Stmt::CallSub { name: "@character_length".into(), args: vec![id(&length), actual.clone()] });
            return Ok([id(&address), int(0), int(0), id(&length)]);
        }
        // A dummy of this entry point passes on its own reference.
        if let Expr::Ident(n) = actual {
            if let Some(b) = scope.borrowed.get(n) {
                return Ok([id(&b.data), id(&b.flags), id(&b.name), id(&b.count)]);
            }
        }
        let element = match actual {
            Expr::Call(n, indices) | Expr::Index(n, indices) if scope.is_array(n) => {
                if indices.iter().any(|i| matches!(i, Expr::Section { .. }) || array_valued(scope, i)) {
                    bail!("array section actual arguments to external subroutine `{callee}` are not yet supported");
                }
                Some((n.clone(), indices.clone()))
            }
            _ => None,
        };
        if let Some((array, indices)) = element {
            let fields: Vec<String> = FIELDS.iter().map(|f| self.fresh(f)).collect();
            out.push(integers(fields.clone()));
            let offset = self.fresh("offset");
            out.push(integers(vec![offset.clone()]));
            if let Some(b) = scope.borrowed.get(&array).cloned() {
                if b.axes.len() != indices.len() {
                    bail!("array `{array}` has rank {} but {} subscripts", b.axes.len(), indices.len());
                }
                let mut sum = int(0);
                for (index, [lower, _, stride]) in indices.iter().zip(&b.axes) {
                    sum = bin(BinOp::Add, sum, bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), id(lower)), id(stride)));
                }
                out.push(assign(&offset, sum));
                self.check_offset(&array, &offset, &id(&b.count), out);
                out.push(assign(&fields[0], bin(BinOp::Add, id(&b.data), bin(BinOp::Mul, int(8), id(&offset)))));
                out.push(assign(&fields[1], bin(BinOp::Add, id(&b.flags), bin(BinOp::Mul, int(8), id(&offset)))));
                out.push(assign(&fields[2], id(&b.name)));
                out.push(assign(&fields[3], bin(BinOp::Sub, id(&b.count), id(&offset))));
            } else {
                let rank = scope.ranks[&array];
                if rank != indices.len() {
                    bail!("array `{array}` has rank {rank} but {} subscripts", indices.len());
                }
                let mut sum = int(0);
                let mut stride = int(1);
                for (axis, index) in indices.iter().enumerate() {
                    let dim = int(axis as i64 + 1);
                    let lower = call("lbound", vec![id(&array), dim.clone()]);
                    sum = bin(BinOp::Add, sum, bin(BinOp::Mul, bin(BinOp::Sub, index.clone(), lower), stride.clone()));
                    stride = bin(BinOp::Mul, stride, call("size", vec![id(&array), dim]));
                }
                out.push(assign(&offset, sum));
                let size = call("size", vec![id(&array)]);
                self.check_offset(&array, &offset, &size, out);
                out.push(Stmt::CallSub {
                    name: "@ptr_bind".into(),
                    args: vec![id(&fields[0]), id(&fields[1]), id(&fields[2]), id(&array)],
                });
                for field in &fields[..2] {
                    out.push(assign(field, bin(BinOp::Add, id(field), bin(BinOp::Mul, int(8), id(&offset)))));
                }
                out.push(assign(&fields[3], bin(BinOp::Sub, size, id(&offset))));
            }
            return Ok(fields.map_into());
        }
        let fields: Vec<String> = FIELDS.iter().map(|f| self.fresh(f)).collect();
        out.push(integers(fields.clone()));
        let target = match actual {
            Expr::Ident(n) if scope.procedures.contains(n) || self.definitions.contains_key(n) || self.others.contains(n) => {
                bail!("procedure actual arguments to external subroutine `{callee}` are not yet supported")
            }
            Expr::Ident(n) if !scope.constants.contains(n) => n.clone(),
            _ => {
                if array_valued(scope, actual) {
                    bail!("array expression actual arguments to external subroutine `{callee}` are not yet supported");
                }
                let kind = infer(scope, actual).with_context(|| format!("actual argument of external subroutine `{callee}`"))?;
                if !numeric(&kind) {
                    bail!("only numeric and LOGICAL actual arguments to external subroutine `{callee}` are supported");
                }
                let temporary = self.fresh("value");
                out.push(Stmt::VarDecl { kind, names: vec![temporary.clone()] });
                out.push(assign(&temporary, actual.clone()));
                temporary
            }
        };
        out.push(Stmt::CallSub {
            name: "@ptr_bind".into(),
            args: vec![id(&fields[0]), id(&fields[1]), id(&fields[2]), id(&target)],
        });
        let count = if scope.is_array(&target) { call("size", vec![id(&target)]) } else { int(1) };
        out.push(assign(&fields[3], count));
        Ok(fields.map_into())
    }

    fn check_offset(&mut self, array: &str, offset: &str, count: &Expr, out: &mut Vec<Stmt>) {
        let inside = bin(BinOp::And, bin(BinOp::Ge, id(offset), int(0)), bin(BinOp::Lt, id(offset), count.clone()));
        out.push(Stmt::If {
            cond: Expr::Un(UnOp::Not, Box::new(inside)),
            then_body: vec![Stmt::CallSub {
                name: "@error_stop".into(),
                args: vec![Expr::Str(format!("array element actual argument is outside array `{array}`"))],
            }],
            else_body: None,
        });
    }

    /// Rewrite the calls in `body` that need a by-reference entry point.
    fn calls(&mut self, body: &mut Vec<Stmt>, scope: &Scope) -> Result<()> {
        let mut location = None;
        for s in body.iter_mut() {
            if let Stmt::SourceLocation(offset) = s {
                location = Some(*offset);
            }
            let result = self.statement(s, scope);
            if let Err(error) = result {
                return Err(crate::source_diagnostics::at(error, location));
            }
        }
        Ok(())
    }

    fn statement(&mut self, s: &mut Stmt, scope: &Scope) -> Result<()> {
        // External function references are evaluated before the statement.
        let mut prefix = vec![];
        match s {
            Stmt::Subroutine { .. } | Stmt::Function { .. } | Stmt::Module { .. } | Stmt::VarDecl { .. }
            | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. } | Stmt::DerivedDecl { .. }
            | Stmt::DummyIntent { .. } => return Ok(()),
            Stmt::DoWhile { cond, body } => {
                // The condition is evaluated before every iteration.
                let mut evaluate = vec![];
                self.hoist(cond, scope, &mut evaluate)?;
                if !evaluate.is_empty() {
                    evaluate.push(Stmt::If { cond: Expr::Un(UnOp::Not, Box::new(cond.clone())), then_body: vec![Stmt::Exit], else_body: None });
                    evaluate.append(body);
                    *body = evaluate;
                    *cond = Expr::Logical(true);
                }
            }
            // An explicit-step DO or FORALL starts with its control, whose
            // bounds are evaluated before the loop.
            Stmt::Block { body } if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name == "@do_step" || name == "@forall") => {
                if let Some(Stmt::CallSub { args, .. }) = body.first_mut() {
                    for control in args.chunks_exact_mut(4) {
                        for e in &mut control[1..] {
                            self.hoist(e, scope, &mut prefix)?;
                        }
                    }
                }
            }
            // Keyword specifiers of I/O and other statements, such as
            // IOSTAT=, name variables rather than function results.
            Stmt::CallSub { name, args } if name.starts_with('@') => {
                let specifier = |e: &Expr| keyword(e) || matches!(e, Expr::Call(tag, _) if tag.starts_with("@io_spec:"));
                for e in args.iter_mut().filter(|e| !specifier(e)) {
                    self.hoist(e, scope, &mut prefix)?;
                }
            }
            _ => {
                for e in crate::procedure_arguments::expressions(s) {
                    self.hoist(e, scope, &mut prefix)?;
                }
            }
        }
        match s {
            Stmt::CallSub { name, args } if self.external(name, args, scope) => {
                let callee = name.clone();
                let mut out = vec![];
                let mut fields = vec![];
                for actual in args.iter() {
                    fields.extend(self.bind(scope, actual, &callee, &mut out)?);
                }
                out.push(Stmt::CallSub { name: format!("{PREFIX}{callee}"), args: fields });
                *s = Stmt::Block { body: out };
            }
            _ => {
                for body in crate::procedure_arguments::children(s) {
                    self.calls(body, scope)?;
                }
            }
        }
        if !prefix.is_empty() {
            prefix.push(std::mem::replace(s, Stmt::Exit));
            *s = Stmt::Block { body: prefix };
        }
        Ok(())
    }

    /// Replace each external function reference in `e` with a temporary
    /// that a call to its entry point sets.
    fn hoist(&mut self, e: &mut Expr, scope: &Scope, out: &mut Vec<Stmt>) -> Result<()> {
        match e {
            Expr::Call(_, args) | Expr::Index(_, args) => {
                for a in args.iter_mut() {
                    self.hoist(a, scope, out)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.hoist(a, scope, out)?;
                self.hoist(b, scope, out)?;
            }
            Expr::Un(_, a) => self.hoist(a, scope, out)?,
            Expr::Section { lower, upper, stride, .. } => {
                for x in [lower, upper, stride].into_iter().flatten() {
                    self.hoist(x, scope, out)?;
                }
            }
            _ => {}
        }
        let Expr::Call(name, args) = e else { return Ok(()) };
        if !self.external_function(name, args, scope) {
            return Ok(());
        }
        let callee = name.clone();
        let kind = match self.definitions.get(&callee).and_then(|d| d.function.clone()) {
            Some(kind) => kind,
            None => scope.kind(&callee).with_context(|| format!("external function `{callee}` has no type"))?,
        };
        if !numeric(&kind) && !matches!(kind, TypeSpec::Character(_)) {
            bail!("external function `{callee}` must currently have a numeric, LOGICAL or fixed-length CHARACTER result");
        }
        let value = self.fresh("result");
        let text = character(&kind);
        out.push(Stmt::VarDecl { kind, names: vec![value.clone()] });
        let mut passed: Vec<Expr> = if text {
            out.push(assign(&value, Expr::Str(String::new())));
            let (address, length) = (self.fresh("address"), self.fresh("length"));
            out.push(integers(vec![address.clone(), length.clone()]));
            out.push(Stmt::CallSub { name: "@character_address".into(), args: vec![id(&address), id(&value)] });
            out.push(Stmt::CallSub { name: "@character_length".into(), args: vec![id(&length), id(&value)] });
            vec![id(&address), int(0), int(0), id(&length)]
        } else {
            let fields: Vec<String> = FIELDS.iter().map(|f| self.fresh(f)).collect();
            out.push(integers(fields.clone()));
            out.push(Stmt::CallSub {
                name: "@ptr_bind".into(),
                args: vec![id(&fields[0]), id(&fields[1]), id(&fields[2]), id(&value)],
            });
            out.push(assign(&fields[3], int(1)));
            fields.map_into().into()
        };
        for actual in args.iter() {
            passed.extend(self.bind(scope, actual, &callee, out)?);
        }
        out.push(Stmt::CallSub { name: format!("{PREFIX}{callee}"), args: passed });
        *e = id(&value);
        Ok(())
    }

    /// Whether a function reference uses a by-reference entry point: one
    /// declared EXTERNAL and defined elsewhere, or one defined here that only
    /// the entry point supports.
    fn external_function(&self, name: &str, args: &[Expr], scope: &Scope) -> bool {
        if name.starts_with('@') || name.contains('%') || scope.is_array(name) || scope.dummies.contains(name)
            || self.others.contains(name) || args.iter().any(keyword) || scope.pure
        {
            return false;
        }
        match self.definitions.get(name) {
            Some(d) => d.function.is_some() && self.needs_entry(d, args, scope),
            // EXTERNAL, or a typed scalar name that is not an intrinsic or
            // statement function: FORTRAN 77 does not require EXTERNAL.
            None => scope.externals.contains(name)
                || self.library && !scope.uses && scope.kinds.contains_key(name) && !scope.statement_functions.contains(name)
                    && !scope.variables.contains(name)
                    && !name.contains('%') && crate::intrinsics::purity(name).is_none(),
        }
    }

    /// Whether a CALL statement uses a by-reference entry point.
    fn external(&self, name: &str, args: &[Expr], scope: &Scope) -> bool {
        if name.starts_with('@') || name.starts_with(PREFIX) || name.contains('%') || INTRINSIC_SUBROUTINES.contains(&name)
            || scope.procedures.contains(name) || scope.dummies.contains(name) || self.others.contains(name)
            || args.iter().any(keyword)
        {
            return false;
        }
        let Some(definition) = self.definitions.get(name) else {
            // An intrinsic, or a procedure a module may supply, needs EXTERNAL.
            let declared = scope.externals.contains(name);
            return declared || !scope.uses && crate::intrinsics::purity(name).is_none();
        };
        definition.function.is_none() && self.needs_entry(definition, args, scope)
    }

    /// Whether a call of a procedure defined here needs its entry point:
    /// call-site specialization cannot take an array element for an array
    /// dummy (sequence association), an actual of another shape than a
    /// runtime explicit shape, or a dummy of an entry point.
    fn needs_entry(&self, definition: &Definition, args: &[Expr], scope: &Scope) -> bool {
        definition.native
            && (definition.native_only
                || definition.runtime_shape
                || args.iter().any(|a| borrowed(a, scope))
                || args.iter().zip(&definition.variable).any(|(a, &variable)| {
                    variable && !matches!(a, Expr::Ident(n) if !scope.constants.contains(n))
                        && !matches!(a, Expr::Call(n, _) | Expr::Index(n, _) if scope.is_array(n))
                })
                || args.iter().zip(&definition.ranks).any(|(a, &rank)| {
                    rank > 0 && matches!(a, Expr::Call(n, _) | Expr::Index(n, _) if scope.is_array(n))
                }))
    }
}

trait MapInto {
    fn map_into(self) -> [Expr; 4];
}
impl MapInto for Vec<String> {
    fn map_into(self) -> [Expr; 4] {
        [id(&self[0]), id(&self[1]), id(&self[2]), id(&self[3])]
    }
}

/// The dummies of `params` that a by-reference entry point can receive, with
/// their kinds and dimensions, or None if one cannot be.
fn dummies(params: &[String], body: &[Stmt], scope: &Scope) -> Option<Vec<(TypeSpec, Vec<Expr>)>> {
    let mut result = vec![];
    for p in params {
        let attributes: Vec<&str> = body
            .iter()
            .filter_map(|s| match s {
                Stmt::DummyIntent { name, intent } if name == p => Some(intent.as_str()),
                _ => None,
            })
            .collect();
        if attributes.iter().any(|a| !a.starts_with("intent(")) || attributes.contains(&"intent(in)") && writes(body, p) {
            return None;
        }
        let declared = body.iter().find_map(|s| match s {
            Stmt::ArrayDecl { kind, name, dims } if name == p => Some((kind.clone(), dims.clone())),
            Stmt::VarDecl { kind, names } if names.contains(p) => Some((kind.clone(), vec![])),
            Stmt::DerivedDecl { names, .. } if names.contains(p) => Some((TypeSpec::Logical, vec![Expr::Ident("@derived".into())])),
            _ => None,
        });
        let (kind, dims) = match declared {
            Some(d) => d,
            None => (scope.kind(p)?, vec![]),
        };
        // A CHARACTER dummy is a copy of its actual. A fixed-length one is
        // copied back; an assumed-length one must not change.
        let copied = match kind {
            TypeSpec::Character(_) => true,
            TypeSpec::CharacterAssumed => !writes(body, p),
            _ => false,
        };
        if !(numeric(&kind) || copied && dims.is_empty()) {
            return None;
        }
        let last = dims.len().saturating_sub(1);
        for (axis, d) in dims.iter().enumerate() {
            let assumed = |e: &Expr| matches!(e, Expr::Ident(n) if n == "@assumed_extent");
            let ok = match d {
                Expr::Ident(n) if n.starts_with('@') => assumed(d) && axis == last,
                Expr::Call(tag, a) if tag == "@declared_bounds" => a.len() == 2 && (!assumed(&a[1]) || axis == last),
                _ => true,
            };
            if !ok {
                return None;
            }
        }
        result.push((kind, dims));
    }
    Some(result)
}

/// Make the DO loops in `body` that count with `name` count with `counter`,
/// assigning it to `name` at the start of each iteration and after the loop.
fn loop_variable(body: &mut Vec<Stmt>, name: &str, counter: &str) -> bool {
    let mut found = false;
    let mut i = 0;
    while i < body.len() {
        let control = match &mut body[i] {
            Stmt::Do { var, body: inner, .. } if var == name => {
                *var = counter.into();
                inner.insert(0, assign(name, id(counter)));
                true
            }
            Stmt::Block { body: inner } => match inner.first_mut() {
                Some(Stmt::CallSub { name: tag, args }) if tag == "@do_step" && matches!(args.first(), Some(Expr::Ident(v)) if v == name) => {
                    args[0] = id(counter);
                    inner.insert(1, assign(name, id(counter)));
                    true
                }
                _ => false,
            },
            _ => false,
        };
        for b in crate::procedure_arguments::children(&mut body[i]) {
            found |= loop_variable(b, name, counter);
        }
        if control {
            found = true;
            body.insert(i + 1, assign(name, id(counter)));
            i += 1;
        }
        i += 1;
    }
    found
}

/// Whether `body` may change variable `name` other than through a call.
fn writes(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { name: n, .. } | Stmt::AssignIndex { name: n, .. } => n == name,
        Stmt::Read { args } => args.iter().any(|a| matches!(a, Expr::Ident(n) | Expr::Section { name: n, .. } if n == name)),
        Stmt::CallSub { name: n, args } if n == "@write" => matches!(args.first(), Some(Expr::Ident(a) | Expr::Section { name: a, .. }) if a == name),
        Stmt::If { then_body, else_body, .. } => writes(then_body, name) || else_body.as_deref().is_some_and(|b| writes(b, name)),
        Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => writes(body, name),
        Stmt::SelectCase { cases, default, .. } => {
            cases.iter().any(|c| writes(&c.body, name)) || default.as_deref().is_some_and(|b| writes(b, name))
        }
        _ => false,
    })
}

/// The by-reference entry point of external subroutine `name`.
fn entry_point(name: &str, params: &[String], body: &[Stmt], dummies: &[(TypeSpec, Vec<Expr>)], scope: &Scope,
               result: Option<&TypeSpec>) -> (Stmt, Scope) {
    let mut native = body.to_vec();
    let mut scope = scope.clone();
    let mut declarations = vec![];
    // Every descriptor is set before bounds that read other dummies.
    let mut setup = vec![];
    let mut entry = vec![];
    let mut native_params = vec![];
    // Deferred-length copies of assumed-length CHARACTER dummies, which
    // only a BLOCK around the statements can declare.
    let mut deferred = vec![];
    // Statements that copy changed CHARACTER dummies back to their actuals.
    let mut copy_out = vec![];
    if let Some(kind) = result {
        // A function's result is passed like a scalar dummy, first.
        let passed: Vec<String> = FIELDS.iter().map(|f| format!("{RESULT}_{f}")).collect();
        native_params.extend(passed.clone());
        declarations.push(integers(passed.clone()));
        for f in &passed {
            declarations.push(Stmt::DummyIntent { name: f.clone(), intent: "intent(in)".into() });
        }
        // A CHARACTER result is copied to the caller's storage; others
        // are stored through a scalar descriptor.
        let text = character(kind);
        if !text {
            let (state, origin) = (format!("{RESULT}_state"), format!("{RESULT}_origin"));
            declarations.push(integers(vec![state.clone(), origin.clone()]));
            setup.push(assign(&state, int(1)));
            setup.push(assign(&origin, int(0)));
            declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![RESULT.into()] });
            declarations.push(Stmt::DummyIntent { name: RESULT.into(), intent: "pointer".into() });
            let descriptor = [&passed[0], &passed[1], &passed[2], &state, &origin].map(|s| s.as_str()).join(",");
            declarations.push(Stmt::DummyIntent { name: RESULT.into(), intent: format!("@borrowed_descriptor:{descriptor}") });
        }
        // The result variable: the function name, or that of RESULT(r).
        fn returned(body: &[Stmt]) -> Option<String> {
            body.iter().find_map(|s| match s {
                Stmt::Return(Some(Expr::Ident(r))) => Some(r.clone()),
                Stmt::If { then_body, else_body, .. } => returned(then_body).or_else(|| else_body.as_deref().and_then(returned)),
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => returned(body),
                Stmt::SelectCase { cases, default, .. } => {
                    cases.iter().find_map(|c| returned(&c.body)).or_else(|| default.as_deref().and_then(returned))
                }
                _ => None,
            })
        }
        let variable = returned(body).unwrap_or_else(|| name.to_string());
        let declared = body.iter().any(|s| matches!(s, Stmt::VarDecl { names, .. } if names.contains(&variable)));
        if !declared {
            declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![variable.clone()] });
        }
        // Each RETURN stores the result first.
        let store = |value: Expr| -> Vec<Stmt> {
            if !text {
                return vec![assign(RESULT, value)];
            }
            let mut out = vec![];
            if value != id(&variable) {
                out.push(assign(&variable, value));
            }
            out.push(Stmt::CallSub { name: "@character_store".into(), args: vec![id(&passed[0]), id(&passed[3]), id(&variable)] });
            out
        };
        fn returns(body: &mut [Stmt], variable: &str, store: &dyn Fn(Expr) -> Vec<Stmt>) {
            for s in body {
                match s {
                    Stmt::Return(value) => {
                        let mut block = store(value.take().unwrap_or_else(|| id(variable)));
                        block.push(Stmt::Return(None));
                        *s = Stmt::Block { body: block };
                    }
                    _ => {
                        for b in crate::procedure_arguments::children(s) {
                            returns(b, variable, store);
                        }
                    }
                }
            }
        }
        let ends = matches!(native.iter().rev().find(|s| !matches!(s, Stmt::SourceLocation(_))), Some(Stmt::Return(_)));
        returns(&mut native, &variable, &store);
        if !ends {
            native.extend(store(id(&variable)));
        }
    }
    for (p, (kind, dims)) in params.iter().zip(dummies) {
        let field = |f: &str| format!("@external_{p}_{f}");
        let passed: Vec<String> = FIELDS.iter().map(|f| field(f)).collect();
        native_params.extend(passed.clone());
        declarations.push(integers(passed.clone()));
        for f in &passed {
            declarations.push(Stmt::DummyIntent { name: f.clone(), intent: "intent(in)".into() });
        }
        if character(kind) {
            // The address and length of the actual's characters.
            let view = |capacity: usize, strict: bool| {
                call("@character_at", vec![id(&passed[0]), id(&passed[3]), int(capacity as i64), int(strict as i64)])
            };
            if let TypeSpec::Character(length) = kind {
                let length = length.unwrap_or(1);
                setup.push(Stmt::CallSub { name: "@character_assign".into(), args: vec![id(p), view(length, false), int(length as i64)] });
                if writes(&native, p) {
                    copy_out.push(Stmt::CallSub { name: "@character_store".into(), args: vec![id(&passed[0]), id(&passed[3]), id(p)] });
                }
            } else {
                // An assumed length becomes a deferred-length copy.
                let buffer = field("buffer");
                declarations.push(Stmt::VarDecl { kind: TypeSpec::Character(Some(CHARACTER_CAPACITY)), names: vec![buffer.clone()] });
                for s in &mut native {
                    if let Stmt::VarDecl { names, .. } = s {
                        names.retain(|n| n != p);
                    }
                }
                native.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } if names.is_empty()));
                deferred.push(Stmt::VarDecl { kind: crate::ast::character_length(id("@deferred_character")), names: vec![p.clone()] });
                deferred.push(Stmt::DummyIntent { name: p.clone(), intent: "allocatable".into() });
                setup.push(Stmt::CallSub {
                    name: "@character_assign".into(),
                    args: vec![id(&buffer), view(CHARACTER_CAPACITY, true), int(CHARACTER_CAPACITY as i64)],
                });
                deferred.push(Stmt::Assign {
                    name: p.clone(),
                    value: Expr::Section { name: buffer, lower: Some(Box::new(int(1))), upper: Some(Box::new(id(&passed[3]))), stride: None },
                });
            }
            continue;
        }
        let (state, origin) = (field("state"), field("origin"));
        let axes: Vec<[String; 3]> = (0..dims.len())
            .map(|i| [field(&format!("lower{i}")), field(&format!("extent{i}")), field(&format!("stride{i}"))])
            .collect();
        let mut locals = vec![state.clone(), origin.clone()];
        locals.extend(axes.iter().flatten().cloned());
        declarations.push(integers(locals));
        setup.push(assign(&state, int(1)));
        setup.push(assign(&origin, int(0)));
        let mut stride = int(1);
        let mut assumed = false;
        for (d, [lower, extent, axis_stride]) in dims.iter().zip(&axes) {
            let (lo, hi) = match d {
                Expr::Call(tag, a) if tag == "@declared_bounds" => (a[0].clone(), a[1].clone()),
                _ => (int(1), d.clone()),
            };
            entry.push(assign(lower, lo));
            entry.push(assign(axis_stride, stride));
            if matches!(&hi, Expr::Ident(n) if n == "@assumed_extent") {
                // The rest of the actual array, whose last column may be
                // partial; each element offset is checked against it.
                assumed = true;
                let divisor = call("max", vec![id(axis_stride), int(1)]);
                let rounded = bin(BinOp::Sub, bin(BinOp::Add, id(&passed[3]), divisor.clone()), int(1));
                entry.push(assign(extent, bin(BinOp::Div, rounded, divisor)));
            } else {
                let size = bin(BinOp::Add, bin(BinOp::Sub, hi, id(lower)), int(1));
                entry.push(assign(extent, call("max", vec![size, int(0)])));
            }
            stride = bin(BinOp::Mul, id(axis_stride), id(extent));
        }
        if assumed {
            declarations.push(Stmt::DummyIntent { name: p.clone(), intent: format!("@borrowed_limit:{}", passed[3]) });
        } else if !dims.is_empty() {
            entry.push(Stmt::If {
                cond: bin(BinOp::Gt, stride, id(&passed[3])),
                then_body: vec![Stmt::CallSub {
                    name: "@error_stop".into(),
                    args: vec![Expr::Str(format!("array dummy `{p}` of `{name}` is larger than its actual argument"))],
                }],
                else_body: None,
            });
        }
        if dims.is_empty() {
            // A DO loop may count with a dummy; it counts with a local
            // copied to the dummy at each iteration and after the loop.
            let counter = field("counter");
            if loop_variable(&mut native, p, &counter) {
                declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![counter] });
            }
        }
        let mut descriptor = vec![passed[0].clone(), passed[1].clone(), passed[2].clone(), state, origin];
        descriptor.extend(axes.iter().flatten().cloned());
        let declared = native.iter().any(|s| match s {
            Stmt::ArrayDecl { name, .. } => name == p,
            Stmt::VarDecl { names, .. } => names.contains(p),
            _ => false,
        });
        if !declared {
            declarations.push(Stmt::VarDecl { kind: kind.clone(), names: vec![p.clone()] });
        }
        for s in &mut native {
            if let Stmt::ArrayDecl { name, dims, .. } = s {
                if name == p {
                    *dims = vec![id("@deferred_extent"); dims.len()];
                }
            }
        }
        declarations.push(Stmt::DummyIntent { name: p.clone(), intent: "pointer".into() });
        declarations.push(Stmt::DummyIntent { name: p.clone(), intent: format!("@borrowed_descriptor:{}", descriptor.join(",")) });
        if native.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == p && intent == "intent(out)")) {
            entry.push(Stmt::CallSub { name: "@borrowed_out".into(), args: vec![id(p)] });
        }
        scope.borrowed.insert(
            p.clone(),
            Borrowed { data: passed[0].clone(), flags: passed[1].clone(), name: passed[2].clone(), count: passed[3].clone(), axes },
        );
    }
    let native_name = format!("{PREFIX}{name}");
    declarations.push(Stmt::DummyIntent { name: native_name.clone(), intent: "@native_recursive".into() });
    // Declarations precede the computed bounds, which precede the statements.
    let first = native
        .iter()
        .position(|s| {
            !matches!(s, Stmt::SourceLocation(_) | Stmt::VarDecl { .. } | Stmt::ArrayDecl { .. } | Stmt::InitializedDecl { .. }
                | Stmt::DummyIntent { .. } | Stmt::Implicit(_) | Stmt::Use { .. } | Stmt::DerivedDecl { .. }
                | Stmt::DerivedType { .. } | Stmt::Accessibility { .. } | Stmt::GenericInterface { .. }
                | Stmt::OperatorInterface { .. })
        })
        .unwrap_or(native.len());
    let mut statements = native.split_off(first);
    // Automatic arrays, whose bounds read dummies, are allocated in the
    // BLOCK after the dummies' descriptors are set.
    let automatic = |s: &Stmt| match s {
        Stmt::ArrayDecl { name, dims, .. } if !params.contains(name) => {
            let mut reads = false;
            for d in dims {
                crate::procedure_arguments::visit_expr(&mut d.clone(), &mut |e| {
                    if matches!(e, Expr::Ident(n) if params.contains(n)) {
                        reads = true;
                    }
                });
            }
            reads
        }
        _ => false,
    };
    let (mut moved, kept): (Vec<Stmt>, Vec<Stmt>) = native.into_iter().partition(|s| automatic(s));
    native = kept;
    // Their bounds are evaluated with the other entry computations, into
    // locals, since later passes evaluate a declared bound again.
    let mut serial = 0;
    for s in &mut moved {
        let Stmt::ArrayDecl { dims, .. } = s else { continue };
        for d in dims {
            let bounds: Vec<&mut Expr> = match d {
                Expr::Call(tag, a) if tag == "@declared_bounds" => a.iter_mut().collect(),
                _ => vec![d],
            };
            for b in bounds {
                serial += 1;
                let bound = format!("@external_bound_{serial}");
                native.push(integers(vec![bound.clone()]));
                entry.push(assign(&bound, std::mem::replace(b, id(&bound))));
            }
        }
    }
    deferred.splice(0..0, moved);
    if !copy_out.is_empty() {
        fn returns(body: &mut [Stmt], copy_out: &[Stmt]) {
            for s in body {
                match s {
                    Stmt::Return(None) => {
                        let mut block = copy_out.to_vec();
                        block.push(Stmt::Return(None));
                        *s = Stmt::Block { body: block };
                    }
                    _ => {
                        for b in crate::procedure_arguments::children(s) {
                            returns(b, copy_out);
                        }
                    }
                }
            }
        }
        returns(&mut statements, &copy_out);
        statements.extend(copy_out);
    }
    native.extend(declarations);
    native.extend(setup);
    native.extend(entry);
    if deferred.is_empty() {
        native.append(&mut statements);
    } else {
        deferred.append(&mut statements);
        native.push(Stmt::Block { body: deferred });
    }
    (Stmt::Subroutine { elemental: false, pure: false, name: native_name, params: native_params, body: native }, scope)
}

pub(crate) fn resolve(mut p: Program) -> Result<Program> {
    // Procedures that are not external: module, internal and generic names.
    fn others(body: &[Stmt], top: bool, contains: &[String], out: &mut HashSet<String>) {
        for s in body {
            match s {
                Stmt::Subroutine { name, body, .. } | Stmt::Function { name, body, .. } => {
                    if !top || contains.contains(name) {
                        out.insert(name.clone());
                    }
                    others(body, false, contains, out);
                }
                Stmt::Module { body, .. } => others(body, false, contains, out),
                Stmt::GenericInterface { name, .. } => {
                    out.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    let mut other = HashSet::new();
    others(&p.body, true, &p.contains, &mut other);
    let mut main = Scope::new(None);
    main.declare(&p.body, &[]);

    // External subroutines and their entry points.
    // A file of only procedures and modules has no name for its main program.
    let main_program = p.name != "<anon>" || p.body.iter().any(|s| !matches!(s, Stmt::Subroutine { .. } | Stmt::Function { .. }
        | Stmt::Module { .. } | Stmt::SourceLocation(_)));
    let mut definitions = HashMap::new();
    let mut generated = vec![];
    let mut scopes = HashMap::new();
    for s in &p.body {
        let (name, params, body, elemental, return_type) = match s {
            Stmt::Subroutine { name, params, body, elemental, pure } => (name, params, body, *elemental || *pure, None),
            Stmt::Function { name, params, body, elemental, pure, return_type } => (name, params, body, *elemental || *pure, Some(return_type)),
            _ => continue,
        };
        if p.contains.contains(name) {
            continue;
        }
        let mut scope = Scope::new(None);
        scope.declare(body, params);
        // A function's result type, which must be numeric or LOGICAL.
        let function = return_type.map(|t| t.clone().or_else(|| scope.kind(name)));
        let supported = match &function {
            None => true,
            Some(kind) => kind.as_ref().is_some_and(|k| numeric(k) || matches!(k, TypeSpec::Character(_))),
        };
        let assumed_size = body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, dims, .. }
            if params.contains(name) && dims.iter().any(|d| match d {
                Expr::Ident(n) => n == "@assumed_extent",
                Expr::Call(tag, a) if tag == "@declared_bounds" => matches!(a.get(1), Some(Expr::Ident(n)) if n == "@assumed_extent"),
                _ => false,
            })));
        let interface = if elemental || !supported { None } else { dummies(params, body, &scope) };
        let function = function.flatten();
        if let Some(interface) = &interface {
            let (native, native_scope) = entry_point(name, params, body, interface, &scope, function.as_ref());
            scopes.insert(format!("{PREFIX}{name}"), native_scope);
            generated.push(native);
        }
        // Call-site specialization cannot take an array element (sequence
        // association) and requires an explicit-shape dummy to match its
        // actual's shape, so procedures with array dummies, and with the
        // CHARACTER dummies that it copies, use only their entry point.
        let ranks = params
            .iter()
            .map(|p| body.iter().find_map(|s| match s {
                Stmt::ArrayDecl { name, dims, .. } if name == p => Some(dims.len()),
                _ => None,
            }).unwrap_or(0))
            .collect();
        let runtime_shape = body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, dims, .. }
            if params.contains(name) && dims.iter().any(|d| crate::parameters::integer_constant(d).is_err()
                && !matches!(d, Expr::Ident(n) if n.starts_with('@')))));
        // Without a main program, the procedures are a library that other
        // compilations call by reference, and only the entry points remain.
        let special = interface.as_ref().is_some_and(|i| i.iter().any(|(kind, dims)| !dims.is_empty() || character(kind)));
        let native_only = interface.is_some() && (assumed_size || !main_program && special);
        let variable = params
            .iter()
            .map(|p| special && !body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent } if name == p && intent.starts_with("intent("))))
            .collect();
        definitions.insert(name.clone(), Definition { ranks, runtime_shape, variable, native: interface.is_some(), native_only, function });
    }
    // Only the entry point supports an assumed-size dummy.
    p.body.retain(|s| !matches!(s, Stmt::Subroutine { name, .. } | Stmt::Function { name, .. }
        if definitions.get(name).is_some_and(|d| d.native_only)));
    p.body.extend(generated);

    let mut lower = Lower { definitions, library: !main_program, others: other, serial: 0 };
    fn walk(lower: &mut Lower, body: &mut Vec<Stmt>, scope: &Scope, top: bool, contains: &[String],
            scopes: &HashMap<String, Scope>, main: &Scope) -> Result<()> {
        lower.calls(body, scope)?;
        for s in body.iter_mut() {
            match s {
                Stmt::Subroutine { name, params, body, pure, elemental } | Stmt::Function { name, params, body, pure, elemental, .. } => {
                    let pure = *pure || *elemental;
                    let mut inner = if let Some(native) = scopes.get(name.as_str()) {
                        native.clone()
                    } else {
                        // External procedures have no host; internal ones see theirs.
                        let host = if top && !contains.contains(name) { None } else if top { Some(main) } else { Some(scope) };
                        let mut inner = Scope::new(host);
                        inner.declare(body, params);
                        inner
                    };
                    inner.pure |= pure;
                    walk(lower, body, &inner, false, contains, scopes, main)?;
                }
                Stmt::Module { body, .. } => {
                    let mut inner = Scope::new(None);
                    inner.declare(body, &[]);
                    walk(lower, body, &inner, false, contains, scopes, main)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    let contains = p.contains.clone();
    walk(&mut lower, &mut p.body, &main, true, &contains, &scopes, &main)?;
    if main_program {
        // A program keeps only the entry points it calls, so that an unused
        // one cannot report errors in place of the original procedure.
        fn called(body: &[Stmt], out: &mut Vec<String>) {
            for s in body {
                if let Stmt::CallSub { name, .. } = s {
                    if name.starts_with(PREFIX) {
                        out.push(name.clone());
                    }
                }
                for b in crate::procedure_arguments::children(&mut s.clone()) {
                    called(b, out);
                }
            }
        }
        let native = |s: &Stmt| matches!(s, Stmt::Subroutine { name, .. } if name.starts_with(PREFIX));
        let mut pending = vec![];
        called(&p.body.iter().filter(|s| !native(s)).cloned().collect::<Vec<_>>(), &mut pending);
        let mut used = HashSet::new();
        while let Some(name) = pending.pop() {
            if used.insert(name.clone()) {
                if let Some(Stmt::Subroutine { body, .. }) = p.body.iter().find(|s| matches!(s, Stmt::Subroutine { name: n, .. } if *n == name)) {
                    called(body, &mut pending);
                }
            }
        }
        p.body.retain(|s| !matches!(s, Stmt::Subroutine { name, .. } if name.starts_with(PREFIX) && !used.contains(name)));
    }
    Ok(p)
}

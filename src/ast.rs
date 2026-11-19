use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    #[allow(dead_code)]
    pub name: String,
    pub body: Vec<Stmt>,
    /// Procedures the main program CONTAINS. They are kept in `body` with the
    /// external procedures until host association lifts them.
    #[serde(default)]
    pub contains: Vec<String>,
}

/// Marks a kind type parameter given by an expression other than a name or a
/// literal; the serialized expression follows and is evaluated with the
/// named constants in scope.
pub const KIND_EXPRESSION: &str = "@kind_expression:";
pub fn kind_text(kind: Expr) -> String {
    match kind {
        Expr::Ident(name) | Expr::IntLit(name) => name,
        kind => format!("{KIND_EXPRESSION}{}", serde_json::to_string(&kind).expect("serializable kind")),
    }
}

pub fn integer_kind(kind: Expr) -> TypeSpec {
    let kind = kind_text(kind);
    match kind.parse::<u8>() { Ok(k) => TypeSpec::Integer(Some(k)), Err(_) => TypeSpec::IntegerKind(kind) }
}

/// The names of the procedures among `units`.
pub fn procedure_names(units: &[Stmt]) -> Vec<String> {
    units.iter().filter_map(|s| match s {
        Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => Some(name.to_ascii_lowercase()),
        _ => None,
    }).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    GenericInterface { name: String, procedures: Vec<String>, end_name: Option<String> },
    OperatorInterface { operator: String, procedures: Vec<String>, #[serde(default)] end_operator: Option<String> },
    DerivedType { name: String, components: Vec<Stmt>, bindings: Vec<(String, String)> },
    DerivedDecl { type_name: String, names: Vec<String>, attributes: Vec<String>, polymorphic: bool, #[serde(default)] dims: Vec<Expr> },
    Accessibility { public: bool, names: Vec<String> },
    DummyIntent { name: String, intent: String },
    /// Byte offset of the following source statement; retained through AST lowering.
    SourceLocation(usize),
    Print {
        items: Vec<Expr>,
    },
    VarDecl {
        kind: TypeSpec,
        names: Vec<String>,
    },
    InitializedDecl {
        kind: TypeSpec,
        attributes: Vec<String>,
        entities: Vec<(String, Option<Expr>)>,
    },

    ArrayDecl {
        kind: TypeSpec,
        name: String,
        dims: Vec<Expr>,
    },
    Assign {
        name: String,
        value: Expr,
    },

    AssignIndex {
        name: String,
        indices: Vec<Expr>,
        value: Expr,
    },
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    Do {
        var: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
    },
    DoWhile {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Function {
        elemental: bool,
        #[serde(default)]
        pure: bool,
        name: String,
        params: Vec<String>,
        return_type: Option<TypeSpec>,
        body: Vec<Stmt>,
    },
    Subroutine {
        #[serde(default)]
        elemental: bool,
        #[serde(default)]
        pure: bool,
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    CallSub {
        name: String,
        args: Vec<Expr>,
    },
    Read {
        args: Vec<Expr>,
    },
    Use {
        module: String,
        #[serde(default)]
        nature: Option<String>,
        #[serde(default)]
        only: Option<Vec<String>>,
        #[serde(default)]
        renames: Vec<(String,String)>,
    },
    Module {
        name: String,
        body: Vec<Stmt>,
    },
    Implicit(Implicit),
    SelectCase {
        expr: Expr,
        cases: Vec<CaseBlock>,
        default: Option<Vec<Stmt>>,
    },
    Block {
        body: Vec<Stmt>,
    },
    Exit,
    Cycle,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum TypeSpec {
    /// Fortran 2023 declaration type, resolved after USE/host association.
    Referenced { source: Box<Expr>, polymorphic: bool },
    Integer(Option<u8>),
    IntegerKind(String),
    Real,
    RealKind(String),
    DoublePrecision,
    /// The kind denotes the REAL component kind, not the total byte size.
    Complex(String),
    Character(Option<usize>),
    CharacterLength(Box<Expr>),
    CharacterAssumed,
    Logical,
    LogicalKind(String),
}

/// A FORTRAN 77 sized type such as `REAL*8` or `COMPLEX*16`.
pub fn sized_type(name: &str, bytes: &str) -> Result<TypeSpec, &'static str> {
    let n: u32 = bytes.parse().map_err(|_| "invalid type size")?;
    Ok(match (name, n) {
        ("real", 4 | 8) => TypeSpec::RealKind(n.to_string()),
        ("complex", 8 | 16) => TypeSpec::Complex((n / 2).to_string()),
        ("integer", 1 | 2 | 4 | 8) => TypeSpec::Integer(Some(n as u8)),
        ("logical", 1 | 2 | 4 | 8) => TypeSpec::LogicalKind(n.to_string()),
        _ => return Err("unsupported type size: REAL*4, REAL*8, COMPLEX*8, COMPLEX*16, INTEGER*n and LOGICAL*n are supported"),
    })
}

/// A computed GO TO: an IF chain that branches to the label the selector
/// picks, or continues with the next statement if it picks none.
pub fn computed_goto(labels: Vec<String>, selector: Expr) -> Stmt {
    let mut chain: Option<Vec<Stmt>> = None;
    for (i, label) in labels.into_iter().enumerate().rev() {
        chain = Some(vec![Stmt::If {
            cond: Expr::eq(selector.clone(), Expr::IntLit((i + 1).to_string())),
            then_body: vec![Stmt::CallSub { name: "@goto".into(), args: vec![Expr::IntLit(label)] }],
            else_body: chain,
        }]);
    }
    chain.and_then(|mut c| c.pop()).unwrap_or(Stmt::CallSub { name: "@continue".into(), args: vec![] })
}

/// `TYPE(t) x, y(3)` or `CLASS(t) x` written without `::`, which allows no
/// attributes or initializers.
/// The statements for the attributes of `TYPE, attributes :: name`: PUBLIC
/// and PRIVATE become accessibility statements, EXTENDS and ABSTRACT markers.
pub fn type_attributes(name: &str, attrs: Vec<String>) -> Result<Vec<Stmt>, &'static str> {
    let mut out = Vec::new();
    for attr in attrs {
        match attr.to_ascii_lowercase().as_str() {
            "public" => out.push(Stmt::Accessibility { public: true, names: vec![name.to_owned()] }),
            "private" => out.push(Stmt::Accessibility { public: false, names: vec![name.to_owned()] }),
            "bind(c)" => {},
            a if a.starts_with("extends(") && a.ends_with(')') => {
                let parent = attr[8..attr.len() - 1].to_string();
                out.push(Stmt::CallSub { name: crate::extensions::EXTENDS.into(), args: vec![Expr::Ident(name.to_owned()), Expr::Ident(parent)] });
            }
            "abstract" => out.push(Stmt::CallSub { name: crate::abstract_types::ABSTRACT.into(), args: vec![Expr::Ident(name.to_owned())] }),
            _ => return Err("unsupported derived-type attribute"),
        }
    }
    Ok(out)
}

pub fn derived_declarations(type_name: String, polymorphic: bool,
    entities: Vec<(String, Vec<Expr>, Option<Expr>, Option<TypeSpec>)>) -> Result<Vec<Stmt>, &'static str> {
    entities.into_iter().map(|(name, dims, value, length)| {
        if value.is_some() { return Err("an initializer in a TYPE or CLASS declaration requires ::"); }
        if length.is_some() { return Err("a length applies only to CHARACTER entities"); }
        Ok(Stmt::DerivedDecl { type_name: type_name.clone(), names: vec![name], attributes: vec![], polymorphic, dims })
    }).collect()
}

/// A FORALL statement or construct as one-assignment FORALLs marked by
/// `@forall` with index triplets. A construct runs each statement for every
/// index in turn, so it becomes one FORALL per statement; a nested FORALL adds
/// its triplets. A mask keeps the old element where it is false:
/// `a(i) = merge(value, a(i), mask)`, so the value is also evaluated where the
/// mask is false.
pub fn forall(header: (Vec<Expr>, Option<Expr>), body: Vec<Stmt>) -> Result<Stmt, &'static str> {
    fn add(controls: &[Expr], mask: Option<&Expr>, s: Stmt, out: &mut Vec<Stmt>) -> Result<(), &'static str> {
        match s {
            Stmt::SourceLocation(_) => out.push(s),
            Stmt::Block { body } if matches!(body.first(), Some(Stmt::CallSub { name, .. }) if name == "@forall") => {
                let mut body = body.into_iter();
                let Some(Stmt::CallSub { args: inner, .. }) = body.next() else { unreachable!() };
                let mut combined = controls.to_vec();
                combined.extend(inner);
                for s in body { add(&combined, mask, s, out)?; }
            }
            // The statements of a nested FORALL construct.
            Stmt::Block { body } if body.iter().all(|s| matches!(s, Stmt::SourceLocation(_) | Stmt::Block { .. })) => {
                for s in body { add(controls, mask, s, out)?; }
            }
            Stmt::AssignIndex { name, indices, value } => {
                let value = match mask {
                    None => value,
                    Some(_) if name.contains('%') => return Err("a FORALL mask with a component assignment is not yet supported"),
                    Some(mask) => {
                        let old = match indices.as_slice() {
                            [section @ Expr::Section { .. }] => section.clone(),
                            _ => Expr::Call(name.clone(), indices.iter().map(|e| match e {
                                Expr::Call(n, a) if n == "@vector_subscript" && matches!(a.as_slice(), [Expr::Section { .. }]) =>
                                    Expr::Call("@section_subscript".into(), a.clone()),
                                e => e.clone(),
                            }).collect()),
                        };
                        Expr::Call("merge".into(), vec![value, old, mask.clone()])
                    }
                };
                out.push(Stmt::Block { body: vec![
                    Stmt::CallSub { name: "@forall".into(), args: controls.to_vec() },
                    Stmt::AssignIndex { name, indices, value },
                ] });
            }
            _ if mask.is_some() => return Err("a FORALL mask currently requires array-element or array-section assignments"),
            s => out.push(Stmt::Block { body: vec![Stmt::CallSub { name: "@forall".into(), args: controls.to_vec() }, s] }),
        }
        Ok(())
    }
    let (controls, mask) = header;
    let mut out = vec![];
    for s in body { add(&controls, mask.as_ref(), s, &mut out)?; }
    Ok(match out.len() {
        1 => out.pop().unwrap(),
        _ => Stmt::Block { body: out },
    })
}

/// Normalize a named scalar result into explicit returns for the existing backends.
pub fn function_with_result(name: String, params: Vec<String>, result: String, mut body: Vec<Stmt>) -> Stmt {
    fn returns(body: &mut [Stmt], result: &str) {
        for stmt in body {
            match stmt {
                Stmt::Return(value @ None) => *value = Some(Expr::Ident(result.into())),
                Stmt::If { then_body, else_body, .. } => {
                    returns(then_body, result);
                    if let Some(body) = else_body { returns(body, result); }
                }
                Stmt::Do { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Block { body } => returns(body, result),
                Stmt::SelectCase { cases, default, .. } => {
                    for case in cases { returns(&mut case.body, result); }
                    if let Some(body) = default { returns(body, result); }
                }
                _ => {}
            }
        }
    }
    let return_type = body.iter().find_map(|stmt| match stmt {
        Stmt::VarDecl { kind, names } if names.contains(&result) => Some(kind.clone()),
        Stmt::InitializedDecl { kind, entities, .. } if entities.iter().any(|(n, _)| n == &result) => Some(kind.clone()),
        _ => None,
    });
    returns(&mut body, &result);
    body.push(Stmt::Return(Some(Expr::Ident(result))));
    Stmt::Function { name, params, return_type, body, pure: false, elemental: false }
}

/// DO CONCURRENT as nested DO loops over its controls, each given as index,
/// start, end and stride, with the body under the mask.
pub fn do_concurrent(controls: Vec<Expr>, mask: Option<Expr>, body: Vec<Stmt>) -> Stmt {
    let mut body = match mask {
        Some(cond) => vec![Stmt::If { cond, then_body: body, else_body: None }],
        None => body,
    };
    for control in controls.chunks(4).rev() {
        let [Expr::Ident(var), start, end, step] = control else { unreachable!("DO CONCURRENT control") };
        body = vec![if matches!(step, Expr::IntLit(n) if n == "1") {
            Stmt::Do { var: var.clone(), start: start.clone(), end: end.clone(), body }
        } else {
            let mut statements = vec![Stmt::CallSub { name: "@do_step".into(), args: vec![Expr::Ident(var.clone()), start.clone(), end.clone(), step.clone()] }];
            statements.extend(body);
            Stmt::Block { body: statements }
        }];
    }
    body.pop().unwrap()
}

/// Resolve each `@generic:name` binding entry, which names a specific
/// binding of the type, to that binding's procedure.
/// Retain the binding identity too: aliases may share an implementation but
/// only one of those bindings may be overridden in an extension.
pub fn generic_bindings(bindings: Vec<(String, String)>) -> Vec<(String, String)> {
    let specific = |b: &str| bindings.iter().find(|(n, _)| n == b && !n.starts_with('@')).map(|(_, t)| t.clone());
    bindings.iter().map(|(n, t)| match n.starts_with("@generic:") || n.starts_with("@operator:") || n.starts_with("@defined_io:") {
        true => (n.clone(), specific(t).unwrap_or_else(|| t.clone())),
        false => (n.clone(), t.clone()),
    }).chain(bindings.iter().filter_map(|(n,b)|n.strip_prefix("@generic:").map(|g|
        (format!("@generic_binding:{g}:{b}"),specific(b).unwrap_or_else(||b.clone())))))
        .chain(bindings.iter().filter_map(|(n,b)|n.strip_prefix("@defined_io:").map(|g|
            (format!("@defined_io_binding:{g}:{b}"),specific(b).unwrap_or_else(||b.clone()))))).collect()
}

/// The dummy argument named by a PASS(arg) binding attribute.
pub fn pass_argument(attributes: &[String]) -> Option<String> {
    attributes.iter().find_map(|a| a.strip_prefix("pass(")?.strip_suffix(')').map(str::to_string))
}

/// Check the attributes of a type-bound procedure binding.
pub fn binding_attributes(attributes: &[String]) -> Result<(), &'static str> {
    for a in attributes {
        match a.to_ascii_lowercase().as_str() {
            "public" | "private" | "pass" | "non_overridable" => {}
            a if a.starts_with("pass(") => {}
            "nopass" => {}
            "deferred" => return Err("DEFERRED bindings require an explicit procedure interface"),
            _ => return Err("unknown type-bound procedure attribute"),
        }
    }
    Ok(())
}

pub fn scalar_decl(kind: TypeSpec, attributes: Vec<String>, entities: Vec<(String, Option<Expr>)>) -> Stmt {
    if attributes.iter().any(|a| a.eq_ignore_ascii_case("parameter") || a.eq_ignore_ascii_case("save"))
        || entities.iter().any(|(_, value)| value.is_some())
    {
        Stmt::InitializedDecl { kind, attributes, entities }
    } else {
        Stmt::VarDecl { kind, names: entities.into_iter().map(|(name, _)| name).collect() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Implicit {
    None,
    Rule {
        type_spec: TypeSpec,
        letter_ranges: Vec<LetterRange>,
    },
    Rules {
        rules: Vec<ImplicitRule>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitRule {
    pub type_spec: TypeSpec,
    pub letter_ranges: Vec<LetterRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LetterRange {
    pub start: char,
    pub end: Option<char>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseBlock {
    pub items: Vec<CaseItem>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaseItem {
    Range(Expr, Expr),
    Single(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaseOrDefault {
    Case(CaseBlock),
    Default(Vec<Stmt>),
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Eqv,
    Neqv,
    Concat,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    Section {
        name: String,
        lower: Option<Box<Expr>>,
        upper: Option<Box<Expr>>,
        stride: Option<Box<Expr>>,
    },
    Str(String),
    IntLit(String),
    RealLit(String),
    Logical(bool),
    Ident(String),

    #[allow(dead_code)]
    Index(String, Vec<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Un(UnOp, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Expr {
    pub fn add(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Add, Box::new(l), Box::new(r))
    }
    pub fn sub(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Sub, Box::new(l), Box::new(r))
    }
    pub fn mul(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Mul, Box::new(l), Box::new(r))
    }
    pub fn div(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Div, Box::new(l), Box::new(r))
    }
    pub fn pow(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Pow, Box::new(l), Box::new(r))
    }

    pub fn eq(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Eq, Box::new(l), Box::new(r))
    }
    pub fn ne(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Ne, Box::new(l), Box::new(r))
    }
    pub fn lt(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Lt, Box::new(l), Box::new(r))
    }
    pub fn gt(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Gt, Box::new(l), Box::new(r))
    }
    pub fn le(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Le, Box::new(l), Box::new(r))
    }
    pub fn ge(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Ge, Box::new(l), Box::new(r))
    }

    pub fn and(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::And, Box::new(l), Box::new(r))
    }
    pub fn or(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Or, Box::new(l), Box::new(r))
    }
    pub fn eqv(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Eqv, Box::new(l), Box::new(r))
    }
    pub fn neqv(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Neqv, Box::new(l), Box::new(r))
    }

    pub fn concat(l: Expr, r: Expr) -> Expr {
        Expr::Bin(BinOp::Concat, Box::new(l), Box::new(r))
    }

    pub fn neg(e: Expr) -> Expr {
        Expr::Un(UnOp::Neg, Box::new(e))
    }
    pub fn not(e: Expr) -> Expr {
        Expr::Un(UnOp::Not, Box::new(e))
    }
}

// Preserve the existing rank-one section representation.
/// A designator with subscripts on two or more parts, as `y(1)%at(j)%i(k)`:
/// the array `base(args)` of records, then component paths each with its
/// subscripts. Array components of derived type are stored with the axes of
/// their own components first, so the chain is the component `at%i` of
/// `y(1)` with the combined subscripts `(k, j)`: those of the last part first.
#[derive(Debug, Clone)]
pub struct Chain {
    pub base: String,
    pub args: Vec<Expr>,
    pub parts: Vec<(String, Option<Vec<Expr>>)>,
}
/// Stands for every axis of the last part of a chain that has no subscripts.
pub const WHOLE_AXES: &str = "@whole_component_axes";
impl Chain {
    fn path(&self) -> String {
        self.parts.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join("%")
    }
    fn subscripts(&self, last: Option<Vec<Expr>>) -> Expr {
        let mut all = match last {
            Some(s) => s,
            None => vec![Expr::Call(WHOLE_AXES.into(), vec![Expr::Str(self.path())])],
        };
        for (_, s) in self.parts.iter().rev() {
            all.extend(s.iter().flatten().cloned());
        }
        Expr::Call("@component_subscripts".into(), all)
    }
    pub fn expr(self, last: Option<Vec<Expr>>) -> Expr {
        let component = self.subscripts(last);
        Expr::Call(format!("@record_component:{}", self.path()), vec![section_or_call(self.base, self.args), component])
    }
    pub fn assign(self, last: Option<Vec<Expr>>, value: Expr) -> Stmt {
        let component = self.subscripts(last);
        let mut indices = self.args.clone();
        indices.push(component);
        Stmt::AssignIndex { name: format!("{}%{}", self.base, self.path()), indices, value }
    }
}
pub fn section_or_call(name: String, mut args: Vec<Expr>) -> Expr {
    let single_section = args.len() == 1 && matches!(&args[0], Expr::Section { name, .. } if name.is_empty());
    for arg in &mut args {
        if let Expr::Section { name: owner, .. } = arg {
            if owner.is_empty() {
                *owner = name.clone();
                if !single_section { *arg = Expr::Call("@section_subscript".into(), vec![arg.clone()]); }
            }
        }
    }
    if single_section { args.remove(0) }
    else { Expr::Call(name, args) }
}

// `name => target` in a declaration. Pointer descriptors start disassociated,
// so NULL() needs no statement; another target is associated after declaration.
pub fn pointer_initializer(target: Expr) -> Expr {
    Expr::Call("@pointer_initializer".into(), vec![target])
}
pub fn split_pointer_initializer(name: &str, value: Option<Expr>) -> (Option<Expr>, Option<Stmt>) {
    match value {
        Some(Expr::Call(marker, mut args)) if marker == "@pointer_initializer" && args.len() == 1 => {
            let target = args.pop().unwrap();
            if matches!(&target, Expr::Call(n, a) if n.eq_ignore_ascii_case("null") && a.is_empty()) {
                (None, None)
            } else {
                (None, Some(Stmt::CallSub { name: "@pointer_associate".into(), args: vec![Expr::Ident(name.into()), target] }))
            }
        }
        value => (value, None),
    }
}

// Attribute specification statements, e.g. `intent(in) :: x` or `pointer p`.
// They record the same per-name attributes as the corresponding declaration
// attribute. EXTERNAL and INTRINSIC only confirm how a name is resolved.
/// `GENERIC [, access] :: spec => specifics`: the interface, and its access.
pub fn generic_statement<'a, T, E>(keyword: &str, access: Option<bool>, interface: Stmt) -> Result<Vec<Stmt>, lalrpop_util::ParseError<T, E, &'a str>> {
    if !keyword.eq_ignore_ascii_case("generic") {
        return Err(lalrpop_util::ParseError::User { error: "expected a GENERIC statement" });
    }
    let mut out = vec![];
    if let Some(public) = access {
        let name = match &interface {
            Stmt::GenericInterface { name, .. } => name.clone(),
            Stmt::OperatorInterface { operator, .. } => format!("operator({operator})"),
            _ => unreachable!(),
        };
        out.push(Stmt::Accessibility { public, names: vec![name] });
    }
    out.insert(0, interface);
    Ok(out)
}
pub fn attribute_statement(attr: &str, argument: Option<String>, names: Vec<String>) -> Result<Vec<Stmt>, &'static str> {
    if matches!(attr, "volatile" | "asynchronous") && argument.is_none() {
        // They only restrict optimization (see the declaration attributes).
        return Ok(vec![]);
    }
    let attribute = match (attr, argument) {
        ("intent", Some(intent)) => match intent.to_ascii_lowercase().as_str() {
            intent @ ("in" | "out" | "inout") => format!("intent({intent})"),
            _ => return Err("INTENT must be IN, OUT or INOUT"),
        },
        ("external", None) => "external".to_string(),
        ("intrinsic", None) => return Ok(vec![]),
        ("save", None) => "save".to_string(),
        (attr @ ("optional" | "pointer" | "target" | "allocatable" | "value" | "protected"), None) => attr.to_string(),
        _ => return Err("invalid attribute specification statement"),
    };
    Ok(names.into_iter().map(|name| Stmt::DummyIntent { name, intent: attribute.clone() }).collect())
}

/// ASSOCIATE (name => selector, ...) block: each associate name is replaced
/// by its selector, giving the aliasing the standard requires for variable
/// selectors. A selector whose value could change inside the block is
/// rejected, because it would be re-evaluated at each use.
pub fn associate(pairs: Vec<(String, Expr)>, mut body: Vec<Stmt>) -> Result<Stmt, &'static str> {
    use crate::procedure_arguments::{children, expressions, visit_expr};
    fn assigned(body: &mut [Stmt], out: &mut Vec<String>) {
        for s in body.iter_mut() {
            match s {
                Stmt::Assign { name, .. } | Stmt::AssignIndex { name, .. } => out.push(name.clone()),
                Stmt::Do { var, .. } => out.push(var.clone()),
                Stmt::Read { args } => out.extend(args.iter().filter_map(|a| match a {
                    Expr::Ident(n) | Expr::Call(n, _) => Some(n.clone()),
                    _ => None,
                })),
                Stmt::CallSub { name, args } if !name.starts_with('@') => out.extend(args.iter().filter_map(|a| match a {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                })),
                _ => {}
            }
            for b in children(s) {
                assigned(b, out);
            }
        }
    }
    fn base(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => Some(n),
            _ => None,
        }
    }
    let mut written = vec![];
    assigned(&mut body, &mut written);
    // A variable selector designates its element or section when the block
    // starts. A subscript that reads a variable the block modifies is
    // evaluated then, into a temporary.
    static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut captures = vec![];
    let mut deferred = vec![];
    let mut pairs = pairs;
    let reads = |e: &Expr| {
        let mut reads = false;
        visit_expr(&mut e.clone(), &mut |x| match x {
            Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. } => reads |= written.contains(n),
            _ => {}
        });
        reads
    };
    for (_, selector) in pairs.iter_mut() {
        // Section bounds are scalars.
        let mut capture = |e: &mut Expr| {
            if reads(e) {
                let temp = format!("@associate_subscript_{}", SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                captures.push((temp.clone(), std::mem::replace(e, Expr::Ident(temp))));
            }
        };
        // Another subscript may be a scalar or a vector, which the
        // declarations tell (see ASSOCIATE_CAPTURE). An array constructor is
        // copied in any case, so that the name can be subscripted through it.
        let copy = |e: &mut Expr, deferred: &mut Vec<(String, Expr)>| {
            let temp = format!("@associate_subscript_{}", SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            deferred.push((temp.clone(), std::mem::replace(e, Expr::Ident(temp))));
        };
        match selector {
            Expr::Call(_, args) | Expr::Index(_, args) if !args.is_empty() => {
                for a in args.iter_mut() {
                    match a {
                        Expr::Call(tag, s) if tag == "@section_subscript" && matches!(s.as_slice(), [Expr::Section { .. }]) => {
                            let Expr::Section { lower, upper, stride, .. } = &mut s[0] else { unreachable!() };
                            for e in [lower, upper, stride].into_iter().flatten() { capture(e); }
                        }
                        a => if matches!(&*a, Expr::Call(tag, _) if tag == "@array_constructor") || reads(a) { copy(a, &mut deferred) },
                    }
                }
            }
            Expr::Section { lower, upper, stride, .. } => {
                for e in [lower, upper, stride].into_iter().flatten() { capture(e); }
            }
            _ => {}
        }
    }
    for (name, selector) in &pairs {
        // Names in the selector other than the designated variable itself.
        let mut used = vec![];
        let mut copy = selector.clone();
        let designated = base(selector).map(str::to_string);
        if let Expr::Call(_, args) | Expr::Index(_, args) = &mut copy {
            for a in args {
                visit_expr(a, &mut |e| if let Expr::Ident(n) = e { used.push(n.clone()) });
            }
        } else if let Expr::Section { lower, upper, stride, .. } = &mut copy {
            for e in [lower, upper, stride].into_iter().flatten() {
                visit_expr(e, &mut |e| if let Expr::Ident(n) = e { used.push(n.clone()) });
            }
        } else if !matches!(copy, Expr::Ident(_)) {
            visit_expr(&mut copy, &mut |e| if let Expr::Ident(n) = e { used.push(n.clone()) });
        }
        let variable = matches!(selector, Expr::Ident(_) | Expr::Section { .. })
            || matches!(selector, Expr::Call(_, args) | Expr::Index(_, args) if !args.is_empty());
        if used.iter().any(|n| written.contains(n))
            || !variable && designated.as_ref().is_some_and(|d| written.contains(d))
            || pairs.iter().any(|(other, _)| other != name && used.contains(other)) {
            return Err("an ASSOCIATE selector that depends on a variable modified in the block is not yet supported");
        }
        substitute(&mut body, name, selector)?;
    }
    if !captures.is_empty() || !deferred.is_empty() {
        let mut start = vec![];
        if !captures.is_empty() {
            start.push(Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: captures.iter().map(|(t, _)| t.clone()).collect() });
        }
        start.extend(captures.into_iter().map(|(name, value)| Stmt::Assign { name, value }));
        start.extend(deferred.into_iter().map(|(name, value)| Stmt::CallSub { name: ASSOCIATE_CAPTURE.into(), args: vec![Expr::Ident(name), value] }));
        start.append(&mut body);
        body = start;
    }
    fn substitute(body: &mut [Stmt], name: &str, selector: &Expr) -> Result<(), &'static str> {
        let prefix = format!("{name}%");
        let whole = match selector { Expr::Ident(n) => Some(n.as_str()), _ => None };
        for s in body.iter_mut() {
            match s {
                Stmt::Assign { name: n, value } if n == name => match selector {
                    Expr::Ident(a) => *n = a.clone(),
                    Expr::Call(a, idx) | Expr::Index(a, idx) => {
                        *s = Stmt::AssignIndex { name: a.clone(), indices: idx.clone(), value: value.clone() };
                    }
                    Expr::Section { name: a, .. } => {
                        *s = Stmt::AssignIndex { name: a.clone(), indices: vec![selector.clone()], value: value.clone() };
                    }
                    _ => return Err("an ASSOCIATE name whose selector is an expression cannot be assigned"),
                },
                Stmt::Assign { name: n, .. } | Stmt::AssignIndex { name: n, .. } | Stmt::CallSub { name: n, .. } if n.starts_with(&prefix) => {
                    let Some(a) = whole else { return Err("components of an ASSOCIATE name require a variable selector") };
                    *n = format!("{a}%{}", &n[prefix.len()..]);
                }
                Stmt::AssignIndex { name: n, indices, .. } if n == name => match whole {
                    Some(a) => *n = a.into(),
                    None => {
                        for index in indices.iter_mut() { inner_sections(index, name); }
                        if let [Expr::Section { name: inner, .. }] = indices.as_mut_slice() { inner.clear(); }
                        match associate_through(selector, indices, &Known::parsed()) {
                            Ok(Some(m)) => { *n = m.array; *indices = m.indices; }
                            Ok(None) => return Err("subscripts of an ASSOCIATE name require a variable selector"),
                            // Resolved when the declarations are known.
                            Err(()) => {
                                let mut marker = vec![selector.clone()];
                                marker.append(indices);
                                *indices = vec![Expr::Call(ASSOCIATE_SUBSCRIPT.into(), marker)];
                                *n = ASSOCIATE_SUBSCRIPT.into();
                            }
                        }
                    }
                },
                Stmt::Do { var, .. } if var == name => return Err("an ASSOCIATE name cannot be a DO variable"),
                _ => {}
            }
            let mut error = Ok(());
            if whole.is_none() {
                for e in expressions(s) { inner_sections(e, name); }
            }
            for e in expressions(s) {
                visit_expr(e, &mut |e| match e {
                    Expr::Ident(n) if n == name => *e = selector.clone(),
                    // A COMPLEX part of an expression selector, as `r%im`.
                    Expr::Ident(n) if whole.is_none() && matches!(n.strip_prefix(&prefix), Some("re" | "im")) => {
                        let part = if n.ends_with("%re") { "real" } else { "aimag" };
                        *e = Expr::Call(part.into(), vec![selector.clone()]);
                    }
                    // A subscript or section of the associate name of a section.
                    // A subscript or section of the associate name of an
                    // element or section selector.
                    Expr::Call(n, args) | Expr::Index(n, args) if n == name && whole.is_none() => {
                        let args = args.clone();
                        match associate_through(selector, &args, &Known::parsed()) {
                            Ok(Some(m)) => *e = m.expression(),
                            Ok(None) => error = Err("subscripts or components of an ASSOCIATE name require a variable selector"),
                            Err(()) => *e = Expr::Call(ASSOCIATE_SUBSCRIPT.into(), [vec![selector.clone()], args].concat()),
                        }
                    }
                    // A rank-one section of the associate name.
                    Expr::Section { name: n, .. } if n == name && whole.is_none() => {
                        let args = vec![e.clone()];
                        match associate_through(selector, &args, &Known::parsed()) {
                            Ok(Some(m)) => *e = m.expression(),
                            Ok(None) => error = Err("subscripts or components of an ASSOCIATE name require a variable selector"),
                            Err(()) => *e = Expr::Call(ASSOCIATE_SUBSCRIPT.into(), [vec![selector.clone()], args].concat()),
                        }
                    }
                    Expr::Ident(n) | Expr::Call(n, _) | Expr::Index(n, _) | Expr::Section { name: n, .. }
                        if n == name || n.starts_with(&prefix) =>
                    {
                        match whole {
                            Some(a) if n == name => *n = a.into(),
                            Some(a) => *n = format!("{a}%{}", &n[prefix.len()..]),
                            None => error = Err("subscripts or components of an ASSOCIATE name require a variable selector"),
                        }
                    }
                    _ => {}
                });
            }
            error?;
            for b in children(s) {
                substitute(b, name, selector)?;
            }
        }
        Ok(())
    }
    Ok(Stmt::Block { body })
}

/// A reference through an ASSOCIATE name, such as `x(2)` with `x => a(v)`,
/// whose selector has a subscript that may be a vector: the selector, then
/// the name's subscripts. Resolved by `resolve_associate_subscripts` once
/// the declarations show which subscripts are arrays. As the variable of an
/// assignment, it is the name of an AssignIndex whose one index is the
/// reference.
pub(crate) const ASSOCIATE_SUBSCRIPT: &str = "@associate_subscript";

/// `@associate_capture(temp, value)`: the subscript `value` of an ASSOCIATE
/// selector, evaluated into `temp` when the block starts because the block
/// changes a variable it reads, or because it is an array constructor.
/// `expand_associate_captures` declares `temp` as an INTEGER scalar, or an
/// ALLOCATABLE INTEGER vector when the declarations show `value` is an array.
pub(crate) const ASSOCIATE_CAPTURE: &str = "@associate_capture";

/// Replaces the ASSOCIATE_CAPTURE statements of a statement list with the
/// declaration and assignment of their temporaries.
pub(crate) fn expand_associate_captures(statements: &mut Vec<Stmt>, rank: &dyn Fn(&str) -> usize) {
    let mut k = 0;
    while k < statements.len() {
        let Stmt::CallSub { name, args } = &statements[k] else { k += 1; continue };
        if name != ASSOCIATE_CAPTURE { k += 1; continue; }
        let [Expr::Ident(temp), value] = args.as_slice() else { unreachable!() };
        let (temp, value) = (temp.clone(), value.clone());
        let vector = is_vector(&value, rank);
        let mut expanded = if vector {
            vec![
                Stmt::DummyIntent { name: temp.clone(), intent: "allocatable".into() },
                Stmt::ArrayDecl { kind: TypeSpec::Integer(None), name: temp.clone(), dims: vec![Expr::Ident("@deferred_extent".into())] },
            ]
        } else {
            vec![Stmt::VarDecl { kind: TypeSpec::Integer(Some(8)), names: vec![temp.clone()] }]
        };
        expanded.push(Stmt::Assign { name: temp, value });
        let n = expanded.len();
        statements.splice(k..k + 1, expanded);
        k += n;
    }
}

/// Whether a subscript is an array, so a vector subscript, given the ranks
/// of the visible names.
pub(crate) fn is_vector(e: &Expr, rank: &dyn Fn(&str) -> usize) -> bool {
    match e {
        Expr::Ident(n) => rank(n) > 0,
        Expr::Section { .. } => true,
        Expr::Call(tag, _) if tag == "@array_constructor" => true,
        Expr::Call(n, args) | Expr::Index(n, args) if rank(n) > 0 =>
            args.iter().any(|a| section(a).is_some() || is_vector(a, rank)),
        Expr::Bin(_, a, b) => is_vector(a, rank) || is_vector(b, rank),
        Expr::Un(_, a) => is_vector(a, rank),
        _ => false,
    }
}

/// The bounds of a section subscript: the bare `Section` of a rank-one
/// reference, or `@section_subscript`.
fn section(e: &Expr) -> Option<(&Option<Box<Expr>>, &Option<Box<Expr>>, &Option<Box<Expr>>)> {
    match e {
        Expr::Section { lower, upper, stride, .. } => Some((lower, upper, stride)),
        Expr::Call(tag, s) if tag == "@section_subscript" && s.len() == 1 => section(&s[0]),
        _ => None,
    }
}

/// Section subscripts of a reference to `name` with several subscripts,
/// `s(1, :)`, are renamed so that they are not taken for rank-one sections
/// of `name` themselves.
fn inner_sections(e: &mut Expr, name: &str) {
    crate::procedure_arguments::visit_expr(e, &mut |e| {
        if let Expr::Call(n, args) | Expr::Index(n, args) = e {
            if n == name {
                for a in args.iter_mut() {
                    if let Expr::Call(tag, s) = a {
                        if tag == "@section_subscript" {
                            if let [Expr::Section { name: inner, .. }] = s.as_mut_slice() { inner.clear(); }
                        }
                    }
                }
            }
        }
    });
}

/// At parse time, a subscript is known to be scalar only when it is a
/// literal expression.
fn parsed_vector(e: &Expr) -> Option<bool> {
    fn literal(e: &Expr) -> bool {
        match e {
            Expr::IntLit(_) => true,
            Expr::Un(_, a) => literal(a),
            Expr::Bin(_, a, b) => literal(a) && literal(b),
            _ => false,
        }
    }
    literal(e).then_some(false)
}

/// The designator that subscripts of an associate name give.
pub(crate) struct Through {
    pub(crate) array: String,
    pub(crate) indices: Vec<Expr>,
    /// The result is a rank-one section of the array, written bare.
    bare: bool,
}

impl Through {
    pub(crate) fn expression(self) -> Expr {
        if self.bare { self.indices.into_iter().next().unwrap() } else { Expr::Call(self.array, self.indices) }
    }
}

/// The array and subscripts that `subscripts` of an associate name designate
/// when its selector is an element or section of an array, such as
/// `a(l:u:s)` or `a(v)`. The associate name's subscripts start at 1:
/// subscript j of a section dimension is `l + (j - 1) * s` of the array, and
/// of a vector subscript `v` it is `a(v(j))`. `vector` tells whether a
/// subscript of the selector is an array; Err when it cannot tell, and
/// Ok(None) when the subscripts cannot be mapped.
/// What the declarations tell about the subscripts of a selector: whether a
/// subscript is an array (None when unknown), and the lower bound of a
/// dimension of an array when it is known.
pub(crate) struct Known<'a> {
    pub(crate) vector: &'a dyn Fn(&Expr) -> Option<bool>,
    pub(crate) lower: &'a dyn Fn(&str, usize) -> Option<Expr>,
}

impl Known<'static> {
    /// At parse time, without declarations.
    fn parsed() -> Self {
        fn unknown(_: &str, _: usize) -> Option<Expr> { None }
        Known { vector: &parsed_vector, lower: &unknown }
    }
}

pub(crate) fn associate_through(selector: &Expr, subscripts: &[Expr], known: &Known<'_>) -> Result<Option<Through>, ()> {
    let (array, outer, rank_one): (&str, Vec<&Expr>, bool) = match selector {
        Expr::Section { name, .. } => (name, vec![selector], true),
        Expr::Call(name, args) | Expr::Index(name, args) => (name, args.iter().collect(), false),
        _ => return Ok(None),
    };
    // A bare section among the subscripts is a vector subscript, such as
    // `v(1:2)` in `a(v(1:2), 2)`; sections of the array are wrapped.
    type Bounds<'a> = (&'a Option<Box<Expr>>, &'a Option<Box<Expr>>, &'a Option<Box<Expr>>);
    fn own(e: &Expr, rank_one: bool) -> Option<Bounds<'_>> {
        if rank_one { return section(e); }
        match e { Expr::Call(tag, _) if tag == "@section_subscript" => section(e), _ => None }
    }
    let mut kinds = vec![];
    for e in &outer {
        kinds.push(if own(e, rank_one).is_some() { 1 } else if (known.vector)(e).ok_or(())? { 2 } else { 0 });
    }
    if kinds.iter().all(|k| *k == 0) { return Ok(None); }
    let bin = |op: BinOp, a: Expr, b: Expr| Expr::Bin(op, Box::new(a), Box::new(b));
    let one = || Expr::IntLit("1".into());
    let mut inner = subscripts.iter();
    let mut out = vec![];
    for (dimension, (e, kind)) in outer.iter().zip(&kinds).enumerate() {
        match kind {
            0 => { out.push(((*e).clone(), false)); continue; }
            2 => {
                // Element j of the vector, or a section of it.
                let Some(j) = inner.next() else { return Ok(None) };
                let Some(element) = vector_element(e, j, known)? else { return Ok(None) };
                out.push((element, false));
                continue;
            }
            _ => {}
        }
        let (lower, upper, stride) = own(e, rank_one).unwrap();
        let lower = lower.as_deref().cloned().or_else(|| (known.lower)(array, dimension + 1)).unwrap_or_else(|| Expr::Call("lbound".into(),
            vec![Expr::Ident(array.into()), Expr::IntLit((dimension + 1).to_string())]));
        let stride = stride.as_deref().cloned();
        // Position j of the associate name in the array.
        let at = |j: Expr| {
            let offset = bin(BinOp::Sub, j, one());
            bin(BinOp::Add, lower.clone(), match &stride { Some(s) => bin(BinOp::Mul, offset, s.clone()), None => offset })
        };
        let Some(j) = inner.next() else { return Ok(None) };
        out.push(match section(j) {
            None => (at(j.clone()), false),
            Some((first, last, step)) => (Expr::Section {
                name: array.into(),
                lower: Some(Box::new(first.as_deref().cloned().map_or(lower.clone(), &at))),
                upper: last.as_deref().cloned().map(&at).or_else(|| upper.as_deref().cloned()).map(Box::new),
                stride: match (&stride, step.as_deref()) {
                    (None, None) => None,
                    (Some(s), None) => Some(Box::new(s.clone())),
                    (None, Some(t)) => Some(Box::new(t.clone())),
                    (Some(s), Some(t)) => Some(Box::new(bin(BinOp::Mul, s.clone(), t.clone()))),
                },
            }, true),
        });
    }
    if inner.next().is_some() { return Ok(None); }
    // A section of a rank-one selector is bare; others are wrapped.
    let bare = rank_one && out[0].1;
    let indices = out.into_iter().map(|(e, own)| if own && !rank_one { Expr::Call("@section_subscript".into(), vec![e]) } else { e }).collect();
    Ok(Some(Through { array: array.into(), indices, bare }))
}

/// Element `j` (or the section `j`) of a vector subscript: of an array, a
/// section, or an elemental expression of them such as `v + 1`.
fn vector_element(e: &Expr, j: &Expr, known: &Known<'_>) -> Result<Option<Expr>, ()> {
    let part = |x: &Expr| -> Result<Option<Expr>, ()> {
        if (known.vector)(x).ok_or(())? { vector_element(x, j, known) } else { Ok(Some(x.clone())) }
    };
    Ok(match e {
        Expr::Bin(op, a, b) => match (part(a)?, part(b)?) {
            (Some(a), Some(b)) => Some(Expr::Bin(op.clone(), Box::new(a), Box::new(b))),
            _ => None,
        },
        Expr::Un(op, a) => part(a)?.map(|a| Expr::Un(op.clone(), Box::new(a))),
        Expr::Ident(v) => associate_through(&Expr::Section { name: v.clone(), lower: None, upper: None, stride: None },
            std::slice::from_ref(j), known)?.map(Through::expression),
        Expr::Section { .. } | Expr::Index(..) => associate_through(e, std::slice::from_ref(j), known)?.map(Through::expression),
        Expr::Call(tag, _) if !tag.starts_with('@') || tag == "@section_subscript" => associate_through(e, std::slice::from_ref(j), known)?.map(Through::expression),
        _ => None,
    })
}

/// Resolves the ASSOCIATE_SUBSCRIPT references of a statement, not of the
/// statements it contains. `rank` gives the rank of a visible name.
pub(crate) fn resolve_associate_subscripts(s: &mut Stmt, rank: &dyn Fn(&str) -> usize, lower: &dyn Fn(&str, usize) -> Option<Expr>) -> Result<(), &'static str> {
    let vector = is_vector;
    let classify = |e: &Expr| Some(vector(e, rank));
    let known = Known { vector: &classify, lower };
    if let Stmt::AssignIndex { name, indices, .. } = s {
        if name == ASSOCIATE_SUBSCRIPT {
            let Some(Expr::Call(_, marker)) = indices.first() else { unreachable!() };
            let (selector, subscripts) = marker.split_first().unwrap();
            if let Expr::Call(_, args) | Expr::Index(_, args) = selector {
                if args.iter().any(|a| !matches!(a, Expr::Call(tag, _) if tag == "@section_subscript") && vector(a, rank)) {
                    return Err("an ASSOCIATE name whose selector has a vector subscript cannot be defined");
                }
            }
            let Ok(Some(m)) = associate_through(selector, subscripts, &known) else {
                return Err("subscripts of an ASSOCIATE name require a variable selector");
            };
            *name = m.array;
            *indices = m.indices;
        }
    }
    let mut error = Ok(());
    for e in crate::procedure_arguments::expressions(s) {
        crate::procedure_arguments::visit_expr(e, &mut |e| {
            if let Expr::Call(tag, marker) = e {
                if tag == ASSOCIATE_SUBSCRIPT {
                    let (selector, subscripts) = marker.split_first().unwrap();
                    match associate_through(selector, subscripts, &known) {
                        Ok(Some(m)) => *e = m.expression(),
                        _ => error = Err("subscripts of an ASSOCIATE name require a variable selector"),
                    }
                }
            }
        });
    }
    error
}

/// Declarations from a lexer ProcedureDeclaration token (`form|entities`).
/// Interface bodies declare their procedures EXTERNAL and record whether each
/// is a function; ABSTRACT interfaces record only that; PROCEDURE(iface)
/// statements record the interface and POINTER/OPTIONAL attributes.
/// Marks the initial target of a procedure pointer, as f1 in
/// `procedure(f1), pointer :: pp => f1`.
pub const INITIAL_TARGET: &str = "@initial_target:";
pub fn procedure_declaration(encoded: &str) -> Vec<Stmt> {
    let (form, entities) = encoded.split_once('|').unwrap_or((encoded, ""));
    let mut out = vec![];
    let entities = entities.split(',').filter(|e| !e.is_empty());
    match form {
        "interface" | "abstract" => {
            for entity in entities {
                let (name, kind) = entity.split_once('=').unwrap_or((entity, "function"));
                let simple = kind.ends_with("+simple");
                let kind = kind.strip_suffix("+simple").unwrap_or(kind);
                if simple { out.push(Stmt::DummyIntent { name: name.into(), intent: "@simple_interface".into() }); }
                if form == "interface" {
                    out.push(Stmt::DummyIntent { name: name.into(), intent: "external".into() });
                }
                out.push(Stmt::DummyIntent { name: name.into(), intent: format!("@{form}:{kind}") });
            }
        }
        _ => {
            let mut attributes = form.split(',');
            let interface = attributes.next().unwrap_or("procedure()").to_string();
            // NOPASS and PASS(arg) matter for procedure pointer components.
            let attributes: Vec<&str> = attributes.filter(|a| matches!(*a, "pointer" | "optional" | "nopass") || a.starts_with("pass")).collect();
            // PROCEDURE(INTEGER): an implicit interface and the result type.
            let (interface, result) = match interface.strip_prefix("procedure(@type:").and_then(|t| t.strip_suffix(')')) {
                Some(t) => ("procedure()".to_string(), Some(match t {
                    "integer" => TypeSpec::Integer(None),
                    "real" => TypeSpec::Real,
                    "logical" => TypeSpec::Logical,
                    "complex" => TypeSpec::Complex("4".into()),
                    _ => TypeSpec::DoublePrecision,
                })),
                None => (interface, None),
            };
            for entity in entities {
                let (name, target) = match entity.split_once("=>") {
                    Some((name, target)) => (name, Some(target)),
                    None => (entity, None),
                };
                if let Some(target) = target {
                    out.push(Stmt::DummyIntent { name: name.into(), intent: format!("{INITIAL_TARGET}{target}") });
                }
                if let Some(kind) = &result {
                    out.push(Stmt::VarDecl { kind: kind.clone(), names: vec![name.into()] });
                }
                out.push(Stmt::DummyIntent { name: name.into(), intent: interface.clone() });
                for a in &attributes {
                    out.push(Stmt::DummyIntent { name: name.into(), intent: (*a).into() });
                }
            }
        }
    }
    out
}

// Preserve the already-resolved representation for ordinary literal lengths.
/// Check the KIND of a CHARACTER type: only the ASCII (default) kind is
/// supported. A named kind constant is not known here and is accepted.
pub fn character_kind(kind: &Expr) -> Result<(), &'static str> {
    let supported = match kind {
        Expr::IntLit(n) => n == "1",
        Expr::Call(name, args) if name.eq_ignore_ascii_case("selected_char_kind") => match args.as_slice() {
            [Expr::Str(s)] => s.eq_ignore_ascii_case("ascii") || s.eq_ignore_ascii_case("default"),
            _ => true,
        },
        _ => true,
    };
    if supported { Ok(()) } else { Err("CHARACTER kinds other than the default ASCII kind (such as UCS-4) are not yet supported") }
}

pub fn character_length(length: Expr) -> TypeSpec {
    if let Expr::IntLit(n) = &length {
        if let Ok(n) = n.parse::<usize>() { return TypeSpec::Character(Some(n)); }
    }
    TypeSpec::CharacterLength(Box::new(length))
}

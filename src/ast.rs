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
pub fn generic_bindings(bindings: Vec<(String, String)>) -> Vec<(String, String)> {
    let specific = |b: &str| bindings.iter().find(|(n, _)| n == b && !n.starts_with('@')).map(|(_, t)| t.clone());
    bindings.iter().map(|(n, t)| match n.starts_with("@generic:") || n.starts_with("@operator:") {
        true => (n.clone(), specific(t).unwrap_or_else(|| t.clone())),
        false => (n.clone(), t.clone()),
    }).collect()
}

/// Check the attributes of a type-bound procedure binding.
pub fn binding_attributes(attributes: &[String]) -> Result<(), &'static str> {
    for a in attributes {
        match a.to_ascii_lowercase().as_str() {
            "public" | "private" | "pass" | "non_overridable" => {}
            "nopass" => return Err("NOPASS type-bound procedures are not yet supported"),
            "deferred" => return Err("DEFERRED type-bound procedures are not yet supported"),
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
pub fn attribute_statement(attr: &str, argument: Option<String>, names: Vec<String>) -> Result<Vec<Stmt>, &'static str> {
    let attribute = match (attr, argument) {
        ("intent", Some(intent)) => match intent.to_ascii_lowercase().as_str() {
            intent @ ("in" | "out" | "inout") => format!("intent({intent})"),
            _ => return Err("INTENT must be IN, OUT or INOUT"),
        },
        ("external", None) => "external".to_string(),
        ("intrinsic", None) => return Ok(vec![]),
        ("save", None) => "save".to_string(),
        (attr @ ("optional" | "pointer" | "target" | "allocatable"), None) => attr.to_string(),
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
                Stmt::AssignIndex { name: n, .. } if n == name => {
                    let Some(a) = whole else { return Err("subscripts of an ASSOCIATE name require a whole-array selector") };
                    *n = a.into();
                }
                Stmt::Do { var, .. } if var == name => return Err("an ASSOCIATE name cannot be a DO variable"),
                _ => {}
            }
            let mut error = Ok(());
            for e in expressions(s) {
                visit_expr(e, &mut |e| match e {
                    Expr::Ident(n) if n == name => *e = selector.clone(),
                    // A COMPLEX part of an expression selector, as `r%im`.
                    Expr::Ident(n) if whole.is_none() && matches!(n.strip_prefix(&prefix), Some("re" | "im")) => {
                        let part = if n.ends_with("%re") { "real" } else { "aimag" };
                        *e = Expr::Call(part.into(), vec![selector.clone()]);
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

/// Declarations from a lexer ProcedureDeclaration token (`form|entities`).
/// Interface bodies declare their procedures EXTERNAL and record whether each
/// is a function; ABSTRACT interfaces record only that; PROCEDURE(iface)
/// statements record the interface and POINTER/OPTIONAL attributes.
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
            let attributes: Vec<&str> = attributes.filter(|a| matches!(*a, "pointer" | "optional")).collect();
            for name in entities {
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
pub fn character_length(length: Expr) -> TypeSpec {
    if let Expr::IntLit(n) = &length {
        if let Ok(n) = n.parse::<usize>() { return TypeSpec::Character(Some(n)); }
    }
    TypeSpec::CharacterLength(Box::new(length))
}

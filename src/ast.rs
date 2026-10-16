use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    #[allow(dead_code)]
    pub name: String,
    pub body: Vec<Stmt>,
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
        ("external" | "intrinsic", None) => return Ok(vec![]),
        (attr @ ("optional" | "pointer" | "target" | "allocatable"), None) => attr.to_string(),
        _ => return Err("invalid attribute specification statement"),
    };
    Ok(names.into_iter().map(|name| Stmt::DummyIntent { name, intent: attribute.clone() }).collect())
}

// Preserve the already-resolved representation for ordinary literal lengths.
pub fn character_length(length: Expr) -> TypeSpec {
    if let Expr::IntLit(n) = &length {
        if let Ok(n) = n.parse::<usize>() { return TypeSpec::Character(Some(n)); }
    }
    TypeSpec::CharacterLength(Box::new(length))
}

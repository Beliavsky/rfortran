use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    #[allow(dead_code)]
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Stmt {
    OperatorInterface { operator: String, procedures: Vec<String> },
    DerivedType { name: String, components: Vec<Stmt>, bindings: Vec<(String, String)> },
    DerivedDecl { type_name: String, names: Vec<String>, attributes: Vec<String>, polymorphic: bool },
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
        #[serde(default)]
        pure: bool,
        name: String,
        params: Vec<String>,
        return_type: Option<TypeSpec>,
        body: Vec<Stmt>,
    },
    Subroutine {
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
        only: Option<Vec<String>>,
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
    Real,
    RealKind(String),
    DoublePrecision,
    Character(Option<usize>),
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
    Stmt::Function { name, params, return_type, body, pure: false }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

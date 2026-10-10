use anyhow::Result;

use crate::ast::Program;
use crate::errors::{CompileError, CompileErrorKind};
use crate::lexer::{Token, TokenKind};

#[allow(unused_variables, dead_code)]
pub mod fortran {
    include!(concat!(env!("OUT_DIR"), "/fortran.rs"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn named_results_and_result_identifiers() {
        let source = include_str!("../tests/39_function_result.f90");
        let tokens = crate::lexer::lex(source);
        let program = super::parse_with_src(source, &tokens, "result.f90").unwrap();
        let errors = crate::sema::analyze_with_src(&program, source, &tokens, "result.f90",
            &crate::sema::SemaSettings::default(), &std::collections::HashSet::new(), false);
        assert!(!errors.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic)), "{errors:?}");
        let p = crate::parameters::resolve(program).unwrap();
        crate::initialized::instrument(p).unwrap();
        for source in [
            "function result(i)\ninteger :: result,i\nresult=i\nend function result",
            "function f(i) RESULT(j)\ninteger :: i,j\nj=i\nend function f",
        ] {
            let tokens = crate::lexer::lex(source);
            super::parse_with_src(source, &tokens, "result.f90").unwrap();
        }
        let source = "module m\ncontains\nfunction f(i) result(j)\ninteger :: i,j\nend function f\nend module m";
        let tokens = crate::lexer::lex(source);
        let p = super::parse_with_src(source, &tokens, "result.f90").unwrap();
        assert!(crate::initialized::instrument(p).unwrap_err().to_string().contains("uninitialized variable `j`"));
    }

    #[test]
    fn unnamed_main_programs_parse_without_changing_library_units() {
        for source in [
            "integer :: i\ni=3\nprint *, i\nend",
            "implicit none\ninteger :: i\ni=3\nprint *, i\nend program",
            "print *, 3\nend",
            "integer :: i\nend",
            "end program",
            "call hello()\ncontains\nsubroutine hello()\nprint *, 3\nend subroutine hello\nend",
            "call hello()\nend\nsubroutine hello()\nprint *, 3\nend subroutine hello",
        ] {
            let tokens = crate::lexer::lex(source);
            let p = super::parse_with_src(source, &tokens, "unnamed.f90").unwrap();
            assert_eq!(p.name, "main", "{source}");
        }
        for source in [
            "subroutine hello()\nprint *, 3\nend subroutine hello",
            "module example\ninteger :: i\nend module example",
        ] {
            let tokens = crate::lexer::lex(source);
            let p = super::parse_with_src(source, &tokens, "library.f90").unwrap();
            assert_eq!(p.name, "<anon>", "{source}");
        }
    }

    #[test]
    fn mixed_scalar_and_array_declarations_preserve_order_and_scope() {
        use crate::ast::{Stmt, TypeSpec};
        for source in [
            "program main\nreal :: x, y(2), z, a(3)\nend program main",
            "subroutine example()\nreal :: x, y(2), z, a(3)\nend subroutine example",
            "module example\nreal :: x, y(2), z, a(3)\nend module example",
            "real :: x, y(2), z, a(3)\nprogram main\nend program main",
        ] {
            let tokens = crate::lexer::lex(source);
            let program = super::parse_with_src(source, &tokens, "mixed.f90").unwrap();
            let body = match &program.body[0] {
                Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => body,
                _ => &program.body,
            };
            assert_eq!(body.len(), 4);
            let expected = [("x", None), ("y", Some("2")), ("z", None), ("a", Some("3"))];
            for (stmt, (expected_name, expected_dim)) in body.iter().zip(expected) {
                match (stmt, expected_dim) {
                    (Stmt::VarDecl { kind: TypeSpec::Real, names }, None) => assert_eq!(names, &[expected_name]),
                    (Stmt::ArrayDecl { kind: TypeSpec::Real, name, dims }, Some(size)) => {
                        assert_eq!(name, expected_name);
                        assert!(matches!(dims.as_slice(), [crate::ast::Expr::IntLit(value)] if value == size));
                    }
                    _ => panic!("unexpected declaration: {stmt:?}"),
                }
            }
        }
    }

    #[test]
    fn bare_end_is_an_empty_main_program() {
        for src in ["end\n", "! Empty main program\nEND ! done\n"] {
            let tokens = crate::lexer::lex(src);
            let program = super::parse_with_src(src, &tokens, "bare_end.f90")
                .expect("a bare END must form an empty main program");
            assert_eq!(program.name, "main");
            assert!(program.body.is_empty());
        }
    }
}

fn human_expected(name: &str) -> String {
    match name {
        "KW_PROGRAM" => "keyword program".into(),
        "KW_RESULT" => "keyword result".into(),
        "KW_PURE" => "keyword pure".into(),
        "KW_END" => "keyword end".into(),
        "KW_IMPLICIT" => "keyword implicit".into(),
        "KW_NONE" => "keyword none".into(),
        "KW_PRINT" => "keyword print".into(),
        "KW_IF" => "keyword if".into(),
        "KW_THEN" => "keyword then".into(),
        "KW_ELSE" => "keyword else".into(),
        "KW_INTEGER" => "keyword integer".into(),
        "KW_REAL" => "keyword real".into(),
        "KW_DOUBLE" => "keyword double".into(),
        "KW_PRECISION" => "keyword precision".into(),
        "KW_CHARACTER" => "keyword character".into(),
        "KW_LOGICAL" => "keyword logical".into(),
        "KW_LEN" => "keyword len".into(),
        "ID" => "identifier".into(),
        "STR" => "string literal".into(),
        "FLOAT" => "real literal".into(),
        "INT" => "integer literal".into(),
        "TRUE" => "logical .true.".into(),
        "FALSE" => "logical .false.".into(),
        "COMMA" => "','".into(),
        "STAR" => "'*'".into(),
        "EQ" => "'='".into(),
        "LPAREN" => "'('".into(),
        "RPAREN" => "')'".into(),
        other => other.to_lowercase(),
    }
}

fn human_token(tok: &TokenKind) -> String {
    match tok {
        TokenKind::KwType => "keyword type".into(),
        TokenKind::KwClass => "keyword class".into(),
        TokenKind::KwProcedure => "keyword procedure".into(),
        TokenKind::KwInterface => "keyword interface".into(),
        TokenKind::KwOperator => "keyword operator".into(),
        TokenKind::KwKind => "keyword kind".into(),
        TokenKind::KwOnly => "keyword only".into(),
        TokenKind::EndInterfaceName(n) => format!("interface name {n}"),
        TokenKind::DefinedOperator(name) => format!("defined operator {name}"),
        TokenKind::Percent => "'%'".into(),
        TokenKind::Arrow => "'=>'".into(),
        TokenKind::_Continuation => "continuation".into(),
        TokenKind::LBracket => "'['".into(),
        TokenKind::RBracket => "']'".into(),
        TokenKind::KwError => "keyword error".into(),
        TokenKind::KwAllocate => "keyword allocate".into(),
        TokenKind::KwProgram => "keyword program".into(),
        TokenKind::KwEnd => "keyword end".into(),
        TokenKind::KwImplicit => "keyword implicit".into(),
        TokenKind::KwNone => "keyword none".into(),
        TokenKind::KwPrint => "keyword print".into(),
        TokenKind::KwRead => "keyword read".into(),
        TokenKind::KwIf => "keyword if".into(),
        TokenKind::KwThen => "keyword then".into(),
        TokenKind::KwElse => "keyword else".into(),
        TokenKind::KwInteger => "keyword integer".into(),
        TokenKind::KwReal => "keyword real".into(),
        TokenKind::KwDouble => "keyword double".into(),
        TokenKind::KwPrecision => "keyword precision".into(),
        TokenKind::KwCharacter => "keyword character".into(),
        TokenKind::KwLogical => "keyword logical".into(),
        TokenKind::KwLen => "keyword len".into(),
        TokenKind::KwDo => "keyword do".into(),
        TokenKind::KwWhile => "keyword while".into(),
        TokenKind::KwSubroutine => "keyword subroutine".into(),
        TokenKind::KwCall => "keyword call".into(),
        TokenKind::KwContains => "keyword contains".into(),
        TokenKind::KwUse => "keyword use".into(),
        TokenKind::KwModule => "keyword module".into(),
        TokenKind::KwSelect => "keyword select".into(),
        TokenKind::KwCase => "keyword case".into(),
        TokenKind::KwDefault => "keyword default".into(),
        TokenKind::Ident(s) => format!("identifier {}", s),
        TokenKind::Str(s) => format!("string literal {:?}", s),
        TokenKind::Float(x) => format!("real literal {}", x),
        TokenKind::Integer(i) => format!("integer literal {}", i),
        TokenKind::True => ".true.".into(),
        TokenKind::False => ".false.".into(),
        TokenKind::Comma => "','".into(),
        TokenKind::Star => "'*'".into(),
        TokenKind::LParen => "'('".into(),
        TokenKind::RParen => "')'".into(),
        TokenKind::DColon => "'::'".into(),
        TokenKind::Colon => "':'".into(),
        TokenKind::Eq => "'='".into(),
        TokenKind::Plus => "'+'".into(),
        TokenKind::Minus => "'-'".into(),
        TokenKind::Slash => "'/'".into(),
        TokenKind::Pow => "'**'".into(),
        TokenKind::Concat => "'//'".into(),
        TokenKind::EqEq => "'=='".into(),
        TokenKind::Ne => "'/='".into(),
        TokenKind::Le => "'<='".into(),
        TokenKind::Ge => "'>='".into(),
        TokenKind::Lt => "'<'".into(),
        TokenKind::Gt => "'>'".into(),
        TokenKind::And => "'.and.'".into(),
        TokenKind::Or => "'.or.'".into(),
        TokenKind::Not => "'.not.'".into(),
        TokenKind::Eqv => "'.eqv.'".into(),
        TokenKind::Neqv => "'.neqv.'".into(),
        TokenKind::KwFunction => "keyword function".into(),
        TokenKind::KwResult => "keyword result".into(),
        TokenKind::KwPure => "keyword pure".into(),
        TokenKind::KwForall => "keyword forall".to_string(),
        TokenKind::KwElemental => "keyword elemental".to_string(),
        TokenKind::KwPublic => "keyword public".into(),
        TokenKind::KwPrivate => "keyword private".into(),
        TokenKind::KwReturn => "keyword return".into(),
        TokenKind::KwBlock => "keyword block".into(),
        TokenKind::KwExit => "keyword exit".into(),
        TokenKind::KwCycle => "keyword cycle".into(),
        TokenKind::KwStop => "keyword stop".into(),
        TokenKind::_WS => "whitespace".into(),
        TokenKind::_COMMENT => "comment".into(),
        TokenKind::Error(e) => format!("error: {}", e),
    }
}

pub fn parse_with_src(
    src: &str,
    tokens: &[Token],
    filename: &str,
) -> Result<Program, Vec<CompileError>> {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use codespan_reporting::files::SimpleFile;
    use codespan_reporting::term::{
        emit_to_io_write,
        termcolor::{ColorChoice, StandardStream},
        Config,
    };

    let mut errs: Vec<CompileError> = Vec::new();

    let file = SimpleFile::new(filename, src);
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);

    for t in tokens {
        if let TokenKind::Error(text) = &t.kind {
            let msg = format!("unrecognized token `{}`", text);
            let span = t.span.clone();
            let diag = Diagnostic::error()
                .with_message(&msg)
                .with_labels(vec![Label::primary((), span.clone())]);
            let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diag);
            errs.push(CompileError::new(CompileErrorKind::Lex, msg, span));
        }
    }

    let triples: Vec<(usize, TokenKind, usize)> = tokens
        .iter()
        .map(|t| (t.span.start, t.kind.clone(), t.span.end))
        .collect();

    let parsed: Result<Program, Vec<CompileError>> = match fortran::ProgramParser::new()
        .parse(triples.into_iter())
    {
        Ok(p) => Ok(p),
        Err(e) => {
            let (span, msg) = match e {
                lalrpop_util::ParseError::InvalidToken { location } => {
                    ((location)..(location + 1), "invalid token".to_string())
                }
                lalrpop_util::ParseError::UnrecognizedEof { expected, .. } => {
                    let human: Vec<String> = expected.iter().map(|s| human_expected(s)).collect();
                    (
                        (src.len())..(src.len()),
                        format!(
                            "unexpected end of input, expected one of: {}",
                            human.join(", ")
                        ),
                    )
                }
                lalrpop_util::ParseError::UnrecognizedToken {
                    token: (start, tok, end),
                    expected,
                } => {
                    let human: Vec<String> = expected.iter().map(|s| human_expected(s)).collect();
                    (
                        (start)..(end),
                        format!(
                            "unexpected {}: expected one of: {}",
                            human_token(&tok),
                            human.join(", ")
                        ),
                    )
                }
                lalrpop_util::ParseError::ExtraToken {
                    token: (start, tok, end),
                } => ((start)..(end), format!("extra token {}", human_token(&tok))),
                lalrpop_util::ParseError::User { error } => ((0)..(0), format!("{}", error)),
            };
            let diag = Diagnostic::error()
                .with_message(&msg)
                .with_labels(vec![Label::primary((), span.clone())]);
            let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diag);
            errs.push(CompileError::new(CompileErrorKind::Parse, msg, span));
            Ok(crate::ast::Program {
                name: "<error>".into(),
                body: vec![],
            })
        }
    };

    match parsed {
        Ok(p) => {
            if errs.is_empty() {
                Ok(p)
            } else {
                Err(errs)
            }
        }
        Err(_) => Err(errs),
    }
}

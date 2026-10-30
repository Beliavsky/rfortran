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
    fn pointer_association_retains_declaration_attributes() {
        use crate::ast::{Expr,Stmt};
        let source="integer,target::x\ninteger,pointer::p\np=>x\nnullify(p)\nend";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"pointer.f90").unwrap();
        for (n,a) in [("x","target"),("p","pointer")] {
            assert!(program.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n && intent==a)));
        }
        assert!(program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,args} if name=="@pointer_associate" && matches!(args.as_slice(),[Expr::Ident(p),Expr::Ident(x)] if p=="p" && x=="x"))));
        assert!(program.body.iter().any(|s|matches!(s,Stmt::CallSub{name,..} if name=="@nullify")));
    }

    #[test]
    fn dimension_attributes_lower_to_array_declarations() {
        use crate::ast::{Expr, Stmt};
        let source = "program main\ninteger, dimension(2,3) :: a,b\nreal, dimension(2,3) :: c(4)\ncontains\npure subroutine f(nn)\ninteger,dimension(:),intent(in)::nn\nend subroutine f\nend program main";
        let program = super::parse_with_src(source, &crate::lexer::lex(source), "dimension.f90").unwrap();
        for name in ["a", "b"] {
            assert!(program.body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name: n, dims, .. }
                if n == name && matches!(&dims[..], [Expr::IntLit(x), Expr::IntLit(y)] if x == "2" && y == "3"))));
        }
        assert!(program.body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, dims, .. }
            if name == "c" && matches!(&dims[..], [Expr::IntLit(x)] if x == "4"))));
        let body = program.body.iter().find_map(|s| match s {
            Stmt::Subroutine { body, .. } => Some(body), _ => None,
        }).unwrap();
        assert!(body.iter().any(|s| matches!(s, Stmt::DummyIntent { name, intent }
            if name == "nn" && intent == "intent(in)")));
        assert!(body.iter().any(|s| matches!(s, Stmt::ArrayDecl { name, dims, .. }
            if name == "nn" && matches!(&dims[..], [Expr::Ident(x)] if x == "@deferred_extent"))));
        for attributes in ["dimension()", "dimension(2),dimension(3)"] {
            let source = format!("program main\ninteger,{attributes}::a\nend program main");
            assert!(super::parse_with_src(&source, &crate::lexer::lex(&source), "invalid.f90").is_err());
        }
    }

    #[test]
    fn dimension_attribute_and_entity_forms_have_identical_trees() {
        fn normalize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Array(items) => {
                    items.retain(|item| item.get("SourceLocation").is_none());
                    for item in items { normalize(item); }
                }
                serde_json::Value::Object(fields) => {
                    for value in fields.values_mut() { normalize(value); }
                }
                _ => {}
            }
        }
        let parse = |source: &str| {
            let program=super::parse_with_src(source,&crate::lexer::lex(source),"dimension.f90")
                .unwrap_or_else(|error|panic!("{source}\n{error:?}"));
            let mut tree=serde_json::to_value(program).unwrap();
            normalize(&mut tree);
            tree
        };
        let prefix="implicit none\ninteger,parameter::n=2\ntype t\ninteger::v\nend type\ninteger::mold\n";
        for kind in ["integer", "real(kind=8)", "double precision", "complex", "logical",
            "character(len=3)", "type(t)", "class(t)", "class(*)", "typeof(mold)", "classof(mold)"] {
            for shape in ["n+1", "-1:n,2:3", ":,:,+n", "0:,*", "..", "1,1,1,1,1,1,1,1,1,1,1,1,1,1,1"] {
                for attrs in ["", ",pointer", ",intent(in)"] {
                    let entity=format!("{kind}{attrs}::a({shape}),b(4)");
                    for attribute in [format!("{kind}{attrs},dimension({shape})::a,b(4)"),
                        format!("{kind},dimension({shape}){attrs}::a,b(4)")] {
                        for scope in 0..4 {
                            // Compare declaration syntax independently of each
                            // scope's subsequent semantic shape restrictions.
                            let wrap=|declaration:&str| match scope {
                                0=>format!("program p\n{prefix}{declaration}\nend"),
                                1=>format!("module m\n{prefix}{declaration}\nend module"),
                                2=>format!("subroutine f(a,b)\n{prefix}{declaration}\nend subroutine"),
                                _=>format!("module m\n{prefix}type holder\n{declaration}\nend type\nend module"),
                            };
                            assert_eq!(parse(&wrap(&attribute)),parse(&wrap(&entity)),"{attribute}, scope {scope}");
                        }
                    }
                }
            }
            for attrs in ["dimension()", "dimension(2),dimension(3)"] {
                let source=format!("program p\n{prefix}{kind},{attrs}::a\nend");
                assert!(super::parse_with_src(&source,&crate::lexer::lex(&source),"invalid.f90").is_err(),"{source}");
            }
        }
    }

    #[test]
    fn impure_elemental_prefixes_and_contextual_identifiers() {
        for prefix in ["impure elemental", "elemental impure", "IMPURE &\n ELEMENTAL"] {
            let source=format!("{prefix} subroutine f(i)\ninteger,intent(in)::i\nprint *,i\nend subroutine f");
            let program=super::parse_with_src(&source,&crate::lexer::lex(&source),"impure.f90").unwrap();
            assert!(matches!(&program.body[0],crate::ast::Stmt::Subroutine {pure:false,elemental:true,..}));
        }
        let tokens=crate::lexer::lex("integer::impure,elemental\nimpure=elemental\ncall impure(elemental)");
        assert!(!tokens.iter().any(|t|matches!(t.kind,crate::lexer::TokenKind::KwImpure|crate::lexer::TokenKind::KwElemental)));
        let source="pure impure subroutine f()\nend subroutine f";
        assert!(super::parse_with_src(source,&crate::lexer::lex(source),"invalid.f90").is_err());
    }
    #[test]
    fn recursive_prefixes_and_contextual_identifiers() {
        for prefix in ["recursive", "pure recursive", "recursive pure", "pure &\n recursive"] {
            let source=format!("module m\ncontains\n{prefix} subroutine f(a)\ninteger,intent(inout)::a\na=a+1\nend subroutine f\nend module m\nprogram main\ninteger::recursive\nrecursive=3\ncall f(recursive)\ncontains\nrecursive integer function g(a) result(v)\ninteger::a,v\nv=a\nend function g\nend program main");
            let tokens=crate::lexer::lex(&source);
            let p=super::parse_with_src(&source,&tokens,"recursive.f90").unwrap();
            let crate::ast::Stmt::Module {body,..}=&p.body[0] else {panic!("module missing")};
            assert!(body.iter().any(|s|matches!(s,crate::ast::Stmt::Subroutine {pure,..} if *pure==prefix.contains("pure"))));
        }
        let tokens=crate::lexer::lex("integer::recursive,pure\nrecursive=pure\ncall recursive(pure)\n");
        assert!(!tokens.iter().any(|t|matches!(t.kind,crate::lexer::TokenKind::KwRecursive | crate::lexer::TokenKind::KwPure)));
    }
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
            "program main\nreal x, y(2), z, a(3)\nend program main",
            "subroutine example()\nreal x, y(2), z, a(3)\nend subroutine",
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
            let body:Vec<_>=body.iter().filter(|s|!matches!(s,Stmt::SourceLocation(_))).collect();
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
    fn declarations_without_colons_preserve_character_lengths_and_kind_selectors() {
        use crate::ast::{Stmt, TypeSpec};
        let source="character(len=8) ampm\ninteger(kind=4) values(8)\nampm='AM'\nend";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"plain.f90").unwrap();
        let declarations:Vec<_>=program.body.iter().filter(|s|!matches!(s,Stmt::SourceLocation(_))).collect();
        assert!(matches!(declarations[0],Stmt::VarDecl{kind:TypeSpec::Character(Some(8)),names} if names==&["ampm"]));
        assert!(matches!(declarations[1],Stmt::ArrayDecl{kind:TypeSpec::IntegerKind(kind),name,dims} if kind=="4" && name=="values" && dims.len()==1));
        for source in ["integer i=1\nend", "character(2) s='hi'\nend",
                       "integer,parameter n=1\nend", "real,dimension(2) a\nend"] {
            assert!(super::parse_with_src(source,&crate::lexer::lex(source),"invalid.f90").is_err(),"{source}");
        }
    }

    #[test]
    fn derived_type_headers_accept_optional_colons_attributes_and_end_names() {
        use crate::ast::Stmt;
        for source in [
            "type box\ninteger :: a\nend type box\nend",
            "type box\ninteger :: a\nend type\nend",
            "type :: box\ninteger :: a\nend type\nx=1\nend",
            "module m\ntype, public :: box\ninteger :: a\nend type box\nend module m",
            "module m\ntype, bind(c) :: box\ninteger :: a\nend type\nend module m",
        ] {
            let program=super::parse_with_src(source,&crate::lexer::lex(source),"type.f90").unwrap();
            let body=match &program.body[..] {
                [Stmt::Module{body,..}] => body, body => body,
            };
            assert!(body.iter().any(|s|matches!(s,Stmt::DerivedType{name,..} if name=="box")),"{source}");
        }
        let source="module m\ntype, private :: box\ninteger :: a\nend type box\nend module m";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"type.f90").unwrap();
        let Stmt::Module{body,..}=&program.body[0] else {panic!("module missing")};
        assert!(body.iter().any(|s|matches!(s,Stmt::Accessibility{public:false,names} if names==&["box"])));
        // A variable named TYPE and a SELECT TYPE guard are not definitions.
        let tokens=crate::lexer::lex("integer :: type\ntype = 3\nprint *, type");
        assert!(!tokens.iter().any(|t|matches!(t.kind,crate::lexer::TokenKind::KwType)));
        // EXTENDS records the parent for type-extension lowering.
        let source="module m\ntype, extends(base) :: child\nend type\nend module m";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"extends.f90").unwrap();
        let Stmt::Module{body,..}=&program.body[0] else {panic!("module missing")};
        assert!(body.iter().any(|s|matches!(s,Stmt::CallSub{name,args} if name==crate::extensions::EXTENDS
            && matches!(args.as_slice(),[crate::ast::Expr::Ident(c),crate::ast::Expr::Ident(p)] if c=="child" && p=="base"))));
        let source="module m\ntype, abstract :: base\nend type\nend module m";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"abstract.f90").unwrap();
        let Stmt::Module{body,..}=&program.body[0] else {panic!("module missing")};
        assert!(body.iter().any(|s|matches!(s,Stmt::CallSub{name,args} if name==crate::abstract_types::ABSTRACT
            && matches!(args.as_slice(),[crate::ast::Expr::Ident(n)] if n=="base"))));
    }

    #[test]
    fn pointer_initializers_lower_to_disassociated_or_associated_pointers() {
        use crate::ast::{Expr,Stmt};
        let source="integer, target :: t\ninteger, pointer :: p => null(), q(:) => NULL()\ninteger, pointer :: r => t\nend";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"pointer_init.f90").unwrap();
        let associations:Vec<_>=program.body.iter().filter_map(|s|match s {
            Stmt::CallSub{name,args} if name=="@pointer_associate" => Some(args.as_slice()), _ => None,
        }).collect();
        assert!(matches!(associations.as_slice(),[[Expr::Ident(p),Expr::Ident(t)]] if p=="r" && t=="t"));
        assert!(!program.body.iter().any(|s|matches!(s,Stmt::InitializedDecl{..})));
        for name in ["p","q","r"] {
            assert!(program.body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==name && intent=="pointer")));
        }
    }

    #[test]
    fn attribute_statements_record_declaration_attributes() {
        use crate::ast::Stmt;
        let source="subroutine s(a,b,c,f)\nreal :: a,b,c\nexternal f\nintent(in) :: a, b\nintent(in out) c\noptional :: b\nintrinsic abs\nend subroutine s";
        let program=super::parse_with_src(source,&crate::lexer::lex(source),"attributes.f90").unwrap();
        let Stmt::Subroutine{body,..}=&program.body[0] else {panic!("subroutine missing")};
        let attributes:Vec<_>=body.iter().filter_map(|s|match s {
            Stmt::DummyIntent{name,intent} => Some((name.as_str(),intent.as_str())), _ => None,
        }).collect();
        assert_eq!(attributes,[("f","external"),("a","intent(in)"),("b","intent(in)"),("c","intent(inout)"),("b","optional")]);
        // Variables and arrays with attribute names remain ordinary names.
        let tokens=crate::lexer::lex("integer :: target, pointer(2), intent\ntarget = 1\npointer(1) = 2\nintent = target");
        assert!(!tokens.iter().any(|t|matches!(t.kind,crate::lexer::TokenKind::AttrStatement(_))));
        let source="subroutine s(a)\nintent(sideways) :: a\nend subroutine s";
        assert!(super::parse_with_src(source,&crate::lexer::lex(source),"bad_intent.f90").is_err());
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
        "KW_IMPURE" => "keyword impure".into(),
        "KW_RECURSIVE" => "keyword recursive".into(),
        "KW_END" => "keyword end".into(),
        "KW_IMPLICIT" => "keyword implicit".into(),
        "KW_NONE" => "keyword none".into(),
        "KW_PRINT" | "KW_PRINT_EMPTY" => "keyword print".into(),
        "KW_WRITE" | "KW_WRITE_EMPTY" => "keyword write".into(),
        "KW_STOP_EMPTY" => "keyword stop".into(),
        "KW_DO_UNBOUNDED" => "kw_do".into(),
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
        TokenKind::KwRank => "keyword rank".into(),
        TokenKind::KwTypeIs => "TYPE IS".into(),
        TokenKind::KwClassIs => "CLASS IS".into(),
        TokenKind::KwClassDefault => "CLASS DEFAULT".into(),
        TokenKind::DotDot => "'..'".into(),
        TokenKind::KwComplex => "keyword complex".into(),
        TokenKind::ComplexOpen => "'('".into(),
        TokenKind::FileControl(name) | TokenKind::FilePosition(name) => format!("keyword {name}"),
        TokenKind::Boz(text) => format!("BOZ literal {text}"),
        TokenKind::KwType => "keyword type".into(),
        TokenKind::KwClass => "keyword class".into(),
        TokenKind::KwTypeof => "keyword typeof".into(),
        TokenKind::KwClassof => "keyword classof".into(),
        TokenKind::KwProcedure => "keyword procedure".into(),
        TokenKind::BindSpec(_) => "BIND(C) specification".into(),
        TokenKind::KwInterface => "keyword interface".into(),
        TokenKind::KwOperator => "keyword operator".into(),
        TokenKind::KwKind => "keyword kind".into(),
        TokenKind::KwOnly => "keyword only".into(),
        TokenKind::AttrStatement(word) => format!("keyword {word}"),
        TokenKind::ProcedureDeclaration(_) => "procedure declaration".into(),
        TokenKind::KwAssociate => "keyword associate".into(),
        TokenKind::EndInterfaceName(n) => format!("interface name {n}"),
        TokenKind::EndUnitName(n) => format!("unit name {n}"),
        TokenKind::BareEnd => "keyword end".into(),
        TokenKind::KwNullify => "keyword nullify".into(),
        TokenKind::DefinedOperator(name) => format!("defined operator {name}"),
        TokenKind::Percent => "'%'".into(),
        TokenKind::Arrow => "'=>'".into(),
        TokenKind::_Continuation => "continuation".into(),
        TokenKind::LBracket => "'['".into(),
        TokenKind::RBracket => "']'".into(),
        TokenKind::KwError => "keyword error".into(),
        TokenKind::KwAllocate => "keyword allocate".into(),
        TokenKind::KwDimension => "keyword dimension".into(),
        TokenKind::KwSaveAll => "keyword save".into(),
        TokenKind::KwDeallocate => "keyword deallocate".into(),
        TokenKind::KwProgram => "keyword program".into(),
        TokenKind::KwEnd => "keyword end".into(),
        TokenKind::KwImplicit => "keyword implicit".into(),
        TokenKind::KwNone => "keyword none".into(),
        TokenKind::KwPrint | TokenKind::KwPrintEmpty => "keyword print".into(),
        TokenKind::KwWrite | TokenKind::KwWriteEmpty => "keyword write".into(),
        TokenKind::KwRead => "keyword read".into(),
        TokenKind::KwReadEmpty => "keyword read".into(),
        TokenKind::KwContinue => "keyword continue".into(),
        TokenKind::StatementLabel(n) => format!("statement label {n}"),
        TokenKind::FormatSpec(_) => "FORMAT statement".into(),
        TokenKind::KwData => "keyword data".into(),
        TokenKind::KwEquivalence => "keyword equivalence".into(),
        TokenKind::KwGoto => "keyword goto".into(),
        TokenKind::KwIf => "keyword if".into(),
        TokenKind::KwThen => "keyword then".into(),
        TokenKind::KwElse => "keyword else".into(),
        TokenKind::KwElseIf => "keyword else if".into(),
        TokenKind::KwInteger => "keyword integer".into(),
        TokenKind::KwReal => "keyword real".into(),
        TokenKind::KwDouble => "keyword double".into(),
        TokenKind::KwPrecision => "keyword precision".into(),
        TokenKind::KwCharacter => "keyword character".into(),
        TokenKind::KwLogical => "keyword logical".into(),
        TokenKind::KwLen => "keyword len".into(),
        TokenKind::KwDo | TokenKind::KwDoUnbounded => "keyword do".into(),
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
        TokenKind::At => "'@'".into(),
        TokenKind::Question => "'?'".into(),
        TokenKind::Nil => "'.nil.'".into(),
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
        TokenKind::KwSimple => "keyword simple".into(),
        TokenKind::KwReduce => "keyword reduce".into(),
        TokenKind::KwEnumeration => "keyword enumeration".into(),
        TokenKind::KwEnum => "keyword enum".into(),
        TokenKind::KwEnumerator => "keyword enumerator".into(),
        TokenKind::KwImpure => "keyword impure".into(),
        TokenKind::KwRecursive => "keyword recursive".into(),
        TokenKind::KwWhere | TokenKind::KwWhereBlock => "keyword where".into(),
        TokenKind::KwElseWhere => "keyword elsewhere".into(),
        TokenKind::KwEndWhere => "keyword end where".into(),
        TokenKind::KwParameter => "keyword parameter".into(),
        TokenKind::KwForall | TokenKind::KwForallBlock => "keyword forall".to_string(),
        TokenKind::KwEndForall => "end forall".to_string(),
        TokenKind::KwElemental => "keyword elemental".to_string(),
        TokenKind::KwPublic => "keyword public".into(),
        TokenKind::KwPrivate => "keyword private".into(),
        TokenKind::KwReturn => "keyword return".into(),
        TokenKind::KwBlock => "keyword block".into(),
        TokenKind::KwExit => "keyword exit".into(),
        TokenKind::KwCycle => "keyword cycle".into(),
        TokenKind::KwStop | TokenKind::KwStopEmpty => "keyword stop".into(),
        TokenKind::_WS => "whitespace".into(),
        TokenKind::_COMMENT => "comment".into(),
        TokenKind::ConstructName(n) => format!("DO construct name {n}"),
        TokenKind::KwEndDo => "keyword enddo".into(),
        TokenKind::EndLoopName(n) => format!("END DO construct name {n}"),
        TokenKind::NamedExit(n) => format!("EXIT {n}"),
        TokenKind::NamedCycle(n) => format!("CYCLE {n}"),
        TokenKind::Error(e) => format!("error: {}", e),
    }
}

/// Give each DIMENSION-statement entity its type: from a type declaration of
/// the same name in the same scope, or by default implicit typing.
fn resolve_dimension_statements(body: &mut Vec<crate::ast::Stmt>) -> Result<(), String> {
    use crate::ast::{Implicit, Stmt, TypeSpec};
    let marker = |kind: &TypeSpec| matches!(kind, TypeSpec::IntegerKind(k) if k == "@dimension");
    let pending: Vec<String> = body.iter().filter_map(|s| match s {
        Stmt::ArrayDecl { kind, name, .. } if marker(kind) => Some(name.clone()),
        _ => None,
    }).collect();
    for name in pending {
        let lower = name.to_ascii_lowercase();
        let mut kind = None;
        let mut derived = None;
        for s in body.iter_mut() {
            match s {
                Stmt::VarDecl { kind: k, names } if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) => {
                    names.retain(|n| !n.eq_ignore_ascii_case(&name));
                    kind = Some(k.clone());
                }
                Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims } if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) => {
                    names.retain(|n| !n.eq_ignore_ascii_case(&name));
                    derived = Some(Stmt::DerivedDecl { type_name: type_name.clone(), names: vec![name.clone()], attributes: attributes.clone(), polymorphic: *polymorphic, dims: dims.clone() });
                }
                Stmt::InitializedDecl { entities, .. } if entities.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) => {
                    return Err(format!("DIMENSION statement for initialized variable `{name}` is not yet supported"));
                }
                _ => {}
            }
        }
        body.retain(|s| !matches!(s, Stmt::VarDecl { names, .. } | Stmt::DerivedDecl { names, .. } if names.is_empty()));
        if kind.is_none() && derived.is_none() {
            if body.iter().any(|s| matches!(s, Stmt::Implicit(Implicit::None))) {
                return Err(format!("DIMENSION entity `{name}` has no type under IMPLICIT NONE"));
            }
            if body.iter().any(|s| matches!(s, Stmt::Implicit(_))) {
                return Err(format!("DIMENSION entity `{name}` with IMPLICIT typing rules is not yet supported"));
            }
            kind = Some(if ('i'..='n').contains(&lower.chars().next().unwrap_or('a')) { TypeSpec::Integer(None) } else { TypeSpec::Real });
        }
        for s in body.iter_mut() {
            if let Stmt::ArrayDecl { kind: k, name: n, dims } = s {
                if marker(k) && *n == name {
                    if let Some(Stmt::DerivedDecl { type_name, names, attributes, polymorphic, .. }) = derived.take() {
                        *s = Stmt::DerivedDecl { type_name, names, attributes, polymorphic, dims: dims.clone() };
                    } else {
                        *k = kind.clone().unwrap();
                    }
                    break;
                }
            }
        }
    }
    for s in body.iter_mut() {
        match s {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } | Stmt::Block { body } => {
                resolve_dimension_statements(body)?
            }
            _ => {}
        }
    }
    Ok(())
}

/// Quiet grammar probe for interactive input; diagnostics are emitted only
/// after a complete submission is selected.
pub(crate) fn probe_fragment(source: &str, end_of_input: usize) -> (bool, bool) {
    let tokens = crate::lexer::lex(source);
    if tokens.iter().any(|t| matches!(t.kind, TokenKind::Error(_))) { return (false, false); }
    let triples = tokens.into_iter().map(|t| (t.span.start, t.kind, t.span.end));
    match fortran::ProgramParser::new().parse(triples) {
        Ok(_) => (true, false),
        Err(lalrpop_util::ParseError::UnrecognizedEof { .. }) => (false, true),
        Err(lalrpop_util::ParseError::UnrecognizedToken { token: (start, _, _), .. })
            if start >= end_of_input => (false, true),
        _ => (false, false),
    }
}

pub fn parse_with_src(
    src: &str,
    tokens: &[Token],
    filename: &str,
) -> Result<Program, Vec<CompileError>> {
    parse_with_origins(src, tokens, filename, None)
}

pub fn parse_with_origins(
    src: &str,
    tokens: &[Token],
    filename: &str,
    origins: Option<&[crate::source_diagnostics::SourceOrigin]>,
) -> Result<Program, Vec<CompileError>> {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use crate::source_diagnostics::{SourceFile as SimpleFile, emit_mapped as emit_to_io_write};
    use codespan_reporting::term::{
        termcolor::{ColorChoice, StandardStream},
        Config,
    };

    let mut errs: Vec<CompileError> = Vec::new();

    let file = SimpleFile::new(filename, src, origins);
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

    // A source file with no statements (only blanks or comments) is valid.
    if tokens.is_empty() {
        return Ok(Program { name: "<anon>".to_string(), body: vec![], contains: vec![] });
    }

    let triples: Vec<(usize, TokenKind, usize)> = tokens
        .iter()
        .map(|t| (t.span.start, t.kind.clone(), t.span.end))
        .collect();

    let parsed: Result<Program, Vec<CompileError>> = match fortran::ProgramParser::new()
        .parse(triples.into_iter())
    {
        Ok(mut p) => {
            if let Err((msg, at)) = resolve_dimension_statements(&mut p.body)
                .and_then(|_| crate::statement_labels::formats(&mut p.body))
                .and_then(|_| crate::data_statements::resolve(&mut p.body))
                .map_err(|msg| (msg, None))
                .and_then(|_| crate::deferred_arrays::resolve(&mut p.body))
                .and_then(|_| crate::deferred_arrays::type_parameter_inquiries(&mut p.body).map_err(|msg| (msg, None))) {
                // Label the statement when the error has a source offset.
                let span = at.map_or(0..0, |p| p..src[p..].find('\n').map_or(src.len(), |n| p + n));
                let mut diag = Diagnostic::error().with_message(&msg);
                if at.is_some() { diag = diag.with_labels(vec![Label::primary((), span.clone())]); }
                let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diag);
                errs.push(CompileError::new(CompileErrorKind::Parse, msg, span));
            }
            Ok(p)
        }
        Err(e) => {
            let (span, msg) = match e {
                lalrpop_util::ParseError::InvalidToken { location } => {
                    ((location)..(location + 1), "invalid token".to_string())
                }
                lalrpop_util::ParseError::UnrecognizedEof { expected, .. } => {
                    let mut human: Vec<String> = expected.iter().map(|s| human_expected(s)).collect();
                    human.dedup();
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
                    let mut human: Vec<String> = expected.iter().map(|s| human_expected(s)).collect();
                    human.dedup();
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
                contains: vec![],
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

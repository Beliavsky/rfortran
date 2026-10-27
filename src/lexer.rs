use logos::Logos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nullify_is_a_contextual_statement() {
        let tokens=lex("integer::nullify\nnullify=3\nnullify(p)\nif (.true.) nullify(p,q)\ncall nullify(x)\nx=nullify(3)");
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwNullify)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(&t.kind,TokenKind::Ident(n) if n=="nullify")).count(),4);
    }

    #[test]
    fn unit_ends_respect_logical_statement_boundaries() {
        let tokens = lex("end subroutine\nx=1\nend function; y=2\nend module ! comment\nz=3\nend & ! continued\n & function &\n & f\nend program main\nend; end if\nend do");
        let names: Vec<_> = tokens.iter().filter_map(|t| match &t.kind {
            TokenKind::EndUnitName(n) => Some(n.as_str()), _ => None,
        }).collect();
        assert_eq!(names, vec!["f", "main"]);
        assert_eq!(tokens.iter().filter(|t| matches!(t.kind, TokenKind::BareEnd)).count(), 1);
        for name in ["x", "y", "z"] {
            assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == name)));
        }
    }

    #[test]
    fn labels_preserve_contextual_statement_keywords() {
        let tokens=lex("10 write(*,*) 7\n20 if (.true.) write(*,*) 8\n30 open(unit=20)\n40 continue\n50 if (.true.) continue\ninteger::continue\ncontinue=1\nend subroutine continue");
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::StatementLabel(_))).count(),5);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwWrite)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::FileControl(_))).count(),1);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwContinue)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(&t.kind,TokenKind::Ident(n) if n=="continue")).count(),2);
        let tokens=lex("a=[1, &\n & 2]\nprint *, &\n & 3");
        assert!(!tokens.iter().any(|t|matches!(t.kind,TokenKind::StatementLabel(_))));
    }

    #[test]
    fn empty_reads_respect_logical_statement_boundaries() {
        for source in ["read(20,*)", "read(*,*); x=1", "read(5,*) ! skip\nx=1",
            "if (.true.) read(iunit,*)", "read(*,*,iostat=ios)\nend",
            "read & ! comment\n & (20,*)"] {
            assert_eq!(lex(source).iter().filter(|t| matches!(t.kind, TokenKind::KwReadEmpty)).count(), 1, "{source}");
        }
        for source in ["read(*,*) n", "read(text,*,iostat=ios) n",
            "read(*,*) & ! comment\n! intervening comment\n & n"] {
            assert!(!lex(source).iter().any(|t| matches!(t.kind, TokenKind::KwReadEmpty)), "{source}");
        }
    }

    #[test]
    fn dimension_is_contextual_to_declaration_attributes() {
        for source in ["integer, dimension(2,3) :: a,b",
            "real(kind=8), intent(in), DIMENSION(:,:) :: a",
            "double precision, & ! comment\n & dimension(2) :: a"] {
            assert_eq!(lex(source).iter().filter(|t| matches!(t.kind, TokenKind::KwDimension)).count(), 1);
        }
        for source in ["integer :: x, dimension(2)", "integer x, dimension(2)", "dimension=2",
            "call dimension(x)", "x=dimension(y)", "call f(x,dimension=y)",
            "integer :: x\ncall f(x,dimension)"] {
            assert!(!lex(source).iter().any(|t| matches!(t.kind, TokenKind::KwDimension)), "{source}");
        }
    }

    #[test]
    fn double_precision_is_a_contextual_phrase() {
        for gap in [" ", " & ! comment\n & "] {
            let tokens = lex(&format!("DoUbLe{gap}PrEcIsIoN :: double, precision"));
            assert!(matches!(tokens[0].kind, TokenKind::KwDouble));
            assert!(matches!(tokens[1].kind, TokenKind::KwPrecision));
            assert!(matches!(&tokens[3].kind, TokenKind::Ident(n) if n == "double"));
            assert!(matches!(&tokens[5].kind, TokenKind::Ident(n) if n == "precision"));
        }
        for gap in ["\n", ";", " ! comment\n"] {
            let tokens = lex(&format!("double{gap}precision=2"));
            assert!(matches!(&tokens[0].kind, TokenKind::Ident(n) if n == "double"));
            assert!(matches!(&tokens[1].kind, TokenKind::Ident(n) if n == "precision"));
        }
        for source in ["real :: double, precision", "double=2", "precision(1)=3", "call double(precision)", "x=double(precision)"] {
            assert!(!lex(source).iter().any(|t| matches!(t.kind, TokenKind::KwDouble | TokenKind::KwPrecision)));
        }
    }

    #[test]
    fn old_constructor_delimiters_preserve_division_and_operator_interfaces() {
        let tokens = lex("a=( / 6/(2), (/3,4/) / )\ninterface operator(/)\nend interface operator(/)");
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::LBracket)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::RBracket)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::Slash)).count(),3);
        assert!(!tokens.iter().any(|t|matches!(t.kind,TokenKind::Error(_))));
    }

    #[test]
    fn else_if_obeys_statement_boundaries() {
        for gap in [" ", " & ! comment\n! intervening comment\n\n & "] {
            let source = format!("else{gap}if (x) then");
            let tokens = lex(&source);
            assert!(matches!(tokens[0].kind, TokenKind::KwElseIf));
            assert_eq!(&source[tokens[0].span.clone()], format!("else{gap}if"));
            assert!(!tokens.iter().any(|t| matches!(t.kind, TokenKind::Error(_))));
        }
        for gap in ["\n", ";", " ! comment\n"] {
            let tokens = lex(&format!("else{gap}if (x) then"));
            assert!(matches!(tokens[0].kind, TokenKind::KwElse));
            assert!(matches!(tokens[1].kind, TokenKind::KwIf));
        }
        for source in ["elseif=1", "elseif(1)=2", "call elseif(x)", "x=elseif(y)"] {
            assert!(!lex(source).iter().any(|t| matches!(t.kind, TokenKind::KwElseIf)));
        }
    }

    #[test]
    fn bare_stop_does_not_consume_the_next_statement() {
        let tokens=lex("stop; x=1\nif (.true.) stop ! done\nx=2\nstop 3\nerror stop 'bad'\nstop");
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwStopEmpty)).count(),3);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwStop)).count(),2);
    }

    #[test]
    fn unbounded_do_obeys_logical_statement_boundaries() {
        for gap in ["\n", ";", " ! comment\r\n", " ! comment\n\n! another comment\n"] {
            let tokens = lex(&format!("Do{gap}i=1\nend do\n"));
            assert!(matches!(tokens[0].kind, TokenKind::KwDoUnbounded));
            assert!(matches!(tokens.last().unwrap().kind, TokenKind::KwDo));
        }
        for source in ["do i=1,3", "do while (.true.)", "do & ! comment\n! comment\n & i=1,3", "do &\n & while (.true.)", "end &\n & do", "end do"] {
            let tokens = lex(source);
            assert!(!tokens.iter().any(|t| matches!(t.kind, TokenKind::KwDoUnbounded)));
        }
    }

    #[test]
    fn spaced_inout_is_contextual_and_preserves_its_span() {
        for gap in [" ", "\t", " & ! comment\n! intervening comment\n & "] {
            let source = format!("integer, InTeNt(In{gap}OuT) :: x");
            let tokens = lex(&source);
            let inout = tokens.iter().find(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "inout")).unwrap();
            assert_eq!(&source[inout.span.clone()], format!("In{gap}OuT"));
        }
        for source in [
            "integer :: in, out", "in=out", "call f(in,out)",
            "integer, intent(in) :: x", "integer, intent(out) :: x",
            "integer, intent(in\nout) :: x", "integer, intent(in;out) :: x",
            "integer, intent(in,out) :: x", "integer, other(in out) :: x",
        ] {
            assert!(!lex(source).iter().any(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "inout")), "{source}");
        }
    }

    #[test]
    fn write_is_contextual_and_empty_lists_end_at_statement_boundaries() {
        let source="integer :: write(2,2)\nwrite(1,2)=3\ncall write(1,2)\nx=write(1,2)\nwrite(*,*) write(1,2)\nif (.true.) write(6,*) 7\nwrite(*,*); x=8\nwrite(*,*) ! blank\nx=9\nwrite(*,*) &\n & 10\n";
        let tokens=lex(source);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwWrite)).count(),3);
        assert_eq!(tokens.iter().filter(|t|matches!(t.kind,TokenKind::KwWriteEmpty)).count(),2);
        assert_eq!(tokens.iter().filter(|t|matches!(&t.kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("write"))).count(),5);
    }

    #[test]
    fn default_is_a_keyword_only_after_case() {
        let source = "integer :: default\ndefault=2\nselect case(default)\ncase(1); default=3\nCaSe &\n & DeFaUlT; default=4\nend select\nuse m, only: default\n";
        let tokens = lex(source);
        let defaults: Vec<_> = tokens.iter().filter(|t| source[t.span.clone()].eq_ignore_ascii_case("default")).collect();
        assert_eq!(defaults.len(), 7);
        assert_eq!(defaults.iter().filter(|t| matches!(t.kind, TokenKind::KwDefault)).count(), 1);
        assert!(defaults.iter().filter(|t| !matches!(t.kind, TokenKind::KwDefault)).all(|t| matches!(&t.kind, TokenKind::Ident(n) if n == "default")));
    }

    #[test]
    fn fortran_character_literals_preserve_backslashes_and_double_quotes() {
        let source = r##"print *, "\", '\', "C:\new\temp\", "a""b", 'c''d', '', "", ";!&"
print *, 7"##;
        let tokens = lex(source);
        assert!(!tokens.iter().any(|t| matches!(t.kind, TokenKind::Error(_))));
        let strings: Vec<_> = tokens.iter().filter_map(|t| if let TokenKind::Str(s) = &t.kind { Some(s.as_str()) } else { None }).collect();
        assert_eq!(strings, ["\\", "\\", "C:\\new\\temp\\", "a\"b", "c'd", "", "", ";!&"]);
        assert!(matches!(&tokens.last().unwrap().kind, TokenKind::Integer(n) if n=="7"));
        for t in tokens.iter().filter(|t| matches!(t.kind, TokenKind::Str(_))) {
            let original = &source[t.span.clone()];
            assert_eq!(original.as_bytes().first(), original.as_bytes().last());
        }
    }

    #[test]
    fn separators_and_comments_preserve_following_statement_spans() {
        let source = "i=1; i=2; ! ; not a statement\nprint *, i";
        let tokens = lex(source);
        assert!(!tokens.iter().any(|t| matches!(t.kind, TokenKind::Error(_))));
        let print = tokens.iter().find(|t| matches!(t.kind, TokenKind::KwPrint)).unwrap();
        assert_eq!(print.span.start, source.find("print").unwrap());
        assert_eq!(tokens.iter().filter(|t| matches!(&t.kind,TokenKind::Ident(n) if n=="i")).count(),3);
    }

    #[test]
    fn unterminated_literal_does_not_swallow_following_lines() {
        let tokens = lex("print *, 'unterminated\nprint *, 'next'\n");
        assert!(tokens.iter().any(|t| matches!(t.kind, TokenKind::Error(_))));
        assert!(tokens.iter().any(|t| matches!(&t.kind, TokenKind::Str(s) if s=="next")));
    }

    #[test]
    fn defined_operators_are_tokens_without_hiding_intrinsic_operators() {
        let tokens = lex(".in. .NOTIN. .exclude. .true. .false. .and. .or. .not. .eq. .eqv.");
        let kinds: Vec<_> = tokens.into_iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::DefinedOperator(".in.".into()),
            TokenKind::DefinedOperator(".notin.".into()), TokenKind::DefinedOperator(".exclude.".into()),
            TokenKind::True, TokenKind::False, TokenKind::And, TokenKind::Or,
            TokenKind::Not, TokenKind::EqEq, TokenKind::Eqv]);
    }
}

/// Remove each `&`, line break, optional `&` sequence from a continued
/// CHARACTER literal; an `&` not at the end of a line is an ordinary character.
fn join_continued_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let tail = after.trim_start_matches([' ', '\t']);
        let newline = if tail.starts_with("\r\n") { 2 } else if tail.starts_with('\n') { 1 } else { 0 };
        if newline == 0 {
            out.push('&');
            rest = after;
            continue;
        }
        let mut next = &tail[newline..];
        // Skip blank and comment lines before the continuation line.
        loop {
            let line = next.trim_start_matches([' ', '\t']);
            if !(line.starts_with('!') || line.starts_with('\n') || line.starts_with("\r\n")) { break; }
            match line.find('\n') { Some(end) => next = &line[end + 1..], None => break }
        }
        let line = next.trim_start_matches([' ', '\t']);
        rest = line.strip_prefix('&').unwrap_or(line);
    }
    out.push_str(rest);
    out
}

#[derive(Logos, Debug, Clone, PartialEq)]
pub enum TokenKind {
    #[regex(r"[ \t\r\n;]+", logos::skip)]
    _WS,
    #[regex(r"![^\n]*", logos::skip)]
    _COMMENT,
    #[regex(r"&[ \t]*(?:![^\n]*)?\r?\n", continuation)]
    _Continuation,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    KwError,
    KwAllocate,
    KwDeallocate,
    KwNullify,
    KwDimension,
    /// A SAVE statement without a list, which saves every local variable.
    KwSaveAll,
    FileControl(String),
    FilePosition(String),
    KwType,
    KwClass,
    KwTypeof,
    KwClassof,
    KwProcedure,
    KwInterface,
    EndInterfaceName(String),
    EndUnitName(String),
    BareEnd,
    KwOperator,
    KwKind,
    KwOnly,
    // INTENT, OPTIONAL, POINTER, TARGET, ALLOCATABLE, EXTERNAL or INTRINSIC
    // beginning an attribute specification statement.
    AttrStatement(String),
    // A whole procedure declaration: an unnamed or ABSTRACT interface block,
    // or a PROCEDURE(interface) statement, encoded as `form|entities`.
    ProcedureDeclaration(String),
    BindSpec(String),
    #[token("%")]
    Percent,
    #[token("=>")]
    Arrow,
    // Fortran names are case-insensitive, so they are kept in lower case.
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_ascii_lowercase())]
    Ident(String),
    // BOZ literal constants; lex() replaces each with an INTEGER literal.
    #[regex(r#"[bB]('[01]+'|"[01]+")|[oO]('[0-7]+'|"[0-7]+")|[zZ]('[0-9A-Fa-f]+'|"[0-9A-Fa-f]+")"#, |lex| lex.slice().to_string())]
    Boz(String),
    #[regex(r"\.[A-Za-z]+\.", |lex| lex.slice().to_ascii_lowercase())]
    DefinedOperator(String),
    // Fortran doubles the delimiter inside a character literal. Backslashes
    // are ordinary characters, including immediately before the closing quote.
    // A literal continued with `&` at the end of a line resumes after the `&`
    // that begins the next line, or at its first nonblank character as gfortran does.
    // Blank and comment lines may come between the continued lines. The
    // lexer does not backtrack, so each run of blanks has one place to match.
    #[regex(r#""([^"\r\n&]|""|&|&[ \t]*\r?\n[ \t]*((![^\r\n]*)?\r?\n[ \t]*)*&?)*""#, |lex| {
        let s = lex.slice();
        join_continued_literal(&s[1..s.len()-1]).replace("\"\"", "\"")
    })]
    #[regex(r"'([^'\r\n&]|''|&|&[ \t]*\r?\n[ \t]*((![^\r\n]*)?\r?\n[ \t]*)*&?)*'", |lex| {
        let s = lex.slice();
        join_continued_literal(&s[1..s.len()-1]).replace("''", "'")
    })]
    Str(String),
    #[regex(r"(([0-9]+\.[0-9]*|\.[0-9]+)([eEdD][+-]?[0-9]+)?|[0-9]+[eEdD][+-]?[0-9]+)(_[A-Za-z0-9_]+)?", |lex| lex.slice().to_ascii_lowercase())]
    Float(String),
    #[regex(r"[0-9]+(_[A-Za-z0-9_]+)?", |lex| lex.slice().to_ascii_lowercase())]
    Integer(String),
    StatementLabel(String),
    /// The parenthesized specification of a FORMAT statement, as written,
    /// with continuation lines joined.
    FormatSpec(String),
    KwData,
    KwEquivalence,
    KwGoto,
    KwContinue,
    #[token(".true.", ignore(ascii_case))]
    True,
    #[token(".false.", ignore(ascii_case))]
    False,
    #[token("program", ignore(ascii_case))]
    KwProgram,
    #[token("end", ignore(ascii_case))]
    KwEnd,
    #[token("function", ignore(ascii_case))]
    KwFunction,
    // Contextual keyword, assigned only after a function's parameter list.
    KwResult,
    KwPure,
    KwSimple,
    KwReduce,
    KwEnumeration,
    KwEnum,
    KwEnumerator,
    KwImpure,
    KwRecursive,
    KwElemental,
    KwForall,
    KwParameter,
    KwWhere,
    KwWhereBlock,
    KwElseWhere,
    KwEndWhere,
    KwWrite,
    KwWriteEmpty,
    #[token("public", ignore(ascii_case))]
    KwPublic,
    #[token("private", ignore(ascii_case))]
    KwPrivate,
    #[token("return", ignore(ascii_case))]
    KwReturn,
    #[token("implicit", ignore(ascii_case))]
    KwImplicit,
    #[token("none", ignore(ascii_case))]
    KwNone,
    #[token("print", ignore(ascii_case))]
    KwPrint,
    KwPrintEmpty,
    #[token("read", ignore(ascii_case))]
    KwRead,
    KwReadEmpty,
    #[token("if", ignore(ascii_case))]
    KwIf,
    #[token("then", ignore(ascii_case))]
    KwThen,
    #[token("else", ignore(ascii_case))]
    KwElse,
    KwElseIf,
    #[token("do", ignore(ascii_case))]
    KwDo,
    KwDoUnbounded,
    #[token("enddo", ignore(ascii_case))]
    KwEndDo,
    ConstructName(String),
    EndLoopName(String),
    NamedExit(String),
    NamedCycle(String),
    #[token("while", ignore(ascii_case))]
    KwWhile,
    #[token("integer", ignore(ascii_case))]
    KwInteger,
    #[token("real", ignore(ascii_case))]
    KwReal,
    #[token("complex", ignore(ascii_case))]
    KwComplex,
    ComplexOpen,
    #[token("double", ignore(ascii_case))]
    KwDouble,
    #[token("precision", ignore(ascii_case))]
    KwPrecision,
    #[token("character", ignore(ascii_case))]
    KwCharacter,
    #[token("logical", ignore(ascii_case))]
    KwLogical,
    KwLen,
    #[token("subroutine", ignore(ascii_case))]
    KwSubroutine,
    #[token("call", ignore(ascii_case))]
    KwCall,
    #[token("contains", ignore(ascii_case))]
    KwContains,
    #[token("use", ignore(ascii_case))]
    KwUse,
    #[token("module", ignore(ascii_case))]
    KwModule,
    #[token("select", ignore(ascii_case))]
    KwSelect,
    #[token("case", ignore(ascii_case))]
    KwCase,
    KwRank,
    KwTypeIs,
    KwClassIs,
    KwClassDefault,
    // Contextual keyword in CASE DEFAULT; otherwise a valid identifier.
    KwDefault,
    #[token("block", ignore(ascii_case))]
    KwBlock,
    // Contextual: ASSOCIATE ( ... ) and END ASSOCIATE.
    KwAssociate,
    #[token("exit", ignore(ascii_case))]
    KwExit,
    #[token("cycle", ignore(ascii_case))]
    KwCycle,
    #[token("stop", ignore(ascii_case))]
    KwStop,
    KwStopEmpty,
    #[token("**")]
    Pow,
    #[token("//")]
    Concat,
    #[token("==")]
    #[token(".eq.", ignore(ascii_case))]
    EqEq,
    #[token("/=")]
    #[token(".ne.", ignore(ascii_case))]
    Ne,
    #[token("<=")]
    #[token(".le.", ignore(ascii_case))]
    Le,
    #[token(">=")]
    #[token(".ge.", ignore(ascii_case))]
    Ge,
    #[token("<")]
    #[token(".lt.", ignore(ascii_case))]
    Lt,
    #[token(">")]
    #[token(".gt.", ignore(ascii_case))]
    Gt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("/")]
    Slash,
    #[token("*")]
    Star,
    #[token("..")]
    DotDot,
    #[token("::")]
    DColon,
    #[token(":")]
    Colon,
    #[token("@")]
    At,
    #[token("?")]
    Question,
    #[token(".nil.", ignore(ascii_case))]
    Nil,
    #[token("=")]
    Eq,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(",")]
    Comma,
    #[token(".and.", ignore(ascii_case))]
    And,
    #[token(".or.", ignore(ascii_case))]
    Or,
    #[token(".not.", ignore(ascii_case))]
    Not,
    #[token(".eqv.", ignore(ascii_case))]
    Eqv,
    #[token(".neqv.", ignore(ascii_case))]
    Neqv,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: std::ops::Range<usize>,
}

// After a trailing &, blank/comment lines may precede the optional leading &.
fn continuation(lexer: &mut logos::Lexer<TokenKind>) -> logos::Skip {
    let bytes = lexer.remainder().as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        match bytes[end] {
            b' ' | b'\t' | b'\r' | b'\n' => end += 1,
            b'!' => { while end < bytes.len() && bytes[end] != b'\n' { end += 1; } }
            b'&' => { end += 1; break; }
            _ => break,
        }
    }
    lexer.bump(end);
    logos::Skip
}

// END INTERFACE's optional name must be in the same logical statement.
pub(crate) fn same_statement(gap: &str) -> bool {
    let mut continued = false;
    for line in gap.split_inclusive('\n') {
        let code = line.split('!').next().unwrap_or("").trim();
        if code.contains(';') { return false; }
        if !code.is_empty() { continued = code.ends_with('&'); }
        if line.ends_with('\n') && !continued { return false; }
    }
    true
}

// True when tokens[index] begins a logical statement (after an optional label).
// Whether the token at `index` follows a comma directly inside the parentheses
// of a CHARACTER type selector that start with LEN= or KIND=.
fn character_selector(tokens: &[Token], index: usize) -> bool {
    let mut depth = 0usize;
    for i in (0..index).rev() {
        match tokens[i].kind {
            TokenKind::RParen => depth += 1,
            TokenKind::LParen if depth > 0 => depth -= 1,
            TokenKind::LParen => {
                return i >= 1 && matches!(tokens[i - 1].kind, TokenKind::KwCharacter)
                    && matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::KwLen | TokenKind::KwKind));
            }
            _ => {}
        }
    }
    false
}

/// Whether the name at `index` is assigned: `=` follows its parenthesized
/// subscripts in the same statement.
fn assigned(tokens: &[Token], input: &str, index: usize) -> bool {
    let mut depth = 0;
    for i in index + 1..tokens.len() {
        if !same_statement(&input[tokens[i - 1].span.end..tokens[i].span.start]) {
            return false;
        }
        match tokens[i].kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    return tokens.get(i + 1).is_some_and(|t| matches!(t.kind, TokenKind::Eq)
                        && same_statement(&input[tokens[i].span.end..t.span.start]));
                }
            }
            _ => {}
        }
    }
    false
}

fn statement_head(tokens: &[Token], input: &str, index: usize) -> bool {
    index == 0 || !same_statement(&input[tokens[index-1].span.end..tokens[index].span.start])
        || matches!(tokens[index-1].kind, TokenKind::StatementLabel(_))
            && (index == 1 || !same_statement(&input[tokens[index-2].span.end..tokens[index-1].span.start]))
}

/// Collapse each unnamed or ABSTRACT interface block, and each
/// `PROCEDURE(interface) [, attributes] :: names` statement, into a single
/// ProcedureDeclaration token. Interface blocks are encoded as
/// `interface|name=function,...` or `abstract|...`; procedure statements as
/// `procedure(iface),attribute...|name,...` (initializers such as `=> null()`
/// are dropped).
fn procedure_declarations(tokens: &mut Vec<Token>, input: &str) {
    fn word(token: Option<&Token>) -> Option<String> {
        match token.map(|t| &t.kind) {
            Some(TokenKind::Ident(n)) => Some(n.to_ascii_lowercase()),
            Some(TokenKind::KwInterface) => Some("interface".into()),
            _ => None,
        }
    }
    let ends_statement = |tokens: &[Token], index: usize| {
        tokens.get(index + 1).is_none_or(|t| !same_statement(&input[tokens[index].span.end..t.span.start]))
    };
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        let head = statement_head(tokens, input, index);
        let abstract_head = head && word(tokens.get(index)).as_deref() == Some("abstract")
            && word(tokens.get(index + 1)).as_deref() == Some("interface")
            && same_statement(&input[tokens[index].span.end..tokens[index + 1].span.start])
            && ends_statement(tokens, index + 1);
        let interface_head = head && (word(tokens.get(index)).as_deref() == Some("interface")
            || matches!(tokens[index].kind, TokenKind::KwInterface)) && ends_statement(tokens, index);
        if abstract_head || interface_head {
            let mut depth = 1;
            let mut entities = vec![];
            let mut cursor = index + usize::from(abstract_head) + 1;
            let mut closed = None;
            while cursor < tokens.len() {
                let at_head = statement_head(tokens, input, cursor);
                match &tokens[cursor].kind {
                    TokenKind::KwEnd if matches!(tokens.get(cursor + 1).map(|t| &t.kind), Some(TokenKind::KwInterface)) => {
                        depth -= 1;
                        if depth == 0 {
                            let mut last = cursor + 1;
                            if matches!(tokens.get(last + 1).map(|t| &t.kind), Some(TokenKind::EndInterfaceName(_))) { last += 1; }
                            closed = Some(last);
                            break;
                        }
                        cursor += 1;
                    }
                    TokenKind::Ident(n) if at_head && n.eq_ignore_ascii_case("interface") => depth += 1,
                    TokenKind::KwInterface if at_head => depth += 1,
                    kind @ (TokenKind::KwFunction | TokenKind::KwSubroutine) if depth == 1
                        && !matches!(cursor.checked_sub(1).map(|i| &tokens[i].kind), Some(TokenKind::KwEnd)) => {
                        if let Some(name) = word(tokens.get(cursor + 1)) {
                            let form = if matches!(kind, TokenKind::KwFunction) { "function" } else { "subroutine" };
                            let mut begin = cursor;
                            while begin > index && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) { begin -= 1; }
                            let simple = tokens[begin..cursor].iter().any(|t| matches!(&t.kind, TokenKind::KwSimple) || matches!(&t.kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("simple")));
                            let modifiers: Vec<_> = tokens[begin..cursor].iter().filter_map(|t| match t.kind {
                                TokenKind::KwSimple => Some((None,false,true)),
                                TokenKind::KwPure => Some((Some(true),false,false)),
                                TokenKind::KwImpure => Some((Some(false),false,false)),
                                TokenKind::KwElemental => Some((None,true,false)),
                                _ => None,
                            }).collect();
                            if let Err(error) = crate::simple::prefix(&modifiers) {
                                tokens[begin].kind = TokenKind::Error(error.into());
                                return;
                            }
                            entities.push(format!("{name}={form}{}", if simple { "+simple" } else { "" }));
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            if let Some(last) = closed {
                let form = if abstract_head { "abstract" } else { "interface" };
                out.push(Token {
                    kind: TokenKind::ProcedureDeclaration(if tokens[index..=last].iter().any(|t|matches!(t.kind,TokenKind::BindSpec(_))) {
                        let begin=index+usize::from(abstract_head)+1;
                        format!("c_interface|{}|{}", tokens[begin].span.start, &input[tokens[begin].span.start..tokens[cursor].span.start])
                    } else if abstract_head {
                        let begin=index+1+1;
                        format!("abstract_interface|{}|{}",tokens[begin].span.start,&input[tokens[begin].span.start..tokens[cursor].span.start])
                    } else {format!("{form}|{}", entities.join(","))}),
                    span: tokens[index].span.start..tokens[last].span.end,
                });
                index = last + 1;
                continue;
            }
        }
        if head && word(tokens.get(index)).as_deref() == Some("procedure")
            && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokenKind::LParen)) {
            if let Some((declaration, last)) = procedure_statement(tokens, input, index) {
                if declaration=="@deferred_binding" {
                    tokens[index].kind=TokenKind::KwProcedure;
                    out.push(tokens[index].clone());index+=1;continue;
                }
                out.push(Token {
                    kind: TokenKind::ProcedureDeclaration(declaration),
                    span: tokens[index].span.start..tokens[last].span.end,
                });
                index = last + 1;
                continue;
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    *tokens = out;
}

/// PROCEDURE ( [interface] ) { , attribute } :: entity [=> initializer] { , ... }
fn procedure_statement(tokens: &[Token], input: &str, index: usize) -> Option<(String, usize)> {
    let kind = |i: usize| tokens.get(i).map(|t| &t.kind);
    let word = |i: usize| match kind(i) {
        Some(TokenKind::Ident(n)) => Some(n.to_ascii_lowercase()),
        _ => None,
    };
    let mut cursor = index + 2;
    let interface = word(cursor).unwrap_or_default();
    if !interface.is_empty() { cursor += 1; }
    if !matches!(kind(cursor), Some(TokenKind::RParen)) { return None; }
    cursor += 1;
    let mut form = vec![format!("procedure({interface})")];
    while matches!(kind(cursor), Some(TokenKind::Comma)) {
        let mut attribute = word(cursor + 1)?;
        cursor += 2;
        if matches!(kind(cursor), Some(TokenKind::LParen)) {
            // e.g. PASS(self) or INTENT(IN)
            let start = cursor;
            let mut depth = 0;
            while cursor < tokens.len() {
                match tokens[cursor].kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
                cursor += 1;
                if depth == 0 { break; }
            }
            attribute.push_str(&input[tokens[start].span.start..tokens[cursor - 1].span.end].to_ascii_lowercase().replace(' ', ""));
        }
        if attribute=="deferred" {return Some(("@deferred_binding".into(),index));}
        form.push(attribute);
    }
    if !matches!(kind(cursor), Some(TokenKind::DColon)) { return None; }
    let mut names = vec![];
    let mut last = cursor;
    cursor += 1;
    while cursor < tokens.len() && same_statement(&input[tokens[cursor - 1].span.end..tokens[cursor].span.start]) {
        match &tokens[cursor].kind {
            TokenKind::Ident(n) => names.push(n.to_ascii_lowercase()),
            TokenKind::Comma => {}
            TokenKind::Arrow => {
                // Skip the initializer up to the next entity.
                let mut depth = 0;
                while cursor + 1 < tokens.len() && same_statement(&input[tokens[cursor].span.end..tokens[cursor + 1].span.start]) {
                    match tokens[cursor + 1].kind {
                        TokenKind::LParen => depth += 1,
                        TokenKind::RParen => depth -= 1,
                        TokenKind::Comma if depth == 0 => break,
                        _ => {}
                    }
                    cursor += 1;
                }
            }
            _ => return None,
        }
        last = cursor;
        cursor += 1;
    }
    (!names.is_empty()).then(|| (format!("{}|{}", form.join(","), names.join(",")), last))
}

/// The value of a BOZ literal such as `b'0101'`, `o"17"` or `z'ff'`, as an
/// INTEGER literal: the bits as a default INTEGER when they fit in 32 bits,
/// otherwise as INTEGER(8). The result is negative when the sign bit is set.
fn boz_value(text: &str) -> Option<(bool, String)> {
    let radix = match text.as_bytes()[0].to_ascii_lowercase() { b'b' => 2, b'o' => 8, _ => 16 };
    let value = u64::from_str_radix(&text[2..text.len()-1], radix).ok()?;
    Some(if value <= u32::MAX as u64 {
        let value = value as u32 as i32;
        (value < 0, value.unsigned_abs().to_string())
    } else {
        let value = value as i64;
        (value < 0, format!("{}_8", value.unsigned_abs()))
    })
}

// A word followed by `.`, such as AND in `1.and.`: the `.` ends a relational
// or logical operator, not a REAL literal.
fn operator_after_number(rest: &str) -> bool {
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    !word.is_empty() && rest[word.len()..].starts_with('.')
}

/// Join the lines of a continued statement, dropping each `&`, the line
/// break, comment lines and the leading `&` of the next line. Character
/// context, where `!` is not a comment, is tracked across lines.
fn join_continued(text: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' if quote.is_none() => { quote = Some(c); out.push(c); }
            c if Some(c) == quote => { quote = None; out.push(c); }
            '!' if quote.is_none() => { while chars.peek().is_some_and(|c| *c != '\n') { chars.next(); } }
            '&' => {
                // Skip to the next line that has code, then past its `&`.
                let rest: String = chars.clone().collect();
                if !rest.split('\n').next().unwrap_or("").trim().is_empty() && !rest.split('\n').next().unwrap_or("").trim_start().starts_with('!') {
                    out.push(c);
                    continue;
                }
                loop {
                    while chars.peek().is_some_and(|c| *c != '\n') { chars.next(); }
                    if chars.next().is_none() { break; }
                    let line: String = chars.clone().take_while(|c| *c != '\n').collect();
                    let code = line.trim_start();
                    if code.is_empty() || code.starts_with('!') { continue; }
                    while chars.peek().is_some_and(|c| *c == ' ' || *c == '\t') { chars.next(); }
                    if chars.peek() == Some(&'&') { chars.next(); }
                    break;
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// `DO label [,] i = ...` ends after the statement with that label, as if
/// END DO followed it. Drop the label and insert that END DO, once for each
/// loop that shares the terminal statement.
fn labeled_do(tokens: &mut Vec<Token>, input: &str) {
    let normal = |n: &str| n.trim_start_matches('0').to_owned();
    let mut pending: Vec<String> = vec![];
    let mut i = 0;
    while i < tokens.len() {
        if matches!(tokens[i].kind, TokenKind::KwDo) {
            if let Some(TokenKind::Integer(label)) = tokens.get(i + 1).filter(|t| same_statement(&input[tokens[i].span.end..t.span.start])).map(|t| &t.kind) {
                if label.bytes().all(|c| c.is_ascii_digit()) {
                    pending.push(normal(label));
                    tokens.remove(i + 1);
                    if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(TokenKind::Comma)) {
                        tokens.remove(i + 1);
                    }
                }
            }
        }
        if let TokenKind::StatementLabel(label) = &tokens[i].kind {
            let label = normal(label);
            let count = pending.iter().filter(|l| **l == label).count();
            if count > 0 {
                pending.retain(|l| *l != label);
                let mut end = i;
                while end + 1 < tokens.len() && same_statement(&input[tokens[end].span.end..tokens[end + 1].span.start]) {
                    end += 1;
                }
                let at = tokens[end].span.end;
                for _ in 0..count {
                    tokens.insert(end + 1, Token { kind: TokenKind::KwEndDo, span: at..at });
                }
                i = end + count;
            }
        }
        i += 1;
    }
}

/// Replace `label FORMAT (spec)` with the label and one FormatSpec token,
/// since a format specification is not made of Fortran expressions.
fn format_statements(tokens: &mut Vec<Token>, input: &str) {
    let mut i = 0;
    while i + 2 < tokens.len() {
        let format = matches!(tokens[i].kind, TokenKind::StatementLabel(_))
            && matches!(&tokens[i + 1].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("format"))
            && matches!(tokens[i + 2].kind, TokenKind::LParen);
        if !format { i += 1; continue; }
        let mut end = i + 2;
        while end + 1 < tokens.len() && same_statement(&input[tokens[end].span.end..tokens[end + 1].span.start]) {
            end += 1;
        }
        let span = tokens[i + 1].span.start..tokens[end].span.end;
        let spec = join_continued(&input[tokens[i + 2].span.start..tokens[end].span.end]);
        tokens.splice(i + 1..=end, [Token { kind: TokenKind::FormatSpec(spec), span }]);
        i += 2;
    }
}

pub fn lex(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    'restart: loop {
        let mut lexer = TokenKind::lexer(&input[offset..]);
        while let Some(res) = lexer.next() {
            let span = lexer.span().start + offset..lexer.span().end + offset;
            match res {
                Ok(kind) => {
                    if matches!(kind, TokenKind::_WS | TokenKind::_COMMENT) {
                        continue;
                    }
                    // `97.and.x` is 97 .AND. x: restart lexing at the operator's `.`.
                    if let TokenKind::Float(text) = &kind {
                        if text.ends_with('.') && operator_after_number(&input[span.end..]) {
                            tokens.push(Token { kind: TokenKind::Integer(text[..text.len()-1].into()), span: span.start..span.end-1 });
                            offset = span.end - 1;
                            continue 'restart;
                        }
                    }
                    if let TokenKind::Boz(text) = &kind {
                        let Some((negative, value)) = boz_value(text) else {
                            tokens.push(Token { kind: TokenKind::Error(text.clone()), span });
                            continue;
                        };
                        if negative {
                            tokens.push(Token { kind: TokenKind::Minus, span: span.start..span.start });
                        }
                        tokens.push(Token { kind: TokenKind::Integer(value), span });
                        continue;
                    }
                    tokens.push(Token { kind, span });
                }
                Err(_) => {
                    let text = input.get(span.clone()).unwrap_or("").to_string();
                    tokens.push(Token {
                        kind: TokenKind::Error(text),
                        span,
                    });
                }
            }
        }
        break;
    }
    // A name or number may be split across lines by `&` at the end of one
    // line and at the start of the next, with nothing else between.
    let mut i = 0;
    while i + 1 < tokens.len() {
        let gap = &input[tokens[i].span.end..tokens[i + 1].span.start];
        let joined = gap.len() > 1 && gap.starts_with('&') && gap.ends_with('&') && {
            let inner = &gap[1..gap.len() - 1];
            inner.contains('\n') && inner.lines().all(|l| l.trim().is_empty() || l.trim_start().starts_with('!'))
        };
        // The pieces form one token when their joined text lexes as one.
        let text = joined.then(|| format!("{}{}", &input[tokens[i].span.clone()], &input[tokens[i + 1].span.clone()]))
            .and_then(|text| {
                let mut lexer = TokenKind::lexer(&text);
                let first = lexer.next()?.ok()?;
                (lexer.span().end == text.len() && !matches!(first, TokenKind::Str(_))).then_some(first)
            });
        if let (true, Some(kind)) = (joined, text) {
            let span = tokens[i].span.start..tokens[i + 1].span.end;
            tokens.splice(i..=i + 1, [Token { kind, span }]);
            continue;
        }
        i += 1;
    }
    // ENDIF, ENDSELECT, SELECTCASE, DOUBLEPRECISION and the like are the
    // two-word forms written without a blank. Split them at the head of a
    // statement, unless the word is a variable being assigned or referenced.
    let mut index = 0;
    while index < tokens.len() {
        let word = match &tokens[index].kind { TokenKind::Ident(n) => n.to_ascii_lowercase(), _ => { index += 1; continue } };
        let split = match word.as_str() {
            "endif" | "endselect" | "endblock" | "endassociate" | "endfunction" | "endsubroutine"
                | "endprogram" | "endmodule" | "endinterface" | "endtype" | "endforall" => Some(3),
            "selectcase" => Some(6),
            "doubleprecision" => Some(6),
            _ => None,
        };
        let following = tokens.get(index + 1).filter(|t| same_statement(&input[tokens[index].span.end..t.span.start]));
        let head = statement_head(&tokens, input, index)
            || index > 0 && matches!(&tokens[index-1].kind, TokenKind::Ident(n)
                if matches!(n.to_ascii_lowercase().as_str(), "pure" | "simple" | "impure" | "elemental" | "recursive" | "module"))
            || word == "doubleprecision" && index > 0 && matches!(tokens[index-1].kind, TokenKind::KwImplicit | TokenKind::Comma)
                && matches!(tokens[(0..index).rev().find(|&i| statement_head(&tokens, input, i)).unwrap_or(0)].kind, TokenKind::KwImplicit);
        let reference = matches!(following.map(|t| &t.kind), Some(TokenKind::Eq | TokenKind::Percent))
            || word != "selectcase" && word != "doubleprecision" && matches!(following.map(|t| &t.kind), Some(TokenKind::LParen));
        if let (Some(at), true, false) = (split, head, reference) {
            let span = tokens[index].span.clone();
            let second = &input[span.start + at..span.end];
            let kind = TokenKind::lexer(second).next().and_then(|r| r.ok())
                .unwrap_or_else(|| TokenKind::Ident(second.to_ascii_lowercase()));
            let first = TokenKind::lexer(&input[span.start..span.start + at]).next().and_then(|r| r.ok()).unwrap();
            tokens[index] = Token { kind: first, span: span.start..span.start + at };
            tokens.insert(index + 1, Token { kind, span: span.start + at..span.end });
            index += 1;
        }
        index += 1;
    }
    // Fortran has no reserved procedure names: a keyword such as PRINT can
    // name a procedure in a header, CALL, or END FUNCTION/SUBROUTINE name.
    for i in 1..tokens.len() {
        if matches!(tokens[i-1].kind, TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwCall)
            && same_statement(&input[tokens[i-1].span.end..tokens[i].span.start]) {
            let word = &input[tokens[i].span.clone()];
            if word.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && word.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                tokens[i].kind = TokenKind::Ident(word.to_ascii_lowercase());
            }
        }
    }
    // IF is a keyword before its condition and after END or ELSE; elsewhere,
    // as in `IF = I - 1` or `A(IF+1)`, it names a variable.
    for i in 0..tokens.len() {
        if !matches!(tokens[i].kind, TokenKind::KwIf) {
            continue;
        }
        let joined = |a: usize, b: usize| same_statement(&input[tokens[a].span.end..tokens[b].span.start]);
        let condition = tokens.get(i + 1).is_some_and(|t| matches!(t.kind, TokenKind::LParen)) && joined(i, i + 1);
        let closing = i > 0 && matches!(tokens[i - 1].kind, TokenKind::KwEnd | TokenKind::KwElse) && joined(i - 1, i);
        if !condition && !closing {
            tokens[i].kind = TokenKind::Ident("if".into());
        }
    }
    // A leading decimal number belongs to the following statement, rather
    // than to an expression. Keep it distinct for grammar and scope checks.
    for i in 0..tokens.len() {
        if (i == 0 || !same_statement(&input[tokens[i-1].span.end..tokens[i].span.start]))
            && tokens.get(i+1).is_some_and(|next| same_statement(&input[tokens[i].span.end..next.span.start])) {
            if let TokenKind::Integer(number) = &tokens[i].kind {
                if number.bytes().all(|c| c.is_ascii_digit()) {
                    tokens[i].kind = TokenKind::StatementLabel(number.clone());
                }
            }
        }
    }
    format_statements(&mut tokens, input);
    // GOTO label and GO TO label; no other statement has a name followed by
    // an integer.
    let mut i = 0;
    while i + 1 < tokens.len() {
        let same = |a: usize, b: usize, tokens: &Vec<Token>| same_statement(&input[tokens[a].span.end..tokens[b].span.start]);
        let word = |t: &Token, w: &str| matches!(&t.kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case(w));
        // A computed GO TO starts a statement with a parenthesized label list.
        let target = |j: usize, tokens: &Vec<Token>| matches!(tokens[j].kind, TokenKind::Integer(_))
            || matches!(tokens[j].kind, TokenKind::LParen) && statement_head(tokens, input, i)
                && tokens.get(j + 1).is_some_and(|t| matches!(t.kind, TokenKind::Integer(_)));
        if word(&tokens[i], "goto") && target(i + 1, &tokens) && same(i, i + 1, &tokens) {
            tokens[i].kind = TokenKind::KwGoto;
        } else if i + 2 < tokens.len() && word(&tokens[i], "go") && word(&tokens[i + 1], "to")
            && target(i + 2, &tokens) && same(i, i + 1, &tokens) && same(i + 1, i + 2, &tokens) {
            let span = tokens[i].span.start..tokens[i + 1].span.end;
            tokens.splice(i..=i + 1, [Token { kind: TokenKind::KwGoto, span }]);
        }
        i += 1;
    }
    // SELECT and CASE are keywords only where a SELECT CASE construct or a
    // CASE statement can be; elsewhere they are names, as `integer :: case`.
    for i in 0..tokens.len() {
        if !matches!(tokens[i].kind, TokenKind::KwCase | TokenKind::KwSelect) { continue; }
        let head = statement_head(&tokens, input, i)
            // `name: select case`; construct names are recognized later.
            || i >= 2 && matches!(tokens[i-1].kind, TokenKind::Colon)
                && matches!(tokens[i-2].kind, TokenKind::Ident(_) | TokenKind::ConstructName(_)) && statement_head(&tokens, input, i-2);
        let next = tokens.get(i+1).filter(|t| same_statement(&input[tokens[i].span.end..t.span.start])).map(|t| &t.kind);
        let assigned = matches!(next, Some(TokenKind::Eq | TokenKind::Percent));
        let ended = i > 0 && matches!(tokens[i-1].kind, TokenKind::KwEnd);
        // A SELECT construct continues with CASE, TYPE or RANK; otherwise
        // SELECT names an array, as in `select(k) = .false.`.
        let construct = tokens.get(i+1).is_some_and(|t| same_statement(&input[tokens[i].span.end..t.span.start])
            && matches!(input[t.span.clone()].to_ascii_lowercase().as_str(), "case" | "type" | "rank"));
        let keyword = ended || match tokens[i].kind {
            TokenKind::KwSelect => head && !assigned && construct,
            _ => head && !assigned || i > 0 && matches!(tokens[i-1].kind, TokenKind::KwSelect),
        };
        if !keyword {
            let name = input[tokens[i].span.clone()].to_ascii_lowercase();
            tokens[i].kind = TokenKind::Ident(name);
        }
    }
    // INTENT(IN OUT) and INTENT(INOUT) are the same declaration attribute.
    // Normalize only this phrase, within a single logical statement, so both
    // the parser and source-token semantic checks see the canonical spelling.
    let mut index = 0;
    while index + 4 < tokens.len() {
        // After a declaration's comma, or beginning an INTENT statement.
        let first = if statement_head(&tokens, input, index) { index }
            else if matches!(tokens[index-1].kind, TokenKind::Comma) { index-1 } else { index += 1; continue };
        if matches!(&tokens[index].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("intent"))
            && matches!(tokens[index+1].kind, TokenKind::LParen)
            && matches!(&tokens[index+2].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("in"))
            && matches!(&tokens[index+3].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("out"))
            && matches!(tokens[index+4].kind, TokenKind::RParen)
            && tokens[first..index+5].windows(2).all(|pair| same_statement(&input[pair[0].span.end..pair[1].span.start])) {
            let end = tokens.remove(index+3).span.end;
            tokens[index+2].kind = TokenKind::Ident("inout".into());
            tokens[index+2].span.end = end;
        }
        index += 1;
    }
    // DIMENSION is a keyword only in the attribute part of an intrinsic
    // type declaration. Variables and procedures named dimension stay identifiers.
    let mut declaration = false;
    let mut depth = 0;
    for i in 0..tokens.len() {
        if i == 0 || !same_statement(&input[tokens[i-1].span.end..tokens[i].span.start]) {
            let mut end = i+1;
            while end < tokens.len() && same_statement(&input[tokens[end-1].span.end..tokens[end].span.start]) {
                end += 1;
            }
            declaration = matches!(tokens[i].kind, TokenKind::KwInteger | TokenKind::KwReal | TokenKind::KwComplex
                | TokenKind::KwDouble | TokenKind::KwCharacter | TokenKind::KwLogical)
                && tokens[i..end].iter().any(|t| matches!(t.kind,TokenKind::DColon));
            depth = 0;
        }
        if declaration && depth == 0 && i > 0
            && matches!(tokens[i-1].kind, TokenKind::Comma)
            && matches!(&tokens[i].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("dimension")) {
            tokens[i].kind = TokenKind::KwDimension;
        }
        match tokens[i].kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => depth -= 1,
            TokenKind::DColon if depth == 0 => declaration = false,
            _ => {}
        }
    }
    // DOUBLE PRECISION is a type phrase, not two reserved identifiers.
    // Preserve the pair only within the same logical source statement.
    let type_words: Vec<bool> = (0..tokens.len()).map(|i| {
        matches!(tokens[i].kind, TokenKind::KwDouble)
            && tokens.get(i+1).is_some_and(|next| matches!(next.kind, TokenKind::KwPrecision | TokenKind::KwComplex)
                && same_statement(&input[tokens[i].span.end..next.span.start]))
    }).collect();
    for i in 0..tokens.len() {
        let identifier = match tokens[i].kind {
            TokenKind::KwDouble => !type_words[i],
            TokenKind::KwPrecision => i == 0 || !type_words[i-1],
            _ => false,
        };
        if identifier {
            tokens[i].kind = TokenKind::Ident(input[tokens[i].span.clone()].to_ascii_lowercase());
        }
    }
    let mut start = 0;
    while start < tokens.len() {
        // WHERE is contextual. The token after ')' on this source statement
        // distinguishes a single assignment from the block form.
        let statement_start = start == 0 || !same_statement(&input[tokens[start-1].span.end..tokens[start].span.start])
            || matches!(tokens[start-1].kind, TokenKind::StatementLabel(_));
        if matches!(&tokens[start].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("continue"))
            && tokens.get(start+1).is_none_or(|next| !same_statement(&input[tokens[start].span.end..next.span.start])) {
            let mut begin=start;
            while begin>0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) { begin-=1; }
            if matches!(tokens[begin].kind,TokenKind::StatementLabel(_)) { begin+=1; }
            let inline_if=matches!(tokens[begin].kind,TokenKind::KwIf) && start>begin
                && matches!(tokens[start-1].kind,TokenKind::RParen)
                && tokens[begin..start].iter().fold(0i32,|depth,t|depth+match t.kind {
                    TokenKind::LParen=>1,TokenKind::RParen=>-1,_=>0
                })==0;
            if statement_start || inline_if { tokens[start].kind = TokenKind::KwContinue; }
        }
        let mut assignment = matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::Eq));
        if matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen)) {
            let mut depth=0;
            for end in start+1..tokens.len() {
                match tokens[end].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                if depth==0 { assignment=matches!(tokens.get(end+1).map(|t|&t.kind),Some(TokenKind::Eq)); break; }
            }
        }
        if statement_start && !assignment {
            if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("endwhere")) {
                tokens[start].kind = TokenKind::KwEndWhere;
            } else if matches!(tokens[start].kind,TokenKind::KwEnd)
                && tokens.get(start+1).is_some_and(|t| matches!(&t.kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("where")) && same_statement(&input[tokens[start].span.end..t.span.start])) {
                tokens[start].kind = TokenKind::KwEndWhere;
                tokens[start].span.end = tokens[start+1].span.end;
                tokens.remove(start+1);
            } else if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("elsewhere")) {
                tokens[start].kind = TokenKind::KwElseWhere;
            } else if matches!(tokens[start].kind,TokenKind::KwElse)
                && tokens.get(start+1).is_some_and(|t| matches!(&t.kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("where")) && same_statement(&input[tokens[start].span.end..t.span.start])) {
                tokens[start].kind = TokenKind::KwElseWhere;
                tokens[start].span.end = tokens[start+1].span.end;
                tokens.remove(start+1);
            } else if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("where"))
                && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen)) {
                let mut depth = 0;
                for end in start+1..tokens.len() {
                    match tokens[end].kind { TokenKind::LParen=>depth+=1, TokenKind::RParen=>depth-=1, _=>{} }
                    if depth == 0 {
                        let next=tokens.get(end+1).filter(|t|same_statement(&input[tokens[end].span.end..t.span.start]));
                        if !matches!(next.map(|t|&t.kind),Some(TokenKind::Eq)) {
                            tokens[start].kind=if next.is_none() {TokenKind::KwWhereBlock} else {TokenKind::KwWhere};
                        }
                        break;
                    }
                }
            }
        }
        start += 1;
    }
    for start in 0..tokens.len() {
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n=="enum")
            && (statement_head(&tokens,input,start) && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::Comma))
                || start>0 && matches!(tokens[start-1].kind,TokenKind::KwEnd)
                    && same_statement(&input[tokens[start-1].span.end..tokens[start].span.start])) {
            tokens[start].kind=TokenKind::KwEnum;
        }
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n=="enumeration")
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::Ident(n)) if n=="type")
            && same_statement(&input[tokens[start].span.end..tokens[start+1].span.start])
            && (statement_head(&tokens,input,start) || start>0 && matches!(tokens[start-1].kind,TokenKind::KwEnd)) {
            tokens[start].kind=TokenKind::KwEnumeration;
            tokens[start+1].kind=TokenKind::KwType;
        }
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n=="enumerator")
            && statement_head(&tokens,input,start)
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::DColon|TokenKind::Ident(_))) {
            tokens[start].kind=TokenKind::KwEnumerator;
        }
        if matches!(tokens[start].kind,TokenKind::KwDo)
            && matches!(tokens.get(start+1).map(|t| &t.kind),Some(TokenKind::Ident(n)) if n=="concurrent") {
            let mut depth=0;
            let mut header=false;
            for next in start+2..tokens.len() {
                if !same_statement(&input[tokens[start].span.end..tokens[next].span.start]) {break;}
                match &tokens[next].kind {
                    TokenKind::LParen => {depth+=1;header=true;},
                    TokenKind::RParen => depth-=1,
                    TokenKind::Ident(n) if header && depth==0 && n=="reduce" => tokens[next].kind=TokenKind::KwReduce,
                    _ => {},
                }
            }
        }
        if let TokenKind::Ident(n) = &tokens[start].kind {
            let next=tokens.get(start+1).map(|t| &t.kind);
            let previous=start.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind);
            let replacement=match n.to_ascii_lowercase().as_str() {
                "default" if matches!(previous,Some(TokenKind::KwCase)) => Some(TokenKind::KwDefault),
                "only" if matches!(next,Some(TokenKind::Colon)) && start >= 3 && (matches!(tokens[start-3].kind,TokenKind::KwUse)
                    || (start >= 4 && matches!(tokens[start-4].kind,TokenKind::KwUse) && matches!(tokens[start-3].kind,TokenKind::DColon))
                    || (start >= 6 && matches!(tokens[start-6].kind,TokenKind::KwUse) && matches!(tokens[start-5].kind,TokenKind::Comma) && matches!(tokens[start-3].kind,TokenKind::DColon))) => Some(TokenKind::KwOnly),
                "len" if matches!(previous,Some(TokenKind::LParen)) && matches!(next,Some(TokenKind::Eq)) => Some(TokenKind::KwLen),
                "kind" if matches!(previous,Some(TokenKind::LParen)) && matches!(next,Some(TokenKind::Eq)) => Some(TokenKind::KwKind),
                // The second selector of CHARACTER(LEN=..., KIND=...) or CHARACTER(KIND=..., LEN=...).
                "kind" | "len" if matches!(previous,Some(TokenKind::Comma)) && matches!(next,Some(TokenKind::Eq)) && character_selector(&tokens,start) =>
                    Some(if n.eq_ignore_ascii_case("kind") {TokenKind::KwKind} else {TokenKind::KwLen}),
                "type" if matches!(next,Some(TokenKind::DColon|TokenKind::LParen)) || matches!(previous,Some(TokenKind::KwEnd)) => Some(TokenKind::KwType),
                // TYPE, attributes :: name and TYPE name (alone on its statement).
                "type" if statement_head(&tokens,input,start) && tokens.get(start+1).is_some_and(|t| same_statement(&input[tokens[start].span.end..t.span.start]))
                    && (matches!(next,Some(TokenKind::Comma))
                        || matches!(next,Some(TokenKind::Ident(_))) && tokens.get(start+2).is_none_or(|t| !same_statement(&input[tokens[start+1].span.end..t.span.start]))) => Some(TokenKind::KwType),
                "class" if matches!(next,Some(TokenKind::LParen)) => Some(TokenKind::KwClass),
                "typeof" | "classof" if matches!(next,Some(TokenKind::LParen)) && statement_head(&tokens,input,start) => {
                    let mut depth=0i32;
                    let mut closing=None;
                    for i in start+1..tokens.len() {
                        match tokens[i].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                        if depth==0 {closing=Some(i);break;}
                    }
                    // Do not reserve the names for procedure calls or assignments.
                    let declaration=closing.and_then(|i|tokens.get(i+1)).is_some_and(|t|
                        same_statement(&input[tokens[start].span.end..t.span.start]) && matches!(t.kind,TokenKind::DColon|TokenKind::Comma|TokenKind::Ident(_)));
                    declaration.then_some(if n.eq_ignore_ascii_case("typeof"){TokenKind::KwTypeof}else{TokenKind::KwClassof})
                },
                // DATA name / values /, but not a variable named DATA.
                "data" if matches!(next,Some(TokenKind::Ident(_))) && statement_head(&tokens,input,start) => Some(TokenKind::KwData),
                // DATA (implied-DO list) / values /.
                "data" if matches!(next,Some(TokenKind::LParen)) && statement_head(&tokens,input,start) => {
                    let mut depth=0i32;
                    let mut after=None;
                    for i in start+1..tokens.len() {
                        match tokens[i].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                        if depth==0 {after=tokens.get(i+1).map(|t|&t.kind);break;}
                    }
                    matches!(after,Some(TokenKind::Slash|TokenKind::Comma)).then_some(TokenKind::KwData)
                },
                // EQUIVALENCE (a, b), ..., but not an array named EQUIVALENCE.
                "equivalence" if matches!(next,Some(TokenKind::LParen)) && statement_head(&tokens,input,start)
                    && !assigned(&tokens,input,start) => Some(TokenKind::KwEquivalence),
                "procedure" if matches!(next,Some(TokenKind::DColon)) || matches!(previous,Some(TokenKind::KwModule))
                    // A type-bound binding with attributes: `procedure, private :: f`.
                    || matches!(next,Some(TokenKind::Comma)) && statement_head(&tokens,input,start) => Some(TokenKind::KwProcedure),
                "interface" if matches!(previous,Some(TokenKind::KwEnd)) || matches!(next,Some(TokenKind::Ident(_))) => Some(TokenKind::KwInterface),
                "operator" if matches!(next,Some(TokenKind::LParen)) => Some(TokenKind::KwOperator),
                // DIMENSION statement: `dimension a(2), b(3)` or `dimension :: a(2)`.
                "dimension" if matches!(next,Some(TokenKind::Ident(_)|TokenKind::DColon)) && statement_head(&tokens,input,start) => Some(TokenKind::KwDimension),
                "associate" if matches!(previous,Some(TokenKind::KwEnd))
                    || matches!(next,Some(TokenKind::LParen)) && statement_head(&tokens,input,start) => Some(TokenKind::KwAssociate),
                _=>None,
            };
            let replacement = if !matches!(replacement, Some(TokenKind::KwOperator)) && start >= 2 && matches!(tokens[start-1].kind, TokenKind::KwInterface)
                && matches!(tokens[start-2].kind, TokenKind::KwEnd)
                && same_statement(&input[tokens[start-1].span.end..tokens[start].span.start]) {
                Some(TokenKind::EndInterfaceName(n.clone()))
            } else { replacement };
            if let Some(kind)=replacement {tokens[start].kind=kind;}
        }
        // File-control words are contextual, including after a single-line IF.
        if let TokenKind::Ident(word) = &tokens[start].kind {
            let word = word.to_ascii_lowercase();
            if matches!(word.as_str(), "open" | "close" | "backspace" | "rewind" | "inquire" | "nullify" | "flush" | "endfile") {
                let mut begin = start;
                while begin > 0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) { begin -= 1; }
                if matches!(tokens[begin].kind, TokenKind::StatementLabel(_)) { begin += 1; }
                let inline_if = matches!(tokens[begin].kind, TokenKind::KwIf) && start > begin
                    && matches!(tokens[start-1].kind, TokenKind::RParen)
                    && tokens[begin..start].iter().fold(0i32, |d,t| d + match t.kind {TokenKind::LParen=>1,TokenKind::RParen=>-1,_=>0}) == 0;
                if begin == start || inline_if {
                    if matches!(tokens.get(start+1).map(|t| &t.kind), Some(TokenKind::LParen)) {
                        let mut depth = 0;
                        for end in start+1..tokens.len() {
                            match tokens[end].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                            if depth == 0 {
                                let following=tokens.get(end+1).filter(|t|same_statement(&input[tokens[end].span.end..t.span.start]));
                                if !matches!(following.map(|t|&t.kind),Some(TokenKind::Eq)) { tokens[start].kind=if word=="nullify" {TokenKind::KwNullify}else{TokenKind::FileControl(word.clone())}; }
                                break;
                            }
                        }
                    } else if matches!(word.as_str(), "backspace" | "rewind" | "flush" | "endfile")
                        && tokens.get(start+1).is_some_and(|t| !matches!(t.kind,TokenKind::Eq) && same_statement(&input[tokens[start].span.end..t.span.start])) {
                        tokens[start].kind=TokenKind::FilePosition(word);
                    }
                }
            }
        }
        // An empty READ ends at the closing control list, within its logical
        // source statement; a continued input list still uses ordinary READ.
        if matches!(tokens[start].kind, TokenKind::KwRead)
            && matches!(tokens.get(start+1).map(|t| &t.kind), Some(TokenKind::LParen)) {
            let mut depth = 0;
            for end in start+1..tokens.len() {
                if !same_statement(&input[tokens[end-1].span.end..tokens[end].span.start]) { break; }
                match tokens[end].kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
                if depth == 0 {
                    if tokens.get(end+1).is_none_or(|next| !same_statement(&input[tokens[end].span.end..next.span.start])) {
                        tokens[start].kind = TokenKind::KwReadEmpty;
                    }
                    break;
                }
            }
        }
        // WRITE is contextual: preserve variables, arrays, and procedures with
        // this name. Mark empty output lists separately because the grammar
        // otherwise has no statement-separator tokens to distinguish the next item.
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("write"))
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen))
            && same_statement(&input[tokens[start].span.end..tokens[start+1].span.start]) {
            let mut begin=start;
            while begin>0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) {begin-=1;}
            if matches!(tokens[begin].kind,TokenKind::StatementLabel(_)) { begin+=1; }
            let inline_if=matches!(tokens[begin].kind,TokenKind::KwIf)
                && start>begin && matches!(tokens[start-1].kind,TokenKind::RParen)
                && tokens[begin..start].iter().fold(0i32,|depth,t|depth+match t.kind {
                    TokenKind::LParen=>1,TokenKind::RParen=>-1,_=>0
                })==0;
            if begin==start || inline_if {
                let mut depth=0;
                for end in start+1..tokens.len() {
                    match tokens[end].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                    if depth==0 {
                        let next=tokens.get(end+1).filter(|t|same_statement(&input[tokens[end].span.end..t.span.start]));
                        if !matches!(next.map(|t|&t.kind),Some(TokenKind::Eq)) {
                            tokens[start].kind=if next.is_none() {TokenKind::KwWriteEmpty} else {TokenKind::KwWrite};
                        }
                        break;
                    }
                }
            }
        }
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("parameter"))
            && (start == 0 || !same_statement(&input[tokens[start-1].span.end..tokens[start].span.start]))
            && matches!(tokens.get(start+1).map(|t| &t.kind),Some(TokenKind::LParen))
            && matches!(tokens.get(start+2).map(|t| &t.kind),Some(TokenKind::Ident(_)))
            && matches!(tokens.get(start+3).map(|t| &t.kind),Some(TokenKind::Eq)) {
            tokens[start].kind = TokenKind::KwParameter;
        }
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("forall"))
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen))
            && matches!(tokens.get(start+2).map(|t|&t.kind),Some(TokenKind::Ident(_)))
            && matches!(tokens.get(start+3).map(|t|&t.kind),Some(TokenKind::Eq)) {
            tokens[start].kind=TokenKind::KwForall;
        }
        if matches!(&tokens[start].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("error")) && matches!(tokens.get(start+1).map(|t| &t.kind), Some(TokenKind::KwStop)) { tokens[start].kind = TokenKind::KwError; }
        if matches!(&tokens[start].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("allocate")) && matches!(tokens.get(start+1).map(|t| &t.kind), Some(TokenKind::LParen)) { tokens[start].kind = TokenKind::KwAllocate; }
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("deallocate"))
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen)) {
            let mut begin=start;
            while begin>0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) {begin-=1;}
            if matches!(tokens[begin].kind,TokenKind::StatementLabel(_)) { begin+=1; }
            let inline_if=matches!(tokens[begin].kind,TokenKind::KwIf) && start>begin
                && matches!(tokens[start-1].kind,TokenKind::RParen)
                && tokens[begin..start].iter().fold(0i32,|d,t|d+match t.kind {TokenKind::LParen=>1,TokenKind::RParen=>-1,_=>0})==0;
            if begin==start || inline_if {
                let mut depth=0;
                for end in start+1..tokens.len() {
                    match tokens[end].kind {TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,_=>{}}
                    if depth==0 {
                        let following=tokens.get(end+1).filter(|t|same_statement(&input[tokens[end].span.end..t.span.start]));
                        if !matches!(following.map(|t|&t.kind),Some(TokenKind::Eq)) {tokens[start].kind=TokenKind::KwDeallocate;}
                        break;
                    }
                }
            }
        }
        if let TokenKind::Ident(name) = &tokens[start].kind {
            if matches!(name.to_ascii_lowercase().as_str(), "pure" | "simple" | "impure" | "elemental" | "recursive") {
                let mut end = start + 1;
                while tokens.get(end).is_some_and(|t| matches!(&t.kind,TokenKind::Ident(n) if matches!(n.to_ascii_lowercase().as_str(),"pure" | "simple" | "impure" | "elemental" | "recursive"))) { end += 1; }
                let following = tokens.get(end).map(|t| &t.kind);
                let same_line = tokens.get(end).is_some_and(|next| same_statement(&input[tokens[start].span.end..next.span.start]));
                let derived_type = matches!(following,Some(TokenKind::Ident(n)) if n.eq_ignore_ascii_case("type"))
                    && matches!(tokens.get(end+1).map(|t|&t.kind),Some(TokenKind::LParen));
                if same_line && (derived_type || matches!(following, Some(TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwType | TokenKind::KwLogical | TokenKind::KwInteger | TokenKind::KwReal | TokenKind::KwComplex | TokenKind::KwDouble | TokenKind::KwCharacter))) {
                    tokens[start].kind = match name.to_ascii_lowercase().as_str() { "elemental" => TokenKind::KwElemental, "recursive" => TokenKind::KwRecursive, "impure" => TokenKind::KwImpure, "simple" => TokenKind::KwSimple, _ => TokenKind::KwPure };
                }
            }
        }
        if !matches!(tokens[start].kind, TokenKind::KwFunction)
            || !matches!(tokens.get(start + 2).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
        let mut depth = 0;
        for end in start + 2..tokens.len() {
            match tokens[end].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        if !matches!(tokens.get(end + 2).map(|t| &t.kind), Some(TokenKind::LParen)) { break; }
                        if let Some(token) = tokens.get_mut(end + 1) {
                            if matches!(&token.kind, TokenKind::Ident(name) if name.eq_ignore_ascii_case("result")) {
                                token.kind = TokenKind::KwResult;
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    // Attribute specification statements, e.g. `intent(in) :: x` or
    // `external f`. A following `::` or name distinguishes them from variables.
    for index in 0..tokens.len() {
        let TokenKind::Ident(word) = &tokens[index].kind else { continue };
        let word = word.to_ascii_lowercase();
        if !matches!(word.as_str(), "intent" | "optional" | "pointer" | "target" | "allocatable" | "external" | "intrinsic" | "save" | "value")
            || !statement_head(&tokens, input, index) {
            continue;
        }
        if word == "save" && tokens.get(index + 1).is_none_or(|t| !same_statement(&input[tokens[index].span.end..t.span.start])) {
            tokens[index].kind = TokenKind::KwSaveAll;
            continue;
        }
        let mut next = index + 1;
        if word == "intent" {
            if !matches!(tokens.get(next).map(|t| &t.kind), Some(TokenKind::LParen)) { continue; }
            let mut depth = 0;
            while next < tokens.len() {
                match tokens[next].kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
                next += 1;
                if depth == 0 { break; }
            }
        }
        if matches!(word.as_str(), "intrinsic" | "external") {
            // The listed names may be spelled like keywords: INTRINSIC REAL.
            let mut i = next;
            while i < tokens.len() && same_statement(&input[tokens[i - 1].span.end..tokens[i].span.start]) {
                let text = &input[tokens[i].span.clone()];
                if !matches!(tokens[i].kind, TokenKind::Comma | TokenKind::DColon | TokenKind::Ident(_))
                    && text.starts_with(|c: char| c.is_ascii_alphabetic())
                    && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    tokens[i].kind = TokenKind::Ident(text.to_ascii_lowercase());
                }
                i += 1;
            }
        }
        if tokens.get(next).is_some_and(|t| matches!(t.kind, TokenKind::DColon | TokenKind::Ident(_))
            && same_statement(&input[tokens[next-1].span.end..t.span.start])) {
            tokens[index].kind = TokenKind::AttrStatement(word);
        }
    }
    crate::assumed_rank::tokens(&mut tokens, input);
    crate::select_type::tokens(&mut tokens, input);
    crate::namelists::tokens(&mut tokens, input);
    crate::c_interop::binding_tokens(&mut tokens, input);
    procedure_declarations(&mut tokens, input);
    // Distinct name tokens keep optional END DO names and named transfers from
    // consuming an identifier on the following logical statement.
    let mut named_index=0;
    while named_index<tokens.len() {
        if named_index+2<tokens.len()
            && matches!(tokens[named_index].kind,TokenKind::Ident(_))
            && matches!(tokens[named_index+1].kind,TokenKind::Colon)
            && (matches!(tokens[named_index+2].kind,TokenKind::KwDo|TokenKind::KwIf|TokenKind::KwSelect|TokenKind::KwBlock|TokenKind::KwAssociate)
                || matches!(&tokens[named_index+2].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("associate"))
                    && matches!(tokens.get(named_index+3).map(|t|&t.kind),Some(TokenKind::LParen)))
            && statement_head(&tokens,input,named_index)
            && same_statement(&input[tokens[named_index].span.end..tokens[named_index+2].span.start]) {
            let TokenKind::Ident(name)=tokens[named_index].kind.clone() else {unreachable!()};
            tokens[named_index].kind=TokenKind::ConstructName(name);
            if matches!(tokens[named_index+2].kind,TokenKind::Ident(_)) {tokens[named_index+2].kind=TokenKind::KwAssociate;}
        }
        if named_index+1<tokens.len()
            && same_statement(&input[tokens[named_index].span.end..tokens[named_index+1].span.start]) {
            if let TokenKind::Ident(name)=tokens[named_index+1].kind.clone() {
                if matches!(tokens[named_index].kind,TokenKind::KwExit|TokenKind::KwCycle) {
                    tokens[named_index].kind=if matches!(tokens[named_index].kind,TokenKind::KwExit) {TokenKind::NamedExit(name)} else {TokenKind::NamedCycle(name)};
                    tokens[named_index].span.end=tokens[named_index+1].span.end;
                    tokens.remove(named_index+1);
                } else if matches!(tokens[named_index].kind,TokenKind::KwEndDo)
                    || (named_index>0 && matches!(tokens[named_index].kind,TokenKind::KwDo)
                    && matches!(tokens[named_index-1].kind,TokenKind::KwEnd)
                    && same_statement(&input[tokens[named_index-1].span.end..tokens[named_index].span.start])) {
                    tokens[named_index+1].kind=TokenKind::EndLoopName(name);
                }
            }
        }
        named_index+=1;
    }
    // The optional construct name after END IF, END SELECT, END BLOCK,
    // END ASSOCIATE, ELSE, ELSE IF (...) THEN and CASE selects nothing more.
    let mut index=0;
    while index+1<tokens.len() {
        let next_same=same_statement(&input[tokens[index].span.end..tokens[index+1].span.start]);
        let named=next_same && matches!(tokens[index+1].kind,TokenKind::Ident(_)) && match tokens[index].kind {
            TokenKind::KwIf|TokenKind::KwSelect|TokenKind::KwBlock|TokenKind::KwAssociate => index>0
                && matches!(tokens[index-1].kind,TokenKind::KwEnd)
                && same_statement(&input[tokens[index-1].span.end..tokens[index].span.start]),
            TokenKind::KwElse|TokenKind::KwDefault|TokenKind::KwClassDefault => true,
            TokenKind::KwThen => (0..index).rev().find(|&i| statement_head(&tokens,input,i))
                .is_some_and(|i| matches!(tokens[i].kind,TokenKind::KwElseIf|TokenKind::KwElse)
                    || matches!(&tokens[i].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("elseif"))),
            TokenKind::RParen => (0..index).rev().find(|&i| statement_head(&tokens,input,i))
                .is_some_and(|i| matches!(tokens[i].kind,TokenKind::KwCase|TokenKind::KwRank|TokenKind::KwTypeIs|TokenKind::KwClassIs)),
            _ => false,
        };
        if named && tokens.get(index+2).is_none_or(|t| !same_statement(&input[tokens[index+1].span.end..t.span.start])) {
            tokens.remove(index+1);
        }
        index+=1;
    }
    for index in 0..tokens.len() {
        // Newlines are skipped by the grammar. Distinguish PRINT * without an
        // output list so it cannot consume the following statement as an item.
        if matches!(tokens[index].kind, TokenKind::KwPrint)
            && tokens.get(index+1).is_some_and(|next| matches!(next.kind, TokenKind::Star)
                && same_statement(&input[tokens[index].span.end..next.span.start]))
            && tokens.get(index+2).is_none_or(|next| !same_statement(&input[tokens[index+1].span.end..next.span.start])) {
            tokens[index].kind = TokenKind::KwPrintEmpty;
        }
        // PRINT remains a valid procedure/variable name. Only classify an
        // empty formatted transfer at a statement start or after a complete IF.
        let mut print_start=index;
        while print_start>0 && same_statement(&input[tokens[print_start-1].span.end..tokens[print_start].span.start]) {print_start-=1;}
        if matches!(tokens[print_start].kind,TokenKind::StatementLabel(_)) {print_start+=1;}
        let print_statement=print_start==index || (print_start<index
            && matches!(tokens[print_start].kind,TokenKind::KwIf)
            && matches!(tokens[index-1].kind,TokenKind::RParen)
            && tokens[print_start..index].iter().fold(0i32,|depth,t|depth+match t.kind {TokenKind::LParen=>1,TokenKind::RParen=>-1,_=>0})==0);
        if print_statement && matches!(tokens[index].kind,TokenKind::KwPrint)
            && tokens.get(index+1).is_some_and(|next|!matches!(next.kind,TokenKind::Star|TokenKind::Eq|TokenKind::Arrow|TokenKind::Percent)
                && same_statement(&input[tokens[index].span.end..next.span.start])) {
            let mut depth=0;let mut comma=false;
            for next in tokens.iter().skip(index+1).take_while(|next|same_statement(&input[tokens[index].span.end..next.span.start])) {
                match next.kind {
                    TokenKind::LParen=>depth+=1,TokenKind::RParen=>depth-=1,
                    TokenKind::Comma|TokenKind::Eq|TokenKind::Arrow if depth==0=>{comma=true;break;},_=>{},
                }
            }
            if !comma {tokens[index].kind=TokenKind::KwPrintEmpty;}
        }
        // Bare DO has no control clause. The grammar otherwise consumes the
        // following assignment as a counted DO header because newlines are skipped.
        // Require a statement start so END DO retains its closing DO token.
        if matches!(tokens[index].kind, TokenKind::KwDo)
            && (index == 0 || !same_statement(&input[tokens[index-1].span.end..tokens[index].span.start])
                || matches!(tokens[index-1].kind,TokenKind::StatementLabel(_))
                || (index>=2 && matches!(tokens[index-1].kind,TokenKind::Colon) && matches!(tokens[index-2].kind,TokenKind::ConstructName(_))))
            && tokens.get(index+1).is_none_or(|next| !same_statement(&input[tokens[index].span.end..next.span.start])) {
            tokens[index].kind = TokenKind::KwDoUnbounded;
        }
        if matches!(tokens[index].kind,TokenKind::KwStop)
            && tokens.get(index+1).is_none_or(|next|!same_statement(&input[tokens[index].span.end..next.span.start])) {
            tokens[index].kind=TokenKind::KwStopEmpty;
        }
    }
    // ELSE IF is one branch only within a logical statement. ELSE followed by
    // an IF on the next statement is a nested construct with its own END IF.
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokenKind::KwElse)
            && tokens.get(index + 1).is_some_and(|t| matches!(t.kind, TokenKind::KwIf)
                && same_statement(&input[tokens[index].span.end..t.span.start])) {
            let end = tokens.remove(index + 1).span.end;
            tokens[index].kind = TokenKind::KwElseIf;
            tokens[index].span.end = end;
        } else if matches!(&tokens[index].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("elseif"))
            && tokens.get(index + 1).is_some_and(|t| matches!(t.kind, TokenKind::LParen)) {
            // Keep ELSEIF available as an ordinary variable/procedure name.
            let mut depth = 0;
            for end in index + 1..tokens.len() {
                match tokens[end].kind { TokenKind::LParen => depth += 1, TokenKind::RParen => depth -= 1, _ => {} }
                if depth == 0 {
                    if tokens.get(end + 1).is_some_and(|t| matches!(t.kind, TokenKind::KwThen)) {
                        tokens[index].kind = TokenKind::KwElseIf;
                    }
                    break;
                }
            }
        }
        index += 1;
    }
    // Unit-ending names belong to this logical statement only. The grammar
    // skips newlines, so an optional ordinary identifier could otherwise eat
    // the first statement following an unnamed END SUBROUTINE or END MODULE.
    for index in 0..tokens.len() {
        if !matches!(tokens[index].kind, TokenKind::KwEnd)
            || index > 0 && same_statement(&input[tokens[index-1].span.end..tokens[index].span.start]) {
            continue;
        }
        let following = tokens.get(index+1).filter(|t|
            same_statement(&input[tokens[index].span.end..t.span.start]));
        if following.is_none() {
            tokens[index].kind = TokenKind::BareEnd;
        } else if matches!(following.map(|t|&t.kind),Some(TokenKind::KwEnumeration)) {
            if let Some(Token{kind:TokenKind::KwType,span})=tokens.get(index+2) {
                let end=span.end;
                if let Some(name)=tokens.get_mut(index+3) {
                    if same_statement(&input[end..name.span.start]) {
                        if let TokenKind::Ident(n)=&name.kind {name.kind=TokenKind::EndUnitName(n.clone());}
                    }
                }
            }
        } else if matches!(following.map(|t| &t.kind), Some(TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwModule | TokenKind::KwProgram | TokenKind::KwType)) {
            let end = tokens[index+1].span.end;
            if let Some(name) = tokens.get_mut(index+2) {
                if same_statement(&input[end..name.span.start]) {
                    if let TokenKind::Ident(n) = &name.kind {
                        name.kind = TokenKind::EndUnitName(n.clone());
                    }
                }
            }
        }
    }
    // Normalize old-style constructor delimiters without confusing OPERATOR(/)
    // or division inside parenthesized expressions with constructor punctuation.
    let mut normalized: Vec<Token> = Vec::new();
    let mut stack = Vec::new();
    let mut iter = tokens.into_iter().peekable();
    while let Some(mut token) = iter.next() {
        match token.kind {
            TokenKind::LParen if iter.peek().is_some_and(|t| matches!(t.kind, TokenKind::Slash))
                && !normalized.last().is_some_and(|t| matches!(t.kind, TokenKind::KwOperator)) => {
                token.span.end = iter.next().unwrap().span.end;
                token.kind = TokenKind::LBracket;
                stack.push('c');
            }
            TokenKind::Slash if stack.last() == Some(&'c')
                && iter.peek().is_some_and(|t| matches!(t.kind, TokenKind::RParen)) => {
                token.span.end = iter.next().unwrap().span.end;
                token.kind = TokenKind::RBracket;
                stack.pop();
            }
            TokenKind::LParen => stack.push('p'),
            TokenKind::LBracket => stack.push('b'),
            TokenKind::RParen | TokenKind::RBracket => { stack.pop(); }
            _ => {}
        }
        normalized.push(token);
    }
    // A complex literal has its own opening token. This distinguishes its
    // comma from implied-DO controls without making the expression grammar
    // ambiguous. Function argument and declaration lists keep ordinary '(' .
    for i in 0..normalized.len() {
        if !matches!(normalized[i].kind,TokenKind::LParen){continue;}
        if i>0 && same_statement(&input[normalized[i-1].span.end..normalized[i].span.start])
            && matches!(normalized[i-1].kind,TokenKind::Ident(_)|TokenKind::RParen|TokenKind::RBracket|TokenKind::KwReal|TokenKind::KwComplex|TokenKind::KwInteger|TokenKind::KwCharacter|TokenKind::KwLogical|TokenKind::KwDimension|TokenKind::KwKind|TokenKind::KwLen|TokenKind::KwCase|TokenKind::KwIf|TokenKind::KwWhile|TokenKind::KwRead|TokenKind::KwReadEmpty|TokenKind::KwWrite|TokenKind::KwWriteEmpty|TokenKind::KwOperator|TokenKind::KwAllocate|TokenKind::KwDeallocate|TokenKind::KwNullify|TokenKind::KwNone){continue;}
        let mut end=i+1;
        while end<normalized.len() && !matches!(normalized[end].kind,TokenKind::RParen|TokenKind::LParen|TokenKind::Eq|TokenKind::LBracket) {end+=1;}
        if end>=normalized.len() || !matches!(normalized[end].kind,TokenKind::RParen){continue;}
        let commas=(i+1..end).filter(|j|matches!(normalized[*j].kind,TokenKind::Comma)).collect::<Vec<_>>();
        if let [comma]=commas.as_slice() {
            let part=|tokens:&[Token]| match tokens {
                [Token{kind:TokenKind::Integer(_)|TokenKind::Float(_)|TokenKind::Ident(_),..}]=>true,
                [Token{kind:TokenKind::Plus|TokenKind::Minus,..},Token{kind:TokenKind::Integer(_)|TokenKind::Float(_),..}]=>true,
                _=>false,
            };
            if part(&normalized[i+1..*comma])&&part(&normalized[*comma+1..end]){normalized[i].kind=TokenKind::ComplexOpen;}
        }
    }
    // Last, since the inserted END DO tokens have no source text to separate
    // them from the statement they follow.
    labeled_do(&mut normalized, input);
    normalized
}

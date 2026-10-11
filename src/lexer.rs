use logos::Logos;

#[cfg(test)]
mod tests {
    use super::*;

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
    KwDimension,
    FileControl(String),
    FilePosition(String),
    KwType,
    KwClass,
    KwProcedure,
    KwInterface,
    EndInterfaceName(String),
    EndUnitName(String),
    BareEnd,
    KwOperator,
    KwKind,
    KwOnly,
    #[token("%")]
    Percent,
    #[token("=>")]
    Arrow,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),
    #[regex(r"\.[A-Za-z]+\.", |lex| lex.slice().to_ascii_lowercase())]
    DefinedOperator(String),
    // Fortran doubles the delimiter inside a character literal. Backslashes
    // are ordinary characters, including immediately before the closing quote.
    #[regex(r#""([^"\r\n]|"")*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].replace("\"\"", "\"")
    })]
    #[regex(r"'([^'\r\n]|'')*'", |lex| {
        let s = lex.slice();
        s[1..s.len()-1].replace("''", "'")
    })]
    Str(String),
    #[regex(r"(([0-9]+\.[0-9]*|\.[0-9]+)([eEdD][+-]?[0-9]+)?|[0-9]+[eEdD][+-]?[0-9]+)(_[A-Za-z0-9_]+)?", |lex| lex.slice().to_string())]
    Float(String),
    #[regex(r"[0-9]+(_[A-Za-z0-9_]+)?", |lex| lex.slice().to_string())]
    Integer(String),
    StatementLabel(String),
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
    #[token("while", ignore(ascii_case))]
    KwWhile,
    #[token("integer", ignore(ascii_case))]
    KwInteger,
    #[token("real", ignore(ascii_case))]
    KwReal,
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
    // Contextual keyword in CASE DEFAULT; otherwise a valid identifier.
    KwDefault,
    #[token("block", ignore(ascii_case))]
    KwBlock,
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
    #[token("::")]
    DColon,
    #[token(":")]
    Colon,
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

pub fn lex(input: &str) -> Vec<Token> {
    let mut lexer = TokenKind::lexer(input);
    let mut tokens = Vec::new();
    while let Some(res) = lexer.next() {
        match res {
            Ok(kind) => {
                if matches!(kind, TokenKind::_WS | TokenKind::_COMMENT) {
                    continue;
                }
                let span = lexer.span();
                tokens.push(Token { kind, span });
            }
            Err(_) => {
                let span = lexer.span();
                let text = input.get(span.clone()).unwrap_or("").to_string();
                tokens.push(Token {
                    kind: TokenKind::Error(text),
                    span,
                });
            }
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
    // INTENT(IN OUT) and INTENT(INOUT) are the same declaration attribute.
    // Normalize only this phrase, within a single logical statement, so both
    // the parser and source-token semantic checks see the canonical spelling.
    let mut index = 1;
    while index + 4 < tokens.len() {
        if matches!(tokens[index-1].kind, TokenKind::Comma)
            && matches!(&tokens[index].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("intent"))
            && matches!(tokens[index+1].kind, TokenKind::LParen)
            && matches!(&tokens[index+2].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("in"))
            && matches!(&tokens[index+3].kind, TokenKind::Ident(n) if n.eq_ignore_ascii_case("out"))
            && matches!(tokens[index+4].kind, TokenKind::RParen)
            && tokens[index-1..index+5].windows(2).all(|pair| same_statement(&input[pair[0].span.end..pair[1].span.start])) {
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
            declaration = matches!(tokens[i].kind, TokenKind::KwInteger | TokenKind::KwReal
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
            && tokens.get(i+1).is_some_and(|next| matches!(next.kind, TokenKind::KwPrecision)
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
                "type" if matches!(next,Some(TokenKind::DColon|TokenKind::LParen)) || matches!(previous,Some(TokenKind::KwEnd)) => Some(TokenKind::KwType),
                "class" if matches!(next,Some(TokenKind::LParen)) => Some(TokenKind::KwClass),
                "procedure" if matches!(next,Some(TokenKind::DColon)) || matches!(previous,Some(TokenKind::KwModule)) => Some(TokenKind::KwProcedure),
                "interface" if matches!(previous,Some(TokenKind::KwEnd)) || matches!(next,Some(TokenKind::Ident(_))) => Some(TokenKind::KwInterface),
                "operator" if matches!(next,Some(TokenKind::LParen)) => Some(TokenKind::KwOperator),
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
            if matches!(word.as_str(), "open" | "close" | "backspace" | "rewind" | "inquire") {
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
                                if !matches!(following.map(|t|&t.kind),Some(TokenKind::Eq)) { tokens[start].kind=TokenKind::FileControl(word.clone()); }
                                break;
                            }
                        }
                    } else if matches!(word.as_str(), "backspace" | "rewind")
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
            if matches!(name.to_ascii_lowercase().as_str(), "pure" | "impure" | "elemental" | "recursive") {
                let mut end = start + 1;
                while tokens.get(end).is_some_and(|t| matches!(&t.kind,TokenKind::Ident(n) if matches!(n.to_ascii_lowercase().as_str(),"pure" | "impure" | "elemental" | "recursive"))) { end += 1; }
                let following = tokens.get(end).map(|t| &t.kind);
                let same_line = tokens.get(end).is_some_and(|next| same_statement(&input[tokens[start].span.end..next.span.start]));
                if same_line && matches!(following, Some(TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwLogical | TokenKind::KwInteger | TokenKind::KwReal | TokenKind::KwDouble | TokenKind::KwCharacter)) {
                    tokens[start].kind = match name.to_ascii_lowercase().as_str() { "elemental" => TokenKind::KwElemental, "recursive" => TokenKind::KwRecursive, "impure" => TokenKind::KwImpure, _ => TokenKind::KwPure };
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
    for index in 0..tokens.len() {
        // Newlines are skipped by the grammar. Distinguish PRINT * without an
        // output list so it cannot consume the following statement as an item.
        if matches!(tokens[index].kind, TokenKind::KwPrint)
            && tokens.get(index+1).is_some_and(|next| matches!(next.kind, TokenKind::Star)
                && same_statement(&input[tokens[index].span.end..next.span.start]))
            && tokens.get(index+2).is_none_or(|next| !same_statement(&input[tokens[index+1].span.end..next.span.start])) {
            tokens[index].kind = TokenKind::KwPrintEmpty;
        }
        // Bare DO has no control clause. The grammar otherwise consumes the
        // following assignment as a counted DO header because newlines are skipped.
        // Require a statement start so END DO retains its closing DO token.
        if matches!(tokens[index].kind, TokenKind::KwDo)
            && (index == 0 || !same_statement(&input[tokens[index-1].span.end..tokens[index].span.start])
                || matches!(tokens[index-1].kind,TokenKind::StatementLabel(_)))
            && tokens.get(index+1).is_none_or(|next| !same_statement(&input[tokens[index].span.end..next.span.start])) {
            tokens[index].kind = TokenKind::KwDoUnbounded;
        }
        if matches!(tokens[index].kind,TokenKind::KwStop)
            && !(index>0 && matches!(tokens[index-1].kind,TokenKind::KwError)
                && same_statement(&input[tokens[index-1].span.end..tokens[index].span.start]))
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
        } else if matches!(following.map(|t| &t.kind), Some(TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwModule | TokenKind::KwProgram)) {
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
    normalized
}

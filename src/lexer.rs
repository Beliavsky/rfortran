use logos::Logos;

#[cfg(test)]
mod tests {
    use super::*;

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
    FileControl(String),
    FilePosition(String),
    KwType,
    KwClass,
    KwProcedure,
    KwInterface,
    EndInterfaceName(String),
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
    #[regex(r"[0-9]+", |lex| lex.slice().to_string())]
    Integer(String),
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
    #[token("read", ignore(ascii_case))]
    KwRead,
    #[token("if", ignore(ascii_case))]
    KwIf,
    #[token("then", ignore(ascii_case))]
    KwThen,
    #[token("else", ignore(ascii_case))]
    KwElse,
    KwElseIf,
    #[token("do", ignore(ascii_case))]
    KwDo,
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
fn same_statement(gap: &str) -> bool {
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
    let mut start = 0;
    while start < tokens.len() {
        // WHERE is contextual. The token after ')' on this source statement
        // distinguishes a single assignment from the block form.
        let statement_start = start == 0 || !same_statement(&input[tokens[start-1].span.end..tokens[start].span.start]);
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
                "only" if matches!(next,Some(TokenKind::Colon)) && start >= 3 && matches!(tokens[start-3].kind,TokenKind::KwUse) => Some(TokenKind::KwOnly),
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
            if matches!(word.as_str(), "open" | "close" | "backspace" | "rewind") {
                let mut begin = start;
                while begin > 0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) { begin -= 1; }
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
        // WRITE is contextual: preserve variables, arrays, and procedures with
        // this name. Mark empty output lists separately because the grammar
        // otherwise has no statement-separator tokens to distinguish the next item.
        if matches!(&tokens[start].kind,TokenKind::Ident(n) if n.eq_ignore_ascii_case("write"))
            && matches!(tokens.get(start+1).map(|t|&t.kind),Some(TokenKind::LParen))
            && same_statement(&input[tokens[start].span.end..tokens[start+1].span.start]) {
            let mut begin=start;
            while begin>0 && same_statement(&input[tokens[begin-1].span.end..tokens[begin].span.start]) {begin-=1;}
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
        if let TokenKind::Ident(name) = &tokens[start].kind {
            if matches!(name.to_ascii_lowercase().as_str(), "pure" | "elemental") {
                let next = tokens.get(start+1).map(|t| &t.kind);
                let prefix = matches!(next, Some(TokenKind::Ident(n)) if matches!(n.to_ascii_lowercase().as_str(), "pure" | "elemental"));
                let following = if prefix { tokens.get(start+2).map(|t| &t.kind) } else { next };
                let same_line = tokens.get(start+if prefix {2} else {1}).is_some_and(|next| same_statement(&input[tokens[start].span.end..next.span.start]));
                if same_line && matches!(following, Some(TokenKind::KwFunction | TokenKind::KwSubroutine | TokenKind::KwLogical | TokenKind::KwInteger | TokenKind::KwReal | TokenKind::KwDouble | TokenKind::KwCharacter)) {
                    tokens[start].kind = if name.eq_ignore_ascii_case("elemental") { TokenKind::KwElemental } else { TokenKind::KwPure };
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

use logos::Logos;

#[cfg(test)]
mod tests {
    use super::*;

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
    #[regex(r"&[ \t]*(?:![^\n]*)?\r?\n[ \t]*&?", logos::skip)]
    _Continuation,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    KwError,
    KwAllocate,
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
    #[token("len", ignore(ascii_case))]
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
    for start in 0..tokens.len() {
        if let TokenKind::Ident(n) = &tokens[start].kind {
            let next=tokens.get(start+1).map(|t| &t.kind);
            let previous=start.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind);
            let replacement=match n.to_ascii_lowercase().as_str() {
                "default" if matches!(previous,Some(TokenKind::KwCase)) => Some(TokenKind::KwDefault),
                "only" if matches!(next,Some(TokenKind::Colon)) && start >= 3 && matches!(tokens[start-3].kind,TokenKind::KwUse) => Some(TokenKind::KwOnly),
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
    tokens
}

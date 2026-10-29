// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Calls and labels are inspected before source normalization loses identity.
use super::*;
use logos::Logos;

fn intrinsic(name: &str) -> bool {
    crate::intrinsics::purity(name).is_some()
        || crate::named_arguments::intrinsic(name).is_some()
        || matches!(name, "execute_command_line" | "command_argument_count")
}

// FORMAT references have already been replaced by strings in the AST.
// Recover just the FMT control from the source tokens, never UNIT or data.
fn format_label(tokens: &[Token]) -> Option<u32> {
    let start = tokens.iter().position(|t| {
        matches!(
            t.kind,
            TokenKind::KwPrint
                | TokenKind::KwPrintEmpty
                | TokenKind::KwRead
                | TokenKind::KwReadEmpty
                | TokenKind::KwWrite
                | TokenKind::KwWriteEmpty
        )
    })?;
    let rest = &tokens[start + 1..];
    let number = |ts: &[Token]| match ts {
        [t] => {
            if let TokenKind::Integer(n) = &t.kind {
                n.parse().ok()
            } else {
                None
            }
        }
        _ => None,
    };
    if !matches!(rest.first().map(|t| &t.kind), Some(TokenKind::LParen)) {
        return rest.first().and_then(|t| number(std::slice::from_ref(t)));
    }
    let mut depth = 0;
    let mut begin = 1;
    let mut position = 0;
    for (i, t) in rest.iter().enumerate().skip(1) {
        match t.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen if depth > 0 => depth -= 1,
            TokenKind::Comma | TokenKind::RParen if depth == 0 => {
                let control = &rest[begin..i];
                let named = matches!(control.first().map(|t| &t.kind), Some(TokenKind::Ident(n)) if n == "fmt")
                    && matches!(control.get(1).map(|t| &t.kind), Some(TokenKind::Eq));
                if named {
                    return number(&control[2..]);
                }
                if position == 1 && control.len() == 1 {
                    if let Some(n) = number(control) {
                        return Some(n);
                    }
                }
                position += 1;
                begin = i + 1;
                if matches!(t.kind, TokenKind::RParen) {
                    break;
                }
            }
            _ => {}
        }
    }
    None
}
impl<'a> Analysis<'a> {
    fn procedure_span(&self, p: usize) -> Option<Range<usize>> {
        let procedure = &self.procedures[p];
        let declaration = self.decls.iter().find(|d| d.procedure == Some(p))?;
        let limit = procedure.body.iter().find_map(|s| {
            if let Stmt::SourceLocation(n) = s {
                Some(self.statement_tokens(*n).last().map_or(*n, |t| t.span.end))
            } else {
                None
            }
        });
        // Internal procedure nodes can inherit the preceding executable
        // statement's offset. Locate the header before the first body statement.
        let headers = self.tokens.windows(2).enumerate().filter(|(i, pair)| {
            pair[0].span.start >= declaration.span.start
                && limit.is_none_or(|end| pair[0].span.start <= end)
                && matches!(
                    pair[0].kind,
                    TokenKind::KwFunction | TokenKind::KwSubroutine
                )
                && (*i == 0 || !matches!(self.tokens[*i - 1].kind, TokenKind::KwEnd))
                && matches!(&pair[1].kind, TokenKind::Ident(n) if n == &procedure.name)
        });
        if limit.is_some() {
            headers.last().map(|(_, pair)| pair[1].span.clone())
        } else {
            headers
                .into_iter()
                .next()
                .map(|(_, pair)| pair[1].span.clone())
        }
        .or_else(|| Some(declaration.span.clone()))
    }
    fn explicit_interface(&self, scope: usize, name: &str) -> bool {
        let Some(id) = self.resolve(scope, name) else {
            return false;
        };
        let d = &self.decls[id];
        if d.attrs.contains("@generic")
            || d.attrs.contains("intrinsic")
            || d.attrs.iter().any(|a| a.starts_with("@interface:"))
        {
            return true;
        }
        for a in &d.attrs {
            if let Some(n) = a
                .strip_prefix("procedure(")
                .and_then(|s| s.strip_suffix(')'))
            {
                if self.resolve(d.scope, n).is_some_and(|id| {
                    self.decls[id]
                        .attrs
                        .iter()
                        .any(|a| a.starts_with("@abstract:") || a.starts_with("@interface:"))
                }) {
                    return true;
                }
            }
        }
        d.procedure
            .is_some_and(|p| self.scopes[self.procedures[p].scope].parent.is_some())
    }
    fn called_procedure(&self, scope: usize, name: &str) -> Option<usize> {
        if let Some(p) = self
            .resolve(scope, name)
            .and_then(|id| self.decls[id].procedure)
        {
            return Some(p);
        }
        let mut p = self
            .procedures
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name == name && self.scopes[p.scope].parent.is_none());
        let first = p.next()?.0;
        if p.next().is_none() {
            Some(first)
        } else {
            None
        }
    }
    fn requirement(&self, p: usize) -> Option<&'static str> {
        let p = &self.procedures[p];
        if p.elemental {
            return Some("the procedure is ELEMENTAL");
        }
        for &id in self.scopes[p.scope].names.values() {
            let d = &self.decls[id];
            if d.dummy {
                if d.attrs.contains("optional") {
                    return Some("a dummy argument is OPTIONAL");
                }
                if ["allocatable", "pointer", "value", "@polymorphic"]
                    .iter()
                    .any(|a| d.attrs.contains(*a))
                {
                    return Some(
                        "a dummy argument has an attribute requiring an explicit interface",
                    );
                }
                if d.dims.iter().any(
                    |e| matches!(e,Expr::Ident(n) if n=="@deferred_extent"||n=="@assumed_rank"),
                ) {
                    return Some("a dummy argument is assumed-shape or assumed-rank");
                }
            }
            if d.result {
                if !d.dims.is_empty()
                    || d.nominal.is_some()
                    || d.attrs.contains("pointer")
                    || d.attrs.contains("allocatable")
                {
                    return Some("the function result requires an explicit interface");
                }
                if matches!(&d.kind,Some(TypeSpec::CharacterLength(e)) if self.integer(e,p.scope,&mut HashSet::new()).is_none())
                {
                    return Some("the CHARACTER function result has a nonconstant length");
                }
            }
        }
        None
    }
    fn check_call(&mut self, name: &str, args: &[Expr], scope: usize, offset: usize) {
        if name.starts_with('@') || name.contains('%') {
            return;
        }
        if self.type_lookup(scope, name).is_some() {
            return;
        }
        if self
            .data_type(scope, name)
            .is_some_and(|(_, _, dims, _, _)| !dims.is_empty())
        {
            return;
        }
        if self
            .procedures
            .iter()
            .any(|p| p.name == name && self.ancestor(p.scope, scope))
        {
            return;
        }
        if self.explicit_interface(scope, name) {
            return;
        }
        let id = self.resolve(scope, name);
        let forced = id.is_some_and(|id| {
            let d = &self.decls[id];
            d.attrs.contains("external")
                || d.procedure.is_some()
                || d.dummy
                || d.attrs.iter().any(|a| a.starts_with("procedure("))
        });
        if intrinsic(name) && !forced {
            return;
        }
        let required = if crate::named_arguments::has_keywords(args) {
            Some("the call uses keyword arguments")
        } else {
            self.called_procedure(scope, name)
                .and_then(|p| self.requirement(p))
        };
        let span = self.span(offset, name);
        if let Some(reason) = required {
            self.interface_errors.push(Advice{category:"implicit-interface",span,
                message:format!("call to `{name}` requires an explicit interface because {reason}"),
                related:self.called_procedure(scope,name).and_then(|p|self.procedure_span(p)).map(|span|(span,"procedure defined here".into())),
                note:"make the procedure internal or module-associated, or provide a matching explicit interface; this is a language requirement, not an optional warning"});
        } else {
            self.warnings.push(Advice{category:"implicit-interface",span,
                message:format!("call to `{name}` has no explicit interface available at this call site"),related:None,
                note:"an implicit interface limits argument checking; use a module/internal procedure or supply a matching interface block"});
        }
    }
    fn call_exprs(&mut self, e: &Expr, scope: usize, offset: usize) {
        if let Expr::Call(n, a) = e {
            self.check_call(n, a, scope, offset);
        }
        for e in expressions(e) {
            self.call_exprs(e, scope, offset);
        }
    }
    pub(super) fn call_checks(&mut self, body: &'a [Stmt], scope: usize, mut offset: usize) {
        let scope = self
            .bodies
            .get(&(body.as_ptr() as usize))
            .copied()
            .unwrap_or(scope);
        for s in body {
            if let Stmt::SourceLocation(n) = s {
                offset = *n;
                continue;
            }
            if let Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } = s
            {
                self.call_checks(body, scope, offset);
                continue;
            }
            // Specification expressions and dead executable code still have
            // interface requirements; do not use reachability to excuse them.
            if let Stmt::CallSub { name, args } = s {
                self.check_call(name, args, scope, offset);
            }
            let mut copy = s.clone();
            for e in crate::procedure_arguments::expressions(&mut copy) {
                self.call_exprs(e, scope, offset);
            }
            for b in children(s) {
                self.call_checks(b, scope, offset);
            }
        }
    }
    pub(super) fn intrinsic_shadows(&mut self) {
        for d in &self.decls {
            if !intrinsic(&d.name) || d.attrs.contains("intrinsic") {
                continue;
            }
            if d.procedure.is_none()
                && !d.attrs.contains("external")
                && !d.attrs.contains("@generic")
                && !d
                    .attrs
                    .iter()
                    .any(|a| a.starts_with("procedure(") || a.starts_with("@interface:"))
            {
                continue;
            }
            let span = d
                .procedure
                .and_then(|p| self.procedure_span(p))
                .unwrap_or_else(|| d.span.clone());
            self.warnings.push(Advice{category:"intrinsic-shadow",span,
                message:format!("user procedure `{}` shadows a known intrinsic procedure",d.name),related:None,
                note:"use a distinct procedure name when intrinsic access is intended; shadowing may be deliberate and is legal"});
        }
    }
    /// Variables, dummy arguments and procedures named like Fortran
    /// statement keywords or attributes, as `block`, `exit` or `if`. Such
    /// names are legal, since Fortran has no reserved words, but make code
    /// harder to read. Intrinsic procedure names are -Wintrinsic-shadow's.
    pub(super) fn keyword_names(&mut self) {
        const KEYWORDS: &[&str] = &[
            "abstract", "allocatable", "allocate", "assign", "associate", "asynchronous", "backspace", "bind",
            "block", "call", "case", "character", "class", "close", "codimension", "common", "complex",
            "contains", "contiguous", "continue", "critical", "cycle", "data", "deallocate", "default",
            "deferred", "dimension", "do", "double", "elemental", "else", "elseif", "elsewhere", "end",
            "endfile", "entry", "enum", "enumerator", "equivalence", "error", "exit", "extends", "external",
            "final", "flush", "forall", "format", "function", "generic", "go", "goto", "if", "implicit",
            "import", "impure", "include", "inquire", "integer", "intent", "interface", "intrinsic", "logical",
            "module", "namelist", "non_overridable", "nopass", "nullify", "only", "open", "operator",
            "optional", "parameter", "pass", "pause", "pointer", "precision", "print", "private", "procedure",
            "program", "protected", "public", "pure", "read", "real", "recursive", "result", "return",
            "rewind", "save", "select", "sequence", "stop", "submodule", "subroutine", "target", "then",
            "type", "use", "value", "volatile", "wait", "where", "while", "write",
        ];
        for d in &self.decls {
            let name = d.name.to_ascii_lowercase();
            if !KEYWORDS.contains(&name.as_str()) || name.starts_with('@') {
                continue;
            }
            let span = d.procedure.and_then(|p| self.procedure_span(p)).unwrap_or_else(|| d.span.clone());
            let what = if d.procedure.is_some() { "procedure" } else if d.dummy { "dummy argument" } else { "variable" };
            self.warnings.push(Advice{category:"keyword-name",span,
                message:format!("{what} `{}` is named like the Fortran keyword {}",d.name,name.to_ascii_uppercase()),related:None,
                note:"Fortran has no reserved words, so this is legal, but a distinct name is clearer"});
        }
    }
    pub(super) fn unused_labels(&mut self, body: &'a [Stmt], offset: usize) {
        let mut labels = HashMap::new();
        let mut references = HashSet::new();
        let mut formats = HashSet::new();
        let mut dynamic_format = false;
        fn number(e: &Expr) -> Option<u32> {
            if let Expr::IntLit(n) = e {
                n.parse().ok()
            } else {
                None
            }
        }
        fn refs(e: &Expr, used: &mut HashSet<u32>) {
            if let Expr::Call(n, a) = e {
                if matches!(
                    n.as_str(),
                    "@io_spec:err"
                        | "@io_spec:end"
                        | "@io_spec:eor"
                        | "@write_option:err"
                        | "@write_option:end"
                        | "@write_option:eor"
                ) {
                    if let Some(n) = a.first().and_then(number) {
                        used.insert(n);
                    }
                }
            }
            for child in expressions(e) {
                refs(child, used);
            }
        }
        fn collect<'a>(
            a: &mut Analysis<'a>,
            body: &'a [Stmt],
            mut offset: usize,
            labels: &mut HashMap<u32, Range<usize>>,
            used: &mut HashSet<u32>,
            formats: &mut HashSet<u32>,
            dynamic: &mut bool,
        ) {
            for (i, s) in body.iter().enumerate() {
                if let Stmt::SourceLocation(n) = s {
                    offset = *n;
                    continue;
                }
                if let Stmt::Function { body, .. }
                | Stmt::Subroutine { body, .. }
                | Stmt::Module { body, .. } = s
                {
                    a.unused_labels(body, offset);
                    continue;
                }
                if let Stmt::CallSub { name, args } = s {
                    if name == "@statement_label" {
                        if let Some(n) = args.first().and_then(number) {
                            let span = a
                                .statement_tokens(offset)
                                .iter()
                                .find(|t| matches!(t.kind, TokenKind::StatementLabel(_)))
                                .map(|t| t.span.clone())
                                .unwrap_or(offset..offset);
                            labels.insert(n, span);
                            if a.statement_tokens(offset)
                                .iter()
                                .any(|t| matches!(t.kind, TokenKind::FormatSpec(_)))
                                || matches!(body.get(i+1),Some(Stmt::CallSub{name,..}) if name=="@format_statement")
                            {
                                formats.insert(n);
                            }
                        }
                    }
                    if name == "@goto" {
                        if let Some(n) = args.first().and_then(number) {
                            used.insert(n);
                        }
                    }
                    let fmt = match name.as_str() {
                        "@formatted_print" => Some(0),
                        "@write"
                        | "@formatted_read"
                        | "@formatted_read_status"
                        | "@formatted_write_noadvance" => Some(1),
                        _ => None,
                    };
                    if let Some(fmt) = fmt.and_then(|i| args.get(i)) {
                        if let Some(n) = number(fmt) {
                            used.insert(n);
                        } else if !matches!(fmt, Expr::Str(_))
                            && !matches!(fmt, Expr::Call(n, _) if matches!(n.as_str(), "@write_star" | "@list_directed"))
                        {
                            *dynamic = true;
                        }
                    }
                    let control = match name.as_str() {
                        "@empty_read" | "@internal_read" => Some(1),
                        "@formatted_read_status" => Some(2),
                        _ => None,
                    };
                    if let Some(i) = control {
                        if matches!(args.get(i),Some(Expr::Str(s)) if matches!(s.as_str(),"end"|"err"|"eor"))
                        {
                            if let Some(n) = args.get(i + 1).and_then(number) {
                                used.insert(n);
                            }
                        }
                    }
                }
                let mut copy = s.clone();
                for e in crate::procedure_arguments::expressions(&mut copy) {
                    refs(e, used);
                }
                if let Some(n) = format_label(a.statement_tokens(offset)) {
                    used.insert(n);
                }
                // The lexer removes a labeled DO's terminal label. Read only
                // the gap following its DO token, using the raw lexer so that
                // comments and continuation lines cannot look like numbers.
                for pair in a.statement_tokens(offset).windows(2) {
                    if matches!(pair[0].kind, TokenKind::KwDo)
                        && pair[0].span.end <= pair[1].span.start
                    {
                        let mut lex =
                            TokenKind::lexer(&a.src[pair[0].span.end..pair[1].span.start]);
                        if let Some(Ok(TokenKind::Integer(n))) = lex.next() {
                            if let Ok(n) = n.parse() {
                                used.insert(n);
                            }
                        }
                    }
                }
                for b in children(s) {
                    collect(a, b, offset, labels, used, formats, dynamic);
                }
            }
        }
        collect(
            self,
            body,
            offset,
            &mut labels,
            &mut references,
            &mut formats,
            &mut dynamic_format,
        );
        for (n, span) in labels {
            if references.contains(&n) || dynamic_format && formats.contains(&n) {
                continue;
            }
            self.warnings.push(Advice{category:"unused-label",span,message:format!("statement label {n} is never referenced in this program unit"),related:None,
                note:"remove the label if it is unnecessary; ordinary execution of a labeled statement is not a reference to its label"});
        }
    }
}

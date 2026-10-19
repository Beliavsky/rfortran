use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use crate::ast::{CaseItem, Expr, Implicit, LetterRange, Program, Stmt, TypeSpec};
use crate::errors::{CompileError, CompileErrorKind};
use crate::lexer::{Token, TokenKind};

fn show_type(type_spec: &TypeSpec) -> String {
    match type_spec {
        TypeSpec::Complex(kind) => format!("COMPLEX({kind})"),
        TypeSpec::RealKind(kind) => format!("REAL({kind})"),
        TypeSpec::LogicalKind(kind) => format!("LOGICAL({kind})"),
        TypeSpec::IntegerKind(kind) => format!("INTEGER({kind})"),
        TypeSpec::Integer(None) => "INTEGER(4)".to_string(),
        TypeSpec::Integer(Some(kind)) => format!("INTEGER({})", kind),
        TypeSpec::Real => "REAL(4)".to_string(),
        TypeSpec::DoublePrecision => "REAL(8)".to_string(),
        TypeSpec::Logical => "LOGICAL".to_string(),
        TypeSpec::CharacterLength(e) => format!("CHARACTER(len={e:?})"),
        TypeSpec::CharacterAssumed => "CHARACTER(len=*)".to_string(),
        TypeSpec::Character(None) => "CHARACTER".to_string(),
        TypeSpec::Character(Some(len)) => format!("CHARACTER(len={})", len),
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImplicitTyping {
    pub disabled: bool,
    pub rules: Vec<ImplicitRule>,
}

#[derive(Debug, Clone)]
pub struct ImplicitRule {
    pub type_spec: TypeSpec,
    pub letter_ranges: Vec<LetterRange>,
    pub span: std::ops::Range<usize>,
}

impl ImplicitTyping {
    pub fn new() -> Self {
        Self {
            disabled: false,
            rules: Vec::new(),
        }
    }
    pub fn apply_implicit_none(&mut self) {
        self.disabled = true;
    }

    pub fn add_rule(
        &mut self,
        type_spec: TypeSpec,
        letter_ranges: Vec<LetterRange>,
        span: std::ops::Range<usize>,
    ) {
        self.rules.push(ImplicitRule {
            type_spec,
            letter_ranges,
            span,
        });
    }

    pub fn get_implicit_type(&self, var_name: &str) -> Option<TypeSpec> {
        if self.disabled {
            return None;
        }

        let first_char = var_name.chars().next()?.to_ascii_lowercase();

        for rule in &self.rules {
            for range in &rule.letter_ranges {
                let matches = if let Some(end_char) = range.end {
                    first_char >= range.start && first_char <= end_char
                } else {
                    first_char == range.start
                };

                if matches {
                    return Some(rule.type_spec.clone());
                }
            }
        }

        if matches!(first_char, 'i' | 'j' | 'k' | 'l' | 'm' | 'n') {
            Some(TypeSpec::Integer(None))
        } else {
            Some(TypeSpec::Real)
        }
    }

    pub fn get_implicit_type_with_span(
        &self,
        var_name: &str,
    ) -> Option<(TypeSpec, Option<std::ops::Range<usize>>)> {
        if self.disabled {
            return None;
        }

        let first_char = var_name.chars().next()?.to_ascii_lowercase();

        for rule in &self.rules {
            for range in &rule.letter_ranges {
                let matches = if let Some(end_char) = range.end {
                    first_char >= range.start && first_char <= end_char
                } else {
                    first_char == range.start
                };

                if matches {
                    return Some((rule.type_spec.clone(), Some(rule.span.clone())));
                }
            }
        }

        let default_type = if matches!(first_char, 'i' | 'j' | 'k' | 'l' | 'm' | 'n') {
            TypeSpec::Integer(None)
        } else {
            TypeSpec::Real
        };
        Some((default_type, None))
    }
}

#[derive(Debug, Clone, Default)]
pub struct SemaSettings {
    pub allow: HashSet<String>,
    pub deny: HashSet<String>,
    pub error: HashSet<String>,
    pub wall: bool,
    pub werror: bool,
}

pub fn parse_directives(src: &str) -> SemaSettings {
    let mut s = SemaSettings::default();
    for line in src.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("!#") {
            let r = rest.trim();
            for (kw, set) in [
                ("allow", &mut s.allow),
                ("deny", &mut s.deny),
                ("error", &mut s.error),
            ] {
                let prefix = format!("{}(", kw);
                if r.starts_with(&prefix) && r.ends_with(')') {
                    let inner = &r[prefix.len()..r.len() - 1];
                    let name = inner.trim().to_ascii_lowercase();
                    set.insert(name);
                }
            }
        }
    }
    s
}

fn warn_or_error(
    kind_name: &str,
    message: String,
    span: std::ops::Range<usize>,
    settings: &SemaSettings,
    stderr: &mut codespan_reporting::term::termcolor::StandardStream,
    file: &codespan_reporting::files::SimpleFile<&str, &str>,
    errors: &mut Vec<CompileError>,
) {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use codespan_reporting::term::{emit_to_io_write, Config};
    let kn = kind_name.to_ascii_lowercase();

    if settings.allow.contains(&kn) {
        return;
    }

    let treat_as_error =
        settings.werror || settings.error.contains(&kn) || settings.deny.contains(&kn);
    if treat_as_error {
        let diag = Diagnostic::error()
            .with_message(&message)
            .with_labels(vec![Label::primary((), span.clone())]);
        let _ = emit_to_io_write(stderr, &Config::default(), file, &diag);
        errors.push(CompileError::new(CompileErrorKind::Semantic, message, span));
    } else {
        let diag = Diagnostic::warning()
            .with_message(&message)
            .with_labels(vec![Label::primary((), span.clone())]);
        let _ = emit_to_io_write(stderr, &Config::default(), file, &diag);
        errors.push(CompileError::new(CompileErrorKind::Warning, message, span));
    }
}
fn report_error(
    message: &str,
    span: std::ops::Range<usize>,
    stderr: &mut codespan_reporting::term::termcolor::StandardStream,
    file: &codespan_reporting::files::SimpleFile<&str, &str>,
    errors: &mut Vec<CompileError>,
    silent_undefined: bool,
) {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use codespan_reporting::term::{emit_to_io_write, Config};

    let is_undefined = message.ends_with("@UNDEFINED");
    let clean_message = if is_undefined {
        &message[..message.len() - "@UNDEFINED".len()]
    } else {
        message
    };

    if !(silent_undefined && is_undefined) {
        let diag = Diagnostic::error()
            .with_message(clean_message)
            .with_labels(vec![Label::primary((), span.clone())]);
        let _ = emit_to_io_write(stderr, &Config::default(), file, &diag);
    }

    errors.push(CompileError::new(
        CompileErrorKind::Semantic,
        clean_message,
        span,
    ));
}

fn report_error_with_note(
    message: &str,
    primary_span: std::ops::Range<usize>,
    note_message: &str,
    note_span: std::ops::Range<usize>,
    stderr: &mut codespan_reporting::term::termcolor::StandardStream,
    file: &codespan_reporting::files::SimpleFile<&str, &str>,
    errors: &mut Vec<CompileError>,
    silent_undefined: bool,
) {
    use codespan_reporting::diagnostic::{Diagnostic, Label};
    use codespan_reporting::term::{emit_to_io_write, Config};

    let is_undefined = message.ends_with("@UNDEFINED");
    let clean_message = if is_undefined {
        &message[..message.len() - "@UNDEFINED".len()]
    } else {
        message
    };

    if !(silent_undefined && is_undefined) {
        let diag = Diagnostic::error()
            .with_message(clean_message)
            .with_labels(vec![
                Label::primary((), primary_span.clone()),
                Label::secondary((), note_span.clone()).with_message(note_message),
            ]);
        let _ = emit_to_io_write(stderr, &Config::default(), file, &diag);
    }

    errors.push(CompileError::new(
        CompileErrorKind::Semantic,
        clean_message,
        primary_span,
    ));
}

pub fn analyze_with_src(
    program: &Program,
    src: &str,
    tokens: &[Token],
    filename: &str,
    settings: &SemaSettings,
    linked_obj_exports: &HashSet<String>,
    silent_undefined: bool,
) -> Vec<CompileError> {
    let mut sym: HashMap<String, TypeSpec> = HashMap::new();
    let mut errors: Vec<CompileError> = Vec::new();
    let mut implicit_typing = ImplicitTyping::new();

    fn find_assign_rhs_span(
        tokens: &[Token],
        name: &str,
        rhs: &Expr,
    ) -> Option<std::ops::Range<usize>> {
        let lname = name.to_ascii_lowercase();
        let mut i = 0usize;
        while i + 2 < tokens.len() {
            if let TokenKind::Ident(id) = &tokens[i].kind {
                if id.eq_ignore_ascii_case(&lname) {
                    if matches!(tokens[i + 1].kind, TokenKind::Eq) {
                        match (rhs, &tokens[i + 2].kind) {
                            (Expr::Str(s), TokenKind::Str(ts)) if s == ts => {
                                return Some(tokens[i + 2].span.clone())
                            }
                            (Expr::IntLit(s), TokenKind::Integer(ts)) if s == ts => {
                                return Some(tokens[i + 2].span.clone())
                            }
                            (Expr::RealLit(s), TokenKind::Float(tf)) if s == tf => {
                                return Some(tokens[i + 2].span.clone())
                            }
                            (Expr::Ident(id2), TokenKind::Ident(tid))
                                if id2.eq_ignore_ascii_case(tid) =>
                            {
                                return Some(tokens[i + 2].span.clone())
                            }
                            (Expr::Logical(b), TokenKind::True) if *b => {
                                return Some(tokens[i + 2].span.clone())
                            }
                            (Expr::Logical(b), TokenKind::False) if !*b => {
                                return Some(tokens[i + 2].span.clone())
                            }
                            _ => {}
                        }
                    }
                }
            }
            i += 1;
        }
        None
    }

    fn find_assign_lhs_span(tokens: &[Token], name: &str) -> Option<std::ops::Range<usize>> {
        let lname = name.to_ascii_lowercase();
        let mut i = 0usize;
        while i + 1 < tokens.len() {
            if let TokenKind::Ident(id) = &tokens[i].kind {
                if id.eq_ignore_ascii_case(&lname) {
                    if matches!(tokens[i + 1].kind, TokenKind::Eq) {
                        return Some(tokens[i].span.clone());
                    }
                }
            }
            i += 1;
        }
        None
    }

    use codespan_reporting::files::SimpleFile;
    use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    let file = SimpleFile::new(filename, src);
    for stmt in &program.body {
        match stmt {
            Stmt::VarDecl { kind, names } => {
                if let TypeSpec::Integer(Some(k)) = kind {
                    if !matches!(*k, 1 | 2 | 4 | 8 | 16) {
                        let span = tokens
                            .iter()
                            .position(|t| matches!(t.kind, TokenKind::KwInteger))
                            .map(|i| tokens.get(i + 2).map(|t| t.span.clone()).unwrap_or(0..0))
                            .unwrap_or(0..0);
                        let msg = format!("invalid INTEGER kind {} (allowed: 1, 2, 4, 8, 16)", k);
                        warn_or_error(
                            "invalid_integer_kind",
                            msg,
                            span,
                            settings,
                            &mut stderr,
                            &file,
                            &mut errors,
                        );
                    }
                }

                for n in names {
                    sym.insert(n.to_ascii_lowercase(), kind.clone());
                }
            }

            Stmt::ArrayDecl { kind, name, .. } => {
                sym.insert(name.to_ascii_lowercase(), kind.clone());
            }

            Stmt::Do { var, .. } => {
                sym.insert(var.to_ascii_lowercase(), TypeSpec::Integer(None));
            }
            Stmt::Implicit(imp) => match imp {
                Implicit::None => {
                    implicit_typing.apply_implicit_none();
                }
                Implicit::Rule {
                    type_spec,
                    letter_ranges,
                } => {
                    let implicit_span = tokens
                        .iter()
                        .position(|t| matches!(t.kind, TokenKind::KwImplicit))
                        .map(|i| {
                            let start = tokens[i].span.start;
                            let end = tokens
                                .iter()
                                .skip(i)
                                .find(|t| matches!(t.kind, TokenKind::RParen))
                                .map(|t| t.span.end)
                                .unwrap_or(tokens[i].span.end);
                            start..end
                        })
                        .unwrap_or(0..0);

                    implicit_typing.add_rule(
                        type_spec.clone(),
                        letter_ranges.clone(),
                        implicit_span,
                    );
                }
                Implicit::Rules { rules } => {
                    let implicit_span = tokens
                        .iter()
                        .position(|t| matches!(t.kind, TokenKind::KwImplicit))
                        .map(|i| {
                            let start = tokens[i].span.start;

                            let end = tokens
                                .iter()
                                .skip(i)
                                .filter(|t| matches!(t.kind, TokenKind::RParen))
                                .last()
                                .map(|t| t.span.end)
                                .unwrap_or(tokens[i].span.end);
                            start..end
                        })
                        .unwrap_or(0..0);

                    for rule in rules {
                        implicit_typing.add_rule(
                            rule.type_spec.clone(),
                            rule.letter_ranges.clone(),
                            implicit_span.clone(),
                        );
                    }
                }
            },
            _ => {}
        }
    }

    fn collect_do_vars(s: &Stmt, out: &mut HashMap<String, TypeSpec>) {
        match s {
            Stmt::Do { var, body, .. } => {
                out.insert(var.to_ascii_lowercase(), TypeSpec::Integer(None));
                for st in body {
                    collect_do_vars(st, out);
                }
            }
            Stmt::DoWhile { body, .. } => {
                for st in body {
                    collect_do_vars(st, out);
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for st in then_body {
                    collect_do_vars(st, out);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        collect_do_vars(st, out);
                    }
                }
            }
            Stmt::Block { body } => {
                for st in body {
                    collect_do_vars(st, out);
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    for st in &c.body {
                        collect_do_vars(st, out);
                    }
                }
                if let Some(d) = default {
                    for st in d {
                        collect_do_vars(st, out);
                    }
                }
            }

            Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
            _ => {}
        }
    }
    for stmt in &program.body {
        collect_do_vars(stmt, &mut sym);
    }

    fn collect_assignment_targets_early(
        s: &Stmt,
        sym: &mut HashMap<String, TypeSpec>,
        implicit_typing: &ImplicitTyping,
    ) {
        match s {
            Stmt::Assign { name, .. } => {
                let lname = name.to_ascii_lowercase();
                if !sym.contains_key(&lname) {
                    if let Some(implicit_type) = implicit_typing.get_implicit_type(&lname) {
                        sym.insert(lname, implicit_type);
                    }
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for st in then_body {
                    collect_assignment_targets_early(st, sym, implicit_typing);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        collect_assignment_targets_early(st, sym, implicit_typing);
                    }
                }
            }
            Stmt::Do { body, .. } => {
                for st in body {
                    collect_assignment_targets_early(st, sym, implicit_typing);
                }
            }
            Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } => {
                for st in body {
                    collect_assignment_targets_early(st, sym, implicit_typing);
                }
            }
            _ => {}
        }
    }

    for stmt in &program.body {
        collect_assignment_targets_early(stmt, &mut sym, &implicit_typing);
    }

    fn check_loop_usage(
        stmt: &Stmt,
        loop_depth: usize,
        tokens: &[Token],
        stderr: &mut codespan_reporting::term::termcolor::StandardStream,
        file: &codespan_reporting::files::SimpleFile<&str, &str>,
        errors: &mut Vec<CompileError>,
    ) {
        match stmt {
            Stmt::Exit => {
                if loop_depth == 0 {
                    let span = tokens
                        .iter()
                        .find(|t| matches!(t.kind, TokenKind::KwExit))
                        .map(|t| t.span.clone())
                        .unwrap_or(0..0);
                    report_error(
                        "EXIT used outside of a loop",
                        span,
                        stderr,
                        file,
                        errors,
                        false,
                    );
                }
            }
            Stmt::Cycle => {
                if loop_depth == 0 {
                    let span = tokens
                        .iter()
                        .find(|t| matches!(t.kind, TokenKind::KwCycle))
                        .map(|t| t.span.clone())
                        .unwrap_or(0..0);
                    report_error(
                        "CYCLE used outside of a loop",
                        span,
                        stderr,
                        file,
                        errors,
                        false,
                    );
                }
            }
            Stmt::Do { var: _, body, .. } => {
                if !tokens.is_empty() {}
                for st in body {
                    check_loop_usage(st, loop_depth + 1, tokens, stderr, file, errors);
                }
            }
            Stmt::DoWhile { body, .. } => {
                for st in body {
                    check_loop_usage(st, loop_depth + 1, tokens, stderr, file, errors);
                }
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                for st in then_body {
                    check_loop_usage(st, loop_depth, tokens, stderr, file, errors);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        check_loop_usage(st, loop_depth, tokens, stderr, file, errors);
                    }
                }
            }
            Stmt::Block { body } => {
                for st in body {
                    check_loop_usage(st, loop_depth, tokens, stderr, file, errors);
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    for st in &c.body {
                        check_loop_usage(st, loop_depth, tokens, stderr, file, errors);
                    }
                }
                if let Some(d) = default {
                    for st in d {
                        check_loop_usage(st, loop_depth, tokens, stderr, file, errors);
                    }
                }
            }
            Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
            _ => {}
        }
    }
    for stmt in &program.body {
        check_loop_usage(stmt, 0, tokens, &mut stderr, &file, &mut errors);
    }

    let mut known_intrinsics: HashMap<String, TypeSpec> = HashMap::new();
    for (name, _) in crate::intrinsics::REAL_MATH {
        known_intrinsics.insert(name.to_string(), TypeSpec::Real);
    }
    known_intrinsics.insert("real".to_string(), TypeSpec::Real);
    known_intrinsics.insert("dble".to_string(), TypeSpec::DoublePrecision);
    known_intrinsics.insert("@ptr_load_i".into(), TypeSpec::Integer(Some(8)));
    known_intrinsics.insert("@ptr_load_r4".into(), TypeSpec::Real);
    known_intrinsics.insert("@ptr_load_r8".into(), TypeSpec::DoublePrecision);
    known_intrinsics.insert("@ptr_load_l".into(), TypeSpec::Logical);
    known_intrinsics.insert("@ptr_name".into(), TypeSpec::Character(None));
    known_intrinsics.insert("@cpu_seconds".into(), TypeSpec::DoublePrecision);
    known_intrinsics.insert("@clock_count".into(), TypeSpec::Integer(Some(8)));
    let mut external_fn_ret: HashMap<String, TypeSpec> = HashMap::new();
    for name in [
        "int", "nint", "floor", "ceiling", "ichar", "iachar", "len", "len_trim", "count", "size", "lbound", "ubound",
        "maxloc", "minloc", "index", "scan", "verify", "abs", "max", "min",
    ] {
        known_intrinsics.insert(name.to_string(), TypeSpec::Integer(None));
    }
    for name in ["all", "any", "allocated", "lge", "lgt", "lle", "llt"].into_iter().chain(crate::intrinsics::REAL_PREDICATES.iter().copied()) {
        known_intrinsics.insert(name.to_string(), TypeSpec::Logical);
    }
    for name in [
        "achar",
        "@substring",
        "@dynamic_substring",
        "char",
        "repeat",
        "trim",
        "adjustl",
        "adjustr",
        "reshape",
        "transpose",
        "spread",
        "pack",
        "unpack",
        "merge",
    ] {
        known_intrinsics.insert(name.to_string(), TypeSpec::Character(None));
    }
    for name in [
        "btest", "iand", "ibclr", "ibits", "ibset", "ieor", "ior", "ishft", "ishftc", "not", "mod",
        "modulo", "sign", "popcnt", "poppar", "leadz", "trailz", "dshiftl", "dshiftr", "bit_size", "iany", "iall", "iparity",
    ] {
        known_intrinsics.insert(name.to_string(), TypeSpec::Integer(None));
    }

    known_intrinsics.insert("btest".into(),TypeSpec::Logical);
    known_intrinsics.insert("parity".into(),TypeSpec::Logical);
    known_intrinsics.insert("out_of_range".into(),TypeSpec::Logical);
    for name in crate::real_intrinsics::NAMES {
        for kind in [4,8] {
            let tag=crate::real_intrinsics::tag(name,kind);
            known_intrinsics.insert(tag.clone(),crate::real_intrinsics::tag_kind(&tag).unwrap());
        }
    }
    for name in crate::bit_intrinsics::NAMES {
        for bytes in [1,2,4,8] {
            let tag=crate::bit_intrinsics::tag(name,bytes);
            known_intrinsics.insert(tag.clone(),crate::bit_intrinsics::tag_kind(&tag).unwrap());
        }
    }
    for (k, v) in known_intrinsics {
        external_fn_ret.entry(k).or_insert(v);
    }
    let mut fn_name_set: HashSet<String> = HashSet::new();
    for stmt in &program.body {
        match stmt {
            Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } => {
                fn_name_set.insert(name.to_ascii_lowercase());
            }
            Stmt::Module { body, .. } => {
                for member in body {
                    if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = member {
                        fn_name_set.insert(name.to_ascii_lowercase());
                    }
                }
            }
            _ => {}
        }
    }
    for stmt in &program.body {
        if let Stmt::VarDecl { kind, names } = stmt {
            for n in names {
                let ln = n.to_ascii_lowercase();
                if fn_name_set.contains(&ln) {
                    external_fn_ret.insert(ln, kind.clone());
                }
            }
        }
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ParamIntent {
        None,
        In,
        Out,
        InOut,
    }

    #[derive(Clone)]
    struct FnInfo {
        return_type: Option<TypeSpec>,
        is_function: bool,
        params: Vec<String>,
        body: Vec<Stmt>,
    }
    let mut fn_map: HashMap<String, FnInfo> = HashMap::new();
    let mut module_fn_names: HashSet<String> = HashSet::new();
    let mut param_intent_map: HashMap<String, ParamIntent> = HashMap::new();
    let mut program_level_reads: HashSet<String> = HashSet::new();
    fn walk_reads_for_program(e: &Expr, out: &mut HashSet<String>) {
        match e {
            Expr::Ident(id) => {
                out.insert(id.to_ascii_lowercase());
            }
            Expr::Call(name, args) | Expr::Index(name, args) => {
                out.insert(name.to_ascii_lowercase());
                for a in args {
                    walk_reads_for_program(a, out);
                }
            }
            Expr::Bin(_, l, r) => {
                walk_reads_for_program(l, out);
                walk_reads_for_program(r, out);
            }
            Expr::Un(_, inner) => walk_reads_for_program(inner, out),
            _ => {}
        }
    }
    fn walk_stmt_program_reads(s: &Stmt, out: &mut HashSet<String>) {
        match s {
            Stmt::Block {body} => {
                let mut reads=HashSet::new();
                for stmt in body {walk_stmt_program_reads(stmt,&mut reads);}
                // Only references to enclosing variables escape the block's scope.
                for stmt in body {match stmt {
                    Stmt::VarDecl {names,..} => for name in names {reads.remove(&name.to_ascii_lowercase());},
                    Stmt::ArrayDecl {name,..} => {reads.remove(&name.to_ascii_lowercase());},
                    _=>{}
                }}
                // Lowering also declares private temporaries inside loops/branches;
                // they are not source-level references to the enclosing program.
                reads.retain(|name| !name.starts_with('@'));
                out.extend(reads);
            }
            Stmt::DummyIntent{name,intent} if matches!(intent.as_str(), "@forall_index" | "@constructor_index") => {out.insert(name.to_ascii_lowercase());}
            Stmt::Assign { value, .. } => walk_reads_for_program(value, out),
            Stmt::AssignIndex {
                name,
                indices,
                value,
            } => {
                out.insert(name.to_ascii_lowercase());
                for idx in indices {
                    walk_reads_for_program(idx, out);
                }
                walk_reads_for_program(value, out);
            }
            Stmt::Print { items } => {
                for it in items {
                    walk_reads_for_program(it, out);
                }
            }
            Stmt::CallSub { name: _, args } => {
                for a in args {
                    walk_reads_for_program(a, out);
                }
            }
            Stmt::SelectCase {
                expr,
                cases,
                default,
            } => {
                walk_reads_for_program(expr, out);
                for c in cases {
                    for item in &c.items {
                        match item {
                            CaseItem::Range(l, r) => {
                                walk_reads_for_program(l, out);
                                walk_reads_for_program(r, out);
                            }
                            CaseItem::Single(e) => walk_reads_for_program(e, out),
                        }
                    }
                    for st in &c.body {
                        walk_stmt_program_reads(st, out);
                    }
                }
                if let Some(d) = default {
                    for st in d {
                        walk_stmt_program_reads(st, out);
                    }
                }
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                walk_reads_for_program(cond, out);
                for st in then_body {
                    walk_stmt_program_reads(st, out);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        walk_stmt_program_reads(st, out);
                    }
                }
            }
            Stmt::Do {
                var, start, end, body, ..
            } => {
                out.insert(var.to_ascii_lowercase());
                walk_reads_for_program(start, out);
                walk_reads_for_program(end, out);
                for st in body {
                    walk_stmt_program_reads(st, out);
                }
            }
            Stmt::DoWhile { cond, body } => {
                walk_reads_for_program(cond, out);
                for st in body {
                    walk_stmt_program_reads(st, out);
                }
            }

            Stmt::Function { .. } | Stmt::Subroutine { .. } | Stmt::Module { .. } => {}
            _ => {}
        }
    }
    for stmt in &program.body {
        walk_stmt_program_reads(stmt, &mut program_level_reads);
    }
    // Lowering declares its temporaries inside the branch or loop that uses them.
    program_level_reads.retain(|name| !name.starts_with('@'));

    for name in &program_level_reads {
        let lname = name.to_ascii_lowercase();

        if sym.contains_key(&lname)
            || fn_map.contains_key(&lname)
            || fn_name_set.contains(&lname)
            || module_fn_names.contains(&lname)
            || external_fn_ret.contains_key(&lname)
            || linked_obj_exports.contains(&lname)
        {
            continue;
        }

        let use_idx_opt = tokens.iter().rposition(|t| match &t.kind {
            TokenKind::Ident(s) => s.eq_ignore_ascii_case(name),
            _ => false,
        });
        let use_span = use_idx_opt.map(|i| tokens[i].span.clone()).unwrap_or(0..0);

        let mut block_stack: Vec<usize> = Vec::new();
        let mut block_ranges: Vec<(usize, usize)> = Vec::new();
        for i in 0..tokens.len() {
            match &tokens[i].kind {
                TokenKind::KwBlock => block_stack.push(i),
                TokenKind::KwEnd => {
                    if i + 1 < tokens.len() {
                        if let TokenKind::KwBlock = &tokens[i + 1].kind {
                            if let Some(sidx) = block_stack.pop() {
                                block_ranges.push((sidx, i + 1));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut integer_decl_opt: Option<std::ops::Range<usize>> = None;
        let mut typed_decl_opt: Option<std::ops::Range<usize>> = None;
        let mut do_decl_opt: Option<std::ops::Range<usize>> = None;
        let mut fallback_decl_opt: Option<std::ops::Range<usize>> = None;
        if let Some(use_idx) = use_idx_opt {
            'outer: for (sidx, eidx) in &block_ranges {
                if *eidx >= use_idx {
                    continue;
                }
                for j in *sidx..=*eidx {
                    if let TokenKind::Ident(id) = &tokens[j].kind {
                        if id.eq_ignore_ascii_case(name) {
                            let start_check = if j >= 6 { j - 6 } else { 0 };

                            let mut seen_integer = false;
                            for k in start_check..=j {
                                if let TokenKind::KwInteger = &tokens[k].kind {
                                    seen_integer = true;
                                    break;
                                }
                            }
                            if seen_integer {
                                integer_decl_opt = Some(tokens[j].span.clone());
                                break 'outer;
                            }

                            let mut seen_type = false;
                            for k in start_check..=j {
                                match &tokens[k].kind {
                                    TokenKind::KwReal
                                    | TokenKind::KwDouble
                                    | TokenKind::KwCharacter
                                    | TokenKind::KwLogical
                                    | TokenKind::DColon => {
                                        seen_type = true;
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            if seen_type && typed_decl_opt.is_none() {
                                typed_decl_opt = Some(tokens[j].span.clone());
                            }

                            if j >= 1 {
                                if let TokenKind::KwDo = &tokens[j - 1].kind {
                                    if do_decl_opt.is_none() {
                                        do_decl_opt = Some(tokens[j].span.clone());
                                    }
                                }
                            }

                            if fallback_decl_opt.is_none() {
                                fallback_decl_opt = Some(tokens[j].span.clone());
                            }
                        }
                    }
                }
            }
        }

        let decl_span_opt: Option<std::ops::Range<usize>> = if integer_decl_opt.is_some() {
            integer_decl_opt
        } else if typed_decl_opt.is_some() {
            typed_decl_opt
        } else if do_decl_opt.is_some() {
            do_decl_opt
        } else {
            fallback_decl_opt
        };

        use codespan_reporting::diagnostic::{Diagnostic, Label};
        use codespan_reporting::term::{emit_to_io_write, Config};
        if let Some(decl_span) = decl_span_opt {
            let msg = format!(
                "undefined variable `{}` (declared in an inner BLOCK)@UNDEFINED",
                name
            );
            let is_undefined = msg.ends_with("@UNDEFINED");
            let clean_msg = if is_undefined {
                &msg[..msg.len() - "@UNDEFINED".len()]
            } else {
                &msg
            };

            if !(silent_undefined && is_undefined) {
                let diag = Diagnostic::error()
                    .with_message(clean_msg)
                    .with_labels(vec![
                        Label::primary((), use_span.clone()),
                        Label::secondary((), decl_span.clone()),
                    ]);
                let _ = emit_to_io_write(&mut stderr, &Config::default(), &file, &diag);
            }
            errors.push(CompileError::new(
                CompileErrorKind::Semantic,
                clean_msg,
                use_span,
            ));
        } else {
            report_error(
                &format!("undefined variable `{}`@UNDEFINED", name),
                use_span,
                &mut stderr,
                &file,
                &mut errors,
                silent_undefined,
            );
        }
    }

    let find_ident_span = |name: &str| -> std::ops::Range<usize> {
        if let Some(t) = tokens.iter().find(|t| match &t.kind {
            TokenKind::Ident(s) => s.eq_ignore_ascii_case(name),
            _ => false,
        }) {
            t.span.clone()
        } else {
            0..0
        }
    };
    let mut type_span_map: HashMap<String, std::ops::Range<usize>> = HashMap::new();
    let find_type_keyword_before = |ident_index: usize| -> Option<std::ops::Range<usize>> {
        let mut j = ident_index;
        while j > 0 {
            j -= 1;
            if !crate::lexer::same_statement(&src[tokens[j].span.end..tokens[j+1].span.start]) {
                break;
            }
            match tokens[j].kind {
                TokenKind::KwInteger
                | TokenKind::KwReal
                | TokenKind::KwDouble
                | TokenKind::KwCharacter
                | TokenKind::KwLogical => {
                    return Some(tokens[j].span.clone());
                }
                _ => {
                    if matches!(
                        tokens[j].kind,
                        TokenKind::KwProgram
                            | TokenKind::KwFunction
                            | TokenKind::KwSubroutine
                            | TokenKind::KwIf
                            | TokenKind::KwDo
                            | TokenKind::KwDoUnbounded
                            | TokenKind::KwEnd
                    ) {
                        break;
                    }
                }
            }
        }
        None
    };
    for stmt in &program.body {
        if let Stmt::VarDecl { kind: _, names } = stmt {
            for n in names {
                let lname = n.to_ascii_lowercase();
                if !type_span_map.contains_key(&lname) {
                    if let Some(tok_idx) = tokens.iter().position(
                        |t| matches!(&t.kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(n)),
                    ) {
                        if let Some(ts) = find_type_keyword_before(tok_idx) {
                            type_span_map.insert(lname.clone(), ts.clone());
                            if let Some(type_tok_idx) = tokens.iter().position(|t| t.span == ts) {
                                let mut intent_found = ParamIntent::None;
                                let mut i = type_tok_idx + 1;
                                while i < tokens.len() {
                                    if !crate::lexer::same_statement(&src[tokens[i-1].span.end..tokens[i].span.start]) {
                                        break;
                                    }
                                    match &tokens[i].kind {
                                        TokenKind::DColon => break,
                                        TokenKind::Ident(s) if s.eq_ignore_ascii_case("intent") => {
                                            if i + 3 < tokens.len() {
                                                if matches!(tokens[i + 1].kind, TokenKind::LParen) {
                                                    if let TokenKind::Ident(arg) =
                                                        &tokens[i + 2].kind
                                                    {
                                                        let lower = arg.to_ascii_lowercase();
                                                        if lower == "in" {
                                                            intent_found = ParamIntent::In;
                                                        } else if lower == "out" {
                                                            intent_found = ParamIntent::Out;
                                                        } else if lower == "inout" {
                                                            intent_found = ParamIntent::InOut;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                    i += 1;
                                }
                                for nm in names {
                                    param_intent_map.insert(nm.to_ascii_lowercase(), intent_found);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for stmt in &program.body {
        if let Stmt::Function {
            name, return_type, ..
        } = stmt
        {
            if return_type.is_some() {
                let lname = name.to_ascii_lowercase();
                if !type_span_map.contains_key(&lname) {
                    if let Some(tok_idx) = tokens.iter().position(
                        |t| matches!(&t.kind, TokenKind::Ident(s) if s.eq_ignore_ascii_case(name)),
                    ) {
                        if let Some(ts) = find_type_keyword_before(tok_idx) {
                            type_span_map.insert(lname, ts);
                        }
                    }
                }
            }
        }
    }

    for stmt in &program.body {
        match stmt {
            Stmt::Function {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                let lname = name.to_ascii_lowercase();
                if fn_map.contains_key(&lname) {
                    let span = type_span_map
                        .get(&lname)
                        .cloned()
                        .unwrap_or(find_ident_span(name));
                    report_error(
                        &format!("duplicate function `{}`", name),
                        span,
                        &mut stderr,
                        &file,
                        &mut errors,
                        false,
                    );
                } else {
                    fn_map.insert(
                        lname,
                        FnInfo {
                            return_type: return_type.clone(),
                            is_function: true,
                            params: params.clone(),
                            body: body.clone(),
                        },
                    );
                }
            }
            Stmt::Subroutine {
                name, params, body, ..
            } => {
                let lname = name.to_ascii_lowercase();
                if fn_map.contains_key(&lname) {
                    let span = type_span_map
                        .get(&lname)
                        .cloned()
                        .unwrap_or(find_ident_span(name));
                    report_error(
                        &format!("duplicate subroutine `{}`", name),
                        span,
                        &mut stderr,
                        &file,
                        &mut errors,
                        false,
                    );
                } else {
                    fn_map.insert(
                        lname,
                        FnInfo {
                            return_type: None,
                            is_function: false,
                            params: params.clone(),
                            body: body.clone(),
                        },
                    );
                }
            }
            Stmt::Module { name: _, body } => {
                for inner in body {
                    match inner {
                        Stmt::Function {
                            name,
                            params,
                            return_type,
                            body,
                            ..
                        } => {
                            let lname = name.to_ascii_lowercase();
                            module_fn_names.insert(lname.clone());
                            if fn_map.contains_key(&lname) {
                                let span = type_span_map
                                    .get(&lname)
                                    .cloned()
                                    .unwrap_or(find_ident_span(name));
                                report_error(
                                    &format!("duplicate function `{}`", name),
                                    span,
                                    &mut stderr,
                                    &file,
                                    &mut errors,
                                    false,
                                );
                            } else {
                                fn_map.insert(
                                    lname,
                                    FnInfo {
                                        return_type: return_type.clone(),
                                        is_function: true,
                                        params: params.clone(),
                                        body: body.clone(),
                                    },
                                );
                            }
                        }
                        Stmt::Subroutine {
                            name, params, body, ..
                        } => {
                            let lname = name.to_ascii_lowercase();
                            module_fn_names.insert(lname.clone());
                            if fn_map.contains_key(&lname) {
                                let span = type_span_map
                                    .get(&lname)
                                    .cloned()
                                    .unwrap_or(find_ident_span(name));
                                report_error(
                                    &format!("duplicate subroutine `{}`", name),
                                    span,
                                    &mut stderr,
                                    &file,
                                    &mut errors,
                                    false,
                                );
                            } else {
                                fn_map.insert(
                                    lname,
                                    FnInfo {
                                        return_type: None,
                                        is_function: false,
                                        params: params.clone(),
                                        body: body.clone(),
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn infer_expr_type(
        e: &Expr,
        sym: &HashMap<String, TypeSpec>,
        fn_map: &HashMap<String, FnInfo>,
        external_fn_ret: &HashMap<String, TypeSpec>,
    ) -> Option<TypeSpec> {
        match e {
            Expr::IntLit(_) => Some(TypeSpec::Integer(None)),
            Expr::RealLit(_) => Some(TypeSpec::Real),
            Expr::Logical(_) => Some(TypeSpec::Logical),
            Expr::Str(_) => Some(TypeSpec::Character(None)),
            Expr::Ident(id) => sym.get(&id.to_ascii_lowercase()).cloned(),
            Expr::Index(name, _) | Expr::Section { name, .. } => sym.get(&name.to_ascii_lowercase()).cloned(),
            Expr::Un(_, inner) => infer_expr_type(inner, sym, fn_map, external_fn_ret),
            Expr::Bin(op, l, r) => {
                use crate::ast::BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Pow => {
                        let lt = infer_expr_type(l, sym, fn_map, external_fn_ret);
                        let rt = infer_expr_type(r, sym, fn_map, external_fn_ret);
                        match (lt, rt) {
                            (Some(TypeSpec::Integer(a)), Some(TypeSpec::Integer(b))) => {
                                let kind = a.unwrap_or(4).max(b.unwrap_or(4));
                                Some(TypeSpec::Integer(if kind == 4 {None} else {Some(kind)}))
                            }
                            (Some(TypeSpec::DoublePrecision), _)
                            | (_, Some(TypeSpec::DoublePrecision)) => {
                                Some(TypeSpec::DoublePrecision)
                            }
                            (Some(TypeSpec::Real), _) | (_, Some(TypeSpec::Real)) => {
                                Some(TypeSpec::Real)
                            }
                            _ => Some(TypeSpec::Real),
                        }
                    }
                    Eq | Ne | Lt | Gt | Le | Ge | And | Or | Eqv | Neqv => Some(TypeSpec::Logical),
                    Concat => Some(TypeSpec::Character(None)),
                }
            }
            Expr::Call(name, args) => {
                let lname = name.to_ascii_lowercase();
                if !fn_map.contains_key(&lname) {
                    if let Some(kind) = sym.get(&lname) { return Some(kind.clone()); }
                    if crate::intrinsics::integer_rounding(&lname) {return crate::intrinsics::rounding_kind(&lname,args).ok();}
                    if crate::intrinsics::real_predicate(&lname) {return Some(TypeSpec::Logical);}
                    if crate::intrinsics::math_arity(&lname).is_some() || matches!(lname.as_str(), "abs" | "max" | "min" | "mod" | "modulo" | "sign") {
                        return args.first().and_then(|arg| infer_expr_type(arg, sym, fn_map, external_fn_ret));
                    }
                }
                if let Some(fi) = fn_map.get(&lname) {
                    fi.return_type
                        .clone()
                        .or_else(|| external_fn_ret.get(&lname).cloned())
                        .or(Some(TypeSpec::Integer(None)))
                } else {
                    external_fn_ret
                        .get(&lname)
                        .cloned()
                        .or(Some(TypeSpec::Integer(None)))
                }
            }
        }
    }
    fn match_types(decl: &TypeSpec, found: &TypeSpec) -> bool {
        use TypeSpec::*;
        match (decl, found) {
            (Integer(_), Integer(_)) => true,
            (Real, Integer(_)) => true,
            (DoublePrecision, Integer(_)) | (DoublePrecision, Real) => true,
            (Real, Real) => true,
            (DoublePrecision, DoublePrecision) => true,
            (Logical, Logical) => true,
            (Character(_), Character(_)) => true,

            (Integer(_), Real) => false,
            (Integer(_), DoublePrecision) => false,
            _ => false,
        }
    }
    fn analyze_fn_body(
        fname: &str,
        is_function: bool,
        declared_ret: &Option<TypeSpec>,
        params: &Vec<String>,
        body: &[Stmt],
        global_sym: &HashMap<String, TypeSpec>,
        fn_map: &HashMap<String, FnInfo>,
        external_fn_ret: &HashMap<String, TypeSpec>,
        param_intent_map: &HashMap<String, ParamIntent>,
        find_ident_span: &dyn Fn(&str) -> std::ops::Range<usize>,
        find_type_keyword_before: &dyn Fn(usize) -> Option<std::ops::Range<usize>>,
        tokens: &[Token],
        errors: &mut Vec<CompileError>,
        stderr: &mut codespan_reporting::term::termcolor::StandardStream,
        file: &codespan_reporting::files::SimpleFile<&str, &str>,
        type_span_map: &HashMap<String, std::ops::Range<usize>>,
        settings: &SemaSettings,
        program_level_reads: &HashSet<String>,
    ) {
        let mut local = global_sym.clone();
        let mut local_declared_names: HashSet<String> = HashSet::new();
        let mut local_param_intent: HashMap<String, ParamIntent> = HashMap::new();
        for p in params.iter() {
            local_param_intent.insert(
                p.to_ascii_lowercase(),
                *param_intent_map
                    .get(&p.to_ascii_lowercase())
                    .unwrap_or(&ParamIntent::None),
            );
        }
        let mut assigned: HashSet<String> = HashSet::new();
        for s in body {
            if let Stmt::DummyIntent { name, intent } = s {
                if params.contains(name) {
                    local_param_intent.insert(name.to_ascii_lowercase(), match intent.as_str() {
                        "intent(in)" => ParamIntent::In,
                        "intent(out)" => ParamIntent::Out,
                        "intent(inout)" => ParamIntent::InOut,
                        _ => ParamIntent::None,
                    });
                }
            }
        }
        for (p, intent) in &local_param_intent {
            if *intent == ParamIntent::In || *intent == ParamIntent::InOut {
                assigned.insert(p.clone());
            }
        }
        if is_function {
            if let Some(rt) = declared_ret {
                local.insert(fname.to_ascii_lowercase(), rt.clone());
            } else if let Some(rt) = external_fn_ret.get(&fname.to_ascii_lowercase()) {
                local.insert(fname.to_ascii_lowercase(), rt.clone());
            }
        }
        for s in body {
            if let Stmt::VarDecl { kind, names } = s {
                for n in names {
                    let lname = n.to_ascii_lowercase();
                    local.insert(lname.clone(), kind.clone());
                    local_declared_names.insert(lname);
                }
            }
        }
        for s in body {
            if let Stmt::VarDecl { kind: _, names } = s {
                for n in names {
                    if let Some(tok_idx) = tokens.iter().position(|t| match &t.kind {
                        TokenKind::Ident(sid) => sid.eq_ignore_ascii_case(n),
                        _ => false,
                    }) {
                        if let Some(ts) = find_type_keyword_before(tok_idx) {
                            if let Some(type_tok_idx) = tokens.iter().position(|t| t.span == ts) {
                                let mut i = type_tok_idx + 1;
                                while i < tokens.len() {
                                    if !crate::lexer::same_statement(&file.source()[tokens[i-1].span.end..tokens[i].span.start]) {
                                        break;
                                    }
                                    match &tokens[i].kind {
                                        TokenKind::DColon => break,
                                        TokenKind::Ident(sid)
                                            if sid.eq_ignore_ascii_case("intent") =>
                                        {
                                            let span = type_span_map
                                                .get(&n.to_ascii_lowercase())
                                                .cloned()
                                                .unwrap_or(find_ident_span(n));
                                            warn_or_error("intent_on_local", format!("`intent` attribute on non-dummy/local variable `{}`", n), span, settings, stderr, file, errors);
                                            break;
                                        }
                                        _ => {}
                                    }
                                    i += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        for (name, intent) in param_intent_map.iter() {
            if *intent != ParamIntent::None {
                let lname = name.to_ascii_lowercase();
                if !params.iter().any(|p| p.eq_ignore_ascii_case(name))
                    && local.contains_key(&lname)
                {
                    let span = type_span_map
                        .get(&lname)
                        .cloned()
                        .unwrap_or(find_ident_span(name));
                    warn_or_error(
                        "intent_on_local",
                        format!("`intent` attribute on non-dummy/local variable `{}`", name),
                        span,
                        settings,
                        stderr,
                        file,
                        errors,
                    );
                }
            }
        }
        let mut has_path = false;
        let mut read_vars: HashSet<String> = HashSet::new();
        fn forall_indices(body:&[Stmt], reads:&mut HashSet<String>) {
            for stmt in body {match stmt {
                Stmt::DummyIntent{name,intent} if matches!(intent.as_str(), "@forall_index" | "@constructor_index") => {reads.insert(name.to_ascii_lowercase());}
                Stmt::Block{body}|Stmt::Do{body,..}|Stmt::DoWhile{body,..}=>forall_indices(body,reads),
                Stmt::If{then_body,else_body,..}=>{forall_indices(then_body,reads);if let Some(body)=else_body{forall_indices(body,reads);}}
                Stmt::SelectCase{cases,default,..}=>{for case in cases{forall_indices(&case.body,reads);}if let Some(body)=default{forall_indices(body,reads);}}
                _=>{}
            }}
        }
        forall_indices(body,&mut read_vars);
        for r in program_level_reads.iter() {
            read_vars.insert(r.clone());
        }
        fn inner(
            stmts: &[Stmt],
            fname: &str,
            is_function: bool,
            declared_ret: &Option<TypeSpec>,
            local: &HashMap<String, TypeSpec>,
            fn_map: &HashMap<String, FnInfo>,
            external_fn_ret: &HashMap<String, TypeSpec>,
            find_ident_span: &dyn Fn(&str) -> std::ops::Range<usize>,
            errors: &mut Vec<CompileError>,
            has_path: &mut bool,
            stderr: &mut codespan_reporting::term::termcolor::StandardStream,
            file: &codespan_reporting::files::SimpleFile<&str, &str>,
            type_span_map: &HashMap<String, std::ops::Range<usize>>,
            params: &Vec<String>,
            local_param_intent: &HashMap<String, ParamIntent>,
            assigned: &mut HashSet<String>,
            read_vars: &mut HashSet<String>,
            settings: &SemaSettings,
        ) {
            // Lowering may declare temporaries inside a loop or branch. Their
            // explicit types take precedence over implicit/global symbol entries.
            let mut scoped=local.clone();
            for st in stmts {
                match st {
                    Stmt::VarDecl {kind,names}=>for n in names {scoped.insert(n.to_ascii_lowercase(),kind.clone());},
                    Stmt::ArrayDecl {kind,name,..}=>{scoped.insert(name.to_ascii_lowercase(),kind.clone());},_=>{}
                }
            }
            let local=&scoped;
            for st in stmts {
                match st {
                    Stmt::Assign { name, value } => {
                        walk_reads_in_expr(value, read_vars);
                        if let Some(vty) = infer_expr_type(value, local, fn_map, external_fn_ret) {
                            if let Some(dty) = local.get(&name.to_ascii_lowercase()) {
                                if !match_types(dty, &vty) {
                                    let span = match type_span_map.get(&name.to_ascii_lowercase()) {
                                        Some(s) => s.clone(),
                                        None => find_ident_span(name),
                                    };
                                    report_error(
                                        &format!("type mismatch in assignment to `{}`", name),
                                        span,
                                        stderr,
                                        file,
                                        errors,
                                        false,
                                    );
                                }
                            }
                        }
                        let lname = name.to_ascii_lowercase();
                        if let Some(intent) = local_param_intent.get(&lname) {
                            if *intent == ParamIntent::In {
                                let span = match type_span_map.get(&lname) {
                                    Some(s) => s.clone(),
                                    None => find_ident_span(name),
                                };
                                warn_or_error(
                                    "intent_in_assignment",
                                    format!("assignment to INTENT(IN) dummy argument `{}`", name),
                                    span,
                                    settings,
                                    stderr,
                                    file,
                                    errors,
                                );
                            } else {
                                assigned.insert(lname.clone());
                            }
                        }
                        read_vars.remove(&lname);
                        if is_function && name.eq_ignore_ascii_case(fname) {
                            *has_path = true;
                        }
                    }
                    Stmt::Return(expr_opt) => {
                        if let Some(e) = expr_opt {
                            if let Some(rt) = declared_ret.clone().or_else(|| {
                                external_fn_ret.get(&fname.to_ascii_lowercase()).cloned()
                            }) {
                                if let Some(vty) =
                                    infer_expr_type(e, local, fn_map, external_fn_ret)
                                {
                                    if !match_types(&rt, &vty) {
                                        let span =
                                            match type_span_map.get(&fname.to_ascii_lowercase()) {
                                                Some(s) => s.clone(),
                                                None => find_ident_span(fname),
                                            };
                                        report_error(
                                            &format!(
                                                "return type mismatch in function `{}`",
                                                fname
                                            ),
                                            span,
                                            stderr,
                                            file,
                                            errors,
                                            false,
                                        );
                                    }
                                }
                            }
                        }
                        if let Some(e) = expr_opt {
                            walk_reads_in_expr(e, read_vars);
                        }
                        *has_path = true;
                    }
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                        ..
                    } => {
                        walk_reads_in_expr(cond, read_vars);
                        inner(
                            then_body,
                            fname,
                            is_function,
                            declared_ret,
                            local,
                            fn_map,
                            external_fn_ret,
                            find_ident_span,
                            errors,
                            has_path,
                            stderr,
                            file,
                            type_span_map,
                            params,
                            local_param_intent,
                            assigned,
                            read_vars,
                            settings,
                        );
                        if let Some(eb) = else_body {
                            inner(
                                eb,
                                fname,
                                is_function,
                                declared_ret,
                                local,
                                fn_map,
                                external_fn_ret,
                                find_ident_span,
                                errors,
                                has_path,
                                stderr,
                                file,
                                type_span_map,
                                params,
                                local_param_intent,
                                assigned,
                                read_vars,
                                settings,
                            );
                        }
                    }
                    Stmt::Do {
                        var,
                        start,
                        end,
                        body: loop_body,
                        ..
                    } => {
                        let lname = var.to_ascii_lowercase();
                        read_vars.insert(lname.clone());
                        let mut new_local = local.clone();
                        if !new_local.contains_key(&lname) {
                            new_local.insert(lname.clone(), TypeSpec::Integer(None));
                        }
                        walk_reads_in_expr(start, read_vars);
                        walk_reads_in_expr(end, read_vars);
                        inner(
                            loop_body,
                            fname,
                            is_function,
                            declared_ret,
                            &new_local,
                            fn_map,
                            external_fn_ret,
                            find_ident_span,
                            errors,
                            has_path,
                            stderr,
                            file,
                            type_span_map,
                            params,
                            local_param_intent,
                            assigned,
                            read_vars,
                            settings,
                        );
                    }
                    Stmt::DoWhile { cond, body } => {
                        walk_reads_in_expr(cond, read_vars);
                        inner(
                            body, fname, is_function, declared_ret, local,
                            fn_map, external_fn_ret, find_ident_span, errors,
                            has_path, stderr, file, type_span_map, params,
                            local_param_intent, assigned, read_vars, settings,
                        );
                    }
                    Stmt::Block { body } => {
                        let mut scoped = local.clone();
                        for s in body {
                            match s {
                                Stmt::VarDecl { kind, names } => {
                                    for name in names {
                                        scoped.insert(name.clone(), kind.clone());
                                    }
                                }
                                Stmt::ArrayDecl { kind, name, .. } => {
                                    scoped.insert(name.clone(), kind.clone());
                                }
                                _ => {}
                            }
                        }
                        inner(
                            body, fname, is_function, declared_ret, &scoped,
                            fn_map, external_fn_ret, find_ident_span, errors,
                            has_path, stderr, file, type_span_map, params,
                            local_param_intent, assigned, read_vars, settings,
                        );
                        // Preserve enclosing reads, including loop controls that
                        // lowering updates after their last source-level use.
                        walk_stmt_program_reads(st, read_vars);
                    }
                    Stmt::Function { .. } | Stmt::Subroutine { .. } => {}
                    Stmt::CallSub { name, args } => {
                        let callee = fn_map.get(&name.to_ascii_lowercase());
                        for (index, arg) in args.iter().enumerate() {
                            let output = callee.is_some_and(|f| {
                                f.params.get(index).is_some_and(|p| f.body.iter().any(|s|
                                    matches!(s, Stmt::DummyIntent { name, intent }
                                        if name == p && intent == "intent(out)")))
                            });
                            if output {
                                if let Expr::Ident(n) = arg { assigned.insert(n.to_ascii_lowercase()); }
                            } else {
                                walk_reads_in_expr(arg, read_vars);
                            }
                        }
                    }
                    Stmt::Print { items } => {
                        for it in items {
                            walk_reads_in_expr(it, read_vars);
                        }
                    }
                    _ => {}
                }
            }
        }
        fn walk_reads_in_expr(e: &Expr, read_vars: &mut HashSet<String>) {
            match e {
                Expr::Ident(id) => {
                    read_vars.insert(id.to_ascii_lowercase());
                }
                Expr::Index(name, args) => {
                    read_vars.insert(name.to_ascii_lowercase());
                    for a in args {
                        walk_reads_in_expr(a, read_vars);
                    }
                }
                Expr::Call(_, args) => {
                    for a in args {
                        walk_reads_in_expr(a, read_vars);
                    }
                }
                Expr::Bin(_, l, r) => {
                    walk_reads_in_expr(l, read_vars);
                    walk_reads_in_expr(r, read_vars);
                }
                Expr::Un(_, inner) => walk_reads_in_expr(inner, read_vars),
                _ => {}
            }
        }

        inner(
            body,
            fname,
            is_function,
            declared_ret,
            &local,
            fn_map,
            external_fn_ret,
            find_ident_span,
            errors,
            &mut has_path,
            stderr,
            file,
            type_span_map,
            params,
            &local_param_intent,
            &mut assigned,
            &mut read_vars,
            settings,
        );
        fn borrowed_storage(body:&[Stmt],names:&mut HashSet<String>) {
            for stmt in body {
                match stmt {
                    Stmt::CallSub {name,args} if name=="@ptr_bind"=>{
                        if let Some(Expr::Ident(target))=args.get(3) {names.insert(target.to_ascii_lowercase());}
                    }
                    Stmt::Block {body}|Stmt::Do {body,..}|Stmt::DoWhile {body,..}=>borrowed_storage(body,names),
                    Stmt::If {then_body,else_body,..}=>{borrowed_storage(then_body,names);if let Some(b)=else_body {borrowed_storage(b,names);}},
                    Stmt::SelectCase {cases,default,..}=>{for c in cases {borrowed_storage(&c.body,names);}if let Some(b)=default {borrowed_storage(b,names);}},
                    _=>{}
                }
            }
        }
        let mut borrowed=HashSet::new();borrowed_storage(body,&mut borrowed);
        read_vars.extend(borrowed.iter().cloned());
        if is_function && declared_ret.is_some() {
            if !has_path {
                let span = match type_span_map.get(&fname.to_ascii_lowercase()) {
                    Some(s) => s.clone(),
                    None => find_ident_span(fname),
                };
                report_error(
                    &format!(
                        "function `{}` with return type has no RETURN or assignment to its name",
                        fname
                    ),
                    span,
                    stderr,
                    file,
                    errors,
                    false,
                );
            }
        }
        for (p, intent) in local_param_intent.iter() {
            if *intent == ParamIntent::Out {
                if !assigned.contains(p) && !borrowed.contains(p) {
                    let span = match type_span_map.get(p) {
                        Some(s) => s.clone(),
                        None => find_ident_span(p),
                    };
                    warn_or_error(
                        "intent_out_never_assigned",
                        format!("INTENT(OUT) dummy argument `{}` was never assigned", p),
                        span,
                        settings,
                        stderr,
                        file,
                        errors,
                    );
                }
            }
        }
        for stmt in body {if let Stmt::DummyIntent{name,intent}=stmt {if intent=="@optional_source_read" {read_vars.insert(name.to_ascii_lowercase());}}}
        for p in params.iter() {
            let lname = p.to_ascii_lowercase();
            if local_param_intent.get(&lname) == Some(&ParamIntent::Out) && assigned.contains(&lname) { continue; }
            if lname == "dummy" || lname.contains('%') || lname.starts_with('@') {
                continue;
            }
            if !read_vars.contains(&lname) {
                let span = find_ident_span(p);
                warn_or_error(
                    "unused_param",
                    format!("parameter `{}` is never read", p),
                    span,
                    settings,
                    stderr,
                    file,
                    errors,
                );
            }
        }
        for name in local_declared_names.iter() {
            let lname = name.to_ascii_lowercase();
            if params.iter().any(|p| p.eq_ignore_ascii_case(name)) { continue; }
            if lname == "dummy" || lname.contains('%') || lname.starts_with('@') {
                continue;
            }
            if !read_vars.contains(&lname) {
                if is_function && name.eq_ignore_ascii_case(fname) {
                    continue;
                }
                warn_or_error(
                    "unused_local",
                    format!("local variable `{}` is never read", name),
                    find_ident_span(name),
                    settings,
                    stderr,
                    file,
                    errors,
                );
            }
        }
    }
    fn check_reads_before_assignment(
        params: &Vec<String>,
        body: &[Stmt],
        param_intent_map: &HashMap<String, ParamIntent>,
        find_ident_span: &dyn Fn(&str) -> std::ops::Range<usize>,
        stderr: &mut codespan_reporting::term::termcolor::StandardStream,
        file: &codespan_reporting::files::SimpleFile<&str, &str>,
        errors: &mut Vec<CompileError>,
        settings: &SemaSettings,
    ) {
        use std::collections::HashSet;
        let mut init_assigned: HashSet<String> = HashSet::new();
        for p in params.iter() {
            match param_intent_map.get(&p.to_ascii_lowercase()) {
                Some(ParamIntent::Out) => {}
                _ => {
                    init_assigned.insert(p.to_ascii_lowercase());
                }
            }
        }
        fn walk_expr_for_reads_flow(
            e: &Expr,
            cur_assigned: &HashSet<String>,
            param_intent_map: &HashMap<String, ParamIntent>,
            find_ident_span: &dyn Fn(&str) -> std::ops::Range<usize>,
            stderr: &mut codespan_reporting::term::termcolor::StandardStream,
            file: &codespan_reporting::files::SimpleFile<&str, &str>,
            errors: &mut Vec<CompileError>,
            settings: &SemaSettings,
        ) {
            match e {
                Expr::Ident(id) => {
                    let lname = id.to_ascii_lowercase();
                    if let Some(ParamIntent::Out) = param_intent_map.get(&lname) {
                        if !cur_assigned.contains(&lname) {
                            let span = find_ident_span(id);
                            warn_or_error(
                                "intent_out_read_before_assign",
                                format!(
                                    "use of INTENT(OUT) dummy argument `{}` before assignment",
                                    id
                                ),
                                span,
                                settings,
                                stderr,
                                file,
                                errors,
                            );
                        }
                    }
                }
                Expr::Call(_, args) => {
                    for a in args {
                        walk_expr_for_reads_flow(
                            a,
                            cur_assigned,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                    }
                }
                Expr::Bin(_, l, r) => {
                    walk_expr_for_reads_flow(
                        l,
                        cur_assigned,
                        param_intent_map,
                        find_ident_span,
                        stderr,
                        file,
                        errors,
                        settings,
                    );
                    walk_expr_for_reads_flow(
                        r,
                        cur_assigned,
                        param_intent_map,
                        find_ident_span,
                        stderr,
                        file,
                        errors,
                        settings,
                    );
                }
                Expr::Un(_, inner) => walk_expr_for_reads_flow(
                    inner,
                    cur_assigned,
                    param_intent_map,
                    find_ident_span,
                    stderr,
                    file,
                    errors,
                    settings,
                ),
                _ => {}
            }
        }
        fn process_stmts(
            stmts: &[Stmt],
            cur_assigned: &HashSet<String>,
            param_intent_map: &HashMap<String, ParamIntent>,
            find_ident_span: &dyn Fn(&str) -> std::ops::Range<usize>,
            stderr: &mut codespan_reporting::term::termcolor::StandardStream,
            file: &codespan_reporting::files::SimpleFile<&str, &str>,
            errors: &mut Vec<CompileError>,
            settings: &SemaSettings,
        ) -> HashSet<String> {
            use std::collections::HashSet;
            let mut cur = cur_assigned.clone();
            for s in stmts {
                match s {
                    Stmt::Assign { name, value } => {
                        walk_expr_for_reads_flow(
                            value,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        cur.insert(name.to_ascii_lowercase());
                    }
                    Stmt::If {
                        cond,
                        then_body,
                        else_body,
                        ..
                    } => {
                        walk_expr_for_reads_flow(
                            cond,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        let then_assigned = process_stmts(
                            then_body,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        let else_assigned = if let Some(eb) = else_body {
                            process_stmts(
                                eb,
                                &cur,
                                param_intent_map,
                                find_ident_span,
                                stderr,
                                file,
                                errors,
                                settings,
                            )
                        } else {
                            cur.clone()
                        };
                        let mut next = HashSet::new();
                        for p in cur
                            .iter()
                            .chain(then_assigned.iter())
                            .chain(else_assigned.iter())
                        {
                            let p = p.clone();
                            if then_assigned.contains(&p) && else_assigned.contains(&p) {
                                next.insert(p);
                            }
                        }
                        cur = next;
                    }
                    Stmt::Do {
                        start, end, body, ..
                    } => {
                        walk_expr_for_reads_flow(
                            start,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        walk_expr_for_reads_flow(
                            end,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        let body_assigned = process_stmts(
                            body,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                        cur = cur.union(&body_assigned).cloned().collect();
                    }
                    Stmt::Return(Some(e)) => {
                        walk_expr_for_reads_flow(
                            e,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                    }
                    Stmt::Function { body, .. }
                    | Stmt::Subroutine { body, .. }
                    | Stmt::Module { body, .. } => {
                        let _ = process_stmts(
                            body,
                            &cur,
                            param_intent_map,
                            find_ident_span,
                            stderr,
                            file,
                            errors,
                            settings,
                        );
                    }
                    _ => {}
                }
            }
            cur
        }

        let _final = process_stmts(
            body,
            &init_assigned,
            param_intent_map,
            find_ident_span,
            stderr,
            file,
            errors,
            settings,
        );
    }
    for (_fname, info) in fn_map.iter() {
        check_reads_before_assignment(
            &info.params,
            &info.body,
            &param_intent_map,
            &find_ident_span,
            &mut stderr,
            &file,
            &mut errors,
            settings,
        );
    }
    for (fname, info) in fn_map.clone() {
        analyze_fn_body(
            &fname,
            info.is_function,
            &info.return_type,
            &info.params,
            &info.body,
            &sym,
            &fn_map,
            &external_fn_ret,
            &param_intent_map,
            &find_ident_span,
            &find_type_keyword_before,
            tokens,
            &mut errors,
            &mut stderr,
            &file,
            &type_span_map,
            settings,
            &program_level_reads,
        );
    }
    for stmt in &program.body {
        if let Stmt::VarDecl { names, .. } = stmt {
            for n in names {
                let lname = n.to_ascii_lowercase();
                if lname == "dummy" || lname.contains('%') || lname.starts_with('@') {
                    continue;
                }
                if fn_map.contains_key(&lname) || external_fn_ret.contains_key(&lname) {
                    continue;
                }
                if !program_level_reads.contains(&lname) {
                    let span = find_ident_span(n);
                    warn_or_error(
                        "unused_program_var",
                        format!("variable `{}` is never used", n),
                        span,
                        settings,
                        &mut stderr,
                        &file,
                        &mut errors,
                    );
                }
            }
        }
    }
    for stmt in &program.body {
        if let Stmt::Assign { name, value } = stmt {
            let lname = name.to_ascii_lowercase();
            if let Some(ts) = sym.get(&lname) {
                match (ts, value) {
                    (TypeSpec::Character(Some(len)), Expr::Str(s)) => {
                        if s.len() > *len {
                            if !settings.allow.contains("character_overflow") {
                                let span =
                                    find_assign_rhs_span(tokens, name, value).unwrap_or(0..0);
                                let msg = format!("string literal length {} exceeds CHARACTER(len={}), will be truncated", s.len(), len);
                                warn_or_error(
                                    "character_overflow",
                                    msg,
                                    span,
                                    settings,
                                    &mut stderr,
                                    &file,
                                    &mut errors,
                                );
                            }
                        }
                    }
                    (TypeSpec::Character(Some(dst_len)), Expr::Ident(src_name)) => {
                        if let Some(TypeSpec::Character(Some(src_len))) =
                            sym.get(&src_name.to_ascii_lowercase())
                        {
                            if src_len > dst_len {
                                if !settings.allow.contains("character_overflow") {
                                    let span =
                                        find_assign_rhs_span(tokens, name, value).unwrap_or(0..0);
                                    let msg = format!("assigning CHARACTER(len={}) to CHARACTER(len={}) will truncate", src_len, dst_len);
                                    warn_or_error(
                                        "character_overflow",
                                        msg,
                                        span,
                                        settings,
                                        &mut stderr,
                                        &file,
                                        &mut errors,
                                    );
                                }
                            }
                        }
                    }
                    (TypeSpec::Integer(kind_opt), Expr::IntLit(s)) => match i128::from_str(s) {
                        Ok(v128) => {
                            let bytes = match kind_opt {
                                Some(k) => *k as u32,
                                None => 4,
                            };
                            let bits = 8u32 * bytes;
                            let (min, max) = match bits {
                                8 => (i8::MIN as i128, i8::MAX as i128),
                                16 => (i16::MIN as i128, i16::MAX as i128),
                                32 => (i32::MIN as i128, i32::MAX as i128),
                                64 => (i64::MIN as i128, i64::MAX as i128),
                                128 => (i128::MIN, i128::MAX),
                                _ => (i64::MIN as i128, i64::MAX as i128),
                            };
                            if v128 < min || v128 > max {
                                let span =
                                    find_assign_rhs_span(tokens, name, value).unwrap_or(0..0);
                                let msg = format!(
                                    "integer literal {} does not fit in INTEGER({} byte{})",
                                    s,
                                    bytes,
                                    if bytes == 1 { "" } else { "s" }
                                );
                                warn_or_error(
                                    "integer_overflow",
                                    msg,
                                    span,
                                    settings,
                                    &mut stderr,
                                    &file,
                                    &mut errors,
                                );
                            }
                        }
                        Err(_) => {
                            let span = find_assign_rhs_span(tokens, name, value).unwrap_or(0..0);
                            let msg = format!("invalid integer literal `{}`", s);
                            warn_or_error(
                                "invalid_integer_literal",
                                msg,
                                span,
                                settings,
                                &mut stderr,
                                &file,
                                &mut errors,
                            );
                        }
                    },
                    (TypeSpec::Real, Expr::RealLit(s))
                    | (TypeSpec::DoublePrecision, Expr::RealLit(s)) => match f64::from_str(s) {
                        Ok(_v) => {
                            if let TypeSpec::Real = ts {
                                if settings.wall {
                                    let v = _v;
                                    let rounded = format!("{:.6}", v).parse::<f64>().unwrap_or(v);
                                    if (rounded - v).abs() > 0.0 {
                                        let span = find_assign_rhs_span(tokens, name, value)
                                            .unwrap_or(0..0);
                                        let msg = "real literal exceeds default precision (6 digits), will be rounded".to_string();
                                        warn_or_error(
                                            "real_precision_strip",
                                            msg,
                                            span,
                                            settings,
                                            &mut stderr,
                                            &file,
                                            &mut errors,
                                        );
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            let span = find_assign_rhs_span(tokens, name, value).unwrap_or(0..0);
                            let msg = format!("invalid real literal `{}`", s);
                            warn_or_error(
                                "invalid_real_literal",
                                msg,
                                span,
                                settings,
                                &mut stderr,
                                &file,
                                &mut errors,
                            );
                        }
                    },
                    _ => {}
                }
            }
        }
    }
    if settings.wall {
        for stmt in &program.body {
            if let Stmt::If {
                cond,
                then_body,
                else_body,
            } = stmt
            {
                match cond {
                    Expr::Logical(true) => {
                        if let Some(else_b) = else_body {
                            if !else_b.is_empty() {
                                if let Some(span) = tokens
                                    .iter()
                                    .position(|t| matches!(t.kind, TokenKind::KwIf))
                                    .map(|i| tokens[i].span.clone())
                                {
                                    let msg = "unreachable ELSE branch".to_string();
                                    warn_or_error(
                                        "unreachable_else",
                                        msg,
                                        span,
                                        settings,
                                        &mut stderr,
                                        &file,
                                        &mut errors,
                                    );
                                }
                            }
                        }
                    }
                    Expr::Logical(false) => {
                        if !then_body.is_empty() {
                            if let Some(span) = tokens
                                .iter()
                                .position(|t| matches!(t.kind, TokenKind::KwIf))
                                .map(|i| tokens[i].span.clone())
                            {
                                let msg = "unreachable THEN branch".to_string();
                                warn_or_error(
                                    "unreachable_then",
                                    msg,
                                    span,
                                    settings,
                                    &mut stderr,
                                    &file,
                                    &mut errors,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for stmt in &program.body {
        if let Stmt::Assign { name, value } = stmt {
            if let Expr::Call(fname, _) = value {
                let lname_target = name.to_ascii_lowercase();
                if let Some(target_ts) = sym.get(&lname_target) {
                    if let Some(finfo) = fn_map.get(&fname.to_ascii_lowercase()) {
                        if finfo.is_function {
                            let frt = finfo.return_type.clone().or_else(|| {
                                external_fn_ret.get(&fname.to_ascii_lowercase()).cloned()
                            });
                            if let Some(ft) = frt {
                                if !match_types(target_ts, &ft) {
                                    let span = find_ident_span(fname);
                                    let msg = format!(
                                        "type mismatch: assigning result of `{}` to `{}`",
                                        fname, name
                                    );
                                    warn_or_error(
                                        "call_assign_type_mismatch",
                                        msg,
                                        span,
                                        settings,
                                        &mut stderr,
                                        &file,
                                        &mut errors,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    for stmt in &program.body {
        if let Stmt::Assign { name, value } = stmt {
            let lname = name.to_ascii_lowercase();
            if let Some(var_type) = sym.get(&lname) {
                if let Some(value_type) = infer_expr_type(value, &sym, &fn_map, &external_fn_ret) {
                    if !match_types(var_type, &value_type) {
                        let assignment_span = find_assign_lhs_span(tokens, name)
                            .unwrap_or_else(|| find_ident_span(name));
                        let msg = format!("type mismatch in assignment to `{}`", name);

                        if let Some((_, Some(implicit_span))) =
                            implicit_typing.get_implicit_type_with_span(&lname)
                        {
                            let note_msg =
                                format!("implicitly defined as {} here", show_type(var_type));
                            report_error_with_note(
                                &msg,
                                assignment_span,
                                &note_msg,
                                implicit_span,
                                &mut stderr,
                                &file,
                                &mut errors,
                                false,
                            );
                        } else {
                            let type_span = type_span_map
                                .get(&lname)
                                .cloned()
                                .unwrap_or_else(|| find_ident_span(name));
                            let note_msg = format!("declared as {} here", show_type(var_type));
                            report_error_with_note(
                                &msg,
                                assignment_span,
                                &note_msg,
                                type_span,
                                &mut stderr,
                                &file,
                                &mut errors,
                                false,
                            );
                        }
                    }
                }
            }
        }
    }
    fn walk_expr_undef<'a>(
        e: &'a Expr,
        fn_map: &HashMap<String, FnInfo>,
        external_fn_ret: &HashMap<String, TypeSpec>,
        sym: &mut HashMap<String, TypeSpec>,
        implicit_typing: &ImplicitTyping,
        undef: &mut Vec<&'a str>,
    ) {
        match e {
            Expr::Ident(name) => {
                let lname = name.to_ascii_lowercase();
                if !sym.contains_key(&lname)
                    && !fn_map.contains_key(&lname)
                    && !external_fn_ret.contains_key(&lname)
                    && lname != "random_number" && lname != "random_seed" && !lname.starts_with('@')
                {
                    if let Some(implicit_type) = implicit_typing.get_implicit_type(name) {
                        sym.insert(lname, implicit_type);
                    } else {
                        undef.push(name);
                    }
                }
            }
            Expr::Call(name, args) => {
                let lname = name.to_ascii_lowercase();
                if !sym.contains_key(&lname)
                    && !fn_map.contains_key(&lname)
                    && !external_fn_ret.contains_key(&lname)
                {
                    undef.push(name);
                }
                for a in args {
                    walk_expr_undef(a, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
            }
            Expr::Bin(_, l, r) => {
                walk_expr_undef(l, fn_map, external_fn_ret, sym, implicit_typing, undef);
                walk_expr_undef(r, fn_map, external_fn_ret, sym, implicit_typing, undef);
            }
            Expr::Un(_, inner) => {
                walk_expr_undef(inner, fn_map, external_fn_ret, sym, implicit_typing, undef)
            }
            Expr::Index(name, indices) => {
                let lname = name.to_ascii_lowercase();
                if !sym.contains_key(&lname)
                    && !fn_map.contains_key(&lname)
                    && !external_fn_ret.contains_key(&lname)
                {
                    if let Some(implicit_type) = implicit_typing.get_implicit_type(name) {
                        sym.insert(lname, implicit_type);
                    } else {
                        undef.push(name);
                    }
                }
                for idx in indices {
                    walk_expr_undef(idx, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
            }
            _ => {}
        }
    }
    fn walk_stmt_undef<'a>(
        s: &'a Stmt,
        fn_map: &HashMap<String, FnInfo>,
        external_fn_ret: &HashMap<String, TypeSpec>,
        sym: &mut HashMap<String, TypeSpec>,
        implicit_typing: &ImplicitTyping,
        undef: &mut Vec<&'a str>,
    ) {
        match s {
            Stmt::ArrayDecl{name,kind,..} => {sym.insert(name.to_ascii_lowercase(),kind.clone());}
            Stmt::VarDecl { kind, names } => {
                for name in names {
                    sym.insert(name.to_ascii_lowercase(), kind.clone());
                }
            }
            Stmt::Assign { value, .. } => {
                walk_expr_undef(value, fn_map, external_fn_ret, sym, implicit_typing, undef)
            }
            Stmt::Return(Some(e)) => {
                walk_expr_undef(e, fn_map, external_fn_ret, sym, implicit_typing, undef)
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                walk_expr_undef(cond, fn_map, external_fn_ret, sym, implicit_typing, undef);
                for st in then_body {
                    walk_stmt_undef(st, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        walk_stmt_undef(st, fn_map, external_fn_ret, sym, implicit_typing, undef);
                    }
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                walk_expr_undef(start, fn_map, external_fn_ret, sym, implicit_typing, undef);
                walk_expr_undef(end, fn_map, external_fn_ret, sym, implicit_typing, undef);
                for st in body {
                    walk_stmt_undef(st, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
            }
            Stmt::CallSub { name, args } => {
                let lname = name.to_ascii_lowercase();
                if !sym.contains_key(&lname)
                    && !fn_map.contains_key(&lname)
                    && !external_fn_ret.contains_key(&lname)
                    && lname != "random_number" && lname != "random_seed" && !lname.starts_with('@')
                {
                    undef.push(name);
                }
                for a in args {
                    walk_expr_undef(a, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
            }
            Stmt::Function { params, body, .. } => {
                let mut local_sym = sym.clone();
                for param in params {
                    let param_lower = param.to_ascii_lowercase();
                    if let Some(implicit_type) = implicit_typing.get_implicit_type(param) {
                        local_sym.insert(param_lower, implicit_type);
                    }
                }
                for st in body {
                    walk_stmt_undef(
                        st,
                        fn_map,
                        external_fn_ret,
                        &mut local_sym,
                        implicit_typing,
                        undef,
                    );
                }
            }
            Stmt::Subroutine { params, body, .. } => {
                let mut local_sym = sym.clone();
                for param in params {
                    let param_lower = param.to_ascii_lowercase();
                    if let Some(implicit_type) = implicit_typing.get_implicit_type(param) {
                        local_sym.insert(param_lower, implicit_type);
                    }
                }
                for st in body {
                    walk_stmt_undef(
                        st,
                        fn_map,
                        external_fn_ret,
                        &mut local_sym,
                        implicit_typing,
                        undef,
                    );
                }
            }
            Stmt::Module { body, .. } => {
                for st in body {
                    walk_stmt_undef(st, fn_map, external_fn_ret, sym, implicit_typing, undef);
                }
            }
            _ => {}
        }
    }
    let mut undef_calls: Vec<&str> = Vec::new();
    for stmt in &program.body {
        walk_stmt_undef(
            stmt,
            &fn_map,
            &external_fn_ret,
            &mut sym,
            &implicit_typing,
            &mut undef_calls,
        );
    }
    use std::collections::HashSet as _HashSet;
    let mut seen_undef = _HashSet::new();
    for name in undef_calls {
        let lname = name.to_ascii_lowercase();
        if linked_obj_exports.contains(&lname) {
            continue;
        }
        if seen_undef.insert(lname.clone()) {
            let span = find_ident_span(name);
            report_error(
                &format!("undefined function or subroutine `{}`@UNDEFINED", name),
                span,
                &mut stderr,
                &file,
                &mut errors,
                silent_undefined,
            );
        }
    }
    let mut called_fns: HashSet<String> = HashSet::new();
    fn walk_calls<'a>(e: &'a Expr, called: &mut HashSet<String>) {
        match e {
            Expr::Call(name, args) => {
                called.insert(name.to_ascii_lowercase());
                for a in args {
                    walk_calls(a, called);
                }
            }
            Expr::Bin(_, l, r) => {
                walk_calls(l, called);
                walk_calls(r, called);
            }
            Expr::Un(_, inner) => walk_calls(inner, called),
            Expr::Index(_, args) => for arg in args { walk_calls(arg, called); },
            _ => {}
        }
    }
    fn walk_stmt_calls<'a>(s: &'a Stmt, called: &mut HashSet<String>) {
        match s {
            Stmt::Assign { value, .. } => walk_calls(value, called),
            Stmt::AssignIndex { value, indices, .. } => {
                walk_calls(value, called);
                for e in indices { walk_calls(e, called); }
            }
            Stmt::Block { body } => for s in body { walk_stmt_calls(s, called); },
            Stmt::DoWhile { cond, body } => {
                walk_calls(cond, called);
                for s in body { walk_stmt_calls(s, called); }
            }
            Stmt::SelectCase { expr, cases, default } => {
                walk_calls(expr, called);
                for c in cases { for s in &c.body { walk_stmt_calls(s, called); } }
                if let Some(b)=default { for s in b { walk_stmt_calls(s, called); } }
            }
            Stmt::Return(Some(e)) => walk_calls(e, called),
            Stmt::If {
                cond,
                then_body,
                else_body,
            } => {
                walk_calls(cond, called);
                for st in then_body {
                    walk_stmt_calls(st, called);
                }
                if let Some(eb) = else_body {
                    for st in eb {
                        walk_stmt_calls(st, called);
                    }
                }
            }
            Stmt::Do {
                start, end, body, ..
            } => {
                walk_calls(start, called);
                walk_calls(end, called);
                for st in body {
                    walk_stmt_calls(st, called);
                }
            }
            Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. } => {
                for st in body {
                    walk_stmt_calls(st, called);
                }
            }
            Stmt::Print { items } => {
                for it in items {
                    walk_calls(it, called);
                }
            }
            Stmt::CallSub { name, args } => {
                called.insert(name.to_ascii_lowercase());
                for a in args {
                    walk_calls(a, called);
                }
            }
            _ => {}
        }
    }
    for stmt in &program.body {
        walk_stmt_calls(stmt, &mut called_fns);
    }
    for (fname, _) in fn_map.iter() {
        if !called_fns.contains(fname) {
            if module_fn_names.contains(fname) {
                continue;
            }
            let span = find_ident_span(fname);
            warn_or_error(
                "unused_function",
                format!("function or subroutine `{}` is never called", fname),
                span,
                settings,
                &mut stderr,
                &file,
                &mut errors,
            );
        }
    }

    fn whole_array(e: &Expr, arrays: &HashSet<String>) -> bool {
        match e {
            Expr::Ident(name) => arrays.contains(&name.to_ascii_lowercase()),
            Expr::Bin(_, a, b) => whole_array(a, arrays) || whole_array(b, arrays),
            Expr::Un(_, a) => whole_array(a, arrays),
            Expr::Call(name, args) if !arrays.contains(&name.to_ascii_lowercase()) => args.iter().any(|a| whole_array(a, arrays)),
            _ => false,
        }
    }

    fn check_math_expr(
        e: &Expr, sym: &HashMap<String, TypeSpec>, arrays: &HashSet<String>,
        functions: &HashMap<String, FnInfo>, returns: &HashMap<String, TypeSpec>,
        problems: &mut Vec<(String, String)>,
    ) {
        match e {
            Expr::Call(name, args) => {
                let key = name.to_ascii_lowercase();
                if !functions.contains_key(&key) && !sym.contains_key(&key) {
                    if key == "random_number" {
                        problems.push((name.clone(), "RANDOM_NUMBER is a subroutine; use CALL RANDOM_NUMBER(harvest)".into()));
                    }
                    if crate::intrinsics::integer_rounding(&key) {
                        if let Err(error)=crate::intrinsics::rounding_kind(&key,args) {problems.push((name.clone(),error.to_string()));}
                        if let Some(arg)=args.first() {
                            if infer_expr_type(arg,sym,functions,returns).is_some_and(|k|!matches!(k,TypeSpec::Real|TypeSpec::DoublePrecision)) {
                                problems.push((name.clone(), format!("{} A must be REAL or DOUBLE PRECISION",key.to_ascii_uppercase())));
                            }
                        }
                    }
                    if crate::intrinsics::real_predicate(&key) {
                        if args.len()!=1 {problems.push((name.clone(),format!("intrinsic `{name}` expects 1 argument, got {}",args.len())));}
                        else if infer_expr_type(&args[0],sym,functions,returns).is_some_and(|kind|!matches!(kind,TypeSpec::Real|TypeSpec::DoublePrecision)) {
                            problems.push((name.clone(),format!("{} requires a REAL or DOUBLE PRECISION argument",key.to_ascii_uppercase())));
                        }
                    }
                    let math = crate::intrinsics::math_arity(&key);
                    let arity = (if matches!(key.as_str(), "max" | "min") { Some(args.len().max(2)) } else if matches!(key.as_str(), "mod" | "modulo" | "sign") {Some(2)} else { math }).or_else(|| matches!(key.as_str(), "abs" | "int" | "real" | "dble").then_some(1));
                    // INT, AINT and ANINT take an optional constant KIND.
                    let kinded = matches!(key.as_str(), "int" | "aint" | "anint") && args.len() == 2
                        && crate::parameters::integer_constant(&args[1]).is_ok_and(|k| matches!(k, 1 | 2 | 4 | 8));
                    let args = if kinded { &args[..1] } else { &args[..] };
                    if let Some(arity) = arity {
                        if args.len() != arity {
                            problems.push((name.clone(), format!("intrinsic `{name}` expects {arity} argument(s), got {}", args.len())));
                        } else {
                            let types: Vec<_> = args.iter().map(|a| infer_expr_type(a, sym, functions, returns)).collect();
                            for (arg, kind) in args.iter().zip(&types) {
                                if whole_array(arg, arrays) {
                                    problems.push((name.clone(), format!("array arguments to intrinsic `{name}` are not yet supported")));
                                } else if let Some(kind) = kind {
                                    let valid = if math.is_some() {
                                        matches!(kind, TypeSpec::Real | TypeSpec::DoublePrecision)
                                    } else {
                                        matches!(kind, TypeSpec::Integer(_) | TypeSpec::Real | TypeSpec::DoublePrecision)
                                    };
                                    if !valid {
                                        let required = if math.is_some() { "REAL or DOUBLE PRECISION" } else { "numeric" };
                                        problems.push((name.clone(), format!("intrinsic `{name}` requires {required} arguments")));
                                    }
                                }
                            }
                            if matches!(key.as_str(), "atan2" | "max" | "min" | "mod" | "modulo" | "sign") {
                                for pair in types.windows(2) {
                                if let (Some(a), Some(b)) = (&pair[0], &pair[1]) {
                                    if std::mem::discriminant(a) != std::mem::discriminant(b) {
                                        problems.push((name.clone(), format!("{} arguments must have the same type and kind", key.to_ascii_uppercase())));
                                    }
                                }
                                }
                            }
                        }
                    }
                }
                for arg in args { check_math_expr(arg, sym, arrays, functions, returns, problems); }
            }
            Expr::Index(_, args) => for arg in args { check_math_expr(arg, sym, arrays, functions, returns, problems); },
            Expr::Bin(_, a, b) => {
                check_math_expr(a, sym, arrays, functions, returns, problems);
                check_math_expr(b, sym, arrays, functions, returns, problems);
            }
            Expr::Un(_, a) => check_math_expr(a, sym, arrays, functions, returns, problems),
            _ => {}
        }
    }

    fn check_math_body(
        body: &[Stmt], inherited: &HashMap<String, TypeSpec>, inherited_arrays: &HashSet<String>,
        functions: &HashMap<String, FnInfo>, returns: &HashMap<String, TypeSpec>,
        problems: &mut Vec<(String, String)>,
    ) {
        let mut symbols = inherited.clone();
        let mut arrays = inherited_arrays.clone();
        for stmt in body {
            match stmt {
                Stmt::VarDecl { kind, names } => for name in names {
                    symbols.insert(name.to_ascii_lowercase(), kind.clone());
                    arrays.remove(&name.to_ascii_lowercase());
                },
                Stmt::ArrayDecl { kind, name, .. } => {
                    symbols.insert(name.to_ascii_lowercase(), kind.clone());
                    arrays.insert(name.to_ascii_lowercase());
                }
                _ => {}
            }
        }
        for stmt in body {
            let mut expressions: Vec<&Expr> = Vec::new();
            if let Stmt::CallSub { name, args } = stmt {
                if name.eq_ignore_ascii_case("random_seed") && !args.is_empty() { problems.push((name.clone(), "RANDOM_SEED currently supports only the no-argument form".into())); }
                if name.eq_ignore_ascii_case("random_number") && !functions.contains_key(&name.to_ascii_lowercase()) {
                    let valid = args.len() == 1 && match &args[0] {
                        Expr::Ident(id) => matches!(symbols.get(&id.to_ascii_lowercase()), Some(TypeSpec::Real | TypeSpec::DoublePrecision)),
                        Expr::Call(id, indices) | Expr::Index(id, indices) => {
                            arrays.contains(&id.to_ascii_lowercase()) && indices.len() == 1
                                && matches!(symbols.get(&id.to_ascii_lowercase()), Some(TypeSpec::Real | TypeSpec::DoublePrecision))
                                && matches!(infer_expr_type(&indices[0], &symbols, functions, returns), Some(TypeSpec::Integer(_)))
                        }
                        _ => false,
                    };
                    if !valid {
                        problems.push((name.clone(), "RANDOM_NUMBER expects one writable REAL or DOUBLE PRECISION variable or array".into()));
                    }
                }
                if (crate::intrinsics::math_arity(name).is_some() || crate::intrinsics::real_predicate(name) || crate::intrinsics::integer_rounding(name)) && !functions.contains_key(&name.to_ascii_lowercase()) {
                    problems.push((name.clone(), format!("intrinsic `{name}` is a function, not a subroutine")));
                }
            }
            match stmt {
                Stmt::Assign { value, .. } => expressions.push(value),
                Stmt::AssignIndex { indices, value, .. } => { expressions.extend(indices); expressions.push(value); }
                Stmt::ArrayDecl { dims, .. } => expressions.extend(dims),
                Stmt::Print { items } => expressions.extend(items),
                Stmt::Read { args } | Stmt::CallSub { args, .. } => expressions.extend(args),
                Stmt::Return(Some(e)) => expressions.push(e),
                Stmt::If { cond, then_body, else_body } => {
                    expressions.push(cond);
                    check_math_body(then_body, &symbols, &arrays, functions, returns, problems);
                    if let Some(body) = else_body { check_math_body(body, &symbols, &arrays, functions, returns, problems); }
                }
                Stmt::Do { start, end, body, .. } => {
                    expressions.extend([start, end]);
                    check_math_body(body, &symbols, &arrays, functions, returns, problems);
                }
                Stmt::DoWhile { cond, body } => {
                    expressions.push(cond);
                    check_math_body(body, &symbols, &arrays, functions, returns, problems);
                }
                Stmt::Block { body } | Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } | Stmt::Module { body, .. } => {
                    check_math_body(body, &symbols, &arrays, functions, returns, problems);
                }
                Stmt::SelectCase { expr, cases, default } => {
                    expressions.push(expr);
                    for case in cases {
                        for item in &case.items {
                            match item { CaseItem::Single(e) => expressions.push(e), CaseItem::Range(a,b) => expressions.extend([a,b]) }
                        }
                        check_math_body(&case.body, &symbols, &arrays, functions, returns, problems);
                    }
                    if let Some(body) = default { check_math_body(body, &symbols, &arrays, functions, returns, problems); }
                }
                _ => {}
            }
            for e in expressions { check_math_expr(e, &symbols, &arrays, functions, returns, problems); }
        }
    }

    let mut math_problems = Vec::new();
    check_math_body(&program.body, &sym, &HashSet::new(), &fn_map, &external_fn_ret, &mut math_problems);
    for (name, message) in math_problems {
        report_error(&message, find_ident_span(&name), &mut stderr, &file, &mut errors, false);
    }
    errors
}

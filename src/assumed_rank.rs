// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Preserve assumed rank in the source AST, then specialize Fortran calls by rank.
//! Ordinary array lowering supplies the storage descriptors for each concrete rank.
use crate::ast::{CaseItem, CaseOrDefault, Expr, Program, Stmt, TypeSpec};
use crate::lexer::{Token, TokenKind};
use crate::procedure_arguments::{children, expressions};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};

const ASSUMED: &str = "@assumed_rank";
const SELECT: &str = "@select_rank";
type Types = HashMap<String, (TypeSpec, usize)>;
type Constants = HashMap<String, Expr>;

pub(crate) fn assumed(dims: &[Expr]) -> bool {
    dims.iter()
        .any(|d| matches!(d, Expr::Ident(n) if n==ASSUMED))
}
pub(crate) fn tokens(tokens: &mut [Token], source: &str) {
    for i in 0..tokens.len() {
        if !matches!(&tokens[i].kind,TokenKind::Ident(n) if n=="rank") {
            continue;
        }
        let head = i == 0
            || !crate::lexer::same_statement(&source[tokens[i - 1].span.end..tokens[i].span.start]);
        let selected = i > 0 && matches!(tokens[i - 1].kind, TokenKind::KwSelect);
        let guard = head
            && tokens.get(i + 1).is_some_and(|t| {
                matches!(t.kind, TokenKind::LParen)
                    || matches!(&t.kind,TokenKind::Ident(n) if n=="default")
            });
        let mut depth = 0;
        let mut assignment = false;
        if guard {
            for t in &tokens[i + 1..] {
                if !crate::lexer::same_statement(&source[tokens[i].span.end..t.span.start]) {
                    break;
                }
                match t.kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => depth -= 1,
                    TokenKind::Eq | TokenKind::Arrow if depth == 0 => {
                        assignment = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        if selected || guard && !assignment {
            tokens[i].kind = TokenKind::KwRank;
            if let Some(t) = tokens.get_mut(i + 1) {
                if matches!(&t.kind,TokenKind::Ident(n) if n=="default") {
                    t.kind = TokenKind::KwDefault;
                }
            }
        }
    }
}

pub(crate) fn construct(
    (alias, selector): (String, String),
    blocks: Vec<CaseOrDefault>,
) -> std::result::Result<Stmt, &'static str> {
    let mut cases = vec![];
    let mut default = None;
    for block in blocks {
        match block {
            CaseOrDefault::Case(case) => cases.push(case),
            CaseOrDefault::Default(body) => {
                if default.replace(body).is_some() {
                    return Err("duplicate RANK DEFAULT guard");
                }
            }
        }
    }
    Ok(Stmt::SelectCase {
        expr: Expr::Call(SELECT.into(), vec![Expr::Ident(selector), Expr::Str(alias)]),
        cases,
        default,
    })
}
fn selection(e: &Expr) -> Option<(&str, &str)> {
    match e {
        Expr::Call(n, args) if n == SELECT => match args.as_slice() {
            [Expr::Ident(selector), Expr::Str(alias)] => Some((selector, alias)),
            _ => None,
        },
        _ => None,
    }
}
fn declarations(body: &[Stmt], inherited: &Types) -> Types {
    let mut types = inherited.clone();
    for s in body {
        match s {
            Stmt::VarDecl { kind, names } => {
                for n in names {
                    types.insert(n.clone(), (kind.clone(), 0));
                }
            }
            Stmt::ArrayDecl { kind, name, dims } => {
                types.insert(name.clone(), (kind.clone(), dims.len()));
                if dims
                    .iter()
                    .any(|d| matches!(d,Expr::Ident(n) if n=="@assumed_extent"))
                {
                    types.insert(format!("@assumed_size:{name}"), (kind.clone(), dims.len()));
                }
            }
            Stmt::InitializedDecl { kind, entities, .. } => {
                for (n, _) in entities {
                    types.insert(n.clone(), (kind.clone(), 0));
                }
            }
            _ => {}
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if matches!(intent.as_str(), "allocatable" | "pointer") {
                types.insert(format!("@descriptor:{name}"), (TypeSpec::Logical, 0));
            }
        }
    }
    for s in body {
        if let Stmt::DummyIntent { name, intent } = s {
            if intent == "external"
                || intent.starts_with("procedure(")
                || intent.starts_with("@interface:")
            {
                types.insert(format!("@procedure:{name}"), (TypeSpec::Logical, 0));
            }
        }
    }
    types
}
fn constants(body: &[Stmt], inherited: &Constants) -> Constants {
    let mut constants = inherited.clone();
    for s in body {
        if let Stmt::InitializedDecl {
            attributes,
            entities,
            ..
        } = s
        {
            if attributes.iter().any(|a| a == "parameter") {
                for (n, value) in entities {
                    if let Some(value) = value {
                        constants.insert(n.clone(), value.clone());
                    }
                }
            }
        }
    }
    constants
}
fn guard(item: &CaseItem, constants: &Constants) -> Result<Option<usize>> {
    let CaseItem::Single(e) = item else {
        bail!("SELECT RANK requires a single integer constant per guard");
    };
    if matches!(e,Expr::Ident(n) if n=="@rank_star") {
        return Ok(None);
    }
    let rank = crate::parameters::constant_integer_with(e, constants)
        .context("RANK guard requires an integer constant expression")?;
    if !(0..=15).contains(&rank) {
        bail!("RANK guard must be between 0 and 15");
    }
    Ok(Some(rank as usize))
}
#[derive(Clone)]
struct Template {
    statement: Stmt,
    params: Vec<String>,
    dummies: Vec<(usize, String)>,
    host: Types,
    constants: Constants,
}
fn collect(
    body: &[Stmt],
    inherited: &Types,
    outer_constants: &Constants,
    templates: &mut HashMap<String, Template>,
    params: &[String],
) -> Result<()> {
    let types = declarations(body, inherited);
    let constants = constants(body, outer_constants);
    let mut location = None;
    for s in body {
        if let Some(pos) = crate::source_diagnostics::statement_offset(s) {
            location = Some(pos);
        }
        let result = (|| -> Result<()> {
            match s {
                Stmt::Function {
                    name,
                    params,
                    body,
                    elemental,
                    ..
                }
                | Stmt::Subroutine {
                    name,
                    params,
                    body,
                    elemental,
                    ..
                } => {
                    let mut dummies = vec![];
                    for s in body {
                        if let Stmt::ArrayDecl {
                            name: dummy, dims, ..
                        } = s
                        {
                            if assumed(dims) {
                                if dims.len() != 1 {
                                    bail!(
                                        "assumed-rank declaration `{dummy}` must contain only (..)"
                                    );
                                }
                                let position=params.iter().position(|p|p==dummy).with_context(||format!("assumed-rank entity `{dummy}` must be a dummy argument"))?;
                                if *elemental {
                                    bail!("ELEMENTAL procedure `{name}` cannot have an assumed-rank dummy");
                                }
                                if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name:n,intent} if n==dummy && intent=="value")) {
                    bail!("assumed-rank dummy `{dummy}` cannot have VALUE");
                }
                                if body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent:_} if name==crate::c_interop::BIND)) {
                    bail!("assumed-rank BIND(C) descriptors are not yet supported");
                }
                                dummies.push((position, dummy.clone()));
                            }
                        }
                    }
                    if !dummies.is_empty() {
                        templates.insert(
                            name.clone(),
                            Template {
                                statement: s.clone(),
                                params: params.clone(),
                                dummies,
                                host: types.clone(),
                                constants: constants.clone(),
                            },
                        );
                    }
                    collect(body, &types, &constants, templates, params)?;
                }
                Stmt::Module { body, .. } => collect(body, &types, &constants, templates, &[])?,
                Stmt::ArrayDecl { name, dims, .. } if assumed(dims) && !params.contains(name) => {
                    bail!("assumed-rank entity `{name}` must be a dummy argument")
                }
                Stmt::DerivedDecl { dims, .. } if assumed(dims) => {
                    bail!("assumed-rank derived-type dummies are not yet supported")
                }
                _ => {}
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, location))?;
    }
    Ok(())
}
fn inquiry(n: &str) -> bool {
    matches!(
        n,
        "rank"
            | "size"
            | "shape"
            | "lbound"
            | "ubound"
            | "kind"
            | "len"
            | "storage_size"
            | "is_contiguous"
            | "allocated"
            | "associated"
            | "present"
            | "c_loc"
            | "c_sizeof"
    )
}
fn validate_expr(
    e: &Expr,
    assumed: &HashSet<String>,
    templates: &HashMap<String, Template>,
) -> Result<()> {
    match e {
        Expr::Ident(n) if assumed.contains(n) => {
            bail!("assumed-rank dummy `{n}` requires SELECT RANK before accessing its value")
        }
        Expr::Call(n, _) if assumed.contains(n) => {
            bail!("assumed-rank dummy `{n}` cannot be subscripted outside a rank-specific block")
        }
        Expr::Call(n, args) => {
            let positions = if let Some(t) = templates.get(n) {
                let body = match &t.statement {
                    Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body,
                    _ => unreachable!(),
                };
                let signature = crate::named_arguments::Signature::new(&t.params, body);
                let slots = signature.associate(n, args)?;
                t.dummies
                    .iter()
                    .filter_map(|(i, _)| slots[*i])
                    .collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
            for (i, arg) in args.iter().enumerate() {
                let value = crate::named_arguments::keyword(arg).map_or(arg, |(_, v)| v);
                if matches!(value,Expr::Ident(v) if assumed.contains(v))
                    && (positions.contains(&i) || i == 0 && inquiry(n))
                {
                    continue;
                }
                validate_expr(value, assumed, templates)?;
            }
        }
        Expr::Index(n, _) | Expr::Section { name: n, .. } if assumed.contains(n) => {
            bail!("assumed-rank dummy `{n}` cannot be subscripted outside a rank-specific block")
        }
        Expr::Bin(_, a, b) => {
            validate_expr(a, assumed, templates)?;
            validate_expr(b, assumed, templates)?;
        }
        Expr::Un(_, a) => validate_expr(a, assumed, templates)?,
        Expr::Index(_, args) => {
            for e in args {
                validate_expr(e, assumed, templates)?;
            }
        }
        Expr::Section {
            lower,
            upper,
            stride,
            ..
        } => {
            for e in [lower, upper, stride].into_iter().flatten() {
                validate_expr(e, assumed, templates)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn validate(
    body: &[Stmt],
    assumed: &HashSet<String>,
    inherited: &Constants,
    templates: &HashMap<String, Template>,
) -> Result<()> {
    let constants = constants(body, inherited);
    let mut location = None;
    for s in body {
        if let Some(pos) = crate::source_diagnostics::statement_offset(s) {
            location = Some(pos);
        }
        let result = (|| -> Result<()> {
            if let Stmt::SelectCase {
                expr,
                cases,
                default,
            } = s
            {
                if let Some((selector, alias)) = selection(expr) {
                    if !assumed.contains(selector) {
                        bail!("SELECT RANK selector `{selector}` must be assumed-rank");
                    }
                    let mut seen = HashSet::new();
                    let mut star = false;
                    for c in cases {
                        let value = guard(&c.items[0], &constants)?;
                        if let Some(rank) = value {
                            if !seen.insert(rank) {
                                bail!("duplicate RANK({rank}) guard");
                            }
                        } else if std::mem::replace(&mut star, true) {
                            bail!("duplicate RANK(*) guard");
                        }
                        let mut local = assumed.clone();
                        local.remove(alias);
                        validate(&c.body, &local, &constants, templates)?;
                    }
                    if let Some(body) = default {
                        let mut local = assumed.clone();
                        local.insert(alias.into());
                        validate(body, &local, &constants, templates)?;
                    }
                    return Ok(());
                }
            }
            match s {
                Stmt::Function{name,body,..}|Stmt::Subroutine{name,body,..}=>{
                    let dummies=templates.get(name).map(|t|t.dummies.iter().map(|(_,n)|n.clone()).collect()).unwrap_or_default();
                    return validate(body,&dummies,&constants,templates);
                }
                Stmt::GenericInterface{procedures,..} if procedures.iter().any(|n|templates.contains_key(n))=>
                    bail!("generic interfaces containing assumed-rank procedures are not yet supported; call the specific procedure"),
                Stmt::Assign{name,..}|Stmt::AssignIndex{name,..} if assumed.contains(name)=>bail!("assumed-rank dummy `{name}` requires SELECT RANK before assignment"),
                Stmt::ArrayDecl{name,dims,..} if super::assumed_rank::assumed(dims)=>{
                    if !assumed.contains(name) {bail!("assumed-rank entity `{name}` must be a dummy argument");}
                    return Ok(());
                },
                Stmt::CallSub{name,args}=>validate_expr(&Expr::Call(name.clone(),args.clone()),assumed,templates)?,
                _=>{let mut copy=s.clone();for e in expressions(&mut copy) {validate_expr(e,assumed,templates)?;}}
            }
            let mut copy = s.clone();
            for b in children(&mut copy) {
                validate(b, assumed, &constants, templates)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, location))?;
    }
    Ok(())
}

fn intrinsic_kind(kind: TypeSpec, constants: &Constants) -> Option<TypeSpec> {
    let kind = crate::parameters::concrete_kind_with(kind, constants).ok()?;
    Some(match kind {
        TypeSpec::Integer(None) => TypeSpec::Integer(Some(4)),
        TypeSpec::Character(_) | TypeSpec::CharacterAssumed | TypeSpec::CharacterLength(_) => {
            TypeSpec::Character(None)
        }
        TypeSpec::RealKind(n) if n == "4" => TypeSpec::Real,
        TypeSpec::RealKind(n) if n == "8" => TypeSpec::DoublePrecision,
        TypeSpec::LogicalKind(n) if n == "4" => TypeSpec::Logical,
        TypeSpec::IntegerKind(_)
        | TypeSpec::RealKind(_)
        | TypeSpec::LogicalKind(_)
        | TypeSpec::Referenced { .. } => return None,
        other => other,
    })
}
fn discard_actuals(args: &mut Vec<Expr>, discarded: Option<&Vec<usize>>) {
    if let Some(discarded) = discarded {
        let mut i = 0;
        args.retain(|_| {
            let keep = !discarded.contains(&i);
            i += 1;
            keep
        });
    }
}
fn uses(body: &[Stmt], name: &str) -> bool {
    let mut body = body.to_vec();
    for s in &mut body {
        if matches!(s,Stmt::Assign{name:n,..}|Stmt::AssignIndex{name:n,..}|Stmt::Do{var:n,..} if n==name)
        {
            return true;
        }
        let mut found = false;
        let mut inspect = |e: &mut Expr| {
            crate::procedure_arguments::visit_expr(e, &mut |e| {
                if matches!(e,Expr::Ident(n)|Expr::Index(n,_)|Expr::Section{name:n,..} if n==name) {
                    found = true;
                }
            })
        };
        for e in expressions(s) {
            inspect(e);
        }
        if let Stmt::VarDecl {
            kind: TypeSpec::CharacterLength(e),
            ..
        }
        | Stmt::ArrayDecl {
            kind: TypeSpec::CharacterLength(e),
            ..
        } = s
        {
            inspect(e);
        }
        if found {
            return true;
        }
        for b in children(s) {
            if uses(b, name) {
                return true;
            }
        }
    }
    false
}
struct Lowerer {
    templates: HashMap<String, Template>,
    types: crate::operators::ExpressionTypes,
    cache: HashMap<(String, Vec<usize>, Vec<bool>), String>,
    discarded: HashMap<String, Vec<usize>>,
    generated: HashMap<String, Vec<Stmt>>,
    names: Vec<(String, String)>,
    procedures: HashSet<String>,
}
impl Lowerer {
    fn rank(&self, e: &Expr, types: &Types) -> Result<usize> {
        self.types
            .kind_rank(e, types)?
            .map(|(_, rank)| rank)
            .context("cannot determine actual argument rank for assumed-rank call")
    }
    fn specialize(&mut self, name: &str, args: &mut Vec<Expr>, types: &Types) -> Result<String> {
        let template = self.templates[name].clone();
        let body = match &template.statement {
            Stmt::Function { body, .. } | Stmt::Subroutine { body, .. } => body,
            _ => unreachable!(),
        };
        crate::named_arguments::Signature::new(&template.params, body).reorder(name, args)?;
        let declared = declarations(body, &template.host);
        let constants = constants(body, &template.constants);
        for (i, dummy) in &template.dummies {
            if let Some(actual) = args.get(*i).filter(|a| !crate::named_arguments::omitted(a)) {
                if let Some((actual_kind, _)) = self.types.kind_rank(actual, types)? {
                    let formal_kind = declared
                        .get(dummy)
                        .context("missing assumed-rank dummy declaration")?
                        .0
                        .clone();
                    if let (Some(formal), Some(actual)) = (
                        intrinsic_kind(formal_kind, &constants),
                        intrinsic_kind(actual_kind, &Constants::new()),
                    ) {
                        if formal != actual {
                            bail!("type or kind mismatch for assumed-rank dummy `{dummy}` of `{name}`");
                        }
                    }
                }
            }
        }
        let ranks=template.dummies.iter().map(|(i,_)| {
            if matches!(args.get(*i),Some(Expr::Ident(n)) if types.contains_key(&format!("@assumed_size:{n}"))) {
                bail!("assumed-size actuals and RANK(*) dispatch are not yet supported by assumed-rank calls");
            }
            if args.get(*i).is_none_or(crate::named_arguments::omitted) {Ok(0)} else {self.rank(&args[*i],types)}
        }).collect::<Result<Vec<_>>>()?;
        if ranks.iter().any(|r| *r > 15) {
            bail!("assumed-rank actual rank exceeds 15");
        }
        // An undefined variable may be passed to a procedure that never reads
        // it. Preserve evaluation of expressions, but allow an unused scalar
        // variable binding to disappear after its rank inquiry has been folded.
        let variable_inputs: Vec<_> = template.dummies.iter().zip(&ranks).map(|((i,n),rank)| {
            *rank==0 && matches!(args.get(*i),Some(Expr::Ident(actual)) if !types.contains_key(&format!("@descriptor:{actual}")))
                && body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n && intent=="intent(in)"))
                && !body.iter().any(|s|matches!(s,Stmt::DummyIntent{name,intent} if name==n && matches!(intent.as_str(),"optional"|"allocatable"|"pointer"|"target")))
        }).collect();
        let key = (name.to_string(), ranks.clone(), variable_inputs.clone());
        if let Some(name) = self.cache.get(&key) {
            discard_actuals(args, self.discarded.get(name));
            return Ok(name.clone());
        }
        let variables = variable_inputs
            .iter()
            .enumerate()
            .filter(|(_, yes)| **yes)
            .map(|(i, _)| i.to_string())
            .collect::<Vec<_>>()
            .join("_");
        let unique = format!(
            "{name}$rank{}$vars{variables}",
            ranks
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("_")
        );
        self.cache.insert(key, unique.clone());
        self.names.push((name.into(), unique.clone()));
        let implicit_result = matches!(&template.statement,Stmt::Function{body,..}
            if !matches!(body.last(),Some(Stmt::Return(Some(Expr::Ident(result)))) if result!=name));
        let mut definition = template.statement.clone();
        let body = match &mut definition {
            Stmt::Function { name, body, .. } | Stmt::Subroutine { name, body, .. } => {
                *name = unique.clone();
                body
            }
            _ => unreachable!(),
        };
        for ((_, dummy), rank) in template.dummies.iter().zip(&ranks) {
            for s in body.iter_mut() {
                if let Stmt::ArrayDecl { kind, name, dims } = s {
                    if name == dummy {
                        if *rank == 0 {
                            *s = Stmt::VarDecl {
                                kind: kind.clone(),
                                names: vec![name.clone()],
                            };
                        } else {
                            *dims = vec![Expr::Ident("@deferred_extent".into()); *rank];
                        }
                    }
                }
            }
        }
        // SELECT RANK is resolved before branches with incompatible ranks reach
        // generic resolution, shape analysis or initialization instrumentation.
        let known = template
            .dummies
            .iter()
            .zip(&ranks)
            .map(|((_, n), r)| (n.clone(), *r))
            .collect();
        self.select(body, &known, &template.constants)?;
        self.body(body, &template.host)?;
        if implicit_result {
            crate::procedure_arguments::rename(body, name, &unique);
        }
        let discarded: Vec<_> = template
            .dummies
            .iter()
            .zip(variable_inputs)
            .filter_map(|((position, name), variable)| {
                (variable && !uses(body, name)).then_some(*position)
            })
            .collect();
        if !discarded.is_empty() {
            let omitted: HashSet<_> = discarded
                .iter()
                .map(|i| template.params[*i].clone())
                .collect();
            body.retain_mut(|s| match s {
                Stmt::VarDecl { names, .. } => {
                    names.retain(|n| !omitted.contains(n));
                    !names.is_empty()
                }
                Stmt::DummyIntent { name, .. } => !omitted.contains(name),
                _ => true,
            });
            let params = match &mut definition {
                Stmt::Function { params, .. } | Stmt::Subroutine { params, .. } => params,
                _ => unreachable!(),
            };
            params.retain(|p| !omitted.contains(p));
            discard_actuals(args, Some(&discarded));
            self.discarded.insert(unique.clone(), discarded);
        }

        self.generated
            .entry(name.into())
            .or_default()
            .push(definition);
        Ok(unique)
    }
    fn select(
        &mut self,
        body: &mut Vec<Stmt>,
        ranks: &HashMap<String, usize>,
        inherited: &Constants,
    ) -> Result<()> {
        let constants = constants(body, inherited);
        let mut output = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::SelectCase {
                expr,
                cases,
                default,
            } = &mut s
            {
                if let Some((selector, alias)) = selection(expr) {
                    let selector = selector.to_owned();
                    let alias = alias.to_owned();
                    let rank = *ranks
                        .get(&selector)
                        .context("missing SELECT RANK specialization")?;
                    let mut chosen = None;
                    for c in cases {
                        if guard(&c.items[0], &constants)? == Some(rank) {
                            chosen = Some(std::mem::take(&mut c.body));
                        }
                    }
                    let mut chosen = chosen.or_else(|| default.take()).unwrap_or_default();
                    if alias != selector {
                        crate::procedure_arguments::rename(&mut chosen, &alias, &selector);
                    }
                    self.select(&mut chosen, ranks, &constants)?;
                    // Keep branch declarations local instead of leaking them into
                    // another SELECT RANK construct in the same procedure.
                    output.push(Stmt::Block { body: chosen });
                    continue;
                }
            }
            for b in children(&mut s) {
                self.select(b, ranks, &constants)?;
            }
            output.push(s);
        }
        *body = output;
        Ok(())
    }
    fn expression(&mut self, e: &mut Expr, types: &Types) -> Result<()> {
        if let Expr::Call(n, args) = e {
            if n == "rank"
                && !self.procedures.contains(n)
                && !types.contains_key(n)
                && !types.contains_key(&format!("@procedure:{n}"))
            {
                let signature = crate::named_arguments::Signature {
                    params: vec!["a".into()],
                    optional: HashSet::new(),
                };
                signature.reorder("rank", args)?;
                *e = Expr::IntLit(self.rank(&args[0], types)?.to_string());
                return Ok(());
            }
            if self.templates.contains_key(n) {
                *n = self.specialize(n, args, types)?;
            }
        }
        match e {
            Expr::Call(_, a) | Expr::Index(_, a) => {
                for e in a {
                    self.expression(e, types)?;
                }
            }
            Expr::Bin(_, a, b) => {
                self.expression(a, types)?;
                self.expression(b, types)?;
            }
            Expr::Un(_, a) => self.expression(a, types)?,
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => {
                for e in [lower, upper, stride].into_iter().flatten() {
                    self.expression(e, types)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn body(&mut self, body: &mut [Stmt], inherited: &Types) -> Result<()> {
        let types = declarations(body, inherited);
        let mut location = None;
        for s in body {
            if let Some(p) = crate::source_diagnostics::statement_offset(s) {
                location = Some(p);
            }
            let result = (|| -> Result<()> {
                if matches!(s,Stmt::Function{name,..}|Stmt::Subroutine{name,..} if self.templates.contains_key(name))
                {
                    return Ok(());
                }
                if let Stmt::CallSub { name, args } = s {
                    if self.templates.contains_key(name) {
                        *name = self.specialize(name, args, &types)?;
                    }
                }
                if let Stmt::VarDecl {
                    kind: TypeSpec::CharacterLength(e),
                    ..
                }
                | Stmt::ArrayDecl {
                    kind: TypeSpec::CharacterLength(e),
                    ..
                }
                | Stmt::InitializedDecl {
                    kind: TypeSpec::CharacterLength(e),
                    ..
                } = s
                {
                    self.expression(e, &types)?;
                }
                for e in expressions(s) {
                    self.expression(e, &types)?;
                }
                for b in children(s) {
                    self.body(b, &types)?;
                }
                Ok(())
            })();
            result.map_err(|e| crate::source_diagnostics::at(e, location))?;
        }
        Ok(())
    }
    fn install(&mut self, body: &mut Vec<Stmt>) {
        let mut out = vec![];
        for mut s in std::mem::take(body) {
            if let Stmt::Function { name, .. } | Stmt::Subroutine { name, .. } = &s {
                if self.templates.contains_key(name) {
                    let mut definitions = self.generated.remove(name).unwrap_or_default();
                    for d in &mut definitions {
                        for b in children(d) {
                            self.install(b);
                        }
                    }
                    out.extend(definitions);
                    continue;
                }
            }
            for b in children(&mut s) {
                self.install(b);
            }
            out.push(s);
        }
        *body = out;
    }
}
fn needed(body: &[Stmt], procedures: &mut HashSet<String>) -> bool {
    fn expr(e: &Expr) -> bool {
        match e {
            Expr::Call(n, a) => n == "rank" || a.iter().any(expr),
            Expr::Index(_, a) => a.iter().any(expr),
            Expr::Bin(_, a, b) => expr(a) || expr(b),
            Expr::Un(_, a) => expr(a),
            Expr::Section {
                lower,
                upper,
                stride,
                ..
            } => [lower, upper, stride]
                .into_iter()
                .flatten()
                .any(|e| expr(e)),
            _ => false,
        }
    }
    let mut found = false;
    for s in body {
        match s {
            Stmt::Function { name, .. }
            | Stmt::Subroutine { name, .. }
            | Stmt::GenericInterface { name, .. } => {
                procedures.insert(name.clone());
            }
            _ => {}
        }
        found |= match s {
            Stmt::VarDecl {
                kind: TypeSpec::CharacterLength(e),
                ..
            } => expr(e),
            Stmt::ArrayDecl {
                kind: TypeSpec::CharacterLength(e),
                dims,
                ..
            } => expr(e) || assumed(dims) || dims.iter().any(expr),
            Stmt::ArrayDecl { dims, .. } => assumed(dims) || dims.iter().any(expr),
            Stmt::Print { items }
            | Stmt::CallSub { args: items, .. }
            | Stmt::Read { args: items } => items.iter().any(expr),
            Stmt::Assign { value, .. } => expr(value),
            Stmt::AssignIndex { indices, value, .. } => expr(value) || indices.iter().any(expr),
            Stmt::Do { start, end, .. } => expr(start) || expr(end),
            Stmt::Return(Some(e)) => expr(e),
            Stmt::InitializedDecl { entities, .. } => {
                entities.iter().filter_map(|(_, e)| e.as_ref()).any(expr)
            }
            Stmt::If { cond, .. } | Stmt::DoWhile { cond, .. } => expr(cond),
            Stmt::SelectCase { expr: e, .. } => selection(e).is_some() || expr(e),
            _ => false,
        };
        match s {
            Stmt::Function { body, .. }
            | Stmt::Subroutine { body, .. }
            | Stmt::Module { body, .. }
            | Stmt::Block { body }
            | Stmt::Do { body, .. }
            | Stmt::DoWhile { body, .. } => {
                found |= needed(body, procedures);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                found |= needed(then_body, procedures);
                if let Some(b) = else_body {
                    found |= needed(b, procedures);
                }
            }
            Stmt::SelectCase { cases, default, .. } => {
                for c in cases {
                    found |= needed(&c.body, procedures);
                }
                if let Some(b) = default {
                    found |= needed(b, procedures);
                }
            }
            _ => {}
        }
    }
    found
}
pub(crate) fn resolve(mut program: Program) -> Result<Program> {
    let mut procedures = HashSet::new();
    if !needed(&program.body, &mut procedures) {
        return Ok(program);
    }
    let mut templates = HashMap::new();
    collect(
        &program.body,
        &Types::new(),
        &Constants::new(),
        &mut templates,
        &[],
    )?;
    validate(
        &program.body,
        &HashSet::new(),
        &Constants::new(),
        &templates,
    )?;
    let types = crate::operators::ExpressionTypes::new(&program)?;
    let mut lower = Lowerer {
        templates,
        types,
        cache: HashMap::new(),
        discarded: HashMap::new(),
        generated: HashMap::new(),
        names: vec![],
        procedures,
    };
    lower.body(&mut program.body, &Types::new())?;
    lower.install(&mut program.body);
    let original_contains: HashSet<_> = program.contains.iter().cloned().collect();
    program
        .contains
        .retain(|n| !lower.templates.contains_key(n));
    program.contains.extend(
        lower
            .names
            .into_iter()
            .filter(|(name, _)| original_contains.contains(name))
            .map(|(_, name)| name),
    );
    crate::block_scopes::resolve(program)
}

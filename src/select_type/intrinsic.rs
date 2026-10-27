// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
//! Intrinsic scalar views for unlimited polymorphic procedure specializations.
use super::*;

pub(crate) fn parsed(kind: TypeSpec) -> Expr {
    Expr::Call(
        "@intrinsic_type".into(),
        vec![Expr::Str(serde_json::to_string(&kind).unwrap())],
    )
}
pub(super) fn guard_kind(e: &Expr) -> Result<Option<TypeSpec>> {
    if let Expr::Call(n, a) = e {
        if n == "@intrinsic_type" {
            if let [Expr::Str(t)] = a.as_slice() {
                return Ok(Some(serde_json::from_str(t)?));
            }
            bail!("invalid intrinsic SELECT TYPE guard");
        }
    }
    Ok(None)
}
pub(super) fn encode(kind: &TypeSpec) -> String {
    format!("@intrinsic:{}", serde_json::to_string(kind).unwrap())
}
pub(super) fn decode(kind: &str) -> Option<TypeSpec> {
    serde_json::from_str(kind.strip_prefix("@intrinsic:")?).ok()
}
pub(super) fn object(kind: &TypeSpec, rank: usize, attrs: Vec<String>) -> Object {
    Object {
        kind: encode(kind),
        polymorphic: false,
        rank,
        attrs,
    }
}
pub(super) fn matches(actual: &str, guard: &str) -> bool {
    match (decode(actual), decode(guard)) {
        (
            Some(
                TypeSpec::Character(_) | TypeSpec::CharacterLength(_) | TypeSpec::CharacterAssumed,
            ),
            Some(TypeSpec::CharacterAssumed),
        ) => true,
        _ => actual == guard,
    }
}
pub(super) fn declaration(
    kind: &str,
    name: &str,
    attrs: &[String],
    polymorphic: bool,
) -> Result<Stmt> {
    Ok(if let Some(kind) = decode(kind) {
        Stmt::VarDecl {
            kind,
            names: vec![name.into()],
        }
    } else {
        Stmt::DerivedDecl {
            type_name: kind.into(),
            names: vec![name.into()],
            attributes: attrs.to_vec(),
            polymorphic,
            dims: vec![],
        }
    })
}
fn normalized(mut kind: TypeSpec, constants: &HashMap<String, Expr>) -> Result<TypeSpec> {
    if !matches!(kind, TypeSpec::CharacterAssumed) {
        crate::parameters::resolve_kind(&mut kind, constants)?;
    }
    Ok(kind)
}
// Preserve the structured TypeSpec through USE/host renaming, then resolve it
// with the same kind evaluator used by declarations. Never read selector data.
pub(super) fn prepare(body: &mut [Stmt], inherited: &HashMap<String, Expr>) -> Result<()> {
    let mut constants = inherited.clone();
    // USE association has already assigned canonical identities. Module
    // constants remain in module bodies until storage lowering, so make those
    // identities available to imported guard and literal kind parameters.
    for s in body.iter() {
        if let Stmt::Module { body, .. } = s {
            for s in body {
                if let Stmt::InitializedDecl {
                    attributes,
                    entities,
                    ..
                } = s
                {
                    if attributes.iter().any(|a| a == "parameter") {
                        for (n, v) in entities {
                            if let Some(v) = v {
                                if let Ok(v) = crate::parameters::evaluate(v, &constants) {
                                    constants.insert(n.clone(), Expr::IntLit(v.to_string()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for s in body.iter() {
        if let Stmt::InitializedDecl {
            kind: TypeSpec::Integer(_) | TypeSpec::IntegerKind(_),
            attributes,
            entities,
        } = s
        {
            if attributes.iter().any(|a| a == "parameter") {
                for (n, value) in entities {
                    if let Some(value) = value {
                        if let Ok(value) = crate::parameters::evaluate(value, &constants) {
                            constants.insert(n.clone(), Expr::IntLit(value.to_string()));
                        }
                    }
                }
            }
        }
    }
    let mut location = None;
    for s in body {
        if let Some(p) = crate::source_diagnostics::statement_offset(s) {
            location = Some(p);
        }
        let result = (|| -> Result<()> {
            match s {
                Stmt::VarDecl { kind, .. }
                | Stmt::InitializedDecl { kind, .. }
                | Stmt::ArrayDecl { kind, .. } => {
                    // Runtime CHARACTER lengths remain declarations, not guard parameters.
                    if let Ok(k) = normalized(kind.clone(), &constants) {
                        *kind = k;
                    }
                }
                Stmt::SelectCase { expr, cases, .. } if selection(expr).is_some() => {
                    for c in cases {
                        if let CaseItem::Single(Expr::Call(_, a)) = &mut c.items[0] {
                            if let Some(k) = guard_kind(&a[0])? {
                                if matches!(
                                    k,
                                    TypeSpec::Character(_) | TypeSpec::CharacterLength(_)
                                ) {
                                    bail!(
                                        "CHARACTER SELECT TYPE guard requires assumed length LEN=*"
                                    );
                                }
                                a[0] = parsed(normalized(k, &constants)?);
                            }
                        }
                    }
                }
                _ => {}
            }
            for e in expressions(s) {
                let mut error = None;
                visit_expr(e, &mut |e| {
                    if let Err(e) = crate::parameters::normalize_literal(e, &constants) {
                        error = Some(e);
                    }
                });
                if let Some(e) = error {
                    return Err(e);
                }
            }
            for b in children(s) {
                prepare(b, &constants)?;
            }
            Ok(())
        })();
        result.map_err(|e| crate::source_diagnostics::at(e, location))?;
    }
    Ok(())
}

pub(super) fn actual(e: &Expr, vars: &Objects) -> Result<Option<(TypeSpec, usize)>> {
    let lookup = |n: &str| {
        vars.get(n)
            .and_then(|v| decode(&v.kind).map(|k| (k, v.rank)))
    };
    let rank = match e {
        Expr::Ident(n) => vars.get(n).map_or(0, |v| v.rank),
        Expr::Bin(_, a, b) => actual(a, vars)?
            .map_or(0, |(_, r)| r)
            .max(actual(b, vars)?.map_or(0, |(_, r)| r)),
        Expr::Un(_, a) => actual(a, vars)?.map_or(0, |(_, r)| r),
        Expr::Section { .. } => 1,
        Expr::Call(_, args) | Expr::Index(_, args) => {
            let mut rank = 0;
            for arg in args {
                rank = rank.max(actual(arg, vars)?.map_or(0, |(_, r)| r));
            }
            rank
        }
        _ => 0,
    };
    let mut kind = match e {
        Expr::Str(s) => Some(TypeSpec::Character(Some(s.len()))),
        Expr::RealLit(s) => Some(if s.contains(['d', 'D']) {
            TypeSpec::DoublePrecision
        } else {
            TypeSpec::Real
        }),
        Expr::Bin(crate::ast::BinOp::Concat, a, b) => match (actual(a, vars)?, actual(b, vars)?) {
            (Some((TypeSpec::Character(a), _)), Some((TypeSpec::Character(b), _))) => {
                Some(TypeSpec::Character(Some(a.unwrap_or(1) + b.unwrap_or(1))))
            }
            _ => None,
        },
        _ => crate::inquiries::model_type(e, &lookup)?,
    };
    if let Some(k) = kind.as_mut() {
        if matches!(k, TypeSpec::Integer(Some(4))) {
            *k = TypeSpec::Integer(None);
        }
    }
    Ok(kind.map(|k| (k, rank)))
}

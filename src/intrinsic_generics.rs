// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Intrinsic alternatives to an extended generic, after user-specific selection.
//! Reuse keyword association and ordinary intrinsic lowering; do not synthesize
//! numeric conversions to make a user-specific procedure match.
use super::*;

fn element(t: &Ty) -> &Ty {
    match t {
        Ty::Array(t, _) => element(t),
        t => t,
    }
}
fn rank(t: &Ty) -> usize {
    match t {
        Ty::Array(_, r) => *r,
        _ => 0,
    }
}
fn integer(t: &Ty) -> bool {
    matches!(element(t), Ty::Scalar(TypeSpec::Integer(_)))
}
fn real(t: &Ty) -> bool {
    matches!(
        element(t),
        Ty::Scalar(TypeSpec::Real | TypeSpec::DoublePrecision | TypeSpec::RealKind(_))
    )
}
fn numeric(t: &Ty) -> bool {
    integer(t) || real(t) || matches!(element(t), Ty::Scalar(TypeSpec::Complex(_)))
}
fn logical(t: &Ty) -> bool {
    matches!(
        element(t),
        Ty::Scalar(TypeSpec::Logical | TypeSpec::LogicalKind(_))
    )
}
fn character(t: &Ty) -> bool {
    matches!(element(t), Ty::Scalar(TypeSpec::Character(_)))
}
fn conformable(types: &[&Ty]) -> bool {
    let mut array_rank = None;
    types.iter().all(|t| {
        let r = rank(t);
        if r == 0 {
            true
        } else if let Some(previous) = array_rank {
            previous == r
        } else {
            array_rank = Some(r);
            true
        }
    })
}

/// Canonical actuals are for type inference only. Extent conformance, constant
/// KIND/DIM values, descriptors and other intrinsic constraints remain checked
/// by the same lowerers as calls without a user generic interface.
pub(super) fn arguments(name: &str, actuals: &[Expr], types: &[Ty]) -> Result<Option<Vec<Expr>>> {
    if crate::intrinsics::purity(name) != Some(crate::intrinsics::Purity::Pure)
        || crate::intrinsics::PURE_SUBROUTINES.contains(&name)
        || matches!(name, "split" | "tokenize")
    {
        return Ok(None);
    }
    let reduction = matches!(
        name,
        "sum"
            | "product"
            | "minval"
            | "maxval"
            | "iany"
            | "iall"
            | "iparity"
            | "any"
            | "all"
            | "parity"
            | "count"
    );
    let mut signature = crate::named_arguments::intrinsic(name);
    if matches!(name, "min" | "max") {
        let mut count = actuals.len().max(2);
        for arg in actuals {
            if let Some((key, _)) = crate::named_arguments::keyword(arg) {
                if let Some(n) = key
                    .strip_prefix('a')
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| (1..=1024).contains(n))
                {
                    count = count.max(n);
                }
            }
        }
        let params: Vec<String> = (1..=count).map(|n| format!("a{n}")).collect();
        signature = Some(crate::named_arguments::Signature {
            optional: params.iter().skip(2).cloned().collect(),
            params,
        });
    } else if reduction {
        let logical_reduction = matches!(name, "any" | "all" | "parity" | "count");
        let keys: &[&str] = if name == "count" {
            &["mask", "dim", "kind"]
        } else if logical_reduction {
            &["mask", "dim"]
        } else {
            &["array", "dim", "mask"]
        };
        signature = Some(crate::named_arguments::Signature {
            params: keys.iter().map(|k| (*k).into()).collect(),
            optional: keys.iter().skip(1).map(|k| (*k).into()).collect(),
        });
        // The ARRAY,MASK form has no DIM positional argument.
        if !logical_reduction
            && actuals
                .get(1)
                .is_some_and(|a| crate::named_arguments::keyword(a).is_none())
            && types.get(1).is_some_and(logical)
        {
            signature = Some(crate::named_arguments::Signature {
                params: vec!["array".into(), "mask".into()],
                optional: HashSet::from(["mask".into()]),
            });
        }
    }
    let (canonical, ordered): (Vec<Expr>, Vec<Option<&Ty>>) = if let Some(signature) = &signature {
        let slots = signature.associate(name, actuals)?;
        let ordered = slots.iter().map(|i| i.map(|i| &types[i])).collect();
        let mut canonical = actuals.to_vec();
        signature.reorder(name, &mut canonical)?;
        (canonical, ordered)
    } else {
        // Shape/location/transformational intrinsics have overload-specific
        // keyword handling in their existing lowerers and expression inference.
        (actuals.to_vec(), types.iter().map(Some).collect())
    };
    let at = |i: usize| ordered.get(i).copied().flatten();
    let kind_ok = |i: usize| at(i).is_none_or(|t| integer(t) && rank(t) == 0);
    let same = |a: &Ty, b: &Ty| element(a) == element(b);
    let valid = if matches!(name, "min" | "max") {
        let present: Vec<_> = ordered.iter().copied().flatten().collect();
        present.first().is_some_and(|first| {
            (integer(first) || real(first) || character(first))
                && present.iter().all(|t| same(first, t))
                && conformable(&present)
        })
    } else if reduction {
        let signature = signature.as_ref().unwrap();
        let parameter = |name: &str| signature.params.iter().position(|n| n == name).and_then(at);
        let logical_reduction = matches!(name, "any" | "all" | "parity" | "count");
        at(0).is_some_and(|array| {
            rank(array) > 0
                && if logical_reduction {
                    logical(array)
                } else if matches!(name, "iany" | "iall" | "iparity") {
                    integer(array)
                } else if matches!(name, "minval" | "maxval") {
                    integer(array) || real(array) || character(array)
                } else {
                    numeric(array)
                }
        }) && parameter("dim").is_none_or(|t| integer(t) && rank(t) == 0)
            && (logical_reduction
                || parameter("mask").is_none_or(|mask| {
                    logical(mask)
                        && (rank(mask) == 0 || at(0).is_some_and(|array| rank(array) == rank(mask)))
                }))
            && parameter("kind").is_none_or(|t| integer(t) && rank(t) == 0)
    } else {
        match name {
            "repeat" => {
                at(0).is_some_and(|t| character(t) && rank(t) == 0)
                    && at(1).is_some_and(|t| integer(t) && rank(t) == 0)
            }
            "int" | "real" => at(0).is_some_and(numeric) && kind_ok(1),
            "dble" => at(0).is_some_and(numeric),
            "nint" | "floor" | "ceiling" | "aint" | "anint" => {
                at(0).is_some_and(real) && kind_ok(1)
            }
            "abs" => at(0).is_some_and(numeric),
            "mod" | "modulo" | "sign" | "dim" => at(0).zip(at(1)).is_some_and(|(a, b)| {
                (integer(a) || real(a)) && same(a, b) && conformable(&[a, b])
            }),
            // These inquiries accept derived objects as well as built-in ones.
            "size" | "shape" | "lbound" | "ubound" | "storage_size" | "allocated"
            | "associated" | "present" => true,
            _ => ordered
                .iter()
                .copied()
                .flatten()
                .all(|t| matches!(element(t), Ty::Scalar(_))),
        }
    };
    if !valid {
        bail!("no matching specific procedure for generic `{name}` with argument types {types:?}");
    }
    Ok(Some(canonical))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_result_inference_uses_keyword_kind_and_reduced_rank() {
        let mut resolver = Resolver::default();
        resolver.procedures.insert(
            "custom".into(),
            Procedure {
                signature: crate::named_arguments::Signature {
                    params: vec!["x".into()],
                    optional: HashSet::new(),
                },
                args: vec![Ty::Derived("tag".into())],
                result: scalar(&TypeSpec::Integer(None)),
                inputs: true,
                subroutine: false,
                array_specialization: false,
                elemental: false,
            },
        );
        let vars = Vars::from([
            (
                "a".into(),
                Ty::Array(Box::new(scalar(&TypeSpec::Integer(None))), 2),
            ),
            (
                "mask".into(),
                Ty::Array(Box::new(scalar(&TypeSpec::Logical)), 2),
            ),
        ]);
        let keyword = |key: &str, value| Expr::Call(format!("@keyword:{key}"), vec![value]);
        for (name, args, expected) in [
            (
                "maxval",
                vec![
                    keyword("dim", Expr::IntLit("1".into())),
                    keyword("array", Expr::Ident("a".into())),
                ],
                Ty::Array(Box::new(scalar(&TypeSpec::Integer(None))), 1),
            ),
            (
                "minval",
                vec![
                    keyword("mask", Expr::Ident("mask".into())),
                    keyword("array", Expr::Ident("a".into())),
                ],
                scalar(&TypeSpec::Integer(None)),
            ),
            (
                "count",
                vec![
                    keyword("kind", Expr::IntLit("8".into())),
                    keyword("mask", Expr::Ident("mask".into())),
                ],
                scalar(&TypeSpec::Integer(Some(8))),
            ),
        ] {
            let bindings = Bindings::from([(format!("generic:{name}"), vec!["custom".into()])]);
            let mut expression = Expr::Call(name.into(), args);
            assert_eq!(
                resolver
                    .expression(&mut expression, &vars, &bindings)
                    .unwrap(),
                expected,
                "{name}"
            );
        }
    }
}

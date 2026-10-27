// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.
use crate::ast::TypeSpec;
use anyhow::{bail, Result};
pub const NAMES: &[&str] = &[
    "scale",
    "fraction",
    "exponent",
    "set_exponent",
    "nearest",
    "spacing",
    "rrspacing",
    "@ieee_value",
];
pub fn keys(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "scale" | "set_exponent" | "@ieee_value" => &["x", "i"],
        "nearest" => &["x", "s"],
        "fraction" | "exponent" | "spacing" | "rrspacing" => &["x"],
        _ => return None,
    })
}
pub fn result_kind(name: &str, kinds: &[TypeSpec]) -> Result<TypeSpec> {
    let keys = keys(name).unwrap();
    if kinds.len() != keys.len() {
        bail!(
            "{} expects {} arguments, got {}",
            name.to_ascii_uppercase(),
            keys.len(),
            kinds.len()
        );
    }
    if !matches!(kinds[0], TypeSpec::Real | TypeSpec::DoublePrecision) {
        bail!(
            "{} X must be REAL or DOUBLE PRECISION",
            name.to_ascii_uppercase()
        );
    }
    if kinds.len() == 2 {
        let valid = if name == "nearest" {
            matches!(kinds[1], TypeSpec::Real | TypeSpec::DoublePrecision)
        } else {
            matches!(kinds[1], TypeSpec::Integer(None | Some(1 | 2 | 4 | 8)))
        };
        if !valid {
            bail!(
                "{} {} must be {}",
                name.to_ascii_uppercase(),
                keys[1].to_ascii_uppercase(),
                if name == "nearest" {
                    "REAL or DOUBLE PRECISION"
                } else {
                    "INTEGER of a supported kind"
                }
            );
        }
    }
    Ok(if name == "exponent" {
        TypeSpec::Integer(None)
    } else {
        kinds[0].clone()
    })
}
pub fn tag(name: &str, kind: u8) -> String {
    format!("@model:{name}:{kind}")
}
pub fn decode(tag: &str) -> Option<(&str, u8)> {
    let (name, kind) = tag.strip_prefix("@model:")?.split_once(':')?;
    let kind = kind.parse().ok()?;
    (keys(name).is_some() && matches!(kind, 4 | 8)).then_some((name, kind))
}
pub fn tag_kind(tag: &str) -> Option<TypeSpec> {
    let (name, kind) = decode(tag)?;
    Some(if name == "exponent" {
        TypeSpec::Integer(None)
    } else if kind == 8 {
        TypeSpec::DoublePrecision
    } else {
        TypeSpec::Real
    })
}
pub fn opcode(name: &str) -> i64 {
    NAMES.iter().position(|n| *n == name).unwrap() as i64
}

/// Specifics of our intrinsic module, after USE/generic resolution. Matching
/// their defining identity keeps user procedures named IEEE_VALUE unchanged.
pub(crate) fn ieee_value_kind(name: &str) -> Option<u8> {
    match name {
        "@module$_intrinsic_ieee_arithmetic$ieee_value4" => Some(4),
        "@module$_intrinsic_ieee_arithmetic$ieee_value8" => Some(8),
        _ => None,
    }
}
pub(crate) fn ieee_value_tag(name: &str) -> bool {
    matches!(decode(name), Some(("@ieee_value", _)))
}
pub(crate) fn zero(kind: &TypeSpec) -> crate::ast::Expr {
    let zero = crate::ast::Expr::RealLit("0.0".into());
    // Kind suffixes have already been normalized at this lowering stage.
    if *kind == TypeSpec::DoublePrecision {
        crate::ast::Expr::Call("dble".into(), vec![zero])
    } else {
        zero
    }
}

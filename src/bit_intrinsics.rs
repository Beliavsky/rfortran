// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Scalar integer bit operations, their kinds, and control argument bounds.
use crate::ast::TypeSpec;
use anyhow::{bail, Result};

pub const NAMES: &[&str] = &[
    "btest", "iand", "ior", "ieor", "not", "ibset", "ibclr", "ibits", "ishft",
];

pub fn keys(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "iand" | "ior" | "ieor" => &["i", "j"],
        "not" => &["i"],
        "btest" | "ibset" | "ibclr" => &["i", "pos"],
        "ibits" => &["i", "pos", "len"],
        "ishft" => &["i", "shift"],
        _ => return None,
    })
}

pub fn result_kind(name: &str, kinds: &[TypeSpec]) -> Result<TypeSpec> {
    let keys = keys(name).expect("known bit intrinsic");
    if kinds.len() != keys.len() {
        bail!(
            "{} expects {} arguments, got {}",
            name.to_ascii_uppercase(),
            keys.len(),
            kinds.len()
        );
    }
    let mut bytes = vec![];
    for (key, kind) in keys.iter().zip(kinds) {
        let TypeSpec::Integer(k) = kind else {
            bail!(
                "{} argument {} must be INTEGER",
                name.to_ascii_uppercase(),
                key.to_ascii_uppercase()
            );
        };
        let k = k.unwrap_or(4);
        if !matches!(k, 1 | 2 | 4 | 8) {
            bail!(
                "{} currently supports INTEGER kinds 1, 2, 4 and 8, not {k}",
                name.to_ascii_uppercase()
            );
        }
        bytes.push(k);
    }
    if matches!(name, "iand" | "ior" | "ieor") && bytes[0] != bytes[1] {
        bail!(
            "{} arguments must have the same INTEGER kind",
            name.to_ascii_uppercase()
        );
    }
    Ok(if name == "btest" {
        TypeSpec::Logical
    } else {
        TypeSpec::Integer(Some(bytes[0]))
    })
}

pub fn tag(name: &str, bytes: u8) -> String {
    format!("@bit:{name}:{bytes}")
}
pub fn decode(tag: &str) -> Option<(&str, u8)> {
    let (name, bytes) = tag.strip_prefix("@bit:")?.split_once(':')?;
    let bytes = bytes.parse().ok()?;
    (keys(name).is_some() && matches!(bytes, 1 | 2 | 4 | 8)).then_some((name, bytes))
}
pub fn tag_kind(tag: &str) -> Option<TypeSpec> {
    let (name, bytes) = decode(tag)?;
    Some(if name == "btest" {
        TypeSpec::Logical
    } else {
        TypeSpec::Integer(Some(bytes))
    })
}
pub fn range_message(name: &str, bits: i64) -> String {
    let limits = match name {
        "ibits" => format!("POS and LEN must be nonnegative with POS + LEN <= {bits}"),
        "ishft" => format!("SHIFT must be between {} and {bits}", -bits),
        _ => format!("POS must be between 0 and {}", bits - 1),
    };
    format!("{}: {limits}", name.to_ascii_uppercase())
}
pub fn validate_controls(name: &str, bytes: u8, values: &[Option<i64>]) -> Result<()> {
    let bits = i64::from(bytes) * 8;
    let bad = match name {
        "btest" | "ibset" | "ibclr" => values[1].is_some_and(|n| n < 0 || n >= bits),
        "ishft" => values[1].is_some_and(|n| n < -bits || n > bits),
        "ibits" => {
            values[1].is_some_and(|n| n < 0 || n > bits)
                || values[2].is_some_and(|n| n < 0 || n > bits)
                || matches!((values[1],values[2]),(Some(p),Some(n)) if i128::from(p)+i128::from(n)>i128::from(bits))
        }
        _ => false,
    };
    if bad {
        bail!("{}", range_message(name, bits));
    }
    Ok(())
}

pub fn evaluate(name: &str, bytes: u8, values: &[i64]) -> Result<i64> {
    validate_controls(
        name,
        bytes,
        &values.iter().map(|n| Some(*n)).collect::<Vec<_>>(),
    )?;
    let bits = u32::from(bytes) * 8;
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let i = values[0] as u64 & mask;
    let value = match name {
        "btest" => (i >> values[1] as u32) & 1,
        "iand" => i & values[1] as u64,
        "ior" => i | values[1] as u64,
        "ieor" => i ^ values[1] as u64,
        "not" => !i,
        "ibset" => i | (1u64 << values[1] as u32),
        "ibclr" => i & !(1u64 << values[1] as u32),
        "ibits" => {
            let len = values[2] as u32;
            let mask = if len == 64 {
                u64::MAX
            } else {
                (1u64 << len) - 1
            };
            i.checked_shr(values[1] as u32).unwrap_or(0) & mask
        }
        "ishft" => {
            if values[1] >= 0 {
                i.checked_shl(values[1] as u32).unwrap_or(0)
            } else {
                i.checked_shr((-values[1]) as u32).unwrap_or(0)
            }
        }
        _ => bail!("unknown bit intrinsic `{name}`"),
    } & mask;
    Ok(if name == "btest" {
        value as i64
    } else {
        ((value << (64 - bits)) as i64) >> (64 - bits)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_sign_bits_zero_fields_and_full_width_shifts() {
        for bytes in [1, 2, 4, 8] {
            let bits = i64::from(bytes) * 8;
            assert_eq!(evaluate("btest", bytes, &[-1, bits - 1]).unwrap(), 1);
            assert_eq!(
                evaluate("ibset", bytes, &[0, bits - 1]).unwrap(),
                if bits == 64 {
                    i64::MIN
                } else {
                    -(1i64 << (bits - 1))
                }
            );
            assert_eq!(evaluate("ibits", bytes, &[-1, 0, bits]).unwrap(), -1);
            assert_eq!(evaluate("ibits", bytes, &[-1, bits, 0]).unwrap(), 0);
            assert_eq!(evaluate("ishft", bytes, &[-1, -bits]).unwrap(), 0);
            assert_eq!(evaluate("ishft", bytes, &[-1, bits]).unwrap(), 0);
            assert_eq!(
                evaluate("ishft", bytes, &[-1, -1]).unwrap(),
                if bits == 64 {
                    i64::MAX
                } else {
                    (1i64 << (bits - 1)) - 1
                }
            );
            assert!(evaluate("ibset", bytes, &[0, bits]).is_err());
            assert!(evaluate("ibits", bytes, &[0, bits, 1]).is_err());
            assert!(evaluate("ishft", bytes, &[0, i64::MIN]).is_err());
        }
    }
}

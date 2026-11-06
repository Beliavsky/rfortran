// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Scalar integer bit operations, their kinds, and control argument bounds.
use crate::ast::{Expr, TypeSpec};
use anyhow::{bail, Result};

pub const NAMES: &[&str] = &[
    "btest", "iand", "ior", "ieor", "not", "ibset", "ibclr", "ibits", "ishft",
    "popcnt", "poppar", "leadz", "trailz", "dshiftl", "dshiftr",
    "shiftl", "shiftr", "shifta", "merge_bits",
    "ishftc", "maskl", "maskr", "bge", "bgt", "ble", "blt",
];

/// Public calls have optional arguments; internal tags always use canonical
/// arguments. Masks carry a typed zero mold followed by their bit count.
pub fn normalize(name: &str, args: &[Expr]) -> Result<Vec<Expr>> {
    let int = |n:i64| Expr::IntLit(n.to_string());
    if name == "ishftc" {
        if !(2..=3).contains(&args.len()) {bail!("ISHFTC expects I, SHIFT and optional SIZE");}
        let size=args.get(2).filter(|a|!crate::named_arguments::omitted(a)).cloned()
            .unwrap_or_else(||Expr::Call("bit_size".into(),vec![args[0].clone()]));
        return Ok(vec![args[0].clone(),args[1].clone(),size]);
    }
    if matches!(name,"maskl"|"maskr") {
        if !(1..=2).contains(&args.len()) {bail!("{} expects I and optional KIND",name.to_ascii_uppercase());}
        let kind=args.get(1).filter(|a|!crate::named_arguments::omitted(a)).cloned().unwrap_or_else(||int(4));
        return Ok(vec![Expr::Call("int".into(),vec![int(0),kind]),args[0].clone()]);
    }
    Ok(args.to_vec())
}
pub fn comparison(name: &str) -> bool {matches!(name,"bge"|"bgt"|"ble"|"blt")}
pub fn logical(name: &str) -> bool {name=="btest" || comparison(name)}
pub fn zero_extend(value:i64,bytes:u8)->i64 {
    if bytes==8 {value} else {(value as u64 & ((1u64 << (bytes*8))-1)) as i64}
}

/// These return a default INTEGER count rather than a value of I's kind.
pub fn counts(name: &str) -> bool {
    matches!(name, "popcnt" | "poppar" | "leadz" | "trailz")
}

pub fn keys(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "iand" | "ior" | "ieor" | "bge" | "bgt" | "ble" | "blt" => &["i", "j"],
        "not" | "popcnt" | "poppar" | "leadz" | "trailz" => &["i"],
        "dshiftl" | "dshiftr" => &["i", "j", "shift"],
        "btest" | "ibset" | "ibclr" => &["i", "pos"],
        "ibits" => &["i", "pos", "len"],
        "ishft" | "shiftl" | "shiftr" | "shifta" => &["i", "shift"],
        "merge_bits" => &["i", "j", "mask"],
        "ishftc" => &["i", "shift", "size"],
        "maskl" | "maskr" => &["i", "kind"],
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
    let canonical_keys: &[&str] = if matches!(name,"maskl"|"maskr") { &["kind", "i"] } else { keys };
    for (key, kind) in canonical_keys.iter().zip(kinds) {
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
    if matches!(name, "iand" | "ior" | "ieor" | "dshiftl" | "dshiftr" | "merge_bits") && bytes[0] != bytes[1]
        || name == "merge_bits" && bytes[0] != bytes[2] {
        bail!(
            "{} arguments must have the same INTEGER kind",
            name.to_ascii_uppercase()
        );
    }
    Ok(if logical(name) {
        TypeSpec::Logical
    } else if counts(name) {
        TypeSpec::Integer(Some(4))
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
    Some(if logical(name) {
        TypeSpec::Logical
    } else if counts(name) {
        TypeSpec::Integer(Some(4))
    } else {
        TypeSpec::Integer(Some(bytes))
    })
}
pub fn range_message(name: &str, bits: i64) -> String {
    let limits = match name {
        "ishftc" => format!("SIZE must be between 1 and {bits}; SHIFT must be between -SIZE and SIZE"),
        "maskl" | "maskr" => format!("I must be between 0 and {bits}"),
        "ibits" => format!("POS and LEN must be nonnegative with POS + LEN <= {bits}"),
        "ishft" => format!("SHIFT must be between {} and {bits}", -bits),
        "dshiftl" | "dshiftr" | "shiftl" | "shiftr" | "shifta" => format!("SHIFT must be between 0 and {bits}"),
        _ => format!("POS must be between 0 and {}", bits - 1),
    };
    format!("{}: {limits}", name.to_ascii_uppercase())
}
pub fn validate_controls(name: &str, bytes: u8, values: &[Option<i64>]) -> Result<()> {
    let bits = i64::from(bytes) * 8;
    let bad = match name {
        "ishftc" => values[2].is_some_and(|n| n<1 || n>bits)
            || matches!((values[1],values[2]),(Some(s),Some(n)) if n>=1 && n<=bits && (s < -n || s > n)),
        "maskl" | "maskr" => values[1].is_some_and(|n|n<0 || n>bits),
        "btest" | "ibset" | "ibclr" => values[1].is_some_and(|n| n < 0 || n >= bits),
        "ishft" => values[1].is_some_and(|n| n < -bits || n > bits),
        "dshiftl" | "dshiftr" => values[2].is_some_and(|n| n < 0 || n > bits),
        "shiftl" | "shiftr" | "shifta" => values[1].is_some_and(|n| n < 0 || n > bits),
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
        "bge" => u64::from((values[0] as u64)>=(values[1] as u64)),
        "bgt" => u64::from((values[0] as u64)>(values[1] as u64)),
        "ble" => u64::from((values[0] as u64)<=(values[1] as u64)),
        "blt" => u64::from((values[0] as u64)<(values[1] as u64)),
        "maskl" | "maskr" => {
            let n=values[1] as u32;
            let low=if n==64 {u64::MAX}else{(1u64<<n)-1};
            if name=="maskl" {low.checked_shl(bits-n).unwrap_or(0)}else{low}
        }
        "ishftc" => {
            let size=values[2] as u32;
            let field_mask=if size==64 {u64::MAX}else{(1u64<<size)-1};
            let shift=values[1].rem_euclid(i64::from(size)) as u32;
            let field=i&field_mask;
            let rotated=(field.checked_shl(shift).unwrap_or(0)|field.checked_shr(size-shift).unwrap_or(0))&field_mask;
            (i & !field_mask)|rotated
        }
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
        "shiftl" => i.checked_shl(values[1] as u32).unwrap_or(0),
        "shiftr" => i.checked_shr(values[1] as u32).unwrap_or(0),
        // The sign bit of I's kind fills the vacated bits.
        "shifta" => ((((i << (64 - bits)) as i64) >> (64 - bits)) >> values[1].min(63)) as u64,
        "merge_bits" => (i & values[2] as u64) | (values[1] as u64 & !(values[2] as u64)),
        "popcnt" => u64::from(i.count_ones()),
        "poppar" => u64::from(i.count_ones() & 1),
        "leadz" => u64::from(i.leading_zeros() - (64 - bits)),
        "trailz" => u64::from(i.trailing_zeros().min(bits)),
        "dshiftl" | "dshiftr" => {
            let j = values[1] as u64 & mask;
            let shift = values[2] as u32;
            // DSHIFTR(I, J, S) is DSHIFTL(I, J, BITS-S).
            let left = if name == "dshiftl" { shift } else { bits - shift };
            i.checked_shl(left).unwrap_or(0) | j.checked_shr(bits - left).unwrap_or(0)
        }
        _ => bail!("unknown bit intrinsic `{name}`"),
    } & mask;
    Ok(if logical(name) || counts(name) {
        value as i64
    } else {
        ((value << (64 - bits)) as i64) >> (64 - bits)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotations_masks_and_unsigned_comparisons() {
        for bytes in [1,2,4,8] {
            let bits=i64::from(bytes)*8;
            for shift in [-bits,0,bits] {
                assert_eq!(evaluate("ishftc",bytes,&[-1,shift,bits]).unwrap(),-1);
                assert_eq!(evaluate("ishftc",bytes,&[1,shift,bits]).unwrap(),1);
            }
            assert_eq!(evaluate("ishftc",bytes,&[29,1,4]).unwrap(),27);
            assert_eq!(evaluate("ishftc",bytes,&[29,-1,4]).unwrap(),30);
            assert_eq!(evaluate("maskl",bytes,&[0,0]).unwrap(),0);
            assert_eq!(evaluate("maskr",bytes,&[0,0]).unwrap(),0);
            assert_eq!(evaluate("maskl",bytes,&[0,bits]).unwrap(),-1);
            assert_eq!(evaluate("maskr",bytes,&[0,bits]).unwrap(),-1);
            assert_eq!(evaluate("maskr",bytes,&[0,3]).unwrap(),7);
            assert_eq!(evaluate("maskl",bytes,&[0,1]).unwrap(),if bytes==8 {i64::MIN}else{-(1i64<<(bits-1))});
            for bad in [i64::MIN,-1,bits+1,i64::MAX] {
                assert!(evaluate("maskl",bytes,&[0,bad]).is_err());
                assert!(evaluate("maskr",bytes,&[0,bad]).is_err());
                assert!(evaluate("ishftc",bytes,&[1,0,bad]).is_err());
            }
            for bad in [i64::MIN,-bits-1,bits+1,i64::MAX] {
                assert!(evaluate("ishftc",bytes,&[1,bad,bits]).is_err());
            }
            let short=zero_extend(-1,bytes);
            assert_eq!(evaluate("bge",8,&[short,-1]).unwrap(),i64::from(bytes==8));
            assert_eq!(evaluate("blt",8,&[short,-1]).unwrap(),i64::from(bytes<8));
        }
    }
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
            assert_eq!(evaluate("popcnt", bytes, &[-1]).unwrap(), bits);
            assert_eq!(evaluate("poppar", bytes, &[7]).unwrap(), 1);
            assert_eq!(evaluate("leadz", bytes, &[0]).unwrap(), bits);
            assert_eq!(evaluate("leadz", bytes, &[1]).unwrap(), bits - 1);
            assert_eq!(evaluate("leadz", bytes, &[-1]).unwrap(), 0);
            assert_eq!(evaluate("trailz", bytes, &[0]).unwrap(), bits);
            assert_eq!(evaluate("trailz", bytes, &[96]).unwrap(), 5);
            assert_eq!(evaluate("dshiftl", bytes, &[-1, 0, 0]).unwrap(), -1);
            assert_eq!(evaluate("dshiftl", bytes, &[0, -1, bits]).unwrap(), -1);
            assert_eq!(evaluate("dshiftl", bytes, &[0, -1, 1]).unwrap(), 1);
            assert_eq!(evaluate("dshiftr", bytes, &[1, 0, 1]).unwrap(), if bits == 64 { i64::MIN } else { -(1i64 << (bits - 1)) });
            assert_eq!(evaluate("dshiftr", bytes, &[0, 2, 1]).unwrap(), 1);
            assert!(evaluate("dshiftl", bytes, &[0, 0, bits + 1]).is_err());
        }
    }
}

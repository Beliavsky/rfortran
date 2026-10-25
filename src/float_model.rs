// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Binary32/binary64 model operations shared by constant folding and execution.
//! Integer significands avoid intermediate overflow and double rounding in SCALE.
fn layout(kind: u8) -> (u32, i64, u64) {
    if kind == 4 {
        (23, 127, 255)
    } else {
        (52, 1023, 2047)
    }
}
fn bits(x: f64, kind: u8) -> u64 {
    if kind == 4 {
        (x as f32).to_bits() as u64
    } else {
        x.to_bits()
    }
}
fn value(bits: u64, kind: u8) -> f64 {
    if kind == 4 {
        f32::from_bits(bits as u32) as f64
    } else {
        f64::from_bits(bits)
    }
}
fn normalized(bits: u64, p: u32, max: u64) -> (u64, i64) {
    let field = (bits >> p) & max;
    let fraction = bits & ((1 << p) - 1);
    if field != 0 {
        ((1 << p) | fraction, field as i64)
    } else {
        let shift = p - (63 - fraction.leading_zeros());
        (fraction << shift, 1 - shift as i64)
    }
}
pub fn scale(x: f64, i: i64, kind: u8) -> f64 {
    let (p, _, max) = layout(kind);
    let b = bits(x, kind);
    let sign = b & (1 << (p + if kind == 4 { 8 } else { 11 }));
    if b & !sign == 0 || (b >> p) & max == max {
        return value(b, kind);
    }
    let (mantissa, exp) = normalized(b, p, max);
    let exp = exp.saturating_add(i);
    if exp >= max as i64 {
        return value(sign | (max << p), kind);
    }
    if exp > 0 {
        return value(
            sign | ((exp as u64) << p) | (mantissa & ((1 << p) - 1)),
            kind,
        );
    }
    if exp < -(p as i64) {
        return value(sign, kind);
    }
    let shift = (1 - exp) as u32;
    let mut rounded = mantissa >> shift;
    let remainder = mantissa & ((1 << shift) - 1);
    let half = 1 << (shift - 1);
    if remainder > half || remainder == half && rounded & 1 != 0 {
        rounded += 1;
    }
    value(sign | rounded, kind)
}
pub fn exponent(x: f64, kind: u8) -> i64 {
    let (p, bias, max) = layout(kind);
    let b = bits(x, kind);
    if (b >> p) & max == max {
        return i32::MAX as i64;
    }
    if b & ((max << p) | ((1 << p) - 1)) == 0 {
        return 0;
    }
    normalized(b, p, max).1 - bias + 1
}
pub fn fraction(x: f64, kind: u8) -> f64 {
    let (p, bias, max) = layout(kind);
    let b = bits(x, kind);
    if (b >> p) & max == max {
        return f64::NAN;
    }
    let sign = b & (1 << (p + if kind == 4 { 8 } else { 11 }));
    if b & !sign == 0 {
        return value(b, kind);
    }
    let mantissa = normalized(b, p, max).0;
    value(
        sign | (((bias - 1) as u64) << p) | (mantissa & ((1 << p) - 1)),
        kind,
    )
}
pub fn nearest(x: f64, s: f64, kind: u8) -> f64 {
    let b = bits(x, kind);
    let x = value(b, kind);
    if x.is_nan() || s.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        let sign = if s < 0.0 {
            1 << if kind == 4 { 31 } else { 63 }
        } else {
            0
        };
        return value(sign | 1, kind);
    }
    if (s > 0.0 && x == f64::INFINITY) || (s < 0.0 && x == f64::NEG_INFINITY) {
        return x;
    }
    value(if (x > 0.0) == (s > 0.0) { b + 1 } else { b - 1 }, kind)
}
/// Opcodes: SCALE, FRACTION, EXPONENT, SET_EXPONENT, NEAREST, SPACING, RRSPACING, IEEE_VALUE.
pub fn evaluate(op: i64, kind: u8, x: f64, i: i64, s: f64) -> f64 {
    let x = if op == 7 { 0.0 } else { value(bits(x, kind), kind) };
    match op {
        0 => scale(x, i, kind),
        1 => fraction(x, kind),
        2 => exponent(x, kind) as f64,
        3 => scale(fraction(x, kind), i, kind),
        4 => nearest(x, s, kind),
        5 => {
            if !x.is_finite() {
                return f64::NAN;
            }
            let (p, bias, _) = layout(kind);
            let e = if x == 0.0 {
                1 - bias
            } else {
                (exponent(x, kind) - p as i64 - 1).max(1 - bias)
            };
            scale(1.0, e, kind)
        }
        6 => scale(
            fraction(x, kind).abs(),
            if kind == 4 { 24 } else { 53 },
            kind,
        ),
        7 => {
            let (fraction, exponent, sign) = if kind == 4 { (23, 255u64, 1u64 << 31) } else { (52, 2047u64, 1u64 << 63) };
            let inf = exponent << fraction;
            let pattern = match i {
                1 => inf | 1, // signaling NaN (conversion may quiet it)
                2 => inf | (1 << (fraction - 1)),
                3 => sign | inf,
                4 => sign | ((exponent / 2) << fraction),
                5 => sign | 1,
                6 => sign,
                7 => 0,
                8 => 1,
                9 => (exponent / 2) << fraction,
                10 => inf,
                _ => return f64::NAN,
            };
            value(pattern, kind)
        }
        _ => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale_matches_hardware_multiplication_by_exact_powers() {
        let mut seed = 0x7192_caae_d528_93fbu64;
        for _ in 0..10000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let x = f64::from_bits(seed);
            let n = ((seed >> 40) % 2001) as i32 - 1000;
            if x.is_finite() {
                assert_eq!(
                    scale(x, n as i64, 8).to_bits(),
                    (x * 2.0f64.powi(n)).to_bits()
                );
            }
            let x = f32::from_bits(seed as u32);
            let n = ((seed >> 40) % 241) as i32 - 120;
            if x.is_finite() {
                assert_eq!(
                    scale(x as f64, n as i64, 4).to_bits(),
                    ((x * 2.0f32.powi(n)) as f64).to_bits()
                );
            }
        }
    }
    #[test]
    fn scaling_boundaries_and_rounding() {
        for kind in [4, 8] {
            let (tiny, smallest, largest, p) = if kind == 4 {
                (
                    f32::MIN_POSITIVE as f64,
                    f32::from_bits(1) as f64,
                    f32::MAX as f64,
                    24,
                )
            } else {
                (f64::MIN_POSITIVE, f64::from_bits(1), f64::MAX, 53)
            };
            assert_eq!(scale(tiny, 1 - p, kind), smallest);
            assert_eq!(scale(smallest, p - 1, kind), tiny);
            assert_eq!(scale(smallest, -1, kind), 0.0);
            assert_eq!(scale(3.0 * smallest, -1, kind), 2.0 * smallest);
            assert_eq!(scale(largest, 1, kind), f64::INFINITY);
            assert_eq!(scale(-1.0, i64::MIN, kind).to_bits(), (-0.0f64).to_bits());
            assert_eq!(scale(1.0, i64::MAX, kind), f64::INFINITY);
            assert_eq!(
                scale(fraction(smallest, kind), exponent(smallest, kind), kind),
                smallest
            );
            assert_eq!(evaluate(5, kind, smallest, 0, 0.0), tiny);
            assert_eq!(fraction(-0.0, kind).to_bits(), (-0.0f64).to_bits());
            assert_eq!(
                evaluate(3, kind, -0.0, 10, 0.0).to_bits(),
                (-0.0f64).to_bits()
            );
            assert_eq!(nearest(-smallest, 1.0, kind).to_bits(), (-0.0f64).to_bits());
        }
    }
}

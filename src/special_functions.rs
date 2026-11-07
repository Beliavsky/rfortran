// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

//! Invocation-local special-function arithmetic shared by both execution modes.
//! Callers supply their existing platform math functions; no shared working state.
pub fn erfc_scaled(x: f64, exponential: impl Fn(f64)->f64, complementary: impl Fn(f64)->f64) -> f64 {
    if x >= 26.0 {
        // At 26 the first omitted term is below double precision. Divide
        // before multiplying to avoid overflowing X squared or X*sqrt(pi).
        let y=(0.5/x)/x;
        let coefficients=[1.0,-1.0,3.0,-15.0,105.0,-945.0,10395.0,-135135.0];
        let mut series=coefficients[7];
        for c in coefficients[..7].iter().rev() {series=c+y*series;}
        (series/x)/1.7724538509055160273
    } else {
        // Includes negative arguments and NaNs. Positive infinity follows the
        // asymptotic branch and returns zero; negative infinity returns infinity.
        exponential(x*x)*complementary(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaled_tail_avoids_intermediate_overflow() {
        for x in [26.0,100.0,1e200,f64::MAX,f64::INFINITY] {
            let y=erfc_scaled(x,|_|panic!("tail must not evaluate exp"),|_|panic!("tail must not evaluate erfc"));
            assert!(y.is_finite() && y>=0.0);
            if x.is_finite() {assert!(y>0.0);}
        }
        assert_eq!(erfc_scaled(0.0,|v|v.exp(),|_|1.0),1.0);
        assert_eq!(erfc_scaled(-0.0,|v|v.exp(),|_|1.0),1.0);
        assert!((erfc_scaled(26.0,|_|0.0,|_|0.0)-0.021683584850562907).abs()<1e-17);
    }
}

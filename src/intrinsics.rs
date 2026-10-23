// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Beliavsky
// See LICENSE-MIT in the repository root.

/// Scalar real math routines with the same argument order as the C math library.
pub const REAL_MATH: &[(&str, usize)] = &[
    ("sqrt", 1), ("exp", 1), ("log", 1), ("log10", 1),
    ("sin", 1), ("cos", 1), ("tan", 1),
    ("asin", 1), ("acos", 1), ("atan", 1), ("atan2", 2),
    ("sinh", 1), ("cosh", 1), ("tanh", 1),
    ("asinh", 1), ("acosh", 1), ("atanh", 1),
    ("erf", 1), ("erfc", 1), ("gamma", 1), ("log_gamma", 1), ("hypot", 2),
    ("aint", 1), ("anint", 1),
    ("bessel_j0", 1), ("bessel_j1", 1), ("bessel_y0", 1), ("bessel_y1", 1),
    // BESSEL_JN(N, X) and BESSEL_YN(N, X) are rewritten to these, with X first.
    ("rfort_bessel_jn", 2), ("rfort_bessel_yn", 2),
];

pub fn math_arity(name: &str) -> Option<usize> {
    REAL_MATH.iter().find(|(n, _)| name.eq_ignore_ascii_case(n)).map(|(_, arity)| *arity)
}

/// The C math library function for a REAL_MATH intrinsic.
pub fn c_symbol(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "gamma" => "tgamma".into(),
        "log_gamma" => "lgamma".into(),
        "aint" => "trunc".into(),
        "anint" => "round".into(),
        "bessel_j0" | "bessel_j1" | "bessel_y0" | "bessel_y1" =>
            format!("{}{}", if cfg!(windows) { "_" } else { "" }, &name[7..]),
        other => other.into(),
    }
}

/// Elemental real predicates return default LOGICAL with their argument's shape.
pub const REAL_PREDICATES: &[&str] = &["isnan"];
pub fn real_predicate(name:&str)->bool {
    REAL_PREDICATES.iter().any(|n|name.eq_ignore_ascii_case(n))
}

/// Real-to-integer elemental rounding intrinsics.
pub fn integer_rounding(name:&str)->bool { matches!(name,"nint"|"floor"|"ceiling") }
pub(crate) fn rounding_kind(name:&str,args:&[crate::ast::Expr])->anyhow::Result<crate::ast::TypeSpec> {
    use anyhow::{bail,Context};
    let label=name.to_ascii_uppercase();
    if !(1..=2).contains(&args.len()) {bail!("{label} expects A and optional KIND");}
    let kind=args.get(1).map(crate::parameters::integer_constant).transpose()
        .with_context(||format!("{label} KIND must be a constant INTEGER"))?.unwrap_or(4);
    if !matches!(kind,1|2|4|8) {bail!("{label} KIND must be a supported INTEGER kind (1, 2, 4 or 8)");}
    Ok(crate::ast::TypeSpec::Integer(Some(kind as u8)))
}

/// Language purity for public intrinsic interfaces known to the compiler. Keep compiler
/// internal expression markers in their owning lowering pass, not this catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purity { Pure, Impure }

pub const PURE_FUNCTIONS: &[&str] = &[
    "cmplx", "aimag", "conjg", "abs", "achar", "adjustl", "adjustr", "aint", "all", "allocated",
    "anint", "any", "associated", "bessel_jn", "bessel_yn", "bit_size", "ceiling", "char", "count", "cshift",
    "dble", "dim", "dot_product", "dprod", "eoshift", "findloc", "floor",
    "huge", "iachar", "iall", "iany", "ichar", "index", "int", "iparity", "ishftc", "kind",
    "lbound", "len", "len_trim", "lge", "lgt", "lle", "llt",
    "matmul", "max", "maxloc", "maxval", "merge", "min", "minloc",
    "minval", "mod", "modulo", "new_line", "next", "previous", "nint", "norm2", "out_of_range", "pack", "parity", "present",
    "product", "real", "repeat", "reshape", "scan", "selected_int_kind", "selected_real_kind", "selected_logical_kind",
    "shape", "sign", "size", "spread", "storage_size", "sum", "tiny", "transpose",
    "trim", "ubound", "unpack", "verify", "split", "tokenize",
];

pub const IMPURE_PROCEDURES: &[&str] = &[
    "random_number", "random_seed", "cpu_time", "system_clock", "date_and_time", "get_environment_variable", "getcwd", "get_command_argument",
];

pub const PURE_SUBROUTINES: &[&str] = &["move_alloc"];

pub fn purity(name: &str) -> Option<Purity> {
    let name=name.to_ascii_lowercase();
    if IMPURE_PROCEDURES.contains(&name.as_str()) {
        Some(Purity::Impure)
    } else if PURE_FUNCTIONS.contains(&name.as_str()) || PURE_SUBROUTINES.contains(&name.as_str()) || math_arity(&name).is_some() || real_predicate(&name)
        || crate::inquiries::numeric_model(&name) || crate::bit_intrinsics::keys(&name).is_some() || crate::real_intrinsics::keys(&name).is_some()
    {
        Some(Purity::Pure)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn classifies_pure_impure_and_unknown_intrinsics() {
        for name in super::PURE_FUNCTIONS.iter().copied()
            .chain(super::REAL_MATH.iter().map(|(name,_)|*name))
            .chain(super::REAL_PREDICATES.iter().copied())
            .chain(crate::bit_intrinsics::NAMES.iter().copied())
        {
            assert_eq!(super::purity(name),Some(super::Purity::Pure),"{name}");
            assert_eq!(super::purity(&name.to_uppercase()),Some(super::Purity::Pure),"{name}");
        }
        for name in super::IMPURE_PROCEDURES {
            assert_eq!(super::purity(name),Some(super::Purity::Impure),"{name}");
        }
        for name in ["user_function","@cpu_seconds","@random_seed_put","@unknown"] {
            assert_eq!(super::purity(name),None,"{name}");
        }
    }

    #[test]
    fn validates_random_number_and_loop_index_usage() {
        for (declarations, statement, valid) in [
            ("real :: x\ninteger :: i", "do i=1,3\ncall random_number(x)\nend do\nprint *, x", true),
            ("double precision :: x(3)", "call random_number(x)", true),
            ("real :: x(3)", "call random_number(x(2))", true),
            ("", "call random_number()", false),
            ("real :: x", "call random_number(x,x)", false),
            ("", "call random_number(1.0)", false),
            ("integer :: x", "call random_number(x)", false),
            ("real :: x", "call random_number(x+1.0)", false),
            ("real :: x(3)", "call random_number(x(1.0))", false),
            ("real :: x", "print *, random_number(x)", false),
        ] {
            let source = format!("program main\nimplicit none\n{declarations}\n{statement}\nend program main\n");
            let tokens = crate::lexer::lex(&source);
            let program = crate::parser::parse_with_src(&source, &tokens, "random.f90").unwrap();
            let errors = crate::sema::analyze_with_src(&program, &source, &tokens, "random.f90",
                &crate::sema::SemaSettings::default(), &std::collections::HashSet::new(), false);
            if valid {
                assert!(errors.is_empty(), "{statement}: {errors:?}");
            } else {
                assert!(errors.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic) && e.message.contains("RANDOM_NUMBER")), "{statement}: {errors:?}");
            }
        }
    }

    #[test]
    fn rejects_invalid_scalar_math_calls() {
        for (declarations, statement, expected) in [
            ("", "print *, max(1)", "expects 2 argument"),
            ("", "print *, min(1,2,3.0)", "same type and kind"),
            ("", "call random_seed(1)", "no-argument form"),
            ("", "print *, sqrt()", "expects 1 argument"),
            ("", "print *, sqrt(4)", "REAL or DOUBLE PRECISION"),
            ("", "print *, sin(\"text\")", "REAL or DOUBLE PRECISION"),
            ("", "print *, atan2(1.0)", "expects 2 argument"),
            ("", "print *, atan2(1.0, dble(2))", "same type and kind"),
            ("real :: a(2)", "print *, cos(a)", "array arguments"),
            ("", "call sqrt(1.0)", "function, not a subroutine"),
        ] {
            let source = format!("program main\nimplicit none\n{declarations}\n{statement}\nend program main\n");
            let tokens = crate::lexer::lex(&source);
            let program = crate::parser::parse_with_src(&source, &tokens, "intrinsic.f90").unwrap();
            let errors = crate::sema::analyze_with_src(&program, &source, &tokens, "intrinsic.f90",
                &crate::sema::SemaSettings::default(), &std::collections::HashSet::new(), false);
            assert!(errors.iter().any(|e| matches!(e.kind, crate::errors::CompileErrorKind::Semantic) && e.message.contains(expected)), "{statement}: {errors:?}");
        }
    }
}

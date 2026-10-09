/// Scalar real math routines with the same argument order as the C math library.
pub const REAL_MATH: &[(&str, usize)] = &[
    ("sqrt", 1), ("exp", 1), ("log", 1), ("log10", 1),
    ("sin", 1), ("cos", 1), ("tan", 1),
    ("asin", 1), ("acos", 1), ("atan", 1), ("atan2", 2),
    ("sinh", 1), ("cosh", 1), ("tanh", 1),
];

pub fn math_arity(name: &str) -> Option<usize> {
    REAL_MATH.iter().find(|(n, _)| name.eq_ignore_ascii_case(n)).map(|(_, arity)| *arity)
}

#[cfg(test)]
mod tests {
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

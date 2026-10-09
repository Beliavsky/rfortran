! echo: 1.5
program test_math_intrinsics
    implicit none
    real :: x, y, result
    double precision :: d
    read(*,*) x
    y = 0.5
    d = 2.0
    print *, "sqrt", sqrt(x)
    print *, "exp", exp(x)
    print *, "log", log(x)
    print *, "log10", log10(x)
    print *, "sin", SIN(x)
    print *, "cos", cos(x)
    print *, "tan", tan(x)
    print *, "asin", asin(y)
    print *, "acos", acos(y)
    print *, "atan", atan(x)
    print *, "atan2", atan2(y, x)
    print *, "atan2 quadrant", atan2(y, -x)
    print *, "sinh", sinh(x)
    print *, "cosh", cosh(x)
    print *, "tanh", tanh(x)
    print *, "nested", log(exp(x))
    print *, "expression", sqrt(x) + cos(y)
    print *, "double", sqrt(d)
    print *, "conversion", sqrt(real(9)), sqrt(dble(9))
    print *, "abs", abs(-3), abs(-x)
    print *, "int", int(2.9)
    result = sqrt(x)
    print *, "assignment", result
    block
        print *, "block", cos(x)
    end block
    print *, "double expression", sqrt(d + 0.0)
end program test_math_intrinsics

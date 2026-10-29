"""COMPLEX ELEMENTAL functions, internal functions of the same name in
different hosts, and dummy functions declared COMPLEX, from nswc_mod.f90.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_complex_procedures.py
"""
import unittest
import test_deallocate


class ComplexProcedures(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_real_storage_for_complex_dummy_is_rejected(self):
        # As BLKTR1 of nswc_mod.f90, which passes elements of a REAL work
        # array to COMPLEX array dummies: an external call, a module
        # procedure, and a dummy procedure.
        cp = """subroutine cp(n, bd)
integer, intent(in) :: n
complex :: bd(*)
print *, real(bd(1)), n
end subroutine cp
"""
        message = "passing REAL storage to a COMPLEX dummy is not supported"
        self.reject(cp + """program p
external cp
real :: w(4)
w = 1
call cp(1, w(1))
end program
""", message)
        self.reject("module m\ncontains\n" + cp + """end module m
program p
use m
real :: w(4)
w = 1
call cp(1, w)
end program
""", message)
        self.reject(cp + """subroutine outer(b, prd)
real :: b(*)
external prd
call prd(1, b(1))
end subroutine outer
program p
external cp
real :: w(4)
w = 1
call outer(w, cp)
end program
""", message)

    def test_elemental_complex_functions(self):
        self.check("""module m
implicit none
integer, parameter :: dp = kind(1.0d0)
contains
pure elemental complex function cdiv(a, b)
complex, intent(in) :: a, b
real :: d
d = real(b)**2 + aimag(b)**2
cdiv = cmplx((real(a)*real(b) + aimag(a)*aimag(b))/d, (aimag(a)*real(b) - real(a)*aimag(b))/d)
end function cdiv
elemental function twice(z) result(w)
complex(dp), intent(in) :: z
complex(dp) :: w
w = 2*z
end function twice
subroutine show(v)
complex, intent(in) :: v(:)
complex :: r(size(v))
r = cdiv(v, (0.0,1.0))
print '(4f7.2)', r
end subroutine show
end module m
program p
use m
implicit none
complex :: x(3), y(3), z(3), q(2,2), s
complex(dp) :: d(2)
x = [(1.0,2.0),(3.0,4.0),(5.0,6.0)]
y = [(1.0,0.0),(0.0,1.0),(2.0,0.0)]
s = cdiv(x(1), y(2))
print '(2f7.2)', s
z = cdiv(x, y)
print '(6f7.2)', z
z = cdiv(x, (2.0,0.0)) + x
print '(6f7.2)', z
q = reshape([x(1:2), y(1:2)], [2,2])
q = cdiv(q, (1.0,0.0))
print '(8f7.2)', q
d = [(1.0_dp,-1.0_dp),(0.5_dp,0.25_dp)]
d = twice(d)
print '(4f7.2)', d
call show(x(1:2))
end program
""", "   2.00  -1.00\n"
     "   1.00   2.00   4.00  -3.00   2.50   3.00\n"
     "   1.50   3.00   4.50   6.00   7.50   9.00\n"
     "   1.00   2.00   3.00   4.00   1.00   0.00   0.00   1.00\n"
     "   2.00  -2.00   1.00   0.50\n"
     "   2.00  -1.00   4.00  -3.00\n")

    def test_same_named_internal_functions(self):
        # Hosts with internal functions of the same name, one with COMPLEX
        # dummies, as ANORM in nswc_mod.f90.
        self.check("""module m
implicit none
contains
real function a(x, y)
real, intent(in) :: x, y
a = anorm(x, y)
contains
pure function anorm(x, y) result(r)
real, intent(in) :: x, y
real :: r
r = max(abs(x), abs(y))
end function anorm
end function a
real function b(z)
complex, intent(in) :: z
b = anorm(z)
contains
pure function anorm(z) result(r)
complex, intent(in) :: z
real :: r
r = max(abs(real(z)), abs(aimag(z)))
end function anorm
end function b
end module m
program p
use m
implicit none
print '(3f6.2)', a(-3.0, 2.0), b((1.0,-5.0)), anorm((2.0,0.0))
contains
real function anorm(z)
complex, intent(in) :: z
anorm = abs(z)
end function anorm
end program
""", "  3.00  5.00  2.00\n")

    def test_dummy_function_declared_complex(self):
        # As F in CPSC of nswc_mod.f90: a dummy declared COMPLEX and invoked.
        self.check("""module m
implicit none
contains
subroutine apply(f, x, y)
complex :: f
complex, intent(in) :: x
complex, intent(out) :: y
y = f(x + 1.0)
end subroutine apply
complex function sq(x)
complex, intent(in) :: x
sq = x*x
end function sq
end module m
program p
use m
implicit none
complex :: y
call apply(sq, (2.0,1.0), y)
print '(2f7.2)', y
end program
""", "   8.00   6.00\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

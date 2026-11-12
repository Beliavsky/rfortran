"""Default REAL literals have single precision; expression actuals of dummies
without INTENT."""
import unittest

import test_deallocate


class DefaultRealLiterals(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_literal_compares_with_single_precision_variable(self):
        # From gfortran.fortran-torture/execute/plusconst_1.f90.
        self.check("""program main
implicit none
real :: p, q = +1.234, r
integer :: i
data p /+3.1415/
data i /+42/
r = 1.234
print *, p == 3.1415, q == 1.234, r == 1.234, r == 1.234_4, r /= 1.234, i
end program main
""", " T T T T F 42\n")

    def test_literal_widened_to_double_precision(self):
        # A default REAL literal assigned to DOUBLE PRECISION keeps the value
        # it has in single precision; a D exponent keeps all digits.
        self.check("""program main
implicit none
double precision :: d, e
real(kind=8) :: f
d = 3.141592653589793
e = 3.141592653589793d0
f = 0.1
print '(3f20.15)', d, e, f
print '(f20.15)', dble(0.5) + 0.1
end program main
""", """   3.141592741012573   3.141592653589793   0.100000001490116
   0.600000001490116
""")

    def test_expression_actual_of_dummy_without_intent(self):
        # From gfortran.fortran-torture/execute/strlen.f90: a dummy without
        # INTENT that the procedure does not define may take a constant.
        self.check("""subroutine test2(c)
character(*) c
character(len(c)) d
d = c
print '(i0,1x,a)', len(d), d
end subroutine test2
program main
implicit none
character(5) :: a = "abcde"
call test2(a)
call test2("Foobar")
call show(2 + 3, "xy")
contains
subroutine show(n, s)
integer n
character(*) s
print '(i0,1x,a)', n, s
end subroutine show
end program main
""", "5 abcde\n6 Foobar\n5 xy\n")


if __name__ == '__main__':
    unittest.main()

"""REAL(4) arithmetic is rounded to REAL(4) after each operation, not only
when stored, as in M_intrinsics demo_epsilon.f90. Native builds only."""
import unittest

import test_deallocate


class SinglePrecision(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_intermediate_rounding(self):
        self.check("""program main
implicit none
integer, parameter :: dp = kind(1.0d0)
real :: x = 3.143, e, d
real(dp) :: y
integer :: n = 3
e = epsilon(x)
d = (x + e*0.5) - x
print *, d == 0
print *, x == x + e*0.999999, x < x + e*0.5, x + e*0.5 == x
print *, (x + e*0.5*n) - x > 0
if (x == x + e*0.5) print *, 'equal'
y = x + 1.0e-10_dp
print *, y > x
end program main
""", " T\n T F T\n T\n equal\n T\n", interpreter=False)


if __name__ == '__main__':
    unittest.main()

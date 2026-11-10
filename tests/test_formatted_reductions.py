"""Reductions with array-constructor arguments in formatted output, as in
M_intrinsics demo_iany.f90: the reduction is a scalar item, not an array
expression to evaluate into a temporary first."""
import unittest

import test_deallocate


class FormattedReductions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_constructor_masks(self):
        self.check("""program main
implicit none
logical, parameter :: T = .true., F = .false.
integer :: a(3) = [1, 2, 4], m(2,2) = reshape([1, 2, 3, 4], [2, 2])
print '(i0)', iany(a, mask=[T, F, T])
print '(i0)', iany(a, [T, F, T])
print '(i0,1x,i0)', sum(a, mask=[.true., .false., .true.]), product([2, 3])
print '(2i3)', sum(m, dim=1, mask=reshape([T, F, T, T], [2, 2]))
print '(3i3)', 2*[1, 2, 3]
print '(i0)', maxval(abs([-5, 2]))
end program main
""", "5\n5\n5 6\n  1  7\n  2  4  6\n5\n")


if __name__ == '__main__':
    unittest.main()

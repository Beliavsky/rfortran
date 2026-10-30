"""Internal formatted WRITE of array-valued expressions: reductions with DIM
and MASK, elemental expressions, SPREAD, UNPACK and MAXLOC results.

An internal WRITE once wrote only the first element of such an item (or 0 in
the interpreter), which failed the gfortran torture tests intrinsic_sum,
intrinsic_product, intrinsic_count, intrinsic_anyall, intrinsic_mmloc,
intrinsic_mmval, intrinsic_spread and intrinsic_unpack.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_internal_write_arrays.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class InternalWriteArrays(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_reductions_and_expressions(self):
        self.check("""program p
implicit none
integer :: a(3,3)
logical :: m(3,3)
character(len=12) :: line
a = reshape([1,2,3,4,5,6,7,8,9], [3,3])
m = .true.
m(1,1) = .false.
m(2,1) = .false.
write (line, '(3I4)') sum(a)
print '(3a)', '[', line, ']'
write (line, '(3I4)') sum(a, 1)
print '(3a)', '[', line, ']'
write (line, '(3I4)') product(a, 2, m)
print '(3a)', '[', line, ']'
write (line, '(3I4)') count(m, 1)
print '(3a)', '[', line, ']'
write (line, '(3L4)') any(m, 2)
print '(3a)', '[', line, ']'
write (line, '(3I4)') maxval(a, 1, m)
print '(3a)', '[', line, ']'
write (line, '(2I4)') maxloc(a)
print '(3a)', '[', line, ']'
write (line, '(3I4)') a(:,2) * 2
print '(3a)', '[', line, ']'
end program p
""", "[  45        ]\n[   6  15  24]\n[  28  40 162]\n[   1   3   3]\n[   T   T   T]\n[   3   6   9]\n[   3   3    ]\n[   8  10  12]\n")

    def test_spread_and_unpack(self):
        self.check("""program p
implicit none
integer :: v(2), f(2,2)
logical :: m(2,2)
character(len=16) :: line
v = [1, 2]
f = 0
m = reshape([.true., .false., .false., .true.], [2,2])
write (line, '(4I4)') spread(v, 1, 2)
print '(3a)', '[', line, ']'
write (line, '(4I4)') unpack(v, m, f)
print '(3a)', '[', line, ']'
end program p
""", "[   1   1   2   2]\n[   1   0   0   2]\n")


if __name__ == "__main__":
    unittest.main()

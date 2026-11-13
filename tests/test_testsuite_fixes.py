"""Fixes for failures found by running the gfortran test suite: scalar
CHARACTER actuals of run-time length passed to ELEMENTAL functions, which
gave executables with 2 GiB of static storage that Windows would not load
(gfortran.dg/elemental_subroutine_9.f90), and defined operators and
assignments bound to deferred type-bound procedures, which crashed the
compiler (gfortran.dg/typebound_operator_8.f03), and NaNs and infinities in
MAXLOC, MINLOC, MAXVAL and MINVAL (gfortran.dg/maxloc_2.f90, minloc_1.f90,
maxlocval_2.f90, maxlocval_4.f90, minlocval_1.f90 and minlocval_4.f90);
also compiled formatted output of MAXLOC, MINLOC and FINDLOC with DIM=, which
wrote only the first element."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import COMPILER


class TestsuiteFixes(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_runtime_length_actuals_to_elemental_functions(self):
        self.check("""module m
implicit none
contains
elemental logical function same(a, b)
character(len=*), intent(in) :: a, b
same = a == b
end function same
elemental integer function width(a, b)
character(len=*), intent(in) :: a, b
width = len(a) + 10 * len(b)
end function width
subroutine y(str)
character(len=*), intent(in) :: str
character(len=len_trim(str)) :: aux
aux = str
print '(2l2,2(1x,i0))', same(['ab ', 'bbb'], aux), width(['x', 'y'], aux)
end subroutine y
end module m
program p
use m
implicit none
integer :: i
call y('bbb  ')
call y('ab')
do i = 1, 3
   call y(repeat('z', i) // '  ')
end do
end program p
""", " F T 31 31\n T F 21 21\n F F 11 11\n F F 21 21\n F F 31 31\n")

    def test_deferred_binding_of_defined_assignment(self):
        source = """module m
implicit none
type, abstract :: field
contains
   procedure(times), deferred :: scale
   procedure(assign), deferred :: assn
   generic :: operator(*) => scale
   generic :: assignment(=) => assn
end type field
abstract interface
   function times(a, r)
      import :: field
      class(field), intent(in) :: a
      real, intent(in) :: r
      class(field), allocatable :: times
   end function times
   subroutine assign(a, b)
      import :: field
      class(field), intent(out) :: a
      class(field), intent(in) :: b
   end subroutine assign
end interface
end module m
program p
use m
implicit none
class(field), allocatable :: u
u = u
end program p
"""
        with tempfile.TemporaryDirectory(prefix='deferred-binding-') as tmp:
            path = Path(tmp) / 'example.f90'
            path.write_text(source, encoding='utf-8')
            result = subprocess.run([COMPILER, 'check', path], cwd=tmp, capture_output=True, text=True, timeout=60)
            self.assertNotIn('panicked', result.stderr)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_nans_and_infinities_in_locations_and_extrema(self):
        # An element that is not a NaN is preferred; an all-NaN array gives
        # the first location and a NaN value; infinities are values.
        self.check("""module m
implicit none
contains
subroutine s(x, y)
use, intrinsic :: ieee_arithmetic
real(8), intent(in) :: x(:), y(:,:)
print '(a,*(1x,g0))', 'dummy', maxloc(x), minloc(x), ieee_is_nan(maxval(x)), maxloc(y, dim=1), minloc(y, dim=2)
print '(a,*(1x,g0))', 'mask', maxloc(x, mask=x > -1d300), maxloc(x, mask=x /= 7d0)
end subroutine s
end module m
program probe
use m
use, intrinsic :: ieee_arithmetic
implicit none
real :: a(3), nan, minf, z
real, allocatable :: c(:)
real(8) :: dnan, inf, d(3), b(2,2)
real(8), pointer :: p(:)
z = 0.0
nan = z/z
minf = -1.0/z
allocate(c(3))
a = nan; c = nan
print '(a,*(1x,g0))', 'allnan', maxloc(a), minloc(a), maxloc(c), maxloc(a,dim=1), &
   ieee_is_nan(maxval(a)), ieee_is_nan(minval(c)), ieee_is_nan(maxval(a,dim=1))
a = minf; c = minf
print '(a,*(1x,g0))', 'allminf', maxloc(a), maxloc(c), maxval(a) == minf, maxval(c, dim=1) == minf
a(1:2) = nan; c(1:2) = nan
print '(a,*(1x,g0))', 'nan,nan,-inf', maxloc(a), maxloc(c), maxloc(a,dim=1), maxval(a) == minf, maxval(c,dim=1) == minf
a(3) = -minf; c(3) = -minf
print '(a,*(1x,g0))', 'nan,nan,+inf', minloc(a), minloc(c), minval(a) == -minf
a = [nan, 2.0, 1.0]; c = a
print '(a,*(1x,g0))', 'nan,2,1', maxloc(a), minloc(a), maxloc(c), minloc(c), maxval(a), minval(c)
print '(a,*(1x,g0))', 'back', maxloc([nan, 2.0, 2.0], back=.true.), minloc([nan, 2.0, 2.0], back=.true.)
print '(a,*(1x,g0))', 'empty', maxval(a, mask=a > 5) == -huge(a), minloc(a, mask=a > 5)
dnan = ieee_value(dnan, ieee_quiet_nan); inf = ieee_value(inf, ieee_positive_inf)
d = [dnan, -inf, dnan]
b = reshape([dnan, dnan, dnan, 1d0], [2, 2])
call s(d, b)
print '(a,*(1x,g0))', 'matrix', maxloc(b), maxval(b), maxloc(b, dim=1), ieee_is_nan(maxval(b, dim=1))
allocate(p(3)); p = [dnan, 1d0, 5d0]
print '(a,*(1x,g0))', 'pointer', maxloc(p, dim=1), minloc(p, dim=1), maxval(p), minval(p)
p = dnan
print '(a,*(1x,g0))', 'pointer nan', maxloc(p, dim=1), ieee_is_nan(maxval(p)), ieee_is_nan(minval(p))
p = -inf
print '(a,*(1x,g0))', 'pointer -inf', maxval(p) == -inf, minloc(p, dim=1)
end program probe
""", """allnan 1 1 1 1 T T T
allminf 1 1 T T
nan,nan,-inf 3 3 3 T T
nan,nan,+inf 3 3 T
nan,2,1 2 3 2 3 2.00000000 1.00000000
back 3 3
empty T 0
dummy 2 2 F 1 2 1 2
mask 0 2
matrix 2 2 1.0000000000000000 1 2 T F
pointer 3 2 5.0000000000000000 1.0000000000000000
pointer nan 1 T T
pointer -inf T 1
""")

    def test_formatted_output_of_locations_along_a_dimension(self):
        self.check("""program p
implicit none
integer :: b(2,3) = reshape([1, 2, 6, 3, 4, 5], [2, 3])
print '(*(1x,g0))', maxloc(b, dim=1)
print '(*(1x,g0))', minloc(b, dim=2), 7
print '(3i3)', findloc(b, 3, dim=1)
write (*, '(*(1x,g0))') 'x', maxloc(b, dim=2, mask=b < 6), maxloc([4, 9, 9], back=.true.)
end program p
""", """ 2 1 2
 1 1 7
  0  2  0
 x 3 3 3
""")


if __name__ == "__main__":
    unittest.main()

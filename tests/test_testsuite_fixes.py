"""Fixes for failures found by running the gfortran test suite: scalar
CHARACTER actuals of run-time length passed to ELEMENTAL functions, which
gave executables with 2 GiB of static storage that Windows would not load
(gfortran.dg/elemental_subroutine_9.f90), and defined operators and
assignments bound to deferred type-bound procedures, which crashed the
compiler (gfortran.dg/typebound_operator_8.f03), and NaNs and infinities in
MAXLOC, MINLOC, MAXVAL and MINVAL (gfortran.dg/maxloc_2.f90, minloc_1.f90,
maxlocval_2.f90, maxlocval_4.f90, minlocval_1.f90 and minlocval_4.f90);
also compiled formatted output of MAXLOC, MINLOC and FINDLOC with DIM=, which
wrote only the first element; and procedure locals larger than the default
1 MiB Windows stack (gfortran.fortran-torture/execute/stack_varsize.f90,
gfortran.dg/largeequiv_1.f90); and explicit-shape dummies of other extents than
their whole-array actuals, associated with the actual's leading elements in
array element order (gfortran.dg/array_1.f90, dependent_decls_3.f90,
nested_modules_2.f90, nested_modules_3.f90), with -Wsequence-association;
initialized module variables as dummy extents; and list-directed output of
single-precision integers beyond 2**24; and dummies of external procedures
passed on to writable explicit-shape and scalar dummies of module procedures."""
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

    def test_large_procedure_locals(self):
        # The interpreter is too slow for arrays of this size. gfortran
        # -std=f2018 also puts these locals on its default stack and overflows.
        self.check("""module m
implicit none
contains
subroutine big(n)
integer, intent(in) :: n
integer :: a(1500000), k(1024,1024)
a = 3
a(n) = 4
k = 1
k(n, 2) = 5
print '(i0,1x,i0)', sum(a), sum(k)
end subroutine big
recursive subroutine deep(n)
integer, intent(in) :: n
real :: b(40000)
b = n
if (n > 0) call deep(n - 1)
if (b(1) /= n) error stop 1
end subroutine deep
end module m
program p
use m
implicit none
call big(7)
call deep(20)
print '(a)', 'ok'
end program p
""", """4500001 1048580
ok
""", interpreter=False, reference=False)

    def test_sequence_association_with_explicit_shape_dummies(self):
        # Views with run-time extents, a lower bound other than 1, a module
        # variable extent, matching shapes chosen at run time, and a section
        # of a view passed on.
        self.check("""module sizes
implicit none
integer :: m
end module sizes
module ops
use sizes
implicit none
contains
subroutine fill(dst, ni, nj)
  integer, intent(in) :: ni, nj
  real(8), intent(out) :: dst(ni, nj)
  integer :: i, j
  do j = 1, nj
     do i = 1, ni
        dst(i, j) = 10*i + j
     end do
  end do
end subroutine fill
subroutine bump(v, n)
  integer, intent(in) :: n
  integer, intent(inout) :: v(0:n-1)
  v = v + 1
  v(0) = v(0) + 100
end subroutine bump
real(8) function total(a, ni, nj)
  integer, intent(in) :: ni, nj
  real(8), intent(in) :: a(ni, nj)
  total = sum(a)
end function total
subroutine report(a, ni, nj)
  integer, intent(in) :: ni, nj
  real(8), intent(in) :: a(ni, nj)
  print '(2(i0,1x),f0.1,1x,f0.1)', size(a, 1), size(a, 2), sum(a), maxval(a(:, nj))
  call show(a(:, nj), ni)
end subroutine report
subroutine show(v, n)
  integer, intent(in) :: n
  real(8), intent(in) :: v(n)
  print '(*(f0.1,:,1x))', v
end subroutine show
subroutine head(k)
  integer, intent(inout) :: k(m)
  k = -k
end subroutine head
subroutine square(q, n)
  integer, intent(in) :: n
  real(8), intent(inout) :: q(n, n)
  q = 2 * q
end subroutine square
end module ops
program main
use ops
implicit none
real(8) :: x(4,3), y(3,3)
integer :: k(5), i, n
x = 0
call fill(x, 2, 3)
print '(*(f0.1,:,1x))', x
call report(x, 2, 3)
call report(x, 4, 3)
k = [(i, i = 1, 5)]
call bump(k, 3)
print '(*(i0,:,1x))', k
m = 2
call head(k)
print '(*(i0,:,1x))', k
y = 1
do n = 2, 3
   call square(y, n)
   print '(*(f0.1,:,1x))', y
end do
call fill(y, 3, 3)
call report(y, 3, 3)
end program main
""", """11.0 21.0 12.0 22.0 13.0 23.0 .0 .0 .0 .0 .0 .0
2 3 102.0 23.0
13.0 23.0
4 3 102.0 .0
.0 .0 .0 .0
102 3 4 4 5
-102 -3 4 4 5
2.0 2.0 2.0 2.0 1.0 1.0 1.0 1.0 1.0
4.0 4.0 4.0 4.0 2.0 2.0 2.0 2.0 2.0
3 3 198.0 33.0
13.0 23.0 33.0
""")

    def test_sequence_association_diagnostics(self):
        source = """module ops
implicit none
contains
subroutine fill(dst, ni, nj)
  integer, intent(in) :: ni, nj
  real(8), intent(out) :: dst(ni, nj)
  dst = 1
end subroutine fill
subroutine take(v)
  real(8), intent(inout) :: v(8)
  v = 0
end subroutine take
end module ops
program main
use ops
implicit none
real(8) :: x(4,3), z(4,3), u(6)
integer :: n
n = 2
call fill(x, 2, 3)
call fill(z, 4, 3)
call fill(z, n, 3)
call take(u)
end program main
"""
        with tempfile.TemporaryDirectory(prefix='sequence-association-') as tmp:
            path = Path(tmp) / 'example.f90'
            path.write_text(source, encoding='utf-8')
            result = subprocess.run([COMPILER, '-Wextra', 'check', path], cwd=tmp, capture_output=True, text=True, timeout=60)
            self.assertIn('explicit-shape dummy `dst` of `fill` has shape [2, 3], but this call passes an array of shape [4, 3] [sequence-association]', result.stderr)
            self.assertEqual(result.stderr.count('[sequence-association]'), 2, result.stderr)
            self.assertIn('explicit-shape dummy `v` of `take` has 8 elements, more than the 6 of its actual argument `u`', result.stderr)
            self.assertNotEqual(result.returncode, 0)
            # Without the oversized call there is no error, and no warning by default.
            path.write_text(source.replace('call take(u)', ''), encoding='utf-8')
            result = subprocess.run([COMPILER, 'check', path], cwd=tmp, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn('sequence-association', result.stderr)

    def test_module_variable_dummy_extents(self):
        # An initialized module variable as a dummy's extent, in a caller
        # that does not otherwise use it and has a variable of the same name.
        self.check("""module sizes
implicit none
integer :: m = 2
integer, parameter :: np = 3
end module sizes
module ops
use sizes
implicit none
contains
subroutine head(k)
  integer, intent(inout) :: k(m)
  k = -k
end subroutine head
subroutine tail(k, n)
  integer, intent(in) :: n
  integer, intent(inout) :: k(n + m - np)
  k = 10 * k
end subroutine tail
end module ops
program main
use ops, only: head, tail
implicit none
integer :: k(5), m
m = 4
k = 1
call head(k)
print '(*(i0,:,1x))', k
call tail(k, 3)
print '(*(i0,:,1x))', k, m
end program main
""", "-1 -1 1 1 1" + chr(10) + "-10 -10 1 1 1 4" + chr(10))

    def test_large_single_precision_integers_in_list_output(self):
        # A REAL beyond 2**24 has digits that single precision does not
        # determine: 2.5e10 rounds to 24999999488. rfort writes it with 6
        # significant digits, unlike gfortran's 9.
        self.check("""program p
real :: x, y
double precision :: d
x = 16777216.0
y = 2.5e10
d = 2.5d10
print *, x, y, -y, 2.5e10, 3 * y, d
end program p
""", " 16777216.0 2.5e10 -2.5e10 2.5e10 7.5e10 25000000000.0" + chr(10), reference=False)

    def test_forwarding_dummies_of_external_procedures(self):
        # The dummies of an external procedure, received by reference, passed
        # on to module procedures: writable explicit-shape arrays, including
        # one with lower bound 0 and a strided POINTER, and scalars.
        self.check("""module m
implicit none
contains
  subroutine scale(a, n, s)
    integer, intent(in) :: n
    double precision, intent(inout) :: a(n)
    double precision, intent(inout) :: s
    a = a * 2
    s = s + sum(a)
  end subroutine scale
  subroutine setm(b, ni, nj)
    integer, intent(in) :: ni, nj
    double precision, intent(out) :: b(0:ni-1, nj)
    integer :: i, j
    do j = 1, nj
      do i = 0, ni - 1
        b(i, j) = 10*i + j
      end do
    end do
  end subroutine setm
  subroutine count_up(k, n)
    integer, intent(in) :: n
    integer, intent(inout) :: k
    k = k + n
  end subroutine count_up
end module m
subroutine outer(a, n, b, ni, nj, s, k)
  use m
  integer, intent(in) :: n, ni, nj
  double precision, intent(inout) :: a(n), s
  double precision, intent(out) :: b(ni, nj)
  integer, intent(inout) :: k
  call scale(a, n, s)
  call setm(b, ni, nj)
  call count_up(k, n)
end subroutine outer
program p
  use m
  implicit none
  double precision, target :: x(4) = 1, y(2,3), w(10)
  double precision, pointer :: q(:)
  double precision :: s = 0
  integer :: k = 5
  call outer(x, 4, y, 2, 3, s, k)
  print '(*(f0.1,:,1x))', x, s
  print '(*(f0.1,:,1x))', y
  print '(i0)', k
  w = 1
  q => w(1:10:3)
  call scale(q, 4, s)
  print '(*(f0.1,:,1x))', w, s
end program p
""", """2.0 2.0 2.0 2.0 8.0
1.0 11.0 2.0 12.0 3.0 13.0
9
2.0 1.0 1.0 2.0 1.0 1.0 2.0 1.0 1.0 2.0 16.0
""", reference=False)  # gfortran's output; it warns of an array temporary for q
        self.check("""module m
contains
  subroutine count_up(k, n)
    integer, intent(in) :: n
    integer, intent(inout) :: k
    k = k + n
  end subroutine count_up
end module m
subroutine outer(a, n, k)
  use m
  integer, intent(in) :: n
  double precision, intent(inout) :: a(n)
  integer, intent(inout) :: k
  call count_up(k, n)
  a = k
end subroutine outer
program p
  integer :: k = 5
  double precision :: x(3) = 0
  call outer(x, 3, k)
  print '(i0,1x,f0.1)', k, x(1)
end program p
""", "8 8.0" + chr(10))
        self.check("""module m
contains
  subroutine set_out(k, v)
    integer, intent(out) :: k
    integer, intent(in) :: v
    k = v
  end subroutine set_out
  subroutine twice(k)
    integer :: k
    k = 2 * k
  end subroutine twice
  subroutine first(a, m)
    integer, intent(in) :: m
    real, intent(inout) :: a(m)
    a = 0
  end subroutine first
end module m
subroutine outer(a, n, k, j)
  use m
  integer, intent(in) :: n
  real, intent(inout) :: a(n)
  integer, intent(out) :: k
  integer, intent(inout) :: j
  call set_out(k, n)
  call twice(j)
  call first(a, n)
end subroutine outer
program p
  integer :: k, j = 3
  real :: x(4) = 1
  call outer(x, 4, k, j)
  print '(2(i0,1x),*(f0.1,:,1x))', k, j, x
end program p
""", "4 6 .0 .0 .0 .0" + chr(10))

    def test_forwarded_pointer_view_of_another_shape(self):
        # Sequence association through a POINTER actual is not yet supported.
        self.check("""module m
contains
  subroutine set_out(k, v)
    integer, intent(out) :: k
    integer, intent(in) :: v
    k = v
  end subroutine set_out
  subroutine twice(k)
    integer :: k
    k = 2 * k
  end subroutine twice
  subroutine first(a, m)
    integer, intent(in) :: m
    real, intent(inout) :: a(m)
    a = 0
  end subroutine first
end module m
subroutine outer(a, n, k, j)
  use m
  integer, intent(in) :: n
  real, intent(inout) :: a(n)
  integer, intent(out) :: k
  integer, intent(inout) :: j
  call set_out(k, n)
  call twice(j)
  call first(a, n - 1)
end subroutine outer
program p
  integer :: k, j = 3
  real :: x(4) = 1
  call outer(x, 4, k, j)
  print *, k, j, x
end program p
""", error="explicit-shape dummy `a` of `first` requires its POINTER actual argument to have the same shape", reference=False)

    def test_allocatable_element_checks_without_repeated_offsets(self):
        # Element offsets of ALLOCATABLE arrays are checked once per access;
        # out-of-bounds subscripts and undefined elements are still reported.
        self.check("""program p
real(8), allocatable :: a(:)
integer :: i
allocate(a(3))
a(1) = 1
i = 5
print *, a(i)
end program p
""", error="subscript outside allocated bounds", reference=False)
        self.check("""program p
real(8), allocatable :: a(:)
integer :: i
allocate(a(3))
a(1) = 1
i = 2
print *, a(i)
end program p
""", error="read of uninitialized variable `a`: element 2", reference=False)
        self.check("""program p
real(8), allocatable :: a(:,:)
integer :: i, j
allocate(a(3,2))
a = 1
do j = 1, 2
   do i = 1, 3
      a(i, j) = a(i, j) + i * j
   end do
end do
print '(*(f0.1,:,1x))', a
end program p
""", "2.0 3.0 4.0 3.0 5.0 7.0" + chr(10))


if __name__ == "__main__":
    unittest.main()

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
passed on to writable explicit-shape and scalar dummies of module procedures;
and fewer run-time checks of ALLOCATABLE elements."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import COMPILER


class TestsuiteFixes(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

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

    def test_allocatable_loops_without_checks(self):
        # Element offsets that skip their second check also compile with
        # -nocheck and -fast, which drop the checks altogether.
        import test_nocheck
        text = """program p
real(8), allocatable :: a(:)
integer :: i
allocate(a(4))
a(1) = 1
do i = 2, 4
   a(i) = a(i - 1) * 2
end do
print '(*(f0.1,:,1x))', a
end program p
"""
        for flags in ([], ["-nocheck"], ["-fast"], ["-O0", "-nocheck"]):
            with self.subTest(flags=flags):
                result = test_nocheck.build_and_run(text, *flags)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.replace(chr(13), ""), "1.0 2.0 4.0 8.0" + chr(10))

    def test_expressions_for_dummies_without_intent(self):
        # Array constructors and expressions passed to array dummies without
        # INTENT that the procedure does not define, functions with such
        # dummies, and formatted output of locations in array constructors.
        self.check("""module m
implicit none
contains
subroutine show(v, n)
  integer :: n
  integer :: v(n)
  print '(*(i0,:,1x))', v, sum(v)
end subroutine show
subroutine total(v, s)
  real :: v(:), s
  s = sum(v)
end subroutine total
function twice(x) result(r)
  integer :: x(:)
  integer :: r(size(x))
  r = 2 * x
end function twice
character(len=5) function label(k)
  integer :: k
  write (label, '(a,i0)') 'k=', k
end function label
end module m
program p
use m
implicit none
integer :: i
real :: s, z(3) = [1.0, 2.0, 3.0]
call show([3, 4, 5], 3)
call show([(i * i, i = 1, 4)], 4)
call show(reshape([1, 2, 3, 4], [4]), 4)
call total(-z, s)
print '(f0.1)', s
call total(z * 2, s)
print '(f0.1)', s
print '(*(i0,:,1x))', twice([1, 2, 3])
print '(a)', label(7)
print '(*(1x,g0))', maxloc([1, 3, 2], dim=1), findloc([1, 3, 2], 3, dim=1), 9
print '(*(1x,g0))', maxloc([1, 3, 2]), minloc(reshape([4, 1, 3, 2], [2, 2]))
end program p
""", """3 4 5 12
1 4 9 16 30
1 2 3 4 10
-6.0
12.0
2 4 6
k=7  
 2 2 9
 2 2 1
""")

    def test_parenthesized_actuals_and_component_implied_dos(self):
        # A parenthesized variable passed as an actual argument is a copy;
        # implied-DOs over ALLOCATABLE components of array elements read the
        # element of each iteration.
        self.check("""module m
implicit none
type t
   real, allocatable :: x(:)
   integer :: k = 4
end type t
contains
subroutine bump(a, b)
  integer :: a, b
  b = b + 10
  a = a + 1
end subroutine bump
end module m
program p
use m
implicit none
integer :: i, j, tryme
type(t) :: v(3)
i = 3
j = tryme((i), i)
print '(2(i0,1x))', j, i
i = 3
call bump((i), i)
print '(i0)', i
do i = 1, 3
   allocate(v(i)%x(i)); v(i)%x = i
end do
print '(*(g0,1x))', (size(v(i)%x), i = 1, 3), (allocated(v(i)%x), i = 1, 3)
print '(3(f0.1,1x),3(i0,1x))', (sum(v(i)%x), i = 1, 3), (v(i)%k, i = 1, 3)
end program p
integer function tryme(rtnme, hitme)
integer rtnme, hitme
hitme = 999
tryme = rtnme
end
""", """3 999
13
1 2 3 T T T
1.0 4.0 9.0 4 4 4
""")

    def test_arrays_of_types_without_components(self):
        # Fixed-size and ALLOCATABLE arrays of a type without components, and
        # polymorphic arrays whose declared type is such an extension base.
        self.check("""module m
implicit none
type :: base
end type base
type, extends(base) :: child
   integer :: k = 3
end type child
end module m
program p
use m
implicit none
type(base), allocatable :: a(:)
type(base) :: b(4)
class(base), allocatable :: c(:)
allocate(a(3))
print '(*(i0,:,1x))', size(a), size(b), shape(b)
allocate(child :: c(2))
select type (c)
type is (child)
   print '(*(i0,:,1x))', size(c), c%k
end select
deallocate(a)
print '(l1)', allocated(a)
end program p
""", "3 4 4" + chr(10) + "2 3 3" + chr(10) + "F" + chr(10))

    def test_pointer_components_of_record_arrays(self):
        # POINTER components of elements of arrays of records: ALLOCATE,
        # assignment through them, pointer assignment to sections of other
        # elements' components, NULLIFY, ASSOCIATED, default null
        # initialization, whole-record copies and DEALLOCATE.
        self.check("""program p
implicit none
type foo
   integer, dimension(:), pointer :: array
   real, pointer :: s => null()
   integer :: tag = 7
end type foo
type(foo), allocatable :: mol(:)
type(foo) :: two(2)
real, target :: x = 2.5
integer :: i
allocate(mol(3))
do i = 1, 3
   allocate(mol(i)%array(i + 1))
   mol(i)%array = i * 10
end do
print '(*(i0,:,1x))', (size(mol(i)%array), i = 1, 3), mol(3)%array, mol(2)%tag
print '(*(l1,:,1x))', associated(mol(1)%s), associated(mol(2)%array)
mol(1)%s => x
mol(1)%s = mol(1)%s * 2
print '(f0.1,1x,f0.1)', mol(1)%s, x
two(1)%array => mol(3)%array(2:3)
two(1)%array(1) = 99
print '(*(i0,:,1x))', mol(3)%array
nullify(mol(2)%array)
print '(l1)', associated(mol(2)%array)
two = mol(2:3)
print '(*(i0,:,1x))', two(2)%array
deallocate(mol(1)%array)
end program p
""", """2 3 4 30 30 30 30 7
F T
5.0 5.0
30 99 30 30
F
30 99 30 30
""")
        self.check("""program p
type element
   integer, pointer, dimension(:)::p
end type
type (element) :: q(5), r(5)
integer, target, dimension(25)::t
n = 5
do i = 1,5
   r(i)%p => t((i-1)*n + 1:i*n)
enddo
do i = 1, 25
   t(i) = i
enddo
do i = 2, 5
   q(i)%p => r(i-1)%p
end do
print '(*(i0,:,1x))', r(2)%p, q(3)%p
end
""", "6 7 8 9 10 6 7 8 9 10" + chr(10))

    def test_derived_component_defaults_in_record_arrays(self):
        # Default values of derived components, nested and with keyword
        # constructor arguments, in fixed-size and ALLOCATABLE arrays of records.
        self.check("""module m
implicit none
type a
   integer :: i = 1
   real :: x = 0.5
end type a
type b
   type(a) :: acomp = a(5, 2.5)
   type(a) :: plain
   integer :: n = 3
end type b
type c
   type(b) :: inner = b(a(x=7.5, i=8), a(), 9)
   logical :: f = .true.
end type c
end module m
program p
use m
implicit none
type(b) :: fixed(2), one
type(b), allocatable :: grown(:)
type(c) :: deep(2)
allocate(grown(3))
print '(i0,1x,f0.1,1x,i0,1x,f0.1,1x,i0)', fixed(2)%acomp%i, fixed(2)%acomp%x, fixed(1)%plain%i, fixed(1)%plain%x, fixed(1)%n
print '(i0,1x,f0.1,1x,i0)', grown(3)%acomp%i, grown(3)%acomp%x, grown(2)%n
print '(i0,1x,f0.1,1x,i0,1x,f0.1,1x,i0,1x,l1)', deep(1)%inner%acomp%i, deep(1)%inner%acomp%x, deep(2)%inner%plain%i, &
   deep(2)%inner%plain%x, deep(2)%inner%n, deep(1)%f
print '(i0,1x,f0.1)', one%acomp%i, one%acomp%x
grown(1)%acomp%i = 40
print '(*(i0,:,1x))', grown%acomp%i
end program p
""", """5 2.5 1 .5 3
5 2.5 3
8 7.5 1 .5 9 T
5 2.5
40 5 5
""")

    def test_forall_pointer_assignment_of_components(self):
        # FORALL pointer assignment of POINTER components of arrays of records,
        # including targets taken from the array being assigned. All targets
        # are determined before any pointer is assigned (Fortran 2008
        # 7.2.4.2.4), so r(5) gets the original r(1); gfortran assigns in
        # order and gives 21 22 instead of 1 2.
        self.check("""program p
implicit none
type element
   integer, pointer :: p(:)
end type
type(element) :: q(5), r(5)
integer, target :: t(25)
integer :: i, n
n = 5
do i = 1, 25
   t(i) = i
end do
do i = 1, 5
   r(i)%p => t((i-1)*n + 1:i*n)
end do
forall (i = 2:5)
   q(i)%p => r(i-1)%p
end forall
print '(*(i0,:,1x))', q(2)%p, q(5)%p
forall (i = 1:5) r(i)%p => r(6 - i)%p
print '(*(i0,:,1x))', r(1)%p, r(5)%p(1:2)
end program p
""", """1 2 3 4 5 16 17 18 19 20
21 22 23 24 25 1 2
""", reference=False)

    def test_open_without_file_and_conditional_reads(self):
        # OPEN without FILE connects fort.N, for constant and variable units;
        # a read of an undefined variable in a branch that never runs is not
        # an error; GOTO-based control flow defines a function result; GETARG
        # (a GNU extension, so not checked against gfortran -std=f2018).
        self.check("""program p
implicit none
integer :: k, n, u, a(3), b(2), never
character(len=20) :: line
logical, external :: zero
open(10)
write(10, '(a)') 'hello unit ten'
rewind(10)
read(10, '(a)') line
close(10, status='delete')
print '(a)', trim(line)
u = 11
open(u)
write(u, *) 42
rewind(u)
read(u, *) n
close(u, status='delete')
print '(i0)', n
b = [1, 2]
do k = 1, 3
   if (k > 5) then
      a(k) = never   ! never runs, so never is not read
   else
      a(k) = k
   end if
end do
print '(*(i0,:,1x))', a
print '(l1)', zero()
call getarg(0, line)
print '(l1)', len_trim(line) > 0
end program p
logical function zero() result(r)
   goto 2
1  return
2  r = .false.
   if (.not. r) goto 1
end function zero
""", "hello unit ten" + chr(10) + "42" + chr(10) + "1 2 3" + chr(10) + "F" + chr(10) + "T" + chr(10), reference=False)

    def test_reads_that_may_not_execute(self):
        # A dummy that an ENTRY lacks, read only under a guard, and a never
        # assigned array read only through a zero-sized section of runtime
        # bounds are checked at run time (ENTRY is obsolescent, so this is
        # not checked against gfortran -std=f2018).
        self.check("""module m
contains
subroutine foo(a, b)
  integer :: a, b
  logical, save :: was_foo = .false.
  was_foo = .true.
  print '(i0,1x,i0)', a, b
entry bar(a)
  if (was_foo) then
    print '(a,i0)', 'foo ', a + b
  else
    print '(a,i0)', 'bar ', a
  end if
  was_foo = .false.
end subroutine
subroutine zero_sized(n1, n2)
  integer :: n1, n2
  real :: c(3,2), x(3,5), y(5,2)
  c = matmul(x(:,n1:n2), y(n1:n2,:))
  print '(6f4.1)', c
end subroutine
end module
program p
use m
call foo(3, 4)
call bar(5)
call zero_sized(9, 8)
end program
""", "3 4\nfoo 7\nbar 5\n 0.0 0.0 0.0 0.0 0.0 0.0\n", reference=False)

    def test_unconditional_read_after_runtime_branch_is_rejected(self):
        # Only reads under a runtime condition are deferred to run time.
        self.reject("""program p
implicit none
integer :: n, x, y
read *, n
if (n > 0) y = 1
print *, x, y
end program
""", "uninitialized variable `x`")

    def test_pass_argument_procedure_statements_and_character_selectors(self):
        # PASS(arg) of the first dummy; PROCEDURE statements in a generic
        # interface naming internal procedures; a component named DO;
        # CHARACTER selectors combining a length and KIND=; ALLOCATE with
        # CHARACTER(n) and CHARACTER(LEN=n, KIND=k).
        self.check("""module m
implicit none
type :: counter
   integer :: n = 0
   integer :: do = 5
contains
   procedure, pass(self) :: bump
   procedure, pass(self), public :: total
end type
contains
subroutine bump(self, by)
   class(counter), intent(inout) :: self
   integer, intent(in) :: by
   self%n = self%n + by
end subroutine
integer function total(self)
   class(counter), intent(in) :: self
   total = self%n + self%do
end function
end module
program p
use m
implicit none
interface show
   procedure show_int
   procedure :: show_text
end interface
type(counter) :: c
character(len=:, kind=1), allocatable :: a
character(:, kind=selected_char_kind('ascii')), allocatable :: b
character(kind=1, len=:), allocatable :: d(:)
call c%bump(3)
c%do = c%do + 1
call show(c%total())
allocate(character(3) :: a)
a = 'xyz'
allocate(character(len=2, kind=1) :: d(2))
d = ['ab', 'cd']
b = a // d(2)
call show(b)
call show(len(b))
contains
subroutine show_int(i)
   integer, intent(in) :: i
   print '(i0)', i
end subroutine
subroutine show_text(s)
   character(*, kind=1), intent(in) :: s
   print '(a)', s
end subroutine
end program
""", "9\nxyzcd\n5\n")

    def test_unsupported_pass_argument_and_character_kind_are_rejected(self):
        self.reject("""module m
type :: t
   integer :: n = 0
contains
   procedure, pass(y) :: f
end type
contains
subroutine f(x, y)
   integer, intent(in) :: x
   class(t), intent(inout) :: y
   y%n = x
end subroutine
end module
program p
use m
type(t) :: a
call a%f(1)
end program
""", "not the first dummy argument")
        self.reject("""program p
character(len=3, kind=4) :: s
s = 'abc'
print *, s
end program
""", "CHARACTER kinds other than the default")

    def test_nopass_bindings_and_unreferenced_ambiguous_imports(self):
        self.check("""module shapes
implicit none
type :: shape
   integer :: sides = 4
contains
   procedure, nopass :: describe
   procedure, nopass :: twice_real
   procedure :: twice_obj
   generic :: twice => twice_real, twice_obj
   procedure :: get
end type
contains
subroutine describe(name, n)
   character(len=*), intent(in) :: name
   integer, intent(in) :: n
   print '(a,1x,i0)', name, n
end subroutine
real function twice_real(x)
   real, intent(in) :: x
   twice_real = 2*x
end function
integer function twice_obj(self, i)
   class(shape), intent(in) :: self
   integer, intent(in) :: i
   twice_obj = self%sides*i
end function
integer function get(self)
   class(shape), intent(in) :: self
   get = self%sides
end function
end module
module first
contains
subroutine set(i)
   integer, intent(out) :: i
   i = 1
end subroutine
end module
module second
contains
subroutine set(i)
   integer, intent(out) :: i
   i = 2
end subroutine
end module
program p
use shapes
use first
use second
type(shape) :: s
integer :: j
call s%describe('square', s%get())
print '(f0.1)', s%twice_real(5.0)
print '(i0)', s%twice(7)
print '(f0.1)', s%twice(1.5)
j = 0
print '(i0)', j
end program
""", "square 4\n10.0\n28\n3.0\n0\n")
        self.reject("""module first
contains
subroutine set(i)
   integer, intent(out) :: i
   i = 1
end subroutine
end module
module second
contains
subroutine set(i)
   integer, intent(out) :: i
   i = 2
end subroutine
end module
program p
use first
use second
integer :: j
call set(j)
print *, j
end program
""", "ambiguous imported name `set`")


    def test_namelist_derived_members_module_groups_and_end(self):
        self.check("""module config
implicit none
type :: inner
   integer :: j = 0
end type
type :: tracer
   character(len=4) :: sname = 'none'
   logical :: lini = .false.
end type
type :: setup
   type(inner) :: d(2)
   real :: w(2) = 0.0
   character(len=8) :: name = 'none'
end type
integer :: steps = 1
type(setup) :: der
namelist /run/ steps, der
end module
program p
use config
implicit none
type :: rec
   integer :: a = 0
   real :: b(2) = 0.0
   type(inner) :: k
end type
type(rec) :: x(3)
type(tracer) :: tr(3)
namelist /g/ x, tr
open(10, status='scratch')
write(10, '(a)') '&run steps = 4, der%d(2)%j = 9, der%w = 1.5, 2.5, der%name = "abc" /'
write(10, '(a)') '&g x(2)%a = 5, x(3)%b = 1.5, 2.5, x(1)%k%j = 7, x%a = 1, 2,'
write(10, '(a)') ' tr(2) = "bb", T, tr(3)%sname = "cc"'
write(10, '(a)') '&end'
rewind(10)
read(10, nml=run)
read(10, nml=g)
close(10)
print '(i0,1x,2(i0,1x),2(f0.1,1x),a)', steps, der%d%j, der%w, trim(der%name)
print '(3(i0,1x),2(f0.1,1x),i0)', x%a, x(3)%b, x(1)%k%j
print '(3(a,1x,l1,1x))', tr(1)%sname, tr(1)%lini, tr(2)%sname, tr(2)%lini, tr(3)%sname, tr(3)%lini
open(11, status='scratch')
write(11, nml=g)
x(3)%b(2) = -1.0
tr(2)%lini = .false.
rewind(11)
read(11, nml=g)
close(11)
print '(f0.1,1x,l1)', x(3)%b(2), tr(2)%lini
end program
""", "4 0 9 1.5 2.5 abc\n1 2 0 1.5 2.5 7\nnone F bb   T cc   F\n2.5 T\n", interpreter=False)
        self.reject("""program p
type :: in
   integer :: j
end type
type :: t
   type(in) :: m(2)
end type
type(t) :: x(2)
namelist /g/ x
read(*, nml=g)
end program
""", "array of derived type within an array of derived type")

    def test_namelist_renamed_group_and_internal_array_files(self):
        # A group renamed by USE ONLY keeps its name and its module members,
        # which the scope does not import; internal WRITE and READ with a
        # CHARACTER array, one item with its values per element.
        self.check("""module m
implicit none
real :: x = -1
integer :: n(3) = [1, 2, 3]
namelist /foo/ x, n
end module
program p
use m, only: bar => foo
implicit none
real :: x = 42
character(len=40) :: lines(8)
character(len=3000) :: buf
buf = "&foo x=7.5 n(2:3)=8 9 /"
read(buf, nml=bar)
call show
lines = '-'
write(lines, nml=bar)
buf = "&foo x=0 n=0 0 0 /"
read(buf, nml=bar)
read(lines, nml=bar)
call show
print '(a,f5.1,a)', 'local ', x, ' ' // trim(lines(8))
contains
subroutine show
use m, only: x, n
print '(f5.1,3i3)', x, n
end subroutine
end program
""", '  7.5  1  8  9\n  7.5  1  8  9\nlocal  42.0 -\n')

    def test_namelist_input_sections_and_internal_scalar_files(self):
        self.check("""module shapes
implicit none
type :: t
   integer :: i = 0
   real :: r = 0
end type
end module
program p
use shapes
implicit none
integer :: a(6) = 0, b(3,4) = 0, c(-2:2) = 0
type(t) :: e(4)
character(len=40) :: input(4)
character(len=1000) :: buf
namelist /g/ a, b, c, e
namelist /h/ a, c
input(1) = "&g a(2:6:2) = 1, 2, 3, a(:2) = 7 8"
input(2) = " b(2,:) = 1 2 3 4, b(1:3:2, 4) = 5, 6"
input(3) = " c(2:-2:-1) = 1 2 3 4 5"
input(4) = " e(2:3) = 5 1.5 6 2.5 /"
read(input, nml=g)
print '(12i2)', b
print '(4(i2,f4.1))', e
write(buf, nml=h)
a = 0; c = 0
read(buf, nml=h)
print '(6i2)', a
print '(5i2)', c
end program
""", ' 0 1 0 0 2 0 0 3 0 5 4 6\n 0 0.0 5 1.5 6 2.5 0 0.0\n 7 8 0 2 0 3\n 5 4 3 2 1\n')

    def test_internal_procedure_use_hides_host_variable(self):
        self.check("""module m
implicit none
real :: x = -1
end module
program p
implicit none
real :: x = 42
call show
print *, x
contains
subroutine show
use m, only: x
print *, x
end subroutine
end program
""", "   -1.00000000    \n   42.0000000    \n")

    def test_designators_with_several_subscripted_parts(self):
        # y(1)%at(j)%i(k): components of elements of array components of
        # elements (gfortran.dg/alloc_comp_assign_3.f90, derived_init_1.f90).
        self.check("""program p
  implicit none
  type a
    integer :: i(4)
    character(len=3) :: s = 'abc'
  end type
  type b
    type(a) :: at(2)
    real :: w = 1.5
  end type
  type c
    type(b) :: bs(3)
  end type
  type(b) :: y(2)
  type(c) :: z(2)
  integer :: j, k
  do j = 1, 2
    do k = 1, 4
      y(1)%at(j)%i(k) = 10*j + k
    end do
  end do
  y(2) = y(1)
  y(2)%at(2)%i(3) = 99
  y(2)%at(1)%s = 'xyz'
  print '(2i4,1x,a,1x,a)', y(2)%at(2)%i(3), y(2)%at(1)%i(2), y(2)%at(1)%s, y(1)%at(1)%s
  print '(4i4)', y(2)%at(2)%i
  print '(3i4)', sum(y(2)%at(2)%i), y(1)%at(2)%i(2:3)
  y(1)%at(2)%i = 7
  y(1)%at(1)%i(2:3) = [5, 6]
  print '(4i4)', y(1)%at(1)%i, y(1)%at(2)%i
  z(2)%bs(3)%at(2)%i(4) = 5
  z(2)%bs(3)%at(1)%i = [1, 2, 3, 4]
  print '(5i4,f5.1)', z(2)%bs(3)%at(2)%i(4), z(2)%bs(3)%at(1)%i, z(2)%bs(2)%w
  print '(2i4)', y(2)%at(:)%i(4)
  if (any(y(2)%at(:)%i(1) /= [11, 21])) stop 1
end program
""", '  99  12 xyz abc\n  21  22  99  24\n 166  22  23\n  11   5   6  14\n   7   7   7   7\n   5   1   2   3   4  1.5\n  14  24\n')

    def test_data_type_parameter_inquiry_and_implicit_objects(self):
        # gfortran.dg/pr87994_1.f90, pr87994_2.f90
        self.check("""program p
  real, parameter :: a = 1.0
  integer :: n
  data b /a%kind/, n /a%kind/
  print '(f4.1,i3)', b, n
end
""", ' 4.0  4\n')

    def test_generic_and_volatile_statements(self):
        # GENERIC statements for names, operators and assignment, with and
        # without an access specification, and VOLATILE and ASYNCHRONOUS
        # statements (gfortran.dg/generic_stmt_4.f90, volatile.f90).
        self.check("""module m
  implicit none
  private
  type, public :: v
    integer :: n = 0
  end type
  generic, public :: twice => twice_i, twice_r
  generic :: twice => twice_v
  generic :: operator(.plus.) => plus_v
  generic :: operator(+) => plus_v
  generic :: assignment(=) => from_int
  public :: operator(+), operator(.plus.), assignment(=)
  integer, public, volatile :: counter = 0
  asynchronous :: counter
contains
  integer function twice_i(x)
    integer, intent(in) :: x
    twice_i = 2*x
  end function
  real function twice_r(x)
    real, intent(in) :: x
    twice_r = 2*x
  end function
  type(v) function twice_v(x)
    type(v), intent(in) :: x
    twice_v%n = 2*x%n
  end function
  type(v) function plus_v(a, b)
    type(v), intent(in) :: a, b
    plus_v%n = a%n + b%n
  end function
  subroutine from_int(a, i)
    type(v), intent(out) :: a
    integer, intent(in) :: i
    a%n = i
    counter = counter + 1
  end subroutine
end module
program p
  use m
  implicit none
  type(v) :: a, b, c, d, e
  volatile :: a
  a = 3
  b = 4
  c = twice(a)
  d = a .plus. b
  e = a + b
  print '(i0,1x,f4.1,3(1x,i0))', twice(5), twice(1.5), c%n, d%n, e%n
  print '(i0)', counter
end program
""", '10  3.0 6 7 7\n2\n')
        self.reject("""module m
private
generic :: g => f
contains
integer function f(x)
integer :: x
f=x
end function
end module
program p
use m
print *, g(1)
end
""", "not accessible")

    def test_procedure_pointers_with_typed_implicit_interfaces(self):
        # PROCEDURE(INTEGER) and PROCEDURE(REAL) pointers declared in
        # internal procedures (gfortran.dg/proc_ptr_5.f90).
        self.check("""program main
  implicit none
  call test(.true.)
  call test(.false.)
  call reals
contains
  integer function hello()
    hello = 42
  end function
  real function half(x)
    real, intent(in) :: x
    half = x / 2
  end function
  real function third(x)
    real, intent(in) :: x
    third = x / 3
  end function
  subroutine test(first)
    logical, intent(in) :: first
    procedure(integer), pointer :: x => null()
    if (first) then
      if (associated(x)) stop 1
      x => hello
    else
      if (.not. associated(x)) stop 2
      print '(i0)', x()
    end if
  end subroutine
  subroutine reals
    procedure(real), pointer :: f
    f => half
    print '(f6.3)', f(3.0)
    f => third
    print '(f6.3)', f(3.0)
  end subroutine
end program
""", '42\n 1.500\n 1.000\n')
        # Calls dispatch through one interface: targets with different
        # interfaces used to receive the wrong arguments.
        self.reject("""program main
procedure(), pointer :: ix => null()
ix => hello
print *, ix()
ix => twice
print *, ix(4)
contains
integer function hello()
 hello = 42
end function hello
integer function twice(n)
integer :: n
twice = 2*n
end function
end program
""", "different interfaces")

    def test_defined_io_interface_blocks(self):
        # INTERFACE WRITE(FORMATTED) and READ/WRITE(UNFORMATTED) blocks, with
        # TYPE dummies of a SEQUENCE type and CLASS dummies of an extensible
        # type, on an unformatted scratch file; the procedures leave IOSTAT
        # undefined, which gfortran treats as success (gfortran.dg/dtio_8.f90,
        # dtio_9.f90, dtio_22.f90).
        self.check("""module m
  implicit none
  type :: pt
    sequence
    integer :: x = 0, y = 0
  end type
  interface write(formatted)
    module procedure wf
  end interface
  interface write(unformatted)
    module procedure wu
  end interface
  interface read(unformatted)
    module procedure ru
  end interface
contains
  subroutine wf(dtv, unit, iotype, vlist, iostat, iomsg)
    type(pt), intent(in) :: dtv
    integer, intent(in) :: unit
    character(*), intent(in) :: iotype
    integer, intent(in) :: vlist(:)
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    write(unit, '(a,i0,a,i0,a)', iostat=iostat, iomsg=iomsg) '(', dtv%x, ',', dtv%y, ')'
  end subroutine
  subroutine wu(dtv, unit, iostat, iomsg)
    type(pt), intent(in) :: dtv
    integer, intent(in) :: unit
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    write(unit) dtv%y, dtv%x
  end subroutine
  subroutine ru(dtv, unit, iostat, iomsg)
    type(pt), intent(inout) :: dtv
    integer, intent(in) :: unit
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    read(unit) dtv%x, dtv%y
  end subroutine
end module
program p
  use m
  implicit none
  type(pt) :: a, b
  a = pt(3, 4)
  write(*, '(dt)') a
  open(10, status='scratch', form='unformatted')
  write(10) a
  rewind(10)
  read(10) b
  close(10)
  write(*, '(dt)') b
end program
""", '(3,4)\n(4,3)\n')
        self.check("""module m
  implicit none
  type :: pt
    integer :: x = 0, y = 0
  end type
  interface write(formatted)
    module procedure wf
  end interface
  interface write(unformatted)
    module procedure wu
  end interface
  interface read(unformatted)
    module procedure ru
  end interface
contains
  subroutine wf(dtv, unit, iotype, vlist, iostat, iomsg)
    class(pt), intent(in) :: dtv
    integer, intent(in) :: unit
    character(*), intent(in) :: iotype
    integer, intent(in) :: vlist(:)
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    write(unit, '(a,i0,a,i0,a)', iostat=iostat, iomsg=iomsg) '(', dtv%x, ',', dtv%y, ')'
  end subroutine
  subroutine wu(dtv, unit, iostat, iomsg)
    class(pt), intent(in) :: dtv
    integer, intent(in) :: unit
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    write(unit) dtv%y, dtv%x
  end subroutine
  subroutine ru(dtv, unit, iostat, iomsg)
    class(pt), intent(inout) :: dtv
    integer, intent(in) :: unit
    integer, intent(out) :: iostat
    character(*), intent(inout) :: iomsg
    read(unit) dtv%x, dtv%y
  end subroutine
end module
program p
  use m
  implicit none
  type(pt) :: a, b
  a = pt(3, 4)
  write(*, '(dt)') a
  open(10, status='scratch', form='unformatted')
  write(10) a
  rewind(10)
  read(10) b
  close(10)
  write(*, '(dt)') b
end program
""", '(3,4)\n(4,3)\n')

    def test_unformatted_scratch_file(self):
        self.check("""program p
  implicit none
  integer :: a(3) = [1, 2, 3], b(3), n
  real :: x = 2.5, y
  open(10, status='scratch', form='unformatted')
  write(10) a, x
  write(10) 42
  rewind(10)
  read(10) b, y
  read(10) n
  print '(3i3,f5.1,i4)', b, y, n
  close(10)
end program
""", "  1  2  3  2.5  42\n")

if __name__ == "__main__":
    unittest.main()

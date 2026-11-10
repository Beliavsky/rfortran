"""Size relationships between array dummies of unknown size:
-Wunchecked-conformance (in -Wextra) for procedures that never check them,
and -Wconformance (in -Wall) for calls whose actuals break them; and arrays
whose sizes depend on the data: -Wmaybe-nonconforming and
-Wmaybe-out-of-bounds (in -Wextra).

Run after cargo build --release: python tests/test_conformance_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

PROCEDURES = '''module m
implicit none
contains
function sumproduct(x, y) result(xy)
real, intent(in) :: x(:), y(:)
real :: xy
xy = sum(x*y)
end function sumproduct
function checked(x, y) result(xy)
real, intent(in) :: x(:), y(:)
real :: xy
if (size(x) /= size(y)) error stop "checked: sizes differ"
xy = dot_product(x, y)
end function checked
function declared(x, y) result(xy)
real, intent(in) :: x(:), y(size(x))
real :: xy
xy = sum(x*y)
end function declared
subroutine mult(a, b, c)
real, intent(in) :: a(:,:), b(:,:)
real, intent(out) :: c(:,:)
c = matmul(a, b)
end subroutine mult
subroutine copy(x, y)
real, intent(in) :: x(:)
real, intent(out) :: y(:)
y = 2*x
end subroutine copy
real function indirect(x, y)
real, intent(in) :: x(:), y(:)
integer :: n
n = size(x)
if (size(y) /= n) error stop
indirect = sum(x*y)
end function indirect
logical function bigger(x, y)
real, intent(in) :: x(:), y(:)
call assert_equal(size(x), size(y))
bigger = any(x > y)
end function bigger
subroutine assert_equal(i, j)
integer, intent(in) :: i, j
if (i /= j) error stop
end subroutine assert_equal
end module m
'''

CALLS = PROCEDURES + '''program main
use m
implicit none
real :: a(2,3), b(2,2), c(2,2), v(3)
print *, sumproduct([10.0, 20.0], [30.0, 40.0])
print *, checked(y=[1.0], x=[1.0, 2.0])
a = 1; b = 1; v = 0
call mult(a, b, c)
call copy([1.0, 2.0], v)
end program main
'''

SIZES = '''program main
implicit none
integer :: n, i
real :: x(10)
logical :: tf(10)
real, allocatable :: x1(:), x2(:), a(:), b(:), c(:), d(:), e(:), f(:), g(:)
call random_number(x)
tf = x > 0.5
x1 = pack(x, tf)
x2 = pack(x, .not. tf)
print *, x1 - x2
print *, x1(1), x1(size(x))
n = 5
a = pack(x, tf)
if (size(a) > 0) print *, a(1)
b = pack(x, tf)
print *, a + b
allocate (c(n), d(n), e(n+1))
c = 1
d = 2
print *, c + d, c + e
f = pack(x, x < 0.1)
if (count(x < 0.1) >= 2) print *, f(2)
g = pack(x, x > 0.9)
do i = 1, size(g)
   print *, g(i)
end do
end program main
'''


def compile_with(source, *options):
    with tempfile.TemporaryDirectory(prefix='conformance-') as tmp:
        path = Path(tmp) / 'main.f90'
        path.write_text(source, encoding='utf-8')
        return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                              text=True, encoding='utf-8', errors='replace', timeout=120)


def found(err, category):
    out = []
    for block in err.split('warning: ')[1:]:
        head = block.split('\n')[0]
        if f'[{category}]' in head:
            out.append((int(re.search(r'main\.f90:(\d+):', block).group(1)), head.replace(f' [{category}]', '')))
    return out


class ConformanceWarnings(unittest.TestCase):
    def test_unchecked_requirements(self):
        err = compile_with(PROCEDURES, '-Wextra').stderr
        # Not `checked`, `declared`, `indirect` (n = size(x)) or `bigger`
        # (a call given both sizes).
        self.assertEqual(found(err, 'unchecked-conformance'), [
            (5, '`x*y` requires `size(x) == size(y)`, which `sumproduct` does not check'),
            (21, '`matmul(a, b)` requires `size(a, 2) == size(b, 1)`, which `mult` does not check'),
            (27, 'assigning to `y` requires `size(x) == size(y)`, which `copy` does not check'),
        ], err)
        self.assertIn('suggestion: if (size(x) /= size(y)) error stop "sumproduct: x and y do not conform"', err)
        self.assertNotIn('unchecked-conformance', compile_with(PROCEDURES, '-Wall').stderr)

    def test_calls_that_break_requirements(self):
        err = compile_with(CALLS, '-Wall').stderr
        self.assertEqual(found(err, 'conformance'), [
            (52, '`checked` requires `size(x) == size(y)`, but this call passes 2 and 1 elements'),
            (54, '`mult` requires `size(a, 2) == size(b, 1)`, but this call passes size(a, 2) = 3 and size(b, 1) = 2'),
            (55, '`copy` requires `size(x) == size(y)`, but this call passes 2 and 3 elements'),
        ], err)
        self.assertNotIn('[conformance]', compile_with(CALLS, '-Wall', '-Wno-conformance').stderr)

    def test_data_dependent_sizes(self):
        err = compile_with(SIZES, '-Wextra').stderr
        # Not `a + b` (one mask), `c + d` (one size), or the guarded `a(1)`,
        # `f(2)` and `g(i)`.
        self.assertEqual(found(err, 'maybe-nonconforming'), [
            (11, '`x1 - x2` combines arrays whose sizes may differ: `x1` has count(tf) elements and `x2` has count(.not. tf)'),
            (21, '`c + e` combines arrays whose sizes may differ: `c` has n elements and `e` has n + 1'),
        ], err)
        self.assertEqual(found(err, 'maybe-out-of-bounds'), [
            (12, '`x1(1)` is out of bounds when `x1` is empty; `x1 = pack(x, tf)` gives 0 to 10 elements'),
            (12, '`x1(size(x))` is `x1(10)`, out of bounds unless all 10 elements of `tf` are true; '
                 '`x1 = pack(x, tf)` gives 0 to 10 elements'),
        ], err)
        self.assertNotIn('maybe-', compile_with(SIZES, '-Wall').stderr)


if __name__ == '__main__':
    unittest.main()

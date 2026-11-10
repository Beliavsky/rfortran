"""Flow and interface warnings: -Wmaybe-uninitialized, -Wcould-be-intent-in,
-Wunused-only and -Wredundant-cycle (in -Wextra), and -Wloop-runs-once (in
-Wall).

Run after cargo build --release: python tests/test_flow_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

UNINITIALIZED = '''program main
implicit none
integer :: i, n, total, count, found, k
real :: x, y
logical :: ok
n = 3
total = 0
do i = 1, n
   total = total + i
end do
if (n > 2) then
   x = 1.0
end if
print *, x
print *, count
do
   read (*, *, iostat=k) y
   if (k /= 0) exit
   found = 1
   exit
end do
print *, found
call fill(ok)
print *, ok, total, y
contains
subroutine fill(v)
logical, intent(out) :: v
v = .true.
end subroutine fill
end program main
'''

LOOPS = '''program main
implicit none
integer :: i, n
n = 3
do i = 1, n
   print *, i
   exit
end do
do i = 1, n
   if (i > 1) exit
   print *, i
end do
do i = 1, n
   print *, i
   cycle
end do
end program main
'''

INTENT = '''module m
implicit none
private
public :: s
contains
subroutine s(a, b, c)
real, intent(inout) :: a, b
real, intent(inout) :: c(:)
b = a
call scale(c)
end subroutine s
subroutine scale(v)
real, intent(inout) :: v(:)
v = 2*v
end subroutine scale
end module m
program main
use m, only: s
use iso_fortran_env, only: real64, int32, output_unit
implicit none
real :: a = 1, b = 0, c(2) = 1
call s(a, b, c)
write (output_unit, *) a, b, c
end program main
'''


def compile_with(source, *options):
    with tempfile.TemporaryDirectory(prefix='flow-') as tmp:
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


class FlowWarnings(unittest.TestCase):
    def test_maybe_uninitialized(self):
        err = compile_with(UNINITIALIZED, '-Wextra').stderr
        # `x` only on one branch, `count` never, `found` not on the path
        # through the first EXIT; not `y`, read before both EXITs, nor `ok`,
        # passed to `fill`.
        self.assertEqual(found(err, 'maybe-uninitialized'), [
            (14, '`x` may be used before it is assigned'),
            (15, '`count` is used but never assigned'),
            (22, '`found` may be used before it is assigned'),
        ], err)
        self.assertNotIn('maybe-uninitialized', compile_with(UNINITIALIZED, '-Wall').stderr)

    def test_loop_runs_once_and_redundant_cycle(self):
        err = compile_with(LOOPS, '-Wall').stderr
        self.assertEqual([l for l, _ in found(err, 'loop-runs-once')], [5], err)
        self.assertNotIn('[redundant-cycle]', err)
        err = compile_with(LOOPS, '-Wextra').stderr
        self.assertEqual([l for l, _ in found(err, 'redundant-cycle')], [15], err)

    def test_could_be_intent_in(self):
        err = compile_with(INTENT, '-Wextra').stderr
        # `a` is only read; `b` is assigned; `c` is passed to `scale`.
        self.assertEqual(found(err, 'could-be-intent-in'), [
            (7, 'dummy argument `a` of subroutine `s` is INTENT(INOUT) but is never modified'),
        ], err)

    def test_unused_only(self):
        err = compile_with(INTENT, '-Wextra').stderr
        self.assertEqual(found(err, 'unused-only'), [
            (19, '`real64`, `int32` are imported from `iso_fortran_env` but never used'),
        ], err)
        self.assertIn('suggestion: use iso_fortran_env, only: output_unit', err)
        self.assertNotIn('unused-only', compile_with(INTENT, '-Wall').stderr)


if __name__ == '__main__':
    unittest.main()

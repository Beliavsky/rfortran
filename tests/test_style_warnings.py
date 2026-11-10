"""Style warnings after Clerman and Spector's "Modern Fortran: Style and
Usage": the -Wstyle group and its members, and -Wcompare-reals and
-Wlogical-comparison, which are also in -Wextra.

Run after cargo build --release: python tests/test_style_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

OLD = '''module m
implicit none
contains
subroutine s(x, n, flag)
integer n, i
real x(*)
logical flag
double precision d
d = 0
if (n .lt. 0 .or. flag .eqv. .true.) stop
do 10 i = 1, n
   if (x(i) > 0) exit
   if (x(i) == 0.5) d = d + 1
10 continue
go to 20
20 continue
end
end module m
program main
use m
implicit none
real :: x(3) = [1.0, -1.0, 0.5]
call s(x, 3, .false.)
print *, x
end program main
'''

MODERN = '''module m
use iso_fortran_env, only: dp => real64
implicit none
private
public :: s, dp
contains
subroutine s(x, n, ok)
real(dp), intent(inout) :: x(:)
integer, intent(in) :: n
logical, intent(in) :: ok
integer :: i
outer: do i = 1, n
   if (x(i) < 0 .and. ok) exit outer
   if (x(i) == 0) cycle outer
   x(i) = 2*x(i)
end do outer
end subroutine s
end module m
program main
use m, only: s, dp
implicit none
real(dp) :: x(3) = [1.0_dp, -1.0_dp, 0.5_dp]
call s(x, 3, .true.)
print *, x
end program main
'''

OBSOLETE = '''subroutine s(k, *)
integer :: k, a(2), b(2)
common /blk/ a
equivalence (a, b)
go to (1, 2), k
1 if (k) 2, 2, 3
2 return 1
3 continue
end subroutine s
'''


def compile_with(source, *options, name='main.f90'):
    with tempfile.TemporaryDirectory(prefix='style-') as tmp:
        path = Path(tmp) / name
        path.write_text(source, encoding='utf-8')
        return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                              text=True, encoding='utf-8', errors='replace', timeout=120)


def found(err):
    """(category, line) of each warning."""
    out = []
    for block in err.split('warning: ')[1:]:
        head = block.split('\n')[0]
        category = re.search(r'\[([a-z-]+)\]$', head)
        line = re.search(r'\.f90:(\d+):', block)
        if category and line:
            out.append((category.group(1), int(line.group(1))))
    return out


class StyleWarnings(unittest.TestCase):
    def test_style_group(self):
        result = compile_with(OLD, '-Wstyle')
        got = found(result.stderr)
        for expected in [('missing-double-colon', 5), ('assumed-size', 6), ('double-precision', 8),
                         ('old-relational', 10), ('logical-comparison', 10), ('stop-in-procedure', 10),
                         ('labeled-do', 11), ('unnamed-exit', 12), ('goto', 15), ('bare-end', 17)]:
            self.assertIn(expected, got, result.stderr)
        self.assertIn('suggestion: if (n < 0 .or. flag .eqv. .true.) stop', result.stderr)
        self.assertIn('suggestion: flag', result.stderr)
        self.assertIn('write `end subroutine s`', result.stderr)
        self.assertIn('suggestion: integer :: n, i', result.stderr)
        self.assertNotIn('[compare-reals]', result.stderr)

    def test_modern_code_is_clean(self):
        result = compile_with(MODERN, '-Wstyle', '-Wcompare-reals', '-Wlogical-comparison')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(found(result.stderr), [], result.stderr)

    def test_extra_members(self):
        got = found(compile_with(OLD, '-Wextra').stderr)
        self.assertIn(('compare-reals', 13), got)
        self.assertIn(('logical-comparison', 10), got)
        self.assertNotIn('old-relational', [c for c, _ in got])
        # Comparisons with an exact zero are not reported.
        self.assertEqual(found(compile_with(MODERN, '-Wcompare-reals').stderr), [])

    def test_obsolescent(self):
        err = compile_with(OBSOLETE, '-Wobsolescent').stderr
        for message in ['COMMON is obsolescent', 'EQUIVALENCE is obsolescent', 'computed GO TO is obsolescent',
                        'arithmetic IF is obsolescent', 'alternate return is obsolescent']:
            self.assertIn(message, err)

    def test_layout_and_case(self):
        source = 'program p\nINTEGER :: i\ndo i = 1, 2\n\tprint *, i\nend do\nprint *, "' + 'x' * 100 + '"\nend program p\n'
        err = compile_with(source, '-Wkeyword-case', '-Wtabs', '-Wline-length=80').stderr
        self.assertIn('keywords are written in more than one case', err)
        self.assertIn('1 line contains tab characters [tabs]', err)
        self.assertIn('1 line is longer than 80 characters [line-length]', err)
        self.assertNotIn('[line-length]', compile_with(source, '-Wline-length').stderr)

    def test_controls(self):
        self.assertEqual([c for c, _ in found(compile_with(OLD, '-Wall').stderr) if c in
                          ('old-relational', 'goto', 'bare-end', 'labeled-do')], [])
        err = compile_with(OLD, '-Wstyle', '-Wno-goto').stderr
        self.assertNotIn('[goto]', err)
        self.assertIn('[bare-end]', err)
        self.assertNotEqual(compile_with(OLD, '-Werror=goto').returncode, 0)
        self.assertIn('unknown warning option', compile_with(OLD, '-Wline-length=x').stderr)


if __name__ == '__main__':
    unittest.main()

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
subroutine s(n, ok, x)
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
call s(3, .true., x)
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

CONSTANTS = '''module constants_a
implicit none
integer, parameter :: dp = kind(1.0d0)
real(dp), parameter :: pi = 3.141592653589793_dp
end module constants_a
module constants_b
use iso_fortran_env, only: real64
implicit none
integer, parameter :: dp = real64
real(dp), parameter :: pi = 3.14159_dp
integer, parameter :: n = 10
end module constants_b
module m
implicit none
contains
function area(r) result(a)
real, intent(in) :: r
real :: a
real, parameter :: pi = 4*atan(1.0)
integer, parameter :: n = 10
a = pi*r**2
end function area
function circumference(r) result(c)
real, intent(in) :: r
real :: c
real, parameter :: pi = 3.14159265
c = 2*pi*r
end function circumference
end module m
program main
use m, only: area, circumference
implicit none
print *, area(1.0), circumference(1.0)
end program main
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

    def test_redundant_implicit(self):
        source = '''module m
implicit none
interface
   subroutine ext(x)
      implicit none
      real :: x
   end subroutine ext
end interface
contains
subroutine sub(x)
implicit none
real, intent(in) :: x
print *, "x =", x
end subroutine sub
end module m
program main
use m
implicit none
call sub(3.4)
end program main
'''
        err = compile_with(source, '-Wredundant-implicit').stderr
        # The module procedure's, not the interface body's or the program's.
        self.assertEqual(found(err), [('redundant-implicit', 11)], err)
        self.assertIn('interface bodies do not', err)

    def test_name_case(self):
        source = 'implicit none\nreal :: x\nX = 2.3\nprint *, x\nend\n'
        err = compile_with(source, '-Wname-case').stderr
        self.assertEqual(found(err), [('name-case', 3)], err)
        self.assertIn('`X` is declared as `x` [name-case]', err)
        self.assertIn('write `x`', err)
        # Different entities in different procedures may differ in case.
        other = '''module m
implicit none
contains
subroutine a(n)
integer :: n
print *, n
end subroutine a
subroutine b(N)
integer :: N
print *, N
end subroutine b
end module m
'''
        self.assertEqual(found(compile_with(other, '-Wname-case').stderr), [])

    def test_function_result(self):
        source = '''module m
implicit none
contains
function a(x) result(y)
real, intent(in) :: x
real :: y
y = x
end function a
real function b(x)
real, intent(in) :: x
b = 2*x
end function b
end module m
'''
        err = compile_with(source, '-Wfunction-result').stderr
        self.assertEqual(found(err), [('function-result', 9)], err)
        self.assertIn('suggestion: function b(x) result(b_result)', err)
        self.assertIn('then declare `real :: b_result` with the arguments', err)
        self.assertIn('rename to `b_result`', err)

    def test_could_be_conditional(self):
        first = 'if (x > 0) then\n   y = sqrt(x)\nelse\n   y = -1\nend if\n'
        second = 'if (x > 9) then\n   k = 1\nelse if (x < 0) then\n   k = -1\nelse\n   k = 0\nend if\n'
        third = 'if (x > 0) then\n   y = 1.0\n   k = 2\nelse\n   y = 0.0\nend if\n'
        source = 'implicit none\nreal :: x, y\ninteger :: k\nx = 4\n' + first + second + third + 'print *, y, k\nend\n'
        err = compile_with(source, '-Wcould-be-conditional').stderr
        # Not the third block, which has two statements in a branch.
        self.assertEqual(found(err), [('could-be-conditional', 5), ('could-be-conditional', 10)], err)
        self.assertIn('suggestion: y = (x > 0 ? sqrt(x) : -1.0)', err)
        self.assertIn('suggestion: k = (x > 9 ? 1 : x < 0 ? -1 : 0)', err)
        self.assertIn('unlike MERGE', err)
        self.assertNotIn('could-be-conditional', compile_with(source, '-Wstyle', '-Wall', '-Wextra').stderr)
        # The suggestions compile and give the same results.
        fixed = source.replace(first, 'y = (x > 0 ? sqrt(x) : -1.0)\n').replace(second, 'k = (x > 9 ? 1 : x < 0 ? -1 : 0)\n')
        self.assertEqual(*self.outputs(source, fixed))

    def test_one_line_ifs(self):
        block = 'if (x > 0) then\n   x = sqrt(x)\nend if\n'
        source = ('implicit none\nreal :: x\nx = 4\n' + block
                  + 'if (x > 1) then ! keep\n   x = 1\nend if\nif (x < 0) print *, x\nprint *, x\nend\n')
        err = compile_with(source, '-Wcould-be-one-line-if').stderr
        self.assertEqual(found(err), [('could-be-one-line-if', 4)], err)
        self.assertIn('suggestion: if (x > 0) x = sqrt(x)', err)
        err = compile_with(source, '-Wone-line-if').stderr
        self.assertEqual(found(err), [('one-line-if', 10)], err)
        self.assertIn('if (x < 0) then', err)
        self.assertNotIn('one-line-if', compile_with(source, '-Wstyle').stderr)
        # The suggestion compiles and gives the same result.
        self.assertEqual(*self.outputs(source, source.replace(block, 'if (x > 0) x = sqrt(x)\n')))

    def test_semicolons_and_trailing_whitespace(self):
        source = ('program main\nimplicit none\ninteger :: i, n\nreal :: x, y\nx = 1.0; y = 2.0\nn = 1;\n'
                  'select case (n)\n   case (1); y = sin(x)\n   case (2); y = cos(x); x = 0\n   case default; y = x\n'
                  'end select\ndo i = 1, n; print *, i; end do  \nprint *, x, y ! a; b\nend program main\n')
        err = compile_with(source, '-Wstyle').stderr
        # Lines 8 and 10 are a CASE selector and one statement.
        self.assertIn('3 lines have more than one statement [semicolon]', err)
        self.assertIn('1 line ends with a semicolon [semicolon]', err)
        self.assertIn('1 line has trailing whitespace [trailing-whitespace]', err)
        lines = sorted({int(n) for n in re.findall(r'(\d+) │', err)})
        self.assertNotIn(8, lines, err)
        self.assertNotIn(10, lines, err)
        self.assertNotIn('[semicolon]', compile_with(source, '-Wall').stderr)
        self.assertNotIn('[semicolon]', compile_with(source, '-Wstyle', '-Wno-semicolon').stderr)

    def test_duplicate_constant(self):
        err = compile_with(CONSTANTS, '-Wduplicate-constant').stderr
        got = [(c, l) for c, l in found(err) if c == 'duplicate-constant']
        # `dp` twice; `pi` at a shorter value, at default kind, and again at
        # default kind with the same value; not the INTEGER `n`.
        self.assertEqual(got, [('duplicate-constant', 9), ('duplicate-constant', 10),
                               ('duplicate-constant', 19), ('duplicate-constant', 26)], err)
        self.assertIn('named constant `dp` is defined in 2 places', err)
        self.assertIn('named constant `pi` = 3.14159_dp has a different value from its definition elsewhere, '
                      '3.141592653589793_dp', err)
        self.assertIn('named constant `pi` is also defined elsewhere with another kind', err)
        self.assertIn('named constant `pi` is defined in 4 places', err)
        self.assertNotIn('`n`', err)
        self.assertIn('[duplicate-constant]', compile_with(CONSTANTS, '-Wstyle').stderr)
        self.assertNotIn('[duplicate-constant]', compile_with(CONSTANTS, '-Wall', '-Wextra').stderr)

    def test_declaration_and_argument_order(self):
        source = """module m
implicit none
contains
subroutine s(z, n, x, tol)
integer, parameter :: wp = kind(1.0d0)
real(wp), intent(in), optional :: tol
real(wp), intent(out) :: z
integer :: i
integer, intent(in) :: n
real(wp), intent(in) :: x(n)
z = 0
do i = 1, n
   z = z + x(i)
end do
end subroutine s
end module m
program main
use m, only: s
implicit none
real(kind(1.0d0)) :: z
call s(z, 2, [1.0d0, 2.0d0])
print *, z
end program main
"""
        err = compile_with(source, '-Wstyle').stderr
        got = [x for x in found(err) if x[0] in ('declaration-order', 'argument-order')]
        self.assertEqual(sorted(got), [('argument-order', 4), ('declaration-order', 9)], err)
        self.assertIn('dummy argument `n` is declared after local variable `i`', err)
        self.assertIn('INTENT(IN) argument `n` follows output argument `z`', err)
        self.assertIn('suggestion: subroutine s(n, x, z, tol)', err)
        self.assertNotIn('-order]', compile_with(source, '-Wall', '-Wextra').stderr)

    def test_do_while_true(self):
        source = ('program main\nimplicit none\ninteger :: i\ni = 0\ndo while (.true.)\n   i = i + 1\n   if (i > 2) exit\nend do\n'
                  'outer: do while (.not. .false.)\n   exit outer\nend do outer\ndo while (i < 5)\n   i = i + 1\nend do\nend program main\n')
        err = compile_with(source, '-Wstyle').stderr
        self.assertEqual([x for x in found(err) if x[0] == 'do-while-true'], [('do-while-true', 5), ('do-while-true', 9)], err)
        self.assertIn('`do while (.true.)` is an endless loop; write `do`', err)
        self.assertIn('suggestion: outer: do\n', err)
        self.assertNotIn('do-while-true', compile_with(source, '-Wall', '-Wextra').stderr)

    def outputs(self, *sources):
        """The output of running each source."""
        out = []
        with tempfile.TemporaryDirectory(prefix='style-') as tmp:
            for i, text in enumerate(sources):
                path = Path(tmp) / f'v{i}.f90'
                path.write_text(text, encoding='utf-8')
                run = subprocess.run([COMPILER, 'run', path], cwd=tmp, capture_output=True, text=True, timeout=120)
                self.assertEqual(run.returncode, 0, run.stderr)
                out.append(run.stdout)
        return out


if __name__ == '__main__':
    unittest.main()

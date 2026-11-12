"""-Wold-array-constructor (in -Wstyle), -Wquote-style[=double|single]
(opt-in), -Wcould-be-function (opt-in) and -Wredundant-use (in -Wstyle).

Run after cargo build --bin rfort: python tests/test_source_style_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

LITERALS = '''program p
implicit none
integer :: a(3) = (/ 1, 2, 3 /), b(2)
character(len=8) :: s = 'it''s', t = "x", u = 'say "hi"'
b = (/ a(1), a(2) /) + [1, 1]
print "(a,i0)", "n = ", b(1)
print *, s, t, u
end program p
'''

FUNCTIONS = '''module m
implicit none
integer :: calls = 0
interface assignment(=)
   module procedure assign
end interface
type t
   integer :: v
end type
contains
subroutine area(w, h, a)
real, intent(in) :: w, h
real, intent(out) :: a
a = w * h
end subroutine area
subroutine scaled(v, s, w)
real, intent(in) :: v(:), s
real, intent(out) :: w(:)
w = s * v
end subroutine scaled
subroutine noisy(x, y)
real, intent(in) :: x
real, intent(out) :: y
y = x
print *, y
end subroutine noisy
subroutine counted(x, y)
real, intent(in) :: x
real, intent(out) :: y
y = x
calls = calls + 1
end subroutine counted
subroutine split(x, lo, hi)
real, intent(in) :: x
real, intent(out) :: lo, hi
lo = x - 1
hi = x + 1
end subroutine split
subroutine assign(a, b)
type(t), intent(out) :: a
integer, intent(in) :: b
a%v = b
end subroutine assign
end module m
'''

USES = '''module m
use iso_fortran_env, only: int32, int64
implicit none
interface
   function f(x)
      use iso_fortran_env, only: int64
      integer(int64) :: f, x
   end function f
end interface
contains
subroutine sub()
use iso_fortran_env, only: int32
print *, int32
end subroutine sub
subroutine other()
use iso_fortran_env, only: int64, real64
print *, int64, real64
end subroutine other
subroutine third()
use iso_fortran_env, only: i8 => int64
print *, i8
end subroutine third
end module m
program p
use iso_fortran_env, only: real32
use iso_fortran_env, only: real32
print *, real32
end program p
'''


class SourceStyleWarnings(unittest.TestCase):
    def check(self, program, flags=()):
        with tempfile.TemporaryDirectory(prefix='source-style-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text(program, encoding='utf-8')
            result = subprocess.run([COMPILER, *flags, 'check', source], cwd=tmp,
                                    capture_output=True, text=True, timeout=60)
            return result.returncode, result.stderr

    def test_old_array_constructor(self):
        code, stderr = self.check(LITERALS, ['-Wold-array-constructor'])
        self.assertEqual(code, 0, stderr)
        self.assertEqual(stderr.count('[old-array-constructor]'), 2, stderr)
        self.assertIn('suggestion: integer :: a(3) = [ 1, 2, 3 ], b(2)', stderr)
        self.assertIn('suggestion: b = [ a(1), a(2) ] + [1, 1]', stderr)
        code, stderr = self.check(LITERALS, ['-Wstyle'])
        self.assertEqual(stderr.count('[old-array-constructor]'), 2, stderr)
        self.assertNotIn('[quote-style]', stderr)
        # Square brackets are Fortran 2003.
        code, stderr = self.check(LITERALS, ['-Wold-array-constructor', '-std=f95'])
        self.assertNotIn('[old-array-constructor]', stderr)

    def test_quote_style(self):
        code, stderr = self.check(LITERALS, ['-Wquote-style'])
        self.assertEqual(code, 0, stderr)
        self.assertEqual(stderr.count('[quote-style]'), 1, stderr)
        # 'say "hi"' keeps its single quotes; 'it''s' loses its doubled quote.
        self.assertIn('''suggestion: character(len=8) :: s = "it's", t = "x", u = 'say "hi"\'''', stderr)
        code, stderr = self.check(LITERALS, ['-Wquote-style=single'])
        self.assertEqual(stderr.count('[quote-style]'), 2, stderr)
        self.assertIn("suggestion: character(len=8) :: s = 'it''s', t = 'x', u = 'say \"hi\"'", stderr)
        self.assertIn("suggestion: print '(a,i0)', 'n = ', b(1)", stderr)
        code, stderr = self.check(LITERALS, ['-Werror=quote-style'])
        self.assertNotEqual(code, 0)
        code, stderr = self.check(LITERALS, ['-Wquote-style=back'])
        self.assertNotEqual(code, 0)
        self.assertIn('unknown warning option', stderr)

    def test_could_be_function(self):
        self.assertEqual(self.check(FUNCTIONS), (0, ''))
        code, stderr = self.check(FUNCTIONS, ['-Wcould-be-function'])
        self.assertEqual(code, 0, stderr)
        # Not noisy (I/O), counted (module variable), split (two outputs)
        # or assign (an assignment interface).
        self.assertEqual(stderr.count('[could-be-function]'), 2, stderr)
        self.assertIn('subroutine `area` could be a function: its only effect is to define INTENT(OUT) `a`', stderr)
        self.assertIn('suggestion: `pure function area(w, h) result(a)`, with `call area(...)` becoming `a = area(w, h)`', stderr)
        self.assertIn('subroutine `scaled` could be a function', stderr)
        self.assertIn('the result `w` needs an explicit shape', stderr)
        self.assertIn('example.f90:11:', stderr)

    def test_redundant_use(self):
        self.assertEqual(self.check(USES), (0, ''))
        code, stderr = self.check(USES, ['-Wredundant-use'])
        self.assertEqual(code, 0, stderr)
        # Not the interface body, which inherits nothing, nor the rename.
        self.assertEqual(stderr.count('[redundant-use]'), 3, stderr)
        self.assertIn('USE of `iso_fortran_env` imports nothing new: the host already imports `int32`', stderr)
        self.assertIn('nothing else in the host uses `int32`; removing the host\'s import instead keeps it local to this procedure', stderr)
        self.assertIn('`int64` in this USE of `iso_fortran_env`: the host already imports it', stderr)
        self.assertIn('suggestion: use iso_fortran_env, only: real64', stderr)
        self.assertIn('an earlier USE in this scope already imports `real32`', stderr)
        self.assertIn('example.f90:12:', stderr)
        code, stderr = self.check(USES, ['-Wstyle'])
        self.assertEqual(stderr.count('[redundant-use]'), 3, stderr)


if __name__ == '__main__':
    unittest.main()

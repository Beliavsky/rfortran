"""-Wdefault-real, opt-in: REAL and COMPLEX declarations without a kind.
The suggestion uses the REAL kind constant the file uses most often, or
real32 from ISO_FORTRAN_ENV, which keeps the current precision.

Run after cargo build --release: python tests/test_default_real.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

PLAIN = '''implicit none
real :: x, y(3)
complex z
x = 1
y = 2
z = (1, 2)
print *, x, y, z, half(x)
contains
real function half(t)
  real, intent(in) :: t
  half = t/2
end function
end
'''

WITH_KIND = '''module kinds
implicit none
integer, parameter :: dp = kind(1.0d0)
end module kinds

program p
use kinds
implicit none
real(dp) :: a
real(kind=dp) :: b
real :: c
double precision :: d
real*8 :: e
a = 1
b = 2
c = real(3)
d = 4
e = 5
print *, a, b, c, d, e
end program p
'''


class DefaultReal(unittest.TestCase):
    def check(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='default-real-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                                  text=True, encoding='utf-8', errors='replace', timeout=120)

    def test_declarations(self):
        err = self.check(PLAIN, '-Wdefault-real').stderr
        self.assertIn('`x`, `y` declared as default REAL [default-real]', err)
        self.assertIn('`z` declared as default COMPLEX [default-real]', err)
        self.assertIn('function `half` has a default REAL result [default-real]', err)
        self.assertIn('`t` declared as default REAL [default-real]', err)
        self.assertEqual(err.count('[default-real]'), 4, err)
        self.assertIn('use real(real32)', err)
        self.assertIn('use complex(real32)', err)
        self.assertIn('use `real64` if double precision was intended', err)

    def test_suggests_the_kind_in_use(self):
        err = self.check(WITH_KIND, '-Wdefault-real').stderr
        # Only `c`: kinds, REAL*8, DOUBLE PRECISION and the conversion real(3) are not reported.
        self.assertEqual(err.count('[default-real]'), 1, err)
        self.assertIn('`c` declared as default REAL', err)
        self.assertIn('use real(dp)', err)
        self.assertIn('`dp` is the REAL kind this file uses', err)

    def test_opt_in_and_controls(self):
        for options in ((), ('-Wall',), ('-Wall', '-Wextra')):
            self.assertNotIn('default-real', self.check(PLAIN, *options).stderr, options)
        self.assertNotEqual(self.check(PLAIN, '-Werror=default-real').returncode, 0)
        self.assertNotIn('default-real', self.check(PLAIN, '-Wdefault-real', '-Wno-default-real').stderr)
        self.assertNotIn('default-real', self.check('!#allow(default-real)\n' + PLAIN, '-Wdefault-real').stderr)


if __name__ == '__main__':
    unittest.main()

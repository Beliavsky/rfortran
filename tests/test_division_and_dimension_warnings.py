"""-Winteger-division (in -Wall) for integer quotients converted to REAL,
the opt-in -Winteger-division-all for every other integer division, and
-Wdimension-override (in -Wextra) for entity shapes or lengths that override
DIMENSION or the CHARACTER length of their declaration.

Run after cargo build --release: python tests/test_division_and_dimension_warnings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

DIVISIONS = '''implicit none
real :: x, y
integer :: n, m, k
n = 7
m = 2
x = n/m
y = 1/n*x
k = n/m
x = real(n/m)
x = real(n)/m
x = 6/2
if (x > n/m) print *, x
print *, y, k, (k - 1)/m + 1
end
'''

DIMENSIONS = '''implicit none
real, dimension(2) :: x, y(3)
integer, dimension(2), save :: a = 0, b(4) = 1
character(len=8) :: s, t*20
x = 1
y = 2
s = 'a'
t = 'b'
print *, x, y, a, b, s, t
end
'''


class DivisionWarnings(unittest.TestCase):
    def check(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='division-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp, capture_output=True,
                                  text=True, encoding='utf-8', errors='replace', timeout=120)

    def lines(self, err, category):
        """Line numbers of the warnings of a category."""
        out = []
        for block in err.split('warning: ')[1:]:
            if f'[{category}]' in block.split('\n')[0]:
                out.append(int(re.search(r'main\.f90:(\d+):', block).group(1)))
        return out

    def test_quotients_converted_to_real(self):
        err = self.check(DIVISIONS, '-Wall').stderr
        # x = n/m, 1/n*x and x > n/m; not k = n/m, real(n/m), real(n)/m or 6/2.
        self.assertEqual(self.lines(err, 'integer-division'), [6, 7, 12], err)
        self.assertIn('integer division truncates its quotient before conversion to REAL', err)
        self.assertIn("Python's // rounds down", err)
        self.assertNotIn('[integer-division-all]', err)

    def test_every_integer_division(self):
        err = self.check(DIVISIONS, '-Winteger-division-all').stderr
        # k = n/m, real(n/m), which still truncates, and (k - 1)/m.
        self.assertEqual(self.lines(err, 'integer-division-all'), [8, 9, 13], err)
        self.assertIn('-7/2 is -3 in Fortran and C but -7//2 is -4 in Python', err)
        self.assertNotIn('integer-division-all', self.check(DIVISIONS, '-Wall', '-Wextra').stderr)


class DimensionOverride(unittest.TestCase):
    check = DivisionWarnings.check

    def test_overrides(self):
        err = self.check(DIMENSIONS, '-Wextra').stderr
        self.assertIn('`y` is declared with its own shape, overriding DIMENSION(2) of this statement [dimension-override]', err)
        self.assertIn('suggestion: real :: x(2), y(3)', err)
        self.assertIn('`b` is declared with its own shape, overriding DIMENSION(2)', err)
        self.assertIn('suggestion: integer, save :: a(2) = 0, b(4) = 1', err)
        self.assertIn('`t` is declared with its own length, overriding (len=8) of this statement', err)
        self.assertEqual(err.count('[dimension-override]'), 3, err)

    def test_suggestion_compiles_with_the_same_result(self):
        fixed = DIMENSIONS.replace('real, dimension(2) :: x, y(3)', 'real :: x(2), y(3)') \
                          .replace('integer, dimension(2), save :: a = 0, b(4) = 1', 'integer, save :: a(2) = 0, b(4) = 1')
        outputs = []
        with tempfile.TemporaryDirectory(prefix='dimension-') as tmp:
            for name, text in (('before.f90', DIMENSIONS), ('after.f90', fixed)):
                path = Path(tmp) / name
                path.write_text(text, encoding='utf-8')
                result = subprocess.run([COMPILER, 'run', path], cwd=tmp, capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stderr)
                outputs.append(result.stdout)
        self.assertEqual(outputs[0], outputs[1])

    def test_controls(self):
        self.assertNotIn('dimension-override', self.check(DIMENSIONS, '-Wall').stderr)
        self.assertIn('[dimension-override]', self.check(DIMENSIONS, '-Wdimension-override').stderr)
        self.assertNotIn('dimension-override', self.check(DIMENSIONS, '-Wextra', '-Wno-dimension-override').stderr)


if __name__ == '__main__':
    unittest.main()

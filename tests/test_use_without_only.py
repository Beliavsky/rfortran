"""-Wuse-without-only, opt-in: a USE statement without ONLY imports every
public entity of its module. The warning suggests the ONLY list the scope
needs, or says that nothing the module provides is used.

Each suggested statement is substituted into the program, which must then
compile and print the same output.
Run after cargo build --release: python tests/test_use_without_only.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

MODULES = '''module kinds
implicit none
integer, parameter :: dp = kind(1.0d0), sp = kind(1.0)
end module kinds

module vectors
use kinds
implicit none
private
public :: vec, norm, operator(+), operator(.dot.), zero
type :: vec
  real(dp) :: x = 0, y = 0
end type
interface operator(+)
  module procedure add
end interface
interface operator(.dot.)
  module procedure dot
end interface
type(vec), parameter :: zero = vec(0, 0)
contains
type(vec) function add(a, b)
  type(vec), intent(in) :: a, b
  add = vec(a%x + b%x, a%y + b%y)
end function
real(dp) function dot(a, b)
  type(vec), intent(in) :: a, b
  dot = a%x*b%x + a%y*b%y
end function
real(dp) function norm(a)
  type(vec), intent(in) :: a
  norm = sqrt(a .dot. a)
end function
end module vectors

module unrelated
implicit none
integer :: counter = 0
end module unrelated
'''

MAIN = '''program main
use kinds
use vectors
use unrelated
implicit none
type(vec) :: a, b
a = vec(3.0_dp, 4.0_dp)
b = a + vec(1, 1)
print '(3f8.3)', norm(a), a .dot. b, norm(b)
call show()
contains
subroutine show()
  print '(f8.3)', norm(zero + a)
end subroutine
end program main
'''


class UseWithoutOnly(unittest.TestCase):
    def check(self, source, *options):
        with tempfile.TemporaryDirectory(prefix='use-only-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *options, 'check', path], cwd=tmp,
                                  capture_output=True, text=True, timeout=120)

    def run_program(self, source):
        with tempfile.TemporaryDirectory(prefix='use-only-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            result = subprocess.run([COMPILER, 'run', path], cwd=tmp,
                                    capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return result.stdout

    def test_opt_in(self):
        for options in ((), ('-Wall',), ('-Wall', '-Wextra')):
            result = self.check(MODULES + MAIN, *options)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn('use-without-only', result.stderr, options)

    def reports(self, source):
        """Each report's line number and suggested statement, or None."""
        err = self.check(source, '-Wuse-without-only').stderr
        out = []
        for block in err.split('warning: ')[1:]:
            line = int(re.search(r'main\.f90:(\d+):', block).group(1))
            suggestion = re.search(r'suggestion: (.*)', block)
            out.append((line, suggestion and suggestion.group(1).strip()))
        return out, err

    def test_suggestions(self):
        reports, err = self.reports(MODULES + MAIN)
        self.assertEqual(len(reports), 4, err)
        self.assertEqual([s for _, s in reports], [
            'use kinds, only: dp',
            'use kinds, only: dp',
            'use vectors, only: norm, vec, zero, operator(+), operator(.dot.)',
            None,
        ], err)
        self.assertIn('USE of unrelated imports nothing that is used [use-without-only]', err)

    def test_suggestions_compile_and_preserve_output(self):
        source = MODULES + MAIN
        expected = self.run_program(source)
        reports, err = self.reports(source)
        lines = source.splitlines()
        for line, suggestion in sorted(reports, reverse=True):
            if suggestion is None:
                del lines[line - 1]
            else:
                lines[line - 1] = suggestion
        source = '\n'.join(lines) + '\n'
        self.assertEqual(self.run_program(source), expected)
        self.assertEqual(self.reports(source)[0], [])

    def test_error_and_disable(self):
        result = self.check(MODULES + MAIN, '-Werror=use-without-only')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('error', result.stderr)
        result = self.check(MODULES + MAIN, '-Wuse-without-only', '-Wno-use-without-only')
        self.assertNotIn('use-without-only', result.stderr)


if __name__ == '__main__':
    unittest.main()

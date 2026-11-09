"""-Wliteral-kind, opt-in: kind numbers such as real(8), integer(kind=4),
real*8, 1.0_8 and int(n, 8) are processor-dependent; the warning suggests
ISO_FORTRAN_ENV names, which keep the meaning they have in rfort.

Run after cargo build --release: python tests/test_literal_kind.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

NUMBERED = '''program p
implicit none
real(8) :: x
integer(kind=4) :: n
real*8 :: y
complex(8) :: z
integer, parameter :: dp = kind(1.0d0)
real(dp) :: w
double precision :: d
x = 1.5_8
n = 7
y = real(n, 8) + x
z = cmplx(x, y, 8)
w = real(8)
d = 2
print '(3f8.3, i4, 2f8.3)', x, y, real(z), n, w, d
print '(i0)', int(n, kind=8)*3_8
end program p
'''

# The same program with the suggestions applied.
NAMED = '''program p
use iso_fortran_env, only: real64, int32, int64
implicit none
real(real64) :: x
integer(int32) :: n
real(real64) :: y
complex(real64) :: z
integer, parameter :: dp = kind(1.0d0)
real(dp) :: w
double precision :: d
x = 1.5_real64
n = 7
y = real(n, real64) + x
z = cmplx(x, y, real64)
w = real(8)
d = 2
print '(3f8.3, i4, 2f8.3)', x, y, real(z), n, w, d
print '(i0)', int(n, kind=int64)*3_int64
end program p
'''


class LiteralKind(unittest.TestCase):
    def run_compiler(self, source, *args):
        with tempfile.TemporaryDirectory(prefix='kind-') as tmp:
            path = Path(tmp) / 'main.f90'
            path.write_text(source, encoding='utf-8')
            return subprocess.run([COMPILER, *args, path], cwd=tmp, capture_output=True,
                                  text=True, encoding='utf-8', errors='replace', timeout=120)

    def test_reports_and_suggestions(self):
        err = self.run_compiler(NUMBERED, '-Wliteral-kind', 'check').stderr
        messages = re.findall(r'warning: (kind numbers? in .*) \[literal-kind\]', err)
        self.assertEqual(messages, [
            'kind number in `real(8)` is processor-dependent',
            'kind number in `integer(kind=4)` is processor-dependent',
            'kind number in `real*8` is processor-dependent',
            'kind number in `complex(8)` is processor-dependent',
            'kind number in `1.5_8` is processor-dependent',
            'kind number in `real(n, 8)` is processor-dependent',
            'kind number in `cmplx(x, y, 8)` is processor-dependent',
            'kind numbers in `int(n, kind=8)`, `3_8` are processor-dependent',
        ], err)
        for suggestion in ('use real(real64)', 'use integer(int32)', 'use complex(real64)',
                           'use 1.5_real64', 'use int64', 'use 3_int64'):
            self.assertIn(suggestion, err)
        self.assertIn('use iso_fortran_env, only: int64', err)
        # real(8) converts the value 8; real(dp) and DOUBLE PRECISION name kinds.
        self.assertNotIn('w = real(8)\n  │     ^', err)

    def test_suggestions_keep_the_output(self):
        before = self.run_compiler(NUMBERED, 'run')
        after = self.run_compiler(NAMED, 'run')
        self.assertEqual(before.returncode, 0, before.stderr)
        self.assertEqual(after.returncode, 0, after.stderr)
        self.assertEqual(before.stdout, after.stdout)
        self.assertNotIn('literal-kind', self.run_compiler(NAMED, '-Wliteral-kind', 'check').stderr)
        if shutil.which('gfortran'):
            with tempfile.TemporaryDirectory(prefix='kind-') as tmp:
                path = Path(tmp) / 'named.f90'
                path.write_text(NAMED, encoding='utf-8')
                exe = Path(tmp) / 'named.exe'
                built = subprocess.run(['gfortran', path, '-o', exe], capture_output=True, text=True, timeout=120)
                self.assertEqual(built.returncode, 0, built.stderr)
                result = subprocess.run([exe], capture_output=True, text=True, timeout=30)
                self.assertEqual(result.stdout.split(), before.stdout.split())

    def test_opt_in_and_controls(self):
        for options in ((), ('-Wall',), ('-Wall', '-Wextra')):
            self.assertNotIn('literal-kind', self.run_compiler(NUMBERED, *options, 'check').stderr, options)
        self.assertNotEqual(self.run_compiler(NUMBERED, '-Werror=literal-kind', 'check').returncode, 0)
        self.assertNotIn('literal-kind', self.run_compiler(NUMBERED, '-Wliteral-kind', '-Wno-literal-kind', 'check').stderr)


if __name__ == '__main__':
    unittest.main()

"""Preprocessor conditionals in .F and .F90 sources and with -cpp, checked
against gfortran.

Run after cargo build --bin rfort: python tests/test_preprocessor.py
Set RFORT_TEST_COMPILER to test a release compiler.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

# As in LAPACK's xSYTRD_SB2ST: OpenMP code, a version test and an #else.
FIXED = """      PROGRAM CONDS
      INTEGER N
#if defined(_OPENMP)
      USE OMP_LIB
#endif
      N = 1
#if defined(_OPENMP) && _OPENMP >= 201307
      N = 2
#else
      N = 3
#endif
#ifdef LEVEL
#if LEVEL > 1
      N = N + 100
#elif LEVEL == 1
      N = N + 10
#endif
#endif
      PRINT '(I4)', N
      END
"""

FREE = """program conds
  implicit none
  integer :: n
#ifndef LEVEL
  n = 7
#else
  n = 8
#endif
  print '(i4)', n
end program
"""


class Preprocessor(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='preprocessor-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def write(self, name, text):
        path = self.directory / name
        path.write_text(text, encoding='utf-8')
        return path

    def build(self, compiler, *args):
        return subprocess.run([compiler, *map(str, args)], cwd=self.directory, capture_output=True, text=True,
                              encoding='utf-8', timeout=120)

    def output(self, *args):
        """The program's output from rfort, checked against gfortran's."""
        result = self.build(COMPILER, *args, '-o', 'rfort.exe')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        got = subprocess.run([self.directory / 'rfort.exe'], capture_output=True, text=True, timeout=15).stdout
        if shutil.which('gfortran'):
            result = self.build('gfortran', *args, '-o', 'gfortran.exe')
            self.assertEqual(result.returncode, 0, result.stderr)
            expected = subprocess.run([self.directory / 'gfortran.exe'], capture_output=True, text=True, timeout=15).stdout
            self.assertEqual(got, expected)
        return got

    def test_upper_case_fixed_form(self):
        source = self.write('conds.F', FIXED)
        self.assertEqual(self.output(source), '   3\n')
        self.assertEqual(self.output('-DLEVEL', source), '  13\n')
        self.assertEqual(self.output('-DLEVEL=2', source), ' 103\n')
        self.assertEqual(self.output('-D', 'LEVEL=0', source), '   3\n')

    def test_upper_case_free_form_and_cpp_option(self):
        self.assertEqual(self.output(self.write('free.F90', FREE)), '   7\n')
        lower = self.write('lower.f90', FREE)
        self.assertEqual(self.output('-cpp', '-DLEVEL', lower), '   8\n')
        # Without -cpp a lower-case extension is not preprocessed.
        result = self.build(COMPILER, lower, '-o', 'lower.exe')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('#', result.stderr)
        result = self.build(COMPILER, '-nocpp', self.write('free2.F90', FREE), '-o', 'nocpp.exe')
        self.assertNotEqual(result.returncode, 0)

    def test_line_numbers_are_kept(self):
        source = self.write('lines.F90', """program lines
#if 0
  x = 1
#endif
  print *, undefined_name
end program
""")
        result = self.build(COMPILER, source, '-o', 'lines.exe')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('lines.F90:5', result.stderr.replace('\\', '/'))

    def test_unsupported_forms_are_errors(self):
        for text, message in [
            ('#include "a.h"\n', 'not supported'),
            ('#define N 3\nprogram p\nprint *, N\nend program\n', 'preprocessor name'),
            ('#if 1\nprogram p\nend program\n', '#endif'),
            ('#error unsupported build\nprogram p\nend program\n', 'unsupported build'),
        ]:
            with self.subTest(text=text):
                result = self.build(COMPILER, self.write('bad.F90', text), '-o', 'bad.exe')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)

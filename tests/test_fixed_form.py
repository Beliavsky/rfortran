"""Fixed source form: .f, .for, .ftn and .f77 files, and the source form options.

Run: python tests/test_fixed_form.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER

PROGRAM = """C     Labels, continuations, FORMAT and blanks inside numbers and operators
      PROGRAM SUMS
      INTEGER I, N
      DOUBLE PRECISION X
      N = 0
      DO 10 I = 1, 5
         N = N +
     1       I
   10 CONTINUE
      X = .70710 67811 8D0
      IF (N .EQ. 15 .AND . X .GT. 0.7D0) THEN
         WRITE (*, 100) N
      END IF
  100 FORMAT (1X, 5HSUM =, I4)
* A literal continued after column 72 keeps its blanks up to that column
      PRINT *, 'A long string continued across the line end at column se
     &venty-two'
d     PRINT *, 'debug line'
      END
"""

MODULE = """module values
implicit none
contains
integer function twice(n)
integer, intent(in) :: n
twice = 2*n
end function twice
end module values
"""

USER = """      PROGRAM USER
      USE VALUES
      PRINT *, TWICE(21)
      END
"""


class FixedForm(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='fixed-form-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def write(self, name, text):
        path = self.directory / name
        path.write_text(text, encoding='utf-8')
        return path

    def compile(self, *args):
        return subprocess.run([COMPILER, *map(str, args)], cwd=self.directory, capture_output=True,
                              text=True, encoding='utf-8', timeout=60)

    def build_and_run(self, *args):
        result = self.compile(*args)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        exe = self.directory / 'program.exe'
        run = subprocess.run([exe], capture_output=True, text=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stderr)
        return result, run.stdout

    def test_fixed_form_extensions(self):
        expected = (' SUM =  15\n'
                    ' A long string continued across the line end at column seventy-two\n')
        for extension in ('f', 'for', 'FTN', 'f77'):
            with self.subTest(extension=extension):
                source = self.write(f'sums.{extension}', PROGRAM)
                result, output = self.build_and_run(source, '-o', 'program.exe')
                self.assertEqual(output, expected)
                self.assertEqual(result.stderr, '')

    def test_run_and_check(self):
        source = self.write('sums.f', PROGRAM)
        result = self.compile('run', source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('SUM =  15', result.stdout)
        result = self.compile('check', source)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_debug_lines_as_code(self):
        source = self.write('sums.f', PROGRAM)
        _, output = self.build_and_run('-fd-lines-as-code', source, '-o', 'program.exe')
        self.assertIn('debug line', output)

    def test_fixed_and_free_sources_together(self):
        module = self.write('values.f90', MODULE)
        user = self.write('user.f', USER)
        _, output = self.build_and_run(module, user, '-o', 'program.exe')
        self.assertEqual(output.strip(), '42')

    def test_form_options_override_extension(self):
        fixed = self.write('user.f90', USER)
        module = self.write('values.f', MODULE)
        result = self.compile(module, fixed, '-o', 'program.exe')
        self.assertNotEqual(result.returncode, 0)
        _, output = self.build_and_run('-ffree-form', self.write('values.f', MODULE),
                                       self.write('user.f', USER.replace('      ', '')), '-o', 'program.exe')
        self.assertEqual(output.strip(), '42')
        source = self.write('sums.f90', PROGRAM)
        _, output = self.build_and_run('-ffixed-form', source, '-o', 'program.exe')
        self.assertIn('SUM =  15', output)

    def test_line_length(self):
        # Sequence numbers in columns 73-80 are ignored, and reported only with -Wall.
        text = ''.join(line.ljust(72) + 'SEQ%05d\n' % (i * 10) if line[:1] not in 'Cc*' else line + '\n'
                       for i, line in enumerate(USER.splitlines()))
        source = self.write('user.f', text)
        module = self.write('values.f90', MODULE)
        result, output = self.build_and_run(module, source, '-o', 'program.exe')
        self.assertEqual(output.strip(), '42')
        self.assertEqual(result.stderr, '')
        result = self.compile('-Wall', module, source, '-o', 'program.exe')
        self.assertIn('text after column 72 ignored on 4 line(s)', result.stderr)
        # Code may extend past column 72 with a longer line length.
        long = self.write('long.f', '      PRINT *, 1' + ' ' * 60 + '+ 2\n      END\n')
        _, output = self.build_and_run(long, '-o', 'program.exe')
        self.assertEqual(output.strip(), '1')
        for option in ('-ffixed-line-length-132', '-ffixed-line-length-none', '--ffixed-line-length=none'):
            with self.subTest(option=option):
                _, output = self.build_and_run(option, long, '-o', 'program.exe')
                self.assertEqual(output.strip(), '3')
        result = self.compile('-ffixed-line-length-3', long)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid fixed-form line length', result.stderr)

    def test_warnings(self):
        source = self.write('warn.f', "      PRINT *, 1\n     1, 2\n  PRINT *, 3\n      END\n")
        result = self.compile(source, '-o', 'program.exe')
        self.assertIn('warn.f:3: Warning: statement starts before column 7', result.stderr)
        result = self.compile('-Werror', source, '-o', 'program.exe')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('treated as errors', result.stderr)

    def test_error_lines(self):
        # Diagnostics point at the original line of a fixed-form source.
        text = PROGRAM.replace('      X = .70710', '      X = UNDEFINED_NAME\n      X = .70710')
        for args in ([], ['check'], ['run']):
            with self.subTest(args=args):
                source = self.write('sums.f', text)
                result = self.compile(*args, source)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('UNDEFINED_NAME'.lower(), result.stderr.lower())
                self.assertIn(':10:', result.stderr)


if __name__ == '__main__':
    unittest.main()

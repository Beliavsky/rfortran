"""Internal integer input regression tests; run after cargo build --bin rfort.

Run: python tests/test_internal_read.py
Uses the same F90C_TEST_COMPILER override as test_record_read.py.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_record_read import ROOT, build_commands


class InternalRead(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="internal-read-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)

    def check_program(self, source, error=None):
        results = []
        for command in build_commands(source, self.directory):
            with self.subTest(command=command):
                result = subprocess.run(command, capture_output=True, text=True, timeout=30)
                if error:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(error, result.stderr)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    results.append(result.stdout)
        if results:
            self.assertTrue(all(output == results[0] for output in results))
            return results[0]

    def test_values_status_and_integer_boundaries(self):
        source = ROOT / "tests/73_internal_integer_read.f90"
        output = self.check_program(source)
        expected = json.loads((ROOT / "tests/expectations.json").read_text(encoding="utf-8"))["cases"][source.stem]["program_stdout"]
        self.assertEqual(output, expected)
        if shutil.which("gfortran"):
            exe = self.directory / "reference.exe"
            result = subprocess.run(["gfortran", source, "-o", exe], cwd=self.directory,
                                    capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.split(), result.stdout.split())

    def test_record_input_followed_by_internal_conversion(self):
        source = self.directory / "dynamic.f90"
        source.write_text("character(len=64) :: text\ninteger :: n,ios\nn=91\nread(*,'(a)') text\nread(text,*,iostat=ios) n\nif (ios>0) ios=1\nprint *,ios\nif (ios==0) print *,n\nend\n")
        commands = build_commands(source, self.directory)
        for text, expected in [("-12", ["0", "-12"]), ("2147483648", ["1"]),
                               ("", ["-1"]), ("1.0", ["1"]), ("3*7", ["0", "7"]),
                               (",", ["0", "91"]), ("\t +42", ["0", "42"])]:
            for command in commands:
                with self.subTest(text=text, command=command):
                    result = subprocess.run(command, input=text + "\n", capture_output=True,
                                            text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stderr, "")
                    self.assertEqual(result.stdout.split(), expected)

    def test_failed_read_does_not_initialize_destination(self):
        for text, initialization in [("bad", ""), ("bad", "n=17"), ("", "n=17"), (",", "")]:
            with self.subTest(text=text, initialization=initialization):
                source = self.directory / "uninitialized.f90"
                source.write_text(f"character(len=8) :: text\ninteger :: n,ios\n{initialization}\ntext='{text}'\nread(text,*,iostat=ios) n\nprint *,ios,n\nend\n")
                self.check_program(source, "read of uninitialized variable `n`")

    def test_formatted_input_from_scalars_and_array_elements(self):
        source = self.directory / "formatted.f90"
        source.write_text("""program formatted
implicit none
character(len=20) :: line
character(len=8) :: s(3)
character(len=5) :: word
integer :: a, b, ios
real :: x
line = '  12  34 2.50 hello'
read(line, '(2i4,f5.2,a6)') a, b, x, word
print *, a, b, x, '[', word, ']'
read(line, '(a)') word
print *, '[', word, ']'
read(line, '(i4/i4)', iostat=ios) a, b
print *, ios < 0, a
s(1) = '   12'
s(2) = '  345'
s(3) = 'abc'
read(s(2), '(i8)') a
read(s(1), *) b
print *, a, b
read(s(3), '(i8)', iostat=ios) a
print *, ios > 0
end program formatted
""", encoding="utf-8")
        output = self.check_program(source)
        self.assertEqual(output.split(), "12 34 2.5 [hello] [ 12 ] T 12 345 12 T".split())

    def test_input_error_without_iostat_terminates(self):
        for text in ["bad", ""]:
            source = self.directory / "error.f90"
            source.write_text(f"character(len=8) :: text\ninteger :: n\ntext='{text}'\nread(text,*) n\nprint *,n\nend\n")
            self.check_program(source, "internal list-directed READ failed")


if __name__ == "__main__":
    unittest.main(verbosity=2)

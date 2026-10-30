"""A compiled program whose standard output cannot be written reports an
error and a failing exit status, instead of exiting successfully with its
output silently missing.

Standard output to a pipe or file is buffered until exit, so a failed write
used to be noticed by nobody. This was the cause of intermittent empty output
with exit status 0 in the test suite.

Run after cargo build --bin rfort: python tests/test_output_errors.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_record_read import build_commands


PROGRAM = """program p
integer :: i
do i = 1, 3
  print *, 'line', i
end do
end program p
"""


class OutputErrors(unittest.TestCase):
    def test_closed_standard_output_is_an_error(self):
        with tempfile.TemporaryDirectory(prefix="output-errors-") as tmp:
            directory = Path(tmp)
            source = directory / "example.f90"
            source.write_text(PROGRAM, encoding="utf-8")
            # build_commands returns the interpreter command, then -O0 and -O2.
            for command in build_commands(source, directory)[1:]:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.split(), ["line", "1", "line", "2", "line", "3"])
                    # The reader goes away before the buffered output is written.
                    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    process.stdout.close()
                    error = process.stderr.read().decode()
                    self.assertNotEqual(process.wait(timeout=15), 0)
                    self.assertIn("write to standard output failed", error)


if __name__ == "__main__":
    unittest.main()

"""List-directed output keeps DOUBLE PRECISION digits for expressions.

Printing a REAL(8) expression or function result directly, rather than a
variable, previously used single-precision digits (`4.` for 4.000004d0).
Run after cargo build --bin rfort: python tests/test_double_expression_output.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_record_read import build_commands


class DoubleExpressionOutput(unittest.TestCase):
    def test_expressions_and_function_results(self):
        program = """program main
implicit none
real(8) :: x = 4.000004d0
print *, x, x*1d0, -x, g(x), h(x)
contains
real(8) function g(a)
real(8), intent(in) :: a
g = a
end function g
function h(a) result(r)
real(8), intent(in) :: a
real(8) :: r
r = a
end function h
end program
"""
        with tempfile.TemporaryDirectory(prefix="double-output-") as tmp:
            source = Path(tmp) / "example.f90"
            source.write_text(program, encoding="utf-8")
            for command in build_commands(source, Path(tmp))[1:]:
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.split(), ["4.000004", "4.000004", "-4.000004", "4.000004", "4.000004"])


if __name__ == "__main__":
    unittest.main()

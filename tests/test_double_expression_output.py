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

    def test_arrays_in_interpreter_and_compiled(self):
        # The interpreter printed REAL(8) arrays, elements and DOUBLE literals
        # with single-precision digits.
        program = """real :: a(2)
real(8) :: b(2)
a = [0.1, 2.0/3.0]
b = [0.1d0, 2d0/3d0]
print *, a, a(2), 2*a(2)
print *, b, b(2), 2*b(2), b(1:1), 0.1d0/3
end
"""
        expected = ["0.1 0.666667 0.666667 1.33333",
                    "0.1 0.666666666666667 0.666666666666667 1.33333333333333 0.1 0.0333333333333333"]
        with tempfile.TemporaryDirectory(prefix="double-output-") as tmp:
            source = Path(tmp) / "example.f90"
            source.write_text(program, encoding="utf-8")
            for command in build_commands(source, Path(tmp)):
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=15)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual([" ".join(line.split()) for line in result.stdout.splitlines()], expected)


if __name__ == "__main__":
    unittest.main()

"""GOTO, forward and backward, and the text of list-directed REAL output.

GOTO is checked in the interpreter, at -O0 and -O2, and against gfortran. The
REAL output text is the same in the interpreter and compiled code, at -O0 and
-O2, and is compared exactly.
Run after cargo build --bin rfort: python tests/test_goto_and_real_output.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_deallocate
from test_record_read import COMPILER, build_commands


class GotoAndRealOutput(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_goto(self):
        """Forward out of nested loops, backward as a loop, skipping part of a
        loop body, both directions to one label, the terminal statement of a
        labeled DO, and leaving a loop in a function."""
        self.check("""program p
implicit none
integer :: i, j, n, s
! forward jump out of nested loops
s = 0
do i = 1, 5
  do j = 1, 5
    if (i*j > 6) go to 20
    s = s + i*j
  end do
end do
20 continue
print *, 'forward', s, i, j
! backward jump as a loop
n = 0
10 n = n + 1
if (n < 4) goto 10
print *, 'backward', n
! skipping statements in a loop body
s = 0
do i = 1, 6
  if (mod(i, 2) == 0) goto 30
  s = s + i
30 continue
end do
print *, 'skip', s
! forward and backward to the same label
n = 0
goto 40
n = 100
40 n = n + 1
if (n < 3) goto 40
print *, 'both', n
end program
""", ' forward 27 2 4\n backward 4\n skip 9\n both 3\n', std="legacy")
        self.check("""program p
implicit none
integer :: i, j, s
s = 0
do 10 i = 1, 3
  do 10 j = 1, 2
    if (j == 2) goto 10
    s = s + 10*i + j
10 continue
print *, s, first_negative([3, 5, -2, 7])
contains
integer function first_negative(a)
  integer, intent(in) :: a(:)
  integer :: k
  do k = 1, size(a)
    if (a(k) < 0) goto 99
  end do
  first_negative = 0
  return
99 first_negative = k
end function
end program
""", ' 63 3\n', std="legacy")

    def test_goto_into_a_construct_is_diagnosed(self):
        self.reject("""program p
integer :: i
goto 5
do i = 1, 2
5 print *, i
end do
end program
""", "jumps into a construct")

    def test_list_directed_real_text(self):
        """Fixed or exponent form as %g chooses, without trailing zeros but
        with a digit after the point, an unpadded exponent, and -0.0."""
        program = """program p
real :: x = 1e-6, y, z
double precision :: d = 1d-10, e
y = acos(0.5) * 180 / acos(-1.0)
z = 0
e = 1d0/3d0
print *, x, 2.5e10, -3.25e-7, 1.5, 100.0, 123456.7
print *, d, e, 0.1d0
print *, z, -z, 1.0/z
end program
"""
        expected = ' 1.0e-6 25000000000.0 -3.25e-7 1.5 100.0 123457.0\n 1.0e-10 0.333333333333333 0.1\n 0.0 -0.0 Infinity\n'
        with tempfile.TemporaryDirectory(prefix="real-output-") as tmp:
            source = Path(tmp) / "example.f90"
            source.write_text(program, encoding="utf-8")
            for command in build_commands(source, Path(tmp)):
                with self.subTest(command=command):
                    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, expected)


if __name__ == "__main__":
    unittest.main(verbosity=2)

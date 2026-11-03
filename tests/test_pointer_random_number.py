"""RANDOM_NUMBER accepts POINTER arguments: an allocated array or scalar,
an array of either REAL kind, and a strided section of a TARGET, whose other
elements are unchanged. Compiled and interpreted.

Run after cargo build --release:
RFORT_TEST_COMPILER=target/release/rfort.exe python tests/test_pointer_random_number.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

SOURCE = """module m
  implicit none
contains
  subroutine mean(n)
    integer, intent(in) :: n
    real, pointer :: x(:)
    allocate(x(n))
    call random_number(x)
    print '(l2)', sum(x) / n > 0 .and. sum(x) / n < 1
    deallocate(x)
  end subroutine
end module
program p
  use m
  implicit none
  real(8), target :: a(6)
  real(8), pointer :: q(:), s
  real, pointer :: r(:,:)
  call mean(5)
  a = -1d0
  q => a(2:6:2)
  call random_number(q)
  print '(6f6.2)', merge(1d0, a, a >= 0d0 .and. a < 1d0)
  allocate(s, r(2,3))
  call random_number(s)
  call random_number(r)
  print '(2l2)', s >= 0 .and. s < 1, all(r >= 0 .and. r < 1)
end program
"""
EXPECTED = " T\n -1.00  1.00 -1.00  1.00 -1.00  1.00\n T T\n"


class PointerRandomNumber(unittest.TestCase):
    def test_pointer_arguments(self):
        with tempfile.TemporaryDirectory(prefix="pointer-random-") as tmp:
            directory = Path(tmp)
            source = directory / "example.f90"
            source.write_text(SOURCE, encoding="utf-8")
            exe = directory / "example.exe"
            build = subprocess.run([COMPILER, source, "-o", exe], cwd=directory,
                                   capture_output=True, text=True, timeout=120)
            self.assertEqual(build.returncode, 0, build.stderr)
            for command in ([exe], [COMPILER, "run", source]):
                with self.subTest(command=command[-1]):
                    result = subprocess.run(command, cwd=directory, capture_output=True, text=True, timeout=60)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, EXPECTED)


if __name__ == "__main__":
    unittest.main()

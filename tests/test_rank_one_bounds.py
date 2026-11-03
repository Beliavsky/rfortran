"""Subscripts of rank-one arrays with the default lower bound are checked at
run time, in compiled code and by `rfort run`: fixed-size arrays, assumed-shape
and explicit-shape dummies. -nocheck removes the check.

Run after cargo build --release:
RFORT_TEST_COMPILER=target/release/rfort.exe python tests/test_rank_one_bounds.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

FIXED = """program p
  real(8) :: a(10)
  integer :: i
  a = [(i, i = 1, 10)]
  read *, i
  print '(f5.1)', a(i)
  a(i) = -1d0
  print '(f5.1)', sum(a)
end program
"""

DUMMIES = """module m
  implicit none
contains
  subroutine s(x, i)
    real(8), intent(in) :: x(:)
    integer, intent(in) :: i
    print '(f5.1)', x(i)
  end subroutine
  subroutine u(x, n, i)
    integer, intent(in) :: n, i
    real(8), intent(in) :: x(n)
    print '(f5.1)', x(i)
  end subroutine
end module
program p
  use m
  implicit none
  real(8) :: a(3)
  integer :: i
  a = [1d0, 2d0, 3d0]
  read *, i
  call s(a, i)
  call u(a, 3, i)
end program
"""

def run(source_text, *flags, data, interpret=False):
    with tempfile.TemporaryDirectory(prefix="rank-one-") as tmp:
        directory = Path(tmp)
        source = directory / "example.f90"
        source.write_text(source_text, encoding="utf-8")
        if interpret:
            return subprocess.run([COMPILER, "run", source], cwd=directory, input=data,
                                  capture_output=True, text=True, timeout=120)
        exe = directory / "example.exe"
        build = subprocess.run([COMPILER, *flags, source, "-o", exe], cwd=directory,
                               capture_output=True, text=True, timeout=120)
        if build.returncode != 0:
            raise AssertionError(f"build with {flags} failed:\n{build.stderr}")
        return subprocess.run([exe], cwd=directory, input=data, capture_output=True, text=True, timeout=60)


class RankOneBounds(unittest.TestCase):
    def assert_stopped(self, result, message):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(message, result.stdout + result.stderr)

    def test_fixed_size_array(self):
        for interpret in (False, True):
            with self.subTest(interpret=interpret):
                result = run(FIXED, data="4\n", interpret=interpret)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "  4.0\n 50.0\n")
                for i in ("0", "11", "-5"):
                    self.assert_stopped(run(FIXED, data=i + "\n", interpret=interpret),
                                        "array `a` dimension 1: subscript outside 1:10")

    def test_dummy_arrays(self):
        result = run(DUMMIES, data="2\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "  2.0\n  2.0\n")
        for interpret in (False, True):
            with self.subTest(interpret=interpret):
                self.assert_stopped(run(DUMMIES, data="4\n", interpret=interpret), "subscript outside 1:3")

    def test_nocheck_removes_the_check(self):
        # a(11) is past the end of a; only the build and absence of a
        # subscript error are certain.
        result = run(FIXED, "-nocheck", data="10\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, " 10.0\n 44.0\n")


if __name__ == "__main__":
    unittest.main()

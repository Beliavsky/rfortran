"""-nocheck compiles without run-time checks of uninitialized reads and array
subscripts; -fast is -O3 -nocheck. Programs whose reads are all defined and
whose subscripts are all in range give the same output with and without the
checks, including allocatable arrays, MOVE_ALLOC, INTENT(OUT), module
variables and CHARACTER arrays, whose initialization flags remain.

Run after cargo build --release:
RFORT_TEST_COMPILER=target/release/rfort.exe python tests/test_nocheck.py
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER


def build_and_run(source_text, *flags, data="3\n"):
    with tempfile.TemporaryDirectory(prefix="nocheck-") as tmp:
        directory = Path(tmp)
        source = directory / "example.f90"
        source.write_text(source_text, encoding="utf-8")
        exe = directory / "example.exe"
        build = subprocess.run([COMPILER, *flags, source, "-o", exe], cwd=directory,
                               capture_output=True, text=True, timeout=120)
        if build.returncode != 0:
            raise AssertionError(f"build with {flags} failed:\n{build.stderr}")
        return subprocess.run([exe], cwd=directory, input=data, capture_output=True, text=True, timeout=60)


class NoCheck(unittest.TestCase):
    def test_uninitialized_read_is_not_reported(self):
        text = """program p
  integer :: n, x
  read *, n
  if (n > 5) x = 1
  print '(a)', 'done'
  if (x == -12345) print '(a)', 'unlikely'
end program
"""
        checked = build_and_run(text)
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("read of uninitialized variable `x`", checked.stderr)
        for flags in (["-nocheck"], ["-fast"], ["-O0", "-nocheck"]):
            with self.subTest(flags=flags):
                result = build_and_run(text, *flags)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(result.stdout.startswith("done"))
                self.assertNotIn("uninitialized", result.stderr)

    def test_subscript_is_not_checked(self):
        # a(3,1) is outside 1:2 in dimension 1 but inside the array's storage,
        # where it is a(1,2).
        text = """program p
  real(8) :: a(2,2)
  integer :: i
  a = reshape([1d0, 2d0, 3d0, 4d0], [2,2])
  read *, i
  print '(f4.1)', a(i,1)
end program
"""
        checked = build_and_run(text)
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("subscript outside 1:2", checked.stdout + checked.stderr)
        result = build_and_run(text, "-nocheck")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, " 3.0\n")

    def test_defined_programs_give_the_same_output(self):
        text = """module store
  implicit none
  real(8) :: totals(3)
  character(len=:), allocatable :: log
contains
  subroutine fill(v, k)
    real(8), intent(out) :: v(:)
    integer, intent(in) :: k
    integer :: i
    do i = 1, size(v)
      v(i) = i * k
    end do
  end subroutine
end module
program p
  use store
  implicit none
  real(8), allocatable :: a(:), b(:), m(:,:)
  character(len=4) :: words(3)
  integer :: i, j, n
  read *, n
  allocate(a(n), m(n, n+1))
  call fill(a, 2)
  do j = 1, n + 1
    do i = 1, n
      m(i,j) = i - j
    end do
  end do
  call move_alloc(a, b)
  totals = [sum(b), sum(m), maxval(m)]
  words = ['ab  ', 'cd  ', 'ef  ']
  words(2:3) = words(1:2)
  log = trim(words(1)) // trim(words(3))
  print '(3f8.1)', totals
  print '(l2,i3,1x,a)', allocated(a), size(b), log
  print '(4a)', words, '|'
end program
"""
        checked = build_and_run(text)
        self.assertEqual(checked.returncode, 0, checked.stderr)
        for flags in (["-nocheck"], ["-fast"]):
            with self.subTest(flags=flags):
                result = build_and_run(text, *flags)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, checked.stdout)

    def test_fast_conflicts_with_an_explicit_level(self):
        with tempfile.TemporaryDirectory(prefix="nocheck-") as tmp:
            source = Path(tmp) / "example.f90"
            source.write_text("program p\nend program\n", encoding="utf-8")
            result = subprocess.run([COMPILER, "-fast", "-O2", source, "-o", Path(tmp) / "example.exe"],
                                    capture_output=True, text=True, timeout=60)
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()

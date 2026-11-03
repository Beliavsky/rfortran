"""-Wmemory-leak, on by default, warns when storage allocated through a local
POINTER of a procedure is lost without DEALLOCATE: when the procedure returns,
or when ALLOCATE, NULLIFY or `=>` reassigns the pointer. A leak on every path
"is not deallocated"; on some paths it "may not be deallocated".

Pointers that might keep their storage elsewhere (passed to a procedure, the
target of `=>`, function results), SAVE pointers, `=> NULL()` initialized
pointers, ALLOCATE with STAT= and the ASSOCIATED-guarded DEALLOCATE idiom are
not reported.

Run after cargo build --bin rfort: python tests/test_memory_leak.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

from test_record_read import COMPILER

CASES = '''module m
implicit none
contains
subroutine never(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  allocate(x(n))
  x = 1
  print *, sum(x)
end subroutine
subroutine fine(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  allocate(x(n))
  x = 1
  if (associated(x)) deallocate(x)
end subroutine
subroutine early(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  allocate(x(n))
  if (n > 3) return
  deallocate(x)
end subroutine
subroutine again(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  integer :: i
  do i = 1, 3
    allocate(x(n))
    x = i
  end do
  deallocate(x)
end subroutine
subroutine lost(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  real, target :: t(3)
  allocate(x(n))
  x => t
end subroutine
subroutine kept(n, y)
  integer, intent(in) :: n
  real, pointer :: x(:), y(:)
  allocate(x(n))
  y => x
end subroutine
subroutine handed(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  allocate(x(n))
  call store(x)
end subroutine
subroutine store(v)
  real, pointer :: v(:)
  deallocate(v)
end subroutine
subroutine cached(n)
  integer, intent(in) :: n
  real, pointer :: x(:) => null()
  if (.not. associated(x)) allocate(x(n))
end subroutine
subroutine status(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  integer :: ierr
  allocate(x(n), stat=ierr)
  if (ierr /= 0) return
  deallocate(x)
end subroutine
function make(n) result(r)
  integer, intent(in) :: n
  real, pointer :: r(:)
  allocate(r(n))
end function
end module
program p
use m
call never(2); call fine(2); call early(2); call again(2); call lost(2)
end program
'''

CONDITIONAL = """subroutine mean(n)
  integer, intent(in) :: n
  real, pointer :: x(:)
  allocate (x(n))
  call random_number(x)
  print *, sum(x) / n
  if (x(1) > 0.5) deallocate(x)
end subroutine
"""


def check(source_text, *flags):
    with tempfile.TemporaryDirectory(prefix="memory-leak-") as tmp:
        source = Path(tmp) / "example.f90"
        source.write_text(source_text, encoding="utf-8")
        return subprocess.run([COMPILER, *flags, "check", source], cwd=tmp,
                              capture_output=True, text=True, timeout=120)


def warnings(stderr):
    """(message, line of the allocation) for each warning."""
    out = []
    for block in stderr.split("warning: ")[1:]:
        message = block.splitlines()[0]
        line = re.search(r"example\.f90:(\d+):", block)
        out.append((message, int(line.group(1)) if line else None))
    return out


class MemoryLeak(unittest.TestCase):
    def test_cases(self):
        result = check(CASES)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(warnings(result.stderr), [
            ("POINTER `x` is not deallocated before `never` returns, so its storage leaks [memory-leak]", 7),
            ("POINTER `x` may not be deallocated before `early` returns, so its storage may leak [memory-leak]", 21),
            ("POINTER `x` may not be deallocated before the pointer is reassigned, so its storage may leak [memory-leak]", 30),
            ("POINTER `x` is not deallocated before the pointer is reassigned, so its storage leaks [memory-leak]", 39),
        ])
        self.assertIn("returns with `x` still allocated", result.stderr)
        self.assertIn("then again on the next iteration without DEALLOCATE", result.stderr)
        self.assertIn("reassigned here", result.stderr)

    def test_conditional_deallocate_names_the_condition(self):
        result = check(CONDITIONAL)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(warnings(result.stderr), [
            ("POINTER `x` may not be deallocated before `mean` returns, so its storage may leak [memory-leak]", 4)])
        self.assertIn("when this condition is false, `x` is still allocated", result.stderr)
        self.assertIn("declare `x` ALLOCATABLE instead of POINTER", result.stderr)

    def test_allocatable_suggestion_only_when_it_ends_the_leak(self):
        result = check(CASES)
        notes = {line: block for block in result.stderr.split("warning: ")[1:]
                 for line in [int(re.search(r"example\.f90:(\d+):", block).group(1))]}
        # never and early: leaks at returns only. again: a leak at ALLOCATE in
        # a loop. lost: `=>` uses x as a pointer.
        self.assertEqual({line: "ALLOCATABLE" in block for line, block in notes.items()},
                         {7: True, 21: True, 30: False, 39: False})

    def test_disable_and_promote(self):
        self.assertNotIn("memory-leak", check(CONDITIONAL, "-Wno-memory-leak").stderr)
        self.assertNotIn("memory-leak", check("!#allow(memory-leak)\n" + CONDITIONAL).stderr)
        result = check(CONDITIONAL, "-Werror=memory-leak")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("error: POINTER `x` may not be deallocated", result.stderr)


if __name__ == "__main__":
    unittest.main()

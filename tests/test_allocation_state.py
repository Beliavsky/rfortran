"""-Wallocation-state, on by default, warns about DEALLOCATE of a local
ALLOCATABLE that is unallocated on every path reaching it, and ALLOCATE of one
that is allocated on every path, without STAT=. Both are run-time errors.

States that depend on a condition the analysis cannot follow, STAT=, the
IF (ALLOCATED(x)) idiom and variables passed to a procedure are not reported.

Run after cargo build --bin rfort: python tests/test_allocation_state.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

from test_memory_leak import check, warnings

CASES = '''module m
implicit none
contains
subroutine twice(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  allocate(x(n))
  deallocate(x)
  deallocate(x)
end subroutine
subroutine realloc(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  x = [1.0, 2.0]
  allocate(x(n))
end subroutine
subroutine guarded(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  integer :: i
  do i = 1, 3
    if (allocated(x)) deallocate(x)
    allocate(x(n))
  end do
  if (i > 1) deallocate(x)
  allocate(x(n), stat=i)
end subroutine
subroutine loop(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  integer :: i
  do i = 1, n
    if (i > 1) deallocate(x)
    allocate(x(n))
  end do
end subroutine
subroutine maybe(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  if (n > 0) allocate(x(n))
  if (n > 0) deallocate(x)
end subroutine
subroutine moved(n)
  integer, intent(in) :: n
  real, allocatable :: x(:), y(:)
  allocate(x(n))
  call move_alloc(x, y)
  deallocate(x)
  deallocate(y)
end subroutine
subroutine passed(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  call make(x, n)
  deallocate(x)
end subroutine
subroutine make(x, n)
  real, allocatable, intent(out) :: x(:)
  integer, intent(in) :: n
  allocate(x(n))
end subroutine
subroutine tested(n)
  integer, intent(in) :: n
  real, allocatable :: x(:)
  call make(x, n)
  if (.not. allocated(x)) then
    deallocate(x)
  end if
end subroutine
end module
'''

MAIN = """implicit none
real, allocatable :: x, y(:)
allocate (x, y(3))
allocate (x, y(3))
deallocate (x)
deallocate (x)
end
"""


class AllocationState(unittest.TestCase):
    def test_cases(self):
        result = check(CASES)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(warnings(result.stderr), [
            ("DEALLOCATE of `x`, which is not allocated, is an error at run time [allocation-state]", 9),
            ("ALLOCATE of `x`, which is already allocated, is an error at run time [allocation-state]", 15),
            ("DEALLOCATE of `x`, which is not allocated, is an error at run time [allocation-state]", 48),
            ("DEALLOCATE of `x`, which is not allocated, is an error at run time [allocation-state]", 67),
        ])
        for label in ["deallocated here", "allocated by this assignment",
                      "moved away by MOVE_ALLOC here", "known to be unallocated from this test"]:
            self.assertIn(label, result.stderr)

    def test_main_program(self):
        result = check(MAIN)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(warnings(result.stderr), [
            ("ALLOCATE of `x`, which is already allocated, is an error at run time [allocation-state]", 4),
            ("ALLOCATE of `y`, which is already allocated, is an error at run time [allocation-state]", 4),
            ("DEALLOCATE of `x`, which is not allocated, is an error at run time [allocation-state]", 6),
        ])
        self.assertIn("declared here, unallocated until ALLOCATE",
                      check("real, allocatable :: x(:)\ndeallocate(x)\nend\n").stderr)

    def test_disable_and_promote(self):
        self.assertNotIn("allocation-state", check(MAIN, "-Wno-allocation-state").stderr)
        self.assertNotIn("allocation-state", check("!#allow(allocation-state)\n" + MAIN).stderr)
        result = check(MAIN, "-Werror=allocation-state")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("error: ALLOCATE of `x`", result.stderr)


if __name__ == "__main__":
    unittest.main()

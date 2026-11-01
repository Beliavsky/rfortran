"""Functions that define components of their derived-type dummy arguments:
the changes reach the actual argument, for scalar and array components and
for module and internal functions, as for intrinsic scalar dummies.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_function_record_arguments.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class FunctionRecordArguments(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_module_functions(self):
        self.check("""module m
  type st
    integer :: top = 0
  end type
  type acc
    integer :: n = 0
    real :: sums(2) = 0
  end type
contains
  function pop(s) result(v)
    type(st), intent(inout) :: s
    integer :: v
    v = s%top
    s%top = s%top - 1
  end function
  function add(a, x) result(total)
    type(acc), intent(inout) :: a
    real, intent(in) :: x
    real :: total
    a%n = a%n + 1
    a%sums(1) = a%sums(1) + x
    total = a%sums(1)
  end function
  function bump(k) result(v)
    integer, intent(inout) :: k
    integer :: v
    k = k + 1
    v = k
  end function
end module
program p
  use m
  type(st) :: s
  type(acc) :: a
  integer :: x, y, k
  real :: t
  s%top = 3
  x = pop(s)
  y = pop(s)
  print *, x, y, s%top
  t = add(a, 2.0)
  t = add(a, 3.0)
  print *, t, a%n, a%sums
  k = 10
  x = bump(k)
  print *, x, k
end program
""", "3 2 1\n5.0 2 5.0 0.0\n11 11\n")

    def test_internal_function(self):
        self.check("""program p
  type st
    integer :: top = 0
    real :: w(2)
  end type
  type(st) :: s
  integer :: a
  s%top = 3
  s%w = 1
  a = pop(s)
  print *, a, s%top, s%w
contains
  function pop(s) result(v)
    type(st), intent(inout) :: s
    integer :: v
    v = s%top
    s%top = s%top - 1
    s%w(2) = 5
  end function
end program
""", "3 2 1.0 5.0\n")


if __name__ == "__main__":
    unittest.main()

"""ALLOCATABLE scalar components: intrinsic scalars, deferred-length
CHARACTER scalars and scalars of another derived type, in scalar objects and
arrays of the type: allocation by ALLOCATE and by assignment, reallocation,
ALLOCATED, DEALLOCATE, substrings, structure constructors, procedure
arguments and nested components. Also deferred-length CHARACTER array
components of arrays of records, which keep their length.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_allocatable_scalar_components.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class AllocatableScalarComponents(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_intrinsic_scalars(self):
        self.check("""program p
  type t
    real, allocatable :: r
    integer, allocatable :: k
    integer :: n = 0
  end type
  type(t) :: x, y
  print *, allocated(x%r), allocated(x%k)
  allocate(x%r)
  x%r = 2.5
  x%k = 7
  print *, allocated(x%r), x%r, x%k, x%r * x%k
  y = x
  y%r = y%r + 1
  print *, x%r, y%r, allocated(y%k)
  deallocate(x%r)
  print *, allocated(x%r), allocated(y%r)
end program
""", "F F\nT 2.5 7 17.5\n2.5 3.5 T\nF T\n")

    def test_deferred_length_character(self):
        self.check("""program p
  type t
    character(len=:), allocatable :: s
  end type
  type(t) :: x
  integer :: n
  x%s = 'hello'
  x%s = x%s // ' world'
  print *, len(x%s), x%s, ' ', x%s(1:4)
  n = 3
  deallocate(x%s)
  allocate(character(len=n) :: x%s)
  x%s = 'abcdef'
  print *, len(x%s), x%s
end program
""", "11 hello world hell\n6 abcdef\n")

    def test_derived_scalar_component(self):
        self.check("""program p
  type u
    integer :: k
    real :: w(2)
  end type
  type t
    type(u), allocatable :: c
  end type
  type(t) :: x
  print *, allocated(x%c)
  allocate(x%c)
  x%c%k = 3
  x%c%w = [1.5, 2.5]
  print *, allocated(x%c), x%c%k, sum(x%c%w)
  x%c = u(4, [0.5, 0.5])
  print *, x%c%k, x%c%w
end program
""", "F\nT 3 4.0\n4 0.5 0.5\n")

    def test_arrays_of_records(self):
        self.check("""program p
  type t
    character(len=:), allocatable :: s
    real, allocatable :: r
  end type
  type(t) :: x(3)
  integer i
  x(1)%s = 'a'
  x(2)%s = 'bb'
  x(3)%s = 'ccc'
  do i = 1, 3
    x(i)%r = 10.0 * i
    print *, len(x(i)%s), x(i)%s, x(i)%r
  end do
  print *, allocated(x(2)%s), x(3)%s
end program
""", "1 a 10.0\n2 bb 20.0\n3 ccc 30.0\nT ccc\n")

    def test_constructors_and_arguments(self):
        self.check("""module m
  type t
    character(len=:), allocatable :: name
    integer, allocatable :: count
  end type
contains
  subroutine show(x)
    type(t), intent(in) :: x
    print *, x%name, len(x%name), x%count
  end subroutine
  subroutine bump(x)
    type(t), intent(inout) :: x
    x%count = x%count + 1
    x%name = x%name // '!'
  end subroutine
end module
program p
  use m
  type(t) :: a
  a = t('apple', 3)
  call show(a)
  call bump(a)
  call show(a)
end program
""", "apple 5 3\napple! 6 4\n")

    def test_character_array_components_of_record_arrays(self):
        self.check("""program p
  type t
    character(len=:), allocatable :: s(:)
  end type
  type(t) :: x(3)
  x(2)%s = ['bb', 'cc']
  print *, size(x(2)%s), len(x(2)%s), x(2)%s(2)
  allocate(character(len=4) :: x(3)%s(1))
  x(3)%s(1) = 'dddd'
  print *, len(x(3)%s), x(3)%s(1)
end program
""", "2 2 cc\n4 dddd\n")


if __name__ == "__main__":
    unittest.main()

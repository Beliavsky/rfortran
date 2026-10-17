"""Automatic arrays: procedure locals whose bounds depend on dummy arguments.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_automatic_arrays.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class AutomaticArrays(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_scalar_dummy_bounds(self):
        self.check("""program main
implicit none
call fill(3)
call fill(5)
call fill(0)
contains
subroutine fill(n)
  integer, intent(in) :: n
  integer :: w(n)
  real(8) :: r(0:n)
  logical :: f(n)
  integer :: i
  do i = 1, n
    w(i) = 10*i
  end do
  r = 0.5d0
  f = w > 20
  print *, size(w), lbound(r), ubound(r), sum(w), sum(r), count(f)
  print *, w
end subroutine fill
end program
""", "3 0 3 60 2.0 1\n10 20 30\n5 0 5 150 3.0 3\n10 20 30 40 50\n0 0 0 0 0.5 0\n\n")

    def test_rank_two_function_and_module(self):
        self.check("""module m
implicit none
contains
integer function trace_of(n)
  integer, intent(in) :: n
  integer :: a(n, n), i, j
  do j = 1, n
    do i = 1, n
      a(i, j) = i + 10*j
    end do
  end do
  trace_of = 0
  do i = 1, n
    trace_of = trace_of + a(i, i)
  end do
end function trace_of
subroutine show(m, n)
  integer, intent(in) :: m, n
  real :: b(m, 2*n)
  b = 1.0
  b(m, 2*n) = 7.0
  print *, shape(b), sum(b)
end subroutine show
end module m
program main
use m
implicit none
print *, trace_of(2), trace_of(3)
call show(2, 2)
end program
""", "33 66\n2 4 14.0\n")

    def test_character_arrays(self):
        self.check("""module m
implicit none
contains
subroutine show(c)
  character(len=*), intent(in) :: c(:)
  print *, size(c), c
end subroutine show
subroutine s(n)
  integer, intent(in) :: n
  character(len=4) :: names(n)
  integer :: i
  names = 'ab'
  do i = 1, n
    names(i) = 'ab' // achar(48 + i)
  end do
  names(n) = trim(names(n)) // 'z'
  print *, size(names), len(names), count(names == 'ab1'), names(1)(2:3)
  call show(names)
end subroutine s
end module m
program main
use m
implicit none
call s(3)
call s(1)
end program
""", "3 4 1 b1\n3 ab1 ab2 ab3z\n1 4 0 b1\n1 ab1z\n")

    def test_derived_type_arrays(self):
        self.check("""module m
implicit none
type :: point
  integer :: x = 0
  real :: y = 1.5
end type point
contains
subroutine s(n)
  integer, intent(in) :: n
  type(point) :: p(n), q(0:n)
  integer :: i
  do i = 1, n
    p(i)%x = 10*i
  end do
  q%x = 1
  print *, size(p), p%x, sum(p%y), lbound(q), sum(q%x)
end subroutine s
end module m
program main
use m
implicit none
call s(2)
call s(4)
end program
""", "2 10 20 3.0 0 3\n4 10 20 30 40 6.0 0 5\n")

    def test_recursion_keeps_separate_arrays(self):
        self.check("""program main
implicit none
call down(3)
contains
recursive subroutine down(n)
  integer, intent(in) :: n
  integer :: w(n)
  w = n
  if (n > 1) call down(n - 1)
  print *, n, sum(w)
end subroutine down
end program
""", "1 1\n2 4\n3 9\n")


if __name__ == "__main__":
    unittest.main()

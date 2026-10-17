"""Intrinsic assignment allocates or reallocates whole ALLOCATABLE arrays.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_allocatable_reallocation.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class AllocatableReallocation(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_rank_one_allocation_resize_and_bounds(self):
        self.check("""program main
implicit none
integer, allocatable :: a(:), b(:)
integer :: i
a = [1, 2, 3]
print *, size(a), a
a = [5, 6]
print *, size(a), a
b = a*10
print *, size(b), b
b = [(i*i, i = 1, 4)]
print *, size(b), b
deallocate(a)
allocate(a(-1:1))
a = [7, 8, 9]
b = a
print *, lbound(a), lbound(b), b
a = b(1:-1:-1) + b
print *, lbound(a), a
a = [4]
print *, lbound(a), size(a), a
end program
""", "3 1 2 3\n2 5 6\n2 50 60\n4 1 4 9 16\n-1 -1 7 8 9\n-1 16 16 16\n1 1 4\n")

    def test_rank_two_allocation_from_arrays_and_sections(self):
        self.check("""program main
implicit none
integer, allocatable :: a(:), m(:,:), n(:,:)
allocate(m(2:3,0:2))
m = 4
m(2,1) = 7
n = m
print *, lbound(n), ubound(n), sum(n)
n = m(:,1:2)
print *, shape(n), lbound(n), n
end program
""", "2 0 3 2 27\n2 2 1 1 7 4 4 4\n")

    def test_procedure_dummies_and_logical_results(self):
        self.check("""module util
implicit none
contains
subroutine squares(x, s)
real(8), intent(in) :: x(:)
real(8), allocatable, intent(out) :: s(:)
s = x**2
end subroutine squares
subroutine evens(x, e)
integer, intent(in) :: x(:)
integer, allocatable, intent(inout) :: e(:)
e = pack(x, mod(x, 2) == 0)
end subroutine evens
end module util
program main
use util
implicit none
real(8), allocatable :: s(:)
integer, allocatable :: e(:)
logical, allocatable :: f(:)
call squares([1d0, 2d0, 3d0], s)
print *, size(s), s
call squares([5d0], s)
print *, size(s), s
call evens([1, 2, 3, 4, 6], e)
print *, size(e), e
call evens([5, 8], e)
print *, size(e), e
f = s > 2d0
print *, size(f), f
end program
""", "3 1.0 4.0 9.0\n1 25.0\n3 2 4 6\n1 8\n1 T\n")

    def test_assigned_local_in_procedure_with_array_dummy(self):
        self.check("""module u
implicit none
contains
subroutine s(x)
integer, intent(in) :: x(:)
integer, allocatable :: t(:)
t = x*2
print *, size(t), t
if (size(x) > 2) t = x(1:2)
print *, size(t), t
t = pack(x, x > 1)
print *, size(t), t
end subroutine s
end module u
program main
use u
implicit none
call s([1, 2, 3])
call s([5])
end program
""", "3 2 4 6\n2 1 2\n2 2 3\n1 10\n1 10\n1 5\n")


if __name__ == "__main__":
    unittest.main()

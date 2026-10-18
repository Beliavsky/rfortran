"""Intrinsic assignment allocates or reallocates whole ALLOCATABLE arrays.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_allocatable_reallocation.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class AllocatableReallocation(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_character_constructor_implicit_allocation(self):
        self.check("""implicit none
character(len=2),allocatable :: s(:)
s=["ab","cd","e "]
print "(*(a4))",s
print *,allocated(s),size(s),len(s)
end
""", "  ab  cd  e \nT 3 2\n")

    def test_character_resize_overlap_bounds_and_element_length(self):
        self.check("""implicit none
character(len=2),allocatable :: s(:),t(:)
allocate(s(-1:1))
s=['ab','cd','ef']
print *,lbound(s),ubound(s)
t=s
print *,lbound(t),ubound(t)
print '(*(a))',t
s=s(1:-1:-1)
print '(*(a))',s
s=s(0:1)
print *,lbound(s),size(s)
print '(*(a))',s
s=[character(3)::'XYZ']
print *,size(s),len(s)
print '(*(a))',s
s=[character(2)::]
print *,allocated(s),size(s),len(s)
s=['gh','ij']
s='q'
print '(*(a))',s
deallocate(s,t)
print *,allocated(s),allocated(t)
end
""", "-1 1\n-1 1\nabcdef\nefcdab\n1 2\ncdab\n1 2\nXY\nT 0 2\nq q \nF F\n")

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

    def test_append_with_runtime_constructors(self):
        self.check("""program t
implicit none
integer, allocatable :: a(:), b(:)
integer :: i
a = [1, 2]
a = [a, 3]
print *, size(a), a
do i = 4, 6
  a = [a, i]
end do
print *, size(a), a
b = [0, a, a(1:2)]
print *, size(b), b
end program
""", '3 1 2 3\n6 1 2 3 4 5 6\n9 0 1 2 3 4 5 6 1 2\n')

    def test_append_to_allocatable_dummy(self):
        self.check("""module stack
implicit none
contains
subroutine push(v, x)
  real(8), allocatable, intent(inout) :: v(:)
  real(8), intent(in) :: x
  if (.not. allocated(v)) then
    v = [x]
  else
    v = [v, x]
  end if
end subroutine push
end module stack
program ap2
use stack
implicit none
real(8), allocatable :: v(:)
integer :: i
do i = 1, 5
  call push(v, 0.5d0*i)
end do
print *, size(v), v
end program ap2
""", '5 0.5 1.0 1.5 2.0 2.5\n')

    def test_constructors_with_runtime_pack(self):
        self.check("""program main
implicit none
integer :: a(3)
logical :: mask(3)
integer, allocatable :: b(:)
a = [1, 2, 3]
mask = [.true., .false., .true.]
print *, sum([pack(a, mask)]), sum([0, pack(a, mask), 10])
b = [pack(a, mask), a]
print *, size(b), b
end program
""", '4 14\n5 1 3 1 2 3\n')


if __name__ == "__main__":
    unittest.main()

"""Polymorphic ALLOCATABLE objects: CLASS(*) objects of any rank and CLASS(t)
arrays, held as a type tag and one ordinary ALLOCATABLE per dynamic type.

Covers ALLOCATE with a type specification, SOURCE= and MOLD=, DEALLOCATE,
ALLOCATED, STAT=, MOVE_ALLOC, assignment, SELECT TYPE with array selectors,
type-bound calls on elements, ALLOCATABLE CLASS(*) dummy arguments
(INTENT(OUT) included), and ALLOCATABLE CLASS(*) assumed-rank dummies.
Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.

Run after cargo build --bin rfort: python tests/test_polymorphic_allocatables.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate

ASSUMED_RANK = '''module alloc_utils
   implicit none
contains
   integer function alloc_size(x) result(n)
      class(*), allocatable, intent(in) :: x(..)

      n = -1
      if (.not. allocated(x)) return

      select rank (x)
      rank (0)
         n = 1
      rank default
         n = size(x)
      end select
   end function alloc_size
end module alloc_utils

program test_alloc_size
   use alloc_utils, only: alloc_size
   implicit none
   class(*), allocatable :: scalar
   class(*), allocatable :: vector(:)
   class(*), allocatable :: matrix(:,:)

   call check(alloc_size(scalar), -1, 'unallocated scalar')
   call check(alloc_size(vector), -1, 'unallocated vector')
   call check(alloc_size(matrix), -1, 'unallocated matrix')

   allocate(scalar, source=42)
   call check(alloc_size(scalar), 1, 'allocated scalar')

   allocate(vector, source=[1.0, 2.0, 3.0])
   call check(alloc_size(vector), 3, 'rank-1 array')
   deallocate(vector)

   allocate(vector, source=[real ::])
   call check(alloc_size(vector), 0, 'zero-sized array')

   allocate(matrix, source=reshape([1, 2, 3, 4, 5, 6], [2, 3]))
   call check(alloc_size(matrix), 6, 'rank-2 array')

   print *, 'All tests passed.'

contains

   subroutine check(actual, expected, description)
      integer, intent(in) :: actual
      integer, intent(in) :: expected
      character(*), intent(in) :: description

      print '(a,": ",i0)', description, actual
      if (actual /= expected) error stop 'alloc_size test failed'
   end subroutine check

end program test_alloc_size
'''

BROAD = '''module shapes
implicit none
type :: shape
  real :: x = 0
contains
  procedure :: area => shape_area
end type
type, extends(shape) :: circle
  real :: r = 1
contains
  procedure :: area => circle_area
end type
contains
real function shape_area(s)
  class(shape), intent(in) :: s
  shape_area = 0
end function
real function circle_area(s)
  class(circle), intent(in) :: s
  circle_area = 3 * s%r**2
end function
subroutine fill(a, n)
  class(*), allocatable, intent(out) :: a(:)
  integer, intent(in) :: n
  integer :: i
  allocate(a, source=[(real(i), i = 1, n)])
end subroutine
end module
program t
use shapes
implicit none
class(shape), allocatable :: s(:), u(:)
class(*), allocatable :: a(:), b(:), c
integer :: i, ierr
allocate(circle :: s(3))
print *, size(s), lbound(s), ubound(s)
do i = 1, size(s)
  s(i)%x = i
end do
select type (s)
type is (circle)
  s%r = [1., 2., 3.]
end select
do i = 1, size(s)
  print *, s(i)%area(), s(i)%x
end do
call move_alloc(s, u)
print *, allocated(s), allocated(u), size(u)
select type (q => u)
class is (shape)
  print *, 'shape', size(q)
type is (circle)
  print *, 'circle', q%r
end select
call fill(a, 4)
select type (a)
type is (real)
  print *, 'real', a, sum(a)
type is (integer)
  print *, 'integer', a
end select
b = [1, 2, 3]
select type (b)
type is (integer)
  print *, 'integer', b
end select
allocate(integer :: c, stat=ierr)
print *, ierr /= 0
allocate(real :: c, stat=ierr)
print *, ierr /= 0
deallocate(c)
c = 2.5d0
select type (c)
type is (double precision)
  print *, 'double', c
class default
  print *, 'other'
end select
deallocate(b)
allocate(b(2:5), mold=1.0)
print *, lbound(b), ubound(b)
deallocate(a, b, stat=ierr)
print *, ierr /= 0, allocated(a), allocated(b)
deallocate(a, stat=ierr)
print *, ierr /= 0
end program
'''

RANK_TWO = '''program p
class(*), allocatable :: m(:,:), k(:,:)
m = reshape([1, 2, 3, 4, 5, 6], [2, 3])
k = m
m = reshape([1.5, 2.5], [1, 2])
select type (k)
type is (integer)
  print *, shape(k), k
end select
select type (m)
type is (real)
  print *, shape(m), m
end select
end
'''


class PolymorphicAllocatables(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_assumed_rank_dummy(self):
        self.check(ASSUMED_RANK, "unallocated scalar: -1\nunallocated vector: -1\nunallocated matrix: -1\n"
                   "allocated scalar: 1\nrank-1 array: 3\nzero-sized array: 0\nrank-2 array: 6\n All tests passed.\n")

    def test_class_arrays_and_unlimited_objects(self):
        self.check(BROAD, " 3 1 3\n 3.0 1.0\n 12.0 2.0\n 27.0 3.0\n F T 3\n circle 1.0 2.0 3.0\n"
                   " real 1.0 2.0 3.0 4.0 10.0\n integer 1 2 3\n F\n T\n double 2.5\n 2 5\n F F F\n T\n")

    def test_rank_two_assignment(self):
        self.check(RANK_TWO, " 2 3 1 2 3 4 5 6\n 1 2 1.5 2.5\n")

    def test_unallocated_and_allocated_errors(self):
        program = "program p\nclass(*), allocatable :: a(:)\n{}\nend\n"
        self.check(program.format("deallocate(a)"), error="DEALLOCATE of unallocated polymorphic object `a`")
        self.check(program.format("allocate(a, source=[1])\nallocate(a, source=[2])"),
                   error="ALLOCATE of already allocated polymorphic object `a`")
        self.check(program.format("select type (a)\ntype is (integer)\nend select"),
                   error="SELECT TYPE on unallocated polymorphic object `a`")

    def test_nonallocatable_assumed_rank_is_rejected(self):
        self.reject("""module m
implicit none
contains
integer function f(x) result(n)
  class(*), intent(in) :: x(..)
  n = rank(x)
end function
end module
""", "polymorphic assumed-rank dummies that are not ALLOCATABLE are not yet supported")


if __name__ == "__main__":
    unittest.main()

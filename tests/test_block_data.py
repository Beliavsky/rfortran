"""BLOCK DATA: named and unnamed units with each END form, initial values from
DATA and declarations for named and blank COMMON, implicit types and bounds
in COMMON, blocks with storage (different layouts and EQUIVALENCE), modules
used by BLOCK DATA, and DATA of array sections and vector subscripts.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_block_data.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class BlockData(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_units_and_end_forms(self):
        # Several units, with implicit types and bounds in COMMON.
        self.check("""block data
 common /a/ y(3)
 data y /3*1./
end

blockdata d1
 common /a/ w(3)
 common /b/ u
 data u /2./
end blockdata d1

block data d2
 common /c/ n, s
 character(len=4) s
 integer, parameter :: m = 7
 data n, s /m, 'abcd'/
end block data d2

common /a/ x(3)
common /b/ y
common /c/ k, t
character(len=4) t
print *, x, y
print *, k, t
end
""", "1.0 1.0 1.0 2.0\n7 abcd\n", std="legacy")

    def test_initialized_declarations(self):
        self.check("""block data names
  implicit none
  integer, parameter :: nmin = 2
  character(len=3) :: name(nmin) = (/'bar', 'baz'/)
  integer :: counts(nmin) = 5
  logical :: flag = .true.
  common /nmstr/ name, counts, flag
end block data names

program p
  implicit none
  character(len=3) :: s(2)
  integer :: c(2)
  logical :: f
  common /nmstr/ s, c, f
  print *, s(2), ' ', s(1), sum(c), f
  call bump
  print *, c
end program

subroutine bump
  integer :: c(2)
  character(len=3) :: s(2)
  logical :: f
  common /nmstr/ s, c, f
  c(1) = c(1) + 1
end subroutine
""", "baz bar 10 T\n6 5\n")

    def test_storage_and_equivalence(self):
        # EQUIVALENCE in BLOCK DATA gives the block storage; its objects and
        # cb share the initial values. /two/ has different layouts.
        self.check("""block data
  common /global/ ca(4)
  integer(4) ca, cb
  equivalence (cb, ca(3))
  data (ca(i), i = 1, 2) /42, 43/, ca(4) /44/
  data cb /99/
  real r(4)
  common /two/ r
  data r /1., 2., 3., 4./
end block data

program p
  call check
  call halves
end program

subroutine check
  common /global/ ca(4)
  integer(4) ca
  print *, ca
end subroutine

subroutine halves
  real lo(2), hi(2)
  common /two/ lo, hi
  print *, hi, lo
end subroutine
""", "42 43 99 44\n3.0 4.0 1.0 2.0\n", std="legacy")

    def test_use_in_block_data(self):
        self.check("""module m
  implicit none
  integer, parameter :: n = 5
contains
  subroutine show
    integer vals
    common /block1/ vals(n)
    print *, vals
  end subroutine
end module

block data blkdat
  use m, only: n
  integer vals
  common /block1/ vals(n)
  data vals /1, 2, 3, 4, 5/
end block data blkdat

program main
  use m, only: show
  implicit none
  call show
end program
""", "1 2 3 4 5\n")

    def test_data_sections_and_vector_subscripts(self):
        self.check("""block data
  implicit none
  integer :: a(8), b(3,2), i
  data a(::2)   /4*1/
  data a([2,6]) /2*2/
  data a([4])   /3/
  data a([(6+2*i,i=1,1)]) /1*5/
  data b( 1   ,[1,2]) /11,12/
  data b([2,3],[2,1]) /22,32,21,31/
  common /com/ a, b
end block data

program test
  implicit none
  integer :: a(8), b(3,2)
  common /com/ a, b
  integer :: c(2,2)
  data c(:,2) /3, 4/, c(2:1:-1,1) /2, 1/
  print *, a
  print *, b
  print *, c
end program test
""", "1 2 1 3 1 2 1 5\n11 21 31 12 22 32\n1 2 3 4\n")

    def test_equivalence_with_element_data(self):
        # DATA leaves a(3) zero; b, sharing it, gives it its value.
        self.check("""program p
  call int4
contains
  subroutine int4
    integer(4) a(4)
    integer(4) b
    equivalence (b, a(3))
    data b /3/
    data (a(i), i=1,2) /1,2/, a(4) /4/
    print *, a
  end subroutine
end program
""", "1 2 3 4\n", std="legacy")

    def test_module_rank_two_initialization(self):
        self.check("""module m
  integer :: b(3,2)
  data b /1, 2, 3, 4, 5, 6/
  integer :: c(2,2) = reshape([7, 8, 9, 10], [2, 2])
end module
program p
  use m
  print *, b(3,1), b(1,2), c(2,1), c(1,2)
end program
""", "3 4 8 9\n")

    def test_rejections(self):
        self.reject("""block data one
  common /c/ x
  data x /1./
end block data
block data two
  common /c/ y
  data y /2./
end block data
common /c/ z
print *, z
end
""", "`y` and `x`, the same object of COMMON block /c/, are both initialized")


if __name__ == "__main__":
    unittest.main()

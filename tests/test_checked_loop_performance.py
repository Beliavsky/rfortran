"""Optimizations of checked array loops keep their checks where they matter.

A DO nest that assigns every element of an array defines the whole array, so
later reads need no initialization checks; a partial nest, or one that may
EXIT, does not. Subscripts proven in range by constant DO bounds are not
tested at run time; others still are. A whole-array assignment whose right
side cannot overlap its destination is assigned without a temporary.
MATMUL of moderately large arrays compiles without unrolling its loops.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --release: python tests/test_checked_loop_performance.py
"""
import unittest

import test_deallocate


class CheckedLoopPerformance(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_full_cover_defines_the_array(self):
        self.check("""program p
  implicit none
  real(8) :: a(4,3), b(3,4), v(12)
  integer :: i, j
  do j = 1, 3
    do i = 1, 4
      a(i,j) = i + 10*j
      b(j,i) = a(i,j)
    end do
  end do
  do i = 1, 12
    v(i) = 2*i
  end do
  print '(3f6.0)', a(4,3), b(3,4), sum(v)
end program
""", "   34.   34.  156.\n")

    def test_partial_cover_still_checked(self):
        self.check("""program p
  real(8) :: a(10)
  integer :: i
  do i = 1, 9
    a(i) = i
  end do
  print *, sum(a)
end program
""", error="read of uninitialized variable `a`: element 10")
        self.check("""program p
  real(8) :: a(4,3)
  integer :: i, j
  do j = 1, 3
    do i = 1, 3
      a(i,j) = i
    end do
  end do
  print *, a(4,2)
end program
""", error="read of uninitialized variable `a`: element 8")
        self.check("""program p
  real(8) :: a(10)
  integer :: i
  do i = 1, 10
    if (i == 7) exit
    a(i) = i
  end do
  print *, a(8)
end program
""", error="read of uninitialized variable `a`: element 8")

    def test_subscripts_outside_loop_ranges_still_checked(self):
        self.check("""program p
  real(8) :: a(5,5)
  integer :: i, j
  a = 0
  do j = 1, 5
    do i = 1, 5
      a(i+1,j) = 1
    end do
  end do
  print *, a(1,1)
end program
""", error="dimension 1: subscript outside 1:5")
        self.check("""program p
  real(8) :: a(5,5)
  integer :: i, j, k
  a = 0
  k = 0
  do j = 1, 5
    do i = 1, 5
      a(i,j+k) = 1
      k = 1
    end do
  end do
  print *, a(1,1)
end program
""", error="dimension 2: subscript outside 1:5")
        self.check("""program p
  real(8), allocatable :: a(:,:)
  integer :: i, n
  n = 3
  allocate(a(n,n))
  a = 1
  do i = 1, n
    a(i,i+1) = 2
  end do
  print *, sum(a)
end program
""", error="dimension 2: subscript outside allocated bounds")

    def test_whole_array_assignment_overlap(self):
        self.check("""program p
  implicit none
  real(8) :: x(5), y(5), c(6), a(5), b(5)
  integer :: i
  equivalence (a(1), c(2)), (b(1), c(1))
  x = [(dble(i), i=1,5)]
  y = 2*x
  y = y + 0.25d0*x
  print '(5f7.2)', y
  y = y(5:1:-1) + x
  print '(5f7.2)', y
  c = [(dble(i), i=1,6)]
  a = b + 10
  print '(6f5.0)', c
  c = [(dble(i), i=1,6)]
  b = a * 2
  print '(6f5.0)', c
end program
""", "   2.25   4.50   6.75   9.00  11.25\n"
     "  12.25  11.00   9.75   8.50   7.25\n"
     "   1.  11.  12.  13.  14.  15.\n"
     "   4.   6.   8.  10.  12.   6.\n")

    def test_matmul_of_large_arrays(self):
        self.check("""program p
  implicit none
  integer, parameter :: n = 96
  real(8) :: a(n,n), b(n,n), c(n,n)
  integer :: i, j
  do j = 1, n
    do i = 1, n
      a(i,j) = real(i, 8)
      b(i,j) = real(j, 8)
    end do
  end do
  c = matmul(a, b)
  print '(3f12.1)', c(1,1), c(n,n), sum(c)
end program
""", "        96.0    884736.02081120256.0\n", interpreter=False, compile_timeout=60)


if __name__ == "__main__":
    unittest.main()

"""ASSOCIATE constructs, and ASSOCIATED with a TARGET argument.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_associate.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class Associate(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_selectors(self):
        self.check("""program as
implicit none
type :: pt
  real :: x, y
end type pt
type(pt) :: p
integer :: a(6), i
real :: r
a = [(i, i = 1, 6)]
p = pt(1.0, 2.0)
associate (s => sum(a), v => a, q => p, e => a(2))
  print *, s, v(3), q%x + q%y, e
  v(1) = 10
  q%y = 5.0
  e = 20
end associate
print *, a, p%y
associate (w => a(2:6:2))
  print *, w
  w = 0
end associate
print *, a
r = 2.0
associate (t => r*3.0)
  print *, t
end associate
end program as
""", '21 3 3.0 2\n10 20 3 4 5 6 5.0\n20 4 6\n10 0 3 0 5 0\n6.0\n')

    def test_subscripts_are_evaluated_when_the_block_starts(self):
        # Changing i and j in the block does not move the element or
        # section that the names designate.
        self.check("""program main
implicit none
integer :: a(3), i
a = [1, 2, 3]
i = 1
associate (p => a(i))
   i = 2
   p = 5
end associate
print *, a
end program
""", '5 2 3\n')
        self.check("""program main
implicit none
integer :: a(6), b(3,3), i, j, k
a = [(k, k = 1, 6)]
b = 0
i = 2; j = 4
associate (s => a(i:j), e => b(i, j-1), w => a)
   i = 5; j = 1
   s = -s
   e = 7
   w(i) = 100
   print *, s, e, size(s)
end associate
print *, a
print *, b
do k = 1, 2
   associate (q => a(k))
      q = q * 10
   end associate
end do
print *, a
end program
""", '-2 -3 -4 7 3\n1 -2 -3 -4 100 6\n0 0 0 0 0 0 0 7 0\n10 -20 -3 -4 100 6\n')

    def test_subscripts_of_section_selectors(self):
        # An associate name of a section has lower bounds 1; its elements and
        # sections are those of the array it designates.
        self.check("""program asec
implicit none
real :: a(0:9), b(0:3,4)
integer :: i, j, k
a = [(real(i), i = 0, 9)]
b = reshape([(real(i), i = 1, 16)], [4, 4])
associate (x => a(1:3))
  print *, lbound(x), size(x), x(1), x(3)
  x(2) = -1
  print *, x(1:2), x(2:)
end associate
associate (y => a(::3))
  print *, size(y), y(1), y(2), y(4)
  y(2:3) = 100
  print *, y(:2), y(2::2)
end associate
associate (z => a(4:))
  k = 2
  print *, z(k), z(k+1), z(:2)
end associate
associate (r => b(1,:), c => b(:,2), s => b(1:2, 2:3))
  print *, r(2), c(1), c(4), s(2,2), s(1,:), s(:,2)
  s(1,1) = 0
  r(4) = -4
end associate
print *, a
print *, b
associate (w => a(:))
  do j = 1, size(w)
    w(j) = w(j) * 2
  end do
  print *, w(1), w(10)
end associate
end program
""", """ 1 3 1.0 3.0
 1.0 -1.0 -1.0 3.0
 4 0.0 3.0 9.0
 0.0 100.0 100.0 9.0
 5.0 100.0 4.0 5.0
 6.0 5.0 8.0 11.0 6.0 10.0 10.0 11.0
 0.0 1.0 -1.0 100.0 4.0 5.0 100.0 7.0 8.0 9.0
 1.0 2.0 3.0 4.0 5.0 0.0 7.0 8.0 9.0 10.0 11.0 12.0 13.0 -4.0 15.0 16.0
 0.0 18.0
""")

    def test_vector_subscript_selectors(self):
        # x(j) with x => a(v) is a(v(j)); a vector may be a section, an
        # elemental expression or an array constructor of literals.
        self.check("""program avec
implicit none
real :: a(0:9), b(3,4), t
integer :: v(3), w(0:1), i, k
a = [(real(i), i = 0, 9)]
b = reshape([(real(i), i = 1, 12)], [3, 4])
v = [7, 2, 5]
w = [3, 1]
associate (x => a(v))
  print *, size(x), x(1), x(3), x(2:3)
  k = 2
  t = x(k) + x(k+1)
  print *, t, sum(x), x
end associate
associate (y => a(w))
  print *, y(1), y(2)
end associate
associate (z => a(v(2:3)))
  print *, z(1), z(2)
end associate
associate (r => b(w(0:1), 2:4), c => b(v(2), 2:4))
  print *, r(1,1), r(2,3), r(:,2), c(3)
end associate
associate (s => b([1,3], 2))
  print *, s(2)
end associate
k = 1
associate (q => a(v(k:k+1)))
  print *, q(2)
end associate
end program
""", """ 3 7.0 5.0 2.0 5.0
 7.0 14.0 7.0 2.0 5.0
 3.0 1.0
 2.0 5.0
 6.0 10.0 9.0 7.0 11.0
 6.0
 2.0
""")

    def test_vector_subscripts_of_module_and_host_arrays(self):
        self.check("""module m
implicit none
real :: g(0:5) = [10., 11., 12., 13., 14., 15.]
integer :: gv(2) = [5, 1]
end module m
program aedge
use m
implicit none
real :: b(3,4)
integer :: i, k, j
b = 0
i = 2
associate (r => b(i, :))
  r(2) = 5
  r(3:4) = [6., 7.]
end associate
print *, b(2,:)
associate (x => g(gv))
  print *, x(1), x(2)
end associate
call inner
k = 1
associate (s => b(k:k+1, 2))
  k = 3
  print *, s(2), size(s)
end associate
contains
subroutine inner
associate (y => g(gv(2:2)))
  print *, y(1)
end associate
do j = 1, 2
  associate (z => b(j, (gv + 1) / 2))
    print *, j, z(1), z(2)
  end associate
end do
end subroutine
end program
""", """ 0.0 5.0 6.0 7.0
 15.0 11.0
 11.0
 1 0.0 0.0
 2 6.0 0.0
 5.0 2
""")

    def test_vector_subscript_selector_is_not_definable(self):
        self.reject("""real :: a(5)
integer :: v(2) = [1, 3]
a = 1
associate (x => a(v))
x(1) = 2
end associate
end
""", "selector has a vector subscript cannot be defined")

    def test_expression_selector_modified_in_block_is_rejected(self):
        self.reject("""program main
implicit none
real :: r
r = 1.0
associate (t => r*3.0)
r = 2.0
print *, t
end associate
end program
""", "ASSOCIATE selector that depends on a variable modified")

    def test_associated_with_target(self):
        self.check("""program at
implicit none
real, target :: a(6), b(6), s
real, pointer :: p(:) => null(), q(:) => null(), r => null()
a = 1.0
b = 2.0
print *, associated(p), associated(p, a)
p => a
print *, associated(p, a), associated(p, b), associated(p, a(1:6)), associated(p, a(2:6))
q => a
print *, associated(p, q)
q => a(1:6:2)
print *, associated(p, q), associated(q, a(1:6:2)), associated(q, a(1:5:2))
r => s
print *, associated(r, s), associated(target=s, pointer=r)
end program at
""", 'F F\nT F T F\nT\nF T T\nT T\n')


if __name__ == "__main__":
    unittest.main()

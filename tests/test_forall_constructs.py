"""FORALL constructs (FORALL ... END FORALL), FORALL masks, implicitly typed
FORALL indices and bounds, and TYPE(t) or CLASS(t) declarations without ::.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_forall_constructs.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class ForallConstructs(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_construct_with_nested_masked_forall(self):
        # Each statement runs for every index before the next; the nested
        # FORALL adds its index and mask.
        self.check("""program p
implicit none
integer :: a(3,3), b(3), i, j
a = 0
forall (i=1:3)
  forall (j=1:3, i /= j)
    a(i,j) = i*10+j
  end forall
  b(i) = i
endforall
print *, a, b
end program p
""", "0 21 31 12 0 32 13 23 0 1 2 3\n")

    def test_statements_see_earlier_statements(self):
        self.check("""program p
implicit none
integer :: a(4), b(4), i
a = [1, 2, 3, 4]
forall (i = 2:4)
  a(i) = a(i-1) + 10
  b(i) = a(i)
end forall
b(1) = 0
print *, a
print *, b
end program p
""", "1 11 12 13\n0 11 12 13\n")

    def test_masked_statement_with_runtime_bounds(self):
        # The mask and values use the old elements: the assignment is simultaneous.
        self.check("""program p
implicit none
integer :: a(5), i, n
n = 5
a = [1, -2, 3, -4, 5]
forall (i=2:n, a(i-1) > 0) a(i) = a(i-1)*10
print *, a
end program p
""", "1 10 3 30 5\n")

    def test_masked_sections_and_real_values(self):
        self.check("""program p
implicit none
real :: x(2,3)
integer :: i
x = 1
forall (i = 1:3, mod(i, 2) == 1)
  x(:, i) = real(i) / 2
end forall
print *, x
end program p
""", "0.5 0.5 1.0 1.0 1.5 1.5\n")

    def test_implicitly_typed_index_and_bound(self):
        self.check("""program p
real a(3)
n = 3
forall (i=1:n) a(i) = i
print *, a
end program p
""", "1.0 2.0 3.0\n")

    def test_index_keeps_outer_value(self):
        self.check("""program p
implicit none
integer :: a(4), j
j = 100
forall (j = 1:4)
  a(j) = j
end forall
print *, j, a
end program p
""", "100 1 2 3 4\n")

    def test_derived_declarations_without_colons(self):
        self.check("""module m
implicit none
type tmp
  integer k
end type
contains
subroutine show(x)
class(tmp) x
print *, x%k
end subroutine
end module
program p
use m
implicit none
type (tmp) t1(2), t2
type(tmp) t3
t1%k = 3
t2%k = 4
t3%k = 5
print *, t1%k, t2%k, t3%k
call show(t3)
end program p
""", "3 3 4 5\n5\n")

    def test_rejections(self):
        self.reject("""program p
type tmp
  integer k
end type
type(tmp) t1 = tmp(1)
end program p
""", "an initializer in a TYPE or CLASS declaration requires ::")
        self.reject("""program p
implicit none
type tmp
  integer k
end type
type(tmp) :: t(3)
integer :: i
forall (i=1:3, i > 1) t(i)%k = i
end program p
""", "a FORALL mask with a component assignment is not yet supported")


if __name__ == "__main__":
    unittest.main()

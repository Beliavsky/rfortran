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

    def test_selector_modified_in_block_is_rejected(self):
        self.reject("""program main
implicit none
integer :: a(3), i
a = 1
i = 1
associate (e => a(i))
i = 2
e = 5
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

"""COMMON blocks: named and blank COMMON shared by the main program, external,
internal and module procedures, with different local names, IMPLICIT and
default types, DIMENSION statements, bounds in the COMMON statement, named
constant bounds and several blocks in one statement.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_common_blocks.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class CommonBlocks(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_named_and_blank_common(self):
        self.check("""program p
real b(2)
integer n
common /c1/ b, n
common k, x
b = [1.5, 2.5]
n = 7
k = 3
x = 0.25
call show
call twice
print *, b, n, k, x
call inner
contains
subroutine inner
print *, 'inner', n
end subroutine
end program p
subroutine show
real q(2)
common /c1/ q, m
common kk, y
print *, q, m, kk, y
end subroutine
subroutine twice
integer, parameter :: nb = 2
dimension z(nb)
common /c1/ z, j
z = z * 2
j = j + 1
end subroutine
""", "1.5 2.5 7 3 0.25\n3.0 5.0 8 3 0.25\ninner 8\n")

    def test_implicit_types_and_several_blocks(self):
        # The comma before a following /name/ is optional.
        self.check("""program main
implicit double precision (a-h,o-z)
common /blk/ a, b, i /two/ w(3), // s
a = 1.5d0
b = 2.5d0
i = 4
w = [1d0, 2d0, 3d0]
s = 9
call sub
print *, a + b, i, sum(w), s
end
subroutine sub
implicit double precision (a-h,o-z)
common /blk/ x, y, j, /two/ v(3)
common // t
x = x * 2
j = j * 10
v(2) = 20
t = t + 1
end
""", "5.5 40 24.0 10.0\n")

    def test_module_procedure(self):
        self.check("""module m
contains
subroutine bump
integer counter
common /state/ counter
counter = counter + 1
end subroutine
end module
program p
use m
integer calls
common /state/ calls
calls = 10
call bump
call bump
print *, calls
end program
""", "12\n")

    def test_rejections(self):
        self.reject("""program p
real a(4)
common /c/ a
call s
end program
subroutine s
real x(2), y(2)
common /c/ x, y
end subroutine
""", "COMMON block /c/ has different layouts")
        self.reject("""program p
real a(2), big(4)
common /c/ a
equivalence (a(1), big(1))
end program
""", "EQUIVALENCE of `big` extends `a`, a COMMON object")
        self.reject("""program p
implicit none
common /c/ q
end program
""", "has no type under IMPLICIT NONE")

    def test_variable_named_common(self):
        self.check("""program p
integer :: common
common = 3
print *, common
end program
""", "3\n")


if __name__ == "__main__":
    unittest.main()

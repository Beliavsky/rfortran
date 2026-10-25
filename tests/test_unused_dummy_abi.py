"""Unused dummy declarations still determine native parameter types."""
import unittest
import test_deallocate as helpers

class UnusedDummyABI(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_optional_real_fallback(self):
        self.check("""module m
implicit none
contains
real(kind=kind(1d0)) function choose(x,xopt) result(y)
real(kind=kind(1d0)),intent(in)::x
real(kind=kind(1d0)),intent(in),optional::xopt
if(present(xopt)) then
y=xopt
else
y=x
end if
end function
end module
program main
use m
implicit none
real(kind=kind(1d0))::x
x=-1d0
x=sqrt(x)
print *,choose(x,2d0),choose(1d100,3d0),choose(4d0)
end
""", "2 3 4\n")

    def test_unused_real_dummies(self):
        self.check("""module m
implicit none
contains
integer function answer(x,y)
real,intent(in)::x
real(kind=kind(1d0)),intent(in)::y
answer=42
end function
end module
program main
use m
implicit none
real::x
x=-1
x=sqrt(x)
print *,answer(x,1d100)
end
""", "42\n")

if __name__ == '__main__':
    unittest.main()

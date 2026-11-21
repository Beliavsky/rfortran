"""Derived scalar components selected across arrays retain parent shape/storage."""
import unittest
import test_deallocate
from test_record_read import ROOT

HEADER = """module m
implicit none
type :: date
integer :: year
end type
type :: stamp
type(date)::day
end type
contains
elemental integer function year_number(a)
type(date),intent(in)::a
year_number=a%year
end function
subroutine advance(a)
type(date),intent(inout)::a(:)
a%year=a%year+1
end subroutine
end module
use m
implicit none
type(stamp)::a(3)
type(date)::b(2)
a(1)%day%year=2023
a(2)%day%year=2024
a(3)%day%year=2025
"""

class DerivedComponentViews(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_nested_component_array.f90').read_text(encoding='utf-8'),
                   '2024 2025\n')

    def test_sections_vectors_and_scalar_elements(self):
        self.check(HEADER + """print *,year_number(a%day)
print *,year_number(a(2:)%day)
print *,year_number(a([3,1])%day)
print *,year_number(a(2)%day)
b=a([3,1])%day
print *,b%year
end
""", '2023 2024 2025\n2024 2025\n2025 2023\n2024\n2025 2023\n')

    def test_writable_view_argument(self):
        self.check(HEADER + """call advance(a%day)
print *,year_number(a%day)
call advance(a(1:3:2)%day)
print *,year_number(a%day)
end
""", '2024 2025 2026\n2025 2025 2027\n', reference=False)
        # gfortran -fcheck=all warns about its array temporaries for this
        # writable component view; checked separately with no-array-temps.

    def test_multidimensional_and_deeper_nesting(self):
        self.check("""module m
implicit none
type :: leaf
integer :: n
end type
type :: middle
type(leaf)::child
end type
type :: outer
type(middle)::child
end type
contains
elemental integer function value(a)
type(leaf),intent(in)::a
value=a%n
end function
end module
use m
implicit none
type(outer)::a(2,2)
a(1,1)%child%child%n=1
a(2,1)%child%child%n=2
a(1,2)%child%child%n=3
a(2,2)%child%child%n=4
print *,value(a%child%child)
print *,value(a(:,2)%child%child)
end
""", '1 2 3 4\n3 4\n')

    def test_nested_component_actuals_select_generic_specifics(self):
        # A nested component of an array or section had no known type, so
        # no specific of a generic matched it.
        self.check("""module m
implicit none
type opt
  real :: strike
end type
type rec
  type(opt) :: option
end type
interface nearest
  module procedure nearest_real
end interface
contains
function nearest_real(x, v) result(y)
real, intent(in) :: x, v(:)
real :: y
y = v(minloc(abs(v - x), 1))
end function
end module
program p
use m
implicit none
type(rec) :: xx(4), one
integer :: i
do i = 1, 4
  xx(i)%option%strike = 10.0*i
end do
one%option%strike = 7.0
print *, nearest(23.0, xx(2:4)%option%strike), nearest(31.0, xx%option%strike)
print *, nearest(one%option%strike, [1.0, 6.0, 9.0])
end program
""", '20.0 30.0\n6.0\n', reference=False)
        # gfortran -fcheck=all warns about the array temporaries for these
        # actuals; its output is the same.

if __name__ == '__main__':
    unittest.main()

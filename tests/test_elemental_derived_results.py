"""Derived-valued ELEMENTAL functions in scalar and fixed-vector calls."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module result_records
implicit none
type :: item
integer :: value
logical :: flag
end type
contains
elemental function make(x,increment) result(y)
integer,intent(in)::x
integer,intent(in),optional::increment
type(item)::y
y%value=x
if(present(increment)) y%value=y%value+increment
y%flag=y%value>0
end function
elemental function bump(x) result(y)
type(item),intent(in)::x
type(item)::y
y%value=x%value+1
y%flag=.not.x%flag
end function
elemental integer function value(x)
type(item),intent(in)::x
value=x%value
end function
end module
"""

class ElementalDerivedResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_elemental_derived_result.f90').read_text(encoding='utf-8'), '9 30 45\n')

    def test_scalar_optional_keywords_and_self_assignment(self):
        self.check(MODULE + """use result_records
implicit none
type(item)::a
a=make(increment=2,x=3)
a=bump(a)
print *,a%value,a%flag
print *,value(make(-10))
end
""", '6 F\n-10\n')

    def test_vectors_scalar_broadcast_and_nested_calls(self):
        self.check(MODULE + """use result_records
implicit none
type(item)::a(3),b(3)
a=make([1,-2,3])
b=bump(a)
a=bump(a)
print *,a%value,a%flag
print *,b%value,b%flag
print *,value(bump(make([5,6,7],increment=10)))
end
""", '2 -1 4 F T F\n2 -1 4 F T F\n16 17 18\n')

    def test_sections_and_array_elements(self):
        self.check(MODULE + """use result_records
implicit none
integer::x(4)
type(item)::a(4)
x=[1,2,3,4]
a=make(0)
a(1:4:2)=make(x(4:1:-2))
a(2)=make(8)
print *,a%value,a%flag
print *,value(bump(a(4:1:-2)))
end
""", '4 8 2 0 T T T F\n1 9\n')

    def test_empty_array(self):
        self.check(MODULE + """use result_records
implicit none
type(item)::a(0)
integer::x(0)
a=make(x)
print *,size(a)
end
""", '0\n')

    def test_expression_arguments_and_parameter_bounds(self):
        self.check(MODULE + """use result_records
implicit none
integer,parameter::n=3
integer::x(0:n-1)
type(item)::a(n)
x=[1,2,3]
a=make(x*2+1)
print *,a%value
end
""", '3 5 7\n')

    def test_output_and_constructor_arguments(self):
        self.check(MODULE + """use result_records
implicit none
print *,make(3)
print *,bump(item(4,.false.))
print *,make([5,-6])
end
""", '3 T\n5 T\n5 T -6 F\n')

    def test_impure_calls_evaluated_once_and_in_order(self):
        self.check("""module m
implicit none
type :: item
integer :: value
logical :: flag
end type
contains
impure elemental function make(x) result(y)
integer,intent(in)::x
type(item)::y
print *,x
y%value=x
y%flag=x>0
end function
end module
use m
implicit none
type(item)::a(2),b
a=make([1,2])
b=make(3)
print *,a%value,b%value
end
""", '1 2 3\n1 2 3\n')

    def test_early_return_and_default_component_initialization(self):
        self.check("""module m
implicit none
type :: item
integer :: value=10
end type
contains
elemental function make(x) result(y)
integer,intent(in)::x
type(item)::y
if(x==0) return
y%value=x
end function
end module
use m
implicit none
type(item)::a(3)
a=make([1,0,3])
print *,a%value
end
""", '1 10 3\n')

    def test_nested_records(self):
        self.check("""module m
implicit none
type :: inner
integer :: value
end type
type :: outer
type(inner)::data
logical::flag
end type
contains
elemental function make(x) result(y)
integer,intent(in)::x
type(outer)::y
y%data%value=x
y%flag=x>0
end function
end module
use m
implicit none
type(outer)::a(2)
a=make([3,-4])
print *,a%data%value,a%flag
end
""", '3 -4 T F\n')

    def test_result_and_local_share_declaration(self):
        self.check(MODULE.replace('type(item)::y\ny%value=x', 'type(item)::y,z\nz%value=x\ny%value=z%value', 1) + """use result_records
implicit none
type(item)::a(2)
a=make([3,4])
print *,a%value
end
""", '3 4\n')

    def test_scalar_character_component(self):
        self.check("""module m
implicit none
type :: item
character(len=3)::text
integer::value
end type
contains
elemental function make(x) result(y)
character(len=*),intent(in)::x
type(item)::y
y%text=x
y%value=len(x)
end function
end module
use m
implicit none
type(item)::a
a=make('abcde')
print *,a%text,a%value
end
""", 'abc 5\n')

    def test_scalar_reduction_argument(self):
        self.check(MODULE + """use result_records
implicit none
integer::x(3)
type(item)::a
x=[1,2,3]
a=make(sum(x))
print *,a%value
a=make(sum(x)+product(x)+minval(x)+maxval(x)+count(x>0)+size(x)+dot_product(x,x))
print *,a%value
a=make(sum(x,mask=x>1))
print *,a%value
end
""", '6\n36\n5\n')

    def test_vector_subscript_argument(self):
        self.check(MODULE + """use result_records
implicit none
integer::x(4)
type(item)::a(2)
x=[10,20,30,40]
a=make(x([4,2]))
print *,a%value
end
""", '40 20\n')

    def test_array_argument_to_scalar_function(self):
        self.check(MODULE + """program test
use result_records
implicit none
type(item)::a
a=make(total([1,2,3]))
print *,a%value
contains
pure integer function total(x)
integer,intent(in)::x(3)
total=sum(x)
end function
end program
""", '6\n')

    def test_allocatable_result_is_not_elemental(self):
        self.reject(MODULE.replace('type(item)::y', 'type(item),allocatable::y') + """use result_records
end
""", 'result cannot be ALLOCATABLE or POINTER')

    def test_nonconforming_arrays(self):
        self.reject(MODULE + """use result_records
type(item)::a(2)
a=make([1,2],[3,4,5])
end
""", 'shapes do not conform')

    def test_run_time_extents(self):
        # Assumed-shape and ALLOCATABLE actuals, and their sections, were
        # rejected: "ELEMENTAL derived results currently require constant
        # rank-one extents".
        self.check("""module m
implicit none
type mdy
  integer :: month, day, year
end type
type my
  integer :: month, year
end type
contains
elemental function to_my(x) result(y)
type(mdy), intent(in) :: x
type(my) :: y
y%month = x%month
y%year = x%year
end function
integer function first_match(v, list)
type(my), intent(in) :: v, list(:)
integer :: i
first_match = 0
do i = 1, size(list)
  if (list(i)%month == v%month .and. list(i)%year == v%year) then
    first_match = i
    return
  end if
end do
end function
subroutine show(d)
type(mdy), intent(in) :: d(:)
type(my) :: r(size(d))
r = to_my(d)
print *, r%month, r%year
print *, first_match(my(3, 2013), to_my(d)), size(to_my(d(2:))), to_my(d(2:3))
end subroutine
end module
program p
use m
implicit none
type(mdy), allocatable :: d(:)
integer :: n
n = 3
allocate(d(n))
d = [mdy(1, 5, 2012), mdy(3, 6, 2013), mdy(7, 1, 2014)]
call show(d)
print *, to_my(d)
end program
""", "1 3 7 2012 2013 2014\n2 2 3 2013 7 2014\n1 2012 3 2013 7 2014\n")

    def test_run_time_extents_with_numeric_and_scalar_actuals(self):
        self.check("""module m
implicit none
type pt
  real :: x, y
end type
contains
elemental function shift(p, dx, dy) result(q)
type(pt), intent(in) :: p
real, intent(in) :: dx, dy
type(pt) :: q
q%x = p%x + dx
q%y = p%y + dy
end function
subroutine run(p, dx)
type(pt), intent(in) :: p(:)
real, intent(in) :: dx(:)
type(pt), allocatable :: q(:)
q = shift(p, dx, 10.0)
print *, q
print *, shift(p(size(p):1:-1), 0.5, dx(1))
end subroutine
end module
program p
use m
implicit none
type(pt), allocatable :: a(:)
real, allocatable :: d(:)
integer :: n
n = 3
allocate(a(0:n-1), d(n))
a = [pt(1, 2), pt(3, 4), pt(5, 6)]
d = [0.25, 0.5, 0.75]
call run(a, d)
print *, shift(a, d, 1.0)
end program
""", "1.25 12.0 3.5 14.0 5.75 16.0\n5.5 6.25 3.5 4.25 1.5 2.25\n1.25 3.0 3.5 5.0 5.75 7.0\n")

    def test_array_valued_function_actuals(self):
        # The array function's result is evaluated once, into a temporary of
        # its run-time extent, including a result of fixed extent and a
        # function of a record array through an ELEMENTAL generic.
        self.check("""module m
implicit none
type date_mdy
   integer :: month, day, year
end type
interface int
   module procedure yyyymmdd
end interface
contains
elemental function int_to_mdy(idate) result(xmdy)
integer, intent(in) :: idate
type(date_mdy) :: xmdy
xmdy%year  = idate/10000
xmdy%month = modulo(idate/100,100)
xmdy%day   = modulo(idate,100)
end function
elemental function yyyymmdd(x) result(idate)
type(date_mdy), intent(in) :: x
integer :: idate
idate = x%year*10000 + x%month*100 + x%day
end function
function sorted(xx) result(yy)
integer, intent(in) :: xx(:)
integer :: yy(size(xx))
integer :: i, t
yy = xx
do i = 1, size(yy) - 1
   if (yy(i) > yy(i+1)) then
      t = yy(i); yy(i) = yy(i+1); yy(i+1) = t
   end if
end do
end function
function pair() result(k)
integer :: k(2)
k = [20200315, 19991231]
end function
subroutine show(dates)
type(date_mdy), allocatable, intent(in out) :: dates(:)
dates = int_to_mdy(sorted(int(dates)))
print *, dates
end subroutine
end module
use m
implicit none
type(date_mdy), allocatable :: d(:)
type(date_mdy) :: e(2)
d = [date_mdy(1, 1, 2013), date_mdy(4, 26, 2012)]
call show(d)
e = int_to_mdy(pair())
print *, e
print *, size(int_to_mdy(sorted([1, 2, 3])))
end
""", "4 26 2012 1 1 2013\n3 15 2020 12 31 1999\n3\n")

    def test_run_time_extents_that_do_not_conform(self):
        self.check("""module m
implicit none
type pt
  real :: x
end type
contains
elemental function shift(p, dx) result(q)
type(pt), intent(in) :: p
real, intent(in) :: dx
type(pt) :: q
q%x = p%x + dx
end function
subroutine run(p, dx)
type(pt), intent(in) :: p(:)
real, intent(in) :: dx(:)
print *, shift(p, dx)
end subroutine
end module
program p
use m
implicit none
call run([pt(1.0), pt(2.0)], [1.0, 2.0, 3.0])
end program
""", error="ELEMENTAL argument shape mismatch")

    def test_pure_function_still_rejects_external_io(self):
        self.reject(MODULE.replace('y%value=x\n','print *,x\ny%value=x\n') + """use result_records
type(item)::a
a=make(1)
end
""", 'external I/O')

if __name__ == '__main__':
    unittest.main()

"""Scalar derived function results used by derived-array lowering."""
import unittest
import test_deallocate
from test_record_read import ROOT


class ScalarRecordResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_nested_reduction_character_consumer(self):
        self.check((ROOT/'tests/programs/xacv_nested_derived_reduction.f90').read_text(encoding='utf-8'),
                   '03:30\n')

    def test_nested_calls_keywords_and_single_evaluation(self):
        self.check("""module m
implicit none
integer :: calls=0
type :: pair
integer :: x,y
end type
contains
function pick(a,k) result(r)
type(pair),intent(in)::a(:)
integer,intent(in)::k
type(pair)::r
calls=calls+1
r=a(k)
end function
integer function total(a,offset) result(n)
type(pair),intent(in)::a(:)
integer,intent(in),optional::offset
n=sum(a%x)+sum(a%y)
if(present(offset)) n=n+offset
end function
integer function identity(n) result(r)
integer,intent(in)::n
r=n
end function
integer function value(p) result(n)
type(pair),intent(in)::p
n=p%x+p%y
end function
elemental function make(n) result(r)
integer,intent(in)::n
type(pair)::r
r%x=n
r%y=-n
end function
subroutine consume(a,n)
type(pair),intent(in)::a(:)
integer,intent(in)::n
print *,sum(a%x),n
end subroutine
end module
use m
implicit none
type(pair)::a(2),r
a=[pair(2,3),pair(7,11)]
print *,identity(identity(total(offset=4,a=a)))
print *,value(p=pick(k=2,a=a))
print *,calls
r=pick(a=a,k=1)
print *,r%x,r%y,calls
r=make(total(a))
print *,r%x,r%y
call consume(a,total(a))
a=[pick(a,1),pick(a,2)]
print *,a%x,a%y,calls
end
""", '27\n18\n1\n2 3 2\n23 -23\n9 23\n2 7 3 11 4\n')

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_derived_result_array_element.f90').read_text(encoding='utf-8'),
                   '2024 2024 1 2\n')

    def test_once_per_call_and_scalar_broadcast(self):
        self.check("""module m
implicit none
integer :: calls=0
type :: pair
integer :: x,y
end type
contains
function make(n) result(r)
integer,intent(in)::n
type(pair)::r
calls=calls+1
r%x=n
r%y=-n
end function
end module
use m
implicit none
type(pair)::a(2)
a=make(7)
print *,calls,a%x,a%y
a(2)=make(9)
print *,calls,a%x,a%y
end
""", '1 7 7 -7 -7\n2 7 9 -7 -9\n')

    def test_nested_character_and_array_components(self):
        self.check("""module m
implicit none
type :: inner
integer :: n
end type
type :: row
type(inner)::child
character(len=3)::label
real :: x(2)
end type
contains
pure function make(n) result(r)
integer,intent(in)::n
type(row)::r
r%child%n=n
r%label='abc'
r%x=[real(n),real(n+1)]
end function
end module
use m
implicit none
type(row)::a(2,2)
a(2,1)=make(n=4)
print *,a(2,1)%child%n,a(2,1)%label,a(2,1)%x
end
""", '4 abc 4 5\n')

    def test_self_referencing_argument_and_constructor(self):
        self.check("""module m
implicit none
type :: row
integer :: x,y
end type
contains
function bump(a) result(r)
type(row),intent(in)::a
type(row)::r
r%x=a%x+1
r%y=a%y+2
end function
end module
use m
implicit none
type(row)::a(2)
a(1)=row(3,5)
a(1)=bump(a(1))
a=[bump(a(1)),bump(a(1))]
print *,a%x,a%y
end
""", '5 5 9 9\n')


if __name__ == '__main__':
    unittest.main()

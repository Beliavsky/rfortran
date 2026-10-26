"""Deep record-array assignment and elemental owned results."""
import unittest
import test_deallocate
from test_record_read import ROOT
class OwnedRecordArrayOperations(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    def test_elemental_broadcast(self):
        self.check((ROOT/'tests/programs/xowned_record_elemental.f90').read_text(), '99 6 6 9 2 3\nname\nname\n')
    def test_overlap(self):
        self.check("""implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(3)
integer::i
do i=1,3
 allocate(a(i)%x(i))
 a(i)%x=i
end do
a(2:3)=a(1:2)
a(1)%x=9
print *,a(1)%x,a(2)%x,a(3)%x
end
""",'9 1 2 2\n')
    def test_elemental_subroutine(self):
        self.check("""module m
implicit none
type :: row
integer,allocatable::x(:)
end type
contains
impure elemental subroutine show(a,title)
type(row),intent(in)::a
character(len=*),intent(in)::title
print *,title,sum(a%x)
end subroutine
impure elemental subroutine bump(a,n)
type(row),intent(inout)::a
integer,intent(in)::n
a%x=a%x+n
end subroutine
end module
use m
implicit none
type(row)::a(2)
allocate(a(1)%x(1),a(2)%x(2))
a(1)%x=1
a(2)%x=2
call bump(a,[3,4])
call show(a,['one','two'])
end
""",'one 4\ntwo 12\n')
    def test_scalar_broadcast_and_empty(self):
        self.check("""implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2),b,c(0)
allocate(b%x(2))
b%x=[4,5]
a=b
c=b
deallocate(b%x)
a(1)%x(1)=9
print *,a(1)%x,a(2)%x
end
""", '9 5 4 5\n')
    def test_multidimensional_assignment(self):
        self.check("""implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2,2),b(2,2)
integer::i,j
do j=1,2
 do i=1,2
  allocate(a(i,j)%x(1))
  a(i,j)%x=10*j+i
 end do
end do
b=a
b(1,1)%x=99
print *,a(1,1)%x,b(1,1)%x,b(2,2)%x
end
""",'11 99 22\n')
    def test_scalar_expression_evaluated_once(self):
        self.check("""module m
implicit none
integer::calls=0
type :: row
integer,allocatable::x(:)
end type
contains
integer function next()
calls=calls+1
next=2
end function
impure elemental subroutine bump(a,n)
type(row),intent(inout)::a
integer,intent(in)::n
a%x=a%x+n
end subroutine
end module
use m
implicit none
type(row)::a(2)
allocate(a(1)%x(1),a(2)%x(1))
a(1)%x=1
a(2)%x=2
call bump(a,next())
print *,calls,a(1)%x,a(2)%x
end
""",'1 3 4\n')
    def test_elemental_array_input_and_reassignment(self):
        program=(ROOT/'tests/programs/xowned_record_elemental.f90').read_text()
        program=program.replace('b(1)%x(1)=99', 'b=scaled(b,[2,3])')
        self.check(program, '8 12 18 27 2 3\nname\nname\n')
    def test_elemental_shape_mismatch(self):
        program=(ROOT/'tests/programs/xowned_record_elemental.f90').read_text()
        program=program.replace('b(1)%x(1)=99', 'b=scaled(b,[2,3,4])')
        self.check(program, error='ELEMENTAL argument shape mismatch', reference=False)
    def test_assignment_shape_mismatch(self):
        self.check("""implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2),b(3)
a=b
end
""",error='record array assignment shape mismatch',reference=False)
    def test_nondefault_intrinsic_argument_bounds(self):
        program=(ROOT/'tests/programs/xowned_record_elemental.f90').read_text()
        program=program.replace('type(row)::a,b(2)', 'type(row)::a,b(2)\ninteger::n(-1:0)')
        program=program.replace('b=scaled(a,[2,3])', 'n=[2,3]\nb=scaled(a,n)')
        self.check(program,'99 6 6 9 2 3\nname\nname\n')
    def test_record_expression_evaluated_once(self):
        program=(ROOT/'tests/programs/xowned_record_elemental.f90').read_text()
        program=program.replace('type :: row','integer :: calls=0\ntype :: row',1)
        program=program.replace('contains\nelemental', """contains
function make() result(a)
type(row)::a
calls=calls+1
allocate(a%x(2),a%label(1))
a%x=[2,3]
a%label='name'
end function
elemental""",1)
        program=program.replace('b=scaled(a,[2,3])','b=scaled(make(),[2,3])')
        program=program.replace('print *,b(1)%x,b(2)%x,a%x','print *,calls,b(1)%x,b(2)%x,a%x')
        self.check(program,'1 99 6 6 9 2 3\nname\nname\n')
if __name__=='__main__': unittest.main()

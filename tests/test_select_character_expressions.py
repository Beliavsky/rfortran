"""CHARACTER SELECT CASE expressions retain active lengths and evaluate once."""
from pathlib import Path
import unittest
import test_deallocate as helpers

class SelectCharacterExpressions(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_fixture(self):
        source=(Path(__file__).parent / "programs/xselect_deferred_character.f90").read_text()
        self.check(source, "rows matched\ncolumns matched\n")

    def test_resizing_padding_empty_and_default(self):
        self.check("""implicit none
character(:),allocatable::s
integer::i
do i=1,5
select case(i)
case(1)
s='long old text'
case(2)
s='rows'
case(3)
s='rows   '
case(4)
s=''
case(5)
s='row'
end select
select case(s)
case('rows')
print *,i,'rows'
case(' ')
print *,i,'empty'
case default
print *,i,'default'
end select
end do
end
""", "1 default\n2 rows\n3 rows\n4 empty\n5 default\n")

    def test_literal_concat_and_dynamic_substring(self):
        self.check("""implicit none
character(12)::s
integer::lo,hi
s='xxrowsyy'
lo=3
hi=6
select case(s(lo:hi))
case('rows')
print *,'substring'
case default
error stop 'substring'
end select
select case('co'//'ls')
case('cols','columns')
print *,'concat'
case default
error stop 'concat'
end select
select case('rows')
case('rows')
print *,'literal'
case default
error stop 'literal'
end select
select case('')
case(' ')
print *,'empty literal'
case default
error stop 'empty literal'
end select
select case(trim(s))
case('not present')
error stop 'unexpected match'
end select
print *,'done'
end
""", "substring\nconcat\nliteral\nempty literal\ndone\n")

    def test_function_evaluated_once(self):
        self.check("""implicit none
integer::calls
calls=0
select case(option())
case('no')
error stop 'wrong case'
case('yes')
print *,'matched'
case default
error stop 'default'
end select
print *,calls
contains
function option() result(s)
character(3)::s
calls=calls+1
s='yes'
end function
end
""", "matched\n1\n")

    def test_nested_select_and_branch_assignment(self):
        self.check("""implicit none
character(:),allocatable::s
s='rows'
select case(trim(s))
case('rows')
s='cols'
select case(s)
case('cols')
print *,'inner'
case default
error stop 'inner'
end select
case default
error stop 'outer'
end select
print *,s
end
""", "inner\ncols\n")

    def test_type_bound_optional_character(self):
        self.check("""module m
implicit none
type :: table
integer,allocatable::x(:,:)
contains
procedure::choose
end type
contains
function choose(self,axis) result(out)
class(table),intent(in)::self
character(*),optional,intent(in)::axis
type(table)::out
character(:),allocatable::axis_
integer::dim
axis_='rows'
if(present(axis)) axis_=na_option(axis)
print *, '[',axis_,']',len(axis_)
select case(axis_)
case('rows')
dim=2
case('cols','columns')
dim=1
case default
error stop 'bad axis'
end select
out%x=self%x
print *,dim
end function
pure function na_option(text) result(option)
character(*),intent(in)::text
character(:),allocatable::option
integer::i,k
option=trim(adjustl(text))
do i=1,len(option)
k=iachar(option(i:i))
if(k>=iachar('A').and.k<=iachar('Z')) option(i:i)=achar(k+32)
end do
end function
end module
use m
implicit none
type(table)::a,b
allocate(a%x(2,3))
a%x=1
b=a%choose()
b=a%choose(' COLUMNS ')
b=a%choose('rows')
end
""", '[rows] 4\n2\n[columns] 7\n1\n[rows] 4\n2\n')

if __name__ == '__main__':
    unittest.main()

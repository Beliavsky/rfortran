"""Numeric array results specialized with read-only CHARACTER arguments."""
import unittest
import test_deallocate
from test_record_read import ROOT

ORIENT = """function orient(x,position) result(y)
real,intent(in)::x(:)
character(len=*),intent(in)::position
real::y(size(x))
if(position=='before')then
y=x
else
y=-x
end if
end function orient
"""

class NumericArrayCharacterArgument(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_original_program(self):
        self.check((ROOT/'tests/149_numeric_array_character_arg.f90').read_text(encoding='utf-8'),
                   '1.0 2.0\n-1.0 -2.0\n')

    def test_runtime_string_named_nested_and_section_calls(self):
        self.check("""program main
real::x(3),y(2)
character(6)::position
x=[1.0,2.0,3.0]
read(*,'(a)')position
y=orient(position=position,x=x(2:3))
print *,y
print *,orient(orient([1.0,2.0],'after'),'after')
print *,size(orient([real::],'before'))
contains
"""+ORIENT+"end program main\n",'-2 -3\n1 2\n0\n',data='after\n')

    def test_integer_and_logical_results_with_character_arrays(self):
        self.check("""program main
print *,lengths(['a ','bc'])
print *,matches(['a','b'],'a')
contains
function lengths(text) result(out)
character(len=*),intent(in)::text(:)
integer::out(size(text)),i
do i=1,size(text)
out(i)=len_trim(text(i))
end do
end function lengths
function matches(text,target) result(out)
character(len=*),intent(in)::text(:),target
logical::out(size(text))
integer::i
do i=1,size(text)
out(i)=text(i)==target
end do
end function matches
end program main
""",'1 2\nT F\n')

    def test_fixed_length_dummy_and_early_return(self):
        self.check("""program main
print *,lengths('abcd')
contains
function lengths(text) result(out)
character(2),intent(in)::text
integer::out(2)
out=len(text)
return
out=-1
end function lengths
end program main
""",'2 2\n')

    def test_argument_type_and_initialization_errors(self):
        self.reject('program main\nprint *,orient([1.0],1)\ncontains\n'+ORIENT+'end program\n',
                    'CHARACTER actual argument requires a fixed length')
        self.reject('program main\ncharacter(6)::position\nprint *,orient([1.0],position)\ncontains\n'+ORIENT+'end program\n',
                    'uninitialized variable')

    def test_runtime_result_extent_is_rejected(self):
        self.reject("""program main
integer::n
read(*,*)n
print *,fill(n,'x')
contains
function fill(n,text) result(out)
integer,intent(in)::n
character(len=*),intent(in)::text
integer::out(n)
out=len(text)
end function fill
end program main
""",'result extent must resolve to a call-site constant')

if __name__ == '__main__':
    unittest.main()

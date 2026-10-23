"""Derived elemental subroutines: writable components and array lifting."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = """module records
implicit none
type :: item
integer :: value
logical :: flag
end type
contains
elemental subroutine put(y,x)
type(item),intent(out)::y
integer,intent(in)::x
y%value=x
y%flag=x>0
end subroutine
elemental subroutine bump(y,x)
type(item),intent(inout)::y
type(item),intent(in),optional::x
if(present(x)) then
 y%value=y%value+x%value
else
 y%value=y%value+1
end if
y%flag=.not.y%flag
end subroutine
end module
"""

class ElementalDerivedSubroutines(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_elemental_derived_out.f90').read_text(encoding='utf-8'), '9 30 0\n')

    def test_scalar_out_inout_optional_and_keywords(self):
        self.check(MODULE + """use records
implicit none
type(item)::a,b
call put(x=3,y=a)
call bump(a)
call put(b,10)
call bump(x=b,y=a)
print *,a%value,a%flag
end
""", '14 T\n')

    def test_array_out_inout_and_scalar_broadcast(self):
        self.check(MODULE + """use records
implicit none
type(item)::a(3),b(3),c
call put(a,[1,-2,3])
call put(b,10)
call bump(a,b)
call put(c,2)
call bump(y=a,x=c)
print *,a%value,a%flag
end
""", '13 10 15 T F T\n')

    def test_sections_and_unset_array_elements(self):
        self.check(MODULE + """use records
implicit none
type(item)::a(4)
call put(a(1),7)
call put(a(2:4:2),[8,9])
call put(a(3),-1)
call bump(a(2))
print *,a%value,a%flag
end
""", '7 9 -1 9 T F F T\n')

    def test_optional_derived_out(self):
        self.check("""module m
implicit none
type :: item
integer :: value
end type
contains
elemental subroutine put(x,y)
integer,intent(in)::x
type(item),intent(out),optional::y
if(present(y)) y%value=x
end subroutine
end module
use m
implicit none
type(item)::a(2),b
call put(10)
call put(y=b,x=2)
call put([3,4],a)
print *,b%value,a%value
end
""", '2 3 4\n')

    def test_nested_record_components(self):
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
elemental subroutine put(y,x)
type(outer),intent(out)::y
integer,intent(in)::x
y%data%value=x
y%flag=x>0
end subroutine
end module
use m
implicit none
type(outer)::a(2)
call put(a,[3,-4])
print *,a%data%value,a%flag
end
""", '3 -4 T F\n')

    def test_unused_record_input_preserves_call_count(self):
        self.check("""module m
implicit none
type :: item
integer :: value
end type
contains
impure elemental subroutine show(x)
type(item),intent(in)::x
print *,42
end subroutine
end module
use m
implicit none
type(item)::a(3)
a%value=1
call show(a)
end
""", '42 42 42\n')

    def test_early_return_still_copies_output_back(self):
        self.check("""module m
implicit none
type :: item
integer :: value
end type
contains
elemental subroutine put(y,x)
type(item),intent(out)::y
integer,intent(in)::x
y%value=x
if(x>0) return
y%value=-1
end subroutine
end module
use m
implicit none
type(item)::a(2)
call put(a(1),5)
call put(a(2),0)
print *,a%value
end
""", '5 -1\n')

    def test_ordinary_scalar_array_element_output(self):
        self.check("""program test
implicit none
integer::a(2)
call put(a(1),3)
call put(a(2),4)
print *,a
contains
elemental subroutine put(y,x)
integer,intent(out)::y
integer,intent(in)::x
y=x
end subroutine
end program
""", '3 4\n')

    def test_empty_record_array(self):
        self.check(MODULE + """use records
implicit none
type(item)::a(0)
call put(a,3)
call bump(a)
print *,size(a)
end
""", '0\n')

    def test_nonconforming_arrays(self):
        self.reject(MODULE + """use records
type(item)::a(2)
call put(a,[1,2,3])
end
""", 'shapes do not conform')

    def test_scalar_output_with_array_input(self):
        self.reject(MODULE + """use records
type(item)::a
call put(a,[1,2])
end
""", 'requires an array actual')

    def test_record_dummy_must_be_scalar(self):
        self.reject(MODULE.replace('type(item),intent(out)::y', 'type(item),intent(out)::y(2)') + """use records
end
""", 'scalar dummy arguments')

    def test_derived_dummy_requires_intent(self):
        self.reject(MODULE.replace('type(item),intent(out)::y', 'type(item)::y') + """use records
end
""", 'explicit INTENT')

if __name__ == '__main__':
    unittest.main()

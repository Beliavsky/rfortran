"""Optional argument positions during derived-type lowering.
Run after cargo build --bin rfort: python tests/test_derived_optional.py
"""
import unittest
import test_deallocate
from test_record_read import ROOT

class DerivedOptional(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_generic_derived_result_with_optional_character(self):
        source=(ROOT/"tests/programs/xderived_optional_character.f90").read_text(encoding="utf-8")
        self.check(source,"1\n4\n8\n4\n")
        self.check(source.replace("function copy(", "pure function copy("),"1\n4\n8\n4\n")
        # TYPE and CLASS dummies store their INTENT on the declaration.
        # Both must survive early constant resolution for generic selection.
        self.check(source.replace("class(box),intent(in)::self", "type(box),intent(in)::self")
                   .replace("contains\nprocedure :: copy", "")
                   .replace("x%copy()", "copy_box(x)")
                   .replace("x%copy(how='all')", "copy_box(x,how='all')")
                   .replace("x%copy('rows','any')", "copy_box(x,'rows','any')"),
                   "1\n4\n8\n4\n")

    def test_reproducer(self):
        source=(ROOT/"tests/programs/xstr_int.f90").read_text(encoding="utf-8")
        self.check(source,"7\n")

    def test_character_optional_combinations(self):
        source=(ROOT/"tests/programs/xstr_int.f90").read_text(encoding="utf-8")
        source=source.replace("print '(a)', trim(str_int(7))", """print '(a)', trim(str_int(7))
print '(a)', trim(str_int(7,3))
print '(a)', trim(str_int(7,prefix='item'))
print '(a)', trim(str_int(prefix='x',ndigits=2,i=7))
print '(a)', trim(str_int(7,2,'y'))""")
        source=source.replace("if (present(ndigits)) ch = repeat('0', ndigits - 1) // trim(ch)", """if (present(ndigits)) then
if (ndigits == 2) ch = '0' // trim(ch)
if (ndigits == 3) ch = '00' // trim(ch)
end if""")
        self.check(source,"7\n007\nitem7\nx07\ny07\n")

    def test_forward_absent_optional(self):
        source=(ROOT/"tests/programs/xstr_int.f90").read_text(encoding="utf-8")
        source=source.replace("print '(a)', trim(str_int(7))", "print '(a)',trim(forward(7))\nprint '(a)',trim(forward(7,'n'))")
        source=source.replace("end program test_str_int", """function forward(i,prefix) result(ch)
integer,intent(in)::i
character(*),intent(in),optional::prefix
character(20)::ch
ch=str_int(i,prefix=prefix)
end function forward
end program test_str_int""")
        self.check(source,"7\nn7\n")

    def test_derived_result_and_input(self):
        self.check("""program main
implicit none
type :: box
integer :: value
end type box
type(box)::a,b
a=box(7)
b=scaled(a)
print *,b%value
b=scaled(a,factor=3)
print *,b%value
contains
function scaled(a,factor) result(b)
type(box),intent(in)::a
integer,intent(in),optional::factor
type(box)::b
b%value=a%value
if(present(factor)) b%value=a%value*factor
end function
end program
""","7\n21\n")

    def test_type_bound_optional_subroutine(self):
        self.check("""module boxes
implicit none
type :: box
integer :: value,other
contains
procedure :: set_value
end type box
contains
subroutine set_value(self,i,extra)
class(box),intent(inout)::self
integer,intent(in)::i
integer,intent(in),optional::extra
self%value=i
self%other=0
if(present(extra)) self%other=extra
end subroutine
end module
program main
use boxes
implicit none
type(box)::b
b=box(0,0)
call b%set_value(7)
print *,b%value,b%other
call b%set_value(extra=3,i=9)
print *,b%value,b%other
end program
""","7 0\n9 3\n")

    def test_required_arguments_remain_required(self):
        source=(ROOT/"tests/programs/xstr_int.f90").read_text(encoding="utf-8")
        self.reject(source.replace("str_int(7)","str_int(prefix='x')"),"missing required argument `i`")
        self.reject(source.replace("str_int(7)","str_int(7,2,'x',3)"),"too many arguments")

if __name__=="__main__": unittest.main()

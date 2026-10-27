"""DIMENSION attributes and SHAPE in the interpreter and native O0/O2 code.

Run after cargo build --bin rfort: python tests/test_dimension_attribute.py
"""
import unittest

import test_deallocate
from test_record_read import ROOT


class DimensionAttribute(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def check_forms(self, source, declarations, expected):
        for attribute in (True,False):
            with self.subTest(attribute=attribute):
                program=source
                for marker,forms in declarations.items():
                    program=program.replace(marker,forms[0 if attribute else 1])
                self.check(program,expected)

    def test_derived_module_objects_components_and_entity_override(self):
        self.check_forms('''module dimension_records
implicit none
integer,parameter::n=2
type item
integer::value=3
end type
type holder
@COMPONENT@
end type
@MODULE@
end module
program main
use dimension_records
implicit none
type(holder)::box
@LOCAL@
a%value=4
b%value=5
c%value=6
box%items(2)%value=9
print *,size(a),size(b),sum(a%value),sum(b%value)
print *,shape(c),sum(c%value),box%items%value
end
''',{
            '@COMPONENT@':('type(item),dimension(n)::items','type(item)::items(n)'),
            '@MODULE@':('type(item),dimension(n)::a,b(3)','type(item)::a(n),b(3)'),
            '@LOCAL@':('type(item),dimension(-1:0,2:3)::c','type(item)::c(-1:0,2:3)'),
        },'2 3 8 15\n2 2 24 3 9')

    def test_derived_dummy_attributes_and_automatic_bounds(self):
        self.check_forms('''module dimension_procedures
implicit none
type item
integer::value
end type
contains
subroutine work(x,y,n)
integer,intent(in)::n
@INPUT@
@OUTPUT@
@LOCAL@
temp%value=x%value+1
y%value=temp%value
end subroutine
end module
program main
use dimension_procedures
implicit none
type(item)::a(2),b(2)
a%value=[2,4]
call work(a,b,2)
print *,b%value
end
''',{
            '@INPUT@':('type(item),intent(in),dimension(:)::x','type(item),intent(in)::x(:)'),
            '@OUTPUT@':('type(item),dimension(:),intent(out)::y','type(item),intent(out)::y(:)'),
            '@LOCAL@':('type(item),dimension(n)::temp','type(item)::temp(n)'),
        },'3 5')

    def test_derived_allocatable_and_pointer_attributes(self):
        self.check_forms('''program main
implicit none
type item
integer::value
end type
@ALLOCATABLE@
@POINTER@
@TARGET@
allocate(a(2))
a%value=[2,4]
t%value=a%value
p=>t
p(2)%value=9
print *,size(a),a%value,p%value,t%value
nullify(p)
deallocate(a)
end
''',{
            '@ALLOCATABLE@':('type(item),dimension(:),allocatable::a','type(item),allocatable::a(:)'),
            '@POINTER@':('type(item),pointer,dimension(:)::p','type(item),pointer::p(:)'),
            '@TARGET@':('type(item),dimension(2),target::t','type(item),target::t(2)'),
        },'2 2 4 2 9 2 9')

    def test_complex_and_named_kind_declarations(self):
        self.check_forms('''program main
implicit none
integer,parameter::dp=kind(1d0),n=2
@COMPLEX@
@REAL@
z=cmplx(3.,4.)
x=2._dp
print *,size(z),sum(abs(z)),sum(x)
end
''',{
            '@COMPLEX@':('complex,dimension(n)::z','complex::z(n)'),
            '@REAL@':('real(kind=dp),dimension(n)::x','real(kind=dp)::x(n)'),
        },'2 10 4')

    def test_semicolon_declarations_and_contextual_names(self):
        self.check_forms('''program main
implicit none
type item
integer::value
end type
@INTRINSIC@; @DERIVED@
dimension=3; values%value=5
print *,sum(dimension),sum(values%value)
end
''',{
            '@INTRINSIC@':('integer,dimension(2)::dimension','integer::dimension(2)'),
            '@DERIVED@':('type(item),dimension(2)::values','type(item)::values(2)'),
        },'6 10')

    def test_uninitialized_shape_inquiries(self):
        self.check((ROOT / "tests/130_dimension_attribute.f90").read_text(
            encoding="utf-8"), "2 3 2 3\n")

    def test_entity_bounds_override_attribute_and_dimension_identifier(self):
        self.check("""program main
implicit none
integer,parameter::n=2
integer,dimension(n+1)::a,b(2),dimension(4)
a=[1,2,3]
b=[4,5]
dimension=7
print *,shape(a),shape(b),shape(dimension),sum(a),sum(b),sum(dimension)
end program main
""", "3 2 4 6 9 28\n")

    def test_assumed_shape_with_intent_and_optional_attributes(self):
        self.check("""program main
implicit none
integer,dimension(3)::a,b
a=[2,4,6]
call twice(a,b)
print *,b
contains
pure subroutine twice(nn,output,extra)
integer,dimension(:),intent(in)::nn
integer,intent(out),dimension(:)::output
integer,intent(in),optional,dimension(:)::extra
output=2*nn+sum(shape(source=nn))-size(nn)
if (present(extra)) output=output+extra
end subroutine twice
end program main
""", "4 8 12\n")

    def test_allocatable_dimension_attribute(self):
        self.check("""program main
integer,dimension(:,:),allocatable::a
print *,allocated(a)
allocate(a(2,3))
a=7
print *,shape(source=a,kind=4),sum(a)
deallocate(a)
print *,allocated(a)
end program main
""", "F\n2 3 42\nF\n")

    def test_parameter_and_initialized_arrays(self):
        self.check_forms("""program main
@PARAMETER@
@SAVED@
print *,p,a,b,shape(a),sum(p)
end program main
""", {
            '@PARAMETER@':('integer,parameter,dimension(3)::p=[2,4,6]','integer,parameter::p(3)=[2,4,6]'),
            '@SAVED@':('integer,dimension(3),save::a=5,b=[1,2,3]','integer,save::a(3)=5,b(3)=[1,2,3]'),
        }, "2 4 6 5 5 5 1 2 3 3 12\n")

    def test_bounds_sections_and_empty_dimensions(self):
        self.check("""program main
integer,dimension(-1:1,2:5)::a
integer,dimension(0,3)::empty
print *,shape(a),shape(a(-1:0,3:5)),shape(empty)
end program main
""", "3 4 2 3 0 3\n")

    def test_character_real_and_logical_arrays(self):
        self.check_forms("""program main
@CHARACTER@
@REAL@
@LOGICAL@
words=['one','two']
values=[1d0,2d0]
flags=.true.
print *,size(words),words(1),shape(values),sum(values),shape(flags),count(flags)
end program main
""", {
            '@CHARACTER@':('character(len=3),dimension(2)::words','character(len=3)::words(2)'),
            '@REAL@':('double precision,dimension(2)::values','double precision::values(2)'),
            '@LOGICAL@':('logical,dimension(2,2)::flags','logical::flags(2,2)'),
        }, "2 one 2 3.0 2 2 4\n")

    def test_shape_keywords_result_vectors_and_scope(self):
        self.check("""program main
integer,dimension(2,3)::a
integer::dims(2)
dims=shape(kind=4,source=a)
print *,dims,shape(a,4),sum(shape(a))
block
integer,dimension(4)::a
print *,shape(a)
end block
print *,shape(source=a)
end program main
""", "2 3 2 3 5\n4\n2 3\n")

    def test_invalid_shape_arguments(self):
        for call, message in [
            ("shape(a,dim=1)", "unknown keyword `dim`"),
            ("shape(source=a,a)", "positional argument after keyword argument"),
            ("shape(a,kind=4,kind=4)", "duplicate argument"),
            ("shape(a,4,4)", "SHAPE expects SOURCE and optional KIND"),
            ("shape(a,kind=8)", "only default INTEGER KIND=4"),
        ]:
            with self.subTest(call=call):
                self.reject(f"program main\ninteger,dimension(2)::a\nprint *,{call}\nend program main", message)


if __name__ == "__main__":
    unittest.main(verbosity=2)

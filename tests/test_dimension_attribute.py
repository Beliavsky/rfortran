"""DIMENSION attributes and SHAPE in the interpreter and native O0/O2 code.

Run after cargo build --bin rfort: python tests/test_dimension_attribute.py
"""
import unittest

import test_deallocate
from test_record_read import ROOT


class DimensionAttribute(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

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
        self.check("""program main
integer,parameter,dimension(3)::p=[2,4,6]
integer,dimension(3),save::a=5,b=[1,2,3]
print *,p,a,b,shape(a),sum(p)
end program main
""", "2 4 6 5 5 5 1 2 3 3 12\n")

    def test_bounds_sections_and_empty_dimensions(self):
        self.check("""program main
integer,dimension(-1:1,2:5)::a
integer,dimension(0,3)::empty
print *,shape(a),shape(a(-1:0,3:5)),shape(empty)
end program main
""", "3 4 2 3 0 3\n")

    def test_character_real_and_logical_arrays(self):
        self.check("""program main
character(len=3),dimension(2)::words
double precision,dimension(2)::values
logical,dimension(2,2)::flags
words=['one','two']
values=[1d0,2d0]
flags=.true.
print *,size(words),words(1),shape(values),sum(values),shape(flags),count(flags)
end program main
""", "2 one 2 3.0 2 2 4\n")

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

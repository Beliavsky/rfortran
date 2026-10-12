"""Typed zero-size constructors in interpreter/native O0/O2 and gfortran."""
import unittest
import test_deallocate
from test_record_read import ROOT

class EmptyConstructors(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_intrinsic_types_and_kinds(self):
        self.check((ROOT/'tests/142_empty_constructors.f90').read_text(),
                   "0 0 0 0 0\n8 8 4 1\n3 0\n0 1 0 1\nT F 0\n7\n")

    def test_procedure_actual(self):
        self.check("""program main
integer,parameter::dp=kind(1.0d0)
print *,tail([real(kind=dp)::])
contains
pure real(kind=dp) function tail(x)
real(kind=dp),intent(in)::x(0)
tail=real(size(x),kind=dp)+7.0_dp
end function tail
end program main
""", "7\n")

    def test_parameters_and_nested_concatenation(self):
        self.check("""program main
integer,parameter::dp=kind(1.0d0)
real(dp),parameter::empty(0)=[real(dp)::]
integer,parameter::n=size([integer::])
integer,parameter::k=kind([real(dp)::])
real(dp)::a(2)
a=[[real(dp)::],1.0_dp,2.0_dp,[real(dp)::]]
print *,size(empty),n,k,sum(a)
end program main
""", "0 0 8 3\n")

    def test_character_output_and_assignment(self):
        self.check("""program main
character(len=3)::a(0)
a=[character(len=3)::]
print *,size(a),len(a)
print *,'before',[character(len=3)::],'after'
end program main
""", "0 3\nbeforeafter\n")

    def test_assumed_shape_subroutine_and_imported_kind(self):
        self.check("""module kinds
integer,parameter::dp=kind(1.0d0)
end module kinds
program main
use kinds
integer::n
call count_values([real(kind=dp)::],n)
print *,n,kind(sum([integer(8)::])),kind(sum([real(8)::]))
print *,size([integer(1)::]),size([integer(2)::]),size([integer(16)::])
contains
subroutine count_values(x,n)
real(dp),intent(in)::x(:)
integer,intent(out)::n
n=size(x)
end subroutine count_values
end program main
""", "0 8 8\n0 0 0\n")

    def test_nonempty_typed_constants_remain_supported(self):
        self.check("""program main
real(8)::a(2)
integer(8)::b(2)
a=[real(8)::1,2]
b=[integer(8)::1,2]
print *,sum(a),kind(sum(a)),sum(b),kind(sum(b))
end program main
""", "3 8 3 8\n")

    def test_invalid_type_parameters(self):
        for value,message in [
            ('[real(kind=16)::]','unsupported REAL kind 16'),
            ('[integer(kind=3)::]','unsupported INTEGER kind 3'),
            ('[real(kind=missing)::]','not a previously defined integer PARAMETER'),
            ('[character(len=*)::]','assumed CHARACTER length'),
            ('[character(len=:)::]','explicit constant length'),
        ]:
            with self.subTest(value=value):
                self.reject('program main\nprint *,size('+value+')\nend program main',message)

    def test_incompatible_parameter_initializer(self):
        self.reject("program main\ninteger,parameter::a(0)=[logical::]\nend program main",
                    "typed array constructor type is incompatible")

if __name__=='__main__':
    unittest.main()

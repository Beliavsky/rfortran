"""Named enum constructors, array conformance and structure composition."""
import subprocess
import tempfile
import unittest
from pathlib import Path

import test_enum_types as enums
import test_fortran2023 as support
from test_record_read import COMPILER, ROOT, build_commands


class EnumArrays(unittest.TestCase):
    check = support.Fortran2023.check
    reject = support.Fortran2023.reject

    def test_example(self):
        self.check((ROOT/'tests/programs/xenum_arrays.f90').read_text(),
                   ' 5 7 8\n 5 6 8\n 7 5 6\n')

    def test_typed_mixed_and_empty_constructors(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season) :: a(3),b(2)
a=[season :: spring,summer,autumn]
b=[spring,season(6)]
print *,int(a),int(b),size([season ::])
a=[season :: b,winter]
print *,int(a)
end
''',' 5 7 8 5 6 0\n 5 6 9\n')

    def test_integer_array_conformance_and_overlap(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season) :: a(-1:2)
integer :: offsets(4)=[0,1,2,3]
a=spring+offsets
print *,int(a)
a(0:2)=[a(-1),a(0),a(1)]
print *,int(a)
a(2:-1:-1)=[season :: winter,autumn,summer,spring]
print *,int(a)
end
''',' 5 6 7 8\n 5 5 6 7\n 5 7 8 9\n')

    def test_implied_do_constructors(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season) :: a(3),b(4)
integer :: i
a=[season :: (spring+i,i=0,2)]
print *,int(a)
a=[(spring+i,i=0,2)]
print *,int(a)
b=[(spring,season(i),i=5,6)]
print *,int(b)
end
''',' 5 6 7\n 5 6 7\n 5 5 5 6\n')

    def test_runtime_allocatable_arrays(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season),allocatable :: a(:),b(:)
integer :: n,i
read *,n
a=[(spring+i-1,i=1,n)]
print *,size(a),int(a)
b=[season :: a, winter]
print *,size(b),int(b)
a=b(2:)
print *,int(a)
end
''',' 3 5 6 7\n 4 5 6 7 9\n 6 7 9\n',data='3\n')

    def test_rank_two_and_fifteen(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season) :: a(2,2),b(1,1,1,1,1,1,1,1,1,1,1,1,1,1,2)
integer :: offsets(1,1,1,1,1,1,1,1,1,1,1,1,1,1,2)
a=reshape([spring,summer,autumn,winter],[2,2])
print *,int(a)
offsets=0
offsets(1,1,1,1,1,1,1,1,1,1,1,1,1,1,2)=1
b=spring+offsets
print *,int(b)
end
''',' 5 7 8 9\n 5 6\n')

    def test_scalar_and_array_integer_intrinsics(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season) :: s,a(3)
integer :: bits(3)=[0,2,3]
s=ior(spring,2)
print *,int(s)
s=season(ieor(spring,3))
print *,int(s)
s=floor(real(spring)+0.9)
print *,int(s)
a=ior(spring,bits)
print *,int(a)
s=index("abc","b")+spring
print *,int(s)
end
''',' 7\n 6\n 5\n 5 7 7\n 7\n')

    def test_integer_conversions_of_real_arrays(self):
        self.check('implicit none\n'+enums.ENUM+'''real :: x(3)=[0.1,1.6,2.2]
type(season) :: a(3)
a=spring+int(x)
print *,int(a)
a=spring+nint(x)
print *,int(a)
end
''',' 5 6 7\n 5 7 7\n')

    def test_integer_intrinsics_named_kind_and_keywords(self):
        self.check('implicit none\n'+enums.ENUM+'''integer,parameter :: ik=selected_int_kind(10)
type(season) :: s,a(2)
real :: x(2)=[1.2,2.6]
s=spring+nint(a=2.3,kind=ik)
print *,int(s)
a=spring+nint(a=x,kind=ik)
print *,int(a)
s=ior(j=2,i=spring)
print *,int(s)
end
''',' 7\n 6 8\n 7\n')

    def test_generic_integer_results_keep_conformance(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''interface code
module procedure scalar_code
end interface
interface values
module procedure vector_code
end interface
contains
integer function scalar_code(i) result(n)
integer,intent(in) :: i
integer :: n
n=i+1
end function
function vector_code(i) result(a)
integer,intent(in) :: i
integer :: a(2)
a=[i,i+1]
end function
end module
use m
implicit none
type(season) :: s,a(2)
s=code(spring)
a=[season :: values(spring)]
print *,int(s),int(a)
end
''',' 6 5 6\n')

    def test_enum_parameter_arrays(self):
        self.check('implicit none\n'+enums.ENUM+'''type(season),parameter :: choices(2)=[season :: spring,summer]
type(season) :: a(2)
a=choices
print *,int(a)
end
''',' 5 7\n')

    def test_intrinsic_constant_initializers(self):
        self.check('''implicit none
enum,bind(c) :: code
enumerator :: first=ior(4,1), second=ibset(first,1)
end enum
type(code),parameter :: chosen=ior(second,8)
print *,first,second,int(chosen)
end
''',' 5 7 15\n')

    def test_structure_constructors_defaults_keywords_and_arrays(self):
        self.check('implicit none\n'+enums.ENUM+'''type calendar
integer :: year=2026
type(season) :: current=summer
type(season) :: next_two(2)
end type
type(calendar) :: c
c=calendar(2027,autumn+1,[season :: spring,summer])
print *,c%year,int(c%current),int(c%next_two)
c=calendar(next_two=[spring,season(6)],current=ior(spring,2))
print *,c%year,int(c%current),int(c%next_two)
c=calendar(next_two=[season :: autumn,winter])
print *,c%year,int(c%current),int(c%next_two)
end
''',' 2027 9 5 7\n 2026 7 5 6\n 2026 7 8 9\n')

    def test_array_component_defaults(self):
        self.check('implicit none\n'+enums.ENUM+'''type calendar
type(season) :: current=summer
type(season) :: following(2)=[season :: autumn,winter]
end type
type(calendar) :: c
print *,int(c%current),int(c%following)
c=calendar(current=spring)
print *,int(c%current),int(c%following)
end
''',' 7 8 9\n 5 8 9\n')

    def test_renamed_module_constructors(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''end module
use m,only:time=>season,first=>spring
implicit none
type(time) :: a(2)
a=[time :: first,time(6)]
print *,int(a)
end
''',' 5 6\n')

    def test_constructor_values_are_evaluated_once(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''integer :: calls=0
contains
integer function value(i) result(n)
integer,intent(in) :: i
integer :: n
calls=calls+1
n=i
end function
end module
use m
implicit none
type(season) :: a(2)
a=[season :: value(spring),value(summer)]
print *,int(a),calls
end
''',' 5 7 2\n')

    def test_generic_resolution_preserves_enum_array_identity(self):
        self.check('module m\nimplicit none\n'+enums.ENUM+'''interface code
module procedure enum_code,int_code
end interface
contains
integer function enum_code(a) result(n)
type(season),intent(in) :: a(:)
integer :: n
n=10+size(a)
end function
integer function int_code(a) result(n)
integer,intent(in) :: a(:)
integer :: n
n=20+size(a)
end function
pure elemental function identity(a) result(b)
type(season),intent(in) :: a
type(season) :: b
b=a
end function
end module
use m
implicit none
type(season) :: a(2)
a=identity([season :: spring,summer])
print *,int(a)
print *,code([season :: spring]),code([spring,season(6)]),code([spring,summer]),code([season ::])
end
''',' 5 7\n 11 12 22 10\n')

    def test_invalid_constructor_elements_and_array_assignment(self):
        base='implicit none\n'+enums.ENUM+'''enum,bind(c)::colour
enumerator red=5,blue
end enum
type(season) :: a(2),matrix(2,2)
integer :: offsets(2)=[0,1]
'''
        for statement,message in (
            ('a=[season :: 5,summer]','same enumeration type'),
            ('a=[season :: red,summer]','same enumeration type'),
            ('a=[season(5),1]','same enumeration type'),
            ('a=offsets','same enumeration type'),
            ('a=[season :: matrix]','scalar or rank one'),
            ('a=[season :: spring+2147483648_8]','C int range'),
            ('a=[season :: real(spring)]','same enumeration type'),
        ):
            with self.subTest(statement=statement):
                self.reject(base+statement+'\nend\n',message)

    def test_bad_structure_component_does_not_erase_identity(self):
        base='implicit none\n'+enums.ENUM+'''type box
type(season) :: current
end type
type(box) :: b
'''
        self.reject(base+'b=box(current=5)\nend\n','component `current`')
        self.reject(base+'b=box(1.0+spring)\nend\n','component `current`')

    def test_runtime_array_conversion_checks_range_and_source_location(self):
        source='implicit none\n'+enums.ENUM+'''integer(8) :: offsets(2)
type(season) :: a(2)
read *,offsets
a=spring+offsets
print *,int(a)
end
'''
        with tempfile.TemporaryDirectory() as tmp:
            work=Path(tmp);path=work/'enum_range.f90';path.write_text(source)
            for command in build_commands(path,work,cwd=work):
                result=subprocess.run(command,cwd=work,input='0 2147483648\n',text=True,capture_output=True,timeout=30)
                self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
                self.assertIn('C int range',result.stderr)
                self.assertIn('enum_range.f90:',result.stderr)


if __name__ == '__main__':
    unittest.main()

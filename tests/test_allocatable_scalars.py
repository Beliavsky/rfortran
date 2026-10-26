# Intrinsic allocatable scalar storage in interpreter/native O0/O2 and gfortran.
import unittest
import test_deallocate
from test_record_read import ROOT

class AllocatableScalars(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_example(self):
        self.check((ROOT/'xalloc_scalar.f90').read_text(encoding='utf-8'), 'F\nT\n42\nF\nT 17\n')

    def test_intrinsic_types(self):
        cases=[('integer(1)','12_1','12'),('integer(2)','-1234_2','-1234'),
               ('integer(8)','1234567890123_8','1234567890123'),
               ('real','1.25','1.25'),('real(8)','1.25_8','1.25'),
               ('logical','.true.','T'),('complex','(1.0,2.0)','(1,2)'),
               ('complex(8)','(1.0_8,2.0_8)','(1,2)')]
        for kind,value,expected in cases:
            with self.subTest(kind=kind):
                self.check(f'''implicit none
{kind},allocatable::x
print *,allocated(x)
x={value}
print *,allocated(x),x
deallocate(x)
allocate(x,source={value})
print *,allocated(x),x
deallocate(x)
print *,allocated(x)
end
''',f'F\nT {expected}\nT {expected}\nF\n')

    def test_allocation_is_not_initialization(self):
        self.reject('integer,allocatable::x\nallocate(x)\nprint *,x\nend\n','uninitialized variable')
        self.reject('integer,allocatable::x\nprint *,x\nend\n','before ALLOCATE')

    def test_reallocation_resets_initialization(self):
        self.reject('integer,allocatable::x\nx=7\ndeallocate(x)\nallocate(x)\nprint *,x\nend\n','uninitialized variable')

    def test_failed_stat_and_mold(self):
        self.check('''implicit none
integer,allocatable::x,y
integer::ios
character(100)::msg
allocate(x,stat=ios)
print *,ios,allocated(x)
allocate(x,stat=ios,errmsg=msg)
print *,ios/=0,len_trim(msg)>0
x=42
allocate(y,mold=x,stat=ios)
print *,ios,allocated(y)
deallocate(x,y,stat=ios)
print *,ios,allocated(x),allocated(y)
deallocate(x,stat=ios,errmsg=msg)
print *,ios/=0,len_trim(msg)>0
end
''','0 T\nT T\n0 T\n0 F F\nT T\n')

    def test_mold_does_not_initialize(self):
        self.reject('integer,allocatable::x,y\nx=42\nallocate(y,mold=x)\nprint *,y\nend\n','uninitialized variable')

    def test_move_alloc(self):
        self.check('''integer,allocatable::x,y
x=3
y=5
call move_alloc(x,y)
print *,allocated(x),allocated(y),y
call move_alloc(x,y)
print *,allocated(x),allocated(y)
end
''','F T 3\nF F\n')

    def test_complex_allocation_inquiries_and_move(self):
        self.check('''complex,allocatable::x,y
print *,kind(x),allocated(x),allocated(y)
x=(1.0,2.0)
call move_alloc(x,y)
print *,allocated(x),allocated(y),y
call move_alloc(x,y)
print *,allocated(x),allocated(y)
end
''','4 F F\nF T (1,2)\nF F\n')
        self.check('complex,allocatable::x,y\nallocate(x)\ncall move_alloc(x,y)\nprint *,y\nend\n',error='uninitialized variable')

    def test_scalar_rank_and_procedure_arguments(self):
        self.check('''program p
implicit none
integer,allocatable::x
x=7
call set_value(x)
print *,x,twice(x),kind(x)
contains
subroutine set_value(a)
integer,intent(out)::a
a=8
end subroutine
integer function twice(a)
integer,intent(in)::a
twice=2*a
end function
end program
''','8 16 4\n')

    def test_read_and_random_number(self):
        self.check('''real,allocatable::x
allocate(x)
read(*,*)x
print *,x
call random_number(x)
print *,x>=0.0 .and. x<1.0
end
''','1.25\nT\n',data='1.25\n')

    def test_local_automatic_cleanup(self):
        self.check('''program p
call f()
call f()
contains
subroutine f()
integer,allocatable::x
print *,allocated(x)
x=42
print *,x
end subroutine
end program
''','F\n42\nF\n42\n')

    def test_pure_local_storage(self):
        self.check('''program p
print *,f(7)
contains
pure integer function f(n)
integer,intent(in)::n
integer,allocatable::x
allocate(x)
x=n
f=x+1
end function
end program
''','8\n')

    def test_character_existing_support(self):
        self.check('''character(:),allocatable::s
print *,allocated(s)
s='hello'
print *,allocated(s),len(s),s
deallocate(s)
print *,allocated(s)
end
''','F\nT 5 hello\nF\n')

    def test_runtime_initialization_and_allocation_checks(self):
        program='integer,allocatable::x\ninteger::n\nallocate(x)\nread(*,*)n\nif(n>0)x=7\nprint *,x\nend\n'
        self.check(program,error='uninitialized variable',data='0\n')
        self.check(program,'7\n',data='1\n')
        program='integer,allocatable::x\ninteger::n\nread(*,*)n\nif(n>0)x=7\nprint *,x\nend\n'
        self.check(program,error='unallocated',data='0\n')

    def test_invalid_array_values_and_source_types(self):
        self.reject('integer,allocatable::x\nx=[1,2]\nend\n','requires a scalar value')
        self.reject('integer,allocatable::x\nallocate(x,source=[1,2])\nend\n','require a scalar expression')
        self.reject('integer(1),allocatable::x\nallocate(x,source=1)\nend\n','same type and kind')
        self.reject('real,allocatable::x\nallocate(x,source=1)\nend\n','same type and kind')
        self.reject("character(4),allocatable::x\nallocate(x,source='hi')\nend\n",'matching CHARACTER lengths')

    def test_module_storage(self):
        self.check('''module m
integer,allocatable::x
contains
subroutine initialize()
x=7
end subroutine
end module
program p
use m
print *,allocated(x)
call initialize()
print *,allocated(x),x
deallocate(x)
print *,allocated(x)
end program
''','F\nT 7\nF\n')

    def test_fixed_character_scalar(self):
        self.check("character(4),allocatable::s\nprint *,allocated(s)\ns='hi'\nprint *,allocated(s),len(s),'['//s//']'\ndeallocate(s)\nprint *,allocated(s)\nend\n", 'F\nT 4 [hi  ]\nF\n')

    def test_character_source_and_substrings(self):
        self.check("character(4),allocatable::s\nallocate(s,source='hi  ')\ns(2:3)='ab'\nprint *,len(s),'['//s//']',s(2:3)\nend\n", '4 [hab ]ab\n')

    def test_type_inquiries_do_not_read_storage(self):
        self.check('integer,allocatable::x\nprint *,kind(x),huge(x)>0,digits(x),allocated(x)\nend\n','4 T 31 F\n')

    def test_source_evaluated_before_allocation(self):
        self.check('''module source_module
integer,allocatable::x,y
integer::calls
contains
integer function source_value()
calls=calls+1
source_value=1
if(allocated(x))source_value=2
end function
end module
program p
use source_module
calls=0
allocate(x,y,source=source_value())
print *,x,y,calls
end program
''','1 1 1\n')

    def test_mold_of_unallocated_scalar(self):
        self.check('integer,allocatable::x,y\nallocate(y,mold=x)\nprint *,allocated(x),allocated(y)\nend\n','F T\n')

    def test_mixed_scalar_array_source(self):
        self.check('integer,allocatable::x,a(:)\nallocate(x,a(3),source=7)\nprint *,x,a\nend\n','7 7 7 7\n')

    def test_invalid_move_rank(self):
        self.reject('integer,allocatable::x,a(:)\nx=1\ncall move_alloc(x,a)\nend\n','same rank')

    def test_user_function_named_like_intrinsic(self):
        self.check('''program p
integer,allocatable::x
x=7
print *,huge(x)
contains
integer function huge(a)
integer,intent(in)::a
huge=a+1
end function
end program
''','8\n')

    def test_module_generic_scalar_rank(self):
        self.check('''module m
integer,allocatable::x
interface f
module procedure f_scalar,f_array
end interface
contains
integer function f_scalar(a)
integer,intent(in)::a
f_scalar=a+3
end function
integer function f_array(a)
integer,intent(in)::a(:)
f_array=sum(a)+13
end function
end module
program p
use m
x=7
print *,f(x)
end program
''','10\n')

    def test_named_kinds(self):
        self.check('''use iso_fortran_env,only:int64,real64
integer(int64),allocatable::i
real(real64),allocatable::x
i=1234567890123_int64
allocate(x,source=1.25_real64)
print *,i,x,kind(i)==int64,kind(x)==real64
end
''','1234567890123 1.25 T T\n')
        self.check("integer,parameter::n=4\ncharacter(n),allocatable::s\nallocate(s,source='abcd')\nprint *,s\nend\n",'abcd\n')

    def test_unsupported_scalar_descriptors_diagnosed(self):
        self.reject('program p\ncontains\nsubroutine f(x)\ninteger,allocatable::x\nend subroutine\nend program\n','scalar dummy arguments are not yet supported')
        self.reject('program p\nprint *,f()\ncontains\nfunction f() result(x)\ninteger,allocatable::x\nx=1\nend function\nend program\n','scalar function results are not yet supported')

    def test_pointer_target(self):
        self.check('integer,allocatable,target::x\ninteger,pointer::p\nx=3\np=>x\np=7\nprint *,x,p,associated(p,x)\nend\n','7 7 T\n')

    def test_no_bounds_on_scalar_allocate(self):
        self.reject('integer,allocatable::x\nallocate(x(2))\nend\n','ALLOCATE')

if __name__=='__main__':
    unittest.main(verbosity=2)

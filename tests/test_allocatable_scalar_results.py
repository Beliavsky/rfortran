"""Intrinsic scalar ALLOCATABLE results and their owned temporaries."""
import unittest
import test_deallocate
from pathlib import Path
import tempfile
import subprocess
from test_record_read import COMPILER, ROOT

class AllocatableScalarResults(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_example(self):
        self.check((ROOT / 'xalloc_scalar_result.f90').read_text(encoding='utf-8'),
                   'T 9\n16\n29\n')

    def test_intrinsic_types(self):
        for kind, value, expected in [
            ('integer', '42', '42'), ('integer(1)', '12_1', '12'),
            ('integer(2)', '-1234_2', '-1234'),
            ('integer(8)', '1234567890123_8', '1234567890123'),
            ('real', '1.25', '1.25'), ('real(8)', '1.25_8', '1.25'),
            ('logical', '.true.', 'T'), ('complex', '(1.0,2.0)', '(1,2)'),
            ('complex(8)', '(1.0_8,2.0_8)', '(1,2)'),
            ('character(4)', "'abcd'", 'abcd')]:
            with self.subTest(kind=kind):
                self.check(f'''program p
implicit none
{kind},allocatable::x
x=f()
print *,allocated(x),x
deallocate(x)
x=f()
print *,x
contains
function f() result(y)
{kind},allocatable::y
y={value}
end function
end program
''', f'T {expected}\n{expected}\n')

    def test_allocate_and_source(self):
        self.check('''program p
implicit none
integer,allocatable::x
x=f(3)
print *,x
x=f(4)
print *,x
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
allocate(y,source=n*n)
end function
end program
''', '9\n16\n')

    def test_without_result_clause(self):
        self.check('''program p
implicit none
print *,f(3)
contains
function f(n)
integer,intent(in)::n
integer,allocatable::f
allocate(f)
f=2*n
end function
end program
''', '6\n')

    def test_expressions_and_ordinary_arguments(self):
        self.check('''program p
implicit none
integer::x
x=f(2)+f(3)*f(4)
print *,x,f(f(5))
call show(f(6))
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
y=n+1
end function
subroutine show(n)
integer,intent(in)::n
print *,n
end subroutine
end program
''', '23 7\n7\n')

    def test_early_returns_and_repeated_calls(self):
        self.check('''program p
implicit none
integer::i
integer,allocatable::x
do i=1,4
x=f(i)
print *,x
end do
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n<3) then
allocate(y)
y=n
return
end if
y=10*n
end function
end program
''', '1\n2\n30\n40\n')

    def test_do_while_reevaluates_and_cycle(self):
        self.check('''program p
implicit none
integer::i
i=0
do while(next(i))
if(i==2) cycle
print *,i
end do
print *,i
contains
function next(n) result(y)
integer,intent(inout)::n
logical,allocatable::y
n=n+1
y=n<4
end function
end program
''', '1\n3\n4\n')

    def test_result_outlives_call_and_other_results(self):
        self.check('''program p
implicit none
integer,allocatable::a,b
a=f(3)
b=f(5)
print *,a,b
deallocate(b)
print *,a
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
y=n
end function
end program
''', '3 5\n3\n')

    def test_optional_and_keywords(self):
        self.check('''program p
implicit none
print *,f(),f(n=7)
contains
function f(n) result(y)
integer,optional,intent(in)::n
integer,allocatable::y
y=-1
if(present(n)) y=n
end function
end program
''', '-1 7\n')

    def test_pure(self):
        self.check('''program p
implicit none
print *,f(7)
contains
pure function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
y=2*n
end function
end program
''', '14\n')

    def test_named_kinds_and_local_parameters(self):
        self.check('''program p
use iso_fortran_env,only:int64
implicit none
integer(int64),allocatable::x
x=f()
print *,x,kind(x)==int64,g()
contains
function f() result(y)
integer,parameter::k=selected_int_kind(18)
integer(k),allocatable::y
y=1234567890123_k
end function
function g() result(s)
integer,parameter::n=4
character(n),allocatable::s
s='abcd'
end function
end program
''', '1234567890123 T abcd\n')

    def test_module_generic_and_constructor(self):
        self.check('''module m
implicit none
interface make
module procedure from_int,from_real
end interface
contains
function from_int(n) result(y)
integer,intent(in)::n
integer,allocatable::y
y=n+1
end function
function from_real(n) result(y)
real,intent(in)::n
real,allocatable::y
y=n+0.5
end function
end module
program p
use m
implicit none
print *,sum([make(1),make(2)]),make(1.0)
end program
''', '5 1.5\n')

    def test_deferred_character_existing_backend(self):
        self.check('''program p
implicit none
character(:),allocatable::s
s=f('abcd')
print *,len(s),s
s=f('xy ')
print *,len(s),s
contains
function f(a) result(y)
character(*),intent(in)::a
character(:),allocatable::y
y=trim(a)
end function
end program
''', '4 abcd\n2 xy\n')

    def test_pure_function_dummies_remain_readonly(self):
        self.reject('''program p
implicit none
integer::n
n=1
print *,f(n)
contains
pure function f(n) result(y)
integer,intent(inout)::n
integer,allocatable::y
n=n+1
y=n
end function
end program
''', 'PURE')

    def test_separately_compiled_module(self):
        with tempfile.TemporaryDirectory(prefix='scalar-result-module-') as tmp:
            directory = Path(tmp)
            module = directory / 'scalar_module.f90'
            module.write_text('''module scalar_module
implicit none
contains
function make(n) result(y)
integer,intent(in)::n
integer,allocatable::y
y=2*n
end function
end module
''', encoding='utf-8')
            main = directory / 'main.f90'
            main.write_text('''program p
use scalar_module,only:make
implicit none
integer,allocatable::x
x=make(7)
print *,allocated(x),x
end program
''', encoding='utf-8')
            obj = directory / 'scalar_module.obj'
            exe = directory / 'main.exe'
            for command in ([COMPILER, '-c', module, '-o', obj],
                            [COMPILER, main, obj, '-o', exe]):
                result = subprocess.run(command, cwd=directory, capture_output=True,
                                        text=True, timeout=60)
                self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([exe], capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.split(), ['T', '14'])

    def test_result_does_not_become_a_variable_actual(self):
        for statement in ['print *,allocated(f())', 'call change(f())',
                          'call take_alloc(f())', 'call move_alloc(f(),x)']:
            with self.subTest(statement=statement):
                self.reject(f'''program p
implicit none
integer,allocatable::x
{statement}
contains
function f() result(y)
integer,allocatable::y
y=3
end function
subroutine change(a)
integer,intent(out)::a
a=4
end subroutine
subroutine take_alloc(a)
integer,allocatable,intent(in)::a
end subroutine
end program
''', 'requires a variable, not a function result')

    def test_runtime_result_initialization_check(self):
        self.check('''program p
implicit none
integer::n
read *,n
print *,f(n)
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
allocate(y)
if(n>0) y=n
end function
end program
''', error='uninitialized', data='0\n')

    def test_result_not_retained_between_loop_iterations(self):
        self.check('''program p
implicit none
integer::i
do i=1,2
print *,f(i)
end do
contains
function f(n) result(y)
integer,intent(in)::n
integer,allocatable::y
if(n==1) y=17
end function
end program
''', error='unallocated result of function')

    def test_type_inquiries_do_not_evaluate_results(self):
        self.check('''program p
implicit none
print *,kind(fi()),kind(fi()+1_8),bit_size(fi()),digits(fi()),radix(fi())
print *,range(fi()),huge(fi())==huge(0_8)
print *,kind(fr()),precision(fr()),range(fr()),minexponent(fr()),maxexponent(fr())
print *,epsilon(fr())==epsilon(0.0_8),tiny(fr())==tiny(0.0_8),huge(fr())==huge(0.0_8)
print *,len(fc()),len(fc()//'xy'),kind(fc()),kind(fl()),kind(fz()),precision(fz())
contains
function fi() result(y)
integer(8),allocatable::y
error stop 'inquiry evaluated fi'
end function
function fr() result(y)
real(8),allocatable::y
error stop 'inquiry evaluated fr'
end function
function fc() result(y)
character(4),allocatable::y
error stop 'inquiry evaluated fc'
end function
function fl() result(y)
logical,allocatable::y
error stop 'inquiry evaluated fl'
end function
function fz() result(y)
complex(8),allocatable::y
error stop 'inquiry evaluated fz'
end function
end program
''', '8 8 64 63 2\n18 T\n8 15 307 -1021 1024\nT T T\n4 6 1 4 8 15\n')

    def test_user_procedure_named_like_inquiry(self):
        self.check('''program p
implicit none
print *,huge(f())
contains
integer function huge(a)
integer,intent(in)::a
huge=a+1
end function
function f() result(y)
integer,allocatable::y
y=3
end function
end program
''', '4\n')

    def test_unallocated_result(self):
        self.reject('''program p
implicit none
print *,f()
contains
function f() result(y)
integer,allocatable::y
end function
end program
''', 'unallocated')

    def test_uninitialized_result(self):
        self.reject('''program p
implicit none
print *,f()
contains
function f() result(y)
integer,allocatable::y
allocate(y)
end function
end program
''', 'uninitialized')

    def test_return_deallocated_result(self):
        self.reject('''program p
implicit none
print *,f()
contains
function f() result(y)
integer,allocatable::y
y=7
deallocate(y)
end function
end program
''', 'unallocated')

if __name__ == '__main__':
    unittest.main(verbosity=2)

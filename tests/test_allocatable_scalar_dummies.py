"""Allocation descriptors for intrinsic scalar dummy arguments."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import COMPILER


class AllocatableScalarDummies(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_out_deallocates_and_inout_changes_caller(self):
        self.check('''program p
implicit none
integer,allocatable::x
call put(x)
print *,allocated(x),x
call put(x)
print *,allocated(x),x
call clear(x)
print *,allocated(x)
contains
subroutine put(a)
integer,allocatable,intent(out)::a
print *,allocated(a)
a=42
end subroutine
subroutine clear(a)
integer,allocatable,intent(inout)::a
deallocate(a)
end subroutine
end program
''', 'F\nT 42\nF\nT 42\nF\n')

    def test_intrinsic_types(self):
        for kind, val, expected in [
            ('integer(1)', '12_1', '12'),
            ('integer(2)', '-1234_2', '-1234'),
            ('integer(8)', '1234567890123_8', '1234567890123'),
            ('real', '1.25', '1.25'),
            ('real(8)', '1.25_8', '1.25'),
            ('logical', '.true.', 'T'),
            ('complex', '(1.0,2.0)', '(1,2)'),
            ('complex(8)', '(1.0_8,2.0_8)', '(1,2)'),
            ('character(4)', "'abcd'", 'abcd'),
        ]:
            with self.subTest(kind=kind):
                self.check(f'''program p
implicit none
{kind},allocatable::x
call put(x)
print *,allocated(x),x
call clear(x)
print *,allocated(x)
contains
subroutine put(a)
{kind},allocatable,intent(out)::a
allocate(a,source={val})
end subroutine
subroutine clear(a)
{kind},allocatable,intent(inout)::a
deallocate(a)
end subroutine
end program
''', f'T {expected}\nF\n')

    def test_in_queries_and_function(self):
        self.check('''program p
implicit none
integer,allocatable::x
print *,has_value(x)
x=7
print *,has_value(x),read_value(x),x
contains
logical function has_value(a)
integer,allocatable,intent(in)::a
has_value=allocated(a)
end function
integer function read_value(a)
integer,allocatable,intent(in)::a
read_value=a
end function
end program
''', 'F\nT 7 7\n')

    def test_forwarding_and_move_alloc(self):
        self.check('''program p
implicit none
integer,allocatable::x,y
call outer(x,y)
print *,allocated(x),allocated(y),y
contains
subroutine outer(a,b)
integer,allocatable,intent(inout)::a,b
call inner(a)
call move_alloc(a,b)
end subroutine
subroutine inner(a)
integer,allocatable,intent(inout)::a
a=17
end subroutine
end program
''', 'F T 17\n')

    def test_optional_presence_is_not_allocation(self):
        self.check('''program p
implicit none
integer,allocatable::x
call inspect()
call inspect(x)
x=3
call inspect(x)
contains
subroutine inspect(a)
integer,optional,allocatable,intent(in)::a
print *,present(a)
if(present(a)) then
print *,allocated(a)
if(allocated(a)) print *,a
end if
end subroutine
end program
''', 'F\nT\nF\nT\nT\n3\n')

    def test_ordinary_and_allocatable_dummies_together(self):
        self.check('''program p
implicit none
integer,allocatable::x,y
x=6
call put(src=x,dest=y)
print *,x,y
contains
subroutine put(dest,src)
integer,allocatable,intent(out)::dest
integer,intent(in)::src
dest=2*src
end subroutine
end program
''', '6 12\n')

    def test_out_can_leave_actual_unallocated(self):
        self.check('''program p
integer,allocatable::x
x=3
call clear(x)
print *,allocated(x)
contains
subroutine clear(a)
integer,allocatable,intent(out)::a
end subroutine
end program
''', 'F\n')

    def test_no_intent(self):
        self.check('''program p
integer,allocatable::x
call put(x)
print *,x
contains
subroutine put(a)
integer,allocatable::a
a=5
end subroutine
end program
''', '5\n')

    def test_function_changes_allocation(self):
        self.check('''program p
implicit none
integer,allocatable::x
print *,f(x)
print *,allocated(x),x
contains
integer function f(a)
integer,allocatable,intent(inout)::a
a=31
f=2
end function
end program
''', '2\nT 31\n')

    def test_separately_compiled_module(self):
        with tempfile.TemporaryDirectory(prefix='scalar-dummy-module-') as tmp:
            directory = Path(tmp)
            module = directory / 'scalar_module.f90'
            module.write_text('''module scalar_module
implicit none
contains
subroutine put(a)
integer,allocatable,intent(out)::a
a=29
end subroutine
end module
''', encoding='utf-8')
            main = directory / 'main.f90'
            main.write_text('''program p
use scalar_module
implicit none
integer,allocatable::x
call put(x)
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
            self.assertEqual(result.stdout.split(), ['T', '29'])

    def test_assumed_character_length(self):
        self.check('''program p
implicit none
character(4),allocatable::x
call put(x)
print *,allocated(x),x
call edit(x)
print *,x
contains
subroutine put(a)
character(*),allocatable,intent(out)::a
allocate(a)
a='abcd'
end subroutine
subroutine edit(a)
character(*),allocatable,intent(inout)::a
a(2:3)='XY'
end subroutine
end program
''', 'T abcd\naXYd\n')

    def test_module_generic_dispatch_preserves_scalar_rank(self):
        self.check('''module m
implicit none
integer,allocatable::x
interface put
module procedure put_scalar,put_array
end interface
contains
subroutine put_scalar(a)
integer,allocatable,intent(out)::a
a=7
end subroutine
subroutine put_array(a)
integer,allocatable,intent(out)::a(:)
a=[8,9]
end subroutine
end module
program p
use m
implicit none
integer,allocatable::y(:)
call put(x)
call put(y)
print *,x,y
end program
''', '7 8 9\n')

    def test_optional_forwarding(self):
        self.check('''program p
integer,allocatable::x
call outer()
call outer(x)
print *,x
contains
subroutine outer(a)
integer,allocatable,optional,intent(inout)::a
call inner(a)
end subroutine
subroutine inner(a)
integer,allocatable,optional,intent(inout)::a
if(present(a)) a=11
end subroutine
end program
''', '11\n')

    def test_optional_keyword_hole(self):
        self.check('''program p
integer,allocatable::x
call f(n=12)
call f(n=13,a=x)
print *,x
contains
subroutine f(a,n)
integer,allocatable,optional,intent(out)::a
integer,intent(in)::n
if(present(a)) a=n
end subroutine
end program
''', '13\n')

    def test_optional_function(self):
        self.check('''program p
implicit none
integer,allocatable::x
print *,f(),f(x)
x=5
print *,f(x)
contains
integer function f(a)
integer,allocatable,optional,intent(in)::a
f=-1
if(present(a)) then
f=0
if(allocated(a)) f=a
end if
end function
end program
''', '-1 0\n5\n')

    def test_out_reallocation_clears_initialization(self):
        self.check('''program p
integer,allocatable::x
x=7
call f(x)
print *,x
contains
subroutine f(a)
integer,allocatable,intent(out)::a
allocate(a)
end subroutine
end program
''', error='uninitialized variable')

    def test_repeated_procedure_local_caller(self):
        self.check('''program p
integer::i
do i=1,3
call outer(i)
end do
contains
subroutine outer(n)
integer,intent(in)::n
integer,allocatable::x
call put(x,n)
print *,allocated(x),x
end subroutine
subroutine put(a,n)
integer,allocatable,intent(out)::a
integer,intent(in)::n
print *,allocated(a)
a=n
end subroutine
end program
''', 'F\nT 1\nF\nT 2\nF\nT 3\n')

    def test_pure_procedures(self):
        self.check('''program p
implicit none
integer,allocatable::x
call put(x)
print *,value(x)
contains
pure subroutine put(a)
integer,allocatable,intent(out)::a
a=23
end subroutine
pure integer function value(a)
integer,allocatable,intent(in)::a
value=a
end function
end program
''', '23\n')

    def test_guarded_read_still_checks_initialization(self):
        self.check('''program p
integer,allocatable::x
allocate(x)
call inspect(x)
contains
subroutine inspect(a)
integer,allocatable,intent(in)::a
if(allocated(a)) print *,a
end subroutine
end program
''', error='uninitialized variable')

    def test_bad_type_kind_and_length(self):
        for kind in ['real', 'integer(8)', 'character(3)']:
            actual_kind = 'character(4)' if kind.startswith('character') else 'integer'
            with self.subTest(kind=kind):
                self.reject(f'''program p
{actual_kind},allocatable::x
call f(x)
contains
subroutine f(a)
{kind},allocatable,intent(inout)::a
end subroutine
end program
''', 'type or rank mismatch')

    def test_uninitialized_read(self):
        self.reject('''program p
integer,allocatable::x
allocate(x)
call read_value(x)
contains
subroutine read_value(a)
integer,allocatable,intent(in)::a
print *,a
end subroutine
end program
''', 'uninitialized variable')

    def test_unallocated_read(self):
        self.reject('''program p
integer,allocatable::x
call read_value(x)
contains
subroutine read_value(a)
integer,allocatable,intent(in)::a
print *,a
end subroutine
end program
''', 'unallocated array')

    def test_invalid_actuals(self):
        for decl, actual in [('integer::x', 'x'), ('integer,allocatable::x', 'x+1'),
                             ('integer,allocatable::x(:)', 'x')]:
            with self.subTest(actual=actual, decl=decl):
                self.reject(f'''program p
{decl}
call f({actual})
contains
subroutine f(a)
integer,allocatable,intent(in)::a
end subroutine
end program
''', 'requires an ALLOCATABLE scalar actual')

    def test_intent_in_is_readonly(self):
        for statement in ['a=1', 'allocate(a)', 'deallocate(a)', 'call change(a)']:
            with self.subTest(statement=statement):
                self.reject(f'''program p
integer,allocatable::x
x=3
call f(x)
contains
subroutine f(a)
integer,allocatable,intent(in)::a
{statement}
end subroutine
subroutine change(b)
integer,allocatable,intent(inout)::b
b=4
end subroutine
end program
''', 'INTENT(IN)')


if __name__ == '__main__':
    unittest.main(verbosity=2)

"""Derived-type assumed-rank calls through the existing record lowering."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import COMPILER, ROOT
import test_deallocate


class DerivedAssumedRank(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_scalar_vector_matrix_writable_alias(self):
        self.check((ROOT/'tests/programs/xassumed_rank_derived.f90').read_text(encoding='utf-8'),
                   '11 3 4 24 24 24 35 35 35 35\n')

    def test_runtime_sections_forwarding(self):
        self.check("""program p
implicit none
type item
integer :: value
end type
type(item),allocatable::a(:,:)
integer::n
read *,n
allocate(a(n,3))
a%value=7
call outer(a)
call outer(a(:,2))
call outer(a(1:n:2,:))
contains
subroutine outer(x)
type(item),intent(in)::x(..)
call inner(x=x)
end subroutine
subroutine inner(x)
type(item),intent(in)::x(..)
select rank(v=>x)
rank(1)
print *,rank(v),size(v),sum(v%value)
rank(2)
print *,rank(v),size(v),sum(v%value)
rank default
print *,rank(v)
end select
end subroutine
end program
""", '2 12 84\n1 4 28\n2 6 42\n',data='4\n')

    def test_two_dummies_different_ranks_optional_and_result(self):
        self.check("""program p
implicit none
type item
integer::v
end type
type(item)::a,b(2)
a%v=3
b%v=4
print *,total(a,b),total(b),total(a)
contains
integer function total(x,y) result(n)
type(item),intent(in)::x(..),y(..)
optional::y
n=0
select rank(x)
rank(0)
n=x%v
rank(1)
n=sum(x%v)
end select
if (present(y)) then
select rank(y)
rank(0)
n=n+y%v
rank(1)
n=n+sum(y%v)
end select
end if
end function
end program
""", '11 8 3\n')

    def test_rank_fifteen(self):
        dims=','.join(['1']*15)
        self.check("""program p
implicit none
type item
integer::v
end type
type(item)::a("""+dims+""")
a%v=9
call f(a)
contains
subroutine f(x)
type(item),intent(in)::x(..)
select rank(x)
rank(15)
print *,rank(x),size(x),sum(x%v)
rank default
error stop
end select
end subroutine
end program
""", '15 1 9\n')

    def test_rank_inquiry_and_component_use(self):
        self.check("""program p
implicit none
type item
integer::v
end type
type(item)::a,b(2)
a%v=7
b%v=8
print *,rank(a),rank(b),rank(b%v)
call f(a)
contains
subroutine f(x)
type(item),intent(in)::x(..)
select rank(v=>x)
rank(0)
print *,v%v
end select
end subroutine
end program
""", '0 1 1\n7\n')

    def test_reject_component_outside_selection(self):
        for statement in ['print *,x%v', 'x%v=1']:
            with self.subTest(statement=statement):
                self.reject("""program p
implicit none
type item
integer::v
end type
contains
subroutine f(x)
type(item)::x(..)
"""+statement+"""
end subroutine
end program
""", 'requires SELECT RANK')

    def test_separate_module(self):
        module="""module m
implicit none
type point
integer::v
end type
contains
subroutine add(x)
type(point),intent(inout)::x(..)
select rank(a=>x)
rank(0)
a%v=a%v+10
rank(2)
a%v=a%v+20
end select
end subroutine
end module
"""
        main="""program p
use m
implicit none
type(point)::a,b(2,3)
a%v=1
b%v=2
call add(a)
call add(b)
print *,a%v,sum(b%v)
end program
"""
        self.check(module+main, '11 132\n')
        with tempfile.TemporaryDirectory(prefix='derived-rank-module-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['11','132'])

    def test_scalar_constructor_actual(self):
        self.check("""program p
implicit none
type point
integer::v
end type
call f(point(42))
contains
subroutine f(x)
type(point),intent(in)::x(..)
select rank(a=>x)
rank(0)
print *,a%v
end select
end subroutine
end program
""", '42\n')

    def test_allocatable_component(self):
        self.check("""program p
implicit none
type item
integer,allocatable::values(:)
end type
type(item)::a
allocate(a%values(3))
a%values=[1,2,3]
call f(a)
print *,a%values
contains
subroutine f(x)
type(item),intent(inout)::x(..)
select rank(v=>x)
rank(0)
v%values=v%values+10
end select
end subroutine
end program
""", '11 12 13\n')

    def test_writable_strided_section(self):
        self.check("""program p
implicit none
type item
integer::v
end type
type(item)::a(4,2)
a%v=1
call change(a(1:4:2,:))
print *,a%v
contains
subroutine change(x)
type(item),intent(inout)::x(..)
select rank(v=>x)
rank(2)
v%v=v%v+10
end select
end subroutine
end program
""", '11 1 11 1 11 1 11 1\n')

    def test_unused_rank_only(self):
        self.check("""program p
implicit none
type point
integer::v
end type
type(point)::a,b(2)
call f(a)
call f(b)
contains
subroutine f(x)
type(point),intent(in)::x(..)
print *,rank(x)
end subroutine
end program
""", '0\n1\n')

    def test_array_allocatable_components(self):
        self.check("""program p
implicit none
type item
integer,allocatable::v(:)
end type
type(item)::a(2)
allocate(a(1)%v(2),a(2)%v(3))
a(1)%v=[1,2]
a(2)%v=[3,4,5]
call f(a)
print *,a(1)%v,a(2)%v
contains
subroutine f(x)
type(item),intent(inout)::x(..)
integer::i
select rank(v=>x)
rank(1)
do i=1,size(v)
v(i)%v=v(i)%v+10
end do
end select
end subroutine
end program
""", '11 12 13 14 15\n')

    def test_declaration_standard_gate_and_diagnostic_line(self):
        source="""subroutine f(x)
implicit none
type point
integer::v
end type
type(point),intent(in)::x(..)
end subroutine
"""
        with tempfile.TemporaryDirectory(prefix='derived-rank-standard-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(source,encoding='utf-8')
            for std,accepted in [('f2008',False),('f2018',True),('f2023',True)]:
                result=subprocess.run([COMPILER,'-std='+std,'parse',path,Path(tmp)/'ast.json'],
                                      capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode==0,accepted,result.stderr)
                if not accepted:
                    self.assertIn('Fortran 2018',result.stderr)
                    self.assertIn('example.f90:6:',result.stderr)

    def test_invalid_declarations(self):
        self.reject("""program p
implicit none
type item
integer::v
end type
type(item)::x(..)
end program
""", 'must be a dummy argument')
        self.reject("""subroutine f(x)
implicit none
type item
integer::v
end type
type(item),value::x(..)
end subroutine
""", 'cannot have VALUE')

    def test_reject_wrong_type(self):
        for actual in ['i','b']:
            with self.subTest(actual=actual):
                self.reject("""program p
implicit none
type first
integer::v
end type
type second
integer::v
end type
type(second)::b
integer::i
i=1
b%v=2
call f("""+actual+""")
contains
subroutine f(x)
type(first),intent(in)::x(..)
print *,rank(x)
end subroutine
end program
""", 'type or kind mismatch')

    def test_reject_polymorphic(self):
        self.reject("""program p
implicit none
type point
integer::v
end type
contains
subroutine f(x)
class(point),intent(in)::x(..)
end subroutine
end program
""", 'polymorphic assumed-rank')


if __name__ == '__main__':
    unittest.main()

"""Assumed-rank Fortran calls and SELECT RANK in interpreter/native modes."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER


class AssumedRank(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_scalar_vector_matrix(self):
        self.check((ROOT/'tests/programs/xassumed_rank.f90').read_text(encoding='utf-8'),
                   '0 2\n1 3 6\n2 2 2 16\n')

    def test_alias_writes_and_multiple_calls(self):
        self.check("""program p
implicit none
integer::a,b(3),c(2,2)
a=1
b=2
c=3
call add(a)
call add(b)
call add(c)
print *,a,b,sum(c)
contains
subroutine add(x)
integer,intent(inout)::x(..)
select rank(v=>x)
rank(0)
v=v+10
rank(1)
v=v+20
rank(2)
v=v+30
end select
end subroutine
end program
""", '11 22 22 22 132\n')

    def test_runtime_allocated_array_and_sections(self):
        self.check("""program p
implicit none
integer::n
real,allocatable::a(:,:)
read *,n
allocate(a(n,3))
a=2
call describe(a)
call describe(a(:,2))
call describe(a(1:n:2,:))
contains
subroutine describe(x)
real,intent(in)::x(..)
select rank(x)
rank(1)
print *,rank(x),size(x),sum(x)
rank(2)
print *,rank(x),size(x),sum(x)
end select
end subroutine
end program
""", '2 12 24\n1 4 8\n2 6 12\n', data='4\n')

    def test_default_and_forwarding_keywords(self):
        self.check("""program p
implicit none
integer::x(2,3,4)
x=1
call outer(x=x)
contains
subroutine outer(x)
integer,intent(in)::x(..)
call inner(a=x)
end subroutine
subroutine inner(a)
integer,intent(in)::a(..)
select rank(v=>a)
rank(0)
print *,v
rank(1)
print *,sum(v)
rank default
print *,rank(v),size(v)
end select
end subroutine
end program
""", '3 24\n')

    def test_ranks_through_fifteen(self):
        for rank in (3,7,8,15):
            with self.subTest(rank=rank):
                dims=','.join(['1']*rank)
                self.check(f"""program p
implicit none
real::x({dims})
x=3
call f(x)
contains
subroutine f(a)
real,intent(in)::a(..)
select rank(a)
rank({rank})
print *,rank(a),size(a),sum(a)
rank default
error stop 'wrong rank'
end select
end subroutine
end program
""", f'{rank} 1 3\n')

    def test_two_assumed_rank_arguments(self):
        self.check("""program p
implicit none
integer::a(2),b(2,3)
a=2
b=3
call f(a,b)
call f(b,a)
contains
subroutine f(x,y)
integer,intent(in)::x(..),y(..)
print *,rank(x),rank(y)
select rank(x)
rank(1)
print *,sum(x)
rank(2)
print *,sum(x)
end select
select rank(y)
rank(1)
print *,sum(y)
rank(2)
print *,sum(y)
end select
end subroutine
end program
""", '1 2\n4\n18\n2 1\n18\n4\n')

    def test_function(self):
        self.check("""program p
implicit none
integer::a(3)
a=[2,4,6]
print *,total(2),total(a)
contains
pure function total(x) result(n)
integer,intent(in)::x(..)
integer::n
select rank(x)
rank(0)
n=x
rank(1)
n=sum(x)
rank default
n=-1
end select
end function
end program
""", '2 12\n')

    def test_logical_and_complex(self):
        self.check("""program p
implicit none
logical::flags(2)
complex::z(2)
flags=[.true.,.false.]
z=[(1.,2.),(3.,4.)]
call logs(flags)
call zs(z)
contains
subroutine logs(x)
logical,intent(in)::x(..)
select rank(x)
rank(1)
print *,all(x),any(x)
end select
end subroutine
subroutine zs(x)
complex,intent(in)::x(..)
select rank(x)
rank(1)
print *,real(sum(x)),aimag(sum(x))
end select
end subroutine
end program
""", 'F T\n4 6\n')

    def test_character(self):
        self.check("""program p
implicit none
character(3)::s='abc',a(2)=['def','ghi']
call f(s)
call f(a)
contains
subroutine f(x)
character(*),intent(in)::x(..)
select rank(x)
rank(0)
print *,len(x),x
rank(1)
print *,len(x),size(x),x
end select
end subroutine
end program
""", '3 abc\n3 2 defghi\n')

    def test_dimension_attribute_named_construct_and_constant_guard(self):
        self.check("""program p
implicit none
integer::a(2)
a=5
call f(a)
contains
subroutine f(x)
integer,parameter::one=1
integer,dimension(..),intent(in)::x
choose: select rank(v=>x)
rank(one) choose
print *,sum(v)
rank default choose
error stop 'wrong rank'
end select choose
end subroutine
end program
""", '10\n')

    def test_rank_intrinsic_and_contextual_name(self):
        self.check("""program p
implicit none
integer::a(2,3)
print *,rank(a),rank(a(:,1)),rank(a(1,1)),rank(1.0),rank([1,2])
block
integer::rank
rank=9
print *,rank
end block
end program
""", '2 1 0 0 1\n9\n')

    def test_optional_and_rank_keyword(self):
        self.check("""program p
implicit none
integer::a(2)
a=3
call f()
call f(x=a)
contains
subroutine f(x)
integer,intent(in),optional::x(..)
if(present(x)) then
print *,rank(a=x)
select rank(x)
rank(1)
print *,sum(x)
end select
else
print *, 'absent'
end if
end subroutine
end program
""", 'absent\n1\n6\n')

    def test_separately_compiled_module(self):
        module="""module rank_module
implicit none
contains
subroutine describe(x)
real,intent(in)::x(..)
select rank(x)
rank(0)
print *,x
rank(2)
print *,sum(x)
end select
end subroutine
end module
"""
        main="""program p
use rank_module
implicit none
real::a(2,3)
a=2
call describe(3.0)
call describe(a)
end program
"""
        self.check(module+main,'3\n12\n')
        with tempfile.TemporaryDirectory(prefix='assumed-rank-module-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['3.0','12.0'])

    def test_standard_gate_and_source_location(self):
        source="""subroutine f(x)
integer::x(..)
select rank(x)
rank(0)
print *,x
end select
end subroutine
"""
        with tempfile.TemporaryDirectory(prefix='assumed-rank-standard-') as tmp:
            source_path=Path(tmp)/'rank.f90'
            source_path.write_text(source,encoding='utf-8')
            for standard,accepted in [('f2008',False),('f2018',True),('f2023',True)]:
                result=subprocess.run([COMPILER,'-std='+standard,'parse',source_path,Path(tmp)/'ast.json'],
                                      capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode==0,accepted,result.stderr)
                if not accepted:
                    self.assertIn('Fortran 2018',result.stderr)
                    self.assertIn('rank.f90:2:',result.stderr)

    def test_rank_only_does_not_read_undefined_actuals(self):
        self.check("""program p
implicit none
integer::x,a(2)
call f(x)
call f(x)
call f(a)
contains
subroutine f(x)
integer,intent(in)::x(..)
print *,rank(x)
end subroutine
end program
""", '0\n0\n1\n')

    def test_expression_actual_still_evaluates(self):
        self.check("""module m
implicit none
integer::calls=0
contains
integer function next() result(n)
calls=calls+1
n=10
end function
subroutine f(x)
integer,intent(in)::x(..)
print *,rank(x)
end subroutine
end module
program p
use m
implicit none
call f(next())
print *,calls
end program
""", '0\n1\n')

    def test_implicit_function_result(self):
        self.check("""program p
implicit none
integer::a(2)
a=1
print *,f(3),f(a)
contains
integer function f(x)
integer,intent(in)::x(..)
f=rank(x)
end function
end program
""", '0 1\n')

    def test_rank_in_specification_subscript_and_loop(self):
        self.check("""program p
implicit none
integer::a(2,3),i
character(len=rank(a))::s
s='ab'
print *,len(s),s
do i=1,rank(a)
print *,i
end do
a(rank(a),1)=7
print *,a(2,1)
end program
""", '2 ab\n1\n2\n7\n')

    def test_wrong_type_is_rejected_even_for_unused_dummy(self):
        self.reject("""program p
real::a=1
call f(a)
contains
subroutine f(x)
integer,intent(in)::x(..)
print *,rank(x)
end subroutine
end program
""", 'type or kind mismatch')

    def test_dummy_procedure_named_rank_and_unrelated_intrinsic(self):
        self.check("""module m
implicit none
abstract interface
integer function callback(x)
integer,intent(in)::x
end function
end interface
contains
integer function custom(x) result(y)
integer,intent(in)::x
y=x+10
end function
integer function apply(rank) result(y)
procedure(callback)::rank
y=rank(3)
end function
end module
program p
use m
implicit none
integer::a(2,3)
print *,apply(custom),rank(a)
end program
""", '13 2\n')

    def test_bad_declarations_and_uses(self):
        for declaration, statement, message in [
            ('integer::x(..)', 'print *,x', 'requires SELECT RANK'),
            ('integer::x(..)', 'print *,x(1)', 'cannot be subscripted'),
            ('integer::x(..)', 'x=1', 'before assignment'),
            ('integer,value::x(..)', '', 'cannot have VALUE'),
            ('integer::x(..,2)', '', 'only (..)'),
        ]:
            with self.subTest(statement=statement,declaration=declaration):
                self.reject(f'subroutine f(x)\n{declaration}\n{statement}\nend subroutine\n', message)
        self.reject('integer::x(..)\nend\n', 'must be a dummy')

    def test_invalid_guards(self):
        for guards,message in [
            ('rank(1)\nrank(1)', 'duplicate RANK(1)'),
            ('rank(16)', 'between 0 and 15'),
            ('rank(-1)', 'between 0 and 15'),
            ('rank(1.5)', 'integer constant'),
            ('rank default\nrank default', 'duplicate RANK DEFAULT'),
            ('rank(*)\nrank(*)', 'duplicate RANK(*)'),
        ]:
            with self.subTest(guards=guards):
                self.reject(f'subroutine f(x)\ninteger::x(..)\nselect rank(x)\n{guards}\nend select\nend subroutine\n',message)
        self.reject('integer::x\nselect rank(x)\nrank(0)\nend select\nend\n','must be assumed-rank')

    def test_default_remains_assumed_rank(self):
        self.reject("""subroutine f(x)
integer::x(..)
select rank(a=>x)
rank default
print *,a
end select
end subroutine
""", 'requires SELECT RANK')

if __name__=='__main__':
    unittest.main(verbosity=2)

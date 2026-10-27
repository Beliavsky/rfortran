"""Scalar SELECT TYPE, preserving static views and dynamic type through calls."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER

TYPES="""implicit none
type base
integer::v
end type
type,extends(base)::child
integer::w
end type
type,extends(child)::grand
integer::z
end type
"""

class SelectType(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture_writable_exact_and_class(self):
        self.check((ROOT/'tests/programs/xselect_type.f90').read_text(encoding='utf-8'),
                   '101 2 13 4 25 6\n')

    def test_class_guard_specificity_independent_of_order(self):
        self.check('program p\n'+TYPES+"""type(grand)::a
a%v=1
a%w=2
a%z=3
call f(a)
contains
subroutine f(x)
class(base),intent(in)::x
select type(x)
class is(base)
print *,1
class is(child)
print *,2
class is(grand)
print *,x%z
class default
error stop
end select
end subroutine
end program
""",'3\n')

    def test_forwarding_keywords_function(self):
        self.check('program p\n'+TYPES+"""type(child)::a
a%v=2
a%w=9
call outer(x=a)
contains
subroutine outer(x)
class(base),intent(in)::x
print *, value(y=x)
end subroutine
integer function value(y) result(n)
class(base),intent(in)::y
select type(v=>y)
type is(child)
n=v%w
class default
n=-1
end select
end function
end program
""",'9\n')

    def test_nested_selection_and_branch_forwarding(self):
        self.check('program p\n'+TYPES+"""type(grand)::a
a%v=1
a%w=2
a%z=3
call f(a)
contains
subroutine f(x)
class(base),intent(in)::x
select type(v=>x)
class is(child)
call inner(v)
select type(w=>v)
type is(grand)
print *,w%z
end select
end select
end subroutine
subroutine inner(x)
class(base),intent(in)::x
select type(x)
type is(grand)
print *,x%z+10
end select
end subroutine
end program
""",'13\n3\n')

    def test_static_type_for_generics(self):
        self.check("""module m
"""+TYPES+"""interface which
module procedure base_id,child_id
end interface
contains
integer function base_id(x) result(n)
type(base),intent(in)::x
n=1
end function
integer function child_id(x) result(n)
type(child),intent(in)::x
n=2
end function
subroutine describe(x)
class(base),intent(in)::x
print *,which(x)
select type(v=>x)
class is(base)
print *,which(v)
type is(child)
print *,which(v)
end select
end subroutine
end module
program p
use m
implicit none
type(child)::a
type(grand)::b
a%v=1
a%w=2
b%v=3
b%w=4
b%z=5
call describe(a)
call describe(b)
end program
""",'1\n2\n1\n1\n')

    def test_two_dummies_and_optional(self):
        self.check('program p\n'+TYPES+"""type(child)::a
type(grand)::b
a%v=1
a%w=2
b%v=3
b%w=4
b%z=5
call f(a)
call f(y=b,x=a)
contains
subroutine f(x,y)
class(base),intent(in)::x,y
optional::y
select type(x)
type is(child)
print *,x%w
end select
if(present(y)) then
select type(y)
type is(grand)
print *,y%z
end select
end if
end subroutine
end program
""",'2\n2\n5\n')

    def test_intent_in_and_invalid_guards(self):
        self.reject('program p\n'+TYPES+"""type(child)::a
a%v=1
a%w=2
call f(a)
contains
subroutine f(x)
class(base),intent(in)::x
select type(v=>x)
type is(child)
v%w=2
end select
end subroutine
end program
""", 'INTENT(IN)')
        self.reject('program p\n'+TYPES+"""type unrelated
integer::n
end type
contains
subroutine f(x)
class(base)::x
select type(x)
type is(unrelated)
end select
end subroutine
end program
""", 'must extend declared type')

    def test_named_construct_and_alias_shadowing(self):
        self.check("""module m
"""+TYPES+"""integer::v=99
contains
subroutine f(x)
class(base),intent(in)::x
choose: select type(v=>x)
type is(child) choose
print *,v%w
class default choose
print *,-1
end select choose
print *,v
end subroutine
end module
program p
use m
implicit none
type(child)::a
a%v=1
a%w=7
call f(a)
end program
""", '7\n99\n')

    def test_separate_module(self):
        module="""module m
"""+TYPES+"""contains
subroutine change(x)
class(base),intent(inout)::x
select type(v=>x)
type is(child)
v%w=v%w+10
end select
end subroutine
end module
"""
        main="""program p
use m
implicit none
type(child)::x
x%v=1
x%w=2
call change(x)
print *,x%w
end program
"""
        self.check(module+main,'12\n')
        with tempfile.TemporaryDirectory(prefix='select-type-module-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['12'])

    def test_standard_gate_and_error_location(self):
        program='program p\n'+TYPES+"""contains
subroutine f(x)
class(base)::x
select type(x)
type is(child)
end select
end subroutine
end program
"""
        with tempfile.TemporaryDirectory(prefix='select-type-standard-') as tmp:
            path=Path(tmp)/'example.f90'
            path.write_text(program,encoding='utf-8')
            for std,accepted in [('f95',False),('f2003',True)]:
                result=subprocess.run([COMPILER,'-std='+std,'parse',path,Path(tmp)/'ast.json'],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode==0,accepted,result.stderr)
                if not accepted:
                    self.assertIn('SELECT TYPE requires Fortran 2003',result.stderr)
                    self.assertIn('example.f90:15:',result.stderr)

    def test_intent_out_dynamic_default_initialization(self):
        self.check("""program p
implicit none
type base
integer::v=11
end type
type,extends(base)::child
integer::w=22
end type
type(child)::a
a%v=1
a%w=2
call f(a)
print *,a%v,a%w
contains
subroutine f(x)
class(base),intent(out)::x
select type(x)
type is(child)
x%w=x%w+1
end select
end subroutine
end program
""", '11 23\n')

    def test_pure_polymorphic_intent_out_rejected(self):
        self.reject('program p\n'+TYPES+"""contains
pure subroutine f(x)
class(base),intent(out)::x
select type(x)
type is(child)
x%w=1
end select
end subroutine
end program
""", 'polymorphic INTENT(OUT)')

    def test_allocatable_component_written_through_alias(self):
        self.check("""program p
implicit none
type base
integer::v
end type
type,extends(base)::child
integer,allocatable::data(:)
end type
type(child)::a
a%v=1
call f(a)
print *,a%data
contains
subroutine f(x)
class(base),intent(inout)::x
select type(v=>x)
type is(child)
allocate(v%data(2))
v%data=[3,4]
end select
end subroutine
end program
""", '3 4\n')

    def test_no_matching_guard_and_block_scope(self):
        self.check('program p\n'+TYPES+"""type(base)::a
a%v=1
call f(a)
contains
subroutine f(x)
class(base),intent(in)::x
integer::i
i=7
select type(x)
type is(child)
print *,x%w
end select
select type(v=>x)
type is(base)
block
integer::i
i=9
print *,i,v%v
end block
end select
print *,i
end subroutine
end program
""", '9 1\n7\n')

    def test_array_selector_explicit_diagnostic(self):
        self.reject('program p\n'+TYPES+"""contains
subroutine f(x)
class(base)::x(:)
select type(x)
type is(child)
end select
end subroutine
end program
""", 'array SELECT TYPE selectors are not yet supported')

    def test_concrete_guard_calls_bound_procedure(self):
        self.check("""module m
implicit none
type base
integer::v
contains
procedure::get=>get_base
end type
type,extends(base)::child
integer::w
contains
procedure::get=>get_child
end type
contains
integer function get_base(x) result(n)
class(base),intent(in)::x
n=x%v
end function
integer function get_child(x) result(n)
class(child),intent(in)::x
n=x%w
end function
subroutine f(x)
class(base),intent(in)::x
select type(v=>x)
type is(child)
print *,v%get()
end select
end subroutine
end module
program p
use m
implicit none
type(child)::a
a%v=1
a%w=7
call f(a)
end program
""", '7\n')

    def test_duplicate_guard(self):
        self.reject('program p\n'+TYPES+"""contains
subroutine f(x)
class(base)::x
select type(x)
type is(child)
type is(child)
end select
end subroutine
end program
""",'duplicate SELECT TYPE guard')

    def test_nonpolymorphic_selector(self):
        self.reject('program p\n'+TYPES+"""type(base)::x
select type(x)
type is(base)
end select
end program
""",'must be polymorphic')

    def test_invalid_components_even_in_uncalled_branches(self):
        for statement in ['print *,x%w','x%w=1']:
            self.reject('program p\n'+TYPES+"""contains
subroutine f(x)
class(base)::x
"""+statement+"""
select type(x)
type is(child)
print *,x%w
end select
end subroutine
end program
""", 'not declared in type `base`')

if __name__=='__main__':
    unittest.main()

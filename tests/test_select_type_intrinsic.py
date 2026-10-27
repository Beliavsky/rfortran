"""Intrinsic SELECT TYPE guards with scalar CLASS(*) specializations."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT, COMPILER

class IntrinsicSelectType(unittest.TestCase):
    def setUp(self):
        self.previous = Path.cwd()
        self.work = tempfile.TemporaryDirectory(prefix='select-type-work-')
        self.addCleanup(self.work.cleanup)
        self.addCleanup(os.chdir, self.previous)
        os.chdir(self.work.name)

    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xselect_type_intrinsic.f90').read_text(),
                   'integer 42\nreal64 2.5\ncharacter 5 hello\nlogical T\nother')

    def test_all_integer_kinds(self):
        self.check("""program p
implicit none
integer(1)::a
integer(2)::b
integer(4)::c
integer(8)::d
a=1
b=2
c=3
d=4
call f(a)
call f(b)
call f(c)
call f(d)
call f(7_8)
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
type is(integer(1))
print *,kind(x),x
type is(integer(kind=2))
print *,kind(x),x
type is(integer)
print *,kind(x),x
type is(integer(8))
print *,kind(x),x
class default
error stop
end select
end subroutine
end
""",'1 1\n2 2\n4 3\n8 4\n8 7')

    def test_real_complex_kinds_and_expressions(self):
        self.check("""program p
use iso_fortran_env,only:real64
implicit none
real::r
real(real64)::d
complex::z
complex(real64)::w
r=1.5
d=2.5_real64
z=(3.0,4.0)
w=(5.0_real64,6.0_real64)
call f(r+1.0)
call f(d+1.0_real64)
call f(z)
call f(w)
call f(7.0_real64)
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
type is(real)
print *,kind(x),x
type is(real(real64))
print *,kind(x),x
type is(complex)
print *,kind(x),real(x),aimag(x)
type is(complex(real64))
print *,kind(x),real(x),aimag(x)
class default
error stop
end select
end subroutine
end
""",'4 2.5\n8 3.5\n4 3 4\n8 5 6\n8 7')

    def test_character_lengths_and_scalar_expression(self):
        self.check("""program p
implicit none
character(3)::s
s='abc'
call f(s)
call f('longer')
call f('')
call f('a'//'bc')
contains
subroutine f(x)
class(*),intent(in)::x
select type(v=>x)
type is(character(*))
print *,len(v),len_trim(v)
end select
end subroutine
end
""",'3 3\n6 6\n0 0\n3 3')

    def test_writable_numeric_and_character(self):
        self.check("""program p
implicit none
integer::i
character(4)::s
i=4
s='test'
call change(i)
call change(s)
print *,i,s
contains
subroutine change(x)
class(*),intent(inout)::x
select type(y=>x)
type is(integer)
y=y+1
type is(character(len=*))
y='abcd'
class default
error stop
end select
end subroutine
end
""",'5 abcd')

    def test_writable_logical(self):
        self.check("""program p
implicit none
logical::x
x=.true.
call flip(x)
print *,x
contains
subroutine flip(x)
class(*),intent(inout)::x
select type(x)
type is(logical)
x=.not.x
class default
error stop
end select
end subroutine
end
""",'F')

    def test_output_argument(self):
        self.check("""program p
implicit none
integer::x
call set(x)
print *,x
contains
subroutine set(x)
class(*),intent(out)::x
select type(x)
type is(integer)
x=19
class default
error stop
end select
end subroutine
end
""",'19')

    def test_forwarding_and_nested_selection(self):
        self.check("""program p
implicit none
call outer(8)
call outer('hello')
contains
subroutine outer(x)
class(*),intent(in)::x
call inner(x)
end subroutine
subroutine inner(x)
class(*),intent(in)::x
select type(v=>x)
type is(integer)
call last(v)
type is(character(*))
print *,len(v)
end select
end subroutine
subroutine last(x)
class(*),intent(in)::x
select type(x)
type is(integer)
print *,x+1
end select
end subroutine
end
""",'9\n5')

    def test_derived_exact_class_and_default(self):
        self.check("""program p
implicit none
type base
integer::n=2
end type
type,extends(base)::child
integer::m=3
end type
type(child)::a
type(base)::b
call f(a)
call f(b)
call f(.false.)
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
class is(base)
print *,x%n
type is(child)
print *,x%n,x%m
type is(integer)
print *,x
class default
print *,0
end select
end subroutine
end
""",'2 3\n2\n0')

    def test_named_kind_module_host_association(self):
        self.check("""module m
use iso_fortran_env,only:k=>real64
implicit none
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
type is(real(k+0))
print *,kind(x),x
class default
error stop
end select
end subroutine
end module
program p
use m,only:g=>f
implicit none
call g(3.0d0)
end
""",'8 3')

    def test_logical_native_dummy_storage(self):
        self.check("""program p
implicit none
call f(.true.)
call f(.false.)
contains
subroutine f(x)
logical,intent(in)::x
print *,x,.not.x
end subroutine
end
""",'T F\nF T')

    def test_module_file_round_trip(self):
        directory=Path(self.work.name)
        source=directory/'guards.f90'
        source.write_text("""module guards
use iso_fortran_env,only:rk=>real64
implicit none
contains
subroutine show(x)
class(*),intent(in)::x
select type(x)
type is(real(rk))
print *,kind(x),x
type is(character(*))
print *,len(x),x
class default
print *,0
end select
end subroutine
end module
""")
        main=directory/'main.f90'
        main.write_text("""program p
use guards,only:f=>show
implicit none
call f(4.0d0)
call f('abc')
end
""")
        def run(args):
            result=subprocess.run(list(map(str,args)),cwd=directory,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            return result
        run([COMPILER,'-c',source,'-o',directory/'guards.obj'])
        run([COMPILER,'-c',main,'-o',directory/'main.obj'])
        exe=directory/('program.exe' if os.name=='nt' else 'program')
        run([COMPILER,'link',directory/'main.obj',directory/'guards.obj','-o',exe])
        self.assertEqual(run([exe]).stdout.split(),['8','4.0','3','abc'])

    def test_duplicate_canonical_kind(self):
        self.reject("""program p
contains
subroutine f(x)
class(*)::x
select type(x)
type is(integer)
type is(integer(kind=4))
end select
end subroutine
end
""",'duplicate SELECT TYPE guard')

    def test_intrinsic_guard_requires_unlimited_selector(self):
        self.reject("""program p
type t
integer::n
end type
contains
subroutine f(x)
class(t)::x
select type(x)
type is(integer)
end select
end subroutine
end
""",'must extend declared type')

    def test_type_narrowing_does_not_escape_guard(self):
        self.reject("""program p
call f(1)
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
type is(integer)
print *,x
end select
print *,x
end subroutine
end
""",'requires a concrete TYPE IS guard')

    def test_fixed_character_guard_rejected(self):
        self.reject("""program p
contains
subroutine f(x)
class(*)::x
select type(x)
type is(character(3))
end select
end subroutine
end
""",'requires assumed length')

    def test_readonly_branch(self):
        self.reject("""program p
contains
subroutine f(x)
class(*),intent(in)::x
select type(x)
type is(integer)
x=1
end select
end subroutine
end
""",'cannot define INTENT(IN)')

    def test_array_actual_rejected(self):
        self.reject("""program p
integer::a(2)
a=1
call f(a)
contains
subroutine f(x)
class(*)::x
select type(x)
type is(integer)
print *,x
end select
end subroutine
end
""",'requires a scalar actual')

    def test_elemental_array_actual_rejected(self):
        self.reject("""program p
real::a(2)
a=1
call f(dble(a))
contains
subroutine f(x)
class(*)::x
select type(x)
type is(real)
print *,x
end select
end subroutine
end
""",'requires a scalar actual')

    def test_writable_expression_rejected(self):
        self.reject("""program p
call f(1+2)
contains
subroutine f(x)
class(*),intent(inout)::x
select type(x)
type is(integer)
x=3
end select
end subroutine
end
""",'requires a variable actual')

if __name__=='__main__':
    unittest.main()

"""Scalar CLASS binding dispatch in interpreter and native modes."""
import unittest
import subprocess
import tempfile
from pathlib import Path
import test_deallocate
from test_record_read import ROOT, COMPILER

class DynamicDispatch(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_functions_subroutines_inherited_binding_and_self_dispatch(self):
        self.check((ROOT/'tests/programs/xdynamic_dispatch.f90').read_text(encoding='utf-8'),
                   '1\n3\n6\n14\n12\n30\n14\n3 7 10\n')

    def test_explicit_parent_bypasses_override(self):
        self.check("""module m
implicit none
type base
integer::v=3
contains
procedure::get=>base_get
procedure::add=>base_add
end type
type,extends(base)::child
contains
procedure::get=>child_get
procedure::add=>child_add
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%v
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=self%base%get()+100
end function
subroutine base_add(self,n)
class(base),intent(inout)::self
integer,intent(in)::n
self%v=self%v+n
end subroutine
subroutine child_add(self,n)
class(child),intent(inout)::self
integer,intent(in)::n
call self%base%add(n)
end subroutine
subroutine show(x)
class(base),intent(inout)::x
call x%add(2)
print *,x%get()
end subroutine
end module
program p
use m
implicit none
type(child)::a
call show(a)
print *,a%v
end program
""",'105\n5\n')

    def test_optional_arguments_and_keyword_reordering(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::get=>base_get
end type
type,extends(base)::child
contains
procedure::get=>child_get
end type
contains
integer function base_get(self,a,b) result(n)
class(base),intent(in)::self
integer,intent(in),optional::a,b
n=self%v
if(present(a)) n=n+a
if(present(b)) n=n+b
end function
integer function child_get(self,a,b) result(n)
class(child),intent(in)::self
integer,intent(in),optional::a,b
n=100+self%v
if(present(a)) n=n+a
if(present(b)) n=n+b
end function
subroutine show(x)
class(base),intent(in)::x
print *,x%get(),x%get(b=3),x%get(b=3,a=2)
end subroutine
end module
program p
use m
implicit none
type(base)::a
type(child)::b
call show(a)
call show(b)
end program
""",'1 4 6\n101 104 106\n')

    def test_bound_method_select_type_and_forwarding(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::show
procedure::get=>base_get
end type
type,extends(base)::child
integer::w=7
contains
procedure::get=>child_get
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%v
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=self%w
end function
subroutine outer(x)
class(base),intent(in)::x
call x%show()
end subroutine
subroutine show(self)
class(base),intent(in)::self
call inner(self)
select type(self)
type is(child)
print *,self%w
class default
print *,self%v
end select
end subroutine
subroutine inner(x)
class(base),intent(in)::x
print *,x%get()
end subroutine
end module
program p
use m
implicit none
type(base)::a
type(child)::b
call outer(a)
call outer(b)
end program
""",'1\n1\n7\n7\n')

    def test_generic_resolution_uses_declared_type_in_method(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::get=>base_get
procedure::show
end type
type,extends(base)::child
contains
procedure::get=>child_get
end type
interface which
module procedure which_base,which_child
end interface
contains
integer function which_base(x) result(n)
type(base),intent(in)::x
n=10
end function
integer function which_child(x) result(n)
type(child),intent(in)::x
n=20
end function
integer function base_get(self) result(n)
class(base),intent(in)::self
n=which(self)
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=which(self)
end function
subroutine show(self)
class(base),intent(in)::self
print *,which(self),self%get()
end subroutine
subroutine outer(x)
class(base),intent(in)::x
call x%show()
end subroutine
end module
program p
use m
implicit none
type(base)::a
type(child)::b
call outer(a)
call outer(b)
end program
""",'10 10\n10 20\n')

    def test_readonly_object_cannot_call_writable_binding(self):
        self.reject("""module m
implicit none
type base
integer::v
contains
procedure::add
end type
contains
subroutine add(self)
class(base),intent(inout)::self
self%v=self%v+1
end subroutine
subroutine readonly(x)
class(base),intent(in)::x
call x%add()
end subroutine
end module
program p
use m
implicit none
type(base)::a
a%v=1
call readonly(a)
end program
""",'INTENT(IN)')

    def test_multiple_class_arguments(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::get=>base_get
procedure::combine
end type
type,extends(base)::child
contains
procedure::get=>child_get
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%v
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=10*self%v
end function
integer function combine(self,other) result(n)
class(base),intent(in)::self,other
n=self%get()+other%get()
end function
end module
program p
use m
implicit none
type(base)::a
type(child)::b
print *,a%combine(b),b%combine(a),b%combine(b)
end program
""", '11 11 20\n')

    def test_separately_compiled_module(self):
        source=(ROOT/'tests/programs/xdynamic_dispatch.f90').read_text(encoding='utf-8')
        module,main=source.split('program test_dynamic_dispatch',1)
        main='program test_dynamic_dispatch'+main
        with tempfile.TemporaryDirectory(prefix='dynamic-dispatch-module-') as tmp:
            work=Path(tmp)
            (work/'m.f90').write_text(module,encoding='utf-8')
            (work/'main.f90').write_text(main,encoding='utf-8')
            for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                            [COMPILER,'main.f90','m.obj','-o','main.exe']):
                result=subprocess.run(command,cwd=work,capture_output=True,text=True,timeout=60)
                self.assertEqual(result.returncode,0,result.stderr)
            result=subprocess.run([work/'main.exe'],capture_output=True,text=True,timeout=15)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['1','3','6','14','12','30','14','3','7','10'])

    def test_inherited_intent_out_initializes_complete_dynamic_type(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::reset
end type
type,extends(base)::child
integer::w=2
end type
contains
subroutine reset(self)
class(base),intent(out)::self
self%v=10
end subroutine
subroutine invoke(x)
class(base),intent(out)::x
call x%reset()
end subroutine
end module
program p
use m
implicit none
type(child)::a
a%v=8
a%w=9
call invoke(a)
print *,a%v,a%w
end program
""", '10 2\n')

    def test_parent_only_call_activates_dispatch_lowering(self):
        self.check("""module m
implicit none
type base
integer::v=3
contains
procedure::get=>base_get
end type
type,extends(base)::child
contains
procedure::get=>child_get
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%v
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=100+self%base%get()
end function
end module
program p
use m
implicit none
type(child)::a
print *,a%get()
end program
""", '103\n')

    def test_optional_class_dummy_with_separate_attribute(self):
        self.check("""module m
implicit none
type base
integer::v=1
contains
procedure::get=>base_get
procedure::combine
end type
type,extends(base)::child
contains
procedure::get=>child_get
end type
contains
integer function base_get(self) result(n)
class(base),intent(in)::self
n=self%v
end function
integer function child_get(self) result(n)
class(child),intent(in)::self
n=10*self%v
end function
integer function combine(self,other) result(n)
class(base),intent(in)::self,other
optional::other
n=self%get()
if(present(other)) n=n+other%get()
end function
end module
program p
use m
implicit none
type(base)::a
type(child)::b
print *,a%combine(),a%combine(other=b),b%combine(other=a)
end program
""", '1 11 11\n')

if __name__=='__main__':
    unittest.main()

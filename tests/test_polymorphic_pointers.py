"""Borrowed scalar polymorphic pointer dispatch, reassociation and guards."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_deallocate
from test_record_read import ROOT,COMPILER

TYPES=(ROOT/'tests/programs/xpolymorphic_pointer.f90').read_text().split('program p\n')[0]
class PolymorphicPointers(unittest.TestCase):
    def setUp(self):
        self.previous=Path.cwd()
        work=tempfile.TemporaryDirectory(prefix='polymorphic-pointer-work-')
        self.work=Path(work.name)
        self.addCleanup(work.cleanup)
        self.addCleanup(os.chdir,self.previous)
        os.chdir(work.name)
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer.f90').read_text(),
                   '1\n3 13 T\n15 13\nF T')

    def test_branch_loop_reassociation_and_forwarding(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
type(base),target::a
type(child),target::b
integer::i
logical::choose
choose=.true.
if(choose) then
ptr=>b
else
ptr=>a
end if
call show(ptr)
do i=1,3
if(i==2) ptr=>a
if(i==3) ptr=>b
call ptr%add(i)
call show(x=ptr)
end do
print *,a%value,b%value
contains
subroutine show(x)
class(base),intent(in)::x
print *,x%get()
end subroutine
end
""",'11\n12\n3\n15\n3 5')

    def test_associated_target_identity_and_snapshot(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr,q
type(child),target::a,b
ptr=>null()
q=>null()
print *,associated(ptr),associated(ptr,q)
ptr=>a
q=>ptr
ptr=>b
print *,associated(pointer=q,target=a),associated(ptr,a),associated(ptr,b),associated(ptr,q)
call q%add(4)
print *,a%value,b%value
nullify(ptr,q)
print *,associated(q),a%value,b%value
end
""",'F F\nT F T F\n5 1\nF 5 1')

    def test_select_type_alias_writes_target(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
type(child),target::a
ptr=>a
select type(v=>ptr)
type is(child)
v%extra=20
call v%add(3)
print *,v%get()
class default
error stop
end select
print *,a%value,a%extra
end
""",'24\n4 20')

    def test_select_alias_survives_selector_reassociation(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
type(child),target::a,b
ptr=>a
select type(v=>ptr)
type is(child)
ptr=>b
call v%add(3)
end select
print *,a%value,b%value
end
""",'4 1')

    def test_class_guard_preserves_static_view_and_dynamic_dispatch(self):
        source=TYPES.replace('contains\ninteger function base_get',"""interface choose
module procedure choose_base,choose_child
end interface
contains
integer function choose_base(x) result(n)
type(base),intent(in)::x
n=1
end function
integer function choose_child(x) result(n)
type(child),intent(in)::x
n=2
end function
integer function base_get""",1)
        self.check(source+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
type(child),target::a
ptr=>a
select type(v=>ptr)
class is(base)
print *,choose(v),v%get()
end select
end
""",'1 11')

    def test_pointer_assignment_across_declared_types(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
class(child),pointer::q
type(base),target::a
type(child),target::b
q=>b
ptr=>q
print *,associated(ptr,q),associated(ptr,b),ptr%get()
ptr=>a
print *,associated(ptr,q),q%get()
end
""",'T T 11\nF 11')

    def test_module_target(self):
        source=TYPES.replace('contains\ninteger function base_get',"""type(child),target::global
contains
integer function base_get""",1)
        source=source.replace('end module',"""subroutine work()
class(base),pointer::ptr
ptr=>global
call ptr%add(2)
print *,ptr%get()
end subroutine
end module""",1)
        self.check(source+"""program p
use pointer_shapes
call work()
print *,global%value
end
""",'13\n3')

    def test_while_condition_tracks_reassociation(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr
type(base),target::a,b
integer::i
a%value=1
b%value=0
ptr=>a
i=0
do while(ptr%get()>0)
i=i+1
ptr=>b
if(i>3) error stop
end do
print *,i
end
""",'1')

    def test_component_assignment_and_multiple_pointer_actuals(self):
        self.check(TYPES+"""program p
use pointer_shapes
implicit none
class(base),pointer::ptr,q
type(child),target::a,b
ptr=>a
q=>b
ptr%value=7
call add_from(ptr,q)
print *,a%value,b%value
contains
subroutine add_from(x,y)
class(base),intent(inout)::x
class(base),intent(in)::y
call x%add(y%get())
end subroutine
end
""",'18 1')

    def test_unassociated_dispatch_fails(self):
        self.check(TYPES+"""program p
use pointer_shapes
class(base),pointer::ptr
type(base),target::a
ptr=>null()
print *,ptr%get()
end
""",error='unassociated polymorphic pointer',reference=False)

    def test_separate_pointer_and_target_attributes(self):
        self.check(TYPES+"""program p
use pointer_shapes
class(base)::ptr
type(child)::a
pointer::ptr
target::a
ptr=>a
call ptr%add(2)
print *,a%value,ptr%get()
end
""",'3 13')

    def test_unused_uninitialized_target_is_not_read(self):
        self.check("""module m
implicit none
type base
integer::v
contains
procedure::get
end type
contains
integer function get(x) result(n)
class(base),intent(in)::x
n=x%v
end function
end module
program p
use m
class(base),pointer::ptr,q
type(base),target::a,b
a%v=8
q=>b
ptr=>a
print *,ptr%get()
end
""",'8')

    def test_target_required(self):
        self.reject(TYPES+"""program p
use pointer_shapes
class(base),pointer::ptr
type(child)::a
ptr=>a
end
""",'must be a compatible concrete scalar TARGET')

    def test_incompatible_target(self):
        self.reject(TYPES+"""program p
use pointer_shapes
type other
integer::n
end type
class(base),pointer::ptr
type(other),target::a
ptr=>a
end
""",'must be a compatible concrete scalar TARGET')

    def test_pointer_dummy_limit_is_explicit(self):
        self.reject(TYPES+"""program p
use pointer_shapes
contains
subroutine f(x)
class(base),pointer::x
print *,x%get()
end subroutine
end
""",'polymorphic POINTER dummy descriptors are not yet supported')

    def test_module_file_round_trip(self):
        module=self.work/'pointer_shapes.f90'
        module.write_text(TYPES)
        main=self.work/'main.f90'
        main.write_text("""program p
use pointer_shapes
class(base),pointer::ptr
type(child),target::a
ptr=>a
call ptr%add(6)
print *,ptr%get(),a%value
end
""")
        def run(args):
            result=subprocess.run(list(map(str,args)),cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
            return result
        run([COMPILER,'-c',module,'-o',self.work/'shapes.obj'])
        run([COMPILER,'-c',main,'-o',self.work/'main.obj'])
        exe=self.work/('program.exe' if os.name=='nt' else 'program')
        run([COMPILER,'link',self.work/'main.obj',self.work/'shapes.obj','-o',exe])
        self.assertEqual(run([exe]).stdout.split(),['17','7'])

if __name__=='__main__':
    unittest.main()

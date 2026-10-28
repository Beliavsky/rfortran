"""Persistent scalar polymorphic pointer components referring to static targets."""
import unittest
import subprocess
import test_polymorphic_pointers as shared
from test_record_read import ROOT, COMPILER

TYPES = shared.TYPES + '''module entries
use pointer_shapes
implicit none
type entry
class(base),pointer::object=>null()
end type
'''
MAIN = '''program p
use entries
implicit none
type(entry)::a,b,c
'''

class PersistentPointerComponents(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check
    reject = shared.PolymorphicPointers.reject

    def test_saved_target_repro(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_saved_target.f90').read_text(),
                   '11 T\n18 18\nF 18')

    def test_private_module_targets_and_distinct_identities(self):
        self.check(TYPES + '''type(child),target,private::first,second
contains
subroutine attach(x,choose)
type(entry),intent(inout)::x
logical,intent(in)::choose
if(choose)then
x%object=>first
else
x%object=>second
end if
end subroutine
end module
''' + MAIN + '''call attach(a,.true.)
call attach(b,.false.)
call attach(c,.true.)
print *,associated(a%object,b%object),associated(a%object,c%object)
a%object%value=7
print *,b%object%get(),c%object%get()
nullify(a%object)
print *,associated(a%object),c%object%get()
end
''', 'F T\n11 17\nF 17')

    def test_saved_target_shared_between_specializations(self):
        self.check(TYPES + '''contains
subroutine attach(x,n)
type(entry),intent(inout)::x
integer,intent(in)::n
type(child),target,save::t
t%value=t%value+n
x%object=>t
end subroutine
end module
''' + MAIN + '''call attach(a,2)
call attach(b,3)
print *,associated(a%object,b%object),a%object%get(),b%object%get()
end
''', 'T 16 16')

    def test_saved_internal_target_and_local_target_coexist(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(child),target::local
call attach(a)
b%object=>local
c=a
print *,a%object%get(),associated(a%object,b%object)
call touch(c)
print *,a%object%get(),local%value
contains
subroutine attach(x)
type(entry),intent(inout)::x
type(child),target,save::t
x%object=>t
end subroutine
subroutine touch(x)
type(entry),intent(in)::x
x%object%value=20
end subroutine
end
''', '11 F\n30 1')

    def test_target_dummy_actual_is_module_storage(self):
        self.check(TYPES + '''type(child),target::t
contains
subroutine attach(x,y)
type(entry),intent(inout)::x
type(child),target,intent(inout)::y
x%object=>y
end subroutine
end module
''' + MAIN + '''call attach(a,t)
a%object%value=9
print *,t%value,associated(a%object,t)
end
''', '9 T')

    def test_saved_constructor_initialized_once(self):
        self.check(TYPES + '''contains
subroutine attach(x)
type(entry),intent(inout)::x
integer,parameter::initial=2+2
type(base),target::t=base(initial)
x%object=>t
t%value=t%value+1
end subroutine
end module
''' + MAIN + '''call attach(a)
call attach(b)
print *,a%object%get(),b%object%get(),associated(a%object,b%object)
end
''', '6 6 T')

    def test_different_saved_locals_with_same_name(self):
        self.check(TYPES + '''contains
subroutine first(x)
type(entry),intent(inout)::x
type(child),target,save::t
x%object=>t
end subroutine
subroutine second(x)
type(entry),intent(inout)::x
type(child),target,save::t
x%object=>t
end subroutine
end module
''' + MAIN + '''call first(a)
call second(b)
a%object%value=5
print *,associated(a%object,b%object),a%object%get(),b%object%get()
end
''','F 15 11')

    def test_automatic_target_still_rejected(self):
        self.reject(TYPES + '''contains
subroutine attach(x)
type(entry),intent(inout)::x
type(child),target::t
x%object=>t
end subroutine
end module
''' + MAIN + 'call attach(a)\nend\n', 'local TARGET storage')

    def test_uninitialized_saved_target_flags(self):
        source = '''module m
implicit none
type node
integer::value
end type
type entry
class(node),pointer::object=>null()
end type
contains
subroutine attach(x)
type(entry),intent(inout)::x
type(node),target,save::t
x%object=>t
end subroutine
end module
program p
use m
implicit none
type(entry)::a
call attach(a)
print *,a%object%value
end
'''
        self.check(source, error='read of uninitialized variable', reference=False)
        self.check(source.replace('print *,a%object%value', 'a%object%value=9\nprint *,a%object%value'), '9')

    def test_separate_compilation_of_saved_storage(self):
        source = (ROOT/'tests/programs/xpolymorphic_pointer_saved_target.f90').read_text()
        module, main = source.split('program p\n')
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text('program p\n'+main)
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True,
                                    text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([self.work/'main.exe'], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), '11 T 18 18 F 18'.split())

    def test_output_owner_reset_does_not_destroy_saved_target(self):
        self.check(TYPES + '''contains
subroutine attach(x)
type(entry),intent(inout)::x
type(child),target,save::t
x%object=>t
end subroutine
subroutine clear(x)
type(entry),intent(out)::x
end subroutine
end module
''' + MAIN + '''call attach(a)
b=a
a%object%value=7
call clear(a)
print *,associated(a%object),b%object%get()
call attach(c)
print *,c%object%get(),associated(c%object,b%object)
end
''', 'F 17\n17 T')

    def test_pure_saved_target_is_not_hidden_by_promotion(self):
        self.reject(TYPES + '''contains
pure subroutine bad(x)
type(entry),intent(in)::x
type(child),target,save::t
end subroutine
end module
''' + MAIN + 'call bad(a)\nend\n', 'PURE procedure cannot have saved')

    def test_saved_target_in_owner_binding_and_generic_call(self):
        source = TYPES.replace('class(base),pointer::object=>null()\nend type\n', '''class(base),pointer::object=>null()
contains
procedure::attach
procedure::set_base
procedure::set_child
generic::set=>set_base,set_child
end type
''', 1)
        self.check(source + '''contains
subroutine attach(self)
class(entry),intent(inout)::self
type(child),target,save::t
call self%set(target=t)
end subroutine
subroutine set_base(self,target)
class(entry),intent(inout)::self
type(base),target,intent(inout)::target
self%object=>target
end subroutine
subroutine set_child(self,target)
class(entry),intent(inout)::self
type(child),target,intent(inout)::target
self%object=>target
end subroutine
end module
''' + MAIN + '''call a%attach()
call b%attach()
a%object%value=9
print *,b%object%get(),associated(a%object,b%object)
end
''', '19 T')

if __name__=='__main__': unittest.main()

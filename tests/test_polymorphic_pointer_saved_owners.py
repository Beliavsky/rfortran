"""Module and SAVE owners with persistent scalar polymorphic pointers."""
import subprocess
import unittest
import test_polymorphic_pointers as shared
from test_record_read import ROOT, COMPILER

TYPES = shared.TYPES + '''module owners
use pointer_shapes
implicit none
type entry
class(base),pointer::object=>null()
end type
type(entry)::current
type(child),target::target
'''
MAIN = '''program p
use owners
implicit none
type(entry)::copy
'''
ERROR = 'persistent pointer component currently requires a saved or module target'

class SavedPointerOwners(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check
    reject = shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_saved_owner.f90').read_text(),
                   'F\nT 5\nF 5\n7\n10\n10')

    def test_module_alias_and_owner_copy(self):
        source = TYPES + '''contains
subroutine attach()
current%object=>target
end subroutine
subroutine clear()
nullify(current%object)
end subroutine
end module
program p
use owners,only:entry,alias=>current,attach,clear
implicit none
type(entry)::copy
print *,associated(alias%object)
call attach()
copy=alias
call clear()
print *,associated(alias%object),copy%object%get()
copy%object%value=7
call attach()
print *,alias%object%get(),associated(alias%object,copy%object)
end
'''
        self.check(source, 'F\nF 11\n17 T')

    def test_saved_owner_and_saved_target_same_activation(self):
        self.check(TYPES + '''contains
subroutine remember(install)
logical,intent(in)::install
type(entry),save::cache
type(child),target,save::local
if(install) cache%object=>local
call cache%object%add(2)
print *,cache%object%get()
end subroutine
end module
''' + MAIN + '''call remember(.true.)
call remember(.false.)
end
''', '13\n15')

    def test_save_statement_splits_entities(self):
        self.check(TYPES + '''contains
subroutine remember(install)
logical,intent(in)::install
type(entry)::cache,scratch
save cache
if(install)then
cache%object=>target
scratch%object=>target
end if
print *,associated(cache%object),associated(scratch%object)
end subroutine
end module
''' + MAIN + 'call remember(.true.)\ncall remember(.false.)\nend\n', 'T T\nT F')

    def test_save_all(self):
        self.check(TYPES + '''contains
subroutine remember(install)
logical,intent(in)::install
type(entry)::cache,scratch
type(child),target::local
save
if(install)then
cache%object=>local
scratch%object=>local
end if
call cache%object%add(1)
print *,associated(scratch%object),scratch%object%get()
end subroutine
end module
''' + MAIN + 'call remember(.true.)\ncall remember(.false.)\nend\n', 'T 12\nT 13')

    def test_module_array_owners(self):
        for rank in (1, 2, 15):
            with self.subTest(rank=rank):
                shape = ','.join(['1']*(rank-1)+['2'])
                source = TYPES.replace('type(entry)::current', f'type(entry)::current({shape}),stored({shape})')
                self.check(source + f'''contains
subroutine attach()
current({shape})%object=>target
end subroutine
end module
''' + MAIN + f'''call attach()
stored=current
nullify(current({shape})%object)
print *,associated(current({shape})%object),stored({shape})%object%get()
call stored({shape})%object%add(3)
print *,target%value
end
''', 'F 11\n4')

    def test_saved_array_owner(self):
        self.check(TYPES + '''contains
subroutine remember(install)
logical,intent(in)::install
integer,parameter::n=2
type(entry),save::cache(n)
if(install)cache(2)%object=>target
print *,associated(cache(1)%object),cache(2)%object%get()
call cache(2)%object%add(2)
end subroutine
end module
''' + MAIN + 'call remember(.true.)\ncall remember(.false.)\nend\n', 'F 11\nF 13')

    def test_nested_owner(self):
        source = TYPES.replace('type(entry)::current', 'type wrapper\ntype(entry)::part\nend type\ntype(wrapper)::current')
        self.check(source + '''contains
subroutine attach()
current%part%object=>target
end subroutine
subroutine reset(x)
type(wrapper),intent(out)::x
end subroutine
end module
''' + MAIN + '''call attach()
copy=current%part
call reset(current)
print *,associated(current%part%object),copy%object%get()
end
''', 'F 11')

    def test_out_owner_preserves_target_and_copy(self):
        self.check(TYPES + '''contains
subroutine reset(x)
type(entry),intent(out)::x
end subroutine
end module
''' + MAIN + '''current%object=>target
copy=current
current%object%value=8
call reset(current)
print *,associated(current%object),copy%object%get(),target%value
end
''', 'F 18 8')

    def test_borrowed_target_direct_diagnostic(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(child),target::local
current%object=>local
end
''', error=ERROR, reference=False)

    def test_borrowed_target_through_forwarded_dummy(self):
        self.check(TYPES + '''contains
subroutine outer(x,y)
type(entry),intent(inout)::x
type(child),target,intent(inout)::y
call inner(x,y)
end subroutine
subroutine inner(x,y)
type(entry),intent(inout)::x
type(child),target,intent(inout)::y
x%object=>y
end subroutine
end module
''' + MAIN + '''type(child),target::local
call outer(current,local)
end
''', error=ERROR, reference=False)

    def test_borrowed_owner_copy_diagnostic(self):
        source = TYPES + 'end module\n' + MAIN + '''type(child),target::local
copy%object=>local
current=copy
end
'''
        self.check(source, error=ERROR, reference=False)

    def test_static_owner_copy_from_mixed_environment(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(child),target::local
logical::choose
choose=.true.
if(choose)then
copy%object=>target
else
copy%object=>local
end if
current=copy
print *,current%object%get(),associated(current%object,target)
end
''', '11 T')

    def test_module_target_attribute_statement(self):
        source = TYPES.replace('type(child),target::target', 'type(child)::target\ntarget::target')
        self.check(source + 'end module\n' + MAIN + '''copy%object=>target
current=copy
print *,current%object%get()
end
''', '11')

    def test_separate_compilation(self):
        source = (ROOT/'tests/programs/xpolymorphic_pointer_saved_owner.f90').read_text()
        module, main = source.split('program p\n')
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text('program p\n'+main)
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
        result = subprocess.run([self.work/'main.exe'], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split(), 'F T 5 F 5 7 10 10'.split())

    def test_pure_read_of_module_owner(self):
        self.check(TYPES + '''contains
pure integer function read_value() result(n)
n=current%object%value
end function
end module
''' + MAIN + '''current%object=>target
print *,read_value()
end
''', '1')

    def test_pure_saved_owner_rejected(self):
        self.reject(TYPES + '''contains
pure subroutine bad()
type(entry),save::cache
end subroutine
end module
''' + MAIN + 'call bad()\nend\n', 'PURE procedure cannot have saved')

    def test_owner_bindings_and_generics(self):
        from test_polymorphic_pointer_owner_bindings import MODULE
        source = MODULE + '''module saved_bindings
use owner_entries
implicit none
type(entry)::current
type(child),target::t
contains
subroutine remember(first)
logical,intent(in)::first
type(entry),save::cache
if(first)call cache%set(target=t)
call cache%add(n=2)
print *,cache%get()
end subroutine
end module
program p
use saved_bindings
implicit none
call current%set(target=t)
call current%set(n=7)
call remember(.true.)
call remember(.false.)
print *,current%get()
call current%clear()
print *,associated(current%object),t%value
end
'''
        self.check(source, '19\n21\n21\nF 11')

    def test_private_owner_is_not_exported(self):
        source = TYPES.replace('type(entry)::current', 'type(entry),private::current')
        self.reject(source+'end module\nprogram p\nuse owners,only:current\nend\n', 'not a public entity')

    def test_main_program_saved_owner_host_access(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(entry),save::cache
type(child),target,save::local
cache%object=>local
call use_saved()
print *,cache%object%get()
contains
subroutine use_saved()
call cache%object%add(3)
end subroutine
end
''', '14')

    def test_uninitialized_module_target(self):
        source = '''module m
implicit none
type node
integer::value
end type
type entry
class(node),pointer::object=>null()
end type
type(entry)::current
type(node),target::t
contains
subroutine attach()
current%object=>t
end subroutine
end module
program p
use m
implicit none
call attach()
print *,current%object%value
end
'''
        self.check(source, error='read of uninitialized variable', reference=False)
        self.check(source.replace('print *,current%object%value', 'current%object%value=7\nprint *,current%object%value'), '7')

    def test_pure_module_target_write_rejected(self):
        self.reject(TYPES + '''contains
pure subroutine bad()
current%object%value=9
end subroutine
end module
''' + MAIN + 'current%object=>target\ncall bad()\nend\n', 'INTENT(IN)')

if __name__=='__main__': unittest.main()

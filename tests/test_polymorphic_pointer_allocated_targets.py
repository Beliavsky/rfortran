"""Scalar allocatable targets retain ownership and allocation identity."""
import subprocess
import unittest
import test_polymorphic_pointers as shared
from test_polymorphic_pointer_saved_owners import TYPES, MAIN
from test_record_read import COMPILER, ROOT

ALLOC_TYPES = TYPES.replace('type(child),target::target', 'type(child),allocatable,target::target')

class AllocatedPointerTargets(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check
    reject = shared.PolymorphicPointers.reject

    def test_allocatable_target_dummy_descriptor(self):
        self.check(ALLOC_TYPES+'''contains
subroutine reset(t)
type(child),allocatable,target,intent(out)::t
allocate(t)
t%value=5
end subroutine
subroutine attach(x,t)
type(entry),intent(inout)::x
type(child),allocatable,target,intent(inout)::t
x%object=>t
end subroutine
end module
'''+MAIN+'''call reset(target)
call attach(copy,target)
call copy%object%add(2)
print *,target%value,copy%object%get(),associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''','7 17 T')

    def test_output_dummy_invalidates_previous_target_identity(self):
        self.check(ALLOC_TYPES+'''contains
subroutine reset(t)
type(child),allocatable,target,intent(out)::t
allocate(t)
end subroutine
end module
'''+MAIN+'''allocate(target)
copy%object=>target
call reset(target)
print *,copy%object%get()
end
''',error='deallocated or reallocated target',reference=False)

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_allocated_target.f90').read_text(), '14 T\nT F 14\nF')

    def test_intrinsic_scalar_fields(self):
        source = ALLOC_TYPES.replace('integer::value=1', '''integer::value=1
real::weight=2.0
logical::enabled=.false.
character(len=4)::label='init'
''')
        self.check(source+'end module\n'+MAIN+'''allocate(target)
copy%object=>target
copy%object%weight=3.5
copy%object%enabled=.true.
copy%object%label='done'
print *,target%weight,target%enabled,target%label
nullify(copy%object)
deallocate(target)
end
''', '3.5 T done')

    def test_module_target_alias_and_dispatch(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''print *,allocated(target)
allocate(target)
current%object=>target
copy=current
call copy%object%add(3)
print *,target%value,current%object%get(),associated(copy%object,target)
nullify(copy%object,current%object)
print *,allocated(target),associated(copy%object)
deallocate(target)
print *,allocated(target)
end
''', 'F\n4 14 T\nT F\nF')

    def test_local_target(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(child),allocatable,target::local
allocate(local)
copy%object=>local
local%value=5
print *,copy%object%get()
call copy%object%add(2)
print *,local%value
nullify(copy%object)
deallocate(local)
end
''', '15\n7')

    def test_forwarded_local_target(self):
        self.check(TYPES + '''contains
subroutine add(x)
type(entry),intent(inout)::x
call x%object%add(3)
end subroutine
end module
''' + MAIN + '''type(child),allocatable,target::local
allocate(local)
copy%object=>local
call add(copy)
print *,local%value,copy%object%get()
nullify(copy%object)
deallocate(local)
end
''', '4 14')

    def test_unallocated_association(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + 'copy%object=>target\nend\n',
                   error='pointer association to unallocated target', reference=False)

    def test_deallocated_target(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''allocate(target)
copy%object=>target
deallocate(target)
print *,copy%object%value
end
''', error='deallocated or reallocated target', reference=False)

    def test_reallocated_target_does_not_retarget_old_alias(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''allocate(target)
copy%object=>target
deallocate(target)
allocate(target)
current%object=>target
print *,copy%object%get()
end
''', error='deallocated or reallocated target', reference=False)

    def test_factory_and_saved_target(self):
        self.check(TYPES + '''contains
function make_entry() result(x)
type(entry)::x
type(child),allocatable,target,save::saved
if(.not.allocated(saved))allocate(saved)
x%object=>saved
end function
end module
''' + MAIN + '''copy=make_entry()
call copy%object%add(5)
current=make_entry()
print *,current%object%get(),associated(copy%object,current%object)
end
''', '16 T')

    def test_assignment_allocates_and_preserves_existing_alias(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''type(child)::value
value%value=3
value%extra=20
target=value
copy%object=>target
value%value=5
value%extra=30
target=value
print *,allocated(target),copy%object%get(),associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''', 'T 35 T')

    def test_failed_allocate_stat_preserves_alias(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''integer::ierr
allocate(target)
copy%object=>target
allocate(target,stat=ierr)
print *,ierr/=0,copy%object%get(),associated(copy%object,target)
nullify(copy%object)
deallocate(target,stat=ierr)
print *,ierr,allocated(target)
end
''', 'T 11 T\n0 F')

    def test_uninitialized_target_field(self):
        source = ALLOC_TYPES.replace('integer::value=1','integer::value') + 'end module\n' + MAIN + '''allocate(target)
copy%object=>target
print *,copy%object%value
end
'''
        self.check(source,error='read of uninitialized variable',reference=False)
        self.check(source.replace('print *,copy%object%value','copy%object%value=9\nprint *,target%value'), '9')

    def test_owned_component(self):
        source = ALLOC_TYPES.replace('integer::value=1', 'integer::value=1\nreal,allocatable::data(:)')
        self.check(source + 'end module\n' + MAIN + '''allocate(target)
target%data=[1.0,2.0,3.0]
copy%object=>target
print *,sum(copy%object%data)
nullify(copy%object)
deallocate(target)
end
''', '6.0')

    def test_target_dummy_association_and_forwarding(self):
        self.check(TYPES + '''contains
subroutine attach(x,t)
type(entry),intent(inout)::x
type(child),target,intent(inout)::t
x%object=>t
end subroutine
subroutine forward(x)
type(entry),intent(inout)::x
call x%object%add(2)
end subroutine
end module
''' + MAIN + '''type(child),allocatable,target::local
allocate(local)
call attach(copy,local)
call forward(copy)
print *,local%value,copy%object%get(),associated(copy%object,local)
nullify(copy%object)
deallocate(local)
end
''', '3 13 T')

    def test_direct_component_subscripts_and_bound_calls(self):
        source = ALLOC_TYPES.replace('integer::value=1', 'integer::value=1\ninteger::items(2)=0')
        self.check(source + 'end module\n' + MAIN + '''allocate(target)
target%items(2)=7
call target%add(3)
copy%object=>target
print *,target%get(),copy%object%items(2),allocated(scalar=target)
nullify(copy%object)
deallocate(target)
end
''', '14 7 T')

    def test_owner_output_does_not_deallocate_target(self):
        self.check(ALLOC_TYPES + '''contains
subroutine clear(x)
type(entry),intent(out)::x
end subroutine
end module
''' + MAIN + '''allocate(target)
copy%object=>target
current=copy
call clear(copy)
print *,allocated(target),associated(copy%object),current%object%get()
nullify(current%object)
deallocate(target)
end
''', 'T F 11')

    def test_source_and_mold(self):
        for option,expected in (('source','17'), ('mold','11')):
            with self.subTest(option=option):
                self.check(ALLOC_TYPES + 'end module\n' + MAIN + f'''type(child)::value
value%value=7
allocate(target,{option}=value)
copy%object=>target
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''', expected)

    def test_shadowed_local_is_not_rewritten(self):
        self.check(TYPES + 'end module\n' + MAIN + '''type(child),allocatable,target::local
allocate(local)
local%value=5
copy%object=>local
call show()
print *,copy%object%get()
nullify(copy%object)
deallocate(local)
contains
subroutine show()
type(child)::local
local%value=9
print *,local%value
end subroutine
end
''', '9\n15')

    def test_move_alloc_transfers_storage(self):
        self.check(ALLOC_TYPES + 'end module\n' + MAIN + '''type(child),allocatable,target::other
allocate(target)
call move_alloc(from=target,to=other)
print *,allocated(target),allocated(other),other%value
end
''', 'F T 1')

    def test_separate_compilation(self):
        module = ALLOC_TYPES + '''contains
subroutine initialize()
allocate(target)
end subroutine
function make_entry() result(x)
type(entry)::x
x%object=>target
end function
subroutine destroy()
deallocate(target)
end subroutine
end module
'''
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text(MAIN + '''call initialize()
copy=make_entry()
call copy%object%add(4)
print *,copy%object%get()
nullify(copy%object)
call destroy()
end
''')
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result = subprocess.run(command, cwd=self.work, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
        result = subprocess.run([self.work/'main.exe'],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),['15'])

if __name__ == '__main__': unittest.main()

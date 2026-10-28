"""MOVE_ALLOC transfers concrete scalar records and their allocation identity."""
import subprocess
import unittest
import test_polymorphic_pointers as shared
from test_polymorphic_pointer_allocated_targets import ALLOC_TYPES
from test_polymorphic_pointer_saved_owners import MAIN
from test_record_read import COMPILER, ROOT

RECORDS = '''module records
implicit none
type item
integer::value=1
integer,allocatable::data(:)
character(len=6),allocatable::label(:)
end type
end module
program p
use records
implicit none
type(item),allocatable::a,b
'''

class DerivedMoveAlloc(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check
    reject = shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xderived_move_alloc.f90').read_text(),
                   'F T 7\n2.0 3.0\nmoved')

    def test_scalar_character_component(self):
        self.check(RECORDS.replace('integer::value=1',
                    "integer::value=1\ncharacter(len=4)::tag='init'")+'''allocate(a)
a%tag='done'
call move_alloc(a,b)
print *,b%tag,allocated(a)
deallocate(b)
end
''','done F')

    def test_without_target_and_allocated_destination(self):
        self.check(RECORDS+'''allocate(a,b)
a%value=7
a%data=[2,3]
a%label=['source']
b%data=[99]
b%label=['oldold']
call move_alloc(to=b,from=a)
print *,allocated(a),allocated(b),b%value,b%data,b%label
allocate(a)
a%data=[8]
print *,a%value,a%data,b%data
deallocate(a,b)
end
''', 'F T 7 2 3 source\n1 8 2 3')

    def test_unallocated_source_deallocates_destination(self):
        self.check(RECORDS+'''allocate(b)
b%data=[1,2]
call move_alloc(a,b)
print *,allocated(a),allocated(b)
call move_alloc(a,b)
print *,allocated(a),allocated(b)
end
''','F F\nF F')

    def test_mixed_component_types_and_ranks(self):
        self.check('''module mixed
implicit none
type item
integer,allocatable::indices(:,:)
real,allocatable::weights(:,:,:)
logical,allocatable::mask(:)
character(len=3),allocatable::names(:)
integer::fixed(2)=[7,8]
end type
contains
subroutine initialize(x)
type(item),intent(inout)::x
allocate(x%indices(-1:0,3:4),x%weights(2,2,2))
x%indices=4
x%weights=2.5
end subroutine
end module
program p
use mixed
implicit none
type(item),allocatable::a,b
allocate(a)
call initialize(a)
a%mask=[.true.,.false.]
a%names=['one','two']
call move_alloc(a,b)
print *,allocated(a),sum(b%indices),sum(b%weights),b%mask,b%names,b%fixed
print *,lbound(b%indices),ubound(b%indices)
deallocate(b)
end
''','F 16 20.0 T F onetwo 7 8\n-1 3 0 4')

    def test_unset_component_is_not_read_during_transfer(self):
        source=RECORDS.replace('value=1','value')
        self.check(source+'''allocate(a)
a%data=[3,4]
call move_alloc(a,b)
b%value=9
print *,b%value,b%data,allocated(b%label)
end
''','9 3 4 F')

    def test_uninitialized_component_still_diagnosed(self):
        source=RECORDS.replace('value=1','value')
        self.check(source+'''allocate(a)
call move_alloc(a,b)
print *,b%value
end
''',error='uninitialized',reference=False)

    def test_local_pointer_follows_repeated_moves(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child),allocatable,target::a,b
class(base),pointer::ptr,q
allocate(a)
a%value=4
ptr=>a
call move_alloc(a,b)
q=>b
print *,associated(ptr,b),associated(ptr,q),ptr%get()
call ptr%add(3)
print *,b%value,allocated(a)
call move_alloc(b,a)
print *,associated(ptr,a),ptr%get(),associated(ptr,q)
deallocate(a)
end
''','T T 14\n7 F\nT 17 T')

    def test_owner_copy_and_select_type(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child),allocatable,target::a,b
allocate(a)
a%value=5
copy%object=>a
call move_alloc(a,b)
select type (x=>copy%object)
type is(child)
print *,x%get(),associated(copy%object,b)
end select
call copy%object%add(2)
print *,b%value
deallocate(b)
end
''','15 T\n7')

    def test_old_destination_pointer_does_not_follow_source(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child),allocatable,target::a,b
allocate(a,b)
copy%object=>b
call move_alloc(a,b)
print *,copy%object%get()
end
''',error='deallocated or reallocated target',reference=False)

    def test_reallocating_source_does_not_steal_pointer(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child),allocatable,target::a,b
allocate(a)
a%value=8
copy%object=>a
call move_alloc(a,b)
allocate(a)
a%value=99
print *,copy%object%get(),associated(copy%object,b),associated(copy%object,a)
end
''','18 T F')

    def test_module_move_and_persistent_owner(self):
        source=ALLOC_TYPES+'''type(child),allocatable,target::other
contains
subroutine transfer()
call move_alloc(target,other)
end subroutine
end module
'''
        self.check(source+MAIN+'''allocate(target)
target%value=6
current%object=>target
call transfer()
copy=current
print *,copy%object%get(),associated(copy%object,other),allocated(target)
call copy%object%add(3)
print *,other%value
nullify(current%object,copy%object)
deallocate(other)
end
''','16 T F\n9')

    def test_forward_owner_after_move_with_unallocated_source(self):
        self.check(ALLOC_TYPES+'''contains
subroutine add(x)
type(entry),intent(inout)::x
call x%object%add(3)
end subroutine
end module
'''+MAIN+'''type(child),allocatable,target::a,b
allocate(a)
a%value=5
copy%object=>a
call move_alloc(a,b)
call add(copy)
print *,b%value,copy%object%get()
deallocate(b)
end
''','8 18')

    def test_same_actual_rejected_with_location(self):
        self.reject(RECORDS+'allocate(a)\ncall move_alloc(a,a)\nend\n',
                    'MOVE_ALLOC FROM and TO must be different variables')

    def test_stat_and_errmsg_success(self):
        self.check(RECORDS+'''integer::status=-1
character(len=4)::message='keep'
allocate(a)
call move_alloc(to=b,stat=status,errmsg=message,from=a)
print *,status,message,allocated(a),allocated(b)
end
''','0 keep F T')

    def test_pure_local_move(self):
        declarations=RECORDS.split('program p\n')[0]
        self.check(declarations+'''program p
use records
implicit none
integer::value
call work(value)
print *,value
contains
pure subroutine work(value)
integer,intent(out)::value
type(item),allocatable::a,b
allocate(a)
a%value=7
call move_alloc(a,b)
value=b%value
deallocate(b)
end subroutine
end
''','7')

    def test_saved_allocatables_without_target(self):
        declarations=RECORDS.split('program p\n')[0]
        self.check(declarations+'''program p
use records
implicit none
call step()
call step()
contains
subroutine step()
type(item),allocatable,save::a,b
if (.not.allocated(b)) then
allocate(a)
a%value=6
a%data=[4,5]
call move_alloc(a,b)
else
b%value=b%value+1
end if
print *,allocated(a),b%value,b%data
end subroutine
end
''','F 6 4 5\nF 7 4 5')

    def test_component_pointer_keeps_owned_storage(self):
        self.check(RECORDS.replace('allocatable::a,b','allocatable,target::a,b')+'''integer,pointer::ptr(:)
allocate(a)
a%data=[4,5]
ptr=>a%data
call move_alloc(a,b)
ptr(2)=9
print *,ptr
print *,b%data
nullify(ptr)
deallocate(b)
end
''','4 9\n4 9')

    def test_pointer_to_scalar_component_follows_move(self):
        self.check(RECORDS.replace('allocatable::a,b','allocatable,target::a,b')+'''integer,pointer::ptr
allocate(a)
a%value=4
ptr=>a%value
call move_alloc(a,b)
ptr=9
print *,ptr,b%value
nullify(ptr)
deallocate(b)
end
''','9 9')

    def test_module_scalar_component_pointer_survives_call(self):
        declarations=RECORDS.split('end module\n')[0]
        self.check(declarations+'''type(item),allocatable,target::a,b
contains
subroutine transfer()
call move_alloc(a,b)
end subroutine
end module
program p
use records
implicit none
integer,pointer::ptr
allocate(a)
a%value=4
ptr=>a%value
call transfer()
ptr=9
print *,ptr,b%value
call move_alloc(b,a)
print *,ptr,a%value
nullify(ptr)
deallocate(a)
end
''','9 9\n9 9')

    def test_fixed_matrix_and_logical_component_pointers(self):
        self.check('''module matrices
implicit none
type item
real::weight(2,2)=1.0
logical::enabled=.false.
end type
end module
program p
use matrices
implicit none
type(item),allocatable,target::a,b
real,pointer::weights(:,:)
logical,pointer::flag
allocate(a)
weights=>a%weight
flag=>a%enabled
call move_alloc(a,b)
weights(2,2)=9.0
flag=.true.
print *,sum(b%weight),b%enabled
call move_alloc(b,a)
print *,sum(weights),flag,sum(a%weight)
nullify(weights,flag)
deallocate(a)
end
''','12.0 T\n12.0 T 12.0')

    def test_owned_derived_array_and_pointer_survive(self):
        self.check('''module boxes
implicit none
type row
integer::value
end type
type box
type(row),allocatable::rows(:)
end type
contains
subroutine initialize(x)
type(box),intent(inout)::x
allocate(x%rows(2))
x%rows=[row(4),row(5)]
end subroutine
end module
program p
use boxes
implicit none
type(box),allocatable,target::a,b
type(row),pointer::ptr(:),other(:)
allocate(a)
call initialize(a)
ptr=>a%rows
call move_alloc(a,b)
ptr(2)%value=9
other=>b%rows
print *,ptr%value,allocated(a)
print *,other%value,associated(ptr,other)
nullify(ptr,other)
deallocate(b)
end
''','4 9 F\n4 9 T')

    def test_move_to_module_before_returning_owner(self):
        source=ALLOC_TYPES+'''contains
function factory() result(x)
type(entry)::x
type(child),allocatable,target::local
allocate(local)
local%value=7
x%object=>local
call move_alloc(local,target)
end function
end module
'''
        self.check(source+MAIN+'''copy=factory()
current=copy
print *,current%object%get(),associated(copy%object,target)
deallocate(target)
end
''','17 T')

    def test_module_to_automatic_cannot_escape(self):
        self.check(ALLOC_TYPES+'''contains
function factory() result(x)
type(entry)::x
type(child),allocatable,target::local
allocate(target)
x%object=>target
call move_alloc(target,local)
end function
end module
'''+MAIN+'copy=factory()\nend\n',
                   error='function result currently requires a saved or module target',reference=False)

    def test_separate_compilation(self):
        module=ALLOC_TYPES+'''type(child),allocatable,target::other
contains
subroutine initialize()
allocate(target)
target%value=8
current%object=>target
end subroutine
subroutine transfer()
call move_alloc(target,other)
end subroutine
end module
'''
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text(MAIN+'''call initialize()
copy=current
call transfer()
call copy%object%add(2)
print *,copy%object%get(),other%value,associated(copy%object,other)
nullify(current%object,copy%object)
deallocate(other)
end
''')
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result=subprocess.run(command,cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
        result=subprocess.run([self.work/'main.exe'],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),['20','10','T'])

    def test_different_types_rejected(self):
        self.reject(RECORDS.replace('type(item),allocatable::a,b',
                    'type other\ninteger::value\nend type\ntype(other),allocatable::b\ntype(item),allocatable::a')+
                    'call move_alloc(a,b)\nend\n','same declared type and rank')

    def test_non_target_component_cannot_be_pointer_target(self):
        self.reject(RECORDS+'''integer,pointer::ptr(:)
allocate(a)
a%data=[1,2]
ptr=>a%data
end
''','requires TARGET attribute')

if __name__ == '__main__': unittest.main()

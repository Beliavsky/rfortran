"""SOURCE and MOLD for concrete scalar allocated pointer targets."""
import unittest
import subprocess
import test_polymorphic_pointers as shared
from test_polymorphic_pointer_saved_owners import MAIN
from test_polymorphic_pointer_allocated_targets import ALLOC_TYPES
from test_record_read import COMPILER, ROOT

class AllocationModels(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check
    reject=shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xallocated_target_models.f90').read_text(), '7 13.0 6.0\n1 F')

    def test_source_copies_value_and_preserves_source(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child)::value
value%value=6
allocate(target,source=value)
copy%object=>target
call copy%object%add(3)
print *,copy%object%get(),value%value,associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''','19 6 T')

    def test_constructor_source(self):
        types=ALLOC_TYPES.replace('type(child),allocatable,target::target','type(base),allocatable,target::target')
        self.check(types+'end module\n'+MAIN+'''allocate(target,source=base(4))
copy%object=>target
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''','4')

    def test_source_deep_copies_owned_components(self):
        types=ALLOC_TYPES.replace('integer::value=1','integer::value=1\nreal,allocatable::data(:)')
        self.check(types+'end module\n'+MAIN+'''type(child)::value
value%data=[2.0,4.0]
allocate(target,source=value)
copy%object=>target
target%data(1)=9.0
print *,sum(copy%object%data),sum(value%data)
deallocate(value%data)
print *,sum(copy%object%data)
nullify(copy%object)
deallocate(target)
end
''','13.0 6.0\n13.0')

    def test_multiple_targets_have_independent_owned_components(self):
        types=ALLOC_TYPES.replace('integer::value=1','integer::value=1\nreal,allocatable::data(:)')
        types=types.replace('type(child),allocatable,target::target','type(child),allocatable,target::target,other')
        self.check(types+'end module\n'+MAIN+'''type(child)::value
value%data=[2.0,4.0]
allocate(target,other,source=value)
target%data(1)=9.0
print *,sum(target%data),sum(other%data),sum(value%data)
deallocate(target,other,value%data)
end
''','13.0 6.0 6.0')

    def test_mold_uses_defaults_without_copying(self):
        types=ALLOC_TYPES.replace('integer::value=1','integer::value=1\nreal,allocatable::data(:)')
        self.check(types+'end module\n'+MAIN+'''type(child)::value
value%value=99
value%data=[2.0,4.0]
allocate(target,mold=value)
copy%object=>target
print *,copy%object%get(),allocated(target%data),sum(value%data)
nullify(copy%object)
deallocate(target,value%data)
end
''','11 F 6.0')

    def test_mold_does_not_read_undefined_components(self):
        types=ALLOC_TYPES.replace('integer::value=1','integer::value')
        self.check(types+'end module\n'+MAIN+'''type(child)::value
allocate(target,mold=value)
copy%object=>target
copy%object%value=8
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''','18')
        self.check(types+'end module\n'+MAIN+'''type(child)::value
allocate(target,mold=value)
copy%object=>target
print *,copy%object%value
end
''',error='read of uninitialized variable',reference=False)

    def test_source_evaluated_once_for_multiple_targets(self):
        source=ALLOC_TYPES.replace('type(child),allocatable,target::target','type(child),allocatable,target::target,other\ninteger::calls=0')+'''contains
function make_value() result(x)
type(child)::x
calls=calls+1
x%value=calls+4
end function
end module
'''+MAIN+'''allocate(target,other,source=make_value())
copy%object=>target
current%object=>other
call copy%object%add(2)
print *,calls,copy%object%get(),current%object%get()
nullify(copy%object,current%object)
deallocate(target,other)
end
'''
        self.check(source,'1 17 15')

    def test_failed_allocate_preserves_value_and_alias(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child)::value
integer::ierr
character(len=100)::message
allocate(target)
target%value=7
copy%object=>target
value%value=99
allocate(target,source=value,stat=ierr,errmsg=message)
print *,ierr/=0,len_trim(message)>0,copy%object%get(),associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''','T T 17 T')

    def test_allocated_scalar_source(self):
        source=ALLOC_TYPES.replace('type(child),allocatable,target::target','type(child),allocatable,target::target,other')+'end module\n'+MAIN+'''allocate(other)
other%value=8
allocate(target,source=other)
copy%object=>target
deallocate(other)
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
'''
        self.check(source,'18')

    def test_model_from_array_element_and_component(self):
        self.check(ALLOC_TYPES+'''type holder
type(child)::item
end type
end module
'''+MAIN+'''type(child)::values(2)
type(holder)::value
values(2)%value=7
allocate(target,source=values(2))
copy%object=>target
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
value%item%value=8
allocate(target,source=value%item)
copy%object=>target
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''','17\n18')

    def test_generic_function_source_and_mold(self):
        self.check(ALLOC_TYPES+'''integer::calls=0
interface make_value
module procedure make_integer
end interface
contains
function make_integer(n) result(x)
integer,intent(in)::n
type(child)::x
calls=calls+1
x%value=n
end function
end module
'''+MAIN+'''allocate(target,source=make_value(7))
copy%object=>target
print *,calls,copy%object%get()
nullify(copy%object)
deallocate(target)
allocate(target,mold=make_value(99))
copy%object=>target
print *,calls,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''','1 17\n1 11')

    def test_failed_mold_allocation_preserves_alias(self):
        self.check(ALLOC_TYPES+'end module\n'+MAIN+'''type(child)::value
integer::ierr
allocate(target)
target%value=7
copy%object=>target
allocate(target,mold=value,stat=ierr)
print *,ierr/=0,copy%object%get(),associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''','T 17 T')

    def test_unallocated_concrete_mold(self):
        types=ALLOC_TYPES.replace('type(child),allocatable,target::target','type(child),allocatable,target::target,other')
        self.check(types+'end module\n'+MAIN+'''allocate(target,mold=other)
copy%object=>target
print *,allocated(other),copy%object%get()
nullify(copy%object)
deallocate(target)
end
''','F 11')

    def test_failed_self_source_preserves_owned_storage(self):
        types=ALLOC_TYPES.replace('integer::value=1','integer::value=1\nreal,allocatable::data(:)')
        self.check(types+'end module\n'+MAIN+'''integer::ierr
allocate(target)
target%data=[2.0,4.0]
copy%object=>target
allocate(target,source=target,stat=ierr)
print *,ierr/=0,sum(copy%object%data),associated(copy%object,target)
nullify(copy%object)
deallocate(target)
end
''','T 6.0 T')

    def test_polymorphic_model_has_specific_diagnostic(self):
        self.reject(ALLOC_TYPES+'''contains
subroutine build(value)
class(child),intent(in)::value
allocate(target,mold=value)
end subroutine
end module
'''+MAIN+'end\n','polymorphic SOURCE/MOLD expressions')

    def test_pure_procedure_local_allocation(self):
        self.check(ALLOC_TYPES+'''contains
pure integer function total(value) result(n)
type(child),intent(in)::value
type(child),allocatable,target::local
allocate(local,source=value)
n=local%value+local%extra
deallocate(local)
end function
end module
'''+MAIN+'''type(child)::value
value%value=4
print *,total(value)
end
''','14')

    def test_source_uses_intrinsic_copy_with_defined_assignment(self):
        self.check(ALLOC_TYPES+'''integer::assignments=0
interface assignment(=)
module procedure assign_child
end interface
contains
subroutine assign_child(to,from)
type(child),intent(out)::to
type(child),intent(in)::from
assignments=assignments+1
to%value=from%value+100
end subroutine
end module
'''+MAIN+'''type(child)::value,assigned
value%value=4
allocate(target,source=value)
copy%object=>target
print *,assignments,copy%object%get()
assigned=value
print *,assignments,assigned%value,target%value
nullify(copy%object)
deallocate(target)
end
''','0 14\n1 104 4')

    def test_rejects_typed_allocation_with_model(self):
        self.reject(ALLOC_TYPES+'end module\n'+MAIN+'''type(child)::value
allocate(child::target,source=value)
end
''','type-spec cannot be combined')

    def test_rejects_invalid_models(self):
        for declaration,options,message in (
            ('type(base)::value','source=value','type must match'),
            ('type(base)::value','mold=value','type must match'),
            ('type(child)::value(2)','source=value','scalar derived-type'),
            ('integer::value','mold=value','scalar derived-type'),
            ('type(child)::value','source=value,mold=value','only one SOURCE or MOLD'),
            ('type(child)::value','source=value,source=value','only one SOURCE or MOLD'),
        ):
            with self.subTest(options=options,declaration=declaration):
                self.reject(ALLOC_TYPES+'end module\n'+MAIN+declaration+'\nallocate(target,'+options+')\nend\n',message)

    def test_separate_compilation(self):
        module=ALLOC_TYPES+'''contains
function make_entry(n) result(x)
integer,intent(in)::n
type(entry)::x
type(child)::value
value%value=n
allocate(target,source=value)
x%object=>target
end function
end module
'''
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text(MAIN+'''copy=make_entry(7)
print *,copy%object%get()
nullify(copy%object)
deallocate(target)
end
''')
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],[COMPILER,'main.f90','m.obj','-o','main.exe']):
            result=subprocess.run(command,cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
        result=subprocess.run([self.work/'main.exe'],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),['17'])

if __name__=='__main__':unittest.main()

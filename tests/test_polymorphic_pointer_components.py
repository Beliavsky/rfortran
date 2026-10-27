"""Scalar polymorphic pointer components in local owners and owner dummies."""
import unittest
import subprocess
import test_polymorphic_pointers as shared
from test_record_read import ROOT, COMPILER

HEADER=shared.TYPES+'''program p
use pointer_shapes
implicit none
type entry
class(base),pointer::object=>null()
end type
'''

class PolymorphicPointerComponents(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check
    reject=shared.PolymorphicPointers.reject

    def test_repro(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_array_component.f90').read_text(),'T 7')

    def test_dispatch_and_reassociation(self):
        self.check(HEADER+'''type(entry)::list(2)
    type(base),target::a
    type(child),target::b
    integer::i
    print *,associated(list(1)%object),associated(list(2)%object)
    list(1)%object=>a
    list(2)%object=>b
    do i=1,2
    print *,list(i)%object%get()
    call list(i)%object%add(i)
    end do
    print *,a%value,b%value
    list(1)%object=>b
    print *,list(1)%object%get()
    nullify(list(2)%object)
    print *,associated(list(1)%object),associated(list(2)%object)
    end
    ''','F F\n1\n11\n2 3\n13\nT F')

    def test_aliasing_and_owner_copy(self):
        self.check(HEADER+'''type(entry)::list(2),copy(2)
    type(child),target::a,b
    list(1)%object=>a
    list(2)%object=>list(1)%object
    copy=list
    list(1)%object=>b
    list(2)%object%value=9
    print *,associated(list(1)%object,b),associated(list(2)%object,a)
    print *,associated(copy(1)%object,list(2)%object),a%value,b%value
    end
    ''','T T\nT 9 1')

    def test_scalar_owner(self):
        self.check(HEADER+'''type(entry)::box
    type(base),target::a
    box%object=>a
    box%object%value=8
    print *,box%object%get(),a%value
    nullify(box%object)
    print *,associated(box%object)
    end
    ''','8 8\nF')

    def test_unassociated_access(self):
        self.check(HEADER+'''type(entry)::list(2)
    print *,list(1)%object%get()
    end
    ''',error='use of unassociated polymorphic pointer `list%object`',reference=False)


    def test_runtime_allocated_owners_and_general_rank(self):
        for rank in (1,2,3,15):
            with self.subTest(rank=rank):
                deferred=','.join(':' for _ in range(rank))
                shape=','.join(['1']*(rank-1)+['2'])
                index=','.join(['1']*(rank-1)+['2'])
                self.check(HEADER+f'''type(entry),allocatable::list({deferred})
type(child),target::a
allocate(list({shape}))
print *,associated(list({index})%object)
list({index})%object=>a
call list({index})%object%add(3)
print *,list({index})%object%get(),associated(list({index})%object,a)
deallocate(list)
print *,a%value
end
''','F\n14 T\n4')

    def test_subscripts_are_evaluated_once(self):
        self.check(HEADER+'''type(entry)::list(2)
type(base),target::a
integer::count
count=0
list(next_index())%object=>a
print *,count
call list(next_index())%object%add(2)
print *,count,a%value
contains
integer function next_index()
count=count+1
next_index=1
end function
end
''','1\n2 3',reference=False)  # gfortran reevaluates this impure subscript.

    def test_while_condition_reloads_association(self):
        self.check(HEADER+'''type(entry)::list(2)
type(base),target::a
list(1)%object=>a
do while(associated(list(1)%object))
call list(1)%object%add(1)
nullify(list(1)%object)
end do
if(associated(list(2)%object)) then
print *,list(2)%object%get()
end if
print *,a%value,associated(list(1)%object)
end
''','2 F')

    def test_interfaces_and_unsupported_descriptors_are_explicit(self):
        self.reject(HEADER+'''type(entry)::list(2)
call external_owner(list)
end
''','requires a component descriptor interface')
        self.check(HEADER+'''type(entry)::list(2)
contains
subroutine take(x)
type(entry),intent(in)::x
end subroutine
end
''','')
        self.reject(HEADER.replace('::object=>','::object(:)=>')+'''type(entry)::list(2)
end
''','polymorphic POINTER component arrays')
        self.reject(HEADER.replace('class(base),pointer::object','class(*),pointer::object')+'''type(entry)::list(2)
end
''','unlimited polymorphic POINTER components')

    def test_constructor_and_target_limits_are_explicit(self):
        self.reject(HEADER+'''type(entry)::box
type(base),target::a
box=entry(a)
end
''','explicit constructor values for polymorphic POINTER components')
        self.reject(HEADER+'''type(entry)::box
type(base)::a
box%object=>a
end
''','must be a compatible concrete scalar TARGET variable')
        self.reject(HEADER+'''type(entry)::box
contains
function make() result(x)
type(entry)::x
end function
end
''','function results with polymorphic POINTER components')

    def test_module_type_and_nested_scalar_components(self):
        self.check(shared.TYPES+'''module entry_types
use pointer_shapes,only:base
implicit none
type entry
class(base),pointer::object=>null()
end type
type outer
type(entry)::part
end type
end module
program p
use pointer_shapes,only:child
use entry_types,only:box_type=>outer
implicit none
type(box_type)::list(2)
type(child),target::a
list(2)%part%object=>a
call list(2)%part%object%add(4)
print *,list(2)%part%object%get(),associated(list(1)%part%object)
end
''','15 F')

    def test_owner_dummy_repro(self):
        self.check((ROOT/'tests/programs/xpolymorphic_pointer_component_dummy.f90').read_text(),
                   'T 1\n3\nT 11\nT 3\nF\nF 3 3')

    def test_owner_dummy_array_forwarding_and_aliasing(self):
        self.check(HEADER+'''type(entry)::list(2)
type(child),target::a,b
list(1)%object=>a
list(2)%object=>a
call forward(list)
print *,a%value,b%value,associated(list(2)%object,b)
contains
subroutine forward(x)
type(entry),intent(inout)::x(:)
call alter(x(1),x(2),b)
end subroutine
subroutine alter(x,y,target)
type(entry),intent(in)::x
type(entry),intent(inout)::y
type(child),target,intent(inout)::target
call x%object%add(2)
print *,y%object%get(),associated(x%object,y%object)
y%object=>target
call y%object%add(4)
end subroutine
end
''','13 T\n3 5 T')

    def test_owner_function_and_keywords(self):
        self.check(HEADER+'''type(entry)::box
type(child),target::a
box%object=>a
print *,get(x=box),get(box)
contains
integer function get(x)
type(entry),intent(in)::x
get=x%object%get()
end function
end
''','11 11')

    def test_multidimensional_allocated_owner_dummy(self):
        self.check(HEADER+'''type(entry),allocatable::list(:,:,:)
type(child),target::a
allocate(list(1,1,2))
list(1,1,2)%object=>a
call update(list)
print *,a%value,associated(list(1,1,2)%object)
contains
subroutine update(x)
type(entry),intent(inout)::x(:,:,:)
call x(1,1,2)%object%add(3)
nullify(x(1,1,2)%object)
end subroutine
end
''','4 F')

    def test_owner_intent_in_reassociation_rejected(self):
        self.reject(HEADER+'''type(entry)::box
call clear(box)
contains
subroutine clear(x)
type(entry),intent(in)::x
nullify(x%object)
end subroutine
end
''','cannot change association of an INTENT(IN) pointer component')

    def test_independent_caller_environments(self):
        self.check(HEADER+'''call first()
call second()
contains
subroutine first()
type(entry)::box
type(child),target::z
box%object=>z
call use_owner(box)
print *,z%value
end subroutine
subroutine second()
type(entry)::box
type(base),target::a
type(child),target::b
box%object=>b
call use_owner(box)
box%object=>a
call use_owner(box)
print *,a%value,b%value
end subroutine
subroutine use_owner(x)
type(entry),intent(in)::x
call x%object%add(1)
print *,x%object%get()
end subroutine
end
''','12\n2\n12\n2\n2 2')

    def test_pure_function_borrows_readonly_target(self):
        self.check(HEADER+'''type(entry)::box
type(child),target::a
box%object=>a
print *,value(box)
contains
pure integer function value(x) result(n)
type(entry),intent(in)::x
n=x%object%value
end function
end
''','1')

    def test_unassociated_owner_dummy_and_out_rebind(self):
        self.check(HEADER+'''type(entry)::box
type(child),target::a
call empty(box)
call make(box,a)
print *,box%object%get(),associated(box%object,a)
contains
subroutine empty(x)
type(entry),intent(in)::x
print *,associated(x%object)
end subroutine
subroutine make(x,target)
type(entry),intent(out)::x
type(child),target,intent(inout)::target
print *,associated(x%object)
x%object=>target
end subroutine
end
''','F\nF\n11 T')

    def test_separate_module_interface(self):
        source=(ROOT/'tests/programs/xpolymorphic_pointer_component_dummy.f90').read_text()
        module,main=source.split('program p\n',1)
        (self.work/'m.f90').write_text(module)
        (self.work/'main.f90').write_text('program p\n'+main)
        for command in ([COMPILER,'-c','m.f90','-o','m.obj'],
                        [COMPILER,'main.f90','m.obj','-o','main.exe']):
            result=subprocess.run(command,cwd=self.work,capture_output=True,text=True,timeout=60)
            self.assertEqual(result.returncode,0,result.stderr)
        result=subprocess.run([self.work/'main.exe'],capture_output=True,text=True,timeout=15)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout.split(),'T 1 3 T 11 T 3 F F 3 3'.split())

    def test_separate_target_attributes(self):
        self.check(HEADER+'''type(entry)::box
type(child)::a,b
target::a,b
box%object=>a
call use_owner(box,b)
print *,associated(box%object,b),b%value
contains
subroutine use_owner(x,y)
type(entry)::x
type(child)::y
target::y
x%object=>y
call x%object%add(2)
end subroutine
end
''','T 3')

    def test_new_local_target_interface_is_explicit(self):
        self.reject(HEADER+'''type(entry)::box
call change(box)
contains
subroutine change(x)
type(entry),intent(inout)::x
type(child),target::local
x%object=>local
end subroutine
end
''','local TARGET storage in a procedure')

    def test_unused_uninitialized_target_is_not_read(self):
        self.check('''module m
implicit none
type node
integer::value
end type
type entry
class(node),pointer::object=>null()
end type
contains
subroutine show(x)
type(entry),intent(in)::x
print *,x%object%value
end subroutine
end module
program p
use m
implicit none
type(entry)::box
type(node),target::a,unrelated
a%value=7
box%object=>a
call show(box)
end
''','7')

    def test_original_host_scope_survives_specialization(self):
        self.check(HEADER+'''integer::count
count=0
call first()
print *,count
contains
subroutine first()
type(entry)::box
type(child),target::a
integer::count
count=100
box%object=>a
call use_owner(box)
print *,count,a%value
end subroutine
subroutine use_owner(x)
type(entry),intent(in)::x
count=count+1
call x%object%add(2)
end subroutine
end
''','100 3\n1')

    def test_borrowed_target_initialization_checks(self):
        source='''module m
implicit none
type node
integer::value
end type
type entry
class(node),pointer::object=>null()
end type
contains
subroutine update(x)
type(entry),intent(in)::x
x%object%value=7
end subroutine
end module
program p
use m
implicit none
type(entry)::box
type(node),target::a
box%object=>a
call update(box)
print *,a%value
end
'''
        self.check(source,'7')
        self.reject(source.replace('x%object%value=7','print *,x%object%value'),
                    'read of uninitialized variable')

    def test_module_data_survives_specialization(self):
        source=(ROOT/'tests/programs/xpolymorphic_pointer_component_dummy.f90').read_text()
        source=source.replace('type entry\n','integer::count=0\ntype entry\n')
        source=source.replace('print *,associated(x%object),x%object%get()',
                              'count=count+1\nprint *,count,associated(x%object),x%object%get()')
        self.check(source,'1 T 1\n3\n2 T 11\nT 3\nF\nF 3 3')

    def test_recursive_owner_interface_is_explicit(self):
        self.reject(HEADER+'''type(entry)::box
call recurse(box)
contains
recursive subroutine recurse(x)
type(entry),intent(in)::x
call recurse(x)
end subroutine
end
''','recursive procedures with polymorphic POINTER components')

if __name__=='__main__':unittest.main()

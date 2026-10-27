"""Local scalar pointer components in scalar records and record arrays."""
import unittest
import test_polymorphic_pointers as shared
from test_record_read import ROOT

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
        self.reject(HEADER+'''type(entry)::list(2)
contains
subroutine take(x)
type(entry),intent(in)::x
end subroutine
end
''','dummy arguments with polymorphic POINTER components')
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

if __name__=='__main__':unittest.main()

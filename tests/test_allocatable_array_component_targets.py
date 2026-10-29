"""Array-element owners retain scalar component allocation identities."""
import unittest
import test_polymorphic_pointers as shared
import test_module_compilation
from test_allocatable_component_targets import MODULE, MAIN
from test_record_read import ROOT

DECL='''use component_target_mod
implicit none
type(container),target::boxes(3)
type(reference)::p,q
'''

class ArrayComponentTargets(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check
    reject=shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xallocatable_array_component_target.f90').read_text(),'7 9\nF 8 T')

    def test_repeated_same_element(self):
        main=('program demo'+MAIN).replace('type(container),target::box','type(container),target::box(3)').replace('box%item','box(2)%item')
        self.check(MODULE+main,'3 3\n9 9 T')

    def test_separate_target_attribute(self):
        self.check(MODULE+DECL.replace('type(container),target::boxes(3)','type(container)::boxes(3)\ntarget::boxes')+'''call create(boxes(2)%item,p)
p%object%value=8
print *,boxes(2)%item%value
end
''','8')

    def test_independent_elements(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
call create(boxes(3)%item,q)
p%object%value=7
q%object%value=9
print *,boxes(1)%item%value,boxes(3)%item%value
print *,associated(p%object,boxes(1)%item),associated(p%object,boxes(3)%item)
end
''','7 9\nT F')

    def test_changed_index(self):
        module=MODULE.replace('end module','''subroutine indexed(x,p,i)
type(resource),allocatable,target,intent(inout)::x
type(reference),intent(out)::p
integer,intent(inout)::i
call create(x,p)
i=3
end subroutine
end module''')
        self.check(module+DECL+'''integer::i
i=1
call indexed(boxes(i)%item,p,i)
p%object%value=8
print *,i,boxes(1)%item%value,allocated(boxes(3)%item)
end
''','3 8 F')

    def test_loop_and_repeated_calls(self):
        self.check(MODULE+DECL+'''integer::i
do i=1,3
call create(boxes(i)%item,q)
end do
call create(boxes(1)%item,p)
p%object%value=7
print *,boxes(1)%item%value,boxes(2)%item%value,q%object%value
end
''','7 3 3')

    def test_deallocate_invalidates_pointer(self):
        self.check(MODULE+DECL+'''call create(boxes(2)%item,p)
deallocate(boxes(2)%item)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_replacement_invalidates_pointer(self):
        module=MODULE.replace('end module','''subroutine reset(x)
type(resource),allocatable,target,intent(out)::x
allocate(x)
end subroutine
end module''')
        self.check(module+DECL+'''call create(boxes(2)%item,p)
call reset(boxes(2)%item)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_move(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
call move_alloc(boxes(1)%item,boxes(3)%item)
p%object%value=7
print *,allocated(boxes(1)%item),boxes(3)%item%value
print *,associated(p%object,boxes(3)%item)
end
''','F 7\nT')

    def test_move_between_root_and_component(self):
        self.check(MODULE+DECL+'''type(resource),allocatable,target::root
call create(boxes(1)%item,p)
call move_alloc(boxes(1)%item,root)
print *,p%object%value,associated(p%object,root)
call move_alloc(root,boxes(3)%item)
p%object%value=8
print *,boxes(3)%item%value,associated(p%object,boxes(3)%item)
end
''','3 T\n8 T')

    def test_direct_association(self):
        self.check(MODULE+DECL+'''class(resource),pointer::r
allocate(boxes(2)%item)
r=>boxes(2)%item
r%value=8
print *,boxes(2)%item%value,associated(r,boxes(2)%item)
end
''','8 T')

    def test_rank_two_and_lower_bounds(self):
        self.check(MODULE+DECL.replace('boxes(3)','boxes(-1:1,4:5)')+'''call create(boxes(0,5)%item,p)
p%object%value=8
print *,boxes(0,5)%item%value,associated(p%object,boxes(0,5)%item)
end
''','8 T')

    def test_rank_three(self):
        self.check(MODULE+DECL.replace('boxes(3)','boxes(2,2,2)')+'''call create(boxes(2,1,2)%item,p)
p%object%value=8
print *,boxes(2,1,2)%item%value
end
''','8')

    def test_module_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type(container),target::boxes(3)
contains
subroutine create''')
        self.check(module+DECL.replace('type(container),target::boxes(3)\n','')+'''call create(boxes(2)%item,p)
p%object%value=8
print *,boxes(2)%item%value
end
''','8')

    def test_copy_has_independent_identity(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
boxes(2)=boxes(1)
boxes(2)%item%value=20
print *,p%object%value,boxes(2)%item%value
print *,associated(p%object,boxes(1)%item),associated(p%object,boxes(2)%item)
end
''','3 20\nT F')

    def test_whole_array_copy(self):
        self.check(MODULE+DECL+'''type(container),target::other(3)
call create(boxes(1)%item,p)
other=boxes
other(1)%item%value=20
print *,p%object%value,other(1)%item%value
print *,associated(p%object,boxes(1)%item),associated(p%object,other(1)%item)
end
''','3 20\nT F')

    def test_copy_invalidates_old_destination(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
call create(boxes(2)%item,q)
boxes(2)=boxes(1)
print *,q%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_section_copy_preserves_untouched_target(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
call create(boxes(3)%item,q)
boxes(2:2)=boxes(1:1)
print *,p%object%value,q%object%value
print *,associated(p%object,boxes(2)%item),associated(q%object,boxes(3)%item)
end
''','3 3\nF T')

    def test_vector_copy(self):
        self.check(MODULE+DECL+'''type(container)::source(2)
allocate(source(1)%item,source(2)%item)
source(1)%item%value=7
source(2)%item%value=8
call create(boxes(2)%item,p)
boxes([1,3])=source
print *,boxes(1)%item%value,boxes(3)%item%value,p%object%value
end
''','7 8 3')

    def test_assumed_shape_owner(self):
        module=MODULE.replace('end module','''subroutine attach(boxes,p)
type(container),target,intent(inout)::boxes(:,:)
type(reference),intent(out)::p
call create(boxes(2,1)%item,p)
end subroutine
end module''')
        self.check(module+DECL.replace('boxes(3)','boxes(2,2)')+'''call attach(boxes,p)
p%object%value=8
print *,boxes(2,1)%item%value
end
''','8')

    def test_rank_fifteen(self):
        shape=','.join(['1']*14+['2'])
        self.check(MODULE+DECL.replace('boxes(3)',f'boxes({shape})')+f'''call create(boxes({shape})%item,p)
p%object%value=8
print *,boxes({shape})%item%value
end
''','8')

    def test_saved_factory(self):
        module=MODULE.replace('end module','''function factory() result(p)
type(reference)::p
type(container),target,save::boxes(3)
call create(boxes(2)%item,p)
end function
end module''')
        self.check(module+DECL+'''p=factory()
q=factory()
p%object%value=12
print *,q%object%value,associated(p%object,q%object)
end
''','12 T')

    def test_finalization_and_move(self):
        module=MODULE.replace('integer :: value=1\nend type','''integer :: value=1
contains
final::finish
end type''').replace('contains\nsubroutine create','''contains
subroutine finish(x)
type(resource),intent(inout)::x
print *,'final',x%value
end subroutine
subroutine create''')
        self.check(module+DECL+'''call create(boxes(1)%item,p)
call create(boxes(3)%item,q)
nullify(q%object)
call move_alloc(boxes(1)%item,boxes(3)%item)
p%object%value=7
print *,allocated(boxes(1)%item),boxes(3)%item%value,associated(p%object,boxes(3)%item)
nullify(p%object)
deallocate(boxes(3)%item)
end
''','final 3\nF 7 T\nfinal 7')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',MODULE)
        helper.success('-c',source)
        source.unlink()
        main=('program demo'+MAIN).replace('type(container),target::box','type(container),target::box(3)').replace('box%item','box(2)%item')
        helper.success('-c',helper.source('main',main))
        helper.run_objects(['owners','main'],'3 3\n9 9 T')

    def test_nested_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type::wrapper
type(container)::nested
end type
contains
subroutine create''')
        self.check(module+DECL.replace('type(container),target::boxes(3)','type(wrapper),target::boxes(3)')+'''call create(boxes(2)%nested%item,p)
p%object%value=8
print *,boxes(2)%nested%item%value
end
''','8')

    def test_pure_allocation(self):
        module=MODULE.replace('end module','''pure subroutine reset(x)
type(resource),allocatable,target,intent(out)::x
allocate(x)
x%value=9
end subroutine
pure subroutine initialize(boxes)
type(container),target,intent(inout)::boxes(:)
call reset(boxes(2)%item)
end subroutine
end module''')
        self.check(module+DECL+'''call initialize(boxes)
print *,boxes(2)%item%value
end
''','9')

    def test_runtime_bounds(self):
        module=MODULE.replace('end module','''subroutine exercise(n)
integer,intent(in)::n
type(container),target::boxes(n,2)
type(reference)::p
call create(boxes(n,2)%item,p)
p%object%value=8
print *,boxes(n,2)%item%value
end subroutine
end module''')
        self.check(module+'''use component_target_mod
implicit none
call exercise(3)
end
''','8')

    def test_failed_allocation_preserves_alias(self):
        self.check(MODULE+DECL+'''integer::status
call create(boxes(2)%item,p)
allocate(boxes(2)%item,stat=status)
print *,status/=0,p%object%value
end
''','T 3')

    def test_readonly_owner(self):
        module=MODULE.replace('end module','''subroutine invalid(boxes,p)
type(container),target,intent(in)::boxes(:)
type(reference),intent(out)::p
call create(boxes(2)%item,p)
end subroutine
end module''')
        self.reject(module+'end\n','INTENT(IN)')

    def test_shadowed_owner(self):
        self.check(MODULE+DECL+'''call create(boxes(1)%item,p)
p%object%value=12
call local()
print *,p%object%value
contains
subroutine local()
type(container),target::boxes(2)
type(reference)::p
call create(boxes(2)%item,p)
print *,p%object%value
end subroutine
end
''','3\n12')

if __name__=='__main__':unittest.main()

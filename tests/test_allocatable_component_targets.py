"""Allocation identities survive calls with scalar component TARGET actuals."""
import unittest
import test_polymorphic_pointers as shared
import test_module_compilation
from test_record_read import ROOT

FIXTURE=(ROOT/'tests/programs/xallocatable_component_target.f90').read_text()
MODULE,MAIN=FIXTURE.split('program demo',1)
DECL='''use component_target_mod
implicit none
type(container),target::box,other
type(reference)::p,q
'''

class ComponentTargets(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check
    reject=shared.PolymorphicPointers.reject

    def test_fixture(self):
        self.check(FIXTURE,'3 3\n9 9 T')

    def test_deallocation_invalidates_pointer(self):
        self.check(MODULE+DECL+'''call create(box%item,p)
deallocate(box%item)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_output_replacement_invalidates_pointer(self):
        module=MODULE.replace('end module','''subroutine reset(x)
type(resource),allocatable,target,intent(out)::x
allocate(x)
end subroutine
end module''')
        self.check(module+DECL+'''call create(box%item,p)
call reset(box%item)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_move_alloc_preserves_pointer(self):
        self.check(MODULE+DECL+'''call create(box%item,p)
call move_alloc(box%item,other%item)
p%object%value=7
print *,allocated(box%item),other%item%value,associated(p%object,other%item)
nullify(p%object)
deallocate(other%item)
end
''','F 7 T')

    def test_owner_copy_has_independent_identity(self):
        self.check(MODULE+DECL+'''call create(box%item,p)
other=box
other%item%value=20
print *,p%object%value,other%item%value
print *,associated(p%object,box%item),associated(p%object,other%item)
end
''','3 20\nT F')

    def test_nested_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type::wrapper
type(container)::nested
end type
contains
subroutine create''')
        main=('program demo'+MAIN).replace('type(container),target::box','type(wrapper),target::box').replace('box%item','box%nested%item')
        self.check(module+main,'3 3\n9 9 T')

    def test_extended_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type,extends(container)::extended
integer::extra=0
end type
contains
subroutine create''')
        main=('program demo'+MAIN).replace('type(container),target::box','type(extended),target::box')
        self.check(module+main,'3 3\n9 9 T')

    def test_module_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type(container),target::box
contains
subroutine create''')
        self.check(module+('program demo'+MAIN).replace('type(container),target::box\n',''),'3 3\n9 9 T')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',MODULE)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','program demo'+MAIN)
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'3 3\n9 9 T')

    def test_assignment_retains_allocation_identity(self):
        self.check(MODULE+DECL+'''call create(box%item,p)
box%item=resource(9)
print *,p%object%value,associated(p%object,box%item)
end
''','9 T')

    def test_saved_factory(self):
        module=MODULE.replace('end module','''function factory() result(p)
type(reference)::p
type(container),target,save::saved
call create(saved%item,p)
end function
end module''')
        self.check(module+DECL+'''p=factory()
q=factory()
p%object%value=12
print *,q%object%value,associated(p%object,q%object)
end
''','12 T')

    def test_separate_target_attribute(self):
        self.check(FIXTURE.replace('type(container),target::box',
                                  'type(container)::box\ntarget::box'), '3 3\n9 9 T')

    def test_owner_dummy(self):
        module=MODULE.replace('end module','''subroutine forward(box,p)
type(container),target,intent(inout)::box
type(reference),intent(out)::p
call create(box%item,p)
end subroutine
end module''')
        self.check(module+('program demo'+MAIN).replace('call create(box%item,','call forward(box,'),
                   '3 3\n9 9 T')

    def test_direct_association(self):
        self.check(MODULE+DECL+'''allocate(box%item)
p%object=>box%item
p%object%value=8
print *,box%item%value,associated(p%object,box%item)
end
''','8 T')

    def test_direct_pointer_variable(self):
        self.check(MODULE+DECL+'''class(resource),pointer::r
allocate(box%item)
r=>box%item
r%value=8
print *,box%item%value,associated(r,box%item)
nullify(r)
deallocate(box%item)
end
''','8 T')

    def test_move_between_root_and_component(self):
        self.check(MODULE+DECL+'''type(resource),allocatable,target::root
call create(box%item,p)
call move_alloc(box%item,root)
print *,p%object%value,associated(p%object,root)
call move_alloc(root,other%item)
p%object%value=8
print *,other%item%value,associated(p%object,other%item)
end
''','3 T\n8 T')

    def test_owner_copy_invalidates_destination_pointer(self):
        self.check(MODULE+DECL+'''call create(box%item,p)
call create(other%item,q)
other=box
print *,q%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_optional_keyword_forwarding(self):
        module=MODULE.replace('end module','''subroutine forward(x,p)
type(resource),allocatable,target,optional,intent(inout)::x
type(reference),intent(out)::p
if(present(x)) call create(p=p,x=x)
end subroutine
end module''')
        self.check(module+DECL+'''call forward(p=q)
print *,associated(q%object)
call forward(p=p,x=box%item)
p%object%value=10
print *,box%item%value,associated(p%object,box%item)
end
''','F\n10 T')

    def test_array_owner_limit(self):
        self.reject(MODULE+DECL+'''type(container),target::boxes(2)
call create(boxes(2)%item,p)
end
''','TARGET ALLOCATABLE scalar component actuals are not yet supported')

    def test_allocatable_owner_limit(self):
        self.reject(MODULE+DECL+'''type(container),allocatable,target::dynamic
allocate(dynamic)
call create(dynamic%item,p)
end
''','TARGET ALLOCATABLE scalar component actuals are not yet supported')

    def test_shadowed_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type(container),target::box
contains
subroutine local()
type(container),target::box
type(reference)::p
call create(box%item,p)
print *,p%object%value
nullify(p%object)
deallocate(box%item)
end subroutine
subroutine create''')
        self.check(module+'''use component_target_mod
implicit none
type(reference)::p
call create(box%item,p)
p%object%value=12
call local()
print *,p%object%value
end
''','3\n12')

    def test_finalization_and_move(self):
        module=MODULE.replace('integer :: value=1\nend type','''integer :: value=1
contains
 final::finish
end type''')
        module=module.replace('contains\nsubroutine create','''contains
subroutine finish(x)
type(resource),intent(inout)::x
print *,'final',x%value
end subroutine
subroutine create''')
        self.check(module+DECL+'''call create(box%item,p)
call create(other%item,q)
nullify(q%object)
call move_alloc(box%item,other%item)
p%object%value=7
print *,allocated(box%item),other%item%value,associated(p%object,other%item)
nullify(p%object)
deallocate(other%item)
end
''','final 3\nF 7 T\nfinal 7')

if __name__=='__main__':unittest.main()

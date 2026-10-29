"""Persistent component targets owned by ALLOCATABLE derived arrays."""
import unittest
import test_polymorphic_pointers as shared
import test_module_compilation
from test_allocatable_component_targets import MODULE
from test_record_read import ROOT

DECL='''program demo
use component_target_mod
implicit none
type(container),allocatable,target::boxes(:),other(:)
type(reference)::p,q
'''


class AllocatableOwnerComponentTargets(unittest.TestCase):
    setUp=shared.PolymorphicPointers.setUp
    check=shared.PolymorphicPointers.check

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xallocatable_owner_component_target.f90').read_text(),'7\nF 9 T')

    def test_basic(self):
        self.check(MODULE+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
p%object%value=7
print *,boxes(2)%item%value,associated(p%object,boxes(2)%item)
end
''','7 T')

    def test_unallocated_other_environment(self):
        self.check(MODULE+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
print *,p%object%value,allocated(other)
end
''','3 F')

    def test_owner_deallocation_invalidates(self):
        self.check(MODULE+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
deallocate(boxes)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_owner_reallocation_invalidates(self):
        self.check(MODULE+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
deallocate(boxes)
allocate(boxes(3))
call create(boxes(2)%item,q)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_move_owner(self):
        self.check(MODULE+DECL+'''allocate(boxes(-1:1))
call create(boxes(0)%item,p)
call move_alloc(boxes,other)
p%object%value=11
print *,allocated(boxes),lbound(other),other(0)%item%value
print *,associated(p%object,other(0)%item)
end
''','F -1 11\nT')

    def test_rank_two(self):
        decl=DECL.replace('boxes(:),other(:)','boxes(:,:),other(:,:)')
        self.check(MODULE+decl+'''allocate(boxes(-1:1,4:5))
call create(boxes(0,5)%item,p)
p%object%value=13
print *,boxes(0,5)%item%value
end
''','13')

    def test_scalar_owner(self):
        decl=DECL.replace('boxes(:),other(:)','boxes,other')
        self.check(MODULE+decl+'''allocate(boxes)
call create(boxes%item,p)
p%object%value=17
print *,boxes%item%value
end
''','17')

    def test_move_replaces_destination(self):
        self.check(MODULE+DECL+'''allocate(boxes(3),other(2))
call create(boxes(2)%item,p)
call create(other(1)%item,q)
call move_alloc(from=boxes,to=other)
print *,p%object%value
print *,q%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_move_unallocated_source(self):
        self.check(MODULE+DECL+'''allocate(other(2))
call create(other(1)%item,p)
call move_alloc(boxes,other)
print *,allocated(boxes),allocated(other)
end
''','F F')

    def test_scalar_move_and_deallocation(self):
        self.check(MODULE+DECL.replace('boxes(:),other(:)','boxes,other')+'''allocate(boxes)
call create(boxes%item,p)
call move_alloc(boxes,other)
p%object%value=19
print *,allocated(boxes),other%item%value
deallocate(other)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_assumed_shape_forwarding(self):
        module=MODULE.replace('end module','''subroutine attach(a,p)
type(container),target,intent(inout)::a(:)
type(reference),intent(out)::p
call create(a(2)%item,p)
end subroutine
end module''')
        self.check(module+DECL+'''allocate(boxes(3))
call attach(boxes,p)
p%object%value=23
print *,boxes(2)%item%value
end
''','23')

    def test_saved_factory(self):
        module=MODULE.replace('end module','''function factory() result(p)
type(reference)::p
type(container),allocatable,target,save::a(:)
if(.not.allocated(a)) allocate(a(3))
call create(a(2)%item,p)
end function
end module''')
        self.check(module+DECL+'''p=factory()
q=factory()
p%object%value=29
print *,q%object%value,associated(p%object,q%object)
end
''','29 T')

    def test_live_module_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type(container),allocatable,target::boxes(:)
contains
subroutine create''').replace('p%object=>x','''p%object=>x
print *,allocated(boxes(2)%item),boxes(2)%item%value''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
allocate(boxes(3))
call create(boxes(2)%item,p)
print *,p%object%value
end
''','T 3\n3')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',MODULE)
        helper.success('-c',source)
        source.unlink()
        helper.success('-c',helper.source('main',DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
p%object%value=31
print *,boxes(2)%item%value
end
'''))
        helper.run_objects(['owners','main'],'31')

    def test_rank_fifteen(self):
        # O2 descriptor compilation in a debug compiler can exceed one minute
        # during parallel regression runs; execution retains the normal limit.
        rank=','.join([':']*15)
        indices=','.join(['1']*14+['2'])
        decl=DECL.replace('boxes(:),other(:)',f'boxes({rank})')
        self.check(MODULE+decl+f'''allocate(boxes({indices}))
call create(boxes({indices})%item,p)
p%object%value=37
print *,boxes({indices})%item%value
end
''','37',compile_timeout=180)

    def test_assignment_allocates_and_deep_copies(self):
        self.check(MODULE+DECL+'''allocate(other(-2:1))
call create(other(0)%item,q)
boxes=other
boxes(0)%item%value=41
print *,lbound(boxes),ubound(boxes),boxes(0)%item%value,q%object%value
end
''','-2 1 41 3')

    def test_assignment_reallocation_invalidates(self):
        self.check(MODULE+DECL+'''allocate(boxes(2),other(4))
call create(boxes(1)%item,p)
call create(other(3)%item,q)
boxes=other
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_overlapping_owner_assignment(self):
        self.check(MODULE+DECL+'''allocate(boxes(-1:1))
call create(boxes(-1)%item,p)
call create(boxes(0)%item,p)
call create(boxes(1)%item,p)
boxes(0)%item%value=43
boxes(1)%item%value=47
boxes=boxes(0:1)
print *,lbound(boxes),size(boxes),boxes(1)%item%value,boxes(2)%item%value
end
''','1 2 43 47')

    def test_assignment_keeps_conforming_destination_bounds(self):
        self.check(MODULE+DECL+'''allocate(boxes(10:13),other(-2:1))
call create(other(0)%item,q)
boxes=other
print *,lbound(boxes),ubound(boxes),boxes(12)%item%value,q%object%value
end
''','10 13 3 3')

    def test_rank_two_assignment(self):
        decl=DECL.replace('boxes(:),other(:)','boxes(:,:),other(:,:)')
        self.check(MODULE+decl+'''allocate(other(-1:0,4:5))
call create(other(0,5)%item,q)
boxes=other
print *,lbound(boxes),shape(boxes),boxes(0,5)%item%value
end
''','-1 4 2 2 3')

    def test_empty_assignment_invalidates(self):
        self.check(MODULE+DECL+'''allocate(boxes(3),other(0))
call create(boxes(2)%item,p)
boxes=other
print *,size(boxes)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_intent_out_owner_invalidates(self):
        module=MODULE.replace('end module','''subroutine reset(a)
type(container),allocatable,target,intent(out)::a(:)
allocate(a(2))
end subroutine
end module''')
        self.check(module+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
call reset(boxes)
print *,p%object%value
end
''',error='deallocated or reallocated target',reference=False)

    def test_finalization_on_owner_move(self):
        module=MODULE.replace('integer :: value=1\nend type','''integer :: value=1
contains
final::finish
end type''').replace('contains\nsubroutine create','''contains
subroutine finish(x)
type(resource),intent(inout)::x
print *,'final',x%value
end subroutine
subroutine create''')
        self.check(module+DECL+'''allocate(boxes(3),other(2))
call create(boxes(2)%item,p)
call create(other(1)%item,q)
other(1)%item%value=53
nullify(q%object)
call move_alloc(boxes,other)
print *,p%object%value
nullify(p%object)
deallocate(other)
end
''','final 53\n3\nfinal 3')

    def test_plain_derived_array_move_without_pointer_types(self):
        self.check('''program demo
implicit none
type::item
integer::value=1
end type
type(item),allocatable::a(:),b(:)
allocate(a(-1:1),b(2))
a(0)%value=59
call move_alloc(to=b,from=a)
print *,allocated(a),lbound(b),b(0)%value
end
''','F -1 59')

    def test_character_component(self):
        module=MODULE.replace('integer :: value=1','character(len=3)::value="abc"').replace('x%value=x%value+2','x%value="def"')
        self.check(module+DECL+'''allocate(boxes(3))
call create(boxes(2)%item,p)
p%object%value='ghi'
call move_alloc(boxes,other)
print *,p%object%value,other(2)%item%value
end
''','ghighi')

    def test_nested_scalar_owner(self):
        module=MODULE.replace('contains\nsubroutine create','''type::wrapper
type(container)::nested
end type
contains
subroutine create''')
        decl=DECL.replace('type(container),allocatable,target::boxes(:),other(:)','type(wrapper),allocatable,target::boxes,other')
        self.check(module+decl+'''allocate(boxes)
call create(boxes%nested%item,p)
p%object%value=61
print *,boxes%nested%item%value
end
''','61')

    def test_readonly_owner_rejected(self):
        module=MODULE.replace('end module','''subroutine bad(a,p)
type(container),allocatable,target,intent(in)::a(:)
type(reference),intent(out)::p
call create(a(2)%item,p)
end subroutine
end module''')
        shared.PolymorphicPointers.reject(self,module+DECL+'end\n','INTENT(IN)')


if __name__=='__main__':
    unittest.main()

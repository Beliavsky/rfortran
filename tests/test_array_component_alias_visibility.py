"""Live array-element allocation descriptors, compared with gfortran."""
import unittest
import test_polymorphic_pointers as shared
import test_module_compilation
from test_allocatable_component_targets import MODULE
from test_record_read import ROOT


class ArrayComponentAliasVisibility(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xarray_component_alias_visibility.f90').read_text(),
                   'T 7\n9\nF\nF')

    def test_separate_module(self):
        helper = test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        module, main = (ROOT/'tests/programs/xarray_component_alias_visibility.f90').read_text().split('program demo', 1)
        source = helper.source('owners', module)
        helper.success('-c', source)
        source.unlink()
        helper.success('-c', helper.source('main', 'program demo'+main))
        helper.run_objects(['owners','main'], 'T 7\n9\nF\nF')

    def test_module_allocation(self):
        module = MODULE.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('p%object=>x',
                                'p%object=>x\nprint *,allocated(boxes(2)%item),boxes(2)%item%value')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
print *,allocated(boxes(2)%item),boxes(2)%item%value
end
''', 'T 3\nT 3')

    def test_module_deallocation_and_assignment(self):
        module = MODULE.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('p%object=>x', '''p%object=>x
boxes(2)%item%value=17
print *,x%value
deallocate(boxes(2)%item)
print *,allocated(x)
allocate(boxes(2)%item)
boxes(2)%item%value=23
print *,allocated(x),x%value''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
print *,boxes(2)%item%value
end
''', '17\nF\nT 23\n23')

    def test_overlapping_dynamic_owner_views(self):
        module = MODULE.replace('end module', '''subroutine both(a,b)
type(container),target,intent(inout)::a,b
allocate(a%item)
a%item%value=7
print *,allocated(b%item),b%item%value
b%item%value=11
print *,a%item%value
deallocate(b%item)
print *,allocated(a%item)
end subroutine
end module''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(container),target::boxes(3)
integer::i,j
i=2
j=2
call both(boxes(i),boxes(j))
print *,allocated(boxes(2)%item)
end
''', 'T 7\n11\nF\nF')

    def test_module_move_alloc(self):
        module = MODULE.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('p%object=>x', '''p%object=>x
call move_alloc(x,boxes(3)%item)
print *,allocated(boxes(2)%item),boxes(3)%item%value
call move_alloc(boxes(3)%item,x)
print *,allocated(boxes(3)%item),boxes(2)%item%value''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
print *,p%object%value
end
''', 'F 3\nF 3\n3')

    def test_two_components_and_nested_nonunit_indices(self):
        module = MODULE.replace(':: item', ':: item,other')
        module = module.replace('contains\nsubroutine create', '''type::wrapper
type(container)::nested
end type
type(wrapper),target::boxes(-1:1,4:5)
contains
subroutine create''')
        module = module.replace('p%object=>x', '''p%object=>x
allocate(boxes(0,5)%nested%other)
boxes(0,5)%nested%other%value=19
print *,boxes(0,5)%nested%item%value,boxes(0,5)%nested%other%value
call transfer(x,boxes(0,5)%nested%other)
print *,allocated(boxes(0,5)%nested%item),boxes(0,5)%nested%other%value''')
        module = module.replace('end module', '''subroutine transfer(a,b)
type(resource),allocatable,target,intent(inout)::a,b
call move_alloc(a,b)
end subroutine
end module''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(0,5)%nested%item,p)
print *,p%object%value
end
''', '3 19\nF 3\n3')

    def test_character_and_logical_owned_arrays(self):
        self.check('''module live_arrays
implicit none
type::box
character(len=3),allocatable::words(:)
logical,allocatable::flags(:)
end type
type(box),target::boxes(2)
contains
subroutine update(x)
type(box),target,intent(inout)::x
allocate(x%words(2),x%flags(2))
x%words=['abc','def']
x%flags=[.true.,.false.]
print *,size(boxes(2)%words),boxes(2)%words,boxes(2)%flags
boxes(2)%words(2)='ghi'
boxes(2)%flags(2)=.true.
print *,x%words(2),all(x%flags)
deallocate(boxes(2)%words,boxes(2)%flags)
print *,allocated(x%words),allocated(x%flags)
allocate(boxes(2)%words(3))
boxes(2)%words=['jkl','mno','pqr']
print *,size(x%words),x%words
end subroutine
end module
program demo
use live_arrays
implicit none
call update(boxes(2))
print *,allocated(boxes(2)%words)
end
''', '2 abcdef T F\nghi T\nF F\n3 jklmnopqr\nT')

    def test_owner_assignment_during_call(self):
        module = MODULE.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('p%object=>x', '''allocate(boxes(1)%item)
boxes(1)%item%value=31
boxes(2)=boxes(1)
print *,x%value
x%value=37
print *,boxes(1)%item%value,boxes(2)%item%value
p%object=>x''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
print *,p%object%value
end
''', '31\n31 37\n37')

    def test_multidimensional_character_and_real_bounds(self):
        self.check('''module live_arrays
implicit none
type::box
character(len=2),allocatable::words(:,:)
real,allocatable::values(:,:)
end type
type(box),target::boxes(2)
contains
subroutine update(x)
type(box),target,intent(inout)::x
allocate(x%words(1,1),x%values(1,1))
x%words='aa'
x%values=1.0
deallocate(boxes(2)%words,boxes(2)%values)
allocate(boxes(2)%words(-1:0,4:6),boxes(2)%values(-1:0,4:6))
boxes(2)%words='bb'
boxes(2)%values=7.0
print *,size(x%words),shape(x%words),lbound(x%words),x%words(0,6)
print *,size(x%values),lbound(x%values),sum(x%values)
end subroutine
end module
program demo
use live_arrays
implicit none
call update(boxes(2))
end
''', '6 2 3 -1 4 bb\n6 -1 4 42.0')

    def test_final_callback_sees_live_owner(self):
        module = MODULE.replace('integer :: value=1\nend type', '''integer :: value=1
contains
final::finish
end type''').replace('contains\nsubroutine create', '''type(container),target::boxes(3)
contains
subroutine finish(x)
type(resource),intent(inout)::x
print *,x%value,allocated(boxes(2)%item)
end subroutine
subroutine create''')
        module = module.replace('p%object=>x', '''p%object=>x
deallocate(x)
print *,allocated(boxes(2)%item)''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
end
''', '3 T\nF')

    def test_uninitialized_component_mask(self):
        module = MODULE.replace('integer :: value=1','integer :: value')
        module = module.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('x%value=x%value+2', 'print *,boxes(2)%item%value')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
call create(boxes(2)%item,p)
end
''', error='uninitialized variable', reference=False)

    def test_skipped_calls_and_loop_rebinding(self):
        module = MODULE.replace('contains\nsubroutine create',
                                'type(container),target::boxes(3)\ncontains\nsubroutine create')
        module = module.replace('p%object=>x',
                                'p%object=>x\nprint *,allocated(boxes(i)%item)')
        module = module.replace('type(container),target::boxes(3)',
                                'type(container),target::boxes(3)\ninteger::i')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(reference)::p
logical::skip
skip=.true.
i=2
if (.not.skip) call create(boxes(i)%item,p)
do i=1,3
if(i/=2) call create(boxes(i)%item,p)
end do
print *,boxes(1)%item%value,allocated(boxes(2)%item),boxes(3)%item%value
end
''', 'T\nT\n3 F 3')

    def test_nan_scalar_sibling_does_not_replay_stale_writes(self):
        self.check('''module nan_alias
use, intrinsic::ieee_arithmetic
implicit none
type::box
real::marker
real,allocatable::values(:)
end type
type(box),target::boxes(2)
contains
subroutine update(x)
type(box),target,intent(inout)::x
print *,ieee_is_nan(x%marker)
boxes(2)%marker=3.0
print *,x%marker
end subroutine
end module
program demo
use nan_alias
implicit none
boxes(2)%marker=ieee_value(0.0,ieee_quiet_nan)
call update(boxes(2))
print *,boxes(2)%marker
end
''', 'T\n3.0\n3.0')

    def test_function_call_and_assumed_shape_alias(self):
        module = MODULE.replace('end module', '''integer function change(x,owners) result(n)
type(resource),allocatable,target,intent(inout)::x
type(container),target,intent(inout)::owners(:)
allocate(x)
x%value=43
print *,allocated(owners(2)%item),owners(2)%item%value
owners(2)%item%value=47
n=x%value
end function
end module''')
        self.check(module+'''program demo
use component_target_mod
implicit none
type(container),target::boxes(3)
integer::n
n=change(boxes(2)%item,boxes)
print *,n,boxes(2)%item%value
end
''', 'T 43\n47 47')

    def test_signed_zero_scalar_sibling(self):
        self.check('''module zero_alias
implicit none
type::box
real::marker=0.0
real,allocatable::values(:)
end type
type(box),target::boxes(2)
contains
subroutine update(x)
type(box),target,intent(inout)::x
boxes(2)%marker=-0.0
print *,sign(1.0,x%marker)
x%marker=0.0
print *,sign(1.0,boxes(2)%marker)
end subroutine
end module
program demo
use zero_alias
implicit none
call update(boxes(2))
end
''', '-1.0\n1.0')

    def test_saved_scalar_snapshot_nan_and_signed_zero(self):
        self.check('''module scalar_alias
use, intrinsic::ieee_arithmetic
implicit none
real,target::value
contains
subroutine update(x)
real,target,intent(inout)::x
print *,ieee_is_nan(x)
value=3.0
print *,x
x=-0.0
print *,sign(1.0,value)
end subroutine
end module
program demo
use scalar_alias
implicit none
value=ieee_value(0.0,ieee_quiet_nan)
call update(value)
end
''', 'T\n3.0\n-1.0')


if __name__ == '__main__':
    unittest.main()

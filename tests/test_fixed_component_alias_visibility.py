"""Fixed component views must remain live beside owned record descriptors."""
import unittest
import test_polymorphic_pointers as shared
import test_module_compilation
from test_record_read import ROOT

FIXTURE = (ROOT/'tests/programs/xfixed_component_alias_visibility.f90').read_text()


class FixedComponentAliasVisibility(unittest.TestCase):
    setUp = shared.PolymorphicPointers.setUp
    check = shared.PolymorphicPointers.check

    def test_fixture(self):
        self.check(FIXTURE,
                   '7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_allocatable_owner(self):
        source=FIXTURE.replace('target :: boxes(0:2)', 'allocatable, target :: boxes(:)')
        source=source.replace('call update(boxes(1))',"allocate(boxes(0:2))\nboxes(1)%counts=0\nboxes(1)%name='initial'\ncall update(boxes(1))")
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_overlapping_dynamic_views(self):
        self.check('''module aliases
implicit none
type :: box
integer, allocatable :: owned(:)
real :: values(-1:0,3:4)=0
logical :: flags(0:1)=.false.
character(4) :: name='old'
end type
contains
subroutine both(a,b)
type(box),target,intent(inout)::a,b
a%values(-1,4)=7
a%flags(0)=.true.
a%name='new'
print *,b%values(-1,4),b%flags(0),b%name
b%values(0,3)=9
b%flags(1)=.true.
b%name='back'
print *,a%values(0,3),all(a%flags),a%name
end subroutine
end module
program demo
use aliases
implicit none
type(box),target::boxes(-2:2)
integer::i,j
i=1
j=1
call both(boxes(i),boxes(j))
print *,boxes(1)%values(-1,4),boxes(1)%values(0,3),boxes(1)%name
end
''','7 T new\n9 T back\n7 9 back')

    def test_nested_components(self):
        source=FIXTURE.replace('type(box), target :: boxes(0:2)',
            'type :: wrapper\ntype(box)::part\nend type\ntype(wrapper),target::boxes(0:2)')
        source=source.replace('boxes(1)%','boxes(1)%part%').replace('update(boxes(1))','update(boxes(1)%part)')
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_owner_index_captured_before_call(self):
        source=FIXTURE.replace('target :: boxes(0:2)','target :: boxes(0:2)\ninteger::index')
        source=source.replace('x%counts(-1) = 7','index=2\nx%counts(-1) = 7')
        source=source.replace('call update(boxes(1))','index=1\ncall update(boxes(index))')
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_partial_initialization_is_not_read(self):
        source=FIXTURE.replace(' = 0','').replace(" = 'initial'",'')
        source=source.replace('boxes(1)%counts(0)', '0')
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_uninitialized_element_stays_uninitialized(self):
        source=FIXTURE.replace(' = 0','')
        self.check(source,error='read of uninitialized variable',reference=False)

    def test_substring_mutation(self):
        source=FIXTURE.replace("x%name = 'updated'", "x%name(1:3) = 'new'")
        source=source.replace("boxes(1)%name = 'reverse'", "boxes(1)%name(4:7) = 'back'")
        self.check(source,'7 newtial\n9 newback\n7 0 9 newback 11')
        source=source.replace('target :: boxes(0:2)', 'allocatable,target :: boxes(:)')
        source=source.replace('call update(boxes(1))',"allocate(boxes(0:2))\nboxes(1)%counts=0\nboxes(1)%name='initial'\ncall update(boxes(1))")
        self.check(source,'7 newtial\n9 newback\n7 0 9 newback 11')

    def test_empty_array_field(self):
        source=FIXTURE.replace('counts(-1:1) = 0','counts(0) = 0')
        source=source.replace('x%counts(-1) = 7','').replace('boxes(1)%counts(1) = 9','')
        source=source.replace('boxes(1)%counts(-1)', 'size(boxes(1)%counts)').replace('x%counts(1)', 'size(x%counts)')
        source=source.replace('boxes(1)%counts(0), boxes(1)%counts(1),','')
        self.check(source,'0 updated\n0 reverse\n0 reverse 11')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        module, main=FIXTURE.split('program demo',1)
        source=helper.source('aliases',module)
        helper.success('-c',source)
        source.unlink()
        helper.success('-c',helper.source('main','program demo'+main))
        helper.run_objects(['aliases','main'],'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_array_nan_and_signed_zero_aliases(self):
        self.check('''module aliases
use,intrinsic::ieee_arithmetic
implicit none
type::box
integer,allocatable::owned(:)
real::values(2)
end type
type(box),target::boxes(2)
contains
subroutine update(a,b)
type(box),target,intent(inout)::a,b
b%values(1)=3.0
print *,a%values(1)
b%values(2)=-0.0
print *,sign(1.0,a%values(2))
end subroutine
end module
program demo
use aliases
implicit none
integer::i,j
boxes(1)%values(1)=ieee_value(0.0,ieee_quiet_nan)
boxes(1)%values(2)=0.0
i=1
j=1
call update(boxes(i),boxes(j))
print *,boxes(1)%values(1),sign(1.0,boxes(1)%values(2))
end
''','3\n-1\n3 -1')

    def test_no_owned_fields_regression(self):
        source=FIXTURE.replace('   integer, allocatable :: owned(:)\n','')
        source=source.replace('allocate(x%owned(1))\nx%owned = 11','')
        source=source.replace(', boxes(1)%owned(1)','')
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse')

    def test_forwarded_view(self):
        source=FIXTURE.replace('x%counts(-1) = 7','call set_fields(x)')
        source=source.replace('end module','''subroutine set_fields(x)
type(box),target,intent(inout)::x
x%counts(-1)=7
x%name='updated'
end subroutine
end module''')
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_local_arrays_without_owned_components(self):
        self.check('''program demo
implicit none
type::box
integer::values(2)=0
end type
type(box),target::boxes(2)
integer::i,j
i=1
j=1
call update(boxes(i),boxes(j))
print *,boxes(1)%values
contains
subroutine update(a,b)
type(box),target,intent(inout)::a,b
a%values(1)=7
b%values(2)=9
print *,b%values(1),a%values(2)
end subroutine
end
''','7 9\n7 9')

    def test_runtime_parent_and_component_bounds(self):
        source=FIXTURE.replace('type(box), target :: boxes(0:2)', 'type(box),allocatable,target::boxes(:,:)')
        source=source.replace('boxes(1)','boxes(0,4)')
        source=source.replace('call update(boxes(0,4))',"allocate(boxes(-1:1,3:4))\nboxes(0,4)%counts=0\nboxes(0,4)%name='initial'\ncall update(boxes(0,4))")
        self.check(source,'7 updated\n9 reverse\n7 0 9 reverse 11')

    def test_local_initialization_masks(self):
        source='''program demo
implicit none
type::box
integer,allocatable::owned(:)
integer::values(2)
end type
type(box),target::boxes(2)
call update(boxes(1))
print *,boxes(1)%values(1)
contains
subroutine update(a)
type(box),target,intent(inout)::a
a%values(1)=7
end subroutine
end
'''
        self.check(source,'7')
        self.check(source.replace('print *,boxes(1)%values(1)','print *,boxes(1)%values(2)'),
                   error='read of uninitialized variable',reference=False)

    def test_skipped_branch_and_repeated_calls(self):
        source=FIXTURE.replace('call update(boxes(1))', '''integer::i
logical::go
go=.false.
if(go) call update(boxes(0))
do i=1,2
call update(boxes(1))
deallocate(boxes(1)%owned)
end do
allocate(boxes(1)%owned(1))
boxes(1)%owned=11''')
        self.check(source,'7 updated\n9 reverse\n7 updated\n9 reverse\n7 0 9 reverse 11')


if __name__ == '__main__':
    unittest.main()

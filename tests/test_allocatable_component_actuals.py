"""Scalar derived ALLOCATABLE component actuals retain ownership in their parent."""
import unittest
import test_deallocate
import test_module_compilation
from test_record_read import ROOT

MODULE=(ROOT/'tests/programs/xallocatable_component_actual.f90').read_text().split('program demo')[0]

class AllocatableComponentActuals(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xallocatable_component_actual.f90').read_text(),
                   'entry F\nT\n9\nfinal 9\nentry F\nfinal 9\n')

    def test_nested_scalar_owner(self):
        self.check(MODULE+'''use allocatable_component_demo
implicit none
type::outer
 type(owner)::nested
end type
type(outer)::x
call reset(x%nested%item)
call show(x%nested%item)
end
''','entry F\nT\n9\n')

    def test_array_element_owner(self):
        self.check(MODULE+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
call reset(x(2)%item)
call show(x(1)%item)
call show(x(2)%item)
call reset(x(2)%item)
end
''','entry F\nF\nT\n9\nfinal 9\nentry F\n')

    def test_function_input(self):
        self.check(MODULE.replace('end module','''integer function get(x) result(n)
type(resource),allocatable,intent(in)::x
n=-1
if(allocated(x)) n=x%value
end function
end module''')+'''use allocatable_component_demo
implicit none
type(owner)::x
integer::n
n=get(x%item)
print *,n
call reset(x%item)
n=get(x%item)
print *,n
end
''','-1\nentry F\n9\n')

    def test_optional_keyword_forwarding(self):
        module=MODULE.replace('end module','''subroutine forward(x)
type(resource),allocatable,optional,intent(inout)::x
if(present(x)) call reset(x=x)
end subroutine
end module''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
call forward()
call forward(x=x(2)%item)
call show(x=x(1)%item)
call show(x=x(2)%item)
end
''','entry F\nF\nT\n9\n')

    def test_no_final_procedure(self):
        module=MODULE.replace('contains\n   final :: finish\n','')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x
call reset(x%item)
call show(x%item)
end
''','entry F\nT\n9\n')

    def test_allocation_inquiry_on_array_owner(self):
        self.check(MODULE+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
print *,allocated(x(2)%item)
call reset(x(2)%item)
print *,allocated(x(2)%item)
deallocate(x(2)%item)
print *,allocated(x(2)%item)
end
''','F\nentry F\nT\nfinal 9\nF\n')

    def test_multidimensional_owner_and_index_capture(self):
        module=MODULE.replace('end module','''subroutine change(x,i)
type(resource),allocatable,intent(inout)::x
integer,intent(inout)::i
allocate(x)
x%value=5
i=1
end subroutine
end module''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(2,3,4)
integer::i
i=2
call change(x(i,3,4)%item,i)
call show(x(1,3,4)%item)
call show(x(2,3,4)%item)
print *,i
end
''','F\nT\n5\n1\n')

    def test_reject_readonly_owner(self):
        module=MODULE.replace('end module','''subroutine change(x)
type(owner),intent(in)::x
call reset(x%item)
end subroutine
end module''')
        self.reject(module+'end\n','INTENT(IN)')

    def test_target_dummy(self):
        self.check(MODULE.replace('allocatable, intent(out)', 'allocatable, target, intent(out)')+'''use allocatable_component_demo
implicit none
type(owner)::x
call reset(x%item)
end
''','entry F\n')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',MODULE)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','''use allocatable_component_demo
implicit none
type(owner)::x(2)
call reset(x(2)%item)
call reset(x(2)%item)
call show(x(2)%item)
end
''')
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'entry F\nfinal 9\nentry F\nT\n9')

    def test_generic_dispatch(self):
        module=MODULE.replace('contains\nsubroutine finish','''interface inspect
 module procedure show,show_array
end interface
contains
subroutine show_array(x)
type(resource),intent(in)::x(:)
print *,size(x)
end subroutine
subroutine finish''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
call reset(x(2)%item)
call inspect(x(2)%item)
end
''','entry F\nT\n9\n')

    def test_pure_output(self):
        module=MODULE.replace('contains\n   final :: finish\n','')
        module=module.replace('subroutine reset(x)','pure subroutine reset(x)').replace("print *, 'entry', allocated(x)",'')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x
call reset(x%item)
call show(x%item)
end
''','T\n9\n')

    def test_allocatable_owner(self):
        self.check(MODULE+'''use allocatable_component_demo
implicit none
type(owner),allocatable::x
allocate(x)
call reset(x%item)
call show(x%item)
deallocate(x)
end
''','entry F\nT\n9\nfinal 9\n')

    def test_type_bound_call(self):
        module=MODULE.replace('contains\nsubroutine finish','''type::factory
contains
 procedure::make
end type
contains
subroutine make(self,x)
class(factory),intent(in)::self
type(resource),allocatable,intent(out)::x
allocate(x)
x%value=8
end subroutine
subroutine finish''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
type(factory)::f
call f%make(x=x(2)%item)
call show(x(2)%item)
end
''','T\n8\n')

    def test_function_in_loop_condition(self):
        module=MODULE.replace('end module','''logical function ready(x)
type(resource),allocatable,intent(in)::x
ready=allocated(x)
end function
end module''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(3)
integer::i
call reset(x(1)%item)
call reset(x(2)%item)
i=1
do while(ready(x(i)%item))
 print *,i
 i=i+1
 if(i>3) error stop 'stale loop condition'
end do
print *,i
end
''','entry F\nentry F\n1\n2\n3\n')

    def test_function_changes_array_owner(self):
        module=MODULE.replace('end module','''integer function make(x) result(n)
type(resource),allocatable,intent(inout)::x
allocate(x)
x%value=6
n=3
end function
end module''')
        self.check(module+'''use allocatable_component_demo
implicit none
type(owner)::x(2)
integer::n
n=make(x(2)%item)
print *,n
call show(x(1)%item)
call show(x(2)%item)
end
''','3\nF\nT\n6\n')

if __name__=='__main__':unittest.main()

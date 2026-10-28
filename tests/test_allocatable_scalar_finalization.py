"""Shared scalar allocation descriptors and FINAL across procedure interfaces."""
import unittest
import test_deallocate
import test_module_compilation
from test_record_read import ROOT

MODULE='''module m
implicit none
type::resource
 integer::value=7
contains
 final::finish
end type
contains
subroutine finish(x)
type(resource),intent(inout)::x
print *, 'final',x%value
end subroutine
'''

class AllocatableScalarFinalization(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xallocatable_scalar_final.f90').read_text(),
                   'final 3\nentry F\nT 9\nfinal 9\nF\n')

    def test_output_unallocated_entry(self):
        self.check(MODULE+'''subroutine reset(x)
type(resource),allocatable,intent(out)::x
print *,allocated(x)
allocate(x)
print *,x%value
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
call reset(x)
call reset(x)
deallocate(x)
end
''','F\n7\nfinal 7\nF\n7\nfinal 7\n')

    def test_input_allocation_status(self):
        self.check(MODULE+'''subroutine inspect(x)
type(resource),allocatable,intent(in)::x
print *,allocated(x)
if (allocated(x)) print *,x%value
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
call inspect(x)
allocate(x)
x%value=4
call inspect(x)
print *,x%value
end
''','F\nT\n4\n4\n')

    def test_optional_and_forwarding(self):
        self.check(MODULE+'''subroutine leaf(x)
type(resource),allocatable,intent(out),optional::x
print *,present(a=x)
if (.not.present(x)) return
print *,allocated(x)
allocate(x)
x%value=8
end subroutine
subroutine forward(x)
type(resource),allocatable,intent(inout),optional::x
call leaf(x=x)
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
call forward()
call forward(x)
call forward(x)
print *,allocated(x),x%value
end
''','F\nT\nF\nfinal 8\nT\nF\nT 8\n')

    def test_reject_nonallocatable(self):
        self.reject(MODULE+'''subroutine use_it(x)
type(resource),allocatable,intent(in)::x
end subroutine
end module
use m
implicit none
type(resource)::x
call use_it(x)
end
''','requires an ALLOCATABLE scalar actual')

    def test_no_dummy_exit_finalization(self):
        self.check(MODULE+'''subroutine create(x)
type(resource),allocatable,intent(inout)::x
allocate(x)
x%value=4
end subroutine
subroutine work()
type(resource),allocatable::x
call create(x)
print *,x%value
end subroutine
end module
use m
implicit none
call work()
end
''','4\nfinal 4\n')

    def test_function_input(self):
        self.check(MODULE+'''integer function get(x) result(n)
type(resource),allocatable,intent(in)::x
n=-1
if (allocated(x)) n=x%value
end function
end module
use m
implicit none
type(resource),allocatable::x
integer::n
n=get(x)
print *,n
allocate(x)
x%value=6
n=get(x)
print *,n
end
''','-1\n6\n')

    def test_function_output(self):
        self.check(MODULE+'''integer function create(x) result(n)
type(resource),allocatable,intent(out)::x
allocate(x)
x%value=8
n=3
end function
end module
use m
implicit none
type(resource),allocatable::x
integer::n
allocate(x)
x%value=4
n=create(x)
print *,n,x%value
deallocate(x)
end
''','final 4\n3 8\nfinal 8\n')

    def test_move_alloc_through_dummies(self):
        self.check(MODULE+'''subroutine move(from,to)
type(resource),allocatable,intent(inout)::from,to
call move_alloc(from,to)
print *,allocated(from),allocated(to)
end subroutine
end module
use m
implicit none
type(resource),allocatable::x,y
allocate(x,y)
x%value=3
y%value=4
call move(x,y)
print *,allocated(x),y%value
deallocate(y)
end
''','final 4\nF T\nF 3\nfinal 3\n')

    def test_assignment_to_unallocated_dummy(self):
        self.check(MODULE+'''subroutine set_it(x)
type(resource),allocatable,intent(inout)::x
x=resource(5)
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
call set_it(x)
call set_it(x)
print *,x%value
deallocate(x)
end
''','final 5\n5\nfinal 5\n')

    def test_generic_scalar_and_array(self):
        module=MODULE.replace('contains\nsubroutine finish', '''interface reset
module procedure reset_scalar,reset_vector
end interface
contains
subroutine finish''')
        self.check(module+'''subroutine reset_scalar(x)
type(resource),allocatable,intent(out)::x
allocate(x)
x%value=3
end subroutine
subroutine reset_vector(x)
type(resource),allocatable,intent(out)::x(:)
allocate(x(2))
x%value=4
end subroutine
end module
use m
implicit none
type(resource),allocatable::x,y(:)
call reset(x)
call reset(y)
print *,x%value,y%value
end
''','3 4 4\n')

    def test_reject_type_and_rank_mismatches(self):
        for declaration in ('type(resource),allocatable::x(:)',
                            'type(other),allocatable::x'):
            with self.subTest(declaration=declaration):
                self.reject(MODULE.replace('type::resource', 'type::other\n integer::value\nend type\ntype::resource',1)+'''subroutine use_it(x)
type(resource),allocatable,intent(in)::x
end subroutine
end module
use m
implicit none
'''+declaration+'''
call use_it(x)
end
''', 'ALLOCATABLE scalar')

    def test_reject_readonly_actual(self):
        self.reject(MODULE+'''subroutine change(x)
type(resource),allocatable,intent(inout)::x
end subroutine
subroutine forward(x)
type(resource),allocatable,intent(in)::x
call change(x)
end subroutine
end module
end
''','cannot pass INTENT(IN)')

    def test_separate_module(self):
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',MODULE+'''subroutine reset(x)
type(resource),allocatable,intent(out)::x
allocate(x)
x%value=9
end subroutine
end module
''')
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','''use m
implicit none
type(resource),allocatable::x
call reset(x)
call reset(x)
print *,x%value
deallocate(x)
end
''')
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'final 9\n9\nfinal 9')

    def test_nested_owned_components_on_output_entry(self):
        module=MODULE.replace('contains\nsubroutine finish', '''type::container
 type(resource),allocatable::item
 type(resource)::items(2)
end type
contains
subroutine finish''')
        self.check(module+'''subroutine reset(x)
type(container),allocatable,intent(out)::x
print *,allocated(x)
allocate(x)
print *,allocated(x%item)
allocate(x%item)
x%item%value=3
end subroutine
end module
use m
implicit none
type(container),allocatable::x
call reset(x)
call reset(x)
deallocate(x)
end
''','F\nF\nfinal 3\nF\nF\nfinal 3\n')

    def test_pure_output_procedure(self):
        module=MODULE.replace('subroutine finish(x)','pure subroutine finish(x)').replace("print *, 'final',x%value",'x%value=0')
        self.check(module+'''pure subroutine reset(x)
type(resource),allocatable,intent(out)::x
allocate(x)
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
allocate(x)
call reset(x)
print *,x%value
end
''','7\n')

    def test_recursive_descriptor_limit_diagnosed(self):
        self.reject(MODULE+'''recursive subroutine create(x,n)
type(resource),allocatable,intent(inout)::x
integer,intent(in)::n
if (n>0) then
 call create(n=n-1,x=x)
else
 allocate(x)
 x%value=6
end if
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
call create(x,3)
print *,x%value
deallocate(x)
end
''','recursive ALLOCATABLE/POINTER dummy')

    def test_reject_definition_of_input_dummy(self):
        self.reject(MODULE+'''subroutine change(x)
type(resource),allocatable,intent(in)::x
x%value=1
end subroutine
end module
use m
implicit none
type(resource),allocatable::x
allocate(x)
call change(x)
end
''', 'INTENT(IN)')

    def test_multiple_descriptors_keywords_and_omission(self):
        self.check(MODULE+'''subroutine create(a,n,b)
type(resource),allocatable,intent(out)::a
integer,intent(in)::n
type(resource),allocatable,intent(out),optional::b
allocate(a)
a%value=n
if (present(b)) then
 allocate(b)
 b%value=n+1
end if
end subroutine
end module
use m
implicit none
type(resource),allocatable::x,y
call create(n=3,a=x,b=y)
print *,x%value,y%value
call create(n=5,a=x)
print *,x%value,y%value
end
''','3 4\nfinal 3\n5 4\n')

    def test_type_bound_procedure(self):
        module=MODULE.replace('contains\nsubroutine finish', '''type::factory
contains
 procedure::create
end type
contains
subroutine finish''')
        self.check(module+'''subroutine create(self,x)
class(factory),intent(in)::self
type(resource),allocatable,intent(out)::x
allocate(x)
x%value=8
end subroutine
end module
use m
implicit none
type(factory)::f
type(resource),allocatable::x
call f%create(x=x)
print *,x%value
deallocate(x)
end
''','8\nfinal 8\n')

    def test_component_actual(self):
        module=MODULE.replace('contains\nsubroutine finish', '''type::container
 type(resource),allocatable::item
end type
contains
subroutine finish''')
        self.check(module+'''subroutine inspect(x)
type(resource),allocatable,intent(in)::x
print *,allocated(x)
end subroutine
end module
use m
implicit none
type(container)::x
call inspect(x%item)
end
''','F\n')

    def test_separate_intent_declaration(self):
        self.reject(MODULE+'''subroutine change(x)
type(resource),allocatable,intent(inout)::x
end subroutine
subroutine forward(x)
type(resource),allocatable::x
intent(in)::x
call change(x)
end subroutine
end module
end
''','cannot pass INTENT(IN)')

if __name__=='__main__':
    unittest.main()

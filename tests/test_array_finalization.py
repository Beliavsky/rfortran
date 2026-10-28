"""Rank-specific FINAL dispatch, array lifetimes and elemental fallback."""
import unittest
import test_deallocate
import test_module_compilation
from test_record_read import ROOT

MODULE='''module m
implicit none
type::item
 integer::value=7
 integer,allocatable::data(:)
contains
 final::scalar_final,vector_final
end type
contains
subroutine scalar_final(x)
type(item),intent(inout)::x
print *, 'scalar',x%value
end subroutine
subroutine vector_final(x)
type(item),intent(inout)::x(:)
print *, 'vector',size(x),sum(x%value)
x%value=-1
end subroutine
'''

class ArrayFinalization(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xarray_final.f90').read_text(encoding='utf-8'),
                   'vector 2 5\nvalues 1 1 2\nvector 3 4\nvector 0 0\nelement 8\nelement 9\n')

    def test_scope_and_empty(self):
        self.check(MODULE+'''subroutine work(n)
integer,intent(in)::n
type(item)::x(n)
x%value=3
end subroutine
end module
use m
implicit none
call work(2)
call work(0)
end
''','vector 2 6\nvector 0 0\n')

    def test_explicit_deallocation(self):
        self.check(MODULE+'''end module
use m
implicit none
type(item),allocatable::x(:)
allocate(x(-2:0))
x%value=[1,2,3]
allocate(x(-2)%data(2))
deallocate(x)
allocate(x(0))
deallocate(x)
end
''','vector 3 6\nvector 0 0\n')

    def test_assignment_rhs_and_overlap(self):
        self.check(MODULE+'''subroutine work()
type(item)::x(3),y
x%value=[1,2,3]
y%value=9
x(2:3)=x(1:2)
print *,x%value
x=y
print *,x%value
end subroutine
end module
use m
implicit none
call work()
end
''','vector 2 5\n1 1 2\nvector 3 4\n9 9 9\nscalar 9\nvector 3 27\n',reference=False)

    def test_output_and_preserved_bounds(self):
        self.check(MODULE+'''subroutine reset(x)
type(item),intent(out)::x(:)
print *,x%value
print *,allocated(x(1)%data)
x%value=4
end subroutine
end module
use m
implicit none
type(item)::x(-2:-1)
x%value=[1,2]
allocate(x(-2)%data(2))
call reset(x)
call reset(x)
print *,lbound(x,1),ubound(x,1),x%value
end
''','vector 2 3\n7 7\nF\nvector 2 8\n7 7\nF\n-2 -1 4 4\n')

    def test_allocatable_assignment_and_move(self):
        self.check(MODULE+'''end module
use m
implicit none
type(item),allocatable::x(:),y(:)
allocate(x(-1:0),y(1))
x%value=[1,2]
y%value=8
x=y
print *,size(x),x%value
call move_alloc(y,x)
print *,allocated(y),x%value
deallocate(x)
end
''','vector 2 3\n1 8\nvector 1 8\nF 8\nvector 1 8\n')

    def test_rank_two_without_scalar_fallback(self):
        self.check(MODULE+'''subroutine work()
type(item)::x(2,2)
x%value=3
end subroutine
end module
use m
implicit none
call work()
print *, 'done'
end
''','done\n')

    def test_rank_two_finalizer(self):
        module=MODULE.replace('final::scalar_final,vector_final','final::scalar_final,vector_final,matrix_final')
        self.check(module+'''subroutine matrix_final(x)
type(item),intent(inout)::x(:,:)
print *, 'matrix',shape(x),sum(x%value)
end subroutine
subroutine work(n)
integer,intent(in)::n
type(item),allocatable::x(:,:)
allocate(x(-1:0,3:n+2))
x%value=2
end subroutine
end module
use m
implicit none
call work(3)
end
''','matrix 2 3 12\n')

    def test_elemental_fallback(self):
        self.check('''module m
implicit none
type::item
 integer::value=7
contains
 final::finish
end type
contains
impure elemental subroutine finish(x)
type(item),intent(inout)::x
print *,x%value
end subroutine
subroutine work()
type(item)::x(2,2)
x%value=reshape([1,2,3,4],[2,2])
end subroutine
end module
use m
implicit none
call work()
end
''','1\n2\n3\n4\n')

    def test_specific_precedes_elemental(self):
        module=MODULE.replace('subroutine scalar_final(x)','impure elemental subroutine scalar_final(x)',1)
        self.check(module+'''end module
use m
implicit none
type(item),allocatable::x(:),y(:,:)
allocate(x(2),y(2,1))
x%value=[1,2]
y%value=3
deallocate(x)
deallocate(y)
end
''','vector 2 3\nscalar 3\nscalar 3\n')

    def test_nested_components_follow_whole_array_finalizer(self):
        self.check('''module m
implicit none
type::child
 integer::value=2,unset
contains
 final::end_child
end type
type::parent
 type(child)::c
 integer,allocatable::data(:)
 integer::unset
contains
 final::end_parent
end type
contains
subroutine end_child(x)
type(child),intent(inout)::x
print *, 'child',x%value
end subroutine
subroutine end_parent(x)
type(parent),intent(inout)::x(:)
print *, 'parent',size(x)
x%c%value=4
end subroutine
subroutine work(n)
integer,intent(in)::n
type(parent)::x(n)
end subroutine
end module
use m
implicit none
call work(2)
call work(0)
end
''','parent 2\nchild 4\nchild 4\nparent 0\n')

    def test_rank_fifteen(self):
        colons=','.join([':']*15)
        ones=','.join(['1']*15)
        module=MODULE.replace('final::scalar_final,vector_final','final::scalar_final,vector_final,rank15_final')
        self.check(module+f'''subroutine rank15_final(x)
type(item),intent(inout)::x({colons})
print *, 'rank15',size(x)
end subroutine
subroutine work()
type(item)::x({ones})
end subroutine
end module
use m
implicit none
call work()
end
''','rank15 1\n')

    def test_pure_rank_finalizer(self):
        self.check('''module m
implicit none
type::item
 integer::value=1
contains
 final::finish
end type
contains
pure subroutine finish(x)
type(item),intent(inout)::x(:)
x%value=0
end subroutine
pure subroutine work(n)
integer,intent(in)::n
type(item)::x(n)
end subroutine
end module
use m
implicit none
call work(2)
print *, 'done'
end
''','done\n')

    def test_all_elemental_calls_precede_nested_components(self):
        self.check('''module m
implicit none
type::child
 integer::value=2
contains
 final::end_child
end type
type::parent
 type(child)::c
contains
 final::end_parent
end type
contains
subroutine end_child(x)
type(child),intent(inout)::x
print *, 'child',x%value
end subroutine
impure elemental subroutine end_parent(x)
type(parent),intent(inout)::x
print *, 'parent'
x%c%value=4
end subroutine
subroutine work()
type(parent)::x(2)
end subroutine
end module
use m
implicit none
call work()
end
''','parent\nparent\nchild 4\nchild 4\n')

    def test_designator_captured_before_finalizer(self):
        # Some gfortran versions reevaluate these bounds after calling FINAL.
        module=MODULE.replace('implicit none','implicit none\ninteger::lo=1,hi=2',1).replace('x%value=-1','x%value=-1\nlo=2\nhi=3')
        self.check(module+'''end module
use m
implicit none
type(item)::x(3)
x%value=[1,2,3]
x(lo:hi)=x(2:3)
print *,x%value,lo,hi
end
''','vector 2 3\n2 3 3 2 3\n',reference=False)

    def test_constructor_rhs_evaluated_once(self):
        module=MODULE.replace(' integer,allocatable::data(:)\n','').replace('implicit none','implicit none\ninteger::calls=0',1)
        self.check(module+'''integer function next()
calls=calls+1
print *, 'rhs',calls
next=calls
end function
end module
use m
implicit none
type(item)::x(2)
x=[item(next()),item(next())]
print *,x%value,calls
end
''','rhs 1\nrhs 2\nvector 2 14\n1 2 2\n')

    def test_allocatable_reallocation_bounds(self):
        self.check(MODULE+'''end module
use m
implicit none
type(item),allocatable::x(:),y(:)
allocate(y(-2:0))
y%value=[1,2,3]
x=y
print *,lbound(x),ubound(x),x%value
deallocate(y)
allocate(y(4:5))
y%value=9
x=y
print *,lbound(x),ubound(x),x%value
deallocate(x)
end
''','-2 0 1 2 3\nvector 3 6\nvector 3 6\n4 5 9 9\nvector 2 18\n')

    def test_vector_and_scalar_elements(self):
        self.check(MODULE+'''end module
use m
implicit none
type(item)::x(3),y
x%value=[1,2,3]
y%value=8
x([3,1])=y
print *,x%value
x(2)=y
print *,x%value
end
''','vector 2 4\n8 2 8\nscalar 2\n8 8 8\n')

    def test_writable_user_call_still_rejects_vector_actual(self):
        self.reject(MODULE+'''end module
use m
implicit none
type(item)::x(3)
call vector_final(x([3,1]))
end
''','vector-subscript actual argument')

    def test_optional_allocatable_output(self):
        self.check(MODULE+'''subroutine reset(x)
type(item),allocatable,intent(out),optional::x(:)
if (.not.present(x)) return
print *,allocated(x)
allocate(x(2))
print *,x%value
end subroutine
end module
use m
implicit none
type(item),allocatable::x(:)
call reset()
allocate(x(0))
call reset(x)
deallocate(x)
end
''','vector 0 0\nF\n7 7\nvector 2 14\n')

    def test_separate_private_rank_finalizer(self):
        module=MODULE.replace('implicit none','implicit none\nprivate\npublic::item',1)+'end module\n'
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',module)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','''use m,only:local_item=>item
implicit none
block
type(local_item)::x(2)
x%value=3
end block
end
''')
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'vector 2 6')

    def test_component_view_assignment(self):
        module=MODULE.replace(
            'end type\ncontains', 'end type\ntype::parent\n type(item)::child\nend type\ncontains',1)
        self.check(module+'''end module
use m
implicit none
type(parent)::x(3)
type(item)::y
x%child%value=[1,2,3]
y%value=9
x(2:3)%child=y
print *,x%child%value
x([3,1])%child=y
print *,x%child%value
end
''','vector 2 5\n1 9 9\nvector 2 10\n9 9 9\n')

    def test_vector_finalizer_releases_owned_storage(self):
        module=MODULE.replace('x%value=-1','''block
integer::i
do i=1,size(x)
if (allocated(x(i)%data)) deallocate(x(i)%data)
end do
end block''')
        self.check(module+'''end module
use m
implicit none
type(item)::x(3),y
allocate(x(3)%data(2),x(1)%data(1))
y%value=9
x([3,1])=y
print *,allocated(x(3)%data),allocated(x(1)%data),x%value
end
''','vector 2 14\nF F 9 7 9\n')

    def test_unallocated_component_in_nonfinal_nested_type(self):
        module=MODULE.replace('type::item','''type::payload
 integer,allocatable::data(:)
end type
type::item''',1).replace(' integer,allocatable::data(:)\ncontains',' type(payload)::p\ncontains',1)
        self.check(module+'''end module
use m
implicit none
type(item)::x(2),y
x=y
print *,allocated(x(1)%p%data),x%value
end
''','vector 2 14\nF 7 7\n')

    def test_output_section_with_unread_undefined_component(self):
        module=MODULE.replace('integer::value=7','integer::value=7,unset',1)
        self.check(module+'''subroutine reset(x)
type(item),intent(out)::x(:)
print *,x%value
end subroutine
end module
use m
implicit none
type(item)::x(3)
call reset(x(1:2))
print *,x%value
end
''','vector 2 14\n7 7\n7 7 7\n')

    def test_output_section_preserves_undefined_masks(self):
        module=MODULE.replace('integer::value=7','integer::value=7,unset',1)
        self.check(module+'''subroutine reset(x)
type(item),intent(out)::x(:)
end subroutine
end module
use m
implicit none
type(item)::x(3)
call reset(x(1:2))
print *,x(1)%unset
end
''',error='uninitialized variable')

    def test_character_output_section_transport(self):
        module=MODULE.replace('integer::value=7','''integer::value=7
 character(3)::label='abc',unset_char''',1)
        for declaration in ['type(item)::x(3)', 'type(item),allocatable::x(:)\nallocate(x(3))']:
            with self.subTest(declaration=declaration):
                self.check(module+'''subroutine reset(x)
type(item),intent(out)::x(:)
end subroutine
end module
use m
implicit none
'''+declaration+'''
call reset(x(1:2))
print *,x%label
end
''','vector 2 14\nabcabcabc\n')

    def test_character_output_section_preserves_undefined_masks(self):
        module=MODULE.replace('integer::value=7','''integer::value=7
 character(3)::unset_char''',1)
        for declaration in ['type(item)::x(3)', 'type(item),allocatable::x(:)\nallocate(x(3))']:
            with self.subTest(declaration=declaration):
                self.check(module+'''subroutine reset(x)
type(item),intent(out)::x(:)
end subroutine
end module
use m
implicit none
'''+declaration+'''
call reset(x(1:2))
print *,x(1)%unset_char
end
''',error='uninitialized variable')

if __name__=='__main__': unittest.main()

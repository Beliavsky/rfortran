"""FINAL dispatch and lifetimes of fixed derived-type array components."""
import unittest
import test_deallocate
from test_record_read import ROOT

MODULE = '''module m
implicit none
type::resource
 integer::value=7
contains
 final::finish_scalar,finish_vector
end type
type::container
 type(resource)::items(-1:1)
end type
contains
subroutine finish_scalar(x)
type(resource),intent(inout)::x
print *, 'scalar',x%value
end subroutine
subroutine finish_vector(x)
type(resource),intent(inout)::x(:)
print *, 'vector',size(x),sum(x%value)
end subroutine
'''

class FixedComponentFinalization(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xfixed_component_final.f90').read_text(),
                   'parent 2 3 4\nitems 3 9\n')

    def test_parent_without_final_and_return(self):
        self.check(MODULE+'''subroutine work()
type(container)::x
x%items%value=[1,2,3]
return
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 6\n')

    def test_explicit_parent_deallocation(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container),allocatable::x
allocate(x)
x%items%value=2
deallocate(x)
end
''', 'vector 3 6\n')

    def test_assignment(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x,y
x%items%value=[1,2,3]
y%items%value=[4,5,6]
x=y
print *,x%items%value
y%items%value=0
print *,x%items%value
end
''', 'vector 3 6\n4 5 6\n4 5 6\n')

    def test_component_assignment_and_bounds(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x,y
x%items%value=[1,2,3]
y%items%value=[4,5,6]
x%items=y%items
print *,lbound(x%items),ubound(x%items),x%items%value
end
''', 'vector 3 6\n-1 1 4 5 6\n')

    def test_intent_out(self):
        self.check(MODULE+'''subroutine reset(x)
type(container),intent(out)::x
print *,x%items%value
x%items%value=9
end subroutine
end module
use m
implicit none
type(container)::x
x%items%value=[1,2,3]
call reset(x)
print *,x%items%value
end
''', 'vector 3 6\n7 7 7\n9 9 9\n')

    def test_array_of_parents(self):
        self.check(MODULE+'''subroutine fill(x,n)
type(container),intent(inout)::x
integer,intent(in)::n
x%items%value=n
end subroutine
subroutine work()
type(container)::x(2)
call fill(x(1),2)
call fill(x(2),3)
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 6\nvector 3 9\n')

    def test_empty_component(self):
        self.check(MODULE.replace('items(-1:1)', 'items(0)')+'''subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 0 0\n')

    def test_empty_components_in_array_parents(self):
        self.check(MODULE.replace('items(-1:1)', 'items(0)')+'''subroutine work()
type(container)::x(2)
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 0 0\nvector 0 0\n')

    def test_scalar_non_elemental_not_array_fallback(self):
        module=MODULE.replace('final::finish_scalar,finish_vector','final::finish_scalar')
        self.check(module+'''subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
print *, 'done'
end
''', 'done\n')

    def test_elemental_fallback(self):
        self.check('''module m
implicit none
integer::count=0
type::resource
 integer::value=7
contains
 final::finish
end type
type::container
 type(resource)::items(2,3)
end type
contains
impure elemental subroutine finish(x)
type(resource),intent(inout)::x
count=count+x%value
end subroutine
subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
print *,count
end
''', '42\n')

    def test_rank_specific_matrix(self):
        self.check(MODULE.replace('items(-1:1)','items(-1:0,3:4)').replace(
            'final::finish_scalar,finish_vector','final::finish_scalar,finish_vector,finish_matrix')+'''
subroutine finish_matrix(x)
type(resource),intent(inout)::x(:,:)
print *, 'matrix',shape(x),sum(x%value)
end subroutine
subroutine work()
type(container)::x
x%items%value=2
end subroutine
end module
use m
implicit none
call work()
end
''', 'matrix 2 2 8\n')

    def test_rank_fifteen(self):
        dims=','.join(['1']*15)
        deferred=','.join([':']*15)
        self.check(MODULE.replace('items(-1:1)',f'items({dims})').replace(
            'final::finish_scalar,finish_vector','final::finish_scalar,finish_vector,finish_rank15')+f'''
subroutine finish_rank15(x)
type(resource),intent(inout)::x({deferred})
print *, 'rank15',size(x),sum(x%value)
end subroutine
subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
end
''', 'rank15 1 7\n')

    def test_combined_storage_rank_limit_diagnosed(self):
        dims=','.join(['1']*15)
        deferred=','.join([':']*15)
        self.reject(MODULE.replace('items(-1:1)',f'items({dims})').replace(
            'final::finish_scalar,finish_vector','final::finish_scalar,finish_vector,finish_rank15')+f'''
subroutine finish_rank15(x)
type(resource),intent(inout)::x({deferred})
print *, 'rank15',size(x),sum(x%value)
end subroutine
subroutine work()
type(container)::x(2)
end subroutine
end module
use m
implicit none
call work()
end
''', 'array rank exceeds 15')

    def test_nested_scalar_and_fixed_components(self):
        self.check(MODULE+'''subroutine work()
type::outer
 type(container)::nested
end type
type(outer)::x
x%nested%items%value=3
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 9\n')

    def test_child_owns_allocation(self):
        module=MODULE.replace('integer::value=7', 'integer::value=7\n integer,allocatable::data(:)')
        self.check(module+'''subroutine work()
type(container)::x
allocate(x%items(-1)%data(2))
x%items(-1)%data=[1,2]
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 21\n')

    def test_assignment_deep_copies_owned_child_storage(self):
        module=MODULE.replace('integer::value=7', 'integer::value=7\n integer,allocatable::data(:)')
        self.check(module+'''end module
use m
implicit none
type(container)::x,y
allocate(x%items(-1)%data(-2:-1))
x%items(-1)%data=[3,4]
y=x
x%items(-1)%data=9
print *,lbound(y%items(-1)%data),y%items(-1)%data
print *,allocated(y%items(0)%data)
end
''', 'vector 3 21\n-2 3 4\nF\n')

    def test_fixed_array_of_containers(self):
        self.check(MODULE+'''subroutine work()
type::outer
 type(container)::children(2)
end type
type(outer)::x
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 21\nvector 3 21\n', reference=False)  # gfortran combines nested component arrays into one FINAL call.

    def test_components_with_unread_undefined_fields(self):
        module=MODULE.replace('integer::value=7', 'integer::value=7\n integer::unused\n real::unused_array(2)\n character(len=3)::unused_char')
        self.check(module+'''subroutine work()
type(container)::x(2)
end subroutine
end module
use m
implicit none
call work()
end
''', 'vector 3 21\nvector 3 21\n')

    def test_pure_cleanup(self):
        module=MODULE.replace('subroutine finish_scalar(x)','pure subroutine finish_scalar(x)').replace(
            'subroutine finish_vector(x)','pure subroutine finish_vector(x)').replace(
            "print *, 'scalar',x%value",'x%value=0').replace(
            "print *, 'vector',size(x),sum(x%value)",'x%value=0')
        self.check(module+'''pure subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
print *, 'done'
end
''', 'done\n')

    def test_block_exit_and_cycle(self):
        self.check(MODULE+'''end module
use m
implicit none
integer::i
do i=1,2
 block
  type(container)::x
  x%items%value=i
  if (i==1) cycle
  exit
 end block
end do
print *, 'done'
end
''', 'vector 3 3\nvector 3 6\ndone\n')

    def test_assignment_preserves_rhs_before_final(self):
        module=MODULE.replace("print *, 'vector',size(x),sum(x%value)",
                              "print *, 'vector',size(x),sum(x%value)\nx%value=-1")
        self.check(module+'''end module
use m
implicit none
type(container)::x
x%items%value=[1,2,3]
x=x
print *,x%items%value
end
''', 'vector 3 6\n1 2 3\n', reference=False)  # gfortran copies after FINAL mutates the RHS alias.

    def test_parent_inquiries_project_component_axes(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x(-2:0,4:5)
integer::i
print *,shape(x),size(x),lbound(x),ubound(x)
do i=1,2
 print *,size(x,i),lbound(x,i),ubound(x,i)
end do
print *,shape(x(:,4)),size(x(-2:0:2,4:5))
end
''', '3 2 6 -2 4 0 5\n3 -2 0\n2 4 5\n3 4\n')

    def test_output_and_pack_preserve_component_bounds(self):
        # Isolate storage bounds from function-result finalization (a separate feature).
        module=MODULE.replace('contains\n final::finish_scalar,finish_vector\n', '')
        self.check(module+'''end module
use m
implicit none
type(container)::x(2)
print *,x(1)
print *,pack(x,[.true.,.false.])
end
''', '7 7 7\n7 7 7\n')

    def test_runtime_dim_rejects_component_axes(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x(2)
integer::axis
read *,axis
print *,size(x,axis)
end
''', data='0\n', error='DIM must be between 1 and 1', reference=False)

    def test_inquiry_keywords(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x(2,3)
integer::axis
axis=2
print *,size(x,kind=4),shape(x,kind=4),lbound(x,kind=4),ubound(x,kind=4)
print *,size(x,dim=axis,kind=4),ubound(x,dim=axis,kind=4)
end
''', '6 2 3 1 1 2 3\n3 3\n')

if __name__ == '__main__':
    unittest.main()

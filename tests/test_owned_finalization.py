"""FINAL for allocated derived-type components and their owning containers."""
import unittest
import test_deallocate
import test_module_compilation
from test_record_read import ROOT

MODULE='''module m
implicit none
type::resource
 integer::value=7
contains
 final::end_scalar,end_vector
end type
type::container
 type(resource),allocatable::item
 type(resource),allocatable::items(:)
contains
 final::end_container
end type
contains
subroutine end_scalar(x)
type(resource),intent(inout)::x
print *, 'scalar',x%value
end subroutine
subroutine end_vector(x)
type(resource),intent(inout)::x(:)
print *, 'vector',size(x),sum(x%value)
end subroutine
subroutine end_container(x)
type(container),intent(inout)::x
print *, 'parent',allocated(x%item),allocated(x%items)
end subroutine
'''

class OwnedFinalization(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_fixture(self):
        self.check((ROOT/'tests/programs/xowned_final.f90').read_text(),
                   'parent T T\nscalar 3\nvector 2 9\n')

    def test_scope_components(self):
        self.check(MODULE+'''subroutine work()
type(container)::x
allocate(x%item,x%items(2))
x%item%value=3
x%items%value=[4,5]
end subroutine
end module
use m
implicit none
call work()
end
''','parent T T\nscalar 3\nvector 2 9\n')

    def test_unallocated_components(self):
        self.check(MODULE+'''subroutine work()
type(container)::x
end subroutine
end module
use m
implicit none
call work()
end
''','parent F F\n')

    def test_empty_array_without_parent_final(self):
        module=MODULE.replace('contains\n final::end_container\n','')
        self.check(module+'''subroutine work()
type(container)::x
allocate(x%items(0))
end subroutine
end module
use m
implicit none
call work()
end
''','vector 0 0\n')

    def test_explicit_component_deallocation(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x
allocate(x%item,x%items(2))
x%item%value=3
x%items%value=[4,5]
deallocate(x%item,x%items)
print *,allocated(x%item),allocated(x%items)
end
''','scalar 3\nvector 2 9\nF F\n')

    def test_assignment_snapshot(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x,y
allocate(x%item,x%items(1),y%item,y%items(2))
x%item%value=1
x%items%value=2
y%item%value=3
y%items%value=4
x=y
print *,x%item%value,sum(x%items%value)
deallocate(x%items)
print *,allocated(y%items)
end
''','parent T T\nscalar 1\nvector 1 2\n3 8\nvector 2 8\nT\n')

    def test_output(self):
        self.check(MODULE+'''subroutine reset(x)
type(container),intent(out)::x
print *,allocated(x%item),allocated(x%items)
allocate(x%item,x%items(0))
print *,x%item%value
end subroutine
end module
use m
implicit none
type(container)::x
allocate(x%item,x%items(1))
x%item%value=1
x%items%value=2
call reset(x)
deallocate(x%item,x%items)
end
''','parent T T\nscalar 1\nvector 1 2\nF F\n7\nscalar 7\nvector 0 0\n')

    def test_child_owns_primitive_allocation(self):
        module=MODULE.replace('integer::value=7','integer::value=7\n integer,allocatable::data(:)',1)
        self.check(module+'''end module
use m
implicit none
type(container)::x,y
allocate(x%item,x%items(1))
x%item%data=[1,2]
x%items(1)%data=[3,4,5]
y=x
x%item%data=9
print *,y%item%data,y%items(1)%data
deallocate(x%item,x%items,y%item,y%items)
end
''','parent F F\n1 2 3 4 5\nscalar 7\nvector 1 7\nscalar 7\nvector 1 7\n')

    def test_rank_two_component(self):
        module=MODULE.replace('items(:)','items(:,:)').replace('x(:)','x(:,:)')
        self.check(module+'''subroutine work()
type(container)::x
allocate(x%items(-1:0,3:4))
x%items%value=5
end subroutine
end module
use m
implicit none
call work()
end
''','parent F T\nvector 4 20\n')

    def test_array_of_containers(self):
        self.check(MODULE+'''subroutine fill(x,n)
type(container),intent(inout)::x
integer,intent(in)::n
allocate(x%item,x%items(0))
x%item%value=n
end subroutine
subroutine work()
type(container)::x(2)
call fill(x(1),3)
call fill(x(2),4)
end subroutine
end module
use m
implicit none
call work()
end
''','scalar 3\nvector 0 0\nscalar 4\nvector 0 0\n')

    def test_scalar_assignment_allocates_component(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x
x%item=resource(3)
print *,allocated(x%item),x%item%value
x%item=resource(4)
deallocate(x%item)
end
''','T 3\nscalar 3\nscalar 4\n')

    def test_array_assignment_allocates_and_resizes_component(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x
x%items=[resource(3),resource(4)]
print *,allocated(x%items),sum(x%items%value)
x%items=[resource(5),resource(5),resource(5)]
deallocate(x%items)
end
''','T 7\nvector 2 7\nvector 3 15\n')

    def test_unsupported_layouts_diagnosed(self):
        self.reject(MODULE.replace('type(resource),allocatable::item','class(resource),allocatable::item')+'end module\nend\n','polymorphic components')
        self.reject(MODULE.replace('type(resource),allocatable::items(:)','type(resource)::items(2)')+'end module\nend\n','fixed array components')
        self.reject(MODULE.replace('integer::value=7','integer::value=7\n type(resource),allocatable::next',1)+'end module\nend\n','recursive owned layouts')

    def test_finalizer_deallocates_own_component(self):
        module=MODULE.replace("print *, 'parent',allocated(x%item),allocated(x%items)","print *, 'parent',allocated(x%item),allocated(x%items)\nif (allocated(x%item)) deallocate(x%item)")
        self.check(module+'''subroutine work()
type(container)::x
allocate(x%item,x%items(1))
x%item%value=3
x%items%value=4
end subroutine
end module
use m
implicit none
call work()
end
''','parent T T\nscalar 3\nvector 1 4\n')

    def test_rank_fifteen_component(self):
        dims=','.join([':']*15)
        bounds=','.join(['1']*15)
        module=MODULE.replace('items(:)',f'items({dims})').replace('x(:)',f'x({dims})')
        self.check(module+f'''subroutine work()
type(container)::x
allocate(x%items({bounds}))
x%items%value=5
end subroutine
end module
use m
implicit none
call work()
end
''','parent F T\nvector 1 5\n')

    def test_nested_owned_container(self):
        module=MODULE.replace('contains\nsubroutine end_scalar','type::outer\n type(container),allocatable::box\nend type\ncontains\nsubroutine end_scalar',1)
        self.check(module+'''subroutine fill(x)
type(container),intent(inout)::x
allocate(x%item,x%items(1))
x%item%value=3
x%items%value=4
end subroutine
subroutine work()
type(outer)::x
allocate(x%box)
call fill(x%box)
end subroutine
end module
use m
implicit none
call work()
end
''','parent T T\nscalar 3\nvector 1 4\n')

    def test_move_alloc_components(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container)::x,y
allocate(x%item,y%item,x%items(1),y%items(1))
x%item%value=3
y%item%value=9
x%items%value=4
call move_alloc(from=x%item,to=y%item)
call move_alloc(x%items,y%items)
print *,allocated(x%item),allocated(x%items),y%item%value
deallocate(y%item,y%items)
end
''','scalar 9\nvector 1 7\nF F 3\nscalar 3\nvector 1 4\n')

    def test_deallocate_allocatable_parent(self):
        self.check(MODULE+'''end module
use m
implicit none
type(container),allocatable::x
allocate(x)
allocate(x%item,x%items(1))
x%item%value=3
x%items%value=4
deallocate(x)
print *,allocated(x)
end
''','parent T T\nscalar 3\nvector 1 4\nF\n')

    def test_pure_owned_cleanup(self):
        module=MODULE.replace('contains\n final::end_container\n','').replace('subroutine end_scalar(x)','pure subroutine end_scalar(x)').replace('subroutine end_vector(x)','pure subroutine end_vector(x)').replace("print *, 'scalar',x%value",'x%value=0').replace("print *, 'vector',size(x),sum(x%value)",'x%value=0')
        self.check(module+'''pure subroutine work()
type(container)::x
allocate(x%item,x%items(2))
end subroutine
end module
use m
implicit none
call work()
print *, 'done'
end
''','done\n')

    def test_array_owner_preserves_unread_undefined_fields(self):
        module=MODULE.replace('type::container','''type::container
 integer::unset,scratch(2)
 character(3)::unset_char''',1)
        self.check(module+'''subroutine fill(x)
type(container),intent(inout)::x
allocate(x%item,x%items(0))
end subroutine
subroutine work()
type(container)::x(2)
call fill(x(1))
end subroutine
end module
use m
implicit none
call work()
end
''','scalar 7\nvector 0 0\n')

    def test_separate_private_finalizers(self):
        module=MODULE.replace('implicit none','implicit none\nprivate\npublic::container',1)+'end module\n'
        helper=test_module_compilation.ModuleCompilation()
        helper.setUp()
        self.addCleanup(helper.doCleanups)
        source=helper.source('owners',module)
        helper.success('-c',source)
        source.unlink()
        main=helper.source('main','''use m,only:container
implicit none
block
type(container)::x
allocate(x%item,x%items(0))
end block
end
''')
        helper.success('-c',main)
        helper.run_objects(['owners','main'],'parent T T\nscalar 7\nvector 0 0')

if __name__=='__main__':unittest.main()

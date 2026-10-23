"""Function argument association retains numeric allocation descriptors."""
import unittest
import test_deallocate
from test_record_read import ROOT


class AllocatableFunctionDummies(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_elemental_allocatable_component_size.f90').read_text(), '2\n')

    def test_allocatable_components_allocated_empty_and_unallocated(self):
        self.check('''module m
implicit none
type :: item
integer::id
end type
type :: container
type(item),allocatable::items(:)
end type
contains
elemental integer function count_items(c) result(n)
type(container),intent(in)::c
n=-1
if(allocated(c%items)) n=size(c%items)
end function
subroutine show(c)
type(container),intent(in)::c(:)
integer::i
do i=1,size(c)
print *,count_items(c(i))
end do
end subroutine
end module
use m
implicit none
type(container)::c(3)
integer::n
read *,n
allocate(c(1)%items(-2:n-3),c(3)%items(0))
call show(c)
deallocate(c(1)%items)
print *,count_items(c(1))
end
''', '3\n-1\n0\n-1\n', data='3\n')

    def test_component_ranks(self):
        for rank in (2,3,15):
            with self.subTest(rank=rank):
                axes=','.join([':']*rank)
                shape=','.join(['1']*(rank-1)+['n'])
                self.check(f'''module m
implicit none
type :: item
integer::id
end type
type :: container
type(item),allocatable::items({axes})
end type
contains
elemental integer function count_items(c) result(n)
type(container),intent(in)::c
n=-1
if(allocated(c%items)) n=size(c%items)
end function
end module
use m
implicit none
type(container)::c(1)
integer::n
read *,n
allocate(c(1)%items({shape}))
print *,count_items(c(1))
end
''', '3\n', data='3\n')

    def test_intrinsic_element_types_and_allocation_state(self):
        for kind in ('integer(1)','integer(2)','integer','integer(8)',
                     'real','double precision','logical'):
            with self.subTest(kind=kind):
                self.check(f'''module m
implicit none
contains
integer function count_items(x) result(n)
{kind},allocatable,intent(in)::x(:)
n=-1
if(allocated(x)) n=size(x)
end function
logical function has_items(x)
{kind},allocatable,intent(in)::x(:)
has_items=allocated(x)
end function
end module
use m
implicit none
{kind},allocatable::x(:)
integer::n
print *,count_items(x),has_items(x)
read *,n
allocate(x(-2:n-3))
print *,count_items(x),has_items(x)
deallocate(x)
allocate(x(0))
print *,count_items(x),has_items(x)
deallocate(x)
print *,count_items(x),has_items(x)
end
''', '-1 F\n3 T\n0 T\n-1 F\n', data='3\n')

    def test_multidimensional_bounds_are_preserved(self):
        self.check('''module m
implicit none
contains
integer function lower(x)
integer,allocatable,intent(in)::x(:,:)
lower=-99
if(allocated(x)) lower=lbound(x,1)
end function
integer function upper(x)
integer,allocatable,intent(in)::x(:,:)
upper=-99
if(allocated(x)) upper=ubound(x,2)
end function
end module
use m
implicit none
integer,allocatable::x(:,:)
integer::n
print *,lower(x)
read *,n
allocate(x(-2:n-3,4:5))
print *,lower(x),upper(x)
end
''', '-99\n-2 5\n', data='3\n')

    def test_inout_function_updates_caller_descriptor(self):
        self.check('''module m
implicit none
contains
integer function reset(x,n) result(count)
integer,allocatable,intent(inout)::x(:)
integer,intent(in)::n
if(allocated(x)) deallocate(x)
allocate(x(-1:n-2))
x=7
count=size(x)
end function
end module
use m
implicit none
integer,allocatable::x(:)
integer::n
n=reset(x,3)
print *,n,allocated(x),lbound(x,1),x
n=reset(x,0)
print *,n,allocated(x),size(x)
end
''', '3 T -1 7 7 7\n0 T 0\n')

    def test_reads_allocated_values(self):
        self.check('''module m
implicit none
contains
double precision function total(x)
double precision,allocatable,intent(in)::x(:)
total=-1d0
if(allocated(x)) total=sum(x)
end function
end module
use m
implicit none
double precision,allocatable::x(:)
print *,total(x)
allocate(x(3))
x=[1d0,2d0,4d0]
print *,total(x)
end
''', '-1\n7\n')

    def test_intent_out_function_replaces_allocation(self):
        self.check('''module m
implicit none
contains
integer function fill(x,n)
integer,allocatable,intent(out)::x(:)
integer,intent(in)::n
allocate(x(n))
x=5
fill=n
end function
end module
use m
implicit none
integer,allocatable::x(:)
integer::n
allocate(x(1))
x=9
n=fill(x,3)
print *,n,x
n=fill(x,0)
print *,n,allocated(x),size(x)
end
''', '3 5 5 5\n0 T 0\n')

    def test_reject_expression_as_allocatable_actual(self):
        self.reject('''module m
contains
integer function count_items(x)
integer,allocatable,intent(in)::x(:)
count_items=-1
if(allocated(x)) count_items=size(x)
end function
end module
use m
print *,count_items([1,2])
end
''', 'requires an ALLOCATABLE variable')

    def test_nonallocatable_dummy_still_requires_allocated_data(self):
        self.reject('''module m
contains
integer function inspect(x,y)
integer,allocatable,intent(in)::x(:)
integer,intent(in)::y(:)
inspect=size(y)
if(allocated(x)) inspect=inspect+size(x)
end function
end module
use m
integer,allocatable::x(:)
allocate(x(0))
deallocate(x)
print *,inspect(x,x)
end
''', 'array `x` is used before ALLOCATE')


if __name__ == '__main__':
    unittest.main()

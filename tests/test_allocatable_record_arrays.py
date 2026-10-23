"""Owned ALLOCATABLE components in arrays of derived types."""
import unittest
import test_deallocate
from test_record_read import ROOT

class AllocatableRecordArrays(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_nested_allocatable_records.f90').read_text(encoding='utf-8'), '2\n')

    def test_elemental_scalar_allocatable_record(self):
        program = (ROOT/'tests/programs/xacv_elemental_allocatable_argument.f90').read_text(encoding='utf-8')
        self.check(program, '3\n')
        self.check(program.replace('dvec(2)', 'dvec').replace('dvec(1)', 'dvec'), '3\n')

    def test_elemental_scalar_fixed_array_component(self):
        self.check("""module m
implicit none
type :: row
integer :: x(2)
end type
contains
elemental integer function total(a)
type(row),intent(in)::a
total=sum(a%x)
end function
end module
use m
implicit none
type(row)::a(2)
a(1)%x=[2,5]
print *,total(a(1))
end
""", '7\n')

    def test_elemental_owned_record_array_rejected(self):
        program = (ROOT/'tests/programs/xacv_elemental_allocatable_argument.f90').read_text(encoding='utf-8')
        self.reject(program.replace('num_obs(dvec(1))', 'num_obs(dvec)'),
                    'ELEMENTAL derived array actuals currently require scalar primitive components')

    def test_elemental_owned_record_scalar_broadcast_rejected(self):
        self.reject("""module m
implicit none
type :: row
integer,allocatable::x(:)
end type
contains
elemental integer function total(a,offset)
type(row),intent(in)::a
integer,intent(in)::offset
total=sum(a%x)+offset
end function
end module
use m
implicit none
type(row)::a
allocate(a%x(2))
a%x=3
print *,total(a,[1,2])
end
""", 'ELEMENTAL derived array actuals currently require scalar primitive components')

    def test_impure_elemental_scalar_record_inout(self):
        self.check("""module m
implicit none
type :: row
integer,allocatable::x(:)
end type
contains
impure elemental subroutine bump(a,n)
type(row),intent(inout)::a
integer,intent(in)::n
a%x=a%x+n
end subroutine
end module
use m
implicit none
type(row)::a(2)
allocate(a(1)%x(2))
a(1)%x=[3,5]
call bump(n=2,a=a(1))
print *,a(1)%x,allocated(a(2)%x)
end
""", '5 7 F\n')

    def test_independent_numeric_components_and_inquiries(self):
        self.check("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2)
print *,allocated(a(1)%x),allocated(a(2)%x)
allocate(a(1)%x(2),a(2)%x(3))
a(1)%x=[1,2]
a(2)%x=[10,20,30]
print *,size(a(1)%x),size(a(2)%x)
print *,sum(a(1)%x),sum(a(2)%x)
a(1)%x(2)=5
print *,a(1)%x,a(2)%x
deallocate(a(1)%x)
print *,allocated(a(1)%x),allocated(a(2)%x)
end
""", 'F F\n2 3\n3 60\n1 5 10 20 30\nF T\n')

    def test_runtime_outer_shape_and_scalar_procedures(self):
        self.check("""module m
implicit none
type :: row
real,allocatable::x(:)
end type
contains
subroutine fill(a,n)
type(row),intent(inout)::a
integer,intent(in)::n
allocate(a%x(n))
a%x=real(n)
end subroutine
real function total(a)
type(row),intent(in)::a
total=sum(a%x)
end function
end module
use m
implicit none
type(row),allocatable::a(:)
integer::n,i
read *,n
allocate(a(n))
do i=1,n
 call fill(a(i),i)
end do
print *,total(a(1)),total(a(n))
deallocate(a)
allocate(a(1))
print *,allocated(a(1)%x)
end
""", '1 9\nF\n', data='3\n')

    def test_bounds_empty_components_and_reallocation(self):
        self.check("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2)
allocate(a(1)%x(-2:0),a(2)%x(0))
a(1)%x=[3,4,5]
print *,lbound(a(1)%x),ubound(a(1)%x),a(1)%x(-1)
print *,allocated(a(2)%x),size(a(2)%x)
a(2)%x=[7,8]
print *,a(2)%x
deallocate(a(1)%x)
allocate(a(1)%x(1))
a(1)%x=42
print *,a(1)%x
end
""", '-2 0 4\nT 0\n7 8\n42\n')

    def test_scalar_element_deep_copy(self):
        self.check("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2)
allocate(a(1)%x(2))
a(1)%x=[1,2]
a(2)=a(1)
a(2)%x(1)=99
print *,a(1)%x,a(2)%x
a(1)=a(1)
deallocate(a(2)%x)
print *,a(1)%x,allocated(a(2)%x)
end
""", '1 2 99 2\n1 2 F\n')

    def test_derived_allocatable_components_through_element_calls(self):
        self.check("""module m
implicit none
type :: leaf
integer::year
end type
type :: row
type(leaf),allocatable::x(:)
end type
contains
subroutine fill(a,n)
type(row),intent(inout)::a
integer,intent(in)::n
allocate(a%x(n))
a%x%year=n
end subroutine
integer function total(a)
type(row),intent(in)::a
total=sum(a%x%year)
end function
end module
use m
implicit none
type(row)::a(2)
call fill(a(1),2)
call fill(a(2),3)
print *,total(a(1)),total(a(2))
end
""", '4 9\n')

    def test_real_logical_and_rank_three_components(self):
        self.check("""program test
implicit none
type :: row
real(kind=kind(0d0)),allocatable::x(:,:,:)
logical,allocatable::mask(:,:)
end type
type(row)::a(2)
allocate(a(1)%x(2,2,2),a(2)%x(1,1,1))
allocate(a(1)%mask(2,2),a(2)%mask(1,1))
a(1)%x=2d0
a(2)%x=9d0
a(1)%mask=.true.
a(2)%mask=.false.
a(1)%x(1,2,2)=3d0
print *,sum(a(1)%x),sum(a(2)%x),all(a(1)%mask),any(a(2)%mask)
print *,shape(a(1)%x)
end
""", '17 9 T F\n2 2 2\n')

    def test_array_dummy_does_not_destroy_caller_components(self):
        self.check("""module m
implicit none
type :: row
integer,allocatable::x(:)
end type
contains
subroutine fill(a)
type(row),intent(inout)::a(:)
allocate(a(1)%x(2))
a(1)%x=[3,4]
end subroutine
end module
use m
implicit none
type(row)::a(2)
call fill(a)
print *,a(1)%x,allocated(a(2)%x)
end
""", '3 4 F\n')

    def test_copy_to_scalar_and_scalar_out_dummy(self):
        self.check("""module m
implicit none
type :: row
integer,allocatable::x(:)
end type
contains
subroutine reset(a)
type(row),intent(out)::a
allocate(a%x(1))
a%x=9
end subroutine
end module
use m
implicit none
type(row)::a(2),b
allocate(a(1)%x(2))
a(1)%x=[1,2]
b=a(1)
call reset(a(1))
print *,a(1)%x,b%x
end
""", '9 1 2\n')

    def test_read_random_number_and_move_alloc(self):
        self.check("""program test
implicit none
type :: row
real,allocatable::x(:)
end type
type(row)::a(2)
allocate(a(1)%x(2))
read *,a(1)%x
print *,sum(a(1)%x)
call random_number(a(1)%x)
print *,all(a(1)%x>=0.0 .and. a(1)%x<1.0)
call move_alloc(a(1)%x,a(2)%x)
print *,allocated(a(1)%x),allocated(a(2)%x),size(a(2)%x)
end
""", '3 T F T 2\n', data='1 2\n')

    def test_rank_fifteen_descriptors(self):
        rank=15
        deferred=','.join([':']*rank)
        unit=','.join(['1']*rank)
        self.check(f"""program test
implicit none
type :: row
integer,allocatable::x({deferred})
end type
type(row)::a({unit})
allocate(a({unit})%x({unit}))
a({unit})%x=42
print *,size(a({unit})%x),sum(a({unit})%x)
end
""", '1 42\n')

    def test_container_copy_rejected_until_deep_outer_copy_is_supported(self):
        self.reject("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type :: container
type(row),allocatable::rows(:)
end type
type(container)::a,b
allocate(a%rows(2))
b=a
end
""", 'intrinsic assignment of containers')

    def test_allocatable_component_requires_scalar_parent(self):
        self.reject("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2)
print *,a%x
end
""", 'requires a scalar parent')

    def test_uninitialized_component_elements_remain_checked(self):
        self.check("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row)::a(2)
allocate(a(1)%x(2))
a(1)%x(1)=7
print *,a(1)%x(2)
end
""", error='uninitialized variable', reference=False)

    def test_repeated_outer_deallocation(self):
        self.check("""program test
implicit none
type :: row
integer,allocatable::x(:)
end type
type(row),allocatable::a(:)
integer::i
do i=1,100
 allocate(a(2))
 allocate(a(1)%x(2),a(2)%x(3))
 a(1)%x=1
 a(2)%x=2
 deallocate(a)
end do
print *,i
end
""", '101\n')

if __name__ == '__main__':
    unittest.main()

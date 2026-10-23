"""Derived record sections preserve element/component order in output.

Check the interpreter, native -O0/-O2 and gfortran where available.
"""
import unittest
import test_deallocate
from test_record_read import ROOT


class DerivedSectionOutput(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_derived_matrix_row_output.f90').read_text(encoding='utf-8'),
                   '01/02/2024 03/04/2025\n')

    def test_rows_columns_and_cartesian_vectors(self):
        self.check("""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair) :: a(2,3)
integer :: i,j
do j=1,3
do i=1,2
a(i,j)=pair(10*j+i,-(10*j+i))
end do
end do
print *,a(2,:)
print *,a(:,2)
print *,a([2,1],[3,1])
print *,a(2:1:-1,3:1:-2)
end
""", '12 -12 22 -22 32 -32\n21 -21 22 -22\n32 -32 31 -31 12 -12 11 -11\n32 -32 31 -31 12 -12 11 -11\n')

    def test_runtime_dummy_and_stride(self):
        self.check("""module m
implicit none
type :: pair
integer :: x,y
end type
contains
subroutine show(a,row,last,step)
type(pair),intent(in)::a(:,:)
integer,intent(in)::row,last,step
write(*,'(*(i0,1x))') 0,a(row,last:1:step),99
print *,a(2:1:step,:)
end subroutine
end module
use m
implicit none
type(pair),allocatable::a(:,:)
integer::n,j
read *,n
allocate(a(2,n))
do j=1,n
a(1,j)=pair(j,-j)
a(2,j)=pair(10+j,-10-j)
end do
call show(a,2,n,-1)
end
""", '0 13 -13 12 -12 11 -11 99\n11 -11 1 -1 12 -12 2 -2 13 -13 3 -3\n', data='3\n')

    def test_rank_one_nonunit_bounds_and_empty_sections(self):
        self.check("""program test
implicit none
type :: item
integer :: x
end type
type(item) :: a(-2:2)
integer::i,step
do i=-2,2
a(i)=item(i)
end do
step=-2
print *,a(2:-2:step)
print *,a([-1,2,-1])
print *,0,a(1:2:-1),9
print *,0,a(::step),9
end
""", '2 0 -2\n-1 2 -1\n0 9\n0 9\n')

    def test_runtime_vector_and_repeated_indices(self):
        self.check("""program test
implicit none
type :: item
integer :: x,y
end type
type(item) :: a(2,3)
integer,allocatable::v(:)
integer::n,i
read *,n
allocate(v(n))
v=[3,1,3]
do i=1,3
a(1,i)=item(i,-i)
a(2,i)=item(10+i,-10-i)
end do
print *,a([2,1],v)
end
""", '13 -13 3 -3 11 -11 1 -1 13 -13 3 -3\n',data='3\n')

    def test_nested_mixed_fields_and_fixed_array_component(self):
        self.check("""program test
implicit none
type :: leaf
integer :: id
real :: v(2)
end type
type :: row
type(leaf)::child
logical::flag
character(len=2)::tag
end type
type(row)::a(2,2)
integer::j
do j=1,2
a(1,j)=row(leaf(1,[1.5,2.5]),.true.,'aa')
a(2,j)=row(leaf(2,[3.5,4.5]),.false.,'bb')
end do
write(*,'(*(i0,1x,2(f3.1,1x),l1,1x,a,1x))') a(2:1:-1,2)
end
""", '2 3.5 4.5 F bb 1 1.5 2.5 T aa\n')

    def test_only_array_components(self):
        self.check("""program test
implicit none
type :: item
integer :: v(2)
end type
type(item) :: a(2,2)
integer::j
do j=1,2
a(1,j)=item([1,2])
a(2,j)=item([3,4])
end do
print *,a(:,2)
print *,a(2,2:1:-1)
end
""", '1 2 3 4\n3 4 3 4\n')

    def test_character_and_logical_array_components(self):
        self.check("""program test
implicit none
type :: item
character(len=2)::tags(2)
logical::flags(2)
end type
type(item)::a(1,2)
a(1,1)%tags=['aa','bb']
a(1,1)%flags=[.true.,.false.]
a(1,2)%tags=['cc','dd']
a(1,2)%flags=[.false.,.true.]
write(*,'(2a,2(1x,l1),1x,2a,2(1x,l1))') a(1,:)
end
""", 'aabb T F ccdd F T\n')

    def test_pointer_parent_and_internal_formatted_output(self):
        self.check("""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair),target::a(2,2)
type(pair),pointer::p(:,:)
character(len=64)::line
a(:,1)=pair(1,2)
a(:,2)=pair(3,4)
p=>a
write(line,'(4(i0,1x))') p(2,:)
print *,trim(line)
end
""", '1 2 3 4\n')

    def test_general_rank(self):
        for rank in (3,15):
            with self.subTest(rank=rank):
                shape=','.join(['2']+['1']*(rank-2)+['2'])
                first=','.join(['1']+['1']*(rank-2)+[':'])
                second=','.join(['2']+['1']*(rank-2)+[':'])
                section=','.join(['2:1:-1']+['1']*(rank-2)+[':'])
                self.check(f"""program test
implicit none
type :: item
integer :: x,y
end type
type(item) :: a({shape})
a({first})=item(1,-1)
a({second})=item(2,-2)
print *,a({section})
end
""", '2 -2 1 -1 2 -2 1 -1\n')

    def test_subscript_expressions_evaluated_once(self):
        self.check("""module m
implicit none
integer :: calls=0
type :: pair
integer :: x,y
end type
contains
integer function next(n) result(r)
integer,intent(in)::n
calls=calls+1
r=n
end function
end module
use m
implicit none
type(pair)::a(2,3)
integer::j
do j=1,3
a(:,j)=pair(j,-j)
end do
print *,a(next(2),next(3):next(1):next(-1))
print *,calls
end
""", '3 -3 2 -2 1 -1\n4\n')

    def test_unselected_elements_need_not_be_initialized(self):
        self.check("""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair) :: a(2,2)
a(1,1)=pair(7,8)
print *,a(1,1:1)
print *,0,a(2:1,:),9
end
""", '7 8\n0 9\n')

    def test_selected_elements_keep_initialization_and_bounds_checks(self):
        source="""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair)::a(2,2)
integer::i
a(1,1)=pair(7,8)
read *,i
print *,a(1,i:i)
end
"""
        self.check(source,error='uninitialized',data='2\n')
        self.check(source,error='subscript',data='3\n')


if __name__ == '__main__':
    unittest.main()

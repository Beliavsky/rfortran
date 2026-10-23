"""PACK of derived arrays in the interpreter and native O0/O2 builds."""
import unittest
import test_deallocate
from test_record_read import ROOT

HEADER="""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair),allocatable::a(:),b(:),v(:)
integer::i,n
allocate(a(4))
a=[pair(1,10),pair(2,20),pair(3,30),pair(4,40)]
"""


class DerivedPack(unittest.TestCase):
    check=test_deallocate.Deallocate.check
    reject=test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_pack_derived_array.f90').read_text(encoding='utf-8'), '1 3\n')

    def test_scalar_masks_empty_and_overlapping_assignment(self):
        self.check(HEADER+"""b=pack(a,.true.)
print *,size(b),b%x,b%y
b=pack(a,.false.)
print *,size(b)
a=pack(a,[.false.,.true.,.false.,.true.])
print *,size(a),a%x,a%y
b=pack(a,[.false.,.false.])
print *,size(b)
deallocate(a)
allocate(a(0))
b=pack(a,.true.)
print *,size(b)
end
""", '4 1 2 3 4 10 20 30 40\n0\n2 2 4 20 40\n0\n0\n')

    def test_keywords_and_vector_tail(self):
        self.check("""program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(3),v(4)
type(pair),allocatable::b(:)
a=[pair(1,10),pair(2,20),pair(3,30)]
v=[pair(5,50),pair(6,60),pair(7,70),pair(8,80)]
b=pack(vector=v,mask=[.true.,.false.,.true.],array=a)
print *,b%x,b%y
b=pack(a,.false.,v)
print *,b%x,b%y
end
""", '1 3 7 8 10 30 70 80\n5 6 7 8 50 60 70 80\n')

    def test_runtime_vector(self):
        self.check(HEADER+"""read *,n
allocate(v(n))
do i=1,n
v(i)=pair(10+i,100+i)
end do
b=pack(a,[.true.,.false.,.true.,.false.],v)
print *,size(b),b%x,b%y
end
""", '4 1 3 13 14 10 30 103 104\n',data='4\n')

    def test_nested_mixed_scalar_components(self):
        self.check("""program test
implicit none
type :: leaf
integer::id
end type
type :: record
type(leaf)::child
character(len=2)::tag
logical::flag
real::value
end type
type(record)::a(3)
type(record),allocatable::b(:)
logical::mask(3)
a=[record(leaf(1),'aa',.true.,1.5),record(leaf(2),'bb',.false.,2.5), &
   record(leaf(3),'cc',.true.,3.5)]
mask=a%flag
b=pack(a,mask)
print *,b
end
""", '1 aa T 1.5 3 cc T 3.5\n')

    def test_array_and_mask_evaluated_once(self):
        self.check("""module m
implicit none
integer::makes=0,masks=0
type :: pair
integer::x,y
end type
contains
function make(n) result(a)
integer,intent(in)::n
type(pair)::a(n)
integer::i
makes=makes+1
do i=1,n
a(i)=pair(i,-i)
end do
end function
impure elemental logical function keep(a)
type(pair),intent(in)::a
masks=masks+1
keep=mod(a%x,2)==1
end function
end module
use m
implicit none
type(pair),allocatable::a(:),b(:)
a=make(3)
b=pack(a,keep(a))
print *,b%x,b%y,makes,masks
b=pack(make(3),[.false.,.true.,.false.])
print *,b%x,b%y,makes,masks
end
""", '1 3 -1 -3 1 3\n2 -2 2 3\n')

    def test_rank_two_column_major_order(self):
        self.check("""program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(2,2)
type(pair),allocatable::b(:)
integer::i,j
do j=1,2
do i=1,2
a(i,j)=pair(10*j+i,-(10*j+i))
end do
end do
b=pack(a,reshape([.false.,.true.,.true.,.true.],[2,2]))
print *,b%x,b%y
end
""", '12 21 22 -12 -21 -22\n')

    def test_sections_nested_pack_and_constructors(self):
        self.check(HEADER+"""b=pack(a(4:1:-1),[.true.,.false.,.true.,.false.])
print *,b%x,b%y
b=pack(pack(a,a%x>1),[.true.,.false.,.true.])
print *,b%x,b%y
b=pack([pair(5,50),pair(6,60)],[.false.,.true.])
print *,b%x,b%y
end
""", '4 2 40 20\n2 4 20 40\n6 60\n')

    def test_only_selected_record_values_are_read(self):
        self.check("""program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(4)
type(pair),allocatable::b(:)
a(2)=pair(7,70)
a(4)=pair(9,90)
b=pack(a,[.false.,.true.,.false.,.true.])
print *,b%x,b%y
b=pack(a,.false.)
print *,size(b)
end
""", '7 9 70 90\n0\n')

    def test_invalid_vector_type(self):
        self.reject("""program test
implicit none
type :: first
integer::x
end type
type :: second
integer::x
end type
type(first)::a(1),b(1)
type(second)::v(1)
a=first(1)
v=second(2)
b=pack(a,.true.,v)
end
""", 'PACK ARRAY and VECTOR must have the same derived type')

    def test_fixed_array_components_and_vector(self):
        self.check("""program test
implicit none
type :: record
integer::id
real::v(2)
character(len=2)::tags(2)
end type
type(record)::a(2,2),v(3)
type(record),allocatable::b(:)
integer::i,j
do j=1,2
do i=1,2
a(i,j)%id=10*j+i
a(i,j)%v=[real(i),real(j)]
a(i,j)%tags=['aa','bb']
end do
end do
do i=1,3
v(i)%id=100+i
v(i)%v=[-1.0,-2.0]
v(i)%tags=['cc','dd']
end do
! Existing CHARACTER component-array assignment requires allocated storage.
allocate(b(3))
b=pack(a,reshape([.false.,.true.,.true.,.false.],[2,2]),v)
print *,b(1)%id,b(1)%v,b(1)%tags
print *,b(2)%id,b(2)%v,b(2)%tags
print *,b(3)%id,b(3)%v,b(3)%tags
end
""", '12 2 1 aabb\n21 1 2 aabb\n103 -1 -2 ccdd\n')

    def test_rank_fifteen(self):
        shape=','.join(['2']+['1']*13+['2'])
        first=','.join(['1']+['1']*13+[':'])
        second=','.join(['2']+['1']*13+[':'])
        self.check(f"""program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a({shape})
type(pair),allocatable::b(:)
a({first})=pair(1,10)
a({second})=pair(2,20)
b=pack(a,reshape([.false.,.true.,.true.,.false.],[{shape}]))
print *,b%x,b%y
end
""", '2 1 20 10\n')

    def test_primitive_elemental_mask_assignment(self):
        self.check("""module m
implicit none
contains
elemental logical function positive(x)
integer,intent(in)::x
positive=x>0
end function
end module
use m
implicit none
integer,allocatable::a(:)
logical,allocatable::mask(:)
allocate(a(3),mask(3))
a=[1,-1,2]
mask=positive(a)
print *,mask
end
""", 'T F T\n')

    def test_scalar_mask_function_evaluated_once(self):
        self.check("""module m
implicit none
integer::calls=0
type :: pair
integer::x,y
end type
contains
logical function all_items()
calls=calls+1
all_items=.true.
end function
end module
use m
implicit none
type(pair)::a(3)
type(pair),allocatable::b(:)
a=[pair(1,10),pair(2,20),pair(3,30)]
b=pack(a,all_items())
print *,b%x,calls
end
""", '1 2 3 1\n')

    def test_runtime_vector_too_short(self):
        self.check(HEADER+"""read *,n
allocate(v(n))
v=pair(9,90)
b=pack(a,.true.,v)
print *,size(b)
end
""",error='PACK VECTOR has too few elements for MASK',data='2\n')

    def test_reject_nonlogical_mask(self):
        self.reject(HEADER+"""b=pack(a,1)
end
""", 'LOGICAL')

    def test_runtime_mask_shape_must_conform(self):
        self.check(HEADER.replace('integer::i,n','integer::i,n\nlogical,allocatable::mask(:)')+"""
read *,n
allocate(mask(n))
mask=.true.
b=pack(a,mask)
print *,size(b)
end
""",error='PACK MASK shape must conform to ARRAY',data='5\n')

    def test_user_defined_pack_is_preserved(self):
        self.check("""module m
implicit none
type :: pair
integer::x,y
end type
contains
function pack(a,n) result(b)
type(pair),intent(in)::a(:)
integer,intent(in)::n
type(pair)::b(size(a))
b=a
b%x=b%x+100*n
end function
end module
use m
implicit none
type(pair)::a(2)
type(pair),allocatable::b(:)
a=[pair(1,10),pair(2,20)]
b=pack(a,1)
print *,b%x,b%y
end
""", '101 102 10 20\n')

    def test_reject_owned_allocatable_components(self):
        self.reject("""program test
implicit none
type :: record
integer,allocatable::v(:)
end type
type(record)::a(1)
type(record),allocatable::b(:)
allocate(a(1)%v(1))
a(1)%v=1
print *,pack(a,.true.)
end
""", 'PACK of records with owned allocatable components is not yet supported')


if __name__=='__main__':
    unittest.main()

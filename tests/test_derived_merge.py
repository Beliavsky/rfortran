"""Scalar derived MERGE, also within ordinary/ELEMENTAL procedures."""
import unittest
import test_deallocate
from test_record_read import ROOT


class DerivedMerge(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_derived_merge.f90').read_text(encoding='utf-8'),
                   '3 30 0\n')

    def test_keywords_nested_merge_and_self_assignment(self):
        self.check("""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair)::a,b,c
a=pair(1,2)
b=pair(7,8)
c=merge(mask=.false.,fsource=b,tsource=a)
print *,c
a=merge(b,a,a%x<0)
print *,a
a=merge(pair(a%y,a%x),b,.true.)
print *,a
c=merge(merge(a,b,.false.),pair(9,10),.true.)
print *,c
end
""", '7 8\n1 2\n2 1\n7 8\n')

    def test_nested_and_fixed_array_components(self):
        self.check("""program test
implicit none
type :: leaf
integer :: n
end type
type :: record
type(leaf)::child
character(len=3)::tag
logical::flag
real::v(2)
end type
type(record)::a,b,c
a=record(leaf(1),'abc',.true.,[1.5,2.5])
b=record(leaf(2),'def',.false.,[3.5,4.5])
c=merge(a,b,.false.)
print *,c
c=merge(a,c,.true.)
print *,c
end
""", '2 def F 3.5 4.5\n1 abc T 1.5 2.5\n')

    def test_allocatable_copy_and_pointer_association(self):
        # gfortran 17 (20260510) aliases the selected allocatable component in
        # this MERGE assignment: changing b%v also changes c%v. Check the
        # required independent allocation in our interpreter and both backends.
        self.check("""program test
implicit none
type :: record
integer :: id
real,allocatable::v(:)
integer,pointer::p
end type
type(record)::a,b,c
integer,target::i,j
i=10
j=20
a%id=1
a%v=[1.0,2.0]
a%p=>i
b%id=2
b%v=[3.0,4.0,5.0]
b%p=>j
c=merge(a,b,.false.)
print *,c%id,c%v,associated(c%p,j),c%p
b%v=99.0
print *,c%v
c=merge(a,b,.true.)
print *,c%id,c%v,associated(c%p,i),c%p
end
""", '2 3 4 5 T 20\n3 4 5\n1 1 2 T 10\n',reference=False)

    def test_procedure_arguments_and_single_mask_evaluation(self):
        self.check("""module m
implicit none
integer :: calls=0
type :: pair
integer :: x,y
end type
contains
logical function mask()
calls=calls+1
mask=.false.
end function
function make(n) result(r)
integer,intent(in)::n
type(pair)::r
r=pair(n,-n)
end function
integer function total(p) result(n)
type(pair),intent(in)::p
n=p%x+p%y
end function
end module
use m
implicit none
type(pair)::a,b,c
a=pair(1,2)
b=pair(7,8)
c=merge(a,b,mask())
print *,c,calls
print *,total(merge(a,b,.true.))
c=merge(make(10),make(20),.false.)
print *,c
end
""", '7 8 1\n3\n20 -20\n')

    def test_elemental_record_arrays(self):
        self.check("""module m
implicit none
type :: pair
integer :: x,y
end type
contains
elemental function choose(a,b,mask) result(r)
type(pair),intent(in)::a,b
logical,intent(in)::mask
type(pair)::r
r=merge(a,b,mask)
end function
end module
use m
implicit none
type(pair)::a(3),b(3),r(3)
a=[pair(1,-1),pair(2,-2),pair(3,-3)]
b=[pair(4,-4),pair(5,-5),pair(6,-6)]
r=choose(a,b,[.true.,.false.,.true.])
print *,r%x,r%y
end
""", '1 5 3 -1 -5 -3\n')

    def test_scalar_array_elements_broadcast_and_constructor(self):
        self.check("""program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(2),b(2),r
a=[pair(1,2),pair(7,8)]
r=merge(a(1),a(2),.false.)
print *,r
b=merge(pair(3,4),pair(5,6),.false.)
print *,b
b=[merge(a(1),a(2),.true.),merge(a(1),a(2),.false.)]
print *,b
end
""", '7 8\n5 6 5 6\n1 2 7 8\n')

    def test_user_defined_merge_is_preserved(self):
        self.check("""module m
implicit none
type :: pair
integer :: x,y
end type
contains
function merge(a,b,mask) result(r)
type(pair),intent(in)::a,b
logical,intent(in)::mask
type(pair)::r
r=pair(42,43)
end function
end module
use m
implicit none
type(pair)::r
r=merge(pair(1,2),pair(3,4),.true.)
print *,r
end
""", '42 43\n')

    def test_reject_nonlogical_mask(self):
        self.reject("""program test
implicit none
type :: pair
integer :: x,y
end type
type(pair)::a,b,c
a=pair(1,2)
b=pair(3,4)
c=merge(a,b,1)
end
""", 'LOGICAL')

    def test_reject_different_source_types_even_with_extension(self):
        self.reject("""program test
implicit none
type :: base
integer :: x
end type
type,extends(base) :: child
integer :: y
end type
type(base)::a,r
type(child)::b
a=base(1)
b%x=2
b%y=3
r=merge(a,b,.true.)
end
""", 'MERGE sources must have the same derived type')


if __name__ == '__main__':
    unittest.main()

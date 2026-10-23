"""Array-valued items in derived constructors, interpreter and native O0/O2."""
import unittest
import test_deallocate
from test_record_read import ROOT

HEADER = '''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(3),b(2)
type(pair),allocatable::c(:)
a=[pair(1,10),pair(2,20),pair(3,30)]
b=[pair(4,40),pair(5,50)]
'''


class DerivedConstructors(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_xacv_reproducer(self):
        self.check((ROOT/'tests/programs/xacv_concat_derived_arrays.f90').read_text(), '2 2024 2025\n')

    def test_assignment_uses_element_count(self):
        self.check(HEADER+'''c=[a,b]
print *,size(c),c%x,c%y
end
''', '5 1 2 3 4 5 10 20 30 40 50\n')

    def test_mixed_nested_and_typed_items(self):
        self.check(HEADER+'''c=[pair(0,0),a,[b,pair(6,60)]]
print *,size(c),c%x,c%y
c=[pair :: b,a(1),a(2:3)]
print *,size(c),c%x
end
''', '7 0 1 2 3 4 5 6 0 10 20 30 40 50 60\n5 4 5 1 2 3\n')

    def test_overlapping_assignment(self):
        self.check(HEADER+'''c=a
c=[c,c(3:1:-1)]
print *,size(c),c%x,c%y
c=[c(6),c(2:4)]
print *,size(c),c%x,c%y
end
''', '6 1 2 3 3 2 1 10 20 30 30 20 10\n4 1 2 3 3 10 20 30 30\n')

    def test_sections_and_vectors(self):
        self.check(HEADER+'''c=[a(3:1:-2),b([2,1]),a(1:0)]
print *,size(c),c%x,c%y
end
''', '4 3 1 5 4 30 10 50 40\n')

    def test_multidimensional_column_major_order(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a(2,2)
type(pair),allocatable::c(:)
integer::i,j
do j=1,2
do i=1,2
a(i,j)=pair(i+10*j,-i-10*j)
end do
end do
c=[a,a(2,:)]
print *,size(c),c%x,c%y
end
''', '6 11 12 21 22 12 22 -11 -12 -21 -22 -12 -22\n')

    def test_runtime_arrays_and_empty_items(self):
        program='''program test
implicit none
type :: pair
integer::x,y
end type
type(pair),allocatable::a(:),b(:),c(:)
integer::i,n
read *,n
allocate(a(n),b(0))
do i=1,n
a(i)=pair(i,10*i)
end do
c=[b,a,b]
print *,size(c)
if(n>0) print *,c%x,c%y
c=[pair :: b,b]
print *,size(c)
c=[pair ::]
print *,size(c)
end
'''
        self.check(program, '3\n1 2 3 10 20 30\n0\n0\n', data='3\n')
        self.check(program, '0\n0\n0\n', data='0\n')

    def test_pack_constructor_counts_records(self):
        self.check(HEADER+'''c=pack([a,b],[.true.,.false.,.true.,.false.,.true.])
print *,size(c),c%x,c%y
c=pack([pair ::],.true.)
print *,size(c)
end
''', '3 1 3 5 10 30 50\n0\n')

    def test_constructor_in_output_and_inquiry(self):
        self.check(HEADER+'''print *,size([a,b])
print *,[a(1),b]
end
''', '5\n1 10 4 40 5 50\n')

    def test_runtime_multidimensional_source(self):
        self.check('''program test
implicit none
type :: pair
integer::x,y
end type
type(pair),allocatable::a(:,:),c(:)
integer::i,j,n
read *,n
allocate(a(2,n))
do j=1,n
do i=1,2
a(i,j)=pair(i+10*j,-i-10*j)
end do
end do
c=[a,a(2,:)]
print *,size(c),c%x,c%y
end
''', '6 11 12 21 22 12 22 -11 -12 -21 -22 -12 -22\n', data='2\n')

    def test_fixed_array_components(self):
        self.check('''program test
implicit none
type :: item
integer::coords(2)
real::values(2,2)
end type
type(item)::a(2)
type(item),allocatable::c(:)
a(1)=item([1,2],reshape([1.,2.,3.,4.],[2,2]))
a(2)=item([3,4],reshape([5.,6.,7.,8.],[2,2]))
c=[a,a(1)]
print *,c
end
''', '1 2 1 2 3 4 3 4 5 6 7 8 1 2 1 2 3 4\n')

    def test_rank_fifteen_source(self):
        dims=','.join(['1']*14+['2'])
        first=','.join(['1']*15)
        last=','.join(['1']*14+['2'])
        self.check(f'''program test
implicit none
type :: pair
integer::x,y
end type
type(pair)::a({dims})
type(pair),allocatable::c(:)
a({first})=pair(1,10)
a({last})=pair(2,20)
c=[a]
print *,size(c),c%x,c%y
end
''', '2 1 2 10 20\n')

    def test_mixed_scalar_components(self):
        self.check('''program test
implicit none
type :: inner
integer::id
real::value
end type
type :: record
type(inner)::p
logical::flag
character(len=3)::name
end type
type(record)::a(2),b
type(record),allocatable::c(:)
a=[record(inner(1,1.5),.true.,'abc'),record(inner(2,2.5),.false.,'def')]
b=record(inner(3,3.5),.true.,'ghi')
c=[a,b]
print *,size(c),c
end
''', '3 1 1.5 T abc 2 2.5 F def 3 3.5 T ghi\n')

    def test_function_items_evaluated_once(self):
        self.check('''module m
implicit none
type :: pair
integer::x,y
end type
integer::calls=0
contains
function one() result(p)
type(pair)::p
calls=calls+1
p=pair(calls,10*calls)
end function
function many() result(p)
type(pair),allocatable::p(:)
calls=calls+1
allocate(p(2))
p=[pair(2,20),pair(3,30)]
end function
end module
use m
implicit none
type(pair),allocatable::c(:)
c=[one(),many()]
print *,size(c),c%x,c%y,calls
end
''', '3 1 2 3 10 20 30 2\n')


if __name__ == '__main__':
    unittest.main()

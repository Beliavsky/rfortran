"""Implied-DO bounds use active heap-array extents, not placeholder storage."""
from pathlib import Path
import unittest
import test_deallocate as helpers

class PackRuntimeConstructor(unittest.TestCase):
    check = helpers.Deallocate.check

    def test_dropna_repro(self):
        self.check((Path(__file__).parent / "programs/xpack_runtime_constructor.f90").read_text(),
                   "3 2 4 5\n0\n0\n")

    def test_reallocation_and_nondefault_bounds(self):
        self.check("""implicit none
logical,allocatable::mask(:)
integer,allocatable::v(:)
integer::n,i
do n=0,6
allocate(mask(-2:n-3))
mask=.true.
v=pack([(i,i=1,size(mask))],mask)
print *,size(v),sum(v)
mask=.false.
v=pack([(i,i=size(mask),1,-1)],mask)
print *,size(v)
deallocate(mask)
end do
end
""", "0 0\n0\n1 1\n0\n2 3\n0\n3 6\n0\n4 10\n0\n5 15\n0\n6 21\n0\n")

    def test_constructor_without_pack_and_multidimensional_size(self):
        self.check("""implicit none
real,allocatable::a(:,:)
integer,allocatable::v(:)
integer::i,j,n
n=3
allocate(a(2,n))
a=1
print *,sum([(i,i=1,size(a))])
v=[((i+10*j,i=1,size(a,1)),j=1,size(a,2))]
print *,v
end
""", "21\n11 12 21 22 31 32\n")

    def test_real_logical_and_vector_argument(self):
        self.check("""implicit none
logical,allocatable::mask(:),b(:)
double precision,allocatable::v(:)
integer::n,i
n=4
allocate(mask(n))
mask=[.true.,.false.,.true.,.false.]
v=pack([(dble(i)/2,i=1,size(mask))],mask)
b=pack([(mod(i,2)==1,i=1,size(mask))],mask)
print *,v,b
v=pack([(dble(i),i=1,size(mask))],mask,[9d0,8d0,7d0,6d0])
print *,v
end
""", "0.5 1.5 T T\n1.0 3.0 7.0 6.0\n")

    def test_local_array_shadows_heap_array(self):
        self.check("""implicit none
integer,allocatable::a(:)
integer::i
a=[1,2]
block
integer::a(3),i
a=1
print *,sum([(i,i=1,size(a))])
end block
print *,sum([(i,i=1,size(a))])
end
""", "6\n3\n")

    def test_mismatched_mask_is_still_rejected(self):
        self.check("""implicit none
logical,allocatable::mask(:)
integer,allocatable::v(:)
integer::n,i
read *,n
allocate(mask(n))
mask=.true.
v=pack([(i,i=1,size(mask)+1)],mask)
print *,v
end
""",error="runtime-sized array shapes do not conform in dimension 1",data="3\n",reference=False)

if __name__ == '__main__':
    unittest.main()

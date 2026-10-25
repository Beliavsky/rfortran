"""SPREAD with runtime source extents and copy counts."""
import unittest
import test_deallocate as helpers

class RuntimeSpread(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_array_axes_and_direct_argument(self):
        self.check("""implicit none
integer::n
real,allocatable::a(:)
n=3
allocate(a(-1:1))
a=[1.25,2.5,3.75]
call show(spread(a,dim=1,ncopies=n))
call show(spread(a(1:-1:-2),dim=2,ncopies=n))
contains
subroutine show(x)
real,intent(in)::x(:,:)
print *,shape(x),x
end subroutine
end
""", "3 3 1.25 1.25 1.25 2.5 2.5 2.5 3.75 3.75 3.75\n2 3 3.75 1.25 3.75 1.25 3.75 1.25\n")

    def test_scalar_and_zero_negative_copies(self):
        self.check("""implicit none
integer::n
real,allocatable::a(:)
n=3
a=spread(1.25,1,n)
print *,size(a),a
n=0
a=spread(2.5,1,n)
print *,size(a)
n=-2
a=spread(2.5,1,n)
print *,size(a)
a=spread(7.0,1,2)
print *,size(a),a
end
""", "3 1.25 1.25 1.25\n0\n0\n2 7.0 7.0\n")

    def test_rank_three_and_empty_source(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:),b(:,:,:)
n=2
allocate(a(n,3))
a=reshape([1,2,3,4,5,6],[2,3])
b=spread(a,2,n)
print *,shape(b),b
b=spread(a,3,2)
print *,shape(b),b
deallocate(a)
allocate(a(0,n))
b=spread(a,1,n)
print *,shape(b),size(b)
end
""", "2 2 3 1 2 1 2 3 4 3 4 5 6 5 6\n2 3 2 1 2 3 4 5 6 1 2 3 4 5 6\n2 0 2 0\n")

    def test_logical(self):
        self.check("""implicit none
integer::n
logical,allocatable::a(:),b(:,:)
n=2
allocate(a(n))
a=[.true.,.false.]
b=spread(a,1,n)
print *,shape(b),b
end
""", "2 2 T T F F\n")

if __name__=='__main__':unittest.main()

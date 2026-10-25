"""Reductions whose DIM value is selected at execution time."""
import unittest
from pathlib import Path
import test_deallocate
import test_runtime_dim_reductions as reference

class VariableDimReductions(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject
    compare = reference.RuntimeDimReductions.compare

    def test_fixture(self):
        self.compare((Path(__file__).parent / "programs/xvariable_dim_reductions.f90").read_text())

    def test_numeric_logical_and_locations(self):
        self.compare("""implicit none
integer,allocatable::a(:,:,:),v(:,:)
logical,allocatable::m(:,:,:),b(:,:)
integer::n,d
n=2
allocate(a(-1:0,3:5,2:n+1),m(n,3,n))
a=reshape([1,2,3,4,5,6,7,8,9,10,11,12],[2,3,2])
m=mod(a,2)==0
do d=1,3
v=sum(a,dim=d)
print *,shape(v),v
print *,product(a,dim=d),minval(a,dim=d),maxval(a,dim=d)
print *,sum(a,mask=m,dim=d),product(a,dim=d,mask=m)
print *,iany(a,dim=d),iall(a,dim=d),iparity(a,dim=d)
b=any(m,dim=d)
print *,shape(b),b,all(m,dim=d),parity(m,dim=d)
print *,count(m,dim=d),count(m,dim=d,kind=8)
print *,minloc(a,dim=d),maxloc(a,dim=d),findloc(a,6,dim=d)
end do
end
""")

    def test_fixed_arrays(self):
        self.compare("""implicit none
real::a(2,3)
integer::d
a=reshape([1.,2.,3.,4.,5.,6.],[2,3])
do d=1,2
print *,sum(a,dim=d),product(a,dim=d),minval(a,dim=d),maxval(a,dim=d)
print *,any(a>3.,dim=d),all(a>3.,dim=d),count(a>3.,dim=d)
print *,minloc(a,dim=d),maxloc(a,dim=d),findloc(a,3.,dim=d)
print "(6f16.8)",norm2(a,dim=d)
end do
end
""")

    def test_empty_axes(self):
        self.compare("""implicit none
integer,allocatable::a(:,:)
logical,allocatable::m(:,:)
integer::n,d
n=0
allocate(a(n,3),m(n,3))
do d=1,2
print *,sum(a,dim=d),product(a,dim=d),minval(a,dim=d),maxval(a,dim=d)
print *,any(m,dim=d),all(m,dim=d),count(m,dim=d)
print *,size(sum(a,dim=d)),minloc(a,dim=d)
end do
end
""")

    def test_dim_evaluated_once_and_rank_one(self):
        # gfortran elides DIM evaluation for rank-one reductions. Check
        # rfort's single-evaluation behavior directly instead.
        self.check("""implicit none
integer::calls,n,where
real::total
real,allocatable::a(:)
calls=0
n=3
allocate(a(n))
a=[1.,2.,3.]
total=sum(a,dim=axis())
print *,total,calls
where=minloc(a,dim=axis())
print *,where,calls
contains
integer function axis()
calls=calls+1
axis=1
end function
end
""", "6.0 1\n1 2\n", reference=False)

    def test_rank_fifteen(self):
        self.compare("""implicit none
integer,allocatable::a(:,:,:,:,:,:,:,:,:,:,:,:,:,:,:)
integer::n,d
n=2
allocate(a(n,1,1,1,1,1,1,1,1,1,1,1,1,1,3))
a=2
do d=1,15
print *,sum(a,dim=d),size(sum(a,dim=d))
end do
end
""")

    def test_character_findloc(self):
        self.compare("""implicit none
character(len=3),allocatable::a(:,:)
integer::n,d
n=2
allocate(a(n,3))
a="yes"
a(1,2)="no"
do d=1,2
print *,findloc(a,"yes",dim=d),findloc(a,"yes",dim=d,back=.true.,kind=8)
end do
end
""")

    def test_section_and_integer_eight_dim(self):
        self.compare("""implicit none
double precision,allocatable::a(:,:)
integer(kind=8)::d
integer::n
n=3
allocate(a(n,4))
a=reshape([1d0,2d0,3d0,4d0,5d0,6d0,7d0,8d0,9d0,10d0,11d0,12d0],[3,4])
do d=1,2
print *,sum(a(3:1:-1,1:4:2),dim=d,mask=a(3:1:-1,1:4:2)>3d0)
print "(6f16.8)",norm2(a,dim=d)
end do
end
""")

    def test_reject_real_dim(self):
        self.reject("""implicit none
integer::a(2,3)
real::d
read *,d
a=1
print *,sum(a,dim=d)
end
""", "DIM must be a scalar INTEGER")

    def test_invalid_dim(self):
        for dim in [0,3]:
            self.check("""implicit none
integer::a(2,3),d
a=1
read *,d
print *,sum(a,dim=d)
end
""", error="DIM must be between 1 and 2", data=f"{dim}\n", reference=False)

if __name__ == "__main__":
    unittest.main()

"""UNPACK with runtime shapes and checked VECTOR consumption."""
import unittest
import test_deallocate as helpers

class RuntimeUnpack(unittest.TestCase):
    check=helpers.Deallocate.check

    def test_matrix_mask_and_direct_argument(self):
        self.check("""implicit none
integer::n
real,allocatable::v(:),f(:,:)
logical,allocatable::m(:,:)
n=2
allocate(v(4),m(-1:0,3:5),f(n,3))
v=[1.25,2.5,3.75,99.0]
m=reshape([.true.,.false.,.false.,.true.,.true.,.false.],[2,3])
f=9.0
call show(unpack(v,m,f))
f=unpack(v,m,-1.0)
print *,f
contains
subroutine show(x)
real,intent(in)::x(:,:)
print *,shape(x),x
end subroutine
end
""", "2 3 1.25 9.0 9.0 2.5 3.75 9.0\n1.25 -1.0 -1.0 2.5 3.75 -1.0\n")

    def test_sections_and_overlap(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:),f(:)
logical,allocatable::m(:)
n=4
allocate(a(-1:2),f(n),m(n))
a=[1,2,3,4]
f=[10,20,30,40]
m=[.true.,.false.,.true.,.false.]
a=unpack(a(2:-1:-1),m,f(4:1:-1))
print *,a
end
""", "4 30 3 10\n")

    def test_logical_and_zero_size(self):
        self.check("""implicit none
integer::n
logical,allocatable::v(:),m(:,:),r(:,:)
n=2
allocate(v(n),m(n,2))
v=[.false.,.true.]
m=reshape([.true.,.false.,.true.,.false.],[2,2])
r=unpack(v,m,.false.)
print *,r
deallocate(v,m)
allocate(v(0),m(0,n))
r=unpack(v,m,.true.)
print *,shape(r),size(r)
deallocate(m)
allocate(m(n,n))
m=.false.
r=unpack(v,m,.true.)
print *,all(r)
end
""", "F F T F\n0 2 0\nT\n")

    def test_fixed_mask_runtime_vector(self):
        self.check("""implicit none
integer::n
integer,allocatable::v(:),a(:)
logical::m(3)
n=2
allocate(v(n))
v=[7,8]
m=[.true.,.false.,.true.]
a=unpack(v,m,9)
print *,a
end
""", "7 9 8\n")

    def test_short_vector_and_nonconforming_field(self):
        self.check("""integer::n
integer,allocatable::v(:)
logical,allocatable::m(:)
n=2
allocate(v(1),m(n))
v=1
m=.true.
print *,unpack(v,m,0)
end
""", error="UNPACK VECTOR has too few elements")
        self.check("""integer::n
integer,allocatable::v(:),f(:,:)
logical,allocatable::m(:,:)
n=2
allocate(v(4),m(n,2),f(2,3))
v=1
f=0
m=.false.
print *,unpack(v,m,f)
end
""", error="UNPACK FIELD shape must match MASK")

if __name__=='__main__':unittest.main()

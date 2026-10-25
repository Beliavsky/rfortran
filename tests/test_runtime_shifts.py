"""Runtime CSHIFT/EOSHIFT against interpreter, native O0/O2 and gfortran."""
import unittest
import test_deallocate as helpers

class RuntimeShifts(unittest.TestCase):
    check=helpers.Deallocate.check
    reject=helpers.Deallocate.reject

    def test_rank_two_controls_and_direct_arguments(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:),sh(:),bound(:)
n=2
allocate(a(-1:0,4:6),sh(3),bound(2))
a=reshape([1,2,3,4,5,6],[2,3])
sh=[1,-1,0]
bound=[9,8]
call show(cshift(a,sh,dim=1))
sh=[1,-1]
call show(eoshift(a,sh,bound,dim=2))
call show(cshift(a(0:-1:-1,6:4:-2),1,dim=2))
a=cshift(a,1,dim=2)
print *,a
contains
subroutine show(x)
integer,intent(in)::x(:,:)
print *,shape(x),x
end subroutine
end
""", "2 3 2 1 4 3 5 6\n2 3 3 8 5 2 9 4\n2 2 2 1 6 5\n3 4 5 6 1 2\n")

    def test_extreme_shifts_and_empty(self):
        self.check("""implicit none
integer::n
integer(8)::big,small
integer,allocatable::a(:),b(:)
n=3
allocate(a(n))
a=[1,2,3]
big=huge(big)
small=-big-1
b=cshift(a,big)
print *,b
b=cshift(a,small)
print *,b
b=eoshift(a,big,9)
print *,b
b=eoshift(a,small)
print *,b
deallocate(a)
allocate(a(0))
b=cshift(a,big)
print *,size(b)
b=eoshift(a,small)
print *,size(b)
end
""", "2 3 1\n2 3 1\n9 9 9\n0 0 0\n0\n0\n", reference=False)  # Installed gfortran crashes on this extreme-shift case.

    def test_real_logical_and_assumed_shape(self):
        self.check("""implicit none
integer::n
real,allocatable::a(:,:)
logical,allocatable::l(:),m(:)
n=2
allocate(a(n,3),l(n))
a=reshape([1.25,2.5,3.75,4.0,5.5,6.0],[2,3])
call work(a)
l=[.true.,.false.]
m=cshift(l,1)
print *,m
m=eoshift(l,-1)
print *,m
contains
subroutine work(x)
real,intent(in)::x(:,:)
real,allocatable::y(:,:)
y=eoshift(x,1,dim=2)
print *,shape(y),y
end subroutine
end
""", "2 3 3.75 4.0 5.5 6.0 0.0 0.0\nF T\nF T\n")

    def test_rank_three(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:,:),b(:,:,:),sh(:,:),bd(:,:)
n=2
allocate(a(n,2,2),sh(n,2),bd(n,2))
a=reshape([1,2,3,4,5,6,7,8],[2,2,2])
sh=1
bd=9
b=cshift(a,sh,dim=2)
print *,shape(b),b
b=eoshift(a,sh,bd,dim=2)
print *,b
end
""", "2 2 2 3 4 1 2 7 8 5 6\n3 4 9 9 7 8 9 9\n")

    def test_fixed_source_runtime_controls(self):
        self.check("""implicit none
integer::n,a(2,3)
integer,allocatable::sh(:),b(:,:)
n=3
allocate(sh(n))
sh=[1,0,-1]
a=reshape([1,2,3,4,5,6],[2,3])
b=cshift(a,sh,dim=1)
print *,b
b=eoshift(a,sh,dim=1)
print *,b
end
""", "2 1 3 4 6 5\n2 0 3 4 0 5\n")

    def test_empty_shifted_axis(self):
        # Installed gfortran rejects the valid (0,3) ARRAY / (3) SHIFT shapes.
        self.check("""integer::n
integer,allocatable::a(:,:),sh(:),b(:,:)
n=3
allocate(a(0,n),sh(n))
sh=[1,0,-1]
b=cshift(a,sh,dim=1)
print *,shape(b),size(b)
b=eoshift(a,sh,dim=1)
print *,shape(b),size(b)
end
""", "0 3 0\n0 3 0\n", reference=False)

    def test_runtime_shape_errors(self):
        for intrinsic,control in [('cshift','SHIFT'),('eoshift','BOUNDARY')]:
            args='a,b,dim=1' if intrinsic=='cshift' else 'a,1,b,dim=1'
            self.check('integer::n\ninteger,allocatable::a(:,:),b(:)\nn=2\nallocate(a(n,3),b(n))\na=1\nb=1\nprint *,'+intrinsic+'('+args+')\nend\n', error=control+' shape must match ARRAY with DIM removed')

if __name__=='__main__':unittest.main()

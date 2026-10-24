"""Array transformation results assigned to runtime-sized sections."""
import unittest
import test_deallocate
from test_record_read import ROOT

class SectionTransformations(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_derived_allocatable_reshape(self):
        self.check((ROOT / 'tests/programs/xmix_covariance_section.f90').read_text(),
                   '0.64 0.3 0.3 1.0')

    def test_reshape_order_and_pad(self):
        self.check("""implicit none
integer,allocatable :: a(:,:,:)
integer :: n
n=2
allocate(a(n,n,2))
a(:,:,1)=reshape([1,2],[2,2],pad=[3,4],order=[2,1])
print *,a(:,:,1)
a(:,:,2)=reshape([9,8,7,6],[2,2])
print *,a(:,:,2)
end
""", '1 3 2 4 9 8 7 6')

    def test_matmul_result_sections(self):
        self.check("""implicit none
real(8),allocatable :: a(:,:),b(:,:),c(:,:)
integer :: n
real(8) :: v(2)
n=2
v=[1d0,2d0]
allocate(a(n,n),b(n,n),c(n,n))
a=reshape([1d0,2d0,3d0,4d0],[2,2])
b=reshape([5d0,6d0,7d0,8d0],[2,2])
c(:,:)=matmul(a,b)
print *,c
c(1,:)=matmul(a,v)+[10d0,20d0]
c(2,:)=matmul(v,b)
print *,c
end
""", '23 34 31 46 17 17 30 23')

if __name__ == '__main__':
    unittest.main()

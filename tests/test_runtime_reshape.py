"""RESHAPE with runtime extents, checked sizes and column-major ordering."""
import unittest
import test_deallocate as helpers
from test_record_read import ROOT

class RuntimeReshape(unittest.TestCase):
    check=helpers.Deallocate.check
    reject=helpers.Deallocate.reject

    def test_function_result(self):
        self.check((ROOT/'tests/programs/xreshape_runtime_shape.f90').read_text(), '3 2\n1.25 2.5 3.75 4.25 5.5 6.75\n')

    def test_order_pad_and_zero_extent(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:)
n=2
a=reshape([1,2],[n,3],pad=[3,4],order=[2,1])
print *,shape(a)
print *,a
n=0
a=reshape([1],[n,3])
print *,shape(a),size(a)
end
""", '2 3\n1 4 2 3 3 4\n0 3 0\n')

    def test_runtime_source_and_multidimensional_source(self):
        self.check("""implicit none
integer::n
integer,allocatable::x(:),a(:,:),b(:,:)
n=6
allocate(x(n))
x=[1,2,3,4,5,6]
a=reshape(x,[2,3])
b=reshape(a,[3,2],order=[2,1])
print *,a
print *,b
end
""", '1 2 3 4 5 6\n1 3 5 2 4 6\n')

    def test_rank_three_and_logical(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:,:)
logical,allocatable::b(:,:)
n=2
a=reshape([1,2,3,4,5,6,7,8],[n,2,2],order=[3,1,2])
print *,a
b=reshape([.true.,.false.],[n,2],pad=[.false.,.true.])
print *,b
end
""", '1 3 5 7 2 4 6 8\nT F F T\n')

    def test_source_sections_and_reassignment(self):
        self.check("""implicit none
integer::n
integer,allocatable::a(:,:),x(:)
n=6
allocate(x(-2:n-3))
x=[1,2,3,4,5,6]
n=2
a=reshape(x(3:-2:-1),[n,3])
print *,a
a=reshape(a,[3,n])
print *,shape(a),a
end
""", '6 5 4 3 2 1\n3 2 6 5 4 3 2 1\n')

    def test_invalid_runtime_shapes(self):
        self.check('integer::n\nn=-1\nprint *,reshape([1],[n])\nend\n', error='RESHAPE SHAPE extents must be nonnegative')
        self.check('integer::n\nn=3\nprint *,reshape([1],[n])\nend\n', error='nonempty PAD')
        self.check('integer::n,e(0)\nn=3\nprint *,reshape([1],[n],pad=e)\nend\n', error='nonempty PAD')

if __name__=='__main__':unittest.main()

"""Complex worker partials must be folded with complex arithmetic."""
import unittest

import test_concurrent_parallel_complex as support
from test_record_read import ROOT


class ComplexReduction(unittest.TestCase):
    build_run = support.ParallelComplex.build_run

    def test_scalar_sum_and_product(self):
        for kind in (4, 8):
            with self.subTest(kind=kind):
                self.build_run(f"""implicit none
integer :: i,n
complex({kind}) :: s,p
n=9001
s=cmplx(3,4,kind={kind})
p=cmplx(2,3,kind={kind})
do concurrent(i=1:n) reduce(+:s) reduce(*:p)
s=s+cmplx(1,-2,kind={kind})
p=p*cmplx(0,1,kind={kind})
end do
if(abs(s-cmplx(9004,-17998,kind={kind}))>0.01) stop 1
if(abs(p-cmplx(-3,2,kind={kind}))>0.01) stop 2
print *, 'PASS'
end
""", 'PASS')

    def test_fixed_and_runtime_multidimensional_arrays(self):
        for kind in (4, 8):
            for dynamic in (False, True):
                with self.subTest(kind=kind, dynamic=dynamic):
                    declaration = (f'complex({kind}),allocatable::s(:,:),p(:,:)\n'
                                   'allocate(s(-1:0,3:4),p(-1:0,3:4))' if dynamic else
                                   f'complex({kind})::s(-1:0,3:4),p(-1:0,3:4)')
                    self.build_run(f"""implicit none
integer::i,n
{declaration}
n=9001
s=cmplx(3,4,kind={kind})
p=cmplx(2,3,kind={kind})
do concurrent(i=1:n) reduce(+:s) reduce(*:p)
s=s+cmplx(1,-2,kind={kind})
p=p*cmplx(0,1,kind={kind})
end do
if(any(abs(s-cmplx(9004,-17998,kind={kind}))>0.01)) stop 1
if(any(abs(p-cmplx(-3,2,kind={kind}))>0.01)) stop 2
print *, 'PASS'
end
""", 'PASS')

    def test_masks_empty_loops_and_inactive_workers(self):
        for selected, expected_s, expected_p in (
            ('modulo(i,3)==0', '(3003,-5996)', '(2,3)'),
            ('i==9001', '(4,2)', '(-3,2)'),
            ('i<0', '(3,4)', '(2,3)'),
        ):
            with self.subTest(mask=selected):
                self.build_run(f"""implicit none
integer::i,n
complex(8)::s,p
n=9001
s=(3d0,4d0)
p=(2d0,3d0)
do concurrent(i=1:n,{selected}) reduce(+:s) reduce(*:p)
s=s+(1d0,-2d0)
p=p*(0d0,1d0)
end do
if(abs(s-{expected_s})>1d-8) stop 1
if(abs(p-{expected_p})>1d-8) stop 2
n=0
do concurrent(i=1:n) reduce(+:s) reduce(*:p)
s=s+(99d0,99d0)
p=p*(99d0,99d0)
end do
if(abs(s-{expected_s})>1d-8) stop 3
if(abs(p-{expected_p})>1d-8) stop 4
print *, 'PASS'
end
""", 'PASS')

    def test_runtime_component_budget_fallback(self):
        for size in (0, 2049):
            with self.subTest(size=size):
                self.build_run(f"""implicit none
integer::i,n
complex(8),allocatable::s(:)
n={size}
allocate(s(n))
s=(3d0,4d0)
do concurrent(i=1:9001) reduce(+:s)
s=s+(1d0,-2d0)
end do
if(any(abs(s-(9004d0,-17998d0))>1d-8)) stop 1
print *, 'PASS'
end
""", 'PASS', dispatch=False)

    def test_fixed_component_budget_fallback(self):
        self.build_run("""implicit none
integer::i
complex(8)::s(2049)
s=(3d0,4d0)
do concurrent(i=1:3) reduce(+:s)
s=s+(1d0,-2d0)
end do
if(any(abs(s-(6d0,-2d0))>1d-8)) stop 1
print *, 'PASS'
end
""", 'PASS', reason='exceeds private-storage budget', threads=(4,), opts=('2',), checks=False)

    def test_array_element_and_section_updates(self):
        self.build_run("""implicit none
integer::i,n
complex(8),allocatable::s(:),p(:)
n=5
allocate(s(-1:n),p(-1:n))
s=(3d0,4d0)
p=(2d0,3d0)
do concurrent(i=1:9001) reduce(+:s) reduce(*:p)
s(0:2)=s(0:2)+(1d0,-2d0)
p(1)=p(1)*(0d0,1d0)
end do
if(any(abs(s(0:2)-(9004d0,-17998d0))>1d-8)) stop 1
if(abs(s(-1)-(3d0,4d0))>1d-8) stop 2
if(abs(p(1)-(-3d0,2d0))>1d-8) stop 3
if(abs(p(-1)-(2d0,3d0))>1d-8) stop 4
if(abs(p(5)-(2d0,3d0))>1d-8) stop 5
print *, 'PASS'
end
""", 'PASS')

    def test_inquiry_subscripts(self):
        self.build_run("""implicit none
complex(8)::a(-2:3),z
a=(3d0,4d0)
z=a(lbound(a,1))+a(ubound(a,1))+a(size(a)-3)
if(abs(z-(9d0,12d0))>1d-8) stop 1
print *, 'PASS'
end
""", 'PASS', parallel=False, threads=(1,))

    def test_rank_fifteen_and_negative_stride(self):
        shape = ','.join(['2'] + ['1']*14)
        self.build_run(f"""implicit none
integer::i
complex(8)::s({shape}),p({shape})
s=(3d0,4d0)
p=(2d0,3d0)
do concurrent(i=9001:1:-1) reduce(+:s) reduce(*:p)
s=s+(1d0,-2d0)
p=p*(0d0,1d0)
end do
if(any(abs(s-(9004d0,-17998d0))>1d-8)) stop 1
if(any(abs(p-(-3d0,2d0))>1d-8)) stop 2
print *, 'PASS'
end
""", 'PASS')

    def test_assumed_shape_repeated_calls(self):
        self.build_run("""implicit none
integer::n
complex(8),allocatable::s(:,:),p(:,:)
do n=2,3
allocate(s(-1:n,3:4),p(-1:n,3:4))
s=(3d0,4d0)
p=(2d0,3d0)
call accumulate(s,p)
call accumulate(s,p)
if(any(abs(s-(18005d0,-36000d0))>1d-8)) stop 1
if(any(abs(p-(-2d0,-3d0))>1d-8)) stop 2
deallocate(s,p)
end do
print *, 'PASS'
contains
subroutine accumulate(s,p)
complex(8),intent(inout)::s(:,:),p(:,:)
integer::i
do concurrent(i=1:9001) reduce(+:s) reduce(*:p)
s=s+(1d0,-2d0)
p=p*(0d0,1d0)
end do
end subroutine
end
""", 'PASS')

    def test_example_serial_and_unchecked(self):
        source = (ROOT/'tests/programs/xconcurrent_parallel_complex_reduce.f90').read_text()
        self.build_run(source, 'PASS')
        self.build_run(source, 'PASS', parallel=False, threads=(1,))
        self.build_run(source, 'PASS', checks=False)


if __name__ == '__main__':
    unittest.main()

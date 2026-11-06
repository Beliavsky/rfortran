"""Native parallel DO CONCURRENT and conservative serial fallback."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT

class ParallelConcurrent(unittest.TestCase):
    def build_run(self, source, expected, parallel=True, reason=None, threads=(1,2,4), opts=('0','2'), checks=True, dispatch=True):
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
            directory=Path(tmp); path=directory/'case.f90';path.write_text(source)
            for optimization in opts:
                exe=directory/'case.exe'
                command=[str(COMPILER),'-O',optimization,'--parallel-report']
                if parallel: command+=['--parallel']
                if not checks: command+=['--nocheck']
                result=subprocess.run(command+[str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                if parallel:
                    self.assertIn(reason or 'native parallel worker emitted',result.stderr)
                    if reason is None: self.assertNotIn('serial fallback',result.stderr)
                else: self.assertNotIn('native parallel worker',result.stderr)
                for n in threads:
                    env=os.environ.copy();env['RFORT_NUM_THREADS']=str(n);env['RFORT_PARALLEL_TRACE']='1'
                    result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(result.stdout.split(),expected.split())
                    if parallel and reason is None and dispatch and n>1:
                        self.assertIn(f'rfort parallel: {n} threads',result.stderr)
                    else: self.assertNotIn('rfort parallel:',result.stderr)

    def test_runtime_array_reduction_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_runtime_reduce.f90').read_text(), '10007.0 10007.0 10007.0 10007.0')

    def test_runtime_array_reduction_assumed_shape(self):
        self.build_run("""implicit none
integer, allocatable :: a(:)
allocate(a(6))
a=7
call accumulate(a(1:5:2))
print *, a
contains
subroutine accumulate(s)
integer, intent(inout) :: s(:)
integer :: i
do concurrent(i=1:10000) reduce(+:s)
s=s+1
end do
end subroutine
end
""", '10007 7 10007 7 10007 7')

    def test_runtime_array_reduction_reallocate(self):
        self.build_run("""implicit none
integer, allocatable :: s(:)
integer :: n,i
n=2
allocate(s(-1:n))
s=3
do concurrent(i=1:10000) reduce(+:s)
s=s+1
end do
print *, s
deallocate(s)
n=3
allocate(s(n))
s=5
do concurrent(i=1:10000) reduce(+:s)
s=s+2
end do
print *, s
end
""", '10003 10003 10003 10003 20005 20005 20005')

    def test_runtime_array_reduction_budget_boundary(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:)
n=4096
allocate(s(n))
s=1
do concurrent(i=1:4096) reduce(+:s)
s(1)=s(1)+1
end do
print *,s(1),s(n)
end
""", '4097 1')

    def test_runtime_array_reduction_unallocated(self):
        source="""implicit none
integer :: i
integer,allocatable :: s(:)
do concurrent(i=1:10000) reduce(+:s)
s=s+1
end do
end
"""
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            exe=Path(tmp)/'case.exe'
            result=subprocess.run([str(COMPILER),'--parallel',str(path),'-o',str(exe),'--run'],capture_output=True,text=True,timeout=90)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('unallocated',result.stderr.lower())

    def test_runtime_array_reduction_budget_fallback(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: a(:),b(:)
n=2050
allocate(a(n),b(n))
a=7
b=9
do concurrent(i=1:5000) reduce(+:a,b)
a=a+1
b=b+2
end do
if(any(a/=5007).or.any(b/=10009)) stop 1
print *,a(1),b(n)
end
""", '5007 10009', dispatch=False, threads=(4,))

    def test_runtime_array_reduction_empty_storage(self):
        self.build_run("""implicit none
integer :: i,n
real,allocatable :: a(:,:)
n=0
allocate(a(2,n))
a=3.0
do concurrent(i=1:10000) reduce(+:a)
a=a+1.0
end do
print *,size(a)
end
""", '0', dispatch=False)

    def test_runtime_array_reduction_masks_and_empty_range(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:)
n=2
allocate(s(n))
s=7
do concurrent(i=1:0) reduce(+:s)
s=s+1
end do
do concurrent(i=1:10000,i<0) reduce(+:s)
s=s+1
end do
print *,s
end
""", '7 7')

    def test_runtime_array_reduction_partial_section(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:)
n=4
allocate(s(-1:n))
s=7
do concurrent(i=1:10000) reduce(+:s)
s(0:2)=s(0:2)+1
end do
print *,s
end
""", '7 10007 10007 10007 7 7')

    def test_runtime_array_reduction_repeated_procedure(self):
        self.build_run("""implicit none
integer :: n
integer,allocatable :: a(:)
allocate(a(2))
a=7
call accumulate(a)
call accumulate(a)
print *,a
deallocate(a)
allocate(a(3))
a=5
call accumulate(a)
print *,a
contains
subroutine accumulate(s)
integer,intent(inout)::s(:)
integer::i
do concurrent(i=1:10000) reduce(+:s)
s=s+1
end do
end subroutine
end
""", '20007 20007 10005 10005 10005')

    def test_runtime_array_reduction_histogram(self):
        self.build_run("""implicit none
integer :: i,n,total,fixed(2),bins(10000)
integer,allocatable :: hist(:)
n=4
allocate(hist(n))
do i=1,10000
bins(i)=mod(i,4)+1
end do
hist=1
total=3
fixed=5
do concurrent(i=1:10000) reduce(+:hist,total,fixed)
hist(bins(i))=hist(bins(i))+1
total=total+1
fixed=fixed+2
end do
print *,hist,total,fixed
end
""", '2501 2501 2501 2501 10003 20005 20005')

    def test_runtime_array_reduction_rank_fifteen(self):
        dims=','.join([':']*15)
        bounds=','.join(['-1:0']+['1']*14)
        self.build_run(f"""implicit none
integer :: i
integer,allocatable :: a({dims})
allocate(a({bounds}))
a=7
do concurrent(i=1:10000) reduce(+:a)
a=a+1
end do
print *,a
end
""", '10007 10007')

    def test_runtime_array_reduction_explicit_dummy_bounds(self):
        self.build_run("""implicit none
integer,allocatable :: a(:,:)
integer :: n
n=2
allocate(a(n,n))
a=5
call accumulate(a,n)
print *,a
contains
subroutine accumulate(s,n)
integer,intent(in)::n
integer,intent(inout)::s(n,n)
integer::i
do concurrent(i=1:10000) reduce(+:s)
s=s+1
end do
end subroutine
end
""", '10005 10005 10005 10005')

    def test_runtime_array_reduction_no_checks(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_runtime_reduce.f90').read_text(), '10007.0 10007.0 10007.0 10007.0', checks=False)

    def test_runtime_array_reduction_runtime_section(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:)
n=4
allocate(s(n))
s=7
do concurrent(i=1:10000) reduce(+:s)
s(1:n)=s(1:n)+1
end do
print *,s
end
""", '10007 10007 10007 10007')

    def test_runtime_section_input_and_reduction(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_sections.f90').read_text(), '9007.0 18007.0 27007.0')

    def test_runtime_section_changing_extent(self):
        self.build_run("""implicit none
integer :: i,k,n
integer,allocatable :: s(:)
n=4
allocate(s(n))
s=7
do concurrent(i=1:9000) local(k) reduce(+:s)
k=1
if (i>4500) k=n
s(1:k)=s(1:k)+1
end do
print *,s
end
""", '9007 4507 4507 4507')

    def test_runtime_section_strides_and_masks(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:),x(:,:)
n=5
allocate(s(-2:n-3),x(n,9000))
s=7
x=1
do concurrent(i=1:9000,i>4500) reduce(+:s)
s(2:-2:-2)=s(2:-2:-2)+x(n:1:-2,i)
end do
print *,s
end
""", '4507 7 4507 7 4507')

    def test_runtime_section_logical_and_minmax(self):
        self.build_run("""implicit none
integer :: i,n
real,allocatable :: lo(:),hi(:)
logical,allocatable :: a(:),b(:)
n=2
allocate(lo(n),hi(n),a(n),b(n))
lo=10000.0
hi=-10000.0
a=.true.
b=.false.
do concurrent(i=1:9000) reduce(min:lo) reduce(max:hi) reduce(.and.:a) reduce(.or.:b)
lo(1:n)=min(lo(1:n),real(i))
hi(n:1:-1)=max(hi(n:1:-1),real(i))
a(1:n)=a(1:n).and.(i<2)
b(1:n)=b(1:n).or.(i==2)
end do
print *,lo,hi,a,b
end
""", '1.0 1.0 9000.0 9000.0 F F T T')

    def test_runtime_section_repeated_calls_and_shapes(self):
        self.build_run("""implicit none
integer :: pass,n
integer,allocatable :: s(:)
do pass=1,3
n=pass+1
allocate(s(n))
s=7
call accumulate(s,n)
call accumulate(s,n)
if(any(s/=18007)) stop 1
print *,s(1),s(n)
deallocate(s)
end do
contains
subroutine accumulate(a,m)
integer,intent(in)::m
integer,intent(inout)::a(:)
integer::i
do concurrent(i=1:9000) reduce(+:a)
a(1:m)=a(1:m)+1
end do
end subroutine
end
""", '18007 18007 18007 18007 18007 18007')

    def test_runtime_section_empty_and_masked(self):
        self.build_run("""implicit none
integer :: i,k,n
integer,allocatable :: s(:)
n=3
allocate(s(n))
s=7
do concurrent(i=1:9000) local(k) reduce(+:s)
k=0
if (i>4500) k=n
s(1:k)=s(1:k)+1
end do
do concurrent(i=1:9000,i<0) reduce(+:s)
s(1:n)=s(1:n)+1
end do
print *,s
end
""", '4507 4507 4507')

    def test_runtime_section_rank_three(self):
        self.build_run("""implicit none
integer :: i,n
integer,allocatable :: s(:,:,:)
n=2
allocate(s(-1:0,3:n+2,0:n-1))
s=7
do concurrent(i=1:9000) reduce(+:s)
s(-1:0,3:n+2,0:n-1)=s(-1:0,3:n+2,0:n-1)+1
end do
print *,s
end
""", '9007 9007 9007 9007 9007 9007 9007 9007')

    def test_runtime_section_uninitialized_input(self):
        source="""implicit none
integer :: i,m,n
real,allocatable :: s(:),x(:,:)
m=3
n=9000
allocate(s(m),x(m,n))
x(:,1:n-1)=1.0
s=7.0
do concurrent(i=1:n) reduce(+:s)
s(1:m)=s(1:m)+x(1:m,i)
end do
print *,s
end
"""
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            exe=Path(tmp)/'case.exe'
            for opt in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                env=os.environ.copy();env['RFORT_NUM_THREADS']='4'
                result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('uninitialized variable `x`',result.stderr)

    def test_runtime_section_no_checks(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_sections.f90').read_text(), '9007.0 18007.0 27007.0', checks=False)

    def test_runtime_array_reduction_numeric_logical(self):
        self.build_run("""implicit none
integer :: n,i
integer,allocatable :: lo(:),hi(:)
double precision,allocatable :: product(:)
logical,allocatable :: both(:),either(:),eq(:),neq(:)
n=2
allocate(lo(n),hi(n),product(n),both(n),either(n),eq(n),neq(n))
lo=99999
hi=-99999
product=3.0d0
both=.true.
either=.false.
eq=.true.
neq=.false.
do concurrent(i=1:10000) reduce(min:lo) reduce(max:hi) reduce(*:product) &
& reduce(.and.:both) reduce(.or.:either) reduce(.eqv.:eq) reduce(.neqv.:neq)
lo=min(lo,i)
hi=max(hi,i)
product=product*1.0d0
both=both.and.(i<2)
either=either.or.(i==2)
eq=eq.eqv.(i/=2)
neq=neq.neqv.(i==2)
end do
print *,lo,hi,product,both,either,eq,neq
end
""", '1 1 10000 10000 3.0 3.0 F F T T F F T T')

    def test_multidimensional_fixed_mask_reduce(self):
        self.build_run("""implicit none
integer :: i,j,s=0,a(100,90)
logical :: b(100,90)
a=0
b=.false.
do concurrent(j=1:90,i=1:100,i>2) reduce(+:s)
a(i,j)=i+j
b(i,j)=a(i,j)>100
s=s+1
end do
print *,a(1,1),a(3,1),a(100,90),b(100,90),s
end
""",'0 4 190 T 8820')

    def test_multidimensional_allocatable(self):
        self.build_run("""implicit none
integer :: i,j,k
real, allocatable :: a(:,:,:)
allocate(a(-2:17,0:19,3:22))
do concurrent(i=-2:17,j=0:19,k=3:22)
a(i,j,k)=real(i+j+k)
end do
print *,a(-2,0,3),a(17,19,22)
end
""",'1.0 58.0')

    def test_multidimensional_assumed_shape(self):
        self.build_run("""implicit none
real :: a(100,90)
call fill(a)
print *,a(1,1),a(100,90)
contains
subroutine fill(x)
real, intent(out) :: x(:,:)
integer :: i,j,n,m
n=size(x,1)
m=size(x,2)
do concurrent(i=1:n,j=1:m)
x(i,j)=real(i+j)
end do
end subroutine
end
""",'2.0 190.0')

    def test_multidimensional_empty(self):
        self.build_run("""implicit none
integer :: i,j,k,s=17
do concurrent(i=1:100,j=1:0,k=1:100) reduce(+:s)
s=s+1
end do
print *,s
end
""",'17',threads=(1,))

    def test_multidimensional_fixed_lower_bounds(self):
        self.build_run("""implicit none
integer :: i,j,a(-49:50,0:89)
do concurrent(i=-49:50,j=0:89)
a(i,j)=i+j
end do
print *,a(-49,0),a(50,89)
end
""",'-49 139')

    def test_multidimensional_permuted_write_fallback(self):
        self.build_run("""integer :: i,j,a(3,3)
do concurrent(i=1:3,j=1:3)
a(i,j)=1
a(j,i)=1
end do
print *,a(1,1)
end
""",'1',reason='same index tuple',threads=(1,))

    def test_multidimensional_rank_fifteen(self):
        indices=[f'i{k}' for k in range(15)]
        dims=','.join(['2']*15)
        controls=','.join(f'{i}=1:2' for i in indices)
        # Continuations keep this a valid free-form Fortran source line.
        controls=controls.replace(',',', &\n')
        source=f"""implicit none
integer :: {','.join(indices)},s=0
integer, allocatable :: a({','.join([':']*15)})
allocate(a({dims}))
do concurrent({controls}) reduce(+:s)
a({','.join(indices)})=1
s=s+1
end do
print *,s,sum(a)
end
"""
        self.build_run(source,'32768 32768',opts=('2',))

    def test_multidimensional_iteration_overflow(self):
        template="""implicit none
integer(kind=8) :: i,j,lo,hi,s=7
lo=LOW
hi=HIGH
do concurrent(i=lo:hi,j=1:LAST) reduce(+:s)
s=s+1
end do
print *,s
end
"""
        with tempfile.TemporaryDirectory(prefix='parallel-extents-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            for low,high in [('0_8','huge(hi)'),('1_8','huge(hi)')]:
                for last in ('0','2'):
                    path.write_text(template.replace('LOW',low).replace('HIGH',high).replace('LAST',last))
                    for opt in ('0','2'):
                        result=subprocess.run([str(COMPILER),'-O',opt,'--parallel',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                        self.assertEqual(result.returncode,0,result.stderr)
                        result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=10)
                        if last=='0':
                            self.assertEqual(result.returncode,0,result.stderr)
                            self.assertEqual(result.stdout.split(),['7'])
                        else:
                            self.assertNotEqual(result.returncode,0)
                            self.assertIn('iteration space exceeds signed 64-bit range',result.stderr)

    def test_multidimensional_runtime_bounds(self):
        source="""implicit none
integer :: i,j
real,allocatable :: a(:,:),b(:,:)
allocate(a(100,90),b(100,90))
a=1.0
do concurrent(i=1:100,j=1:90)
b(i,j)=a(i,j+1)
end do
end
"""
        with tempfile.TemporaryDirectory(prefix='parallel-matrix-check-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe';path.write_text(source)
            for opt in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                for n in (1,2,4):
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(n)),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn('bounds',result.stderr)

    def test_multidimensional_strided_actual_and_locality(self):
        self.build_run("""implicit none
double precision :: a(200,180),total
a=-1.0d0
total=7.0d0
call fill(a(1:200:2,1:180:2),total)
print *,a(1,1),a(199,179),a(2,2),total
contains
subroutine fill(x,total)
double precision,intent(out)::x(:,:)
double precision,intent(inout)::total
double precision :: t
integer :: i,j,n,m
n=size(x,1)
m=size(x,2)
do concurrent(i=1:n,j=1:m) local(t) reduce(+:total)
t=2.0d0
x(i,j)=t
total=total+t
end do
end subroutine
end
""",'2.0 2.0 -1.0 18007.0')

    def test_multidimensional_written_read_fallback(self):
        self.build_run("""integer :: i,j,a(3,3)
a=1
do concurrent(i=1:3,j=1:3)
a(i,j)=a(j,i)
end do
print *,a(1,1)
end
""",'1',reason='not confined to the current iteration',threads=(1,))

    def test_multidimensional_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_matrix.f90').read_text(),'8000 1.0 58.0')

    def test_stride_positive_offsets(self):
        self.build_run("""implicit none
integer :: i,n,step,slot,total,a(18001,2)
n=18000
step=2
slot=2
total=0
a=-1
do concurrent(i=1:n:step) reduce(+:total)
a(i+1,slot)=i
total=total+a(i+1,slot)
end do
print *,a(1,2),a(2,2),a(18000,2),a(18001,2),total
end
""",'-1 1 17999 -1 81000000')

    def test_stride_negative_allocatable(self):
        self.build_run("""implicit none
integer :: i,n,step,total
integer,allocatable :: a(:,:)
n=18000
step=-2
allocate(a(0:n,2))
a=-1
total=0
do concurrent(i=n:1:step) reduce(+:total)
a(2+i-2,2)=i
total=total+a(2+i-2,2)
end do
print *,a(n,2),a(2,2),a(1,2),total
end
""",'18000 2 -1 81009000')

    def test_stride_multiple_controls_mask(self):
        self.build_run("""implicit none
integer :: i,j,total,a(101,180,2)
a=0
total=0
do concurrent(j=1:180:2,i=100:1:-1,i>2) reduce(+:total)
a(i+1,j,2)=i+j
total=total+1
end do
print *,a(2,1,2),a(4,1,2),a(101,179,2),total
end
""",'0 4 279 8820')

    def test_stride_empty_and_uneven(self):
        self.build_run("""implicit none
integer :: i,s=17,a(12)
a=0
do concurrent(i=1:10:-2) reduce(+:s)
s=s+i
end do
do concurrent(i=10:1:2) reduce(+:s)
s=s+i
end do
do concurrent(i=2:12:3)
a(i)=i
end do
do concurrent(i=12:2:-3)
a(i)=i
end do
print *,s,a
end
""",'17 0 2 3 0 5 6 0 8 9 0 11 12',threads=(1,))

    def test_stride_assumed_shape_offsets(self):
        self.build_run("""implicit none
real :: a(18002,2)
a=-1.0
call fill(a,-2)
print *,a(18001,2),a(3,2),a(2,2)
contains
subroutine fill(x,stride)
real,intent(inout)::x(:,:)
integer,intent(in)::stride
integer::i,n
n=size(x,1)-2
do concurrent(i=n:1:stride)
x(i+1,2)=real(i)
end do
end subroutine
end
""",'18000.0 2.0 -1.0')

    def test_stride_fixed_subscript_unit(self):
        self.build_run("""implicit none
integer :: i,a(9001,2)
a=-1
do concurrent(i=1:9000)
a(i+1,1)=i
end do
print *,a(1,1),a(2,1),a(9001,1),a(1,2)
end
""",'-1 1 9000 -1')

    def test_stride_mutable_offset_fallback(self):
        self.build_run("""implicit none
integer :: i,k=1,a(3,2)
a=0
do concurrent(i=1:3) local_init(k)
k=2
a(i,k)=i
end do
print *,a(:,2)
end
""",'1 2 3',reason='invariant subscripts or index offsets',threads=(1,))

    def test_stride_different_offset_fallback(self):
        self.build_run("""implicit none
integer :: i,a(4)
a=0
do concurrent(i=1:3)
a(i)=1
a(i+1)=1
end do
print *,a
end
""",'1 1 1 1',reason='same index tuple',threads=(1,))

    def test_stride_missing_index_fallback(self):
        self.build_run("""implicit none
integer :: i,j,a(3)
a=0
do concurrent(i=1:3,j=1:2)
a(i)=1
end do
print *,a
end
""",'1 1 1',reason='every concurrent index exactly once',threads=(1,))

    def test_stride_zero_diagnostic(self):
        with tempfile.TemporaryDirectory(prefix='parallel-stride-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            path.write_text('integer :: i,k,s=0\nk=0\ndo concurrent(i=1:9:k) reduce(+:s)\ns=s+i\nend do\nend\n')
            for opt in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--nocheck',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=10)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('stride must not be zero',result.stderr)

    def test_stride_snapshot_before_reduction(self):
        self.build_run("""implicit none
integer :: i,stride=2
do concurrent(i=1:18000:stride) reduce(+:stride)
stride=stride+1
end do
print *,stride
end
""",'9002')

    def test_stride_runtime_bounds_and_initialization(self):
        template="""implicit none
integer :: i,stride
real,allocatable :: a(:),b(:)
stride=-2
allocate(a(18000),b(18000))
a=1.0
INITIALIZE
do concurrent(i=18000:1:stride)
b(i+OFFSET)=a(i)
end do
end
"""
        with tempfile.TemporaryDirectory(prefix='parallel-stride-check-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            for message,source in [('bounds',template.replace('INITIALIZE','').replace('OFFSET','1')),
                                   ('uninitialized',template.replace('a=1.0','a(1:17999)=1.0').replace('INITIALIZE','').replace('OFFSET','0'))]:
                path.write_text(source)
                for opt in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    for n in (1,2,4):
                        result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(n)),capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(message,result.stderr)

    def test_stride_example_without_checks(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_stride.f90').read_text(),'81009000 2 18000',checks=False)

    def test_stride_negative_bounds(self):
        self.build_run("""implicit none
integer :: i,a(-7:8,2)
a=0
do concurrent(i=-7:8:3)
a(i,1)=i
end do
do concurrent(i=8:-7:-3)
a(i,2)=i
end do
print *,a(-7,:),a(8,:),sum(a(:,1)),sum(a(:,2))
end
""",'-7 -7 8 8 3 3',threads=(1,))

    def test_locality_repeated_jobs(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel.f90').read_text(),'16.0 40014.0 99 -1 10')

    def test_default_serial(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel.f90').read_text(),'16.0 40014.0 99 -1 10',parallel=False,opts=('2',))

    def test_mask_and_logical_output(self):
        self.build_run('''implicit none
integer :: i,a(9000)
logical :: b(9000)
a=0
b=.false.
do concurrent(i=1:9000,i>2) shared(a,b) default(none)
a(i)=i
b(i)=i>8000
end do
print *,a(1),a(3),a(9000),b(1),b(9000)
end
''','0 3 9000 F T')

    def test_readonly_array_and_intrinsics(self):
        self.build_run('''implicit none
integer :: i
real :: x(9000),y(9000)
x=4.0
do concurrent(i=1:9000) shared(x,y) default(none)
y(i)=sqrt(x(9001-i))+abs(-3.0)
end do
print *,y(1),y(9000)
end
''','5.0 5.0')

    def test_empty_and_small_ranges(self):
        self.build_run('''implicit none
integer :: i,a(4)
a=0
do concurrent(i=1:0) shared(a)
a(i)=i
end do
do concurrent(i=1:4) shared(a)
a(i)=i
end do
print *,a
end
''','1 2 3 4',threads=(1,),opts=('2',))

    def test_strided_small_range(self):
        self.build_run('''integer :: i,a(6)
a=0
do concurrent(i=1:6:2)
a(i)=i
end do
print *,a
end
''','1 0 3 0 5 0',threads=(1,),opts=('2',))

    def test_scalar_reduction(self):
        self.build_run('''integer :: i,s=0
do concurrent(i=1:10000) reduce(+:s)
s=s+i
end do
print *,s
end
''','50005000')

    def test_multiple_reductions(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_reduce.f90').read_text(),
                       '50005007 2.0 1 10000 T T T F 10007.0')

    def test_reduction_empty_mask_and_small(self):
        self.build_run("""implicit none
integer :: i,s=17,m=100
logical :: flag=.true.
do concurrent(i=1:9000,i<0) reduce(+:s) reduce(min:m) reduce(.neqv.:flag)
s=s+i
m=min(m,i)
flag=flag .neqv. .true.
end do
print *,s,m,flag
do concurrent(i=1:0) reduce(+:s)
s=s+i
end do
do concurrent(i=1:3) reduce(+:s)
s=s+i
end do
print *,s
end
""",'17 100 T 23')

    def test_reduction_partial_mask(self):
        self.build_run("""implicit none
integer :: i,s=13,a(9000)
a=0
do concurrent(i=1:9000,i>8990) reduce(+:s) shared(a) default(none)
s=s+i
a(i)=i
end do
print *,s,a(9000),a(1)
end
""",'89968 9000 0')

    def test_double_dot_product_and_integer_product(self):
        self.build_run("""implicit none
integer :: i,p
integer(kind=8) :: big
real(kind=8) :: x(9001),y(9001),dot,lo,hi
x=0.25d0
y=0.5d0
dot=7.0d0
lo=100.0d0
hi=-100.0d0
p=2
big=3000000000_8
do concurrent(i=1:9001) shared(x,y) reduce(+:dot,big) reduce(*:p) reduce(min:lo) reduce(max:hi)
dot=dot+x(i)*y(i)
big=big+1_8
lo=min(lo,x(i))
hi=max(hi,y(i))
if(i==9001) p=p*3
end do
if(abs(dot-1132.125d0)>1d-10) stop 1
print *,p,big,lo,hi
end
""",'6 3000009001 0.25 0.5')

    def test_array_reduction_rank_two_sections(self):
        self.build_run("""implicit none
integer :: i
real :: x(2,2,9000),s(-1:0,3:4)
x=2.0
s=7.0
do concurrent(i=1:9000) reduce(+:s)
s=s+x(:,:,i)
end do
print *,s
end
""",'18007.0 18007.0 18007.0 18007.0')

    def test_array_reduction_element_updates(self):
        self.build_run("""implicit none
integer :: i,s(2)
s=7
do concurrent(i=1:9000) reduce(+:s)
s(1)=s(1)+1
s(2)=s(2)+2
end do
print *,s
end
""",'9007 18007')

    def test_array_reduction_numeric_operations(self):
        self.build_run("""implicit none
integer :: i,lo(2),hi(2)
double precision :: product(2)
lo=20000
hi=-1
product=3.0d0
do concurrent(i=1:9000) reduce(min:lo) reduce(max:hi) reduce(*:product)
lo=min(lo,[i,-i])
hi=max(hi,[i,-i])
product=product*[1.0d0,1.0d0]
end do
print *,lo,hi,product
end
""",'1 -9000 9000 -1 3.0 3.0')

    def test_array_reduction_logical_operations(self):
        self.build_run("""implicit none
integer :: i
logical :: a(2),b(2),c(2),d(2)
a=.true.
b=.false.
c=.true.
d=.false.
do concurrent(i=1:9001) reduce(.and.:a) reduce(.or.:b) reduce(.eqv.:c) reduce(.neqv.:d)
a=a .and. [.true.,i<9001]
b=b .or. [.false.,i==9001]
c=c .eqv. [.true.,.false.]
d=d .neqv. [.false.,.true.]
end do
print *,a,b,c,d
end
""",'T F F T T F F T')

    def test_array_reduction_empty_and_masks(self):
        self.build_run("""implicit none
integer :: i,s(2)
s=[7,9]
do concurrent(i=1:0) reduce(+:s)
s=s+[1,1]
end do
do concurrent(i=1:9000,i<0) reduce(+:s)
s=s+[1,1]
end do
do concurrent(i=1:3,i==2) reduce(+:s)
s=s+[2,3]
end do
print *,s
end
""",'9 12',threads=(1,2,4))

    def test_array_reduction_procedure(self):
        self.build_run("""implicit none
real :: s(2),x(2,9000)
x=1.0
s=5.0
call accumulate(x,s)
call accumulate(x,s)
print *,s
contains
subroutine accumulate(x,s)
real,intent(in)::x(:,:)
real,intent(inout)::s(2)
integer::i,n
n=size(x,2)
do concurrent(i=1:n) reduce(+:s)
s=s+x(:,i)
end do
end subroutine
end
""",'18005.0 18005.0')

    def test_array_reduction_budget_fallback(self):
        self.build_run("""implicit none
integer,parameter::n=4097
integer :: i,s(n)
s=0
do concurrent(i=1:2) reduce(+:s)
s=s+1
end do
print *,s(1),s(n)
end
""",'2 2',reason='private-storage budget',threads=(1,),opts=('2',))

    def test_array_reduction_rank_three_and_scalar(self):
        self.build_run("""implicit none
integer,parameter :: n=2
integer :: i,j,total=7,s(n,n,n)
s=3
do concurrent(i=100:1:-1,j=1:90) reduce(+:s,total)
s=s+1
total=total+1
end do
print *,minval(s),maxval(s),total
end
""",'9003 9003 9007')

    def test_array_reduction_rank_fifteen(self):
        dims=','.join(['2']+['1']*14)
        self.build_run(f"""implicit none
integer :: i,s({dims})
s=1
do concurrent(i=1:9000) reduce(+:s)
s=s+1
end do
print *,sum(s)
end
""",'18002',opts=('2',))

    def test_array_reduction_empty_storage_fallback(self):
        self.build_run("""implicit none
integer :: i,s(0)
s=0
do concurrent(i=1:10) reduce(+:s)
s=s+1
end do
print *,size(s)
end
""",'0',reason='zero-sized parallel array REDUCE',threads=(1,),opts=('2',))

    def test_array_reduction_combined_budget(self):
        self.build_run("""implicit none
integer :: i,s(3000),t(3000)
s=0
t=0
do concurrent(i=1:2) reduce(+:s,t)
s=s+1
t=t+1
end do
print *,s(1),t(1)
end
""",'2 2',reason='private-storage budget',threads=(1,),opts=('2',))

    def test_array_reduction_partial_section(self):
        self.build_run("""implicit none
integer :: i,s(4)
s=5
do concurrent(i=1:9000) reduce(+:s)
s(2:3)=s(2:3)+[1,2]
end do
print *,s
end
""",'5 9005 18005 5')

    def test_array_reduction_no_checks(self):
        self.build_run("""implicit none
integer :: i,s(2)
s=3
do concurrent(i=1:9000) reduce(+:s)
s=s+[1,2]
end do
print *,s
end
""",'9003 18003',checks=False)

    def test_array_reduction_histogram(self):
        self.build_run("""implicit none
integer :: i,bins(10000),hist(4)
do i=1,10000
bins(i)=mod(i,4)+1
end do
hist=1
do concurrent(i=1:10000) reduce(+:hist)
hist(bins(i))=hist(bins(i))+1
end do
print *,hist
end
""",'2501 2501 2501 2501')

    def test_array_reduction(self):
        self.build_run("""integer :: i,s(2)
s=0
do concurrent(i=1:10000) reduce(+:s)
s=s+[i,1]
end do
print *,s
end
""",'50005000 10000')

    def test_procedure_missing_intent_fallback(self):
        self.build_run('''program p
integer :: a(9000)
call fill(a)
print *,a(1),a(9000)
contains
subroutine fill(a)
integer :: a(9000),i
do concurrent(i=1:9000)
a(i)=i
end do
end subroutine
end program
''','1 9000',reason='procedure captures',opts=('2',))

    def test_pure_scalar_call(self):
        self.build_run('''program p
integer :: a(9000),i
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
contains
pure integer function f(i)
integer,intent(in)::i
f=i+1
end function
end program
''','2 9001')

    def test_pure_call_chain(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_functions.f90').read_text(),'10.0 10.0')

    def test_pure_call_chain_from_array_procedure(self):
        source="""implicit none
integer::a(9000)
call fill(a)
print *,a(1),a(9000)
contains
subroutine fill(x)
integer,intent(out)::x(:)
integer::i
do concurrent(i=1:size(x))
x(i)=increment(i)
end do
end subroutine
pure integer function increment(x)
integer,intent(in)::x
increment=identity(x)+1
end function
simple integer function identity(x)
integer,intent(in)::x
identity=x
end function
end
"""
        self.build_run(source,'2 9001')
        self.build_run(source,'2 9001',parallel=False,threads=(1,))

    def test_pure_call_keywords(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(b=2,a=i)
end do
print *,a(1),a(9000)
contains
pure integer function f(a,b)
integer,intent(in)::a,b
f=a+b
end function
end
""",'3 9002')

    def test_pure_call_logical_mask_and_reduction(self):
        self.build_run("""implicit none
integer::i,s=7,a(9000)
a=0
do concurrent(i=1:9000,accept(i)) reduce(+:s)
a(i)=twice(i)
s=s+1
end do
print *,a(1),a(4501),a(9000),s
contains
pure logical function accept(i)
integer,intent(in)::i
accept=i>4500
end function
pure integer function twice(i)
integer,value::i
twice=2*i
end function
end
""",'0 9002 18000 4507')

    def test_pure_call_value_dummy_mutation(self):
        self.build_run("""implicit none
integer::i,x(9000),y(9000)
do i=1,9000
x(i)=i
end do
do concurrent(i=1:9000)
y(i)=f(x(i))
end do
print *,x(1),x(9000),y(1),y(9000)
contains
pure integer function f(x)
integer,value::x
x=x+1
f=x
end function
end
""",'1 9000 2 9001')

    def test_pure_call_local_loop_and_early_return(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
contains
pure integer function f(n) result(r)
integer,intent(in)::n
integer::j,t
t=0
if (n==1) then
r=17
return
end if
do j=1,3
t=t+j
end do
r=t+n
end function
end
""",'17 9006')

    def test_pure_call_real_kinds_and_unrelated_constant(self):
        self.build_run("""use iso_fortran_env, only:real64,int64
implicit none
character(*),parameter::unrelated='unused'
integer::i
double precision::a(9000)
integer(int64)::b(9000)
do concurrent(i=1:9000)
a(i)=f(real(i,real64))
b(i)=g(int(i,int64))
end do
print *,a(1),a(9000),b(1),b(9000)
contains
pure real(real64) function f(x)
real(real64),intent(in)::x
f=sqrt(x*x)+1.0_real64
end function
simple integer(int64) function g(x)
integer(int64),intent(in)::x
g=x+2_int64
end function
end
""",'2.0 9001.0 3 9002')

    def test_pure_call_uninitialized_local(self):
        source="""implicit none
integer::i,a(9001)
do concurrent(i=0:9000)
a(i+1)=f(i)
end do
print *,a(1)
contains
pure integer function f(n) result(r)
integer,intent(in)::n
integer::t
if(n>0) t=7
r=t+n
end function
end
"""
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            exe=Path(tmp)/'case.exe'
            for opt in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                env=os.environ.copy();env['RFORT_NUM_THREADS']='4'
                result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('uninitialized variable `t`',result.stderr)

    def test_pure_call_module_constant(self):
        self.build_run("""module m
implicit none
integer,parameter::offset=4
contains
pure integer function f(i)
integer,intent(in)::i
f=i+offset
end function
end module
program main
use m, only:f
implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
end
""",'5 9004')

    def test_pure_call_module_variable_fallback(self):
        self.build_run("""module m
implicit none
integer::offset=4
contains
pure integer function f(i)
integer,intent(in)::i
f=i+offset
end function
end module
program main
use m, only:f
implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
end
""",'5 9004',reason='nonlocal reference')

    def test_pure_call_transitive_state_fallback(self):
        self.build_run("""module m
implicit none
integer::offset=4
contains
pure integer function f(i)
integer,intent(in)::i
f=g(i)
end function
pure integer function g(i)
integer,intent(in)::i
g=i+offset
end function
end module
program main
use m, only:f
implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
end
""",'5 9004',reason='not a proven PURE function')

    def test_pure_call_recursion_fallback(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(min(i,4))
end do
print *,a(1),a(9000)
contains
recursive pure integer function f(n) result(r)
integer,intent(in)::n
if (n==0) then
r=0
else
r=f(n-1)+1
end if
end function
end
""",'1 4',reason='not a proven PURE function')

    def test_pure_call_optional_fallback(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
contains
pure integer function f(n)
integer,intent(in),optional::n
f=0
if(present(n)) f=n+1
end function
end
""",'2 9001',reason='unsupported or saved storage')

    def test_pure_call_fixed_array_argument(self):
        self.build_run("""implicit none
integer::i,a(9000),x(2)
x=[2,3]
do concurrent(i=1:9000)
a(i)=f(x)
end do
print *,a(1),a(9000)
contains
pure integer function f(x)
integer,intent(in)::x(2)
f=x(1)+x(2)
end function
end
""",'5 5')

    def test_pure_array_columns(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_array_functions.f90').read_text(), '20.0 20.0 8.0 8.0')

    def test_pure_workspace_example(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_workspace.f90').read_text(),'16.0 16.0')
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_workspace.f90').read_text(),'16.0 16.0',parallel=False,threads=(1,))
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_workspace.f90').read_text(),'16.0 16.0',checks=False)

    def test_pure_result_example(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_array_results.f90').read_text()
        self.build_run(source,'16.0 16.0')
        self.build_run(source,'16.0 16.0',parallel=False,threads=(1,))
        self.build_run(source,'16.0 16.0',checks=False)

    def test_pure_result_fixed_and_nested_calls(self):
        self.build_run('''implicit none
integer::i,j,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
do j=1,9000
if(y(j)/=12*j) stop 1
end do
print *,y(1),y(9000)
contains
pure function pair(n) result(r)
integer,intent(in)::n
integer::r(2)
r=[n,2*n]
end function
pure function twice(a) result(r)
integer,intent(in)::a(:)
integer::r(size(a))
r=2*a
end function
pure integer function total(n)
integer,intent(in)::n
total=sum(twice(pair(n)))+sum(twice(pair(n)))
end function
end
''','12 108000')

    def test_pure_result_changing_size_and_empty(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i-4*(i/4))
end do
print *,y(1:4),y(9000)
contains
simple function values(n) result(r)
integer,intent(in)::n
integer::r(n)
r=3
end function
simple integer function total(n)
integer,intent(in)::n
total=sum(values(n))
end function
end
''','3 6 9 0 0')

    def test_pure_result_multidimensional_and_bounds(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
contains
pure function matrix(n) result(r)
integer,intent(in)::n
integer::r(-1:0,2:4)
r=n
r(-1,2)=2*n
end function
pure integer function total(n)
integer,intent(in)::n
total=sum(matrix(n))
end function
end
''','7 63000')

    def test_pure_result_logical_and_early_return(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
contains
pure function flags(n) result(r)
integer,intent(in)::n
logical::r(2)
r=.false.
if(n==1) return
r(1)=.true.
end function
pure integer function total(n)
integer,intent(in)::n
total=count(flags(n))
end function
end
''','0 1')

    def test_pure_result_direct_section_assignment(self):
        source='''implicit none
integer::i,n,y(5,9000)
n=2
y=-1
do concurrent(i=1:9000)
y(2:4:2,i)=pair(i)
end do
if(any(y(1,:)/=-1).or.any(y(3,:)/=-1).or.any(y(5,:)/=-1)) stop 1
print *,y(2,1),y(4,1),y(2,9000),y(4,9000)
contains
pure function pair(n) result(r)
integer,intent(in)::n
integer::r(2)
r=[n,2*n]
end function
end
'''
        self.build_run(source,'1 2 9000 18000')
        self.build_run(source,'1 2 9000 18000',parallel=False,threads=(1,))

    def test_pure_result_direct_reduction(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(pair(i))
end do
print *,y(1),y(9000)
contains
pure function pair(n) result(r)
integer,intent(in)::n
integer::r(2)
r=n
end function
end
''','2 18000')

    def test_pure_result_runtime_destination_and_rank_three(self):
        self.build_run('''implicit none
integer::i,n
integer,allocatable::y(:,:)
n=3
allocate(y(n,9000))
do concurrent(i=1:9000)
y(:,i)=values(n,i)
end do
print *,y(:,1),y(:,9000)
contains
pure function cube(n,k) result(r)
integer,intent(in)::n,k
integer::r(n,2,2)
r=k
end function
pure function values(n,k) result(r)
integer,intent(in)::n,k
integer::r(n)
r=sum(cube(n,k))
end function
end
''','12 12 12 108000 108000 108000')

    def test_pure_result_private_whole_array_assignment(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
contains
pure integer function total(n)
integer,intent(in)::n
integer::work(2)
work=pair(n)
total=sum(work)
end function
pure function pair(n) result(r)
integer,intent(in)::n
integer::r(2)
r=n
end function
end
''','2 18000')

    def test_pure_result_runtime_checks(self):
        for layout,failure in ((layout,failure) for layout in ('2','n') for failure in ('uninitialized','bounds')):
            source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(pair(2,i==9000))
end do
print *,y(1)
contains
pure function pair(n,skip) result(r)
integer,intent(in)::n
logical,intent(in)::skip
integer::r(EXTENT)
if(.not.skip) r=2
end function
end
'''.replace('EXTENT',layout)
            if failure=='bounds': source=source.replace('if(.not.skip) r=2','r=2\nr(n+1)=3')
            with self.subTest(layout=layout,failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn('uninitialized' if failure=='uninitialized' else 'outside',result.stderr.lower())

    def test_pure_result_real_kinds_and_runtime_bounds(self):
        self.build_run('''implicit none
integer,parameter::dp=kind(1.0d0)
integer::i
real(kind=dp)::y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(3,i))
end do
print *,y(1),y(9000)
contains
pure function values(n,k) result(r)
integer,intent(in)::n,k
real(kind=dp)::r(-n:-1)
r=real(k,dp)
end function
end
''','3.0 27000.0')

    def test_pure_result_overlapping_section_fallback(self):
        self.build_run('''implicit none
integer::i,y(9001)
do concurrent(i=1:9000)
y(i:i+1)=pair(i)
end do
print *,y(1),y(9001)
contains
pure function pair(n) result(r)
integer,intent(in)::n
integer::r(2)
r=n
end function
end
''','1 9000',reason='independent array-element writes')

    def test_pure_result_allocatable_explicit(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(pair(i))
end do
print *,y(1),y(9000)
contains
pure function pair(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:)
allocate(r(2))
r=n
end function
end
''','2 18000')

    def test_pure_alloc_result_example(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_allocatable_results.f90').read_text()
        self.build_run(source,'1 4 9 0 0')
        self.build_run(source,'1 4 9 0 0',parallel=False,threads=(1,))
        self.build_run(source,'1 4 9 0 0',checks=False)

    def test_pure_alloc_result_assignment_and_nested_calls(self):
        self.build_run('''implicit none
integer::i,n
real,allocatable::x(:,:),y(:)
n=3
allocate(x(n,9000),y(9000))
x=2.0
do concurrent(i=1:9000)
y(i)=sum(twice(twice(x(:,i))))
end do
if(any(y/=24.0)) stop 1
print *,y(1),y(9000)
contains
pure function twice(a) result(r)
real,intent(in)::a(:)
real,allocatable::r(:)
r=2*a
end function
end
''','24.0 24.0')

    def test_pure_alloc_result_reallocation_and_early_return(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i-4*(i/4)))
end do
print *,y(1:4),y(9000)
contains
simple function values(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:)
r=[1,2]
if(n==0) return
deallocate(r)
allocate(r(n))
r=3
r=[4,5,6]
end function
end
''','15 15 15 3 3')

    def test_pure_alloc_result_logical_and_section_output(self):
        self.build_run('''implicit none
integer::i
logical::y(3,9000)
do concurrent(i=1:9000)
y(:,i)=flags(i)
end do
print *,count(y(:,1)),count(y(:,9000))
contains
pure function flags(n) result(r)
integer,intent(in)::n
logical,allocatable::r(:)
r=[n==1,.true.,.false.]
end function
end
''','2 1')

    def test_pure_alloc_result_rank_three(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(cube(2,i))
end do
print *,y(1),y(9000)
contains
pure function cube(n,k) result(r)
integer,intent(in)::n,k
integer,allocatable::r(:,:,:)
allocate(r(-1:n-2,2:3,4:5))
r=k
end function
end
''','8 72000')

    def test_pure_alloc_result_inquiries(self):
        source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i))
end do
print *,y(1),y(9000)
contains
pure function values(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:)
if(allocated(r)) deallocate(r)
allocate(r(2))
r=n
if(allocated(r)) r(1)=size(r)+lbound(r,1)+ubound(r,1)
end function
end
'''
        self.build_run(source,'6 9005')
        self.build_run(source.replace('allocated(r)','allocated(array=r)'),'6 9005')

    def test_pure_alloc_result_self_assignment(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i))
end do
print *,y(1),y(9000)
contains
pure function values(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:)
r=[n,2*n]
r=[r,3*n]
end function
end
''','6 54000')

    def test_pure_alloc_result_runtime_checks(self):
        for failure,body in (
            ('unallocated','if(skip) return\nallocate(r(n))\nr=2'),
            ('uninitialized','allocate(r(n))\nif(.not.skip) r=2'),
            ('outside','allocate(r(n))\nr=2\nr(n+1)=3')):
            source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(pair(2,i==9000))
end do
print *,y(1)
contains
pure function pair(n,skip) result(r)
integer,intent(in)::n
logical,intent(in)::skip
integer,allocatable::r(:)
BODY
end function
end
'''.replace('BODY',body)
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(failure,result.stderr.lower())

    def test_pure_alloc_result_local_workspace(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i))
end do
print *,y(1),y(9000)
contains
pure function values(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:),work(:)
allocate(work(2))
work=n
r=work
end function
end
''','2 18000')

    def test_pure_alloc_result_allocation_status(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i))
end do
print *,y(1),y(9000)
contains
pure function values(n) result(r)
integer,intent(in)::n
integer::istat
integer,allocatable::r(:)
allocate(r(2),stat=istat)
if(istat/=0) return
r=n
end function
end
''','2 18000')

    def test_pure_alloc_result_module_call_chain_and_kinds(self):
        source='''module factory
implicit none
integer,parameter::dp=kind(1.0d0)
contains
pure function values(n) result(r)
integer,intent(in)::n
real(kind=dp),allocatable::r(:)
allocate(r(2))
r=real(n,dp)
end function
pure real(kind=dp) function total(n)
integer,intent(in)::n
total=sum(values(n))
end function
end module
program main
use factory,only:dp,total
implicit none
integer::i
real(kind=dp)::y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
end
'''
        self.build_run(source,'2.0 18000.0')
        implicit_result=source.replace('function values(n) result(r)','function values(n)').replace('allocatable::r(:)','allocatable::values(:)').replace('allocate(r(2))','allocate(values(2))').replace('r=real(n,dp)','values=real(n,dp)')
        self.build_run(implicit_result,'2.0 18000.0')

    def test_pure_alloc_workspace_example(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_allocatable_workspace.f90').read_text()
        self.build_run(source,'16.0 16.0')
        self.build_run(source,'16.0 16.0',parallel=False,threads=(1,))
        self.build_run(source,'16.0 16.0',checks=False)

    def test_pure_alloc_workspace_changing_shape_and_early_return(self):
        self.build_run('''implicit none
integer::i,j,n,expected,y(9000)
do concurrent(i=1:9000)
y(i)=total(i-4*(i/4))
end do
do j=1,9000
n=j-4*(j/4)
expected=3*n
if(n>1) expected=expected+6
if(y(j)/=expected) stop 1
end do
print *,y(1:4),y(9000)
contains
simple integer function total(n)
integer,intent(in)::n
integer,allocatable::work(:)
if(.not.allocated(work)) allocate(work(-1:n-2))
work=3
total=sum(work)
if(n==0.or.n==1) return
if(allocated(array=work)) deallocate(work)
work=[1,2]
work=[work,3]
total=total+sum(work)
end function
end
''','3 12 15 0 0')

    def test_pure_alloc_workspace_nested_calls_and_sections(self):
        self.build_run('''implicit none
integer::i,n
real,allocatable::x(:,:),y(:)
n=5
allocate(x(n,9000),y(9000))
x=2.0
do concurrent(i=1:9000)
y(i)=outer(x(:,i))
end do
if(any(y/=24.0)) stop 1
print *,y(1),y(9000)
contains
pure real function outer(a)
real,intent(in)::a(:)
real,allocatable::work(:)
work=a*a
outer=inner(work(5:1:-2))
end function
pure real function inner(a)
real,intent(in)::a(:)
real,allocatable::work(:)
work=2*a
inner=sum(work)
end function
end
''','24.0 24.0')

    def test_pure_alloc_workspace_logical(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
contains
pure integer function total(n)
integer,intent(in)::n
logical,allocatable::work(:)
work=[n==1,.true.,.false.]
total=count(work)
end function
end
''','2 1')

    def test_pure_alloc_workspace_ranks_and_kinds(self):
        for rank in (1,2,3,15):
            source='''implicit none
integer,parameter::dp=kind(1.0d0)
integer::i
real(kind=dp)::y(9000)
do concurrent(i=1:9000)
y(i)=total(2,i)
end do
print *,y(1),y(9000)
contains
pure real(kind=dp) function total(n,k)
integer,intent(in)::n,k
real(kind=dp),allocatable::work(DIMS)
allocate(work(BOUNDS))
work=real(k,dp)
total=sum(work)
end function
end
'''.replace('DIMS',','.join([':']*rank)).replace('BOUNDS',','.join(['-1:n-2']+['2:2']*(rank-1)))
            with self.subTest(rank=rank): self.build_run(source,'2.0 18000.0')

    def test_pure_alloc_workspace_runtime_checks(self):
        for failure,body in (
            ('unallocated','if(skip) return\nallocate(work(n))\nwork=2'),
            ('uninitialized','allocate(work(n))\nif(.not.skip) work=2'),
            ('outside','allocate(work(n))\nwork=2\nwork(n+1)=3')):
            # The early-return case reads the unallocated local before leaving.
            if failure=='unallocated': body=body.replace('if(skip) return','if(skip) then\ntotal=sum(work)\nreturn\nend if')
            source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(2,i==9000)
end do
print *,y(1)
contains
pure integer function total(n,skip)
integer,intent(in)::n
logical,intent(in)::skip
integer,allocatable::work(:)
BODY
total=sum(work)
end function
end
'''.replace('BODY',body)
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(failure,result.stderr.lower())

    def test_pure_alloc_workspace_target_fallback(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(i)
end do
print *,y(1),y(9000)
contains
pure integer function total(n)
integer,intent(in)::n
integer,allocatable,target::work(:)
allocate(work(2))
work=n
total=sum(work)
end function
end
''','2 18000',reason='unsupported or saved storage')

    def test_pure_allocation_options_example(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_allocation_options.f90').read_text()
        self.build_run(source,'4 6 8 2 2')
        self.build_run(source,'4 6 8 2 2',parallel=False,threads=(1,))
        self.build_run(source,'4 6 8 2 2',checks=False)

    def test_pure_allocation_options_source_bounds(self):
        self.build_run('''implicit none
integer::i,a(2,2),y(9000)
a=3
do concurrent(i=1:9000)
y(i)=total(a,2)
end do
print *,y(1),y(9000)
contains
pure integer function total(a,n)
integer,intent(in)::a(:,:),n
integer,allocatable::work(:,:)
integer::istat
total=-1
allocate(work(-1:n-2,2:3),source=a,stat=istat)
if(istat/=0) return
total=sum(work)+lbound(work,1)+lbound(work,2)
end function
end
''','13 13')

    def test_pure_allocation_options_mold_undefined_input(self):
        source='''implicit none
integer::i,a(2),y(9000)
do concurrent(i=1:9000)
y(i)=total(a)
end do
print *,y(1),y(9000)
contains
pure integer function total(a)
integer,intent(in)::a(:)
integer,allocatable::work(:)
integer::istat
total=-1
allocate(work,mold=a,stat=istat)
if(istat/=0) return
work=3
total=sum(work)
end function
end
'''
        self.build_run(source,'6 6')
        self.build_run(source.replace('mold=a','mold=a+a'),'6 6')

    def test_pure_allocation_options_source_expression_and_nested_result(self):
        source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=sum(values(i))
end do
print *,y(1),y(9000)
contains
pure function values(n) result(r)
integer,intent(in)::n
integer,allocatable::r(:)
integer::istat
allocate(r,source=[n,2*n],stat=istat)
end function
end
'''
        self.build_run(source,'3 27000')
        callback=source.replace('source=[n,2*n]','source=pair(n)').replace('end function\nend\n','''end function
pure function pair(n) result(a)
integer,intent(in)::n
integer::a(2)
a=[n,2*n]
end function
end
''')
        self.build_run(callback,'3 27000')

    def test_pure_allocation_options_logical_and_real(self):
        for kind,fill,read,expected in (
            ('logical','.true.','count(work)','2 2'),
            ('real','2.5','int(sum(work))','5 5')):
            source='''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total()
end do
print *,y(1),y(9000)
contains
pure integer function total()
KIND,allocatable::work(:)
integer::istat
allocate(work(2),source=FILL,stat=istat)
total=READ
end function
end
'''.replace('KIND',kind).replace('FILL',fill).replace('READ',read)
            with self.subTest(kind=kind): self.build_run(source,expected)

    def test_pure_allocation_options_source_allocation_state(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total()
end do
print *,y(1),y(9000)
contains
pure integer function total()
logical,allocatable::work(:)
integer::istat
allocate(work(2),source=allocated(work),stat=istat)
total=count(work)
end function
end
''','0 0')

    def test_pure_allocation_options_value_status_fallback(self):
        self.build_run('''implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=total(0)
end do
print *,y(1),y(9000)
contains
pure integer function total(istat)
integer,value::istat
integer,allocatable::work(:)
allocate(work(2),stat=istat)
work=3
total=sum(work)
end function
end
''','6 6',reason='STAT requires a local INTEGER variable')

    def test_pure_allocation_options_runtime_checks(self):
        for failure,allocation in (
            ('uninitialized','allocate(work,mold=a,stat=istat)'),
            ('shape','allocate(work(3),source=a,stat=istat)')):
            source='''implicit none
integer::i,a(2),y(9000)
a=2
do concurrent(i=1:9000)
y(i)=total(a)
end do
print *,y(1)
contains
pure integer function total(a)
integer,intent(in)::a(:)
integer,allocatable::work(:)
integer::istat
ALLOCATION
total=sum(work)
end function
end
'''.replace('ALLOCATION',allocation)
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1'),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn(failure,result.stderr.lower())

    def test_pure_workspace_fixed_scalar_input(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=f(i)
end do
print *,y(1),y(9000)
contains
pure integer function f(n)
integer,intent(in)::n
integer::work(3),j
do j=1,3
work(j)=n
end do
f=sum(work)
end function
end
""",'3 27000')

    def test_pure_workspace_automatic_scalar_input(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=f(i/3000)
end do
print *,y(1),y(9000)
contains
simple integer function f(n)
integer,intent(in)::n
integer::work(n)
work=2
f=sum(work)
end function
end
""",'0 6')

    def test_pure_workspace_early_return_and_reallocation(self):
        self.build_run("""implicit none
integer::i,n,pass
real,allocatable::x(:,:),y(:)
allocate(y(9000))
do pass=1,3
n=pass+1
allocate(x(n,9000))
x=2.0
x(1,1)=-2.0
do concurrent(i=1:9000)
y(i)=f(x(:,i))
end do
if(y(1)/=-4*n .or. any(y(2:)/=4*n)) stop 1
deallocate(x)
end do
print *,y(1),y(9000)
contains
pure real function f(a)
real,intent(in)::a(:)
real::work(size(a))
work=a*a
if(a(1)<0.0) then
f=-sum(work)
return
end if
f=sum(work)
end function
end
""",'-16.0 16.0')

    def test_pure_workspace_return_before_assignment(self):
        self.build_run("""implicit none
integer::i,n,y(9000)
n=4
do concurrent(i=1:9000)
y(i)=f(n,i==1)
end do
print *,y(1),y(9000)
contains
pure integer function f(n,skip)
integer,intent(in)::n
logical,intent(in)::skip
integer::work(n)
f=0
if(skip) return
work=2
f=sum(work)
end function
end
""",'0 8')

    def test_pure_workspace_logical_and_sections(self):
        self.build_run("""implicit none
integer::i,n,y(9000)
n=6
do concurrent(i=1:9000)
y(i)=f(n)
end do
print *,y(1),y(9000)
contains
simple integer function f(n)
integer,intent(in)::n
logical::work(n)
work=.false.
work(1:n:2)=.true.
f=count(work)
end function
end
""",'3 3')

    def test_pure_workspace_multidimensional(self):
        self.build_run("""use iso_fortran_env,only:real64
implicit none
integer::i,n
real(real64)::y(9000)
n=3
do concurrent(i=1:9000)
y(i)=f(n)
end do
print *,y(1),y(9000)
contains
pure real(real64) function f(n)
integer,intent(in)::n
real(real64)::work(n,2)
work=2.0_real64
f=sum(work)
end function
end
""",'12.0 12.0')

    def test_pure_workspace_nondefault_bounds(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=f(i/3000)
end do
print *,y(1),y(9000)
contains
pure integer function f(n)
integer,intent(in)::n
integer::work(-2:n),fixed(-2:2)
work=2
fixed=-1
f=sum(work)+sum(fixed)
end function
end
""",'1 7')

    def test_pure_workspace_value_argument(self):
        source="""implicit none
integer::i,n,y(9000)
n=3
do concurrent(i=1:9000)
y(i)=f(n)
end do
print *,n,y(1),y(9000)
contains
pure integer function f(n)
integer,value::n
integer::work(n)
work=n
n=n+1
f=sum(work)+n
end function
end
"""
        self.build_run(source,'3 13 13')
        self.build_run(source,'3 13 13',parallel=False,threads=(1,))

    def test_pure_workspace_scalar_call_chain(self):
        self.build_run("""implicit none
integer::i,n,y(9000)
n=3
do concurrent(i=1:9000)
y(i)=outer(n)
end do
print *,y(1),y(9000)
contains
pure integer function outer(n)
integer,intent(in)::n
outer=middle(n)+1
end function
simple integer function middle(n)
integer,intent(in)::n
middle=inner(n)+1
end function
simple integer function inner(n)
integer,intent(in)::n
integer::work(n)
work=2
inner=sum(work)
end function
end
""",'8 8')

    def test_pure_workspace_separate_compilation(self):
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-objects-') as tmp:
            directory=Path(tmp)
            source=directory/'workspace.f90'
            source.write_text('module workspace_mod\nimplicit none\ncontains\npure integer function f(n)\ninteger,intent(in)::n\ninteger::work(3)\nwork=n\nf=sum(work)\nend function\nend module\n')
            main=directory/'main.f90'
            main.write_text('use workspace_mod\nimplicit none\nprint *,f(3)\nend\n')
            obj=directory/'workspace.obj';exe=directory/'case.exe'
            for optimization in ('0','2'):
                for command in ([str(COMPILER),'-O',optimization,'--parallel','-J',str(directory),'-c',str(source),'-o',str(obj)],
                                [str(COMPILER),'-O',optimization,'-I',str(directory),str(main),str(obj),'-o',str(exe)]):
                    result=subprocess.run(command,cwd=directory,capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                result=subprocess.run([str(exe)],capture_output=True,text=True,timeout=30)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(result.stdout.split(),['9'])

    def test_pure_workspace_runtime_checks(self):
        for layout,failure in ((layout,failure) for layout in ('fixed','automatic') for failure in ('uninitialized','bounds')):
            source="""implicit none
integer::i,n,y(9000)
n=3
do concurrent(i=1:9000)
y(i)=f(n,i==9000)
end do
print *,y(1)
contains
pure integer function f(n,skip)
integer,intent(in)::n
logical,intent(in)::skip
integer::work(n)
if(.not.skip) work=2
f=RESULT
end function
end
""".replace('RESULT','sum(work)' if failure=='uninitialized' else 'work(n+1)')
            if layout=='fixed': source=source.replace('work(n)','work(3)')
            if failure=='bounds': source=source.replace('if(.not.skip) work=2','work=2')
            with self.subTest(layout=layout,failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1')
                    result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn('uninitialized' if failure=='uninitialized' else 'outside',result.stderr.lower())

    def test_pure_workspace_local_allocatable_explicit(self):
        self.build_run("""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=f(i)
end do
print *,y(1),y(9000)
contains
pure integer function f(n)
integer,intent(in)::n
integer,allocatable::work(:)
allocate(work(2))
work=n
f=sum(work)
end function
end
""",'2 18000')

    def test_pure_workspace_recursion_fallback(self):
        source="""implicit none
integer::i,y(9000)
do concurrent(i=1:9000)
y(i)=f(min(i,3))
end do
print *,y(1),y(9000)
contains
recursive pure integer function f(n) result(r)
integer,intent(in)::n
integer::work(2)
work=n
r=sum(work)
if(n>1) r=r+f(n-1)
end function
end
"""
        self.build_run(source,'2 12',reason='not a proven PURE function')
        self.build_run(source,'2 12',parallel=False,threads=(1,))

    def test_pure_array_columns_no_checks(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_array_functions.f90').read_text(), '20.0 20.0 8.0 8.0',checks=False)

    def test_pure_array_rank_fifteen(self):
        dims=','.join(['1']*15)
        assumed=','.join([':']*15)
        self.build_run(f"""implicit none
integer::i,x({dims}),y(9000)
x=3
do concurrent(i=1:9000)
y(i)=total(x)
end do
print *,y(1),y(9000)
contains
pure integer function total(a)
integer,intent(in)::a({assumed})
total=sum(a)
end function
end
""",'3 3')

    def test_pure_array_runtime_sections(self):
        self.build_run("""implicit none
integer::i,n
real,allocatable::x(:,:),y(:)
n=5
allocate(x(-2:n,9000),y(9000))
x=2.0
do concurrent(i=1:9000)
y(i)=energy(x(n:-2:-2,i))
end do
print *,y(1),y(9000)
contains
pure real function energy(x) result(r)
real,intent(in)::x(:)
r=sum(x*x)+dot_product(x,x)
end function
end
""", '32.0 32.0')

    def test_pure_array_loop_and_chain(self):
        self.build_run("""implicit none
integer::i,n
integer,allocatable::x(:,:),y(:)
n=4
allocate(x(n,9000),y(9000))
x=3
do concurrent(i=1:9000)
y(i)=outer(x(:,i))
end do
print *,y(1),y(9000)
contains
pure integer function outer(a)
integer,intent(in)::a(:)
outer=total(a)+size(a)
end function
simple integer function total(a) result(r)
integer,intent(in)::a(:)
integer::j
r=0
do j=lbound(a,1),ubound(a,1)
r=r+a(j)
end do
end function
end
""", '16 16')

    def test_pure_array_logical(self):
        self.build_run("""implicit none
integer::i,n,y(9000)
logical,allocatable::x(:,:)
n=3
allocate(x(n,9000))
x=.true.
do concurrent(i=1:9000)
y(i)=score(a=x(:,i))
end do
print *,y(1),y(9000)
contains
pure integer function score(a)
logical,intent(in)::a(:)
score=count(a)
if(all(a)) score=score+1
if(any(.not.a)) score=-1
end function
end
""", '4 4')

    def test_pure_array_function_in_mask(self):
        self.build_run("""implicit none
integer::i,y(9000),total
logical::x(2,9000)
x=.true.
x(2,1)=.false.
y=0
total=0
do concurrent(i=1:9000,accept(x(:,i))) reduce(+:total)
y(i)=i
total=total+1
end do
print *,y(1),y(9000),total
contains
pure logical function accept(a)
logical,intent(in)::a(:)
accept=all(a)
end function
end
""", '0 9000 8999')

    def test_pure_array_rank_two_and_kinds(self):
        self.build_run("""use iso_fortran_env,only:real64,int64
implicit none
integer::i,n
real(real64),allocatable::x(:,:,:),y(:)
n=2
allocate(x(n,3,9000),y(9000))
x=2.0_real64
do concurrent(i=1:9000)
y(i)=total(x(:,:,i))
end do
print *,y(1),y(9000)
contains
pure real(real64) function total(a) result(r)
real(real64),intent(in)::a(:,:)
integer(int64)::j,k
r=0.0_real64
do k=1,size(a,2)
do j=1,size(a,1)
r=r+a(j,k)
end do
end do
end function
end
""", '12.0 12.0')

    def test_pure_array_runtime_reallocate(self):
        self.build_run("""implicit none
integer::i,n,pass
integer,allocatable::x(:,:),y(:)
allocate(y(9000))
do pass=1,2
n=2*pass
allocate(x(n,9000))
x=3
do concurrent(i=1:9000)
y(i)=total(x(:,i))
end do
if(any(y/=3*n)) stop 1
deallocate(x)
end do
print *,y(1),y(9000)
contains
pure integer function total(a)
integer,intent(in)::a(:)
total=sum(a)
end function
end
""", '12 12')

    def test_pure_array_empty_and_minmax_product(self):
        self.build_run("""implicit none
integer::i,n,y(9000)
integer,allocatable::x(:,:)
n=0
allocate(x(n,9000))
do concurrent(i=1:9000)
y(i)=total(x(:,i))
end do
if(any(y/=1)) stop 1
deallocate(x)
n=3
allocate(x(n,9000))
x=2
do concurrent(i=1:9000)
y(i)=total(x(:,i))
end do
print *,y(1),y(9000)
contains
pure integer function total(a)
integer,intent(in)::a(:)
total=sum(a)+product(a)
if(size(a)>0) total=total+minval(a)+maxval(a)
end function
end
""", '18 18')

    def test_pure_array_target_dummy_fallback(self):
        self.build_run("""implicit none
integer::i,y(9000),x(2)
x=3
do concurrent(i=1:9000)
y(i)=total(x)
end do
print *,y(1),y(9000)
contains
pure integer function total(a)
integer,intent(in),target::a(:)
total=sum(a)
end function
end
""", '6 6',reason='unsupported or saved storage')

    def test_pure_array_select_case(self):
        self.build_run("""implicit none
integer::i,x(3,9000),y(9000)
x=2
x(1,1)=0
do concurrent(i=1:9000)
y(i)=f(x(:,i))
end do
print *,y(1),y(9000)
contains
pure integer function f(a) result(r)
integer,intent(in)::a(:)
select case(a(1))
case(0)
r=0
case(2)
r=sum(a*a)+a(1)
case default
r=-1
end select
end function
end
""",'0 14')

    def test_pure_array_changing_section_extent(self):
        self.build_run("""implicit none
integer::i,n
real,allocatable::x(:,:),y(:)
n=3
allocate(x(n,9000),y(9000))
x=2.0
do concurrent(i=1:9000)
y(i)=f(x(1:i/3000,i))
end do
print *,y(1),y(9000)
contains
pure real function f(a)
real,intent(in)::a(:)
f=sum(a)
end function
end
""",'0.0 6.0')

    def test_pure_array_runtime_checks(self):
        for failure in ('uninitialized','bounds'):
            source="""implicit none
integer::i,j,n
real,allocatable::x(:,:),y(:)
n=3
allocate(x(n,9000),y(9000))
do j=1,9000
do i=1,n
if(INITIALIZE) x(i,j)=2.0
end do
end do
do concurrent(j=1:9000)
y(j)=f(x(:,j))
end do
print *,y(1)
contains
pure real function f(a)
real,intent(in)::a(:)
f=RESULT
end function
end
""".replace('INITIALIZE','i/=2' if failure=='uninitialized' else '.true.').replace('RESULT','sum(a)' if failure=='uninitialized' else 'a(size(a)+1)')
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
                path=Path(tmp)/'case.f90';path.write_text(source)
                exe=Path(tmp)/'case.exe'
                for optimization in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',optimization,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    env=dict(os.environ,RFORT_NUM_THREADS='4',RFORT_PARALLEL_TRACE='1')
                    result=subprocess.run([str(exe)],env=env,capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0,result.stdout)
                    self.assertIn('uninitialized' if failure=='uninitialized' else 'bound',result.stderr.lower())
                    if failure=='uninitialized': self.assertIn('case.f90:',result.stderr)

    def test_pure_array_read_of_written_storage_fallback(self):
        self.build_run("""implicit none
integer::i,x(9000)
x=1
do concurrent(i=1:9000)
x(i)=f(x)
end do
print *,x(1),x(9000)
contains
pure integer function f(a)
integer,intent(in)::a(:)
f=a(1)
end function
end
""", '1 1',reason='whole-array expressions require serial execution')

    def test_pure_call_impure_fallback(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
a(i)=f(i)
end do
print *,a(1),a(9000)
contains
integer function f(i)
integer,intent(in)::i
f=i+1
end function
end
""",'2 9001',reason='not PURE or SIMPLE')

    def test_parameter_extent(self):
        self.build_run("""implicit none
integer,parameter::n=9000
integer :: i,a(n)
do concurrent(i=1:n) shared(a) default(none)
a(i)=i
end do
print *,a(1),a(n)
end
""",'1 9000',opts=('2',))

    def test_permuted_output_fallback(self):
        self.build_run("""integer :: i,a(9000)
do concurrent(i=1:9000)
a(9001-i)=i
end do
print *,a(1),a(9000)
end
""",'9000 1',reason='independent array-element writes',opts=('2',))

    def test_parallel_runtime_checks(self):
        sources=[("""implicit none
integer :: i,t,a(9000)
do concurrent(i=1:9000) local(t) shared(a)
if(i<9000) t=i
a(i)=t
end do
print *,a(9000)
end
""",'uninitialized variable'),("""implicit none
integer :: i,n=9001,a(9000)
do concurrent(i=1:n) shared(a)
a(i)=i
end do
print *,a(1)
end
""",'subscript outside')]
        with tempfile.TemporaryDirectory(prefix='parallel-check-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            for source,message in sources:
                path.write_text(source)
                result=subprocess.run([str(COMPILER),'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('native parallel worker emitted',result.stderr)
                result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4'),capture_output=True,text=True,timeout=30)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(message,result.stderr)

    def test_allocatable_reallocation_and_reduction(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_allocatable.f90').read_text(),
                       '20007.0 2.0 2.0 20009.0 2.0 2.0 20011.0 2.0 2.0')

    def test_allocatable_without_checks(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_allocatable.f90').read_text(),
                       '20007.0 2.0 2.0 20009.0 2.0 2.0 20011.0 2.0 2.0', checks=False)

    def test_allocatable_types_and_lower_bounds(self):
        self.build_run("""implicit none
integer :: n,i
integer, allocatable :: a(:)
logical, allocatable :: b(:)
real(kind=8), allocatable :: x(:)
n=9000
allocate(a(-7:n),b(-7:n),x(-7:n))
do concurrent(i=lbound(a,1):ubound(a,1)) shared(a,b,x)
a(i)=i
b(i)=i>0
x(i)=dble(i)*0.5d0
end do
print *,a(-7),a(n),b(-7),b(n),x(-7),x(n)
end
""",'-7 9000 F T -3.5 4500.0')

    def test_allocatable_mask_and_readonly_reverse(self):
        self.build_run("""implicit none
integer :: n,i
real,allocatable :: x(:),y(:)
n=9000
allocate(x(n),y(n))
x=4.0
y=-1.0
do concurrent(i=1:n,i>2) shared(x,y,n) default(none)
y(i)=sqrt(x(n+1-i))
end do
print *,y(1),y(3),y(n)
end
""",'-1.0 2.0 2.0')

    def test_allocatable_empty_and_small(self):
        self.build_run("""implicit none
integer :: n,i
integer,allocatable :: a(:)
n=0
allocate(a(n))
do concurrent(i=1:n)
a(i)=i
end do
deallocate(a)
n=3
allocate(a(n))
do concurrent(i=1:n)
a(i)=i
end do
print *,a
end
""",'1 2 3',threads=(1,),opts=('2',))

    def test_allocatable_target_fallback(self):
        self.build_run("""implicit none
integer :: n,i
integer,allocatable,target :: a(:)
n=9000
allocate(a(n))
do concurrent(i=1:n)
a(i)=i
end do
print *,a(1),a(n)
end
""",'1 9000',reason='unsupported attributes',opts=('2',))

    def test_allocatable_runtime_checks(self):
        template="""implicit none
integer :: n,i
real,allocatable :: x(:),y(:)
n=9000
allocate(x(n),y(n))
INITIALIZE
do concurrent(i=1:LAST) shared(x,y)
y(i)=x(i)
end do
print *,y(n)
end
"""
        with tempfile.TemporaryDirectory(prefix='parallel-alloc-check-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            for initialization,last,message in [('x=1.0','n+1','bounds'),
                                                 ('x(1:n-1)=1.0','n','uninitialized')]:
                path.write_text(template.replace('INITIALIZE',initialization).replace('LAST',last))
                for opt in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    for n in (1,2,4):
                        result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(n)),capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(result.returncode,0)
                        self.assertIn(message,result.stderr)

    def test_assumed_shape_procedure(self):
        self.build_run((ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text(),'2.0 2.0')

    def test_procedure_reduction(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('integer :: n','integer :: n\nreal :: total')
        source=source.replace('call transform(x,y)','total=7.0\ncall transform(x,y,total)')
        source=source.replace('print *,y(1),y(n)','print *,y(1),y(n),total')
        source=source.replace('subroutine transform(x,y)','subroutine transform(x,y,total)')
        source=source.replace('integer :: i','real,intent(inout)::total\ninteger :: i')
        source=source.replace('shared(x,y) default(none)','shared(x,y) reduce(+:total) default(none)')
        source=source.replace('y(i)=sqrt(x(i))','y(i)=sqrt(x(i))\n   total=total+y(i)')
        self.build_run(source,'2.0 2.0 20007.0')

    def test_procedure_host_state_fallback(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('integer :: n','integer :: n\nreal :: factor')
        source=source.replace('x=4.0','x=4.0\nfactor=2.0')
        source=source.replace('sqrt(x(i))','factor*sqrt(x(i))').replace(' default(none)','')
        self.build_run(source,'4.0 4.0',reason='host or module state',opts=('2',))

    def test_procedure_fixed_actual_and_locality(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('real, allocatable :: x(:),y(:)','real :: x(10000),y(10000)').replace('allocate(x(n),y(n))','')
        source=source.replace('integer :: i','integer :: i\nreal :: t')
        source=source.replace('shared(x,y) default(none)','shared(x,y) local(t) default(none)')
        source=source.replace('y(i)=sqrt(x(i))','t=sqrt(x(i))\n   y(i)=t+1.0')
        self.build_run(source,'3.0 3.0')

    def test_procedure_strided_actuals(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('allocate(x(n),y(n))','allocate(x(2*n),y(2*n))\ny=-1.0')
        source=source.replace('call transform(x,y)','call transform(x(1:2*n:2),y(1:2*n:2))')
        source=source.replace('print *,y(1),y(n)','print *,y(1),y(2),y(2*n-1),y(2*n)')
        self.build_run(source,'2.0 -1.0 2.0 -1.0')

    def test_procedure_inout_mask(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('x=4.0','x=4.0\ny=-1.0').replace('intent(out)','intent(inout)')
        source=source.replace('i=1:size(x)','i=1:size(x),i>2')
        self.build_run(source,'-1.0 2.0')

    def test_procedure_empty_and_small(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('n=10000','n=0').replace('print *,y(1),y(n)','print *,size(y)')
        self.build_run(source,'0',threads=(1,),opts=('2',))
        source=source.replace('n=0','n=3')
        self.build_run(source,'3',threads=(1,),opts=('2',))

    def test_procedure_pointer_dummy_fallback(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('allocatable :: x(:),y(:)','pointer :: x(:),y(:)')
        source=source.replace('real,intent(in)','real,pointer,intent(inout)').replace('real,intent(out)','real,pointer,intent(inout)')
        self.build_run(source,'2.0 2.0',reason='unsupported attributes',opts=('2',))

    def test_procedure_runtime_checks(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        with tempfile.TemporaryDirectory(prefix='parallel-dummy-check-') as tmp:
            path=Path(tmp)/'case.f90';exe=Path(tmp)/'case.exe'
            for text,message in [(source.replace('y(n))','y(n-1))'),'bounds'),
                                 (source.replace('x=4.0','x(1:n-1)=4.0'),'uninitialized')]:
                path.write_text(text)
                for opt in ('0','2'):
                    result=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertIn('native parallel worker emitted',result.stderr)
                    self.assertNotIn('serial fallback',result.stderr)
                    result=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS='4'),capture_output=True,text=True,timeout=30)
                    self.assertNotEqual(result.returncode,0)
                    self.assertIn(message,result.stderr)

    def test_module_procedure_repeated_reduction(self):
        self.build_run("""module kernels
contains
subroutine accumulate(x,total)
real,intent(in),contiguous :: x(:)
real,intent(inout) :: total
integer :: i
do concurrent(i=1:size(x)) shared(x) reduce(+:total) default(none)
total=total+x(i)
end do
end subroutine
end module
program demo
use kernels
implicit none
real,allocatable :: x(:)
real :: total
integer :: n
n=9000
allocate(x(n))
x=0.5
total=7.0
call accumulate(x,total)
call accumulate(x,total)
print *,total
end program
""",'9007.0')

    def test_function_reduction(self):
        self.build_run("""program demo
implicit none
real,allocatable :: x(:)
integer :: n
n=9000
allocate(x(n))
x=0.5
print *,sum_values(x)
contains
function sum_values(x) result(s)
real,intent(in) :: x(:)
real :: s
integer :: i
s=7.0
do concurrent(i=1:size(x)) shared(x) reduce(+:s) default(none)
s=s+x(i)
end do
end function
end program
""",'4507.0')

    def test_procedure_saved_local_fallback(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('integer :: i','integer :: i\nreal,save :: factor=2.0')
        source=source.replace('sqrt(x(i))','factor*sqrt(x(i))').replace(' default(none)','')
        self.build_run(source,'4.0 4.0',reason='SAVE storage',opts=('2',))

    def test_procedure_rebased_actual_bounds(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_dummy.f90').read_text()
        source=source.replace('allocate(x(n),y(n))','allocate(x(-7:n-8),y(-7:n-8))')
        source=source.replace('print *,y(1),y(n)','print *,y(-7),y(n-8)')
        self.build_run(source,'2.0 2.0')

    def test_interpreter_option_rejected(self):
        result=subprocess.run([str(COMPILER),'--parallel','run',str(ROOT/'tests/programs/xconcurrent_parallel.f90')],capture_output=True,text=True,timeout=30)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('interpreter remains serial',result.stderr)

if __name__=='__main__': unittest.main()

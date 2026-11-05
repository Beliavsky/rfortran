"""Native parallel DO CONCURRENT and conservative serial fallback."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT

class ParallelConcurrent(unittest.TestCase):
    def build_run(self, source, expected, parallel=True, reason=None, threads=(1,2,4), opts=('0','2'), checks=True):
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
                    if parallel and reason is None and n>1:
                        self.assertIn(f'rfort parallel: {n} threads',result.stderr)
                    else: self.assertNotIn('rfort parallel:',result.stderr)

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

    def test_strided_fallback(self):
        self.build_run('''integer :: i,a(6)
a=0
do concurrent(i=1:6:2)
a(i)=i
end do
print *,a
end
''','1 0 3 0 5 0',reason='unit stride',opts=('2',))

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

    def test_array_reduction_fallback(self):
        self.build_run("""integer :: i,s(2)
s=0
do concurrent(i=1:10000) reduce(+:s)
s=s+[i,1]
end do
print *,s
end
""",'50005000 10000',reason='parallel array REDUCE',opts=('2',))

    def test_procedure_fallback(self):
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

    def test_unknown_call_fallback(self):
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
''','2 9001',reason='outside the parallel subset',opts=('2',))

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

    def test_interpreter_option_rejected(self):
        result=subprocess.run([str(COMPILER),'--parallel','run',str(ROOT/'tests/programs/xconcurrent_parallel.f90')],capture_output=True,text=True,timeout=30)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('interpreter remains serial',result.stderr)

if __name__=='__main__': unittest.main()

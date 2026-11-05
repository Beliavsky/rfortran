"""Native parallel DO CONCURRENT and conservative serial fallback."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_record_read import COMPILER, ROOT

class ParallelConcurrent(unittest.TestCase):
    def build_run(self, source, expected, parallel=True, reason=None, threads=(1,2,4), opts=('0','2')):
        with tempfile.TemporaryDirectory(prefix='rfort-parallel-') as tmp:
            directory=Path(tmp); path=directory/'case.f90';path.write_text(source)
            for optimization in opts:
                exe=directory/'case.exe'
                command=[str(COMPILER),'-O',optimization,'--parallel-report']
                if parallel: command+=['--parallel']
                result=subprocess.run(command+[str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                self.assertEqual(result.returncode,0,result.stderr)
                if parallel:
                    self.assertIn(reason or 'native parallel worker emitted',result.stderr)
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

    def test_reduction_fallback(self):
        self.build_run('''integer :: i,s=0
do concurrent(i=1:10000) reduce(+:s)
s=s+i
end do
print *,s
end
''','50005000',reason='parallel REDUCE',opts=('2',))

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

    def test_interpreter_option_rejected(self):
        result=subprocess.run([str(COMPILER),'--parallel','run',str(ROOT/'tests/programs/xconcurrent_parallel.f90')],capture_output=True,text=True,timeout=30)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('interpreter remains serial',result.stderr)

if __name__=='__main__': unittest.main()

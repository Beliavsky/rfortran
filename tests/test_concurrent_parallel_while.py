"""Worker-local WHILE loops, transfers, conditions and conservative fallbacks."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_captures
import test_concurrent_parallel_inner_do
import test_concurrent_parallel
import test_fortran2023


class ParallelWhile(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    source = staticmethod(test_concurrent_parallel_inner_do.InnerDo.source)

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_while.f90').read_text())

    def test_real_convergence_loop(self):
        self.check_source('''implicit none
integer::i
real(kind=8)::x(9001),y(9001),t,tolerance
x=4.0d0
tolerance=1.0d-12
do concurrent(i=1:9001) local(t)
 t=1.0d0
 do while(abs(t*t-x(i))>tolerance)
  t=0.5d0*(t+x(i)/t)
 end do
 y(i)=t
end do
if(any(abs(y-2.0d0)>tolerance)) stop 1
print *,"PASS"
end
''')

    def test_zero_iterations_and_rechecked_condition(self):
        for initial,expected in [(0,10), (4,4), (7,7)]:
            with self.subTest(initial=initial):
                self.check_source(self.source(f'''j={initial}
total=j
do while(j<4)
 j=j+1
 total=total+j
end do
a(i)=total''', 'local(j,total)', str(expected)))

    def test_nested_counted_and_while_loops(self):
        for outer,inner in [('do j=1,3', 'do while(k<5)'),
                            ('do j=3,1,stride', 'do while(k<5)')]:
            with self.subTest(outer=outer):
                source=self.source(f'''total=0
{outer}
 k=0
 {inner}
  k=k+1
  if(k==2) cycle
  if(k==4) exit
  total=total+k*i
 end do
end do
a(i)=total''', 'local(j,k,total)', '12*i' if outer=='do j=1,3' else '8*i')
                self.check_source(source.replace('i,j,total,','i,j,k,total,'))

    def test_while_around_counted_loop_and_outer_cycle(self):
        source=self.source('''j=0
total=0
do while(j<3)
 j=j+1
 if(j==2) cycle
 do k=5,1,stride
  if(k==3) cycle
  if(k==1) exit
  total=total+k*i
 end do
end do
if(mod(i,2)==0) then
 a(i)=0
 cycle
end if
a(i)=total''', 'local(j,k,total)', '10*i*mod(i,2)')
        self.check_source(source.replace('i,j,total,','i,j,k,total,'))

    def test_nested_while_transfer_targets(self):
        source=self.source('''j=0
total=0
do while(j<3)
 j=j+1
 k=0
 do while(k<4)
  k=k+1
  if(k==2) cycle
  if(k==4) exit
  total=total+i
 end do
 if(j==2) cycle
 total=total+10*i
end do
a(i)=total''', 'local(j,k,total)', '26*i')
        self.check_source(source.replace('i,j,total,','i,j,k,total,'))

    def test_pure_condition_and_local_init(self):
        source=self.source('''do while(keep_going(j))
 j=j+1
end do
a(i)=j''', 'local_init(j)', '4')
        source=source.replace('stride=-2','stride=-2\nj=0')
        source=source.replace('print *,"PASS"', 'if(j/=0) stop 2\nprint *,"PASS"')
        source=source.replace('\nend\n','''
contains
pure logical function keep_going(j)
integer,intent(in)::j
keep_going=j<4
end function
end
''')
        self.check_source(source)

    def test_runtime_workspace_reduction_and_early_exit(self):
        self.check_source('''implicit none
call compute(5)
print *,"PASS"
contains
subroutine compute(n)
integer,intent(in)::n
integer::i,j,work(n),a(9001)
integer(kind=8)::s
work=7
s=11
do concurrent(i=1:9001) local(j) local_init(work) reduce(+:s)
 j=0
 do while(j<n)
  j=j+1
  if(j==2) cycle
  if(j==4) exit
  work(j)=j*i
  s=s+i
 end do
 a(i)=sum(work)
end do
if(s/=11_8+9001_8*9002) stop 1
if(any(work/=7)) stop 2
do i=1,9001
 if(a(i)/=4*i+21) stop 3
end do
end subroutine
end
''')

    def test_uninitialized_local_after_empty_while(self):
        source=self.source('''j=0
do while(j<mod(i,2))
 j=j+1
 total=i
end do
a(i)=total''', 'local(j,total)', 'i')
        self.check_source(source, runtime_error='uninitialized')

    def test_enclosing_counted_index_still_protected(self):
        source=self.source('''total=0
do j=1,3
 do while(total==0)
  j=3
  total=1
 end do
end do
a(i)=total''', 'local(j,total)', '1')
        with tempfile.TemporaryDirectory(prefix='rfort-while-index-') as tmp:
            path=Path(tmp)/'case.f90'
            path.write_text(source)
            for opt in ('0','2'):
                run=subprocess.run([str(COMPILER),'-O',opt,'--parallel',str(path),
                    '-o',str(Path(tmp)/'case.exe')],capture_output=True,text=True,timeout=90)
                self.assertNotEqual(run.returncode,0)
                self.assertIn('cannot be redefined inside the loop',run.stderr)

    def test_impure_condition_falls_back(self):
        source=self.source('''j=0
do while(keep_going(j))
 j=j+1
end do
a(i)=j''', 'local(j)', '2')
        source=source.replace('\nend\n','''
contains
logical function keep_going(j)
integer,intent(in)::j
keep_going=j<2
end function
end
''')
        self.check_source(source, reason='serial fallback')

    def test_runtime_array_result_in_condition(self):
        self.check_source('''implicit none
integer::i,j,n,a(9001)
n=3
do concurrent(i=1:9001) local(j)
 j=0
 do while(sum(values(n,j))<3*n)
  j=j+1
  if(j==2) cycle
 end do
 a(i)=j
end do
if(any(a/=3)) stop 1
print *,"PASS"
contains
pure function values(n,k) result(r)
integer,intent(in)::n,k
integer::r(n)
r=k
end function
end
''')

    def test_shared_writes_still_checked(self):
        self.check_source(self.source('''j=0
do while(j<1)
 a(i+j)=i
 j=j+1
end do''','local(j)','i'),reason='independent array-element writes require')

    def test_nocheck_and_serial_modes(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_while.f90').read_text()
        test_concurrent_parallel.ParallelConcurrent.build_run(self,source,'PASS',checks=False)
        test_fortran2023.Fortran2023.check(self,source.replace('9001','9').replace('9002','10'),' PASS\n')


if __name__=='__main__':
    unittest.main()

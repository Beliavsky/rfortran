"""Unnamed EXIT/CYCLE must stay within the worker's innermost serial DO."""
import subprocess
import tempfile
from pathlib import Path
import unittest
from test_record_read import COMPILER, ROOT
import test_concurrent_parallel_captures
import test_concurrent_parallel_inner_do


class InnerTransfers(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    source = staticmethod(test_concurrent_parallel_inner_do.InnerDo.source)

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_inner_transfers.f90').read_text())

    def test_cycle_with_unit_stride(self):
        self.check_source(self.source('''total=0
do j=1,4
 if(j==2) cycle
 total=total+j*i
end do
a(i)=total''', 'local(j,total)', '8*i'))

    def test_explicit_signed_and_runtime_strides(self):
        for start,end,step,skip,stop,expected in [
            (1,9,'2',3,7,6), (9,1,'-2',7,3,14),
            (9,1,'stride',7,3,14), (1,9,'-stride',3,7,6),
        ]:
            with self.subTest(step=step):
                self.check_source(self.source(f'''total=0
do j={start},{end},{step}
 if(j=={skip}) cycle
 if(j=={stop}) exit
 total=total+j*i
end do
a(i)=total''', 'local(j,total)', f'{expected}*i'))

    def test_nested_loop_targets(self):
        for outer,inner,expected in [
            ('1,3','1,5',208), ('1,3,1','1,5',208),
            ('1,3,1','1,5,1',208), ('1,3,1','5,1,stride',200),
        ]:
            with self.subTest(outer=outer,inner=inner):
                body=f'''total=0
do j={outer}
 if(j==2) cycle
 do k={inner}
  if(k==2.or.k==5) cycle
  if(k==4) exit
  total=total+k*i
 end do
 total=total+100*i
end do
a(i)=total'''
                # Negative step skips 5, exits at 3, so contributes zero.
                if inner=='5,1,stride':
                    body=body.replace('if(k==4) exit','if(k==3) exit')
                source=self.source(body,'local(j,k,total)',f'{expected}*i')
                source=source.replace('i,j,total,a(9001)', 'i,j,k,total,a(9001)')
                self.check_source(source)

    def test_all_cycles_and_empty_loop(self):
        for control,final in [('1,3',4), ('3,1',3), ('3,1,-1',0)]:
            with self.subTest(control=control):
                self.check_source(self.source(f'''total=0
do j={control}
 cycle
 total=total+1
end do
a(i)=total+j''','local(j,total)',str(final)))

    def test_local_init_workspace_and_reduction(self):
        self.check_source('''implicit none
integer::i,a(9001)
call compute(5,a)
do i=1,size(a)
 if(a(i)/=4*i+21) stop 1
end do
print *,"PASS"
contains
subroutine compute(n,a)
integer,intent(in)::n
integer,intent(out)::a(:)
integer::i,j,work(n)
integer(kind=8)::s
work=7
s=0
do concurrent(i=1:size(a)) local(j) local_init(work) reduce(+:s)
 do j=1,n
  if(j==2) cycle
  if(j==4) exit
  work(j)=j*i
 end do
 a(i)=sum(work)
 s=s+sum(work)
end do
if(s/=2_8*9001*9002+21_8*9001) stop 2
if(any(work/=7)) stop 3
end subroutine
end
''')

    def test_exit_does_not_initialize_skipped_element(self):
        source=self.source('''work(1)=i
do j=2,2
 if(mod(i,2)==0) exit
 work(j)=i
end do
a(i)=sum(work)''', 'local(j,work)', '2*i')
        source=source.replace('stride\n', 'stride,work(2)\n')
        self.check_source(source, runtime_error='uninitialized')

    def test_concurrent_body_cycle_still_falls_back(self):
        self.check_source(self.source('''a(i)=i
cycle
a(i)=0''', 'local(j)', 'i'), reason='body requires')

    def test_exit_cannot_leave_concurrent_construct(self):
        source=self.source('''do j=1,2
 cycle
end do
exit
a(i)=i''', 'local(j)', 'i')
        with tempfile.TemporaryDirectory(prefix='rfort-inner-exit-') as tmp:
            path=Path(tmp)/'case.f90'
            path.write_text(source)
            for opt in ('0','2'):
                result=subprocess.run([str(COMPILER),'-O',opt,'--parallel',str(path),
                    '-o',str(Path(tmp)/'case.exe')], capture_output=True,text=True,timeout=90)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('EXIT cannot leave a DO CONCURRENT',result.stderr)


if __name__=='__main__':
    unittest.main()

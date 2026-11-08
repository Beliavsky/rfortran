"""SELECT CASE branches share the parallel independence and transfer proofs."""
import subprocess
import tempfile
from pathlib import Path
import unittest
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_captures
import test_concurrent_parallel_inner_do
import test_concurrent_parallel
import test_fortran2023


class ParallelSelectCase(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    source = staticmethod(test_concurrent_parallel_inner_do.InnerDo.source)

    def test_example(self):
        self.check_source((ROOT/'tests/programs/xconcurrent_parallel_select_case.f90').read_text())

    def test_integer_kinds_ranges_and_default(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind):
                source=self.source(f'''key=int(mod(i,7)-3,kind={kind})
select case(key)
case(-3,-2)
 a(i)=1
case(-1:1)
 a(i)=2
case default
 a(i)=3
end select''', 'local(key)', '1+merge(1,0,mod(i,7)>1)+merge(1,0,mod(i,7)>4)')
                source=source.replace('stride\n',f'stride\ninteger(kind={kind})::key\n')
                self.check_source(source)

    def test_open_ranges_and_named_constants(self):
        source=self.source('''select case(mod(i,7)-3)
case(:lo)
 a(i)=1
case(lo+1:hi)
 a(i)=2
case(hi+1:)
 a(i)=3
end select''', '', '1+merge(1,0,mod(i,7)>1)+merge(1,0,mod(i,7)>4)')
        source=source.replace('stride=-2','integer,parameter::lo=-2,hi=1\nstride=-2')
        self.check_source(source)

    def test_logical_selector(self):
        self.check_source(self.source('''select case(mod(i,2)==0)
case(.true.)
 a(i)=2
case(.false.)
 a(i)=1
end select''', '', '2-mod(i,2)'))

    def test_no_matching_case_or_default(self):
        source=self.source('''select case(mod(i,2))
case(0)
 a(i)=i
end select''', '', 'i*(1-mod(i,2))')
        source=source.replace('stride=-2','stride=-2\na=0')
        self.check_source(source)

    def test_nested_select_and_if(self):
        self.check_source(self.source('''total=0
select case(mod(i,3))
case(0)
 if(mod(i,2)==0) then
  total=i
 else
  select case(mod(i,5))
  case(0:2)
   total=i
  case default
   total=i
  end select
 end if
case default
 total=i
end select
a(i)=total''', 'local(total)', 'i'))

    def test_inner_loop_transfer_targets(self):
        for loop in ('do j=1,5','j=0\ndo while(j<5)\n j=j+1'):
            with self.subTest(loop=loop):
                self.check_source(self.source(f'''total=0
{loop}
 select case(j)
 case(2)
  cycle
 case(4:)
  exit
 case default
  total=total+j*i
 end select
end do
a(i)=total''', 'local(j,total)', '4*i'))

    def test_array_reduction_and_runtime_local_init(self):
        self.check_source('''implicit none
call compute(3)
print *,"PASS"
contains
subroutine compute(n)
integer,intent(in)::n
integer::i,work(n),a(2)
work=7
a=[3,5]
do concurrent(i=1:9001) local_init(work) reduce(+:a)
 select case(mod(i,2))
 case(0)
  work=99
  a=a+1
  cycle
 case default
  a=a+sum(work)
 end select
end do
if(any(work/=7)) stop 1
if(any(a/=[3+4500+21*4501,5+4500+21*4501])) stop 2
end subroutine
end
''')

    def test_every_branch_cycles_after_reduction(self):
        self.check_source('''implicit none
integer::i
integer(kind=8)::s
s=11
do concurrent(i=1:9001) reduce(+:s)
 select case(mod(i,2))
 case(0)
  s=s+1
  cycle
 case default
  s=s+2
  cycle
 end select
 s=s+100
end do
if(s/=13513) stop 1
print *,"PASS"
end
''')

    def test_uninitialized_local_without_default(self):
        self.check_source(self.source('''select case(mod(i,2))
case(1)
 total=i
end select
a(i)=total''','local(total)','i'), runtime_error='uninitialized')

    def test_all_branches_share_write_proof(self):
        for branch in ('case(0)', 'case default'):
            with self.subTest(branch=branch):
                self.check_source(self.source(f'''select case(mod(i,2))
{branch}
 a(i+1-1)=i
end select
a(i)=i''','','i'), reason='array writes must use the same index tuple')

    def test_selector_accesses_checked(self):
        source=self.source('''select case(a(1))
case default
 a(i)=i
end select''','','i')
        self.check_source(source.replace('stride=-2','stride=-2\na=0'),
            reason='not confined to the current iteration')

    def test_pure_array_result_selector(self):
        source=self.source('''select case(sum(values(n,mod(i,2))))
case(0)
 a(i)=0
 cycle
case default
 a(i)=n
end select''','','3*mod(i,2)')
        source=source.replace('stride=-2','integer::n\nstride=-2\nn=3')
        source=source.replace('\nend\n','''
contains
pure function values(n,k) result(r)
integer,intent(in)::n,k
integer::r(n)
r=k
end function
end
''')
        self.check_source(source)

    def test_impure_selector_falls_back(self):
        source=self.source('''select case(key(i))
case default
 a(i)=i
end select''','','i')
        source=source.replace('\nend\n','''
contains
integer function key(i)
integer,intent(in)::i
key=i
end function
end
''')
        self.check_source(source,reason='serial fallback')

    def test_character_selector_falls_back(self):
        source=self.source('''select case(ch)
case('a')
 a(i)=i
case default
 a(i)=0
end select''','','i')
        source=source.replace('stride=-2',"character(len=1)::ch\nstride=-2\nch='a'")
        self.check_source(source,reason='ordinary numeric or LOGICAL type')

    def test_exit_cannot_leave_concurrent_construct(self):
        source=self.source('''select case(i)
case default
 exit
end select
a(i)=i''','','i')
        with tempfile.TemporaryDirectory(prefix='rfort-select-exit-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            for opt in ('0','2'):
                run=subprocess.run([str(COMPILER),'-O',opt,'--parallel',str(path),
                    '-o',str(Path(tmp)/'case.exe')],capture_output=True,text=True,timeout=90)
                self.assertNotEqual(run.returncode,0)
                self.assertIn('EXIT cannot leave a DO CONCURRENT',run.stderr)

    def test_nocheck_and_serial_modes(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_select_case.f90').read_text()
        test_concurrent_parallel.ParallelConcurrent.build_run(self,source,'PASS',checks=False)
        test_fortran2023.Fortran2023.check(self,source.replace('9001','9'),' PASS\n')


if __name__=='__main__':
    unittest.main()

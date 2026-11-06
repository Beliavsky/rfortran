"""PURE/SIMPLE subroutine contracts and scoped parallel call lowering."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import test_concurrent_parallel as support
from test_record_read import ROOT,COMPILER

class ParallelSubroutines(unittest.TestCase):
    build_run=support.ParallelConcurrent.build_run

    def test_nested_scalar_outputs(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_subroutines.f90').read_text()
        self.build_run(source,'3 18001')
        self.build_run(source,'3 18001',parallel=False,threads=(1,))
        self.build_run(source,'3 18001',checks=False)

    def test_readonly_array_input_and_keyword_arguments(self):
        source="""implicit none
integer::i,a(3),y(9000)
a=[2,3,4]
do concurrent(i=1:9000)
 call total(y=y(i),a=a,offset=i)
end do
print *,y(1),y(9000)
contains
pure subroutine total(a,offset,y)
integer,intent(in)::a(:),offset
integer,intent(out)::y
y=sum(a)+offset
end subroutine
end
"""
        self.build_run(source,'10 9009')
        runtime=source.replace('integer::i,a(3),y(9000)','integer::i,y(9000)\ninteger,allocatable::a(:)\nallocate(a(-1:1))')
        self.build_run(runtime,'10 9009')

    def test_disjoint_array_output_sections(self):
        self.build_run("""implicit none
integer::i,a(2,9000)
do concurrent(i=1:9000)
 call column(i,a(:,i))
end do
print *,a(:,1),a(:,9000)
contains
pure subroutine column(i,a)
integer,intent(in)::i
integer,intent(out)::a(:)
a=[i,2*i]
end subroutine
end
""",'1 2 9000 18000')

    def test_rank_three_disjoint_inout_sections(self):
        self.build_run("""implicit none
integer::i,a(2,2,9000)
a=1
do concurrent(i=1:9000)
 call modify(i,a(:,:,i))
end do
print *,a(1,1,1),a(2,2,9000)
contains
pure subroutine modify(i,a)
integer,intent(in)::i
integer,intent(inout)::a(:,:)
a=a+i
end subroutine
end
""",'2 9001')

    def test_private_workspace_and_early_return(self):
        source="""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
 call compute(i,a(i))
end do
if(sum(a)/=81009000) stop 1
print *,a(1),a(9000)
contains
pure subroutine compute(i,r)
integer,intent(in)::i
integer,intent(out)::r
integer,allocatable::work(:)
integer::s
r=2*i
if(i==1) return
allocate(work(2),source=i,stat=s)
if(s/=0) return
r=sum(work)
deallocate(work)
end subroutine
end
"""
        self.build_run(source,'2 18000')
        wrapper=source.replace('call compute(i,a(i))','a(i)=wrapper(i)').replace('contains','contains\npure integer function wrapper(i) result(r)\ninteger,intent(in)::i\ncall compute(i,r)\nend function',1)
        self.build_run(wrapper,'2 18000')

    def test_scalar_types_and_local_output(self):
        for kind,value,expected in [('real','2.5','3.5 3.5'),('real(kind=8)','2.5d0','3.5 3.5'),('logical','.true.','F F')]:
            op='x+1.0' if kind.startswith('real') else '.not.x'
            with self.subTest(kind=kind):
                self.build_run(f"""implicit none
integer::i
{kind}::a(9000),t
do concurrent(i=1:9000) local(t)
 call compute({value},t)
 a(i)=t
end do
print *,a(1),a(9000)
contains
pure subroutine compute(x,y)
{kind},intent(in),value::x
{kind},intent(out)::y
y={op}
end subroutine
end
""",expected)

    def test_alias_fallback(self):
        self.build_run("""implicit none
integer::i,a(2,9000)
a=1
do concurrent(i=1:9000)
 call bump(a(1,i),a(2,i))
end do
print *,a(2,1),a(2,9000)
contains
pure subroutine bump(x,y)
integer,intent(in)::x
integer,intent(inout)::y
y=x+1
end subroutine
end
""",'2 2',reason='may alias',opts=('2',))

        # VALUE supplies a private input copy, so this alias is legal Fortran.
        # The parallel proof remains conservative, and serial binding must
        # preserve the copy before undefining the OUT actual.
        self.build_run("""implicit none
integer::i,a(9000)
a=1
do concurrent(i=1:9000)
 call copy_value(a(i),a(i))
end do
print *,a(1),a(9000)
contains
pure subroutine copy_value(x,y)
integer,intent(in),value::x
integer,intent(out)::y
y=x+1
end subroutine
end
""",'2 2',reason='may alias',opts=('2',))

    def test_impure_subroutine_fallback(self):
        source=(ROOT/'tests/programs/xconcurrent_parallel_subroutines.f90').read_text().replace('pure subroutine transform','subroutine transform')
        self.build_run(source,'3 18001',reason='not PURE or SIMPLE',opts=('2',))

    def test_sequence_association_retains_serial_fallback(self):
        self.build_run("""implicit none
integer::i,a(18000)
do concurrent(i=1:18000:2)
 call pair(i,a(i))
end do
print *,a(1),a(2),a(17999),a(18000)
contains
pure subroutine pair(i,a)
integer,intent(in)::i
integer,intent(out)::a(2)
a(1)=i
a(2)=i+1
end subroutine
end
""",'1 2 17999 18000',reason='sequence association',opts=('2',))

    def test_shared_scalar_output_fallback(self):
        self.build_run("""implicit none
integer::i,a(9000),t
do concurrent(i=1:9000)
 call compute(i,t)
 a(i)=t
end do
print *,a(1),a(9000)
contains
pure subroutine compute(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=x+1
end subroutine
end
""",'2 9001',reason='requires LOCAL storage',opts=('2',))

    def test_function_wrapper_and_input_only_nested_subroutine(self):
        self.build_run("""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
 a(i)=compute(i)
end do
print *,a(1),a(9000)
contains
pure integer function compute(i) result(r)
integer,intent(in)::i
call calculate(i,r)
end function
pure subroutine calculate(i,r)
integer,intent(in)::i
integer,intent(out)::r
call observe(i)
r=twice(i)
end subroutine
pure subroutine observe(i)
integer,intent(in)::i
integer::local
local=i+1
end subroutine
pure integer function twice(i)
integer,intent(in)::i
twice=i*2
end function
end
""",'2 18000')

    def test_read_of_written_array_through_call_fallback(self):
        self.build_run("""implicit none
integer::i,a(9000),b(9000)
a=1
do concurrent(i=1:9000)
 call compute(a,b(i))
 a(i)=b(i)
end do
print *,a(1),a(9000)
contains
pure subroutine compute(a,r)
integer,intent(in)::a(:)
integer,intent(out)::r
r=a(1)
end subroutine
end
""",'1 1',reason='whole-array expressions',opts=('2',))

    def test_procedure_dummy_shadows_known_definition(self):
        self.build_run("""implicit none
integer::a(9000)
call driver(other,a)
print *,a(1),a(9000)
contains
pure subroutine step(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=x+1
end subroutine
pure subroutine other(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=x+2
end subroutine
subroutine driver(step,a)
interface
 pure subroutine step(x,y)
 integer,intent(in)::x
 integer,intent(out)::y
 end subroutine
end interface
integer,intent(out)::a(:)
integer::i
do concurrent(i=1:size(a))
 call step(i,a(i))
end do
end subroutine
end
""",'3 9002',reason='procedure dummy',opts=('2',))

    def test_optional_argument_fallback(self):
        source="""implicit none
integer::i,a(9000)
do concurrent(i=1:9000)
 call compute(i,a(i))
end do
print *,a(1),a(9000)
contains
pure subroutine compute(i,r)
integer,intent(in) ATTR::i
integer,intent(out)::r
r=i+1
end subroutine
end
"""
        self.build_run(source.replace('ATTR',',optional '),'2 9001',reason='unsupported or saved storage',opts=('2',))

    def test_inout_initialization_and_out_undefined_elements(self):
        source="""implicit none
integer::i,a(9000)
INITIALIZE
do concurrent(i=1:9000)
 call compute(i,a(i))
end do
print *,a(9000)
contains
pure subroutine compute(i,a)
integer,intent(in)::i
integer,intent(INTENT)::a
BODY
end subroutine
end
"""
        with tempfile.TemporaryDirectory(prefix='rfort-subroutine-checks-') as temp:
            d=Path(temp);path=d/'case.f90';exe=d/'case.exe'
            cases=[source.replace('INTENT',intent).replace('INITIALIZE',initial).replace('BODY',body)
                   for intent,initial,body in [('inout','a(1:8999)=1','a=a+1'),('out','a=1','if(i<9000) a=i')]]
            cases.append(source.replace('a(9000)','a(2,9000)').replace('call compute(i,a(i))','call compute(i,a(:,i))')
                         .replace('intent(INTENT)::a','intent(out)::a(:)').replace('INITIALIZE','a=1').replace('BODY','a(1)=i'))
            for case in cases:
                path.write_text(case)
                for opt in ('0','2'):
                    p=subprocess.run([str(COMPILER),'-O',opt,'--parallel','--parallel-report',str(path),'-o',str(exe)],capture_output=True,text=True,timeout=90)
                    self.assertEqual(p.returncode,0,p.stderr)
                    self.assertIn('native parallel worker emitted',p.stderr)
                    for threads in (1,2,4):
                        p=subprocess.run([str(exe)],env=dict(os.environ,RFORT_NUM_THREADS=str(threads)),capture_output=True,text=True,timeout=30)
                        self.assertNotEqual(p.returncode,0)
                        self.assertIn('uninitialized',p.stderr)

if __name__=='__main__':
    unittest.main()

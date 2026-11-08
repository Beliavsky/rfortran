"""Primitive-field locality for bounded scalar derived types."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_captures
import test_concurrent_parallel


class ParallelLocalRecords(unittest.TestCase):
    check_source=test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run=test_concurrent_parallel.ParallelConcurrent.build_run

    @staticmethod
    def example():
        return (ROOT/'tests/programs/xconcurrent_parallel_local_records.f90').read_text()

    def reject(self,source,message):
        with tempfile.TemporaryDirectory(prefix='rfort-local-record-reject-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(source)
            result=subprocess.run([str(COMPILER),'--parallel',str(path),'-o',str(Path(tmp)/'case.exe')],cwd=tmp,capture_output=True,text=True,timeout=60)
            self.assertNotEqual(result.returncode,0,result.stderr)
            self.assertIn(message,result.stderr)
            self.assertIn('.f90:',result.stderr)

    def test_example(self):
        self.check_source(self.example())

    def test_serial(self):
        self.build_run(self.example(),'PASS',parallel=False,threads=(1,))

    def test_interpreter(self):
        with tempfile.TemporaryDirectory(prefix='rfort-local-record-interpreter-') as tmp:
            path=Path(tmp)/'case.f90';path.write_text(self.example().replace('n=9000','n=3'))
            result=subprocess.run([str(COMPILER),'run',str(path)],cwd=tmp,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['PASS'])

    def test_without_checks(self):
        self.build_run(self.example(),'PASS',checks=False)

    def test_default_none(self):
        source=self.example().replace('local(work)','local(work) shared(values) default(none)')
        source=source.replace('local_init(work)','local_init(work) shared(values) default(none)')
        self.check_source(source)

    def test_default_components_reset(self):
        source=self.example().replace(' work%state%position=real(i)', ' work%state%count=work%state%count+i\n work%state%position=real(i)')
        self.check_source(source.replace('real(3*i+8)','real(4*i+8)'))

    def test_local_init_mask(self):
        source=self.example().replace('do concurrent(i=1:n) local_init(work)', 'do concurrent(i=1:n,.not.work%active) local_init(work)')
        self.check_source(source)

    def test_nonunit_bounds(self):
        source=self.example().replace('position(3), velocity(3)','position(-1:1), velocity(-1:1)')
        self.check_source(source)

    def test_uninitialized_outer_is_not_read_by_local(self):
        self.check_source("""implicit none
type :: state
 real :: x(2)
 integer :: k
end type
type(state) :: work
real :: a(9000)
integer :: i
do concurrent(i=1:9000) local(work)
 work%x=real(i)
 work%k=1
 a(i)=sum(work%x)+real(work%k)
end do
do i=1,9000
 if(a(i)/=real(2*i+1)) stop 1
end do
print *, 'PASS'
end
""")

    def test_initialization_reset(self):
        self.check_source("""implicit none
type :: state
 integer :: x(2)
end type
type(state) :: work
integer :: i,a(9000)
work%x=99
do concurrent(i=1:9000) local(work)
 if(i==1) work%x(1)=i
 work%x(2)=i
 a(i)=sum(work%x)
end do
print *, 'PASS'
end
""",runtime_error='uninitialized')

    def test_complex_fields(self):
        for kind in (4,8):
            for locality in ('local','local_init'):
                with self.subTest(kind=kind,locality=locality):
                    init='work%z=work%z+cmplx(real(i,k),-real(i,k),kind=k)' if locality=='local_init' else 'work%z=cmplx(real(i,k),-real(i,k),kind=k)\n work%v=(3.0_k,4.0_k)'
                    offset=7 if locality=='local_init' else 6
                    imag=6 if locality=='local_init' else 8
                    self.check_source(f"""implicit none
integer,parameter :: k={kind}
type :: state
 complex(k) :: z,v(2)
end type
type(state) :: work
complex(k) :: a(9000)
integer :: i
work%z=(1.0_k,2.0_k)
work%v=(3.0_k,4.0_k)
do concurrent(i=1:9000) {locality}(work)
 {init}
 work%v=conjg(work%v)
 a(i)=work%z+sum(work%v)
end do
do i=1,9000
 if(abs(a(i)-cmplx(real(i+{offset},k),-real(i+{imag},k),kind=k))>1e-5_k) stop 1
end do
if(work%z/=(1.0_k,2.0_k)) stop 2
if(any(work%v/=(3.0_k,4.0_k))) stop 3
print *, 'PASS'
end
""")

    def test_storage_budget(self):
        self.check_source("""implicit none
type :: state
 integer :: x(2050),y(2050)
end type
type(state) :: work
integer :: i,a(9000)
do concurrent(i=1:9000) local(work)
 work%x(1)=i
 work%y(1)=1
 a(i)=work%x(1)+work%y(1)
end do
do i=1,9000
 if(a(i)/=i+1) stop 1
end do
print *, 'PASS'
end
""",reason='private-storage budget')

    def test_module_owned_record(self):
        source=self.example()
        declarations=source[:source.index('real :: values(n)')]
        code=source[source.index('work%state%position='):source.rindex('end')]
        source='module shared\n'+declarations+'contains\nsubroutine calculate()\nreal :: values(n)\ninteger :: i\n'+code+'end subroutine\nend module\nprogram p\nuse shared\nimplicit none\ncall calculate()\nend\n'
        self.check_source(source)
        self.build_run(source,'PASS',parallel=False,threads=(1,))

    def test_deeper_nesting(self):
        source=self.example().replace('type :: workspace', 'type :: middle\n type(state_type) :: inner\nend type\ntype :: workspace')
        source=source.replace('type(state_type) :: state','type(middle) :: state').replace('%state%','%state%inner%')
        self.check_source(source)

    def test_private_fields_with_reduction(self):
        self.check_source("""implicit none
type :: state
 integer :: count=2
end type
type(state) :: work
integer :: i
integer(kind=8) :: total
work%count=99
total=0
do concurrent(i=1:9000) local(work) reduce(+:total)
 work%count=work%count+i
 total=total+int(work%count,kind=8)
end do
if(total/=40522500_8) stop 1
if(work%count/=99) stop 2
print *, 'PASS'
end
""")

    def test_nested_concurrent_rejected(self):
        source=self.example().replace(' work%state%position=real(i)', ' do concurrent(j=1:1)\n work%state%position=real(i)\n end do').replace('integer :: i','integer :: i,j')
        self.reject(source,'nested DO CONCURRENT with derived LOCAL/LOCAL_INIT')

    def test_runtime_budget_includes_record_scalars(self):
        source="""implicit none
call calculate(4094)
contains
subroutine calculate(m)
integer,intent(in) :: m
type :: state
 complex :: z
 integer :: count
end type
type(state) :: work
integer :: i,a(9000),scratch(m)
do concurrent(i=1:9000) local(work,scratch)
 work%z=(1.0,2.0)
 work%count=i
 scratch(1)=1
 a(i)=work%count+scratch(1)
end do
do i=1,9000
 if(a(i)/=i+1) stop 1
end do
print *, 'PASS'
end subroutine
end
"""
        self.build_run(source,'PASS',dispatch=False)
        self.build_run(source.replace('4094','4093'),'PASS')

    def test_cycle(self):
        source=self.example().replace('do concurrent(i=1:n) local(work)', 'values=-1.0\ndo concurrent(i=1:n) local(work)')
        source=source.replace(' work%state%position=real(i)', ' if(mod(i,2)==0) cycle\n work%state%position=real(i)')
        source=source.replace(' if(values(i)/=real(3*i+8)) stop 2', ' if(mod(i,2)==0) then\n  if(values(i)/=-1.0) stop 2\n else\n  if(values(i)/=real(3*i+8)) stop 2\n end if')
        self.check_source(source)

    def test_pure_subroutine_field_output(self):
        source=self.example().replace(' work%state%position=real(i)', ' call fill(i,work%state%position)')
        source=source.rsplit('end',1)[0]+"""contains
pure subroutine fill(i,v)
integer,intent(in) :: i
real,intent(out) :: v(:)
v=real(i)
end subroutine
end
"""
        self.check_source(source)

    def test_component_bounds(self):
        source=self.example().replace('integer :: i','integer :: i,bad')
        source=source.replace('do concurrent(i=1:n) local(work)', 'bad=4\ndo concurrent(i=1:n) local(work)')
        source=source.replace('sum(work%state%position+work%state%velocity)+real(work%state%count)', 'work%state%position(bad)')
        self.check_source(source,runtime_error='subscript outside')

    def test_integer_kinds(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind):
                self.check_source(self.example().replace('integer :: count=2',f'integer(kind={kind}) :: count=2'))

    def test_array_component_default(self):
        source=self.example().replace('real :: position(3), velocity(3)', 'real :: position(3)\n real :: velocity(3)=[1.0,2.0,3.0]')
        source=source.replace(' work%state%velocity=[1.0,2.0,3.0]\n','')
        self.check_source(source)

    def test_private_module_kind_and_bounds_keep_defining_scope(self):
        source="""module types
implicit none
integer,parameter,private :: dp=kind(1d0),ndim=3
type :: workspace
 real(dp) :: x(ndim)
end type
end module
program p
use types
implicit none
integer,parameter :: dp=kind(1.0),ndim=7,wide=kind(1d0)
type(workspace) :: work
real(wide) :: a(9000)
integer :: i
do concurrent(i=1:9000) local(work)
 work%x=real(i,dp)+1d-8
 a(i)=sum(work%x)
end do
do i=1,9000
 if(abs(a(i)-(3d0*real(i,wide)+3d-8))>1d-10) stop 1
end do
print *, 'PASS'
end
"""
        self.check_source(source)
        self.build_run(source,'PASS',parallel=False,threads=(1,))
        default=source.replace('real(dp) :: x(ndim)', 'real(dp) :: x(ndim)=1.000000000931322574615478515625_dp')
        default=default.replace(' work%x=real(i,dp)+1d-8\n','')
        default=default.replace('3d0*real(i,wide)+3d-8', '3d0+3d0*2d0**(-30)')
        self.check_source(default)

    def test_pure_callee_keeps_module_host_binding(self):
        source="module m\nimplicit none\ntype :: state\n integer :: count=2\nend type\ntype(state) :: work\ncontains\npure integer function get_count() result(r)\nr=work%count\nend function\nsubroutine calculate()\ninteger :: i,a(9000)\nwork%count=7\ndo concurrent(i=1:9000) local(work)\n work%count=i\n a(i)=get_count()+work%count\nend do\ndo i=1,9000\n if(a(i)/=i+7) stop 1\nend do\nif(work%count/=7) stop 2\nprint *, 'PASS'\nend subroutine\nend module\nprogram p\nuse m\nimplicit none\ncall calculate()\nend\n"
        for locality in ('local','local_init'):
            with self.subTest(locality=locality):
                case=source.replace('local(work)',f'{locality}(work)')
                self.check_source(case)
                self.check_source(case.replace('do concurrent(i=1:9000)', 'do concurrent(i=1:9000,get_count()==7)'))
        self.build_run(source,'PASS',parallel=False,threads=(1,))

    def test_component_sections(self):
        source=self.example().replace(' work%state%position=real(i)',
            ' work%state%position(3:1:-1)=[real(i),real(i+1),real(i+2)]')
        source=source.replace(' if(.not.work%active) values(i)=-999.0',
            ' if(.not.work%active) values(i)=-999.0\n if(work%state%position(1)/=real(i+2)) values(i)=-999.0')
        self.check_source(source.replace('real(3*i+8)','real(3*i+11)'))

    def test_unsafe_component(self):
        self.reject(self.example().replace('integer :: count=2','real,allocatable :: scratch(:)\n integer :: count=2'),'bounded numeric/LOGICAL layout')

    def test_pointer_component(self):
        self.reject(self.example().replace('integer :: count=2','real,pointer :: scratch\n integer :: count=2'),'bounded numeric/LOGICAL layout')

    def test_array_owner(self):
        self.reject(self.example().replace('type(workspace) :: work','type(workspace) :: work(2)'),'requires a scalar record')

    def test_whole_object_use(self):
        self.reject(self.example().replace(' work%state%position=real(i)',' work=workspace(state_type([1.,2.,3.],[1.,2.,3.],2),.true.)'),'whole-object or unsupported component')

    def test_local_header_reference(self):
        self.reject(self.example().replace('do concurrent(i=1:n) local(work)','do concurrent(i=1:n,work%active) local(work)'),'cannot appear in the concurrent header')

    def test_duplicate_locality(self):
        self.reject(self.example().replace('local(work)','local(work) local_init(work)',1),'duplicate locality')

    def test_default_none_missing_shared(self):
        self.reject(self.example().replace('local(work)','local(work) default(none)',1),'DEFAULT(NONE) requires a locality specifier for `values`')


if __name__=='__main__':
    unittest.main()

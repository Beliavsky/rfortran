"""COMPLEX fields share native record ownership and paired definition checks."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_captures
import test_concurrent_parallel


class ParallelComplexRecords(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    @staticmethod
    def example(kind=8):
        source=(ROOT/'tests/programs/xconcurrent_parallel_complex_records.f90').read_text()
        return source.replace('dp=kind(1d0)', 'dp=kind(1.0)' if kind==4 else 'dp=kind(1d0)')

    def test_interpreter_serial_lowering(self):
        with tempfile.TemporaryDirectory(prefix='rfort-complex-fields-') as tmp:
            path=Path(tmp)/'case.f90'
            path.write_text(self.example().replace('n=9000','n=3'),encoding='utf-8')
            result=subprocess.run([str(COMPILER),'run',str(path)],cwd=tmp,capture_output=True,text=True,timeout=30)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.split(),['PASS'])

    def test_kinds(self):
        for kind in (4,8):
            with self.subTest(kind=kind): self.check_source(self.example(kind))

    def test_without_nesting(self):
        source=self.example().replace('type :: particle\n type(state_type) :: state\nend type\n','')
        source=source.replace('type(particle)', 'type(state_type)').replace('%state%', '%')
        self.check_source(source)

    def test_scalar_fields(self):
        source='\n'.join(line for line in self.example().splitlines() if '%vector' not in line)
        source=source.replace('amplitude, vector(3)', 'amplitude')
        self.check_source(source)

    def test_element_updates(self):
        source=self.example().replace('do concurrent(i=1:n)', 'do concurrent(i=1:n) local(j)')
        source=source.replace(' particles(i)%state%vector=conjg(particles(i)%state%vector)*(0.0_dp,1.0_dp)',
            ' do j=1,3\n particles(i)%state%vector(j)=conjg(particles(i)%state%vector(j))*(0.0_dp,1.0_dp)\n end do')
        self.check_source(source)

    def test_sections(self):
        for section in ('1:3:2','3:1:-1'):
            with self.subTest(section=section):
                source=self.example().replace(' particles(i)%state%vector=conjg(particles(i)%state%vector)*(0.0_dp,1.0_dp)',
                    f' particles(i)%state%vector({section})=conjg(particles(i)%state%vector({section}))*(0.0_dp,1.0_dp)')
                if section=='1:3:2':
                    source=source.replace('do j=1,3','do j=1,3,2')
                self.check_source(source)

    def test_nonunit_component_bounds(self):
        source=self.example().replace('vector(3)','vector(-1:1)').replace('do j=1,3','do j=-1,1')
        source=source.replace('real(2*j,dp),real(2*j-1,dp)', 'real(2*j+4,dp),real(2*j+3,dp)')
        self.check_source(source)

    def test_runtime_owner(self):
        source=self.example().replace('type(particle) :: particles(n)', 'type(particle),allocatable :: particles(:)')
        source=source.replace('do i=1,n', 'allocate(particles(n))\ndo i=1,n',1)
        self.check_source(source)

    def test_multidimensional_component(self):
        source=self.example().replace('vector(3)', 'vector(3,1)').replace('vector(j)', 'vector(j,1)')
        source=source.replace('vector=[(1.0_dp,2.0_dp),(3.0_dp,4.0_dp),(5.0_dp,6.0_dp)]',
            'vector=reshape([(1.0_dp,2.0_dp),(3.0_dp,4.0_dp),(5.0_dp,6.0_dp)],[3,1])')
        self.check_source(source)

    def test_runtime_sections(self):
        source=self.example().replace('integer :: i,j', 'integer :: i,j,lo,hi,step')
        source=source.replace('do concurrent(i=1:n)', 'lo=3\nhi=1\nstep=-1\ndo concurrent(i=1:n)')
        source=source.replace(' particles(i)%state%vector=conjg(particles(i)%state%vector)*(0.0_dp,1.0_dp)',
            ' particles(i)%state%vector(lo:hi:step)=conjg(particles(i)%state%vector(lo:hi:step))*(0.0_dp,1.0_dp)')
        self.check_source(source)

    def test_assumed_shape_dummy(self):
        source=self.example()
        declarations=source[source.index('integer,parameter'):source.index('type(particle) :: particles(n)')]
        code=source[source.index('do i=1,n'):source.index("print *, 'PASS'")]
        self.check_source('module data\nimplicit none\n'+declarations+'contains\nsubroutine update(particles)\ntype(particle),intent(inout) :: particles(:)\ninteger :: i,j\n'+code+"end subroutine\nend module\nprogram p\nuse data\nimplicit none\ntype(particle) :: particles(n)\ncall update(particles)\nprint *, 'PASS'\nend\n")

    def test_scalar_intrinsics(self):
        source=self.example().replace('particles(i)%state%amplitude*(0.0_dp,1.0_dp)',
            'sqrt(particles(i)%state%amplitude)+exp((0.0_dp,1.0_dp))')
        source=source.replace('cmplx(-2.0_dp,real(i,dp),kind=dp)',
            '(sqrt(cmplx(real(i,dp),2.0_dp,kind=dp))+exp((0.0_dp,1.0_dp)))')
        self.check_source(source)

    def test_readonly_module_fields(self):
        declarations=self.example().split('type(particle) :: particles(n)')[0].replace('implicit none\n','',1)
        self.check_source('module shared\nimplicit none\n'+declarations+"type(particle) :: coeff(1)\nend module\n"+"""program p
use shared
implicit none
type(particle) :: particles(n)
integer :: i,j
coeff(1)%state%amplitude=(1.0_dp,2.0_dp)
coeff(1)%state%vector=[(1.0_dp,2.0_dp),(3.0_dp,4.0_dp),(5.0_dp,6.0_dp)]
do concurrent(i=1:n)
 particles(i)%state%amplitude=coeff(1)%state%amplitude+real(i,dp)
 particles(i)%state%vector=conjg(coeff(1)%state%vector)+real(i,dp)
end do
do i=1,n
 if(abs(particles(i)%state%amplitude-cmplx(real(i+1,dp),2.0_dp,kind=dp))>1e-8_dp) stop 1
 do j=1,3
  if(abs(particles(i)%state%vector(j)-cmplx(real(i+2*j-1,dp),real(-2*j,dp),kind=dp))>1e-8_dp) stop 2
 end do
end do
print *, 'PASS'
end
""")

    def test_pure_function_actual(self):
        source=self.example().replace('particles(i)%state%amplitude*(0.0_dp,1.0_dp)', 'rotate(particles(i)%state%amplitude)')
        source=source.rsplit('end',1)[0]+"""contains
pure function rotate(z) result(r)
complex(dp),intent(in) :: z
complex(dp) :: r
r=z*(0.0_dp,1.0_dp)
end function
end
"""
        self.check_source(source)

    def test_mask(self):
        source=self.example().replace('do concurrent(i=1:n)', 'do concurrent(i=1:n,abs(particles(i)%state%amplitude)>0.0_dp)')
        self.check_source(source)

    def test_without_checks(self):
        self.build_run(self.example(), 'PASS', checks=False)

    def test_serial(self):
        self.build_run(self.example(), 'PASS', parallel=False, threads=(1,))

    def test_partial_initialization(self):
        source=self.example().replace('particles(i)%state%vector=[(1.0_dp,2.0_dp),(3.0_dp,4.0_dp),(5.0_dp,6.0_dp)]',
            'particles(i)%state%vector(1:2)=[(1.0_dp,2.0_dp),(3.0_dp,4.0_dp)]')
        self.check_source(source, runtime_error='uninitialized')

    def test_dynamic_bounds_check(self):
        source=self.example().replace('integer :: i,j', 'integer :: i,j,bad')
        source=source.replace('do concurrent(i=1:n)', 'bad=4\ndo concurrent(i=1:n)')
        source=source.replace(' particles(i)%state%vector=conjg(particles(i)%state%vector)*(0.0_dp,1.0_dp)',
            ' particles(i)%state%vector(bad)=(0.0_dp,1.0_dp)')
        self.check_source(source, runtime_error='outside')

    def test_cross_record_dependency(self):
        source=self.example().replace('=conjg(particles(i)%state%vector)', '=conjg(particles(1)%state%vector)')
        source=source.split('end do\ndo i=1,n')[0]+"end do\nprint *, 'PASS'\nend\n"
        self.check_source(source, reason='not confined to the current iteration')

    def test_unsafe_sibling(self):
        source=self.example().replace('complex(dp) :: amplitude', 'real,allocatable :: scratch(:)\n complex(dp) :: amplitude')
        self.check_source(source, reason='serial fallback:')


if __name__ == '__main__':
    unittest.main()

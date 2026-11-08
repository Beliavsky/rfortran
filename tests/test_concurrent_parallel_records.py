"""Parallel component updates reuse the primitive-array ownership proof."""
import unittest
import subprocess
import tempfile
from pathlib import Path
from test_record_read import ROOT, COMPILER
import test_concurrent_parallel_captures
import test_concurrent_parallel


class ParallelRecords(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    @staticmethod
    def example():
        return (ROOT/'tests/programs/xconcurrent_parallel_records.f90').read_text()

    def test_example(self):
        self.check_source(self.example())

    def test_named_extent_without_parallel(self):
        self.build_run(self.example(), 'PASS', parallel=False, threads=(1,))

    def test_without_runtime_checks(self):
        self.build_run(self.example(), 'PASS', checks=False)

    def test_explicit_sharing(self):
        source = self.example().replace('do concurrent (i=1:n)',
                                       'do concurrent (i=1:n) default(none) shared(particles)')
        self.check_source(source)

    def test_component_initialization_checks(self):
        source = self.example().replace('   particles(i)%velocity=2.0_dp',
                                       '   if(i/=2) particles(i)%velocity=2.0_dp')
        self.check_source(source, runtime_error='uninitialized')

    def test_scalar_function_actual(self):
        source = self.example().replace('dt*particles(i)%velocity',
                                       'dt*abs(particles(i)%velocity)')
        self.check_source(source)

    def test_component_mask_and_default_initializer(self):
        source = self.example().replace('real(dp) :: x, velocity',
                                       'real(dp) :: x\n   real(dp) :: velocity=2.0_dp')
        source = source.replace('   particles(i)%velocity=2.0_dp\n', '')
        source = source.replace('do concurrent (i=1:n)',
                                'do concurrent (i=1:n, particles(i)%active)')
        source = source.replace('particles(i)%id/=i+1',
                                'particles(i)%id/=i+merge(1,0,mod(i,2)==0)')
        self.check_source(source)

    def test_integer_component_kinds(self):
        for kind in (1,2,4,8):
            with self.subTest(kind=kind):
                source = self.example().replace('integer :: id', f'integer(kind={kind}) :: id')
                source = source.replace('particles(i)%id=i', 'particles(i)%id=mod(i,50)')
                source = source.replace('particles(i)%id/=i+1', 'particles(i)%id/=mod(i,50)+1')
                self.check_source(source)

    def test_component_bounds_checks(self):
        source = self.example().replace('particles(i)%id=particles(i)%id+1',
                                       'particles(i+1)%id=particles(i+1)%id+1')
        self.check_source(source, runtime_error='outside')

    def test_component_bounds_without_parallel(self):
        source = self.example().replace('particles(i)%id=particles(i)%id+1',
                                       'particles(i+1)%id=particles(i+1)%id+1')
        with tempfile.TemporaryDirectory(prefix='rfort-record-bounds-') as tmp:
            path = Path(tmp)/'case.f90'
            exe = Path(tmp)/'case.exe'
            path.write_text(source)
            build = subprocess.run([str(COMPILER), '-O2', str(path), '-o', str(exe)],
                                   capture_output=True, text=True, timeout=90)
            self.assertEqual(build.returncode, 0, build.stderr)
            run = subprocess.run([str(exe)], capture_output=True, text=True, timeout=30)
            self.assertNotEqual(run.returncode, 0)
            self.assertIn('outside', run.stderr)

    def test_target_owner_keeps_serial_fallback(self):
        source = self.example().replace('type(particle) :: particles(n)',
                                       'type(particle),target :: particles(n)')
        self.check_source(source, reason='unsupported attributes')

    def test_runtime_allocatable_and_nonunit_lower_bound(self):
        source = self.example().replace('type(particle) :: particles(n)',
                                       'type(particle), allocatable :: particles(:)')
        source = source.replace('do i=1,n', 'do i=-3,n-4')
        source = source.replace('do concurrent (i=1:n)', 'do concurrent (i=-3:n-4)')
        source = source.replace('do i=-3,n-4', 'allocate(particles(-3:n-4))\ndo i=-3,n-4', 1)
        self.check_source(source)

    def test_multidimensional_fields_and_multiple_indices(self):
        self.check_source('''implicit none
type :: entry
   real :: x
   integer(kind=8) :: number
   logical :: ok
end type
type(entry) :: a(100,100)
integer :: i,j
do j=1,100
 do i=1,100
  a(i,j)%x=real(i+j)
  a(i,j)%number=i-j
  a(i,j)%ok=.true.
 end do
end do
do concurrent(i=1:100,j=1:100)
 a(i,j)%x=a(i,j)%x*2
 a(i,j)%number=a(i,j)%number+1
 a(i,j)%ok=a(i,j)%x>0
end do
do j=1,100
 do i=1,100
  if(a(i,j)%x/=2*(i+j)) stop 1
  if(a(i,j)%number/=i-j+1) stop 2
  if(.not.a(i,j)%ok) stop 3
 end do
end do
print *, 'PASS'
end
''')

    def test_rank_fifteen(self):
        trailing = ',1'*14
        source = self.example().replace('particles(n)', f'particles(n{trailing})')
        source = source.replace('particles(i)', f'particles(i{trailing})')
        self.check_source(source)

    def test_assumed_shape_dummy_and_module_type(self):
        source = self.example()
        declarations = source[source.index('integer, parameter'):source.index('type(particle) :: particles(n)')]
        code = source[source.index('do i=1,n'):source.index("print *, 'PASS'")]
        source = ('module types\nimplicit none\n' + declarations + '''contains
subroutine update(particles)
type(particle),intent(inout) :: particles(:)
integer :: i
real(dp),parameter :: dt=0.5_dp
''' + code + '''end subroutine
end module
program p
use types
implicit none
type(particle) :: a(n)
call update(a)
print *, 'PASS'
end program
''')
        self.check_source(source)

    def test_character_layout_keeps_serial_fallback(self):
        source = self.example().replace('   real(dp) :: x, velocity',
                                       '   character :: tag\n   real(dp) :: x, velocity')
        source = source.replace('n=9000', 'n=10')
        self.check_source(source, reason='serial fallback:')

    def test_unused_array_component_layout(self):
        source = self.example().replace('   real(dp) :: x, velocity',
                                       '   real :: vector(2)\n   real(dp) :: x, velocity')
        self.check_source(source)

    def test_read_from_written_field_must_stay_in_owned_element(self):
        source = self.example().replace('particles(i)%x+dt*particles(i)%velocity',
                                        'particles(1)%x+dt*particles(i)%velocity')
        # Inspect serial fallback without imposing a parallel interpretation on
        # the intentionally dependent construct.
        source = source.split('end do\ndo i=1,n')[0] + "end do\nprint *, 'PASS'\nend program\n"
        self.check_source(source, reason='not confined to the current iteration')

    def test_owned_component_type_keeps_serial_fallback(self):
        source = self.example().replace('   real(dp) :: x, velocity',
                                       '   real, allocatable :: scratch(:)\n   real(dp) :: x, velocity')
        self.check_source(source, reason='serial fallback:')


if __name__ == '__main__':
    unittest.main()

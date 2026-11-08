"""Fixed component arrays within independent records in native workers."""
import unittest
from test_record_read import ROOT
import test_concurrent_parallel_captures
import test_concurrent_parallel


class ParallelRecordArrays(unittest.TestCase):
    check_source = test_concurrent_parallel_captures.ParallelCaptures.check_source
    build_run = test_concurrent_parallel.ParallelConcurrent.build_run

    @staticmethod
    def example():
        return (ROOT/'tests/programs/xconcurrent_parallel_record_arrays.f90').read_text()

    def test_example(self):
        self.check_source(self.example())

    def test_scalar_component_subscripts(self):
        source = self.example().replace('do concurrent (i=1:n)', 'do concurrent (i=1:n) local(j)')
        source = source.replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                '   do j=1,3\n      particles(i)%position(j)=particles(i)%position(j)+0.5*particles(i)%velocity(j)\n   end do')
        self.check_source(source)

    def test_sections(self):
        source = self.example().replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                '   particles(i)%position(1:3:2)=particles(i)%position(1:3:2)+0.5*particles(i)%velocity(1:3:2)\n   particles(i)%position(2)=particles(i)%position(2)+2')
        self.check_source(source)

    def test_negative_stride_sections(self):
        source = self.example().replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                '   particles(i)%position(3:1:-1)=particles(i)%position(3:1:-1)+0.5*particles(i)%velocity(3:1:-1)')
        self.check_source(source)

    def test_component_index_depends_on_owner_index(self):
        source = self.example().replace('do concurrent (i=1:n)', 'do concurrent (i=1:n) local(j)')
        source = source.replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                '   j=mod(i-1,3)+1\n   particles(i)%position(j)=particles(i)%position(j)+0.5*particles(i)%velocity(j)')
        source = source.replace('real(i+2*j-1)', 'real(i+j-1)+merge(real(j),0.0,j==mod(i-1,3)+1)')
        self.check_source(source)

    def test_nonunit_component_bounds(self):
        source = self.example().replace('position(3), velocity(3)', 'position(-1:1), velocity(-1:1)')
        source = source.replace('do j=1,3', 'do j=-1,1').replace('real(i+2*j-1)', 'real(i+2*j+3)')
        self.check_source(source)

    def test_named_component_extent(self):
        source = self.example().replace('n=9000', 'n=9000, ndim=3')
        source = source.replace('position(3), velocity(3)', 'position(ndim), velocity(ndim)')
        self.check_source(source)

    def test_named_component_extent_without_parallel(self):
        source = self.example().replace('n=9000', 'n=9000, ndim=3')
        source = source.replace('position(3), velocity(3)', 'position(ndim), velocity(ndim)')
        self.build_run(source, 'PASS', parallel=False, threads=(1,))

    def test_dimension_attribute(self):
        source = self.example().replace('real :: position(3), velocity(3)',
                                       'real,dimension(3) :: position,velocity')
        self.check_source(source)

    def test_combined_storage_rank_fifteen(self):
        trailing = ',1'*13
        source = self.example().replace('particles(n)', f'particles(n{trailing})')
        source = source.replace('particles(i)', f'particles(i{trailing})')
        self.check_source(source)

    def test_assumed_shape_dummy(self):
        source = self.example()
        declarations = source[source.index('integer, parameter'):source.index('type(particle) :: particles(n)')]
        code = source[source.index('do i=1,n'):source.index("print *, 'PASS'")]
        self.check_source('module types\nimplicit none\n'+declarations+'''contains
subroutine update(particles)
type(particle),intent(inout) :: particles(:)
integer :: i,j
'''+code+'''end subroutine
end module
program p
use types
implicit none
type(particle) :: particles(n)
call update(particles)
print *, 'PASS'
end program
''')

    def test_integer_and_logical_arrays(self):
        self.check_source('''implicit none
type :: entry
 integer(kind=8) :: values(3)
 logical :: flags(3)
end type
type(entry) :: a(5000)
integer :: i
do i=1,5000
 a(i)%values=[i,i+1,i+2]
 a(i)%flags=[.true.,.false.,.true.]
end do
do concurrent(i=1:5000,any(a(i)%flags))
 a(i)%values=a(i)%values+2
 a(i)%flags=.not.a(i)%flags
end do
do i=1,5000
 if(any(a(i)%values/=[i+2,i+3,i+4])) stop 1
 if(any(a(i)%flags.neqv.[.false.,.true.,.false.])) stop 2
end do
print *, 'PASS'
end
''')

    def test_multidimensional_component(self):
        self.check_source('''implicit none
type :: entry
 integer :: matrix(2,3)
end type
type(entry) :: a(5000)
integer :: i,j,k
do i=1,5000
 a(i)%matrix=reshape([1,2,3,4,5,6],[2,3])
end do
do concurrent(i=1:5000)
 a(i)%matrix=a(i)%matrix+3
end do
do i=1,5000
 do k=1,3
  do j=1,2
   if(a(i)%matrix(j,k)/=j+2*k+1) stop 1
  end do
 end do
end do
print *, 'PASS'
end
''')

    def test_runtime_owner(self):
        source = self.example().replace('type(particle) :: particles(n)', 'type(particle),allocatable :: particles(:)')
        source = source.replace('do i=1,n', 'allocate(particles(n))\ndo i=1,n', 1)
        self.check_source(source)

    def test_nonunit_runtime_owner_bounds(self):
        source = self.example().replace('type(particle) :: particles(n)', 'type(particle),allocatable :: particles(:)')
        source = source.replace('do i=1,n', 'do i=-3,n-4').replace('do concurrent (i=1:n)', 'do concurrent (i=-3:n-4)')
        source = source.replace('do i=-3,n-4', 'allocate(particles(-3:n-4))\ndo i=-3,n-4', 1)
        self.check_source(source)

    def test_default_array_initializer(self):
        source = self.example().replace('real :: position(3), velocity(3)',
                                       'real :: position(3)\n   real :: velocity(3)=[2.0,4.0,6.0]')
        source = source.replace('   particles(i)%velocity=[2.0,4.0,6.0]\n', '')
        self.check_source(source)

    def test_cross_particle_dependency_falls_back(self):
        source = self.example().replace('particles(i)%position+0.5', 'particles(1)%position+0.5')
        source = source.split('end do\ndo i=1,n')[0]+"end do\nprint *, 'PASS'\nend program\n"
        self.check_source(source, reason='not confined to the current iteration')

    def test_component_index_cannot_establish_record_ownership(self):
        source = self.example().replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                       '   particles(1)%position(mod(i-1,3)+1)=0.0')
        source = source.split('end do\ndo i=1,n')[0]+"end do\nprint *, 'PASS'\nend program\n"
        self.check_source(source, reason='every concurrent index')

    def test_partial_initialization(self):
        source = self.example().replace('particles(i)%velocity=[2.0,4.0,6.0]',
                                       'particles(i)%velocity(1:2)=[2.0,4.0]')
        self.check_source(source, runtime_error='uninitialized')

    def test_component_bounds(self):
        source = self.example().replace('integer :: i,j', 'integer :: i,j,bad')
        source = source.replace('do concurrent (i=1:n)', 'bad=4\ndo concurrent (i=1:n)')
        source = source.replace('   particles(i)%position=particles(i)%position+0.5*particles(i)%velocity',
                                '   particles(i)%position(bad)=0.0')
        self.check_source(source, runtime_error='outside')

    def test_readonly_module_component(self):
        self.check_source('''module shared
implicit none
integer,parameter,private :: ndim=3
type :: entry
 real :: v(ndim)
end type
type(entry) :: coeff(2)
end module
program p
use shared
implicit none
integer,parameter :: ndim=7
type(entry) :: a(5000)
integer :: i,j
coeff(1)%v=[1.,2.,3.]
do concurrent(i=1:5000)
 a(i)%v=coeff(1)%v+real(i)
end do
do i=1,5000
 do j=1,3
  if(a(i)%v(j)/=real(i+j)) stop 1
 end do
end do
print *, 'PASS'
end program
''')


if __name__ == '__main__':
    unittest.main()

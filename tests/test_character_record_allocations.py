"""Owned CHARACTER arrays in arrays of derived types, native and interpreted."""
import unittest
import test_deallocate
from test_record_read import ROOT

class CharacterRecordAllocations(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_copy_and_deallocate(self):
        self.check((ROOT / 'tests/programs/xrecord_allocatable_character.f90').read_text(),
                   'F F\nalpha\nbeta\n10 2\nF T\n')

    def test_bounds_empty_and_reallocate(self):
        self.check("""implicit none
type :: row
character(len=3), allocatable :: x(:)
end type
type(row) :: a(2)
allocate(a(1)%x(-1:0), a(2)%x(0))
a(1)%x = ['abc','def']
print *, lbound(a(1)%x), ubound(a(1)%x), size(a(2)%x), allocated(a(2)%x)
print '(a)', a(1)%x(-1)
a(2)%x = ['uvw','xyz']
print '(a)', a(2)%x
deallocate(a(2)%x)
allocate(a(2)%x(1))
a(2)%x = 'new'
print '(a)', a(2)%x
end
""", '-1 0 0 T\nabc\nuvw\nxyz\nnew\n')

    def test_runtime_parent_and_scalar_procedure(self):
        self.check("""module m
implicit none
type :: row
character(len=4), allocatable :: x(:)
end type
contains
subroutine fill(a,n)
type(row), intent(inout) :: a
integer, intent(in) :: n
allocate(a%x(n))
a%x = 'data'
end subroutine
end module
use m
implicit none
type(row), allocatable :: a(:)
integer :: n
read *, n
allocate(a(n))
call fill(a(n),2)
print '(a)', a(n)%x
print *, allocated(a(1)%x)
deallocate(a)
allocate(a(1))
print *, allocated(a(1)%x)
end
""", 'data\ndata\nF\nF\n', data='3\n')

    def test_parameter_length_and_component_arguments(self):
        self.check("""implicit none
integer, parameter :: width = 4
type :: row
character(len=width), allocatable :: x(:), y(:)
end type
type(row) :: a(1)
allocate(a(1)%x(1), a(1)%y(1))
a(1)%x = 'left'
a(1)%y = 'stay'
call change(a(1)%x)
print '(a)', a(1)%x, a(1)%y
contains
subroutine change(x)
character(len=*), intent(inout) :: x(:)
x = 'new'
end subroutine
end
""", 'new\nstay\n')

    def test_uninitialized_character_is_checked(self):
        self.check("""implicit none
type :: row
character(len=3), allocatable :: x(:)
end type
type(row) :: a(1)
allocate(a(1)%x(2))
a(1)%x(1) = 'abc'
print '(a)', a(1)%x(2)
end
""", error='uninitialized', reference=False)

    def test_multidimensional_and_zero_length(self):
        self.check("""implicit none
type :: row
character(len=2), allocatable :: x(:,:)
character(len=0), allocatable :: z(:)
end type
type(row) :: a(2)
allocate(a(1)%x(2,2),a(1)%z(3))
a(1)%x = reshape(['ab','cd','ef','gh'],[2,2])
a(1)%z = ''
a(2) = a(1)
print '(a)', a(2)%x
print *, shape(a(2)%x), len(a(2)%z), size(a(2)%z)
deallocate(a(1)%x,a(1)%z)
print *, allocated(a(2)%x), allocated(a(2)%z)
end
""", 'ab\ncd\nef\ngh\n2 2 0 3\nT T\n')

if __name__ == '__main__':
    unittest.main()

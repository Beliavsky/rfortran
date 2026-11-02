"""ALLOCATE with a CHARACTER type specification, `allocate(character(len=n)
:: s)`, of deferred-length scalars and arrays. Programs run in the
interpreter, at -O0 and -O2, and with gfortran when it is installed.

Run after cargo build --bin rfort: python tests/test_typed_allocate.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class TypedAllocate(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_array_then_constructor_assignment(self):
        # Every ALLOCATE and assignment gives the array the same length.
        self.check('''character(:), allocatable :: s(:)
allocate(character(len=3) :: s(2))
s = ["abc","def"]
print '(2a,i2,i2)', s, len(s), size(s)
end
''', 'abcdef 3 2\n')

    def test_array_with_stat_and_reallocation(self):
        self.check('''program main
implicit none
character(len=:), allocatable :: s(:), t
integer :: k
allocate(character(len=3) :: s(2), stat=k)
s = 'x'
print '(2a4,i2)', s, k
deallocate(s)
allocate(character(len=3) :: s(3), t)
s = ['ab', 'cd', 'ef']
t = 'xyz'
print '(3a3,1x,a)', s, t
end program main
''', 'x   x    0\nab cd ef  xyz\n')

    def test_runtime_lengths(self):
        self.check('''program main
implicit none
character(len=:), allocatable :: s(:)
integer :: n
n = 4
allocate(character(len=3) :: s(2))
s = ['abc', 'de ']
print '(2a4,2i3)', s, len(s), size(s)
deallocate(s)
allocate(character(len=n) :: s(3), stat=n)
s(3) = 'wxyz'
print '(a,2i3)', s(3), len(s(3)), n
end program main
''', 'abc de    3  2\nwxyz  4  0\n')

    def test_allocated_status_of_character_arrays(self):
        self.check('''program main
implicit none
character(len=:), allocatable :: s(:)
character(len=4), allocatable :: w(:)
real, allocatable :: x(:)
print '(3l2)', allocated(s), allocated(w), allocated(x)
allocate(character(len=3) :: s(2))
allocate(w(3), x(2))
s = 'ab'
w = 'cd'
print '(3l2,1x,2a,1x,3a)', allocated(s), allocated(w), allocated(x), s, w
deallocate(w)
print '(2l2)', allocated(s), allocated(w)
if (.not. allocated(w)) print '(a,i2,i2)', 'w gone', size(s), len(s)
end program main
''', ' F F F\n T T T ab ab  cd  cd  cd\n T F\nw gone 2 3\n')

    def test_dummy_argument(self):
        self.check('''program main
implicit none
character(len=:), allocatable :: s(:)
call make(s)
print '(3a3)', s
contains
subroutine make(a)
character(len=:), allocatable, intent(out) :: a(:)
allocate(character(len=2) :: a(3))
a = 'q'
end subroutine
end program main
''', 'q  q  q\n')


if __name__ == '__main__':
    unittest.main()

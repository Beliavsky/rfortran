"""REAL(4) precision, KIND arguments of INT, AINT and ANINT, elemental
intrinsics of array constructors in formatted output, bit intrinsics with array
positions, deferred-length CHARACTER (fallback storage, ALLOCATE with a type
specification, growth by concatenation, arrays of one length), type parameter
inquiries and ERROR STOP without a code.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_kinds_and_deferred_strings.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class KindsAndDeferredStrings(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_real4_values_are_single_precision(self):
        """A value stored in a REAL(4) variable, element or dummy is rounded to single precision."""
        self.check("""program p
real :: a, b, s
integer :: i
a = 1.0/3.0
print '(f20.15)', a
b = 16777216.0
b = b + 1.0
print '(f12.1)', b
s = 0
do i = 1, 10
 s = s + 0.1
end do
print '(f20.15)', s
print *, a == 1.0d0/3.0d0
end program
""", '   0.333333343267441\n  16777216.0\n   1.000000119209290\n F\n')
        self.check("""module m
contains
subroutine acc(s, n)
  real, intent(inout) :: s
  integer, intent(in) :: n
  integer :: i
  do i = 1, n
    s = s + 0.1
  end do
end subroutine
real function third()
  third = 1.0/3.0
end function
end module
program p
use m
real :: a(3), s, t
real, allocatable :: h(:)
integer :: i, n
n = 3
allocate(h(n))
a = 0; h = 0
do i = 1, 10
  a(2) = a(2) + 0.1
  h(2) = h(2) + 0.1
end do
s = 0
call acc(s, 10)
t = third()
print '(3f20.15)', a(2), h(2), s
print *, t == 1.0d0/3.0d0, a(2) == 1.0d0, h(2) == 1.0d0
end program
""", '   1.000000119209290   1.000000119209290   1.000000119209290\n F F F\n')

    def test_kind_arguments(self):
        """INT, AINT and ANINT with a KIND argument."""
        self.check("""program p
use, intrinsic :: iso_fortran_env, only: int8, int64
real :: x(2) = [0.5, -2.5]
integer(int8) :: b
print *, int(x, kind=8), kind(int(x(1), 8)), int([1.5, 2.5], kind=int8)
print *, anint(x, 8), kind(anint(2.5, 8)), aint(2.7d0, kind=4), kind(aint(2.7d0, kind=4))
b = int(100.7, kind=int8)
print *, b, kind(b), int(3.0e9, kind=int64)
end program
""", ' 0 -2 8 1 2\n 1.0 -3.0 8 2.0 4\n 100 1 3000000000\n')

    def test_elemental_intrinsics_of_constructors(self):
        """Formatted output of expressions of array constructors, and bit intrinsics with array positions."""
        self.check("""program p
integer :: i
character(len=*), parameter :: g = '(*(g0,1x))'
print g, 'abs', abs([20, 0, -1])
print g, acos([-1.0, 0.5])
print g, 2*[1.0, 2.0], 'end'
print '(*(i3))', [(i, i=1,3)]*2
write(*, g) 'v', [1, 2] + 1, 'w'
print *, ibset(0, [1, 2, 3]), ibclr(i=-1, pos=[1, 2])
print *, ishftc(3, [1, 2, -1]), ishftc([2, 8, -128], 3)




end program
""", 'abs 20 0 1\n3.14159274 1.04719758\n2.00000000 4.00000000 end\n  2  4  6\nv 2 3 w\n 2 4 8 -3 -5\n 6 12 -2147483647 16 64 -1017\n')

    def test_deferred_length_character(self):
        """Deferred-length CHARACTER beyond inferred sizes, ALLOCATE with a type, growth, arrays of one length and LEN and KIND inquiries."""
        self.check("""program p
character(len=:), allocatable :: s, t
integer :: n
s = 'hello'
t = s // ' world'
print *, len(s), len(t), t
n = 3
s = repeat('ab', n)
print *, s, len(s)
s = trim(adjustl('   xy  '))
print '(a,i0)', s, len(s)
end program
""", ' 5 11 hello world\n ababab 6\nxy2\n')
        self.check("""program p
character(len=:), allocatable :: s
integer :: n
n = 4
allocate(character(len=n) :: s)
s = 'abcdefg'
print *, s, len(s)
end program
""", ' abcdefg 7\n')
        self.check("""program p
character(len=:), allocatable :: s
integer :: i
s = ''
do i = 1, 5
  s = s // char(48 + i)
end do
print '(a,1x,i0)', s, len(s)
deallocate(s)
print *, allocated(s)
end program
""", '12345 5\n F\n')
        self.check("""program p
character(len=:), allocatable :: names(:)
names = [character(len=5) :: 'ann', 'bob', 'carol']
print *, size(names), len(names), names(3)
end program
""", ' 3 5 carol\n')
        self.check("""program p
character(len=6) :: s
real(8) :: x
print *, s%len, x%kind, s%kind
end program
""", ' 6 8 1\n')

    def test_error_stop_without_code(self):
        """A bare ERROR STOP ends its statement, so the next CASE is not read as its code."""
        self.check("""program p
character(len=5) :: w = 'a'
select case(w)
case('a')
  error stop
case('b')
  print *, 2
end select
end program
""", error="ERROR STOP")


if __name__ == "__main__":
    unittest.main(verbosity=2)

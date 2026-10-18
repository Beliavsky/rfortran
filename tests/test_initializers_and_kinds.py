"""Array declaration initializers with RESHAPE (SHAPE, PAD and ORDER) and
implied-DO constructors, and kind type parameters given by expressions:
KIND(...), SELECTED_REAL_KIND, SELECTED_CHAR_KIND and CHARACTER(LEN=, KIND=).

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_initializers_and_kinds.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class InitializersAndKinds(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_array_initializers(self):
        self.check("""program in1
implicit none
integer :: ii, i
integer :: box(3,4) = reshape([1,2,3,4,5,6,7,8,9,10,11,12], shape(box))
integer :: v(-3:3) = [(abs(abs(ii)-50), ii=-3,3)]
integer :: r(3,4) = reshape([(-6+i, i=0,11)], [3,4])
real :: x(2,3) = reshape([1, 2, 3, 4, 5, 6], shape(x), order=[2,1])
integer :: p(2,3) = reshape([1, 2], [2,3], pad=[9])
integer(1) :: b(2,2) = reshape([2,4,8,16], [2,2])
print *, box
print *, box(2,3), v, lbound(v)
print *, r(3,4), sum(r)
print *, x
print *, p
print *, b
end program
""", "1 2 3 4 5 6 7 8 9 10 11 12\n8 47 48 49 50 49 48 47 -3\n5 -6\n"
            "1.0 4.0 2.0 5.0 3.0 6.0\n1 2 9 9 9 9\n2 4 8 16\n")

    def test_kind_expressions(self):
        self.check("""program p
implicit none
integer, parameter :: dp = kind(0.0d0), ascii = selected_char_kind('ascii')
integer :: i
integer, parameter :: sq(4) = [(i*i, i=1,4)], m = max(3, abs(-7), mod(17, 5))
complex :: z
complex(kind=dp) :: w
character(len=3, kind=ascii) :: s
real(kind=kind(0.0)) :: a
z = (1.0, 2.0)
w = cmplx(1, 2, kind=kind(w))
s = 'abc'
a = 2
print *, dp, sq, m, kind(z), kind(w), kind(a), kind(s), s
print *, cmplx(abs(z%re), abs(z%im), kind=kind(z)), w
end program
""", "8 1 4 9 16 7 4 8 4 1 abc\n(1.0,2.0) (1.0,2.0)\n")
        self.check("""program p
implicit none
integer, parameter :: wp = selected_real_kind(15)
real(kind=kind(0.0)) :: a
real(kind(0.0d0)) :: b = 0.17d0
integer(kind(1_8)) :: c
real(wp) :: d
integer(selected_int_kind(2)) :: e
a = 2; c = 3; d = 4; e = 5
print *, kind(a), kind(b), kind(c), kind(d), kind(e), b
end program
""", "4 8 8 8 1 0.17000000000000001\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""POPCNT, POPPAR, LEADZ, TRAILZ, DSHIFTL, DSHIFTR, BIT_SIZE, IANY, IALL,
IPARITY, PARITY, Bessel functions, NORM2, OUT_OF_RANGE, specific intrinsic
names, and wrapping on assignment to a narrower INTEGER kind.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_more_intrinsics.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class MoreIntrinsics(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_bit_counts_and_double_shifts(self):
        self.check("""program p
implicit none
integer(1) :: b1
integer(2) :: b2
integer(8) :: b8
integer :: i, j
i = 96
b1 = -1_1
b2 = 256_2
b8 = huge(0_8)
print *, popcnt(i), popcnt(b1), popcnt(b8), poppar(i), poppar(7_2)
print *, leadz(i), leadz(b1), leadz(0_2), leadz(b8), trailz(i), trailz(0), trailz(b2)
print *, bit_size(i), bit_size(b1), bit_size(b2), bit_size(b8)
j = 2
print *, dshiftl(1, 2**30, 2), dshiftr(1, 16, 3), dshiftl(i, 0, j), dshiftr(0, i, j)
print *, dshiftl(-1_1, 0_1, 8_1), dshiftr(5_8, 0_8, 64), dshiftl(3, 5, 0)
print *, trailz([1, 2, 4, 8]), popcnt([7, 15]), leadz([1_2, 256_2])
end program
""", "2 8 63 0 1\n25 0 16 1 5 32 8\n32 8 16 64\n5 536870914 384 24\n0 5 3\n0 1 2 3 3 4 15 7\n")

    def test_bit_and_logical_reductions(self):
        self.check("""program p
implicit none
integer :: a(4), m(2,3)
logical :: l(3)
a = [12, 10, 6, 3]
m = reshape([1, 2, 4, 8, 16, 32], [2,3])
l = [.true., .false., .true.]
print *, iany(a), iall(a), iparity(a)
print *, iany(a, mask=a > 5), iall(a, [.true., .true., .false., .false.]), iparity(m, dim=2)
print *, iany(m, 1), iall(m, dim=1, mask=m > 1)
print *, parity(l), parity(reshape([l, l], [3,2]), dim=1)
end program
""", "15 0 3\n14 8 21 42\n3 12 48 2 0 0\nF F F\n")

    def test_bessel_norm2_out_of_range_and_specific_names(self):
        self.check("""program p
implicit none
double precision :: x
real :: y(2,3)
integer :: i
integer(1) :: s1
x = 2.5d0
y = reshape([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], [2,3])
print *, bessel_j0(x), bessel_j1(x), bessel_jn(3, x)
print *, bessel_y0(x), bessel_y1(x), bessel_yn(2, x)
print *, norm2(y), norm2(y, dim=1), norm2([3.0, 4.0])
i = 130
s1 = 0_1
print *, out_of_range(i, s1), out_of_range(-128, s1), out_of_range(127, s1)
print *, out_of_range(-128.5, s1), out_of_range(-128.5, s1, .true.), out_of_range(1.0e10, i)
print *, out_of_range(1.0d300, 1.0), out_of_range(3.0d0, 1.0), out_of_range(5, 1.0)
i = -7
print *, iabs(i), float(i), isign(3, i), amod(7.5, 2.0), max0(1, 5, 3), dsqrt(x)
end program
""", "-4.8383776468197914E-002 0.49709410246427399 0.21660039103911310\n"
            "0.49807035961523183 0.14591813796678582 -0.38133584924180319\n"
            "9.53939152 2.23606801 5.00000000 7.81024933 5.00000000\n"
            "T F F\nF T T\nT F F\n7 -7.00000000 -3 1.50000000 5 1.5811388300841898\n")

    def test_narrowing_assignment_wraps(self):
        self.check("""program p
implicit none
integer :: i, big8
integer(8) :: wide
integer(1) :: n1, a1(2)
integer(2) :: n2
i = 130
n1 = i
wide = 5000000000_8
big8 = wide
n2 = 40000 + i - 130
a1(1) = n1 + 1
a1(2) = 200 + i - 130
print *, n1, n2, big8, a1
end program
""", "-126 -25536 705032704 -125 -56\n")

    def test_shift_out_of_range_is_rejected(self):
        self.check("""program p
integer :: s
s = 33
print *, dshiftl(1, 2, s)
end program
""", error="DSHIFTL: SHIFT must be between 0 and 32")


if __name__ == "__main__":
    unittest.main(verbosity=2)

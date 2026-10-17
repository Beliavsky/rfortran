"""LGE, LGT, LLE, LLT and ISHFTC.

These were accepted but returned zero before they were implemented, so the
cases check values rather than only successful compilation. Each case is
checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_intrinsic_rewrites.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class IntrinsicRewrites(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_lexical_comparisons(self):
        self.check("""program main
implicit none
character(len=4) :: a = 'abc', b = 'abd'
print *, lge('b', 'a'), lgt('b', 'a'), lle('b', 'a'), llt('b', 'a')
print *, lge(a, b), lgt(a, b), lle(a, b), llt(a, b)
print *, lge('ab', 'ab  '), llt('ab', 'ab '), lgt('a', ''), lle(string_a='Z', string_b='a')
if (llt(a, b)) print *, 'ordered'
end program
""", "T T F F\nF F T T\nT F T T\nordered\n")

    def test_ishftc_values_and_kinds(self):
        self.check("""program main
implicit none
integer :: i, s
integer(1) :: b = 3_1
integer(8) :: l = 5_8
print *, ishftc(1, 1), ishftc(6, 1), ishftc(1, -1), ishftc(-2, 1)
print *, ishftc(b, 7), ishftc(l, -1), ishftc(13, 2, 4), ishftc(13, -1, 4)
do s = -3, 3
  i = ishftc(11, s, 5)
  print *, s, i
end do
print *, ishftc(i=6, shift=-1), ishftc(size=3, shift=1, i=5)
end program
""", "2 12 -2147483648 -3\n-127 -9223372036854775806 7 14\n"
            "-3 13\n-2 26\n-1 21\n0 11\n1 22\n2 13\n3 26\n3 3\n")

    def test_user_procedure_keeps_its_name(self):
        self.check("""program main
implicit none
print *, lge(1, 2)
contains
integer function lge(a, b)
  integer, intent(in) :: a, b
  lge = a + b
end function lge
end program
""", "3\n")

    def test_degree_and_math_library_functions(self):
        self.check("""program p
implicit none
real :: x(3) = [-1.0, 0.5, 1.0]
double precision :: d = 0.5d0
print '(7f8.3)', sind(30.0), cosd(60.0), tand(45.0), asind(0.5), acosd(0.5), atand(1.0), atan2d(1.0, 1.0)
print '(3f8.3)', atan(1.0, -1.0), sind(30d0), asind(d)
print '(3f8.2)', acosd(x)
print '(4f9.5)', asinh(1.0), acosh(2.0), atanh(0.5), asinh(d)
print '(5f9.4)', erf(0.5), erfc(0.5), gamma(5.0), log_gamma(10.0), hypot(3.0, 4.0)
print '(6f5.1)', aint(2.7), aint(-2.7), anint(2.5), anint(-2.5), aint(1.5d0), anint(d)
print '(3f5.1)', aint(x*2.6)
end program
""", "0.500 0.500 1.000 30.000 60.000 45.000 45.000\n2.356 0.500 30.000\n180.00 60.00 0.00\n"
     "0.88137 1.31696 0.54931 0.48121\n0.5205 0.4795 24.0000 12.8018 5.0000\n"
     "2.0 -2.0 3.0 -3.0 1.0 1.0\n-2.0 1.0 2.0\n", std="f2023")

    def test_iso_fortran_env_entities(self):
        self.check("""program p
use iso_fortran_env, only: integer_kinds, real_kinds, output_unit, error_unit, input_unit, &
  iostat_end, iostat_eor, compiler_version, character_storage_size
implicit none
print *, iostat_end, iostat_eor, is_iostat_end(-1), is_iostat_eor(-1)
print *, integer_kinds(2), real_kinds(2), input_unit, output_unit, error_unit, character_storage_size
write(output_unit, '(l1)') len_trim(compiler_version()) > 0
end program
""", "-1 -2 T F\n2 8 5 6 0 8\nT\n")


if __name__ == "__main__":
    unittest.main()

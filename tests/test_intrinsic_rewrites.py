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


if __name__ == "__main__":
    unittest.main()

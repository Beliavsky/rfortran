"""EQUIVALENCE: scalar sets, sets merged through a shared object, the choice of
the object that keeps the storage, COMMON objects, SAVEd objects, and REAL
and INTEGER objects of the same size sharing bits, with scalar TRANSFER
between them.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_equivalence_storage.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest

import test_deallocate


class EquivalenceStorage(unittest.TestCase):
    check = test_deallocate.Deallocate.check
    reject = test_deallocate.Deallocate.reject

    def test_bits_of_real_and_integer(self):
        self.check("""program p
real s
integer i
double precision d
integer(8) k
character(len=10) :: line
equivalence (s, i), (d, k)
s = 1.0
print *, i
i = 1077936128
print *, s, s * 2
write (line, '(f6.2)') s
print *, line
d = 2.5d0
print *, k, mod(k, 1000_8)
k = k + 1
print *, d > 2.5d0
end program
""", "1065353216\n3.0 6.0\n3.00\n4612811918334230528 528\nT\n", std="legacy")

    def test_scalar_transfer(self):
        self.check("""program p
real s, t
integer i
s = 1.0
i = transfer(s, i)
t = transfer(1077936128, t)
print *, i, t, transfer(-1.5, 0)
end program
""", "1065353216 3.0 -1077936128\n")

    def test_primary_choice_and_merged_sets(self):
        # The object passed to the subroutine keeps its storage, whatever
        # the order of the set; (b, a) and (c, b) form one group.
        self.check("""program p
integer i
real x
real a(4), b(2), c
equivalence (i, x)
equivalence (b(1), a(3)), (c, b(2))
x = 0.5
call show(x)
a = [1., 2., 3., 4.]
print *, b(1), b(2), c
c = 40.
print *, a(4)
end program
subroutine show(r)
real r
print *, r * 2
end subroutine
""", "1.0\n3.0 4.0 4.0\n40.0\n")

    def test_common_and_saved_objects(self):
        self.check("""program prog
common /block/ i
equivalence (a, b, c), (i, j, k, l)
a = 1.0
b = 2.0
c = 3.0
i = 1
j = 2
k = 3
l = 4
print *, a, b, c, i, j, k, l
call foo
call foo
end program
subroutine foo
integer n, g, h
data n /0/
equivalence (g, h)
save g
if (n == 0) then
  n = 1
  h = 12345
end if
print *, g
end subroutine
""", "3.0 3.0 3.0 4 4 4 4\n12345\n12345\n")

    def test_rejections(self):
        self.reject("""program p
double precision d
integer m(2)
equivalence (d, m(1))
end program
""", "whose elements have different sizes")
        # Only one object of a set can keep its storage.
        self.reject("""program p
real x
integer i
equivalence (i, x)
read *, i
call show(x)
end program
subroutine show(r)
real r
end subroutine
""", "reading into it or passing it to a procedure is not yet supported")


if __name__ == "__main__":
    unittest.main()

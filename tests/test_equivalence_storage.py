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

    def test_common_storage_association(self):
        # EQUIVALENCE extends /com1/ past b into c, and the units of /two/
        # declare different layouts. Whole arrays become sections of the
        # block's storage, and are passed by their first element.
        self.check("""program p
real(8) a(8), b(5), c(5)
common /com1/ b, c
equivalence (a(1), b(2))
real x(10)
common /two/ x
b = 100
c = 200
print *, a
call par(a, b, c)
x = [(real(i), i = 1, 10)]
call halves
end program
subroutine par(a, b, c)
real(8) a(8), b(5), c(5)
print *, sum(a), sum(b), sum(c)
end subroutine
subroutine halves
real lo(5), hi(5)
common /two/ lo, hi
print *, lo(5), hi(1), sum(hi)
end subroutine
""", "100.0 100.0 100.0 100.0 200.0 200.0 200.0 200.0\n1200.0 500.0 1000.0\n5.0 6.0 40.0\n")

    def test_elements_of_different_sizes(self):
        # The D1MACH idiom: INTEGER*4 pairs over DOUBLE PRECISION elements.
        self.check("""program p
double precision dmach(2)
integer small(2), large(2)
equivalence (dmach(1), small(1)), (dmach(2), large(1))
dmach(1) = 1.0d13
print *, small
large(1) = 0
large(2) = 1072693248
print *, dmach(2), dmach(1) + dmach(2)
end program
""", "-448790528 1117925532\n1.0 10000000000001.0\n")

    def test_character_storage(self):
        self.check("""program p
character*8 c
character*1 d(2), f(2)
character*4 e
equivalence (c(1:1), d(1)), (c(3:5), e(2:4)), (c(6:6), f(2))
c = 'abcdefgh'
print *, d(1), d(2), ' ', e, ' ', f(1), f(2)
e = 'WXYZ'
print *, c
call show
end program
subroutine show
character(len=6) s, t
common /block/ s, t
s = 'Hello '
t = 'World '
call both
end subroutine
subroutine both
character(len=12) c
common /block/ c
print *, c
end subroutine
""", "ab bcde ef\naWXYZfgh\nHello World\n")

    def test_initial_values_and_local_storage(self):
        self.check("""program p
call one
call two
end program
subroutine one
integer :: a(3)
integer :: x = 1
integer :: z = 3
equivalence (a(1), x), (a(3), z)
print *, a(1), a(3), x, z
end subroutine
subroutine two
integer a(2), b(2), c
equivalence (a(2), b(1)), (b(2), c)
data a /1, 2/
data c /3/
print *, b, c
b(2) = 30
print *, c
end subroutine
""", "1 3 1 3\n2 3 3\n30\n")

    def test_rejections(self):
        self.reject("""program p
character(len=4) s
integer m
equivalence (s, m)
end program
""", "storage association of `s` with `m`, of types CHARACTER(len=4) and INTEGER(4), is not yet supported")
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

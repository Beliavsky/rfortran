"""Array-valued expressions in PRINT and WRITE output lists.

Elemental expressions of fixed-size arrays, such as X*2 or SQRT(X), print each
element. Earlier builds printed only the first element.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_print_array_expressions.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class PrintArrayExpressions(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_runtime_array_items(self):
        self.check("""program p
implicit none
integer :: n
read *, n
call work(n)
contains
subroutine work(n)
integer, intent(in) :: n
integer :: a(n), b(n)
a=2
b=0
print *, int(a), b
write(*,*) b, int(a), 9
print *, int(a)
print *, a+1, -a, b
print *, sum(a), int(a), size(b)
print *, 'values:', int(a), b
end subroutine
end program
""", "2 2 2 0 0 0\n0 0 0 2 2 2 9\n2 2 2\n"
     "3 3 3 -2 -2 -2 0 0 0\n6 2 2 2 3\nvalues: 2 2 2 0 0 0\n", data="3\n")

    def test_empty_runtime_array_items(self):
        self.check("""program p
implicit none
integer :: n
read *, n
call work(n)
contains
subroutine work(n)
integer, intent(in) :: n
integer :: a(n), b(n)
a=2
b=0
print *, 1, int(a), b, 9
print *, int(a), b
end subroutine
end program
""", "1 9\n\n", data="0\n")

    def test_runtime_elemental_intrinsics_and_character_items(self):
        self.check("""program p
implicit none
integer :: n
read *, n
call work(n)
contains
subroutine work(n)
integer, intent(in) :: n
real :: a(n)
integer :: b(n), fixed(2)
character(1) :: c(2)
a=4
b=0
fixed=[1,2]
c=['x','y']
print *, int(a), c, b
print *, c, fixed*2, int(a)
write(*,*) sqrt(a), abs(-a), b
print *, real(b), dble(b), nint(a), b
print *, a>3, b
print '(3i2,3i2)', int(a), b
end subroutine
end program
""", "4 4 4 xy 0 0 0\nxy 2 4 4 4 4\n"
     "2.0 2.0 2.0 4.0 4.0 4.0 0 0 0\n"
     "0.0 0.0 0.0 0.0 0.0 0.0 4 4 4 0 0 0\n"
     "T T T 0 0 0\n4 4 4 0 0 0\n", data="3\n")

    def test_list_directed(self):
        self.check("""program p
implicit none
real :: x(3), m(2,2)
integer :: i(2)
x = [4.0, 9.0, 16.0]
i = [1, 2]
m = 1
print *, x*2
print *, 1, x+x, 2
print *, i*2, -i
print *, x(1:2)*2
write(*,*) sqrt(x)
print *, nint(x), abs(-x)
print *, x > 5.0
print *, real(i), sum(x*2), x(2)*2
print *, m*2
end program
""", "8.0 18.0 32.0\n1 8.0 18.0 32.0 2\n2 4 -1 -2\n8.0 18.0\n2.0 3.0 4.0\n"
     "4 9 16 4.0 9.0 16.0\nF T T\n1.0 2.0 58.0 18.0\n2.0 2.0 2.0 2.0\n")

    def test_formatted(self):
        self.check("""program p
implicit none
real :: x(3), m(2,2)
integer :: i(2)
x = [4.0, 9.0, 16.0]
i = [1, 2]
m = 1
print '(3f6.2)', sqrt(x)
write(*,'(3f6.1)') x*2
print '(2i3,3f5.1)', i*2, abs(-x)
print '(4f5.1)', m+1
end program
""", "2.00 3.00 4.00\n8.0 18.0 32.0\n2 4 4.0 9.0 16.0\n2.0 2.0 2.0 2.0\n")

    def test_formatted_array_functions(self):
        self.check("""program p
implicit none
integer :: i1(5), m(2,3)
i1 = [10, 20, 30, 40, 50]
m = 0
print '(5i3)', cshift(i1, -2)
write(*,'(5i3)') eoshift(i1, 1)
print '(2i3)', pack(i1, i1 > 30)
print '(5i3)', spread(7, 1, 5)
print '(5i3)', [i1(2:3), 7]
print '(5i3)', reshape(i1, [5])
print '(5i3)', shape(m), size(m)
print '(i4,5i3)', sum(i1), i1
end program
""", "40 50 10 20 30\n20 30 40 50 0\n40 50\n7 7 7 7 7\n20 30 7\n10 20 30 40 50\n2 3 6\n150 10 20 30 40 50\n")


if __name__ == "__main__":
    unittest.main()

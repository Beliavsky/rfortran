"""MATMUL of ALLOCATABLE, assumed-shape and explicit-shape runtime-sized
arrays, with nondefault lower bounds, rank-one operands, mixed types,
expression and TRANSPOSE operands, and the runtime shape check.

Each case is checked in the interpreter, at -O0 and -O2, and against gfortran.
Run after cargo build --bin rfort: python tests/test_runtime_matmul.py
Set RFORT_TEST_COMPILER to check another compiler build.
"""
import unittest
import test_deallocate


class RuntimeMatmul(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_runtime_sized_products(self):
        self.check("""program m4
implicit none
real, allocatable :: a(:,:), b(:,:), c(:,:), v(:), w(:)
double precision, allocatable :: d(:,:)
integer, allocatable :: ia(:,:), ib(:,:)
integer :: i, j, it
allocate(a(0:1,3), b(-1:1,2), v(3), d(2,2), ia(2,2), ib(2,2))
do j = 1, 3
   do i = 0, 1
      a(i,j) = i + 10*j
   end do
end do
do j = 1, 2
   do i = -1, 1
      b(i,j) = i - j
   end do
end do
v = [1.0, 2.0, 3.0]
d = reshape([1d0, 2d0, 3d0, 4d0], [2,2])
ia = reshape([1, 2, 3, 4], [2,2])
ib = reshape([5, 6, 7, 8], [2,2])
c = matmul(a, b)
print *, shape(c), c
w = matmul(a, v)
print *, w
w = matmul(v, b)
print *, w
print *, matmul(ia, ib)
print *, sum(matmul(d, d)), matmul(d, ia)
do it = 1, 3
   c = matmul(c, reshape([1.0, 0.0, 0.0, 1.0], [2,2]) * 0 + c)
end do
print *, c
call sub(a, b, 2, 3)
print *, total(d)
contains
subroutine sub(x, y, n, m)
real, intent(in) :: x(:,:), y(:,:)
integer, intent(in) :: n, m
real :: e(n, m)
e = x
print *, matmul(x, y), maxval(matmul(e, y))
end subroutine
double precision function total(x)
double precision, intent(in) :: x(:,:)
total = sum(matmul(x, transpose(x)))
end function
end program
""", "2 2 -40.0 -43.0 -100.0 -106.0\n140.0 146.0\n-4.0 -10.0\n23 34 31 46\n"
            "54.0 7.0 10.0 15.0 22.0\n5.81117665E+16 6.18366198E+16 1.43806096E+17 1.53023792E+17\n"
            "-40.0 -43.0 -100.0 -106.0 -40.0\n58.0\n")

    def test_nonconforming_shapes_stop(self):
        self.check("""program m7
real, allocatable :: a(:,:), b(:,:)
allocate(a(2,3), b(2,2))
a = 1
b = 1
print *, matmul(a, b)
end program
""", error="MATMUL operand shapes do not conform")


if __name__ == "__main__":
    unittest.main(verbosity=2)

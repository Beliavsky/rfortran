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

    def test_correlation_function(self):
        from pathlib import Path
        source = (Path(__file__).parent / 'programs/xcorr_mat_function.f90').read_text()
        self.check(source, "1 1 1 1\n")

    def test_fixed_call_heap_temporaries(self):
        # Fixed call-site shapes select function inlining, but MATMUL locals
        # still own runtime descriptors rather than fixed backing arrays.
        template = """module m
implicit none
contains
function gram(x) result(y)
KIND,intent(in)::x(:,:)
KIND::y(size(x,2),size(x,2))
y=matmul(transpose(x),x)
end function
end module
program main
use m
implicit none
KIND::x(-1:1,4:5),empty(0,2)
integer::i
x(:,4)=[1,2,3]
x(:,5)=[3,2,1]
do i=1,3
print *,gram(x)
end do
print *,gram(empty)
end
"""
        for kind in ['integer', 'real', 'double precision']:
            with self.subTest(kind=kind):
                self.check(template.replace('KIND', kind), "14 10 10 14\n"*3+"0 0 0 0\n")

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

    def test_mixture_section_reproducer(self):
        from test_record_read import ROOT
        self.check((ROOT / 'tests/programs/xmix_matmul_section.f90').read_text(),
                   '17 29 30 42')

    def test_strided_sections_and_overlap(self):
        self.check("""implicit none
integer,allocatable :: a(:,:), b(:,:), v(:)
integer :: i,j,n
n=4
allocate(a(-1:2,0:3),b(2,2),v(-2:1))
do j=0,3
do i=-1,2
a(i,j)=i+2+10*j
end do
end do
b=reshape([1,0,0,1],[2,2])
v=[1,2,3,4]
print *,matmul(a(2:-1:-2,3:0:-2),b)
print *,matmul(a(-1:2:2,0:3:2),v(-2:1:2))
print *,matmul(v(-2:1:2),a(-1:2:2,0:3:2))
a(-1:2:2,0:3:2)=matmul(a(2:-1:-2,3:0:-2),b)
print *,a(-1:2:2,0:3:2)
end
""", '34 32 14 12 64 72 10 90 34 32 14 12')

    def test_selector_evaluated_once(self):
        self.check("""implicit none
integer,allocatable :: a(:,:,:),v(:)
integer :: calls
allocate(a(2,2,2),v(2))
a=2
v=3
calls=0
print *,matmul(a(:,:,next_index()),v)
print *,calls
contains
integer function next_index()
calls=calls+1
next_index=1
end function
end
""", '12 12 1', reference=False)
        # This asserts rfort's single-evaluation contract. The installed
        # gfortran evaluates this scalar section selector twice.

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

"""ELEMENTAL functions of array constructors in formatted PRINT, list-directed
and formatted output of COMPLEX sections and runtime-sized COMPLEX arrays,
sections of multidimensional assumed-shape COMPLEX dummies, and implied-DO
indices with an INTEGER type-spec (Fortran 2008)."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_deallocate
from test_record_read import COMPILER


class OutputItemFixes(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_elemental_of_constructor_in_formatted_print(self):
        self.check("""module m
implicit none
integer, parameter :: dp = kind(1d0)
contains
elemental logical function negative(x)
real(dp), intent(in) :: x
negative = x < 0
end function negative
elemental real(dp) function twice(x)
real(dp), intent(in) :: x
twice = 2 * x
end function twice
end module m
program main
use m
implicit none
print '(3l2)', negative([-1.0_dp, 1.0_dp, -2.0_dp])
print '(3f5.1)', twice([0.5_dp, 1.0_dp, -2.0_dp])
print '(3f5.1)', twice([0.5_dp, 1.0_dp, -2.0_dp]) + 1
end program main
""", " T F T\n  1.0  2.0 -4.0\n  2.0  3.0 -3.0\n")

    def test_complex_sections_list_directed(self):
        # rfort writes each COMPLEX value with G0-style parts, unlike gfortran.
        self.check("""module m
implicit none
contains
subroutine show(a)
complex, intent(in) :: a(:,:)
print *, a
end subroutine show
end module m
program main
use m
implicit none
complex :: z(4), s(3,2)
complex, allocatable :: w(:)
integer :: i, k
z = [(cmplx(i, -i), i = 1, 4)]
s = reshape([(cmplx(i, 0), i = 1, 6)], [3, 2])
print *, z(2:3)
print *, 'x', z(1:4:2), 5
print *, s(3, :)
print *, s(:, 2)
k = 2
print *, 'x', z(k:4:2), 5
k = 5
print *, 'empty', z(k:4), 'end'
allocate(w(2)); w = (1.5, -0.5)
print *, w
call show(s(1:2, :))
end program main
""", """ (2.00000000,-2.00000000) (3.00000000,-3.00000000)
 x (1.00000000,-1.00000000) (3.00000000,-3.00000000) 5
 (3.00000000,0.00000000) (6.00000000,0.00000000)
 (4.00000000,0.00000000) (5.00000000,0.00000000) (6.00000000,0.00000000)
 x (2.00000000,-2.00000000) (4.00000000,-4.00000000) 5
 empty end
 (1.50000000,-0.500000000) (1.50000000,-0.500000000)
 (1.00000000,0.00000000) (2.00000000,0.00000000) (4.00000000,0.00000000) (5.00000000,0.00000000)
""", reference=False)

    def test_sections_of_complex_dummies(self):
        # Rows, columns and strided sections of rank-2 and rank-3
        # assumed-shape COMPLEX dummies, assigned to automatic and ALLOCATABLE
        # arrays, printed and passed on; and rows of a fixed-size REAL matrix
        # assigned to an ALLOCATABLE vector.
        self.check("""module m
implicit none
contains
subroutine take(v)
complex, intent(in) :: v(:)
print '(*(f0.1,:,1x))', v
end subroutine take
subroutine rows(a)
complex, intent(in) :: a(:,:)
complex :: r(size(a,2)), c(size(a,1))
complex, allocatable :: w(:)
r = a(2,:)
print '(*(f0.1,:,1x))', r
c = a(:,3)
print '(*(f0.1,:,1x))', c
w = a(1,1:3:2) * 2
print '(*(f0.1,:,1x))', w
print '(*(f0.1,:,1x))', a(2,:)
call take(a(1,:))
end subroutine rows
subroutine cube(b)
complex(8), intent(in) :: b(:,:,:)
complex(8) :: p(size(b,2), size(b,3))
p = b(2,:,:)
print '(*(f0.1,:,1x))', p
end subroutine cube
end module m
program main
use m
implicit none
complex :: a(2,3)
complex(8) :: b(2,2,2)
real :: x(2,3)
real, allocatable :: q(:)
integer :: i
a = reshape([(cmplx(i, -i), i = 1, 6)], [2, 3])
b = reshape([(cmplx(i, 10*i, kind=8), i = 1, 8)], [2, 2, 2])
call rows(a)
call cube(b)
x = reshape([(real(i), i = 1, 6)], [2, 3])
q = x(2,:)
print '(*(f0.1,:,1x))', q
q = 2 * real(x(1,2:3))
print '(*(f0.1,:,1x))', q
end program main
""", """2.0 -2.0 4.0 -4.0 6.0 -6.0
5.0 -5.0 6.0 -6.0
2.0 -2.0 10.0 -10.0
2.0 -2.0 4.0 -4.0 6.0 -6.0
1.0 -1.0 3.0 -3.0 5.0 -5.0
2.0 20.0 4.0 40.0 6.0 60.0 8.0 80.0
2.0 4.0 6.0
6.0 10.0
""")

    def test_complex_dummy_sections_in_output_lists(self):
        self.check("""module m
implicit none
contains
subroutine show(a, x)
complex, intent(in) :: a(:,:)
real, intent(in) :: x(:)
integer :: i
do i = 1, size(a, 1)
   print '(i0,*(1x,f0.1))', i, a(i,:), x(1)
   print '(i0,*(1x,g0))', i, a(i,2:), size(a)
end do
end subroutine show
end module m
program p
use m
implicit none
complex :: c(2,3)
real :: y(2) = 5
integer :: i
c = reshape([(cmplx(i, -i), i = 1, 6)], [2, 3])
call show(c, y)
end program p
""", """1 1.0 -1.0 3.0 -3.0 5.0 -5.0 5.0
1 3.00000000 -3.00000000 5.00000000 -5.00000000 6
2 2.0 -2.0 4.0 -4.0 6.0 -6.0 5.0
2 4.00000000 -4.00000000 6.00000000 -6.00000000 6
""")
        # List-directed, with other items in the list, in a loop.
        self.check("""module m
implicit none
contains
subroutine show(a, x)
complex, intent(in) :: a(:,:)
real, intent(in) :: x(:)
integer :: i
do i = 1, size(a, 1)
   print *, i, a(i,:i+1), x(1)
   print '(i0,*(1x,g0))', i, a(i,2:), size(a)
end do
end subroutine show
end module m
program p
use m
implicit none
complex :: c(2,3)
real :: y(2) = 5
integer :: i
c = reshape([(cmplx(i, -i), i = 1, 6)], [2, 3])
call show(c, y)
end program p
""", """ 1 (1.00000000,-1.00000000) (3.00000000,-3.00000000) 5.0
 1 3.00000000 -3.00000000 5.00000000 -5.00000000 6
 2 (2.00000000,-2.00000000) (4.00000000,-4.00000000) (6.00000000,-6.00000000) 5.0
 2 4.00000000 -4.00000000 6.00000000 -6.00000000 6
""", reference=False)

    def test_typed_implied_do_index(self):
        # gfortran does not accept implied-DO type-specs.
        self.check("""module m
implicit none
integer, parameter :: ik = selected_int_kind(12)
integer, parameter :: sq(4) = [(k*k, integer :: k = 1, 4)]
contains
function evens(n) result(v)
integer, intent(in) :: n
integer :: v(n)
v = [(2*j, integer(ik) :: j = 1, n)]
end function evens
end module m
program main
use m
implicit none
integer :: i, t(2,3)
i = 99
! The index is local to the implied-DO: I keeps its value.
print '(*(i0,:,1x))', [(i**2, integer :: i = 1, 3)], i
print '(*(i0,:,1x))', sq
print '(*(i0,:,1x))', evens(3)
t = reshape([((10*r + c, integer :: r = 1, 2), integer :: c = 1, 3)], [2, 3])
print '(*(i0,:,1x))', t
block
   print '(*(i0,:,1x))', [(i + q, integer(kind=8) :: q = 1, 2)]
end block
end program main
""", "1 4 9 99\n1 4 9 16\n2 4 6\n11 21 12 22 13 23\n100 101\n", reference=False)

    def test_typed_implied_do_index_standard(self):
        with tempfile.TemporaryDirectory(prefix='typed-implied-do-') as tmp:
            source = Path(tmp) / 'example.f90'
            source.write_text('print *, [(i, integer :: i = 1, 3)]\nend\n', encoding='utf-8')
            run = lambda std: subprocess.run([COMPILER, f'-std={std}', 'check', source], cwd=tmp,
                                             capture_output=True, text=True, timeout=60)
            result = run('f2003')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('implied-DO index type-specs requires Fortran 2008', result.stderr)
            self.assertEqual(run('f2008').returncode, 0)


if __name__ == "__main__":
    unittest.main()

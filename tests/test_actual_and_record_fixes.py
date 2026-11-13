"""MATMUL results passed directly as actual arguments to subroutines and
functions, sections of fixed-size arrays assigned to multidimensional
ALLOCATABLE arrays, and internal WRITE to substrings, including ones with
run-time bounds."""
import unittest

import test_deallocate


class ActualAndRecordFixes(unittest.TestCase):
    check = test_deallocate.Deallocate.check

    def test_matmul_actual_arguments(self):
        self.check("""module m
implicit none
contains
subroutine show(x)
real(8), intent(in) :: x(:,:)
print '(f0.1,1x,i0,1x,i0)', sum(x), size(x, 1), size(x, 2)
end subroutine show
subroutine fixed(v, n)
integer, intent(in) :: n
integer, intent(in) :: v(n)
print '(*(i0,:,1x))', v
end subroutine fixed
subroutine twice(x, y)
real(8), intent(in) :: x(:)
real(8), intent(out) :: y(:)
y = 2 * x
end subroutine twice
real(8) function total(v)
real(8), intent(in) :: v(:)
total = sum(v)
end function total
end module m
program main
use m
implicit none
real(8) :: a(2,3), b(3,2), v(3), y(2)
integer :: ia(2,2), iv(2), i
a = 1; b = 2; v = [1d0, 2d0, 3d0]
ia = reshape([1, 2, 3, 4], [2, 2]); iv = [1, 1]
call show(matmul(a, b))
print '(f0.1)', total(matmul(a, v))
call fixed(matmul(ia, iv), 2)
call fixed(matmul(iv, ia), 2)
call twice(matmul(a, v), y)
print '(*(f0.1,:,1x))', y
call twice(matmul(a(:,1:2), v(1:2)), y)
print '(*(f0.1,:,1x))', y
do i = 1, 2
   call fixed(matmul(ia * i, iv), 2)
end do
call inner(matmul(a, v))
contains
subroutine inner(w)
real(8), intent(in) :: w(:)
print '(*(f0.1,:,1x))', w
end subroutine inner
end program main
""", "24.0 2 2\n12.0\n4 6\n3 7\n12.0 12.0\n6.0 6.0\n4 6\n8 12\n6.0 6.0\n")

    def test_fixed_section_to_allocatable_matrix(self):
        self.check("""program main
implicit none
integer :: c(3,4,2), i
integer, allocatable :: b(:,:)
c = reshape([(i, i = 1, 24)], [3, 4, 2])
b = c(:,2:4:2,2)
print '(*(i0,:,1x))', shape(b), b
b = c(3:1:-1,1,:)
print '(*(i0,:,1x))', shape(b), b
end program main
""", "3 2 16 17 18 22 23 24\n3 2 3 2 1 15 14 13\n")

    def test_internal_write_to_substring(self):
        self.check("""program main
implicit none
character(len=30) :: buf
integer :: i, k, ios
buf = ''
k = 1
do i = 1, 4
   write (buf(k:), '(i0,",")') i * i
   k = len_trim(buf) + 1
end do
print '(a)', '[' // trim(buf) // ']'
buf = 'abcdefghij'
write (buf(:3), '(a)') 'XY'
print '(a)', trim(buf)
write (buf(2:5), '(i4)', iostat=ios) 12
print '(a,1x,i0)', trim(buf), ios
end program main
""", "[1,4,9,16,]\nXY defghij\nX  12fghij 0\n")
        # The record is the substring, not the whole variable. gfortran
        # reports this as "End of record".
        self.check("""program main
implicit none
character(len=10) :: buf
integer :: k
k = 7
write (buf(k:k+1), '(a)') 'xyz'
end program main
""", error="internal WRITE record exceeds CHARACTER destination length", reference=False)


if __name__ == "__main__":
    unittest.main()

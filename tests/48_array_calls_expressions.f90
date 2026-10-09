module array_operations
implicit none
contains
subroutine transform(a, b, offset)
real(8), intent(in) :: a(:)
real(8), intent(out) :: b(:)
real(8), intent(in), optional :: offset
real(8), allocatable :: scratch(:)
integer :: n
real(8) :: shift
n = size(a)
shift = 0.0d0
if (present(offset)) shift = offset
allocate(scratch(n))
scratch = exp(a - maxval(a))
b = scratch / sum(scratch) + shift
end subroutine transform
end module array_operations
program test_arrays
use array_operations
implicit none
real(8), parameter :: values(3) = [0.0d0, 0.0d0, 0.0d0]
real(8) :: a(3), b(3)
integer :: j
call transform(values, b)
print '(3f10.6)', b(1), b(2), b(3)
call transform(values, b, 2.0d0)
print '(3f10.6)', b(1), b(2), b(3)
a = [1.0d0, 2.0d0, 3.0d0]
a = a + sum(a)
print '(3f8.2)', a(1), a(2), a(3)
print '(f8.2,l1)', sum((a-sum(a)/3.0d0)**2), any(a < 0.0d0)
do j = 1, 3
end do
print '(i0)', j
do j = 5, 3
end do
print '(i0)', j
do j = 1, 3
if (j == 2) cycle
end do
print '(i0)', j
do j = 1, 3
if (j == 2) exit
end do
print '(i0)', j
print '(i2,f4.1,a5,l2)', 123, 123.0d0, 'abc', .true.
end program test_arrays

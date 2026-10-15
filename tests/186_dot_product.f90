program dot_product_regression
use, intrinsic :: iso_fortran_env, only: real64, int64
implicit none
real :: a(3), empty(0)
real(real64) :: b(3)
integer(int64) :: i(3)
logical :: mask(3), other(3), empty_logical(0)
a = [1.0, 2.0, 3.0]
b = [2.0_real64, 3.0_real64, 4.0_real64]
i = [10000000001_int64, 2_int64, 3_int64]
mask = [.true., .false., .true.]
other = [.false., .true., .false.]
print '(3f8.1)', dot_product(a,a), dot_product(a,b), dot_product(b,a)
print *, kind(dot_product(a,b)), dot_product(i,[1,2,3])
print *, dot_product(mask,other), dot_product(.not.mask,other)
print '(f8.1)', dot_product(empty,empty)
print *, dot_product(empty_logical,empty_logical)
end program dot_product_regression

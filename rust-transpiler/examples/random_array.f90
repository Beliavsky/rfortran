program main
implicit none
integer, parameter :: pow10 = 3, n = 10**pow10, nprod=10
real :: x(n), xprod
integer :: i
call random_number(x)
print*, pow10, sum(x)/n, sum(x**2)/n, minval(x), maxval(x)
xprod = 1.0
do i=1,nprod
   xprod = xprod*x(i)
end do
print*,xprod,product(x(1:nprod))
end program main

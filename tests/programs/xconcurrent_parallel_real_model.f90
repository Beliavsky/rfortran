! Real model intrinsics in direct loops and PURE/SIMPLE callees.
use iso_fortran_env, only: real64
implicit none
integer :: i
real(real64) :: a(9000), y(9000)
a=6.0_real64
do concurrent (i=1:9000)
   call normalize(a(i),y(i))
   y(i)=nearest(y(i),1.0_real64)
end do
if (any(y<=0.75_real64)) stop 1
if (any(y/=0.75_real64+spacing(0.75_real64))) stop 2
print *, 'PASS'
contains
simple subroutine normalize(x,y)
   real(real64), intent(in) :: x
   real(real64), intent(out) :: y
   y=mantissa(x)
end subroutine
simple real(real64) function mantissa(x) result(y)
   real(real64), intent(in) :: x
   y=scale(x,-exponent(x))
end function
end

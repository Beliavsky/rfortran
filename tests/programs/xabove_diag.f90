! Tests an array-valued function with a rank-two array argument.
! Expected output: 4.0 7.0 8.0.
module above_diag_mod
implicit none
integer, parameter :: dp = selected_real_kind(15,307)
contains
function above_diag(xx) result(yy)
real(kind=dp), intent(in) :: xx(:,:)
real(kind=dp) :: yy((size(xx,1)*size(xx,1)-size(xx,1))/2)
integer :: i,j,iy,n1,n2
n1 = size(xx,1)
n2 = size(xx,2)
iy = 0
do i=1,n1
   do j=i+1,n2
      iy = iy+1
      yy(iy) = xx(i,j)
      if (iy == size(yy)) return
   end do
end do
end function above_diag
end module above_diag_mod

program main
use above_diag_mod
implicit none
real(kind=dp) :: x(3,3)
x = reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp,5.0_dp,6.0_dp, &
             7.0_dp,8.0_dp,9.0_dp],[3,3])
print '(3f8.1)',above_diag(x)
end program main

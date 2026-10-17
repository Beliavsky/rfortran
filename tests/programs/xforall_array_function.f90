! Reproduces the specialized array-valued function call inside FORALL.
! Expected rows: 0 1 3 6, followed by 0 4 9 15.
module cumulative_sum_mod
use iso_fortran_env, only: real64
implicit none
contains
pure function cumulative_sum(x) result(y)
real(real64), intent(in) :: x(:)
real(real64) :: y(size(x))
integer :: i
if (size(x) == 0) return
y(1) = x(1)
do i = 2, size(x)
   y(i) = y(i-1) + x(i)
end do
end function cumulative_sum
end module cumulative_sum_mod

program main
use iso_fortran_env, only: real64
use cumulative_sum_mod, only: cumulative_sum
implicit none
real(real64) :: x(3,2), y(4,2)
integer :: j
x(:,1) = [1.0_real64, 2.0_real64, 3.0_real64]
x(:,2) = [4.0_real64, 5.0_real64, 6.0_real64]
forall (j=1:size(x,2)) y(:,j) = [0.0_real64, cumulative_sum(x(:,j))] ! no output
! forall (j=1:size(x,2)) y(:,j) = 0.0_real64 ! gives output
print *, y(:,1)
print *, y(:,2)
print*,"done"
end program main

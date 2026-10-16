module swapm
implicit none
interface swap
  module procedure swap_i, swap_r
end interface
contains
elemental subroutine swap_i(a,b)
  integer, intent(inout) :: a,b
  integer :: t
  t=a; a=b; b=t
end subroutine
elemental subroutine swap_r(a,b)
  real, intent(inout) :: a,b
  real :: t
  t=a; a=b; b=t
end subroutine
end module
program p10
use swapm
implicit none
integer :: i(3)=[1,2,3], j(3)=[4,5,6]
real :: x=1., y=2.
call swap(i,j); call swap(x,y)
print *, i, j, x, y
end program

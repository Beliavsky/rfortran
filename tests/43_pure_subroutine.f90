module pure_output_subroutines
implicit none
contains
pure subroutine sub(i, i2, i3)
integer, intent(in) :: i
integer, intent(out) :: i2, i3
i2=2*i
i3=3*i
end subroutine sub
pure subroutine bump(i)
integer, intent(inout) :: i
i=i+1
end subroutine bump
pure subroutine scaled(x,y)
real, intent(in) :: x
real, intent(out) :: y
y=2*x
end subroutine scaled
pure subroutine nested(i,j)
integer, intent(in) :: i
integer, intent(out) :: j
integer :: local
call sub(i,j,local)
call bump(j)
end subroutine nested
end module pure_output_subroutines
program main
use pure_output_subroutines
implicit none
integer :: i,i2,i3,local
real :: y
i=10
local=99
call sub(i,i2,i3)
print *,i,i2,i3
call sub(4,i2,i3)
call bump(i2)
print *,i2,i3
call scaled(1.5,y)
print *,y
call nested(7,i2)
print *,i2,local
end program main

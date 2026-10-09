module generic_values
implicit none
private
public :: default, set_value, dp
integer, parameter :: dp = selected_real_kind(15,307)
interface default
module procedure int_default, real_default
module procedure double_default, pair_default, wide_default
end interface default
interface set_value
module procedure set_scalar, set_vector
end interface
contains
integer function int_default(x)
integer :: x
int_default = x + 10
end function int_default
real function real_default(x)
real :: x
real_default = x + 20.0
end function real_default
real(kind=dp) function double_default(x)
real(kind=dp) :: x
double_default = x + 30.0_dp
end function double_default
integer function pair_default(x,y)
integer :: x,y
pair_default = x + y
end function pair_default
integer function wide_default(x)
integer(8) :: x
wide_default = 40 + int(x)
end function wide_default
subroutine set_scalar(x)
real, intent(out) :: x
x = 4.0
end subroutine set_scalar
subroutine set_vector(x)
real, intent(out) :: x(:)
x = 5.0
end subroutine set_vector
end module generic_values

module generic_bridge
use generic_values, only: default, set_value, dp
implicit none
end module generic_bridge

program test_generic
use generic_bridge, only: default, set_value, dp
implicit none
real :: scalar, vector(2)
integer(8) :: wide
wide = 2
print *, default(1), default(1.0), default(1.0_dp)
print *, default(default(2)), default(3,4)
print *, default(wide), default(1+wide)
call set_value(scalar)
call set_value(vector)
print *, scalar, vector
print *, default(vector(1))
end program test_generic

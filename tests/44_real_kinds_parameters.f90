module real_constants
implicit none
private
integer, parameter, public :: dp = kind(1.0d0)
real(dp), parameter, public :: twopi = 2.0_dp * acos(-1.0_dp)
end module real_constants
program main
use real_constants
implicit none
real(dp) :: x
real, parameter :: integer_division = 3/2
x = 1.25d0
print *, kind(1.0d0), kind(1.0), real(3,dp), twopi
print *, x, 1.0d-3, max(1.0_dp,2.5_dp), min(1.0_dp,-2.5_dp)
print *, max(2,9,4), min(2,9,4), integer_division
print *, tiny(1.0_dp), tiny(1.0)
end program main

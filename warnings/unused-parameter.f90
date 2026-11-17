! Warning:    unused-parameter
! Enabled by: -Wall
! A named constant that nothing references. A constant used only in a kind,
! a bound or another constant's value counts as used.
program unused_parameter
implicit none
integer, parameter :: dp = kind(1.0d0)
integer, parameter :: n = 3
real(dp), parameter :: pi = 3.141592653589793_dp
real(dp), parameter :: e = 2.718281828459045_dp    ! expect: unused-parameter
integer, parameter :: max_iter = 100               ! expect: unused-parameter
real(dp) :: x(n)
x = pi
print *, sum(x)
end program unused_parameter

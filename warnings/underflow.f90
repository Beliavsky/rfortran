! Warning:    underflow
! Enabled by: -Wall
! A nonzero constant expression whose value is too small for its REAL kind
! and rounds to zero. Here the product is 1.0e-60, below the smallest
! default REAL, so x is 0. Use a wider kind or rescale the calculation.
program underflow
use iso_fortran_env, only: dp => real64
implicit none
real, parameter :: tiny_step = 1.0e-30
real :: x
real(dp) :: y
x = tiny_step*tiny_step              ! expect: underflow
y = 1.0e-30_dp*1.0e-30_dp
print *, x, y
end program underflow

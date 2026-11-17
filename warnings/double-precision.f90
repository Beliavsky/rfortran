! Warning:    double-precision
! Enabled by: -Wstyle
! DOUBLE PRECISION and DOUBLE COMPLEX. A kind parameter, as real(dp) with dp
! from ISO_FORTRAN_ENV, states the precision and changes in one place.
program double_precision
use iso_fortran_env, only: dp => real64
implicit none
double precision :: x          ! expect: double-precision
real(dp) :: y
x = 1.0d0/3
y = 1.0_dp/3
print *, x, y
end program double_precision

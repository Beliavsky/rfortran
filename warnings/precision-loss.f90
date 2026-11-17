! Warning:    precision-loss
! Enabled by: -Wall
! A default REAL literal or calculation stored in a double precision
! variable: x = sqrt(2.0) computes in single precision and then widens the
! result, so x has only about 7 correct digits. Exactly representable values
! such as 0.5 are not reported.
program precision_loss
use iso_fortran_env, only: dp => real64
implicit none
real(dp) :: x, y
x = sqrt(2.0)        ! expect: precision-loss
y = 0.1              ! expect: precision-loss
print *, x, y
x = sqrt(2.0_dp)
y = 0.5
print *, x, y
end program precision_loss

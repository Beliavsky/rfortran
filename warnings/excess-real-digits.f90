! Warning:    excess-real-digits
! Enabled by: -Wall
! A REAL literal with more significant digits than its kind holds: a default
! REAL guarantees 6 decimal digits and REAL(real64) 15, so the default REAL
! 3.141592653589793 keeps only about 7 of its 16. A missing kind suffix is
! the usual cause.
program excess_real_digits
use iso_fortran_env, only: dp => real64
implicit none
real :: x
real(dp) :: y
x = 3.141592653589793              ! expect: excess-real-digits
y = 3.14159265358979_dp
print *, x, y
x = 3.14159
print *, x
end program excess_real_digits

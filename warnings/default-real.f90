! Warning:    default-real
! Enabled by: -Wstyle
! A REAL or COMPLEX declaration without a kind. The default kind is usually
! single precision, too little for many calculations, and changing it means
! editing every declaration. A kind parameter states the precision.
program default_real
use iso_fortran_env, only: dp => real64
implicit none
real :: x                  ! expect: default-real
real(dp) :: y
x = 1.0
y = 2.0_dp
print *, x + y
end program default_real

! Warning:    real-literal-kind
! Enabled by: -Weverything
! A REAL literal without a kind suffix or D exponent, which is default REAL
! whatever it is assigned to. Writing the kind on every literal keeps a
! program consistent when its working precision changes.
program real_literal_kind
use iso_fortran_env, only: dp => real64
implicit none
real(dp) :: x
x = 1.5                 ! expect: real-literal-kind
print *, x
x = 1.5_dp
print *, x
end program real_literal_kind

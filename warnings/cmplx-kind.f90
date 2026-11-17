! Warning:    cmplx-kind
! Enabled by: -Wall
! CMPLX without KIND= returns default COMPLEX even for double precision
! arguments, so cmplx(x, y) silently drops their extra digits. Give the kind,
! as cmplx(x, y, kind=dp).
program cmplx_kind
use iso_fortran_env, only: dp => real64
implicit none
real(dp) :: x, y
complex(dp) :: z
x = 1.0_dp/3
y = 2.0_dp/3
z = cmplx(x, y)              ! expect: cmplx-kind
print *, z
z = cmplx(x, y, kind=dp)
print *, z
end program cmplx_kind

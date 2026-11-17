! Warning:    literal-kind
! Enabled by: -Wstyle
! A kind given as a number, as real(8), real*8 or 1.0_8. Kind numbers are
! not portable: compilers may number kinds differently. Use the names in
! ISO_FORTRAN_ENV, such as real64, or selected_real_kind.
program literal_kind
use iso_fortran_env, only: real64
implicit none
real(8) :: x                ! expect: literal-kind
real(real64) :: y
x = 1.0
y = 1.0_real64
print *, x + y
end program literal_kind

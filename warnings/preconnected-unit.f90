! Warning:    preconnected-unit
! Enabled by: -Wextra
! READ from unit 5, or WRITE to unit 6 or 0. Which units are preconnected,
! and to what, depends on the compiler; `*` and the INPUT_UNIT, OUTPUT_UNIT
! and ERROR_UNIT constants of ISO_FORTRAN_ENV are portable.
program preconnected_unit
use iso_fortran_env, only: output_unit
implicit none
write (6, *) "hello"                  ! expect: preconnected-unit
write (output_unit, *) "hello"
write (*, *) "hello"
end program preconnected_unit

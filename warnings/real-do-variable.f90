! Warning:    real-do-variable
! Enabled by: -Wall
! A DO loop with a REAL variable, deleted from the language in Fortran 95.
! The iteration count is computed from REAL values, so with a step such as
! 0.1 it depends on rounding. Loop over an integer and compute x from it.
program real_do_variable
implicit none
real :: x
integer :: i
do x = 0.0, 3.0          ! expect: real-do-variable
   print *, x
end do
do i = 0, 3
   x = i
   print *, x
end do
end program real_do_variable

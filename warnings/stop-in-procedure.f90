! Warning:    stop-in-procedure
! Enabled by: -Wstyle
! STOP in a function or subroutine ends the whole program from deep inside
! it, which a caller cannot prevent or handle. Return an error status
! instead, or use ERROR STOP for an unrecoverable error.
program stop_in_procedure
implicit none
print *, checked_sqrt(4.0)
contains
function checked_sqrt(x) result(r)
real, intent(in) :: x
real :: r
if (x < 0) stop "negative argument"          ! expect: stop-in-procedure
if (x > huge(x)/2) error stop "argument too large"
r = sqrt(x)
end function checked_sqrt
end program stop_in_procedure

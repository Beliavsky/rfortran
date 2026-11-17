! Warning:    implicit-typing
! Enabled by: -Wall, -Wstyle
! A variable typed by the implicit rules, without a declaration. Names
! starting with I to N are INTEGER and others REAL, so a misspelled name
! silently becomes a new variable. IMPLICIT NONE makes such names errors.
program implicit_typing
integer :: n
n = 10
total = 0.0              ! expect: implicit-typing
do k = 1, n              ! expect: implicit-typing
   total = total + k
end do
print *, total
end program implicit_typing

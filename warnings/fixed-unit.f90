! Warning:    fixed-unit
! Enabled by: -Wextra
! An OPEN of a constant unit number in a procedure or module. Another part
! of the program may already use that number; OPEN(NEWUNIT=u) picks a free
! one. A main program, which owns its units, is not reported.
program fixed_unit
implicit none
call save_value(1.5)
call save_value_newunit(2.5)
contains
subroutine save_value(x)
real, intent(in) :: x
open (20, file="fixed_unit.txt", status="replace")      ! expect: fixed-unit
write (20, *) x
close (20, status="delete")
end subroutine save_value
subroutine save_value_newunit(x)
real, intent(in) :: x
integer :: u
open (newunit=u, file="fixed_unit.txt", status="replace")
write (u, *) x
close (u, status="delete")
end subroutine save_value_newunit
end program fixed_unit

! Warning:    unreachable-code
! Enabled by: -Wall
! Statements that can never run: after STOP, RETURN, EXIT or CYCLE, or in a
! branch whose condition is a constant. One warning marks the first
! statement of each unreachable sequence.
program unreachable_code
implicit none
logical, parameter :: debug = .false.
integer :: i
do i = 1, 3
   if (i == 2) then
      cycle
      print *, "skipped", i    ! expect: unreachable-code
   end if
   print *, i
end do
if (debug) then
   print *, "debugging"        ! expect: unreachable-code
end if
call finish()
contains
subroutine finish()
print *, "done"
return
print *, "after return"        ! expect: unreachable-code
end subroutine finish
end program unreachable_code

! Warning:    unit-leak
! Enabled by: -Wall
! OPEN(NEWUNIT=u) of a local unit variable in a procedure that never closes
! it. The file stays connected to a unit whose number is lost on return, and
! each call connects another.
program unit_leak
implicit none
call write_log("one")
call write_log_and_close("two")
contains
subroutine write_log(text)
character(len=*), intent(in) :: text
integer :: u
open (newunit=u, file="unit_leak_log.txt", position="append")    ! expect: unit-leak
write (u, "(a)") text
end subroutine write_log
subroutine write_log_and_close(text)
character(len=*), intent(in) :: text
integer :: u
open (newunit=u, file="unit_leak_log.txt", position="append")
write (u, "(a)") text
close (u)
end subroutine write_log_and_close
end program unit_leak

! Warning:    action-mismatch
! Enabled by: default
! READ from a unit opened with ACTION="write", or WRITE to one opened with
! ACTION="read". Either stops the program with an error unless IOSTAT= is
! given.
program action_mismatch
implicit none
integer :: n
open (10, file="action_mismatch.txt", action="write", status="replace")
write (10, *) 42
read (10, *) n               ! expect: action-mismatch
close (10)
open (10, file="action_mismatch.txt", action="read", status="old")
read (10, *) n
close (10, status="delete")
print *, n
end program action_mismatch

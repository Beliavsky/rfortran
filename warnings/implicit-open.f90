! Warning:    implicit-open
! Enabled by: default
! READ or WRITE on a unit that no OPEN connects. Most compilers then connect
! a file named like fort.10, an extension that is not portable Fortran.
program implicit_open
implicit none
integer :: u
write (10, *) "to fort.10"          ! expect: implicit-open
open (newunit=u, file="implicit_open.txt")
write (u, *) "to an opened file"
close (u, status="delete")
end program implicit_open

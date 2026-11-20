! Warning:    gnu-extension
! Enabled by: -Wall
! A GNU extension procedure, such as CALL SYSTEM or DFLOAT, that the program
! does not define itself; the message names the standard replacement.
! With -std= these are errors.
program gnu_extension
implicit none
character(len=100) :: arg
integer :: n, status
call getarg(1, arg)                           ! expect: gnu-extension
call get_command_argument(1, arg)
call system("echo from system", status)       ! expect: gnu-extension
call execute_command_line("echo from execute_command_line", exitstat=status)
n = 3
print *, dfloat(n), dble(n)                   ! expect: gnu-extension
call flush(6)                                 ! expect: gnu-extension
flush (6)
call show("done")
contains
subroutine show(text)
character(len=*), intent(in) :: text
print *, text
end subroutine show
end program gnu_extension

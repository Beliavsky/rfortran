! Warning:    semicolon
! Enabled by: -Wstyle
! More than one statement on a line, separated by semicolons. One statement
! per line is easier to read, debug and diff. The warning is given once per
! file.
program semicolon
implicit none
integer :: i, j
i = 1; j = 2          ! expect: semicolon
print *, i + j
end program semicolon

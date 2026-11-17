! Warning:    goto
! Enabled by: -Wstyle
! GO TO statements. Constructs such as DO with EXIT and CYCLE, IF and
! SELECT CASE show the control flow directly.
program goto
implicit none
integer :: n
n = 0
10 n = n + 1
if (n < 3) go to 10          ! expect: goto
print *, n
end program goto

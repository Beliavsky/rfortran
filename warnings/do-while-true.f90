! Warning:    do-while-true
! Enabled by: -Wstyle
! `do while (.true.)`, an endless loop, which plain `do` expresses directly.
program do_while_true
implicit none
integer :: n
n = 0
do while (.true.)           ! expect: do-while-true
   n = n + 1
   if (n >= 3) exit
end do
do
   n = n + 1
   if (n >= 6) exit
end do
print *, n
end program do_while_true

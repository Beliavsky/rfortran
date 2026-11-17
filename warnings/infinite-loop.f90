! Warning:    infinite-loop
! Enabled by: -Wall
! A DO loop without loop control, or DO WHILE (.true.), that nothing in its
! body can end: no EXIT, RETURN, STOP or branch out of it.
program infinite_loop
implicit none
integer :: n
n = 0
do
   n = n + 1
   if (n > 10) exit
end do
print *, n
do                     ! expect: infinite-loop
   n = n + 1
end do
end program infinite_loop

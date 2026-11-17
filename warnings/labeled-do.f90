! Warning:    labeled-do
! Enabled by: -Wstyle
! A DO loop ended by a labeled statement, as `do 10 i = 1, n` ... `10
! continue`. END DO needs no label and cannot be confused with another
! loop's end. (Ending a loop on a statement other than CONTINUE or END DO,
! or sharing one end between loops, is obsolescent.)
program labeled_do
implicit none
integer :: i, total
total = 0
do 10 i = 1, 3          ! expect: labeled-do
   total = total + i
10 continue
do i = 1, 3
   total = total + i
end do
print *, total
end program labeled_do

! Warning:    unused-but-set-variable
! Enabled by: -Wall
! A local variable that is assigned but whose value is never read. The
! computation that sets it is wasted, or a later statement was meant to use
! it.
program unused_but_set_variable
implicit none
integer :: i, total
integer :: last    ! expect: unused-but-set-variable
total = 0
do i = 1, 5
   total = total + i
   last = i
end do
print *, total
end program unused_but_set_variable

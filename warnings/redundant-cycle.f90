! Warning:    redundant-cycle
! Enabled by: -Wextra
! CYCLE as the last statement of a loop body, directly or at the end of a
! branch that ends the body, where the next iteration starts anyway.
program redundant_cycle
implicit none
integer :: i, total
total = 0
do i = 1, 6
   if (mod(i, 2) == 0) then
      total = total + i
      cycle                  ! expect: redundant-cycle
   end if
end do
do i = 1, 6
   if (mod(i, 3) == 0) cycle
   total = total + i
end do
print *, total
end program redundant_cycle

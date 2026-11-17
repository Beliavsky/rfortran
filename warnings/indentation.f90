! Warning:    indentation
! Enabled by: -Wstyle
! A construct body not indented past its opening statement, statements of
! one body in different columns, or END, ELSE and CASE statements not
! aligned with the construct.
program indentation
implicit none
integer :: i, total
total = 0
do i = 1, 3
total = total + i         ! expect: indentation
end do
do i = 1, 3
   total = total + i
end do
print *, total
end program indentation

! Warning:    unused-variable
! Enabled by: -Wall
! A local variable that is declared but never referenced: often left over
! from an earlier version of the code, or a sign that a name is misspelled
! where it should have been used.
program unused_variable
implicit none
integer :: total, i
integer :: counter    ! expect: unused-variable
real :: scale         ! expect: unused-variable
total = 0
do i = 1, 4
   total = total + i
end do
print *, total
end program unused_variable

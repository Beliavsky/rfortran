! Warning:    name-case
! Enabled by: -Wstyle
! A name written in a different case from its declaration, as Total for a
! variable declared total. Fortran ignores case, but a consistent spelling
! is easier to search for and read. The warning is given once per name.
program name_case
implicit none
integer :: total
total = 1
Total = total + 1          ! expect: name-case
print *, total
end program name_case

! Warning:    old-relational
! Enabled by: -Wstyle
! The FORTRAN 77 relational operators .lt., .le., .eq., .ne., .gt. and .ge..
! The symbols <, <=, ==, /=, > and >= are clearer.
program old_relational
implicit none
integer :: n
n = 3
if (n .gt. 2) print *, "big"      ! expect: old-relational
if (n > 2) print *, "big"
end program old_relational

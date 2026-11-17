! Warning:    missing-double-colon
! Enabled by: -Wstyle
! A type declaration without ::, as `integer i, j`. The :: is required once
! attributes or initial values are added, so writing it always is
! consistent.
program missing_double_colon
implicit none
integer i, j          ! expect: missing-double-colon
integer :: k
i = 1
j = 2
k = i + j
print *, k
end program missing_double_colon

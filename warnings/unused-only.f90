! Warning:    unused-only
! Enabled by: -Wextra
! A name in a USE statement's ONLY list that the scope never uses.
module constants
implicit none
real, parameter :: pi = 3.14159, e = 2.71828
end module constants

program unused_only
use constants, only: pi, e       ! expect: unused-only
implicit none
print *, 2*pi
end program unused_only

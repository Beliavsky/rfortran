! Warning:    duplicate-constant
! Enabled by: -Wstyle
! A REAL or COMPLEX named constant, or a kind parameter, defined in more
! than one scope. Copies can drift apart, as with different numbers of
! digits of pi; define it once in a module.
module geometry
implicit none
real, parameter :: pi = 3.14159265
end module geometry

program duplicate_constant
implicit none
real, parameter :: pi = 3.14159          ! expect: duplicate-constant
print *, 2*pi
end program duplicate_constant

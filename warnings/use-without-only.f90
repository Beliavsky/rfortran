! Warning:    use-without-only
! Enabled by: -Wstyle
! USE without ONLY imports every public name of the module, so a reader
! cannot tell where a name comes from, and names added to the module later
! can clash. The warning suggests the ONLY list the scope needs.
module constants
implicit none
real, parameter :: pi = 3.14159
end module constants

program use_without_only
use constants              ! expect: use-without-only
implicit none
print *, 2*pi
end program use_without_only

! Warning:    module-default-public
! Enabled by: -Wstyle
! A module without a PRIVATE or PUBLIC statement, whose entities are all
! public by default, including helpers meant for internal use. A PRIVATE
! statement, with the interface listed as PUBLIC, makes the interface
! explicit.
module helpers               ! expect: module-default-public
implicit none
contains
function twice(n) result(m)
integer, intent(in) :: n
integer :: m
m = 2*n
end function twice
end module helpers

module tools
implicit none
private
public :: thrice
contains
function thrice(n) result(m)
integer, intent(in) :: n
integer :: m
m = 3*n
end function thrice
end module tools

program module_default_public
use helpers, only: twice
use tools, only: thrice
implicit none
print *, twice(2), thrice(2)
end program module_default_public

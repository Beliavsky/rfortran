! Warning:    redundant-implicit
! Enabled by: -Wstyle
! IMPLICIT NONE in an internal or module procedure whose host already has
! IMPLICIT NONE, which applies there too. (An interface body does not
! inherit it and is not reported.)
program redundant_implicit
implicit none
print *, twice(2)
contains
function twice(n) result(m)
implicit none                 ! expect: redundant-implicit
integer, intent(in) :: n
integer :: m
m = 2*n
end function twice
end program redundant_implicit

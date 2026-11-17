! Warning:    unused-function
! Enabled by: -Wall
! A function or subroutine that nothing calls: an internal procedure, or a
! PRIVATE module procedure. Public module procedures may be called from
! other programs and are not reported.
module shapes
implicit none
private
public :: square
contains
function square(x) result(y)
real, intent(in) :: x
real :: y
y = x*x
end function square
function cube(x) result(y)    ! expect: unused-function
real, intent(in) :: x
real :: y
y = x*x*x
end function cube
end module shapes

program unused_function
use shapes, only: square
implicit none
print *, square(3.0)
contains
subroutine report(x)    ! expect: unused-function
real, intent(in) :: x
print *, "value:", x
end subroutine report
end program unused_function

! Warning:    unused-dummy-argument
! Enabled by: -Wall
! A dummy argument that the procedure never references. Callers still have
! to supply it, so it may be a leftover or a sign of a missing calculation.
program unused_dummy_argument
implicit none
print *, area(2.0, 3.0, 4.0)
contains
function area(width, height, depth) result(a)
real, intent(in) :: width, height
real, intent(in) :: depth    ! expect: unused-dummy-argument
real :: a
a = width*height
end function area
end program unused_dummy_argument

module shapes
implicit none
type, abstract :: shape
contains
  procedure(area_i), deferred :: area
end type
abstract interface
  real function area_i(self)
    import shape
    class(shape), intent(in) :: self
  end function
end interface
type, extends(shape) :: circle
  real :: r
contains
  procedure :: area => circ_area
end type
type, extends(shape) :: square
  real :: s
contains
  procedure :: area => sq_area
end type
contains
real function circ_area(self)
  class(circle), intent(in) :: self
  circ_area = 3.14159*self%r**2
end function
real function sq_area(self)
  class(square), intent(in) :: self
  sq_area = self%s**2
end function
end module
program p4
use shapes
implicit none
class(shape), allocatable :: s
s = circle(1.0)
print *, s%area()
deallocate(s)
s = square(2.0)
print *, s%area()
end program

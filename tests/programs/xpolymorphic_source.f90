module runtime_shapes
implicit none
type, abstract :: shape
contains
procedure(area_interface), deferred :: area
end type
type, extends(shape) :: circle
real :: radius=2.0
contains
procedure :: area => circle_area
end type
type, extends(shape) :: rectangle
real :: width=3.0, height=4.0
contains
procedure :: area => rectangle_area
end type
abstract interface
real function area_interface(self)
import :: shape
class(shape), intent(in) :: self
end function
end interface
contains
real function circle_area(self)
class(circle), intent(in) :: self
circle_area=3.0*self%radius**2
end function
real function rectangle_area(self)
class(rectangle), intent(in) :: self
rectangle_area=self%width*self%height
end function
end module
program test_polymorphic_source
use runtime_shapes
implicit none
class(shape),allocatable :: x,y
! SOURCE copies the value and dynamic type.
allocate(x,source=circle(radius=4.0))
! MOLD selects the type and applies default initialization.
allocate(y,mold=x)
print *,x%area(),y%area()
! Assignment replaces circle storage with rectangle storage.
x=rectangle(width=2.0,height=7.0)
y=x
print *,x%area(),y%area()
deallocate(x,y)
end program

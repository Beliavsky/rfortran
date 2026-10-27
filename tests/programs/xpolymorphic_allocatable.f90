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
program test_runtime_shapes
use runtime_shapes
implicit none
class(shape), allocatable :: x
integer :: i
print *,allocated(x)
do i=1,4
if (mod(i,2)==1) then
allocate(circle :: x)
else
allocate(rectangle :: x)
end if
print *,allocated(x),x%area()
select type (s=>x)
type is (circle)
s%radius=3.0
type is (rectangle)
s%height=5.0
end select
print *, x%area()
deallocate(x)
end do
print *,allocated(x)
end program

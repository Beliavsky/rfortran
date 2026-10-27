module abstract_shapes
implicit none
type,abstract :: shape
contains
   procedure(area_interface),deferred :: area
   procedure(scale_interface),deferred :: scale
   procedure :: describe
end type
type,abstract,extends(shape) :: planar_shape
end type
type,extends(planar_shape) :: rectangle
   real :: width=2.0, height=3.0
contains
   procedure :: area => rectangle_area
   procedure :: scale => rectangle_scale
end type
type,extends(planar_shape) :: square
   real :: side=4.0
contains
   procedure :: area => square_area
   procedure :: scale => square_scale
end type
abstract interface
   pure function area_interface(self) result(area)
      import :: shape
      class(shape),intent(in) :: self
      real :: area
   end function
   subroutine scale_interface(self,factor)
      import :: shape
      class(shape),intent(inout) :: self
      real,intent(in) :: factor
   end subroutine
end interface
contains
pure real function rectangle_area(self) result(area)
class(rectangle),intent(in) :: self
area=self%width*self%height
end function
pure real function square_area(self) result(area)
class(square),intent(in) :: self
area=self%side**2
end function
subroutine rectangle_scale(self,factor)
class(rectangle),intent(inout) :: self
real,intent(in) :: factor
self%width=self%width*factor
self%height=self%height*factor
end subroutine
subroutine square_scale(self,factor)
class(square),intent(inout) :: self
real,intent(in) :: factor
self%side=self%side*factor
end subroutine
subroutine describe(self)
class(shape),intent(in) :: self
print *,self%area()
end subroutine
subroutine test_shape(x)
class(shape),intent(inout) :: x
print *,x%area()
call x%scale(factor=2.0)
print *,x%area()
end subroutine
end module
program test_abstract_shapes
use abstract_shapes
implicit none
type(rectangle) :: a
type(square) :: b
call test_shape(a)
call test_shape(b)
call a%describe()
call b%describe()
end program

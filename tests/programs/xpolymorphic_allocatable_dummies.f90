module allocatable_shapes
implicit none
type,abstract::shape
contains
procedure(area_iface),deferred::area
end type
type,extends(shape)::circle
real::radius=2
contains
procedure::area=>circle_area
end type
type,extends(shape)::rectangle
real::width=3,height=4
contains
procedure::area=>rectangle_area
end type
abstract interface
real function area_iface(self)
import::shape
class(shape),intent(in)::self
end function
end interface
contains
real function circle_area(self)
class(circle),intent(in)::self
circle_area=3*self%radius**2
end function
real function rectangle_area(self)
class(rectangle),intent(in)::self
rectangle_area=self%width*self%height
end function
subroutine make_shape(x,which)
class(shape),allocatable,intent(out)::x
integer,intent(in)::which
print *,allocated(x)
if(which==1)then
allocate(circle::x)
else
x=rectangle(width=2.0,height=7.0)
end if
end subroutine
subroutine inspect(x)
class(shape),allocatable,intent(in)::x
print *,allocated(x)
if(allocated(x))print *,x%area()
end subroutine
subroutine change(x)
class(shape),allocatable,intent(inout)::x
if(allocated(x))deallocate(x)
allocate(circle::x)
select type(x)
type is(circle)
x%radius=3
end select
end subroutine
end module
program p
use allocatable_shapes
implicit none
class(shape),allocatable::x
call inspect(x)
call make_shape(x,1)
call inspect(x)
call make_shape(which=2,x=x)
call inspect(x)
call change(x)
call inspect(x)
deallocate(x)
end program

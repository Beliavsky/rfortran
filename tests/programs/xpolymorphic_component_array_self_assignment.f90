! Repro: assigning an array result back to its initialized input array.
module component_array_result
implicit none
type,abstract::base
contains
procedure(eval_interface),deferred::eval
end type
abstract interface
function eval_interface(this,x) result(y)
import base
class(base),intent(in)::this
real,intent(in)::x(:)
real::y(size(x))
end function
end interface
type,extends(base)::scale
real::factor=2.0
contains
procedure::eval=>evaluate
end type
type holder
class(base),allocatable::item
contains
procedure::apply
end type
contains
function evaluate(this,x)
class(scale),intent(in)::this
real,intent(in)::x(:)
real::evaluate(size(x))
evaluate(1)=this%factor*x(1)
evaluate(2)=this%factor*x(2)
end function
subroutine apply(this,x,y)
class(holder),intent(in)::this
real,intent(in)::x(:)
real,intent(out)::y(:)
y=x
y=this%item%eval(y)
end subroutine
end module
program test_array_binding
use component_array_result
implicit none
type(holder)::box
real::y(2)
allocate(scale::box%item)
call box%apply([1.0,2.0],y)
print *,y
end

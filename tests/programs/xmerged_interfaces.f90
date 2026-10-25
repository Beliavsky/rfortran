! Generic interfaces extend imported interfaces and survive re-export.
module base_interfaces
implicit none
private
public :: box, choose, set_value, operator(-), operator(.join.), assignment(=)
type box
integer :: value
end type
interface choose
module procedure choose_integer
end interface
interface set_value
module procedure set_integer
end interface
interface operator(-)
module procedure subtract_boxes
end interface
interface operator(.join.)
module procedure join_integer
end interface
interface assignment(=)
module procedure assign_integer
end interface
contains
integer function choose_integer(x)
integer,intent(in)::x
choose_integer=x+10
end function
subroutine set_integer(x,y)
integer,intent(in)::x
integer,intent(out)::y
y=x+30
end subroutine
integer function subtract_boxes(x,y)
type(box),intent(in)::x,y
subtract_boxes=x%value-y%value
end function
integer function join_integer(x,y)
integer,intent(in)::x,y
join_integer=x+y
end function
subroutine assign_integer(x,y)
type(box),intent(out)::x
integer,intent(in)::y
x%value=y
end subroutine
end module
module extended_interfaces
use base_interfaces
implicit none
private
public :: box, choose, set_value, operator(-), operator(.join.), assignment(=)
interface choose
module procedure choose_real
end interface
interface set_value
module procedure set_real
end interface
interface operator(-)
module procedure subtract_integer
end interface
interface operator(.join.)
module procedure join_real
end interface
interface assignment(=)
module procedure assign_real
end interface
contains
integer function choose_real(x)
real,intent(in)::x
choose_real=int(x)+20
end function
subroutine set_real(x,y)
real,intent(in)::x
integer,intent(out)::y
y=int(x)+40
end subroutine
integer function subtract_integer(x,y)
type(box),intent(in)::x
integer,intent(in)::y
subtract_integer=x%value-y
end function
real function join_real(x,y)
real,intent(in)::x,y
join_real=x+y
end function
subroutine assign_real(x,y)
type(box),intent(out)::x
real,intent(in)::y
x%value=int(y)
end subroutine
end module
module reexport_interfaces
use base_interfaces
use extended_interfaces
implicit none
end module
program main
use reexport_interfaces, only: box, choose, set_value, operator(-), operator(.join.), assignment(=)
implicit none
type(box)::x,y
integer::i,j
x=7
y=5.0
call set_value(1,i)
call set_value(2.0,j)
print *,choose(2),choose(3.0),i,j
print *,x-y,x-1,2 .join. 3,2.0 .join. 4.0
end

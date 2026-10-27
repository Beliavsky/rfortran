module boxes
implicit none
type :: box
private
integer :: value = 3
contains
private
procedure,public :: get
procedure :: set
end type
contains
integer function get(this)
class(box),intent(in)::this
get=this%value
end function
subroutine set(this,n)
class(box),intent(inout)::this
integer,intent(in)::n
this%value=n
end subroutine
function make(n) result(x)
integer,intent(in)::n
type(box)::x
call x%set(n)
end function
end module
program p
use boxes
implicit none
type(box)::a,b
a=make(7)
b=a
print *,a%get(),b%get()
b=box()
print *,b%get()
end program

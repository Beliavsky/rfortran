! Expected output: 7. The constructor result uses the function name.
module private_constructor_mod
implicit none
private
public :: box, make_box
type box
private
integer :: value
contains
procedure, public :: get
end type
contains
function make_box(n)
integer,intent(in) :: n
type(box) :: make_box
make_box%value=n
end function
integer function get(this)
class(box),intent(in) :: this
get=this%value
end function
end module
program test_constructor
use private_constructor_mod
implicit none
type(box) :: x
x=make_box(7)
print *,x%get()
end program

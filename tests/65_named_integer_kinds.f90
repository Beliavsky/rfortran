module integer_kind_values
implicit none
private
public :: i8, i16, i32, i64, box, twice, width, set_default
integer, parameter :: i8=selected_int_kind(2), i16=selected_int_kind(4)
integer(kind=4), parameter :: i32=selected_int_kind(9), i64=selected_int_kind(15)
type :: box
integer(kind=i64) :: value=17
end type box
interface width
module procedure width_default, width_long
end interface width
contains
integer(kind=i64) function twice(x)
integer(i64) :: x
twice=x*2
end function twice
integer function width_default(x)
integer(kind=i32) :: x
width_default=4
if (x<0) width_default=-4
end function width_default
integer function width_long(x)
integer(kind=i64) :: x
width_long=8
if (x<0) width_long=-8
end function width_long
subroutine set_default(x)
integer(kind=i32), intent(out) :: x(:)
x=11
end subroutine set_default
end module integer_kind_values

program test_named_integer_kinds
use integer_kind_values, only: i8, i16, i32, i64, box, twice, width, set_default
implicit none
integer(kind=i8) :: small
integer(i16) :: medium
integer(kind=i32) :: normal
integer(i64) :: wide, values(2)
integer(kind=16) :: huge_integer
integer :: default_array(2)
type(box) :: object
small=100
medium=30000
normal=1000000000
wide=normal
wide=wide*9
values(1)=wide
values(2)=twice(wide)
huge_integer=123
print *, small,medium,normal
print *, wide,values,huge_integer
print *, width(normal),width(wide),width(object%value),object%value
call set_default(default_array)
print *, default_array
block
integer, parameter :: local_kind=selected_int_kind(15)
integer(kind=local_kind) :: local_value
local_value=wide+1
print *, local_value
end block
end program test_named_integer_kinds

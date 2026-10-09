module default_names
implicit none
private
public :: default, box, local_value
type :: box
integer :: default = 9
end type box
contains
integer function default(value)
integer :: value
default = value + 10
end function default

integer function local_value()
integer :: default
default = 5
select case(default)
case(5)
default = default + 1
case default
default = -1
end select
local_value = default
end function local_value
end module default_names

program test_default
use default_names, only: default, box, local_value
implicit none
type(box) :: b
print *, default(3), local_value(), b%default
b%default = 12
select case(b%default)
case(9)
print *, -1
case &
 & DEFAULT; print *, b%default
end select
end program test_default

! Regression for the former xacv.f90 blocker at line 2560: yyyymmdd(xx%date).
module nested_component_array_mod
implicit none
type :: date_mdy
integer :: year
end type
type :: date_time
type(date_mdy) :: date
end type
contains
elemental integer function year_number(date)
type(date_mdy),intent(in)::date
year_number=date%year
end function
subroutine show(xx)
type(date_time),intent(in)::xx(:)
print *,year_number(xx%date)
end subroutine
end module
program test
use nested_component_array_mod
implicit none
type(date_time)::xx(2)
xx(1)%date%year=2024
xx(2)%date%year=2025
call show(xx)
! Expected: 2024 2025
end program

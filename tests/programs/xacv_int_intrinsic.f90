! Reduced from xacv.f90: INT generic and numeric calls in caldat.
! Extending INT for dates must preserve intrinsic REAL-to-INTEGER conversion.
module acv_int_repro
implicit none
type :: month_year
   integer :: year
end type
interface int
   module procedure int_month_year
end interface
contains
integer function int_month_year(date) result(year)
   type(month_year), intent(in) :: date
   year=date%year
end function
integer function numeric_year(x) result(year)
   real, intent(in) :: x
   year=int(x)
end function
end module
program test
use acv_int_repro
implicit none
type(month_year) :: date
date%year=2024
print '(2(i0,1x))', numeric_year(3.75),int(date)
! Expected: 3 2024
end program

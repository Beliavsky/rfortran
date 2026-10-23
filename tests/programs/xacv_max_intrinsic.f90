! Reduced from xacv.f90: pos_first_month_year, originally line 835.
! Extending MAX for a derived type must preserve intrinsic integer MAX.
module acv_max_repro
implicit none
type :: month_year
   integer :: year
end type
interface max
   module procedure max_month_year
end interface
contains
function max_month_year(a,b) result(c)
   type(month_year), intent(in) :: a,b
   type(month_year) :: c
   c=a
   if (b%year > a%year) c=b
end function
integer function year_stride(mod_year) result(stride)
   integer, intent(in), optional :: mod_year
   stride=1
   if (present(mod_year)) stride=max(1,mod_year)
end function
end module
program test
use acv_max_repro
implicit none
type(month_year) :: a,b,c
a%year=2020
b%year=2024
c=max(a,b)
print '(3(i0,1x))', year_stride(0),year_stride(5),c%year
! Expected: 1 5 2024
end program

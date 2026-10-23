! Reduced from xacv.f90: MINVAL generic in date_mdy_mod.
! The date overload must coexist with intrinsic numeric MINVAL.
module acv_minval_repro
implicit none
type :: date_type
   integer :: year
end type
interface minval
   module procedure minval_date
end interface
contains
function minval_date(dates) result(date)
   type(date_type), intent(in) :: dates(:)
   type(date_type) :: date
   integer :: i
   date%year=dates(1)%year
   do i=2,size(dates)
      if (dates(i)%year < date%year) date%year=dates(i)%year
   end do
end function
integer function numeric_bound(years) result(bound)
   integer, intent(in) :: years(:)
   bound=minval(years)
end function
end module
program test
use acv_minval_repro
implicit none
type(date_type) :: dates(2),date
dates(1)%year=2020
dates(2)%year=2024
date=minval(dates)
print '(2(i0,1x))', numeric_bound([3,1,5]),date%year
! Expected: 1 2020
end program

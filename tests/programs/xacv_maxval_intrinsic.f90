! Reduced from xacv.f90: MAXVAL generic in date_mdy_mod.
! The date overload must coexist with intrinsic numeric MAXVAL.
module acv_maxval_repro
implicit none
type :: date_type
   integer :: year
end type
interface maxval
   module procedure maxval_date
end interface
contains
function maxval_date(dates) result(date)
   type(date_type), intent(in) :: dates(:)
   type(date_type) :: date
   integer :: i
   date%year=dates(1)%year
   do i=2,size(dates)
      if (dates(i)%year > date%year) date%year=dates(i)%year
   end do
end function
integer function numeric_bound(years) result(bound)
   integer, intent(in) :: years(:)
   bound=maxval(years)
end function
end module
program test
use acv_maxval_repro
implicit none
type(date_type) :: dates(2),date
dates(1)%year=2020
dates(2)%year=2024
date=maxval(dates)
print '(2(i0,1x))', numeric_bound([3,1,5]),date%year
! Expected: 5 2024
end program

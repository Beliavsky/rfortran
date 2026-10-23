! xacv.f90 line 1854: PACK of records passed to an allocatable-output routine.
module pack_dates_mod
implicit none
type :: date_mdy
integer :: month,day,year
end type
contains
elemental logical function is_weekday(date)
type(date_mdy),intent(in)::date
! A small synthetic calendar is sufficient to exercise the array mask.
is_weekday=date%day<=5
end function
pure subroutine set_alloc(xx,yy)
type(date_mdy),intent(in)::xx(:)
type(date_mdy),allocatable,intent(out)::yy(:)
allocate(yy(size(xx)))
yy=xx
end subroutine
end module
program test
use pack_dates_mod
implicit none
type(date_mdy),allocatable::dates(:),week_dates(:)
allocate(dates(3))
dates=[date_mdy(1,1,2024),date_mdy(1,6,2024),date_mdy(1,3,2024)]
call set_alloc(pack(dates,is_weekday(dates)),week_dates)
print *,week_dates%day
! Expected: 1 3
end program

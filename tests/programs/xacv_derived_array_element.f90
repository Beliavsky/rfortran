! Reduced from xacv.f90: minval_date_mdy/maxval_date_mdy, yy=xx(1).
! The whole-element assignment fails even for a fixed-size local array.
program test
implicit none
type :: date_type
   integer :: year
end type
type(date_type) :: dates(2),date
dates(1)%year=2020
dates(2)%year=2024
date=dates(1)
print '(i0)',date%year
! Expected: 2020
! Workaround for this one-component type: date%year=dates(1)%year
end program

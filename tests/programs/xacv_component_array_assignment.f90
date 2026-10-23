! Regression for the former xacv.f90 blocker at line 4271: assign to a derived component across a record array.
program test
implicit none
type :: date_mdy
integer :: year
end type
type :: date_time
type(date_mdy) :: date
integer :: hour=0
end type
type(date_mdy)::dates(2)
type(date_time)::dt(2)
dates(1)=date_mdy(2024)
dates(2)=date_mdy(2025)
dt%date=dates
print *,dt%date%year
! Expected: 2024 2025
end program

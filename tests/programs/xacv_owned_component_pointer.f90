! Regression for the former xacv.f90 blocker at line 4162: pointer association to an owned record allocation.
program test
implicit none
type :: date_time
integer :: year
end type
type :: date_time_vec
type(date_time),allocatable :: xx(:)
end type
type(date_time_vec),target :: v(1)
type(date_time),pointer :: z(:)
allocate(v(1)%xx(2))
z => v(1)%xx
z(1)%year=2024
z(2)%year=2025
print *,size(z),z%year
! Expected: 2 2024 2025
end program

! xacv.f90 line 4154: POINTER array of a derived type.
program test
implicit none
type :: date_time
integer :: year
end type
type(date_time),target :: dates(2)
type(date_time),pointer :: z(:)
dates(1)%year=2024
dates(2)%year=2025
z => dates
print *,size(z),z%year
z(2)%year=2030
print *,dates%year
! Expected: 2 2024 2025, then 2024 2030
end program

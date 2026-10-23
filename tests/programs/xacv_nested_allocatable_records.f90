! xacv.f90 blocker: date_time_vec_vec contains an array of date_time_vec.
! Each date_time_vec owns an independently allocated xx array.
program test_nested_allocatable_records
implicit none
type :: date_time
   integer :: year
end type
type :: date_time_vec
   type(date_time), allocatable :: xx(:)
end type
type :: date_time_vec_vec
   type(date_time_vec), allocatable :: dd(:)
end type
type(date_time_vec_vec) :: groups
allocate(groups%dd(2))
print *,size(groups%dd)
! Expected: 2
end program

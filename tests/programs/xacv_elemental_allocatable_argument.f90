! Regression for the former xacv.f90 blocker at line 722: num_obs(dvec(ivec)).
! The derived actual is scalar, although its component is an allocated array.
module elemental_owned_record_mod
implicit none
type :: date_time_vec
   integer,allocatable :: xx(:)
end type
contains
elemental integer function num_obs(vec)
type(date_time_vec),intent(in)::vec
num_obs=size(vec%xx)
end function
end module
program test
use elemental_owned_record_mod
implicit none
type(date_time_vec)::dvec(2)
allocate(dvec(1)%xx(3))
dvec(1)%xx=0
print *,num_obs(dvec(1))
! Expected: 3
end program

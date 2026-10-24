! Regression for xacv.f90 line 6550: a main descriptor must not leak into a dummy scope.
! Expected: 7, independently of the unrelated main-program array's name or rank.
module dummy_descriptor_shadow_mod
implicit none
contains
integer function first_cell(xx)
integer,intent(in)::xx(:,:)
first_cell=xx(1,1)
end function
integer function first_plane(xx)
integer,intent(in)::xx(:,:,:)
first_plane=first_cell(xx(:,:,1))
end function
end module
program test
use dummy_descriptor_shadow_mod
implicit none
integer,allocatable::xx(:,:),cube(:,:,:)
allocate(xx(1,1),cube(2,2,1))
cube=7
print *,first_plane(cube)
end program

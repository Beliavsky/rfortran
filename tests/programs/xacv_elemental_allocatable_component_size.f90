! Next lowering blocker exposed by xacv.f90 line 722: NUM_OBS(dvec(ivec)).
module date_vectors_mod
implicit none
type :: date_mdy
integer::month,day,year
end type
type :: date_vec
type(date_mdy),allocatable::xx(:)
end type
contains
elemental integer function num_obs(vec) result(n)
type(date_vec),intent(in)::vec
if(allocated(vec%xx)) then
   n=size(vec%xx)
else
   n=-1
end if
end function
subroutine show(dvec)
type(date_vec),intent(in)::dvec(:)
integer::i
do i=1,size(dvec)
   print *,num_obs(dvec(i))
end do
end subroutine
end module
program test
use date_vectors_mod
implicit none
type(date_vec)::v(1)
allocate(v(1)%xx(2))
v(1)%xx(1)=date_mdy(1,2,2024)
v(1)%xx(2)=date_mdy(3,4,2025)
call show(v)
! Expected: 2
end program

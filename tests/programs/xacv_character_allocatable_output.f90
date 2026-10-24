! Regression for xacv.f90:21084: set_alloc(["min","max"],cstats_indep_).
! Expected output:
! 2 20
! min|max
module character_allocatable_output_mod
implicit none
contains
subroutine labels()
character(len=20),allocatable::cstats_indep_(:)
call set_alloc(["min","max"],cstats_indep_)
print *,size(cstats_indep_),len(cstats_indep_)
print '(a)',trim(cstats_indep_(1))//'|'//trim(cstats_indep_(2))
end subroutine

subroutine set_alloc(xx,yy)
character(len=*),intent(in)::xx(:)
character(len=*),intent(out),allocatable::yy(:)
allocate(yy(size(xx)))
yy=xx
end subroutine
end module

program test_character_allocatable_output
use character_allocatable_output_mod
implicit none
call labels()
end program

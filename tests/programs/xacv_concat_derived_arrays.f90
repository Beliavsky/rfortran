! xacv.f90 line 2603: concatenate derived arrays as an INTENT(IN) actual.
module concat_dates_mod
implicit none
type :: date_mdy
integer :: month,day,year
end type
contains
function join_dates(xx,yy) result(zz)
type(date_mdy),intent(in)::xx(:),yy(:)
type(date_mdy),allocatable::zz(:)
call copy_dates([xx,yy],zz)
end function
subroutine copy_dates(xx,yy)
type(date_mdy),intent(in)::xx(:)
type(date_mdy),allocatable,intent(out)::yy(:)
allocate(yy(size(xx)))
yy=xx
end subroutine
end module
program test
use concat_dates_mod
implicit none
type(date_mdy)::a(1),b(1)
type(date_mdy),allocatable::c(:)
a=[date_mdy(1,2,2024)]
b=[date_mdy(3,4,2025)]
c=join_dates(a,b)
print *,size(c),c%year
! Expected: 2 2024 2025
end program

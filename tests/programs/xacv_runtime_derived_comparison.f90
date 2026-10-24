! Regression for ALL(dates(2:) < dates(:nread-1)) in xacv.f90:16965.
! Expected output: T
module runtime_date_comparison_mod
implicit none
type :: date_mdy
   integer :: day
end type
interface operator (<)
   module procedure date_mdy_lt
end interface
contains
elemental logical function date_mdy_lt(xx,yy) result(x_lt_y)
type(date_mdy), intent(in) :: xx,yy
x_lt_y = xx%day < yy%day
end function
end module

program test_runtime_date_comparison
use runtime_date_comparison_mod
implicit none
type(date_mdy), allocatable :: dates(:)
integer :: nread,i
nread=3
allocate(dates(nread))
do i=1,nread
   dates(i)%day=nread-i+1
end do
print *,all(dates(2:) < dates(:nread-1))
end program

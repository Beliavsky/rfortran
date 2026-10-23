! xacv.f90 line 4281: nested STR(MINVAL(times)) after derived-array flattening.
module nested_derived_reduction_mod
implicit none
type :: time_of_day
integer :: hour,minute
end type
interface minval
module procedure minval_time
end interface
interface str
module procedure str_time_of_day
end interface
contains
pure function minval_time(xx) result(yy)
type(time_of_day),intent(in)::xx(:)
type(time_of_day)::yy
integer::i
yy=xx(1)
do i=2,size(xx)
if(xx(i)%hour < yy%hour) yy=xx(i)
end do
end function
elemental function str_time_of_day(xx,print_seconds) result(text)
type(time_of_day),intent(in)::xx
logical,intent(in),optional::print_seconds
character(len=5)::text
write(text,'(i2.2,a,i2.2)') xx%hour,':',xx%minute
end function
end module
program test
use nested_derived_reduction_mod
implicit none
type(time_of_day)::times(2)
times=[time_of_day(9,0),time_of_day(3,30)]
write(*,'(a)') trim(str(minval(times)))
! Expected: 03:30
end program

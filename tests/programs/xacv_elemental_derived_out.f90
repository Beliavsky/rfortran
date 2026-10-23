! Next xacv.f90 blocker: set_time_of_day, around line 940.
! A scalar call already requires support for a derived ELEMENTAL OUT dummy.
module elemental_time_mod
implicit none
type :: time_of_day
   integer :: hour,minute,second
end type
contains
elemental subroutine set_time(yy,hour,minute,second)
   type(time_of_day), intent(out) :: yy
   integer, intent(in) :: hour
   integer, intent(in), optional :: minute,second
   yy%hour=hour
   if(present(minute)) yy%minute=minute
   if(present(second)) yy%second=second
end subroutine
end module
program test
use elemental_time_mod
implicit none
type(time_of_day)::tod
call set_time(tod,9,30,0)
print '(3(i0,1x))',tod%hour,tod%minute,tod%second
! Expected: 9 30 0
end program

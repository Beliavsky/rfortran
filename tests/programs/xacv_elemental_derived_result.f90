! Next xacv.f90 blocker: tod_int, around line 958.
module elemental_time_result_mod
implicit none
type :: time_of_day
   integer :: hour,minute,second
end type
contains
elemental function tod_int(ii) result(tod)
integer, intent(in) :: ii
type(time_of_day) :: tod
tod%second=modulo(ii,100)
tod%minute=modulo(ii/100,100)
tod%hour=ii/10000
end function
end module
program test
use elemental_time_result_mod
implicit none
type(time_of_day)::tod
tod=tod_int(93045)
print '(3(i0,1x))',tod%hour,tod%minute,tod%second
! Expected: 9 30 45
end program

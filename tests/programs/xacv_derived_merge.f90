! Regression for xacv.f90 line 1046: derived MERGE in an ELEMENTAL function.
module time_merge_mod
implicit none
type :: time_of_day
integer :: hour,minute,second
end type
interface operator(>)
module procedure time_gt
end interface
contains
elemental logical function time_gt(t1,t2)
type(time_of_day),intent(in)::t1,t2
time_gt=3600*t1%hour+60*t1%minute+t1%second > &
        3600*t2%hour+60*t2%minute+t2%second
end function
elemental function min_time(t1,t2) result(tmin)
type(time_of_day),intent(in)::t1,t2
type(time_of_day)::tmin
tmin=merge(t1,t2,t2>t1)
end function
end module
program test
use time_merge_mod
implicit none
type(time_of_day)::a,b,c
a=time_of_day(9,0,0)
b=time_of_day(3,30,0)
c=min_time(a,b)
print *,c%hour,c%minute,c%second
! Expected: 3 30 0
end program

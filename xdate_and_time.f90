program calendar_test
use, intrinsic :: iso_fortran_env, only: int64
implicit none
character(8) :: date
character(10) :: time
character(5) :: zone
integer(int64) :: values(8)
call date_and_time(values=values,zone=zone,time=time,date=date)
print *,date,time,zone
print *,values
end program calendar_test

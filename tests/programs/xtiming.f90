program timing
use, intrinsic :: iso_fortran_env, only: real64, int64
implicit none
real :: cpu
real(real64) :: cpu8
integer :: count,rate,maximum
integer(int64) :: count8,rate8,maximum8
call cpu_time(time=cpu)
call cpu_time(cpu8)
call system_clock(count_rate=rate,count_max=maximum,count=count)
call system_clock(count8,rate8,maximum8)
print *,cpu>=0.0,cpu8>=0.0_real64
print *,count>=0,count<=maximum,rate>0
print *,count8>=0_int64,count8<=maximum8,rate8>0_int64
end program timing

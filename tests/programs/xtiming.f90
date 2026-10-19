program timing
use, intrinsic :: iso_fortran_env, only: real64, int64
implicit none
real :: cpu
real(real64) :: cpu8
integer :: icount,rate,maximum
integer(int64) :: icount8,rate8,maximum8
call cpu_time(cpu)
call cpu_time(cpu8)
call system_clock(count_rate=rate,count_max=maximum,count=icount)
call system_clock(icount8,rate8,maximum8)
print*,"count_rate:", rate
print *,cpu>=0.0,cpu8>=0.0_real64
print *,icount>=0,icount<=maximum,rate>0
print *,icount8>=0_int64,icount8<=maximum8,rate8>0_int64
end program timing

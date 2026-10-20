! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
real(dp) :: x(n),base,low,high
read(*,*) seed
do i=1,n
   x(i)=real(mod(i+seed,97),dp)/97.0_dp
end do
base=sum(x)
low=minval(x)
high=maxval(x)
value=0.0_dp
call start_timer(tick,rate)
do k=1,repetitions
   x(1)=x(1)+0.0001_dp
   value=value+sum(x)+maxval(x)-minval(x)
end do
call stop_timer(tick,rate,seconds)
expected=real(repetitions,dp)*(base+high-low)+0.0001_dp*real(repetitions,dp)*real(repetitions+1,dp)/2.0_dp
error=abs(value-expected)/max(1.0_dp,abs(expected))
call report(seconds,value,error)
end program

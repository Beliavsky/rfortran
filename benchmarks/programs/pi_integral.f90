! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
integer :: steps
real(dp) :: x,h
read(*,*) seed
steps=n+seed
h=1.0_dp/real(steps,dp)
value=0.0_dp
call start_timer(tick,rate)
do i=1,steps
   x=(real(i,dp)-0.5_dp)*h
   value=value+4.0_dp/(1.0_dp+x*x)
end do
value=value*h
call stop_timer(tick,rate,seconds)
expected=acos(-1.0_dp)
error=abs(value-expected)/expected
call report(seconds,value,error)
end program

! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error

read(*,*) seed
value=real(seed,dp)
call start_timer(tick,rate)
do i=1,n
   value=0.999999_dp*value+0.000001_dp
end do
call stop_timer(tick,rate,seconds)
expected=1.0_dp+real(seed-1,dp)*0.999999_dp**n
error=abs(value-expected)/max(1.0_dp,abs(expected))
call report(seconds,value,error)
end program

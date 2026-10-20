! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
real(dp) :: x(n),y(n),factor
read(*,*) seed
do i=1,n
   x(i)=real(mod(i+seed,97),dp)/97.0_dp
   y(i)=0.5_dp*x(i)
end do
call start_timer(tick,rate)
do k=1,repetitions
   y=y+0.25_dp*x
end do
call stop_timer(tick,rate,seconds)
value=sum(y)
factor=0.5_dp+0.25_dp*real(repetitions,dp)
error=0.0_dp
do i=1,n
   error=max(error,abs(y(i)-factor*x(i)))
end do
call report(seconds,value,error)
end program

! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
real(dp) :: x(n),y(n)
read(*,*) seed
do i=1,n
   x(i)=real(mod(i+seed,97),dp)/97.0_dp
   y(i)=real(mod(i+2*seed,89),dp)/89.0_dp
end do
value=0.0_dp
call start_timer(tick,rate)
do k=1,repetitions
   y(1)=y(1)+0.0001_dp
   value=value+dot_product(x,y)
end do
call stop_timer(tick,rate,seconds)
expected=0.0_dp
do i=1,n
   expected=expected+x(i)*real(mod(i+2*seed,89),dp)/89.0_dp
end do
expected=real(repetitions,dp)*expected+x(1)*0.0001_dp*real(repetitions,dp)*real(repetitions+1,dp)/2.0_dp
error=abs(value-expected)/max(1.0_dp,abs(expected))
call report(seconds,value,error)
end program

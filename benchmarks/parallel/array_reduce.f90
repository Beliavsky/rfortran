! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,k,steps
integer(clock_kind) :: tick,rate
real(dp) :: seconds,total(8),expected,error,scale
read(*,*) seed
steps=n+seed
total=real(seed,dp)
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   scale=real(k,dp)
   do concurrent(i=1:steps) reduce(+:total)
      total=total+scale*real(modulo(i+seed,97),dp)/97.0_dp
   end do
   if(k==0) then
      total=real(seed,dp)
   end if
end do
call stop_timer(tick,rate,seconds)
expected=0.0_dp
do i=1,steps
   expected=expected+real(modulo(i+seed,97),dp)/97.0_dp
end do
scale=real(repetitions*(repetitions+1),dp)/2.0_dp
expected=real(seed,dp)+scale*expected
error=maxval(abs(total-expected))/max(1.0_dp,abs(expected))
call report(seconds,sum(total),error)
end program

! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,k,steps
integer(clock_kind) :: tick,rate
real(dp) :: seconds,total,expected,error
read(*,*) seed
steps=n+seed
total=real(seed,dp)
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   do concurrent(i=1:steps) reduce(+:total)
      total=total+real(modulo(i+seed,97),dp)/97.0_dp
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
expected=real(seed,dp)+real(repetitions,dp)*expected
error=abs(total-expected)/max(1.0_dp,abs(expected))
call report(seconds,total,error)
end program

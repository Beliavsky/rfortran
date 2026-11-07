! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,k,steps
integer(clock_kind) :: tick,rate
real(dp),allocatable :: y(:)
real(dp) :: seconds,value,error,expected,scale
read(*,*) seed
steps=n+seed
allocate(y(steps))
y=0.0_dp
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   scale=real(k,dp)
   do concurrent(i=1:steps)
      y(i)=y(i)+scale*(sqrt(real(modulo(i+seed,97)+1,dp))+sin(real(modulo(i+seed,97),dp)*0.01_dp))
   end do
   if(k==0) then
      y=0.0_dp
   end if
end do
call stop_timer(tick,rate,seconds)
scale=real(repetitions*(repetitions+1),dp)/2.0_dp
error=0.0_dp
do i=1,steps
   expected=scale*(sqrt(real(modulo(i+seed,97)+1,dp))+sin(real(modulo(i+seed,97),dp)*0.01_dp))
   error=max(error,abs(y(i)-expected)/max(1.0_dp,abs(expected)))
end do
value=sum(y)
call report(seconds,value,error)
end program

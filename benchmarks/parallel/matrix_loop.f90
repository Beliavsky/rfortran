! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,j,k,steps
integer(clock_kind) :: tick,rate
real(dp),allocatable :: y(:,:)
real(dp) :: seconds,error,expected,scale
read(*,*) seed
steps=n+seed
allocate(y(4,steps))
y=0.0_dp
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   scale=real(k,dp)
   do concurrent(i=1:steps,j=1:4)
      y(j,i)=y(j,i)+scale*real(modulo(i+seed+j,97),dp)/97.0_dp
   end do
   if(k==0) then
      y=0.0_dp
   end if
end do
call stop_timer(tick,rate,seconds)
scale=real(repetitions*(repetitions+1),dp)/2.0_dp
error=0.0_dp
do i=1,steps
   do j=1,4
      expected=scale*real(modulo(i+seed+j,97),dp)/97.0_dp
      error=max(error,abs(y(j,i)-expected)/max(1.0_dp,abs(expected)))
   end do
end do
call report(seconds,sum(y),error)
end program

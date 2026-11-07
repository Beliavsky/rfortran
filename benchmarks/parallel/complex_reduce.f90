! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,k,steps
integer(clock_kind) :: tick,rate
complex(dp) :: total(4),product,expected,expected_product
real(dp) :: seconds,value,error,weight,scale
read(*,*) seed
steps=n+seed
total=cmplx(real(seed,dp),2.0_dp,kind=dp)
product=cmplx(2.0_dp,3.0_dp,kind=dp)
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   scale=real(k,dp)
   do concurrent(i=1:steps) reduce(+:total) reduce(*:product)
      total=total+cmplx(scale*real(modulo(i+seed,97),dp)/97.0_dp,-0.5_dp*scale,kind=dp)
      product=product*cmplx(0.0_dp,1.0_dp,kind=dp)
   end do
   if(k==0) then
      total=cmplx(real(seed,dp),2.0_dp,kind=dp)
      product=cmplx(2.0_dp,3.0_dp,kind=dp)
   end if
end do
call stop_timer(tick,rate,seconds)
weight=0.0_dp
do i=1,steps
   weight=weight+real(modulo(i+seed,97),dp)/97.0_dp
end do
scale=real(repetitions*(repetitions+1),dp)/2.0_dp
expected=cmplx(real(seed,dp)+scale*weight,2.0_dp-0.5_dp*scale*real(steps,dp),kind=dp)
expected_product=cmplx(2.0_dp,3.0_dp,kind=dp)
do i=1,modulo(steps*repetitions,4)
   expected_product=expected_product*cmplx(0.0_dp,1.0_dp,kind=dp)
end do
error=maxval(abs(total-expected))/max(1.0_dp,abs(expected))
error=max(error,abs(product-expected_product))
! Sum magnitudes of components to avoid cancellation in the checksum.
value=sum(abs(real(total,dp))+abs(aimag(total)))+real(product,dp)+2.0_dp*aimag(product)
call report(seconds,value,error)
end program

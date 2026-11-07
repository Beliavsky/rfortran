! SPDX-License-Identifier: MIT
program benchmark
use bench_config, only: n,repetitions,warm_pool
use bench_support, only: clock_kind,start_timer,stop_timer,report
implicit none
integer,parameter :: dp=kind(0d0)
integer :: seed,i,j,k,steps
integer(clock_kind) :: tick,rate
real(dp),allocatable :: x(:,:),y(:)
real(dp) :: seconds,error,expected,scale
read(*,*) seed
steps=n+seed
allocate(x(4,steps),y(steps))
do i=1,steps
   do j=1,4
      x(j,i)=real(modulo(i+seed,97)+j,dp)
   end do
end do
y=0.0_dp
! Iteration zero warms the pool and is discarded before timing.
do k=0,repetitions
   if(k==0.and..not.warm_pool) cycle
   if(k==1) call start_timer(tick,rate)
   do concurrent(i=1:steps)
      y(i)=y(i)+energy(x(:,i),k)
   end do
   if(k==0) then
      y=0.0_dp
   end if
end do
call stop_timer(tick,rate,seconds)
scale=real(repetitions*(repetitions+1)*(2*repetitions+1),dp)/6.0_dp
error=0.0_dp
do i=1,steps
   expected=0.0_dp
   do j=1,4
      expected=expected+real(modulo(i+seed,97)+j,dp)**2
   end do
   expected=scale*expected
   error=max(error,abs(y(i)-expected)/max(1.0_dp,abs(expected)))
end do
call report(seconds,sum(y),error)
contains
pure function energy(a,k) result(r)
real(dp),intent(in) :: a(:)
integer,intent(in) :: k
real(dp) :: r,work(size(a))
work=a*real(k,dp)
r=sum(work*work)
end function
end program

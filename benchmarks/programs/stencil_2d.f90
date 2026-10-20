! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
integer :: j
real(dp) :: u(n,n),v(n,n),pi,lambda,target
read(*,*) seed
pi=acos(-1.0_dp)
u=0.0_dp
v=0.0_dp
do j=2,n-1
   do i=2,n-1
      u(i,j)=real(seed,dp)*sin(pi*real(i-1,dp)/real(n-1,dp))*sin(pi*real(j-1,dp)/real(n-1,dp))
   end do
end do
call start_timer(tick,rate)
do k=1,repetitions
   do j=2,n-1
      do i=2,n-1
         v(i,j)=0.25_dp*(u(i-1,j)+u(i+1,j)+u(i,j-1)+u(i,j+1))
      end do
   end do
   u=v
end do
call stop_timer(tick,rate,seconds)
value=sum(u)
lambda=cos(pi/real(n-1,dp))**repetitions
error=0.0_dp
do j=2,n-1
   do i=2,n-1
      target=real(seed,dp)*lambda*sin(pi*real(i-1,dp)/real(n-1,dp))*sin(pi*real(j-1,dp)/real(n-1,dp))
      error=max(error,abs(u(i,j)-target))
   end do
end do
error=error/max(1.0_dp,abs(real(seed,dp)))
call report(seconds,value,error)
end program

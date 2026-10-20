! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
integer :: j,l
real(dp) :: a(n,n),b(n,n),c(n,n),denom,target
read(*,*) seed
denom=real(n+seed,dp)
do j=1,n
   do i=1,n
      a(i,j)=real(i+seed,dp)/denom
      b(i,j)=real(j,dp)/denom
   end do
end do
call start_timer(tick,rate)
do k=1,repetitions
   a(1,1)=a(1,1)+0.000001_dp
   c=matmul(a,b)
end do
call stop_timer(tick,rate,seconds)
value=sum(c)
error=0.0_dp
do j=1,n
   do i=1,n
      target=real(n,dp)*real(i+seed,dp)*real(j,dp)/(denom*denom)
      if (i==1) target=target+real(repetitions,dp)*0.000001_dp*real(j,dp)/denom
      error=max(error,abs(c(i,j)-target))
   end do
end do
error=error/max(1.0_dp,maxval(abs(c)))
call report(seconds,value,error)
end program

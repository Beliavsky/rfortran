! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
real(dp), allocatable :: x(:)
integer(clock_kind) :: subtotal
read(*,*) seed
value=0.0_dp
call start_timer(tick,rate)
do k=1,repetitions
   allocate(x(n))
   do i=1,n
      x(i)=real(mod(i+k+seed,97),dp)/97.0_dp
   end do
   value=value+sum(x)
   deallocate(x)
end do
call stop_timer(tick,rate,seconds)
expected=0.0_dp
do k=1,repetitions
   subtotal=int(n/97,clock_kind)*4656
   do i=1,mod(n,97)
      subtotal=subtotal+int(mod(i+k+seed,97),clock_kind)
   end do
   expected=expected+real(subtotal,dp)/97.0_dp
end do
error=abs(value-expected)/max(1.0_dp,abs(expected))
call report(seconds,value,error)
end program

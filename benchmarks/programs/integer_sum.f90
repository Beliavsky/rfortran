! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
program benchmark
use bench_config, only: n,repetitions
use bench_support, only: dp,clock_kind,start_timer,stop_timer,report
implicit none
integer :: seed,i,k
integer(clock_kind) :: tick,rate
real(dp) :: seconds,value,expected,error
integer(clock_kind) :: total,reference
integer :: cycles,tail
read(*,*) seed
total=0
call start_timer(tick,rate)
do i=1,n
   total=total+int(mod(i+seed,97),clock_kind)
end do
value=real(total,dp)
call stop_timer(tick,rate,seconds)
cycles=n/97
reference=int(cycles,clock_kind)*4656
! A complete residue cycle sums to 0+...+96 = 4656.
do i=1,mod(n,97)
   reference=reference+int(mod(i+seed,97),clock_kind)
end do
expected=real(reference,dp)
error=abs(value-expected)/max(1.0_dp,abs(expected))
call report(seconds,value,error)
end program

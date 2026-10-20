! SPDX-License-Identifier: MIT
! Copyright (c) 2026 Beliavsky
module bench_support
implicit none
integer, parameter :: dp=selected_real_kind(15,307)
integer, parameter :: clock_kind=selected_int_kind(18)
contains
subroutine start_timer(tick,rate)
integer(clock_kind), intent(out) :: tick,rate
call system_clock(count=tick,count_rate=rate)
if (rate<=0) error stop "SYSTEM_CLOCK is unavailable"
end subroutine
subroutine stop_timer(tick,rate,seconds)
integer(clock_kind), intent(in) :: tick,rate
real(dp), intent(out) :: seconds
integer(clock_kind) :: finish
call system_clock(count=finish)
seconds=real(finish-tick,dp)/real(rate,dp)
end subroutine
subroutine report(seconds,value,error)
real(dp), intent(in) :: seconds,value,error
print '(a,es25.16)', 'BENCH_SECONDS ',seconds
print '(a,es25.16)', 'BENCH_CHECKSUM ',value
print '(a,es25.16)', 'BENCH_ERROR ',error
end subroutine
end module

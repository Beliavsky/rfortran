! SPDX-License-Identifier: MIT
! Valid Fortran. --parallel-report currently reports a serial fallback.
module parallel_kind_repro
implicit none
integer,parameter :: dp=selected_real_kind(15,307)
end module
program example
use parallel_kind_repro, only: dp
implicit none
integer :: i
real(dp) :: total
total=3.0_dp
do concurrent(i=1:10000) reduce(+:total)
   total=total+1.0_dp
end do
if(abs(total-10003.0_dp)>1e-8_dp) stop 1
print *, 'PASS'
end program

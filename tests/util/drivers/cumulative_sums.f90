! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_cumulative_sums
use util_mod, only:cumul_sum,cumul_sum0
use kind_mod, only:dp
implicit none
real(dp)::a(2,2)
a=reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp],[2,2])
print *,cumul_sum([1.0_dp,2.0_dp,3.0_dp])
print *,cumul_sum0([1.0_dp,2.0_dp,3.0_dp])
print *,cumul_sum(a),cumul_sum0(a)
end program test_cumulative_sums

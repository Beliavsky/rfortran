! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_differences
use util_mod, only:diff
use kind_mod, only:dp
implicit none
real(dp)::a(3,2)
a=reshape([1.0_dp,3.0_dp,6.0_dp,2.0_dp,6.0_dp,12.0_dp],[3,2])
print *,diff([1,3,6]),size(diff([1]))
print *,diff([1.0_dp,3.0_dp,6.0_dp]),diff(a)
end program test_differences

! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_sign_functions
use util_mod, only:sgn,sign_changes
use kind_mod, only:dp
implicit none
print *,sgn([-2.0_dp,0.0_dp,2.0_dp])
print *,sgn(0.5_dp,thresh=1.0_dp),sgn(0.5_dp,thresh=1.0_dp,smooth=.false.)
print *,sign_changes([-1.0_dp,2.0_dp,-3.0_dp,0.0_dp])
end program test_sign_functions

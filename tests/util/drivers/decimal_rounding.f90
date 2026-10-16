! Differential checks of unchanged utility procedures.
program test_decimal_rounding
use util_mod,only:floor_10,ceiling_10
use kind_mod,only:dp
implicit none
real(dp)::x(8)
x=[1.0_dp,12.5_dp,0.125_dp,-1.0_dp,-12.5_dp,-0.125_dp,999.0_dp,-999.0_dp]
print *,floor_10(x)
print *,ceiling_10(x)
print *,floor_10(1.25_dp),ceiling_10(-1.25_dp)
end program

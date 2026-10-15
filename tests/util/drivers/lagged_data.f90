! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_lagged_data
use util_mod, only:lag_good_data
use kind_mod, only:dp
implicit none
integer::lag
real(dp)::y(4)
logical::good(4)
do lag=-5,5
call lag_good_data(lag,[10.0_dp,20.0_dp,30.0_dp,40.0_dp], &
 [.true.,.false.,.true.,.true.],y,good,ydefault=-999.0_dp)
print *,lag,y,good
end do
end program test_lagged_data

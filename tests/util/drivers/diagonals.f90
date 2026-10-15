! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_diagonals
use util_mod, only: above_diag,diag_matrix,diag,offdiag,diag_3d
use kind_mod, only: dp
implicit none
real(dp)::a(3,3),b(1,1),c(2,2,2)
a=reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp,5.0_dp,6.0_dp,7.0_dp,8.0_dp,9.0_dp],[3,3])
b=1.0_dp
c=reshape([1.0_dp,2.0_dp,3.0_dp,4.0_dp,5.0_dp,6.0_dp,7.0_dp,8.0_dp],[2,2,2])
print *,above_diag(a),size(above_diag(b)),diag(a),offdiag(a)
print *,diag_matrix([2.0_dp,5.0_dp]),diag_3d(c)
end program test_diagonals

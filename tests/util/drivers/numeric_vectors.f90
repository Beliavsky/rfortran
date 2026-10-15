! Differential driver: compare unchanged util.f90 procedures with gfortran.
program test_numeric_vectors
use util_mod, only: normalize,frac_true,reverse,seq
use kind_mod, only: dp
implicit none
print *, normalize([1.0_dp,-2.0_dp,1.0_dp])
print *, normalize([0.0_dp,0.0_dp])
print *, frac_true([.true.,.false.,.true.])
print *, reverse([1.0_dp,2.0_dp,3.0_dp])
print *, size(reverse([real(dp)::]))
print *, seq(4),size(seq(0)),size(seq(-2))
end program test_numeric_vectors
